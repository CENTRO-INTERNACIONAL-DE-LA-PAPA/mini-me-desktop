// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, sidebar::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
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


/// The colour for one CSV column.
///
/// Cycles the theme's own roles rather than inventing a rainbow: colours already checked
/// against every surface for contrast, so a wide table stays readable in every palette —
/// including the light one, where a fixed rainbow would wash out.
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


/// Consecutive identical steps folded into one line with a count.
///
/// An agent hunting for a file emits `glob` eight times in a row, and eight identical lines
/// carry exactly as much information as one — while costing eight lines of the answer's
/// screen space. `glob ×8` says the same thing and reads as one glance.
///
/// Only *consecutive* runs are folded. `read_file ×3, ls ×2, read_file ×3` is a different
/// story from `read_file ×6`, and flattening the order would erase it.
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


/// The specialist named in the most recent "delegating to X" step, if any.
///
/// The coordinator's own record of who it handed the turn to — `protocol::with_attachments`'s
/// sibling on the delegating side builds this exact label as `delegating to {subagent}` or
/// `delegating to {subagent} — {description}` the moment a `task` call is made, before that
/// namespace has produced a single frame of its own. It is what is left to attribute a message
/// to when the delegated run never surfaces an [`AgentTrace`] — a backend that does not stream
/// a subagent's own tokens back to this client still says, in its own words, who it asked.
pub(crate) fn last_delegated_to(steps: &[String]) -> Option<&str> {
    const PREFIX: &str = "delegating to ";
    steps.iter().rev().find_map(|step| {
        let rest = step.strip_prefix(PREFIX)?;
        Some(rest.split(" — ").next().unwrap_or(rest).trim())
    })
}


/// A labelled, bulleted list of spine entries.
pub(crate) fn spine_list(label: &'static str, items: &[String], bullet: &'static str) -> impl IntoElement {
    let mut list = div().flex().flex_col().gap_1().child(ui::Label::new(label).colour(theme::text_faint()).size(ui::Size::Compact));
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


/// Render one Markdown block as an element.
///
/// Emphasis becomes a `HighlightStyle` run rather than a nested element, which is how GPUI
/// wants inline styling: one shaped line per block, with ranges carrying the differences.
/// The gutter glyph for a list item at a given depth.
///
/// Only bullets change. A numbered item keeps the number the author wrote — renumbering it, or
/// swapping it for a bullet because it happens to be nested, would change what the answer says.
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


/// Render one Markdown block.
///
/// `selectable` is the transcript's span registry when this block is part of a conversation,
/// and `None` when it is not — the file preview renders the same blocks, and a drag there
/// must not run through the transcript's spans as if the two were one document.
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
                    // No monospace family is bundled yet, so code is marked by colour.
                    // Honest and legible; a real code face is a follow-up.
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
        let element = div().w_full().min_w_0().text_color(rgb(base)).text_sm();
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
        // Every level reads at the same `text_sm` as the rest of the chat — an answer is not
        // a document, and heading levels are told apart by the source's own emphasis, not by
        // a typographic scale.
        Block::Heading { inlines, .. } => styled(inlines, theme::text()).into_any_element(),
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
            // Indent per level. Capped at four because past that the text column is
            // narrower than the gutter, and a plan nested five deep is a plan nobody reads.
            .pl(px(16. * (*depth).min(4) as f32))
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    // A different glyph per level, so nesting survives a screenshot and a
                    // reader who cannot see the indentation of a wrapped line. A numbered
                    // item keeps its own number at any depth.
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
            // A rule down the left, which is what a quote looks like everywhere. The text is
            // muted, because a quote is something the answer is *referring* to.
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
                    .text_sm()
                    // Named, not fetched. See [`markdown::Block::Image`]: the path lives in
                    // the distro and figures the agent really produced are already shown
                    // below, found on the host (§42). Saying which file it meant is the
                    // useful part; pretending to display it would not be.
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
                // Nothing bundled — a stack ending at a face Windows always has. See
                // `ui::code_font`.
                .font(ui::code_font());
            // Selectable like any other run, and arguably the one that matters most: a
            // snippet is written to be copied.
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
            // Equal-width columns via `flex_1`, rather than measuring content. GPUI has no
            // table layout and measuring text before shaping is not something this app can
            // do honestly; even columns are predictable and never collapse a column to
            // nothing, which is what a naive proportional split does to a long cell.
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
            // Pad short rows so columns stay aligned when the source is ragged.
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
                // A hairline between rows, but not under the last one — the table's own
                // border already closes it.
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
    /// What finished while the researcher was away, and a press to go and look at it.
    ///
    /// **Above the composer rather than in a modal.** §40 settled where a thing that needs
    /// attention goes: there, because that is where attention already is and it cannot be scrolled
    /// away. A modal on launch is the first thing somebody fights before they can work, and worse
    /// when two runs finished — while a banner can do the thing a modal cannot, which is *take you
    /// there*. The status line it replaces is a strip at the bottom that the next message
    /// overwrites, and this is the one thing the app knows that the researcher has no other way to
    /// discover (§244).
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
        // The first one, because a single press has to mean something definite. With several, the
        // sidebar is the right place to choose and this only says to look.
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
            // **Two targets, not one.** The first version made the whole strip the press and gave
            // the single-run case no dismiss at all: *"I cannot dismiss the modal."* Opening a
            // conversation and deciding not to are different answers, so they are different
            // buttons — and the one that means "I have read this" has to exist in both cases,
            // because a notice you can only clear by going somewhere is a notice that holds the
            // window hostage (§250).
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
                        // Only the banner goes. The runs were recorded as announced when they were
                        // collected, so dismissing does not make them come back next launch — and
                        // their results are on disk either way.
                        workbench.collected_runs.clear();
                        cx.notify();
                    })),
            );
        Some(row.into_any_element())
    }
}


