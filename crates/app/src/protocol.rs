//! Typed client for the Mini-Me backend's LangGraph HTTP/SSE protocol.
//!
//! The desktop app is *another client* of the protocol the React frontend already
//! speaks — no new agent code. Mapped from the Mini-Me repo (2026-07-30):
//!
//! - graph id / `assistant_id` = `"agent"` (`langgraph.json`)
//! - `GET  /ok`                        → `200 {"ok":true}` (readiness)
//! - `POST /threads`                   → `{"thread_id": "<uuid>"}`
//! - `POST /threads/{id}/runs/stream`  → SSE stream of the run
//!
//! We ask for `stream_mode: ["messages-tuple", "values", "custom"]` with
//! `stream_subgraphs: true`, so subagent work arrives too — namespaced by the
//! event name:
//!
//! - `messages-tuple` → `event: messages` frames, `[chunk, metadata]` — the tokens
//! - `values`         → full state snapshots carrying `artifacts` (and the spine
//!                      nested at `artifacts.project`)
//! - `custom`         → `sandbox_status` provisioning progress
//! - `messages|tools:<uuid>` → the same, but produced *inside* a subagent
//!
//! Verified against a live backend that requesting all three still yields
//! `event: messages` (asking for plain `messages` instead of `messages-tuple`
//! degrades them to `messages/partial` frames and silently breaks tokens), and that
//! subagent frames arrive namespaced once `stream_subgraphs` is on.
//!
//! In local dev the backend needs no `Authorization` header (`backend/auth.py`
//! admits an unauthenticated `local-user`) and falls back to `OPENAI_API_KEY`.

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

/// A decoded, UI-relevant event from a streaming run.
#[derive(Clone, Debug, PartialEq)]
pub enum TurnEvent {
    /// Human-readable progress for the status line (not model output).
    Status(String),
    /// A chunk of assistant text to append to the transcript.
    Token(String),
    /// One line of work: a tool call, or a delegation to a subagent. `agent` is
    /// `None` when the coordinator itself did it.
    Step {
        agent: Option<AgentRef>,
        label: String,
    },
    /// Text streamed by a *subagent*. Kept apart from [`TurnEvent::Token`] so the
    /// coordinator's answer stays the primary thing in the transcript.
    SubagentToken { agent: AgentRef, text: String },
    /// The run has paused: a tool call needs a human decision before it proceeds.
    /// The turn is **not** over — it continues when the client resumes.
    Approval(ApprovalRequest),
    /// A full snapshot of the run's artifacts (and the spine, which rides along).
    /// Emitted by the `values` stream mode; **replaces** prior state rather than
    /// accumulating, since each event carries the whole picture.
    Snapshot(Snapshot),
    /// The run finished cleanly.
    Done,
    /// The run failed; the string is display-safe.
    Error(String),
}

/// One subagent invocation.
///
/// `ns` is the pregel checkpoint namespace carried in the SSE event name
/// (`messages|tools:<uuid>`). It is unique per *invocation*, so two concurrent runs
/// of the same subagent type stay in separate groups. `name` is the display name the
/// backend hands us in the chunk metadata as `lc_agent_name`.
///
/// Deliberately **not** keyed on the originating `task` tool-call id: the namespace
/// uuid is a pregel task id, and the two can only be reconciled by matching the
/// delegation's `description` against the subgraph's first human message — a
/// three-pass heuristic the JS SDK carries, which mis-attributes when two subagents
/// get identical descriptions. We don't need it, because `lc_agent_name` already
/// names the agent (plan §15b).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentRef {
    pub ns: String,
    pub name: String,
}

/// A paused run waiting on the person.
///
/// Measured shape (2026-07-31), carried in a `values` event as `__interrupt__`:
/// `[{"value": {"action_requests": [...], "review_configs": [...]}, "id": "…"}]`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalRequest {
    /// One entry per action awaiting a decision. Order matters: the resume payload
    /// must carry exactly one decision per action, in this order.
    pub actions: Vec<PendingAction>,
}

/// One tool call held at the gate.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingAction {
    pub tool: String,
    /// What the tool would actually do, rendered for a human. For `execute` this is
    /// the shell command verbatim — the thing the user is really deciding about.
    pub detail: String,
    pub description: String,
    /// Decisions the agent will accept (`approve`, `reject`, …).
    pub allowed: Vec<String>,
}

/// Research outputs produced so far, as carried by a `values` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub buckets: Vec<Bucket>,
    /// The `values` payload nests the spine under `artifacts.project`, so a turn
    /// updates the mission for free — no extra `GET /project` round trip.
    pub project: Option<Project>,
    /// Long jobs the turn left running. See [`Job`].
    pub jobs: Vec<Job>,
    /// Work handed to a background worker. See [`AsyncTask`].
    pub tasks: Vec<AsyncTask>,
}

/// A long-running Asta job that outlived the turn that started it.
///
/// The theorizer (5–15 min) and DataVoyager (20–40 min) submit with `--no-wait` and return
/// a `task_id` immediately, so a chat turn is never blocked on them. The **client** is
/// then responsible for polling — and this client never did, which was not merely a
/// missing panel: `persist_theory_outputs` and `persist_analysis_outputs` are called from
/// the poll route and **nowhere else** (`backend/routes/artifacts.py:202,243`), so a
/// completed run never wrote its results anywhere, while `prompts.py` told the
/// coordinator they had been saved to the sandbox.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub kind: JobKind,
    pub task_id: String,
    /// The question the job was started for — its label, and a query parameter the
    /// theorizer route uses when persisting results.
    pub question: String,
    /// DataVoyager only; passed back as `ctx`.
    pub context_id: Option<String>,
    /// Status as of the last snapshot or poll.
    pub status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Theorizer,
    Analysis,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            JobKind::Theorizer => "Theorizer",
            JobKind::Analysis => "Data analysis",
        }
    }

    /// Roughly how long these take, so the panel can set expectations rather than
    /// leaving someone watching a spinner wondering if it is stuck.
    pub fn expected(self) -> &'static str {
        match self {
            JobKind::Theorizer => "5–15 min",
            JobKind::Analysis => "20–40 min",
        }
    }
}

impl Job {
    /// Whether this job has stopped moving.
    ///
    /// `unavailable` counts: the thread's sandbox is gone, so no further poll can tell us
    /// anything and continuing would just burn requests forever.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status.as_str(),
            "completed" | "failed" | "canceled" | "cancelled" | "unavailable" | "error"
        )
    }

    pub fn succeeded(&self) -> bool {
        self.status == "completed"
    }

    /// The poll route for this job, relative to the backend.
    fn route(&self, thread_id: &str) -> String {
        let task = urlencode(&self.task_id);
        match self.kind {
            JobKind::Theorizer => format!(
                "/theorizer/{}/{task}?q={}",
                urlencode(thread_id),
                urlencode(&self.question)
            ),
            JobKind::Analysis => format!(
                "/analyze-data/{}/{task}?ctx={}",
                urlencode(thread_id),
                urlencode(self.context_id.as_deref().unwrap_or_default())
            ),
        }
    }
}

/// A task the coordinator handed to a background worker.
///
/// Fields from `deepagents.middleware.async_subagents.AsyncTask`, carried in agent state
/// under `async_tasks` (a `task_id → task` map) and so arriving in every `values`
/// snapshot. `thread_id` is the load-bearing one: the background worker runs on its **own**
/// thread, which is why its approval requests were invisible — the client only ever
/// resumed the conversation's thread.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncTask {
    pub task_id: String,
    pub thread_id: String,
    pub agent_name: String,
    pub status: String,
    /// What the worker was asked to do, when the middleware recorded it.
    pub description: String,
    /// Set when the background run has stopped at the approval gate. Until this existed,
    /// such a task simply hung — nothing in the UI could answer it (docs §31).
    pub pending: Option<ApprovalRequest>,
    /// What actually went wrong, read off the worker's own thread.
    ///
    /// Not the same thing as the middleware's report. `check_async_task` looks for an
    /// `error` on the *run* record, the dev server does not put one there, and so it says
    /// "The async subagent encountered an error" — a placeholder that cost this project
    /// two rounds of guessing (docs §38). The server does record the failure on the
    /// thread's pending task; this is that text.
    pub error: Option<String>,
    /// The subagent or tool the worker is on right now, so "running" for ten minutes says
    /// something about *what* is running (docs §42).
    pub activity: Option<String>,
}

impl AsyncTask {
    /// Whether the task has stopped for good.
    ///
    /// LangGraph run statuses: `pending`, `running`, `error`, `success`, `timeout`,
    /// `interrupted`. Only the last three of those are terminal — `interrupted` is *not*,
    /// because it is a task waiting for a person.
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status.as_str(),
            "success" | "error" | "timeout" | "cancelled" | "canceled"
        )
    }

    pub fn succeeded(&self) -> bool {
        self.status == "success"
    }

    /// Whether it is stopped, waiting on a decision.
    pub fn needs_approval(&self) -> bool {
        self.pending.is_some()
    }
}

/// Metadata key marking a thread as a conversation the researcher started.
///
/// The distinguishing fact is *who created it*: this app tags what it creates, and nothing
/// else does — not the async-subagent middleware, not the theorizer. Filtering on the tag
/// is therefore exact, where filtering on "has messages" or "has a title" would be a guess
/// that keeps being wrong.
const CONVERSATION_TAG: &str = "minime_conversation";

/// One past conversation, for the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub thread_id: String,
    /// What to call it in the list. Never empty — see [`decode_conversation`].
    pub title: String,
    /// ISO-8601, as the server reports it. Used for grouping, not for display.
    pub updated_at: String,
}

