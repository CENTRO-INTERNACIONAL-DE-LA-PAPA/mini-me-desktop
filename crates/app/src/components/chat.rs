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
            // Only two sizes: an answer is not a document, and six heading levels of
            // typography would be noise.
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
                    .text_xs()
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
    /// The `/name` picker, shown above the composer.
    ///
    /// Above it for the same reason the approval card is (§40): that is where attention already
    /// is, and it cannot be scrolled away from. A plain flex child rather than a floating popup —
    /// no position to measure, and it behaves like part of the composer, which is what it is.
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
            // The registry is written when the backend assembles a coordinator, so before the
            // first turn there is nothing to offer. Say which, rather than showing an empty box.
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
                    // The description is not decoration: none of these names says what it does,
                    // and the request's own guesses show that nobody can be expected to know.
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
}


impl Workbench {
    /// Who was consulted for this answer, how long it took, how many steps.
    ///
    /// The path reads `academic_researcher → theorizer → data_analysis · 19s · 4 steps`, which is
    /// the summary people were expanding the trace to reconstruct.
    pub(crate) fn answer_chips(&self, index: usize, message: &Message) -> impl IntoElement {
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

        // Steps across the whole turn: the coordinator's own, plus every specialist's.
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
}


impl Workbench {
    /// What to do with a finished answer.
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
                    // Disabled rather than hidden, so the affordance is discoverable before
                    // there is a report to use it on.
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
                        // Into the composer, not straight to the backend. Re-running is a
                        // decision, and a question worth asking twice is usually worth editing
                        // first — the same rule every other suggestion here follows.
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
                    .child(section_label("PICK UP WHERE YOU LEFT OFF"))
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
                    // The first is marked, not louder: one suggestion carrying the accent is a
                    // recommendation, three would be a menu shouting.
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
            .gap_1()
            .pb_3()
            .when(asked, |block| block.items_end());
        // The summary stays above the trace and answer: it answers who did the work without
        // requiring the researcher to expand anything.
        if !asked && !message.agents.is_empty() {
            block = block.child(self.answer_chips(index, message));
        }
        // The trace precedes the answer because that is the order the work happened in.
        if has_activity {
            block = block.child(self.activity_block(index, message, cx));
        }
        if waiting {
            block = block.child(div().text_color(rgb(theme::text_muted())).child("…"));
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
        // Marked, not hidden. A truncated answer looks exactly like a finished one, and whether
        // it was cut off decides whether the researcher can rely on it (§63).
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
                    .text_xs()
                    .child(format!(
                        "— named above but not in this conversation's folder: {named}"
                    )),
            );
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

        // Only the latest completed answer gets export actions. Repeating them under every row
        // would make a long virtual transcript a wall of controls just as it did when eager.
        if !asked
            && !message.body.is_empty()
            && index + 1 == self.transcript.len()
            && !self.streaming
        {
            block = block.child(self.export_row(message, cx));
        }
        block.into_any_element()
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
                    .px(px(TRANSCRIPT_INSET))
                    .child(row)
                    .into_any_element()
            })
        })
        .w_full()
        .h_full()
        // Vertical only, which is all this ever applied.
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
                    // The region the thumb belongs to, so it appears when the pointer is over
                    // the transcript and nowhere else.
                    .group(SCROLL_GROUP)
                    .child(col)
                    .children(list_scrollbar(&list_state)),
            );
        // Above the composer, so the decision sits where the user's attention already
        // is and cannot be scrolled out of view.
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
            // A row now: the road, then everything else.
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
}


impl Workbench {
    /// The agent activity trace for one turn: coordinator steps as one-liners, then
    /// a collapsible group per subagent.
    ///
    /// This exists because a delegated turn is otherwise *silent*: the coordinator
    /// emits only a `task` tool call while a subagent does the real work, so the user
    /// sees a frozen window and then an answer with no account of where it came from
    /// (plan §15).
    pub(crate) fn activity_block(
        &self,
        message_index: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut block = div().flex().flex_col().w_full().min_w_0().gap_1();

        // The coordinator's own steps, behind the same disclosure the subagent groups have
        // had all along. Flat and unbounded, they ran to twenty lines of `read_file`, `ls`
        // and `glob` and pushed the actual answer off the screen (docs §47).
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
                        // Unique per (turn, trace) so GPUI keeps each group's click
                        // state to itself.
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
                // Not the raw stream: a subagent's answer often arrives as one JSON
                // object, which is unreadable as a trace line.
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
}


impl Workbench {
    /// The input row: the text field plus a Send affordance.
    pub(crate) fn composer_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Three states, which is what every shipped chat composer converged on: a filled
        // circular button that sends, the same button greyed when there is nothing to
        // send, and a stop control while a turn streams. Empty-means-disabled is the
        // near-universal rule, and a send/stop toggle in the composer is how the running
        // state is expressed without adding a second control (docs §52).
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
            // The composer reads as one field with a control inside it, rather than a
            // text box sitting next to an unrelated button.
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            // Which field has the keyboard is otherwise invisible — there is a caret, and it
            // is two pixels wide. `in_focus` rather than `focus` because the thing with the
            // focus is a child entity, not this box.
            .track_focus(&self.composer.focus_handle(cx))
            // .in_focus(|style| style.border_color(rgb(theme::accent())))
            // **Feedback for a gesture that had none.** A file dragged over the window changed
            // nothing on screen, so there was no way to tell the app would take it until you
            // let go. Lit here rather than over the whole window because this is where the
            // file lands: it becomes part of the question, and the drop does not send it.
            //
            // A style refinement rather than a flag on `Workbench`, deliberately. gpui clears
            // `active_drag` on `FileDropEvent::Exited` but dispatches that event to no element,
            // so a flag set on enter would have no way to learn the drag left the window and
            // would stay lit until the next drop.
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
                        // Same path as Enter. Calling the entity directly rather than
                        // dispatching an action keeps this working regardless of where
                        // focus is when the button is clicked.
                        workbench
                            .composer
                            .update(cx, |composer, cx| composer.submit_now(cx));
                    })),
            )
    }
}

