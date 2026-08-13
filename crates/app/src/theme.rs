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
/// Eight, chosen to cover real situations rather than to fill a gallery: the native-potato
/// pair, the bench pair, the original pair, one for anybody who does not want a warm palette,
/// and one for anybody who finds the others too low-contrast.
///
/// **First is the default**, and that is not a comment — [`DEFAULT`] and [`DEFAULT_NAME`] read
/// this array rather than restating it, so the palette a fresh install opens on is decided in
/// exactly one place. It used to be decided in three (the `live_theme!` defaults,
/// `Settings::default`, and `apply_theme`'s fallback), which is the shape this project keeps
/// getting wrong: several facts that have to agree, with more than one of them saying so.
pub const THEMES: [(&str, Theme); 8] = [
    ("Papa Nativa", PAPA_NATIVA),
    ("Papa Nativa Light", PAPA_NATIVA_LIGHT),
    ("Bench", BENCH),
    ("Bench Night", BENCH_NIGHT),
    ("Mini-Me Dark", MINI_ME_DARK),
    ("Slate", SLATE),
    ("Paper", PAPER),
    ("High Contrast", HIGH_CONTRAST),
];

/// The palette a fresh install opens on.
pub const DEFAULT: Theme = THEMES[0].1;

/// Its name, as `settings.toml` writes it.
pub const DEFAULT_NAME: &str = THEMES[0].0;

/// A reading palette on CIP's colours: one aubergine hue for the room, four for the signals.
///
/// **This is not an editor theme, and §183 is the correction.** Reported as *"it seems these
/// themes are for coders — our users are scientists that read, analyze"*, which is a description
/// of the whole genre this palette had been borrowing from: dark ground, saturated accents,
/// syntax-colour habits. A scientist reading a two-page answer wants a room that holds still.
///
/// Every value below was solved rather than picked, against published guidance (docs §183):
///
/// - **Body text sits at APCA Lc 90**, the level APCA names as *preferred for columns of body
///   text*. Not higher: the first cut ran at 15.3:1 WCAG, near the maximum available, and
///   maximum contrast is what makes light text bloom into a dark ground.
/// - **No pure black.** The ground is OKLCH L 0.20, in the region every dark-mode guide settles
///   on; reading speed measurably drops on pure-black themes.
/// - **Saturated colour is what vibrates on dark**, so the signals carry OKLCH chroma **0.09**
///   against print originals of 0.15–0.24. Equal chroma across all four, which in a perceptual
///   space means equal visual weight — no single status shouts over the others.
/// - **The surface ladder is four even OKLCH lightness steps** (0.20 → 0.32) at chroma 0.018 on
///   one hue. Even in OKLCH means even to the eye, which is the entire reason for using it.
///
/// **The hue is CIP's, throughout.** The room and the accent are both 2607 C purple's hue
/// (308°); what separates a panel from a link is *chroma alone* — 0.018 against 0.09. The four
/// signals are 369 C green, 137 C amber, 1795 C red and Process Cyan, each within 1° of the
/// printed colour. Only lightness and chroma moved, because those are the two a screen forces
/// and hue is what makes a colour recognisable.
///
/// `accent_soft` is CIP orange at 12% over `surface`, pre-composited to an opaque colour. GPUI's
/// theme roles are solid `u32` fills and the Zed importer deliberately drops alpha, so storing a
/// translucent orange would render differently depending on which of four surfaces happened to
/// sit behind it. The composite gives the requested soft orange one predictable appearance (§181).
pub const PAPA_NATIVA: Theme = Theme {
    background: 0x18141c,
    surface: 0x221d26,
    elevated: 0x2b2730,
    overlay: 0x35303a,
    accent_soft: 0x3a2722,
    // Lc 90 / 76 / 66 against the page: three roles a reader can tell apart without any of them
    // being a colour. The previous pair measured 76 and 66 and rendered five hex digits apart.
    text: 0xe7e4eb,
    text_muted: 0xd1ccd6,
    text_faint: 0xc0bbc6,
    border: 0x3c3444,
    border_strong: 0x594d64,
    // 2607 C's hue at reading weight. The transcript draws filenames and column names in this,
    // so it is body text and is held to the body-text floor: Lc 76 on `surface`. Its predecessor
    // was Lc 47 — under the floor for *incidental* text — while WCAG called it a healthy 6.1:1.
    accent: 0xe2c3ff,
    accent_hover: 0xf3d3ff,
    success: 0xb1d396,
    warning: 0xe6c485,
    error: 0xffb7a7,
    running: 0x8ed0fb,
};

