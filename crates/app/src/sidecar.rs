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

use crate::backend::{BackendConfig, BackendSupervisor, Started};
use crate::preflight::Cancel;
use crate::protocol::urlencode;
use crate::references;
use crate::protocol::{
    AgentRef, Answer, AsyncTask, Conversation, Decision, Job, LangGraphClient, ModelChoice,
    Project,
    TurnEvent, TurnOutcome,
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

/// Which managed folder accompanies a confirmed server-side deletion.
///
/// Kept with the request so deleting the durable thread and deleting its files cannot drift into
/// two unrelated click handlers. They still cannot be atomic across HTTP and a disk, so the
/// server goes first: a rejected request preserves the files; a failed cleanup after success is
/// returned as an orphan the researcher can recover manually (§155).
#[derive(Clone, Debug)]
pub enum DeleteFiles {
    Conversation {
        project: Option<String>,
        thread_id: String,
    },
    Project {
        name: String,
    },
}

/// A conversation listing, and whether the pre-tag scan ran to completion behind it.
///
/// `scanned` is what the caller persists. Reported separately from the list because the two
/// answer different questions: the list is what to show, `scanned` is whether the migration is
/// finished and must never run again (docs §166).
#[derive(Debug)]
pub struct Adopted {
    pub conversations: Vec<Conversation>,
    pub scanned: bool,
}

#[derive(Debug)]
pub struct DeleteOutcome {
    /// `Some` means the server records are gone but Windows could not remove the named folder.
    pub files_error: Option<String>,
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
    /// The project folder the current conversation's outputs belong in.
    ///
    /// Beside the model because it has the same lifetime and the same problem: it is chosen in
    /// the UI and needed on a background task, and both are read afresh per request so a change
    /// applies to the next turn without restarting anything.
    project: SyncMutex<Option<String>>,
    /// Where the agent's code runs, for the status bar. The user should be able to
    /// see at a glance that commands are landing on their own machine.
    execution: &'static str,
    /// A **redacted** copy of the configuration, for the preflight checks — they need
    /// to know where the backend runs, never what its credentials are.
    config: BackendConfig,
    /// The turn currently in flight, so it can be stopped. See [`Sidecar::cancel_turn`].
    running: Arc<SyncMutex<Option<RunningTurn>>>,
}

/// A turn being streamed right now.
#[derive(Default)]
struct RunningTurn {
    /// The task pumping the SSE stream. Aborting it drops the HTTP response, which closes
    /// the connection — the client half of a cancel.
    task: Option<tokio::task::JoinHandle<()>>,
    /// LangGraph's id for the run, from its first `metadata` frame. `None` for the few
    /// milliseconds before that frame arrives, and the reason `cancel_turn` reports whether
    /// it could reach the server at all.
    run_id: Option<String>,
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
            project: SyncMutex::new(None),
            base_url,
            log_path,
            execution,
            config: redacted,
            running: Arc::new(SyncMutex::new(None)),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// A dropped file's path, as the backend would have to open it.
    pub fn path_for_backend(&self, path: &std::path::Path) -> String {
        self.config.path_for_backend(path)
    }

    /// Whether the backend could open this path at all — see [`crate::backend::wsl_can_open`].
    pub fn can_open(&self, path: &std::path::Path) -> bool {
        self.config.can_open(path)
    }

    pub fn execution(&self) -> &'static str {
        self.execution
    }

    /// Whether the agent's code runs on this machine.
    ///
    /// **A typed answer, not a string comparison.** The About box asked
    /// `execution() == "local"`, and the label is `"host (local)"` — so it never matched and the
    /// window told every researcher their code ran in an isolated sandbox when it was running on
    /// their own filesystem. That is the exact defect this repo reported upstream against
    /// `guardrails.py` the same morning, reintroduced by comparing against a string I assumed
    /// instead of read (docs §107). §79 had already settled the rule — matching on prose to
    /// discover a fact is how the two get confused.
    pub fn runs_locally(&self) -> bool {
        matches!(
            self.config.execution,
            crate::backend::Execution::Local { .. }
        )
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
        let project = self.project();

        let running = self.running.clone();
        let record = running.clone();
        let task = self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url)
                .with_model(model)
                .with_project(project);
            // Send failures just mean the UI dropped the receiver (window closed).
            let mut emit = |event: TurnEvent| {
                // Noted on the way past rather than asked for separately: this is the only
                // moment LangGraph names the run, and `cancel_turn` needs that name.
                if let TurnEvent::Started { run_id } = &event {
                    if let Ok(mut slot) = record.lock() {
                        if let Some(turn) = slot.as_mut() {
                            turn.run_id = Some(run_id.clone());
                        }
                    }
                }
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
            // However it ended, there is nothing left to cancel.
            if let Ok(mut slot) = running.lock() {
                *slot = None;
            }
        });
        self.register(task);

        rx
    }

    /// Remember the task streaming the current turn, so it can be stopped.
    ///
    /// Replaces whatever was there: the UI refuses to start a turn while one is running, so
    /// two live at once would be a bug elsewhere — and keeping the newer one is the reading
    /// that cannot strand the stop button on a task that has already finished.
    fn register(&self, task: tokio::task::JoinHandle<()>) {
        if let Ok(mut slot) = self.running.lock() {
            *slot = Some(RunningTurn {
                task: Some(task),
                run_id: slot.as_mut().and_then(|turn| turn.run_id.take()),
            });
        }
    }

    /// Stop the turn in flight, both here and on the backend.
    ///
    /// Returns whether the *server* could be told. Aborting our stream task closes the
    /// connection, but `on_disconnect` defaults to `continue`, so on its own that would only
    /// stop us listening while the graph carried on spending tokens. The run id from the
    /// first metadata frame is what makes the difference, and for the moment before it
    /// arrives the honest answer is `false`.
    pub fn cancel_turn(&self) -> bool {
        let Some(turn) = self.running.lock().ok().and_then(|mut slot| slot.take()) else {
            return false;
        };
        if let Some(task) = turn.task {
            task.abort();
        }
        let Some(run_id) = turn.run_id else {
            return false;
        };
        let Some(thread_id) = self.thread.lock().expect("thread id mutex").clone() else {
            return false;
        };
        let base_url = self.base_url.clone();
        // Detached: the click should not wait on a round trip, and there is nothing useful
        // to do with a failure the user has already moved on from.
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            match client.cancel_run(&thread_id, &run_id).await {
                // Logged on success too, not only on failure. Whether the *backend* actually
                // stopped is the one part of stop a person cannot see, and a log that only
                // speaks up when something breaks cannot be used to confirm it worked —
                // silence would mean both "cancelled" and "never tried".
                Ok(()) => tracing::info!(run_id, "cancelled the run on the backend"),
                Err(error) => tracing::warn!(%error, run_id, "could not cancel the run"),
            }
        });
        true
    }

    /// Swap the model/key the next turn will use. No restart, because the backend
    /// resolves the model per request.
    pub fn set_model(&self, model: Option<ModelChoice>) {
        *self.model.lock().expect("model mutex") = model;
    }

    /// Name the project this conversation's outputs belong in, or clear it.
    pub fn set_project(&self, project: Option<String>) {
        *self.project.lock().expect("project mutex") = project;
    }

    /// What the current conversation is filed under.
    pub fn project(&self) -> Option<String> {
        self.project.lock().expect("project mutex").clone()
    }

    /// Answer a paused run's approval request and stream what follows.
    ///
    /// Runs on the conversation's existing thread, so the continuation lands in the
    /// same turn rather than starting a new one.
    pub fn resume(&self, answers: Vec<Answer>) -> mpsc::UnboundedReceiver<TurnEvent> {
        let (tx, rx) = mpsc::unbounded();
        let thread = self.thread.clone();
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();
        let project = self.project();

        // A resumed continuation is as cancellable as the turn that started it: an approved
        // command can be the slowest part of a run, and that is exactly when someone reaches
        // for stop.
        let running = self.running.clone();
        let record = running.clone();
        let task = self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url)
                .with_model(model)
                .with_project(project);
            let mut emit = |event: TurnEvent| {
                if let TurnEvent::Started { run_id } = &event {
                    if let Ok(mut slot) = record.lock() {
                        if let Some(turn) = slot.as_mut() {
                            turn.run_id = Some(run_id.clone());
                        }
                    }
                }
                let _ = tx.unbounded_send(event);
            };
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                emit(TurnEvent::Error(
                    "there is no thread to resume — the run was already lost".into(),
                ));
                return;
            };
            match client.resume_turn(&thread_id, &answers, &mut emit).await {
                // A second gate in the same turn is normal: an agent often runs several
                // commands, and each one stops here.
                Ok(TurnOutcome::AwaitingApproval) => {}
                Ok(TurnOutcome::Finished) => emit(TurnEvent::Done),
                Err(error) => emit(TurnEvent::Error(format!("{error:#}"))),
            }
            if let Ok(mut slot) = running.lock() {
                *slot = None;
            }
        });
        self.register(task);

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
    /// to it. A *project's* spine outlives the conversation, so filing a new thread under one
    /// picks up its mission. A conversation in no project now has a spine of its own (§282), so
    /// forgetting the thread here is also forgetting which spine to read — and until the next
    /// turn mints one, the panel has nothing to show, which is correct: a conversation that has
    /// not happened yet has achieved nothing.
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
        // The spine belongs to the project, not to the person (docs §109) — and when there is no
        // project, to the conversation, not to every conversation that was never filed (§282).
        let project = self.project();
        let thread = self.thread_id();

        // **Neither means nothing to read, and that is an answer rather than a missing one.** A
        // conversation with no project and no thread has not happened yet, so it has no mission,
        // no plan and nothing completed. Asking anyway sends a `/project` with no scope at all,
        // which the overlay answers from the record shared by every client that does not know
        // about the parameters — and the panel would open on somebody else's work again (§282).
        if project.is_none() && thread.is_none() {
            let _ = tx.unbounded_send(Ok(crate::protocol::Project::default()));
            return rx;
        }

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url)
                .with_project(project)
                .with_thread(thread);
            let outcome = client
                .fetch_project()
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });

        rx
    }

    /// Write a hand-edited mission, and hand back the spine the store saved.
    ///
    /// **Not fire-and-forget**, unlike renaming a conversation: the backend normalises what it
    /// stores — 500 characters, runs of whitespace collapsed — so the panel has to render the
    /// answer rather than the request, or a mission that was quietly truncated would look intact
    /// until the next reload disagreed.
    ///
    /// Like `fetch_project` it does not start a backend that is not running — but unlike it, the
    /// failure is reported: a spine read that finds nothing listening is a blank decoration, and a
    /// spine write that finds nothing listening is an edit the researcher believes they made.
    pub fn set_mission(&self, mission: String) -> mpsc::UnboundedReceiver<Result<Project, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let project = self.project();
        let thread = self.thread_id();

        // Refused rather than written somewhere shared. Saying so is the whole point: a mission
        // that silently landed in the record every ungrouped conversation reads is the defect
        // this call is being fixed for, and a write is the half a researcher trusts most.
        if project.is_none() && thread.is_none() {
            let _ = tx.unbounded_send(Err(
                "ask something first — a mission is saved with its conversation".to_string(),
            ));
            return rx;
        }

        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url)
                .with_project(project)
                .with_thread(thread);
            let outcome = client
                .set_mission(&mission)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });

        rx
    }

    /// Search Zed's theme gallery.
    ///
    /// On the sidecar's runtime because that is where this app's HTTP lives; it has
    /// nothing to do with the backend, which may not even be running.
    pub fn search_themes(
        &self,
        query: String,
    ) -> mpsc::UnboundedReceiver<Result<Vec<crate::gallery::Listing>, String>> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let outcome = crate::gallery::search(&client, &query)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Ask CIP Dataverse which files a dataset holds, and whether any are restricted.
    ///
    /// On the sidecar's runtime for the same reason `search_themes` is: this is the app's own
    /// HTTP, and has nothing to do with the backend, which may not be running.
    pub fn dataset_access(
        &self,
        persistent_id: String,
    ) -> mpsc::UnboundedReceiver<Result<crate::dataverse::Access, String>> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let outcome = crate::dataverse::Access::of(&client, &persistent_id)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Fetch a dataset into the thread's folder, which is also the sandbox's working directory.
    ///
    /// So the file is downloadable *and* immediately analysable — the reason this is a button in
    /// the app rather than a tool the model calls (`crate::dataverse`).
    pub fn download_dataset(
        &self,
        persistent_id: String,
        folder: std::path::PathBuf,
    ) -> mpsc::UnboundedReceiver<Result<std::path::PathBuf, String>> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let outcome = crate::dataverse::download(&client, &persistent_id, &folder)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Ask one provider what models it offers, and cache the answer.
    ///
    /// Returns how many came back. The key is read on the **main thread** by the caller and
    /// passed in, for the same reason every other secret is: the Linux keychain client panics
    /// when called from a Tokio worker (docs §22).
    pub fn refresh_models(
        &self,
        provider: String,
        url: String,
        auth: crate::catalogue::Auth,
        api_key: Option<String>,
        now_ms: u64,
    ) -> mpsc::UnboundedReceiver<Result<(String, usize), String>> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let outcome =
                crate::catalogue::fetch(&client, &url, auth, api_key.as_deref()).await;
            let result = match outcome {
                Ok(models) => {
                    let count = models.len();
                    // Read, amend, write: other providers' listings live in the same file and a
                    // whole-map overwrite would drop whichever was fetched most recently.
                    let mut catalogue = crate::catalogue::load();
                    catalogue.insert(
                        provider.clone(),
                        crate::catalogue::Listing {
                            models,
                            fetched_ms: now_ms,
                        },
                    );
                    match crate::catalogue::save(&catalogue) {
                        Ok(()) => Ok((provider, count)),
                        Err(error) => Err(format!("{error:#}")),
                    }
                }
                Err(error) => Err(format!("{error:#}")),
            };
            let _ = tx.unbounded_send(result);
        });
        rx
    }

    /// Install one theme extension into the researcher's `themes/` directory.
    pub fn install_theme(
        &self,
        id: String,
    ) -> mpsc::UnboundedReceiver<Result<Vec<String>, String>> {
        let (tx, rx) = mpsc::unbounded();
        let dir = crate::settings::themes_dir();
        self.runtime.spawn(async move {
            let client = reqwest::Client::new();
            let outcome = crate::gallery::install(&client, &id, &dir)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Start the backend now, rather than waiting for the first question.
    ///
    /// It used to spawn lazily on the first turn, which cost twice: the sidebar had
    /// nothing to list until the researcher had already typed something — so the app
    /// opened looking as though it had no history — and the 20-40 second build then
    /// happened while they waited on an answer instead of while they read the window
    /// (docs §50).
    pub fn warm_up(&self) -> mpsc::UnboundedReceiver<Started> {
        let (tx, rx) = mpsc::unbounded();
        let supervisor = self.supervisor.clone();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let mut supervisor = supervisor.lock().await;
            match supervisor.ensure_running(&client).await {
                Ok(status) => {
                    let _ = tx.unbounded_send(status);
                }
                // Deliberately quiet. Setup problems are the Setup pane's job to explain,
                // and an error before the researcher has done anything is not how they
                // should learn that WSL is missing.
                Err(error) => tracing::debug!(%error, "backend not ready at launch"),
            }
        });
        rx
    }

    /// Build the graph while startup already tells the researcher the agent is loading.
    ///
    /// Kept separate from [`Self::warm_up`] so the UI can populate conversation names as soon as
    /// the HTTP server is ready, then keep the honest "loading research tools" status until the
    /// expensive factory finishes. Folding this into backend readiness would turn the measured
    /// 15-second graph load into a new 15-second sidebar delay (docs §176).
    pub fn warm_graph(&self) -> mpsc::UnboundedReceiver<Result<()>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let outcome = LangGraphClient::new(base_url).warm_graph().await;
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Stop the backend and start it again, reporting what happened.
    ///
    /// The verb that was missing. `ensure_running` attaches to a healthy backend rather than
    /// replacing it — right for speed, and it means the Python overlay a running server holds in
    /// memory survives an app update. Reloading it needed the process gone, and nothing in the
    /// app could ask for that (docs §79).
    pub fn restart_backend(&self) -> mpsc::UnboundedReceiver<Result<Started>> {
        let (tx, rx) = mpsc::unbounded();
        let supervisor = self.supervisor.clone();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let mut supervisor = supervisor.lock().await;
            supervisor.stop();
            // The port has to come free before the replacement can bind it. `stop` has already
            // asked the distro to reap, so this is waiting on the OS rather than on the server.
            for _ in 0..40 {
                if !client.is_healthy().await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let _ = tx.unbounded_send(supervisor.ensure_running(&client).await);
        });
        rx
    }

    /// The researcher's past conversations, newest first.
    /// `adopt` runs §90's one-time migration of pre-tag conversations first.
    ///
    /// **The caller decides, and remembers.** It used to decide for itself by asking whether any
    /// tagged conversation existed, which is empty both on the launch that needs the migration
    /// and on the launch after a researcher deletes their last conversation — so deleting
    /// everything re-tagged whatever threads were left, background workers included, and the
    /// deleted rows came back (docs §166). Emptiness is a symptom; whether the migration has run
    /// is a fact, and it belongs in the settings file.
    /// Collect any background run that finished while nobody was watching its conversation.
    ///
    /// One search for unfinished jobs, then one poll of each — and the poll is the whole point:
    /// `routes/artifacts.py` persists a run's charts and metrics into the workspace when it *sees*
    /// a terminal state, so asking is what collects them (§243).
    ///
    /// `Some(runs)` once the search succeeded — each entry the thread that owns a run and the run
    /// as it now stands, so the caller can name it and open it. `None` if the backend could not be
    /// asked at all —
    /// which happens on every launch, because the first attempt races the server coming up. Errors
    /// on an individual job are swallowed: one unreachable run must not stop the others being
    /// collected, and none of this is worth interrupting a researcher over.
    pub fn sweep_finished_jobs(&self) -> mpsc::UnboundedReceiver<Option<Vec<(String, Job)>>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            //: Enough to cover anything a researcher is plausibly still waiting on.
            const RECENT: usize = 25;
            let jobs = match client.unfinished_jobs(RECENT).await {
                Ok(jobs) => jobs,
                // `debug`: the first sweep fires while the backend is still starting, so this
                // fails once on every launch — the same argument `list_conversations` makes.
                // `None`, not zero: the first sweep fires while the backend is still starting,
                // and "could not ask" has to be distinguishable from "nothing to collect" or the
                // one attempt this gets per launch is spent on a server that was not up.
                Err(error) => {
                    tracing::debug!(%error, "could not sweep background runs yet");
                    let _ = tx.unbounded_send(None);
                    return;
                }
            };
            let mut collected: Vec<(String, Job)> = Vec::new();
            for (thread_id, job) in jobs {
                match client.poll_job(&thread_id, &job).await {
                    Ok(status) => {
                        let mut settled = job.clone();
                        settled.status = status;
                        if settled.is_finished() {
                            tracing::info!(
                                thread = %thread_id,
                                task = %job.task_id,
                                kind = job.kind.label(),
                                "collected a background run that finished unattended"
                            );
                            collected.push((thread_id, settled));
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%error, task = %job.task_id, "could not poll a run")
                    }
                }
            }
            let _ = tx.unbounded_send(Some(collected));
        });
        rx
    }

    pub fn list_conversations(&self, adopt: bool) -> mpsc::UnboundedReceiver<Adopted> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let mut scanned = false;
            if adopt {
                match client.adopt_untagged_conversations().await {
                Ok(count) => {
                    scanned = true;
                    if count > 0 {
                        tracing::info!(count, "adopted conversations from before the tag");
                    }
                }
                    // `debug`, not `warn`. The first refresh fires while the backend is still
                    // starting, so this fails once on every launch — and a warning that appears
                    // every time is one nobody reads the day it means something. And `scanned`
                    // stays false, so the marker is not written for a scan that never ran.
                    Err(error) => {
                        tracing::debug!(%error, "could not adopt older conversations yet")
                    }
                }
            }
            // 200 is far past what the sidebar can usefully show and still one request.
            match client.list_conversations(200).await {
                Ok(conversations) => {
                    let _ = tx.unbounded_send(Adopted {
                        conversations,
                        scanned,
                    });
                }
                // Not an error the researcher needs: an empty sidebar says the same thing,
                // and a backend that is still starting will answer the next refresh.
                Err(error) => tracing::debug!(%error, "could not list conversations"),
            }
        });
        rx
    }

    /// Reopen a conversation: switch to its thread and hand back its messages.
    pub fn open_conversation(
        &self,
        thread_id: String,
    ) -> mpsc::UnboundedReceiver<crate::protocol::StoredConversation> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        // Switch first, so a turn sent before the history arrives still lands on the right
        // thread rather than silently starting a new one.
        *self.thread.lock().expect("thread id mutex") = Some(thread_id.clone());
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            match client.conversation_state(&thread_id).await {
                Ok(state) => {
                    let _ = tx.unbounded_send(state);
                }
                Err(error) => tracing::warn!(%error, "could not read a conversation"),
            }
        });
        rx
    }

    /// Delete confirmed conversations, then remove the workspace folder that represented them.
    ///
    /// This used to be fire-and-forget while the caller removed the row immediately. A failed or
    /// interrupted request therefore looked successful until the next launch restored the still-
    /// existing thread — and its project heading with it (§154). The UI now waits for this answer
    /// before claiming either one is gone. Recursive filesystem work stays off both GPUI and
    /// Tokio's reactor workers; either one may be carrying a live research turn.
    pub fn delete_conversations(
        &self,
        thread_ids: Vec<String>,
        files: DeleteFiles,
    ) -> mpsc::UnboundedReceiver<Result<DeleteOutcome>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            for thread_id in &thread_ids {
                if let Err(error) = client.delete_conversation(thread_id).await {
                    tracing::warn!(%error, %thread_id, "could not delete a conversation");
                    let _ = tx.unbounded_send(Err(error));
                    return;
                }
            }

            let files_error = tokio::task::spawn_blocking(move || match files {
                DeleteFiles::Conversation { project, thread_id } => {
                    crate::workspace::delete_thread(project.as_deref(), &thread_id)
                }
                DeleteFiles::Project { name } => crate::workspace::delete_project(&name),
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result.map(|_| ()))
            .err()
            .map(|error| format!("{error:#}"));

            let _ = tx.unbounded_send(Ok(DeleteOutcome { files_error }));
        });
        rx
    }

    /// Name a conversation. Fire-and-forget: the sidebar already shows the new name.
    pub fn rename_conversation(&self, thread_id: String, title: String) {
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            if let Err(error) = client.rename_conversation(&thread_id, &title).await {
                tracing::warn!(%error, "could not rename a conversation");
            }
        });
    }

    /// Render a report to PDF and write it beside the conversation's other outputs.
    ///
    /// Reports the path it wrote, or why it could not. Off the UI thread because a Typst compile
    /// with figures in it takes seconds, and this is a button press, not a frame.
    pub fn render_report(
        &self,
        title: String,
        markdown: String,
        // Whole, links included — the route builds the bibliography from `citation` *and* `link`,
        // and reducing these to strings is what made the first download a 502 (§141).
        sources: Vec<crate::protocol::Source>,
        // Whether an Asta-backed specialist actually ran — see `Workbench::used_asta`. Passed
        // through rather than derived here: this layer cannot see the provenance record, and
        // guessing from `sources` is the mistake being fixed.
        used_asta: bool,
        into: std::path::PathBuf,
    ) -> mpsc::UnboundedReceiver<Result<std::path::PathBuf>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let Some(thread_id) = self.thread_id() else {
            // Nothing to render against: the route resolves image references relative to the
            // thread's own working directory, so there is no sensible thread-less version.
            let _ = tx.unbounded_send(Err(anyhow::anyhow!(
                "there is no conversation to render a report from yet"
            )));
            return rx;
        };
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            let result = client
                .render_report(&thread_id, &title, &markdown, &sources, used_asta)
                .await
                .and_then(|pdf| {
                    let path = into.with_extension("pdf");
                    std::fs::write(&path, pdf)
                        .with_context(|| format!("writing {}", path.display()))?;
                    Ok(path)
                });
            let _ = tx.unbounded_send(result);
        });
        rx
    }

    /// Check each DOI against Crossref, and report a verdict per reference.
    ///
    /// **The one call in this app that does not go to the backend.** Everything else here talks
    /// to the local sidecar; this reaches a public registry, so the rules are stricter and worth
    /// stating where the request is made:
    ///
    /// * **A DOI goes out, and nothing else.** Not the citation, not the question, not the
    ///   conversation. The comparison happens on this machine against text that never leaves it.
    /// * **Sequentially, with a small delay.** Crossref is a free service run for everyone; forty
    ///   parallel requests from a desktop app is not how to use it. A bibliography is tens of
    ///   items and the check is not something anyone waits on with a stopwatch.
    /// * **Bounded.** A conversation that gathered hundreds of sources checks the first
    ///   [`MAX_CHECKS`] and says so, rather than quietly stopping — §51's rule.
    ///
    /// Results arrive one at a time so the panel fills in as it goes, keyed by DOI because a
    /// source's position can change while the check is running.
    pub fn resolve_references(
        &self,
        // `(key, doi or none, citation)`. The key is the caller's — it is what the answer is
        // filed against, and it is not the DOI: the references that most need resolving are the
        // ones that have none.
        wanted: Vec<(String, Option<String>, String)>,
    ) -> mpsc::UnboundedReceiver<(String, references::Verdict, Option<references::Repair>)> {
        /// Enough for any real bibliography; a guard against a runaway list, not a policy.
        const MAX_CHECKS: usize = 60;

        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            // Identifies the app to Crossref, as their etiquette asks. **No contact address**:
            // that would be the researcher's own email leaving the machine to a third party on
            // every reference, which is the one thing org policy names outright.
            let client = match reqwest::Client::builder()
                .user_agent(concat!(
                    "mini-me-desktop/",
                    env!("CARGO_PKG_VERSION"),
                    " (research reference checker)"
                ))
                .timeout(std::time::Duration::from_secs(20))
                .build()
            {
                Ok(client) => client,
                Err(error) => {
                    tracing::warn!(%error, "could not build the reference checker");
                    return;
                }
            };

            for (key, doi, citation) in wanted.into_iter().take(MAX_CHECKS) {
                // Verify the identifier the citation carries, if it carries one.
                let verdict = match &doi {
                    Some(doi) => check_one(&client, doi, &citation).await,
                    None => references::Verdict::NoIdentifier,
                };
                // And, without being asked again, find the work it actually describes whenever
                // that identifier turned out to be wrong or missing. Making the researcher press
                // a second button to learn which paper their citation meant is asking them to do
                // the job this exists to do.
                let repair = if verdict.is_problem()
                    || matches!(verdict, references::Verdict::NoIdentifier)
                {
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    repair_one(&client, &citation).await
                } else {
                    None
                };
                if tx.unbounded_send((key, verdict, repair)).is_err() {
                    // The window has gone, or the conversation changed. Stop rather than keep
                    // asking a public service for answers nobody is waiting for.
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            }
        });
        rx
    }

    /// Ask GitHub whether a newer build has been published.
    ///
    /// **The second call in this app that does not go to the backend**, and it follows the same
    /// stricter rules as the Crossref one above:
    ///
    /// * **Nothing goes out.** A `GET` with no query, no body and no identifier. The comparison
    ///   happens here, against a number compiled into this binary.
    /// * **No credentials, deliberately.** The repository is public and the endpoint answers an
    ///   anonymous request. Sending a token would mean an update check that works for whoever
    ///   built the app and silently fails for everyone else — and it would fail *as* "you are up
    ///   to date", which is the worst shape a failure can take here.
    /// * **A short timeout, and never fatal.** A researcher on a bad connection, or behind a proxy
    ///   that blocks api.github.com, gets a sentence in the About page and an app that works.
    ///
    /// The body is read **before** the status is consulted, because GitHub writes the reason into
    /// it — the mistake §261 records making three times in one day was reporting *that* something
    /// failed instead of *what*.
    pub fn check_for_update(&self) -> mpsc::UnboundedReceiver<crate::update::Standing> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let client = match reqwest::Client::builder()
                .user_agent(concat!(
                    "mini-me-desktop/",
                    env!("CARGO_PKG_VERSION"),
                    " (update check)"
                ))
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.unbounded_send(crate::update::Standing::Unknown(error.to_string()));
                    return;
                }
            };
            let standing = match client
                .get(crate::update::LATEST_URL)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    // Body first. `error_for_status()` here would throw away the sentence GitHub
                    // wrote to be read (§261).
                    match response.text().await {
                        Ok(body) if status.is_success() => match crate::update::decode_release(&body) {
                            Ok(release) => {
                                crate::update::standing(crate::update::Version::running(), &release)
                            }
                            Err(reason) => crate::update::Standing::Unknown(reason),
                        },
                        Ok(body) => crate::update::Standing::Unknown(format!(
                            "GitHub answered {status}: {}",
                            crate::protocol::clip(body.trim(), 200)
                        )),
                        Err(error) => crate::update::Standing::Unknown(error.to_string()),
                    }
                }
                Err(error) if error.is_timeout() => {
                    crate::update::Standing::Unknown("GitHub did not answer in ten seconds".into())
                }
                Err(error) => crate::update::Standing::Unknown(error.to_string()),
            };
            let _ = tx.unbounded_send(standing);
        });
        rx
    }

    /// Take a published update: download it, check it against what was published, unpack it.
    ///
    /// **Stops short of installing.** What comes back is a staged folder beside the current one;
    /// replacing the running app is a separate, deliberate step, because it ends this process.
    ///
    /// The staging folder is created before the first byte is requested. "This folder is not
    /// writable" is a failure worth having in the first second rather than after a ten-megabyte
    /// download, and an install under `Program Files` is exactly where it happens.
    pub fn take_update(
        &self,
        release: crate::update::Release,
        install: std::path::PathBuf,
    ) -> mpsc::UnboundedReceiver<crate::update::Fetch> {
        let (tx, rx) = mpsc::unbounded();
        self.runtime.spawn(async move {
            let staged = crate::update::staging(&install, &release.tag);
            // A leftover from an abandoned attempt is not a reason to refuse; it is a reason to
            // start clean, since a half-unpacked bundle would pass every check a whole one does.
            let _ = std::fs::remove_dir_all(&staged);
            if let Err(error) = std::fs::create_dir_all(&staged) {
                let _ = tx.unbounded_send(crate::update::Fetch::Failed(format!(
                    "could not make room beside the app at {}: {error}",
                    staged.display()
                )));
                return;
            }

            let client = match reqwest::Client::builder()
                .user_agent(concat!(
                    "mini-me-desktop/",
                    env!("CARGO_PKG_VERSION"),
                    " (update download)"
                ))
                // No overall timeout: ten megabytes on a slow connection is legitimately slow, and
                // the progress messages below are what tell a stalled download from a patient one.
                .connect_timeout(std::time::Duration::from_secs(20))
                .build()
            {
                Ok(client) => client,
                Err(error) => {
                    let _ = tx.unbounded_send(crate::update::Fetch::Failed(error.to_string()));
                    return;
                }
            };

            let response = match client.get(&release.asset).send().await {
                Ok(response) => response,
                Err(error) => {
                    let _ = tx.unbounded_send(crate::update::Fetch::Failed(error.to_string()));
                    return;
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                // The body carries the reason; reading it before reporting is the §261 rule.
                let body = response.text().await.unwrap_or_default();
                let _ = tx.unbounded_send(crate::update::Fetch::Failed(format!(
                    "GitHub answered {status} for the download: {}",
                    crate::protocol::clip(body.trim(), 200)
                )));
                return;
            }

            let mut bytes: Vec<u8> = Vec::with_capacity(release.size as usize);
            // Pinned so it can be polled in place, and `StreamExt` imported here rather than at the
            // top of the file: it is the only thing in this module that streams bytes.
            use futures::StreamExt as _;
            let mut stream = std::pin::pin!(response.bytes_stream());
            let mut announced = 0u64;
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(chunk) => {
                        bytes.extend_from_slice(&chunk);
                        // Every tenth of the file rather than every chunk: the point is a moving
                        // number, and a message per 8KB is a message nobody reads and a redraw
                        // per frame of a download.
                        let step = (release.size / 10).max(1);
                        if bytes.len() as u64 >= announced + step {
                            announced = bytes.len() as u64;
                            let _ = tx.unbounded_send(crate::update::Fetch::Progress(
                                announced,
                                release.size,
                            ));
                        }
                    }
                    Err(error) => {
                        let _ = tx.unbounded_send(crate::update::Fetch::Failed(format!(
                            "the download stopped after {} of {} bytes: {error}",
                            bytes.len(),
                            release.size
                        )));
                        return;
                    }
                }
            }

            let integrity = match crate::update::verify(&bytes, &release) {
                Ok(integrity) => integrity,
                Err(reason) => {
                    // The staged folder goes too. Leaving a verified-bad download on disk is how
                    // a later attempt finds it and trusts it.
                    let _ = std::fs::remove_dir_all(&staged);
                    let _ = tx.unbounded_send(crate::update::Fetch::Failed(reason));
                    return;
                }
            };

            let outcome = match crate::update::unpack(&bytes, &staged) {
                Ok(root) => crate::update::Fetch::Ready(root, integrity),
                Err(reason) => {
                    let _ = std::fs::remove_dir_all(&staged);
                    crate::update::Fetch::Failed(reason)
                }
            };
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Record a conversation's project on the thread itself.
    ///
    /// Fire-and-forget: the folder has already moved and the app already shows the new grouping,
    /// so a failure here means the label and the disk disagree until the next successful write —
    /// worth logging, not worth blocking the researcher on.
    pub fn set_thread_project(&self, thread_id: String, project: Option<String>) {
        let base_url = self.base_url.clone();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url);
            if let Err(error) = client.set_project(&thread_id, project.as_deref()).await {
                tracing::warn!(%error, "could not record the conversation's project");
            }
        });
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
    /// Ask what a drafted discovery run would cost, for the modal that approves it.
    pub fn discovery_draft(
        &self,
        run_id: String,
    ) -> mpsc::UnboundedReceiver<Result<crate::protocol::DraftCost, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .discovery_draft(&thread_id, &run_id)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Bring in the files this conversation's commands wrote outside it.
    pub fn collect_outside(&self) -> mpsc::UnboundedReceiver<Result<crate::protocol::Collected, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .collect_outside(&thread_id)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Decode one experiment's figures, on demand.
    pub fn discovery_figures(
        &self,
        run_id: String,
        experiment_id: String,
    ) -> mpsc::UnboundedReceiver<Result<Vec<std::path::PathBuf>, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .discovery_figures(&thread_id, &run_id, &experiment_id)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Record a discovery run's status when a poll sees it change.
    pub fn discovery_status_changed(
        &self,
        run_id: String,
        status: String,
    ) -> mpsc::UnboundedReceiver<Result<(), String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .discovery_status_changed(&thread_id, &run_id, &status)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Record an approved run as started, so it survives a reload as a job.
    pub fn discovery_started(
        &self,
        run_id: String,
        experiments: u32,
        status: String,
    ) -> mpsc::UnboundedReceiver<Result<(), String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .discovery_started(&thread_id, &run_id, experiments, &status)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Read a discovery run's experiments for the results view.
    pub fn discovery_run(
        &self,
        run_id: String,
    ) -> mpsc::UnboundedReceiver<Result<serde_json::Value, String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .discovery_run(&thread_id, &run_id)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

    /// Approve a drafted run and start it. **This spends the researcher's credits.**
    ///
    /// One shot, no retry. A failed submit is reported and left alone: retrying a call that may
    /// already have charged is worse than saying it did not work.
    pub fn submit_discovery(
        &self,
        run_id: String,
        approval: String,
        experiments: u32,
        intent: String,
    ) -> mpsc::UnboundedReceiver<Result<(), String>> {
        let (tx, rx) = mpsc::unbounded();
        let base_url = self.base_url.clone();
        let thread = self.thread.clone();
        self.runtime.spawn(async move {
            let Some(thread_id) = thread.lock().expect("thread id mutex").clone() else {
                let _ = tx.unbounded_send(Err("this conversation has no thread yet".into()));
                return;
            };
            let client = LangGraphClient::new(base_url);
            let outcome = client
                .submit_discovery(&thread_id, &run_id, &approval, experiments, &intent)
                .await
                .map_err(|error| format!("{error:#}"));
            let _ = tx.unbounded_send(outcome);
        });
        rx
    }

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
            // Said once per task, not once per four-second poll. Everything about "a finished
            // worker keeps saying running" turns on what `next` holds, and until now the poll
            // reported nothing at any level a researcher sees (§207).
            let mut reported = false;
            let mut complained = false;
            loop {
                tokio::time::sleep(TASK_POLL_INTERVAL).await;
                match client.thread_state(&task.thread_id).await {
                    Ok(state) => {
                        if !reported {
                            reported = true;
                            tracing::info!(
                                thread = %task.thread_id,
                                status = %state.status,
                                next = ?state.next,
                                activity = ?state.activity,
                                "watching a background task — this is the state its status is read from"
                            );
                        }
                        let changed = state.status != task.status
                            || state.pending != task.pending
                            || state.error != task.error
                            || state.activity != task.activity
                            // A plan that advanced is news even when nothing else moved — it is
                            // the only thing that changes during a 40-second command (§209).
                            || state.todos != task.todos;
                        task.status = state.status;
                        task.pending = state.pending;
                        task.error = state.error;
                        task.activity = state.activity;
                        task.todos = state.todos;
                        if !changed {
                            continue;
                        }
                        let finished = task.is_finished();
                        tracing::info!(
                            thread = %task.thread_id,
                            status = %task.status,
                            next = ?state.next,
                            activity = ?task.activity,
                            "a background task's state changed"
                        );
                        if tx.unbounded_send(task.clone()).is_err() {
                            return;
                        }
                        if finished {
                            return;
                        }
                    }
                    Err(error) => {
                        // **Was `debug!`, which the default filter hides.** A poll that fails every
                        // four seconds leaves the panel saying `running` for ever and says nothing
                        // a researcher can see — indistinguishable from a worker that is genuinely
                        // still working. Once per task, so a backend restart does not fill the log.
                        if !complained {
                            complained = true;
                            tracing::warn!(
                                thread = %task.thread_id,
                                %error,
                                "could not read a background task's thread; its status will not \
                                 update until this starts working (docs §207)"
                            );
                        }
                        tracing::debug!(thread = %task.thread_id, %error, "task poll failed; retrying")
                    }
                }
            }
        });

        rx
    }

    /// Answer a background worker's approval request on its own thread.
    ///
    /// `owner` is the conversation whose folder the worker writes into — passed in by the
    /// caller, and **never** read from `self.thread_id()` here. A backend restart can erase the
    /// backend's in-memory worker→conversation map while a task waits for approval, which is why
    /// the resume carries the owner at all; but the conversation *open* at approval time need not
    /// be the one that launched the task, and naming it files the worker's output under an
    /// unrelated conversation (docs §159).
    ///
    /// `None` means the caller does not know. It is sent as no key at all, so the backend falls
    /// back to its own inference — at worst a visible sibling folder, which is the failure this
    /// project already knows how to spot.
    pub fn decide_task(&self, thread_id: String, owner: Option<String>, answers: Vec<Answer>) {
        let base_url = self.base_url.clone();
        let model = self.model.lock().expect("model mutex").clone();
        let project = self.project();
        self.runtime.spawn(async move {
            let client = LangGraphClient::new(base_url)
                .with_model(model)
                .with_project(project);
            if let Err(error) = client
                .resume_background(&thread_id, owner.as_deref(), &answers)
                .await
            {
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
            let report = tokio::task::spawn_blocking(move || {
                crate::preflight::inspect(&config, has_model_key)
            })
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
    /// Run a setup repair, and hand back the way to stop it alongside its output.
    ///
    /// **The handle has to come out with the receiver.** §146 declined to ship a Stop button
    /// because nothing at this layer owned anything killable: dropping the receiver does not
    /// cancel `spawn_blocking`, and aborting the Tokio task does not either. What is killable is
    /// the process `run_streaming` spawns, so that is what escapes — see `preflight::Cancel`, and
    /// §170 for why it is a pid and not the process-group handshake §168 designed.
    pub fn run_fix(&self, argv: Vec<String>) -> (mpsc::UnboundedReceiver<FixEvent>, Cancel) {
        let (tx, rx) = mpsc::unbounded();
        let cancel = Cancel::default();
        let armed = cancel.clone();
        self.runtime.spawn(async move {
            let emit = tx.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                crate::preflight::run_streaming(&argv, &armed, |line| {
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
                    // Never promise output that may not exist: on the first clean machine this
                    // said "the last lines say why" above an empty log, because nothing had
                    // been captured at all (docs §57). The card adds the lines when there
                    // are lines.
                    note: "the command reported a failure".into(),
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
        (rx, cancel)
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
        let project = self.project();
        println!("url      : {base_url}");
        println!("log      : {}", self.log_path);
        self.runtime.block_on(async move {
            let client = LangGraphClient::new(base_url)
                .with_model(model)
                .with_project(project);
            // Scoped: `run_turn` locks the supervisor itself, so holding it here
            // would deadlock the first turn.
            {
                let mut supervisor = supervisor.lock().await;
                let status = supervisor.ensure_running(&client).await?;
                println!("health   : ok ({})", status.label());
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
                let pending: std::cell::RefCell<Vec<Answer>> = Default::default();
                let mut handle = |event: TurnEvent| match event {
                    TurnEvent::Token(token) => {
                        chunks += 1;
                        text.push_str(&token);
                    }
                    TurnEvent::Status(status) => println!("status   : {status}"),
                    TurnEvent::Started { run_id } => println!("run      : {run_id}"),
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
                            // The interrupt id travels with the answer here too, so the
                            // headless check exercises the same resume shape a window does.
                            pending.borrow_mut().push(Answer {
                                interrupt: action.interrupt.clone(),
                                decision: Decision::Approve,
                            });
                        }
                    }
                    TurnEvent::Error(error) => println!("error    : {error}"),
                    TurnEvent::Done => {}
                };

                let mut outcome =
                    run_turn(&client, &supervisor, &thread, prompt, &mut handle).await?;
                while outcome == TurnOutcome::AwaitingApproval {
                    let answers: Vec<Answer> = pending.borrow_mut().drain(..).collect();
                    anyhow::ensure!(
                        !answers.is_empty(),
                        "the run paused but no approval request was decoded"
                    );
                    // The thread was created (or reused) by `run_turn` above.
                    let thread_id = thread
                        .lock()
                        .expect("thread id mutex")
                        .clone()
                        .context("the run paused but no thread was recorded")?;
                    outcome = client
                        .resume_turn(&thread_id, &answers, &mut handle)
                        .await?;
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
        emit(TurnEvent::Status(status.label().to_string()));
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

/// Ask Crossref about one DOI.
///
/// Every failure is distinguished, because they mean different things to a researcher. A **404**
/// is the registry saying no such DOI was ever registered — a fact about the reference. Anything
/// else is a fact about the network, and must not be shown as though the reference were at
/// fault: reporting "unregistered" to somebody on a train would be worse than reporting nothing,
/// because they would go and delete a citation that was fine.
async fn check_one(
    client: &reqwest::Client,
    doi: &str,
    citation: &str,
) -> references::Verdict {
    // Percent-encoded: a DOI suffix may legally contain characters that would otherwise start a
    // query string or a fragment, and a truncated DOI would be reported as unregistered.
    let url = format!("https://api.crossref.org/works/{}", urlencode(doi));
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(error) => {
            return references::Verdict::Unreachable {
                why: if error.is_timeout() {
                    "timed out".to_string()
                } else {
                    "no connection".to_string()
                },
            }
        }
    };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return references::Verdict::Unregistered;
    }
    if !response.status().is_success() {
        return references::Verdict::Unreachable {
            why: format!("registry returned {}", response.status().as_u16()),
        };
    }
    let body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(_) => {
            return references::Verdict::Unreachable {
                why: "unreadable reply".to_string(),
            }
        }
    };
    match references::title_of(&body) {
        Some(title) => references::judge(citation, &title),
        // It resolved, but the record carries no title to compare against — a data gap at the
        // registry, not a wrong citation.
        None => references::Verdict::Unreachable {
            why: "the record has no title".to_string(),
        },
    }
}

/// Ask Crossref which registered work a citation describes.
///
/// `query.bibliographic` is the field built for this: it takes a whole reference string —
/// authors, year, title, journal, pages, in whatever order — and ranks registered works against
/// it. That is why the citation goes out whole rather than being parsed into a title first: APA
/// prose is not reliably splittable, and the registry is better at this than a regex would be.
///
/// Only the top few candidates are fetched, and [`references::best_match`] then refuses any of
/// them that does not actually match. A failure here yields `None`, never a guess.
async fn repair_one(
    client: &reqwest::Client,
    citation: &str,
) -> Option<references::Repair> {
    let url = format!(
        "https://api.crossref.org/works?rows=5&select=DOI,title&query.bibliographic={}",
        urlencode(citation)
    );
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    references::best_match(citation, &references::candidates_of(&body))
}
