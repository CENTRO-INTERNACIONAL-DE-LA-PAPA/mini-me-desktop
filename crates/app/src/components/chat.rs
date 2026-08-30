#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

/// Whether a file is column-separated, and so worth colouring by column.
pub(crate) fn is_delimited(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".csv") || name.ends_with(".tsv")
}

/// The colour for one CSV column, cycling the theme's own roles.
pub(crate) fn column_colour(column: usize) -> u32 {
    const WHEEL: [fn() -> u32; 6] = [
        theme::text,
        theme::accent,
        theme::running,
        theme::success,
        theme::warning,
        theme::text_muted,
    ];
    WHEEL[column % WHEEL.len()]()
}

/// Consecutive identical steps folded into one line with a count (only consecutive runs fold, so
/// `read_file ×3, ls ×2, read_file ×3` stays distinct from `read_file ×6`).
pub(crate) fn fold_steps(steps: &[String]) -> Vec<String> {
    let mut folded: Vec<(String, usize)> = Vec::new();
    for step in steps {
        match folded.last_mut() {
            Some((label, count)) if label == step => *count += 1,
            _ => folded.push((step.clone(), 1)),
        }
    }
    folded
        .into_iter()
        .map(|(label, count)| {
            if count > 1 {
                format!("{label} ×{count}")
            } else {
                label
            }
        })
        .collect()
}

/// A labelled, bulleted list of spine entries.
pub(crate) fn spine_list(label: &'static str, items: &[String], bullet: &'static str) -> impl IntoElement {
    let mut list = div().flex().flex_col().gap_1().child(section_label(label));
    for item in items {
        list = list.child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
                        .text_sm()
                        .child(bullet),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .text_color(rgb(theme::text()))
                        .text_sm()
                        .child(item.clone()),
                ),
        );
    }
    list
}

/// The gutter glyph for a list item at a given depth. A numbered item keeps its own number
/// regardless of depth — renumbering it would change what the answer says.
pub(crate) fn nested_marker(marker: &str, depth: usize) -> String {
    if marker.ends_with('.') {
        return marker.to_string();
    }
    match depth {
        0 => "·".to_string(),
        1 => "‣".to_string(),
        _ => "–".to_string(),
    }
}

/// Render one Markdown block. `selectable` is the transcript's span registry when this block is
/// part of a conversation, and `None` when it's rendered standalone (e.g. a file preview).
pub(crate) fn markdown_block(
    block: &markdown::Block,
    selectable: Option<&selection::Transcript>,
) -> gpui::AnyElement {
    use markdown::{Block, Emphasis};

    let styled = |inlines: &markdown::Inlines, base: u32| {
        let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = inlines
            .styles
            .iter()
            .map(|(range, emphasis)| {
                let style = match emphasis {
                    Emphasis::Strong => HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                    Emphasis::Italic => HighlightStyle {
                        font_style: Some(FontStyle::Italic),
                        ..Default::default()
                    },
                    Emphasis::Code => HighlightStyle {
                        color: Some(rgb(theme::accent()).into()),
                        ..Default::default()
                    },
                    Emphasis::Link => HighlightStyle {
                        color: Some(rgb(theme::accent()).into()),
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Emphasis::Url => HighlightStyle {
                        color: Some(rgb(theme::text_muted()).into()),
                        ..Default::default()
                    },
                };
                (range.clone(), style)
            })
            .collect();
        let text = StyledText::new(inlines.text.clone()).with_highlights(highlights);
        let element = div().w_full().min_w_0().text_color(rgb(base));
        match selectable {
            Some(transcript) => element.child(selection::Selectable::new(
                transcript,
                inlines.text.clone(),
                text,
            )),
            None => element.child(text),
        }
    };

    match block {
        Block::Heading { level, inlines } => {
            let element = styled(inlines, theme::text());
            if *level <= 2 {
                element.text_lg().into_any_element()
            } else {
                element.into_any_element()
            }
        }
        Block::Paragraph(inlines) => styled(inlines, theme::text()).into_any_element(),
        Block::ListItem {
            marker,
            inlines,
            depth,
        } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .gap_2()
            .pl(px(16. * (*depth).min(4) as f32))
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .child(nested_marker(marker, *depth)),
            )
            .child(styled(inlines, theme::text()))
            .into_any_element(),
        Block::Quote { depth, inlines } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .pl(px(12. * (*depth).min(3) as f32))
            .border_l_2()
            .border_color(rgb(theme::border_strong()))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl_3()
                    .child(styled(inlines, theme::text_muted())),
            )
            .into_any_element(),
        Block::Image { alt, url } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(div().flex_none().child("🖼"))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(if alt.trim().is_empty() {
                        url.clone()
                    } else {
                        format!("{alt} — {url}")
                    }),
            )
            .into_any_element(),
        Block::Code { text, .. } => {
            let block = div()
                .w_full()
                .min_w_0()
                .p_2()
                .bg(rgb(theme::surface()))
                .border_1()
                .border_color(rgb(theme::border()))
                .text_color(rgb(theme::text()))
                .text_sm()
                .font(ui::code_font());
            match selectable {
                Some(transcript) => block
                    .child(selection::Selectable::new(
                        transcript,
                        text.clone(),
                        StyledText::new(text.clone()),
                    ))
                    .into_any_element(),
                None => block.child(text.clone()).into_any_element(),
            }
        }
        Block::Table { header, rows } => {
            // Equal-width columns via `flex_1` rather than measuring content: GPUI has no table
            // layout, and a naive proportional split can collapse a long cell's column to nothing.
            let columns = block.columns();
            let cell = |inlines: &markdown::Inlines, bold: bool| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .child(styled(
                        inlines,
                        if bold {
                            theme::text()
                        } else {
                            theme::text_muted()
                        },
                    ))
                    .when(bold, |row| row.font_weight(FontWeight::BOLD))
            };
            let padded = |row: &Vec<markdown::Inlines>| {
                let mut cells: Vec<markdown::Inlines> = row.clone();
                cells.resize_with(columns, Default::default);
                cells
            };

            let mut table = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .border_1()
                .border_color(rgb(theme::border()));
            if !header.is_empty() {
                let mut head = div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .bg(rgb(theme::surface()))
                    .border_b_1()
                    .border_color(rgb(theme::border()));
                for value in padded(header) {
                    head = head.child(cell(&value, true));
                }
                table = table.child(head);
            }
            for (index, row) in rows.iter().enumerate() {
                let mut line = div().flex().flex_row().w_full().min_w_0();
                if index + 1 < rows.len() {
                    line = line.border_b_1().border_color(rgb(theme::border()));
                }
                for value in padded(row) {
                    line = line.child(cell(&value, false));
                }
                table = table.child(line);
            }
            table.into_any_element()
        }
        Block::Rule => div()
            .w_full()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .into_any_element(),
    }
}

