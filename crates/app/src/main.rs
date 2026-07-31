//! Mini-Me Desktop — GPUI entry point and root workbench view.
//!
//! P6.3. The three-pane workbench (rail / chat / artifacts) streams a **real
//! coordinator turn** from the local Python sidecar: `Sidecar` spawns and
//! health-checks the backend, assistant tokens land in the transcript as they
//! arrive over SSE, and the **agent activity trace** shows what subagents are doing
//! while they do it instead of leaving a silent gap (plan §15c).
//!
//! Built against the published `gpui 0.2.2` (see crates/app/Cargo.toml). Markdown
//! rendering and the command palette are still open.

mod backend;
mod composer;
mod protocol;
mod sidecar;

use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Entity, Focusable,
    SharedString, Window, WindowBounds, WindowOptions,
};

use composer::{Composer, ComposerEvent};
use protocol::{AgentRef, Bucket, Project, TurnEvent};
use sidecar::Sidecar;

// ---- Palette (placeholder; align with the web app's tokens in P6.3) --------
const BG: u32 = 0x1e1e22;
const PANEL: u32 = 0x26262b;
const BORDER: u32 = 0x3a3a42;
const TEXT: u32 = 0xe8e8ea;
const MUTED: u32 = 0x9a9aa2;
const ACCENT: u32 = 0xe8703a; // Mini-Me orange
const ERROR: u32 = 0xe05252;

/// Prefilled into the composer on first launch so Enter alone proves the round
/// trip; the user can clear or replace it.
const SEED_PROMPT: &str = "In one short paragraph, what is your role as the Mini-Me coordinator?";

/// A small caps-ish section heading for the side panel.
fn section_label(text: &'static str) -> impl IntoElement {
    div().text_color(rgb(ACCENT)).text_xs().child(text)
}

/// One line of the activity trace: a tool call, or a delegation.
fn step_line(label: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(MUTED))
        .text_xs()
        .child(format!("· {label}"))
}

/// A labelled, bulleted list of spine entries.
fn spine_list(label: &'static str, items: &[String], bullet: &'static str) -> impl IntoElement {
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label(label));
    for item in items {
        list = list.child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().flex_none().text_color(rgb(MUTED)).text_sm().child(bullet))
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .text_color(rgb(TEXT))
                        .text_sm()
                        .child(item.clone()),
                ),
        );
    }
    list
}

/// A single chat message in the transcript, plus the agent activity behind it.
struct Message {
    role: &'static str,
    body: String,
    /// Coordinator-level steps (tool calls, delegations), in the order they happened.
    steps: Vec<String>,
    /// One group per subagent invocation.
    agents: Vec<AgentTrace>,
}

impl Message {
    fn new(role: &'static str, body: String) -> Self {
        Self {
            role,
            body,
            steps: Vec::new(),
            agents: Vec::new(),
        }
    }

    /// Nothing happened here worth keeping. A turn that produced only tool calls
    /// still has activity, so "empty body" alone is not enough to drop a message —
    /// that would throw away the only record of a purely delegated turn.
    fn is_silent(&self) -> bool {
        self.body.is_empty() && self.steps.is_empty() && self.agents.is_empty()
    }
}

/// Live trace of one subagent invocation.
struct AgentTrace {
    /// The namespace from [`AgentRef`] — the grouping key, unique per invocation.
    ns: String,
    name: String,
    steps: Vec<String>,
    text: String,
    expanded: bool,
}

/// Most text one trace keeps. A trace is a tail-followed log, and a research turn
/// can stream far more subagent text than the answer it produces, so when a group
/// overflows we drop from the *front*: the newest work is what the user is watching.
const MAX_TRACE_CHARS: usize = 4_000;

impl AgentTrace {
    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
        let overflow = self.text.chars().count().saturating_sub(MAX_TRACE_CHARS);
        if overflow > 0 {
            let kept: String = self.text.chars().skip(overflow).collect();
            self.text = format!("…{kept}");
        }
    }
}

