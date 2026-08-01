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

use std::sync::{Arc, Mutex as SyncMutex};

use anyhow::{Context as _, Result};
use futures::channel::mpsc;
use tokio::sync::Mutex;

use crate::backend::{BackendConfig, BackendSupervisor};
use crate::protocol::{
    AgentRef, AsyncTask, Decision, Job, LangGraphClient, ModelChoice, Project, TurnEvent, TurnOutcome,
};

/// Find (or start) the tally row for one subagent invocation, keyed by namespace so
/// two concurrent runs of the same subagent type are counted separately.
fn entry<'a>(
    agents: &'a mut Vec<(String, String, usize, usize)>,
    agent: &AgentRef,
) -> &'a mut (String, String, usize, usize) {
    if let Some(index) = agents.iter().position(|row| row.0 == agent.ns) {
        return &mut agents[index];
    }
    agents.push((agent.ns.clone(), agent.name.clone(), 0, 0));
    agents.last_mut().expect("just pushed")
}

/// How often a background job is polled.
///
/// These runs take 5–40 minutes, and each poll shells out to the `asta` CLI inside the
/// sandbox — so this is about noticing within a reasonable time, not about latency.
/// Twenty seconds is roughly a hundred polls across the longest job, which is nothing.
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);

/// How often a *background worker's* thread is checked.
///
/// Faster than the Asta jobs above, because this poll is what surfaces an approval
/// request — and someone may be sitting in front of the app waiting to answer it.
const TASK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(4);

/// Progress from a setup fix the app is running on the user's behalf.
#[derive(Debug, Clone)]
pub enum FixEvent {
    Line(String),
    Finished { ok: bool, note: String },
}

/// The conversation's thread id: created on first use, then reused.
///
/// A plain `std::sync::Mutex` rather than a Tokio one because the guard is never
/// held across an `await` — and because the UI thread resets it synchronously.
type ThreadId = Arc<SyncMutex<Option<String>>>;

/// Owns the Tokio runtime and the supervised backend process.
pub struct Sidecar {
    runtime: tokio::runtime::Runtime,
    supervisor: Arc<Mutex<BackendSupervisor>>,
    /// One thread for the whole conversation. Until 2026-07-31 every turn created a
    /// *fresh* thread, which meant the coordinator had no memory of the previous
    /// question — a follow-up like "and its dataset?" started from nothing.
    thread: ThreadId,
    base_url: String,
    log_path: String,
    /// The user's model choice and key, from settings + the keychain. Attached to every
    /// client, since the backend resolves the model per request.
    /// Behind a lock so Settings can change it without a restart: `submit` clones it per
    /// turn, so the next turn simply uses the new one.
    model: SyncMutex<Option<ModelChoice>>,
    /// Where the agent's code runs, for the status bar. The user should be able to
    /// see at a glance that commands are landing on their own machine.
    execution: &'static str,
    /// A **redacted** copy of the configuration, for the preflight checks — they need
    /// to know where the backend runs, never what its credentials are.
    config: BackendConfig,
}

impl Sidecar {
    pub fn new(config: BackendConfig, model: Option<ModelChoice>) -> Result<Self> {
        let base_url = config.base_url();
        let log_path = config.log_path.display().to_string();
        let execution = config.execution_label();
        let redacted = config.redacted();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("mini-me-sidecar")
            .build()?;
        Ok(Self {
            runtime,
            supervisor: Arc::new(Mutex::new(BackendSupervisor::new(config))),
            thread: Arc::new(SyncMutex::new(None)),
            model: SyncMutex::new(model),
            base_url,
            log_path,
            execution,
            config: redacted,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// A dropped file's path, as the backend would have to open it.
    pub fn path_for_backend(&self, path: &std::path::Path) -> String {
        self.config.path_for_backend(path)
    }

    pub fn execution(&self) -> &'static str {
        self.execution
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
        let thread = self.thread.clone();
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url).with_model(model);
            // Send failures just mean the UI dropped the receiver (window closed).
            let mut emit = |event: TurnEvent| {
                let _ = tx.unbounded_send(event);
            };

            match run_turn(&client, &supervisor, &thread, &prompt, &mut emit).await {
                // A paused run is *not* done: the UI keeps the turn open, shows the
                // command, and calls `resume` with the person's decision. Emitting
                // `Done` here would close the turn and strand the run forever.
                Ok(TurnOutcome::AwaitingApproval) => {}
                Ok(TurnOutcome::Finished) => emit(TurnEvent::Done),
                // `{:#}` includes the anyhow context chain, which is where the
                // actionable part of these failures lives.
                Err(error) => emit(TurnEvent::Error(format!("{error:#}"))),
            }
        });

