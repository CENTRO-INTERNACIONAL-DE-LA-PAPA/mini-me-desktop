//! The palette — **switchable at runtime**, as roles rather than colours.
//!
//! Two problems, one file. The first was that seven `const`s lived in `main.rs` and the
//! brand orange did six different jobs, so nothing read as emphasised. The second is that a
//! single fixed palette is a bet on everyone's taste and everyone's room: orange-on-charcoal
//! is unreadable on a projector and unpleasant to some people at any time.
//!
//! **Modelled on Zed's theme system**, adapted rather than copied. Zed ships a *theme
//! family* as JSON with semantic style keys — `background`, `text`, `accent`, `border`,
//! `elevated_surface.background` — and loads extra families from extensions. We take the
//! shape (named roles, JSON, several families) and skip the registry: a researcher wants to
//! pick a palette, not publish one.
//!
//! Three rules this file enforces, with tests rather than judgement:
//!
//! 1. **Orange — or whatever the accent is — means "you can act on this", and nothing
//!    else.** Headings are muted text; status has its own colours.
//! 2. **Every text/surface pair in every theme passes WCAG AA (4.5:1).** A theme that ships
//!    here cannot be unreadable, including ones added later.
//! 3. **Surfaces form an elevation ladder**, so panels and popovers separate without
//!    drawing yet another border.
//!
//! Colours are read through functions backed by atomics rather than consts, so switching a
//! theme is a store and the next frame picks it up — and so the free rendering helpers,
//! which have no `Context` to reach a GPUI global through, can still ask.

use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

/// One palette. Field names are the roles, and are what a theme file writes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Window background — the deepest surface.
    pub background: u32,
    /// Panels: the sidebar, the side panes, the composer strip.
    pub surface: u32,
    /// Lifted off the panel: cards, list rows, hover.
    pub elevated: u32,
    /// Floating above everything: modals, the command palette, popovers.
    pub overlay: u32,
    /// The selected row's fill.
    pub accent_soft: u32,

    /// Primary reading colour.
    pub text: u32,
    /// Secondary: labels, descriptions, supporting text.
    pub text_muted: u32,
    /// Tertiary: timestamps, counts, sizes. Still AA — "faint" is not permission to be
    /// unreadable.
    pub text_faint: u32,

    pub border: u32,
    /// Borders that must be seen: an input's edge, a card that reads as one unit.
    pub border_strong: u32,

    /// **Interactive things only.**
    pub accent: u32,
    /// The accent with the lift a pointer expects.
    pub accent_hover: u32,

    pub success: u32,
    pub warning: u32,
    pub error: u32,
    /// Working right now.
    pub running: u32,
}

/// The themes that ship with the app.
///
/// Four, chosen to cover real situations rather than to fill a gallery: the default, one
/// for people who do not want orange, one for a bright room or a projector, and one for
/// anybody who finds the others too low-contrast.
pub const THEMES: [(&str, Theme); 4] = [
    ("Mini-Me Dark", MINI_ME_DARK),
    ("Slate", SLATE),
    ("Paper", PAPER),
    ("High Contrast", HIGH_CONTRAST),
];

/// Warm charcoal and the Mini-Me orange. Neutrals carry a slight warm tint so they sit
/// with the accent — a pure grey beside a saturated warm colour reads blue by comparison.
pub const MINI_ME_DARK: Theme = Theme {
    background: 0x16161a,
    surface: 0x1c1c21,
    elevated: 0x232329,
    overlay: 0x2a2a31,
    accent_soft: 0x3a2419,
    text: 0xececf0,
    text_muted: 0xb0b0ba,
    text_faint: 0x9a9aa5,
    border: 0x2f2f37,
    border_strong: 0x3f3f49,
    accent: 0xe8703a,
    accent_hover: 0xf58b5c,
    success: 0x5bbd7a,
    warning: 0xd9a441,
    error: 0xf1676b,
    running: 0x6aa9e0,
};

