//! The button that opens a picker, and the floating popup it opens.

use gpui::{div, prelude::*, rgb, App, ClickEvent, ElementId, IntoElement, SharedString, Window};

use super::{Label, OnClick};
use crate::theme;

/// The button that opens a picker: what is chosen now, and a chevron.
///
/// Zed's settings put every choice behind one of these rather than an always-open list, and
/// the reason shows up as soon as there is more than one: an inline list of a hundred themes
/// is a hundred rows of a window that has four other settings in it. The list itself does not
/// change — it moves into a popup and gains a trigger.
#[derive(IntoElement)]
pub struct Dropdown {
    id: ElementId,
    value: SharedString,
    open: bool,
    on_click: Option<OnClick>,
}

impl Dropdown {
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            open: false,
            on_click: None,
        }
    }

    /// Marks the trigger while its popup is showing, so it is obvious which one is open.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
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

impl RenderOnce for Dropdown {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let trigger = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .flex_none()
            .min_w(gpui::px(150.))
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(if self.open {
                theme::accent()
            } else {
                theme::border_strong()
            }))
            .text_color(rgb(theme::text()))
            .text_sm()
            .hover(|style| style.cursor_pointer())
            .child(Label::new(self.value).ellipsis())
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .child("⌄"),
            );
        match self.on_click {
            Some(handler) => trigger.on_click(move |event, window, cx| handler(event, window, cx)),
            None => trigger,
        }
    }
}

/// The floating panel a [`Dropdown`] opens.
///
/// Anchored at the click and painted after everything else, the same two elements the
/// right-click menu is built from (§64) — `anchored` keeps it inside the window, `deferred`
/// keeps the pane it opened over from clipping it.
pub fn picker_popup(at: gpui::Point<gpui::Pixels>, panel: impl IntoElement) -> impl IntoElement {
    gpui::deferred(
        gpui::anchored().position(at).snap_to_window().child(
            div()
                .flex()
                .flex_col()
                .w(gpui::px(320.))
                // A declared width is not a promise on its own: a flex child's `min-width: auto`
                // lets its intrinsic content override it, and one long unbreakable path inside
                // the theme picker widened this panel to nearly 400px (docs §86). These two make
                // the number mean what it says, whatever a future caller puts inside.
                .min_w_0()
                .overflow_hidden()
                .gap_2()
                .p_2()
                .rounded_md()
                .bg(rgb(theme::elevated()))
                .border_1()
                .border_color(rgb(theme::border_strong()))
                .child(panel),
        ),
    )
}
