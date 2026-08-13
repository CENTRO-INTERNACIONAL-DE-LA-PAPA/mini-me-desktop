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

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
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
    /// Whether §90's one-time adoption of pre-tag conversations has already run.
    ///
    /// **The migration had no "done" marker, only a symptom.** It ran whenever the tagged search
    /// came back empty, and its doc called that self-cancelling — but "no conversations" is true
    /// in two situations it cannot tell apart: a fresh pull where the tag is new and old history
    /// is hidden, and *the researcher having just deleted everything*. In the second it re-tagged
    /// every remaining thread with human messages, including the background workers' own, so
    /// deleted test conversations came back on the next refresh (docs §166).
    ///
    /// Written once the scan completes, whatever it adopted — including zero, which is the
    /// ordinary case on an installation that never had untagged threads.
    #[serde(default)]
    pub adopted_untagged: bool,
    /// Whether the conversation list on the left is showing.
    ///
    /// Someone who closed the conversation list to get the screen back did not mean "until I next
    /// launch". These were `true` on every start until persistence was added, which made folding
    /// a panel a thing the researcher had to do again every morning.
    ///
    /// Safe to persist closed because all three toggles live in the status bar and are always
    /// present — a folded panel is never a one-way door.
    #[serde(default = "yes")]
    pub sidebar_open: bool,
    /// Whether the research panel on the right is showing.
    #[serde(default = "yes")]
    pub panel_open: bool,
    /// Whether the road strip down the left of the chat is showing.
    #[serde(default = "yes")]
    pub road_open: bool,
    /// Whether the app created that directory.
    ///
    /// **Load-bearing.** Updating means `rm -rf backend skills` and copying this repo's
    /// `mini-me/` over them (`backend::sync_source_command`), and running that on a checkout
    /// the user cloned themselves would destroy work — the reference checkout on this
    /// developer's own machine has ten local branches, several live in worktrees. The app may
    /// only overwrite what it made.
    ///
    /// The mechanism changed with §139 — it used to be `git fetch && git checkout <pin>` — and
    /// the reason did not, so this comment is the one place the old sentence survived.
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
            adopted_untagged: false,
            sidebar_open: true,
            panel_open: true,
            road_open: true,
            backend_dir_owned: true,
        }
    }
}