/// Cool neutrals and a blue accent, for people who simply do not want an orange editor.
pub const SLATE: Theme = Theme {
    background: 0x14171c,
    surface: 0x1a1e24,
    elevated: 0x222731,
    overlay: 0x2a303b,
    accent_soft: 0x1c3048,
    text: 0xe6eaf0,
    text_muted: 0xacb4c0,
    text_faint: 0x949dab,
    border: 0x2b313b,
    border_strong: 0x3b434f,
    accent: 0x6cb0f5,
    accent_hover: 0x93c6fa,
    success: 0x5cc08a,
    warning: 0xd9a441,
    error: 0xf87a7e,
    running: 0x8ab4f8,
};

/// A light theme, for a bright room, a projector, or a shared screen. The one situation
/// where a dark UI genuinely fails, and the reason this is not a dark-only app.
pub const PAPER: Theme = Theme {
    // A grey canvas with white cards, so elevation raises luminance here exactly as it
    // does in the dark themes — one rule for every palette rather than a special case.
    background: 0xf1efea,
    surface: 0xf7f5f1,
    elevated: 0xfbfaf8,
    overlay: 0xffffff,
    accent_soft: 0xf7ddcd,
    text: 0x24242a,
    text_muted: 0x55555f,
    text_faint: 0x5b5b66,
    border: 0xdcd8d1,
    border_strong: 0xc3beb5,
    accent: 0xa8451a,
    accent_hover: 0x8c3813,
    success: 0x14663a,
    warning: 0x855c05,
    error: 0xb32431,
    running: 0x1f5fa8,
};

/// Maximum separation, for low vision or a bad screen.
pub const HIGH_CONTRAST: Theme = Theme {
    background: 0x000000,
    surface: 0x0b0b0d,
    elevated: 0x17171b,
    overlay: 0x1f1f25,
    accent_soft: 0x442a12,
    text: 0xffffff,
    text_muted: 0xd8d8de,
    text_faint: 0xb9b9c2,
    border: 0x40404a,
    border_strong: 0x5a5a66,
    accent: 0xffa05c,
    accent_hover: 0xffbb85,
    success: 0x67e08d,
    warning: 0xf2c14e,
    error: 0xff7d80,
    running: 0x8cc2ff,
};

/// Whether a theme is light, so anything computing a shade knows which way is "darker".
pub fn is_light(theme: &Theme) -> bool {
    luminance(theme.background) > 0.5
}

macro_rules! live_theme {
    ($($field:ident => $getter:ident, $slot:ident, $default:expr;)*) => {
        $(
            static $slot: AtomicU32 = AtomicU32::new($default);
            /// The live value of this role. Cheap enough to call per element per frame.
            pub fn $getter() -> u32 { $slot.load(Ordering::Relaxed) }
        )*

        /// Make `theme` the one the next frame draws with.
        pub fn apply(theme: &Theme) {
            $( $slot.store(theme.$field, Ordering::Relaxed); )*
        }

        /// The live theme, as a value — for anything that wants the whole palette.
        pub fn current() -> Theme {
            Theme { $( $field: $getter(), )* }
        }
    };
}

live_theme! {
    background   => background,   BACKGROUND_SLOT,    0x16161a;
    surface      => surface,      SURFACE_SLOT,       0x1c1c21;
    elevated     => elevated,     ELEVATED_SLOT,      0x232329;
    overlay      => overlay,      OVERLAY_SLOT,       0x2a2a31;
    accent_soft  => accent_soft,  ACCENT_SOFT_SLOT,   0x3a2419;
    text         => text,         TEXT_SLOT,          0xececf0;
    text_muted   => text_muted,   TEXT_MUTED_SLOT,    0xb0b0ba;
    text_faint   => text_faint,   TEXT_FAINT_SLOT,    0x9a9aa5;
    border       => border,       BORDER_SLOT,        0x2f2f37;
    border_strong=> border_strong,BORDER_STRONG_SLOT, 0x3f3f49;
    accent       => accent,       ACCENT_SLOT,        0xe8703a;
    accent_hover => accent_hover, ACCENT_HOVER_SLOT,  0xf58b5c;
    success      => success,      SUCCESS_SLOT,       0x5bbd7a;
    warning      => warning,      WARNING_SLOT,       0xd9a441;
    error        => error,        ERROR_SLOT,         0xf1676b;
    running      => running,      RUNNING_SLOT,       0x6aa9e0;
}

