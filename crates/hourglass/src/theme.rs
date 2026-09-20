//! Colour, type, and spacing for the whole app.
//!
//! The look is an almanac: paper, ink, and hairline rules, set with a serif for
//! words and a monospace for figures. Structure comes from rules and
//! whitespace, never from cards or shadows.
//!
//! There are two palettes. Light is paper under daylight; dark is the same page
//! under a lamp — warm near-black rather than the cool graphite every other
//! developer tool reaches for. Both carry one warm signal, and it means exactly
//! one thing: the clock is running right now.
//!
//! Views call the accessors below rather than naming colours, so switching
//! palette is a single assignment and nothing else in the app has to know.

use gpui::{App, Font, FontFeatures, FontStyle, FontWeight, Hsla, Rgba, SharedString, rgb, rgba};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// A resolved appearance. [`hourglass_core::model::ThemeChoice::System`] turns
/// into one of these once macOS has been asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

/// Every colour the app draws with.
pub struct Palette {
    /// The page: the report pane and every open expanse.
    pub paper: u32,
    /// The margin: the project rail, set off from the page.
    pub margin: u32,
    /// A surface lifted off the page, for inputs and hovered rows.
    pub raised: u32,
    /// Rules that carry structure.
    pub rule: u32,
    /// Rules between rows of a list, quiet enough to read as texture.
    pub rule_soft: u32,
    /// Primary text. Never pure black on paper, never pure white on ink.
    pub ink: u32,
    /// Secondary text: labels, timestamps, units.
    pub muted: u32,
    /// Text that should recede almost fully.
    pub faint: u32,
    /// The running state. Used nowhere else.
    pub ember: u32,
    /// Destructive actions.
    pub alert: u32,
    /// Project swatches, kept clear of `ember`.
    pub swatches: [u32; 8],
    /// The base a hover wash is mixed from: ink on paper, light on dark.
    wash_base: u32,
}

/// Paper under daylight.
pub const LIGHT: Palette = Palette {
    paper: 0xFAF7F0,
    margin: 0xF1ECE1,
    raised: 0xFFFFFF,
    rule: 0xDCD4C4,
    rule_soft: 0xEBE5D9,
    ink: 0x1A1815,
    muted: 0x6E6659,
    faint: 0xA9A091,
    ember: 0xC2410C,
    alert: 0x9F1239,
    swatches: [
        0x2F4B7C, // indigo
        0x1F6F6B, // teal
        0x6B3FA0, // plum
        0xA83A5B, // rose
        0x55702A, // olive
        0x2A6F97, // sky
        0x5A6472, // slate
        0x8A6516, // ochre
    ],
    // Hovers on paper have to darken; a white wash would do nothing.
    wash_base: 0x1A1815,
};

/// The same page under a lamp.
///
/// Every tone keeps its warmth. A neutral grey here would land straight back on
/// the graphite dashboard this design exists to avoid. The swatches are lifted
/// well above their daylight versions, which would read as mud on this ground.
pub const DARK: Palette = Palette {
    paper: 0x16130F,
    margin: 0x100E0A,
    raised: 0x201C16,
    rule: 0x383127,
    rule_soft: 0x272219,
    ink: 0xF0EAE0,
    muted: 0xA2988A,
    faint: 0x6B6355,
    // Burnt sienna is too dark to carry on this ground, so the signal warms up.
    ember: 0xE8763A,
    alert: 0xE5647E,
    // Lifted for legibility but held back from full saturation: bright pastels
    // would read as a children's chart rather than as the same almanac.
    swatches: [
        0x8098C9, // indigo
        0x5AB3A6, // teal
        0xA88FD0, // plum
        0xCE8595, // rose
        0xA2B56A, // olive
        0x6FAAC9, // sky
        0x98A1AC, // slate
        0xC9B25F, // ochre
    ],
    wash_base: 0xFFFFFF,
};

/// The palette in force, as a plain atom so that whichever thread resolves the
/// appearance and whichever thread draws need not be the same one. Views read
/// it through the accessors below rather than being handed a theme.
static CURRENT: AtomicU8 = AtomicU8::new(LIGHT_TAG);

const LIGHT_TAG: u8 = 0;
const DARK_TAG: u8 = 1;

