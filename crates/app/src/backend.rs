//! Local sidecar supervision for the Mini-Me Python backend.
//!
//! The desktop app is a *client* of the existing Mini-Me agent stack, not a
//! reimplementation of it. `BackendSupervisor` owns the lifecycle of a locally
//! spawned backend process: it starts it on a localhost port, waits for health,
//! and tears it down on quit. Running the backend locally is what lets the app
//! inherit the local `asta` CLI's auth story (the web app has to paste a token
//! that expires; locally the CLI refreshes it only when its seven-day lifetime is ending).
//!
//! Verified against the Mini-Me repo (2026-07-30): the backend is a LangGraph
//! server started with `uv run langgraph dev`, defaulting to `127.0.0.1:2024`,
//! which auto-loads `.env` from the repo root and does not open a browser.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;

use crate::protocol::LangGraphClient;

/// Where the sidecar's own stdout/stderr is tee'd. A GUI has no useful terminal,
/// and piping to us would let the child hold our stdout open (and deadlock once
/// the pipe buffer fills), so the logs go to a file we can point the user at.
fn default_log_path() -> PathBuf {
    std::env::temp_dir().join("mini-me-desktop-backend.log")
}

/// Where the agent's files and shell commands run.
///
/// Upstream Mini-Me executes inside a remote LangSmith sandbox. For a local-first
/// desktop app that is infrastructure we neither need nor want (docs §10/§11), and
/// `Local` replaces it with a directory on this machine via the Python overlay in
/// `overlay/` — **without modifying the Mini-Me checkout** (docs §18).
#[derive(Clone, Debug, PartialEq)]
pub enum Execution {
    /// Upstream's remote LangSmith sandbox. Still the default.
    Sandbox,
    /// The host. `overlay_dir` goes on `PYTHONPATH`, where its `sitecustomize`
    /// swaps the sandbox class at interpreter startup.
    Local { overlay_dir: PathBuf },
}

/// Decide the execution locality.
///
/// **Host execution is the default** (2026-07-31). This is a local-first, single-user
/// workbench: the researcher's files are on this machine, and shipping them to a rented
/// VM to be read was always the wrong shape (docs §10/§11). What made defaulting safe
/// is the approval gate — every `execute` call now stops and asks (docs §19).
///
/// `MINIME_EXECUTION_BACKEND=sandbox`, or `--sandbox`, still gets the old path, so the
/// change is reversible for anyone who needs it.
///
/// `override_local` comes from `--local` / `--sandbox` and wins over the environment.
/// A flag you just typed is more obviously in force than a variable your shell has
/// been holding since an hour ago — and on Windows, `$env:` assignments outlive the
/// command, which has already caused one confusing session.
fn resolve_execution(override_local: Option<bool>) -> Execution {
    let local = match override_local {
        Some(local) => local,
        None => {
            let requested = std::env::var("MINIME_EXECUTION_BACKEND").unwrap_or_default();
            let requested = requested.trim();
            // Anything explicit is honoured; an unset variable now means local.
            !requested.eq_ignore_ascii_case("sandbox") || requested.is_empty()
        }
    };
    if !local {
        return Execution::Sandbox;
    }
    Execution::Local {
        overlay_dir: overlay_dir(),
    }
}

/// Where the Python overlay lives.
///
/// Defaults to this repo's `overlay/`, resolved at compile time. That is sound here
/// precisely *because* of how this app ships: the user builds it themselves from a
/// checkout (`git pull` + `cargo build` is also the update story, docs §5), so the
/// compiled-in path is a real path on their machine. `MINIME_OVERLAY_DIR` overrides
/// it for a packaged layout.
fn overlay_dir() -> PathBuf {
    resource("MINIME_OVERLAY_DIR", "overlay")
}

/// Find a directory that ships with the app.
///
/// Three places, in order:
///
/// 1. **An environment override**, for anything unusual.
/// 2. **Next to the executable** — how a *packaged* build is laid out
///    (`mini-me-desktop.exe` beside `overlay/`, `scripts/`, `vendor/`). Checked before
///    the compiled-in path so a shipped copy never reaches back to a source tree that
///    exists only on the machine it was built on.
/// 3. **The repo**, resolved at compile time, which is the development case and was the
///    only case until packaging existed.
///
/// Falls back to (3) unconditionally when nothing is found, so the error a user sees names
/// a real path rather than an empty one.
fn resource(env_var: &str, name: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os(env_var) {
        return PathBuf::from(dir);
    }
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|dir| dir.is_dir())
    {
        return beside;
    }
    // `CARGO_MANIFEST_DIR` is `crates/app`; these sit at the repo root.
    normalized(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(name),
    )
}

