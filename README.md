# Hourglass

A time tracker for macOS that refuses to bill you for sleep.

Hourglass lives in the menu bar and counts the hours you spend on each project.
When the Mac sleeps, the screen locks, or you stop typing, it stops the clock at
the moment you actually stopped working — not at the moment it noticed — and
asks whether to pick up where you left off. Short interruptions resume on their
own; long ones ask, because silently restarting the clock after lunch invents
hours you never worked.

It is written in Rust and drawn with [gpui](https://www.gpui.rs), the GPU
renderer behind the Zed editor.

## Status

Early but working. The timer, storage, reporting, menu bar item, and window are
all in place and covered by tests. Not yet signed or notarised, so it runs on
your own machine only.

## Building

Requires a recent stable Rust toolchain and macOS 13 or later.

```sh
cargo build --release
./scripts/bundle.sh release
open dist/Hourglass.app
```

`scripts/bundle.sh` assembles `dist/Hourglass.app` around the compiled binary.
Running the bare binary works for quick checks, but only the bundle carries the
`LSUIElement` flag that keeps Hourglass out of the Dock, so the menu-bar
behaviour is only true to life from the app.

For development:

```sh
cargo run -p hourglass
cargo test --workspace
```

## Using it

The menu bar shows the running project's hours and minutes, and its menu can
start, stop, resume, open the window, or quit. Everything else happens in the
window.

The window is a project rail on the left and the day on the right: a large
clock for the current session, a band showing when today's work actually
happened, and a ledger of totals and entries underneath. The band is the point
of the whole app — a bar chart tells you how much you worked, but the band tells
you *when*, and where the gaps are.

Click a project to start it. Clicking another switches, closing the first entry
at the same instant the second opens, so no second is ever counted twice.
Right-click a project to rename it; hover it to archive or delete it. Archiving
keeps the history and takes the project out of the start menu.

### Keyboard

| Key | Does |
| --- | --- |
| `Space` | Start or stop the clock |
| `1`–`8` | Start the project at that position in the rail |
| `N` | Add a project |
| `A` | Write down hours you forgot to time |
| `T` / `W` / `M` | Show today, this week, or this month |
| `Enter` | Resume, when asked |
| `Esc` | Leave it stopped, when asked |
| `⌘E` | Export the current range to CSV |

### Hours you forgot to time

Forgetting to press start is the most common way a time tracker loses an
afternoon, so hours can be written down after the fact. Press `A`, or use
**+ Add hours** above the entry list, and fill in four fields: project, date,
from, and to. Tab moves between them, Return saves, Escape closes.

Both the date and the times are read generously. `9:30`, `0930`, `9.30am` and
`5pm` all name the hour you meant; the date takes `today`, `yesterday`,
`2026-09-07`, or `9/7` for the current year.

What is not generous is what gets accepted. An entry that ends before it
starts, runs into the future, or covers time some other entry already claims is
refused with the reason, and the form stays open so you can fix the one field
that was wrong. Overlapping entries are the only way this app can quietly lie
about a day's total, so it does not allow them — including over the session
currently being timed.

Hand-written entries are marked `manual` in the ledger and in exported CSV, so
a timed hour and a remembered one are never confused for each other.

### Appearance

Light, dark, or following the system, chosen from the switch in the top-right
of the window. **Auto** is the default and tracks macOS as it changes, so a Mac
that switches at sunset takes Hourglass with it. The choice is remembered
between launches.

### Reports and export

Reports cover today, the current week (Monday to Sunday), or the calendar
month. A session that runs across midnight is split between the two days rather
than counted twice in either.

`⌘E` writes the current range to `~/Downloads/hourglass-<range>-<date>.csv` with
one row per entry:

```csv
project,date,start,end,duration,hours
Atlas Redesign,2026-09-07,09:12,10:44,1h 32m,1.53
```

The `hours` column is decimal, which is the unit invoices want.

## When the clock stops on its own

| Situation | What happens |
| --- | --- |
| Mac sleeps | Entry closes at the moment of sleep |
| Screen locks or the saver starts | Entry closes at the moment of locking |
| No input for 10 minutes | Entry closes at the *last input*, not when noticed |
| Back within 2 minutes | Resumes silently |
| Back after longer | Asks first, and the time away stays unbilled |

Both thresholds are stored in the database and default to 10 minutes and 2
minutes.

If the app is killed while a timer is running, the entry stays open in the
database and the clock picks it up again on the next launch.

## Where the data lives

A single SQLite file:

```
~/Library/Application Support/Hourglass/hourglass.db
```

Set `HOURGLASS_DB` to point somewhere else, which is how the demo database below
avoids touching your real hours. The schema is small enough to query by hand,
and timestamps are Unix seconds.

To look at the app with plausible data in it:

```sh
cargo run -p hourglass-store --example seed -- /tmp/hourglass-demo.db
HOURGLASS_DB=/tmp/hourglass-demo.db cargo run -p hourglass
```

## Layout

```
crates/hourglass-core    the timer, the rules, the reports — no I/O at all
crates/hourglass-store   SQLite persistence and migrations
crates/hourglass         the app: gpui window, menu bar, macOS signals
scripts/bundle.sh        assembles dist/Hourglass.app
docs/                    architecture, design language, macOS notes
```

`hourglass-core` touches no database, no AppKit, and no GPU, so every rule about
when the clock runs is unit-tested at fixed timestamps rather than by waiting
around.

## Contributing

Start with [`AGENTS.md`](AGENTS.md) — it is written for coding agents but it is
the fastest orientation for a person too. [`docs/architecture.md`](docs/architecture.md)
explains how the pieces fit, [`docs/design.md`](docs/design.md) is the visual
language, and [`docs/macos.md`](docs/macos.md) collects the platform traps.

## License

MIT.