/// Find (or start) the trace group for a subagent invocation.
fn trace_for<'a>(message: &'a mut Message, agent: &AgentRef) -> &'a mut AgentTrace {
    if let Some(index) = message.agents.iter().position(|trace| trace.ns == agent.ns) {
        return &mut message.agents[index];
    }
    message.agents.push(AgentTrace {
        ns: agent.ns.clone(),
        name: agent.name.clone(),
        steps: Vec::new(),
        text: String::new(),
        // A subagent that just started is what is happening *now*, so it opens
        // expanded; the turn ending collapses everything so the answer stays primary.
        expanded: true,
    });
    message.agents.last_mut().expect("just pushed")
}

/// Root view: the three-pane research workbench.
struct Workbench {
    /// The project spine from `GET /project`. `None` until the first fetch lands
    /// (or if the backend isn't up yet) — the panel says so rather than lying.
    project: Option<Project>,
    /// Research outputs, from the latest `values` snapshot of the current run.
    buckets: Vec<Bucket>,
    transcript: Vec<Message>,
    sidecar: Arc<Sidecar>,
    /// Status line text (backend/stream progress, not model output).
    status: String,
    /// True while a turn is in flight — gates the run button.
    streaming: bool,
    /// Set when the last turn failed, rendered in the status line.
    error: Option<String>,
    /// The text field. Owns its own focus/selection state.
    composer: Entity<Composer>,
}