/// `true`, as a path serde can name.
///
/// A bare `#[serde(default)]` on a `bool` field is `false`, which for these three would mean an
/// older `settings.toml` opening with every panel folded shut.
fn yes() -> bool {
    true
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

    /// Why a turn would not reach the provider that was chosen — as opposed to failing at it.
    ///
    /// **The distinction is which failures are silent.** A wrong model id fails loudly, from the
    /// provider you picked, in a sentence naming the model; there is nothing to protect anybody
    /// from. These two do not fail at all: with no key, `run_request_body` omits `__llm_keys`
    /// entirely — and `base_url` lives *inside* that block — so the backend builds a bare
    /// OpenAI client, picks up whatever `OPENAI_API_KEY` the distro holds, and bills an account
    /// nobody selected. The researcher's first news of it was an out-of-credits page for a
    /// service they were not using (docs §186).
    ///
    /// Typed here rather than matched on prose at the call site, so rewording a message cannot
    /// quietly stop a turn from being blocked.
    pub fn misdirects_a_turn(&self, has_key: bool) -> Option<String> {
        let spec = provider(&self.provider)?;
        if !has_key {
            return Some(format!(
                "No API key stored for {} — a turn would run on whichever provider the backend \
                 falls back to",
                spec.label
            ));
        }
        if spec.needs_base_url && self.base_url.trim().is_empty() {
            return Some(format!(
                "{} needs its base URL — without one the request has no address and falls back \
                 to OpenAI's",
                spec.label
            ));
        }
        None
    }

    pub fn load() -> Self {
        let path = settings_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<Self>(&text) {
            Ok(mut settings) => {
                settings.theme = crate::theme::canonical_name(&settings.theme).to_string();
                settings
            }
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

/// One palette in the picker, including the file that can remove it when it did not ship here.
///
/// The source is attached to the parsed palette rather than inferred later from its name. A Zed
/// family file may contain several differently named palettes, and a file may replace a built-in;
/// names alone cannot answer which one piece of user-owned data must be removed (docs §181).
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeEntry {
    pub name: String,
    pub palette: crate::theme::Theme,
    /// `None` is a built-in and cannot be uninstalled. `Some` is the exact JSON file read.
    pub source: Option<PathBuf>,
}

/// Every palette the researcher can choose: the built-ins, then any of their own.
///
/// A file whose name matches a built-in replaces it, which is how someone tweaks the
/// default rather than being stuck with it. The replacing entry keeps the file as its source, so
/// removing it reveals the built-in again instead of making an overridden theme permanent.
pub fn available_theme_entries() -> Vec<ThemeEntry> {
    let mut themes: Vec<ThemeEntry> = crate::theme::THEMES
        .iter()
        .map(|(name, theme)| ThemeEntry {
            name: name.to_string(),
            palette: *theme,
            source: None,
        })
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
                .find(|existing| existing.name.eq_ignore_ascii_case(&found_name))
            {
                // A file naming a built-in replaces it: how someone tweaks the default
                // rather than ending up with two entries a letter apart.
                Some(slot) => {
                    slot.palette = theme;
                    slot.source = Some(path.clone());
                }
                None => themes.push(ThemeEntry {
                    name: found_name,
                    palette: theme,
                    source: Some(path.clone()),
                }),
            }
        }
    }
    themes
}

/// The old tuple shape for callers that only need to apply or compare palettes.
pub fn available_themes() -> Vec<(String, crate::theme::Theme)> {
    available_theme_entries()
        .into_iter()
        .map(|entry| (entry.name, entry.palette))
        .collect()
}

/// Remove one theme file the picker actually discovered.
///
/// A UI callback handing a filesystem path to deletion is still a deletion boundary. Restricting
/// it to one immediate `.json` child of `themes/` means a stale or malformed event cannot turn
/// "remove this palette" into removal of an arbitrary settings or research file. One Zed file can
/// contain a family, so removing it intentionally removes every palette sourced from that file.
pub fn uninstall_theme_file(path: &Path) -> Result<()> {
    let dir = themes_dir();
    if path.parent() != Some(dir.as_path())
        || path.extension().and_then(|extension| extension.to_str()) != Some("json")
    {
        bail!("only an installed theme file can be removed");
    }
    std::fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
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
    fn uninstalling_one_zed_file_removes_its_whole_family_and_nothing_beside_it() {
        let _env = crate::backend::env_lock::hold();
        let dir = std::env::temp_dir().join(format!(
            "minime-theme-uninstall-test-{}",
            std::process::id()
        ));
        let themes = dir.join("themes");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&themes).expect("a themes directory");
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe { std::env::set_var("MINIME_SETTINGS", dir.join("settings.toml")) };

        let family = themes.join("andean.json");
        std::fs::write(
            &family,
            serde_json::json!({
                "themes": [
                    {"name": "Oca Purple", "appearance": "dark", "style": {"accent": "#d991c8ff"}},
                    {"name": "Mashua Gold", "appearance": "light", "style": {"accent": "#8a5d04ff"}}
                ]
            })
            .to_string(),
        )
        .expect("write a Zed family");

        let installed: Vec<_> = available_theme_entries()
            .into_iter()
            .filter(|entry| entry.source.as_deref() == Some(family.as_path()))
            .map(|entry| entry.name)
            .collect();
        assert_eq!(installed, ["Oca Purple", "Mashua Gold"]);

        let beside = dir.join("keep.json");
        std::fs::write(&beside, "research, not a theme").expect("write the neighbour");
        assert!(
            uninstall_theme_file(&beside).is_err(),
            "a path outside themes/ must never reach remove_file"
        );
        assert!(beside.exists(), "the rejected neighbour was removed");

        uninstall_theme_file(&family).expect("uninstall the family");
        assert!(!family.exists());
        let names: Vec<_> = available_themes()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(!names.iter().any(|name| name == "Oca Purple"));
        assert!(!names.iter().any(|name| name == "Mashua Gold"));

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
            adopted_untagged: false,
            subagents: [("report_writer".to_string(), "openai::gpt-5.4".to_string())]
                .into_iter()
                .collect(),
            // All three deliberately *not* the default, so a round trip that silently reset
            // them to `true` would be caught here rather than by someone whose folded panels
            // kept reappearing.
            sidebar_open: false,
            panel_open: false,
            road_open: false,
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
        // A `bool` with a bare `#[serde(default)]` is `false`. These three carry `default = "yes"`
        // precisely so an existing settings.toml — every one written before this build — does not
        // open with all three panels shut.
        assert!(settings.sidebar_open && settings.panel_open && settings.road_open);
    }

    #[test]
    fn the_pre_tag_scan_is_remembered_so_it_cannot_resurrect_deleted_work() {
        // §90's migration guarded itself on "there are no tagged conversations", which is true
        // both on the launch that needs it and on the launch after a researcher deletes their
        // last conversation. In the second case it re-tagged whatever threads were left — the
        // background workers' among them — and the deleted rows came back (§166).
        let fresh = Settings::default();
        assert!(
            !fresh.adopted_untagged,
            "a new installation still owes the one-time scan"
        );

        // An older settings file predates the field, and must still parse — it is exactly the
        // installation whose history the migration was written to rescue.
        let older: Settings = toml::from_str(
            "provider = \"anthropic\"\nmodel_id = \"claude-sonnet-4-5\"",
        )
        .expect("a settings file from before this field");
        assert!(!older.adopted_untagged);

        // And once recorded it survives the round trip, which is the whole point: the fact has
        // to outlive the process that learned it.
        let done = Settings {
            adopted_untagged: true,
            ..older
        };
        let text = toml::to_string_pretty(&done).expect("serialise");
        let read_back: Settings = toml::from_str(&text).expect("parse");
        assert!(read_back.adopted_untagged);
    }

    #[test]
    fn a_project_remembered_by_an_older_build_cannot_file_new_work() {
        // §106 defines projects from conversation metadata. The removed `project` setting was a
        // second registry: after the last conversation was deleted, this stale value restored its
        // project on launch and put the next conversation inside it (§154). Serde deliberately
        // accepts the old key for upgrade compatibility, but a save no longer writes it back.
        let settings: Settings = toml::from_str(
            "provider = \"anthropic\"\nmodel_id = \"claude-sonnet-4-5\"\nproject = \"Deleted work\"",
        )
        .expect("an older settings file");
        let rewritten = toml::to_string_pretty(&settings).expect("serialise current settings");
        assert!(!rewritten.contains("project ="), "{rewritten}");
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
    fn a_turn_is_blocked_only_by_the_failures_that_would_be_silent() {
        // §186: with no key, `run_request_body` omits `__llm_keys` — and `base_url` is inside
        // that block — so the backend builds a bare OpenAI client, uses whatever
        // `OPENAI_API_KEY` the distro holds, and bills an account nobody chose. That is what has
        // to be refused; it cannot be left as a warning, because there is nothing to warn *in*.
        let ready = Settings::default();
        assert_eq!(ready.misdirects_a_turn(true), None, "nothing wrong here");

        let no_key = ready.misdirects_a_turn(false).expect("blocked");
        assert!(no_key.contains("No API key stored"), "{no_key}");
        // The message has to say what would otherwise happen, or "no key" reads as a formality.
        assert!(no_key.contains("falls back"), "{no_key}");

        // OpenRouter is reached through `custom`, and its endpoint is what makes it OpenRouter
        // rather than OpenAI. A key without a URL is the shape that lost an afternoon.
        let openrouter = Settings {
            provider: "custom".into(),
            model_id: "openai/gpt-4o-mini".into(),
            base_url: String::new(),
            ..Default::default()
        };
        let missing_url = openrouter.misdirects_a_turn(true).expect("blocked");
        assert!(missing_url.contains("base URL"), "{missing_url}");
        let with_url = Settings {
            base_url: "https://openrouter.ai/api/v1".into(),
            ..openrouter.clone()
        };
        assert_eq!(with_url.misdirects_a_turn(true), None);

        // A missing key outranks a missing URL: it is the one that decides whether *any*
        // endpoint is sent, and reporting the second first would have somebody fill in a URL
        // that still goes nowhere.
        let neither = openrouter.misdirects_a_turn(false).expect("blocked");
        assert!(neither.contains("No API key stored"), "{neither}");

        // **A wrong model id is not blocked**, deliberately. It fails loudly, at the provider
        // that was chosen, in a sentence naming the model — there is nothing silent to protect
        // anybody from, and refusing here would stop somebody trying a model released this week.
        let unlisted = Settings {
            model_id: "claude-opus-9".into(),
            ..Default::default()
        };
        assert_eq!(unlisted.misdirects_a_turn(true), None);

        // An unknown provider cannot be reasoned about, so it is not this function's to refuse;
        // `problems()` already reports it and the pane shows it.
        let nonsense = Settings {
            provider: "not-a-provider".into(),
            ..Default::default()
        };
        assert_eq!(nonsense.misdirects_a_turn(false), None);
        assert!(!nonsense.problems(false).is_empty(), "still reported");
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
