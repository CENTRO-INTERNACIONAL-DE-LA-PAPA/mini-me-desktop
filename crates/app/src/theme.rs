//! The palette, as **roles** rather than colours.
//!
//! Before this, seven `const`s lived in `main.rs` and one of them — the brand orange — was
//! used for section labels, links, buttons, the running mark, the host-execution warning
//! and every border that wanted attention. When everything is emphasised nothing is, which
//! is most of why the app read as "awful": there was no visual difference between *this is
//! a heading*, *this is clickable* and *this needs your attention*.
//!
//! The fix is the one every dark-mode design system converges on: name colours for the job
//! they do, not the colour they are, and give surfaces an **elevation ladder** so panels,
//! cards and popovers separate without borders everywhere.
//!
//! Two rules this file exists to enforce:
//!
//! 1. **Orange means "you can act on this", and nothing else.** Headings are muted text.
//!    Status has its own colours. A researcher should be able to learn "orange = clickable"
//!    in one session and never be wrong.
//! 2. **Every text/background pair passes WCAG AA (4.5:1).** Not judged by eye — there is a
//!    test below that computes the ratios and fails the build.
//!
//! Neutrals carry a slight warm tint so they sit with the orange rather than fighting it;
//! a pure grey next to a saturated warm accent reads blue by comparison.

/// Window background — the deepest surface.
pub const BG: u32 = 0x16161a;
/// Panels: the rail, the side panes, the composer strip.
pub const SURFACE: u32 = 0x1c1c21;
/// Lifted off the panel: cards, list rows, hover.
pub const RAISED: u32 = 0x232329;
/// Floating above everything: the command palette, popovers.
pub const OVERLAY: u32 = 0x2a2a31;
/// The selected conversation, and any other "this is the current one" fill.
///
/// A dark, desaturated orange rather than the accent at low opacity — GPUI composites
/// solid colours here, and a tint that dark keeps the row's text at full contrast.
pub const ACCENT_SOFT: u32 = 0x3a2419;

/// Primary reading colour.
pub const TEXT: u32 = 0xececf0;
/// Secondary: labels, descriptions, anything supporting the primary text.
pub const TEXT_MUTED: u32 = 0xb0b0ba;
/// Tertiary: timestamps, counts, the quietest metadata. Still AA — "faint" is not
/// permission to be unreadable.
pub const TEXT_FAINT: u32 = 0x9a9aa5;

/// Ordinary separators.
pub const BORDER: u32 = 0x2f2f37;
/// Separators that need to be seen — an input's edge, a card that must read as a unit.
pub const BORDER_STRONG: u32 = 0x3f3f49;

/// Mini-Me orange. **Interactive things only.**
pub const ACCENT: u32 = 0xe8703a;
/// The same orange with the lift a pointer expects. Every clickable surface should change
/// under the cursor; without that, a thing that looks like a button but does not react
/// reads as broken.
pub const ACCENT_HOVER: u32 = 0xf58b5c;

/// Finished, succeeded, present.
pub const SUCCESS: u32 = 0x5bbd7a;
/// Waiting on a person, or proceeding with a caveat.
pub const WARNING: u32 = 0xd9a441;
/// Failed.
pub const ERROR: u32 = 0xf1676b;
/// Working right now.
pub const RUNNING: u32 = 0x6aa9e0;

#[cfg(test)]
mod tests {
    use super::*;

    /// One channel, linearised — the sRGB → linear step of the WCAG formula.
    fn channel(value: u32) -> f64 {
        let c = value as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(colour: u32) -> f64 {
        let r = channel((colour >> 16) & 0xff);
        let g = channel((colour >> 8) & 0xff);
        let b = channel(colour & 0xff);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// WCAG 2.1 contrast ratio, 1.0 (identical) to 21.0 (black on white).
    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        let (lighter, darker) = if x > y { (x, y) } else { (y, x) };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn every_text_colour_is_readable_on_every_surface() {
        // 4.5:1 is WCAG AA for normal-size text. This app's smallest text is 12px, so AA
        // is the floor, not a stretch goal — and "faint" metadata is exactly the text
        // someone squints at, which is why it is in this list rather than exempt from it.
        for (surface, surface_name) in [
            (BG, "BG"),
            (SURFACE, "SURFACE"),
            (RAISED, "RAISED"),
            (OVERLAY, "OVERLAY"),
            (ACCENT_SOFT, "ACCENT_SOFT"),
        ] {
            for (ink, ink_name) in [
                (TEXT, "TEXT"),
                (TEXT_MUTED, "TEXT_MUTED"),
                (TEXT_FAINT, "TEXT_FAINT"),
                (ACCENT, "ACCENT"),
                (SUCCESS, "SUCCESS"),
                (WARNING, "WARNING"),
                (ERROR, "ERROR"),
                (RUNNING, "RUNNING"),
            ] {
                let ratio = contrast(ink, surface);
                assert!(
                    ratio >= 4.5,
                    "{ink_name} on {surface_name} is {ratio:.2}:1 — below WCAG AA (4.5:1)"
                );
            }
        }
    }

    #[test]
    fn the_surfaces_form_a_ladder_and_the_hover_state_lifts() {
        // Each step up must actually be lighter, or "elevated" means nothing and the
        // panels read as one flat sheet — which is what they did before this file.
        let ladder = [BG, SURFACE, RAISED, OVERLAY];
        for pair in ladder.windows(2) {
            assert!(
                luminance(pair[1]) > luminance(pair[0]),
                "{:06x} should sit above {:06x}",
                pair[1],
                pair[0]
            );
        }
        // A pointer landing on something interactive has to produce a visible change.
        assert!(luminance(ACCENT_HOVER) > luminance(ACCENT));
        assert!(luminance(BORDER_STRONG) > luminance(BORDER));
    }
}
