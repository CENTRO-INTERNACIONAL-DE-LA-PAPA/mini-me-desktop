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
//! We ask for `stream_mode: ["messages-tuple"]` — the same mode the React
//! frontend uses — and leave `stream_subgraphs` off, so we receive only the
//! *coordinator's* token chunks with no subagent namespaces to filter. In local
//! dev the backend needs no `Authorization` header (`backend/auth.py` admits an
//! unauthenticated `local-user`) and falls back to `OPENAI_API_KEY` from `.env`.

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
    /// The run finished cleanly.
    Done,
    /// The run failed; the string is display-safe.
    Error(String),
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
        "stream_mode": ["messages-tuple"],
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
        let body = run_request_body("hi");
        assert_eq!(body["stream_mode"], json!(["messages-tuple"]));
        assert_eq!(body["assistant_id"], "agent");
        assert_eq!(body["input"]["messages"][0]["type"], "human");
        assert_eq!(body["input"]["messages"][0]["content"], "hi");
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
