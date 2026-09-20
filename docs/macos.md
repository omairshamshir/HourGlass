# macOS notes

Platform integration, and the traps that cost real time. Most of this is not
discoverable from documentation — it was learned by things failing oddly.

## System signals

Sleep and wake come from `NSWorkspace`'s own notification centre. Screen lock
and unlock are **only** published on `NSDistributedNotificationCenter`, so both
centres are observed:

| Notification | Centre | Becomes |
| --- | --- | --- |
| `NSWorkspaceWillSleepNotification` | workspace | `WillSleep` |
| `NSWorkspaceDidWakeNotification` | workspace | `DidWake` |
| `NSWorkspaceScreensDidSleepNotification` | workspace | `ScreenLocked` |
| `NSWorkspaceScreensDidWakeNotification` | workspace | `ScreenUnlocked` |
| `com.apple.screenIsLocked` | distributed | `ScreenLocked` |
| `com.apple.screenIsUnlocked` | distributed | `ScreenUnlocked` |

The two `com.apple.screenIs*` names are private but long-stable; there is no
public API for lock state. Display sleep is mapped to `ScreenLocked` because
nobody is looking at the screen either way.

`SystemSignals` holds the observer tokens. **Dropping it silently unsubscribes**
— the app keeps it alive for the whole run, which is why `main.rs` moves it into
the tick loop as `_signals` rather than letting it fall out of scope.

The notification name constants are `extern` statics, so reading them requires
`unsafe` even though nothing else about the call does. Scope the `unsafe` to the
reads themselves; wrapping the whole block earns an `unnecessary unsafe` warning.

### Why callbacks go through a queue

AppKit callbacks never touch gpui. They call `push_signal`, which appends a
timestamped `Signal` to a global queue that the tick loop drains.

This is not indirection for its own sake. A notification can fire while gpui is
mid-frame, and reaching into its context from there is reentrancy the framework
does not expect. The queue makes the direction one-way and means no lock is ever
held across a frame.

It also keeps `AppState` testable: `tick` receives events as a parameter, so the
whole sleep-and-resume story is driven from unit tests with no Mac involved.

## Idle detection

`CGEventSourceSecondsSinceLastEventType` with `kCGAnyInputEventType`, polled by
`IdleWatcher`.

The important property: **this needs no Accessibility or Input Monitoring
permission.** A time tracker that demands input monitoring on first launch is a
time tracker people delete. Do not replace this with an event tap.

`WentIdle` is backdated to `now - quiet`, the moment input actually stopped,
which is a full idle threshold earlier than when it was noticed. That is the
whole point — otherwise the ten minutes you spent away get billed.

Idle is only polled while the clock runs or while already idle.

## Activation policy

Hourglass is an accessory app: no Dock tile, no app switcher entry.

Two separate mechanisms, and you need both:

- `LSUIElement` in `Info.plist` — the real one, but it only exists in a bundle.
- `become_menu_bar_app()` calling `setActivationPolicy(.Accessory)` — covers
  running the bare binary.

`become_menu_bar_app()` **must run after gpui has started**, not before. gpui
sets the regular activation policy during startup and would override an earlier
call.

An accessory app has to ask to come forward, hence `activate_app()` before
surfacing the window.

## Bundling

`scripts/bundle.sh [debug|release]` assembles `dist/Hourglass.app` from an
already-compiled binary. It writes `Info.plist`, copies `assets/icon.icns` if
one exists, and applies an ad-hoc signature so macOS does not re-prompt for
permissions on every rebuild.

`cargo-bundle` is not required and is not installed. The `[package.metadata.bundle]`
block in `crates/hourglass/Cargo.toml` is there for the day it is.

Run the bundle, not the binary, whenever the menu bar or Dock behaviour matters.

### Launch it detached

```sh
open -n dist/Hourglass.app --env HOURGLASS_DB=/tmp/hourglass-demo.db
```

`open` goes through LaunchServices, so the app is adopted by launchd (PPID 1)
and survives the shell that started it. This matters more than it sounds:

A binary backgrounded from a terminal — even with `nohup` — stays in that
terminal's process group and dies when the group is killed. During development
that means the app you are trying to inspect disappears the moment an unrelated
command is interrupted. Use `open`.

