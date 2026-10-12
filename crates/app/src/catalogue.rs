//! The models a provider actually offers, asked of the provider rather than remembered here.
//!
//! # Why a curated list was never going to be enough
//!
//! [`settings::PROVIDERS`] carries four or five model ids each, and §58 already recorded why they
//! are a starting point and not a catalogue: *"a provider ships a model the day after a release
//! and typing it must still work"*. The field stays editable for exactly that reason.
//!
//! That holds up for Anthropic or OpenAI, whose useful list is short enough to name. It falls
//! apart for an OpenAI-compatible gateway. OpenRouter carries several hundred models — including
//! the open-weight ones a research centre has good reason to prefer, DeepSeek and Kimi and
//! Llama among them — and a hand-written four cannot represent that. Asked for directly: *"for
//! the case of openrouter which have more models including opensource models like kimi or
//! deepseek we should have a longer list."*
//!
//! # What leaves the machine
//!
//! One `GET` to the provider's own `/models` endpoint, carrying the key already stored for that
//! provider — the same key the same company receives on every turn. Never a question, never a
//! conversation, never a file. OpenRouter's endpoint needs no key at all, which is the one case
//! where a list can be fetched before anything is configured.
//!
//! Org policy forbids sending confidential or unpublished material to third parties. A request
//! for "what models do you have" sends neither.
//!
//! # Why it is cached on disk
//!
//! A picker that blocks on the network is a picker that hangs when a researcher is on a train.
//! The list is read from a file, and refreshed in the background when it is a day old — so the
//! panel always opens instantly, on the last answer the provider gave, and quietly improves.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// How long a fetched list is trusted before it is worth asking again.
///
/// A day. Providers announce models on their own schedule and nobody needs a list fresher than
/// that; the cost of being wrong is one editable text field, which §58 kept editable for this.
pub const STALE_AFTER_MS: u64 = 24 * 60 * 60 * 1000;

/// Everything one provider said it offers, and when it said so.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    /// Model ids, exactly as the provider spells them.
    pub models: Vec<String>,
    /// Unix milliseconds. Zero for a listing that has never been fetched.
    pub fetched_ms: u64,
}

impl Listing {
    pub fn is_stale(&self, now_ms: u64) -> bool {
        // **Never fetched is stale, said rather than arithmetic.** With a real clock
        // `now - 0` is a colossal number and this falls out anyway, which is exactly why it is
        // worth stating: the one property that has to hold is "ask the first time", and leaving
        // it resting on the epoch being far away is a rule nobody wrote down.
        if self.fetched_ms == 0 {
            return true;
        }
        // Saturating, so a clock that moved backwards reports *fresh* rather than wrapping to a
        // vast age and refetching on every open.
        now_ms.saturating_sub(self.fetched_ms) >= STALE_AFTER_MS
    }
}

/// One file, provider id to listing, beside `settings.toml`.
pub type Catalogue = std::collections::BTreeMap<String, Listing>;

pub fn catalogue_path() -> PathBuf {
    crate::settings::settings_path()
        .parent()
        .map(|dir| dir.join("models.json"))
        .unwrap_or_else(|| PathBuf::from("models.json"))
}

/// Every listing on disk. An unreadable or malformed file is *no* listings, never an error:
/// the curated lists still work and a broken cache must not keep the panel from opening.
pub fn load() -> Catalogue {
    std::fs::read_to_string(catalogue_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(catalogue: &Catalogue) -> Result<()> {
    let path = catalogue_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(catalogue).context("could not serialise the models")?;
    std::fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))
}

