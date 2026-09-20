# Architecture

How Hourglass is put together, and why it is arranged this way.

The organising idea: **the rules about when a clock runs are hard, and
everything else is plumbing.** So the rules live in a crate with no I/O, where
they can be tested at any timestamp, and the plumbing is kept thin enough to
read in one sitting.

## The layers

```
┌─────────────────────────────────────────────────────────────┐
│ crates/hourglass            the app                         │
│                                                             │
│   main.rs      startup, the 250 ms tick loop                │
│   state.rs     AppState: owns the timer and the store       │
│   ui/          gpui views (rail, header, band, report)      │
│   mac/         signals, idle polling, the menu bar item     │
│   theme.rs     palette and typefaces                        │
└───────────────┬─────────────────────────┬───────────────────┘
                │                         │
                ▼                         ▼
┌───────────────────────────┐  ┌──────────────────────────────┐
│ crates/hourglass-store    │  │ crates/hourglass-core        │
│                           │  │                              │
│   lib.rs    Store         │  │   model.rs   value types     │
│   schema.rs migrations    │──▶│   timer.rs   state machine   │
│                           │  │   report.rs  aggregation     │
│   depends on: rusqlite    │  │   depends on: chrono         │
└───────────────────────────┘  └──────────────────────────────┘
```

Dependencies point inward. `hourglass-core` knows about nothing else, which is
the property the whole design rests on: **no SQLite, no AppKit, no GPU.**

## `hourglass-core`

### `model.rs`

Value types shared by everything: `ProjectId`, `EntryId`, `Project`,
`TimeEntry`, `Settings`, and `StopReason`.

`StopReason` is the interesting one. It distinguishes what the user did
(`Manual`, `Switch`) from what the machine decided (`Sleep`, `Idle`, `Lock`),
which `is_automatic()` exposes. The entry list uses it to annotate a short
session, because an unexplained fifteen-minute entry looks like lost work.

It round-trips through text for storage, and `from_str_lossy` reads an unknown
value back as `Manual` so a database written by a future version still opens.

The three formatters matter more than they look: `format_clock` gives `H:MM:SS`
for the live timer, `format_duration` gives `4h 05m` for lists, and
`format_decimal_hours` gives `4.08` because that is the unit invoices use.

### `timer.rs`

The state machine, and the only place that decides when the clock runs.

It holds three things: an optional running `Session`, an optional `Paused` the
app is remembering, and whether a resume prompt is currently up.

```
                start(project)
   ┌─────────┐ ─────────────────▶ ┌──────────┐
   │ stopped │                    │ running  │
   └─────────┘ ◀───────────────── └──────────┘
        ▲          stop()              │
        │                              │ WillSleep / ScreenLocked / WentIdle
        │                              ▼
        │  discard_pause()        ┌──────────┐
        └──────────────────────── │  paused  │
                                  └──────────┘
                                       │ DidWake / ScreenUnlocked / BecameActive
                                       ▼
                          away ≤ threshold → resume silently
                          away >  threshold → AskToResume
```

It never performs I/O and never reads the clock. Callers pass timestamps in,
which is not fussiness — the honest moment is usually in the past. A wake
notification arrives long after sleep began, and idle is only noticed minutes
after the user stopped typing. A machine that called `Utc::now()` internally
could not record either one truthfully, and could not be tested without waiting.

Methods return `Vec<Effect>` describing work for the caller:

| Effect | Meaning |
| --- | --- |
| `OpenEntry` | Insert a new open entry |
| `CloseEntry` | Close the open entry at this instant, with this reason |
| `AskToResume` | Put a prompt up; the pause was long enough that silence would lie |
| `CancelPrompt` | Take a prompt down that no longer applies |

Note what is absent: entry ids. The machine knows only that at most one entry is
open, and the store closes "the open entry" when told to. That keeps the two
sides from having to agree on identity.

`Timer::resumed_from` rebuilds the machine from a session already open in the
database, which is how the app survives being killed mid-session.

### `report.rs`

Turns entries into totals.

Ranges are local-time questions — "what did I do today?" — answered against UTC
storage, so `local_day`, `local_week` (Monday start), and `local_month` compute
boundaries in the local zone and convert. `local_midnight` handles the daylight
saving case where 00:00 does not exist on a given date by stepping forward to
the first real hour rather than failing.