impl Workbench {
    /// The whole chat column: transcript, approval card, attachments/picker, composer.
    pub(crate) fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.text_selection.begin_frame();
        self.sync_transcript_list();
        let view = cx.entity().clone();
        let list_state = self.transcript_list.clone();
        let rows = gpui::list(list_state.clone(), move |index, _window, cx| {
            view.update(cx, |workbench, cx| {
                let row = if index < workbench.transcript.len() {
                    workbench.transcript_message(index, cx)
                } else {
                    workbench.live_turn_row()
                };
                // The inset lives on the row, not the list: GPUI's `list` only applies the
                // vertical half of its own padding to each item.
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(TRANSCRIPT_INSET))
                    .child(row)
                    .into_any_element()
            })
        })
        .w_full()
        .h_full()
        .py_4();
        let mut col = div()
            .id("transcript")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.clear());
                    if let Some(spot) = workbench.text_selection.spot_at(event.position) {
                        workbench
                            .text_selection
                            .update(|selection| selection.begin(spot));
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(|workbench, event: &gpui::MouseMoveEvent, _window, cx| {
                    if !workbench.text_selection.selection().dragging() {
                        return;
                    }
                    if let Some(spot) = workbench.text_selection.spot_at(event.position) {
                        workbench
                            .text_selection
                            .update(|selection| selection.extend(spot));
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.finish());
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.finish());
                    cx.notify();
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Transcript, cx);
                }),
            );

        if self.opening {
            col = col.child(self.opening_state());
        } else if self.transcript.is_empty() {
            col = col.child(self.empty_state(cx));
        } else {
            col = col.child(rows);
        }
        let title = self
            .sidecar
            .thread_id()
            .and_then(|id| self.conversations.iter().find(|c| c.thread_id == id))
            .map(|conversation| conversation.title.clone());

        let mut column = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .h_full()
            .children(title.map(|title| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .w_full()
                    .min_w_0()
                    .px_4()
                    .py_2()
                    .text_base()
                    .text_color(rgb(theme::text()))
                    .child(app_icon(
                        "icons/chat-circle-dots.svg",
                        theme::text(),
                        Some(ui::IconSize::Small.px()),
                    ))
                    .child(title)
            }))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .min_w_0()
                    .overflow_hidden()
                    .group(SCROLL_GROUP)
                    .child(col)
                    .children(list_scrollbar(&list_state)),
            );
        if let Some(request) = &self.pending_approval {
            column = column.child(self.approval_card(request, cx));
        }
        let column = column
            .children(self.collected_banner(cx))
            .children(self.attachment_chips(cx))
            .children(self.subagent_picker(cx))
            .child(self.composer_row(cx));

        div()
            .flex()
            .flex_row()
            .flex_grow()
            .min_w_0()
            .h_full()
            .m_2()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(column)
    }

    /// What finished while the researcher was away, with a press to go and look at it. Above the
    /// composer, where attention already is and cannot be scrolled away.
    pub(crate) fn collected_banner(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.collected_runs.is_empty() {
            return None;
        }
        let label = match self.collected_runs.as_slice() {
            [(_, job)] => format!(
                "{} finished while you were away — its results are in its conversation",
                job.kind.label()
            ),
            runs => format!(
                "{} background runs finished while you were away",
                runs.len()
            ),
        };
        let opens = match self.collected_runs.as_slice() {
            [(thread_id, _)] => Some(thread_id.clone()),
            _ => None,
        };
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_none()
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .mb_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(theme::accent()))
            .text_color(rgb(theme::text()))
            .text_xs()
            .child(
                ui::Label::new(label)
                    .inherit()
                    .size(ui::Size::Compact)
                    .ellipsis(),
            )
            .children(opens.map(|thread_id| {
                div()
                    .id("collected-open")
                    .flex_none()
                    .px_2()
                    .py_px()
                    .rounded_md()
                    .text_color(rgb(theme::accent()))
                    .hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .child("open it")
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.collected_runs.clear();
                        workbench.open_conversation(thread_id.clone(), cx);
                    }))
            }))
            .child(
                div()
                    .id("collected-dismiss")
                    .flex_none()
                    .px_1()
                    .rounded_md()
                    .text_color(rgb(theme::text_faint()))
                    .hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .child("×")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.collected_runs.clear();
                        cx.notify();
                    })),
            );
        Some(row.into_any_element())
    }

    /// The files going with the next question, each removable.
    pub(crate) fn attachment_chips(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .flex_none()
            .w_full()
            .min_w_0()
            .gap_1()
            .pb_1();
        for (at, attachment) in self.attachments.iter().enumerate() {
            let label = attachment.label.clone();
            row = row.child(
                ui::Chip::new(SharedString::from(format!("attached-{at}")), label)
                    .bg(theme::surface())
                    .removable(true)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if at < workbench.attachments.len() {
                            let gone = workbench.attachments.remove(at);
                            workbench.status =
                                format!("{} will not go with this question", gone.label);
                        }
                        cx.notify();
                    })),
            );
        }
        Some(row.into_any_element())
    }

    /// The approval card: the pending command(s), verbatim, and the decision. Shown inline above
    /// the composer, not as a modal — it must stay reachable no matter how long the command is.
    pub(crate) fn approval_card(&self, request: &ApprovalRequest, cx: &mut Context<Self>) -> impl IntoElement {
        let card = div()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .min_w_0()
            .gap_2()
            .m_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme::accent()))
            .bg(rgb(theme::surface()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::accent()))
                            .text_size(px(11.))
                            .child("RUN THIS ON YOUR MACHINE?"),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::text_faint()))
                            .text_size(px(11.))
                            .child(match request.actions.len() {
                                0 | 1 => request
                                    .actions
                                    .first()
                                    .map(|action| action.tool.clone())
                                    .unwrap_or_default(),
                                many => format!("{many} commands"),
                            }),
                    ),
            );

        let mut commands = div()
            .id("approval-commands")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .max_h(px(260.))
            .overflow_y_scroll();

        for action in &request.actions {
            if !action.description.is_empty() {
                commands = commands.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(action.description.clone()),
                );
            }
            commands = commands.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(theme::background()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .text_color(rgb(theme::text()))
                    .font(ui::code_font())
                    .text_size(px(12.5))
                    .line_height(px(19.))
                    .child(action.detail.clone()),
            );
        }

        let effect = match self.thread_workspace() {
            Some(dir) => format!(
                "Runs on {} with your permissions, in {}.",
                self.sidecar.execution(),
                dir.display()
            ),
            None => format!(
                "Runs on {} with your permissions.",
                self.sidecar.execution()
            ),
        };

        card.child(commands)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(effect),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        ui::Button::new("approve", "Approve")
                            .tone(ui::Tone::Accent)
                            .on_click(
                                cx.listener(|workbench, _event, _window, cx| {
                                    workbench.decide(true, cx)
                                }),
                            ),
                    )
                    .child(ui::Button::new("reject", "Reject").on_click(
                        cx.listener(|workbench, _event, _window, cx| workbench.decide(false, cx)),
                    ))
                    .child(div().flex_grow())
                    // Bounded grants only — nothing here persists past this turn/conversation, so
                    // a security gate can't calcify into an unread habit.
                    .child(
                        ui::Button::new("approve-turn", "Approve the rest of this turn")
                            .size(ui::Size::Compact)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.approve_rest_of_turn = true;
                                workbench.decide(true, cx);
                            })),
                    )
                    .child(
                        ui::Button::new(
                            "approve-conversation",
                            "Approve everything in this conversation",
                        )
                        .size(ui::Size::Compact)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_conversation = true;
                            workbench.decide(true, cx);
                        })),
                    ),
            )
    }

    /// The `/name` specialist picker, shown above the composer while typing a `/name`.
    pub(crate) fn subagent_picker(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let text = self.composer.read(cx).text().to_string();
        if !subagent::completing(&text) {
            return None;
        }
        let agents = workspace::subagents();
        let query = subagent::parse(&text).map(|c| c.name).unwrap_or_default();
        let matched = subagent::ranked(&query, &agents);

        let mut list = div()
            .id("subagent-picker")
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .min_w_0()
            .mx_2()
            .max_h(px(220.))
            .overflow_y_scroll()
            .rounded_lg()
            .bg(rgb(theme::elevated()))
            .border_1()
            .border_color(rgb(theme::border_strong()));

        if agents.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(
                        "No specialist list yet. It is written when the backend builds a \
                         coordinator, so ask one ordinary question first — and if you just \
                         updated the app, restart the backend (ctrl-p → Restart).",
                    ),
            );
        } else if matched.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(format!("No specialist matches \"{query}\".")),
            );
        }

        let selected = self.subagent_selected.min(matched.len().saturating_sub(1));
        for (index, agent) in matched.iter().enumerate() {
            let chosen = agent.name.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("subagent-{}", agent.name)))
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .gap_px()
                    .when(index == selected, |row| row.bg(rgb(theme::accent_soft())))
                    .hover(|style| style.bg(rgb(theme::overlay())).cursor_pointer())
                    .child(
                        ui::Label::new(format!("/{}", agent.name))
                            .colour(if index == selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            })
                            .ellipsis(),
                    )
                    .child(
                        ui::Label::new(agent.description.clone())
                            .colour(theme::text_faint())
                            .size(ui::Size::Compact)
                            .ellipsis(),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.choose_subagent(&chosen, cx);
                    })),
            );
        }
        Some(list.into_any_element())
    }

    /// The input row: attach button, text field, send/stop.
    pub(crate) fn composer_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_text = !self.composer.read(cx).text().trim().is_empty();
        let (send_icon, ink, hint) = if self.streaming {
            ("icons/stop-circle.svg", theme::error(), "Stop this turn")
        } else if has_text {
            ("icons/paper-plane-right.svg", theme::accent(), "Send")
        } else {
            (
                "icons/paper-plane-right.svg",
                theme::text_muted(),
                "Type a question first",
            )
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_none()
            .m_2()
            .px_2()
            .py_1()
            .rounded_lg()
            .text_sm()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .track_focus(&self.composer.focus_handle(cx))
            .drag_over::<gpui::ExternalPaths>(|style, _paths, _window, _cx| {
                style
                    .bg(rgb(theme::accent_soft()))
                    .border_color(rgb(theme::accent()))
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Composer, cx);
                }),
            )
            .child(
                ui::IconButton::new("attach-file", "icons/plus.svg")
                    .icon_size(ui::IconSize::Medium.px())
                    .hover_ink(theme::accent())
                    .tooltip("Add a file from this computer")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.choose_files(cx);
                    })),
            )
            .child(self.composer.clone())
            .child(
                ui::IconButton::new("send-turn", send_icon)
                    .icon_size(ui::IconSize::Medium.px())
                    .ink(ink)
                    .hoverable(has_text && !self.streaming || self.streaming)
                    .tooltip(hint)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        if workbench.streaming {
                            workbench.stop_turn(cx);
                            return;
                        }
                        workbench
                            .composer
                            .update(cx, |composer, cx| composer.submit_now(cx));
                    })),
            )
    }

    /// What the centre shows while a conversation is being fetched.
    pub(crate) fn opening_state(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .w_full()
            .flex_grow()
            .min_h_0()
            .child(ui::Spinner::new("opening-conversation").colour(theme::text_muted()))
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child("Opening this conversation…"),
            )
    }

    /// The "What are you working on?" landing screen: recent conversations plus starter prompts.
    pub(crate) fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const RECENT: usize = 3;

        let now = provenance::now_ms() as i64 / 1_000;

        let mut block = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .justify_center()
            .gap_10()
            .px(px(60.))
            .py(px(34.))
            .mx_auto()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .text_color(rgb(theme::text()))
                            .text_2xl()
                            .items_center()
                            .child(
                                div()
                                    .relative()
                                    .flex_none()
                                    .w(px(36.))
                                    .h(px(18.))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(0.))
                                            .child(app_icon_at("icons/agent-ellipse.svg", 0xF47920, 18.)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(9.))
                                            .child(app_icon_at("icons/agent-ellipse.svg", 0x20ADF4, 18.)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(18.))
                                            .child(app_icon_at("icons/agent-ellipse.svg", 0xF42091, 18.)),
                                    ),
                            )
                            .child("What are you working on?"),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::text_muted()))
                            .text_base()
                            .child(
                                "Ask below, or add one of your own data files with the clip — \
                                 dropping it on this window works too. Everything a turn \
                                 produces is saved into your Documents folder.",
                            ),
                    ),
            );

        let recent: Vec<&protocol::Conversation> = self
            .conversations
            .iter()
            .filter(|conversation| Some(&conversation.thread_id) != self.sidecar.thread_id().as_ref())
            .take(RECENT)
            .collect();
        if !recent.is_empty() {
            let mut cards = div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .w_full()
                .min_w_0();
            for conversation in recent {
                let thread_id = conversation.thread_id.clone();
                let outputs: usize = workspace::outputs(&workspace::thread_dir_in(
                    conversation.project.as_deref(),
                    &conversation.thread_id,
                ))
                .iter()
                .map(|(_, items)| items.len())
                .sum();
                let when = protocol::how_long_ago(&conversation.updated_at, now);
                let note = match (outputs, when.is_empty()) {
                    (0, true) => String::new(),
                    (0, false) => when,
                    (1, true) => "1 output".to_string(),
                    (1, false) => format!("1 output · {when}"),
                    (many, true) => format!("{many} outputs"),
                    (many, false) => format!("{many} outputs · {when}"),
                };
                cards = cards.child(
                    div()
                        .id(SharedString::from(format!("resume-{thread_id}")))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_grow()
                        .flex_basis(relative(0.33))
                        .min_w_0()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(theme::elevated()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .hover(|style| {
                            style
                                .border_color(rgb(theme::accent()))
                                .cursor_pointer()
                        })
                        .child(
                            ui::Label::new(conversation.title.clone())
                                .size(ui::Size::Compact)
                                .ellipsis(),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(11.))
                                .child(note),
                        )
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            workbench.open_conversation(thread_id.clone(), cx);
                        })),
                );
            }
            block = block.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_label("PICK UP WHERE YOU LEFT OFF"))
                    .child(cards),
            );
        }

        const MOVES: [(&str, &str, &str); 3] = [
            (
                "icons/binoculars.svg",
                "Find datasets in CIP Dataverse on a topic",
                "Search CIP Dataverse for datasets about ",
            ),
            (
                "icons/book-open-text.svg",
                "Summarise what the literature says, with references",
                "Summarise what the literature says about , with references.",
            ),
            (
                "icons/broom.svg",
                "Clean and profile a file I drop here",
                "Clean and profile the file I am about to drop, and tell me what is in it.",
            ),
        ];
        let mut moves = div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0();
        for (at, (icon, label, prompt)) in MOVES.into_iter().enumerate() {
            let leading = at == 0;
            moves = moves.child(
                div()
                    .id(SharedString::from(format!("start-{at}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .py_2()
                    .px_2p5()
                    .rounded_lg()
                    .border_1()
                    .when(leading, |row| {
                        row.bg(rgb(theme::accent_soft()))
                            .border_color(rgb(theme::accent()))
                    })
                    .when(!leading, |row| row.border_color(rgb(theme::border())))
                    .when(!leading, |row| {
                        row.hover(|style| style.bg(rgb(theme::surface())).cursor_pointer())
                    })
                    .when(leading, |row| row.hover(|style| style.cursor_pointer()))
                    .child(
                        div()
                            .child(app_icon(icon, theme::accent(), Some(ui::IconSize::Medium.px())))
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(theme::text_muted()))
                            .text_sm()
                            .when(leading, |row| row.text_color(rgb(theme::accent())))
                            .child(label),
                    )
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.composer.update(cx, |composer, cx| {
                            composer.set_text(prompt, cx);
                        });
                        window.focus(&workbench.composer.focus_handle(cx));
                        cx.notify();
                    })),
            );
        }
        block.child(moves)
    }

    /// Build one transcript row only when GPUI's virtualized list asks for it.
    pub(crate) fn transcript_message(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(message) = self.transcript.get(index) else {
            return div().into_any_element();
        };
        self.text_selection.begin_message(index);
        let asked = message.role == "you";
        let has_activity = !message.steps.is_empty() || !message.agents.is_empty();
        let waiting = message.body.is_empty() && self.streaming && !has_activity;
        let body = message.body.clone();
        let mut block = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .pb_3()
            .when(asked, |block| block.items_end());
        if !asked && !message.agents.is_empty() {
            block = block.child(self.answer_chips(index, message));
        }
        if has_activity {
            block = block.child(self.activity_block(index, message, cx));
        }
        if waiting {
            block = block.child(div().text_color(rgb(theme::text_muted())).child("…"));
        }
        if !body.is_empty() {
            if asked {
                block = block.child(
                    div()
                        .max_w(relative(0.78))
                        .min_w_0()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(rgb(theme::surface()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .text_color(rgb(theme::text()))
                        .child(selection::Selectable::new(
                            &self.text_selection,
                            body.clone(),
                            StyledText::new(body),
                        )),
                );
            } else {
                let mut rendered = div().flex().flex_col().w_full().min_w_0().gap_2();
                for parsed in &message.blocks {
                    rendered = rendered.child(markdown_block(parsed, Some(&self.text_selection)));
                }
                block = block.child(rendered);
            }
        }
        if message.stopped {
            block = block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::warning()))
                    .text_xs()
                    .child("— you stopped this turn; the answer above is incomplete"),
            );
        }
        if !message.unverified.is_empty() {
            let named = message.unverified.join(", ");
            block = block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::warning()))
                    .text_xs()
                    .child(format!(
                        "— named above but not in this conversation's folder: {named}"
                    )),
            );
        }

        // Search records are filtered here rather than in `workspace::outputs`, so the thread's
        // folder and the Outputs panel still list them — only the transcript hides them.
        let shown: Vec<workspace::Output> = message
            .outputs
            .iter()
            .filter(|output| !is_search_record(output))
            .cloned()
            .collect();
        for (band, (worker, produced)) in by_producer(&shown, &self.tasks, &self.authorship)
            .into_iter()
            .enumerate()
        {
            let (images, others) = split_images(&produced);
            if !images.is_empty() {
                block = block.child(self.output_grid(
                    &format!("transcript-{index}-i{band}"),
                    images_heading(images.len(), worker.as_deref()),
                    &images,
                    false,
                    cx,
                ));
            }
            for (at, group) in output_folder_groups(&others).iter().enumerate() {
                if let [output] = group.outputs.as_slice() {
                    block = block.child(self.output_card(
                        index * 64 + band * 16 + at,
                        output,
                        worker.as_deref(),
                        cx,
                    ));
                } else {
                    block = block.child(self.output_grid(
                        &format!("transcript-{index}-{band}-{at}"),
                        shorten_path_label(
                            &output_folder_label(&group.folder, worker.as_deref()),
                            TRANSCRIPT_HEADING_CHARS,
                        ),
                        &group
                            .outputs
                            .iter()
                            .map(|output| (*output).clone())
                            .collect::<Vec<_>>(),
                        false,
                        cx,
                    ));
                }
            }
        }

        if !asked
            && !message.body.is_empty()
            && index + 1 == self.transcript.len()
            && !self.streaming
        {
            block = block.child(self.export_row(message, cx));
        }
        block.into_any_element()
    }

    /// The agent activity trace for one turn: coordinator steps, then a collapsible group per
    /// subagent — without this a delegated turn looks like a frozen window until the answer lands.
    pub(crate) fn activity_block(
        &self,
        message_index: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut block = div().flex().flex_col().w_full().min_w_0().gap_1();

        let folded = fold_steps(&message.steps);
        if !folded.is_empty() {
            let count = message.steps.len();
            block = block.child(
                div()
                    .id(SharedString::from(format!("steps-{message_index}")))
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .hover(|style| style.cursor_pointer())
                    .child(format!(
                        "{} {count} {}",
                        if message.steps_expanded { "▾" } else { "▸" },
                        if count == 1 { "step" } else { "steps" },
                    ))
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if let Some(message) = workbench.transcript.get_mut(message_index) {
                            message.steps_expanded = !message.steps_expanded;
                        }
                        workbench.invalidate_transcript_message(message_index);
                        cx.notify();
                    })),
            );
            if message.steps_expanded {
                for step in &folded {
                    block = block.child(step_line(step));
                }
            }
        }

        for (trace_index, trace) in message.agents.iter().enumerate() {
            let steps = if trace.steps.len() == 1 {
                "1 step".to_string()
            } else {
                format!("{} steps", trace.steps.len())
            };
            let header = format!(
                "{} {} · {steps} · {} chars",
                if trace.expanded { "▾" } else { "▸" },
                trace.name,
                trace.text.chars().count(),
            );

            let mut group = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .pl_2()
                .border_l_1()
                .border_color(rgb(theme::border()))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "trace-{message_index}-{trace_index}"
                        )))
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::accent()))
                        .text_xs()
                        .hover(|style| style.cursor_pointer())
                        .child(header)
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            if let Some(message) = workbench.transcript.get_mut(message_index) {
                                if let Some(trace) = message.agents.get_mut(trace_index) {
                                    trace.expanded = !trace.expanded;
                                }
                            }
                            workbench.invalidate_transcript_message(message_index);
                            cx.notify();
                        })),
                );

            if trace.expanded {
                for step in &fold_steps(&trace.steps) {
                    group = group.child(step_line(step));
                }
                let preview = protocol::summarize_agent_result(&trace.text);
                if !preview.is_empty() {
                    group = group.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_color(rgb(theme::text_muted()))
                            .text_xs()
                            .child(preview),
                    );
                }
            }
            block = block.child(group);
        }

        block
    }

    /// Who was consulted for an answer, how long it took, how many steps.
    pub(crate) fn answer_chips(&self, index: usize, message: &Message) -> impl IntoElement {
        const MAX_PILLS: usize = 6;

        let path = consulted(&message.agents);
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .w_full()
            .min_w_0();

        for (at, name) in path.iter().take(MAX_PILLS).enumerate() {
            if at > 0 {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
                        .text_size(px(11.))
                        .child("→"),
                );
            }
            row = row.child(
                div()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(theme::elevated()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .text_color(rgb(specialist_ink(name).unwrap_or(theme::text_muted())))
                    .text_size(px(11.))
                    .child(name.replace('_', " ")),
            );
        }
        if path.len() > MAX_PILLS {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(format!("+{}", path.len() - MAX_PILLS)),
            );
        }

        let steps: usize = message.steps.len()
            + message
                .agents
                .iter()
                .map(|agent| agent.steps.len())
                .sum::<usize>();
        let mut note = String::new();
        if let Some(turn) = self.turn_for(index) {
            let span = turn
                .invocations
                .iter()
                .map(|invocation| invocation.last_seen)
                .max()
                .unwrap_or(turn.sent_at)
                .saturating_sub(turn.sent_at);
            if span >= 1_000 {
                note.push_str(&format!(" · {}", duration_label(span)));
            }
        }
        if steps > 0 {
            note.push_str(&format!(" · {steps} steps"));
        }
        if !note.is_empty() {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(note),
            );
        }
        row
    }

    /// What to do with a finished answer: export, cite, or re-run.
    pub(crate) fn export_row(&self, message: &Message, cx: &mut Context<Self>) -> impl IntoElement {
        let again = self
            .transcript
            .iter()
            .rev()
            .find(|earlier| earlier.role == "you")
            .map(|earlier| earlier.body.clone());
        let bibtex = bibliography(&self.sources, &self.source_origins());
        let answer = message.body.clone();

        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .pt_1()
            .child(
                ui::Button::new("export-pdf", "Save as PDF with references")
                    .tone(ui::Tone::Accent)
                    .size(ui::Size::Compact)
                    .disabled(self.reports.is_empty())
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.render_report(cx);
                    })),
            )
            .child(
                ui::Button::new("export-bibtex", "Copy BibTeX")
                    .size(ui::Size::Compact)
                    .disabled(bibtex.is_empty())
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        let entries = bibtex.matches("@misc").count();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(bibtex.clone()));
                        workbench.say(format!("{entries} references copied as BibTeX"), cx);
                    })),
            )
            .child(
                ui::Button::new("export-rerun", "Re-run this turn")
                    .size(ui::Size::Compact)
                    .disabled(again.is_none() || self.streaming)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if let Some(prompt) = again.clone() {
                            workbench
                                .composer
                                .update(cx, |composer, cx| composer.set_text(prompt, cx));
                            workbench.restore_focus = true;
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(format!("{} words", answer.split_whitespace().count())),
            )
    }

    pub(crate) fn live_turn_row(&self) -> gpui::AnyElement {
        let elapsed = self.provenance.turns.last()
            .map(|turn| provenance::now_ms().saturating_sub(turn.sent_at))
            .filter(|elapsed| *elapsed >= 1_000)
            .map(|elapsed| format!(" · {}", duration_label(elapsed))).unwrap_or_default();
        div().flex().flex_row().items_center().w_full().min_w_0().gap_2().pb_3()
            .text_color(rgb(theme::text_muted())).text_xs()
            .child(format!("{}{elapsed}", self.status)).into_any_element()
    }

    /// The provenance rail beside the chat pane: which specialist is running, fold/unfold, and a
    /// timeline of stages visited.
    pub(crate) fn provenance_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const OPEN: f32 = 172.;
        const FOLDED: f32 = 38.;
        const DOT: f32 = 9.;
        const GUTTER: f32 = 12.;
        const ROW_OPEN: f32 = 46.;
        const ROW_FOLDED: f32 = 26.;

        let stages = self.provenance.road();
        let running = self
            .streaming
            .then(|| stages.iter().max_by_key(|stage| stage.last_seen))
            .flatten()
            .map(|stage| stage.name.clone());

        let mut strip = div()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(px(if self.road_open { OPEN } else { FOLDED }))
            .pt(px(18.))
            .pb(px(14.))
            .when(self.road_open, |strip| strip.px(px(14.)).gap_3())
            .when(!self.road_open, |strip| strip.items_center().gap_2())
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()));

        strip = strip.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .flex_none()
                .when(!self.road_open, |header| header.justify_center())
                .when(self.road_open, |header| {
                    header.child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .text_size(px(11.))
                            .child("THE ROAD"),
                    )
                })
                .child(
                    div()
                        .id("fold-road")
                        .flex_none()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(12.))
                        .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                        .child(if self.road_open { "‹" } else { "›" })
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.toggle_road(cx);
                        })),
                ),
        );

        if stages.is_empty() {
            if self.road_open {
                strip = strip.child(
                    div()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .child("The specialists this enquiry consults appear here as it reaches them."),
                );
            }
            return strip;
        }

        let mut body = div().flex().flex_col().flex_grow().min_h_0().w_full();
        let last = stages.len().saturating_sub(1);
        for (at, stage) in stages.iter().enumerate() {
            let is_running = running.as_deref() == Some(stage.name.as_str());

            let gutter = div()
                .flex()
                .flex_col()
                .items_center()
                .flex_none()
                .w(px(GUTTER))
                .child(
                    div()
                        .flex_none()
                        .size(px(DOT))
                        .rounded_full()
                        .when(is_running, |dot| {
                            dot.border_2().border_color(rgb(theme::running()))
                        })
                        .when(!is_running, |dot| dot.bg(rgb(theme::accent()))),
                )
                .when(at < last, |gutter| {
                    gutter.child(
                        div()
                            .flex_grow()
                            .w(px(2.))
                            .min_h(px(14.))
                            .bg(rgb(theme::border_strong())),
                    )
                });

            let mut row = div()
                .flex()
                .flex_row()
                .w_full()
                .min_w_0()
                .when(!self.road_open, |row| row.justify_center())
                .when(at < last, |row| {
                    row.min_h(px(if self.road_open { ROW_OPEN } else { ROW_FOLDED }))
                })
                .child(gutter);

            if self.road_open {
                let note = if is_running {
                    format!("running · {}", duration_label(stage.busy_ms))
                } else if stage.visits > 1 {
                    format!(
                        "visited {} times · {}",
                        stage.visits,
                        duration_label(stage.busy_ms)
                    )
                } else {
                    duration_label(stage.busy_ms)
                };
                row = row.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow()
                        .min_w_0()
                        .items_start()
                        .pl_2()
                        .mt(px(-3.))
                        .child(
                            ui::Label::new(stage.name.replace('_', " "))
                                .colour(if is_running { theme::running() } else { theme::text() })
                                .ellipsis(),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(11.))
                                .child(note),
                        ),
                );
            }
            body = body.child(row);
        }
        strip = strip.child(body);

        if self.road_open {
            strip = strip
                .child(
                    div().w_full().flex_none().child(
                        ui::Button::new("road-full-graph", "Full graph")
                            .tone(ui::Tone::Accent)
                            .size(ui::Size::Compact)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.provenance_view = ProvenanceView::Graph;
                                workbench.provenance_open = true;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .child("Written beside this conversation's files, so it survives a reload."),
                );
        }
        strip
    }
}
