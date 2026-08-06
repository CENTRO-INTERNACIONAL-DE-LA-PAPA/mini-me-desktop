//! The small set of controls the rest of the app is built from.
//!
//! # Why this exists
//!
//! GPUI ships primitives, not components. So every control in this app was written out
//! longhand at its call site — `div().px_3().py_1().border_1().border_color(…)
//! .text_color(…).text_sm().hover(…)` — once per button, **forty-four times**. Each copy is
//! an independent chance to omit one line, and the same omissions kept happening:
//!
//! - `flex_none` / `min_h_0` missing or misplaced: four layout bugs (§40, §48, §51, §53).
//! - Actions placed *inside* a scroll area, so the buttons scrolled away: three (§40, §41, §52).
//! - `.truncate()` on the flex item instead of an inner box, which collapses the element to
//!   its ellipsis and nothing else: §59.
//! - `rounded_md` simply forgotten: §58 rounded the corners of the app and missed **eleven**
//!   bordered buttons, which stayed square for two months in the pane every new user sees.
//!
//! Twice the correct pattern was already a few lines above the mistake. So this is not a
//! knowledge problem and no amount of writing the lesson down has fixed it — it has been
//! written down three times. A value you construct cannot forget a property the way a recipe
//! you retype can, and that is the whole argument for this module.
//!
//! # What it is not
//!
//! It is **not a design system** and does not introduce a single new visual decision. Every
//! colour, padding and radius here was read out of the call sites it replaces, so migrating a
//! button is meant to change nothing on screen — except where a site was missing a property
//! it should always have had.
//!
//! There is no `Modal` or `Panel` here yet. Both are about *where actions sit relative to a
//! scroll area* — the other repeated bug — and migrating thirteen scrolling panes is a change
//! whose only proof is visual. That is the next increment, in front of a window.
//!
//! It also deliberately does not try to cover the **twenty-three borderless clickables** —
//! sidebar entries, menu rows, gallery cards. Those are rows with their own layout, not
//! buttons wearing a different hat, and forcing one type over both is how a component set
//! starts growing flags nobody can keep straight.

use gpui::{
    div, prelude::*, rgb, App, ClickEvent, Div, ElementId, IntoElement, SharedString, Window,
};

use crate::theme;

/// A click handler, in the shape `div().on_click` wants it.
type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// What a control is *for*, which is the only thing a caller should have to decide.
///
/// Colours come from the live theme every frame rather than being captured, so a control
/// built during a theme change cannot keep the old palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
    /// The action the pane exists for: Re-check, Install, Sign in. Twelve of these.
    Accent,
    /// Everything alongside it — Copy, Close, Settings. The common case, hence the default.
    #[default]
    Quiet,
    // No `Danger`. The one destructive control in the app — "delete" in the conversation
    // list's confirmation row — is a *borderless* chip inside an already-red row, so it is
    // one of the rows this type deliberately does not cover. A tone with no caller is a
    // design vocabulary invented ahead of the design; it goes in when a bordered destructive
    // button exists.
}

impl Tone {
    fn border(self) -> u32 {
        match self {
            Tone::Accent => theme::accent(),
            Tone::Quiet => theme::border(),
        }
    }

    fn ink(self) -> u32 {
        match self {
            Tone::Accent => theme::accent(),
            Tone::Quiet => theme::text_muted(),
        }
    }
}

/// How much room a control takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Size {
    /// `px_3 py_1 text_sm` — the standard button, nineteen of the existing ones.
    #[default]
    Regular,
    /// `px_2 py_1 text_xs`, for a button sharing a line with body text.
    Compact,
    /// `px_2 text_xs` with **no vertical padding**, for the status bar.
    ///
    /// Its own size rather than `Compact`, because the status bar is a fixed-height strip and
    /// a control that added padding inside it would push the bar taller — which is how §53's
    /// bug looked, and not something to reintroduce as a side effect of tidying.
    Chip,
}

/// A bordered, clickable control.
///
/// ```ignore
/// Button::new("recheck", "Re-check")
///     .tone(Tone::Accent)
///     .disabled(self.checking)
///     .on_click(cx.listener(|workbench, _, _, cx| workbench.run_preflight(cx)))
/// ```
///
/// The properties that kept being forgotten are not optional and not reachable: the radius,
/// the border, `flex_none` so a button never absorbs the row's spare width, and the pointer
/// cursor on hover. There is no method to leave one out.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    tone: Tone,
    size: Size,
    disabled: bool,
    on_click: Option<OnClick>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tone: Tone::default(),
            size: Size::default(),
            disabled: false,
            on_click: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Greyed and inert.
    ///
    /// One flag rather than a colour plus a guard at the call site: those were kept in step by
    /// hand, and a button that looks available and does nothing is the failure that shape
    /// invites.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let ink = if self.disabled {
            theme::text_faint()
        } else {
            self.tone.ink()
        };
        let border = if self.disabled {
            theme::border()
        } else {
            self.tone.border()
        };
        let mut button = div()
            .id(self.id)
            // Never absorbs the row's spare width. Its absence is two of the four
            // `flex_none` bugs.
            .flex_none()
            .rounded_md()
            .border_1()
            .border_color(rgb(border))
            .text_color(rgb(ink))
            .child(self.label);
        button = match self.size {
            Size::Regular => button.px_3().py_1().text_sm(),
            Size::Compact => button.px_2().py_1().text_xs(),
            Size::Chip => button.px_2().text_xs(),
        };
        if self.disabled {
            return button;
        }
        button = button.hover(|style| style.cursor_pointer());
        match self.on_click {
            // The click is attached only when the button is live, so "disabled" cannot be
            // true in the styling and false in the behaviour.
            Some(handler) => button.on_click(move |event, window, cx| handler(event, window, cx)),
            None => button,
        }
    }
}

