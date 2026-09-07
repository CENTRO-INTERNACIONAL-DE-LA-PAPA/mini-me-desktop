//! A visible scrollbar thumb for a vertically-scrolling container.
//!
//! Two entry points rather than one, because the app has two different things that scroll
//! vertically and neither exposes the other's shape: a plain `div` tracks its own
//! [`gpui::ScrollHandle`], but `list` (GPUI's virtualised list, used for the transcript) keeps its
//! offset in a [`gpui::ListState`] instead (docs §156). Both funnel into the same drawing, so the
//! only difference between [`scrollbar`] and [`list_scrollbar`] is which one they read from.

use gpui::{div, prelude::*, px, rgb, IntoElement, ListState, Pixels, ScrollHandle};

use crate::theme;
use crate::SCROLL_GROUP;

fn thumb(overflow: Pixels, viewport: Pixels, offset: Pixels) -> Option<impl IntoElement> {
    if overflow <= px(0.) || viewport <= px(0.) {
        return None;
    }
    let content = viewport + overflow;
    // Floored, so a very long transcript still leaves something big enough to see.
    let thumb = (viewport * (viewport / content)).max(px(28.));
    let travel = viewport - thumb;
    let progress = (-offset / overflow).clamp(0.0, 1.0);

    Some(
        div()
            .absolute()
            .invisible()
            .group_hover(SCROLL_GROUP, |style| style.visible())
            .top(travel * progress)
            .right(px(2.))
            .w(px(6.))
            .h(thumb)
            .rounded_full()
            .bg(rgb(theme::border_strong())),
    )
}

/// A visible scrollbar for a plain scrolling `div`.
pub fn scrollbar(handle: &ScrollHandle) -> Option<impl IntoElement> {
    let overflow = handle.max_offset().height;
    let viewport = handle.bounds().size.height;
    thumb(overflow, viewport, handle.offset().y)
}

/// A visible scrollbar for GPUI's variable-height list.
///
/// `list` stores its offset in [`ListState`], not a `ScrollHandle`. This keeps §40's visible
/// affordance without adding a second scroll container around the virtual list (docs §156).
pub fn list_scrollbar(state: &ListState) -> Option<impl IntoElement> {
    let overflow = state.max_offset_for_scrollbar().height;
    let viewport = state.viewport_bounds().size.height;
    thumb(overflow, viewport, state.scroll_px_offset_for_scrollbar().y)
}