/// Switch palettes. Takes effect on the next frame.
pub fn set_appearance(appearance: Appearance) {
    let tag = match appearance {
        Appearance::Light => LIGHT_TAG,
        Appearance::Dark => DARK_TAG,
    };
    CURRENT.store(tag, Ordering::Relaxed);
}

pub fn appearance() -> Appearance {
    match CURRENT.load(Ordering::Relaxed) {
        DARK_TAG => Appearance::Dark,
        _ => Appearance::Light,
    }
}

pub fn palette() -> &'static Palette {
    match appearance() {
        Appearance::Light => &LIGHT,
        Appearance::Dark => &DARK,
    }
}

pub fn paper() -> Rgba {
    rgb(palette().paper)
}
pub fn margin() -> Rgba {
    rgb(palette().margin)
}
pub fn raised() -> Rgba {
    rgb(palette().raised)
}
pub fn rule() -> Rgba {
    rgb(palette().rule)
}
pub fn rule_soft() -> Rgba {
    rgb(palette().rule_soft)
}
pub fn ink() -> Rgba {
    rgb(palette().ink)
}
pub fn muted() -> Rgba {
    rgb(palette().muted)
}
pub fn faint() -> Rgba {
    rgb(palette().faint)
}
pub fn ember() -> Rgba {
    rgb(palette().ember)
}
pub fn alert() -> Rgba {
    rgb(palette().alert)
}

/// The swatch for a project's stored colour index.
pub fn swatch(index: u8) -> Rgba {
    let swatches = palette().swatches;
    rgb(swatches[index as usize % swatches.len()])
}

/// A swatch thinned to sit as a track or a border behind text.
pub fn swatch_soft(index: u8, alpha: f32) -> Hsla {
    let mut color: Hsla = swatch(index).into();
    color.a = alpha;
    color
}

/// A hover wash at the given alpha, mixed from whichever base the current
/// palette darkens or lightens with.
pub fn wash(alpha: u32) -> Rgba {
    rgba((palette().wash_base << 8) | (alpha & 0xFF))
}

/// The running colour at a given alpha, for the one tinted surface in the app.
pub fn ember_wash(alpha: u32) -> Rgba {
    rgba((palette().ember << 8) | (alpha & 0xFF))
}

/// The colour text takes when it sits on top of a filled `ember` surface.
pub fn on_ember() -> Rgba {
    match appearance() {
        Appearance::Light => rgb(LIGHT.paper),
        Appearance::Dark => rgb(DARK.margin),
    }
}

/// A slightly stronger `ember`, for the hovered state of a filled button.
pub fn ember_hover() -> Rgba {
    match appearance() {
        Appearance::Light => rgb(0xA83609),
        Appearance::Dark => rgb(0xF2894F),
    }
}

/// Letterspacing the hard way.
///
/// gpui has no tracking control, and small caps set solid read as a smudge, so
/// the wordmark carries thin spaces between its letters.
pub fn letterspaced(text: &str) -> String {
    let mut spaced = String::with_capacity(text.len() * 2);
    for (index, character) in text.chars().enumerate() {
        if index > 0 {
            spaced.push('\u{2009}');
        }
        spaced.push(character);
    }
    spaced
}

/// Build an OpenType feature set from tag/value pairs.
fn features(tags: &[(&str, u32)]) -> FontFeatures {
    FontFeatures(Arc::new(
        tags.iter()
            .map(|(tag, value)| ((*tag).to_string(), *value))
            .collect(),
    ))
}

/// The three typefaces the app uses, resolved once at startup.
#[derive(Clone)]
pub struct Fonts {
    /// System UI face, for controls and short labels.
    pub ui: SharedString,
    /// Serif face, for the hero clock and anything that reads as prose.
    pub display: SharedString,
    /// Monospaced face, for figures in lists where columns must line up.
    pub mono: SharedString,
}