/// Where to ask a provider what it offers, and whether the key has to go with the question.
///
/// `None` for a provider whose listing endpoint this app does not speak. Google's is neither
/// OpenAI-shaped nor Anthropic-shaped, and guessing at a third response shape to save one curated
/// list of three would be inventing work; it keeps its list from [`settings::PROVIDERS`].
pub fn endpoint(provider: &str, base_url: &str) -> Option<(String, Auth)> {
    match provider {
        "openai" => Some(("https://api.openai.com/v1/models".into(), Auth::Bearer)),
        "mistral" => Some(("https://api.mistral.ai/v1/models".into(), Auth::Bearer)),
        "anthropic" => Some((
            "https://api.anthropic.com/v1/models".into(),
            Auth::AnthropicHeader,
        )),
        // Whatever endpoint the researcher pointed at — this is the case the feature exists for,
        // and the one where the answer is hundreds of models rather than four.
        "custom" => {
            let base = base_url.trim().trim_end_matches('/');
            (!base.is_empty()).then(|| (format!("{base}/models"), Auth::BearerIfPresent))
        }
        _ => None,
    }
}

/// How a provider wants to be told who is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    Bearer,
    AnthropicHeader,
    /// OpenRouter answers `/models` to anybody, which is what lets the list arrive before a key
    /// does. Other gateways may not, so the key is sent when there is one.
    BearerIfPresent,
}

/// Read model ids out of whichever shape came back.
///
/// Two shapes, because two are all that are asked for: `{"data": [{"id": …}]}` is the
/// OpenAI-compatible listing that OpenAI, Mistral and every gateway including OpenRouter return,
/// and Anthropic's `/v1/models` happens to use the same envelope.
///
/// **Sorted and de-duplicated**, because a picker's order should not depend on what a server felt
/// like returning, and OpenRouter lists some ids more than once across its provider routes.
///
/// **Models that say they cannot call tools are left out.** Mini-Me does nothing without tools,
/// and such a model fails on the first request of every turn. OpenRouter lists each model's
/// `supported_parameters`; one that has the list but not `"tools"` is skipped. A listing without
/// the field (OpenAI, Mistral, Anthropic, most gateways) is kept, because absence says nothing.
pub fn parse(body: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = body
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| can_use_tools(entry))
                .filter_map(|entry| entry.get("id")?.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

/// Whether a listing entry allows tool calls, as far as it says.
fn can_use_tools(entry: &serde_json::Value) -> bool {
    match entry
        .get("supported_parameters")
        .and_then(serde_json::Value::as_array)
    {
        Some(parameters) => parameters.iter().any(|parameter| parameter == "tools"),
        None => true,
    }
}

/// Ask one provider what it offers.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    auth: Auth,
    api_key: Option<&str>,
) -> Result<Vec<String>> {
    let mut request = client.get(url);
    match (auth, api_key) {
        (Auth::Bearer | Auth::BearerIfPresent, Some(key)) => {
            request = request.bearer_auth(key);
        }
        (Auth::AnthropicHeader, Some(key)) => {
            request = request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        }
        // No key and none required — OpenRouter's case.
        (Auth::BearerIfPresent, None) => {}
        (_, None) => anyhow::bail!("no key stored for this provider"),
    }
    let body: serde_json::Value = request
        .send()
        .await
        .with_context(|| format!("could not reach {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} refused the request"))?
        .json()
        .await
        .with_context(|| format!("{url} did not return a model list"))?;
    let models = parse(&body);
    if models.is_empty() {
        anyhow::bail!("{url} returned no models");
    }
    Ok(models)
}

/// The models to offer for a provider: what it last told us, or the curated list until it does.
///
/// **Never the union.** A fetched list is the provider's own answer and a curated one is this
/// repo's guess; merging them would put ids that have been retired back into the picker forever,
/// and the whole point is that the provider is the authority on this.
pub fn models_for(provider: &crate::settings::Provider, catalogue: &Catalogue) -> Vec<String> {
    match catalogue.get(provider.id) {
        Some(listing) if !listing.models.is_empty() => listing.models.clone(),
        _ => provider.models.iter().map(|id| (*id).to_string()).collect(),
    }
}

