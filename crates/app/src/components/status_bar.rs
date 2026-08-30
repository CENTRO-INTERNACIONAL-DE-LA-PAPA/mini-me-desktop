#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    /// The full-width bottom bar: status text, work summary, approval/restart chips, execution
    /// location, and the base URL.
    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), theme::error()),
            None => (self.status.clone(), theme::text_muted()),
        };

        div()
            .flex_none()
            .when(self.is_waiting(), |bar| bar.child(ui::Spinner::new("status-working")))
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::surface()))
            .child(ui::Label::new(status_text).colour(status_color).ellipsis())
            .children(self.work_summary().map(|summary| {
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(summary)
            }))
            .when(self.approve_conversation, |bar| {
                bar.child(
                    ui::Button::new("revoke-approval", "approving everything — click to stop")
                        .tone(ui::Tone::Accent)
                        .size(ui::Size::Chip)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_conversation = false;
                            workbench.approve_tasks.clear();
                            workbench.status = "asking before each command again".into();
                            cx.notify();
                        })),
                )
            })
            .children(self.restart_chip(cx))
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(if self.sidecar.execution() == "host (local)" {
                        theme::accent()
                    } else {
                        theme::text_muted()
                    }))
                    .text_xs()
                    .child(self.sidecar.execution()),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("ctrl-p commands"),
            )
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(self.sidecar.base_url().to_string()),
            )
    }

    /// The "Restart to Update" chip shown once an update is downloaded and waiting. `None` when
    /// there's nothing staged, or it was dismissed for this session.
    pub(crate) fn restart_chip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.update_dismissed {
            return None;
        }
        self.ready_update()?;
        Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    ui::Button::new("restart-to-update", "Restart to Update")
                        .tone(ui::Tone::Accent)
                        .size(ui::Size::Chip)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.restart_to_update(cx);
                        })),
                )
                .child(
                    ui::Button::new("dismiss-update", "×")
                        .size(ui::Size::Chip)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.update_dismissed = true;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }

    /// The stack of recent outcomes, floating above the status bar.
    pub(crate) fn toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack = div()
            .absolute()
            .right_4()
            .bottom_10()
            .flex()
            .flex_col()
            .gap_2()
            .items_end();
        for (index, toast) in self.toasts.iter().enumerate() {
            stack = stack.child(
                div()
                    .id(SharedString::from(format!("toast-{index}")))
                    .occlude()
                    .max_w(px(360.))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(theme::elevated()))
                    .border_1()
                    .border_color(rgb(theme::border_strong()))
                    .text_color(rgb(theme::text()))
                    .text_sm()
                    .hover(|style| style.cursor_pointer())
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if index < workbench.toasts.len() {
                            workbench.toasts.remove(index);
                        }
                        cx.notify();
                    }))
                    .child(toast.clone()),
            );
        }
        stack
    }
}