impl Workbench {
    /// The files going with the next question, each removable.
    ///
    /// Above the composer, where the picker and the approval card already are (§40): that is where
    /// attention is, and it cannot be scrolled away from.
    ///
    /// Each chip is its own remove button rather than the row carrying one action — §225a's rule.
    /// There is exactly one thing to do to an attachment you can see, and it is take it back.
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
                // The whole chip removes it, so the target is the chip and not a four-pixel
                // glyph at the end of a filename.
                ui::Chip::new(SharedString::from(format!("attached-{at}")), label)
                    .bg(theme::surface())
                    .removable(true)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if at < workbench.attachments.len() {
                            let gone = workbench.attachments.remove(at);
                            // The copy in the conversation's folder stays. It is the researcher's
                            // file now, it appears in Outputs, and deleting somebody's data
                            // because they changed their mind about one question would be a much
                            // worse surprise than a file they can delete themselves.
                            workbench.status =
                                format!("{} will not go with this question", gone.label);
                        }
                        cx.notify();
                    })),
            );
        }
        Some(row.into_any_element())
    }
}


impl Workbench {
    /// Who was consulted for this answer: `academic_researcher → theorizer → data_analysis`.
    ///
    /// How long it took and how many steps ran now live in [`Workbench::turn_footer`], shown
    /// under every answer rather than repeated here too.
    pub(crate) fn answer_chips(&self, message: &Message) -> impl IntoElement {
        /// Past this the row wraps into a paragraph and stops being a glance.
        const MAX_PILLS: usize = 6;

        let path = consulted(&message.agents);
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .w_full()
            .min_w_0()
            .text_sm();

        for (at, name) in path.iter().take(MAX_PILLS).enumerate() {
            if at > 0 {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
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
                    .child(name.replace('_', " ")),
            );
        }
        if path.len() > MAX_PILLS {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .child(format!("+{}", path.len() - MAX_PILLS)),
            );
        }
        row
    }
}


