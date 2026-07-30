//! Mini-Me Desktop — GPUI entry point and root workbench view.
//!
//! P6.0 SKETCH. This lays out the *shape* of the workbench — a left rail, a
//! central chat pane, and a right artifacts/spine panel — as a single GPUI
//! `Render` view with hard-coded content, so P6.1 can turn it into a real
//! window on a Rust machine.
//!
//! ⚠️ GPUI has no stable published API. The calls below follow GPUI's recent
//! `Application`/`Context`/`Render` shape, but **must be reconciled against the
//! `examples/` in the pinned `gpui` rev** (see crates/app/Cargo.toml). Treat
//! compile errors here as P6.1's first task, not a surprise — this file is a
//! starting point, not verified code.

mod backend;

use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window,
    WindowBounds, WindowOptions,
};

// ---- Palette (placeholder; align with the web app's tokens in P6.3) --------
const BG: u32 = 0x1e1e22;
const PANEL: u32 = 0x26262b;
const BORDER: u32 = 0x3a3a42;
const TEXT: u32 = 0xe8e8ea;
const MUTED: u32 = 0x9a9aa2;
const ACCENT: u32 = 0xe8703a; // Mini-Me orange

/// A single chat message in the transcript (placeholder shape).
struct Message {
    role: &'static str,
    body: String,
}

/// Root view: the three-pane research workbench.
struct Workbench {
    mission: String,
    transcript: Vec<Message>,
}

impl Workbench {
    fn new() -> Self {
        Self {
            mission: "Whether coffea canephora or eugenioides gave heat-shock \
                      resistant features to coffea arabica."
                .to_string(),
            transcript: vec![
                Message { role: "you", body: "Search the literature for recent work on the mission.".into() },
                Message { role: "mini-me", body: "Streaming a real coordinator turn lands in P6.2.".into() },
            ],
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

    fn chat_pane(&self) -> impl IntoElement {
        let mut col = div().flex().flex_col().flex_grow().h_full().p_4().gap_3();
        for m in &self.transcript {
            let label_color = if m.role == "you" { MUTED } else { ACCENT };
            col = col.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_color(rgb(label_color)).text_sm().child(m.role))
                    .child(div().text_color(rgb(TEXT)).child(m.body.clone())),
            );
        }
        col
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
            .child(div().text_color(rgb(ACCENT)).text_sm().child("RESEARCH PROJECT"))
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(self.rail())
            .child(self.chat_pane())
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

    // P6.2: construct (but don't yet start) the local backend supervisor so the
    // window and the sidecar lifecycle live side by side.
    let _backend = backend::BackendSupervisor::new(backend::BackendConfig::default());

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| Workbench::new()),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