/// The same room with the lights on — for the bench, the greenhouse window and the projector.
///
/// **The counterpart §181 said to build.** Making a dark theme the default reversed a decision
/// that was never about colour identity — *"it is read next to a bench, a greenhouse window and
/// a projector, and those are the rooms a dark UI actually fails in"* — and the answer recorded
/// there was to ship Papa Nativa's own light half rather than send anybody back to teal.
///
/// Built to the same targets as its dark half and on the same CIP hues, so the two are one
/// identity under two lights rather than two themes. Three things differ, and each is forced:
///
/// - **The page is not pure white.** `background` is OKLCH L 0.955; a full-brightness white page
///   is the light-mode equivalent of a pure-black one, and off-white is the standing advice for
///   long reading.
/// - **Surface chroma drops to 0.006**, a third of the dark half's. The same tint reads far
///   stronger against paper than against near-black.
/// - **Signal chroma rises to 0.11**, because a light ground needs more of it to say the same
///   thing — and can carry it without vibrating.
///
/// `text_faint` is the one value APCA did not settle alone. At the Lc it wanted, it measured 4.4
/// against the orange-tinted row — under the WCAG floor this repo enforces. It is darker than
/// APCA asks so that both scales pass, which is the honest resolution when two measures disagree.
pub const PAPA_NATIVA_LIGHT: Theme = Theme {
    background: 0xf1eff3,
    surface: 0xf7f5f9,
    elevated: 0xfbf9fd,
    overlay: 0xfefcff,
    // CIP orange at 12% over `surface` — the tint a selected row gets, and the only place the
    // logo orange appears. On paper this is the *darkest* surface, so it is what every ink here
    // had to be checked against.
    accent_soft: 0xf6e5db,
    text: 0x333135,
    text_muted: 0x545158,
    text_faint: 0x68646c,
    border: 0xe4e0e7,
    border_strong: 0xc4bfc8,
    // 2607 C `#56217A` almost exactly — on paper the printed purple needs no rescuing, only a
    // little lightness so it reads as ink rather than as a block of colour.
    accent: 0x6a4688,
    accent_hover: 0x512d6d,
    success: 0x456920,
    warning: 0x7d5800,
    error: 0x964738,
    running: 0x006492,
};

/// Neutral paper and one deep teal. For bright rooms and shared screens.
///
/// This was the default until §181, on an argument that is *not* about colour identity and so
/// was not answered by the one that replaced it: the app opened on charcoal because editors do,
/// and this is not an editor — it is read next to a bench, a greenhouse window and a projector,
/// and those are the rooms a dark UI actually fails in. Papa Nativa is the better palette and a
/// dark one; if a fresh install in a greenhouse turns out to be the case that matters, the fix
/// is to ship Papa Nativa's own light counterpart, not to move the default back to teal.
///
/// The teal is held to the things you can act on; nothing else in the window is saturated.
pub const BENCH: Theme = Theme {
    background: 0xedebe6,
    surface: 0xf6f5f1,
    elevated: 0xfcfcfa,
    overlay: 0xffffff,
    accent_soft: 0xddede7,
    text: 0x2f343a,
    text_muted: 0x5e656b,
    // Two inks sit a shade darker than the design named them: `text_faint` came as 0x666d73
    // and `running` as 0x2f6fa8, which measure 4.41:1 and 4.45:1 on the background — under the
    // 4.5 floor `every_shipped_theme_is_readable` enforces, and under it again on `accent_soft`.
    // Hue and saturation are the designer's; only lightness moved, by two or three points per
    // channel, which is the smallest change that clears AA.
    text_faint: 0x646a70,
    border: 0xdfddd6,
    border_strong: 0xc3c1b8,
    accent: 0x1f6f63,
    accent_hover: 0x17564d,
    success: 0x2f6b23,
    warning: 0x8a5d04,
    error: 0xa63a34,
    running: 0x2e6da5,
};

/// The same bench after dark. Blue-charcoal, off-white ink, teal held back.
pub const BENCH_NIGHT: Theme = Theme {
    background: 0x23262a,
    surface: 0x2a2e33,
    elevated: 0x333840,
    overlay: 0x383e46,
    accent_soft: 0x284840,
    text: 0xe3e5e2,
    text_muted: 0xaeb3b0,
    text_faint: 0xaab0ad,
    border: 0x383d42,
    border_strong: 0x495057,
    accent: 0x6fc3ae,
    accent_hover: 0x8fd6c4,
    success: 0x9cc96b,
    warning: 0xe3b95c,
    // Lightened from 0x e58f8b for the same reason as BENCH's two: 4.44:1 on `overlay` and
    // 4.13:1 on `accent_soft`, both below AA. An error message is the last text in the window
    // that should be hard to read.
    error: 0xe89a97,
    running: 0x85b8e8,
};

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

