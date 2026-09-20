//! Hourglass: a menu-bar time tracker that refuses to bill you for sleep.

mod mac;
mod state;
mod theme;
mod ui;

use crate::mac::idle::IdleWatcher;
use crate::mac::tray::{Tray, TrayModel};
use crate::state::AppState;
use crate::theme::Appearance;
use crate::ui::Root;
use crate::ui::root::menu_bar_title;
use chrono::Utc;
use gpui::{
    App, AppContext, Application, Bounds, Entity, Timer, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions, point, px, size,
};
use hourglass_core::model::ThemeChoice;
use hourglass_core::timer::idle_threshold;
use hourglass_store::Store;
use std::time::Duration;

/// How often the app looks for sleep signals, menu clicks, and idleness.
///
/// Four times a second is fast enough that a menu click feels immediate and
/// slow enough to be invisible on a power graph. Nothing is redrawn unless
/// something actually changed.
const TICK: Duration = Duration::from_millis(250);

fn main() {
    let store = match Store::open(&Store::default_path()) {
        Ok(store) => store,
        Err(err) => {
            eprintln!("Hourglass could not open its database: {err}");
            std::process::exit(1);
        }
    };

    Application::new().run(move |cx: &mut App| {
        let state = cx.new(|_| AppState::load(store));

        // gpui starts as a regular app; Hourglass belongs in the menu bar.
        mac::become_menu_bar_app();
        let signals = mac::signals::SystemSignals::install();
        let tray = Tray::new();

        // Settle the palette before the first frame, so the window never
        // flashes the wrong one on launch.
        let mut appearance = AppearanceTracker::new();
        appearance.refresh(state.read(cx).settings().theme, true);

        let window = open_window(state.clone(), cx);
        run_tick_loop(state, tray, signals, window, appearance, cx);
    });
}

/// Open the window, sized for the day band to breathe.
fn open_window(state: Entity<AppState>, cx: &mut App) -> Option<WindowHandle<Root>> {
    let bounds = Bounds::centered(None, size(px(940.), px(660.)), cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Hourglass".into()),
            appears_transparent: true,
            // Centre the lights in the top strip. The rail's wordmark sits at
            // the far end of that strip, so the two never meet.
            traffic_light_position: Some(point(px(20.), px(19.))),
        }),
        window_min_size: Some(size(px(720.), px(520.))),
        ..Default::default()
    };

    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| Root::new(state, window, cx))
        })
        .ok();

    cx.activate(true);
    handle
}

/// The heartbeat: drain the platform, update the menu bar, redraw if needed.
fn run_tick_loop(
    state: Entity<AppState>,
    tray: Option<Tray>,
    signals: mac::signals::SystemSignals,
    window: Option<WindowHandle<Root>>,
    appearance: AppearanceTracker,
    cx: &mut App,
) {
    let mut tray = tray;
    let mut window = window;
    let mut idle = IdleWatcher::new();
    let mut appearance = appearance;
    // The observers stop working the moment they are dropped, so they live as
    // long as the loop does.
    let _signals = signals;

    cx.spawn(async move |cx| {
        loop {
            Timer::after(TICK).await;
            let now = Utc::now();

            let commands = tray.as_ref().map(Tray::poll).unwrap_or_default();
            let signals = mac::drain_signals();
            let system_switched = signals
                .iter()
                .any(|(signal, _)| *signal == mac::Signal::AppearanceChanged);
            let mut events: Vec<_> = signals
                .into_iter()
                .filter_map(|(signal, at)| signal.into_event(at))
                .collect();

            let outcome = cx.update(|cx| {
                let threshold = idle_threshold(state.read(cx).settings());
                // Idleness only matters while the clock is running.
                if state.read(cx).is_running() || idle.is_idle() {
                    events.extend(idle.poll(threshold, now));
                }

                let repaint =
                    appearance.refresh(state.read(cx).settings().theme, system_switched);

                state.update(cx, |state, cx| {
                    let outcome = state.tick(now, &events, &commands);
                    if outcome.redraw || repaint {
                        cx.notify();
                    }
                    outcome
                })
            });

            let Ok(outcome) = outcome else {
                // The app is shutting down.
                return;
            };

            if outcome.quit {
                let _ = cx.update(|cx| cx.quit());
                return;
            }

            if let Some(tray) = tray.as_mut() {
                let _ = cx.update(|cx| sync_tray(tray, &state, now, cx));
            }

            if outcome.surface_window {
                let _ = cx.update(|cx| {
                    window = surface(window.take(), &state, cx);
                });
            }
        }
    })
    .detach();
}

