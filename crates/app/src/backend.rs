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
    if let Some(dir) = std::env::var_os("MINIME_OVERLAY_DIR") {
        return PathBuf::from(dir);
    }
    // `CARGO_MANIFEST_DIR` is `crates/app`; the overlay sits at the repo root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../overlay")
        .components()
        .collect()
}

/// Render a path the way WSL sees it: `C:\\Users\\x` becomes `/mnt/c/Users/x`.
///
/// The overlay lives in *this* repo, which on Windows is on the Windows filesystem,
/// while the interpreter that must import it runs inside the distro.
fn wsl_path(path: &Path) -> String {
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
}

impl BackendConfig {
    /// Build the configuration, letting a command-line flag override the environment.
    ///
    /// `Some(true)` forces host execution, `Some(false)` forces the sandbox, `None`
    /// falls back to `MINIME_EXECUTION_BACKEND`.
    pub fn with_execution_override(override_local: Option<bool>) -> Self {
        let settings = crate::settings::Settings::load();
        let mut config = Self::default();
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
            );
        config.execution = execution;
        config.approve_execute = settings.approve_execute;
        // Read here, on the main thread: see `secret_env`.
        config.secrets = crate::settings::asta_env();
        config
    }
}

impl Default for BackendConfig {
    fn default() -> Self {
        let port = std::env::var("MINIME_BACKEND_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(2024);
        let wsl = resolve_wsl_target();
        let project_dir = resolve_project_dir();
        let execution = resolve_execution(None);
        Self {
            port,
            launch_command: launch_command_for(&project_dir, port, wsl.as_ref(), &execution, true),
            project_dir,
            wsl,
            attach_only: std::env::var_os("MINIME_BACKEND_ATTACH_ONLY").is_some(),
            log_path: default_log_path(),
            execution,
            secrets: Vec::new(),
            approve_execute: true,
        }
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
            exports.push_str(&format!("{name}={} ", shell_quote(&value)));
        }
        argv.push(format!(
            "cd {dir} && {exports}exec .venv/bin/langgraph dev --host 0.0.0.0 \
             --port {port} --no-reload --no-browser --n-jobs-per-worker {jobs}",
            dir = wsl.dir,
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
    argv
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
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
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
/// `MINIME_BACKEND_WSL_DIR`, defaulting to `~/Mini-Me`.
fn resolve_wsl_target() -> Option<WslTarget> {
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
    let dir = std::env::var("MINIME_BACKEND_WSL_DIR")
        .ok()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
        .unwrap_or_else(|| "~/Mini-Me".to_string());
    Some(WslTarget { distro, dir })
}

/// Where the Mini-Me Python checkout lives. `MINIME_BACKEND_DIR` wins; otherwise
/// try the conventional sibling locations before giving up on the cwd.
fn resolve_project_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MINIME_BACKEND_DIR") {
        return PathBuf::from(dir);
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
    candidates
        .into_iter()
        .find(|p| p.join("langgraph.json").is_file())
        .unwrap_or_else(|| PathBuf::from("."))
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
}

/// Owns the spawned backend process and shuts it down on drop.
pub struct BackendSupervisor {
    config: BackendConfig,
    child: Option<Child>,
}

impl BackendSupervisor {
    pub fn new(config: BackendConfig) -> Self {
        Self {
            config,
            child: None,
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
        for (name, value) in secret_env(&self.config.secrets, self.config.wsl.is_some()) {
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

/// TODO(P6.4): Windows needs a Job Object to reap a whole process tree; this
/// kills only the direct child, which is correct for the venv entry point but
/// would orphan a `uv run` grandchild.
#[cfg(not(unix))]
fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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
        );
        let command = argv.last().expect("the bash -lc payload");
        assert!(!command.contains("MINIME_EXECUTION_BACKEND"), "{command}");
        assert!(command.contains("cd ~/Mini-Me && exec .venv/bin/langgraph dev"), "{command}");
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
        );
        let command = argv.last().expect("the bash -lc payload");
        // Assignments must land *before* `exec`, or the server never sees them.
        assert!(
            command.contains(
                "MINIME_EXECUTION_BACKEND='local' MINIME_APPROVE_EXECUTE='1'                  PYTHONPATH='/mnt/c/repo/overlay' exec"
                    .replace("                 ", "")
                    .as_str()
            ),
            "{command}"
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
