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
pub fn parse(body: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = body
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
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

#[cfg(test)]
mod tests {
    use super::*;

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
