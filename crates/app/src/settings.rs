//! User settings and secrets — kept in two different places, deliberately.
//!
//! **Settings** (`settings.toml`) are plain text: which provider, which model, whether
//! execution runs on the host. Readable, diffable, safe to paste into a bug report.
//!
//! **Secrets** (API keys) go in the **OS keychain** — Windows Credential Manager,
//! Secret Service, macOS Keychain — never in that file. A key must not land in
//! something the user might sync, zip, or attach to an email, and CIP policy is that
//! credentials stay the user's own on the user's own machine.
//!
//! This is what makes the app installable: the model key travels from the keychain into
//! the *run request* (`configurable.__llm_keys`, see `protocol.rs`), so nobody has to
//! hand-edit a `.env` inside a WSL distro to get started (docs §20).

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// The keychain service every secret is filed under.
const KEYCHAIN_SERVICE: &str = "mini-me-desktop";

/// A model provider the backend knows how to construct.
///
/// Mirrors `PROVIDER_SPECS` in the backend's `models.py`, whose table is commented
/// *"provider id (from the panel)"* — this is that panel. `custom` is an
/// OpenAI-compatible endpoint, which is how OpenRouter, Groq, Ollama and vLLM are
/// reached, and it is the only one where `base_url` is mandatory.
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub needs_base_url: bool,
    /// A model id that actually exists, so a fresh install has something that works
    /// rather than an empty field.
    pub suggested_model: &'static str,
    /// Models worth offering as a list, newest first.
    ///
    /// A short curated set, not a catalogue: nobody should have to remember whether it is
    /// `claude-sonnet-4-5` or `claude-4.5-sonnet` to get started. The field stays editable,
    /// because a list here can only ever be out of date — a provider ships a model the day
    /// after a release and typing it must still work (docs §58).
    pub models: &'static [&'static str],
}

pub const PROVIDERS: [Provider; 5] = [
    Provider {
        id: "anthropic",
        label: "Anthropic",
        needs_base_url: false,
        suggested_model: "claude-sonnet-4-5",
        models: &[
            "claude-opus-4-5",
            "claude-sonnet-4-5",
            "claude-haiku-4-5",
            "claude-3-7-sonnet-latest",
        ],
    },
    Provider {
        id: "openai",
        label: "OpenAI",
        needs_base_url: false,
        suggested_model: "gpt-5.4",
        models: &["gpt-5.4", "gpt-5", "gpt-4.1", "gpt-4o", "o4-mini"],
    },
    Provider {
        id: "google",
        label: "Google",
        needs_base_url: false,
        suggested_model: "gemini-2.5-pro",
        models: &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"],
    },
    Provider {
        id: "mistral",
        label: "Mistral",
        needs_base_url: false,
        suggested_model: "mistral-large-latest",
        models: &[
            "mistral-large-latest",
            "mistral-small-latest",
            "codestral-latest",
        ],
    },
    Provider {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        needs_base_url: true,
        suggested_model: "openai/gpt-4o-mini",
        // An OpenAI-compatible endpoint could be anything, so these are only examples of
        // the *shape* an OpenRouter or Ollama id takes.
        models: &[
            "openai/gpt-4o-mini",
            "anthropic/claude-sonnet-4.5",
            "meta-llama/llama-3.3-70b-instruct",
            "qwen2.5-coder:14b",
        ],
    },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

/// Everything that is *not* a secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: String,
    pub model_id: String,
    /// Only meaningful for the `custom` provider.
    pub base_url: String,
    /// Run the agent's code on this machine rather than in the remote sandbox.
    pub local_execution: bool,
    /// Ask before every `execute`. Off is for automation, not a recommendation.
    pub approve_execute: bool,
    pub backend_port: u16,
    /// Where the backend checkout lives. Empty means "the app-owned default".
    ///
    /// Written by the Setup pane when it adopts a checkout it found, so the discovery
    /// probe runs once rather than on every launch.
    pub backend_dir: String,
    /// Let the coordinator hand whole pieces of work to a background Mini-Me.
    ///
    /// Off by default: it rests on a **preview** deepagents API whose docs say "APIs may
    /// change", and it needs the extra graph the app generates into the checkout's config.
    /// Opt-in keeps a bad interaction from being everyone's first experience.
    pub async_subagents: bool,
    /// Which palette to draw with, by name (`theme.rs`), or the stem of a JSON file in
    /// `themes/` beside `settings.toml`.
    ///
    /// A setting rather than a constant because a fixed palette is a bet on everyone's
    /// taste *and* everyone's room — the same charcoal that reads well at a desk is
    /// unusable on a projector (docs §49).
    pub theme: String,
    /// A model per specialist, by name — `{"academic_researcher": "openai::gpt-4.1"}`.
    ///
    /// Absent means "use the coordinator's". The backend has accepted this since before the
    /// desktop app existed: `configurable.model_config.subagents` is read at
    /// `backend/models.py:114` and merged into the provider set the request needs keys for. Only
    /// the client had never sent it (docs §104).
    ///
    /// **Why anyone wants it.** The specialists do genuinely different work. Literature search
    /// wants a long context window and cheap tokens across many calls; a report wants the best
    /// prose available; data cleaning wants neither and is run dozens of times. One model for all
    /// ten is either an expensive way to grep or a cheap way to write a paper.
    ///
    /// A `BTreeMap` so the file has a stable order — a settings file that reshuffles itself on
    /// every save is one nobody can diff.
    #[serde(default)]
    pub subagents: std::collections::BTreeMap<String, String>,
    /// The project new conversations start in. Empty means none.
    ///
    /// Remembered rather than asked, because a researcher works through one line of enquiry over
    /// days: choosing once and continuing is the shape of the work, and a dialog before every
    /// question is not (docs §106).
    #[serde(default)]
    pub project: String,
    /// Whether the app created that directory.
    ///
    /// **Load-bearing.** Updating means `git fetch && git checkout <pin> && uv sync`, and
    /// running that on a checkout the user cloned themselves can destroy work — the
    /// reference checkout on this developer's own machine has ten local branches, several
    /// live in worktrees. The app may only update what it made.
    pub backend_dir_owned: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model_id: PROVIDERS[0].suggested_model.to_string(),
            base_url: String::new(),
            local_execution: true,
            approve_execute: true,
            backend_port: 2024,
            backend_dir: String::new(),
            async_subagents: false,
            theme: crate::theme::DEFAULT_NAME.to_string(),
            subagents: std::collections::BTreeMap::new(),
            project: String::new(),
            backend_dir_owned: true,
        }
    }
}