Confirm with `ps -o ppid= -p <pid>`; it should print `1`.

### Two target directories

If some of your `cargo` invocations are sandboxed and some are not, they write
to **different target directories** — the sandboxed ones land in a
`cursor-sandbox-cache/…/cargo-target` path.

This produces a genuinely confusing failure: you rebuild, relaunch, and see your
old UI, because the bundle script picked up a stale binary from the other
directory. `bundle.sh` respects `CARGO_TARGET_DIR`, so keep it consistent with
however you are building, and check timestamps when a change seems not to have
taken:

```sh
ls -la target/debug/hourglass
```

## Screenshots

Verifying the UI is harder than it should be. Three separate things go wrong.

**1. `screencapture -l <window-id>` fails for this window.** The gpui window is
GPU-backed, and macOS keeps no capturable backing store for it while another
window covers it. It works when Hourglass is frontmost and fails with `could not
create image from window` otherwise.

**2. `screencapture -R <x,y,w,h>` can fail outright.** On this machine it
returns `could not create image from rect` even for a trivial `0,0,200,200`,
while full-screen capture works fine.

**3. A sleeping display captures as pure black.** Not an error — a valid PNG of
nothing. This looks exactly like a broken renderer. Before debugging the UI,
check the mean luminance of the grab. `caffeinate -u -t 2` wakes the display,
but if the Mac is locked you will capture the lock screen instead, and there is
no way around that but unlocking.

### What works

Grab the whole screen and crop to the window's bounds:

```sh
# window bounds in points, via CGWindowListCopyWindowInfo
# then: screencapture -x -o full.png
# then: crop full.png to (x, y, w, h) × display scale
```

Read the window rectangle from `CGWindowListCopyWindowInfo` with
`kCGWindowListOptionOnScreenOnly` and `layer == 0`, take the full-screen grab,
and crop it with CoreGraphics. The scale factor is the grab's pixel width
divided by `CGDisplayBounds(CGMainDisplayID()).width`.

`CGWindowListCopyWindowInfo` needs no permission at all, which also makes it a
good way to check the window exists and is on screen without capturing anything.

### Permission

Screen Recording is granted to the **responsible application** — the terminal or
editor that owns the shell, not `screencapture` itself. Check the ancestry:

```sh
ps -o pid=,ppid=,comm= -p $$
```

A newly granted permission is picked up without relaunching in some cases and
not others; if capture still fails immediately after granting, the owning app
needs restarting. Launch the app with `open` first so it survives that restart.

## gpui 0.2.2

Pinned, and the API moves between versions. When in doubt, read the vendored
source rather than guessing:

```sh
ls ~/.cargo/registry/src/*/gpui-0.2.2/src/
```

Known gaps, all of which have shaped the design:

- **No letter-spacing.** `theme::letterspaced()` interleaves thin spaces.
- **No offscreen frame capture.** There is no way to render a window to a PNG
  from inside the app, which is why screenshots go through the screen.
- **Few built-in components.** Buttons, tabs, and the text field are all
  hand-rolled in `ui/kit.rs` and `ui/text_field.rs`.
- **Arbitrary OpenType features are available**, through
  `FontFeatures(pub Arc<Vec<(String, u32)>>)`, even though only
  `disable_ligatures()` is offered as a helper. This is how `tnum` and `lnum`
  are enabled.
- **`traffic_light_position` is set in `WindowOptions`** and is the top-left of
  the close button. Keep it centred in the window's top strip.

## Menu bar

`tray-icon` provides the status item. Two things are deliberate:

The title shows hours and minutes, never seconds. A per-second repaint in the
menu bar is motion the user sees thousands of times a day, and it nudges the
neighbouring icons on every tick.

The menu is rebuilt only when a fingerprint of its text changes, and that
fingerprint ignores elapsed seconds — otherwise the menu would be reconstructed
four times a second forever.

The icon is generated in code as an alpha-only RGBA buffer and marked as a
template image, so macOS inverts it for light and dark menu bars automatically.
Template images carry their shape in the alpha channel; the colour channels are
ignored.