`seconds_within` clips an entry at both range edges. This is what stops a
session that runs across midnight from being counted twice; it is split between
the two days instead.

`build_report` sums per project, sorts biggest first, and skips entries whose
project has been deleted rather than showing a blank row. `report_to_csv` emits
oldest-first (which reads better in a spreadsheet than the on-screen order) and
quotes a field only when it contains a comma, quote, or newline.

### `manual.rs`

Parsing and checking for hours the user types in rather than times.

`parse_time` and `parse_date` are generous about form: `9:30`, `0930`, `9.30am`
and `5pm` all resolve to the same minute, and a date may be `today`,
`yesterday`, `2026-09-07`, or `9/7`. Being strict here would only make the
feature annoying — the strictness that matters is about meaning, not notation.

`resolve_span` reads the three fields in the *local* zone, because that is the
zone the user was working in when they forgot to press start, and returns UTC.
It refuses an entry that ends at or before it starts, one that runs into the
future, and a clock time that daylight saving means did not exist on that date.

`first_conflict` is the important one. It finds any recorded entry sharing time
with the proposed span, treating a still-open entry as reaching up to `now`.
Spans are half-open, so `10:00–11:00` and `11:00–12:00` sit next to each other
without colliding. Overlap is the only way this app could silently inflate a
day's total, which is why the check lives in core with the rest of the rules
rather than in the form.

The module has no opinion about which projects exist; `AppState` resolves the
typed name and owns the "unknown project" case.

## `hourglass-store`

SQLite through `rusqlite` with the `bundled` feature, so there is no dependency
on the system library.

The store is deliberately dumb: it records what the timer decided. It enforces
exactly one rule of its own, because that rule has to survive a crash rather
than merely a well-behaved caller:

```sql
CREATE UNIQUE INDEX idx_entries_single_open
    ON time_entries((ended_at IS NULL)) WHERE ended_at IS NULL;
```

Every open row indexes the same value, so a second insert is rejected and
`open_entry` returns `AlreadyRunning`. A double click, or a race between a wake
event and a menu command, cannot double-count.

Two more details in the schema. A `CHECK (ended_at IS NULL OR ended_at >=
started_at)` makes negative entries unrepresentable, and `close_open_entry`
clamps an early end to the start so a backdated idle close becomes a zero-length
entry rather than an error. Deleting a project cascades to its entries.

Timestamps are Unix seconds as `INTEGER`: sortable, comparable, and free of
parsing on every read. The connection runs in WAL mode so the UI's reads never
block on the writer.

`entries_in_range` deliberately includes an entry that *starts* before the range
but runs into it, so a session across midnight still appears on the later day:

```sql
WHERE started_at < ?2 AND (ended_at IS NULL OR ended_at > ?1)
```

`Store::default_path()` resolves to
`~/Library/Application Support/Hourglass/hourglass.db` unless `HOURGLASS_DB`
overrides it. The override exists so development can run against a scratch
database without touching real recorded hours.

## `hourglass`

### `AppState` (`state.rs`)

The one place that owns the timer, the database, and the platform queue. Views
read it and ask it to do things; nothing else writes to the store.

Its core is `apply`, which carries out whatever the timer decided and then
reloads. Reports are built on demand from cached entries rather than cached
themselves, so a running total is never a second stale.

`tick` folds in everything that happened since the last one:

```rust
pub fn tick(
    &mut self,
    now: DateTime<Utc>,
    events: &[SystemEvent],
    tray_commands: &[TrayCommand],
) -> TickOutcome
```

Events and commands arrive as parameters rather than being read from globals
inside. That is what lets the whole sleep-and-resume story be tested without a
Mac in the loop — see the tests at the bottom of `state.rs`, which drive sleep,
wake, and idle entirely through this signature.

`TickOutcome` reports what the caller must do: `redraw`, `surface_window`
(the user asked for it, or a prompt appeared that needs answering), and `quit`.

While the clock runs, `tick` redraws at most once a second by comparing against
`last_drawn_second`, so the loop can run four times a second without repainting
four times a second.

### The tick loop (`main.rs`)

250 ms. Fast enough that a menu click feels immediate, slow enough to be
invisible on a power graph.

Each pass drains the macOS signal queue, polls the menu bar for clicks, polls
the idle timer, hands everything to `AppState::tick`, then syncs the menu bar
title and surfaces the window if asked.

