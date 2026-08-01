//! Local sidecar supervision for the Mini-Me Python backend.
//!
//! The desktop app is a *client* of the existing Mini-Me agent stack, not a
//! reimplementation of it. `BackendSupervisor` owns the lifecycle of a locally
//! spawned backend process: it starts it on a localhost port, waits for health,
//! and tears it down on quit. Running the backend locally is what lets the app
//! inherit the local `asta` CLI's auth story (the web app has to paste a token
//! that expires; locally it is minted once from the CLI into the repo's `.env`).
//!
//! Verified against the Mini-Me repo (2026-07-30): the backend is a LangGraph
//! server started with `uv run langgraph dev`, defaulting to `127.0.0.1:2024`,
//! which auto-loads `.env` from the repo root and does not open a browser.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::protocol::LangGraphClient;

/// Where the sidecar's own stdout/stderr is tee'd. A GUI has no useful terminal,
/// and piping to us would let the child hold our stdout open (and deadlock once
/// the pipe buffer fills), so the logs go to a file we can point the user at.
fn default_log_path() -> PathBuf {
    std::env::temp_dir().join("mini-me-desktop-backend.log")
}

/// Run the backend inside a WSL2 distribution instead of on the host.
///
/// This is the **Windows strategy** (~98% of our users): the agent stack shells
/// out with POSIX commands (`>/dev/null`, `| python3 -c …`) and expects `bash`,
/// `python3` and the `asta` CLI, none of which behave under `cmd.exe`. Inside WSL
/// the backend simply *is* on Linux, so nothing upstream has to change — and the
/// client/backend boundary is HTTP on localhost, which WSL2 forwards. It also
/// dodges the MSVC build pain of installing the scientific stack on Windows.
#[derive(Clone, Debug)]
pub struct WslTarget {
    /// Distribution name; `None` uses WSL's default distro.
    pub distro: Option<String>,
    /// Checkout path *inside* the distro (a Linux path — `~` is expanded by the
    /// shell we launch through, so `~/Mini-Me` is fine).
    pub dir: String,
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
    normalized(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(name))
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
/// `setup-wsl.sh` is the provisioning script the Setup pane offers to run, and it has
/// to be named as a path the *backend's* shell can reach — inside WSL that means
/// `/mnt/c/…`, which is what [`BackendConfig::setup_script`] does with this.
fn scripts_dir() -> PathBuf {
    resource("MINIME_SCRIPTS_DIR", "scripts")
}

/// A Mini-Me copy shipped with the app, if there is one.
///
/// `vendor/Mini-Me`, populated by `scripts/bundle-backend.sh` and gitignored. Its
/// presence is what lets provisioning skip GitHub entirely — see [`BackendConfig::setup_script`].
fn bundled_backend_dir() -> Option<PathBuf> {
    // The variable names the checkout itself, not the directory holding it — the default
    // is `vendor/Mini-Me`, but someone overriding it is pointing at a specific copy.
    let dir = match std::env::var_os("MINIME_BUNDLED_BACKEND") {
        Some(dir) => PathBuf::from(dir),
        None => resource("MINIME_VENDOR_DIR", "vendor").join("Mini-Me"),
    };
    dir.join("langgraph.json").is_file().then_some(dir)
}

/// Render a path the way WSL sees it: `C:\\Users\\x` becomes `/mnt/c/Users/x`.
///
/// The overlay lives in *this* repo, which on Windows is on the Windows filesystem,
/// while the interpreter that must import it runs inside the distro.
pub(crate) fn wsl_path(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut chars = raw.chars();
    let drive = chars.next();
    let colon = chars.next();
    match (drive, colon) {
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic() => {
            format!("/mnt/{}{}", drive.to_ascii_lowercase(), &raw[2..])
        }
        // Already a POSIX path (or a UNC path we can't translate) — pass it through.
        _ => raw,
    }
}

/// How the client reaches the backend. Defaults to a locally spawned sidecar.
#[derive(Clone, Debug)]
pub struct BackendConfig {
    /// Port the local sidecar listens on.
    pub port: u16,
    /// The Mini-Me checkout to launch from (its `.env` supplies the API keys).
    /// Ignored when `wsl` is set — see [`WslTarget::dir`].
    pub project_dir: PathBuf,
    /// When set, the sidecar is launched inside WSL2 rather than on the host.
    pub wsl: Option<WslTarget>,
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
    /// When on, the launch regenerates an extended LangGraph config declaring a second
    /// graph — see `generate_config_command` and docs §30.
    pub async_subagents: bool,
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
        let execution = resolve_execution(
            override_local.or_else(|| {
                if std::env::var_os("MINIME_EXECUTION_BACKEND").is_some() {
                    None
                } else {
                    Some(settings.local_execution)
                }
            }),
        );
        // The launch command embeds both the port and the execution environment, so it is
        // rebuilt rather than patched.
        config.launch_command =
            launch_command_for(
                &config.project_dir,
                config.port,
                config.wsl.as_ref(),
                &execution,
                settings.approve_execute,
                settings.async_subagents,
            );
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
    /// the discovery probe — which has to shell into the distro — runs once rather than
    /// on every launch.
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
        let wsl = resolve_wsl_target(recorded.clone());
        // A recorded directory belongs to whichever side the backend runs on, so it is
        // consumed by exactly one of these two.
        let (project_dir, host_owned) = resolve_project_dir(
            recorded
                .filter(|_| wsl.is_none())
                .map(|(dir, owned)| (PathBuf::from(dir), owned)),
        );
        let owned = match &wsl {
            Some((_, owned)) => *owned,
            None => host_owned,
        };
        let wsl = wsl.map(|(target, _)| target);
        let execution = resolve_execution(None);
        Self {
            port,
            launch_command: launch_command_for(&project_dir, port, wsl.as_ref(), &execution, true, false),
            project_dir,
            wsl,
            attach_only: std::env::var_os("MINIME_BACKEND_ATTACH_ONLY").is_some(),
            log_path: default_log_path(),
            execution,
            secrets: Vec::new(),
            approve_execute: true,
            async_subagents: false,
            owned,
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
fn launch_command_for(
    project_dir: &Path,
    port: u16,
    wsl: Option<&WslTarget>,
    execution: &Execution,
    approve_execute: bool,
    async_subagents: bool,
) -> Vec<String> {
    if let Some(wsl) = wsl {
        let mut argv = vec!["wsl.exe".to_string()];
        if let Some(distro) = &wsl.distro {
            argv.push("-d".into());
            argv.push(distro.clone());
        }
        // Go through a login shell so PATH/uv are set up as the user's own shell
        // would have them, and `exec` so the shell is *replaced* by the server —
        // otherwise killing our child leaves the real process behind.
        //
        // Bind 0.0.0.0, not 127.0.0.1: WSL2's localhost forwarding reliably
        // reaches services bound to all interfaces, while loopback-only binds
        // are not always visible from Windows.
        argv.push("--".into());
        argv.push("bash".into());
        argv.push("-lc".into());
        // Host execution needs two variables set *inside* the distro, so they go in
        // the command line rather than on `wsl.exe`'s own environment.
        let mut exports = String::new();
        for (name, value) in execution_env(execution, true, approve_execute) {
            if name == "PYTHONPATH" {
                exports.push_str(&format!(
                    "PYTHONPATH=\"{}\" ",
                    overlay_expression(&wsl.dir, &value)
                ));
            } else {
                exports.push_str(&format!("{name}={} ", shell_quote(&value)));
            }
        }
        // Background work needs a second graph, declared in a config we generate from
        // upstream's just before launch (docs §30). `&&`, so a generator failure stops the
        // launch instead of silently starting a server whose coordinator holds tools
        // pointing at a graph nobody serves.
        let overlay = execution_env(execution, true, approve_execute)
            .into_iter()
            .find(|(name, _)| name == "PYTHONPATH")
            .map(|(_, value)| value)
            .unwrap_or_default();
        // Always, not only for async subagents: the backend loads the *in-distro* copy,
        // so without this an updated app keeps running the overlay it was provisioned
        // with (see `sync_overlay_command`).
        let mut prepare = String::new();
        if !overlay.is_empty() {
            prepare.push_str(&sync_overlay_command(&overlay, &wsl.dir));
            prepare.push_str("; ");
        }
        let config_flag = if async_subagents {
            prepare.push_str(&generate_config_command(&overlay, ".venv/bin/python"));
            prepare.push_str(" && ");
            format!(" --config {GENERATED_CONFIG}")
        } else {
            String::new()
        };
        argv.push(format!(
            "cd {dir} && {prepare}{exports}exec .venv/bin/langgraph dev --host 0.0.0.0 \
             --port {port}{config_flag} --no-reload --no-browser --n-jobs-per-worker {jobs}",
            // `quote_path`, not `shell_quote`: the default is `~/Mini-Me`, and quoting
            // the tilde would stop it expanding. A configured dir with a space in it
            // used to split into a bogus command.
            dir = quote_path(&wsl.dir),
            jobs = JOBS_PER_WORKER,
        ));
        return argv;
    }

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
    ]);
    if async_subagents {
        argv.push("--config".into());
        argv.push(GENERATED_CONFIG.into());
    }
    argv
}

/// The overlay copy that provisioning installs next to the checkout.
///
/// One definition, used by both the launch expression and the Setup pane's check — the
/// two spelled it separately and immediately disagreed.
fn provisioned_overlay(backend_dir: &str) -> String {
    format!("{}/.desktop-overlay", backend_dir.trim_end_matches('/'))
}

/// Where the overlay is found inside the distro, as a shell expression.
///
/// Provisioning copies the overlay to `<checkout>/.desktop-overlay`, and that copy is
/// preferred over the one in this repo. The repo's copy lives on the Windows filesystem,
/// which the distro reaches only while the app's folder still exists and the drive is
/// still mounted — and if it *isn't* reachable, Python imports nothing, raises nothing,
/// and the backend silently falls back to the remote sandbox (docs §24).
///
/// Decided by the distro's own shell at launch, not by probing from Windows: a `wsl.exe`
/// round trip costs seconds on every start, and there would be nowhere to cache the
/// answer that would not go stale the moment the user re-provisioned.
fn overlay_expression(wsl_dir: &str, fallback: &str) -> String {
    let local = quote_path(&provisioned_overlay(wsl_dir));
    format!(
        "$(if [ -f {local}/sitecustomize.py ]; then printf %s {local}; \
         else printf %s {fallback}; fi)",
        local = local,
        fallback = shell_quote(fallback),
    )
}

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

/// The shell fragment that regenerates the extended config before launching.
///
/// Run **every** launch rather than once at provisioning: upstream's `langgraph.json` is
/// what it extends, and a stale copy would quietly serve yesterday's dependencies after a
/// backend update.
///
/// One generator, invoked identically in both modes, because the alternative was writing
/// the JSON from Rust for the host path and from Python inside the distro for WSL — the
/// same logic twice, which is how the two drift.
/// Refresh the overlay copy that lives beside the checkout.
///
/// **The launch prefers the in-distro copy** (§25), which is what removed host execution's
/// dependence on `/mnt/c` being reachable. The cost, unnoticed until it bit: that copy is
/// made at *provisioning* time, so `git pull` + rebuild updated the repo's `overlay/` and
/// the backend went on loading a months-old copy. A fix shipped in the overlay simply
/// never ran — which is exactly how the Asta token fix appeared not to work.
///
/// Three small files, so copying them on every launch is cheaper than reasoning about
/// when to. `|| true` because a *stale* overlay still beats a failed launch, and the
/// repo's copy may genuinely be unreachable — the case the in-distro copy exists for.
fn sync_overlay_command(source: &str, backend_dir: &str) -> String {
    let target = quote_path(&provisioned_overlay(backend_dir));
    format!(
        "{{ mkdir -p {target} && cp -r {source}/. {target}/ ; }} >/dev/null 2>&1 || true",
        source = shell_quote(source.trim_end_matches('/')),
    )
}

fn generate_config_command(overlay: &str, python: &str) -> String {
    format!(
        "{python} {} .",
        shell_quote(&format!("{}/minime_local/make_config.py", overlay.trim_end_matches('/')))
    )
}

/// The environment that switches the backend to host execution.
///
/// Empty for [`Execution::Sandbox`], so the sandbox path is byte-for-byte the launch
/// it always was. `for_wsl` selects how the overlay path is spelled.
fn execution_env(execution: &Execution, for_wsl: bool, approve: bool) -> Vec<(String, String)> {
    let Execution::Local { overlay_dir } = execution else {
        return Vec::new();
    };
    let overlay = if for_wsl {
        wsl_path(overlay_dir)
    } else {
        overlay_dir.to_string_lossy().into_owned()
    };
    vec![
        ("MINIME_EXECUTION_BACKEND".to_string(), "local".to_string()),
        (
            "MINIME_APPROVE_EXECUTE".to_string(),
            if approve { "1" } else { "0" }.to_string(),
        ),
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
/// **Never on the command line.** In WSL mode the execution flags ride in the `bash -lc`
/// string, which `ps` would show to anyone else on the machine — fine for a flag, not for
/// a token. WSL's documented mechanism is `WSLENV`: set the variables on `wsl.exe` and
/// name them in `WSLENV`, and the distro inherits them.
///
/// Takes the already-read values rather than reading the keychain itself. That is not a
/// style choice: the Linux keychain client (zbus) runs its own `block_on`, and calling it
/// from a thread that is already driving a Tokio runtime panics with "Cannot start a
/// runtime from within a runtime" — which is exactly how the first live run of this code
/// died. Secrets are read once, on the main thread, before any runtime exists.
fn secret_env(secrets: &[(String, String)], wsl: bool) -> Vec<(String, String)> {
    if secrets.is_empty() {
        return Vec::new();
    }
    let mut env = secrets.to_vec();
    if wsl {
        let names: Vec<&str> = secrets.iter().map(|(name, _)| name.as_str()).collect();
        env.push(("WSLENV".to_string(), names.join(":")));
    }
    env
}

/// Single-quote a value for `bash -lc`, so a path with spaces survives.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Quote a path for a shell **while leaving a leading `~` able to expand**.
///
/// The WSL checkout defaults to `~/Mini-Me`, and `cd '~/Mini-Me'` does not work — the
/// quotes suppress tilde expansion and bash looks for a directory literally named `~`.
/// Quoting only the part after the tilde gets both: `~/'My Docs/Mini-Me'` expands *and*
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

/// Read the WSL configuration from the environment.
///
/// **On Windows this is the default**, because native Windows cannot host the
/// agent stack's execution: it shells out with POSIX commands and expects
/// `bash`/`python3`/`asta` (see docs §13). Set `MINIME_BACKEND_WSL=0` to opt out
/// and run the backend on the host anyway.
///
/// `MINIME_BACKEND_WSL=1` (or `true`) uses WSL's default distro; any other value
/// is taken as the distro name. The checkout path inside the distro comes from
/// `MINIME_BACKEND_WSL_DIR`, or from what Settings recorded, or from
/// [`owned_wsl_dir`].
///
/// Returns the target and whether the app owns that directory.
fn resolve_wsl_target(recorded: Option<(String, bool)>) -> Option<(WslTarget, bool)> {
    let raw = std::env::var("MINIME_BACKEND_WSL").unwrap_or_default();
    let raw = raw.trim();

    let explicitly_off =
        raw.eq_ignore_ascii_case("0") || raw.eq_ignore_ascii_case("false") || raw == "-";
    if explicitly_off {
        return None;
    }
    // Unset: on by default on Windows, off elsewhere (there is no `wsl.exe` to
    // call on Linux/macOS, where the backend runs natively).
    if raw.is_empty() && !cfg!(windows) {
        return None;
    }

    let use_default_distro =
        raw.is_empty() || raw.eq_ignore_ascii_case("1") || raw.eq_ignore_ascii_case("true");
    let distro = if use_default_distro {
        None
    } else {
        Some(raw.to_string())
    };
    let (dir, owned) = match std::env::var("MINIME_BACKEND_WSL_DIR")
        .ok()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
    {
        // Pointed at by hand: someone else's checkout, so not ours to update.
        Some(dir) => (dir, false),
        None => recorded.unwrap_or_else(|| (owned_wsl_dir(), true)),
    };
    Some((WslTarget { distro, dir }, owned))
}

/// The checkout the app provisions and owns, inside the WSL distro.
///
/// **On the distro's own filesystem, never `/mnt/c`.** WSL2 reaches Windows drives over
/// a 9p mount whose per-file overhead is high, and a Python environment holding the
/// scientific stack is thousands of small files that get stat'd on every interpreter
/// start. A venv on `/mnt/c` is the one placement guaranteed to feel broken.
pub fn owned_wsl_dir() -> String {
    std::env::var("MINIME_OWNED_WSL_DIR")
        .ok()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
        // A tilde path on purpose: it is expanded by the distro's own login shell, and
        // we cannot know the Linux user's home directory from Windows.
        .unwrap_or_else(|| "~/.local/share/mini-me-desktop/backend".to_string())
}

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
    ///
    /// In WSL mode the checkout lives on the distro's filesystem, which we can't
    /// cheaply stat from Windows, so we defer to the spawn error instead of
    /// pretending to validate it here.
    pub fn looks_like_backend_repo(&self) -> bool {
        if self.wsl.is_some() {
            return true;
        }
        self.project_dir.join("langgraph.json").is_file()
    }

    /// Human-readable description of where the sidecar will run.
    pub fn location(&self) -> String {
        match &self.wsl {
            Some(wsl) => format!(
                "WSL ({}) {}",
                wsl.distro.as_deref().unwrap_or("default distro"),
                wsl.dir
            ),
            None => self.project_dir.display().to_string(),
        }
    }

    /// Human-readable execution locality, for the log line and the status bar.
    pub fn execution_label(&self) -> &'static str {
        match self.execution {
            Execution::Sandbox => "remote sandbox",
            Execution::Local { .. } => "host (local)",
        }
    }

    /// Wrap a POSIX shell command so it runs **where the backend runs**.
    ///
    /// This is what makes the preflight checks worth trusting: looking for `langgraph`
    /// on Windows says nothing at all when the backend lives inside a WSL distro. Every
    /// probe and every offered fix is routed through the same hop as the launch command
    /// itself, so a green check means green *for the process that matters*.
    ///
    /// A **login** shell (`-lc`), matching [`launch_command_for`]: `uv` installs itself
    /// into `~/.local/bin`, which only a login shell has on `PATH`.
    pub fn shell_argv(&self, script: &str) -> Vec<String> {
        let mut argv = Vec::new();
        if let Some(wsl) = &self.wsl {
            argv.push("wsl.exe".to_string());
            if let Some(distro) = &wsl.distro {
                argv.push("-d".into());
                argv.push(distro.clone());
            }
            argv.push("--".into());
        }
        argv.extend(["bash".to_string(), "-lc".to_string(), script.to_string()]);
        argv
    }

    /// The checkout path **as the backend's own shell spells it** — a Linux path inside
    /// the distro, a host path otherwise.
    pub fn backend_dir(&self) -> String {
        match &self.wsl {
            Some(wsl) => wsl.dir.clone(),
            None => self.project_dir.to_string_lossy().into_owned(),
        }
    }

    /// The overlay path as the backend's interpreter would have to import it, or `None`
    /// when execution is remote and there is no overlay in play.
    pub fn overlay_for_backend(&self) -> Option<String> {
        let Execution::Local { overlay_dir } = &self.execution else {
            return None;
        };
        Some(if self.wsl.is_some() {
            wsl_path(overlay_dir)
        } else {
            overlay_dir.to_string_lossy().into_owned()
        })
    }

    /// Where the overlay might be, **in the order the launch command prefers**.
    ///
    /// Exists so the Setup pane and the launch cannot drift apart. They did: the pane
    /// reported the copy on the Windows drive while the launch was already preferring the
    /// one provisioning had installed inside the distro — a check that reports a different
    /// path from the one actually used is worse than no check.
    ///
    /// Host mode has one candidate on purpose. Provisioning copies the overlay there too,
    /// but on a host run the repo's own copy is always reachable, so preferring one over
    /// the other would add a branch that can never change the outcome.
    pub fn overlay_candidates(&self) -> Vec<String> {
        let Some(fallback) = self.overlay_for_backend() else {
            return Vec::new();
        };
        let mut candidates = Vec::new();
        if self.wsl.is_some() {
            candidates.push(provisioned_overlay(&self.backend_dir()));
        }
        candidates.push(fallback);
        candidates
    }

    /// The provisioning command: `bash …/setup-wsl.sh <checkout>`, spelled for the
    /// backend's shell. Re-running it is safe — the script never overwrites a checkout
    /// or a `.env`.
    ///
    /// When a backend copy ships with the app, its path is passed in so the script
    /// provisions from it instead of cloning. That is the difference between an install
    /// a scientist can complete and one that stops at a GitHub token prompt, because
    /// Mini-Me is a private repository (see `scripts/bundle-backend.sh`).
    pub fn setup_script(&self) -> String {
        let for_wsl = self.wsl.is_some();
        let spell = |path: &Path| {
            if for_wsl {
                wsl_path(path)
            } else {
                path.to_string_lossy().into_owned()
            }
        };
        let script = spell(&scripts_dir().join("setup-wsl.sh"));
        let mut command = String::new();
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
        command
    }

    /// Spell a path on *this* machine the way the backend would have to open it.
    ///
    /// This is what makes "drop a file on the window" work at all on Windows: the file is
    /// at `C:\Users\…\yield.csv`, and the agent lives inside a distro where that same file
    /// is `/mnt/c/Users/…/yield.csv`. The researcher should never have to know that.
    ///
    /// The file is **referenced, not copied**. Keeping a scientist's data where they put
    /// it is most of the point of a desktop app; copying it into a working directory
    /// creates a second version that goes stale the moment they edit the first.
    pub fn path_for_backend(&self, path: &Path) -> String {
        if self.wsl.is_some() {
            wsl_path(path)
        } else {
            path.to_string_lossy().into_owned()
        }
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

        // Secrets always go on the process environment, in both modes — see
        // `secret_env` for why they must not travel on the command line.
        let mut secrets = self.config.secrets.clone();
        // A minted token beats a stored one. Asta access tokens last **seven days**
        // (measured: `exp - iat` = 604800), so a token pasted into Settings becomes a
        // weekly chore — and its expiry surfaces as "the theorizer returned no task id",
        // which names neither the token nor the fix.
        if let Some(token) = mint_asta_token(&self.config) {
            secrets.retain(|(name, _)| name != "ASTA_TOKEN");
            secrets.push(("ASTA_TOKEN".to_string(), token));
        }
        for (name, value) in secret_env(&secrets, self.config.wsl.is_some()) {
            command.env(name, value);
        }

        // Host execution on the host itself: the variables go straight onto the
        // child. (In WSL mode they are already inside the `bash -lc` string, because
        // `wsl.exe`'s own environment does not cross into the distro.)
        if self.config.wsl.is_none() {
            for (name, value) in execution_env(&self.config.execution, false, self.config.approve_execute) {
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
        }

        // In WSL mode the working directory is set by the shell we launch *inside*
        // the distro; pointing `wsl.exe` at a host path would be meaningless, and
        // would fail the spawn outright if that path doesn't exist on Windows.
        if self.config.wsl.is_none() {
            command.current_dir(&self.config.project_dir);
        }

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
            if self.config.wsl.is_some() {
                format!(
                    "failed to launch the backend in {}. Check that WSL is running \
                     (`wsl --status`), that the checkout exists there, and that it \
                     was synced with `uv sync --extra dev`.",
                    self.config.location()
                )
            } else {
                format!(
                    "failed to spawn the backend ({program}). If the LangGraph CLI is \
                     missing, install the dev extra in {}:\n    uv sync --extra dev",
                    self.config.project_dir.display()
                )
            }
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
    pub async fn ensure_running(&mut self, client: &LangGraphClient) -> Result<String> {
        if client.is_healthy().await {
            return Ok("attached to a running backend".into());
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
        Ok("sidecar started".into())
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

impl Drop for BackendSupervisor {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            tracing::info!("terminating backend sidecar");
            terminate(&mut child);
            // Killing `wsl.exe` does not reliably reap the Linux process it
            // fronted, so ask the distro to clean up. Best-effort: if the server
            // already exited, `pkill` just finds nothing.
            if let Some(wsl) = &self.config.wsl {
                let mut command = Command::new("wsl.exe");
                if let Some(distro) = &wsl.distro {
                    command.args(["-d", distro]);
                }
                let _ = command
                    .args(["--", "pkill", "-f", "langgraph dev"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
}

/// Ask the `asta` CLI for a fresh access token, where the backend runs.
///
/// **Why the app does this instead of the user.** Asta access tokens last seven days
/// (`exp - iat` = 604800 on a real one), so storing one in the keychain means re-pasting
/// it every week — and when it lapses the failure reads "the Asta theorizer returned no
/// task id", which names neither the token nor the fix. `asta auth login` already leaves a
/// *refresh* credential behind, and `print-token --refresh` turns that into a valid access
/// token on demand. So the app mints one per launch and the researcher logs in once.
///
/// Run at spawn rather than at window-open: this can cost seconds on a cold WSL distro,
/// and by here we are already starting the backend, which the user is waiting on anyway.
///
/// `None` on any failure — no CLI, not logged in, a changed flag. The stored token (if
/// any) still applies, and the Setup pane reports a missing `asta` separately.
fn mint_asta_token(config: &BackendConfig) -> Option<String> {
    if std::env::var_os("MINIME_NO_ASTA_MINT").is_some() {
        return None;
    }
    let argv = config.shell_argv("asta auth print-token --raw --refresh 2>/dev/null");
    let (program, rest) = argv.split_first()?;
    let output = Command::new(program)
        .args(rest)
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
    tracing::info!("minted a fresh Asta token from the CLI");
    Some(token)
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
/// point, but `uv run` forks the real server as a grandchild and `wsl.exe` fronts a
/// process living in another kernel — both would survive and keep holding the port, so
/// the next launch attaches to a stale backend or fails outright.
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
    use super::*;

    #[test]
    fn translates_windows_paths_for_wsl() {
        // The overlay lives in this repo — on Windows that means the Windows
        // filesystem, while the interpreter that imports it runs inside the distro.
        assert_eq!(
            wsl_path(Path::new(r"C:\Users\piero\mini-me-desktop\overlay")),
            "/mnt/c/Users/piero/mini-me-desktop/overlay"
        );
        assert_eq!(wsl_path(Path::new(r"D:\repos\overlay")), "/mnt/d/repos/overlay");
        // A POSIX path is already what WSL wants.
        assert_eq!(wsl_path(Path::new("/home/piero/overlay")), "/home/piero/overlay");
    }

    #[test]
    fn the_sandbox_path_is_left_exactly_as_it_was() {
        let _env = env_lock::hold();
        // Regression guard: no stray variables on the default launch, so choosing
        // nothing keeps upstream's behaviour byte for byte.
        assert!(execution_env(&Execution::Sandbox, false, true).is_empty());
        assert!(execution_env(&Execution::Sandbox, true, true).is_empty());

        let argv = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            &Execution::Sandbox,
            true,
            false,
        );
        let command = argv.last().expect("the bash -lc payload");
        assert!(!command.contains("MINIME_EXECUTION_BACKEND"), "{command}");
        assert!(command.contains("cd ~/'Mini-Me' && exec .venv/bin/langgraph dev"), "{command}");
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
    fn probes_are_routed_to_wherever_the_backend_runs() {
        let _env = env_lock::hold();
        // A check that runs on the wrong side of the WSL boundary is worse than no
        // check: it reports green for a machine that cannot launch anything.
        let mut config = BackendConfig::default();
        config.wsl = Some(WslTarget {
            distro: Some("Ubuntu".into()),
            dir: "~/Mini-Me".into(),
        });
        assert_eq!(
            config.shell_argv("echo ok"),
            vec!["wsl.exe", "-d", "Ubuntu", "--", "bash", "-lc", "echo ok"],
        );
        assert_eq!(config.backend_dir(), "~/Mini-Me");

        config.wsl = None;
        config.project_dir = PathBuf::from("/home/x/Mini-Me");
        assert_eq!(config.shell_argv("echo ok"), vec!["bash", "-lc", "echo ok"]);
        assert_eq!(config.backend_dir(), "/home/x/Mini-Me");
    }

    #[test]
    fn the_setup_script_is_named_the_way_the_backend_shell_sees_it() {
        let _env = env_lock::hold();
        let mut config = BackendConfig::default();
        config.wsl = Some(WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        });
        let command = config.setup_script();
        assert!(command.starts_with("bash '"), "{command}");
        assert!(command.contains("setup-wsl.sh"), "{command}");
        assert!(command.ends_with("~/'Mini-Me'"), "{command}");
    }

    #[test]
    fn the_app_only_claims_ownership_of_what_it_provisioned() {
        let _env = env_lock::hold();
        // The whole safety property of the update story. A checkout somebody pointed us
        // at may be their working clone — the reference checkout on this developer's own
        // machine has ten local branches — so `git checkout <pin>` on it would destroy
        // work. Ownership is what gates that, and it must never be assumed.
        std::env::remove_var("MINIME_BACKEND_WSL_DIR");
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

        // Same rule inside the distro. WSL mode has to be asked for explicitly here
        // because it is only on by default on Windows, and this runs on Linux.
        std::env::set_var("MINIME_BACKEND_WSL", "1");
        std::env::set_var("MINIME_BACKEND_WSL_DIR", "~/their-clone");
        let (target, owned) = resolve_wsl_target(None).expect("wsl target");
        assert!(!owned);
        assert_eq!(target.dir, "~/their-clone");
        std::env::remove_var("MINIME_BACKEND_WSL_DIR");

        let (target, owned) = resolve_wsl_target(None).expect("wsl target");
        assert!(owned);
        assert_eq!(target.dir, owned_wsl_dir());
        std::env::remove_var("MINIME_BACKEND_WSL");
        // On the distro's own filesystem: a venv over /mnt/c is the placement that makes
        // everything feel broken.
        assert!(!owned_wsl_dir().starts_with("/mnt/"), "{}", owned_wsl_dir());
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
        let mut config = BackendConfig::default();
        config.wsl = None;
        config.project_dir = PathBuf::from("/opt/backend");
        let command = config.setup_script();
        assert!(command.starts_with("MINIME_BUNDLED_SOURCE="), "{command}");
        assert!(command.contains("setup-wsl.sh"), "{command}");

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

    #[test]
    fn a_redacted_config_carries_no_credentials() {
        let _env = env_lock::hold();
        let mut config = BackendConfig::default();
        config.secrets = vec![("ASTA_TOKEN".into(), "super-secret".into())];
        let redacted = config.redacted();
        assert!(redacted.secrets.is_empty());
        // Everything else has to survive, or preflight would probe the wrong machine.
        assert_eq!(redacted.port, config.port);
        assert_eq!(redacted.backend_dir(), config.backend_dir());
    }

    #[test]
    fn local_execution_reaches_the_interpreter_inside_wsl() {
        let execution = Execution::Local {
            overlay_dir: PathBuf::from(r"C:\repo\overlay"),
        };
        let argv = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: Some("Ubuntu".into()),
                dir: "~/Mini-Me".into(),
            }),
            &execution,
            true,
            true,
        );
        let command = argv.last().expect("the bash -lc payload");
        // Assignments must land *before* `exec`, or the server never sees them.
        assert!(
            command.contains("MINIME_EXECUTION_BACKEND='local' MINIME_APPROVE_EXECUTE='1' PYTHONPATH="),
            "{command}"
        );
        assert!(command.contains("PYTHONPATH=\"$(if [ -f "), "{command}");
        // The in-distro copy is preferred, with the repo's copy on the Windows drive as
        // the fallback — the whole point being that a working install stops depending on
        // /mnt/c at all.
        assert!(
            command.contains("~/'Mini-Me/.desktop-overlay'/sitecustomize.py"),
            "{command}"
        );
        assert!(command.contains("printf %s '/mnt/c/repo/overlay'"), "{command}");
        // And it still ends up as one assignment in front of exec.
        assert!(command.contains("fi)\" exec .venv/bin/langgraph dev"), "{command}");
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
        for file in ["minime_local/make_config.py", "minime_local/async_agents.py"] {
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
    fn every_launch_refreshes_the_overlay_the_backend_actually_loads() {
        // The launch prefers the copy inside the distro, so without this an updated app
        // keeps running the overlay it was provisioned with — a fix shipped in the overlay
        // never reaching the machine, which is exactly what happened with the Asta token.
        let _env = env_lock::hold();
        let argv = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            &Execution::Local {
                overlay_dir: PathBuf::from(r"C:\repo\overlay"),
            },
            true,
            false,
        );
        let command = argv.last().expect("the bash -lc payload");
        let sync = command.find("cp -r").expect("the overlay sync");
        let serve = command.find("exec .venv/bin/langgraph").expect("the server");
        assert!(sync < serve, "{command}");
        assert!(command.contains("~/'Mini-Me/.desktop-overlay'"), "{command}");
        // Never fatal: a stale overlay beats a backend that will not start, and the repo's
        // copy may be genuinely unreachable — the case the in-distro copy exists for.
        assert!(command.contains("|| true"), "{command}");

        // The sandbox path stays untouched: no overlay, nothing to sync.
        let sandbox = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            &Execution::Sandbox,
            true,
            false,
        );
        assert!(!sandbox.last().unwrap().contains("cp -r"), "{sandbox:?}");
    }

    #[test]
    fn background_work_registers_its_graph_before_the_server_starts() {
        let _env = env_lock::hold();
        let execution = Execution::Local {
            overlay_dir: PathBuf::from(r"C:\repo\overlay"),
        };
        let wsl = WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        };
        let argv = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            &execution,
            true,
            true,
        );
        let command = argv.last().expect("the bash -lc payload");

        // The generator runs *before* the server, joined with `&&` — if it fails the
        // launch must stop, not start a coordinator holding tools that point at a graph
        // nobody serves.
        let generate = command.find("make_config.py").expect("the generator");
        let serve = command.find("exec .venv/bin/langgraph").expect("the server");
        assert!(generate < serve, "{command}");
        assert!(command.contains("&& exec") || command.contains("&& MINIME"), "{command}");
        assert!(command.contains("--config .mini-me-desktop.langgraph.json"), "{command}");

        // And with the feature off, the launch is exactly what it always was.
        let plain = launch_command_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            &execution,
            true,
            false,
        );
        let plain = plain.last().expect("payload");
        assert!(!plain.contains("make_config"), "{plain}");
        assert!(!plain.contains("--config"), "{plain}");
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
        assert!(matches!(resolve_execution(Some(true)), Execution::Local { .. }));
        std::env::remove_var("MINIME_EXECUTION_BACKEND");
    }
}
