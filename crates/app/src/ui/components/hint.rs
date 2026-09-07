//! A one-line tooltip.

use gpui::{div, prelude::*, rgb, Context, IntoElement, SharedString, Window};

use crate::theme;

/// GPUI wants a whole view for a tooltip, so this is the smallest one that renders text —
/// and having it means a control can be an icon without becoming a guess.
pub struct Hint {
    pub text: SharedString,
}

impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::overlay()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            .text_color(rgb(theme::text()))
            .text_xs()
            .child(self.text.clone())
    }
}