impl Workbench {
    fn new(sidecar: Arc<Sidecar>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut composer = Composer::new(cx, "Ask Mini-Me…  (Enter to send)");
            composer.set_text(SEED_PROMPT, cx);
            composer
        });
        // The composer only reports *that* text was submitted; deciding it means
        // "run a coordinator turn" stays here.
        cx.subscribe(&composer, |workbench, _composer, event, cx| match event {
            ComposerEvent::Submit(text) => workbench.start_turn(text.clone(), cx),
        })
        .detach();

        let workbench = Self {
            project: None,
            buckets: Vec::new(),
            transcript: Vec::new(),
            sidecar,
            status: "idle — type a prompt and press Enter".to_string(),
            streaming: false,
            error: None,
            composer,
        };
        // Populate the spine if a backend is already listening. This does not
        // start one — see `Sidecar::fetch_project`.
        workbench.refresh_project(cx);
        workbench
    }

    /// Pull the project spine in the background and swap it in when it arrives.
    fn refresh_project(&self, cx: &mut Context<Self>) {
        let mut results = self.sidecar.fetch_project();
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = results.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match outcome {
                        Ok(project) => workbench.project = Some(project),
                        // A missing spine is not worth interrupting the user for —
                        // the panel already shows an honest placeholder.
                        Err(error) => tracing::debug!(%error, "could not load the project spine"),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Kick off one coordinator turn and pump its events into the transcript.
    fn start_turn(&mut self, prompt: String, cx: &mut Context<Self>) {
        if self.streaming || prompt.trim().is_empty() {
            return;
        }
        self.streaming = true;
        self.error = None;
        self.status = "starting…".into();
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(true, cx));
        self.transcript.push(Message::new("you", prompt.clone()));
        // The assistant message — text *and* activity — streams into this entry.
        self.transcript.push(Message::new("mini-me", String::new()));

        let mut events = self.sidecar.submit(prompt);
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                // `Err` here means the view is gone (window closed) — stop pumping.
                if this
                    .update(cx, |workbench, cx| {
                        workbench.apply(event, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    fn apply(&mut self, event: TurnEvent, cx: &mut Context<Self>) {
        match event {
            TurnEvent::Status(status) => self.status = status,
            TurnEvent::Token(text) => {
                if let Some(last) = self.transcript.last_mut() {
                    last.body.push_str(&text);
                }
            }
            // Activity attaches to the in-flight assistant message, so it sits with
            // the answer it produced instead of in a panel the user has to correlate.
            TurnEvent::Step { agent, label } => {
                if let Some(message) = self.transcript.last_mut() {
                    match agent {
                        None => message.steps.push(label),
                        Some(agent) => trace_for(message, &agent).steps.push(label),
                    }
                }
            }
            TurnEvent::SubagentToken { agent, text } => {
                if let Some(message) = self.transcript.last_mut() {
                    trace_for(message, &agent).push_text(&text);
                }
            }
            // Each `values` event is a *whole* snapshot, so replace rather than
            // merge. The spine rides along in the same payload, which keeps the
            // mission current without another HTTP round trip.
            TurnEvent::Snapshot(snapshot) => {
                if let Some(project) = snapshot.project {
                    self.project = Some(project);
                }
                if !snapshot.buckets.is_empty() {
                    self.buckets = snapshot.buckets;
                }
            }
            TurnEvent::Done => {
                self.streaming = false;
                self.finish_turn(cx);
                self.status = "done".into();
                if let Some(last) = self.transcript.last() {
                    if last.body.is_empty() {
                        self.status = "done — but no assistant text arrived".into();
                    }
                }
            }
            TurnEvent::Error(message) => {
                self.streaming = false;
                self.finish_turn(cx);
                self.status = "failed".into();
                // Point at the sidecar log: backend-side failures (a missing key,
                // a bad graph import) surface there, not in the HTTP error.
                self.error = Some(format!(
                    "{message} — sidecar log: {}",
                    self.sidecar.log_path()
                ));
            }
        }
    }

    /// A turn ended (either way): collapse its activity trace, drop the assistant
    /// placeholder if nothing at all arrived, and hand the field back to the user.
    fn finish_turn(&mut self, cx: &mut Context<Self>) {
        // While a turn runs the trace is the only sign of progress; once the answer
        // is there, the answer is the point.
        if let Some(message) = self.transcript.last_mut() {
            for trace in &mut message.agents {
                trace.expanded = false;
            }
        }
        if self
            .transcript
            .last()
            .is_some_and(|message| message.role == "mini-me" && message.is_silent())
        {
            self.transcript.pop();
        }
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(false, cx));
        // A turn can change the spine — the mission is derived from the first
        // question, and completed/pending shift as work lands.
        self.refresh_project(cx);
    }

    fn rail(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(64.))
            .h_full()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(div().p_3().text_color(rgb(ACCENT)).child("◎"))
    }

    fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // `min_w_0` is what makes long assistant text *wrap* instead of running off
        // the right edge: a flex item defaults to min-width:auto, so its content
        // width becomes its floor and a long paragraph widens the pane instead of
        // flowing down.
        // `id` + `overflow_y_scroll` is what lets a long transcript scroll; GPUI
        // keeps the scroll offset keyed on that id across re-renders.
        let mut col = div()
            .id("transcript")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3();

        if self.transcript.is_empty() {
            col = col.child(
                div()
                    .text_color(rgb(MUTED))
                    .child("No turns yet. Press Run to stream one from the local sidecar."),
            );
        }
        for (index, message) in self.transcript.iter().enumerate() {
            let label_color = if message.role == "you" { MUTED } else { ACCENT };
            let has_activity = !message.steps.is_empty() || !message.agents.is_empty();
            // An empty assistant body means we're still waiting on the first token —
            // unless a trace is already showing what's going on, which says more.
            let body = if message.body.is_empty() && self.streaming && !has_activity {
                "…".to_string()
            } else {
                message.body.clone()
            };
            let mut block = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_color(rgb(label_color))
                        .text_sm()
                        .child(message.role),
                );
            // The trace goes *above* the answer, because that is the order it
            // happened in and because the answer should be the last thing read.
            if has_activity {
                block = block.child(self.activity_block(index, message, cx));
            }
            if !body.is_empty() {
                col = col.child(block.child(div().w_full().text_color(rgb(TEXT)).child(body)));
            } else {
                col = col.child(block);
            }
        }

        div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .h_full()
            .child(col)
            .child(self.composer_row(cx))
            .child(self.status_bar())
    }

    /// The agent activity trace for one turn: coordinator steps as one-liners, then
    /// a collapsible group per subagent.
    ///
    /// This exists because a delegated turn is otherwise *silent*: the coordinator
    /// emits only a `task` tool call while a subagent does the real work, so the user
    /// sees a frozen window and then an answer with no account of where it came from
    /// (plan §15).
    fn activity_block(
        &self,
        message_index: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut block = div().flex().flex_col().w_full().min_w_0().gap_1();

        for step in &message.steps {
            block = block.child(step_line(step));
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
                .border_color(rgb(BORDER))
                .child(
                    div()
                        // Unique per (turn, trace) so GPUI keeps each group's click
                        // state to itself.
                        .id(SharedString::from(format!(
                            "trace-{message_index}-{trace_index}"
                        )))
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(ACCENT))
                        .text_xs()
                        .hover(|style| style.cursor_pointer())
                        .child(header)
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            if let Some(message) = workbench.transcript.get_mut(message_index) {
                                if let Some(trace) = message.agents.get_mut(trace_index) {
                                    trace.expanded = !trace.expanded;
                                }
                            }
                            cx.notify();
                        })),
                );

            if trace.expanded {
                for step in &trace.steps {
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
                            .text_color(rgb(MUTED))
                            .text_xs()
                            .child(preview),
                    );
                }
            }
            block = block.child(group);
        }

        block
    }

    /// The input row: the text field plus a Send affordance.
    fn composer_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (send_label, send_color) = if self.streaming {
            ("Streaming…", MUTED)
        } else {
            ("Send ⏎", ACCENT)
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .p_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(self.composer.clone())
            .child(
                div()
                    .id("send-turn")
                    .flex_none()
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(send_color))
                    .text_color(rgb(send_color))
                    .text_sm()
                    .child(send_label)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        // Same path as Enter. Calling the entity directly rather
                        // than dispatching an action keeps this working regardless
                        // of where focus is when the button is clicked.
                        workbench
                            .composer
                            .update(cx, |composer, cx| composer.submit_now(cx));
                    })),
            )
    }

    fn status_bar(&self) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), ERROR),
            None => (self.status.clone(), MUTED),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(
                div()
                    .flex_grow()
                    .min_w_0()
                    .text_color(rgb(status_color))
                    .text_sm()
                    .child(status_text),
            )
            .child(
                div()
                    .text_color(rgb(MUTED))
                    .text_sm()
                    .child(self.sidecar.base_url().to_string()),
            )
    }

    /// The project spine: mission, what's done, what's queued, what's suggested.
    fn artifacts_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("spine")
            .flex()
            .flex_col()
            .w(px(320.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(BORDER))
            .p_4()
            .gap_4()
            .child(section_label("RESEARCH PROJECT"));

        let Some(project) = &self.project else {
            // No spine yet, but a run may already be producing outputs — still show
            // them rather than an empty panel.
            return panel
                .child(
                    div()
                        .text_color(rgb(MUTED))
                        .text_sm()
                        .child("No project loaded yet. Run a turn — the mission is derived from your first question."),
                )
                .child(self.outputs_section());
        };

        panel = panel.child(if project.mission.is_empty() {
            div()
                .text_color(rgb(MUTED))
                .text_sm()
                .child("No mission yet — it comes from your first question.")
        } else {
            div()
                .w_full()
                .text_color(rgb(TEXT))
                .child(project.mission.clone())
        });

        if !project.completed.is_empty() {
            panel = panel.child(spine_list("COMPLETED", &project.completed, "✓"));
        }
        if !project.pending.is_empty() {
            panel = panel.child(spine_list("PENDING", &project.pending, "○"));
        }

        // Advisory only: shown so the user can choose to ask for one. Nothing here
        // auto-runs — org policy is human-gated.
        if !project.suggestions.is_empty() {
            let mut suggestions = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("SUGGESTED NEXT"));
            for (index, suggestion) in project.suggestions.iter().enumerate() {
                let prompt = suggestion.prompt.clone();
                suggestions = suggestions.child(
                    div()
                        .id(("suggestion", index))
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .p_2()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .hover(|style| style.border_color(rgb(ACCENT)).cursor_pointer())
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(TEXT))
                                .text_sm()
                                .child(suggestion.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(MUTED))
                                .text_xs()
                                .child(suggestion.rationale.clone()),
                        )
                        // Clicking *loads* the prompt into the composer; it never
                        // runs it. Suggestions are advisory and org policy is
                        // human-gated, so the user still presses Enter.
                        .on_click(cx.listener(move |workbench, _event, window, cx| {
                            if workbench.streaming || prompt.is_empty() {
                                return;
                            }
                            workbench.composer.update(cx, |composer, cx| {
                                composer.set_text(prompt.clone(), cx);
                            });
                            let focus = workbench.composer.focus_handle(cx);
                            window.focus(&focus);
                            workbench.status = "suggestion loaded — press Enter to run it".into();
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(suggestions);
        }

        if project.completed.is_empty() && project.pending.is_empty() {
            panel = panel.child(
                div()
                    .text_color(rgb(MUTED))
                    .text_xs()
                    .child("Completed and pending work will appear here as the project grows."),
            );
        }

        panel.child(self.outputs_section())
    }

    /// Research outputs from the current run, grouped by kind.
    ///
    /// Fed by the `values` stream event, so it fills in as a turn produces papers,
    /// datasets, theories and reports — not only at the end.
    fn outputs_section(&self) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(section_label("OUTPUTS"));

        if self.buckets.is_empty() {
            return section.child(
                div()
                    .text_color(rgb(MUTED))
                    .text_xs()
                    .child("Papers, datasets, theories and reports show up here as a turn produces them."),
            );
        }

        for bucket in &self.buckets {
            // Show a bounded number of titles — a literature search can return
            // dozens, and the count already conveys the scale.
            const MAX_SHOWN: usize = 4;
            let mut group = div().flex().flex_col().gap_1().child(
                div()
                    .text_color(rgb(TEXT))
                    .text_sm()
                    .child(format!("{} · {}", bucket.name, bucket.items.len())),
            );
            for item in bucket.items.iter().take(MAX_SHOWN) {
                group = group.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(MUTED))
                        .text_xs()
                        .child(item.clone()),
                );
            }
            if bucket.items.len() > MAX_SHOWN {
                group = group.child(
                    div()
                        .text_color(rgb(MUTED))
                        .text_xs()
                        .child(format!("+{} more", bucket.items.len() - MAX_SHOWN)),
                );
            }
            section = section.child(group);
        }

        section
    }
}

