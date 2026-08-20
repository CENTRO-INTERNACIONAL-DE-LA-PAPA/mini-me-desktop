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
//!   nested at `artifacts.project`)
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

/// The one internal assistant used to make LangGraph run the graph factory during startup.
///
/// A stable UUID is load-bearing: `POST /assistants` rejects the graph id (`agent`) in this API
/// version, while a fresh UUID on every launch would leave one invisible assistant behind per
/// session. `if_exists: do_nothing` makes simultaneous/repeated warm-ups converge on this record.
const GRAPH_WARM_UP_ASSISTANT_ID: &str = "709fdf35-66dd-4c0a-bc5f-35d0f33cb91e";

/// Bound a dependency outage rather than leaving the desktop saying "starting" forever.
///
/// A healthy run on the Windows machine took 14-15 seconds because the graph factory connected to
/// four MCP servers in sequence. Sixty seconds leaves room for that measured path while matching
/// the backend supervisor's existing one-minute health budget. If hotel Wi-Fi black-holes an MCP
/// host, the researcher gets the app back and the ordinary request can report the real error.
const GRAPH_WARM_UP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// The run is live, and this is the id LangGraph filed it under.
    ///
    /// Carried in the first `event: metadata` frame — verified against the captured stream
    /// in `tests/fixtures/delegated-turn.sse`, whose first metadata frame is
    /// `{"run_id":"019fb670-…","attempt":1}`. It used to be discarded, which is why the stop
    /// button had nothing to stop with: cancelling needs a run to name (docs §63).
    Started { run_id: String },
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
    /// Which interrupt raised this, as `Interrupt.id` — an xxh3-128 hex digest.
    ///
    /// **Load-bearing when more than one specialist stops at once.** LangGraph accepts a bare
    /// `Command(resume=…)` only while exactly one interrupt is pending; past that it refuses, in
    /// its own words: *"When there are multiple pending interrupts, you must specify the interrupt
    /// id when resuming"* (`pregel/_loop.py:733`). Three subagents launched together each hit the
    /// `execute` gate, and the approval could not be delivered to any of them (§215).
    ///
    /// Empty when the backend did not send one, which is the older shape and still resumes the
    /// legacy way — correct there, because a backend that cannot name its interrupts can only have
    /// had one.
    pub interrupt: String,
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
    /// Reports, with their bodies. See [`Report`].
    pub reports: Vec<Report>,
    /// The coordinator's own plan for this turn, when it wrote one. See [`Todo`].
    pub todos: Vec<Todo>,
    /// Datasets the explorer recommended, **whole**. See [`Dataset`].
    pub datasets: Vec<Dataset>,
    /// Documents the PDF librarian indexed, whole. See [`Document`].
    pub documents: Vec<Document>,
    /// Citations gathered so far, **whole**.
    ///
    /// Separate from the `sources` bucket, whose items are truncated to 96 characters for a
    /// side panel that has to stay scannable. A rendered report's citation list must not be —
    /// a bibliography ending in `…` is not a bibliography.
    pub sources: Vec<Source>,
}

/// One step of an agent's own plan.
///
/// **The agent writes these; we only read them.** `TodoListMiddleware` gives every agent a
/// `write_todos` tool and keeps the result in state as `todos`, and a background worker calls it
/// between commands — which is what makes a six-minute run describable rather than merely long
/// (§209). Nothing here is derived, estimated or filled in: an empty plan renders as no plan.
///
/// Each agent's list is its own. `deepagents`' `_EXCLUDED_STATE_KEYS` keeps `todos` out of what a
/// subagent inherits, so a worker's plan is the worker's, which is what lets a plan belong to
/// exactly one row on screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Todo {
    pub content: String,
    /// `pending`, `in_progress` or `completed`, as `langchain`'s `Todo` defines them. Kept as the
    /// string it arrived as rather than an enum: an unknown fourth value should render as "not
    /// finished", not panic or silently count as done.
    pub status: String,
}

impl Todo {
    pub fn is_done(&self) -> bool {
        self.status == "completed"
    }

    pub fn is_running(&self) -> bool {
        self.status == "in_progress"
    }

    /// The mark this step gets in a list: done, doing, or waiting.
    pub fn mark(&self) -> &'static str {
        if self.is_done() {
            "✓"
        } else if self.is_running() {
            "◐"
        } else {
            "○"
        }
    }
}

/// How far through a plan the work is — `(completed, total)`.
///
/// `None` when there is no plan, which is not the same as nothing done: `write_todos` is optional
/// and the model skips it for simple requests. A "0 of 0" would be a claim about work that was
/// never planned.
pub fn plan_progress(todos: &[Todo]) -> Option<(usize, usize)> {
    if todos.is_empty() {
        return None;
    }
    Some((todos.iter().filter(|todo| todo.is_done()).count(), todos.len()))
}

