//! The rail of sections a [`super::Modal::nav`] shows, and the rows inside it.

use gpui::{div, prelude::*, rgb, App, ClickEvent, Div, ElementId, IntoElement, SharedString, Window};

use super::{Icon, IconSize, OnClick};
use crate::theme;

/// One entry in a [`super::Modal::nav`] rail.
///
/// A row, not a [`super::Button`]: it is full width and marks a *chosen* state, which is the
/// same reason the provider pill and the settings toggle stayed hand-written.
#[derive(IntoElement)]
pub struct NavEntry {
    id: ElementId,
    label: SharedString,
    icon: Option<&'static str>,
    selected: bool,
    on_click: Option<OnClick>,
}

impl NavEntry {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            selected,
            on_click: None,
        }
    }

    /// An icon left of the label.
    pub fn icon(mut self, path: &'static str) -> Self {
        self.icon = Some(path);
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

impl RenderOnce for NavEntry {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        // Figma "Settings Modal": the chosen row is an `accent_soft()` fill with `accent()` text,
        // which stays put on hover; the others are muted and take `surface()` on hover.
        let selected = self.selected;
        let colour = if selected {
            theme::accent()
        } else {
            theme::text_muted()
        };
        let row = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .px_2p5()
            .py_1p5()
            .rounded_lg()
            // The app's base text size, as every other row and label uses.
            .text_sm()
            .text_color(rgb(colour))
            .when(self.selected, |row| row.bg(rgb(theme::accent_soft())))
            // One `hover` call only: gpui panics ("hover style already set") on a second, and on
            // Windows that panic aborts the app.
            .hover(move |style| {
                let style = style.cursor_pointer();
                if selected {
                    style
                } else {
                    style.bg(rgb(theme::surface()))
                }
            })
            .when_some(self.icon, |row, path| {
                row.child(Icon::new(path).size(IconSize::Medium).colour(colour))
            })
            .child(self.label);
        match self.on_click {
            Some(handler) => row.on_click(move |event, window, cx| handler(event, window, cx)),
            None => row,
        }
    }
}

/// The rail those entries sit in.
pub fn nav_rail() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w(gpui::px(150.))
        .gap_1()
        .p_2()
        .border_r_1()
        .border_color(rgb(theme::border()))
}