impl Settings {
    /// The `"provider::model_id"` form the backend's `model_config.default` expects.
    pub fn model_spec(&self) -> String {
        format!("{}::{}", self.provider, self.model_id)
    }

    /// Which keychain entry holds this provider's key.
    pub fn key_name(&self) -> String {
        format!("llm:{}", self.provider)
    }

    /// What is missing before a turn can succeed. Empty means ready.
    ///
    /// Checked up front so a misconfiguration shows up in the panel next to the field,
    /// rather than as a model error on the user's first real question.
    pub fn problems(&self, has_key: bool) -> Vec<String> {
        let mut problems = Vec::new();
        let Some(spec) = provider(&self.provider) else {
            problems.push(format!("Unknown provider {:?}.", self.provider));
            return problems;
        };
        if self.model_id.trim().is_empty() {
            problems.push("No model id.".to_string());
        }
        if spec.needs_base_url && self.base_url.trim().is_empty() {
            problems.push("A custom provider needs its base URL.".to_string());
        }
        if !has_key {
            problems.push(format!("No API key stored for {}.", spec.label));
        }
        problems
    }

    pub fn load() -> Self {
        let path = settings_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str(&text) {
            Ok(settings) => settings,
            // A corrupt file must not stop the app from opening — the panel is how the
            // user would fix it.
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not parse settings; using defaults");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("could not serialise settings")?;
        std::fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))
    }
}

/// Where a researcher's own palettes live: one JSON file per theme, named by its file
/// stem, using the field names of `theme::Theme`.
///
/// This is the flexibility Zed gets from theme extensions, minus the registry — dropping a
/// file in a folder is the whole install step.
pub fn themes_dir() -> PathBuf {
    settings_path()
        .parent()
        .map(|dir| dir.join("themes"))
        .unwrap_or_else(|| PathBuf::from("themes"))
}