/// Read an agent's plan out of a `values`-shaped payload.
fn decode_todos(values: &Value) -> Vec<Todo> {
    values
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .filter_map(|todo| {
                    let content = todo.get("content").and_then(Value::as_str)?.trim();
                    if content.is_empty() {
                        return None;
                    }
                    Some(Todo {
                        content: content.to_string(),
                        status: todo
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("pending")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One work this conversation cited.
///
/// # Why the link is a field and not something read out of the citation
///
/// `citation` is **written by the model.** `AcademicSourceFinding.citation`
/// (`backend/schemas.py:31`) is a Pydantic field described as *"APA-style or equivalent citation
/// for the source"*, so the model composes the authors, the year, the journal — and the DOI — as
/// one sentence. A DOI produced that way is as reliable as any other detail an LLM writes from
/// memory, which is to say: usually right, and wrong without warning.
///
/// `link` is the *separate* field the same payload carries
/// (`SourceArtifactPayload.link`), and for a theory's papers it is built by
/// `backend/theory_tools.py:_paper_ref` straight from `s2Metadata.externalIds.DOI` — the
/// identifier Semantic Scholar returned, not one recalled. That function carries a comment about
/// having already sent users to the wrong paper once, via an S2 URL form that resolved
/// unreliably. Somebody has paid for this mistake before.
///
/// The client used to decode `citation` and drop the rest, so every link in the app was scraped
/// out of the prose by regex while the real one sat one key away — the §91/§115 shape again: a
/// value the program already had and never read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    /// The citation as the agent wrote it, whole.
    pub citation: String,
    /// The stable link the backend supplied, if it supplied one.
    pub link: Option<String>,
}

/// A dataset CIP Dataverse holds, as the explorer reported it.
///
/// **Separate from the `datasets` bucket, and for a sharper reason than `sources` had.** That
/// bucket keeps one truncated *title* per dataset and nothing else — which is why five distinct
/// datasets from one multi-site study rendered as five identical rows reading *"Replication data
/// for: Qualification of a Plant Disease Simulation Model; performance of the…"*. The thing that
/// tells them apart is the `persistent_id`, and the bucket never carried it.
///
/// Every field comes from `DataVerseFindings` (`backend/schemas.py:692`) and is optional except
/// the two the schema requires, so a sparser payload renders less rather than nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dataset {
    pub title: String,
    /// `doi:10.21223/P3/0F9T62` — the identifier a researcher pastes into a citation, and the one
    /// the access and file-listing APIs take.
    pub persistent_id: String,
    /// Where the dataset's page lives, when the payload carried a resolvable one.
    pub link: Option<String>,
    pub description: String,
    pub authors: Vec<String>,
    /// How many files the dataset holds, when the search reported it. Not a promise about how
    /// many are *downloadable* — that needs the files API, which reports `restricted` per file.
    pub file_count: Option<u64>,
    pub repository: Option<String>,
}

impl Dataset {
    /// The identifier without its `doi:` scheme, for building a URL.
    pub fn bare_doi(&self) -> Option<&str> {
        self.persistent_id.strip_prefix("doi:").map(str::trim)
    }

    /// The page to open when the row is pressed.
    ///
    /// The payload's own link first, because it is what the backend resolved; a DOI resolver
    /// second, because `persistent_id` is required by the schema and a link is not — so a row
    /// that reports a dataset can always be opened, even when the model omitted the URL.
    pub fn page(&self) -> Option<String> {
        if let Some(link) = &self.link {
            return Some(link.clone());
        }
        self.bare_doi()
            .filter(|doi| !doi.is_empty())
            .map(|doi| format!("https://doi.org/{doi}"))
    }
}

/// One document in the researcher's own library, as `pdf_librarian` indexed it.
///
/// Flattened out of `libraries[].papers[]`: the payload carries one library artifact per turn and
/// the library is cumulative, so what a reader wants is the documents, not the envelopes. Keyed on
/// `path`, which is what makes two indexings of one paper one row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub title: String,
    /// Where the librarian recorded it — relative to the workspace, absolute, or a URL. Turned
    /// into something openable by [`crate::workspace::local_path`].
    pub path: String,
    pub doi: Option<String>,
    pub summary: String,
    pub tags: Vec<String>,
    pub page_count: Option<u64>,
}

/// A written report, whole.
///
/// **The one output that never reaches disk.** Figures are written by a plotting script inside
/// `execute` and found by diffing the workspace (§42); datasets and downloaded papers are files
/// by nature. A report is neither — `ReportArtifactPayload` is `{title, markdown}`
/// (`backend/schemas.py:321`), it lives in the run's state, and the only copy that ever leaves
/// the backend is the one in this snapshot.
///
/// So the client used to reduce it to a title for the Outputs panel and drop the body, which is
/// how the agent could say "the report is in the Outputs panel" and be right, while the
/// researcher opened the thread's folder and found no report at all (docs §89).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub title: String,
    pub markdown: String,
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
    /// The worker's plan, so "running" for ten minutes also says *how far through* (§209).
    ///
    /// Filled by the watcher, never by [`decode_async_tasks`]: the `async_tasks` map records what
    /// the coordinator asked for, and the plan lives in the worker's own state.
    pub todos: Vec<Todo>,
    /// The conversation whose folder owns this worker's files.
    ///
    /// **Not decodable from the payload**, which is why it is filled by whoever ingests the
    /// task rather than by `decode_async_tasks`: the `async_tasks` map records the worker's
    /// own thread and nothing about its parent. The parent is a property of *where the
    /// snapshot came from*, and only the caller that asked for that snapshot knows it.
    ///
    /// Carried on the task instead of read from "the conversation open right now" because
    /// those are not the same thing: pressing New thread leaves a pending task on screen
    /// while the open thread moves on, and answering it then named the wrong owner
    /// (docs §159). Empty means unknown — see [`AsyncTask::owning_conversation`].
    pub owner: String,
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

    /// The conversation whose folder this worker's files belong in, or `None` when unknown.
    ///
    /// The rule lives here rather than at the call site because it is easy to get wrong in a
    /// way nothing notices: blank must become `None`, never a directory name. A run pinned to
    /// an empty path segment writes to the *workspace root* — beside every conversation instead
    /// of inside one — which is the shape §150 spent a night on. `None` sends no key at all and
    /// lets the backend fall back to its own inference.
    pub fn owning_conversation(&self) -> Option<&str> {
        let owner = self.owner.trim();
        (!owner.is_empty()).then_some(owner)
    }
}

/// Metadata key marking a thread as a conversation the researcher started.
///
/// The distinguishing fact is *who created it*: this app tags what it creates, and nothing
/// else does — not the async-subagent middleware, not the theorizer. Filtering on the tag
/// is therefore exact, where filtering on "has messages" or "has a title" would be a guess
/// that keeps being wrong.
const CONVERSATION_TAG: &str = "minime_conversation";

/// Metadata key naming the project a conversation belongs to.
///
/// Paired with the folder the backend writes into (`__workspace_project__`, see
/// `overlay/minime_local/workspace.py`). The folder is where the files are; this is how the app
/// knows which folder to look in — see docs §105 for why it is both and not either.
const PROJECT_KEY: &str = "minime_project";

/// Config key naming the conversation whose folder owns a run's files.
///
/// This must agree exactly with `WORKSPACE_THREAD_KEY` in
/// `overlay/minime_local/workspace.py`. The client already knows the conversation id; making the
/// backend rediscover it from LangGraph metadata was intermittent because that metadata is not
/// present in every tool-call context (docs §159).
const WORKSPACE_THREAD_KEY: &str = "__workspace_thread__";

/// A stored thread that is not yet tagged, by id.
///
/// Only the cheap half of the decision. Whether it is a *conversation* is settled by asking
/// whether it holds any messages, which needs a request per thread — see
/// [`LangGraphClient::adopt_untagged_conversations`].
///
/// **A title is not the test, though it was.** The first version of this filtered on
/// `metadata.title`, on the reasoning that `rename_conversation` writes it and nothing else does.
/// That is true and it is useless here: `rename_conversation` shipped in `4911094`, the *same day*
/// as the filter it was meant to work around. No thread old enough to be hidden is new enough to
/// have a title. Measured against a real store, it adopted **1 of 30** and left 25 threads with
/// genuine history exactly as invisible as before — the identical mistake as the original bug,
/// one level down, and caught only because the numbers were checked (docs §91).
fn untagged(thread: &Value) -> Option<&str> {
    if thread
        .get("metadata")
        .and_then(|metadata| metadata.get(CONVERSATION_TAG))
        .is_some()
    {
        return None;
    }
    thread
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// What reopening a conversation yields: its transcript, and whatever else the thread still
/// holds — outputs, the spine, and the task ids of work that is still running somewhere else.
pub type StoredConversation = (Vec<(String, String)>, Option<Snapshot>);

/// One past conversation, for the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct Conversation {
    pub thread_id: String,
    /// Which project it belongs to, or `None` for ungrouped.
    ///
    /// On the thread rather than inferred from the folder: renaming a directory in Explorer is
    /// something a scientist will do, and an app that read the project only from the path would
    /// then be silently wrong about where a conversation lives (docs §106).
    pub project: Option<String>,
    /// What to call it in the list. Never empty — see [`decode_conversation`].
    pub title: String,
    /// ISO-8601, as the server reports it. Used for grouping, not for display.
    pub updated_at: String,
}

/// An ISO-8601 UTC stamp as seconds since the epoch, or `None` if it is not one.
///
/// Only the fixed-width prefix `YYYY-MM-DDTHH:MM:SS` is read; a fractional part and the trailing
/// `Z` are ignored, and an offset other than UTC is refused rather than silently misread. That is
/// what LangGraph sends (`langgraph_api/schema.py` stamps these in UTC).
///
/// Civil date to days by Howard Hinnant's algorithm, which is the standard one and needs no
/// lookup table: shift the year so it starts in March, and leap days land at the end where they
/// stop perturbing the month lengths.
pub fn epoch_seconds(stamp: &str) -> Option<i64> {
    let bytes = stamp.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' {
        return None;
    }
    // A stamp carrying a non-UTC offset would be hours out if read as UTC.
    if let Some(rest) = stamp.get(19..) {
        let rest = rest.trim_end_matches('Z');
        let rest = rest.split('.').next_back().unwrap_or(rest);
        if rest.contains('+') || rest.contains('-') {
            return None;
        }
    }
    let number = |from: usize, to: usize| stamp.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// How long ago `stamp` was, said the way a person would.
///
/// **Relative, not "today 14:22".** A wall-clock time needs the researcher's timezone, and
/// converting a UTC stamp to local time means either a timezone database or a dependency — for a
/// line whose whole job is to say how fresh a conversation is. "2 hours ago" answers that exactly,
/// in every timezone, with no table.
///
/// Empty for a stamp that cannot be read, so the caller renders nothing rather than "unknown".
pub fn how_long_ago(stamp: &str, now: i64) -> String {
    let Some(then) = epoch_seconds(stamp) else {
        return String::new();
    };
    // A clock that has been corrected backwards, or a server slightly ahead. "just now" is the
    // one answer that is never absurd; "in 4 seconds" would be.
    let seconds = (now - then).max(0);
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    // Named because a range pattern takes a path or a literal, not an expression.
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    // Bounded on both sides rather than a ladder of `..X`. Those match correctly — arms are
    // tried in order — but each one contains the last, so the intent has to be read out of the
    // ordering instead of the arm.
    let (count, unit) = match seconds {
        0..MINUTE => return "just now".to_string(),
        MINUTE..HOUR => (seconds / MINUTE, "minute"),
        HOUR..DAY => (seconds / HOUR, "hour"),
        DAY..MONTH => (seconds / DAY, "day"),
        MONTH..YEAR => (seconds / MONTH, "month"),
        _ => (seconds / YEAR, "year"),
    };
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
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
        project: metadata
            .and_then(|metadata| metadata.get(PROJECT_KEY))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|project| !project.is_empty())
            .map(str::to_string),
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
    /// The worker's own plan, from the same request. See [`Todo`].
    pub todos: Vec<Todo>,
    /// The nodes the thread would run next — the value `status` is derived from.
    ///
    /// Carried out of this function so the *watcher* can report it, once per task rather than
    /// once per four-second poll. Everything about the "a finished worker keeps saying running"
    /// hunt turns on what is in here, and it was the one number nobody could see (§207).
    pub next: Vec<String>,
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
                // Empty for the same reason `activity` is: this map is what the coordinator
                // recorded, and the worker's plan lives in the worker's own state. The watcher
                // fills both.
                todos: Vec::new(),
                // Deliberately blank. This function is a pure decoder of one payload, and the
                // payload does not say which conversation the worker belongs to; guessing —
                // `thread_id`, say — would name the worker as its own owner and defeat the
                // nesting entirely. See the field's own note.
                owner: String::new(),
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
pub(crate) fn urlencode(value: &str) -> String {
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
    /// A model per specialist, by name. Empty means every one follows the coordinator.
    ///
    /// Sent as `model_config.subagents`, which `backend/models.py:114` reads and folds into the
    /// set of providers the request needs keys for.
    pub subagents: std::collections::BTreeMap<String, String>,
    /// A key per *other* provider a specialist uses, by provider id.
    ///
    /// **Necessary, and easy to miss.** The backend gathers providers from the coordinator's spec
    /// *and every override* (`models.py:117-122`), so pointing one specialist at a second
    /// provider makes that provider's key part of the request. Without it the turn fails inside a
    /// subagent, several minutes in, for a reason that reads like the specialist being broken.
    pub extra_keys: std::collections::BTreeMap<String, String>,
}

/// Thin HTTP client bound to a backend base URL.
pub struct LangGraphClient {
    http: reqwest::Client,
    base_url: String,
    model: Option<ModelChoice>,
    /// The project whose folder this thread's outputs belong in, if any.
    project: Option<String>,
}

impl LangGraphClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            model: None,
            project: None,
        }
    }

    /// Name the project folder the backend should write this thread's outputs into.
    pub fn with_project(mut self, project: Option<String>) -> Self {
        self.project = project;
        self
    }

    /// Attach the user's model choice and key. Without one the backend falls back to
    /// whatever provider variables its own environment happens to have.
    pub fn with_model(mut self, model: Option<ModelChoice>) -> Self {
        self.model = model;
        self
    }

    /// `GET /ok` — true when the HTTP server is accepting requests.
    ///
    /// It does **not** mean the graph is loaded. Measured on 2026-08-12: `/ok` answered, then the
    /// first read-only `GET /threads/{id}/state` spent 14,982 ms constructing the graph and opening
    /// its MCP clients. Keeping the boundary honest matters because startup uses this answer to
    /// decide whether the researcher may safely open saved work (docs §176).
    pub async fn is_healthy(&self) -> bool {
        match self.http.get(format!("{}/ok", self.base_url)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    /// Force the deployed graph factory to run before a researcher opens a conversation.
    ///
    /// LangGraph API 0.9.0 does not accept `GET /assistants/agent/schemas`: that route validates
    /// `assistant_id` as a UUID. Searching assistants is cheap but also does not touch the graph.
    /// A fixed internal assistant followed by its schemas route is the smallest read-only graph
    /// access that did trigger the factory in a live probe. The assistant is deliberately retained:
    /// one stable internal row is safer than create/delete races between two app windows, and it is
    /// not a conversation or a second source of conversation metadata (docs §154, §176).
    pub async fn warm_graph(&self) -> Result<()> {
        self.http
            .post(format!("{}/assistants", self.base_url))
            .json(&graph_warm_up_assistant())
            .send()
            .await
            .context("creating the internal graph warm-up assistant failed")?
            .error_for_status()
            .context("creating the internal graph warm-up assistant returned an error status")?;

        self.http
            .get(format!(
                "{}/assistants/{GRAPH_WARM_UP_ASSISTANT_ID}/schemas",
                self.base_url
            ))
            .timeout(GRAPH_WARM_UP_TIMEOUT)
            .send()
            .await
            .context("warming the agent graph failed")?
            .error_for_status()
            .context("warming the agent graph returned an error status")?;
        Ok(())
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
            .get(self.project_url())
            .send()
            .await
            .context("GET /project failed (is the sidecar running?)")?
            .error_for_status()
            .context("GET /project returned an error status")?;
        resp.json()
            .await
            .context("could not decode the project spine from GET /project")
    }

    /// `/project`, scoped to the project this client is working in.
    ///
    /// The project this spine belongs to. Upstream keys it `(user_id, "project")` — one per
    /// person, accumulating forever — which mixes every line of work a researcher has ever had
    /// and never forgets a deleted conversation. The overlay reads this parameter and scopes the
    /// namespace to match what a turn writes; without it the two sides would disagree and the
    /// panel would go blank rather than become correct (`overlay/minime_local/spine.py`, §109).
    ///
    /// Shared by the read and the write, which is not tidiness: the overlay wraps `get_project`
    /// and `patch_project` alike, so a PATCH that spelled its scope differently from the GET
    /// would save the mission into a namespace the panel never reads and look like a save that
    /// silently did nothing.
    fn project_url(&self) -> String {
        format!(
            "{}/project{}",
            self.base_url,
            match self
                .project
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                Some(project) => format!("?project={}", urlencode(project)),
                None => String::new(),
            }
        )
    }

    /// `PATCH /project` → set the mission by hand, and get the saved spine back.
    ///
    /// **The route was already there and this client had never called it.** Its own docstring
    /// says the point out loud — *"let the user read and edit it by hand — rename the mission,
    /// add a backlog item"* (`backend/routes/project.py`) — while the panel rendered
    /// `project.mission` as plain text with nothing to press, so the only way to change a mission
    /// was to phrase the first question of a project differently and never revisit it (§199).
    ///
    /// Two facts from the backend make this worth having rather than decorative:
    ///
    /// * A hand-set mission **survives every later turn**. The middleware seeds the mission from
    ///   the first human message only when it is empty (`backend/project.py:373`), so this is an
    ///   edit and not a suggestion that the next question overwrites.
    /// * The mission is **injected into the coordinator's system prompt**
    ///   (`backend/middleware/project.py:136`), so editing it changes what the agent does, not
    ///   only what the panel shows.
    ///
    /// The response is the saved state, so the caller renders what the store holds rather than
    /// what was typed — the backend caps the mission at 500 characters and collapses runs of
    /// whitespace, and echoing the request would hide both.
    pub async fn set_mission(&self, mission: &str) -> Result<Project> {
        let resp = self
            .http
            .patch(self.project_url())
            .json(&json!({ "mission": mission }))
            .send()
            .await
            .context("PATCH /project failed (is the sidecar running?)")?
            .error_for_status()
            .context("PATCH /project returned an error status")?;
        resp.json()
            .await
            .context("could not decode the project spine from PATCH /project")
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

    /// Every unfinished background run across recent conversations, with the thread that owns it.
    ///
    /// # Why this exists
    ///
    /// A run's outputs are collected by `routes/artifacts.py` **only when something polls its
    /// route and sees a terminal state** — and the only poller was `sidecar::watch_job`, which
    /// ticks every twenty seconds *while the app is open on that conversation* and returns the
    /// moment the open thread changes. So a forty-minute analysis that finished while the
    /// researcher was reading something else was never collected, and its charts and metrics sat
    /// on Asta's side indefinitely (docs §242, §243).
    ///
    /// For a feature whose whole premise is *"you can keep working"*, that had it backwards: the
    /// longer the run, the likelier they had moved on, and the likelier its results were lost.
    ///
    /// # Why it is one request
    ///
    /// `values` is a selectable column on `POST /threads/search`
    /// (`langgraph_api.schema.THREAD_FIELDS`), and a thread's `values.artifacts` is exactly what
    /// [`decode_jobs`] already reads. So the sweep needs no per-thread fetch — one search, asking
    /// only for the id and the state.
    ///
    /// Bounded deliberately. `limit` is small and the sort is most-recently-updated: a run old
    /// enough to fall off that list is one nobody is waiting for, and paying for two hundred
    /// threads' full artifact bundles at every launch to find it would be the wrong trade.
    pub async fn unfinished_jobs(&self, limit: usize) -> Result<Vec<(String, Job)>> {
        let resp = self
            .http
            .post(format!("{}/threads/search", self.base_url))
            .json(&json!({
                "limit": limit,
                "sort_by": "updated_at",
                "sort_order": "desc",
                "metadata": { CONVERSATION_TAG: true },
                // Only what the sweep reads. The sidebar's own search asks for the metadata it
                // needs and deliberately does not ask for this, because `values` carries every
                // artifact a conversation ever produced.
                "select": ["thread_id", "values"],
            }))
            .send()
            .await
            .context("searching for unfinished background runs failed")?
            .error_for_status()
            .context("the thread-search route returned an error status")?;
        let threads: Value = resp
            .json()
            .await
            .context("could not decode the background-run search")?;
        let mut found = Vec::new();
        for thread in threads.as_array().map(Vec::as_slice).unwrap_or_default() {
            let Some(thread_id) = thread
                .get("thread_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            let Some(artifacts) = thread.get("values").and_then(|v| v.get("artifacts")) else {
                continue;
            };
            for job in decode_jobs(artifacts) {
                if !job.is_finished() {
                    found.push((thread_id.to_string(), job));
                }
            }
        }
        Ok(found)
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

        // **The whole status hangs on this one field, so say when it is not there.**
        //
        // `is_some_and` reads a *missing* `next` as "not empty", which resolves to `running` —
        // forever, silently, for a worker that finished minutes ago. Reported as: *"if I don't ask
        // about the status the app is not checking the success or failure; if I ask, the success
        // appears even though the agent had already finished"* (§204). The coordinator's own
        // `check_async_task` reads a different source and gets it right, which is why asking works.
        //
        // Whether that is what is happening here cannot be settled from this machine — so the value
        // the argument needs goes in the log, naming what the payload *did* carry. Fifth time in
        // this project that the missing evidence was a value the program already had (§116).
        let next = state.get("next");
        // `as_str` before `to_string`: a `Value::String` stringifies *with* its JSON quotes, so the
        // first version of this logged `next=["\"model\""]` — readable, but every reader has to
        // discount a layer of escaping that is an artefact of how it was printed.
        let next_nodes: Vec<String> = next
            .and_then(Value::as_array)
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|node| match node.as_str() {
                        Some(name) => name.to_string(),
                        None => node.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if next.and_then(Value::as_array).is_none() {
            let carried: Vec<&str> = state
                .as_object()
                .map(|fields| fields.keys().map(String::as_str).collect())
                .unwrap_or_default();
            tracing::warn!(
                thread = %thread_id,
                next = %next.map(ToString::to_string).unwrap_or_else(|| "<absent>".into()),
                carried = ?carried,
                "a background task's thread state has no usable `next`, so its status cannot be \
                 read — it will report running until the coordinator is asked (docs §204)"
            );
        }
        let next_is_empty = next
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
            next: next_nodes,
            todos: state.get("values").map(decode_todos).unwrap_or_default(),
        })
    }

    /// Answer a background worker's approval request, on **its** thread.
    ///
    /// Deliberately not streamed into the transcript: the background run's tokens are not
    /// the answer to anything the researcher asked in the chat, and mixing them into the
    /// conversation is how "what did I just read?" happens. The Jobs panel reports it.
    pub async fn resume_background(
        &self,
        thread_id: &str,
        workspace_thread: Option<&str>,
        answers: &[Answer],
    ) -> Result<()> {
        // The same body a foreground resume sends — one definition, so a change to the
        // decision shape cannot fix one path and leave the other broken.
        let payload = resume_request_body(
            answers,
            self.model.as_ref(),
            self.project.as_deref(),
            workspace_thread,
        );
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
    /// The body `POST /threads` is sent, separated so it can be asserted without a server.
    fn new_thread_body(&self) -> Value {
        json!({
            "metadata": {
                CONVERSATION_TAG: true,
                PROJECT_KEY: self.project.as_deref().map(str::trim).filter(|p| !p.is_empty()),
            }
        })
    }

    pub async fn create_thread(&self) -> Result<String> {
        let resp = self
            .http
            .post(format!("{}/threads", self.base_url))
            // Marked as *ours*. Every background worker creates a thread of its own
            // (§43), and without this the sidebar filled with dozens of "New
            // conversation" rows that were machinery, not conversations (docs §51).
            //
            // **And filed, at birth.** The project drives two different things and they were
            // wired separately: `self.project` tells the backend which folder to write into,
            // and this key tells the sidebar which heading to show it under. Setting only the
            // first meant a conversation started with the `+` on a project heading had its
            // files in the right folder and its row under "No project" — right by one measure
            // and wrong by the other, which is the worst of both (docs §108).
            .json(&self.new_thread_body())
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

    /// Adopt conversations that predate the tag, once.
    ///
    /// **Why this is needed at all.** `dfea94a` started filtering the sidebar on
    /// [`CONVERSATION_TAG`] so that background workers' threads (§43, §51) stopped filling the
    /// list with machinery. The tag is written by [`Self::create_thread`], so only threads
    /// created *after* that commit carry it — and every conversation from before became
    /// invisible the moment the researcher pulled. The commit anticipated that and judged the
    /// affected threads to be "almost all junk rows". Measured on a real checkout: 26 of 30 had
    /// genuine message history, and at least one was a titled piece of research. Reported, fairly,
    /// as *"the conversations doesn't load, like this was erased"* — which is exactly what a
    /// filtered-out history looks like from the outside (docs §90).
    ///
    /// **Why messages are the test.** The obvious discriminator — a title — does not exist on the
    /// data that needs adopting: `rename_conversation` shipped the same day as the filter, so no
    /// thread old enough to be hidden carries one. What every hidden conversation does have, and
    /// no background worker does, is **human messages**. So each untagged thread is read once and
    /// adopted if anyone wrote in it. That costs a request per thread on a list bounded at 200,
    /// paid on the single launch that repairs the history and never again.
    ///
    /// **Runs once, and only when there is nothing to lose.** It returns immediately unless the
    /// tagged search comes back empty, so a researcher who has since started a conversation is
    /// never re-scanned, and an installation that never had old threads pays one extra request.
    /// Failures are the caller's to report but not to panic over: a migration that cannot run is
    /// a sidebar that stays short, not a broken app.
    pub async fn adopt_untagged_conversations(&self) -> Result<usize> {
        if !self.list_conversations(1).await?.is_empty() {
            return Ok(0);
        }
        let resp = self
            .http
            .post(format!("{}/threads/search", self.base_url))
            .json(&json!({
                "limit": 200,
                "sort_by": "updated_at",
                "sort_order": "desc",
            }))
            .send()
            .await
            .context("searching for untagged conversations failed")?
            .error_for_status()
            .context("the thread-search route returned an error status")?;
        let threads: Value = resp
            .json()
            .await
            .context("could not decode the untagged conversation list")?;

        let mut adopted = 0;
        for id in threads
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(untagged)
        {
            // The test that actually holds on old data: does anyone's writing live in here?
            // A background worker's thread carries the machinery of a delegation and no human
            // messages, so this excludes them without depending on a marker that postdates them.
            // One request per thread, once, on a list bounded at 200 — the only time this app
            // pays it is the single launch that repairs a hidden history.
            match self.conversation_messages(id).await {
                Ok(messages) if messages.is_empty() => continue,
                Err(error) => {
                    tracing::warn!(%error, thread = id, "could not read a thread while adopting");
                    continue;
                }
                Ok(_) => {}
            }
            // One thread failing to adopt must not abandon the rest: the next one may be the
            // conversation the researcher is actually looking for.
            match self.tag_conversation(id).await {
                Ok(()) => adopted += 1,
                Err(error) => tracing::warn!(%error, thread = id, "could not adopt a thread"),
            }
        }
        Ok(adopted)
    }

    /// File a conversation under a project, or take it out of one.
    ///
    /// Metadata only — the caller moves the folder, because that has to happen while no turn is
    /// running and this does not know.
    pub async fn set_project(&self, thread_id: &str, project: Option<&str>) -> Result<()> {
        self.http
            .patch(format!(
                "{}/threads/{}",
                self.base_url,
                urlencode(thread_id)
            ))
            // `null` clears it: LangGraph merges metadata, so omitting the key would leave the
            // old project in place and the conversation would file itself back on next read.
            .json(&json!({ "metadata": { PROJECT_KEY: project } }))
            .send()
            .await
            .context("filing the conversation failed")?
            .error_for_status()
            .context("the thread-update route returned an error status")?;
        Ok(())
    }

    /// Mark an existing thread as one of ours, leaving its other metadata alone.
    async fn tag_conversation(&self, thread_id: &str) -> Result<()> {
        self.http
            .patch(format!(
                "{}/threads/{}",
                self.base_url,
                urlencode(thread_id)
            ))
            .json(&json!({ "metadata": { CONVERSATION_TAG: true } }))
            .send()
            .await
            .context("tagging the thread failed")?
            .error_for_status()
            .context("the thread-update route returned an error status")?;
        Ok(())
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

    /// Render a report to PDF, on the backend, and hand back the bytes.
    ///
    /// **The rendering already existed and had never been called.** `POST /render-report/{thread}`
    /// converts markdown through `pypandoc` to Typst, wraps it in a template that lays out a title
    /// page and a citation list, and compiles it with `typst` — all of it host-side, in-process,
    /// no LaTeX (`backend/routes/rendering.py:253`). It also resolves image references against the
    /// thread's working directory, so the figures a turn drew end up *in* the PDF rather than as
    /// broken links.
    ///
    /// Doing it here rather than in Rust is not a shortcut. A faithful markdown-to-PDF pipeline is
    /// a large dependency and a long tail of edge cases, and this one is already written, already
    /// installed in the backend's venv, and already the thing the web client uses — so a report
    /// rendered from the desktop app comes out identical to one rendered anywhere else.
    ///
    /// # Each source goes as an object, because that is what the route reads
    ///
    /// This sent a list of bare citation strings, on the belief — written into a comment in
    /// `main.rs` — that *"the backend's Typst template takes a list of citation strings"*. It does
    /// not. `_build_typst_wrapper` calls `source.get("citation")` on every entry, so the first
    /// report a researcher tried to download came back `502 PDF render failed: 'str' object has no
    /// attribute 'get'` (docs §141). Nothing had caught it because the loop it dies in does not
    /// run when there are no sources, and until the literature path started working there usually
    /// were none.
    ///
    /// Sending the object also sends the `link` — which [`Source`] has held all along, straight
    /// from the backend, and which the old mapping dropped on the floor. The DOIs in a rendered
    /// bibliography are now the ones Semantic Scholar returned rather than nothing at all.
    pub async fn render_report(
        &self,
        thread_id: &str,
        title: &str,
        markdown: &str,
        sources: &[Source],
        used_asta: bool,
    ) -> Result<Vec<u8>> {
        let response = self
            .http
            .post(format!(
                "{}/render-report/{}",
                self.base_url,
                urlencode(thread_id)
            ))
            .json(&render_request_body(title, markdown, sources, used_asta))
            .send()
            .await
            .context("asking the backend to render the report failed")?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("reading the rendered report failed")?;
        if !status.is_success() {
            // The route answers a failed Typst compile with JSON, so the reason is in the body
            // and saying "502" alone would throw away the only useful part.
            let detail = String::from_utf8_lossy(&bytes);
            anyhow::bail!("the backend could not render the report ({status}): {detail}");
        }
        Ok(bytes.to_vec())
    }

    /// Delete a conversation and everything the backend stored for it.
    ///
    /// **Irreversible, and the caller must have asked first.** This is why the sidebar
    /// makes it a two-step: a conversation is somebody's work, and there is no undo on the
    /// server side (docs §58). This route knows nothing about the Windows workspace; the desktop
    /// deletes those files only after this durable operation succeeds and after its centred modal
    /// has named that consequence explicitly (docs §155).
    pub async fn delete_conversation(&self, thread_id: &str) -> Result<()> {
        self.http
            .delete(format!(
                "{}/threads/{}",
                self.base_url,
                urlencode(thread_id)
            ))
            .send()
            .await
            .context("deleting the conversation failed")?
            .error_for_status()
            .context("the thread-delete route returned an error status")?;
        Ok(())
    }

    /// The messages of an existing conversation, for reopening it.
    ///
    /// Only role and text: the activity trace is not replayable — it was assembled from a
    /// stream that is over — and pretending otherwise would show an empty trace next to a
    /// real answer, which reads as a bug rather than as history.
    pub async fn conversation_state(&self, thread_id: &str) -> Result<StoredConversation> {
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
        let Some(values) = state.get("values") else {
            return Ok((Vec::new(), None));
        };
        let messages = values
            .get("messages")
            .and_then(Value::as_array)
            .map(|messages| {
                messages
                    .iter()
                    .filter_map(decode_stored_message)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // The same response already carries `artifacts` — the outputs, the spine, and the
        // **task ids of background jobs**. Decoding it here costs nothing and is what lets a
        // reopened conversation pick a long run back up (docs §102).
        Ok((messages, decode_values(&values.to_string())))
    }

    /// Just the messages, for callers that do not care what else the thread holds.
    pub async fn conversation_messages(&self, thread_id: &str) -> Result<Vec<(String, String)>> {
        Ok(self.conversation_state(thread_id).await?.0)
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
        self.stream(
            thread_id,
            run_request_body(
                prompt,
                self.model.as_ref(),
                self.project.as_deref(),
                Some(thread_id),
            ),
            on_event,
        )
        .await
    }

    /// Resume a run that stopped at the approval gate, streaming the continuation.
    pub async fn resume_turn(
        &self,
        thread_id: &str,
        answers: &[Answer],
        on_event: impl FnMut(TurnEvent),
    ) -> Result<TurnOutcome> {
        self.stream(
            thread_id,
            resume_request_body(
                answers,
                self.model.as_ref(),
                self.project.as_deref(),
                Some(thread_id),
            ),
            on_event,
        )
        .await
    }

    /// Stop a run the server is still working on.
    ///
    /// Dropping our end of the SSE stream is *not* enough: `on_disconnect` defaults to
    /// `continue`, so the graph keeps running — and keeps spending tokens — with nobody
    /// reading the answer. Verified against the SDK in the reference checkout:
    /// `cancel(threadId, runId, wait?, action?)` posts to this path, and the default action
    /// is `interrupt` (docs §63).
    ///
    /// `interrupt` rather than `rollback`: the partial answer already on screen is real work,
    /// and rolling the thread back would erase what the reader is looking at.
    pub async fn cancel_run(&self, thread_id: &str, run_id: &str) -> Result<()> {
        self.http
            .post(format!(
                "{}/threads/{}/runs/{}/cancel",
                self.base_url, thread_id, run_id
            ))
            .query(&[("action", "interrupt")])
            .send()
            .await
            .context("POST /runs/{id}/cancel failed")?
            .error_for_status()
            .context("POST /runs/{id}/cancel returned an error status")?;
        Ok(())
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
fn run_request_body(
    prompt: &str,
    model: Option<&ModelChoice>,
    project: Option<&str>,
    workspace_thread: Option<&str>,
) -> Value {
    let mut body = stream_request_body(model, project, workspace_thread);
    body["input"] = json!({ "messages": [ { "type": "human", "content": prompt } ] });
    body
}

fn graph_warm_up_assistant() -> Value {
    json!({
        "assistant_id": GRAPH_WARM_UP_ASSISTANT_ID,
        "graph_id": "agent",
        "if_exists": "do_nothing",
        "name": "Mini-Me startup warm-up (internal)",
        "metadata": {"minime_internal": true},
    })
}

/// Body for resuming a paused run with the human's decisions.
///
/// Shape from the HITL middleware (`human_in_the_loop.py`:
/// `decisions = interrupt(hitl_request)["decisions"]`): exactly one decision per
/// held action, in the order they were presented.
fn resume_request_body(
    answers: &[Answer],
    model: Option<&ModelChoice>,
    project: Option<&str>,
    workspace_thread: Option<&str>,
) -> Value {
    let wire = |decision: &Decision| match decision {
        Decision::Approve => json!({ "type": "approve" }),
        Decision::Reject { message } => json!({ "type": "reject", "message": message }),
    };
    let mut body = stream_request_body(model, project, workspace_thread);

    // **Keyed by interrupt id when every answer has one.** LangGraph decides which shape it is
    // looking at by testing whether *all* the map's keys are xxh3-128 digests
    // (`pregel/_loop.py:727`), so a half-filled map is not a partial improvement — it is the legacy
    // shape with a nonsense key. All or nothing, therefore.
    //
    // The legacy shape stays for a backend that sends no ids, where it is not merely acceptable but
    // correct: a version that cannot name its interrupts cannot have had two of them pending.
    let keyed = !answers.is_empty() && answers.iter().all(|answer| !answer.interrupt.is_empty());
    tracing::info!(
        answers = answers.len(),
        keyed,
        "resuming"
    );
    if keyed {
        // Grouped, in the order the actions were presented, because the middleware reads each
        // interrupt's `decisions` positionally against its own `action_requests`.
        let mut grouped: serde_json::Map<String, Value> = serde_json::Map::new();
        for answer in answers {
            let slot = grouped
                .entry(answer.interrupt.clone())
                .or_insert_with(|| json!({ "decisions": [] }));
            slot["decisions"]
                .as_array_mut()
                .expect("decisions is an array")
                .push(wire(&answer.decision));
        }
        body["command"] = json!({ "resume": Value::Object(grouped) });
        return body;
    }

    let decisions: Vec<Value> = answers.iter().map(|a| wire(&a.decision)).collect();
    body["command"] = json!({ "resume": { "decisions": decisions } });
    body
}

/// What the user decided about one held action.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Approve,
    Reject { message: String },
}

/// One decision, and the interrupt it answers.
///
/// **A struct rather than two parallel slices.** The decisions and their interrupt ids have to stay
/// in step — one answer per held action, grouped by the interrupt that held it — and this project
/// has already paid for the version where two collections that must agree were passed separately:
/// the providers derived from the specs and the keys sent beside them (§187, §212).
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// `Interrupt.id`, or empty when the backend did not send one.
    pub interrupt: String,
    pub decision: Decision,
}

impl Answer {
    /// Answer every held action of a request the same way, in the order presented.
    ///
    /// The order is the contract: the middleware reads `decisions` positionally against its own
    /// `action_requests`, and validates the count.
    pub fn all(request: &ApprovalRequest, decision: Decision) -> Vec<Self> {
        request
            .actions
            .iter()
            .map(|action| Answer {
                interrupt: action.interrupt.clone(),
                decision: decision.clone(),
            })
            .collect()
    }
}

/// How a stream ended: finished, or stopped at the approval gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TurnOutcome {
    Finished,
    AwaitingApproval,
}

/// The parts of the request body shared by a fresh run and a resume.
fn stream_request_body(
    model: Option<&ModelChoice>,
    project: Option<&str>,
    workspace_thread: Option<&str>,
) -> Value {
    json!({
        "assistant_id": "agent",
        "stream_mode": ["messages-tuple", "values", "custom"],
        // Without this the whole stream stops at the coordinator: a delegated turn
        // then emits a `task` tool call and nothing else until the answer, which is
        // the silent gap the activity trace exists to close. On a measured turn this
        // flag is the difference between 176 and 495 message events.
        "stream_subgraphs": true,
        "config": config_for(model, project, workspace_thread),
    })
}

/// The `config` object: recursion limit, model routing, and the key.
fn config_for(
    model: Option<&ModelChoice>,
    project: Option<&str>,
    workspace_thread: Option<&str>,
) -> Value {
    let mut configurable = json!({
        // Marks this as a real run rather than a read-only graph load, which is what the
        // backend's key check keys off.
        "__is_for_execution__": true,
    });

    // Which folder under the workspace root this turn's outputs belong in. The overlay reads
    // this key and sanitises it again on its own side — a project name is a path segment and a
    // thing a person types (docs §105).
    if let Some(project) = project.map(str::trim).filter(|name| !name.is_empty()) {
        configurable["__workspace_project__"] = json!(project);
    }

    // **Name the owner instead of asking the backend to infer it.** A background tool is launched
    // from the conversation's run, but LangGraph does not expose `metadata.thread_id` in every
    // context where that tool executes. When it was absent, the worker used its own UUID as a
    // top-level folder and a complete EDA appeared beside the conversation (docs §159). This key
    // is deliberately separate from LangGraph's `thread_id`: it chooses a directory and cannot
    // make the worker write checkpoints into the conversation's thread.
    if let Some(thread_id) = workspace_thread
        .map(str::trim)
        .filter(|thread_id| !thread_id.is_empty())
    {
        configurable[WORKSPACE_THREAD_KEY] = json!(thread_id);
    }

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
        if !model.subagents.is_empty() {
            model_config
                .as_object_mut()
                .expect("object")
                .insert("subagents".into(), json!(model.subagents));
        }
        configurable["model_config"] = model_config;

        // One entry per provider the request will actually touch — the coordinator's, plus any a
        // specialist was pointed at. The backend derives that same set from the specs
        // (`models.py:117-122`); sending fewer keys than providers is the failure that surfaces
        // minutes later, inside a subagent, looking like the subagent's fault.
        let mut keys = serde_json::Map::new();
        if let Some(api_key) = &model.api_key {
            keys.insert(
                model.provider.clone(),
                json!({ "api_key": api_key, "base_url": model.base_url }),
            );
        }
        for (provider, api_key) in &model.extra_keys {
            // The coordinator's own entry carries its base_url and must not be flattened by a
            // specialist that happens to share its provider.
            keys.entry(provider.clone())
                .or_insert_with(|| json!({ "api_key": api_key, "base_url": null }));
        }
        if !keys.is_empty() {
            configurable["__llm_keys"] = Value::Object(keys);
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
        // `id` since langgraph 0.6; `interrupt_id` is its deprecated spelling. Both are read
        // because the version that matters is whichever is installed on a researcher's machine.
        let id = interrupt
            .get("id")
            .or_else(|| interrupt.get("interrupt_id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
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
                interrupt: id.clone(),
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
    // **The precondition for §215's fix, said out loud.** The keyed resume is only sent when every
    // action carries an interrupt id, and if the payload does not have them this silently falls
    // back to the shape that fails on more than one pending interrupt — which is indistinguishable,
    // from outside, from the fix not being installed at all (§216).
    let named = actions.iter().filter(|a| !a.interrupt.is_empty()).count();
    tracing::info!(
        interrupts = interrupts.len(),
        actions = actions.len(),
        with_id = named,
        "an approval request arrived"
    );
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
    let reports = decode_reports(artifacts);
    let sources = decode_sources(artifacts);
    let datasets = decode_datasets(artifacts);
    let documents = decode_documents(artifacts);
    // Read from the top level, not from `artifacts`: `todos` is the agent's own state, written by
    // its `write_todos` tool, and never passes through the artifacts middleware.
    let todos = decode_todos(&value);

    if buckets.is_empty()
        && project.is_none()
        && jobs.is_empty()
        && tasks.is_empty()
        && reports.is_empty()
        && todos.is_empty()
    {
        return None;
    }
    Some(Snapshot {
        buckets,
        project,
        jobs,
        tasks,
        reports,
        sources,
        datasets,
        documents,
        todos,
    })
}

/// The body of a `POST /render-report` request.
///
/// **Pure, so the wire shape can be pinned by a test.** The bug this replaced was a payload the
/// route could not read, and no test could have caught it while the JSON was assembled inline in
/// the middle of an HTTP call — the only way to see the shape was to make the call. It is the
/// reason `paper_tools._build_search_command` upstream is a separate function too.
fn render_request_body(
    title: &str,
    markdown: &str,
    sources: &[Source],
    used_asta: bool,
) -> Value {
    json!({
        "markdown": markdown,
        "title": title,
        // Objects, not strings: `_build_typst_wrapper` reads `citation` and `link` off each entry.
        // The link is sent even when empty, because the route distinguishes "no link" from a
        // missing key by the same emptiness check either way, and an explicit field says which of
        // the two this is.
        "sources": sources
            .iter()
            .map(|source| json!({
                "citation": source.citation,
                "link": source.link.clone().unwrap_or_default(),
            }))
            .collect::<Vec<_>>(),
        // **Decided by the caller from the provenance record, not from this list.**
        //
        // The backend's own default is `len(sources) > 0` (`backend/routes/rendering.py`), and the
        // footer it controls reads *"Academic literature search performed using Asta tools (Allen
        // Institute for AI)"*. Those two do not match: `sources` is a list of citation objects the
        // **model** produced, so a run where nothing was ever searched — where the model wrote
        // five plausible references from memory — puts that sentence in the report and credits AI2
        // for work their tools did not do.
        //
        // That is not hypothetical. Five references from a real run were checked against Crossref:
        // three DOIs resolved to different papers (one to a paper about lichens) and two did not
        // exist at all. The footer would have claimed Asta for every one of them (docs §119).
        //
        // An attribution is a claim about provenance, so it should come from the provenance
        // record. See `Workbench::used_asta`.
        "used_asta": used_asta,
    })
}

/// Full citations, for the bibliography of a rendered report.
fn decode_sources(artifacts: &Value) -> Vec<Source> {
    let Some(entries) = artifacts.get("sources").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let citation = entry.get("citation").and_then(Value::as_str)?.trim();
            if citation.is_empty() {
                return None;
            }
            Some(Source {
                citation: citation.to_string(),
                link: stable_link(entry),
            })
        })
        .collect()
}

/// Every dataset the explorer recommended, whole.
///
/// Keyed on `persistent_id` rather than `title`: the run that prompted this returned five
/// datasets from one multi-site study whose titles differ only past the 96th character, and a
/// list that dropped or merged them on title would be losing exactly the rows a researcher needs
/// to tell apart. A finding with no identifier is dropped — it is the required field, the thing
/// the access APIs take, and a dataset that cannot be opened or checked is not one we can offer.
fn decode_datasets(artifacts: &Value) -> Vec<Dataset> {
    let Some(entries) = artifacts.get("datasets").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: Vec<String> = Vec::new();
    let mut found = Vec::new();
    for entry in entries {
        let persistent_id = entry
            .get("persistent_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if persistent_id.is_empty() || seen.contains(&persistent_id) {
            continue;
        }
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let title = match text("title") {
            title if title.is_empty() => persistent_id.clone(),
            title => title,
        };
        seen.push(persistent_id.clone());
        found.push(Dataset {
            title,
            persistent_id,
            // `doi_url` first: `DataVerseFindings` carries it beside the id, and `stable_link`
            // already knows the three shapes a link arrives in.
            link: stable_link(entry).or_else(|| {
                entry
                    .get("doi_url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|url| url.starts_with("http"))
                    .map(str::to_string)
            }),
            description: text("description"),
            authors: entry
                .get("authors")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            file_count: entry.get("file_count").and_then(Value::as_u64),
            repository: Some(text("repository")).filter(|name| !name.is_empty()),
        });
    }
    found
}

/// Every document the librarian has indexed, across the turn's library artifacts.
///
/// Deduped on `path` rather than title: re-indexing a paper is the ordinary way the library grows,
/// and two entries for one file would make `paper_count` and this list disagree in the one place a
/// researcher would notice.
fn decode_documents(artifacts: &Value) -> Vec<Document> {
    let Some(libraries) = artifacts.get("libraries").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: Vec<String> = Vec::new();
    let mut found = Vec::new();
    for library in libraries {
        let Some(papers) = library.get("papers").and_then(Value::as_array) else {
            continue;
        };
        for paper in papers {
            let text = |key: &str| {
                paper
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let path = text("path");
            if path.is_empty() || seen.contains(&path) {
                continue;
            }
            seen.push(path.clone());
            found.push(Document {
                title: match text("title") {
                    title if title.is_empty() => path.clone(),
                    title => title,
                },
                path,
                doi: Some(text("doi")).filter(|doi| !doi.is_empty()),
                summary: text("summary"),
                tags: paper
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(Value::as_str)
                            .map(str::trim)
                            .filter(|tag| !tag.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                page_count: paper.get("page_count").and_then(Value::as_u64),
            });
        }
    }
    found
}

/// The trustworthy link on an artifact, whichever shape it arrived in.
///
/// Three keys because two payloads carry this and they do not agree on a name:
/// `SourceArtifactPayload` has `link`, `PaperRefPayload` has `url` **and** a bare `doi`
/// (`backend/schemas.py:316,334`). Tried in that order — `link` and `url` are already complete
/// URLs, while `doi` is the identifier alone and has to be given a resolver.
///
/// Only `http(s)`, for the same reason [`crate::workspace::browse`] refuses anything else: this
/// value is on its way to a process launcher, and it reaches us from a model.
fn stable_link(entry: &Value) -> Option<String> {
    for key in ["link", "url"] {
        if let Some(found) = entry.get(key).and_then(Value::as_str).map(str::trim) {
            if found.starts_with("https://") || found.starts_with("http://") {
                return Some(found.to_string());
            }
        }
    }
    let doi = entry.get("doi").and_then(Value::as_str)?.trim();
    if doi.is_empty() {
        return None;
    }
    // A bare `10.1007/…`, which is how `_paper_ref` reports it beside the URL it built.
    Some(match doi.strip_prefix("doi:") {
        Some(bare) => format!("https://doi.org/{}", bare.trim()),
        None if doi.starts_with("http") => doi.to_string(),
        None => format!("https://doi.org/{doi}"),
    })
}

/// Pull whole reports, bodies and all, out of a `values` payload.
///
/// Tolerant of a missing title and strict about a missing body: a report with no markdown is
/// nothing to write to disk, and writing an empty file would be worse than writing none — it
/// would look like the report had been saved.
fn decode_reports(artifacts: &Value) -> Vec<Report> {
    let Some(entries) = artifacts.get("reports").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let markdown = entry.get("markdown").and_then(Value::as_str)?.trim();
            if markdown.is_empty() {
                return None;
            }
            let title = entry
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .unwrap_or("Report");
            Some(Report {
                title: title.to_string(),
                markdown: markdown.to_string(),
            })
        })
        .collect()
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
        "title", "citation", "name", "question", "summary", "filename", "label", "id",
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
            "metadata" => {
                let mut events = vec![TurnEvent::Status("run started".into())];
                // A frame without an id is still a started run — worth saying so, even
                // though the stop button will then have nothing to cancel by name.
                if let Some(run_id) = serde_json::from_str::<Value>(&event.data)
                    .ok()
                    .as_ref()
                    .and_then(|data| data.get("run_id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                {
                    events.push(TurnEvent::Started {
                        run_id: run_id.to_string(),
                    });
                }
                return events;
            }
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

    /// One approval for one interrupt. `""` is the older backend that sends no id.
    fn approve_of(interrupt: &str) -> Answer {
        Answer {
            interrupt: interrupt.to_string(),
            decision: Decision::Approve,
        }
    }

    #[test]
    fn an_iso_stamp_becomes_seconds_and_then_something_a_person_reads() {
        // Known anchors. The epoch itself, and a date past 2000 so the era arithmetic is
        // exercised rather than just the year-zero case.
        assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_seconds("2000-03-01T00:00:00Z"), Some(951_868_800));
        // 2024 is a leap year: the 29th exists and the 1st of March is the day after.
        assert_eq!(
            epoch_seconds("2024-03-01T00:00:00Z").unwrap()
                - epoch_seconds("2024-02-29T00:00:00Z").unwrap(),
            86_400
        );
        // 1900 is not a leap year — the century rule, which a naive `% 4` gets wrong.
        assert_eq!(
            epoch_seconds("1900-03-01T00:00:00Z").unwrap()
                - epoch_seconds("1900-02-28T00:00:00Z").unwrap(),
            86_400
        );
        // The real shape LangGraph sends, fractional seconds and all.
        assert_eq!(
            epoch_seconds("2026-08-07T14:22:31.482913Z"),
            epoch_seconds("2026-08-07T14:22:31Z")
        );
        // Not readable, and — importantly — not *misread*: an offset that is not UTC would be
        // hours out if this returned a number anyway.
        assert_eq!(epoch_seconds("2026-08-07T14:22:31+05:00"), None);
        assert_eq!(epoch_seconds("yesterday"), None);
        assert_eq!(epoch_seconds(""), None);
        assert_eq!(epoch_seconds("2026-13-07T14:22:31Z"), None, "no 13th month");

        let now = epoch_seconds("2026-08-07T14:22:31Z").expect("a stamp");
        assert_eq!(how_long_ago("2026-08-07T14:22:30Z", now), "just now");
        assert_eq!(how_long_ago("2026-08-07T14:21:31Z", now), "1 minute ago");
        assert_eq!(how_long_ago("2026-08-07T13:52:31Z", now), "30 minutes ago");
        assert_eq!(how_long_ago("2026-08-07T12:22:31Z", now), "2 hours ago");
        assert_eq!(how_long_ago("2026-08-05T14:22:31Z", now), "2 days ago");
        assert_eq!(how_long_ago("2026-06-07T14:22:31Z", now), "2 months ago");
        assert_eq!(how_long_ago("2024-08-07T14:22:31Z", now), "2 years ago");
        // A server marginally ahead of this clock reads as "just now", never as the future.
        assert_eq!(how_long_ago("2026-08-07T14:22:35Z", now), "just now");
        // Unreadable renders nothing at all, so the caller shows a card with no sub-line
        // rather than a card that says "unknown".
        assert_eq!(how_long_ago("not a date", now), "");
    }

    /// Decode a single event in isolation. Anything that depends on *sequence*
    /// (tool-call argument fragments) drives a `TurnDecoder` directly instead.
    /// The run id is the only thing that makes the stop button able to stop anything, and it
    /// arrives exactly once, in the first frame. Shape taken from the captured stream in
    /// `tests/fixtures/delegated-turn.sse`, not from the documentation (docs §63).
    #[test]
    fn the_first_metadata_frame_names_the_run() {
        let events = decode(&SseEvent {
            name: "metadata".into(),
            data: r#"{"run_id":"019fb670-c72a-7330-98be-0f52520fb23b","attempt":1}"#.into(),
        });
        assert!(
            events.iter().any(|event| matches!(
                event,
                TurnEvent::Started { run_id } if run_id == "019fb670-c72a-7330-98be-0f52520fb23b"
            )),
            "{events:?}"
        );
        // Still says the run started, because that is what the status line shows.
        assert!(events
            .iter()
            .any(|event| matches!(event, TurnEvent::Status(_))));
    }

    #[test]
    fn a_metadata_frame_without_an_id_still_reports_a_started_run() {
        // Then the stop button can only stop us listening, and `stop_turn` says so rather
        // than claiming the backend was told.
        for data in [r#"{"attempt":1}"#, r#"{"run_id":""}"#, "not json at all"] {
            let events = decode(&SseEvent {
                name: "metadata".into(),
                data: data.into(),
            });
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, TurnEvent::Started { .. })),
                "{data}"
            );
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, TurnEvent::Status(_))),
                "{data}"
            );
        }
    }

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
        let body = run_request_body("hi", None, None, None);
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
    fn repeated_startups_reuse_one_internal_graph_warm_up_assistant() {
        let body = graph_warm_up_assistant();
        assert_eq!(body["assistant_id"], GRAPH_WARM_UP_ASSISTANT_ID);
        assert_eq!(body["graph_id"], "agent");
        assert_eq!(body["if_exists"], "do_nothing");
        assert_eq!(body["metadata"]["minime_internal"], true);
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
            ..Default::default()
        };
        let body = run_request_body("hi", Some(&model), None, None);
        let configurable = &body["config"]["configurable"];
        assert_eq!(
            configurable["model_config"]["default"],
            "custom::openai/gpt-4o-mini"
        );
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
        let resumed = resume_request_body(&[approve_of("")], Some(&model), None, None);
        assert_eq!(
            resumed["config"]["configurable"]["__llm_keys"],
            configurable["__llm_keys"]
        );
        assert_eq!(
            resumed["command"]["resume"]["decisions"][0]["type"],
            "approve"
        );
    }

    #[test]
    fn a_specialist_on_a_second_provider_takes_its_key_with_it() {
        // The failure this prevents is expensive and misleading: the backend collects providers
        // from the coordinator's spec *and every override* (`backend/models.py:117-122`), so
        // pointing one specialist at a second provider makes that provider's key part of the
        // request. Send the overrides without the key and the turn dies inside a subagent,
        // minutes in, reading like the specialist is broken (docs §104).
        let model = ModelChoice {
            spec: "anthropic::claude-sonnet-4-5".into(),
            provider: "anthropic".into(),
            api_key: Some("sk-ant".into()),
            base_url: None,
            subagents: [
                (
                    "academic_researcher".to_string(),
                    "openai::gpt-4.1".to_string(),
                ),
                (
                    "report_writer".to_string(),
                    "anthropic::claude-opus-4-1".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
            extra_keys: [("openai".to_string(), "sk-openai".to_string())]
                .into_iter()
                .collect(),
        };
        let body = run_request_body("hi", Some(&model), None, None);
        let configurable = &body["config"]["configurable"];

        assert_eq!(
            configurable["model_config"]["subagents"]["academic_researcher"],
            "openai::gpt-4.1"
        );
        // A key per provider the request will actually touch — both of them.
        assert_eq!(configurable["__llm_keys"]["anthropic"]["api_key"], "sk-ant");
        assert_eq!(configurable["__llm_keys"]["openai"]["api_key"], "sk-openai");
        // The coordinator's own entry keeps its base_url; a specialist sharing its provider
        // must not flatten it.
        let model = ModelChoice {
            spec: "custom::openai/gpt-4o-mini".into(),
            provider: "custom".into(),
            api_key: Some("sk-custom".into()),
            base_url: Some("https://openrouter.ai/api/v1".into()),
            subagents: [(
                "data_cleaning".to_string(),
                "custom::openai/gpt-4o".to_string(),
            )]
            .into_iter()
            .collect(),
            extra_keys: [("custom".to_string(), "sk-should-not-win".to_string())]
                .into_iter()
                .collect(),
        };
        let keys = &run_request_body("hi", Some(&model), None, None)["config"]["configurable"]
            ["__llm_keys"];
        assert_eq!(keys["custom"]["api_key"], "sk-custom");
        assert_eq!(keys["custom"]["base_url"], "https://openrouter.ai/api/v1");
    }

    #[test]
    fn no_overrides_means_no_subagents_key_at_all() {
        // Every specialist follows the coordinator by default, and an empty map in the request
        // would be a shape the backend has to interpret for no reason.
        let model = ModelChoice {
            spec: "openai::gpt-5.4".into(),
            provider: "openai".into(),
            api_key: Some("sk".into()),
            ..Default::default()
        };
        let configurable =
            &run_request_body("hi", Some(&model), None, None)["config"]["configurable"];
        assert!(
            configurable["model_config"].get("subagents").is_none(),
            "{}",
            configurable["model_config"]
        );
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
            ..Default::default()
        };
        let body = run_request_body("hi", Some(&model), None, None);
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
        assert_eq!(
            title_from_prompt("What drives yield?"),
            "What drives yield?"
        );
        // Whitespace from a pasted prompt would otherwise reach the sidebar verbatim.
        assert_eq!(title_from_prompt("  many\n\n spaces  "), "many spaces");

        // Long prompts are cut on a word boundary — a title ending mid-word looks like a
        // rendering bug rather than an abbreviation.
        let long = "Genera un dataset sintético de 400 parcelas de papa y ajusta un modelo";
        let title = title_from_prompt(long);
        assert!(title.ends_with('…'), "{title}");
        assert!(title.chars().count() <= 49, "{title}");
        assert!(
            long.starts_with(title.trim_end_matches('…').trim()),
            "{title}"
        );

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
        assert_eq!(
            decode_stored_message(&json!({"type": "tool", "content": "{}"})),
            None
        );
        assert_eq!(
            decode_stored_message(&json!({"type": "ai", "content": "  "})),
            None
        );
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
    fn adoption_screens_on_the_tag_and_nothing_a_hidden_thread_could_not_have() {
        // The first version of this filtered on `metadata.title`, which sounded exact and was
        // measured at 1 adoption out of 30 — because `rename_conversation` shipped the same day
        // as the filter it was working around, so no thread old enough to be hidden has a title
        // (docs §91). The cheap screen is now *only* "not already ours"; whether it is a
        // conversation is decided by reading its messages, which is the one property every
        // hidden conversation has and no background worker does.

        // A thread from before any of this: no tag, no title, no metadata to speak of.
        let ancient = json!({"thread_id": "t-1", "metadata": {"assistant_id": "agent"}});
        assert_eq!(
            untagged(&ancient),
            Some("t-1"),
            "a thread with no title must still be considered — that was the whole bug"
        );
        assert_eq!(untagged(&json!({"thread_id": "t-2"})), Some("t-2"));

        // Already ours: adopting again would be a wasted request on every launch.
        let tagged = json!({
            "thread_id": "t-3",
            "metadata": {"title": "Yield trials", CONVERSATION_TAG: true}
        });
        assert_eq!(untagged(&tagged), None);

        // Nothing to PATCH.
        assert_eq!(untagged(&json!({"metadata": {"title": "No id"}})), None);
        assert_eq!(untagged(&json!({"thread_id": "   "})), None);
    }

    #[test]
    fn a_new_thread_is_filed_at_birth() {
        // The project drives two things that were wired separately: which folder the backend
        // writes into, and which heading the sidebar shows the row under. Setting only the first
        // put a conversation started from a project's `+` into the right folder and under
        // "No project" — right by one measure, wrong by the other (docs §108).
        let filed = LangGraphClient::new("http://x").with_project(Some("Late blight".into()));
        let body = filed.new_thread_body();
        assert_eq!(body["metadata"][CONVERSATION_TAG], true);
        assert_eq!(body["metadata"][PROJECT_KEY], "Late blight");

        // Ungrouped stays ungrouped, and sends `null` rather than an empty string — the sidebar
        // treats blank as no project, but only one of the two is honest about it.
        let plain = LangGraphClient::new("http://x");
        assert!(plain.new_thread_body()["metadata"][PROJECT_KEY].is_null());
        let blank = LangGraphClient::new("http://x").with_project(Some("   ".into()));
        assert!(blank.new_thread_body()["metadata"][PROJECT_KEY].is_null());
    }

    #[test]
    fn a_conversations_project_is_read_from_the_thread_not_the_folder() {
        // The label is what survives contact with a scientist. Rename a project folder in
        // Explorer, or move one to a shared drive, and an app that inferred the project only
        // from the path is silently wrong about where a conversation lives (docs §105).
        let filed = json!({
            "thread_id": "t-1",
            "metadata": {"title": "Late blight resistance", "minime_project": "Late blight"}
        });
        let conversation = decode_conversation(&filed).expect("decodes");
        assert_eq!(conversation.project.as_deref(), Some("Late blight"));

        // Ungrouped is the absence of the key, and stays that way — every conversation from
        // before projects existed reads as ungrouped rather than as an error.
        let old = json!({"thread_id": "t-2", "metadata": {"title": "Older work"}});
        assert_eq!(decode_conversation(&old).expect("decodes").project, None);
        // Blank is not a project name.
        let blank = json!({"thread_id": "t-3", "metadata": {"minime_project": "   "}});
        assert_eq!(decode_conversation(&blank).expect("decodes").project, None);
    }

    #[test]
    fn a_run_names_the_project_folder_its_outputs_belong_in() {
        // The other half of §105's pair: the label tells the app where to look, this tells the
        // backend where to write. They are computed from the same string by two sanitisers that
        // `workspace.rs` has a test to keep byte-identical.
        let body = run_request_body("hi", None, Some("Late blight"), None);
        assert_eq!(
            body["config"]["configurable"]["__workspace_project__"],
            "Late blight"
        );
        // No project, no key — an ungrouped conversation keeps the path it always had.
        let plain = run_request_body("hi", None, None, None);
        assert!(
            plain["config"]["configurable"]
                .get("__workspace_project__")
                .is_none(),
            "{}",
            plain["config"]["configurable"]
        );
        assert!(
            run_request_body("hi", None, Some("   "), None)["config"]["configurable"]
                .get("__workspace_project__")
                .is_none()
        );
    }

    #[test]
    fn every_run_names_the_conversation_that_owns_its_files() {
        // The UUID is already the request URL. Sending it again under a directory-only key is
        // intentional: LangGraph's thread metadata is absent in some tool-call contexts, and an
        // async worker launched there otherwise creates a sibling folder under the workspace
        // root. This is the client-side fact that makes the backend's §151 nesting deterministic
        // instead of dependent on context propagation (docs §159).
        let fresh = run_request_body("hi", None, None, Some("conversation-1"));
        assert_eq!(
            fresh["config"]["configurable"][WORKSPACE_THREAD_KEY],
            "conversation-1"
        );

        // Resumes carry the owner too. In particular, a background task may wait across a backend
        // restart, which clears the backend's in-memory worker→conversation map; the decision
        // request must restore the owner before the worker writes anything else.
        let resumed = resume_request_body(&[approve_of("")], None, None, Some("conversation-1"));
        assert_eq!(
            resumed["config"]["configurable"][WORKSPACE_THREAD_KEY],
            "conversation-1"
        );

        // Blank is absence, never a directory name and never an instruction to pin a worker to
        // an empty path segment.
        let blank = run_request_body("hi", None, None, Some("   "));
        assert!(blank["config"]["configurable"]
            .get(WORKSPACE_THREAD_KEY)
            .is_none());
    }

    #[test]
    fn a_stored_thread_still_carries_the_task_ids_of_running_work() {
        // The observation this rests on: a theorizer or DataVoyager run lives on Asta's own
        // hosted service, keyed by a task id. Closing the window never stopped the work — it
        // stopped our watching of it, and the poll is also what persists the result. So the ids
        // have to survive a reopen, and they do: `GET /threads/{id}/state` returns `values`
        // holding both the messages *and* the artifacts, and the client used to read only the
        // messages out of it (docs §102).
        //
        // This is the `values` half of a real stored state, in the shape `conversation_state`
        // hands to `decode_values`.
        let snapshot = decode_values(
            &json!({
                "messages": [{"type": "human", "content": "generate theories about X"}],
                "artifacts": {
                    "hypotheses": [
                        {"question": "how do lightning strikes form?",
                         "task_id": "task-still-going", "status": "running"},
                        {"question": "an older one", "task_id": "task-done",
                         "status": "completed"},
                    ],
                    "analyses": [
                        {"question": "yield vs rainfall", "task_id": "voyager-1",
                         "status": "running", "context_id": "ctx-9"},
                    ],
                }
            })
            .to_string(),
        )
        .expect("a stored state with artifacts decodes");

        let ids: Vec<&str> = snapshot
            .jobs
            .iter()
            .map(|job| job.task_id.as_str())
            .collect();
        assert_eq!(ids, ["task-still-going", "task-done", "voyager-1"]);

        // Finished ones come back too — the Jobs panel should show what a conversation did, not
        // only what it is still doing. `track_job` is what declines to poll them.
        let running: Vec<&str> = snapshot
            .jobs
            .iter()
            .filter(|job| !job.is_finished())
            .map(|job| job.task_id.as_str())
            .collect();
        assert_eq!(running, ["task-still-going", "voyager-1"]);

        // The question rides along, because the theorizer's poll route needs it in the query
        // string to persist the outcome under the right heading.
        let theorizer = &snapshot.jobs[0];
        assert!(
            theorizer.route("t-1").contains("how%20do%20lightning"),
            "{}",
            theorizer.route("t-1")
        );
    }

    #[test]
    fn a_report_arrives_whole_and_a_bodyless_one_is_not_a_file() {
        // The shape is `ReportArtifactPayload = {title, markdown}` (`backend/schemas.py:321`) —
        // a cross-repo contract, so it is pinned here rather than assumed. The body was being
        // decoded to a label and thrown away, which is how the agent could truthfully say the
        // report was in the Outputs panel while the folder held no report at all (docs §89).
        let snapshot = decode_values(
            &json!({
                "artifacts": {
                    "reports": [
                        {"title": "EDA Report: Simulated Potato Field Trials",
                         "markdown": "# Yield\n\nClone A led on every site.\n"},
                        // No body: nothing to write, and an empty file would look like a saved
                        // report rather than a missing one.
                        {"title": "Draft", "markdown": "   "},
                        {"title": "Untitled but real", "markdown": "# Something"},
                    ],
                    "sources": [
                        {"citation": "Love, M. I., Huber, W., & Anders, S. (2014). Moderated estimation of fold change and dispersion for RNA-seq data with DESeq2. Genome Biology, 15, 550."}
                    ],
                }
            })
            .to_string(),
        )
        .expect("a payload with reports decodes");

        let titles: Vec<&str> = snapshot
            .reports
            .iter()
            .map(|report| report.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "EDA Report: Simulated Potato Field Trials",
                "Untitled but real"
            ]
        );
        assert!(snapshot.reports[0].markdown.contains("Clone A led"));

        // The bibliography gets the citation *whole*. The `sources` bucket beside it is
        // truncated for the side panel, and a reference list ending in `…` is not one.
        assert_eq!(snapshot.sources.len(), 1);
        assert!(snapshot.sources[0].citation.ends_with("Genome Biology, 15, 550."));
        let panel = snapshot
            .buckets
            .iter()
            .find(|bucket| bucket.name == "sources")
            .expect("the panel still lists sources");
        assert!(panel.items[0].ends_with('…'), "{}", panel.items[0]);
        assert!(
            snapshot.sources[0].citation.len() > panel.items[0].len(),
            "the rendered citation must outlive the panel's truncation"
        );
        // This payload carried no link, and inventing one would be worse than having none.
        assert_eq!(snapshot.sources[0].link, None);
    }

    #[test]
    fn the_stable_link_is_read_from_whichever_field_carries_it() {
        // The three shapes `backend/schemas.py` actually sends. `SourceArtifactPayload` has
        // `link`; `PaperRefPayload` has `url` and a bare `doi`. The client used to read
        // `citation` and drop all three, so every link in the app was scraped out of prose the
        // model wrote while the identifier Semantic Scholar returned sat one key away.
        let decoded = decode_sources(&json!({
            "sources": [
                {"citation": "A. (2021).", "link": "https://doi.org/10.1/a"},
                {"citation": "B. (2020).", "url": "https://arxiv.org/abs/2401.00001"},
                {"citation": "C. (2019).", "doi": "10.2307/3558433"},
                {"citation": "D. (2018).", "doi": "doi:10.1/d"},
                {"citation": "E. — no link anywhere"},
                // A link we will not hand to a process launcher. This value reaches us from a
                // model and ends up as an argument to `explorer.exe`.
                {"citation": "F.", "link": "file:///etc/passwd"},
            ]
        }));
        let links: Vec<Option<&str>> = decoded
            .iter()
            .map(|source| source.link.as_deref())
            .collect();
        assert_eq!(
            links,
            [
                Some("https://doi.org/10.1/a"),
                Some("https://arxiv.org/abs/2401.00001"),
                // A bare DOI is given a resolver; it is an identifier, not a URL.
                Some("https://doi.org/10.2307/3558433"),
                Some("https://doi.org/10.1/d"),
                None,
                None,
            ]
        );
        // `link` wins over `url` when a payload somehow carries both, matching the order the
        // two payload types are documented in.
        let both = decode_sources(&json!({
            "sources": [{"citation": "G.", "link": "https://doi.org/10.1/g", "url": "https://example.org/g"}]
        }));
        assert_eq!(both[0].link.as_deref(), Some("https://doi.org/10.1/g"));
    }

    #[test]
    fn a_rendered_report_sends_each_source_the_way_the_route_reads_it() {
        // `_build_typst_wrapper` does `source.get("citation")` on every entry
        // (`mini-me/backend/routes/rendering.py`). Sending strings made the first report anybody
        // downloaded come back `502 PDF render failed: 'str' object has no attribute 'get'`, and
        // it went unnoticed because that loop does not run when the list is empty (docs §141).
        let sources = vec![
            Source {
                citation: "Barrera, V. (2016). Pests and diseases affecting potato landraces."
                    .into(),
                link: Some("https://doi.org/10.1234/rlp".into()),
            },
            Source {
                citation: "Ames, M. (2010). Blight in landraces.".into(),
                link: None,
            },
        ];
        let body = render_request_body("Late blight", "# Findings", &sources, true);

        let entries = body["sources"].as_array().expect("sources is a list");
        assert!(
            entries.iter().all(|entry| entry.is_object()),
            "a bare string here is the 502: {body}"
        );
        assert_eq!(entries[0]["citation"], json!(sources[0].citation));
        // The link the backend supplied travels with it — `Source` has carried it all along and
        // the old mapping to `Vec<String>` dropped it, so no rendered bibliography ever resolved.
        assert_eq!(entries[0]["link"], json!("https://doi.org/10.1234/rlp"));
        // A source with no link still renders; it just renders without one.
        assert_eq!(entries[1]["link"], json!(""));

        // Attribution stays the caller's call, not `len(sources) > 0`.
        assert_eq!(body["used_asta"], json!(true));
        assert_eq!(
            render_request_body("t", "m", &sources, false)["used_asta"],
            json!(false),
            "a report whose citations came from memory must not credit Asta"
        );
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
            error_text(&json!({"message": "no API key configured", "type": "ValueError"}))
                .as_deref(),
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
            todos: Vec::new(),
            owner: String::new(),
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
    fn a_worker_with_no_recorded_owner_names_no_conversation_at_all() {
        // The half of §159 that is easy to get backwards. Naming the owner is what stops a
        // background worker filing its figures beside the conversation instead of inside it —
        // but an owner the app does not actually know must not be invented, and blank is not a
        // directory name. Sending `""` would pin the run to the workspace *root*, which is
        // strictly worse than the sibling folder the backend falls back to on its own.
        let task = AsyncTask {
            task_id: "t".into(),
            thread_id: "worker-thread".into(),
            agent_name: "background_worker".into(),
            status: "interrupted".into(),
            description: String::new(),
            pending: None,
            error: None,
            activity: None,
            todos: Vec::new(),
            owner: String::new(),
        };
        assert_eq!(task.owning_conversation(), None);
        // Whitespace is absence too: it survives a round trip through JSON looking like a value.
        let blank = AsyncTask {
            owner: "   ".into(),
            ..task.clone()
        };
        assert_eq!(blank.owning_conversation(), None);
        let owned = AsyncTask {
            owner: "conversation-1".into(),
            ..task.clone()
        };
        assert_eq!(owned.owning_conversation(), Some("conversation-1"));
    }

    #[test]
    fn decoding_a_snapshot_does_not_guess_which_conversation_owns_a_worker() {
        // `async_tasks` records the worker's own thread and says nothing about its parent, so
        // this decoder cannot know — and the ingesting caller can. Pinned here so a later reader
        // does not "helpfully" default the field to `thread_id`.
        let snapshot = decode_values(
            &json!({
                "artifacts": {
                    "async_tasks": {
                        "task-1": {
                            "task_id": "task-1",
                            "thread_id": "worker-thread",
                            "agent_name": "background_worker",
                            "status": "running"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("a snapshot with one task");
        let task = snapshot.tasks.first().expect("the task");
        assert_eq!(task.thread_id, "worker-thread");
        assert_eq!(
            task.owning_conversation(),
            None,
            "the payload carries no owner, so the decoder must not supply one"
        );
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
        assert!(
            route.starts_with("/theorizer/thread-1/1f0a2b3c-"),
            "{route}"
        );
        assert!(route.contains("q=%C2%BFqu%C3%A9%20papa"), "{route}");
        assert!(
            !route.contains(' '),
            "a raw space would break the request: {route}"
        );

        let analysis = &snapshot.jobs[1];
        assert_eq!(analysis.kind, JobKind::Analysis);
        assert!(
            analysis.route("t").contains("ctx=ctx-42"),
            "{}",
            analysis.route("t")
        );
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
        for status in ["completed", "failed", "canceled", "unavailable", "error"] {
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

    /// The plan the panel counts, read from the shape `TodoListMiddleware` actually writes.
    #[test]
    fn a_plan_is_read_from_the_agents_own_todo_list() {
        // `langchain`'s `Todo` verbatim: `content` plus one of three statuses.
        let values = json!({
            "todos": [
                {"content": "Generate the synthetic dataset", "status": "completed"},
                {"content": "Clean and validate the columns", "status": "completed"},
                {"content": "Build the diagnostic model", "status": "in_progress"},
                {"content": "Write the report", "status": "pending"},
            ]
        });
        let todos = decode_todos(&values);
        assert_eq!(todos.len(), 4);
        assert_eq!(plan_progress(&todos), Some((2, 4)));
        let marks: Vec<&str> = todos.iter().map(Todo::mark).collect();
        assert_eq!(marks, ["✓", "✓", "◐", "○"]);

        // **No plan is not a plan of zero steps.** `write_todos` is optional and the model skips it
        // for simple requests, so "0 of 0" would be a claim about work nobody planned.
        assert_eq!(plan_progress(&[]), None);
        assert!(decode_todos(&json!({})).is_empty());

        // A step with no text is not a step. A status we have never seen counts as unfinished,
        // rather than panicking or quietly counting as done.
        let ragged = decode_todos(&json!({
            "todos": [
                {"content": "  ", "status": "completed"},
                {"content": "Real step", "status": "cancelled"},
                {"status": "pending"},
            ]
        }));
        assert_eq!(ragged.len(), 1);
        assert_eq!(ragged[0].content, "Real step");
        assert!(!ragged[0].is_done() && !ragged[0].is_running());
        assert_eq!(ragged[0].mark(), "○");
        assert_eq!(plan_progress(&ragged), Some((0, 1)));

        // A missing `status` defaults to pending, not to done — the safe direction for a count a
        // researcher reads as "how much is left".
        let bare = decode_todos(&json!({"todos": [{"content": "Step"}]}));
        assert_eq!(bare[0].status, "pending");
    }

    /// A whole `values` frame carrying a plan, so the wire shape is pinned and not just the helper.
    #[test]
    fn a_turn_that_only_wrote_a_plan_is_still_a_snapshot() {
        // `todos` sits beside `artifacts`, not inside it: it is agent state, not an artifact.
        let data = json!({
            "todos": [{"content": "Read the papers", "status": "in_progress"}],
            "artifacts": {}
        })
        .to_string();
        let decoded = decode(&SseEvent {
            name: "values".into(),
            data,
        });
        let [TurnEvent::Snapshot(snapshot)] = decoded.as_slice() else {
            panic!("a plan alone is worth a snapshot, got {decoded:?}");
        };
        assert_eq!(snapshot.todos.len(), 1);
        assert!(snapshot.buckets.is_empty(), "and nothing else is invented");

        // An empty frame is still nothing at all.
        assert!(decode(&SseEvent {
            name: "values".into(),
            data: json!({"artifacts": {}}).to_string(),
        })
        .is_empty());
    }

    /// Three specialists stopping at once, and one resume that can answer all of them (§215).
    #[test]
    fn several_pending_interrupts_are_answered_by_id() {
        // Two interrupts, the first holding two commands. LangGraph decides which shape it is
        // looking at by testing that *every* key is an xxh3-128 digest, so the map is all-or-nothing.
        let first = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
        let second = "112233445566778899aabbccddeeff00";
        let answers = vec![
            approve_of(first),
            approve_of(first),
            Answer {
                interrupt: second.to_string(),
                decision: Decision::Reject {
                    message: "no".into(),
                },
            },
        ];
        let body = resume_request_body(&answers, None, None, None);
        let resume = &body["command"]["resume"];

        assert!(resume.get("decisions").is_none(), "not the legacy shape: {resume}");
        assert_eq!(
            resume[first]["decisions"].as_array().map(Vec::len),
            Some(2),
            "both of the first interrupt's commands, in order: {resume}"
        );
        assert_eq!(resume[first]["decisions"][0]["type"], "approve");
        assert_eq!(resume[second]["decisions"][0]["type"], "reject");
        assert_eq!(resume[second]["decisions"][0]["message"], "no");

        // **One interrupt still uses the map**, because there is nothing to lose by it and the
        // shape a researcher meets should not depend on how many specialists happened to run.
        let one = resume_request_body(&[approve_of(first)], None, None, None);
        assert_eq!(one["command"]["resume"][first]["decisions"][0]["type"], "approve");

        // A backend that sends no ids gets the legacy shape — correct there, because a version
        // that cannot name its interrupts cannot have had two pending.
        let legacy = resume_request_body(&[approve_of("")], None, None, None);
        assert_eq!(legacy["command"]["resume"]["decisions"][0]["type"], "approve");

        // Mixed is treated as legacy on purpose: a half-filled map is not a partial improvement,
        // it is the old shape with a nonsense key, and LangGraph would read it as such.
        let mixed = resume_request_body(&[approve_of(first), approve_of("")], None, None, None);
        assert_eq!(
            mixed["command"]["resume"]["decisions"].as_array().map(Vec::len),
            Some(2),
            "{}",
            mixed["command"]["resume"]
        );
    }

    /// The id has to survive the decode, or nothing above it can use it.
    #[test]
    fn an_interrupt_carries_its_id_into_every_action_it_holds() {
        let id = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
        let request = decode_interrupt(&json!({
            "__interrupt__": [{
                "id": id,
                "value": {
                    "action_requests": [
                        {"name": "execute", "args": {"command": "python a.py"}},
                        {"name": "execute", "args": {"command": "python b.py"}},
                    ]
                }
            }]
        }))
        .expect("an approval request");
        assert_eq!(request.actions.len(), 2);
        assert!(request.actions.iter().all(|a| a.interrupt == id));

        // `interrupt_id` is the deprecated spelling and still read, because the version installed
        // on a researcher's machine is the one that matters.
        let older = decode_interrupt(&json!({
            "__interrupt__": [{
                "interrupt_id": id,
                "value": {"action_requests": [{"name": "execute", "args": {}}]}
            }]
        }))
        .expect("an approval request");
        assert_eq!(older.actions[0].interrupt, id);

        // And no id at all is empty rather than invented — that is what selects the legacy resume.
        let none = decode_interrupt(&json!({
            "__interrupt__": [{"value": {"action_requests": [{"name": "execute", "args": {}}]}}]
        }))
        .expect("an approval request");
        assert!(none.actions[0].interrupt.is_empty());

        // Every held action of every interrupt, keyed correctly across both.
        let second = "112233445566778899aabbccddeeff00";
        let both = decode_interrupt(&json!({
            "__interrupt__": [
                {"id": id, "value": {"action_requests": [{"name": "execute", "args": {}}]}},
                {"id": second, "value": {"action_requests": [{"name": "execute", "args": {}}]}},
            ]
        }))
        .expect("an approval request");
        assert_eq!(both.actions.len(), 2);
        assert_eq!(both.actions[0].interrupt, id);
        assert_eq!(both.actions[1].interrupt, second);
    }

    /// Reading and writing the spine must name the same project, or a saved mission lands in a
    /// namespace the panel never reads and the edit looks like it silently did nothing (§199).
    #[test]
    fn a_mission_is_written_to_the_project_it_was_read_from() {
        let ungrouped = LangGraphClient::new("http://127.0.0.1:2024");
        assert_eq!(ungrouped.project_url(), "http://127.0.0.1:2024/project");

        let filed = LangGraphClient::new("http://127.0.0.1:2024")
            .with_project(Some("Potato Late Blight".into()));
        assert_eq!(
            filed.project_url(),
            "http://127.0.0.1:2024/project?project=Potato%20Late%20Blight",
            "the overlay reads `?project` on GET and PATCH alike",
        );

        // Whitespace is not a project. `with_project(Some("  "))` used to be indistinguishable
        // from naming one, and would have scoped the write to a namespace nothing else uses.
        let blank =
            LangGraphClient::new("http://127.0.0.1:2024").with_project(Some("  ".into()));
        assert_eq!(blank.project_url(), "http://127.0.0.1:2024/project");
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
        assert_eq!(decoded, vec![TurnEvent::Status("Creating sandbox…".into())]);

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
            (
                json!({"citation": "Love MI et al. 2014."}),
                "Love MI et al. 2014.",
            ),
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
        assert_eq!(decode(&string_form), vec![TurnEvent::Token("plain".into())]);
        assert_eq!(decode(&block_form), vec![TurnEvent::Token("blocks".into())]);
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
        let outer = agent_ref(
            "tools:aaa",
            Some(&json!({"lc_agent_name": "coordinator_two"})),
        );
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
                label: "delegating to academic_researcher — Find the canonical DESeq2 paper."
                    .into(),
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
        assert_eq!(
            summarize_agent_result(r#"{"summary":"half"#),
            r#"{"summary":"half"#
        );
    }

    #[test]
    fn surfaces_error_events() {
        let err = SseEvent {
            name: "error".into(),
            data: r#"{"message":"boom"}"#.into(),
        };
        assert_eq!(decode(&err), vec![TurnEvent::Error("boom".into())]);
    }

    #[test]
    fn handles_crlf_terminators() {
        let mut decoder = SseDecoder::default();
        let events = decoder.push(
            b"event: messages\r\ndata: [{\"type\":\"AIMessageChunk\",\"content\":\"crlf\"},{}]\r\n\r\n",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(decode(&events[0]), vec![TurnEvent::Token("crlf".into())]);
    }

    /// The run that prompted all of this: five records of one multi-site study whose titles are
    /// identical past the truncation the bucket applies.
    #[test]
    fn datasets_that_share_a_title_prefix_stay_five_rows() {
        let long = "Replication data for: Qualification of a Plant Disease Simulation Model; \
                    performance of the LATEBLIGHT Model for";
        let artifacts = json!({"datasets": [
            {"title": format!("{long} New York, USA, 2001"), "persistent_id": "doi:10.21223/P3/0F9T62"},
            {"title": format!("{long} Elbaz, Israel, 2000"), "persistent_id": "doi:10.21223/P3/XKKVJS"},
            {"title": format!("{long} Nir-Eliyahu, Israel, 2000"), "persistent_id": "doi:10.21223/P3/XIF8Q9"},
            {"title": format!("{long} Sde-Varburg, Israel, 2000"), "persistent_id": "doi:10.21223/P3/DNJKC3"},
        ]});
        let decoded = decode_datasets(&artifacts);
        assert_eq!(decoded.len(), 4);
        let ids: Vec<&str> = decoded.iter().map(|d| d.persistent_id.as_str()).collect();
        assert_eq!(ids[0], "doi:10.21223/P3/0F9T62");
        assert_eq!(ids[3], "doi:10.21223/P3/DNJKC3");
    }

    /// A dataset with no identifier cannot be opened, cited or access-checked.
    #[test]
    fn a_finding_with_no_persistent_id_is_dropped() {
        let decoded = decode_datasets(&json!({"datasets": [
            {"title": "Nameless"},
            {"title": "Real", "persistent_id": "doi:10.21223/P3/AAA"},
        ]}));
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].title, "Real");
    }

    /// The same dataset recommended twice is one row.
    #[test]
    fn a_repeated_identifier_appears_once() {
        let decoded = decode_datasets(&json!({"datasets": [
            {"title": "First wording", "persistent_id": "doi:10.21223/P3/AAA"},
            {"title": "Second wording", "persistent_id": "doi:10.21223/P3/AAA"},
        ]}));
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].title, "First wording");
    }

    /// Every row must open somewhere: `persistent_id` is required by the schema, a link is not.
    #[test]
    fn a_dataset_without_a_link_still_opens_through_its_doi() {
        let decoded = decode_datasets(&json!({"datasets": [
            {"title": "No link", "persistent_id": "doi:10.21223/P3/0F9T62"},
        ]}));
        assert_eq!(
            decoded[0].page().as_deref(),
            Some("https://doi.org/10.21223/P3/0F9T62")
        );
    }

    /// The payload's own link leads, because it is the one the backend resolved.
    #[test]
    fn a_supplied_link_is_preferred_over_a_built_one() {
        let decoded = decode_datasets(&json!({"datasets": [{
            "title": "Linked",
            "persistent_id": "doi:10.21223/P3/0F9T62",
            "doi_url": "https://data.cipotato.org/dataset.xhtml?persistentId=doi:10.21223/P3/0F9T62"
        }]}));
        assert!(decoded[0].page().expect("a page").contains("cipotato.org"));
    }

    /// A record whose title the model omitted still says which dataset it is.
    #[test]
    fn a_missing_title_falls_back_to_the_identifier() {
        let decoded = decode_datasets(&json!({"datasets": [
            {"persistent_id": "doi:10.21223/P3/AAA"},
        ]}));
        assert_eq!(decoded[0].title, "doi:10.21223/P3/AAA");
    }

    #[test]
    fn the_fuller_fields_are_carried_when_the_payload_has_them() {
        let decoded = decode_datasets(&json!({"datasets": [{
            "title": "Trials",
            "persistent_id": "doi:10.21223/P3/AAA",
            "authors": ["Andrade-Piedra, Jorge", "", "Forbes, Gregory"],
            "description": "Late blight epidemics.",
            "file_count": 6,
            "repository": "CIP Dataverse"
        }]}));
        let dataset = &decoded[0];
        assert_eq!(dataset.authors, vec!["Andrade-Piedra, Jorge", "Forbes, Gregory"]);
        assert_eq!(dataset.file_count, Some(6));
        assert_eq!(dataset.repository.as_deref(), Some("CIP Dataverse"));
        assert_eq!(dataset.description, "Late blight epidemics.");
    }

    #[test]
    fn no_datasets_key_is_an_empty_list_rather_than_a_panic() {
        assert!(decode_datasets(&json!({"sources": []})).is_empty());
    }

    /// The library is cumulative, and its documents are what a reader wants — not the envelopes.
    #[test]
    fn the_documents_are_flattened_out_of_the_library_artifacts() {
        let decoded = decode_documents(&json!({"libraries": [
            {"index_path": ".asta/documents", "paper_count": 2, "papers": [
                {"title": "Graph neural networks", "path": "Graph-neural-networks.pdf",
                 "doi": "10.1000/gnn", "summary": "Expressivity of message passing.",
                 "tags": ["gnn", "expressivity"], "page_count": 24},
                {"title": "Late blight", "path": "papers/blight.pdf"}]},
        ]}));
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].tags, vec!["gnn", "expressivity"]);
        assert_eq!(decoded[0].page_count, Some(24));
        assert_eq!(decoded[0].doi.as_deref(), Some("10.1000/gnn"));
        // The sparse one still renders rather than being dropped.
        assert_eq!(decoded[1].title, "Late blight");
        assert!(decoded[1].tags.is_empty());
    }

    /// Re-indexing a paper is how a library grows; two rows for one file would make the list and
    /// `paper_count` disagree in the one place a researcher would notice.
    #[test]
    fn the_same_document_indexed_twice_is_one_row() {
        let decoded = decode_documents(&json!({"libraries": [
            {"papers": [{"title": "First pass", "path": "a.pdf"}]},
            {"papers": [{"title": "Re-indexed", "path": "a.pdf"}]},
        ]}));
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].title, "First pass");
    }

    #[test]
    fn a_document_with_no_path_is_dropped_and_one_with_no_title_keeps_its_path() {
        let decoded = decode_documents(&json!({"libraries": [{"papers": [
            {"title": "Nowhere"},
            {"path": "untitled.pdf"},
        ]}]}));
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].title, "untitled.pdf");
    }

    #[test]
    fn no_libraries_key_is_an_empty_list() {
        assert!(decode_documents(&json!({"datasets": []})).is_empty());
    }

    /// §243: the sweep asks about a thread other than the open one, which is the whole point.
    #[test]
    fn an_unfinished_run_is_collected_from_whichever_thread_owns_it() {
        let running = Job {
            kind: JobKind::Analysis,
            task_id: "4c290c71-be43-421a-8273-2f98dcc7b331".into(),
            question: "SOC modelling".into(),
            context_id: Some("eb663608-04c1-4140-a673-ac8dc98a2507".into()),
            status: "working".into(),
        };
        // The route is built from the thread id it is asked about, not from whatever is open —
        // `watch_job` reads the current thread and returns when it changes, which is exactly why
        // an unattended run was never collected.
        let mine = running.route("01a02077-afba-7c41-8b13-6a1a8553a20b");
        let other = running.route("01a02025-13df-7f80-8de0-2df11585ab3e");
        assert_ne!(mine, other);
        assert!(mine.contains("01a02077-afba-7c41-8b13-6a1a8553a20b"), "{mine}");
        assert!(mine.contains(&running.task_id), "{mine}");
    }

}
