//! Text that behaves in a flex row.
//!
//! Two rules, both learned the hard way:
//!
//! - **`min_w_0` always.** A flex item's min-width defaults to its content, so a long line
//!   widens its container instead of wrapping — which is how one long assistant paragraph
//!   used to push the whole chat pane sideways.
//! - **`.truncate()` only ever on a box that can grow.** Applied to the flex item itself,
//!   together with `min_w_0`, it gives the element zero intrinsic width and the ellipsis is
//!   all that survives: every model in the picker rendered as `…` (§59). So [`Label::ellipsis`]
//!   produces `flex_grow().min_w_0().truncate()`, and there is no way to ask for the broken
//!   combination.

use gpui::{div, prelude::*, rgb, App, IntoElement, SharedString, Window};

use crate::theme;

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

#[derive(IntoElement)]
pub struct Label {
    text: SharedString,
    colour: u32,
    /// When false the label paints no colour of its own and takes the parent's.
    ///
    /// **Necessary for any state a parent expresses**, hover most of all: a colour written onto
    /// the element wins over a parent's refinement, so a label that names its own can never
    /// change with the row it sits in (docs §189).
    owns_colour: bool,
    size: Size,
    ellipsis: bool,
}

impl Label {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            colour: theme::text(),
            owns_colour: true,
            size: Size::Regular,
            ellipsis: false,
        }
    }

    /// Take whatever colour the parent is painting, so the parent can change it per state.
    pub fn inherit(mut self) -> Self {
        self.owns_colour = false;
        self
    }

    pub fn colour(mut self, colour: u32) -> Self {
        self.colour = colour;
        self.owns_colour = true;
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
        let text = div().min_w_0();
        // No `text_color` at all when inheriting — setting one, even the parent's current value,
        // would pin it and defeat the point.
        let text = if self.owns_colour {
            text.text_color(rgb(self.colour))
        } else {
            text
        };
        let text = match self.size {
            Size::Regular => text.text_sm(),
            Size::Compact | Size::Chip => text.text_xs(),
        };
        if self.ellipsis {
            // **`w_full` *and* `flex_grow`.** `flex_grow` alone gives a width only in a *row*;
            // in a column it grows the height and the width stays at content — which is how
            // every model name in the specialist picker rendered as a bare "…" even after
            // `items_start` was removed. `w_full` supplies the width in both, and `flex_grow`
            // still does §59's job where the parent is a row (docs §192).
            text.w_full().flex_grow().truncate().child(self.text)
        } else {
            text.w_full().child(self.text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn regular_is_the_default_size() {
        assert_eq!(Size::default(), Size::Regular);
    }
}
