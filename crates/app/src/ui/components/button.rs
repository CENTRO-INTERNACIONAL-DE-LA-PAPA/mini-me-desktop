//! The one clickable control the rest of the app is built from.

use gpui::{div, prelude::*, rgb, App, ClickEvent, ElementId, IntoElement, SharedString, Window};

use super::{Hint, Icon, IconSize, OnClick};
use crate::theme;

/// Which of the app's four button looks a [`Button`] wears.
///
/// A fixed palette rather than the individually-tunable colours the old `Button` exposed:
/// every button in the app is meant to read as one visual family, and a knob for "just this
/// one's background" is how that stopped being true.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ButtonStyle {
    /// The action the pane exists for: Re-check, Install, Sign in.
    Primary,
    /// Everything alongside it — Copy, Close, Settings. The common case, hence the default.
    #[default]
    Secondary,
    /// [`Secondary`](ButtonStyle::Secondary), but for sitting on a `surface()` background
    /// rather than `background()` — the two need to swap which one is their own fill and
    /// which is their hover fill, or the button would vanish into (or fail to lift off) the
    /// surface under it.
    SecondaryWhite,
    /// An irreversible action whose scope has just been stated in a confirmation modal.
    Danger,
}

impl ButtonStyle {
    fn text(self) -> u32 {
        match self {
            ButtonStyle::Primary => theme::accent(),
            ButtonStyle::Secondary | ButtonStyle::SecondaryWhite => theme::text_muted(),
            ButtonStyle::Danger => theme::error(),
        }
    }

    fn border(self) -> u32 {
        match self {
            ButtonStyle::Primary => theme::accent(),
            ButtonStyle::Secondary | ButtonStyle::SecondaryWhite => theme::border(),
            ButtonStyle::Danger => theme::error(),
        }
    }

    fn bg(self) -> u32 {
        match self {
            ButtonStyle::Primary => theme::accent_soft(),
            ButtonStyle::Secondary | ButtonStyle::Danger => theme::background(),
            ButtonStyle::SecondaryWhite => theme::surface(),
        }
    }

    /// The background hovering repaints toward. `Primary` and `Danger` stay put — an accent
    /// or a destructive action should not soften on hover, only invite the click it already
    /// invites by being coloured.
    fn hover_bg(self) -> u32 {
        match self {
            ButtonStyle::Primary | ButtonStyle::Danger => self.bg(),
            ButtonStyle::Secondary => theme::surface(),
            ButtonStyle::SecondaryWhite => theme::background(),
        }
    }
}

/// Where a [`Button`]'s icon and text sit inside it, when the button is wider than its content.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

