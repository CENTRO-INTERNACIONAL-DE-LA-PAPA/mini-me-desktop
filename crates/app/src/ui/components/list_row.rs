//! A selectable, single-line row — the shape used down the conversation list, minus the copy
//! that differed from one row to the next only in role, colours and what trailed it.

use gpui::{div, prelude::*, rgb, App, ClickEvent, ElementId, IntoElement, SharedString, Window};

use super::{Label, OnClick};
use crate::theme;

#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    label: SharedString,
    label_color: u32,
    bg: Option<u32>,
    hover_bg: u32,
    hover_text: u32,
    hover_border: u32,
    trailing: Option<gpui::AnyElement>,
    on_click: Option<OnClick>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            label_color: theme::text_muted(),
            bg: None,
            hover_bg: theme::accent_soft(),
            hover_text: theme::accent(),
            hover_border: theme::accent(),
            trailing: None,
            on_click: None,
        }
    }

    /// The row this enquiry is currently open on: filled and its label recoloured, same as
    /// hovering it — a selected row is "as if the pointer never left".
    pub fn selected(mut self, selected: bool) -> Self {
        if selected {
            self.bg = Some(theme::accent_soft());
            self.label_color = theme::accent();
        }
        self
    }

    pub fn label_color(mut self, label_color: u32) -> Self {
        self.label_color = label_color;
        self
    }

    pub fn bg(mut self, bg: u32) -> Self {
        self.bg = Some(bg);
        self
    }

    pub fn hover_bg(mut self, hover_bg: u32) -> Self {
        self.hover_bg = hover_bg;
        self
    }

    pub fn hover_text(mut self, hover_text: u32) -> Self {
        self.hover_text = hover_text;
        self
    }

    pub fn hover_border(mut self, hover_border: u32) -> Self {
        self.hover_border = hover_border;
        self
    }

    /// A row's own menu button, or anything else that rides along after the label.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
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

impl RenderOnce for ListRow {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut row = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .w_full()
            .min_w_0()
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .text_sm();
        if let Some(bg) = self.bg {
            row = row.bg(rgb(bg));
        }
        let hover_bg = self.hover_bg;
        let hover_text = self.hover_text;
        let hover_border = self.hover_border;
        row = row.hover(move |style| {
            style
                .bg(rgb(hover_bg))
                .text_color(rgb(hover_text))
                .border_1()
                .border_color(rgb(hover_border))
                .cursor_pointer()
        });
        row = row.child(Label::new(self.label).colour(self.label_color).ellipsis());
        row = row.children(self.trailing);
        match self.on_click {
            Some(handler) => row.on_click(move |event, window, cx| handler(event, window, cx)),
            None => row,
        }
    }
}