/// Resolve `..` segments lexically, so a path built by joining reads like a path.
///
/// `Path::components()` drops `.` but keeps `..`, which is why the log line and the
/// Setup pane were showing `…/crates/app/../../overlay`. Lexical, not `canonicalize`:
/// that hits the filesystem and fails outright on a path that does not exist yet, which
/// is precisely the case the Setup pane has to be able to *report on*.
fn normalized(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir if out.parent().is_some() => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Where this repo's helper scripts live, resolved the same way as [`overlay_dir`].
///
/// `setup-backend.sh` is the provisioning script the Setup pane offers to run, and it has
/// to be named as a path this machine's own shell can reach, which is what
/// [`BackendConfig::setup_script`] does with this.
fn scripts_dir() -> PathBuf {
    resource("MINIME_SCRIPTS_DIR", "scripts")
}

/// The Mini-Me source this app runs.
///
/// **`mini-me/` in this repository, tracked.** *"from now I want a mono repo in mini me desktop.
/// I dont want to depende on a secod repo anymmore."*
///
/// It is also what makes updates work at all. The backend used to be fetched from its own
/// repository, which is private: a fresh install has no credentials for it, so `git fetch`
/// either hung waiting for a sign-in dialog (§131) or failed fast and left the checkout on
/// last month's commit while every log line looked healthy (§134). Shipping the source here
/// replaces a network call needing credentials with a file copy needing nothing — `git pull`
/// on this repo *is* the backend update.
///
/// `vendor/Mini-Me` is still honoured behind it, for a packaged build laid out by
/// `scripts/bundle-backend.sh`, and `MINIME_BUNDLED_BACKEND` overrides both.
pub(crate) fn bundled_backend_dir() -> Option<PathBuf> {
    // The variable names the checkout itself, not the directory holding it — someone overriding
    // it is pointing at a specific copy.
    if let Some(dir) = std::env::var_os("MINIME_BUNDLED_BACKEND") {
        let dir = PathBuf::from(dir);
        return dir.join("langgraph.json").is_file().then_some(dir);
    }
    [
        resource("MINIME_SOURCE_DIR", "mini-me"),
        resource("MINIME_VENDOR_DIR", "vendor").join("Mini-Me"),
    ]
    .into_iter()
    .find(|dir| dir.join("langgraph.json").is_file())
}

/// The checkout's own Python interpreter, if its venv has been provisioned.
///
/// Mirrors the venv-entry-point lookup [`launch_command_for`] uses to find `langgraph`:
/// `.venv/Scripts/python.exe` on Windows, `.venv/bin/python` elsewhere. Used to run `asta`
/// as a module of the backend's own interpreter (`python -m asta.cli ...`) now that Asta is
/// a normal dependency of that venv rather than a separately installed, PATH-reached CLI —
/// no shell, no WSL, just the interpreter that already has it importable.
///
/// `None` when the venv does not exist yet, which callers treat the same way a missing
/// checkout is treated elsewhere in this file: quietly skip, and let the Setup pane say so.
pub(crate) fn venv_python(project_dir: &Path) -> Option<PathBuf> {
    let python = if cfg!(windows) {
        project_dir.join(".venv/Scripts/python.exe")
    } else {
        project_dir.join(".venv/bin/python")
    };
    python.is_file().then_some(python)
}

/// Write `GENERATED_CONFIG` beside upstream's `langgraph.json`, for `--config` to find.
///
/// **Missing since WSL2 was removed.** The WSL launch used to chain this generator into
/// the same shell command as `langgraph dev` (`&&`, so a generator failure stopped the
/// launch rather than starting a server whose coordinator points at a graph nobody
/// serves). Host execution's branch of `launch_command_for` already added `--config
/// GENERATED_CONFIG` to the argv on its own — nothing ever ran the generator first — so
/// enabling "async subagents" failed outright: `Error: Invalid value for '--config':
/// Path '.mini-me-desktop.langgraph.json' does not exist.`
///
/// Run every launch, not only once: upstream's `langgraph.json` is what this extends, and
/// a stale copy would quietly serve yesterday's dependencies after a backend update —
/// same reasoning the WSL generator's own docs gave.
fn generate_extended_config(project_dir: &Path) -> Result<()> {
    let python = venv_python(project_dir).with_context(|| {
        format!(
            "no Python venv at {} — run backend setup first",
            project_dir.display()
        )
    })?;
    let script = overlay_dir().join("minime_local/make_config.py");
    let out = Command::new(&python)
        .env("PYTHONIOENCODING", "utf-8")
        .arg(&script)
        .arg(project_dir)
        .output()
        .with_context(|| format!("could not run {}", script.display()))?;
    anyhow::ensure!(
        out.status.success(),
        "make_config.py failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// How the client reaches the backend. Defaults to a locally spawned sidecar.
#[derive(Clone, Debug)]
pub struct BackendConfig {
    /// Port the local sidecar listens on.
    pub port: u16,
    /// The Mini-Me checkout to launch from (its `.env` supplies the API keys).
    pub project_dir: PathBuf,
    /// Command + args that start the dev backend. Kept configurable so packaging
    /// can swap it later.
    pub launch_command: Vec<String>,
    /// When set, never spawn — just talk to a backend someone else is running.
    pub attach_only: bool,
    /// File the sidecar's stdout/stderr is written to.
    pub log_path: PathBuf,
    /// Where the agent's code runs — the remote sandbox, or this machine.
    pub execution: Execution,
    /// Credentials the `asta` CLI needs, read from the keychain **once at startup**
    /// (see `secret_env`). Never logged.
    pub secrets: Vec<(String, String)>,
    /// Whether the backend should stop and ask before every `execute`. Off is for
    /// automation, not a recommendation (docs §19).
    pub approve_execute: bool,
    /// Let the coordinator delegate whole pieces of work to a background Mini-Me.
    ///
    /// When on, the launch points `langgraph dev` at an extended config declaring a second
    /// graph (docs §30).
    pub async_subagents: bool,
    /// The `"provider::model_id"` the researcher chose, or `None` before settings are read.
    ///
    /// Reaches the backend as `MINIME_DEFAULT_MODEL`, so a graph built without a run config —
    /// `GET /threads/{id}/state`, which the client polls — does not fall back to the OpenAI
    /// default this installation can never satisfy. See `model_env`.
    pub default_model: Option<String>,
    /// Whether the app provisioned the checkout, and so may update it.
    ///
    /// False for anything it merely *found* or was pointed at. Updating runs
    /// `git checkout <pin>` and `uv sync`, which on someone's own working clone destroys
    /// work — so this decides whether the update button exists at all (docs §25).
    pub owned: bool,
}

impl BackendConfig {
    /// Build the configuration, letting a command-line flag override the environment.
    ///
    /// `Some(true)` forces host execution, `Some(false)` forces the sandbox, `None`
    /// falls back to `MINIME_EXECUTION_BACKEND`.
    pub fn with_execution_override(override_local: Option<bool>) -> Self {
        let settings = crate::settings::Settings::load();
        let mut config = Self::with_recorded_dir(&settings);
        // Settings lose to an explicit environment variable, which is the debugging
        // escape hatch, but win over the built-in default.
        if std::env::var_os("MINIME_BACKEND_PORT").is_none() {
            config.port = settings.backend_port;
        }
        let execution = resolve_execution(override_local.or_else(|| {
            if std::env::var_os("MINIME_EXECUTION_BACKEND").is_some() {
                None
            } else {
                Some(settings.local_execution)
            }
        }));
        // The launch command embeds both the port and the execution environment, so it is
        // rebuilt rather than patched.
        config.launch_command = launch_command_for(
            &config.project_dir,
            config.port,
            &execution,
            settings.approve_execute,
            settings.async_subagents,
            config.owned,
            Some(&settings.model_spec()),
        );
        config.default_model = Some(settings.model_spec());
        config.execution = execution;
        config.approve_execute = settings.approve_execute;
        config.async_subagents = settings.async_subagents;
        // Read here, on the main thread: see `secret_env`.
        config.secrets = crate::settings::asta_env();
        config
    }

    /// Build a configuration that honours the checkout Settings recorded.
    ///
    /// The Setup pane writes `backend_dir` when it adopts a checkout it discovered, so
    /// the discovery probe runs once rather than on every launch.
    fn with_recorded_dir(settings: &crate::settings::Settings) -> Self {
        let recorded = Some(settings.backend_dir.trim())
            .filter(|dir| !dir.is_empty())
            .map(|dir| (dir.to_string(), settings.backend_dir_owned));
        Self::build(recorded)
    }

    fn build(recorded: Option<(String, bool)>) -> Self {
        let port = std::env::var("MINIME_BACKEND_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(2024);
        let (project_dir, owned) =
            resolve_project_dir(recorded.map(|(dir, owned)| (PathBuf::from(dir), owned)));
        let execution = resolve_execution(None);
        Self {
            port,
            launch_command: launch_command_for(
                &project_dir,
                port,
                &execution,
                true,
                false,
                owned,
                None,
            ),
            project_dir,
            attach_only: std::env::var_os("MINIME_BACKEND_ATTACH_ONLY").is_some(),
            log_path: default_log_path(),
            execution,
            secrets: Vec::new(),
            approve_execute: true,
            async_subagents: false,
            default_model: None,
            owned,
            // Set by `with_recorded_dir`, which is the only path that has read settings.
            // `build` is also reached from tests and from the environment-only path, where
            // leaving the checkout alone is the right default.
        }
    }
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::build(None)
    }
}

/// Build the launch argv.
///
/// Prefer the checkout's own venv entry point over `uv run langgraph`: `uv run`
/// **forks** the real server as a grandchild, so killing our direct child leaves
/// an orphaned server holding the port (observed in P6.2). Invoking the venv
/// binary directly keeps it a single process we actually own.
///
/// `execution`, `approve_execute`, `owned` and `default_model` are not spelled into this
/// argv: on the host they ride as environment variables set directly on the child in
/// [`BackendSupervisor::start`] (see `execution_env`, `feature_env`, `model_env`), not on
/// the command line. They stay parameters here anyway, because the call sites deliberately
/// spell every launch-time choice out in one place rather than let half of them travel
/// silently through `self` (the explicit-boundary rule from docs §41 and §96).
#[allow(clippy::too_many_arguments)]
fn launch_command_for(
    project_dir: &Path,
    port: u16,
    _execution: &Execution,
    _approve_execute: bool,
    async_subagents: bool,
    _owned: bool,
    _default_model: Option<&str>,
) -> Vec<String> {
    let venv_entry = if cfg!(windows) {
        project_dir.join(".venv/Scripts/langgraph.exe")
    } else {
        project_dir.join(".venv/bin/langgraph")
    };

    let mut argv: Vec<String> = if venv_entry.is_file() {
        vec![venv_entry.to_string_lossy().into_owned(), "dev".into()]
    } else {
        // Fallback: let uv resolve the environment (it will create one if needed).
        vec!["uv".into(), "run".into(), "langgraph".into(), "dev".into()]
    };
    argv.extend([
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        // Keep it a single supervised process: the reloader forks children we
        // don't own.
        "--no-reload".into(),
        // We are the client — don't hijack the user's browser with Studio.
        "--no-browser".into(),
        // `langgraph dev` defaults to ONE concurrent job (langgraph_api/cli.py:
        // `n_jobs_per_worker if ... else 1`). With one slot, a second turn — from
        // another thread or another window — queues behind the first, and any
        // background run (async subagents, docs §14) would starve because the
        // supervisor's own run holds the only slot.
        "--n-jobs-per-worker".into(),
        JOBS_PER_WORKER.to_string(),
        // `blockbuster` (langgraph dev's own event-loop-stall detector) flags
        // `Path(root_dir).resolve()` in deepagents' filesystem backend as a blocking call
        // and aborts every run with `BlockingError: Blocking call to os.getcwd` — on
        // Windows only, because `ntpath.realpath` (unlike `posixpath.realpath`) queries
        // the current directory internally, which is exactly what blockbuster watches
        // for. This never fired under WSL2/Linux; it is not a bug in this app's own
        // code to fix, just `langgraph dev`'s own documented dev-only escape hatch for
        // a false positive in a single-user local server where an occasional blocking
        // call has no one else's request to stall.
        "--allow-blocking".into(),
    ]);
    if async_subagents {
        argv.push("--config".into());
        argv.push(GENERATED_CONFIG.into());
    }
    argv
}

/// Install the SQLite checkpointer if it is not already there.
///
/// **Only ever on a checkout the app provisioned and owns.** Installing a package into someone
/// else's virtualenv is a change to an environment they are responsible for, and the rule that
/// keeps this app welcome on a developer's own clone is that it never runs anything destructive
/// or surprising there (see `resolve_project_dir`). For those, Setup offers the same command and
/// a person decides.
///
/// **Why at launch and not only at provisioning.** Everyone already using the app provisioned
/// before this existed, and the alternative was a warning row in Setup that a researcher has to
/// notice, understand and act on. They would have to know the pickle store's failure modes to go
/// looking for the switch — which is the opposite of who this app is for (docs §96).
///
/// The graph id the background worker is served under.
///
/// Must match `BACKGROUND_GRAPH_ID` in `overlay/minime_local/async_agents.py` — the
/// coordinator's tool points at this id, and a mismatch fails mid-task rather than at
/// startup.
/// Not read at runtime — the Python side registers the graph and names the id itself.
/// It lives here as the anchor for the test that holds all three files to the same value,
/// which is the only thing standing between a rename and a failure that lands mid-task.
#[allow(dead_code)]
const BACKGROUND_GRAPH_ID: &str = "background";

/// The config file the generator writes, next to upstream's own.
///
/// **Next to it, not elsewhere.** Every path inside `langgraph.json` is relative to the
/// file itself, so a copy written somewhere else silently breaks `dependencies` and the
/// `http.app` route module that serves the spine and the job-poll routes.
const GENERATED_CONFIG: &str = ".mini-me-desktop.langgraph.json";

/// Tell the backend which model to build when a request did not choose one.
///
/// # Why this is not cosmetic
///
/// `backend/models.py` reads `MINIME_DEFAULT_MODEL` and falls back to **`openai::gpt-5.4`**. This
/// app never set it, and never puts an OpenAI key anywhere — provider keys ride in the run request
/// precisely so the agent's own `execute` tool cannot read them off the environment.
///
/// That is fine for a *run*, which carries its own model. It is fatal for every call that builds
/// the graph **without** a run config, and `GET /threads/{id}/state` is one — the route the client
/// polls while watching a background task. Constructing an OpenAI client with no key raises at
/// construction:
///
/// ```text
/// openai.OpenAIError: The api_key client option must be set ...
/// GET /threads/019fe9aa-.../state 500
/// ```
///
/// So a background run would finish, the poll would 500, and the coordinator would report
/// *"completed, but it returned no result text"* — the work done and the answer unreadable. The
/// same 500 is why older conversations reported `could not read a conversation` (docs §148).
///
/// **Only the name travels, never the key.** `anthropic:…` constructs happily with no credential
/// — measured, not assumed — so naming the configured model is enough to keep a config-less build
/// on a provider this installation can actually use. A model id is not a secret, so the rule that
/// sent this project down the request-only-keys path is untouched.
fn model_env(spec: Option<&str>) -> Vec<(String, String)> {
    let Some(spec) = spec.map(str::trim).filter(|spec| spec.contains("::")) else {
        return Vec::new();
    };
    vec![("MINIME_DEFAULT_MODEL".to_string(), spec.to_string())]
}

/// The variable that turns background work on inside the backend.
///
/// Separate from [`execution_env`] on purpose: that returns **nothing** for the remote
/// sandbox, so folding this into it would silently disable background work under
/// `--sandbox`. Kept apart, the two settings stay independent — which is what they are.
fn feature_env(async_subagents: bool) -> Vec<(String, String)> {
    if !async_subagents {
        return Vec::new();
    }
    vec![("MINIME_ASYNC_SUBAGENTS".to_string(), "1".to_string())]
}

/// The environment that switches the backend to host execution.
///
/// Empty for [`Execution::Sandbox`], so the sandbox path is byte-for-byte the launch
/// it always was.
fn execution_env(execution: &Execution, approve: bool) -> Vec<(String, String)> {
    let Execution::Local { overlay_dir } = execution else {
        return Vec::new();
    };
    let overlay = overlay_dir.to_string_lossy().into_owned();
    // Where a turn's files land. Chosen by the *app* rather than left to the backend's
    // own default of `~/.mini-me/workspaces` — see `workspace.rs` for why that one decision
    // is what makes outputs findable, downloadable and renderable at all.
    let workspace = crate::workspace::root().to_string_lossy().into_owned();

    vec![
        ("MINIME_EXECUTION_BACKEND".to_string(), "local".to_string()),
        (
            "MINIME_APPROVE_EXECUTE".to_string(),
            if approve { "1" } else { "0" }.to_string(),
        ),
        (crate::workspace::WORKSPACE_ENV.to_string(), workspace),
        // Python imports `sitecustomize` from here at startup; that is the whole
        // injection mechanism. Prepended, so an existing PYTHONPATH survives.
        ("PYTHONPATH".to_string(), overlay),
    ]
}

/// The Asta credentials, delivered as environment variables on the *process*.
///
/// These two genuinely have to be variables: the `asta` CLI reads them from its
/// environment when `execute` runs a command, so there is no in-request path for them the
/// way there is for the model key (docs §20).
///
/// **Never on the command line**, so `ps` never shows a token to anyone else on the
/// machine — fine for a flag, not for a credential.
///
/// Takes the already-read values rather than reading the keychain itself. That is not a
/// style choice: the Linux keychain client (zbus) runs its own `block_on`, and calling it
/// from a thread that is already driving a Tokio runtime panics with "Cannot start a
/// runtime from within a runtime" — which is exactly how the first live run of this code
/// died. Secrets are read once, on the main thread, before any runtime exists.
fn secret_env(secrets: &[(String, String)]) -> Vec<(String, String)> {
    secrets.to_vec()
}

/// Single-quote a value for `bash -lc`, so a path with spaces survives.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Single-quote a value for `powershell -Command`, so a path with spaces survives.
///
/// PowerShell's single-quoted strings are literal (no `$`/backtick expansion), and a
/// literal quote inside one is escaped by doubling it — not by backslash, which is what
/// [`shell_quote`] uses for `bash`.
pub(crate) fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Change into `dir` before running `cmd`, spelled for whichever shell [`BackendConfig::shell_argv`]
/// will run it through.
pub(crate) fn in_dir(dir: &str, cmd: &str) -> String {
    if cfg!(windows) {
        format!("Set-Location -LiteralPath {}; {cmd}", ps_quote(dir))
    } else {
        format!("cd {} && {cmd}", quote_path(dir))
    }
}

/// Quote a path for a shell **while leaving a leading `~` able to expand**.
///
/// A checkout path can be typed or configured as `~/Mini-Me`, and `cd '~/Mini-Me'` does not
/// work — the quotes suppress tilde expansion and bash looks for a directory literally named
/// `~`. Quoting only the part after the tilde gets both: `~/'My Docs/Mini-Me'` expands *and*
/// survives the space, which `Documents\My Repos\…` makes a real case on Windows.
pub(crate) fn quote_path(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("~/{}", shell_quote(rest)),
        None => shell_quote(path),
    }
}

/// Concurrent runs the sidecar may process. Modest on purpose: each run can drive
/// model calls and sandbox execution, so this is about not self-deadlocking, not
/// about throughput.
const JOBS_PER_WORKER: u8 = 10;

/// The checkout the app provisions and owns, on this machine.
fn owned_host_dir() -> PathBuf {
    crate::settings::data_dir().join("backend")
}

/// Where the Mini-Me Python checkout lives, and whether the app owns it.
///
/// Order: an explicit `MINIME_BACKEND_DIR`, then what Settings recorded, then the
/// conventional developer locations, then the app-owned path. Only the last is *owned* —
/// everything else is a checkout someone else is responsible for, and the app must not
/// run destructive git on it.
fn resolve_project_dir(recorded: Option<(PathBuf, bool)>) -> (PathBuf, bool) {
    if let Some(dir) = std::env::var_os("MINIME_BACKEND_DIR") {
        return (PathBuf::from(dir), false);
    }
    if let Some(recorded) = recorded {
        return recorded;
    }
    let owned = owned_host_dir();
    if owned.join("langgraph.json").is_file() {
        return (owned, true);
    }
    let mut candidates = Vec::new();
    // Windows sets USERPROFILE, not HOME — without this the candidates below are
    // skipped entirely and discovery falls through to the cwd.
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home {
        candidates.push(PathBuf::from(&home).join("Documents/Mini-Me"));
        candidates.push(PathBuf::from(&home).join("Documents/GitHub/Mini-Me"));
    }
    // A sibling of this repo, the layout a `git clone` pair produces.
    candidates.push(PathBuf::from("../Mini-Me"));
    match candidates
        .into_iter()
        .find(|p| p.join("langgraph.json").is_file())
    {
        Some(found) => (found, false),
        // Nothing anywhere: name the path we would provision *into*, so the Setup pane
        // reports "not installed here" rather than "no langgraph.json in `.`".
        None => (owned, true),
    }
}

impl BackendConfig {
    pub fn base_url(&self) -> String {
        std::env::var("MINIME_BACKEND_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{}", self.port))
    }

    /// Whether the configured directory actually looks like the Mini-Me backend.
    pub fn looks_like_backend_repo(&self) -> bool {
        self.project_dir.join("langgraph.json").is_file()
    }

    /// Human-readable description of where the sidecar will run.
    pub fn location(&self) -> String {
        self.project_dir.display().to_string()
    }

    /// Human-readable execution locality, for the log line and the status bar.
    pub fn execution_label(&self) -> &'static str {
        match self.execution {
            Execution::Sandbox => "remote sandbox",
            Execution::Local { .. } => "host (local)",
        }
    }

    /// Wrap a POSIX shell command so it runs on this machine.
    ///
    /// This is what makes the preflight checks worth trusting: every probe and every
    /// offered fix is routed through the same shell the launch command itself would use,
    /// so a green check means green *for the process that matters*.
    ///
    /// A **login** shell (`-lc`): `uv` installs itself into `~/.local/bin`, which only a
    /// login shell has on `PATH`.
    ///
    /// On Windows this runs through `powershell.exe` instead — there is no WSL or bash
    /// dependency to provision, since `powershell.exe` ships with every supported Windows.
    pub fn shell_argv(&self, script: &str) -> Vec<String> {
        if cfg!(windows) {
            vec![
                "powershell.exe".to_string(),
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                script.to_string(),
            ]
        } else {
            vec!["bash".to_string(), "-lc".to_string(), script.to_string()]
        }
    }

    /// The checkout path as this machine spells it.
    pub fn backend_dir(&self) -> String {
        self.project_dir.to_string_lossy().into_owned()
    }

    /// The overlay path as the backend's interpreter would have to import it, or `None`
    /// when execution is remote and there is no overlay in play.
    pub fn overlay_for_backend(&self) -> Option<String> {
        let Execution::Local { overlay_dir } = &self.execution else {
            return None;
        };
        Some(overlay_dir.to_string_lossy().into_owned())
    }

    /// Where the overlay might be, **in the order the launch command prefers**.
    ///
    /// Exists so the Setup pane and the launch cannot drift apart. One candidate on
    /// purpose: on a host run the repo's own copy is always reachable, so a second
    /// candidate would add a branch that can never change the outcome.
    pub fn overlay_candidates(&self) -> Vec<String> {
        self.overlay_for_backend().into_iter().collect()
    }

    /// The provisioning command: `bash …/setup-backend.sh <checkout>` on macOS/Linux, or
    /// `powershell …/setup-backend.ps1 -Dir <checkout>` on Windows — the two scripts are kept
    /// in step (see `scripts/setup-backend.ps1`'s header). Re-running it is safe: neither
    /// script ever overwrites a checkout or a `.env`.
    ///
    /// When a backend copy ships with the app, its path is passed in so the script
    /// provisions from it instead of cloning. That is the difference between an install
    /// a scientist can complete and one that stops at a GitHub token prompt, because
    /// Mini-Me is a private repository (see `scripts/bundle-backend.sh`).
    pub fn setup_script(&self) -> String {
        let spell = |path: &Path| path.to_string_lossy().into_owned();
        let mut command = String::new();
        if cfg!(windows) {
            let script = spell(&scripts_dir().join("setup-backend.ps1"));
            if let Some(bundled) = bundled_backend_dir() {
                command.push_str(&format!(
                    "$env:MINIME_BUNDLED_SOURCE = {}; ",
                    ps_quote(&spell(&bundled))
                ));
            }
            command.push_str(&format!(
                "& {} -Dir {}",
                ps_quote(&script),
                ps_quote(&self.backend_dir())
            ));
        } else {
            let script = spell(&scripts_dir().join("setup-backend.sh"));
            if let Some(bundled) = bundled_backend_dir() {
                command.push_str(&format!(
                    "MINIME_BUNDLED_SOURCE={} ",
                    shell_quote(&spell(&bundled))
                ));
            }
            command.push_str(&format!(
                "bash {} {}",
                shell_quote(&script),
                quote_path(&self.backend_dir())
            ));
        }
        command
    }

    /// Spell a path on *this* machine the way the backend would have to open it.
    ///
    /// The file is **referenced, not copied**. Keeping a scientist's data where they put
    /// it is most of the point of a desktop app; copying it into a working directory
    /// creates a second version that goes stale the moment they edit the first.
    pub fn path_for_backend(&self, path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    /// Whether the backend could open this path at all, once spelled its way.
    ///
    /// Always true now that the backend runs on this host: it reads exactly the path the
    /// researcher's own file manager gave us, network share or not.
    pub fn can_open(&self, _path: &Path) -> bool {
        true
    }

    /// A copy with the credentials stripped.
    ///
    /// Anything that only needs the *shape* of the configuration takes this, so the
    /// secrets stay in exactly one place and there is one thing to audit rather than a
    /// clone in every struct that wanted to know the port number.
    pub fn redacted(&self) -> Self {
        Self {
            secrets: Vec::new(),
            ..self.clone()
        }
    }
}

/// Owns the spawned backend process and shuts it down on drop.
pub struct BackendSupervisor {
    config: BackendConfig,
    child: Option<Child>,
    /// Windows only: the Job Object holding the sidecar's whole process tree. Dropping it
    /// is what reaps them (see [`job`]).
    #[cfg(windows)]
    job: Option<job::Job>,
}

impl BackendSupervisor {
    pub fn new(config: BackendConfig) -> Self {
        Self {
            config,
            child: None,
            #[cfg(windows)]
            job: None,
        }
    }

    /// Spawn the local backend sidecar. Idempotent: a no-op while a child runs.
    pub fn start(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Ok(());
        }
        anyhow::ensure!(
            self.config.looks_like_backend_repo(),
            "no langgraph.json under {} — set MINIME_BACKEND_DIR to the Mini-Me checkout",
            self.config.project_dir.display()
        );

        // `launch_command` already carries `--config GENERATED_CONFIG` when async subagents
        // are on (see `launch_command_for`) — that file has to exist before the process
        // below reads it, or `langgraph dev` refuses to start at all.
        if self.config.async_subagents {
            generate_extended_config(&self.config.project_dir)?;
        }

        let (program, rest) = self
            .config
            .launch_command
            .split_first()
            .context("launch_command must not be empty")?;

        tracing::info!(
            program = %program,
            location = %self.config.location(),
            port = self.config.port,
            log = %self.config.log_path.display(),
            "spawning backend sidecar"
        );

        let log = File::create(&self.config.log_path).with_context(|| {
            format!(
                "could not open the sidecar log at {}",
                self.config.log_path.display()
            )
        })?;
        let log_err = log.try_clone().context("could not dup the sidecar log")?;

        let mut command = Command::new(program);
        command
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        // Secrets always go on the process environment — see `secret_env` for why they
        // must not travel on the command line.
        let mut secrets = self.config.secrets.clone();
        // A usable stored or CLI-cached token beats a freshly minted one. Asta access tokens last
        // **seven days** (measured: `exp - iat` = 604800), while `--refresh` costs about ten
        // seconds on the startup path. `mint_asta_token` checks `exp` before paying that cost;
        // the name survives because refreshing is still its final fallback (§131/§145).
        if let Some(token) = mint_asta_token(&self.config) {
            secrets.retain(|(name, _)| name != "ASTA_TOKEN");
            secrets.push(("ASTA_TOKEN".to_string(), token));
        }
        for (name, value) in secret_env(&secrets) {
            command.env(name, value);
        }

        // Host execution on the host itself: the variables go straight onto the child.
        for (name, value) in execution_env(&self.config.execution, self.config.approve_execute)
            .into_iter()
            .chain(feature_env(self.config.async_subagents))
            .chain(model_env(self.config.default_model.as_deref()))
        {
            if name == "PYTHONPATH" {
                // Prepend rather than replace: whatever the user had still works.
                let existing = std::env::var("PYTHONPATH").unwrap_or_default();
                let combined = if existing.is_empty() {
                    value
                } else {
                    format!("{value}{}{existing}", if cfg!(windows) { ";" } else { ":" })
                };
                command.env(name, combined);
            } else {
                command.env(name, value);
            }
        }

        command.current_dir(&self.config.project_dir);

        // Put the child in its own process group so we can signal the whole tree
        // on shutdown (see `terminate`) rather than just the process we spawned.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let child = command.spawn().with_context(|| {
            // The usual cause: the checkout is synced but the LangGraph *CLI* is not
            // installed. It lives in an optional extra (`langgraph-cli[inmem]` under
            // `[project.optional-dependencies] dev`), which plain `uv sync` skips —
            // so the server libraries are present and the `langgraph` entry point is
            // simply absent. Name the fix rather than reporting "program not found".
            format!(
                "failed to spawn the backend ({program}). If the LangGraph CLI is \
                 missing, install the dev extra in {}:\n    uv sync --extra dev",
                self.config.project_dir.display()
            )
        })?;

        // Windows has no process groups to signal, so the tree is held in a Job Object
        // that kills everything when its last handle closes — including if *we* crash.
        #[cfg(windows)]
        {
            self.job = job::adopt(&child);
            if self.job.is_none() {
                tracing::warn!(
                    "could not create a Job Object; closing the window may leave the \
                     backend running and holding the port"
                );
            }
        }

        self.child = Some(child);
        Ok(())
    }

    /// Ensure *something* healthy is listening: attach if it is already up,
    /// otherwise spawn and wait. Returns a status string for the UI.
    pub async fn ensure_running(&mut self, client: &LangGraphClient) -> Result<Started> {
        if client.is_healthy().await {
            // **Ours, or somebody else's?** This is called once per turn, not once per launch, so
            // after the app spawns its own sidecar every later turn finds a healthy backend — and
            // reported it as one that "was already running". A researcher who had just killed the
            // old server, watched this app start a new one, and then read three warnings telling
            // them to kill it again has been told the fix did not work when it did (docs §202).
            //
            // `try_wait` and not just `is_some()`: a child that died leaves the handle behind, and
            // whatever answered the health check after that is not the process we started. An
            // error means we cannot tell, which is the same as not knowing it is ours.
            if matches!(
                self.child.as_mut().map(std::process::Child::try_wait),
                Some(Ok(None))
            ) {
                return Ok(Started::Spawned);
            }
            // A server left over from a previous session has already imported whatever it
            // imported, and `langgraph dev` survives the app closing — so this is the usual
            // case rather than an edge one. Said plainly, with what to do about it, because
            // the researcher just ran `git pull` and has every reason to believe the new
            // code is running (docs §130).
            tracing::warn!(
                "attached to a backend that was already running — it is on the code it started \
                 with, not what this app now ships. To pick up a backend change, close this app \
                 and stop the leftover process (e.g. `pkill -f \"langgraph dev\"` on macOS/Linux, \
                 or end the langgraph task in Task Manager on Windows), then reopen the app"
            );
            return Ok(Started::Attached);
        }
        if self.config.attach_only {
            // Name the variable: this mode is opt-in via the environment, and a
            // value left over in a shell session looks exactly like a bug ("why
            // won't it start the backend?").
            anyhow::bail!(
                "no backend at {} and attach-only mode is on, so the app will not \
                 start one. Unset MINIME_BACKEND_ATTACH_ONLY to let it spawn the \
                 sidecar (PowerShell: Remove-Item Env:MINIME_BACKEND_ATTACH_ONLY), \
                 or start the backend yourself",
                self.config.base_url()
            );
        }
        self.start()?;
        // `langgraph dev` imports the graph on boot, so first health can take a
        // while on a cold venv.
        self.wait_until_healthy(client, 120).await?;
        Ok(Started::Spawned)
    }

    /// Poll `GET /ok` until it responds or the budget runs out.
    pub async fn wait_until_healthy(
        &mut self,
        client: &LangGraphClient,
        attempts: u32,
    ) -> Result<()> {
        for attempt in 1..=attempts {
            // Fail fast if the process died rather than waiting out the budget.
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait().context("could not poll the sidecar")? {
                    anyhow::bail!("backend exited during startup with {status}");
                }
            }
            if client.is_healthy().await {
                tracing::info!("backend healthy after {attempt} attempt(s)");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        anyhow::bail!(
            "backend did not become healthy within {} attempts",
            attempts
        )
    }
}