/// Perceptual lightness contrast, APCA 0.1.9 — the measure [`contrast`] cannot replace.
///
/// **Why a second measure exists here.** WCAG 2's ratio is a fixed formula applied to both
/// polarities, and APCA's own documentation is blunt about the consequence: it *"far overstates
/// contrast for dark colors to the point that 4.5:1 can be functionally unreadable"*, and
/// *"cannot be used for guidance designing dark mode"*. Reading performance differs between dark
/// text on light and light text on dark at the same ratio, so APCA models the two separately.
/// This app is a reading tool with a dark default; measuring it with the wrong instrument is how
/// §182 shipped an accent at Lc 47 — under even the floor for incidental text — while WCAG
/// reported a comfortable 6.1:1 (docs §183).
///
/// The scale is roughly 0–106 for dark-on-light and 0 to −108 for light-on-dark; the sign is the
/// polarity. [`lc`] takes the magnitude, which is what thresholds are quoted against:
///
/// | Lc | for |
/// |----|-----|
/// | 90 | preferred for columns of body text |
/// | 75 | minimum for columns of body text |
/// | 60 | minimum for content text that is not body text |
/// | 45 | minimum for headlines |
/// | 30 | absolute minimum for placeholder and disabled text |
///
/// `contrast` stays because WCAG 2 is what accessibility policy is written against, and because
/// the two disagree in *both* directions — the light half of Papa Nativa needed its faintest ink
/// pushed past what APCA alone asked in order to clear WCAG's 4.5 on the tinted row.
///
/// Gated to tests because that is honestly where it is used: nothing renders differently for it,
/// it exists so a palette cannot ship unreadable. The gate comes off the first time something at
/// runtime needs to ask.
#[cfg(test)]
pub fn apca(text: u32, background: u32) -> f64 {
    fn y(colour: u32) -> f64 {
        let channel = |shift: u32| {
            (((colour >> shift) & 0xff) as f64 / 255.).powf(2.4)
        };
        0.2126729 * channel(16) + 0.7151522 * channel(8) + 0.0721750 * channel(0)
    }
    // Soft black clamp: near-black surfaces flatten perceptually, and without this the algorithm
    // reports contrast that is not there.
    fn clamp(value: f64) -> f64 {
        if value > 0.022 {
            value
        } else {
            value + (0.022 - value).powf(1.414)
        }
    }

    let (text_y, background_y) = (clamp(y(text)), clamp(y(background)));
    if (background_y - text_y).abs() < 0.0005 {
        return 0.;
    }
    let raw = if background_y > text_y {
        // Dark text on a light ground.
        let sapc = (background_y.powf(0.56) - text_y.powf(0.57)) * 1.14;
        if sapc < 0.1 { 0. } else { sapc - 0.027 }
    } else {
        // Light text on a dark ground — different exponents, which is the whole point.
        let sapc = (background_y.powf(0.65) - text_y.powf(0.62)) * 1.14;
        if sapc > -0.1 { 0. } else { sapc + 0.027 }
    };
    raw * 100.
}

/// [`apca`] without the polarity sign, for comparing against a threshold.
#[cfg(test)]
pub fn lc(text: u32, background: u32) -> f64 {
    apca(text, background).abs()
}

