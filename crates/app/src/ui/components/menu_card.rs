//! The card every popup menu is drawn on.

use gpui::{div, prelude::*, px, rgb, Div};

use crate::theme;

/// One definition because the discipline is easy to omit and invisible when it is: a menu must
/// `occlude`, or a click on a row also lands on whatever the menu was drawn over (§163), and it
/// must swallow the left press, or choosing an item starts a text selection in the transcript
/// underneath. The right-click menu learned both the hard way; a second menu written from scratch
/// beside it would have learned them again.
pub fn menu_card() -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(190.))
        .py_1()
        .rounded_md()
        .bg(rgb(theme::elevated()))
        .border_1()
        .border_color(rgb(theme::border_strong()))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, |_event, _window, cx| {
            cx.stop_propagation();
        })
}
