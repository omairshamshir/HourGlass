# Working on Hourglass

Hourglass is a macOS menu-bar time tracker in Rust, drawn with gpui. It records
hours per project and stops the clock by itself when the Mac sleeps, the screen
locks, or the user stops typing.

Read this file first. [`docs/architecture.md`](docs/architecture.md),
[`docs/design.md`](docs/design.md), and [`docs/macos.md`](docs/macos.md) go
deeper on structure, visual language, and platform traps respectively.

## Commands

```sh
cargo build -p hourglass            # dev build
cargo build --release               # release build
cargo test --workspace              # 94 tests, all should pass
cargo clippy --workspace --all-targets

./scripts/bundle.sh debug           # -> dist/Hourglass.app
./scripts/bundle.sh release

# Run against a scratch database instead of real recorded hours
cargo run -p hourglass-store --example seed -- /tmp/hourglass-demo.db
HOURGLASS_DB=/tmp/hourglass-demo.db cargo run -p hourglass
```

To exercise the menu bar, run the bundle rather than the binary — `LSUIElement`
only takes effect from `Info.plist`:

```sh
./scripts/bundle.sh debug && open -n dist/Hourglass.app --env HOURGLASS_DB=/tmp/hourglass-demo.db
```

`open` also detaches the app through LaunchServices, so it survives the shell
that started it. A bare `cargo run` backgrounded from a terminal dies with that
terminal's process group.

## The three crates

| Crate | Contains | May depend on |
| --- | --- | --- |
| `hourglass-core` | `model.rs` value types, `timer.rs` state machine, `report.rs` aggregation and CSV, `manual.rs` parsing and checking of hand-typed hours | `chrono` only |
| `hourglass-store` | SQLite via `rusqlite`, schema and migrations | `hourglass-core` |
| `hourglass` | gpui UI, macOS integration, `AppState` | both |

**`hourglass-core` must never gain a dependency on SQLite, AppKit, or gpui.**
Its whole value is that every rule about when the clock runs can be tested at
fixed timestamps without a database or a running app. If you find yourself
wanting I/O in there, the logic belongs in `AppState` instead.

## How a change flows through the app

```
click / keypress / menu item / system signal
        │
        ▼
   AppState                     (crates/hourglass/src/state.rs)
        │  asks
        ▼
   Timer::start/stop/handle     (pure decision, no I/O)
        │  returns Vec<Effect>
        ▼
   AppState::apply              (the only writer to the store)
        │
        ▼
   Store  ──▶ reload() ──▶ views redraw
```

`Timer` decides, `AppState` carries it out. `Effect` is the whole vocabulary
between them: `CloseEntry`, `OpenEntry`, `AskToResume`, `CancelPrompt`.

A 250 ms tick loop in `main.rs` drains macOS signals, polls the menu bar and the
idle timer, and hands all of it to `AppState::tick`.

## Invariants worth protecting

These are the things most likely to be broken by a well-meaning change.

1. **At most one time entry is open.** Enforced by a partial unique index
   (`idx_entries_single_open`), not merely by the caller, so it survives a
   crash or a double click. `Store::open_entry` returns `AlreadyRunning`
   rather than inserting a second.
2. **`Timer` never reads the clock.** Every timestamp is a parameter. This is
   what makes sleep and idle behaviour testable, and it is why `pending_resume`
   and `tick` take a `now` rather than calling `Utc::now()` internally.
3. **Time away is never billed.** Resuming opens a *new* entry at the moment of
   the answer, never backdated to the pause.
4. **Idle is backdated, sleep is not.** Idle closes the entry at the last input
   (`WentIdle { since }`), because the app only notices minutes later. Sleep and
   lock close at the moment they happened.
5. **The first pause reason wins.** If the screen locks and then the Mac sleeps,
   the entry reads `lock` at the lock time. A later pause never overwrites an
   earlier one.
6. **An entry can never end before it starts.** Clamped in both `Timer::pause`
   and `Store::close_open_entry`; a backdated close becomes a zero-length entry
   rather than negative time.
7. **Ranges are local-time questions against UTC storage.** Day, week, and
   month boundaries are computed in the local zone and converted.
   `seconds_within` clips at both edges, so a session across midnight is split
   between the days instead of counted twice.
8. **One frame, one clock reading.** `Root::render` takes `Utc::now()` once and
   passes it down, so the header, band, and totals can never disagree by a tick.