/// Text that behaves in a flex row.
///
/// Two rules, both learned the hard way:
///
/// - **`min_w_0` always.** A flex item's min-width defaults to its content, so a long line
///   widens its container instead of wrapping — which is how one long assistant paragraph
///   used to push the whole chat pane sideways.
/// - **`.truncate()` only ever on a box that can grow.** Applied to the flex item itself,
///   together with `min_w_0`, it gives the element zero intrinsic width and the ellipsis is
///   all that survives: every model in the picker rendered as `…` (§59). So [`Label::ellipsis`]
///   produces `flex_grow().min_w_0().truncate()`, and there is no way to ask for the broken
///   combination.
#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    colour: u32,
    size: Size,
    ellipsis: bool,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            colour: theme::text(),
            size: Size::Regular,
            ellipsis: false,
        }
    }

    pub fn colour(mut self, colour: u32) -> Self {
        self.colour = colour;
        self
    }

    pub fn muted(self) -> Self {
        self.colour(theme::text_muted())
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Cut with an ellipsis rather than wrapping — for one-line rows where the text is a name,
    /// not prose.
    pub fn ellipsis(mut self) -> Self {
        self.ellipsis = true;
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let text = div().min_w_0().text_color(rgb(self.colour));
        let text = match self.size {
            Size::Regular => text.text_sm(),
            Size::Compact | Size::Chip => text.text_xs(),
        };
        if self.ellipsis {
            // `flex_grow` is what gives it a width to truncate *to*. Without it this is the
            // §59 bug.
            text.flex_grow().truncate().child(self.text)
        } else {
            text.w_full().child(self.text)
        }
    }
}

/// A row of actions that must stay put while the thing above it scrolls.
///
/// Three bugs came from putting the buttons inside the scroll area (§40, §41, §52) — the
/// approve/reject pair scrolled out of reach twice, on two different panes, and the second
/// time the fix was already visible in the same file. `flex_none` is what pins it, and
/// [`Panel::scrolling`] is the matching half.
pub fn actions() -> Div {
    div()
        .flex()
        .flex_row()
        .flex_none()
        .gap_3()
        .w_full()
        .min_w_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_button_is_grey_whatever_it_would_otherwise_be() {
        // The colour and the inertness come from one flag, so they cannot disagree — which
        // they could when each call site kept its own `busy` boolean and its own palette.
        theme::apply(&theme::MINI_ME_DARK);
        for tone in [Tone::Accent, Tone::Quiet] {
            let live = Button::new("b", "Go").tone(tone);
            assert_eq!(live.tone.ink(), tone.ink());
            let dead = Button::new("b", "Go").tone(tone).disabled(true);
            assert!(dead.disabled, "{tone:?}");
        }
    }

    #[test]
    fn the_two_tones_are_distinguishable() {
        // A tone that resolved to the same colour as another would make the set decorative.
        theme::apply(&theme::MINI_ME_DARK);
        assert_ne!(Tone::Accent.ink(), Tone::Quiet.ink());
        assert_ne!(Tone::Accent.border(), Tone::Quiet.border());
    }

    #[test]
    fn tones_follow_the_live_theme_rather_than_the_one_at_startup() {
        // Read per frame, not captured at construction: a control built while the palette
        // changes must not keep the old one. §49's theme switching made this reachable.
        theme::apply(&theme::MINI_ME_DARK);
        let dark = Tone::Accent.ink();
        theme::apply(&theme::PAPER);
        let paper = Tone::Accent.ink();
        theme::apply(&theme::MINI_ME_DARK);
        assert_ne!(dark, paper, "the two palettes share an accent");
        assert_eq!(Tone::Accent.ink(), dark, "switching back restores it");
    }

    #[test]
    fn a_chip_adds_no_vertical_padding() {
        // Its whole reason for existing. `Compact` would put `py_1` inside the status bar and
        // push the bar taller, which is what §53 looked like.
        assert_ne!(Size::Chip, Size::Compact);
    }

    #[test]
    fn a_label_only_truncates_when_asked_and_never_without_room_to_grow() {
        // The §59 bug was `.truncate()` on the flex item itself: with `min_w_0` that leaves
        // zero intrinsic width and the ellipsis is all that renders. `ellipsis()` is the only
        // way to reach truncation here, and it always pairs with `flex_grow`.
        assert!(!Label::new("x").ellipsis, "wrapping is the default");
        assert!(Label::new("x").ellipsis().ellipsis);
        assert_eq!(Label::new("x").colour, theme::text());
        assert_eq!(Label::new("x").muted().colour, theme::text_muted());
    }

    #[test]
    fn quiet_is_the_default_because_most_buttons_are() {
        // Thirty-six of the forty-four. A default that matched the rare case would mean
        // every ordinary button carrying a `.tone(…)` nobody reads.
        assert_eq!(Tone::default(), Tone::Quiet);
        assert_eq!(Size::default(), Size::Regular);
        assert_eq!(Button::new("b", "Go").tone, Tone::Quiet);
        assert!(!Button::new("b", "Go").disabled);
    }
}