/// Whether this app started the backend it is talking to.
///
/// A typed answer rather than a sentence, because it is load-bearing: the Python overlay lives
/// in the backend *process*, so an attached one may be running an older copy than this app
/// ships — and every symptom of that is identical to a broken feature (docs §80). Matching on
/// prose to find that out is how the two get confused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Started {
    /// Already healthy, so it was left running by an earlier session — possibly an earlier
    /// *version*.
    Attached,
    /// Spawned by this app, so it is running the overlay this app shipped.
    Spawned,
}

impl Started {
    pub fn label(self) -> &'static str {
        match self {
            Started::Attached => "attached to a backend that was already running",
            Started::Spawned => "backend started",
        }
    }
}

impl BackendSupervisor {
    /// Stop the backend this app started.
    ///
    /// Factored out of `Drop` so **restarting** is possible at all. Until now the only way to
    /// reload the Python overlay was to quit the app *and* make sure nothing had survived it:
    /// `ensure_running` attaches to a healthy backend rather than replacing it, so an app that
    /// had just been updated kept talking to a process holding the previous overlay in memory —
    /// with no symptom except a feature that did nothing (docs §79).
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            tracing::info!("terminating backend sidecar");
            terminate(&mut child);
        }
    }
}

impl Drop for BackendSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Reuse a valid Asta token, or ask the CLI for a fresh one where the backend runs.
///
/// **Why the app does this instead of the user.** Asta access tokens last seven days
/// (`exp - iat` = 604800 on a real one), so storing one in the keychain means re-pasting
/// it every week — and when it lapses the failure reads "the Asta theorizer returned no
/// task id", which names neither the token nor the fix. `asta auth login` already leaves a
/// *refresh* credential behind, and `print-token --refresh` turns that into a valid access
/// token on demand. So the researcher logs in once.
///
/// The order is load-bearing: the keychain value costs no process at all; `print-token --raw`
/// reads the CLI's cache; only an absent or nearly expired token reaches the network through
/// `--refresh`. Before §145 the last command ran unconditionally and consumed about ten of every
/// seventeen startup seconds measured in §131.
///
/// `None` on any failure — no CLI, not logged in, a changed flag. The stored token (if
/// any) still applies, and the Setup pane reports a missing `asta` separately.
fn mint_asta_token(config: &BackendConfig) -> Option<String> {
    if std::env::var_os("MINIME_NO_ASTA_MINT").is_some() {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if let Some(token) = reusable_stored_asta_token(&config.secrets, now) {
        tracing::info!("reusing the valid Asta token from the keychain");
        return Some(token.to_string());
    }

    if let Some(token) = read_asta_token(config, false) {
        if asta_token_is_valid_at(&token, now) {
            tracing::info!("reusing the valid Asta token cached by the CLI");
            return Some(token);
        }
    }

    let token = read_asta_token(config, true)?;
    if !asta_token_is_valid_at(&token, now) {
        tracing::debug!("asta returned a token without enough lifetime; using whatever is stored");
        return None;
    }
    tracing::info!("minted a fresh Asta token from the CLI");
    Some(token)
}

fn read_asta_token(config: &BackendConfig, refresh: bool) -> Option<String> {
    let python = venv_python(&config.project_dir)?;
    let mut args = vec!["-m", "asta.cli", "auth", "print-token", "--raw"];
    if refresh {
        args.push("--refresh");
    }
    let output = Command::new(python)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // A JWT and nothing else. The CLI prints a decoded header/payload without `--raw`, and
    // a friendly "please log in" on stderr — neither of which is a credential, and both of
    // which would otherwise be handed to the backend as if they were one.
    if !looks_like_a_jwt(&token) {
        tracing::debug!("asta did not return a token; using whatever is stored");
        return None;
    }
    Some(token)
}

/// Five minutes is negligible beside a seven-day token and avoids starting a long turn with a
/// credential that expires while the backend is still importing or assembling its MCP tools.
const ASTA_TOKEN_MIN_VALIDITY_SECS: u64 = 5 * 60;

fn reusable_stored_asta_token(secrets: &[(String, String)], now: u64) -> Option<&str> {
    secrets
        .iter()
        .find(|(name, token)| name == "ASTA_TOKEN" && asta_token_is_valid_at(token, now))
        .map(|(_, token)| token.as_str())
}

/// Read the unverified `exp` claim only to decide whether refreshing is worth doing.
///
/// This is not authentication: the backend and Asta still verify the signature. A forged or
/// corrupted token can at worst defer a refresh and fail exactly as it did before; it cannot gain
/// trust here. Missing, non-numeric and malformed claims all choose the safe slow path.
fn asta_token_is_valid_at(value: &str, now: u64) -> bool {
    if !looks_like_a_jwt(value) {
        return false;
    }
    let Some(payload) = value.split('.').nth(1) else {
        return false;
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload));
    let Ok(decoded) = decoded else {
        return false;
    };
    let Ok(claims) = serde_json::from_slice::<serde_json::Value>(&decoded) else {
        return false;
    };
    claims
        .get("exp")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|expires| expires > now.saturating_add(ASTA_TOKEN_MIN_VALIDITY_SECS))
}