/// A thread from `POST /threads/search`, or `None` if it has no usable id.
fn decode_conversation(thread: &Value) -> Option<Conversation> {
    let thread_id = thread
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let metadata = thread.get("metadata");
    // A title the researcher set wins. Failing that, the first thing they asked — which
    // is what every chat app does, and is very nearly always the better label anyway.
    let title = metadata
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "New conversation".to_string());
    Some(Conversation {
        thread_id: thread_id.to_string(),
        title,
        updated_at: thread
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// One stored message as `(role, text)`, or `None` if it is not worth showing.
///
/// Tool messages and empty assistant turns are dropped: reopening a conversation should
/// look like the conversation, not like its plumbing.
fn decode_stored_message(message: &Value) -> Option<(String, String)> {
    let kind = message
        .get("type")
        .or_else(|| message.get("role"))
        .and_then(Value::as_str)?;
    let role = match kind {
        "human" | "user" => "you",
        "ai" | "assistant" => "mini-me",
        _ => return None,
    };
    // Content is a string, or a list of blocks in the newer content-block shape.
    let content = message.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some((role.to_string(), text.to_string()))
}

/// A conversation's name, from the first thing the researcher asked.
///
/// Chat apps auto-title from the opening prompt because a list of "New conversation" is a
/// list of nothing. Truncated on a word boundary so a title ends mid-sentence rather than
/// mid-word.
pub fn title_from_prompt(prompt: &str) -> String {
    const LIMIT: usize = 48;
    let cleaned = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= LIMIT {
        return cleaned;
    }
    let mut title = String::new();
    for word in cleaned.split(' ') {
        if title.chars().count() + word.chars().count() + 1 > LIMIT {
            break;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    // A single word longer than the limit leaves nothing; cut it rather than give up.
    if title.is_empty() {
        title = cleaned.chars().take(LIMIT).collect();
    }
    format!("{title}…")
}

/// What a background worker's own thread reports about itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadState {
    /// Derived, not reported: `error` beats a pending approval beats an empty `next`.
    pub status: String,
    pub pending: Option<ApprovalRequest>,
    pub error: Option<String>,
    /// What the worker is doing right now — the subagent it delegated to, or the tool it
    /// is running. `None` when nothing has been called yet.
    pub activity: Option<String>,
}

/// What the worker is busy with, from the last tool call in its thread.
///
/// A background worker is a whole coordinator, so "running" alone says nothing: it might
/// be reading papers, running a script or writing a report, for ten minutes, and the panel
/// showed the same word throughout. The `task` tool carries the subagent's name in its
/// arguments, which is the interesting case; every other tool is reported by its own name.
fn last_activity(state: &Value) -> Option<String> {
    let messages = state
        .get("values")
        .and_then(|values| values.get("messages"))
        .and_then(Value::as_array)?;

    for message in messages.iter().rev() {
        let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
            continue;
        };
        let Some(call) = calls.last() else { continue };
        let name = call.get("name").and_then(Value::as_str)?.trim();
        if name.is_empty() {
            continue;
        }
        // Delegation: `task(subagent_type=…)` is deepagents' own subagent tool, and the
        // subagent's name is the only part of this a researcher cares about.
        let delegated = call
            .get("args")
            .and_then(|args| args.get("subagent_type").or_else(|| args.get("agent_type")))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|agent| !agent.is_empty());
        return Some(match delegated {
            Some(agent) => agent.replace('_', " "),
            None => name.replace('_', " "),
        });
    }
    None
}

/// The failure text out of a thread task's `error`, whatever shape it arrives in.
///
/// A string in the versions measured, but LangGraph has shipped an `{message, type}`
/// object here too, and a panel that renders `[object]` teaches the user nothing.
fn error_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(text) => text.clone(),
        Value::Object(fields) => fields
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| value.to_string()),
        other => other.to_string(),
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Python tracebacks arrive whole. The last line is the exception itself, which is the
    // part that names the cause; the frames above it are the backend's business, and the
    // panel is a column roughly forty characters wide.
    let last = text.lines().rev().find(|line| !line.trim().is_empty())?;
    Some(last.trim().to_string())
}

/// Pull background tasks out of a `values` payload.
fn decode_async_tasks(artifacts: &Value, root: &Value) -> Vec<AsyncTask> {
    // The middleware writes to agent state, which in a `values` frame sits at the top
    // level — but the same payload nests artifacts, so check both rather than guessing.
    let map = root
        .get("async_tasks")
        .or_else(|| artifacts.get("async_tasks"))
        .and_then(Value::as_object);
    let Some(map) = map else {
        return Vec::new();
    };
    let mut tasks: Vec<AsyncTask> = map
        .values()
        .filter_map(|task| {
            let thread_id = task.get("thread_id").and_then(Value::as_str)?;
            Some(AsyncTask {
                task_id: task
                    .get("task_id")
                    .and_then(Value::as_str)
                    .unwrap_or(thread_id)
                    .to_string(),
                thread_id: thread_id.to_string(),
                agent_name: task
                    .get("agent_name")
                    .and_then(Value::as_str)
                    .unwrap_or("background_worker")
                    .to_string(),
                status: task
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("running")
                    .to_string(),
                description: task
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                pending: None,
                error: None,
                activity: None,
            })
        })
        .collect();
    // A map has no order; without this the panel would reshuffle on every frame.
    tasks.sort_by(|a, b| a.task_id.cmp(&b.task_id));
    tasks
}

/// Percent-encode a path or query value.
///
/// Hand-rolled rather than pulling in a URL crate for one function: the values here are a
/// UUID, a thread id and a research question, and the question is the only one that
/// reliably contains spaces, punctuation and accented Spanish.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// One named group of outputs (datasets, sources, reports, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Bucket {
    pub name: &'static str,
    pub items: Vec<String>,
}

/// The artifact buckets we surface, in display order.
///
/// Taken from a live `values` payload (2026-07-30), which carries exactly:
/// `datasets, sources, reports, files, hypotheses, libraries, analyses, edges,
/// project`. `edges` is graph wiring rather than a user-facing output, and
/// `project` is the spine, so neither is listed here.
const ARTIFACT_BUCKETS: [&str; 7] = [
    "datasets",
    "sources",
    "reports",
    "files",
    "hypotheses",
    "libraries",
    "analyses",
];

/// The research project "spine": the durable mission plus what has been done and
/// what is queued. This is the workbench's identity — the thing a chat window
/// alone can't express.
///
/// Every field defaults, so a sparse or older backend response still decodes.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub mission: String,
    #[serde(default)]
    pub completed: Vec<String>,
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
}

/// An advisory next step. **Advisory only** — org policy is human-gated, so the
/// app never runs one of these on its own; `prompt` is what gets *offered* to the
/// user, not executed.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Suggestion {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub prompt: String,
}

#[derive(Deserialize)]
struct ThreadCreated {
    thread_id: String,
}

/// Which model to use, and the key to use it with.
///
/// The backend resolves this **per request** — its provider table is even commented
/// *"provider id (from the panel)"* — so the key travels from the OS keychain into the
/// request body and never becomes an environment variable, a line in a `.env`, or an
/// argument on a `wsl.exe` command line (docs §20).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelChoice {
    /// `"provider::model_id"`, e.g. `anthropic::claude-sonnet-4-5`.
    pub spec: String,
    pub provider: String,
    pub api_key: Option<String>,
    /// Mandatory for the `custom` provider, ignored otherwise.
    pub base_url: Option<String>,
}

/// Thin HTTP client bound to a backend base URL.
pub struct LangGraphClient {
    http: reqwest::Client,
    base_url: String,
    model: Option<ModelChoice>,
}