impl Workbench {
    /// Steps and elapsed time for one answer — shown under every assistant turn, always, not
    /// only the latest one and not only once the turn has finished. Replaces the old activity
    /// block's disclosure and the finished-only export row's word count in one line.
    pub(crate) fn turn_footer(
        &self,
        index: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let steps: usize = message.steps.len()
            + message
                .agents
                .iter()
                .map(|agent| agent.steps.len())
                .sum::<usize>();

        // While this is the turn still streaming, the end of its span is "now" — recomputed on
        // every token — rather than its last recorded invocation, which would freeze the clock
        // mid-answer.
        let live = self.streaming && index + 1 == self.transcript.len();
        let elapsed = self.turn_for(index).map(|turn| {
            let end = if live {
                provenance::now_ms()
            } else {
                turn.invocations
                    .iter()
                    .map(|invocation| invocation.last_seen)
                    .max()
                    .unwrap_or(turn.sent_at)
            };
            end.saturating_sub(turn.sent_at)
        });

        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .text_xs()
            .text_color(rgb(theme::text_faint()));

        // Time first, steps right beside it — one phrase, not two separate facts.
        if let Some(elapsed) = elapsed {
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .flex_none()
                    .child(
                        ui::Icon::new("icons/ladder.svg")
                            .size(ui::IconSize::ExtraSmall)
                            .colour(theme::text_faint()),
                    )
                    .child(duration_label(elapsed)),
            );
        }
        if steps > 0 {
            if elapsed.is_some() {
                row = row.child(div().flex_none().child("·"));
            }
            row = row.child(
                div()
                    .id(SharedString::from(format!("steps-{index}")))
                    .flex_none()
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                    .child(format!(
                        "{} {steps} {}",
                        if message.steps_expanded { "▾" } else { "▸" },
                        if steps == 1 { "step" } else { "steps" },
                    ))
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if let Some(message) = workbench.transcript.get_mut(index) {
                            message.steps_expanded = !message.steps_expanded;
                        }
                        workbench.invalidate_transcript_message(index);
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// The flat step list the `turn_footer` line above discloses: the coordinator's own steps,
    /// then every specialist's, named rather than grouped into its own collapsible block.
    pub(crate) fn turn_steps(&self, message: &Message) -> impl IntoElement {
        let mut list = div().flex().flex_col().w_full().min_w_0().gap_1();
        for step in fold_steps(&message.steps) {
            list = list.child(step_line(&step));
        }
        for trace in &message.agents {
            for step in fold_steps(&trace.steps) {
                list = list.child(step_line(&format!("{}: {step}", trace.name.replace('_', " "))));
            }
        }
        list
    }
}


impl Workbench {
    /// What an empty transcript says.
    ///
    /// It used to say one grey sentence. The replacement answers the two questions a researcher
    /// actually opens this window with — *where was I* and *what can this thing do* — using what
    /// the app already knows: their own recent conversations, and three things it is genuinely
    /// good at.
    ///
    /// **Nothing here runs anything.** Every starting move loads the composer and stops, which is
    /// the rule the project suggestions already follow and is org policy besides: the human
    /// decides what is asked.
    /// What the centre says while a conversation is being fetched.
    ///
    /// In the middle, because that is where the answer is about to be and where the researcher is
    /// already looking — the status bar reports it too, at the bottom of the window, which is the
    /// right place for a second copy and the wrong place for the only one.
    ///
    /// Deliberately plain: a mark and a word. A skeleton of grey bars would have to guess how many
    /// messages are coming and how tall each is, and guessing wrong makes the real transcript jump
    /// when it arrives.
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
}


impl Workbench {
    pub(crate) fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        /// Three, because a row of them has to stay readable in a narrow pane, and because a
        /// list of recent work long enough to scroll is the sidebar's job.
        const RECENT: usize = 3;

        let now = provenance::now_ms() as i64 / 1_000;

        let mut block = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            // Centred vertically: with nothing in the transcript there is no reading order to
            // preserve, and a page of prose pinned to the top of a tall window reads as a header.
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
                                            .child(ui::Icon::new("icons/agent-ellipse.svg").size(ui::IconSize::Small).colour(0xF47920)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(9.))
                                            .child(ui::Icon::new("icons/agent-ellipse.svg").size(ui::IconSize::Small).colour(0x20ADF4)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(18.))
                                            .child(ui::Icon::new("icons/agent-ellipse.svg").size(ui::IconSize::Small).colour(0xF42091)),
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

        // Where they left off. Only conversations that have actually been used — a list whose
        // first card is an empty thread from a mis-click is a list nobody trusts.
        let recent: Vec<&protocol::Conversation> = self
            .conversations
            .iter()
            .filter(|conversation| Some(&conversation.thread_id) != self.sidecar.thread_id().as_ref())
            .take(RECENT)
            .collect();
        if !recent.is_empty() {
            // `items_start`, or a flex row's default cross-axis stretch makes every card match
            // the tallest sibling's height instead of hugging its own two lines of content.
            let mut cards = div()
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .w_full()
                .min_w_0();
            for conversation in recent {
                let thread_id = conversation.thread_id.clone();
                // What is in it, counted off disk rather than remembered — the same source the
                // research panel reads, so the two cannot disagree.
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
                        // Equal thirds, and `min_w_0` so a long title ellipsises instead of
                        // widening its own card past the other two.
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
                    .child(ui::Label::new("PICK UP WHERE YOU LEFT OFF").colour(theme::text_faint()).size(ui::Size::Compact))
                    .child(cards),
            );
        }

        // Three things this is good at, in the researcher's words. Deliberately not a feature
        // list: each one is a sentence they could have typed themselves, and clicking it puts
        // exactly that in the composer for them to edit.
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
                    .bg(rgb(theme::surface()))
                    .text_color(rgb(theme::text_muted()))
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer().bg(rgb(theme::accent_soft())).border_color(rgb(theme::accent())))
                    .child(
                        div()
                            .child(ui::Icon::new(icon).size(ui::IconSize::Medium).colour(theme::accent()))
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_sm()
                            .child(label),
                    )
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.composer.update(cx, |composer, cx| {
                            composer.set_text(prompt, cx);
                        });
                        // Focused, because the prompt is a stem they have to finish.
                        window.focus(&workbench.composer.focus_handle(cx));
                        cx.notify();
                    })),
            );
        }
        block.child(moves)
    }
}