/// Where to ask a provider whether a key is good.
///
/// **Not `endpoint`.** OpenRouter's `/models` answers anybody, so a model list arriving proves
/// nothing about the key — a machine with a revoked key refreshed 464 models and then failed
/// every turn with `401 User not found`. OpenRouter's `/key` is the endpoint that checks it.
/// Other OpenAI-compatible gateways get `/models` with the key, which the ones that need a key
/// refuse when it is wrong.
pub fn key_check_url(provider: &str, base_url: &str) -> Option<String> {
    match provider {
        "openai" => Some("https://api.openai.com/v1/models".into()),
        "mistral" => Some("https://api.mistral.ai/v1/models".into()),
        "anthropic" => Some("https://api.anthropic.com/v1/models".into()),
        "google" => Some("https://generativelanguage.googleapis.com/v1beta/models".into()),
        "custom" => {
            let base = base_url.trim().trim_end_matches('/');
            if base.is_empty() {
                None
            } else if base.contains("openrouter.ai") {
                Some(format!("{base}/key"))
            } else {
                Some(format!("{base}/models"))
            }
        }
        _ => None,
    }
}

/// Ask the provider whether `key` is accepted. One `GET`, no prompt, nothing billed.
///
/// `Ok` carries what to tell the researcher; `Err` says why it failed, in their words.
pub async fn test_key(
    client: &reqwest::Client,
    provider: &str,
    base_url: &str,
    key: &str,
) -> std::result::Result<String, String> {
    let Some(url) = key_check_url(provider, base_url) else {
        return Err(if provider == "custom" {
            "set the endpoint URL in Settings first".into()
        } else {
            "this provider can't be tested from here".into()
        });
    };
    let request = match provider {
        "anthropic" => client
            .get(&url)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        "google" => client.get(&url).header("x-goog-api-key", key),
        _ => client.get(&url).bearer_auth(key),
    };
    let response = request
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| format!("could not reach {url} — check the internet connection"))?;
    let status = response.status();
    if status.is_success() {
        Ok("the key works".into())
    } else if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        Err("the provider rejected this key — check it was copied in full, or create a new one".into())
    } else {
        Err(format!("the provider answered {status}"))
    }
}

// ------------------------------------------------------------ does the model work with tools?
//
// **Why a live request and not the listing.** On 2026-10-12 OpenRouter's `supported_parameters`
// listed `tools` and `tool_choice` for Claude Sonnet 5.5 and Opus 5.5 exactly as for Claude
// Sonnet 4.5 and GPT-5.4 mini, yet the first two refuse to be *required* to call a tool
// (HTTP 400 "tool_choice: type "tool" and "any" are not supported for this model"). Mini-Me
// requires one wherever a specialist must search or run a step before it answers, and wherever
// a specialist returns a structured answer, so on those models such specialists fail and the
// researcher saw only "Internal error". The listing cannot tell these apart; one tiny request
// that requires a tool call can.

/// How well a model works with the tools Mini-Me's specialists use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "reason", rename_all = "lowercase")]
pub enum ToolSupport {
    /// Calls tools, including when required to call a specific one.
    Full,
    /// Calls tools, but refuses to be required to call one.
    Partial,
    /// Cannot call tools at all.
    None,
    /// The check could not run or could not tell; the reason is for the researcher.
    Unknown(String),
}

impl ToolSupport {
    /// Whether this is an answer about the model rather than about the check, and so worth
    /// remembering. "No key yet" or "offline" must be asked again next time.
    pub fn is_definitive(&self) -> bool {
        !matches!(self, ToolSupport::Unknown(_))
    }
}

/// Models seen working with every specialist, named in the warning so a researcher has
/// somewhere to go. Short on purpose; extend it as models are confirmed.
pub const KNOWN_GOOD: [&str; 2] = ["Claude Sonnet 4.5", "GPT-5.4 mini"];

/// Enough for a reasoning model to think briefly and still call the tool; far below a cent.
const PROBE_MAX_TOKENS: u64 = 512;

