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
//! We ask for `stream_mode: ["messages-tuple", "values", "custom"]` and leave
//! `stream_subgraphs` off, so we receive only the *coordinator's* token chunks
//! with no subagent namespaces to filter:
//!
//! - `messages-tuple` → `event: messages` frames, `[chunk, metadata]` — the tokens
//! - `values`         → full state snapshots carrying `artifacts` (and the spine
//!                      nested at `artifacts.project`)
//! - `custom`         → `sandbox_status` provisioning progress
//!
//! Verified against a live backend that requesting all three still yields
//! `event: messages` (asking for plain `messages` instead of `messages-tuple`
//! degrades them to `messages/partial` frames and silently breaks tokens).
//!
//! In local dev the backend needs no `Authorization` header (`backend/auth.py`
//! admits an unauthenticated `local-user`) and falls back to `OPENAI_API_KEY`.

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
    /// A full snapshot of the run's artifacts (and the spine, which rides along).
    /// Emitted by the `values` stream mode; **replaces** prior state rather than
    /// accumulating, since each event carries the whole picture.
    Snapshot(Snapshot),
    /// The run finished cleanly.
    Done,
    /// The run failed; the string is display-safe.
    Error(String),
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

        let mut decoder = SseDecoder::default();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("the run stream broke mid-turn")?;
            for event in decoder.push(&bytes) {
                for decoded in decode_sse_event(&event) {
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

/// Map one SSE event onto UI events.
///
/// The `messages` stream mode emits a 2-element array `[message_chunk, metadata]`.
/// Assistant text arrives as `AIMessageChunk`s whose `content` is either a plain
/// string or a list of typed blocks.
pub fn decode_sse_event(event: &SseEvent) -> Vec<TurnEvent> {
    // Subagent tokens arrive as `messages|<namespace>`; we only asked for
    // top-level, but match the prefix so a future `stream_subgraphs` still works.
    let is_messages = event.name == "messages" || event.name.starts_with("messages|");

    if event.name == "error" {
        return vec![TurnEvent::Error(summarize_error(&event.data))];
    }
    if event.name == "metadata" {
        return vec![TurnEvent::Status("run started".into())];
    }
    if event.name == "values" {
        return decode_values(&event.data)
            .map(|snapshot| vec![TurnEvent::Snapshot(snapshot)])
            .unwrap_or_default();
    }
    if event.name == "custom" {
        return decode_custom(&event.data)
            .map(|status| vec![TurnEvent::Status(status)])
            .unwrap_or_default();
    }
    if !is_messages {
        return Vec::new();
    }

    let Ok(value) = serde_json::from_str::<Value>(&event.data) else {
        return Vec::new();
    };
    // Expected shape: [chunk, metadata].
    let Some(chunk) = value.get(0) else {
        return Vec::new();
    };
    if chunk.get("type").and_then(Value::as_str) != Some("AIMessageChunk") {
        return Vec::new();
    }
    let text = chunk
        .get("content")
        .map(extract_text)
        .unwrap_or_default();
    if text.is_empty() {
        return Vec::new();
    }
    vec![TurnEvent::Token(text)]
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

        let decoded = decode_sse_event(&SseEvent {
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
        let decoded = decode_sse_event(&SseEvent {
            name: "custom".into(),
            data: json!({"sandbox_status": {"state": "preparing", "message": "Creating sandbox…"}})
                .to_string(),
        });
        assert_eq!(
            decoded,
            vec![TurnEvent::Status("Creating sandbox…".into())]
        );

        // Falls back to the state when no message is given.
        let decoded = decode_sse_event(&SseEvent {
            name: "custom".into(),
            data: json!({"sandbox_status": {"state": "ready"}}).to_string(),
        });
        assert_eq!(decoded, vec![TurnEvent::Status("ready".into())]);

        // Unrelated custom payloads are ignored rather than shown as noise.
        let decoded = decode_sse_event(&SseEvent {
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
        let decoded = decode_sse_event(&SseEvent {
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
                decoded.extend(decode_sse_event(&event));
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
            decode_sse_event(&string_form),
            vec![TurnEvent::Token("plain".into())]
        );
        assert_eq!(
            decode_sse_event(&block_form),
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
        assert!(decode_sse_event(&human).is_empty());
        assert!(decode_sse_event(&values).is_empty());
    }

    #[test]
    fn accepts_subagent_namespaced_messages() {
        let ns = SseEvent {
            name: "messages|theorizer:abc".into(),
            data: r#"[{"type":"AIMessageChunk","id":"m2","content":"sub"},{}]"#.into(),
        };
        assert_eq!(decode_sse_event(&ns), vec![TurnEvent::Token("sub".into())]);
    }

    #[test]
    fn surfaces_error_events() {
        let err = SseEvent {
            name: "error".into(),
            data: r#"{"message":"boom"}"#.into(),
        };
        assert_eq!(
            decode_sse_event(&err),
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
            decode_sse_event(&events[0]),
            vec![TurnEvent::Token("crlf".into())]
        );
    }
}