/// One channel, linearised — the sRGB → linear step of the WCAG formula.
fn channel(value: u32) -> f64 {
    let c = value as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn luminance(colour: u32) -> f64 {
    let r = channel((colour >> 16) & 0xff);
    let g = channel((colour >> 8) & 0xff);
    let b = channel(colour & 0xff);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2.1 contrast ratio: 1.0 (identical) to 21.0 (black on white).
///
/// Used by the tests that gate every shipped palette. Kept public and outside `cfg(test)`
/// so a theme added later can be checked the same way rather than by eye.
#[cfg_attr(not(test), allow(dead_code))]
pub fn contrast(a: u32, b: u32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (lighter, darker) = if x > y { (x, y) } else { (y, x) };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_theme_is_readable() {
        // 4.5:1 is WCAG AA for normal text, and the smallest text here is 12px — so AA is
        // the floor, not a stretch goal. This runs over *every* theme, which is the point:
        // a palette added later cannot be added unreadable.
        for (name, theme) in THEMES {
            for (surface, surface_name) in [
                (theme.background, "background"),
                (theme.surface, "surface"),
                (theme.elevated, "elevated"),
                (theme.overlay, "overlay"),
                (theme.accent_soft, "accent_soft"),
            ] {
                for (ink, ink_name) in [
                    (theme.text, "text"),
                    (theme.text_muted, "text_muted"),
                    (theme.text_faint, "text_faint"),
                    (theme.accent, "accent"),
                    (theme.success, "success"),
                    (theme.warning, "warning"),
                    (theme.error, "error"),
                    (theme.running, "running"),
                ] {
                    let ratio = contrast(ink, surface);
                    assert!(
                        ratio >= 4.5,
                        "{name}: {ink_name} on {surface_name} is {ratio:.2}:1, below AA"
                    );
                }
            }
        }
    }

    #[test]
    fn every_theme_has_a_ladder_and_a_visible_hover() {
        for (name, theme) in THEMES {
            let ladder = [
                theme.background,
                theme.surface,
                theme.elevated,
                theme.overlay,
            ];
            for pair in ladder.windows(2) {
                // One rule for every palette: elevation raises luminance. The light theme
                // gets there with a grey canvas and white cards rather than by inverting,
                // which keeps "elevated" meaning the same thing everywhere.
                assert!(
                    luminance(pair[1]) > luminance(pair[0]),
                    "{name}: {:06x} does not sit above {:06x}",
                    pair[1],
                    pair[0]
                );
            }
            // A pointer landing on something interactive must produce a visible change,
            // in whichever direction that theme moves.
            assert!(
                (luminance(theme.accent_hover) - luminance(theme.accent)).abs() > 0.01,
                "{name}: hover is indistinguishable from the accent"
            );
        }
    }

    #[test]
    fn a_zed_theme_file_loads() {
        // The shape Zed publishes: a family with several themes, each with an appearance
        // and a style object of the 142 keys we take fifteen of.
        let family = serde_json::json!({
            "name": "Example Family",
            "themes": [
                {
                    "name": "Example Dark",
                    "appearance": "dark",
                    "style": {
                        "background": "#101014ff",
                        "panel.background": "#16161bff",
                        "elevated_surface.background": "#1e1e25ff",
                        "text": "#e0e0e6ff",
                        "text.muted": "#a0a0aaff",
                        "border": "#2a2a32ff",
                        "accent": "#7aa2f7ff",
                        "error": "#f77070ff",
                        "unknown.key.we.ignore": "#123456ff"
                    }
                },
                { "name": "Example Light", "appearance": "light", "style": {"background": "#fafafaff"} }
            ]
        });
        let themes = from_zed_family(&family);
        assert_eq!(themes.len(), 2);

        let (name, dark) = &themes[0];
        assert_eq!(name, "Example Dark");
        assert_eq!(
            dark.background, 0x101014,
            "alpha is dropped, not parsed as colour"
        );
        assert_eq!(dark.surface, 0x16161b);
        assert_eq!(dark.accent, 0x7aa2f7);
        // A key Zed does not define falls back rather than landing on black.
        assert_eq!(dark.warning, MINI_ME_DARK.warning);
        // A derived hover has to actually differ, and move away from the background.
        assert!(luminance(dark.accent_hover) > luminance(dark.accent));

        // A light theme falls back to the light built-in, not the dark one.
        let (_, light) = &themes[1];
        assert_eq!(light.text, PAPER.text);

        // Anything that is not a theme family yields nothing rather than erroring.
        assert!(from_zed_family(&serde_json::json!({"nope": 1})).is_empty());
    }

    #[test]
    fn applying_a_theme_changes_what_the_next_frame_reads() {
        apply(&SLATE);
        assert_eq!(accent(), SLATE.accent);
        assert_eq!(current(), SLATE);
        apply(&PAPER);
        assert_eq!(background(), PAPER.background);
        assert!(is_light(&current()));
        apply(&MINI_ME_DARK);
    }
}

/// Import palettes from a **Zed theme file**.
///
/// Zed's gallery is the answer to "I want more than four themes", and a Zed theme is just
/// JSON — so a researcher can download any of them and drop it in `themes/`. What we
/// cannot use is a Zed *extension*: those are WASM against Zed's own API, and installing
/// one here would mean implementing Zed. The theme JSON inside them is portable, and that
/// is the part worth having.
///
/// Keys are from the published schema (`zed.dev/schema/themes/v0.2.0.json`), which carries
/// 142 style properties — this maps the fifteen that mean something to this app and lets
/// the rest go. Every field falls back, so a partial theme loads rather than failing.
pub fn from_zed_family(json: &serde_json::Value) -> Vec<(String, Theme)> {
    let Some(themes) = json.get("themes").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    themes
        .iter()
        .filter_map(|entry| {
            let name = entry.get("name")?.as_str()?.trim().to_string();
            let style = entry.get("style")?;
            // Which built-in to fall back to, so a missing key lands somewhere sane
            // rather than on black.
            let base = match entry.get("appearance").and_then(|a| a.as_str()) {
                Some("light") => PAPER,
                _ => MINI_ME_DARK,
            };
            let pick = |key: &str, fallback: u32| -> u32 {
                style
                    .get(key)
                    .and_then(|value| value.as_str())
                    .and_then(parse_hex)
                    .unwrap_or(fallback)
            };
            let accent = pick("accent", base.accent);
            let background = pick("background", base.background);
            let surface = pick("panel.background", pick("surface.background", base.surface));
            Some((
                name,
                Theme {
                    background,
                    surface,
                    elevated: pick("elevated_surface.background", base.elevated),
                    // Zed has no separate overlay role; its elevated surface is what
                    // modals sit on, nudged so our ladder still ascends.
                    overlay: nudge(
                        pick("elevated_surface.background", base.overlay),
                        background,
                    ),
                    accent_soft: pick("element.selected", base.accent_soft),
                    text: pick("text", base.text),
                    text_muted: pick("text.muted", base.text_muted),
                    text_faint: pick("text.placeholder", pick("text.muted", base.text_faint)),
                    border: pick("border", base.border),
                    border_strong: pick("border.focused", pick("border", base.border_strong)),
                    accent,
                    // Zed states no hover colour for the accent, so derive one that moves
                    // away from the background — lighter on dark, darker on light.
                    accent_hover: nudge(accent, background),
                    success: pick("success", base.success),
                    warning: pick("warning", base.warning),
                    error: pick("error", base.error),
                    running: pick("info", base.running),
                },
            ))
        })
        .collect()
}

/// `#rrggbb` or `#rrggbbaa` → `0xrrggbb`. Alpha is dropped: GPUI composites these as
/// solid fills here, and a half-transparent panel over a half-transparent panel is how
/// text stops meeting its contrast ratio.
fn parse_hex(value: &str) -> Option<u32> {
    let hex = value.trim().trim_start_matches('#');
    if hex.len() < 6 || !hex.is_char_boundary(6) {
        return None;
    }
    u32::from_str_radix(&hex[..6], 16).ok()
}

/// `colour`, moved one step further from `background`.
///
/// Lighter on a dark theme, darker on a light one — so a derived hover state is visible
/// whichever way the palette runs.
fn nudge(colour: u32, background: u32) -> u32 {
    let towards_light = luminance(background) < 0.5;
    let shift = |channel: u32| -> u32 {
        if towards_light {
            (channel + 26).min(255)
        } else {
            channel.saturating_sub(26)
        }
    };
    (shift((colour >> 16) & 0xff) << 16) | (shift((colour >> 8) & 0xff) << 8) | shift(colour & 0xff)
}