/// The request that asks `model` to call one specific tool, or `None` for a provider whose
/// chat API this app does not speak. Pure, so each provider's shape is pinned by a test.
pub fn probe_request(
    provider: &str,
    base_url: &str,
    model: &str,
) -> Option<(String, serde_json::Value)> {
    const PROMPT: &str = "Call the ping tool.";
    let parameters = serde_json::json!({
        "type": "object",
        // One optional property rather than none: Gemini rejects an OBJECT schema without any.
        "properties": {"ok": {"type": "boolean"}},
    });
    let openai_shaped = |url: String, max_tokens_field: &str| {
        let mut body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": PROMPT}],
            "tools": [{
                "type": "function",
                "function": {"name": "ping", "description": "Connectivity check.", "parameters": parameters},
            }],
            "tool_choice": {"type": "function", "function": {"name": "ping"}},
        });
        body[max_tokens_field] = serde_json::json!(PROBE_MAX_TOKENS);
        (url, body)
    };
    match provider {
        // OpenAI's newer models refuse `max_tokens` and want `max_completion_tokens`.
        "openai" => Some(openai_shaped(
            "https://api.openai.com/v1/chat/completions".into(),
            "max_completion_tokens",
        )),
        "mistral" => Some(openai_shaped(
            "https://api.mistral.ai/v1/chat/completions".into(),
            "max_tokens",
        )),
        "custom" => {
            let base = base_url.trim().trim_end_matches('/');
            (!base.is_empty())
                .then(|| openai_shaped(format!("{base}/chat/completions"), "max_tokens"))
        }
        "anthropic" => Some((
            "https://api.anthropic.com/v1/messages".into(),
            serde_json::json!({
                "model": model,
                "max_tokens": PROBE_MAX_TOKENS,
                "messages": [{"role": "user", "content": PROMPT}],
                "tools": [{"name": "ping", "description": "Connectivity check.", "input_schema": parameters}],
                "tool_choice": {"type": "tool", "name": "ping"},
            }),
        )),
        "google" => Some((
            format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"),
            serde_json::json!({
                "contents": [{"role": "user", "parts": [{"text": PROMPT}]}],
                "tools": [{"functionDeclarations": [
                    {"name": "ping", "description": "Connectivity check.", "parameters": parameters},
                ]}],
                "toolConfig": {"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["ping"]}},
                "generationConfig": {"maxOutputTokens": PROBE_MAX_TOKENS},
            }),
        )),
        _ => None,
    }
}

/// Whether an error says the model will not be *required* to call a tool, as opposed to not
/// calling tools at all ([`model_cannot_use_tools`]).
pub fn model_refuses_forced_tools(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("tool_choice")
        && ["not supported", "unsupported", "does not support", "is not available"]
            .iter()
            .any(|marker| message.contains(marker))
}

/// Whether an error says the model cannot call tools at all.
///
/// Providers word the refusal differently — OpenRouter answers 404 *"No endpoints found that
/// support tool use"*, Ollama-style servers *"… does not support tools"*, others name function
/// calling — so the check is a short list of phrasings, compared case-insensitively.
pub fn model_cannot_use_tools(message: &str) -> bool {
    const MARKERS: [&str; 7] = [
        "support tool use",
        "does not support tools",
        "do not support tools",
        "tools are not supported",
        "tool use is not supported",
        "does not support function calling",
        "function calling is not supported",
    ];
    let message = message.to_lowercase();
    MARKERS.iter().any(|marker| message.contains(marker))
}

/// What a probe's answer says about the model. Pure, so real provider responses can be pinned.
pub fn classify(status: u16, body: &str) -> ToolSupport {
    if (200..300).contains(&status) {
        let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let openai_call = json
            .pointer("/choices/0/message/tool_calls")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|calls| !calls.is_empty());
        let anthropic_call = json
            .get("content")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_use"));
        let google_call = body.contains("\"functionCall\"");
        if openai_call || anthropic_call || google_call {
            return ToolSupport::Full;
        }
        // Ran out of room before calling anything: says nothing about tools.
        let length = serde_json::Value::from("length");
        let max_tokens = serde_json::Value::from("max_tokens");
        let truncated = json.pointer("/choices/0/finish_reason") == Some(&length)
            || json.get("stop_reason") == Some(&max_tokens)
            || body.contains("\"MAX_TOKENS\"");
        if truncated {
            return ToolSupport::Unknown("the test answer was cut short".into());
        }
        return ToolSupport::Partial;
    }
    if model_refuses_forced_tools(body) {
        ToolSupport::Partial
    } else if model_cannot_use_tools(body) {
        ToolSupport::None
    } else if status == 401 || status == 403 {
        ToolSupport::Unknown("the provider rejected the API key".into())
    } else {
        ToolSupport::Unknown(format!("the provider answered {status}"))
    }
}

