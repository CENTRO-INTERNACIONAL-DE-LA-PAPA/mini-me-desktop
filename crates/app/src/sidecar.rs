//! Bridges the async backend to the GPUI main thread.
//!
//! GPUI runs its own executor and `reqwest` needs a Tokio reactor, so rather
//! than mixing runtimes we keep a Tokio runtime here and hand results back over
//! a `futures` channel — which is executor-agnostic, so GPUI can await it
//! directly. The UI never blocks on HTTP.
//!
//! Lifetime note: the runtime and the supervised child process live in `Sidecar`,
//! which the root view holds for the whole session. Individual turns are just
//! tasks on that runtime, so ending a turn never kills the backend.

use std::sync::Arc;

use anyhow::Result;
use futures::channel::mpsc;
use tokio::sync::Mutex;

use crate::backend::{BackendConfig, BackendSupervisor};
use crate::protocol::{LangGraphClient, Project, TurnEvent};

/// Owns the Tokio runtime and the supervised backend process.
pub struct Sidecar {
    runtime: tokio::runtime::Runtime,
    supervisor: Arc<Mutex<BackendSupervisor>>,
    base_url: String,
    log_path: String,
}

impl Sidecar {
    pub fn new(config: BackendConfig) -> Result<Self> {
        let base_url = config.base_url();
        let log_path = config.log_path.display().to_string();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mini-me-sidecar")
            .build()?;
        Ok(Self {
            runtime,
            supervisor: Arc::new(Mutex::new(BackendSupervisor::new(config))),
            base_url,
            log_path,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Where the sidecar's own logs land — the first place to look when a turn
    /// fails for reasons the HTTP layer can't explain.
    pub fn log_path(&self) -> &str {
        &self.log_path
    }

    /// Start one coordinator turn. Returns the receiving end of its event
    /// stream; the caller drives it from the UI thread.
    pub fn submit(&self, prompt: String) -> mpsc::UnboundedReceiver<TurnEvent> {
        let (tx, rx) = mpsc::unbounded();
        let supervisor = self.supervisor.clone();
        let base_url = self.base_url.clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            // Send failures just mean the UI dropped the receiver (window closed).
            let emit = |event: TurnEvent| {
                let _ = tx.unbounded_send(event);
            };

            if let Err(error) = run_turn(&client, &supervisor, &prompt, &emit).await {
                // `{:#}` includes the anyhow context chain, which is where the
                // actionable part of these failures lives.
                emit(TurnEvent::Error(format!("{error:#}")));
                return;
            }
            emit(TurnEvent::Done);
        });

        rx
    }

    /// Fetch the project spine. Returns the receiver of a one-shot result so the
    /// UI thread never blocks on HTTP.
    ///
    /// Deliberately does **not** start the backend: the spine is decoration, and
    /// spawning a sidecar as a side effect of rendering a panel would be
    /// surprising. If nothing is listening yet this just reports an error the
    /// caller can ignore.
    pub fn fetch_project(&self) -> mpsc::UnboundedReceiver<Result<Project, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .fetch_project()
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });

        rx
    }

    /// Headless check of the backend path — no GPUI, no window. Verifies the
    /// sidecar comes up and a thread can be created; with `stream` it also runs
    /// one real coordinator turn (which calls the model, so it costs tokens).
    ///
    /// Exists so the whole client/backend contract can be exercised on a
    /// headless machine, where no window can be opened.
    pub fn check(&self, stream: bool) -> Result<()> {
        let supervisor = self.supervisor.clone();
        let base_url = self.base_url.clone();
        println!("url      : {base_url}");
        println!("log      : {}", self.log_path);
        self.runtime.block_on(async move {
            let client = LangGraphClient::new(base_url);
            let mut supervisor = supervisor.lock().await;
            let status = supervisor.ensure_running(&client).await?;
            println!("health   : ok ({status})");

            let thread_id = client.create_thread().await?;
            println!("thread   : {thread_id}");

            // The spine panel depends on this custom route, so cover it here too —
            // a decode change would otherwise only show up as an empty panel.
            match client.fetch_project().await {
                Ok(project) => println!(
                    "project  : mission {} · {} completed · {} pending · {} suggestion(s)",
                    if project.mission.is_empty() {
                        "unset"
                    } else {
                        "set"
                    },
                    project.completed.len(),
                    project.pending.len(),
                    project.suggestions.len(),
                ),
                Err(error) => println!("project  : unavailable — {error:#}"),
            }

            if !stream {
                println!("stream   : skipped (pass --stream to run a real turn)");
                return Ok(());
            }

            let mut text = String::new();
            let mut chunks = 0usize;
            client
                .stream_turn(&thread_id, super::SEED_PROMPT, |event| match event {
                    TurnEvent::Token(token) => {
                        chunks += 1;
                        text.push_str(&token);
                    }
                    TurnEvent::Status(status) => println!("status   : {status}"),
                    // Covers the `values` decode path, so an artifacts-shape
                    // regression fails the headless check instead of quietly
                    // emptying the outputs panel.
                    TurnEvent::Snapshot(snapshot) => {
                        let summary: Vec<String> = snapshot
                            .buckets
                            .iter()
                            .map(|bucket| format!("{} {}", bucket.items.len(), bucket.name))
                            .collect();
                        println!(
                            "outputs  : {}",
                            if summary.is_empty() {
                                "none yet".to_string()
                            } else {
                                summary.join(", ")
                            }
                        );
                    }
                    TurnEvent::Error(error) => println!("error    : {error}"),
                    TurnEvent::Done => {}
                })
                .await?;
            println!("stream   : {chunks} chunk(s), {} chars", text.len());
            println!("--- assistant text ---\n{}", text.trim());
            anyhow::ensure!(!text.trim().is_empty(), "no assistant text was streamed");
            Ok(())
        })
    }
}

async fn run_turn(
    client: &LangGraphClient,
    supervisor: &Arc<Mutex<BackendSupervisor>>,
    prompt: &str,
    emit: &impl Fn(TurnEvent),
) -> Result<()> {
    emit(TurnEvent::Status("checking backend…".into()));
    {
        let mut supervisor = supervisor.lock().await;
        let status = supervisor.ensure_running(client).await?;
        emit(TurnEvent::Status(status));
    }

    emit(TurnEvent::Status("creating thread…".into()));
    let thread_id = client.create_thread().await?;
    tracing::info!(%thread_id, "streaming coordinator turn");

    emit(TurnEvent::Status("streaming…".into()));
    client.stream_turn(&thread_id, prompt, emit).await?;
    Ok(())
}
