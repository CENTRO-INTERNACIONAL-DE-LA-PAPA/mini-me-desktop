//! Mini-Me Desktop — GPUI entry point and root workbench view.
//!
//! P6.2. The three-pane workbench (rail / chat / artifacts) now streams a **real
//! coordinator turn** from the local Python sidecar: `Sidecar` spawns and
//! health-checks the backend, and assistant tokens land in the transcript as
//! they arrive over SSE.
//!
//! Built against the published `gpui 0.2.2` (see crates/app/Cargo.toml). Rich
//! rendering (markdown, artifacts, spine) is P6.3.

mod backend;
mod composer;
mod protocol;
mod sidecar;

use std::sync::Arc;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Entity, Focusable, Window,
    WindowBounds, WindowOptions,
};

use composer::{Composer, ComposerEvent};
use protocol::{Bucket, Project, TurnEvent};
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

/// A single chat message in the transcript.
struct Message {
    role: &'static str,
    body: String,
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
        self.transcript.push(Message {
            role: "you",
            body: prompt.clone(),
        });
        // The assistant message streams into this (initially empty) entry.
        self.transcript.push(Message {
            role: "mini-me",
            body: String::new(),
        });

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

    /// A turn ended (either way): drop the empty assistant placeholder if no
    /// token ever arrived, and hand the field back to the user.
    fn finish_turn(&mut self, cx: &mut Context<Self>) {
        if self
            .transcript
            .last()
            .is_some_and(|message| message.role == "mini-me" && message.body.is_empty())
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
        for message in &self.transcript {
            let label_color = if message.role == "you" { MUTED } else { ACCENT };
            // An empty assistant body means we're still waiting on first token.
            let body = if message.body.is_empty() && self.streaming {
                "…".to_string()
            } else {
                message.body.clone()
            };
            col = col.child(
                div()
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
                    )
                    .child(div().w_full().text_color(rgb(TEXT)).child(body)),
            );
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check-backend") {
        let stream = args.iter().any(|a| a == "--stream");
        let outcome = sidecar.check(stream);
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
