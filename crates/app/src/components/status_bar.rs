#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    /// The stack of recent outcomes, above the status bar.
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
                    // A toast floats over the composer and the transcript, and dismissing one used
                    // to press whatever it was covering as well (docs §163). Only the card
                    // occludes, not the stack: the gaps between toasts are not the toast's, and
                    // blocking them would put a dead strip across the window.
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
                    // Clicking one dismisses it: four seconds is right for a glance and wrong
                    // for a message you have already read.
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


impl Workbench {
    /// The one control an update needs, or the sentence that replaces it.
    ///
    /// Returns nothing at all when there is nothing to offer — an up-to-date app shows no button,
    /// because a button that does nothing is a question the researcher has to answer every time
    /// they open this page.
    pub(crate) fn update_action(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        // Named in words, per §199: "Download 0.4.0" says what the press will do and what will
        // arrive. "Update" says neither.
        let offer = match (&self.update, &self.install) {
            (Some(update::Standing::Behind(release)), update::Layout::Packaged(_)) => release,
            _ => return None,
        };
        let line = match &self.taking {
            Some(update::Fetch::Progress(so_far, total)) => {
                // A percentage rather than bytes: the number a person can read at a glance while
                // waiting is "how much of it", not "how many megabytes of it".
                let percent = if *total == 0 {
                    0
                } else {
                    (so_far.saturating_mul(100) / total).min(100)
                };
                return Some(
                    ui::Label::new(format!("downloading {} — {percent}%", offer.tag))
                        .muted()
                        .size(ui::Size::Compact)
                        .into_any_element(),
                );
            }
            Some(update::Fetch::Ready(root, integrity)) => {
                let checked = match integrity {
                    update::Integrity::Digest => "checked against the digest GitHub published",
                    // Said plainly rather than implied. Claiming more was verified than was is the
                    // §252 mistake, and this is the line where it would be made.
                    update::Integrity::SizeOnly => "checked by length only — no digest was published",
                };
                // No button here. **Restart to Update lives in the status bar**, and two places to
                // take one update is two places for it to be half-taken. This is the detail view:
                // where it came from, what was verified, where it is sitting.
                return Some(
                    ui::Label::new(format!(
                        "{} is downloaded and {checked}. Press Restart to Update in the status \
                         bar. It is waiting at {}.",
                        offer.tag,
                        root.display()
                    ))
                    .muted()
                    .size(ui::Size::Compact)
                    .into_any_element(),
                );
            }
            Some(update::Fetch::Failed(reason)) => Some(reason.clone()),
            None => None,
        };
        // Only ever a retry now: the download starts on its own when the check finds something, so
        // reaching this with nothing in flight means it failed or was interrupted.
        let label = format!("Try {} again", offer.tag);
        let mut column = div().flex().flex_col().w_full().min_w_0().gap_1();
        if let Some(reason) = line {
            column = column.child(
                ui::Label::new(format!("could not download it: {reason}"))
                    .muted()
                    .size(ui::Size::Compact),
            );
        }
        Some(
            column
                .child(
                    ui::Button::new("take-update", label)
                        .size(ui::Size::Compact)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.take_update(cx);
                        })),
                )
                .into_any_element(),
        )
    }
}


impl Workbench {
    /// The chip in the status bar, once an update is downloaded and waiting.
    ///
    /// **Not in About.** Asked for in these terms: *"It doesnt make any sense a user need to enter
    /// to about section to update th eprogram. A button must appear like in zed so with a click we
    /// restart and update."* — and that is right. An update behind a page nobody opens is an
    /// update nobody takes.
    ///
    /// The status bar rather than a titlebar because this app uses the OS titlebar; the status bar
    /// is its full-width, always-in-the-same-place strip, which §53 chose for Zed's own reason. It
    /// also means the chip costs no layout when it appears — a banner that pushes the transcript
    /// down would move the thing the researcher is reading.
    pub(crate) fn restart_chip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.update_dismissed {
            return None;
        }
        // The same question the press asks, asked once. "Restart to Update" with nothing staged
        // would be a button that lies, and a button whose conditions differ from its action's is
        // one that lies only sometimes — which is worse, because it looks like a broken app.
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
                // Sent away for this session only. The staged folder stays, so dismissing costs
                // nothing but the reminder, and next launch offers it again.
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
}


impl Workbench {
    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), theme::error()),
            None => (self.status.clone(), theme::text_muted()),
        };

        div()
            // Never squeezed. It is the last child of a column whose transcript grows, and
            // a flex child shrinks by default — which is how the toggles and the host
            // indicator ended up cut off at the bottom edge (docs §51).
            .flex_none()
            // A moving mark while anything is running. The first turn after launch spends
            // 20–40 seconds building the agent — MCP tool fetches, middleware, model
            // construction — and a still window during that reads as a hang, which is the
            // single most common reason someone kills an app that was working fine.
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
            // **The glance version, so the panel need not be open.** `running · execute` for six
            // minutes was the whole complaint; this is the same fact with a denominator on it, and
            // it appears only when there is a plan to count (§209).
            .children(self.work_summary().map(|summary| {
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(summary)
            }))
            // A blanket grant that is in force must never be invisible — and must be
            // revocable without starting a new conversation, or "just this once" becomes
            // permanent by inconvenience. Click to hand the gate back.
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
            // Panel toggles. Both always present, so a closed panel is never a one-way
            // door — the commonest way a collapsible panel becomes a bug report.
            .child(
                div()
                    .id("toggle-sidebar")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .text_color(rgb(if self.sidebar_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child(app_icon(
                        "icons/conversations.svg",
                        if self.sidebar_open {
                            theme::accent()
                        } else {
                            theme::text_faint()
                        },
                        None
                    ))
                    .child("conversations")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.sidebar_open = !workbench.sidebar_open;
                        workbench.remember_panels();
                        cx.notify();
                    })),
            )
            // The third of the same kind, between the two it belongs with — the road folds from
            // here as well as from its own chevron, because a strip folded to 38px hides its
            // chevron among the dots and the status bar is where the other two live.
            .child(
                div()
                    .id("toggle-road")
                    .flex_none()
                    .text_color(rgb(if self.road_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(app_icon(
                                "icons/road.svg",
                                if self.road_open {
                                    theme::accent()
                                } else {
                                    theme::text_faint()
                                },
                                None
                            ))
                            .child("road"),
                    )
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.toggle_road(cx);
                    })),
            )
            .child(
                div()
                    .id("toggle-panel")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .text_color(rgb(if self.panel_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child(app_icon(
                        "icons/research.svg",
                        if self.panel_open {
                            theme::accent()
                        } else {
                            theme::text_faint()
                        },
                        None
                    ))
                    .child("research")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.panel_open = !workbench.panel_open;
                        workbench.remember_panels();
                        cx.notify();
                    })),
            )
            // Say where the agent's code runs. When that is the user's own machine
            // it should be visible without opening a log (docs §18).
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
            // Discoverability: a palette nobody knows the shortcut for is a palette
            // nobody opens.
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
}

