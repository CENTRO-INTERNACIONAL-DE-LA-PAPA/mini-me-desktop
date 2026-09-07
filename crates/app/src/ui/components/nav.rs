//! The rail of sections a [`super::Modal::nav`] shows, and the rows inside it.

use gpui::{div, prelude::*, rgb, App, ClickEvent, Div, ElementId, IntoElement, SharedString, Window};

use super::OnClick;
use crate::theme;

/// One entry in a [`super::Modal::nav`] rail.
///
/// A row, not a [`super::Button`]: it is full width and marks a *chosen* state, which is the
/// same reason the provider pill and the settings toggle stayed hand-written.
#[derive(IntoElement)]
pub struct NavEntry {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<OnClick>,
}

impl NavEntry {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected,
            on_click: None,
        }
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
        let row = div()
            .id(self.id)
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .rounded_md()
            .text_sm()
            .text_color(rgb(if self.selected {
                theme::text()
            } else {
                theme::text_muted()
            }))
            .when(self.selected, |row| row.bg(rgb(theme::elevated())))
            .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
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
