//! An on/off switch, and the settings row it sits at the end of.

use gpui::{div, prelude::*, rgb, App, ClickEvent, Div, ElementId, IntoElement, SharedString, Window};

use super::{Label, OnClick, Size};
use crate::theme;

/// A settings row: what it is on the left, the control that changes it on the right.
///
/// Zed's shape — `h_flex().justify_between()` with a title-and-description stack on one side
/// and the control on the other — read off `settings_ui.rs` rather than guessed from a
/// screenshot. The description matters: half of these settings are things a researcher has no
/// reason to have an opinion about until someone says what they do.
pub fn setting_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .w_full()
        .min_w_0()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme::border()))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .min_w_0()
                .gap_1()
                .child(Label::new(title))
                .child(Label::new(description).muted().size(Size::Compact)),
        )
        .child(div().flex_none().child(control))
}

/// An on/off switch: a track that fills when on, and a knob that slides to the end.
///
/// A switch rather than the `☑`/`☐` row it replaces. The row was one element and read the
/// same way, which was true — but it meant a setting's *name* and its *state* were the same
/// piece of text, so nothing could say what the setting did without making the line longer.
/// Split into [`setting_row`] plus this, the description finally has somewhere to live.
///
/// Built from two flex boxes rather than absolute positioning: `justify_end` is what moves the
/// knob, so there is no arithmetic to get wrong at a size nobody re-measures.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    on: bool,
    on_click: Option<OnClick>,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
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

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let track = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .w(gpui::px(34.))
            .h(gpui::px(18.))
            .px(gpui::px(2.))
            .rounded_full()
            .border_1()
            .border_color(rgb(if self.on {
                theme::accent()
            } else {
                theme::border_strong()
            }))
            .bg(rgb(if self.on {
                theme::accent_soft()
            } else {
                theme::surface()
            }))
            .hover(|style| style.cursor_pointer())
            .when(self.on, |track| track.justify_end())
            .child(div().size(gpui::px(12.)).rounded_full().bg(rgb(if self.on {
                theme::accent()
            } else {
                theme::text_faint()
            })));
        match self.on_click {
            Some(handler) => track.on_click(move |event, window, cx| handler(event, window, cx)),
            None => track,
        }
    }
}