Idle is only polled while the clock is running or while already idle. There is
no reason to ask how long the user has been away from the keyboard when nothing
is being recorded.

### `mac/`

`signals.rs` registers observers on `NSWorkspace`'s notification centre for
sleep and wake, and on `NSDistributedNotificationCenter` for screen lock and
unlock. Display sleep is treated as a lock, since nobody is looking at the
screen either way. The same distributed centre carries
`AppleInterfaceThemeChangedNotification`, which is how the app hears about a
light/dark switch without polling for it.

Not every signal is a timer event: `Signal::into_event` returns `Option`, and
an appearance change resolves to `None` so it never reaches the clock.

`appearance.rs` reads `AppleInterfaceStyle` from the standard defaults. The key
is absent in light mode rather than holding "Light", so absence is the light
case.

Callbacks do not touch gpui. They push a timestamped `Signal` onto a global
queue drained by the tick loop — one direction, no locks held across a frame, no
reentrancy. `docs/macos.md` explains why this matters.

`idle.rs` polls `CGEventSourceSecondsSinceLastEventType` and turns the result
into `WentIdle { since }` / `BecameActive { at }`, backdating the former to the
last real input.

`tray.rs` owns the status item. The title shows hours and minutes, never
seconds: a per-second repaint in the menu bar is motion the user sees thousands
of times a day, and it shifts the neighbouring icons on every tick. The menu is
rebuilt only when a cheap fingerprint of its text changes, and that fingerprint
deliberately ignores elapsed seconds.

The icon is drawn in code as an alpha-only template image, so macOS inverts it
for light and dark menu bars automatically.

### `ui/`

gpui views. `root.rs` is the window: layout, the one key handler, and the modes
the window can be in (browsing, adding a project, renaming, confirming a
delete, adding hours). `rail.rs` is the project list, `band.rs` the day strip,
`report.rs` the totals and entry ledger, `entry_form.rs` the add-hours band,
`kit.rs` the shared pieces, `text_field.rs` a minimal editable field.

`Root::render` reads the clock exactly once and passes that instant down, so
every figure on screen belongs to the same moment.

Views never write to the store directly. They call `Root::edit`, which mutates
`AppState` and notifies observers, so a change made in the window and a change
made from the menu bar take the same path.

`entry_form.rs` holds four `TextField`s and a focus marker, and nothing else.
It does not know what a valid entry is; it collects strings and hands them to
`AppState::add_manual_entry`, which owns every rule. That is why a rejected
entry can leave the form exactly as it was — the form was never the thing that
decided.

Colour is the one piece of shared state that is not threaded through views.
`theme.rs` holds the current appearance in an atomic and exposes colours by
role, so `set_appearance` is a single assignment rather than a prop passed
through every element. See `docs/design.md` for what each role means.

## Testing

140 tests, no test needs a Mac to fall asleep — or a human at the keyboard.

| Where | Covers |
| --- | --- |
| `hourglass-core/src/timer.rs` | Every sleep, lock, idle, switch, and prompt path at fixed timestamps |
| `hourglass-core/src/report.rs` | Range boundaries, midnight splitting, CSV shape |
| `hourglass-core/src/model.rs` | Formatters, `StopReason` and `ThemeChoice` round-tripping |
| `hourglass-core/src/manual.rs` | Time and date notation, refusals, overlap detection |
| `hourglass-store/src/lib.rs` | The single-open-entry rule, range queries, cascades, settings |
| `hourglass/src/state.rs` | Sleep and resume end to end, hand-written hours, theme persistence |
| `hourglass/src/ui/band.rs` | The band's time window and hour marks |
| `hourglass/src/ui/entry_form.rs` | Field focus order and where typing lands |
| `hourglass/src/theme.rs` | Both palettes: contrast, swatch separation, wash direction |

The pattern to follow: drive behaviour through the public surface at explicit
timestamps, and name the test as a sentence about the behaviour.

## Deliberate omissions

Worth knowing so they are not mistaken for oversights.

- **No async runtime.** A 250 ms poll is simpler than wiring AppKit callbacks
  into a reactor, and the workload is trivial.
- **No ORM or migration framework.** One versioned SQL string per migration,
  applied by `user_version`.
- **No settings UI yet.** `idle_minutes` and `auto_resume_under_secs` live in
  the `settings` table and default to 10 minutes and 2 minutes.
- **No editing of entry times in the UI**, though `Store::update_entry_times`
  exists for it.