        rx
    }

    /// Swap the model/key the next turn will use. No restart, because the backend
    /// resolves the model per request.
    pub fn set_model(&self, model: Option<ModelChoice>) {
        *self.model.lock().expect("model mutex") = model;
    }

    /// Answer a paused run's approval request and stream what follows.
    ///
    /// Runs on the conversation's existing thread, so the continuation lands in the
    /// same turn rather than starting a new one.
    pub fn resume(&self, decisions: Vec<Decision>) -> mpsc::UnboundedReceiver<TurnEvent> {
        let (tx, rx) = mpsc::unbounded();
        let thread = self.thread.clone();
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url).with_model(model);
            let mut emit = |event: TurnEvent| {
                let _ = tx.unbounded_send(event);
            };
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                emit(TurnEvent::Error(
                    "there is no thread to resume — the run was already lost".into(),
                ));
                return;
            };
            match client.resume_turn(&thread_id, &decisions, &mut emit).await {
                // A second gate in the same turn is normal: an agent often runs several
                // commands, and each one stops here.
                Ok(TurnOutcome::AwaitingApproval) => {}
                Ok(TurnOutcome::Finished) => emit(TurnEvent::Done),
                Err(error) => emit(TurnEvent::Error(format!("{error:#}"))),
            }
        });

        rx
    }

    /// The thread this conversation is on, once a turn has created one.
    ///
    /// `None` before the first turn. The workspace directory is named after it, so this is
    /// what lets the app find the files a conversation produced.
    pub fn thread_id(&self) -> Option<String> {
        self.thread.lock().expect("thread id mutex").clone()
    }

    /// Forget the current thread, so the next turn starts a fresh conversation.
    ///
    /// The backend keeps the old thread — nothing is deleted; we simply stop adding
    /// to it. The project spine is thread-independent, so the mission survives.
    pub fn reset_thread(&self) {
        self.thread.lock().expect("thread id mutex").take();
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

    /// Watch one long job until it stops moving, reporting every status change.
    ///
    /// **Outlives the turn that started it.** That is the whole point: the theorizer and
    /// DataVoyager submit with `--no-wait` and hand back a task id, so the conversation is
    /// free again immediately — but nothing was collecting the result. Polling is also
    /// what makes a finished run *durable*, since the route persists its output into the
    /// sandbox on a terminal state and nothing else does (docs §29).
    ///
    /// Runs on the Tokio runtime, which lives as long as the window. Closing the window
    /// ends the poll; the job itself continues on Asta's service and can be picked up
    /// again by task id.
    pub fn watch_job(&self, job: Job) -> mpsc::UnboundedReceiver<Job> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let mut job = job;
            loop {
                tokio::time::sleep(POLL_INTERVAL).await;
                // Read the thread each time rather than capturing it: "New thread" can
                // change it, and polling the old one would ask about a task that thread
                // no longer knows.
                let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                    return;
                };
                match client.poll_job(&thread_id, &job).await {
                    Ok(status) => {
                        if status == job.status {
                            continue;
                        }
                        job.status = status;
                        let finished = job.is_finished();
                        if tx.unbounded_send(job.clone()).is_err() {
                            return; // the window went away
                        }
                        if finished {
                            return;
                        }
                    }
                    // Transport failures are expected — the sidecar may be restarting, or
                    // a turn may be saturating it. Keep waiting rather than declaring a
                    // 20-minute job dead over one refused connection.
                    Err(error) => {
                        tracing::debug!(task = %job.task_id, %error, "job poll failed; retrying")
                    }
                }
            }
        });

        rx
    }

    /// Watch a background worker's thread: progress, and the moment it needs a person.
    ///
    /// **This is what makes background work usable at all.** The worker inherits the same
    /// `execute` gate as the foreground agent (the overlay wraps one `create_deep_agent`
    /// for both), but it runs on its own thread — and the client only ever resumed the
    /// conversation's. So the first command a background task tried to run stopped it
    /// dead, waiting for an approval nothing could deliver. It simply looked hung.
    ///
    /// Polled faster than the Asta jobs because a person may be sitting in front of it
    /// waiting to say yes.
    pub fn watch_task(&self, task: AsyncTask) -> mpsc::UnboundedReceiver<AsyncTask> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let mut task = task;
            loop {
                tokio::time::sleep(TASK_POLL_INTERVAL).await;
                match client.thread_state(&task.thread_id).await {
                    Ok(state) => {
                        let changed = state.status != task.status
                            || state.pending != task.pending
                            || state.error != task.error
                            || state.activity != task.activity;
                        task.status = state.status;
                        task.pending = state.pending;
                        task.error = state.error;
                        task.activity = state.activity;
                        if !changed {
                            continue;
                        }
                        let finished = task.is_finished();
                        if tx.unbounded_send(task.clone()).is_err() {
                            return;
                        }
                        if finished {
                            return;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(thread = %task.thread_id, %error, "task poll failed; retrying")
                    }
                }
            }
        });

        rx
    }

    /// Answer a background worker's approval request on its own thread.
    pub fn decide_task(&self, thread_id: String, decisions: Vec<Decision>) {
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url).with_model(model);
            if let Err(error) = client.resume_background(&thread_id, &decisions).await {
                tracing::error!(%thread_id, %error, "could not answer a background task");
            }
        });
    }

    /// Run the first-run checks off the UI thread.
    ///
    /// `spawn_blocking`, not `spawn`: every probe is a synchronous `Command` that can
    /// take seconds (a cold WSL distro), and blocking a reactor worker would stall any
    /// turn sharing it.
    ///
    /// `has_model_key` is decided by the caller on the main thread — see
    /// [`crate::preflight::inspect`] for why a keychain read must not happen here.
    pub fn preflight(
        &self,
        has_model_key: bool,
    ) -> mpsc::UnboundedReceiver<crate::preflight::Report> {
        let (tx, rx) = mpsc::unbounded();
        let config = self.config.clone();
        self.runtime.spawn(async move {
            let report =
                tokio::task::spawn_blocking(move || crate::preflight::inspect(&config, has_model_key))
                    .await;
            match report {
                Ok(report) => {
                    let _ = tx.unbounded_send(report);
                }
                // A panicking probe would otherwise leave the pane saying "checking…"
                // forever, which is the one thing a first-run diagnosis must not do.
                Err(error) => tracing::error!(%error, "the preflight checks panicked"),
            }
        });
        rx
    }

    /// Run one setup fix, streaming its output to the pane.
    ///
    /// `spawn_blocking` again, and for a much longer stay than the probes: provisioning
    /// clones a repository and syncs the scientific stack, so this task can live for
    /// minutes. It must not sit on a reactor worker that a turn also needs.
    pub fn run_fix(&self, argv: Vec<String>) -> mpsc::UnboundedReceiver<FixEvent> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let emit = tx.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                crate::preflight::run_streaming(&argv, |line| {
                    let _ = emit.unbounded_send(FixEvent::Line(line));
                })
            })
            .await;
            let event = match outcome {
                Ok(Ok(true)) => FixEvent::Finished {
                    ok: true,
                    note: "finished".into(),
                },
                Ok(Ok(false)) => FixEvent::Finished {
                    ok: false,
                    note: "the command reported a failure — the last lines say why".into(),
                },
                Ok(Err(error)) => FixEvent::Finished {
                    ok: false,
                    note: format!("{error:#}"),
                },
                // A panic here would otherwise leave the pane showing "running…" with no
                // way out except restarting the app.
                Err(error) => FixEvent::Finished {
                    ok: false,
                    note: format!("the fix crashed: {error}"),
                },
            };
            let _ = tx.unbounded_send(event);
        });
        rx
    }

    /// Headless check of the backend path — no GPUI, no window. Verifies the
    /// sidecar comes up and a thread can be created; with `prompt` it also runs one
    /// real coordinator turn (which calls the model, so it costs tokens). Pass a
    /// prompt that delegates to exercise the activity trace end to end.
    ///
    /// Exists so the whole client/backend contract can be exercised on a
    /// headless machine, where no window can be opened.
    pub fn check(&self, prompts: &[&str]) -> Result<()> {
        let supervisor = self.supervisor.clone();
        let thread = self.thread.clone();
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();
        println!("url      : {base_url}");
        println!("log      : {}", self.log_path);
        self.runtime.block_on(async move {
            let client = LangGraphClient::new(base_url).with_model(model);
            // Scoped: `run_turn` locks the supervisor itself, so holding it here
            // would deadlock the first turn.
            {
                let mut supervisor = supervisor.lock().await;
                let status = supervisor.ensure_running(&client).await?;
                println!("health   : ok ({status})");
            }

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

            if prompts.is_empty() {
                println!("stream   : skipped (pass --stream to run a real turn)");
                return Ok(());
            }

            // Every prompt goes through `run_turn` — the *same* function the window
            // uses — so this covers thread reuse, not just the HTTP surface. Passing
            // two prompts is how multi-turn continuity gets checked headlessly.
            for (index, prompt) in prompts.iter().enumerate() {
                println!("\nturn {}   : {prompt}", index + 1);
                let mut text = String::new();
                let mut chunks = 0usize;
                // (namespace, display name, steps, characters) per subagent
                // invocation, so a regression in the activity trace fails the check
                // instead of quietly emptying a panel nobody can see on this machine.
                let mut agents: Vec<(String, String, usize, usize)> = Vec::new();

                // A `RefCell` because the event handler and the resume loop both need
                // it, and the handler holds its borrow for as long as it lives.
                let pending: std::cell::RefCell<Vec<Decision>> = Default::default();
                let mut handle = |event: TurnEvent| match event {
                    TurnEvent::Token(token) => {
                        chunks += 1;
                        text.push_str(&token);
                    }
                    TurnEvent::Status(status) => println!("status   : {status}"),
                    TurnEvent::Step { agent, label } => match agent {
                        Some(agent) => {
                            println!("step     : {} · {label}", agent.name);
                            entry(&mut agents, &agent).2 += 1;
                        }
                        None => println!("step     : {label}"),
                    },
                    TurnEvent::SubagentToken { agent, text } => {
                        entry(&mut agents, &agent).3 += text.len();
                    }
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
                    // Headless runs approve automatically — a check with no window
                    // cannot ask anyone, and refusing would make the gate untestable
                    // here. The window always asks.
                    TurnEvent::Approval(request) => {
                        for action in &request.actions {
                            println!(
                                "approve  : {} — {}",
                                action.tool,
                                action.detail.replace('\n', " ⏎ ")
                            );
                            pending.borrow_mut().push(Decision::Approve);
                        }
                    }
                    TurnEvent::Error(error) => println!("error    : {error}"),
                    TurnEvent::Done => {}
                };

                let mut outcome =
                    run_turn(&client, &supervisor, &thread, prompt, &mut handle).await?;
                while outcome == TurnOutcome::AwaitingApproval {
                    let decisions: Vec<Decision> = pending.borrow_mut().drain(..).collect();
                    anyhow::ensure!(
                        !decisions.is_empty(),
                        "the run paused but no approval request was decoded"
                    );
                    // The thread was created (or reused) by `run_turn` above.
                    let thread_id = thread
                        .lock()
                        .expect("thread id mutex")
                        .clone()
                        .context("the run paused but no thread was recorded")?;
                    outcome = client.resume_turn(&thread_id, &decisions, &mut handle).await?;
                }

                println!("stream   : {chunks} chunk(s), {} chars", text.len());
                if agents.is_empty() {
                    println!("activity : no subagent ran on this prompt");
                }
                for (_, name, steps, chars) in &agents {
                    println!("activity : {name} · {steps} step(s) · {chars} chars");
                }
                println!("--- assistant text ---\n{}", text.trim());
                anyhow::ensure!(!text.trim().is_empty(), "no assistant text was streamed");
            }
            // One thread across every prompt is the point — a second `create_thread`
            // would mean the coordinator forgot the first question.
            println!(
                "\nthread   : {} (reused across {} turn(s))",
                thread
                    .lock()
                    .expect("thread id mutex")
                    .clone()
                    .unwrap_or_else(|| "none".into()),
                prompts.len(),
            );
            Ok(())
        })
    }
}

async fn run_turn(
    client: &LangGraphClient,
    supervisor: &Arc<Mutex<BackendSupervisor>>,
    thread: &ThreadId,
    prompt: &str,
    emit: &mut impl FnMut(TurnEvent),
) -> Result<TurnOutcome> {
    emit(TurnEvent::Status("checking backend…".into()));
    {
        let mut supervisor = supervisor.lock().await;
        let status = supervisor.ensure_running(client).await?;
        emit(TurnEvent::Status(status));
    }

    // Reuse the conversation's thread; only create one when there isn't one yet.
    // The guard is dropped before the await, so no lock is held across it.
    let existing = thread.lock().expect("thread id mutex").clone();
    let thread_id = match existing {
        Some(thread_id) => thread_id,
        None => {
            emit(TurnEvent::Status("creating thread…".into()));
            let thread_id = client.create_thread().await?;
            *thread.lock().expect("thread id mutex") = Some(thread_id.clone());
            thread_id
        }
    };
    tracing::info!(%thread_id, "streaming coordinator turn");

    emit(TurnEvent::Status("streaming…".into()));
    client.stream_turn(&thread_id, prompt, emit).await
}