impl Workbench {
    /// Who answered message `index`, and the colour and tooltip its rail dot should carry.
    /// `None` for a "you" message, which never carries a dot at all.
    ///
    /// The same colour the agent picker already wears for that specialist, so the mapping is
    /// one a researcher has already seen rather than a second legend to learn. The *last* one
    /// consulted, since that is whose answer this is; a turn that never delegated is the
    /// coordinator's own, in a neutral tone rather than no dot at all, so a run of the
    /// coordinator's own answers still has a colour for the rail to fade toward.
    ///
    /// Four tiers, most trustworthy first.
    ///
    /// `AgentTrace` is the live, per-namespace record. The provenance turn is next — the *same*
    /// record `turn_for` already reads for elapsed time, which also carries a **background**
    /// invocation `message.agents` never will: `observe_background` files it there the moment a
    /// snapshot names it, precisely because a background worker runs on its own LangGraph thread
    /// and none of its events reach this conversation's stream to populate `message.agents` at
    /// all (docs on `Record::observe_background`). Then the coordinator's own "delegating to X"
    /// step, for a backend that never streams the delegated namespace's own frames back to this
    /// client either way. Last, a guess — this backend's coordinator turns out to answer
    /// everything itself while only *narrating* a plan ("I'll use the dataverse_explorer
    /// subagent to…"), leaving the first three nothing to find. Reading a name back out of that
    /// prose is not a report of what happened, only of what the answer said, which is why it is
    /// the last resort and the tooltip says so.
    fn dot_for(&self, index: usize, message: &Message) -> Option<(u32, SharedString)> {
        if message.role == "you" {
            return None;
        }
        let real_agent = message
            .agents
            .last()
            .map(|agent| agent.name.as_str())
            .or_else(|| {
                self.turn_for(index)
                    .and_then(|turn| turn.invocations.last())
                    .map(|invocation| invocation.name.as_str())
            });
        let delegated = real_agent.or_else(|| last_delegated_to(&message.steps));
        let guessed = delegated.or_else(|| subagent::mentioned_in(&message.body));
        let colour = guessed
            .map(|name| subagent::display(name).1)
            .unwrap_or_else(theme::text_faint);
        // What the dot is actually reading, on hover — so "why is this the wrong colour" has an
        // answer other than reading the source: the raw name behind the tooltip is exactly what
        // `subagent::display` keyed off of, unrecognised-and-white included.
        let hint: SharedString = if delegated.is_some() {
            let name = delegated.expect("checked");
            format!("answered by {} ({name})", subagent::display(name).0).into()
        } else if let Some(name) = guessed {
            format!(
                "likely {} ({name}) — named in the answer's own text, not in any recorded step",
                subagent::display(name).0
            )
            .into()
        } else {
            "answered directly by the coordinator — no specialist consulted or named".into()
        };
        Some((colour, hint))
    }