impl LangGraphClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            model: None,
        }
    }

    /// Attach the user's model choice and key. Without one the backend falls back to
    /// whatever provider variables its own environment happens to have.
    pub fn with_model(mut self, model: Option<ModelChoice>) -> Self {
        self.model = model;
        self
    }

    /// `GET /ok` — true when the server is up and the graph is loaded.
    pub async fn is_healthy(&self) -> bool {
        match self.http.get(format!("{}/ok", self.base_url)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// `GET /project` → the research project spine.
    ///
    /// A custom Mini-Me route (not part of the LangGraph API). Verified against a
    /// live backend 2026-07-30: `200 {"mission":…,"completed":[],"pending":[],
    /// "suggestions":[]}`. The mission is derived server-side from the first human
    /// message of the project, so it is empty until a turn has run.
    pub async fn fetch_project(&self) -> Result<Project> {
        let resp = self
            .http
            .get(format!("{}/project", self.base_url))
            .send()
            .await
            .context("GET /project failed (is the sidecar running?)")?
            .error_for_status()
            .context("GET /project returned an error status")?;
        resp.json()
            .await
            .context("could not decode the project spine from GET /project")
    }

    /// Poll one long job, returning its status.
    ///
    /// The route does more than report: on a terminal state it **persists the outcome
    /// into the sandbox**, which is how the agent can read the theories on a later turn.
    /// Polling is therefore not a display nicety — it is the only thing that makes a
    /// finished run durable (`backend/routes/artifacts.py:196-203`).
    ///
    /// A transport failure is *not* an error worth stopping for: the sidecar may simply be
    /// restarting. The caller keeps the job running and tries again.
    pub async fn poll_job(&self, thread_id: &str, job: &Job) -> Result<String> {
        let resp = self
            .http
            .get(format!("{}{}", self.base_url, job.route(thread_id)))
            .send()
            .await
            .context("polling a background job failed")?
            .error_for_status()
            .context("the job poll route returned an error status")?;
        let value: Value = resp
            .json()
            .await
            .context("could not decode the job poll response")?;
        Ok(value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("running")
            .trim()
            .to_string())
    }

    /// Read a background worker's thread: is it running, finished, or waiting on a person?
    ///
    /// `GET /threads/{id}/state` answers all three at once. Its `tasks[].interrupts[]`
    /// carry exactly the payload `decode_interrupt` already understands, so a background
    /// approval and a foreground one are the same shape — which is what lets the pane
    /// render one card for both.
    ///
    /// Returns `(status, pending_approval)`. `status` is derived rather than reported: an
    /// interrupted thread is *waiting*, an empty `next` with no interrupt is *done*, and
    /// anything else is still working.
    pub async fn thread_state(&self, thread_id: &str) -> Result<ThreadState> {
        let resp = self
            .http
            .get(format!(
                "{}/threads/{}/state",
                self.base_url,
                urlencode(thread_id)
            ))
            .send()
            .await
            .context("reading a background task's thread failed")?
            .error_for_status()
            .context("the thread-state route returned an error status")?;
        let state: Value = resp
            .json()
            .await
            .context("could not decode the background thread's state")?;

        // Interrupts live on the pending tasks; the same payload also shows up under
        // `values.__interrupt__` in some versions, so both are checked.
        // Reshaped into the `{"__interrupt__": [...]}` envelope `decode_interrupt` already
        // parses, rather than writing a second parser for the same payload.
        let from_tasks: Vec<Value> = state
            .get("tasks")
            .and_then(Value::as_array)
            .map(|tasks| {
                tasks
                    .iter()
                    .filter_map(|task| task.get("interrupts").and_then(Value::as_array))
                    .flatten()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let pending = decode_interrupt(&json!({"__interrupt__": from_tasks}))
            .or_else(|| state.get("values").and_then(decode_interrupt));

        // The same tasks carry the failure, when there is one. This is the only place the
        // real text is available: the run record has no `error` field on the dev server,
        // which is why the middleware falls back to a placeholder (docs §38).
        let error = state
            .get("tasks")
            .and_then(Value::as_array)
            .and_then(|tasks| {
                tasks
                    .iter()
                    .filter_map(|task| task.get("error"))
                    .find_map(error_text)
            });

        let next_is_empty = state
            .get("next")
            .and_then(Value::as_array)
            .is_some_and(|next| next.is_empty());
        // Failure first. A run that died leaves its task pending, so `next` is *not*
        // empty — without this the watcher reported "running" forever and the panel only
        // ever learned of a failure when the researcher happened to ask.
        let status = if error.is_some() {
            "error"
        } else if pending.is_some() {
            "interrupted"
        } else if next_is_empty {
            "success"
        } else {
            "running"
        };
        Ok(ThreadState {
            status: status.to_string(),
            pending,
            error,
            activity: last_activity(&state),
        })
    }

    /// Answer a background worker's approval request, on **its** thread.
    ///
    /// Deliberately not streamed into the transcript: the background run's tokens are not
    /// the answer to anything the researcher asked in the chat, and mixing them into the
    /// conversation is how "what did I just read?" happens. The Jobs panel reports it.
    pub async fn resume_background(&self, thread_id: &str, decisions: &[Decision]) -> Result<()> {
        // The same body a foreground resume sends — one definition, so a change to the
        // decision shape cannot fix one path and leave the other broken.
        let payload = resume_request_body(decisions, self.model.as_ref());
        self.http
            .post(format!(
                "{}/threads/{}/runs",
                self.base_url,
                urlencode(thread_id)
            ))
            .json(&payload)
            .send()
            .await
            .context("resuming a background task failed")?
            .error_for_status()
            .context("resuming a background task returned an error status")?;
        Ok(())
    }

    /// `POST /threads` → a fresh thread id.
    pub async fn create_thread(&self) -> Result<String> {
        let resp = self
            .http
            .post(format!("{}/threads", self.base_url))
            // Marked as *ours*. Every background worker creates a thread of its own
            // (§43), and without this the sidebar filled with dozens of "New
            // conversation" rows that were machinery, not conversations (docs §51).
            .json(&json!({ "metadata": { CONVERSATION_TAG: true } }))
            .send()
            .await
            .context("POST /threads failed (is the sidecar running?)")?
            .error_for_status()
            .context("POST /threads returned an error status")?;
        let created: ThreadCreated = resp
            .json()
            .await
            .context("could not decode the thread_id from POST /threads")?;
        Ok(created.thread_id)
    }

    /// The researcher's past conversations, most recently touched first.
    ///
    /// The backend has stored every thread all along; the app simply never asked, so each
    /// launch looked like the first. `POST /threads/search` is the list route
    /// (`langgraph_sdk.ThreadsClient.search`).
    pub async fn list_conversations(&self, limit: usize) -> Result<Vec<Conversation>> {
        let resp = self
            .http
            .post(format!("{}/threads/search", self.base_url))
            .json(&json!({
                "limit": limit,
                "sort_by": "updated_at",
                "sort_order": "desc",
                // Only threads this app started as a conversation. A background worker's
                // thread is real, and is deliberately not one of these.
                "metadata": { CONVERSATION_TAG: true },
            }))
            .send()
            .await
            .context("listing conversations failed")?
            .error_for_status()
            .context("the thread-search route returned an error status")?;
        let threads: Value = resp
            .json()
            .await
            .context("could not decode the conversation list")?;
        Ok(threads
            .as_array()
            .map(|threads| threads.iter().filter_map(decode_conversation).collect())
            .unwrap_or_default())
    }

    /// Give a conversation a name, stored on the thread itself.
    ///
    /// Metadata rather than a local file: the title belongs with the conversation, so it
    /// survives a reinstall and cannot drift out of sync with the thread it names.
    pub async fn rename_conversation(&self, thread_id: &str, title: &str) -> Result<()> {
        self.http
            .patch(format!(
                "{}/threads/{}",
                self.base_url,
                urlencode(thread_id)
            ))
            .json(&json!({ "metadata": { "title": title } }))
            .send()
            .await
            .context("renaming the conversation failed")?
            .error_for_status()
            .context("the thread-update route returned an error status")?;
        Ok(())
    }

    /// The messages of an existing conversation, for reopening it.
    ///
    /// Only role and text: the activity trace is not replayable — it was assembled from a
    /// stream that is over — and pretending otherwise would show an empty trace next to a
    /// real answer, which reads as a bug rather than as history.
    pub async fn conversation_messages(&self, thread_id: &str) -> Result<Vec<(String, String)>> {
        let resp = self
            .http
            .get(format!(
                "{}/threads/{}/state",
                self.base_url,
                urlencode(thread_id)
            ))
            .send()
            .await
            .context("reading the conversation failed")?
            .error_for_status()
            .context("the thread-state route returned an error status")?;
        let state: Value = resp
            .json()
            .await
            .context("could not decode the conversation")?;
        let Some(messages) = state
            .get("values")
            .and_then(|values| values.get("messages"))
            .and_then(Value::as_array)
        else {
            return Ok(Vec::new());
        };
        Ok(messages.iter().filter_map(decode_stored_message).collect())
    }

    /// Stream one coordinator turn, invoking `on_event` for each decoded event.
    ///
    /// Kept as a callback rather than a returned stream so the caller (the
    /// sidecar task) can forward straight into a channel without buffering the
    /// whole turn.
    pub async fn stream_turn(
        &self,
        thread_id: &str,
        prompt: &str,
        on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnOutcome> {
        self.stream(thread_id, run_request_body(prompt, self.model.as_ref()), on_event)
            .await
    }

    /// Resume a run that stopped at the approval gate, streaming the continuation.
    pub async fn resume_turn(
        &self,
        thread_id: &str,
        decisions: &[Decision],
        on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnOutcome> {
        self.stream(
            thread_id,
            resume_request_body(decisions, self.model.as_ref()),
            on_event,
        )
        .await
    }

    async fn stream(
        &self,
        thread_id: &str,
        body: Value,
        mut on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnOutcome> {

        let resp = self
            .http
            .post(format!(
                "{}/threads/{}/runs/stream",
                self.base_url, thread_id
            ))
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .context("POST /runs/stream failed")?
            .error_for_status()
            .context("POST /runs/stream returned an error status")?;

        let mut outcome = TurnOutcome::Finished;
        let mut frames = SseDecoder::default();
        let mut turn = TurnDecoder::default();
        // `MINIME_CAPTURE_SSE=<path>` appends the raw stream, which is how every wire
        // shape in the plan was measured — and what `--replay` consumes afterwards.
        // Synchronous writes on purpose: this is a debug aid, not a hot path.
        let mut capture = std::env::var_os("MINIME_CAPTURE_SSE").and_then(|path| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        });
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("the run stream broke mid-turn")?;
            if let Some(file) = capture.as_mut() {
                use std::io::Write as _;
                let _ = file.write_all(&bytes);
            }
            for event in frames.push(&bytes) {
                for decoded in turn.push(&event) {
                    // A paused run looks exactly like a finished one at the transport
                    // layer — the stream just ends. Remembering the interrupt is what
                    // lets the caller tell "done" from "waiting on you".
                    if matches!(decoded, TurnEvent::Approval(_)) {
                        outcome = TurnOutcome::AwaitingApproval;
                    }
                    on_event(decoded);
                }
            }
        }
        Ok(outcome)
    }
}

/// Body for `POST /threads/{id}/runs/stream`.
///
/// `stream_mode` **must** be `messages-tuple`, not `messages`: the server rewrites
/// `messages-tuple` into `event: messages` frames carrying `[chunk, metadata]`
/// tuples (`langgraph_api/stream.py`), which is what token streaming needs.
/// Asking for `messages` instead selects the v1 path, which emits
/// `messages/partial` + `messages/complete` with a different payload shape — and
/// yields no tokens through this decoder.
fn run_request_body(prompt: &str, model: Option<&ModelChoice>) -> Value {
    let mut body = stream_request_body(model);
    body["input"] = json!({ "messages": [ { "type": "human", "content": prompt } ] });
    body
}

/// Body for resuming a paused run with the human's decisions.
///
/// Shape from the HITL middleware (`human_in_the_loop.py`:
/// `decisions = interrupt(hitl_request)["decisions"]`): exactly one decision per
/// held action, in the order they were presented.
fn resume_request_body(decisions: &[Decision], model: Option<&ModelChoice>) -> Value {
    let decisions: Vec<Value> = decisions
        .iter()
        .map(|decision| match decision {
            Decision::Approve => json!({ "type": "approve" }),
            Decision::Reject { message } => json!({ "type": "reject", "message": message }),
        })
        .collect();
    let mut body = stream_request_body(model);
    body["command"] = json!({ "resume": { "decisions": decisions } });
    body
}

/// What the user decided about one held action.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Approve,
    Reject { message: String },
}

/// How a stream ended: finished, or stopped at the approval gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TurnOutcome {
    Finished,
    AwaitingApproval,
}

/// The parts of the request body shared by a fresh run and a resume.
fn stream_request_body(model: Option<&ModelChoice>) -> Value {
    json!({
        "assistant_id": "agent",
        "stream_mode": ["messages-tuple", "values", "custom"],
        // Without this the whole stream stops at the coordinator: a delegated turn
        // then emits a `task` tool call and nothing else until the answer, which is
        // the silent gap the activity trace exists to close. On a measured turn this
        // flag is the difference between 176 and 495 message events.
        "stream_subgraphs": true,
        "config": config_for(model),
    })
}

/// The `config` object: recursion limit, model routing, and the key.
fn config_for(model: Option<&ModelChoice>) -> Value {
    let mut configurable = json!({
        // Marks this as a real run rather than a read-only graph load, which is what the
        // backend's key check keys off.
        "__is_for_execution__": true,
    });

    if let Some(model) = model {
        let mut model_config = json!({
            "default": model.spec,
            // "client" keeps the backend's *server-side* Vault path dormant. Left unset
            // with no inline keys it tries a Vault lookup that needs a user identity —
            // i.e. the WorkOS world this product dropped (docs §11/§20).
            "storage_mode": "client",
        });
        if model.api_key.is_none() {
            // Nothing to supply inline, so don't claim client-only storage.
            model_config
                .as_object_mut()
                .expect("object")
                .remove("storage_mode");
        }
        configurable["model_config"] = model_config;

        if let Some(api_key) = &model.api_key {
            configurable["__llm_keys"] = json!({
                model.provider.clone(): {
                    "api_key": api_key,
                    "base_url": model.base_url,
                }
            });
        }
    }

    json!({
        // LangGraph defaults to 25 supersteps, and one turn already spends ~22 on
        // middleware alone (PII scrubbing, call limits, todos, skills, sandbox sync)
        // before any delegation -- so a multi-subagent research turn would hit the
        // ceiling and fail. The web frontend sets the same value.
        "recursion_limit": 10_000,
        "configurable": configurable,
    })
}

/// One raw `event:` / `data:` block from an SSE stream.
#[derive(Debug, Default, PartialEq)]
pub struct SseEvent {
    pub name: String,
    pub data: String,
}

/// Incremental SSE framer. Network chunks split anywhere — including mid-line
/// and mid-event — so bytes are buffered until a `\n\n` terminator is seen.
#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Feed raw bytes; returns whatever complete events they completed.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut events = Vec::new();
        // Tolerate CRLF as well as LF terminators.
        while let Some((end, sep_len)) = self
            .buffer
            .find("\n\n")
            .map(|i| (i, 2))
            .or_else(|| self.buffer.find("\r\n\r\n").map(|i| (i, 4)))
        {
            let block: String = self.buffer.drain(..end + sep_len).collect();
            if let Some(event) = parse_sse_block(&block) {
                events.push(event);
            }
        }
        events
    }
}