/// Every palette the researcher can choose: the built-ins, then any of their own.
///
/// A file whose name matches a built-in replaces it, which is how someone tweaks the
/// default rather than being stuck with it.
pub fn available_themes() -> Vec<(String, crate::theme::Theme)> {
    let mut themes: Vec<(String, crate::theme::Theme)> = crate::theme::THEMES
        .iter()
        .map(|(name, theme)| (name.to_string(), *theme))
        .collect();
    let Ok(entries) = std::fs::read_dir(themes_dir()) else {
        return themes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Two formats. Ours is one theme per file, named by the file. Zed's is a whole
        // *family* in one file, each theme named inside it — which is what a researcher
        // downloads from zed.dev/extensions, so it has to work without editing.
        let parsed: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            // A broken palette must never stop the app opening — the researcher would
            // have no way back in to fix it.
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read a theme");
                continue;
            }
        };

        let found: Vec<(String, crate::theme::Theme)> = if parsed.get("themes").is_some() {
            crate::theme::from_zed_family(&parsed)
        } else {
            match serde_json::from_value::<crate::theme::Theme>(parsed) {
                Ok(theme) => vec![(name.to_string(), theme)],
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "could not read a theme");
                    continue;
                }
            }
        };

        for (found_name, theme) in found {
            match themes
                .iter_mut()
                .find(|(existing, _)| existing.eq_ignore_ascii_case(&found_name))
            {
                // A file naming a built-in replaces it: how someone tweaks the default
                // rather than ending up with two entries a letter apart.
                Some(slot) => slot.1 = theme,
                None => themes.push((found_name, theme)),
            }
        }
    }
    themes
}

/// Apply the configured palette. Falls back to the default rather than failing.
pub fn apply_theme(settings: &Settings) {
    let chosen = available_themes()
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&settings.theme))
        .map(|(_, theme)| theme)
        .unwrap_or(crate::theme::DEFAULT);
    crate::theme::apply(&chosen);
}

/// Where `settings.toml` lives. `MINIME_SETTINGS` overrides it, which is also how the
/// tests avoid touching a real user's file.
pub fn settings_path() -> PathBuf {
    if let Some(path) = std::env::var_os("MINIME_SETTINGS") {
        return PathBuf::from(path);
    }
    config_dir().join("settings.toml")
}

/// Where the app keeps things it owns and provisions — the backend checkout, not
/// configuration. Separate from [`settings_path`] because one is a file the user may
/// reasonably open and edit, and the other is several gigabytes of Python.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MINIME_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        // LOCALAPPDATA, not APPDATA: this is machine-local and must never follow a
        // roaming profile onto another computer, where a venv full of compiled wheels
        // would be worse than useless.
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("mini-me-desktop");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("mini-me-desktop");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/mini-me-desktop")
}

fn config_dir() -> PathBuf {
    // Deliberately not a `dirs`-style crate: this is one branch, and a dependency that
    // exists to compute two paths is a dependency to keep updated.
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("mini-me-desktop");
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("mini-me-desktop");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("mini-me-desktop")
}

/// Read a secret from the OS keychain.
///
/// `None` covers both "not stored" and "no keychain available" — the distinction only
/// matters for the message we show, and [`keychain_status`] reports that separately.
/// A headless Linux box often has no Secret Service at all, which must not be fatal.
pub fn secret(name: &str) -> Option<String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, name).ok()?;
    entry.get_password().ok()
}

pub fn set_secret(name: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, name)
        .with_context(|| format!("could not open the keychain entry for {name}"))?;
    if value.trim().is_empty() {
        // Clearing a field means "forget this key", not "store an empty one".
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error).context("could not remove the stored key"),
        };
    }
    entry
        .set_password(value)
        .with_context(|| format!("could not store {name} in the keychain"))
}

/// Whether a keychain is usable at all, for the panel to say so plainly.
pub fn keychain_status() -> Result<()> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, "probe")
        .context("no keychain is available on this system")?;
    match entry.get_password() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("the keychain is present but unreadable"),
    }
}

/// The two Asta credentials, which — unlike the model key — genuinely have to reach the
/// backend as environment variables: the `asta` CLI reads them from its environment when
/// `execute` runs a command, so there is no in-request path for them (docs §20).
///
/// **The model key is deliberately not on this list, and background work is not an
/// exception.** It briefly was, to feed runs the async-subagent middleware starts itself
/// with no config; the overlay now forwards the conversation's own config onto those runs
/// instead (docs §38). That is both more correct — a `custom` endpoint needs its
/// `base_url`, which no environment variable carries — and safer, since the environment
/// here is one the agent's own `execute` tool can read.
pub const ASTA_SECRETS: [&str; 2] = ["ASTA_TOKEN", "ASTA_API_KEY"];