/// Whether a string is shaped like a JWT: three dot-separated base64url segments.
///
/// Never logs or returns the value — this is only ever asked *about* a secret.
fn looks_like_a_jwt(value: &str) -> bool {
    let mut parts = value.split('.');
    let shaped = |part: Option<&str>| {
        part.is_some_and(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
    };
    shaped(parts.next()) && shaped(parts.next()) && shaped(parts.next()) && parts.next().is_none()
}

/// Stop the sidecar and everything it spawned.
///
/// On Unix the child leads its own process group, so we signal the *group*:
/// `Child::kill` only reaps the process we spawned, which left an orphaned
/// server holding the port when a wrapper had forked the real one. SIGTERM
/// first so the server can shut its workers down, then SIGKILL if it lingers.
#[cfg(unix)]
fn terminate(child: &mut Child) {
    let group = -(child.id() as i32);
    unsafe { libc::kill(group, libc::SIGTERM) };
    for _ in 0..40 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }
    tracing::warn!("sidecar ignored SIGTERM; sending SIGKILL");
    unsafe { libc::kill(group, libc::SIGKILL) };
    let _ = child.wait();
}

/// Stop the sidecar on Windows.
///
/// `Child::kill` reaps only the process we spawned. That is correct for the venv entry
/// point, but `uv run` forks the real server as a grandchild — it would survive and keep
/// holding the port, so the next launch attaches to a stale backend or fails outright.
///
/// The actual reaping is done by the **Job Object** created in
/// [`BackendSupervisor::start`], which kills its whole tree when the last handle closes.
/// This function just asks nicely first.
#[cfg(not(unix))]
fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reaping the sidecar's process tree on Windows.
///
/// Windows has no process group to signal, and killing a parent leaves its children
/// running. A **Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the OS-level
/// answer: every process in the job dies when the last handle to it closes. Crucially
/// that includes the case where the app **crashes** — the handle closes with the process,
/// so the kernel cleans up even when no destructor of ours ever runs. A `taskkill /T`
/// would only work during an orderly shutdown.
///
/// Verified by cross-checking against `x86_64-pc-windows-msvc`, which is also how the two
/// missing feature gates were found. It cannot be *run* from the Linux dev box.
#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// An owned job handle. Dropping it kills everything inside.
    pub struct Job(HANDLE);

    // A raw HANDLE is not `Send` by default, but a job handle is just a kernel object
    // reference with no thread affinity, and the supervisor holding it moves across
    // threads inside the Tokio mutex.
    unsafe impl Send for Job {}

    /// Put `child` — and anything it goes on to spawn — into a fresh job.
    ///
    /// Returns `None` rather than failing the launch: a backend that runs and might
    /// outlive us is much better than no backend at all, and the caller logs it.
    pub fn adopt(child: &Child) -> Option<Job> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 {
                CloseHandle(job);
                return None;
            }
            // There is a small window between spawn and this call in which the child could
            // fork something that escapes the job. Closing it needs CREATE_SUSPENDED,
            // which `std::process::Command` does not expose; the child here is a server
            // that spends its first moments importing Python, so the race is theoretical.
            if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Job(job))
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // This is the kill. Everything still in the job goes with it.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Serialising tests that touch the process environment.
///
/// Configuration here is resolved from environment variables, and `cargo test` runs tests
/// as **threads in one process** — so a test setting `HOME` or `MINIME_BACKEND_DIR` changes
/// what every concurrently running test sees. That produced a suite which passed with
/// `--test-threads=1` and failed at random otherwise, which is worse than a failing test:
/// it teaches people to re-run until green.
///
/// Every test that reads *or* writes one of these variables takes this lock first.
#[cfg(test)]
pub(crate) mod env_lock {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub(crate) fn hold() -> MutexGuard<'static, ()> {
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            // A test that panics while holding the lock poisons it. The data is `()`, so
            // there is nothing to be corrupted — recovering keeps one failure from
            // cascading into every other test in the suite.
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    /// The shared globals a test may change, and the lock each one requires.
    ///
    /// Two locks, the same rule, and the same failure twice: a lock only works if *everyone* takes
    /// it. §267 found one test setting `MINIME_LOCAL_WORKSPACE` without `env_lock`; §271 found
    /// seven changing the live palette without the theme lock, which had been fixed at §197 for
    /// the four tests that could reach it and left open for the seven that could not.
    ///
    /// Both surfaced as a single failure with a different name each run — the shape that costs
    /// days, because it looks like flakiness rather than a race.
    const GUARDED: [(&str, &[&str], &str); 2] = [
        (
            "env_lock::hold()",
            &["std::env::set_var", "std::env::remove_var"],
            "process-global environment",
        ),
        (
            "theme_lock::hold()",
            &["theme::apply(", "install_theme", "= apply("],
            "the live palette",
        ),
    ];

