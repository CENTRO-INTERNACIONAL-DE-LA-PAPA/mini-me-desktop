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
mod protocol;
mod sidecar;

use std::sync::Arc;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds,
    WindowOptions,
};

use protocol::TurnEvent;
use sidecar::Sidecar;

// ---- Palette (placeholder; align with the web app's tokens in P6.3) --------
const BG: u32 = 0x1e1e22;
const PANEL: u32 = 0x26262b;
const BORDER: u32 = 0x3a3a42;
const TEXT: u32 = 0xe8e8ea;
const MUTED: u32 = 0x9a9aa2;
const ACCENT: u32 = 0xe8703a; // Mini-Me orange
const ERROR: u32 = 0xe05252;

/// The seeded prompt for P6.2. A real composer (text input) lands in P6.3.
const SEED_PROMPT: &str = "In one short paragraph, what is your role as the Mini-Me coordinator?";

/// A single chat message in the transcript.
struct Message {
    role: &'static str,
    body: String,
}

/// Root view: the three-pane research workbench.
struct Workbench {
    mission: String,
    transcript: Vec<Message>,
    sidecar: Arc<Sidecar>,
    /// Status line text (backend/stream progress, not model output).
    status: String,
    /// True while a turn is in flight — gates the run button.
    streaming: bool,
    /// Set when the last turn failed, rendered in the status line.
    error: Option<String>,
}

impl Workbench {
    fn new(sidecar: Arc<Sidecar>) -> Self {
        Self {
            mission: "Whether coffea canephora or eugenioides gave heat-shock \
                      resistant features to coffea arabica."
                .to_string(),
            transcript: Vec::new(),
            sidecar,
            status: "idle — press Run to stream a coordinator turn".to_string(),
            streaming: false,
            error: None,
        }
    }

    /// Kick off one coordinator turn and pump its events into the transcript.
    fn start_turn(&mut self, cx: &mut Context<Self>) {
        if self.streaming {
            return;
        }
        self.streaming = true;
        self.error = None;
        self.status = "starting…".into();
        self.transcript.push(Message {
            role: "you",
            body: SEED_PROMPT.to_string(),
        });
        // The assistant message streams into this (initially empty) entry.
        self.transcript.push(Message {
            role: "mini-me",
            body: String::new(),
        });

        let mut events = self.sidecar.submit(SEED_PROMPT.to_string());
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                // `Err` here means the view is gone (window closed) — stop pumping.
                if this
                    .update(cx, |workbench, cx| {
                        workbench.apply(event);
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

    fn apply(&mut self, event: TurnEvent) {
        match event {
            TurnEvent::Status(status) => self.status = status,
            TurnEvent::Token(text) => {
                if let Some(last) = self.transcript.last_mut() {
                    last.body.push_str(&text);
                }
            }
            TurnEvent::Done => {
                self.streaming = false;
                self.status = "done".into();
                if let Some(last) = self.transcript.last() {
                    if last.body.is_empty() {
                        self.status = "done — but no assistant text arrived".into();
                    }
                }
            }
            TurnEvent::Error(message) => {
                self.streaming = false;
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
        let mut col = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .h_full()
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
            .h_full()
            .child(col)
            .child(self.status_bar(cx))
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), ERROR),
            None => (self.status.clone(), MUTED),
        };
        let button_label = if self.streaming {
            "Streaming…"
        } else {
            "Run coordinator turn"
        };
        let button_color = if self.streaming { MUTED } else { ACCENT };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .p_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(PANEL))
            .child(
                div()
                    .id("run-turn")
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(button_color))
                    .text_color(rgb(button_color))
                    .text_sm()
                    .child(button_label)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.start_turn(cx);
                    })),
            )
            .child(
                div()
                    .flex_grow()
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

    fn artifacts_panel(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(320.))
            .h_full()
            .bg(rgb(PANEL))
            .border_l_1()
            .border_color(rgb(BORDER))
            .p_4()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(ACCENT))
                    .text_sm()
                    .child("RESEARCH PROJECT"),
            )
            .child(div().text_color(rgb(TEXT)).child(self.mission.clone()))
            .child(
                div()
                    .text_color(rgb(MUTED))
                    .text_sm()
                    .child("Outputs / spine / plan panels port in P6.3."),
            )
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
            .child(self.artifacts_panel())
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
        dir = %config.project_dir.display(),
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
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| Workbench::new(sidecar.clone())),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