/// Ask `model` to call one specific tool, and say what that shows.
///
/// The key goes only into this request's auth header: it is never logged, and only the outcome
/// is kept ([`save_model_checks`]).
pub async fn check_model_tools(
    client: &reqwest::Client,
    provider: &str,
    base_url: &str,
    key: &str,
    model: &str,
) -> ToolSupport {
    let Some((url, body)) = probe_request(provider, base_url, model) else {
        return ToolSupport::Unknown(if provider == "custom" {
            "set the endpoint URL first".into()
        } else {
            "this provider can't be checked from here".into()
        });
    };
    let request = match provider {
        "anthropic" => client
            .post(&url)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        "google" => client.post(&url).header("x-goog-api-key", key),
        _ => client.post(&url).bearer_auth(key),
    };
    let response = match request
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return ToolSupport::Unknown("could not reach the provider — check the connection".into())
        }
    };
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    classify(status, &text)
}

/// A remembered check: what it found, and when.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelCheck {
    pub support: ToolSupport,
    /// Unix milliseconds.
    pub checked_ms: u64,
}

/// Remembered checks, keyed by [`model_check_key`], in a file beside `models.json`.
pub type ModelChecks = std::collections::BTreeMap<String, ModelCheck>;

/// One model at one endpoint. The base URL counts only for `custom`, where the same id at a
/// different gateway can be served by a different provider.
pub fn model_check_key(provider: &str, base_url: &str, model: &str) -> String {
    let base = if provider == "custom" {
        base_url.trim().trim_end_matches('/')
    } else {
        ""
    };
    format!("{provider}|{base}|{model}")
}

pub fn model_checks_path() -> PathBuf {
    catalogue_path().with_file_name("model-checks.json")
}

