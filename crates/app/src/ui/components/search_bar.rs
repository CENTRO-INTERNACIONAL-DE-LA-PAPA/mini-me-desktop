//! A search field: a magnifying-glass icon beside a live, editable query.
//!
//! The same bordered, focus-ringed box `Workbench::filter_field` wraps every filter composer
//! in — `track_focus` on the child's own handle, `in_focus` to light the border, the flex-row
//! shape `filter_field`'s doc comment explains is not optional (a `div` is block-level by
//! default, so the field inside has no row to fill without it). This is that same shape with
//! the icon that marks a field as a *search* specifically, rather than one filter among several
//! in a list.

use gpui::{div, prelude::*, rgb, App, Entity, Focusable, IntoElement, Window};

use super::{Icon, IconSize};
use crate::composer::Composer;
use crate::theme;

#[derive(IntoElement)]
pub struct SearchBar {
    field: Entity<Composer>,
}

impl SearchBar {
    pub fn new(field: Entity<Composer>) -> Self {
        Self {
            field,
        }
    }
}

impl RenderOnce for SearchBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bar = div()
            .track_focus(&self.field.focus_handle(cx))
            // Never absorbs the sidebar column's spare vertical space — the same `flex_none`
            // every other control here carries so its own padding is the only thing that
            // decides its size.
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .text_sm()
            .text_color(rgb(theme::text_muted()))
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .in_focus(|style| style.border_color(rgb(theme::accent())));
        bar.child(
            Icon::new("icons/magnifying-glass.svg")
                .size(IconSize::Small)
                .colour(theme::text_muted()),
        )
        .child(self.field)
    }
}