9. **AppKit callbacks never touch gpui.** They push a timestamped `Signal` onto
   a global queue that the tick loop drains. See `docs/macos.md` for why.
10. **Recorded time is claimed exactly once.** Hand-written entries are refused
    when they overlap anything already stored, including the session running
    right now (`manual::first_conflict` treats an open entry as reaching to
    `now`). Overlap is the one way this app could quietly inflate a day's
    total, so the check belongs on the write path and not in the UI. Spans are
    half-open: `10:00–11:00` and `11:00–12:00` do not collide.
11. **Hand-written entries are inserted closed.** `Store::insert_entry` writes
    a row that already has an `ended_at`, so it can never contend with
    `idx_entries_single_open`. Do not "simplify" it into `open_entry` followed
    by `close_open_entry` — that would fail whenever a timer is running, which
    is exactly when people notice a missing entry.
12. **The palette is global state, deliberately.** `theme::set_appearance`
    writes an atomic that every accessor in `theme.rs` reads, so views name
    colours by role (`theme::ink()`) and never receive a theme. Adding a raw
    `rgb(0x…)` to a view is the mistake this design exists to prevent: it will
    look correct in whichever appearance you tested and wrong in the other.
    `theme::wash` flips between darkening and lightening for the same reason.

## Conventions

**Tests are prose.** Names are full sentences describing the behaviour, not the
method under test:

```rust
#[test]
fn a_pause_never_ends_before_the_entry_started() { … }

#[test]
fn switching_closes_the_old_entry_at_the_same_instant_it_opens_the_new_one() { … }
```

Match this. A test called `test_pause_2` will look wrong here.

**Comments explain the decision, not the code.** Existing comments say *why* a
rule exists ("silently restarting the clock after lunch invents hours"), never
what the next line does. Do not add comments that narrate the change you just
made or justify it to a reviewer.

**No dead code.** The build is warning-clean; a `pub fn` nobody calls will trip
`dead_code`. Delete rather than `#[allow]`.

**Readability over cleverness**, per the house style. This is a small app whose
correctness argument has to stay obvious.

## Before you claim it works

- `cargo test --workspace` passes and the build has no new warnings.
- If you touched the UI, actually look at it. Build the bundle, run it against
  the seeded database, and capture the window — `docs/macos.md` has the
  screenshot recipe, which is not obvious because `screencapture` fails in two
  different ways on GPU-backed windows.
- If you touched sleep, lock, or idle handling, add a `Timer` test at fixed
  timestamps. Do not verify it by waiting for your Mac to fall asleep.

## Things that will surprise you

- **gpui 0.2.2 is pinned and its API moves.** There is no letter-spacing, no
  offscreen frame capture, and few built-in components. Check the vendored
  source in `~/.cargo/registry` before assuming an API exists.
- **The window is GPU-backed**, so `screencapture -l <window>` fails whenever
  another window covers it, and `screencapture -R <rect>` fails outright on some
  machines. Grab the full screen and crop.
- **A sleeping display captures as pure black**, which looks exactly like a
  broken renderer. Check mean luminance before debugging the UI.
- **`become_menu_bar_app()` runs after gpui starts**, not before — gpui sets the
  regular activation policy during startup and would override an earlier call.
- **Burnt sienna means "running" and nothing else.** `#C2410C` on paper,
  `#E8763A` under the lamp, because the daylight tone disappears on a dark
  ground. There is a unit test asserting every project swatch in *both*
  palettes stays clear of it. Do not spend the accent on hover states or
  emphasis.
- **`Keystroke::parse` fills in `key` but not `key_char`**, so a test that
  parses `"9"` and expects a character to be typed will silently get nothing.
  Set `key_char` by hand — see the `typed` helper in `ui/entry_form.rs`.
- **gpui only paints when something asks it to.** `with_animation` requests the
  next frame until its own duration elapses, then stops. That makes the
  animation's *key* the thing that controls cost: the clock's settle is keyed
  on the elapsed second so it lands and goes quiet. Key an animation on
  anything that changes per frame and the window paints at the display's
  refresh rate for as long as it is open.
- **Tests must not assume a human is at the keyboard.** `IdleWatcher` reads the
  real HID idle time, so any test asserting "no idle event" fails whenever the
  Mac is left locked. Derive the threshold from the current reading instead of
  hard-coding one.