impl Fonts {
    /// Resolve the faces against what is actually installed, so a missing font
    /// can never blank the timer. Every fallback listed here ships with macOS.
    pub fn resolve(cx: &App) -> Self {
        let installed = cx.text_system().all_font_names();
        let pick = |candidates: &[&'static str], last: &'static str| -> &'static str {
            candidates
                .iter()
                .copied()
                .find(|name| installed.iter().any(|found| found == name))
                .unwrap_or(last)
        };

        Fonts {
            ui: SharedString::new_static(".SystemUIFont"),
            display: SharedString::from(pick(
                &["New York", "Iowan Old Style", "Charter", "Palatino", "Georgia"],
                "Georgia",
            )),
            mono: SharedString::from(pick(&["SF Mono", "Menlo", "Monaco"], "Menlo")),
        }
    }

    /// Monospaced figures. Ligatures off so the app never turns `->` into a
    /// glyph, and tabular figures so a seconds column cannot jitter.
    pub fn numeric(&self, weight: FontWeight) -> Font {
        Font {
            family: self.mono.clone(),
            features: features(&[("calt", 0), ("tnum", 1)]),
            fallbacks: None,
            weight,
            style: FontStyle::Normal,
        }
    }

    /// The serif, set for the hero clock.
    ///
    /// `tnum` holds every digit to the same width, and `lnum` forces lining
    /// figures: the old-style figures that Georgia and Iowan use by default
    /// would leave the clock hopping above and below its own baseline.
    pub fn display_numeric(&self, weight: FontWeight) -> Font {
        Font {
            family: self.display.clone(),
            features: features(&[("tnum", 1), ("lnum", 1)]),
            fallbacks: None,
            weight,
            style: FontStyle::Normal,
        }
    }

    /// The serif, set for words.
    pub fn serif(&self, weight: FontWeight) -> Font {
        Font {
            family: self.display.clone(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight,
            style: FontStyle::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels(color: u32) -> (i32, i32, i32) {
        (
            ((color >> 16) & 0xFF) as i32,
            ((color >> 8) & 0xFF) as i32,
            (color & 0xFF) as i32,
        )
    }

    /// Rough perceived brightness, enough to tell paper from lamplight.
    fn luminance(color: u32) -> f32 {
        let (r, g, b) = channels(color);
        0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
    }

    fn distance(a: u32, b: u32) -> i32 {
        let (ar, ag, ab) = channels(a);
        let (br, bg, bb) = channels(b);
        (ar - br).abs() + (ag - bg).abs() + (ab - bb).abs()
    }

    #[test]
    fn the_wordmark_is_spaced_between_letters_only() {
        assert_eq!(letterspaced("AB"), "A\u{2009}B");
        assert_eq!(letterspaced("A"), "A");
        assert_eq!(letterspaced(""), "");
    }

    #[test]
    fn every_swatch_stays_clear_of_the_running_colour_in_both_palettes() {
        // A project swatch that reads as ember would make a stopped project
        // look like a running one at a glance.
        for palette in [&LIGHT, &DARK] {
            for swatch in palette.swatches {
                assert!(
                    distance(swatch, palette.ember) > 80,
                    "{swatch:#08X} sits too close to ember"
                );
            }
        }
    }

    #[test]
    fn text_carries_real_contrast_against_its_own_ground() {
        for palette in [&LIGHT, &DARK] {
            let gap = (luminance(palette.ink) - luminance(palette.paper)).abs();
            assert!(gap > 150.0, "ink and paper are too close: {gap}");
        }
    }

    #[test]
    fn the_dark_palette_is_actually_dark_and_the_light_one_light() {
        assert!(luminance(LIGHT.paper) > 200.0);
        assert!(luminance(DARK.paper) < 60.0);
        assert!(luminance(LIGHT.ink) < 60.0);
        assert!(luminance(DARK.ink) > 200.0);
    }

    #[test]
    fn a_wash_darkens_on_paper_and_lightens_under_the_lamp() {
        set_appearance(Appearance::Light);
        assert_eq!(wash(0x0A), rgba(0x1A18150A));

        set_appearance(Appearance::Dark);
        assert_eq!(wash(0x0A), rgba(0xFFFFFF0A));

        set_appearance(Appearance::Light);
    }

    #[test]
    fn switching_appearance_switches_every_colour() {
        set_appearance(Appearance::Light);
        let light_paper = paper();
        set_appearance(Appearance::Dark);
        assert_ne!(paper(), light_paper);
        assert_eq!(paper(), rgb(DARK.paper));

        set_appearance(Appearance::Light);
    }
}