pub fn asta_env() -> Vec<(String, String)> {
    ASTA_SECRETS
        .iter()
        .filter_map(|name| secret(name).map(|value| (name.to_string(), value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_researchers_own_palette_can_replace_a_built_in() {
        let _env = crate::backend::env_lock::hold();
        let dir = std::env::temp_dir().join(format!("minime-theme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("themes")).expect("a temp dir");
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe { std::env::set_var("MINIME_SETTINGS", dir.join("settings.toml")) };

        // The built-ins are always offered, even with no themes directory of one's own.
        assert!(available_themes().len() >= crate::theme::THEMES.len());

        // A file named after a built-in *replaces* it — how someone tweaks the default
        // rather than being stuck beside it.
        let mut mine = crate::theme::MINI_ME_DARK;
        mine.accent = 0x00ff00;
        std::fs::write(
            dir.join("themes").join("Mini-Me Dark.json"),
            serde_json::to_string(&mine).expect("serialise"),
        )
        .expect("write");
        let themes = available_themes();
        assert_eq!(
            themes.len(),
            crate::theme::THEMES.len(),
            "replaced, not appended"
        );
        let replaced = themes
            .iter()
            .find(|(name, _)| name == "Mini-Me Dark")
            .expect("the built-in name");
        assert_eq!(replaced.1.accent, 0x00ff00);

        // A broken palette must never stop the app opening: the researcher would have no
        // way back in to fix it.
        std::fs::write(dir.join("themes").join("Broken.json"), "{ not json").expect("write");
        assert!(available_themes().iter().all(|(name, _)| name != "Broken"));

        // An unknown name falls back rather than failing.
        let settings = Settings {
            theme: "Does Not Exist".into(),
            ..Default::default()
        };
        apply_theme(&settings);
        // Named as `DEFAULT`, not as whichever palette that currently is: this test is about
        // *falling back*, and it should not have to be edited every time the default moves.
        assert_eq!(crate::theme::current(), crate::theme::DEFAULT);

        unsafe { std::env::remove_var("MINIME_SETTINGS") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trips_through_toml() {
        let settings = Settings {
            provider: "custom".into(),
            model_id: "openai/gpt-4o-mini".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            local_execution: true,
            approve_execute: true,
            backend_port: 2100,
            backend_dir: "~/Mini-Me".into(),
            async_subagents: true,
            theme: "Slate".into(),
            project: "Late blight".into(),
            subagents: [("report_writer".to_string(), "openai::gpt-5.4".to_string())]
                .into_iter()
                .collect(),
            backend_dir_owned: false,
        };
        let text = toml::to_string_pretty(&settings).expect("serialise");
        assert_eq!(
            toml::from_str::<Settings>(&text).expect("parse"),
            settings,
            "{text}"
        );
    }

    #[test]
    fn a_partial_file_still_loads() {
        // Every field defaults, so a settings file written by an older build — or edited
        // by hand and left incomplete — must not brick the app.
        let settings: Settings = toml::from_str("provider = \"openai\"").expect("parse");
        assert_eq!(settings.provider, "openai");
        assert_eq!(settings.backend_port, Settings::default().backend_port);
        assert!(settings.approve_execute, "the gate must not default off");
    }

    #[test]
    fn builds_the_spec_the_backend_expects() {
        let settings = Settings {
            provider: "anthropic".into(),
            model_id: "claude-sonnet-4-5".into(),
            ..Default::default()
        };
        assert_eq!(settings.model_spec(), "anthropic::claude-sonnet-4-5");
        assert_eq!(settings.key_name(), "llm:anthropic");
    }

    #[test]
    fn reports_what_is_missing_before_a_turn_fails() {
        let ready = Settings::default();
        assert!(ready.problems(true).is_empty());
        assert_eq!(ready.problems(false).len(), 1, "the missing key");

        // A custom endpoint without its URL is the classic misconfiguration.
        let custom = Settings {
            provider: "custom".into(),
            model_id: "x/y".into(),
            base_url: String::new(),
            ..Default::default()
        };
        let problems = custom.problems(true);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("base URL"), "{problems:?}");

        let nonsense = Settings {
            provider: "not-a-provider".into(),
            ..Default::default()
        };
        assert!(!nonsense.problems(true).is_empty());
    }

    #[test]
    fn every_provider_suggests_a_model_and_only_custom_needs_a_url() {
        for provider in &PROVIDERS {
            assert!(!provider.suggested_model.is_empty(), "{}", provider.id);
            assert!(!provider.label.is_empty(), "{}", provider.id);
            assert_eq!(
                provider.needs_base_url,
                provider.id == "custom",
                "{}",
                provider.id
            );
        }
        assert!(provider("anthropic").is_some());
        assert!(provider("nope").is_none());
    }
}