/// Keep the menu bar in step with the clock.
fn sync_tray(tray: &mut Tray, state: &Entity<AppState>, now: chrono::DateTime<Utc>, cx: &App) {
    let state = state.read(cx);
    let running = state.running_project();
    let elapsed = state.elapsed_seconds(now);

    let shortcuts: Vec<_> = state
        .active_projects()
        .take(5)
        .map(|project| (project.id, project.name.as_str()))
        .collect();

    tray.sync_menu(&TrayModel {
        running: running.map(|project| (project.name.as_str(), elapsed)),
        paused: state.pending_resume(now).map(|(project, _)| project.name.as_str()),
        shortcuts,
    });

    tray.set_title(running.map(|_| menu_bar_title(elapsed)));
}

/// Bring the window to the front, opening a fresh one if it was closed.
fn surface(
    window: Option<WindowHandle<Root>>,
    state: &Entity<AppState>,
    cx: &mut App,
) -> Option<WindowHandle<Root>> {
    mac::activate_app();

    if let Some(handle) = window {
        let raised = handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok();
        if raised {
            return Some(handle);
        }
    }

    open_window(state.clone(), cx)
}

impl mac::Signal {
    /// Translate a platform signal into something the timer understands.
    ///
    /// Not every signal is one: a light/dark switch is handled by
    /// [`AppearanceTracker`] and never reaches the clock.
    fn into_event(self, at: chrono::DateTime<Utc>) -> Option<hourglass_core::timer::SystemEvent> {
        use hourglass_core::timer::SystemEvent;
        match self {
            mac::Signal::WillSleep => Some(SystemEvent::WillSleep { at }),
            mac::Signal::DidWake => Some(SystemEvent::DidWake { at }),
            mac::Signal::ScreenLocked => Some(SystemEvent::ScreenLocked { at }),
            mac::Signal::ScreenUnlocked => Some(SystemEvent::ScreenUnlocked { at }),
            mac::Signal::AppearanceChanged => None,
        }
    }
}

/// Keeps the drawn palette in step with the saved preference and with macOS.
///
/// Asking macOS which appearance it is in costs a defaults lookup, so the
/// answer is only refreshed when it could actually have changed: the user
/// picked a different option, or macOS said it switched.
struct AppearanceTracker {
    choice: Option<ThemeChoice>,
    applied: Option<Appearance>,
}

impl AppearanceTracker {
    fn new() -> Self {
        AppearanceTracker {
            choice: None,
            applied: None,
        }
    }

    /// Returns whether the palette changed and the window needs repainting.
    fn refresh(&mut self, choice: ThemeChoice, system_switched: bool) -> bool {
        if !system_switched && self.choice == Some(choice) {
            return false;
        }
        self.choice = Some(choice);

        let wanted = match choice {
            ThemeChoice::Light => Appearance::Light,
            ThemeChoice::Dark => Appearance::Dark,
            ThemeChoice::System => {
                if mac::appearance::system_is_dark() {
                    Appearance::Dark
                } else {
                    Appearance::Light
                }
            }
        };

        if self.applied == Some(wanted) {
            return false;
        }
        self.applied = Some(wanted);
        theme::set_appearance(wanted);
        true
    }
}