impl Render for Workbench {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.rail())
            .child(self.chat_pane(cx))
            .child(self.artifacts_panel(cx))
    }
}

/// Decode a whole captured SSE stream into the transcript state it would produce.
///
/// Shared by `--replay` and the fixture test, so both exercise the same path the
/// window does: frame → decode → transcript, with nothing simulated in between.
fn decode_capture(raw: &[u8], mut on_status: impl FnMut(&str)) -> (Message, Vec<Bucket>) {
    let mut frames = protocol::SseDecoder::default();
    let mut turn = protocol::TurnDecoder::default();
    let mut message = Message::new("mini-me", String::new());
    let mut outputs: Vec<Bucket> = Vec::new();

    // One push: the framer handles the split, exactly as it does off the socket.
    for frame in frames.push(raw) {
        for event in turn.push(&frame) {
            match event {
                TurnEvent::Token(text) => message.body.push_str(&text),
                TurnEvent::Step { agent, label } => match agent {
                    None => message.steps.push(label),
                    Some(agent) => trace_for(&mut message, &agent).steps.push(label),
                },
                TurnEvent::SubagentToken { agent, text } => {
                    trace_for(&mut message, &agent).push_text(&text);
                }
                TurnEvent::Snapshot(snapshot) => {
                    if !snapshot.buckets.is_empty() {
                        outputs = snapshot.buckets;
                    }
                }
                TurnEvent::Status(status) => on_status(&status),
                TurnEvent::Error(error) => on_status(&format!("error: {error}")),
                TurnEvent::Done => {}
            }
        }
    }
    (message, outputs)
}

