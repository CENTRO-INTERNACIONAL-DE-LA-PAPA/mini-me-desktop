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

/// Research outputs produced so far, as carried by a `values` event.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub buckets: Vec<Bucket>,
    /// The `values` payload nests the spine under `artifacts.project`, so a turn
    /// updates the mission for free — no extra `GET /project` round trip.
    pub project: Option<Project>,
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

/// Thin HTTP client bound to a backend base URL.
pub struct LangGraphClient {
    http: reqwest::Client,
    base_url: String,
}

impl LangGraphClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
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

    /// `POST /threads` → a fresh thread id.
    pub async fn create_thread(&self) -> Result<String> {
        let resp = self
            .http
            .post(format!("{}/threads", self.base_url))
            .json(&json!({}))
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

    /// Stream one coordinator turn, invoking `on_event` for each decoded event.
    ///
    /// Kept as a callback rather than a returned stream so the caller (the
    /// sidecar task) can forward straight into a channel without buffering the
    /// whole turn.
    pub async fn stream_turn(
        &self,
        thread_id: &str,
        prompt: &str,
        mut on_event: impl FnMut(TurnEvent),
    ) -> Result<()> {
        let body = run_request_body(prompt);

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

        let mut frames = SseDecoder::default();
        let mut turn = TurnDecoder::default();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("the run stream broke mid-turn")?;
            for event in frames.push(&bytes) {
                for decoded in turn.push(&event) {
                    on_event(decoded);
                }
            }
        }
        Ok(())
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
fn run_request_body(prompt: &str) -> Value {
    json!({
        "assistant_id": "agent",
        "input": { "messages": [ { "type": "human", "content": prompt } ] },
        "stream_mode": ["messages-tuple", "values", "custom"],
        // Without this the whole stream stops at the coordinator: a delegated turn
        // then emits a `task` tool call and nothing else until the answer, which is
        // the silent gap the activity trace exists to close. On a measured turn this
        // flag is the difference between 176 and 495 message events.
        "stream_subgraphs": true,
        // LangGraph defaults to 25 supersteps, and one turn already spends ~22 on
        // middleware alone (PII scrubbing, call limits, todos, skills, sandbox
        // sync) before any delegation -- so a multi-subagent research turn would
        // hit the ceiling and fail. The web frontend sets the same value
        // (`streamConfig.ts`: `{ recursionLimit: 10000 }`), so this matches the
        // client the backend was built against.
        "config": { "recursion_limit": 10_000 },
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

    if buckets.is_empty() && project.is_none() {
        return None;
    }
    Some(Snapshot { buckets, project })
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
                return decode_values(&event.data)
                    .map(|snapshot| vec![TurnEvent::Snapshot(snapshot)])
                    .unwrap_or_default()
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
        let body = run_request_body("hi");
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