    /// Every test that changes a shared global must hold its lock.
    ///
    /// The lock only works if everyone takes it. One test set `MINIME_LOCAL_WORKSPACE` without
    /// it, and for the microseconds it held that value *every other test running concurrently*
    /// saw the workspace pointing at `/tmp/somewhere-else`. The result was a single failure with
    /// a different name each time, roughly once in fifty runs — twice observed, never reproduced
    /// on demand, and it took a release build to force the issue (§267).
    ///
    /// So this reads the sources rather than trusting a comment: the offending test carried
    /// "SAFETY: single-threaded test setup", which is simply not what `cargo test` does. Chunks
    /// are split on `#[test]`, which is not exactly a function boundary but is close enough to
    /// catch the mistake and never fires on correct code.
    #[test]
    fn a_test_that_changes_a_shared_global_holds_its_lock() {
        let sources: [(&str, &str); 8] = [
            ("workspace.rs", include_str!("workspace.rs")),
            ("backend.rs", include_str!("backend.rs")),
            ("preflight.rs", include_str!("preflight.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("sidecar.rs", include_str!("sidecar.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("ui.rs", include_str!("ui.rs")),
            ("main.rs", include_str!("main.rs")),
        ];
        let mut unguarded = Vec::new();
        for (lock, writes, what) in GUARDED {
            for (name, source) in sources {
                for chunk in source.split("#[test]").skip(1) {
                    if !writes.iter().any(|write| chunk.contains(write)) {
                        continue;
                    }
                    if chunk.contains(lock) {
                        continue;
                    }
                    let signature = chunk
                        .lines()
                        .find(|line| line.trim_start().starts_with("fn "))
                        .unwrap_or("<unnamed>")
                        .trim();
                    unguarded.push(format!("{name}: {signature} changes {what} without {lock}"));
                }
            }
        }
        assert!(
            unguarded.is_empty(),
            "these tests change a shared global without its lock, so they corrupt whatever runs \
             beside them:\n  {}",
            unguarded.join("\n  ")
        );
    }


    /// The generated config must extend upstream's, not replace it — and the generator must run
    /// **the way the launch command runs it**: as a script, with nothing arranged on `sys.path`.
    ///
    /// That last clause is the whole point. The first version of this test imported
    /// `make_config` as a module with the overlay root on `sys.path`, which passed while
    /// production was failing: the launch invokes
    /// `.venv/bin/python <overlay>/minime_local/make_config.py .`, and Python then puts the
    /// *script's* directory on the path — `minime_local/`, not the overlay above it. A
    /// `from minime_local import ...` at the top of the file therefore raised
    /// `ModuleNotFoundError`, the generator exited non-zero, and the `&&` in the launch
    /// expression stopped the backend from starting at all (docs §98).
    ///
    /// So this shells out to the file by path, exactly as `generate_config_command` does. A test
    /// that exercises a different invocation than production is not testing production.
    ///
    /// Skipped rather than failed when `python3` is absent, and it says so — a test that quietly
    /// covers nothing is one nobody notices has stopped (docs §81).
    #[test]
    fn the_generated_config_survives_being_run_as_a_script() {
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        let script = overlay.join("minime_local/make_config.py");
        if std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }

        let scratch = std::env::temp_dir().join(format!("mini-me-config-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("scratch");
        std::fs::write(
            scratch.join("langgraph.json"),
            r#"{"graphs":{"agent":"./backend/agent.py:agent"},
                "http":{"app":"./backend/routes/__init__.py:app"},
                "env":".env","dependencies":["."]}"#,
        )
        .expect("upstream config");

        // No PYTHONPATH, no cwd trickery, no `-c` wrapper: the launch sets none of those for
        // this step, and every one of them would hide the failure it shipped with.
        let out = std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
            .arg(&script)
            .arg(&scratch)
            .env_remove("PYTHONPATH")
            .output()
            .expect("running make_config");
        assert!(
            out.status.success(),
            "the generator must not fail — the launch joins it with `&&`:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let written = scratch.join(".mini-me-desktop.langgraph.json");
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&written).expect("generated config"))
                .expect("valid JSON");

        // Everything upstream carries survives. `http` is the load-bearing one: it mounts the
        // routes the project spine and the report renderer depend on, and rebuilding the file
        // by hand instead of extending it is how that gets dropped.
        assert_eq!(config["http"]["app"], "./backend/routes/__init__.py:app");
        assert_eq!(config["env"], ".env");
        assert!(config["graphs"]["agent"].is_string());
        // The background graph the async subagents need (docs §30).
        assert!(config["graphs"]["background"]
            .as_str()
            .is_some_and(|path| path.ends_with("async_agents.py:background_graph")));
        // The checkpointer key tracks whether the package is importable *in this interpreter*,
        // so assert the relationship rather than a fixed answer — the test has to pass on a
        // machine with the package and on one without.
        let available = std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
            .arg("-c")
            .arg("import langgraph.checkpoint.sqlite.aio")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        assert_eq!(
            config.get("checkpointer").is_some(),
            available,
            "the key must appear exactly when the backend could load it"
        );
        if available {
            assert!(config["checkpointer"]["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("checkpointer.py:checkpointer")));
        }

        std::fs::remove_dir_all(&scratch).ok();
    }
    use super::*;

    #[test]
    fn the_sandbox_path_sets_no_environment() {
        let _env = env_lock::hold();
        // Regression guard: no stray variables when execution is remote, so choosing
        // nothing keeps upstream's behaviour byte for byte.
        assert!(execution_env(&Execution::Sandbox, true).is_empty());

        let argv = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            &Execution::Sandbox,
            true,
            false,
            true,
            None,
        );
        // Sandbox or local, the host launch argv is the same shape: it is the child's
        // environment (set in `BackendSupervisor::start`), not the argv, that switches
        // execution mode.
        assert!(argv[0].ends_with("langgraph") || argv[0] == "uv", "{argv:?}");
    }

    #[test]
    fn a_packaged_build_finds_its_files_beside_the_executable() {
        let _env = env_lock::hold();
        // A shipped copy must never reach back into a source tree that only exists on the
        // machine it was built on. `CARGO_MANIFEST_DIR` is baked in at compile time, so
        // without this the packaged app would look for the overlay under whatever path
        // the build machine happened to use — and silently fall back to the sandbox.
        let exe = std::env::current_exe().expect("test binary path");
        let beside = exe.parent().expect("a parent").join("overlay");
        let _ = std::fs::remove_dir_all(&beside);

        std::env::remove_var("MINIME_OVERLAY_DIR");
        let from_repo = resource("MINIME_OVERLAY_DIR", "overlay");
        assert!(
            from_repo.ends_with("overlay") && !from_repo.starts_with(exe.parent().unwrap()),
            "with nothing beside the exe it falls back to the repo: {}",
            from_repo.display()
        );

        std::fs::create_dir_all(&beside).expect("packaged layout");
        assert_eq!(
            resource("MINIME_OVERLAY_DIR", "overlay"),
            beside,
            "a directory beside the executable wins"
        );

        // An explicit override still beats both.
        std::env::set_var("MINIME_OVERLAY_DIR", "/somewhere/else");
        assert_eq!(
            resource("MINIME_OVERLAY_DIR", "overlay"),
            PathBuf::from("/somewhere/else")
        );
        std::env::remove_var("MINIME_OVERLAY_DIR");
        let _ = std::fs::remove_dir_all(&beside);
    }

    #[test]
    fn a_joined_path_reads_like_a_path() {
        // The overlay is reached as `crates/app/../../overlay`, and that spelling was
        // showing up verbatim in the log line and the Setup pane.
        assert_eq!(
            normalized(PathBuf::from("/repo/crates/app/../../overlay")),
            PathBuf::from("/repo/overlay")
        );
        // A `..` that would escape the root has nowhere to go and must stay put rather
        // than silently rewriting the path to something else.
        assert_eq!(normalized(PathBuf::from("/..")), PathBuf::from("/.."));
        assert_eq!(
            normalized(PathBuf::from("relative/../overlay")),
            PathBuf::from("overlay")
        );
    }

    #[test]
    fn a_checkout_path_with_a_space_still_expands_its_tilde() {
        // `cd '~/My Repos/Mini-Me'` looks for a directory literally named `~`; quoting
        // only what follows the tilde gets expansion *and* survives the space.
        assert_eq!(quote_path("~/My Repos/Mini-Me"), "~/'My Repos/Mini-Me'");
        assert_eq!(quote_path("/opt/Mini Me"), "'/opt/Mini Me'");
    }

    #[test]
    fn probes_run_through_a_login_shell_on_this_machine() {
        let _env = env_lock::hold();
        // A check that runs somewhere other than the backend does is worse than no
        // check: it reports green for a machine that cannot launch anything.
        let config = BackendConfig {
            project_dir: PathBuf::from("/home/x/Mini-Me"),
            ..Default::default()
        };
        #[cfg(windows)]
        assert_eq!(
            config.shell_argv("echo ok"),
            vec![
                "powershell.exe",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "echo ok",
            ]
        );
        #[cfg(not(windows))]
        assert_eq!(config.shell_argv("echo ok"), vec!["bash", "-lc", "echo ok"]);
        assert_eq!(config.backend_dir(), "/home/x/Mini-Me");
    }

    #[test]
    fn the_setup_script_is_named_the_way_the_backend_shell_sees_it() {
        let _env = env_lock::hold();
        let config = BackendConfig {
            project_dir: PathBuf::from("~/Mini-Me"),
            ..Default::default()
        };
        let command = config.setup_script();
        // The source now ships in this repository (`mini-me/`), so provisioning always has one
        // to copy from and the script never reaches GitHub for it. This assertion used to be
        // `starts_with("bash '")` — true only while a developer tree had no bundled copy, which
        // stopped being the case the moment the backend moved in here.
        assert!(command.contains("MINIME_BUNDLED_SOURCE"), "{command}");
        #[cfg(windows)]
        {
            assert!(command.contains("& '"), "{command}");
            assert!(command.contains("setup-backend.ps1"), "{command}");
            assert!(command.ends_with("-Dir '~/Mini-Me'"), "{command}");
        }
        #[cfg(not(windows))]
        {
            assert!(command.contains("bash '"), "{command}");
            assert!(command.contains("setup-backend.sh"), "{command}");
            assert!(command.ends_with("~/'Mini-Me'"), "{command}");
        }
    }

    /// The backend source is found in this repository, without an environment variable.
    ///
    /// **The point of the monorepo, as an assertion.** Provisioning and updates both hang off
    /// `bundled_backend_dir()`; if it stops finding `mini-me/`, the app silently falls back to
    /// cloning a *private* repository a fresh install has no credentials for — which is the
    /// failure that cost §131 and §134, and it presents as a backend that simply never changes.
    #[test]
    fn the_backend_source_ships_in_this_repository() {
        let _env = env_lock::hold();
        std::env::remove_var("MINIME_BUNDLED_BACKEND");
        std::env::remove_var("MINIME_SOURCE_DIR");
        std::env::remove_var("MINIME_VENDOR_DIR");
        let found = bundled_backend_dir().expect("mini-me/ is part of this repo");
        assert!(
            found.ends_with("mini-me"),
            "expected the in-repo source, found {found:?}"
        );
        assert!(found.join("backend/agent.py").is_file(), "{found:?}");
        assert!(found.join("skills").is_dir(), "{found:?}");
    }

    #[test]
    fn the_app_only_claims_ownership_of_what_it_provisioned() {
        let _env = env_lock::hold();
        // The whole safety property of the update story. A checkout somebody pointed us
        // at may be their working clone — the reference checkout on this developer's own
        // machine has ten local branches — so `git checkout <pin>` on it would destroy
        // work. Ownership is what gates that, and it must never be assumed.
        std::env::remove_var("MINIME_BACKEND_DIR");

        // A fresh machine — nothing recorded, nothing to discover — lands on the
        // app-owned path and claims it. `HOME` is redirected because the developer box
        // running this test *does* have a checkout to discover, and finding one is a
        // different case (asserted below).
        let empty = std::env::temp_dir().join("mini-me-fresh-machine");
        // Cleared first: the second half of this test plants a checkout under this home,
        // so without it the *next* run would discover that one and "fresh machine" would
        // no longer be fresh. It failed exactly that way once.
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).expect("scratch home");
        let real_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &empty);
        std::env::set_var("MINIME_DATA_DIR", empty.join("data"));
        let (dir, owned) = resolve_project_dir(None);
        assert!(owned, "the app-owned path is ours to manage");
        assert_eq!(dir, empty.join("data/backend"), "{}", dir.display());

        // A checkout the app merely *found* is adopted, never owned — this is the case
        // that protects a developer's working clone.
        let theirs = empty.join("Documents/Mini-Me");
        std::fs::create_dir_all(&theirs).expect("their checkout");
        std::fs::write(theirs.join("langgraph.json"), "{}").expect("write");
        let (dir, owned) = resolve_project_dir(None);
        assert_eq!(dir, theirs);
        assert!(!owned, "a discovered checkout belongs to whoever made it");

        std::env::remove_var("MINIME_DATA_DIR");
        match real_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }

        // Recorded as adopted stays adopted across launches.
        let (dir, owned) = resolve_project_dir(Some((PathBuf::from("/home/x/Mini-Me"), false)));
        assert!(!owned);
        assert_eq!(dir, PathBuf::from("/home/x/Mini-Me"));

        // An explicit environment variable is always somebody else's checkout.
        std::env::set_var("MINIME_BACKEND_DIR", "/srv/theirs");
        let (dir, owned) = resolve_project_dir(None);
        assert!(!owned, "a hand-pointed checkout is never ours");
        assert_eq!(dir, PathBuf::from("/srv/theirs"));
        std::env::remove_var("MINIME_BACKEND_DIR");
    }

    /// **The directory the app looks for is the directory the packager writes.**
    ///
    /// `bundled_backend_dir` has preferred `mini-me/` since the monorepo move, and
    /// `scripts/package.sh` copied `vendor/Mini-Me` — a clone of the separate private repo. So
    /// every release shipped a backend months behind this repository: four middleware modules
    /// absent, a route absent, and the dataverse reader still passing the argument name that had
    /// been corrected nine days before (§283).
    ///
    /// Neither side was wrong on its own, which is why both suites stayed green. The join was.
    #[test]
    fn the_packager_writes_the_directory_this_looks_for() {
        let packager = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/package.sh");
        let script = std::fs::read_to_string(&packager).expect("the packager is in this repo");

        // What `bundled_backend_dir` prefers, taken from the call rather than restated.
        assert!(
            script.contains("BACKEND_DEST=\"$OUT/mini-me\""),
            "package.sh must place the backend where bundled_backend_dir looks first"
        );
        // And it stamps it, or an installed copy can never tell it is out of date.
        assert!(
            script.contains(".bundled-backend"),
            "package.sh must stamp the bundle so setup-backend.sh can compare builds"
        );

        let setup = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/setup-backend.sh"),
        )
        .expect("the setup script is in this repo");
        assert!(
            setup.contains(".bundled-backend"),
            "setup-backend.sh must read the stamp, or the bundle updates and the machine does not"
        );

        let setup_ps1 = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/setup-backend.ps1"),
        )
        .expect("the Windows setup script is in this repo");
        assert!(
            setup_ps1.contains(".bundled-backend"),
            "setup-backend.ps1 must read the stamp too, or a Windows machine never updates"
        );
    }

    #[test]
    fn provisioning_prefers_a_bundled_copy_over_cloning_a_private_repo() {
        let _env = env_lock::hold();
        // Mini-Me is private, so a clone wants a personal access token — a wall for the
        // people this app is for. When a copy ships with the app, the script must be told
        // where it is.
        let scratch = std::env::temp_dir().join("mini-me-bundle-test");
        let _ = std::fs::create_dir_all(&scratch);
        std::fs::write(scratch.join("langgraph.json"), "{}").expect("write");

        std::env::set_var("MINIME_BUNDLED_BACKEND", &scratch);
        let config = BackendConfig {
            project_dir: PathBuf::from("/opt/backend"),
            ..Default::default()
        };
        let command = config.setup_script();
        #[cfg(windows)]
        {
            assert!(command.starts_with("$env:MINIME_BUNDLED_SOURCE"), "{command}");
            assert!(command.contains("setup-backend.ps1"), "{command}");
        }
        #[cfg(not(windows))]
        {
            assert!(command.starts_with("MINIME_BUNDLED_SOURCE"), "{command}");
            assert!(command.contains("setup-backend.sh"), "{command}");
        }

        // No bundle: the variable must be absent rather than empty, or the script would
        // treat "" as a source and skip straight to cloning with a confusing message.
        std::env::set_var("MINIME_BUNDLED_BACKEND", scratch.join("nope"));
        let command = BackendConfig::default().setup_script();
        assert!(!command.contains("MINIME_BUNDLED_SOURCE"), "{command}");
        std::env::remove_var("MINIME_BUNDLED_BACKEND");
    }

    #[test]
    fn only_something_shaped_like_a_token_is_treated_as_one() {
        // Without `--raw` the CLI pretty-prints a decoded header and payload, and when
        // nobody is logged in it prints prose. Handing either to the backend as a
        // credential produces an authentication failure that blames the wrong thing.
        assert!(looks_like_a_jwt(
            "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJhYmMifQ.c2ln-bmF0dXJl_x"
        ));
        for not_a_token in [
            "",
            "JWT Header:",
            "{\n  \"alg\": \"RS256\"\n}",
            "Not logged in. Run `asta auth login`.",
            "one.two",
            "one.two.three.four",
            "has spaces.in it.here",
        ] {
            assert!(!looks_like_a_jwt(not_a_token), "{not_a_token:?}");
        }
    }

    fn token_expiring_at(expires: u64) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine as _;

        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{expires}}}"#));
        format!("{header}.{payload}.signature")
    }

    #[test]
    fn a_stored_asta_token_with_time_left_avoids_a_refresh() {
        let now = 1_800_000_000;
        let token = token_expiring_at(now + 604_800);
        let secrets = vec![("ASTA_TOKEN".to_string(), token.clone())];

        assert_eq!(
            reusable_stored_asta_token(&secrets, now),
            Some(token.as_str())
        );
    }

    #[test]
    fn an_asta_token_near_expiry_is_refreshed_before_a_turn_can_outlive_it() {
        let now = 1_800_000_000;
        assert!(!asta_token_is_valid_at(
            &token_expiring_at(now + ASTA_TOKEN_MIN_VALIDITY_SECS),
            now
        ));
        assert!(asta_token_is_valid_at(
            &token_expiring_at(now + ASTA_TOKEN_MIN_VALIDITY_SECS + 1),
            now
        ));
    }

    #[test]
    fn a_token_without_a_numeric_expiry_never_skips_the_refresh() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(br#"{"exp":"next week"}"#);
        assert!(!asta_token_is_valid_at(
            &format!("{header}.{payload}.signature"),
            1_800_000_000
        ));
        assert!(!asta_token_is_valid_at("not-a-token", 1_800_000_000));
    }

    #[test]
    fn a_redacted_config_carries_no_credentials() {
        let _env = env_lock::hold();
        let config = BackendConfig {
            secrets: vec![("ASTA_TOKEN".into(), "super-secret".into())],
            ..Default::default()
        };
        let redacted = config.redacted();
        assert!(redacted.secrets.is_empty());
        // Everything else has to survive, or preflight would probe the wrong machine.
        assert_eq!(redacted.port, config.port);
        assert_eq!(redacted.backend_dir(), config.backend_dir());
    }

    #[test]
    fn local_execution_sets_the_variables_the_backend_needs() {
        let _env = env_lock::hold();
        // Pinned, or this test reads whichever `Documents` the machine running it has.
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe {
            std::env::set_var(
                crate::workspace::WORKSPACE_ENV,
                "/Users/researcher/Documents/Mini-Me",
            )
        };
        let execution = Execution::Local {
            overlay_dir: PathBuf::from("/repo/overlay"),
        };
        let env = execution_env(&execution, true);
        let get = |name: &str| {
            env.iter()
                .find(|(found, _)| found == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(get("MINIME_EXECUTION_BACKEND"), Some("local"));
        assert_eq!(get("MINIME_APPROVE_EXECUTE"), Some("1"));
        // The workspace the *app* chose — this is what puts the researcher's outputs
        // somewhere their file manager can reach and the chat can render (docs §42).
        assert_eq!(
            get(crate::workspace::WORKSPACE_ENV),
            Some("/Users/researcher/Documents/Mini-Me")
        );
        // Python imports `sitecustomize` from here at startup.
        assert_eq!(get("PYTHONPATH"), Some("/repo/overlay"));
        // SAFETY: same lock.
        unsafe { std::env::remove_var(crate::workspace::WORKSPACE_ENV) };
    }

    #[test]
    fn the_background_graph_id_is_the_same_on_both_sides() {
        // Three files name this id: here, `make_config.py` (which registers the graph) and
        // `async_agents.py` (whose tool points at it). They only had a comment saying they
        // must agree — and a disagreement fails when the coordinator first delegates,
        // mid-task and in front of the user, rather than at startup. Now it is checked.
        //
        // Reading the sources rather than importing them keeps this a plain unit test; the
        // Python is not ours to run from here.
        let overlay = normalized(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay"));
        for file in [
            "minime_local/make_config.py",
            "minime_local/async_agents.py",
        ] {
            let path = overlay.join(file);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                source.contains(&format!("BACKGROUND_GRAPH_ID = \"{BACKGROUND_GRAPH_ID}\"")),
                "{} does not declare BACKGROUND_GRAPH_ID = {BACKGROUND_GRAPH_ID:?}",
                path.display()
            );
        }
    }

    #[test]
    fn a_config_less_graph_build_never_reaches_for_a_provider_we_have_no_key_for() {
        // `backend/models.py` falls back to `openai::gpt-5.4` when `MINIME_DEFAULT_MODEL` is
        // unset, and this app deliberately keeps provider keys **out** of the environment. So
        // every call that builds the graph without a run config — `GET /threads/{id}/state`,
        // which the client polls while watching a background task — constructed an OpenAI client
        // with no key and returned 500. A background run finished and its result was unreadable
        // (docs §148).
        let _env = env_lock::hold();
        // The name and nothing else. A key on the backend's environment is readable by the
        // agent's own `execute` tool, which is the whole reason they ride in the run request.
        let env = model_env(Some("anthropic::claude-sonnet-4-5"));
        assert_eq!(
            env,
            vec![(
                "MINIME_DEFAULT_MODEL".to_string(),
                "anthropic::claude-sonnet-4-5".to_string()
            )]
        );

        // A spec that is not a spec is not exported: half a variable would send the backend to a
        // provider named after the whole string, which fails later and less clearly than the
        // default it replaced.
        assert!(model_env(None).is_empty());
        assert!(model_env(Some("claude-sonnet-4-5")).is_empty());
        assert!(model_env(Some("   ")).is_empty());
    }

    #[test]
    fn async_subagents_asks_langgraph_for_the_generated_config() {
        let _env = env_lock::hold();
        let plain = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            &Execution::Sandbox,
            true,
            false,
            true,
            None,
        );
        assert!(!plain.contains(&"--config".to_string()), "{plain:?}");

        let with_async = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            &Execution::Sandbox,
            true,
            true,
            true,
            None,
        );
        // Registering the background graph is what lets the coordinator's
        // `start_async_task` delegate work without blocking the chat (docs §30).
        assert!(with_async.contains(&"--config".to_string()), "{with_async:?}");
        assert!(
            with_async.contains(&GENERATED_CONFIG.to_string()),
            "{with_async:?}"
        );
    }

    #[test]
    fn quotes_paths_that_contain_spaces() {
        // "Documents\My Repos\..." is entirely normal on Windows, and an unquoted
        // assignment would silently split into a bogus command.
        let quoted = shell_quote("/mnt/c/Users/a b/overlay");
        assert_eq!(quoted, "'/mnt/c/Users/a b/overlay'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn host_execution_is_the_default_and_sandbox_is_the_escape_hatch() {
        let _env = env_lock::hold();
        // Nothing set, or anything other than `local`, must keep the sandbox: host
        // execution is not safe to default to until `execute` is human-gated (§18).
        // Unset, or anything that is not `sandbox`, is now host execution — the
        // default flipped once `execute` was gated (§19). `sandbox` is the escape hatch.
        for value in ["", "local", "Local ", "anything"] {
            std::env::set_var("MINIME_EXECUTION_BACKEND", value);
            assert!(
                matches!(resolve_execution(None), Execution::Local { .. }),
                "for {value:?}"
            );
        }
        for value in ["sandbox", " SANDBOX "] {
            std::env::set_var("MINIME_EXECUTION_BACKEND", value);
            assert!(
                matches!(resolve_execution(None), Execution::Sandbox),
                "for {value:?}"
            );
        }
        std::env::remove_var("MINIME_EXECUTION_BACKEND");
        assert!(matches!(resolve_execution(None), Execution::Local { .. }));

        // A flag beats a stale variable in both directions — `--sandbox` has to be
        // able to switch host execution *off* without the user hunting for what set it.
        std::env::set_var("MINIME_EXECUTION_BACKEND", "local");
        assert!(matches!(resolve_execution(Some(false)), Execution::Sandbox));
        std::env::set_var("MINIME_EXECUTION_BACKEND", "sandbox");
        assert!(matches!(
            resolve_execution(Some(true)),
            Execution::Local { .. }
        ));
        std::env::remove_var("MINIME_EXECUTION_BACKEND");
    }
}