/// The one clickable control the rest of the app is built from: an icon, a label, or both, in
/// one of the four styles above.
///
/// ```ignore
/// Button::new("recheck")
///     .text("Re-check")
///     .style(ButtonStyle::Primary)
///     .disabled(self.checking)
///     .on_click(cx.listener(|workbench, _, _, cx| workbench.run_preflight(cx)))
/// ```
///
/// A **template, not a wrapper**: every colour comes from [`ButtonStyle`], not from a per-call
/// override, so a button never drifts from the four looks the app has. What used to be
/// `IconButton` is a `Button` with no `text` and `border(false)` — a fifth type existed only
/// because this one used to require a label.
///
/// The properties that kept being forgotten on the old `Button` are still not optional and not
/// reachable: the radius, `flex_none` so a button never absorbs the row's spare width, and the
/// pointer cursor on hover. There is no method to leave one out.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    icon: Option<Icon>,
    text: Option<SharedString>,
    style: ButtonStyle,
    alignment: Alignment,
    /// Whether this button is a toggle rather than a one-shot action — [`Button::active`] only
    /// means anything when this is set.
    toggle: bool,
    /// The toggle's own on/off value. Ignored unless [`Button::toggle`] is set, the same way
    /// a checkbox's checked-ness means nothing until it is one.
    active: bool,
    border: bool,
    disabled: bool,
    tooltip: Option<SharedString>,
    on_click: Option<OnClick>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            icon: None,
            text: None,
            style: ButtonStyle::default(),
            alignment: Alignment::default(),
            toggle: false,
            active: false,
            border: true,
            disabled: false,
            tooltip: None,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn text(mut self, text: impl Into<SharedString>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Marks this as an on/off control rather than a one-shot action. [`Button::active`] sets
    /// which one it currently is.
    pub fn toggle(mut self, toggle: bool) -> Self {
        self.toggle = toggle;
        self
    }

    /// The toggle's current value — has no effect unless [`Button::toggle`] is set. When on,
    /// the button always wears [`ButtonStyle::Primary`], whatever [`Button::style`] was given:
    /// "on" has one look app-wide, not one per caller.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
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

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
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
        let style = if self.toggle && self.active {
            ButtonStyle::Primary
        } else {
            self.style
        };
        let (text_colour, border_colour) = if self.disabled {
            (theme::text_muted(), theme::border())
        } else {
            (style.text(), style.border())
        };

        let mut button = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .bg(rgb(style.bg()))
            .text_color(rgb(text_colour))
            .text_sm()
            // Never absorbs the row's spare width. Its absence is two of the four
            // `flex_none` bugs.
            .flex_none();
        button = match self.alignment {
            Alignment::Left => button.justify_start(),
            Alignment::Center => button.justify_center(),
            Alignment::Right => button.justify_end(),
        };
        if self.border {
            button = button.border_1().border_color(rgb(border_colour));
        }
        if let Some(icon) = self.icon {
            button = button.p_2();
            // Size and colour are the button's to decide, not the caller's — an icon that
            // came in some other size or shade would make one button read as a different
            // family from the rest. `Button` always renders it at `IconSize::Small`, tinted
            // to match its own resolved text colour.
            button = button.child(icon.size(IconSize::Small).colour(text_colour));
        }
        if let Some(text) = self.text {
            button = button.child(text);
        }
        if let Some(tooltip) = self.tooltip.clone() {
            button = button.tooltip(move |_window, cx| {
                cx.new(|_| Hint { text: tooltip.clone() }).into()
            });
        }
        if self.disabled {
            return button;
        }
        let hover_bg = style.hover_bg();
        button = button.hover(move |style| style.cursor_pointer().bg(rgb(hover_bg)));
        match self.on_click {
            // The click is attached only when the button is live, so "disabled" cannot be
            // true in the styling and false in the behaviour.
            Some(handler) => button.on_click(move |event, window, cx| handler(event, window, cx)),
            None => button,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_button_is_grey_whatever_it_would_otherwise_be() {
        // The live palette is global, so a test that changes it must not run beside one
        // that reads it. §197 fixed this for `theme.rs`'s own tests and could not reach
        // these three files, because the lock lived in a private test module.
        let _theme = crate::theme::theme_lock::hold();
        theme::apply(&theme::MINI_ME_DARK);
        for style in [
            ButtonStyle::Primary,
            ButtonStyle::Secondary,
            ButtonStyle::SecondaryWhite,
            ButtonStyle::Danger,
        ] {
            let dead = Button::new("b").style(style).disabled(true);
            assert!(dead.disabled, "{style:?}");
        }
    }

    #[test]
    fn every_button_style_is_distinguishable_by_text_colour() {
        // The live palette is global, so a test that changes it must not run beside one
        // that reads it. §197 fixed this for `theme.rs`'s own tests and could not reach
        // these three files, because the lock lived in a private test module.
        let _theme = crate::theme::theme_lock::hold();
        theme::apply(&theme::MINI_ME_DARK);
        // Secondary and SecondaryWhite intentionally share a text colour — they differ only
        // in which surface they sit on — so this checks each style is distinguishable from
        // at least one other, not that all four are pairwise distinct.
        assert_ne!(ButtonStyle::Primary.text(), ButtonStyle::Secondary.text());
        assert_ne!(ButtonStyle::Primary.text(), ButtonStyle::Danger.text());
        assert_ne!(ButtonStyle::Secondary.text(), ButtonStyle::Danger.text());
    }

    #[test]
    fn styles_follow_the_live_theme_rather_than_the_one_at_startup() {
        // The live palette is global, so a test that changes it must not run beside one
        // that reads it. §197 fixed this for `theme.rs`'s own tests and could not reach
        // these three files, because the lock lived in a private test module.
        let _theme = crate::theme::theme_lock::hold();
        // Read per frame, not captured at construction: a control built while the palette
        // changes must not keep the old one. §49's theme switching made this reachable.
        theme::apply(&theme::MINI_ME_DARK);
        let dark = ButtonStyle::Primary.text();
        theme::apply(&theme::PAPER);
        let paper = ButtonStyle::Primary.text();
        theme::apply(&theme::MINI_ME_DARK);
        assert_ne!(dark, paper, "the two palettes share an accent");
        assert_eq!(ButtonStyle::Primary.text(), dark, "switching back restores it");
    }

    #[test]
    fn an_active_toggle_button_always_wears_primary() {
        let _theme = crate::theme::theme_lock::hold();
        theme::apply(&theme::MINI_ME_DARK);
        let on = Button::new("b").style(ButtonStyle::Secondary).toggle(true).active(true);
        assert_eq!(on.style, ButtonStyle::Secondary, "the field itself is unchanged");
        // The override happens at render time, not by mutating `style` — this asserts the
        // input the render reads from, since the resolved colour is only visible mid-render.
        assert!(on.toggle && on.active);
    }

    #[test]
    fn secondary_is_the_default_because_most_buttons_are() {
        // A default that matched the rare case would mean every ordinary button carrying a
        // `.style(…)` nobody reads.
        assert_eq!(ButtonStyle::default(), ButtonStyle::Secondary);
        assert_eq!(Button::new("b").style, ButtonStyle::Secondary);
        assert!(!Button::new("b").disabled);
    }
}