macro_rules! live_theme {
    ($($field:ident => $getter:ident, $slot:ident;)*) => {
        $(
            // Seeded from [`DEFAULT`] rather than from a literal repeated here. The literals
            // were a second copy of one palette, and a copy of a palette is a palette that can
            // be half-changed — which is what "a fresh install opens on Bench" would have meant
            // if only some of these rows were updated.
            static $slot: AtomicU32 = AtomicU32::new(DEFAULT.$field);
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
    background   => background,   BACKGROUND_SLOT;
    surface      => surface,      SURFACE_SLOT;
    elevated     => elevated,     ELEVATED_SLOT;
    overlay      => overlay,      OVERLAY_SLOT;
    accent_soft  => accent_soft,  ACCENT_SOFT_SLOT;
    text         => text,         TEXT_SLOT;
    text_muted   => text_muted,   TEXT_MUTED_SLOT;
    text_faint   => text_faint,   TEXT_FAINT_SLOT;
    border       => border,       BORDER_SLOT;
    border_strong=> border_strong,BORDER_STRONG_SLOT;
    accent       => accent,       ACCENT_SLOT;
    accent_hover => accent_hover, ACCENT_HOVER_SLOT;
    success      => success,      SUCCESS_SLOT;
    warning      => warning,      WARNING_SLOT;
    error        => error,        ERROR_SLOT;
    running      => running,      RUNNING_SLOT;
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

// These live-palette tests sit at the boundary they protect; the importer below is a
// separate concern with its own fixtures. Keeping that narrative order is more useful
// than moving the entire test module to satisfy a source-order preference (docs §118).
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apca_agrees_with_its_own_published_reference_values() {
        // If this drifts, every threshold below is measuring something else. The published
        // extremes for APCA 0.1.9 are the cheapest possible check on the constants.
        assert!((apca(0xffffff, 0x000000) + 107.88).abs() < 0.05);
        assert!((apca(0x000000, 0xffffff) - 106.04).abs() < 0.05);
        // Polarity is the sign, and it is the whole reason this exists beside `contrast`.
        assert!(apca(0xffffff, 0x000000) < 0., "light on dark is negative");
        assert!(apca(0x000000, 0xffffff) > 0., "dark on light is positive");
        // Identical colours are no contrast rather than a divide-by-something.
        assert_eq!(apca(0x808080, 0x808080), 0.);
    }

    #[test]
    fn text_meant_for_reading_clears_the_apca_body_text_floor() {
        // APCA's levels: Lc 90 preferred for columns of body text, **Lc 75 the minimum**, Lc 60
        // for content text that is not body text. §183 measures against these because WCAG 2
        // "cannot be used for guidance designing dark mode" — its ratio far overstates contrast
        // for dark colours, which is how an accent at Lc 47 shipped reporting 6.1:1.
        for (name, theme) in THEMES {
            let body = lc(theme.text, theme.background);
            assert!(
                body >= 85.,
                "{name}: body text is Lc {body:.0}, under the level APCA prefers for columns \
                 of body text"
            );
        }

        // The accent is held to the *body-text* floor rather than the incidental-text one, and
        // only for these two, deliberately. The transcript renders filenames and column names in
        // `accent` — `hola_eda_correlation.csv`, `annual_income` — so in this app that colour is
        // something a researcher reads by the paragraph, not a button label glanced at.
        //
        // Not asserted for the six older palettes: Mini-Me Dark measures Lc 48 and Slate Lc 51,
        // which is the same defect and a bigger change than this one. Recorded rather than
        // silently exempted (docs §183).
        for (name, theme) in [
            ("Papa Nativa", PAPA_NATIVA),
            ("Papa Nativa Light", PAPA_NATIVA_LIGHT),
        ] {
            let reading = lc(theme.accent, theme.surface);
            assert!(
                reading >= 75.,
                "{name}: accent is Lc {reading:.0} on surface, under the body-text minimum — \
                 and this app sets filenames in it"
            );
        }
    }

    #[test]
    fn the_ink_a_whole_answer_is_set_in_stays_near_grey() {
        /// How far a colour is from grey: the gap between its strongest and weakest channel.
        ///
        /// Chroma in the most literal sense, and deliberately **not** HSL saturation. That
        /// measure explodes near white — it called the old cream `#f2ebdd` 45% saturated when
        /// its channels span 21 of 255 — so a threshold written against it would have to be
        /// different for light and dark themes to mean the same thing. The raw span does not.
        fn channel_spread(colour: u32) -> u32 {
            let (r, g, b) = (
                (colour >> 16) & 0xff,
                (colour >> 8) & 0xff,
                colour & 0xff,
            );
            r.max(g).max(b) - r.min(g).min(b)
        }

        // §182: Papa Nativa shipped its body text as cream, 21 apart, over a ground with a
        // violet cast — two hues on opposite sides of the wheel at 15:1 luminance contrast.
        // Reported as *"the letters and the background compete"*, which is what opposed chroma
        // does. Nothing else caught it: cream passes AA comfortably, and contrast is the only
        // thing the other tests measure.
        //
        // 16 is chosen against the shipped set, not picked round: the highest body text among
        // these eight is 13, and the cream that caused the report was 21.
        for (name, theme) in THEMES {
            let spread = channel_spread(theme.text);
            assert!(
                spread <= 16,
                "{name}: body text #{:06x} spans {spread} between channels — that is a colour, \
                 not an ink, and it will fight whatever ground it is set on",
                theme.text
            );
        }

        // Only `text`. `text_muted` and `text_faint` are timestamps, labels and counts — small,
        // sparse, and a tint there is part of how they read as a lesser role rather than as
        // dimmer body copy. Slate's faint ink spans 23 and is right to.
        assert!(channel_spread(SLATE.text_faint) > 16);
    }

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
    fn a_fresh_window_draws_the_default_before_settings_load() {
        // The window paints its first frame from these atomics, before `apply_theme` has read
        // `settings.toml` — so if the seeds and `THEMES[0]` disagreed, a fresh install would
        // flash one palette and settle on another. They cannot disagree now; this checks the
        // macro actually wires them, which a comment cannot.
        apply(&MINI_ME_DARK);
        for (name, theme) in THEMES {
            apply(&theme);
            assert_eq!(current(), theme, "{name} did not survive a round trip");
        }
        assert_eq!(DEFAULT, PAPA_NATIVA);
        assert_eq!(DEFAULT_NAME, "Papa Nativa");
        // What a fresh install writes must name a theme that exists, or it silently falls back.
        assert!(
            THEMES.iter().any(|(name, _)| *name == DEFAULT_NAME),
            "the default name is not in THEMES"
        );
        apply(&DEFAULT);
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
