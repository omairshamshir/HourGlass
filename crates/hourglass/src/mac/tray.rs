//! The menu bar item: an hourglass, the running time, and a short menu.
//!
//! The title shows hours and minutes rather than seconds. A per-second
//! repaint in the menu bar is motion the user sees thousands of times a day,
//! which reads as noise and shifts the neighbouring icons on every tick. The
//! seconds live in the window, where they are actually being watched.

use hourglass_core::model::ProjectId;
use std::collections::HashMap;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Something the user picked from the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Start(ProjectId),
    Stop,
    Resume,
    Dismiss,
    OpenWindow,
    Quit,
}

/// What the menu should currently say.
pub struct TrayModel<'a> {
    /// Project name and elapsed seconds, when the clock is running.
    pub running: Option<(&'a str, i64)>,
    /// A project waiting on a resume answer.
    pub paused: Option<&'a str>,
    /// Projects offered for a one-click start, most recent first.
    pub shortcuts: Vec<(ProjectId, &'a str)>,
}

impl TrayModel<'_> {
    /// A cheap value that changes exactly when the menu's text changes, so the
    /// menu is only rebuilt when it would actually look different.
    fn fingerprint(&self) -> String {
        let running = self.running.map(|(name, _)| name).unwrap_or("");
        let shortcuts: Vec<&str> = self.shortcuts.iter().map(|(_, name)| *name).collect();
        format!(
            "{running}|{}|{}",
            self.paused.unwrap_or(""),
            shortcuts.join(",")
        )
    }
}

/// The status item and the menu hanging off it.
pub struct Tray {
    icon: TrayIcon,
    commands: HashMap<MenuId, TrayCommand>,
    menu_fingerprint: String,
    title: Option<String>,
}

impl Tray {
    /// Create the status item. Returns `None` if macOS refuses, in which case
    /// the app still works from its window.
    pub fn new() -> Option<Self> {
        let icon = TrayIconBuilder::new()
            .with_icon(hourglass_icon())
            .with_icon_as_template(true)
            .with_tooltip("Hourglass")
            .build()
            .ok()?;

        Some(Tray {
            icon,
            commands: HashMap::new(),
            menu_fingerprint: String::new(),
            title: None,
        })
    }

    /// Rebuild the menu if anything in it would read differently.
    pub fn sync_menu(&mut self, model: &TrayModel<'_>) {
        let fingerprint = model.fingerprint();
        if fingerprint == self.menu_fingerprint {
            return;
        }
        self.menu_fingerprint = fingerprint;
        self.commands.clear();

        let menu = Menu::new();
        let mut remember = |item: &MenuItem, command: TrayCommand| {
            self.commands.insert(item.id().clone(), command);
        };

        match model.running {
            Some((name, _)) => {
                let stop = MenuItem::new(format!("Stop {name}"), true, None);
                remember(&stop, TrayCommand::Stop);
                let _ = menu.append(&stop);
            }
            None => {
                let idle = MenuItem::new("No timer running", false, None);
                let _ = menu.append(&idle);
            }
        }

        if let Some(paused) = model.paused {
            let resume = MenuItem::new(format!("Resume {paused}"), true, None);
            remember(&resume, TrayCommand::Resume);
            let _ = menu.append(&resume);

            let dismiss = MenuItem::new("Leave it stopped", true, None);
            remember(&dismiss, TrayCommand::Dismiss);
            let _ = menu.append(&dismiss);
        }

        if !model.shortcuts.is_empty() {
            let _ = menu.append(&PredefinedMenuItem::separator());
            let start = Submenu::new("Start", true);
            for (id, name) in &model.shortcuts {
                let item = MenuItem::new(*name, true, None);
                remember(&item, TrayCommand::Start(*id));
                let _ = start.append(&item);
            }
            let _ = menu.append(&start);
        }

        let _ = menu.append(&PredefinedMenuItem::separator());

        let open = MenuItem::new("Open Hourglass", true, None);
        remember(&open, TrayCommand::OpenWindow);
        let _ = menu.append(&open);

        let quit = MenuItem::new("Quit", true, None);
        remember(&quit, TrayCommand::Quit);
        let _ = menu.append(&quit);

        self.icon.set_menu(Some(Box::new(menu)));
    }

    /// Show elapsed time beside the icon, or nothing when stopped.
    pub fn set_title(&mut self, title: Option<String>) {
        if title == self.title {
            return;
        }
        self.icon.set_title(title.as_deref());
        self.title = title;
    }

    /// Everything the user clicked since the last call.
    pub fn poll(&self) -> Vec<TrayCommand> {
        let receiver = MenuEvent::receiver();
        let mut commands = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            if let Some(command) = self.commands.get(&event.id) {
                commands.push(*command);
            }
        }
        commands
    }
}

/// The menu bar glyph: two triangles meeting at a waist, drawn as a template
/// image so macOS inverts it for light and dark menu bars automatically.
fn hourglass_icon() -> Icon {
    const SIZE: u32 = 36;
    const FRAME: u32 = 2;

    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    let centre = (SIZE - 1) as f32 / 2.0;
    let cap_top = 4.0;
    let cap_bottom = (SIZE - 5) as f32;
    let half_width = 11.0;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let fx = x as f32;
            let fy = y as f32;

            // The two horizontal caps.
            let in_cap = (fy >= cap_top - FRAME as f32 && fy < cap_top)
                || (fy > cap_bottom && fy <= cap_bottom + FRAME as f32);
            let within_cap_width = (fx - centre).abs() <= half_width;

            // The bulbs: half-width shrinks to nothing at the waist.
            let distance_from_waist = (fy - centre).abs();
            let reach = (distance_from_waist / (cap_bottom - centre)) * half_width;
            let in_bulb = fy >= cap_top && fy <= cap_bottom && (fx - centre).abs() <= reach;

            if (in_cap && within_cap_width) || in_bulb {
                let offset = ((y * SIZE + x) * 4) as usize;
                pixels[offset + 3] = 255; // template images carry shape in alpha
            }
        }
    }

    Icon::from_rgba(pixels, SIZE, SIZE).expect("the generated icon is a valid RGBA buffer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_icon_has_the_expected_dimensions() {
        // Building it at all proves the buffer length matches the dimensions,
        // which is the only way from_rgba can fail.
        let _ = hourglass_icon();
    }

    #[test]
    fn the_fingerprint_changes_when_the_running_project_changes() {
        let stopped = TrayModel {
            running: None,
            paused: None,
            shortcuts: vec![],
        };
        let running = TrayModel {
            running: Some(("Atlas", 60)),
            paused: None,
            shortcuts: vec![],
        };
        assert_ne!(stopped.fingerprint(), running.fingerprint());
    }

    #[test]
    fn the_fingerprint_ignores_the_ticking_seconds() {
        let a = TrayModel {
            running: Some(("Atlas", 60)),
            paused: None,
            shortcuts: vec![],
        };
        let b = TrayModel {
            running: Some(("Atlas", 61)),
            paused: None,
            shortcuts: vec![],
        };
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
}