    /// Build one row only when GPUI's variable-height list asks for it (docs §156).
    pub(crate) fn transcript_message(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(message) = self.transcript.get(index) else {
            return div().into_any_element();
        };
        self.text_selection.begin_message(index);
        let asked = message.role == "you";
        let has_activity = !message.steps.is_empty() || !message.agents.is_empty();
        // An empty assistant body means we're still waiting on the first token — unless a trace
        // is already showing what's going on, which says more. The placeholder is not part of
        // the body, so it is not parsed and never reaches §70's Markdown cache.
        let waiting = message.body.is_empty() && self.streaming && !has_activity;
        let body = message.body.clone();
        // Side carries the role, so no label does: questions ride right in a bubble and answers
        // run full width as prose (§86). `pb_3` replaces the eager column's old inter-row gap;
        // list rows are independent elements and cannot inherit spacing from one another.
        let mut block = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_2()
            .p_2()
            .when(asked, |block| block.items_end());
        // The summary stays above the answer: it answers who did the work without requiring
        // the researcher to expand anything. Steps and elapsed time now run below the answer
        // instead, in `turn_footer`.
        if !asked && !message.agents.is_empty() {
            block = block.child(self.answer_chips(message));
        }
        if waiting {
            block = block.child(div().text_color(rgb(theme::text_muted())).text_sm().child("…"));
        }
        if !body.is_empty() {
            // The user's text is shown as typed. Assistant text uses the already-cached Markdown
            // blocks; virtualization must not undo §70 by parsing again when a row remounts.
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
                        .text_sm()
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
        // What was sent along with the question, in the same tile a turn's own files get —
        // rather than the raw `> Attached files (already saved in the sandbox working
        // directory): …` blockquote the backend reads them from, which is what a researcher
        // used to see verbatim on reopening a conversation (§310).
        if asked && !message.attached.is_empty() {
            if let Some(thread_dir) = self.thread_workspace() {
                let sent: Vec<workspace::Output> = message
                    .attached
                    .iter()
                    .filter_map(|name| workspace::attachment_output(&thread_dir, name))
                    .collect();
                if !sent.is_empty() {
                    block = block.child(
                        div()
                            .mt(px(6.))
                            .child(self.attachment_row(&format!("sent-{index}"), &sent, cx)),
                    );
                }
            }
        }
        // Marked, not hidden. A truncated answer looks exactly like a finished one, and whether
        // it was cut off decides whether the researcher can rely on it (§63).
        if message.stopped {
            block = block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::warning()))
                    .text_sm()
                    .child("— you stopped this turn; the answer above is incomplete"),
            );
        }
        // **Named above, not in the folder.** Stated as the fact it is rather than as an
        // accusation: a file can be missing because the command failed, because it was written
        // somewhere outside the conversation (§160), or because the answer recited a name it
        // never wrote. All three are worth knowing and the app cannot tell them apart, so it
        // reports the check and not a verdict (§175).
        if !message.unverified.is_empty() {
            let named = message.unverified.join(", ");
            block = block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::warning()))
                    .text_sm()
                    .child(format!(
                        "— named above but not in this conversation's folder: {named}"
                    )),
            );
        }

        // **And the way to get them back, here, where the note is.** This control has existed
        // since §279 and lived at the bottom of the WHAT RAN modal: Outputs panel, click the
        // card, scroll. A researcher whose eight plots landed in another folder read the note
        // above, found nothing to press, and lost the figures — *"I couldnt bring the plots
        // because any button appeared!!"*. The offer was two clicks away from the sentence that
        // explains why it is needed (§301).
        //
        // Once per conversation, not once per note: `recovery_on` decides which message carries
        // it, because `collect_outside` fetches everything whichever button is pressed.
        if self.recovery_on == Some(index) {
            if let Some((label, caveat)) = recovery_offer(message.unverified.len(), self.stray.len())
            {
                block = block.child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div().flex().flex_row().child(
                                ui::Button::new(("recover-stray", index))
                                    .text(if self.collect_in_flight {
                                        "Bringing them in…".to_string()
                                    } else {
                                        label
                                    })
                                    .style(ui::ButtonStyle::Primary)
                                    .disabled(self.collect_in_flight)
                                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                                        workbench.collect_outside(cx);
                                    })),
                            ),
                        )
                        .children(caveat.map(|note| {
                            ui::Label::new(note).muted().size(ui::Size::Compact)
                        })),
                );
            }
        }

        // Files remain after the answer that explains them. Preserve §162–§164's two bounded
        // galleries here: keeping PR #11's old per-file loop would compile and pass unit tests
        // while silently turning seven plots back into seven full transcript cards.
        //
        // **Minus the search records.** *"Papers is working, but I think its not necesary to show
        // it in the ui."* `papers.json` and `dataverse_search.json` exist so a researcher can take
        // the search away with them (§220) — they are not results to read in the conversation, and
        // the Sources and Datasets panels already say what is in them. Filtered here rather than
        // in `workspace::outputs`, so the Outputs panel and the thread's folder still list them.
        // One row, whoever produced them. Which subagent wrote which file is bookkeeping the
        // researcher did not ask for — the files themselves are the point (§310).
        let shown: Vec<workspace::Output> = message
            .outputs
            .iter()
            .filter(|output| !is_search_record(output))
            .cloned()
            .collect();
        if !shown.is_empty() {
            // A bit more room than the block's own `gap_2` gives every other pair of sections —
            // the attachments are a distinct thing the answer produced, not another line of it.
            block = block.child(
                div()
                    .mt(px(6.))
                    .child(self.attachment_row(&format!("transcript-{index}"), &shown, cx)),
            );
        }

        // Steps and elapsed time, on every assistant turn, always — not gated on being the
        // latest answer or on the turn having finished (§310).
        if !asked {
            // A negative margin against the block's own `gap_2`, rather than another spacing
            // constant to keep in step with it — the footer sits closer to the answer above it
            // than every other pair of sections does.
            block = block.child(div().mt(px(-4.)).child(self.turn_footer(index, message, cx)));
            if message.steps_expanded {
                let has_steps = !message.steps.is_empty()
                    || message.agents.iter().any(|agent| !agent.steps.is_empty());
                if has_steps {
                    block = block.child(self.turn_steps(message));
                }
            }
        }
        // A continuous rail down the whole conversation, not only the messages that carry a
        // dot: a "you" row draws the same line through itself with nothing on it, so the
        // segments either side of it still meet edge to edge instead of leaving a gap at every
        // question. Each row only ever draws its own segment — there is no way to see a
        // neighbour from here — so the join relies on rows sitting flush with no list-level gap
        // between them, which `gpui::list` already gives for free.
        //
        // Dashed rather than a solid fill: GPUI has no dashed *background*, only a dashed
        // *border* (the same `border_dashed` the provenance legend already draws a straight
        // line with) — so the colour comes from a handful of short bordered segments instead of
        // one continuous gradient fill.
        //
        // Absolute and pinned to all four edges, not a flex sibling relying on `align-items:
        // stretch` to match `block`'s height — that dependency was briefly suspected of being
        // why the transcript could not scroll to its own end, and it turned out not to be the
        // cause (`collect_plots` growing a finished message's outputs without telling the list
        // was), but positioning against `block`'s own wrapper here is still the more direct
        // reading: this rail has no content of its own to give it a height, so it takes the one
        // thing in this row that was never ambiguous instead of asking the layout to infer it.
        const RAIL: f32 = 16.;
        const DOT: f32 = 12.;
        const DOT_TOP: f32 = 12.;
        const DOT_CENTER: f32 = DOT_TOP + DOT / 2.;
        let x = px(RAIL / 2. - 1.);
        let dot = self.dot_for(index, message);
        // The colour this segment fades *from* — the nearest earlier row that had one, so the
        // line reads as one continuous thread shifting colour at each new answer rather than a
        // hard cut, and a run of "you" rows in between does not reset it to nothing. `None`
        // rather than a neutral default: a default here would draw a line above the very first
        // answer, where nothing has happened yet to connect to.
        let top = (0..index).rev().find_map(|earlier| {
            let earlier_message = self.transcript.get(earlier)?;
            self.dot_for(earlier, earlier_message).map(|(colour, _)| colour)
        });
        let bottom = dot.as_ref().map(|(colour, _)| *colour).or(top);

        const LINE: f32 = 3.;
        const PLATE: f32 = DOT + 4.;
        let mut rail = div()
            .absolute()
            .left_0()
            .top_0()
            .bottom_0()
            .w(px(RAIL));
        // Above the dot: solid in the *previous* colour, stopping dead at the circle rather
        // than fading into this row's own — one dash segment for the whole run rather than
        // several short ones, which is what made the pattern look squashed at each seam.
        if let Some(top) = top {
            rail = rail.child(
                div()
                    .absolute()
                    .left(x)
                    .top_0()
                    .h(px(DOT_CENTER))
                    .border_l(px(LINE))
                    .border_dashed()
                    .border_color(rgb(top)),
            );
        }
        // Below the dot, in this row's own colour — the next shift in colour belongs to
        // whichever later row has the next dot. Stops with this row when it is the
        // transcript's last: the line ends at the last dot instead of trailing into nothing.
        if let Some(bottom) = bottom {
            if index + 1 < self.transcript.len() {
                rail = rail.child(
                    div()
                        .absolute()
                        .left(x)
                        .top(px(DOT_CENTER))
                        .bottom_0()
                        .border_l(px(LINE))
                        .border_dashed()
                        .border_color(rgb(bottom)),
                );
            }
        }
        if let Some((colour, hint)) = dot {
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("who-answered-{index}")))
                    .absolute()
                    .left(px(RAIL / 2. - PLATE / 2.))
                    .top(px(DOT_CENTER - PLATE / 2.))
                    .size(px(PLATE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    // The icon itself fades to transparent at its edge, and the dashed line
                    // sits directly behind it — an opaque plate the same colour as the panel
                    // masks that out instead of letting the line show through.
                    .bg(rgb(theme::background()))
                    .child(
                        ui::Icon::new("icons/agent-ellipse.svg")
                            .size(ui::IconSize::ExtraSmall)
                            .colour(colour),
                    )
                    .tooltip(move |_window, cx| cx.new(|_| ui::Hint { text: hint.clone() }).into()),
            );
        }

        // `block` is the only normal-flow content here, so this wrapper's height is exactly
        // `block`'s own. The rail is laid over that in absolute position, reserving its own
        // room via padding rather than by consuming space as a flex item.
        div()
            .relative()
            .w_full()
            .min_w_0()
            .pl(px(RAIL + 8.))
            .child(rail)
            .child(block)
            .into_any_element()
    }
}


