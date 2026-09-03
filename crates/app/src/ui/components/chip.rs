//! A bordered pill: a label the pointer can act on, filled and recoloured on hover rather than
//! merely showing a cursor — the "attached file" / "open what a specialist produced" idiom that
//! was hand-copied at both call sites down to the same `theme::hover_over` line.

use gpui::{div, prelude::*, rgb, App, ClickEvent, ElementId, IntoElement, SharedString, Window};

use super::{Label, OnClick, Size};
use crate::theme;

#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    ink: u32,
    border: u32,
    bg: Option<u32>,
    hover_base: u32,
    removable: bool,
    on_click: Option<OnClick>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ink: theme::text_muted(),
            border: theme::border(),
            bg: None,
            hover_base: theme::surface(),
            removable: false,
            on_click: None,
        }
    }

    pub fn ink(mut self, ink: u32) -> Self {
        self.ink = ink;
        self
    }

    pub fn border(mut self, border: u32) -> Self {
        self.border = border;
        self
    }

    pub fn bg(mut self, bg: u32) -> Self {
        self.bg = Some(bg);
        self
    }

    /// The surface hovering repaints toward — `theme::hover_over` computes the actual fill,
    /// this only picks which base colour that lift is measured from.
    pub fn hover_base(mut self, hover_base: u32) -> Self {
        self.hover_base = hover_base;
        self
    }

    /// Adds the trailing "×" that marks a chip as dismissible. The whole chip stays the click
    /// target — §225a's rule — this only draws the hint that it can be removed.
    pub fn removable(mut self, removable: bool) -> Self {
        self.removable = removable;
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

impl RenderOnce for Chip {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut chip = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .flex_none()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(self.border))
            .text_color(rgb(self.ink))
            .text_xs();
        if let Some(bg) = self.bg {
            chip = chip.bg(rgb(bg));
        }
        let hover_base = self.hover_base;
        chip = chip.hover(move |style| {
            let fill = theme::hover_over(hover_base);
            style
                .bg(rgb(fill))
                .text_color(rgb(theme::ink_on(fill)))
                .cursor_pointer()
        });
        chip = chip.child(Label::new(self.label).inherit().size(Size::Compact).ellipsis());
        if self.removable {
            chip = chip.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .child("×"),
            );
        }
        match self.on_click {
            Some(handler) => chip.on_click(move |event, window, cx| handler(event, window, cx)),
            None => chip,
        }
    }
}