/// Every remembered check. A missing or malformed file is none: the checks run again.
pub fn load_model_checks() -> ModelChecks {
    std::fs::read_to_string(model_checks_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_model_checks(checks: &ModelChecks) -> Result<()> {
    let path = model_checks_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(checks).context("could not serialise the checks")?;
    std::fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_keys_are_checked_where_a_bad_key_is_refused() {
        // `/models` answers without a key, so it can't tell a good key from a revoked one.
        assert_eq!(
            key_check_url("custom", "https://openrouter.ai/api/v1/").as_deref(),
            Some("https://openrouter.ai/api/v1/key")
        );
        assert_eq!(
            key_check_url("custom", "https://api.groq.com/openai/v1").as_deref(),
            Some("https://api.groq.com/openai/v1/models")
        );
        assert_eq!(key_check_url("custom", "  "), None);
    }

    #[test]
    fn reads_the_shape_every_openai_compatible_gateway_returns() {
        // OpenRouter, OpenAI and Mistral all answer in this envelope, and Anthropic's `/v1/models`
        // happens to as well — which is why two shapes were never needed.
        let body = serde_json::json!({
            "data": [
                {"id": "deepseek/deepseek-r1"},
                {"id": "moonshotai/kimi-k2"},
                {"id": "openai/gpt-4.1"},
                // A duplicate across provider routes, which OpenRouter really does return.
                {"id": "openai/gpt-4.1"},
                // Junk that must not become a row somebody can select.
                {"id": "   "},
                {"name": "no id at all"}
            ]
        });
        assert_eq!(
            parse(&body),
            [
                "deepseek/deepseek-r1",
                "moonshotai/kimi-k2",
                "openai/gpt-4.1"
            ]
        );
        // Sorted, so the order does not depend on what the server felt like returning.
        assert!(parse(&body).windows(2).all(|pair| pair[0] < pair[1]));
        // A shape we do not understand is no models, not a panic and not a half-read list.
        assert!(parse(&serde_json::json!({"models": ["gemini-2.5-pro"]})).is_empty());
        assert!(parse(&serde_json::json!([])).is_empty());
    }

    #[test]
    fn the_tool_check_requires_one_named_tool_in_each_providers_shape() {
        let (url, body) = probe_request("custom", "https://openrouter.ai/api/v1/", "anthropic/claude-sonnet-5.5")
            .expect("custom endpoints are checked");
        assert_eq!(url, "https://openrouter.ai/api/v1/chat/completions");
        assert_eq!(body["tool_choice"]["function"]["name"], "ping");
        assert_eq!(body["max_tokens"], PROBE_MAX_TOKENS);

        let (_, body) = probe_request("openai", "", "gpt-5.4-mini").unwrap();
        assert!(body.get("max_completion_tokens").is_some(), "{body}");
        assert!(body.get("max_tokens").is_none(), "{body}");

        let (url, body) = probe_request("anthropic", "", "claude-sonnet-4-5").unwrap();
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
        assert_eq!(body["tool_choice"], serde_json::json!({"type": "tool", "name": "ping"}));

        let (url, body) = probe_request("google", "", "gemini-2.5-pro").unwrap();
        assert!(url.ends_with("/models/gemini-2.5-pro:generateContent"), "{url}");
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");

        assert!(probe_request("custom", "  ", "x").is_none());
        assert!(probe_request("somewhere-else", "", "x").is_none());
    }

    #[test]
    fn a_model_that_refuses_a_required_tool_is_told_apart_from_one_that_works() {
        // Verbatim from the backend log of 2026-10-12 (Claude Sonnet 5.5 through OpenRouter).
        let refused = r#"{"error":{"message":"Provider returned error","code":400,"metadata":{"raw":"{\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"tool_choice: type \\\"tool\\\" and \\\"any\\\" are not supported for this model.\"}}","provider_name":"Amazon Bedrock"}}}"#;
        assert_eq!(classify(400, refused), ToolSupport::Partial);

        let called = r#"{"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"c1","type":"function","function":{"name":"ping","arguments":"{}"}}]}}]}"#;
        assert_eq!(classify(200, called), ToolSupport::Full);

        let anthropic = r#"{"content":[{"type":"tool_use","id":"t1","name":"ping","input":{}}],"stop_reason":"tool_use"}"#;
        assert_eq!(classify(200, anthropic), ToolSupport::Full);

        let no_tools = r#"{"error":{"message":"No endpoints found that support tool use.","code":404}}"#;
        assert_eq!(classify(404, no_tools), ToolSupport::None);

        let cut_short = r#"{"choices":[{"finish_reason":"length","message":{"content":""}}]}"#;
        assert!(matches!(classify(200, cut_short), ToolSupport::Unknown(_)));
        assert!(matches!(classify(401, "{}"), ToolSupport::Unknown(reason) if reason.contains("API key")));
        assert!(matches!(classify(500, "oops"), ToolSupport::Unknown(_)));
    }

    #[test]
    fn a_remembered_check_survives_a_round_trip_and_never_holds_a_key() {
        let mut checks = ModelChecks::new();
        checks.insert(
            model_check_key("custom", "https://openrouter.ai/api/v1/", "anthropic/claude-sonnet-5.5"),
            ModelCheck { support: ToolSupport::Partial, checked_ms: 1 },
        );
        checks.insert(
            model_check_key("anthropic", "ignored for named providers", "claude-sonnet-4-5"),
            ModelCheck { support: ToolSupport::Full, checked_ms: 2 },
        );
        let text = serde_json::to_string(&checks).unwrap();
        assert!(text.contains("\"outcome\":\"partial\""), "{text}");
        assert_eq!(serde_json::from_str::<ModelChecks>(&text).unwrap(), checks);
        assert!(checks.contains_key("anthropic||claude-sonnet-4-5"));
        assert!(checks.contains_key("custom|https://openrouter.ai/api/v1|anthropic/claude-sonnet-5.5"));
        assert!(!ToolSupport::Unknown("offline".into()).is_definitive());
    }

    #[test]
    fn models_that_say_they_cannot_call_tools_are_not_offered() {
        // OpenRouter's shape: each entry lists the parameters it accepts.
        let body = serde_json::json!({
            "data": [
                {"id": "a/with-tools", "supported_parameters": ["temperature", "tools", "tool_choice"]},
                {"id": "b/no-tools", "supported_parameters": ["temperature", "max_tokens"]},
                // No field at all is how every other provider answers, and says nothing.
                {"id": "c/unknown"}
            ]
        });
        assert_eq!(parse(&body), ["a/with-tools", "c/unknown"]);
    }

    #[test]
    fn a_custom_gateway_is_asked_at_the_url_the_researcher_gave() {
        // The case the feature exists for: OpenRouter's own catalogue, at the base URL already
        // configured, needing no key — which is what lets the list arrive before one is stored.
        let (url, auth) = endpoint("custom", "https://openrouter.ai/api/v1").expect("an endpoint");
        assert_eq!(url, "https://openrouter.ai/api/v1/models");
        assert_eq!(auth, Auth::BearerIfPresent);
        // A trailing slash is what somebody pastes, and must not produce a doubled one.
        let (slashed, _) = endpoint("custom", "https://openrouter.ai/api/v1/").expect("endpoint");
        assert_eq!(slashed, "https://openrouter.ai/api/v1/models");
        // No base URL is nothing to ask, not a request to a malformed address.
        assert!(endpoint("custom", "   ").is_none());

        // Google's listing is neither shape this module reads, so it is not claimed.
        assert!(endpoint("google", "").is_none());
        assert_eq!(endpoint("openai", "").map(|(_, a)| a), Some(Auth::Bearer));
        assert_eq!(
            endpoint("anthropic", "").map(|(_, a)| a),
            Some(Auth::AnthropicHeader)
        );
    }

    #[test]
    fn the_provider_is_the_authority_once_it_has_answered() {
        let openai = crate::settings::provider("openai").expect("a shipped provider");
        let mut catalogue = Catalogue::new();

        // Until it answers, the curated list is what there is.
        assert_eq!(
            models_for(openai, &catalogue),
            openai.models.iter().map(|m| m.to_string()).collect::<Vec<_>>()
        );

        // Once it has, the curated list is *replaced* rather than merged: a union would keep a
        // retired model in the picker forever, and the provider is the authority here.
        catalogue.insert(
            "openai".into(),
            Listing {
                models: vec!["gpt-6".into()],
                fetched_ms: 1,
            },
        );
        assert_eq!(models_for(openai, &catalogue), ["gpt-6"]);

        // An empty answer is treated as no answer — a provider that returns nothing must not
        // leave somebody with an empty picker and no way back.
        catalogue.insert(
            "openai".into(),
            Listing {
                models: Vec::new(),
                fetched_ms: 1,
            },
        );
        assert_eq!(models_for(openai, &catalogue).len(), openai.models.len());
    }

    #[test]
    fn a_listing_goes_stale_after_a_day_and_a_missing_one_is_stale_from_the_start() {
        let day = STALE_AFTER_MS;
        let fresh = Listing {
            models: vec!["x".into()],
            fetched_ms: day,
        };
        assert!(!fresh.is_stale(day + 1));
        assert!(fresh.is_stale(day + day));
        // Never fetched, so there is nothing to trust — and `saturating_sub` keeps a clock that
        // moved backwards from reporting a listing as fresher than time itself.
        assert!(Listing::default().is_stale(0));
        assert!(!fresh.is_stale(0), "a backwards clock is not a refresh");
    }
}