impl Workbench {
    pub(crate) fn live_turn_row(&self) -> gpui::AnyElement {
        let elapsed = self.provenance.turns.last()
            .map(|turn| provenance::now_ms().saturating_sub(turn.sent_at))
            .filter(|elapsed| *elapsed >= 1_000)
            .map(|elapsed| format!(" · {}", duration_label(elapsed))).unwrap_or_default();
        div().flex().flex_row().items_center().w_full().min_w_0().gap_2().pb_3()
            .text_color(rgb(theme::text_muted())).text_xs()
            .child(format!("{}{elapsed}", self.status)).into_any_element()
    }
}


impl Workbench {
    pub(crate) fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // `min_w_0` is what makes long assistant text *wrap* instead of running off
        // the right edge: a flex item defaults to min-width:auto, so its content
        // width becomes its floor and a long paragraph widens the pane instead of
        // flowing down.
        // `list` owns scrolling and cached row heights; the surrounding id belongs to pointer
        // selection and inspection, not to a competing scroll container (§156).
        // Last frame's span rectangles go now, before this frame registers its own: the
        // transcript moves under a scroll, a resize and every streamed token, and a highlight
        // painted from stale bounds is a highlight over the wrong words.
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
                // **The inset has to be on the row, not on the list.** GPUI's `list` applies only
                // the *vertical* half of its padding: `prepaint_items` places each item at
                // `bounds.origin + Point::new(px(0.), padding.top)`, so the horizontal half is
                // computed and then never used. The eager scrolling div this replaced honoured
                // all four sides, so §156 moved the transcript flush against its own border and
                // nothing said so (§174).
                div()
                    .w_full()
                    .min_w_0()
                    // Less on the left than `TRANSCRIPT_INSET`'s own doc comment calls for — it
                    // exists to line the transcript up with the composer below it, and this
                    // still comes within a few pixels of that. What sits at this edge is the
                    // rail's line now, not text, and that inset was reading as a bigger gap to
                    // the border than either the row's own margin or the panel's own padding
                    // alone (both tried first — visibly nothing, because this is the one that
                    // actually reaches the border).
                    .pl(px(TRANSCRIPT_INSET - 8.))
                    .pr(px(TRANSCRIPT_INSET))
                    .child(row)
                    .into_any_element()
            })
        })
        .w_full()
        .h_full()
        // Vertical only, which is all this ever applied. More at the bottom than the top so the
        // last message sits a little clear of the composer below it, not flush against it.
        .pt_4()
        .pb_8();
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
            // Releasing outside the transcript has to end the drag too, or the selection
            // keeps following the pointer after the button is long since up.
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.finish());
                    cx.notify();
                }),
            )
            // Deliberately leaves the selection alone: right-clicking a shade off the text
            // you just highlighted, in order to copy it, must not be what throws it away.
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Transcript, cx);
                }),
            );

        if self.opening {
            // **Not the empty state.** `open_conversation` clears the transcript before the fetch
            // lands, so for the width of that request the centre said *"What are you working
            // on?"* over a conversation that was already chosen — an invitation to start
            // something, offered because the app had nothing else to draw (§178).
            col = col.child(self.opening_state());
        } else if self.transcript.is_empty() {
            col = col.child(self.empty_state(cx));
        } else {
            col = col.child(rows);
        }
        // The conversation's own name, read the same way the sidebar row does: by the thread
        // the sidecar is currently attached to, looked up against the list it renders.
        let title = self
            .sidecar
            .thread_id()
            .and_then(|id| self.conversations.iter().find(|c| c.thread_id == id))
            .map(|conversation| conversation.title.clone());

        // Everything that is not the road: transcript, approval, picker, composer. Built as its
        // own column so the road can sit *beside* all of it rather than above the transcript
        // and below the composer.
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
                    .gap_3()
                    .flex_none()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .pb_3()
                    .text_color(rgb(theme::text()))
                    .child(
                        ui::Icon::new("icons/chat-circle-dots.svg")
                            .size(ui::IconSize::Small)
                            .colour(theme::text_muted())
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_base()
                                    .line_height(px(20.))
                                    .child(title),
                            )
                            // The conversation's own workspace, said once here instead of
                            // repeated inline on every attached turn (§267) — see
                            // `without_attached_blockquote`. Only the thread's own folder name
                            // (a UUID) is shown — the parent directories are the same on every
                            // conversation, so naming them again here is noise, not information.
                            .children(self.thread_workspace().map(|dir| {
                                let id = dir
                                    .file_name()
                                    .map(|name| name.to_string_lossy().to_string())
                                    .unwrap_or_else(|| dir.display().to_string());
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(rgb(theme::text_muted()))
                                            .child("Local Workspace:"),
                                    )
                                    .child(
                                        div()
                                            .id("open-thread-workspace")
                                            .min_w_0()
                                            .truncate()
                                            .text_color(rgb(theme::accent()))
                                            .underline()
                                            .child(format!("/{}", id))
                                            .hover(|style| style.cursor_pointer())
                                            .on_click(move |_event, _window, _cx| {
                                                if let Err(error) = workspace::open(&dir) {
                                                    tracing::warn!(%error, "could not open the workspace folder");
                                                }
                                            }),
                                    )
                            }))
                    )
            }))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .min_w_0()
                    .overflow_hidden()
                    // The region the thumb belongs to, so it appears when the pointer is over
                    // the transcript and nowhere else.
                    .group(SCROLL_GROUP)
                    .child(col)
                    .children(ui::list_scrollbar(&list_state)),
            );
        // Above the composer, so the decision sits where the user's attention already
        // is and cannot be scrolled out of view.
        if let Some(request) = &self.pending_approval {
            column = column.child(self.approval_card(request, cx));
        }
        let column = column
            .children(self.collected_banner(cx))
            .children(self.attachment_chips(cx))
            // The indicator anchors to *this* box, not the transcript's — `collected_banner`
            // and `attachment_chips` above are both optional, so anything anchored further up
            // the tree would land a different distance from the composer depending on which of
            // them happened to be showing (§263).
            .child(self.composer_input(cx));

        // The actual middle panel
        div()
            .flex()
            // A row now: the road, then everything else.
            .flex_row()
            .flex_grow()
            .min_w_0()
            .h_full()
            .my_2()
            .mx_1()
            .p_3()
            .gap_5()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(column)
    }
}