fn parse_sse_block(block: &str) -> Option<SseEvent> {
    let mut event = SseEvent::default();
    let mut data_lines: Vec<&str> = Vec::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("event:") {
            event.name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            // Per the SSE spec a single leading space is stripped.
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // `:` comments / `id:` / `retry:` are irrelevant here.
    }
    if event.name.is_empty() && data_lines.is_empty() {
        return None;
    }
    event.data = data_lines.join("\n");
    Some(event)
}

/// Pull the artifact buckets (and the nested spine) out of a `values` payload.
///
/// Returns `None` when there is nothing to show, so an early snapshot doesn't
/// blank a panel that already has content.
/// Pull a pending-approval request out of a `values` payload.
fn decode_interrupt(value: &Value) -> Option<ApprovalRequest> {
    let interrupts = value.get("__interrupt__")?.as_array()?;
    let mut actions = Vec::new();
    for interrupt in interrupts {
        let payload = interrupt.get("value")?;
        let requests = payload.get("action_requests")?.as_array()?;
        let configs = payload.get("review_configs").and_then(Value::as_array);
        for (index, request) in requests.iter().enumerate() {
            let tool = request
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            // `execute`'s whole argument is the command; for anything else, show the
            // arguments as they are rather than inventing a summary.
            let args = request.get("args");
            let detail = args
                .and_then(|args| args.get("command"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| args.map(|args| args.to_string()))
                .unwrap_or_default();
            let allowed = configs
                .and_then(|configs| configs.get(index))
                .and_then(|config| config.get("allowed_decisions"))
                .and_then(Value::as_array)
                .map(|decisions| {
                    decisions
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_else(|| vec!["approve".to_string(), "reject".to_string()]);
            actions.push(PendingAction {
                tool,
                detail,
                description: request
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                allowed,
            });
        }
    }
    if actions.is_empty() {
        return None;
    }
    Some(ApprovalRequest { actions })
}

fn decode_values(data: &str) -> Option<Snapshot> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let artifacts = value.get("artifacts")?;

    let buckets: Vec<Bucket> = ARTIFACT_BUCKETS
        .iter()
        .filter_map(|name| {
            let items: Vec<String> = artifacts
                .get(name)?
                .as_array()?
                .iter()
                .map(artifact_label)
                .collect();
            if items.is_empty() {
                return None;
            }
            Some(Bucket { name, items })
        })
        .collect();

    let project = artifacts
        .get("project")
        .and_then(|project| serde_json::from_value::<Project>(project.clone()).ok());

    let jobs = decode_jobs(artifacts);
    let tasks = decode_async_tasks(artifacts, &value);

    if buckets.is_empty() && project.is_none() && jobs.is_empty() && tasks.is_empty() {
        return None;
    }
    Some(Snapshot {
        buckets,
        project,
        jobs,
        tasks,
    })
}

/// Pull the still-running long jobs out of a `values` payload.
///
/// Fields come from `HypothesisArtifactPayload` / `DataAnalysisArtifactPayload`
/// (`backend/schemas.py:353,388`): both carry `status` and `task_id`, and the analysis one
/// adds `context_id`. A job with no task id cannot be polled, so it is skipped rather than
/// shown as something the user could wait on.
fn decode_jobs(artifacts: &Value) -> Vec<Job> {
    let mut jobs = Vec::new();
    for (bucket, kind) in [
        ("hypotheses", JobKind::Theorizer),
        ("analyses", JobKind::Analysis),
    ] {
        let Some(items) = artifacts.get(bucket).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(task_id) = item
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("running")
                .trim()
                .to_string();
            jobs.push(Job {
                kind,
                task_id: task_id.to_string(),
                question: item
                    .get("question")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                context_id: item
                    .get("context_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status,
            });
        }
    }
    jobs
}

/// Turn a `custom` event into status-line text.
///
/// The backend emits sandbox provisioning progress here — verified live
/// (2026-07-30): `{"sandbox_status":{"state":"preparing","message":"Creating
/// sandbox…"}}` then `{"state":"ready","message":"Sandbox ready"}`. Surfacing it
/// matters because the first turn on a cold thread waits on that provisioning, and
/// without this the UI just looks stuck.
fn decode_custom(data: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    let status = value.get("sandbox_status")?;
    // Prefer the human message; fall back to the bare state.
    let text = status
        .get("message")
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())
        .or_else(|| status.get("state").and_then(Value::as_str))?;
    Some(text.trim().to_string())
}

/// Longest label we show in the side panel. Citations in particular run long, and
/// this is a scannable summary — full detail belongs in an artifact view.
const MAX_LABEL_CHARS: usize = 96;

/// Best-effort human label for an artifact.
///
/// The key differs per artifact type, so this walks a fallback list. Taken from the
/// `*Payload` TypedDicts in `backend/schemas.py` (2026-07-30):
///
/// | bucket      | field      |
/// |-------------|------------|
/// | datasets    | `title`    |
/// | sources     | `citation` |
/// | reports     | `title`    |
/// | files       | `name`     |
/// | hypotheses  | `question` |
/// | libraries   | `summary`  |
/// | analyses    | `question` |
///
/// An unrecognised artifact is still *counted* rather than dropped — an empty panel
/// would misrepresent work that actually happened.
fn artifact_label(item: &Value) -> String {
    const LABEL_KEYS: [&str; 8] = [
        "title",
        "citation",
        "name",
        "question",
        "summary",
        "filename",
        "label",
        "id",
    ];

    let text = LABEL_KEYS
        .iter()
        .find_map(|key| item.get(key).and_then(Value::as_str))
        // A bare string entry is plausible too.
        .or_else(|| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty());

    match text {
        Some(text) => truncate_label(text),
        None => "(untitled)".to_string(),
    }
}

/// Shorten to [`MAX_LABEL_CHARS`] on a word boundary where possible. Operates on
/// `char`s, never bytes, so multi-byte text can't be split mid-character.
fn truncate_label(text: &str) -> String {
    if text.chars().count() <= MAX_LABEL_CHARS {
        return text.to_string();
    }
    let clipped: String = text.chars().take(MAX_LABEL_CHARS).collect();
    let cut = clipped.rfind(' ').unwrap_or(clipped.len());
    // Only prefer the word boundary if it keeps most of the text.
    let kept = if cut > MAX_LABEL_CHARS / 2 {
        &clipped[..cut]
    } else {
        clipped.as_str()
    };
    format!("{}…", kept.trim_end())
}

/// deepagents' delegation tool. A call to it *is* a subagent being launched.
const DELEGATE_TOOL: &str = "task";

/// Decodes one run's SSE stream into UI events.
///
/// Stateful, unlike the rest of this module, because tool calls arrive in fragments:
/// only the **first** `tool_call_chunk` of a call carries its name and id, later
/// fragments are keyed by `index` alone, and the JSON arguments only mean anything
/// once they are complete. Everything else here is a pure function of one event.
#[derive(Default)]
pub struct TurnDecoder {
    /// Tool calls still streaming their arguments, keyed by (namespace, index).
    calls: HashMap<(String, i64), PendingCall>,
}

struct PendingCall {
    name: String,
    args: String,
    /// Set once a [`TurnEvent::Step`] has been emitted, so each call is reported
    /// exactly once however many fragments it takes.
    announced: bool,
}

impl TurnDecoder {
    /// Map one SSE event onto UI events.
    ///
    /// The `messages` stream mode emits a 2-element array `[message_chunk, metadata]`.
    /// Assistant text arrives as `AIMessageChunk`s whose `content` is either a plain
    /// string or a list of typed blocks.
    pub fn push(&mut self, event: &SseEvent) -> Vec<TurnEvent> {
        match event.name.as_str() {
            "error" => return vec![TurnEvent::Error(summarize_error(&event.data))],
            "metadata" => return vec![TurnEvent::Status("run started".into())],
            // Only the *top-level* snapshot. A subagent's `values|tools:…` carries
            // the same artifacts a few events earlier (measured: `sources: 1` showed
            // up in the subagent's snapshot three events before the coordinator's),
            // so consuming both would render the same outputs twice.
            "values" => {
                let mut events = Vec::new();
                // A paused run's `values` frame carries both the state *and* the
                // interrupt, so decode both rather than treating them as alternatives.
                if let Ok(value) = serde_json::from_str::<Value>(&event.data) {
                    if let Some(request) = decode_interrupt(&value) {
                        events.push(TurnEvent::Approval(request));
                    }
                }
                if let Some(snapshot) = decode_values(&event.data) {
                    events.push(TurnEvent::Snapshot(snapshot));
                }
                return events;
            }
            "custom" => {
                return decode_custom(&event.data)
                    .map(|status| vec![TurnEvent::Status(status)])
                    .unwrap_or_default()
            }
            _ => {}
        }
        // Everything else we care about is a `messages` frame, either top-level or
        // namespaced. `updates` is deliberately not requested: on a measured turn 27
        // of 35 were middleware plumbing (`PIIMiddleware[email].before_model`,
        // `ModelCallLimitMiddleware.*`, …), which is noise, not activity.
        let Some(namespace) = messages_namespace(&event.name) else {
            return Vec::new();
        };
        self.decode_messages(namespace, &event.data)
    }

    fn decode_messages(&mut self, namespace: &str, data: &str) -> Vec<TurnEvent> {
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Vec::new();
        };
        // Expected shape: [chunk, metadata].
        let Some(chunk) = value.get(0) else {
            return Vec::new();
        };
        if chunk.get("type").and_then(Value::as_str) != Some("AIMessageChunk") {
            // `ToolMessage` frames (`type: "tool"`) also arrive here, carrying the
            // whole tool result — up to hundreds of KB. Their content belongs to the
            // outputs panel (via `values`), not to an activity line.
            return Vec::new();
        }

        let agent = agent_ref(namespace, value.get(1));
        let mut events = self.decode_tool_calls(namespace, agent.as_ref(), chunk);

        let text = chunk.get("content").map(extract_text).unwrap_or_default();
        if !text.is_empty() {
            events.push(match &agent {
                Some(agent) => TurnEvent::SubagentToken {
                    agent: agent.clone(),
                    text,
                },
                None => TurnEvent::Token(text),
            });
        }
        events
    }

    /// Turn streaming `tool_call_chunks` into at most one [`TurnEvent::Step`] per call.
    fn decode_tool_calls(
        &mut self,
        namespace: &str,
        agent: Option<&AgentRef>,
        chunk: &Value,
    ) -> Vec<TurnEvent> {
        let Some(fragments) = chunk.get("tool_call_chunks").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for fragment in fragments {
            let index = fragment.get("index").and_then(Value::as_i64).unwrap_or(0);
            let key = (namespace.to_string(), index);

            // A fragment carrying a name starts a fresh call at this index — which is
            // also how a second call reusing index 0 later in the turn is handled.
            if let Some(name) = fragment.get("name").and_then(Value::as_str) {
                self.calls.insert(
                    key.clone(),
                    PendingCall {
                        name: name.to_string(),
                        args: String::new(),
                        announced: false,
                    },
                );
            }
            let Some(call) = self.calls.get_mut(&key) else {
                continue;
            };
            if let Some(args) = fragment.get("args").and_then(Value::as_str) {
                call.args.push_str(args);
            }
            if call.announced {
                continue;
            }
            // A normal tool is worth announcing the moment we know its name. A
            // delegation waits, because its useful label ("delegating to
            // academic_researcher") lives in arguments that are still arriving.
            if call.name != DELEGATE_TOOL {
                call.announced = true;
                events.push(TurnEvent::Step {
                    agent: agent.cloned(),
                    label: call.name.clone(),
                });
                continue;
            }
            if let Some(label) = delegation_label(&call.args) {
                call.announced = true;
                events.push(TurnEvent::Step {
                    agent: agent.cloned(),
                    label,
                });
            }
        }
        events
    }
}