#[cfg(test)]
mod source_tests {
    /// A real repository, so the version stamp reads what git actually writes rather than a
    /// fixture of what we believe it writes.
    fn repo() -> Option<(std::path::PathBuf, String)> {
        let base = std::env::temp_dir().join(format!("minime-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let origin = base.join("origin.git");
        let work = base.join("work");
        std::fs::create_dir_all(&origin).ok()?;

        let run = |at: &std::path::Path, args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(at)
                .args(args)
                .output()
                .ok()
                .filter(|out| out.status.success())
        };
        run(&origin, &["init", "-q", "--bare", "--initial-branch=main"])?;
        std::fs::create_dir_all(&work).ok()?;
        run(&work, &["init", "-q", "--initial-branch=main"])?;
        run(&work, &["config", "user.email", "t@example.org"])?;
        run(&work, &["config", "user.name", "t"])?;
        run(&work, &["remote", "add", "origin", &origin.to_string_lossy()])?;
        std::fs::write(work.join("a.txt"), "one").ok()?;
        run(&work, &["add", "-A"])?;
        run(&work, &["commit", "-qm", "one"])?;
        run(&work, &["push", "-q", "origin", "main"])?;
        // A branch the checkout can actually be moved to.
        run(&work, &["checkout", "-q", "-b", "target"])?;
        std::fs::write(work.join("b.txt"), "two").ok()?;
        run(&work, &["add", "-A"])?;
        run(&work, &["commit", "-qm", "two"])?;
        run(&work, &["push", "-q", "origin", "target"])?;
        run(&work, &["checkout", "-q", "main"])?;
        Some((work, base.to_string_lossy().into_owned()))
    }


    /// The version stamp reads a real checkout, including a linked worktree.
    ///
    /// **Because the whole point of it is to be trusted at 11pm.** Four diagnoses this week were
    /// made without knowing which commit was running, and two of them were wrong because of it.
    /// A stamp that prints `unresolved refs/heads/…` for an ordinary layout would be worse than
    /// none: it invites the shrug it exists to prevent. The worktree case was broken when first
    /// written — a linked worktree keeps its own `HEAD` and shares refs with the repository it
    /// came from, via `commondir`.
    #[test]
    fn the_backend_says_which_commit_it_is_running() {
        if std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let Some((clone, _)) = repo() else {
            eprintln!("skipping: git is not on PATH");
            return;
        };
        let tree = clone.parent().unwrap_or(&clone).join("linked");
        let linked = std::process::Command::new("git")
            .args(["-C", &clone.to_string_lossy(), "worktree", "add", "-q"])
            .arg(&tree)
            .arg("HEAD")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        let read = |dir: &std::path::Path| -> String {
            let script = format!(
                "import sys; sys.path.insert(0, {overlay:?})\n\
                 from minime_local import _checkout_version\n\
                 print(_checkout_version({dir:?}))",
                overlay = overlay.to_string_lossy(),
                dir = dir.to_string_lossy(),
            );
            let out = std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
                .arg("-c")
                .arg(&script)
                .output()
                .expect("python3 runs");
            assert!(
                out.status.success(),
                "reading the checkout version raised:
{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        let head = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .current_dir(&clone)
                .args(["rev-parse", "HEAD"])
                .output()
                .expect("git")
                .stdout,
        )
        .trim()
        .to_string();
        let stamp = read(&clone);
        assert!(
            !head.is_empty() && stamp.starts_with(&head[..7]),
            "a plain clone should stamp its own HEAD, said {stamp:?} for {head:?}"
        );
        if linked {
            let stamp = read(&tree);
            assert!(
                stamp.starts_with(&head[..7]),
                "a linked worktree shares its refs through commondir, said {stamp:?}"
            );
        }
        // Somewhere with no repository at all must say so rather than guessing.
        assert_eq!(read(std::env::temp_dir().as_path()), "not a git checkout");
    }

}