impl Workbench {
    /// Every attachment a turn produced, in one capped row.
    ///
    /// No per-producer heading and no divider between them: which subagent wrote which file is
    /// bookkeeping a researcher did not ask for, so images and other files alike sit in one row
    /// in the order they were produced. Past three, the last tile carries the same `+N` overlay
    /// the image gallery already uses (§310).
    pub(crate) fn attachment_row(
        &self,
        scope: &str,
        items: &[workspace::Output],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        /// Past this the row wraps into the existing `+N` overlay instead of growing sideways.
        const MAX_TILES: usize = 3;

        let shown = items.len().min(MAX_TILES);
        let hidden = if items.len() > MAX_TILES {
            items.len() - (MAX_TILES - 1)
        } else {
            0
        };

        let mut row = div().flex().flex_row().flex_wrap().gap_2().flex_none();
        for at in 0..shown {
            let more = (hidden > 0 && at + 1 == shown).then_some(hidden);
            row = row.child(self.attachment_tile(
                format!("attachment-{scope}-{at}"),
                items,
                at,
                more,
                cx,
            ));
        }
        row
    }

    /// One attachment: a thumbnail (a picture for a figure, a glyph for anything else) with its
    /// filename underneath, inside the same bordered box. Clicking it opens the existing
    /// preview — there is no separate "Open"/"Reveal" control to press first.
    pub(crate) fn attachment_tile(
        &self,
        id: String,
        set: &[workspace::Output],
        at: usize,
        more: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        const TILE: f32 = GRID_TILE_COMPACT;
        // The tile's own `p_2()` eats into its width on both sides before `media`'s `w_full()`
        // ever sees it — the same subtraction `name_chars` makes for the same reason — so
        // squaring the *content* box means matching that width, not the outer tile's own.
        const MEDIA: f32 = TILE - 16.;

        let output = &set[at];
        let opening = set.to_vec();
        let is_image = output.kind == workspace::Kind::Figure;
        let (glyph, ink) = file_mark(&output.path);

        let scrim = |more: usize| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(gpui::rgba(0x000000a6))
                .text_color(rgb(SCRIM_INK))
                .text_size(px(media_scrim_size(TILE)))
                .child(format!("+{more}"))
        };

        let media = if is_image {
            div()
                .relative()
                .w_full()
                .h(px(MEDIA))
                .flex_none()
                .rounded_md()
                .overflow_hidden()
                .child(
                    img(output.path.clone())
                        .w_full()
                        .h_full()
                        .rounded_md()
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .when_some(more, |media, more| media.child(scrim(more)))
                .into_any_element()
        } else {
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .h(px(MEDIA))
                .flex_none()
                .child(ui::Icon::new(glyph).size(ui::IconSize::Large).colour(ink))
                .when_some(more, |media, more| media.child(scrim(more)))
                .into_any_element()
        };

        div()
            .id(SharedString::from(id))
            .flex()
            .flex_col()
            .flex_none()
            .w(px(TILE))
            .gap_1()
            .p_2()
            .rounded_lg()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .hover(|style| style.border_color(rgb(theme::accent())).cursor_pointer())
            .child(media)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(distinguishing_tail(&output_filename(output), name_chars(TILE))),
            )
            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                workbench.preview = Preview::opening(opening.clone(), at);
                cx.notify();
            }))
    }
}