/// The namespace part of a `messages` event name, or `None` for other events.
/// Top-level frames are plain `messages`, hence the empty string.
fn messages_namespace(event_name: &str) -> Option<&str> {
    if event_name == "messages" {
        return Some("");
    }
    event_name.strip_prefix("messages|")
}

/// Identify the subagent a frame came from, if any.
///
/// A subagent's events are namespaced `tools:<uuid>`; the coordinator's carry no
/// namespace. We key on the **whole** namespace rather than the first `tools:`
/// segment (what the JS SDK does), so a nested delegation `tools:a|tools:b` gets its
/// own group under its own name instead of being folded into its parent's group
/// while wearing the inner agent's name.
///
/// Note the metadata's own `langgraph_checkpoint_ns` is *not* usable as the
/// discriminator: measured on a real turn, top-level coordinator frames carry
/// `model:<uuid>` there, so it names a node, not a delegation.
fn agent_ref(namespace: &str, metadata: Option<&Value>) -> Option<AgentRef> {
    if !namespace
        .split('|')
        .any(|segment| segment.starts_with("tools:"))
    {
        return None;
    }
    let name = metadata
        .and_then(|metadata| metadata.get("lc_agent_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("subagent");
    Some(AgentRef {
        ns: namespace.to_string(),
        name: name.to_string(),
    })
}

/// Label for a `task` delegation, once its streamed JSON arguments are complete.
///
/// Returns `None` while they are still partial — streaming JSON only parses once
/// closed, which is exactly the "is it complete yet" signal we need, with no
/// dependence on a `chunk_position` marker the backend leaves null. The
/// `subagent_type` shape check is the guard the web client applies too, so a
/// half-formed value can never become a step label.
fn delegation_label(args: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(args).ok()?;
    let subagent = parsed.get("subagent_type").and_then(Value::as_str)?;
    if !looks_like_subagent_type(subagent) {
        return None;
    }
    let description = parsed
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|description| !description.is_empty());
    let label = match description {
        Some(description) => format!("delegating to {subagent} — {description}"),
        None => format!("delegating to {subagent}"),
    };
    Some(truncate_label(&label))
}

/// The web client's `^[a-zA-Z][a-zA-Z0-9_-]{2,49}$`, without pulling in a regex crate.
fn looks_like_subagent_type(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && (3..=50).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Render a subagent's streamed text for the activity trace.
///
/// A subagent's "text" is often not prose: measured on a real delegation (plan §15),
/// `academic_researcher` streamed its entire answer as one JSON object — its
/// structured response — so dumping the raw text would show the user a wall of
/// braces. When the text parses as a JSON object we lift the readable parts out.
/// Partial (still streaming) or plain-prose text is returned untouched, which also
/// means the user watches the JSON assemble live and then sees it resolve into a
/// sentence.
pub fn summarize_agent_result(text: &str) -> String {
    let text = text.trim();
    let Some(object) = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| match value {
            Value::Object(object) => Some(object),
            _ => None,
        })
    else {
        return text.to_string();
    };

    let mut parts: Vec<String> = Vec::new();
    if let Some(summary) = object.get("summary").and_then(Value::as_str) {
        let summary = summary.trim();
        if !summary.is_empty() {
            parts.push(summary.to_string());
        }
    }
    // Everything else is counted rather than dumped: a `sources` list is 20 lines of
    // citation the outputs panel already renders properly.
    for (key, value) in &object {
        if key == "summary" {
            continue;
        }
        if let Value::Array(items) = value {
            if !items.is_empty() {
                parts.push(format!("{} {key}", items.len()));
            }
        }
    }

    if parts.is_empty() {
        text.to_string()
    } else {
        parts.join(" · ")
    }
}

/// `content` is either a string or a list of blocks like `{"type":"text","text":…}`.
fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

fn summarize_error(data: &str) -> String {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|v| {
            v.get("message")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| data.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a single event in isolation. Anything that depends on *sequence*
    /// (tool-call argument fragments) drives a `TurnDecoder` directly instead.
    fn decode(event: &SseEvent) -> Vec<TurnEvent> {
        TurnDecoder::default().push(event)
    }

    /// Build the SSE frames for one streamed tool call: the first fragment names it,
    /// the rest carry argument text only — the shape measured on a real turn.
    fn tool_call_frames(event_name: &str, name: &str, args: &[&str]) -> Vec<SseEvent> {
        let mut frames = Vec::new();
        let mut fragments: Vec<Value> = vec![json!({
            "name": name, "args": "", "id": "call_1", "index": 0, "type": "tool_call_chunk"
        })];
        for arg in args {
            fragments.push(json!({
                "name": null, "args": arg, "id": null, "index": 0, "type": "tool_call_chunk"
            }));
        }
        for fragment in fragments {
            frames.push(SseEvent {
                name: event_name.to_string(),
                data: json!([
                    {"type": "AIMessageChunk", "content": "", "tool_call_chunks": [fragment]},
                    {"lc_agent_name": "academic_researcher"}
                ])
                .to_string(),
            });
        }
        frames
    }

    fn drain(decoder: &mut TurnDecoder, frames: &[SseEvent]) -> Vec<TurnEvent> {
        frames
            .iter()
            .flat_map(|frame| decoder.push(frame))
            .collect()
    }

    fn tokens(events: &[TurnEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                TurnEvent::Token(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn requests_the_tuple_stream_mode() {
        // Regression guard: `messages` (without `-tuple`) silently yields zero
        // tokens, because the server then emits `messages/partial` frames.
        // `values` rides alongside for the artifacts/spine snapshot — verified on a
        // live backend that asking for both still produces `event: messages`.
        let body = run_request_body("hi", None);
        assert_eq!(
            body["stream_mode"],
            json!(["messages-tuple", "values", "custom"])
        );
        assert_eq!(body["assistant_id"], "agent");
        assert_eq!(body["input"]["messages"][0]["type"], "human");
        assert_eq!(body["input"]["messages"][0]["content"], "hi");
        // Without subgraphs a delegated turn streams nothing while the subagent
        // works — the silent gap the activity trace exists to close.
        assert_eq!(body["stream_subgraphs"], json!(true));
    }

    #[test]
    fn sends_the_model_choice_and_key_in_the_run_config() {
        // The contract measured in `backend/models.py`: `model_config.default` is
        // `provider::model_id`, keys ride in `__llm_keys` per provider, and
        // `storage_mode: "client"` keeps the server-side Vault path dormant.
        let model = ModelChoice {
            spec: "custom::openai/gpt-4o-mini".into(),
            provider: "custom".into(),
            api_key: Some("sk-test".into()),
            base_url: Some("https://openrouter.ai/api/v1".into()),
        };
        let body = run_request_body("hi", Some(&model));
        let configurable = &body["config"]["configurable"];
        assert_eq!(configurable["model_config"]["default"], "custom::openai/gpt-4o-mini");
        assert_eq!(configurable["model_config"]["storage_mode"], "client");
        assert_eq!(configurable["__llm_keys"]["custom"]["api_key"], "sk-test");
        assert_eq!(
            configurable["__llm_keys"]["custom"]["base_url"],
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(configurable["__is_for_execution__"], true);
        // The limit still has to be there — it is what keeps a multi-subagent turn from
        // hitting LangGraph's 25-superstep ceiling.
        assert_eq!(body["config"]["recursion_limit"], 10_000);

        // A resume carries the same routing: the continuation must not silently switch
        // model or lose the key mid-turn.
        let resumed = resume_request_body(&[Decision::Approve], Some(&model));
        assert_eq!(
            resumed["config"]["configurable"]["__llm_keys"],
            configurable["__llm_keys"]
        );
        assert_eq!(resumed["command"]["resume"]["decisions"][0]["type"], "approve");
    }

    #[test]
    fn omits_client_key_storage_when_there_is_no_key() {
        // Claiming client-only storage with nothing to supply would tell the backend to
        // skip its own lookup and then find no key at all.
        let model = ModelChoice {
            spec: "openai::gpt-5.4".into(),
            provider: "openai".into(),
            api_key: None,
            base_url: None,
        };
        let body = run_request_body("hi", Some(&model));
        let configurable = &body["config"]["configurable"];
        assert_eq!(configurable["model_config"]["default"], "openai::gpt-5.4");
        assert!(configurable["model_config"]["storage_mode"].is_null());
        assert!(configurable["__llm_keys"].is_null());
    }

    #[test]
    fn background_tasks_are_found_with_the_thread_that_can_answer_them() {
        // `thread_id` is the whole point: the worker runs on its own thread, and until the
        // client watched *that* thread a background task that hit the execute gate simply
        // hung — the approval was real, on a thread nothing in the UI ever looked at.
        // Field names from deepagents' AsyncTask.
        let snapshot = decode_values(
            &json!({
                "artifacts": {},
                "async_tasks": {
                    "th-2": {
                        "task_id": "th-2", "thread_id": "th-2",
                        "agent_name": "background_worker", "status": "running",
                        "description": "Analyse the yield data"
                    },
                    "th-1": {
                        "task_id": "th-1", "thread_id": "th-1",
                        "agent_name": "background_worker", "status": "success"
                    }
                }
            })
            .to_string(),
        )
        .expect("a snapshot");

        assert_eq!(snapshot.tasks.len(), 2);
        // A map has no order; the panel must not reshuffle every frame.
        assert_eq!(snapshot.tasks[0].task_id, "th-1");
        assert!(snapshot.tasks[0].is_finished() && snapshot.tasks[0].succeeded());
        assert!(!snapshot.tasks[1].is_finished());
        assert_eq!(snapshot.tasks[1].thread_id, "th-2");
        assert_eq!(snapshot.tasks[1].description, "Analyse the yield data");
        // Nothing is waiting until a thread poll says so.
        assert!(!snapshot.tasks[1].needs_approval());
    }

    #[test]
    fn a_conversation_is_named_by_its_first_question() {
        // Short enough to stand as the title unchanged.
        assert_eq!(title_from_prompt("What drives yield?"), "What drives yield?");
        // Whitespace from a pasted prompt would otherwise reach the sidebar verbatim.
        assert_eq!(title_from_prompt("  many\n\n spaces  "), "many spaces");

        // Long prompts are cut on a word boundary — a title ending mid-word looks like a
        // rendering bug rather than an abbreviation.
        let long = "Genera un dataset sintético de 400 parcelas de papa y ajusta un modelo";
        let title = title_from_prompt(long);
        assert!(title.ends_with('…'), "{title}");
        assert!(title.chars().count() <= 49, "{title}");
        assert!(long.starts_with(title.trim_end_matches('…').trim()), "{title}");

        // One unbroken word longer than the limit still has to produce something.
        let wall = "x".repeat(200);
        let title = title_from_prompt(&wall);
        assert!(title.chars().count() <= 49, "{title}");
        assert!(title.ends_with('…'));
    }

    #[test]
    fn a_stored_conversation_reduces_to_what_is_worth_showing() {
        // The shape `POST /threads/search` returns. A researcher's own name wins; a
        // thread that has none is labelled rather than left blank.
        let named = json!({"thread_id": "t1", "metadata": {"title": "Potato yield"}, "updated_at": "2026-08-01T10:00:00Z"});
        let decoded = decode_conversation(&named).expect("a conversation");
        assert_eq!(decoded.title, "Potato yield");
        assert_eq!(decoded.thread_id, "t1");

        let unnamed = json!({"thread_id": "t2", "metadata": {}});
        assert_eq!(
            decode_conversation(&unnamed).expect("a conversation").title,
            "New conversation"
        );
        // No id, nothing to open.
        assert_eq!(decode_conversation(&json!({"metadata": {}})), None);

        // Reopening shows the conversation, not its plumbing: tool traffic and empty
        // assistant turns are dropped, and both content shapes are understood.
        assert_eq!(
            decode_stored_message(&json!({"type": "human", "content": "hola"})),
            Some(("you".to_string(), "hola".to_string()))
        );
        assert_eq!(
            decode_stored_message(
                &json!({"type": "ai", "content": [{"type": "text", "text": "hi "}, {"type": "text", "text": "there"}]})
            ),
            Some(("mini-me".to_string(), "hi there".to_string()))
        );
        assert_eq!(decode_stored_message(&json!({"type": "tool", "content": "{}"})), None);
        assert_eq!(decode_stored_message(&json!({"type": "ai", "content": "  "})), None);
    }

    #[test]
    fn a_background_worker_says_which_subagent_it_is_running() {
        // Delegation: `task(subagent_type=…)`. The subagent's name is the only part of
        // this a researcher cares about — "running" alone said nothing for ten minutes.
        let delegating = json!({"values": {"messages": [
            {"tool_calls": [{"name": "write_todos", "args": {}}]},
            {"tool_calls": [{"name": "task", "args": {"subagent_type": "academic_researcher"}}]},
        ]}});
        assert_eq!(
            last_activity(&delegating).as_deref(),
            Some("academic researcher"),
            "the newest call wins, and underscores are not for reading"
        );

        // A plain tool is reported under its own name.
        let executing = json!({"values": {"messages": [
            {"tool_calls": [{"name": "execute", "args": {"command": "python3 fit.py"}}]},
        ]}});
        assert_eq!(last_activity(&executing).as_deref(), Some("execute"));

        // Messages with no tool calls are skipped rather than ending the search — the
        // worker's own commentary sits between its calls.
        let chatty = json!({"values": {"messages": [
            {"tool_calls": [{"name": "read_file", "args": {}}]},
            {"content": "Let me look at that."},
        ]}});
        assert_eq!(last_activity(&chatty).as_deref(), Some("read file"));

        // Nothing has run yet, and a state with no messages at all.
        assert_eq!(last_activity(&json!({"values": {"messages": []}})), None);
        assert_eq!(last_activity(&json!({})), None);
    }

    #[test]
    fn a_failed_background_run_reports_what_went_wrong() {
        // The shape `/threads/{id}/state` returns: the failure hangs off the pending task,
        // and `next` is *not* empty because the task that died is still pending. Both
        // matter — reading only `next` is what made a dead worker read as "running", and
        // reading only the run record is why the middleware says "The async subagent
        // encountered an error" and nothing else (docs §38).
        let traceback = "Traceback (most recent call last):\n  File \"agent.py\", line 9\n\
             langgraph.errors.GraphRecursionError: Recursion limit of 25 reached";
        assert_eq!(
            error_text(&json!(traceback)).as_deref(),
            Some("langgraph.errors.GraphRecursionError: Recursion limit of 25 reached"),
            "the exception line is the part that names the cause"
        );

        // An object-shaped error must not render as JSON at the user.
        assert_eq!(
            error_text(&json!({"message": "no API key configured", "type": "ValueError"})).as_deref(),
            Some("no API key configured")
        );
        // A task that simply has not failed contributes nothing.
        assert_eq!(error_text(&json!(null)), None);
        assert_eq!(error_text(&json!("   ")), None);
    }

    #[test]
    fn an_interrupted_background_task_is_not_mistaken_for_a_finished_one() {
        // `interrupted` means "waiting for a person", not "over". Treating it as terminal
        // would stop the watcher on the exact tick that needed a human.
        let waiting = AsyncTask {
            task_id: "t".into(),
            thread_id: "t".into(),
            agent_name: "background_worker".into(),
            status: "interrupted".into(),
            description: String::new(),
            pending: None,
            error: None,
            activity: None,
        };
        assert!(!waiting.is_finished(), "interrupted is not terminal");
        for status in ["success", "error", "timeout", "cancelled"] {
            let done = AsyncTask {
                status: status.to_string(),
                ..waiting.clone()
            };
            assert!(done.is_finished(), "{status}");
            assert_eq!(done.succeeded(), status == "success", "{status}");
        }
        for status in ["running", "pending"] {
            let going = AsyncTask {
                status: status.to_string(),
                ..waiting.clone()
            };
            assert!(!going.is_finished(), "{status}");
        }
    }

    #[test]
    fn a_running_long_job_is_found_and_pollable() {
        // Field names from HypothesisArtifactPayload / DataAnalysisArtifactPayload
        // (backend/schemas.py:353,388). Getting these wrong means the app silently never
        // collects a 40-minute run — which is what it did before this existed.
        let snapshot = decode_values(
            &json!({
                "artifacts": {
                    "hypotheses": [{
                        "question": "¿qué papa es más resistente?",
                        "status": "running",
                        "task_id": "1f0a2b3c-4d5e-6f70-8192-a3b4c5d6e7f8"
                    }],
                    "analyses": [{
                        "question": "What drives yield?",
                        "status": "running",
                        "task_id": "aaaabbbb-cccc-dddd-eeee-ffff00001111",
                        "context_id": "ctx-42"
                    }]
                }
            })
            .to_string(),
        )
        .expect("a snapshot");

        assert_eq!(snapshot.jobs.len(), 2, "{:?}", snapshot.jobs);
        let theorizer = &snapshot.jobs[0];
        assert_eq!(theorizer.kind, JobKind::Theorizer);
        assert!(!theorizer.is_finished());

        // The question rides in the query string, and the theorizer route uses it when
        // persisting results — so accented Spanish has to survive encoding intact.
        let route = theorizer.route("thread-1");
        assert!(route.starts_with("/theorizer/thread-1/1f0a2b3c-"), "{route}");
        assert!(route.contains("q=%C2%BFqu%C3%A9%20papa"), "{route}");
        assert!(!route.contains(' '), "a raw space would break the request: {route}");

        let analysis = &snapshot.jobs[1];
        assert_eq!(analysis.kind, JobKind::Analysis);
        assert!(analysis.route("t").contains("ctx=ctx-42"), "{}", analysis.route("t"));
    }

    #[test]
    fn a_job_with_no_task_id_is_not_offered_as_something_to_wait_for() {
        // A completed theorizer artifact carries results but no id to poll. Listing it as
        // a running job would leave a spinner nobody can ever resolve.
        let snapshot = decode_values(
            &json!({
                "artifacts": {
                    "hypotheses": [
                        {"question": "done already", "status": "completed"},
                        {"question": "no id", "status": "running", "task_id": "  "}
                    ],
                    "sources": [{"citation": "Love MI et al. 2014"}]
                }
            })
            .to_string(),
        )
        .expect("a snapshot");
        assert!(snapshot.jobs.is_empty(), "{:?}", snapshot.jobs);
        assert_eq!(snapshot.buckets.len(), 2, "the artifacts still show up");
    }

    #[test]
    fn every_terminal_state_the_backend_can_report_stops_the_poll() {
        // `unavailable` is the subtle one: the thread's sandbox is gone, so no further
        // poll can ever tell us anything and looping would burn requests forever.
        for status in [
            "completed",
            "failed",
            "canceled",
            "unavailable",
            "error",
        ] {
            let job = Job {
                kind: JobKind::Theorizer,
                task_id: "x".into(),
                question: String::new(),
                context_id: None,
                status: status.to_string(),
            };
            assert!(job.is_finished(), "{status}");
            assert_eq!(job.succeeded(), status == "completed", "{status}");
        }
        for status in ["running", "input-required", "submitted"] {
            let job = Job {
                kind: JobKind::Analysis,
                task_id: "x".into(),
                question: String::new(),
                context_id: None,
                status: status.to_string(),
            };
            assert!(!job.is_finished(), "{status}");
        }
    }

    #[test]
    fn decodes_artifacts_and_the_nested_spine_from_values() {
        // Shape copied from a live `values` payload (2026-07-30).
        let data = json!({
            "messages": [],
            "artifacts": {
                "datasets": [],
                "sources": [{"title": "A paper"}, {"title": "Another"}],
                "reports": [],
                "files": [{"name": "eda.png"}],
                "hypotheses": [{"statement": "no label field here"}],
                "libraries": [],
                "analyses": [],
                "edges": [{"from": "a", "to": "b"}],
                "project": {"mission": "M", "completed": ["c"], "pending": [], "suggestions": []}
            }
        })
        .to_string();

        let decoded = decode(&SseEvent {
            name: "values".into(),
            data,
        });
        let [TurnEvent::Snapshot(snapshot)] = decoded.as_slice() else {
            panic!("expected exactly one snapshot, got {decoded:?}");
        };

        // Empty buckets are dropped, `edges` is never surfaced.
        let names: Vec<&str> = snapshot.buckets.iter().map(|b| b.name).collect();
        assert_eq!(names, vec!["sources", "files", "hypotheses"]);
        assert_eq!(snapshot.buckets[0].items, vec!["A paper", "Another"]);
        assert_eq!(snapshot.buckets[1].items, vec!["eda.png"]);
        // An item with no recognised label is still counted, not dropped.
        assert_eq!(snapshot.buckets[2].items, vec!["(untitled)"]);

        let project = snapshot.project.as_ref().expect("spine rides along");
        assert_eq!(project.mission, "M");
        assert_eq!(project.completed, vec!["c"]);
    }

    #[test]
    fn surfaces_sandbox_provisioning_as_status() {
        // Live shape (2026-07-30). Without this the first turn on a cold thread
        // looks stuck while the sandbox is created.
        let decoded = decode(&SseEvent {
            name: "custom".into(),
            data: json!({"sandbox_status": {"state": "preparing", "message": "Creating sandbox…"}})
                .to_string(),
        });
        assert_eq!(
            decoded,
            vec![TurnEvent::Status("Creating sandbox…".into())]
        );

        // Falls back to the state when no message is given.
        let decoded = decode(&SseEvent {
            name: "custom".into(),
            data: json!({"sandbox_status": {"state": "ready"}}).to_string(),
        });
        assert_eq!(decoded, vec![TurnEvent::Status("ready".into())]);

        // Unrelated custom payloads are ignored rather than shown as noise.
        let decoded = decode(&SseEvent {
            name: "custom".into(),
            data: json!({"something_else": 1}).to_string(),
        });
        assert!(decoded.is_empty(), "got {decoded:?}");
    }

    #[test]
    fn labels_every_artifact_kind_from_its_own_field() {
        // Regression guard: a real `sources` artifact carries `citation`, not
        // `title`, and rendered as "(untitled)" until this list covered it.
        let cases = [
            (json!({"title": "A dataset"}), "A dataset"),
            (json!({"citation": "Love MI et al. 2014."}), "Love MI et al. 2014."),
            (json!({"name": "eda.png"}), "eda.png"),
            (json!({"question": "Does X affect Y?"}), "Does X affect Y?"),
            (json!({"summary": "Indexed 12 papers"}), "Indexed 12 papers"),
            (json!("a bare string"), "a bare string"),
            (json!({"unknown": "x"}), "(untitled)"),
        ];
        for (input, expected) in cases {
            assert_eq!(artifact_label(&input), expected, "for {input}");
        }
    }

    #[test]
    fn truncates_long_labels_without_splitting_characters() {
        let long = "á".repeat(200);
        let label = artifact_label(&json!({"citation": long}));
        // Truncated, and still valid UTF-8 with every char intact.
        assert!(label.chars().count() <= MAX_LABEL_CHARS + 1, "{label}");
        assert!(label.ends_with('…'));
        assert!(label.chars().filter(|c| *c == 'á').count() > 0);

        // Short labels are untouched.
        assert_eq!(artifact_label(&json!({"title": "short"})), "short");
    }

    #[test]
    fn ignores_a_values_payload_with_nothing_to_show() {
        // Must not blank an already-populated panel.
        let data = json!({"messages": [], "artifacts": {"datasets": [], "edges": []}}).to_string();
        let decoded = decode(&SseEvent {
            name: "values".into(),
            data,
        });
        assert!(decoded.is_empty(), "got {decoded:?}");
    }

    #[test]
    fn frames_events_split_across_arbitrary_chunk_boundaries() {
        let wire = "event: metadata\ndata: {\"run_id\":\"r1\"}\n\n\
                    event: messages\ndata: [{\"type\":\"AIMessageChunk\",\"id\":\"m1\",\"content\":\"Hel\"},{}]\n\n\
                    event: messages\ndata: [{\"type\":\"AIMessageChunk\",\"id\":\"m1\",\"content\":\"lo\"},{}]\n\n";

        // Feed one byte at a time — the pathological split.
        let mut decoder = SseDecoder::default();
        let mut decoded = Vec::new();
        for byte in wire.as_bytes() {
            for event in decoder.push(&[*byte]) {
                decoded.extend(decode(&event));
            }
        }

        assert_eq!(tokens(&decoded), "Hello");
        assert!(decoded.contains(&TurnEvent::Status("run started".into())));
    }

    #[test]
    fn decodes_string_and_block_content() {
        let string_form = SseEvent {
            name: "messages".into(),
            data: r#"[{"type":"AIMessageChunk","id":"m1","content":"plain"},{}]"#.into(),
        };
        let block_form = SseEvent {
            name: "messages".into(),
            data: r#"[{"type":"AIMessageChunk","id":"m1","content":[{"type":"text","text":"blocks"},{"type":"other","text":"skip"}]},{}]"#.into(),
        };
        assert_eq!(
            decode(&string_form),
            vec![TurnEvent::Token("plain".into())]
        );
        assert_eq!(
            decode(&block_form),
            vec![TurnEvent::Token("blocks".into())]
        );
    }

    #[test]
    fn ignores_non_assistant_chunks_and_unknown_events() {
        let human = SseEvent {
            name: "messages".into(),
            data: r#"[{"type":"HumanMessage","content":"hi"},{}]"#.into(),
        };
        let values = SseEvent {
            name: "values".into(),
            data: r#"{"messages":[]}"#.into(),
        };
        assert!(decode(&human).is_empty());
        assert!(decode(&values).is_empty());
    }

    #[test]
    fn attributes_subagent_text_to_its_agent_and_leaves_coordinator_text_alone() {
        // Namespace + `lc_agent_name` as measured on a real delegation.
        let sub = SseEvent {
            name: "messages|tools:d6c187d3-3eef-774e-4c2f-7151df99cffb".into(),
            data: json!([
                {"type": "AIMessageChunk", "id": "m2", "content": "sub"},
                {"lc_agent_name": "academic_researcher", "langgraph_node": "model"}
            ])
            .to_string(),
        };
        assert_eq!(
            decode(&sub),
            vec![TurnEvent::SubagentToken {
                agent: AgentRef {
                    ns: "tools:d6c187d3-3eef-774e-4c2f-7151df99cffb".into(),
                    name: "academic_researcher".into(),
                },
                text: "sub".into(),
            }]
        );

        // A top-level frame stays a plain token even though its own metadata carries
        // a `model:<uuid>` checkpoint namespace — which is why the event name, not
        // the metadata, is the discriminator.
        let coordinator = SseEvent {
            name: "messages".into(),
            data: json!([
                {"type": "AIMessageChunk", "id": "m1", "content": "answer"},
                {"langgraph_checkpoint_ns": "model:27ee27ea-dda1-bd14-912e-e10f"}
            ])
            .to_string(),
        };
        assert_eq!(
            decode(&coordinator),
            vec![TurnEvent::Token("answer".into())]
        );
    }

    #[test]
    fn names_a_subagent_even_when_the_backend_omits_lc_agent_name() {
        let sub = SseEvent {
            name: "messages|tools:abc".into(),
            data: json!([{"type": "AIMessageChunk", "content": "x"}, {}]).to_string(),
        };
        let decoded = decode(&sub);
        let [TurnEvent::SubagentToken { agent, .. }] = decoded.as_slice() else {
            panic!("expected a subagent token");
        };
        assert_eq!(agent.name, "subagent");
    }

    #[test]
    fn groups_nested_delegations_separately_from_their_parent() {
        // The JS SDK keys on the *first* `tools:` segment, which would file an inner
        // agent's work under its parent's group while labelling it with the inner
        // agent's name. We key on the whole namespace instead.
        let outer = agent_ref("tools:aaa", Some(&json!({"lc_agent_name": "coordinator_two"})));
        let inner = agent_ref(
            "tools:aaa|tools:bbb",
            Some(&json!({"lc_agent_name": "report_writer"})),
        );
        assert_ne!(outer.unwrap().ns, inner.unwrap().ns);

        // `model:<uuid>` is a node, not a delegation.
        assert!(agent_ref("model:abc", None).is_none());
        assert!(agent_ref("", None).is_none());
    }

    #[test]
    fn announces_a_delegation_only_once_its_streamed_args_are_complete() {
        // Measured shape: `{"subagent_type":"academic_researcher","description":"…"}`
        // arrives in fragments, and only the first one names the tool.
        let frames = tool_call_frames(
            "messages",
            DELEGATE_TOOL,
            &[
                r#"{"subagent_type":"aca"#,
                r#"demic_researcher","description":"Find the canonical DESeq2 paper."#,
                r#""}"#,
            ],
        );
        let mut decoder = TurnDecoder::default();
        let events = drain(&mut decoder, &frames);

        // Exactly one step, and not until the JSON closed — a partial
        // `"subagent_type":"aca` must never become a label.
        assert_eq!(
            events,
            vec![TurnEvent::Step {
                agent: None,
                label: "delegating to academic_researcher — Find the canonical DESeq2 paper.".into(),
            }]
        );
        // Replaying the closing fragment must not announce a second time.
        assert!(decoder.push(frames.last().unwrap()).is_empty());
    }

    #[test]
    fn announces_an_ordinary_tool_call_on_sight_and_attributes_it() {
        // A subagent's own tool call: the useful label is the name, which arrives in
        // the first fragment, so there is nothing to wait for.
        let frames = tool_call_frames(
            "messages|tools:d6c187d3",
            "search_paper_by_title",
            &[r#"{"title":"Moderated estimation"#, r#""}"#],
        );
        let mut decoder = TurnDecoder::default();
        assert_eq!(
            drain(&mut decoder, &frames),
            vec![TurnEvent::Step {
                agent: Some(AgentRef {
                    ns: "tools:d6c187d3".into(),
                    name: "academic_researcher".into(),
                }),
                label: "search_paper_by_title".into(),
            }]
        );
    }

    #[test]
    fn rejects_a_malformed_subagent_type() {
        assert!(delegation_label(r#"{"subagent_type":"9bad"}"#).is_none());
        assert!(delegation_label(r#"{"subagent_type":"ab"}"#).is_none());
        assert!(delegation_label(r#"{"subagent_type":"has space"}"#).is_none());
        assert!(delegation_label(r#"{"description":"no type"}"#).is_none());
        assert_eq!(
            delegation_label(r#"{"subagent_type":"report_writer"}"#).as_deref(),
            Some("delegating to report_writer"),
        );
    }

    #[test]
    fn ignores_a_subagents_own_state_snapshot() {
        // It carries the same artifacts as the coordinator's `values` a few events
        // later; consuming both would render the outputs twice.
        let sub = SseEvent {
            name: "values|tools:d6c187d3".into(),
            data: json!({"artifacts": {"sources": [{"citation": "c"}]}}).to_string(),
        };
        assert!(decode(&sub).is_empty(), "{:?}", decode(&sub));
    }

    #[test]
    fn summarizes_a_structured_subagent_result_instead_of_dumping_json() {
        // A real `academic_researcher` reply: the whole answer is one JSON object.
        let structured = json!({
            "summary": "The canonical DESeq2 paper is the 2014 Genome Biology article.",
            "sources": [{"citation": "Love MI et al. 2014."}],
        })
        .to_string();
        assert_eq!(
            summarize_agent_result(&structured),
            "The canonical DESeq2 paper is the 2014 Genome Biology article. · 1 sources",
        );

        // Prose is untouched, and so is a partial object still streaming in — which
        // is what makes the trace look alive rather than empty.
        assert_eq!(summarize_agent_result("  plain prose  "), "plain prose");
        assert_eq!(summarize_agent_result(r#"{"summary":"half"#), r#"{"summary":"half"#);
    }

    #[test]
    fn surfaces_error_events() {
        let err = SseEvent {
            name: "error".into(),
            data: r#"{"message":"boom"}"#.into(),
        };
        assert_eq!(
            decode(&err),
            vec![TurnEvent::Error("boom".into())]
        );
    }

    #[test]
    fn handles_crlf_terminators() {
        let mut decoder = SseDecoder::default();
        let events = decoder.push(
            b"event: messages\r\ndata: [{\"type\":\"AIMessageChunk\",\"content\":\"crlf\"},{}]\r\n\r\n",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(
            decode(&events[0]),
            vec![TurnEvent::Token("crlf".into())]
        );
    }
}