/// Replay a captured SSE stream and print what the transcript would show. No
/// backend, no window, no tokens spent.
///
/// The activity trace is the one feature whose input is 500 events of a real
/// delegation, so being able to re-run a saved capture is the difference between
/// testing it and paying for a research turn every time the decoder changes.
fn replay(path: &str) -> anyhow::Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("could not read {path}"))?;
    let (message, outputs) = decode_capture(&raw, |status| println!("status   : {status}"));

    println!("\n--- activity ---");
    for step in &message.steps {
        println!("· {step}");
    }
    for trace in &message.agents {
        println!(
            "▾ {} · {} step(s) · {} chars   [{}]",
            trace.name,
            trace.steps.len(),
            trace.text.chars().count(),
            trace.ns,
        );
        for step in &trace.steps {
            println!("    · {step}");
        }
        println!("    {}", protocol::summarize_agent_result(&trace.text));
    }
    println!("\n--- outputs ---");
    for bucket in &outputs {
        println!("{} · {}", bucket.name, bucket.items.len());
    }
    println!("\n--- assistant text ---\n{}", message.body.trim());

    anyhow::ensure!(
        !message.steps.is_empty() || !message.agents.is_empty(),
        "the capture decoded no activity at all — did `stream_subgraphs` get dropped?"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real delegated turn, reduced to fit the repo (see the fixture's header).
    /// Replaying it is what proves the trace works on *measured* wire data rather
    /// than on shapes hand-written from the docs.
    const DELEGATED_TURN: &[u8] = include_bytes!("../tests/fixtures/delegated-turn.sse");

    #[test]
    fn a_real_delegated_turn_produces_one_named_trace_with_its_steps() {
        let mut statuses = Vec::new();
        let (message, outputs) = decode_capture(DELEGATED_TURN, |status| {
            statuses.push(status.to_string())
        });

        // The coordinator's own line: one delegation, announced once, labelled from
        // arguments that arrived across 60 fragments.
        assert_eq!(
            message.steps,
            vec![
                "delegating to academic_researcher — Find the canonical DESeq2 paper. Return a concise citation…"
            ]
        );

        // One group, named by the backend, with the subagent's real tool call in it.
        let [trace] = message.agents.as_slice() else {
            panic!("expected exactly one subagent group, got {}", message.agents.len());
        };
        assert_eq!(trace.name, "academic_researcher");
        assert!(trace.ns.starts_with("tools:"), "{}", trace.ns);
        assert_eq!(trace.steps, vec!["search_paper_by_title"]);

        // Its answer was a JSON object, so the trace shows the readable part.
        let preview = protocol::summarize_agent_result(&trace.text);
        assert!(preview.starts_with("The canonical DESeq2 paper"), "{preview}");
        assert!(preview.ends_with("· 1 sources"), "{preview}");

        // The coordinator's answer still arrives, and the outputs panel still fills:
        // subagent frames must not be mistaken for either.
        assert!(message.body.contains("Genome Biology"), "{}", message.body);
        assert_eq!(
            outputs.iter().map(|b| (b.name, b.items.len())).collect::<Vec<_>>(),
            vec![("sources", 1)]
        );

        // Sandbox provisioning reaches the status line — the first turn on a cold
        // thread waits on it, and without this the window looks stuck.
        assert!(
            statuses.iter().any(|status| status == "Creating sandbox…"),
            "{statuses:?}"
        );
    }

    #[test]
    fn a_purely_delegated_turn_is_not_discarded_as_empty() {
        // The web client filters tool-call-only assistant messages out of the
        // transcript, which is precisely why a delegation there renders as nothing.
        // Activity has to count as content or we would reproduce the same silence.
        let mut message = Message::new("mini-me", String::new());
        assert!(message.is_silent());
        message.steps.push("delegating to report_writer".into());
        assert!(!message.is_silent());
    }

    #[test]
    fn a_trace_keeps_the_newest_text_when_it_overflows() {
        let mut trace = AgentTrace {
            ns: "tools:a".into(),
            name: "academic_researcher".into(),
            steps: Vec::new(),
            text: String::new(),
            expanded: true,
        };
        // Multi-byte on purpose: the cap counts characters, so a naive byte slice
        // would split one and panic.
        trace.push_text(&"á".repeat(MAX_TRACE_CHARS));
        trace.push_text("tail");
        assert!(trace.text.ends_with("tail"));
        assert!(trace.text.starts_with('…'));
        assert!(trace.text.chars().count() <= MAX_TRACE_CHARS + 1);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // `--replay <capture>` needs no backend at all, so it runs before one is
    // configured.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(path) = args.iter().position(|a| a == "--replay") {
        let Some(capture) = args.get(path + 1) else {
            eprintln!("--replay needs a path to a captured SSE stream");
            std::process::exit(2);
        };
        if let Err(error) = replay(capture) {
            eprintln!("\nreplay: FAIL — {error:#}");
            std::process::exit(1);
        }
        println!("\nreplay: PASS");
        return;
    }

    let config = backend::BackendConfig::default();
    tracing::info!(
        location = %config.location(),
        url = %config.base_url(),
        "backend sidecar configured"
    );
    if !config.looks_like_backend_repo() {
        tracing::warn!(
            dir = %config.project_dir.display(),
            "no langgraph.json found — set MINIME_BACKEND_DIR to the Mini-Me checkout"
        );
    }
    let sidecar = Arc::new(Sidecar::new(config).expect("failed to build the sidecar runtime"));

    // `--check-backend [--stream]` exercises the sidecar without a window, so the
    // client/backend contract can be verified on a headless machine.
    if args.iter().any(|a| a == "--check-backend") {
        // `--stream` runs the seed prompt; `--prompt "…"` runs your own, which is how
        // a delegating turn (and so the activity trace) gets verified headlessly.
        let custom = args
            .iter()
            .position(|a| a == "--prompt")
            .and_then(|at| args.get(at + 1))
            .map(String::as_str);
        let prompt = match (custom, args.iter().any(|a| a == "--stream")) {
            (Some(prompt), _) => Some(prompt),
            (None, true) => Some(SEED_PROMPT),
            (None, false) => None,
        };
        let outcome = sidecar.check(prompt);
        let failed = match &outcome {
            Ok(()) => {
                println!("\nbackend check: PASS");
                false
            }
            Err(error) => {
                eprintln!("\nbackend check: FAIL — {error:#}");
                true
            }
        };
        // `process::exit` skips destructors, which would leak the spawned
        // backend. Drop the sidecar (shutting the child down) *before* exiting.
        drop(sidecar);
        if failed {
            std::process::exit(1);
        }
        return;
    }

    Application::new().run(move |cx: &mut App| {
        // Without these the composer receives no editing keys at all — GPUI
        // dispatches actions, and nothing binds to them by default.
        cx.bind_keys(composer::key_bindings());

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_window, cx| cx.new(|cx| Workbench::new(sidecar.clone(), cx)),
            )
            .expect("failed to open window");

        // Focus the composer so the user can type immediately on launch.
        window
            .update(cx, |workbench, window, cx| {
                let composer = workbench.composer.focus_handle(cx);
                window.focus(&composer);
            })
            .expect("failed to focus the composer");

        cx.activate(true);
    });
}
