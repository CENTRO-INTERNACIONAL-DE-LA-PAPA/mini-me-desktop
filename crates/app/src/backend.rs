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
}

impl Default for BackendConfig {
    fn default() -> Self {
        let port = std::env::var("MINIME_BACKEND_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(2024);
        let wsl = resolve_wsl_target();
        let project_dir = resolve_project_dir();
        Self {
            port,
            launch_command: launch_command_for(&project_dir, port, wsl.as_ref()),
            project_dir,
            wsl,
            attach_only: std::env::var_os("MINIME_BACKEND_ATTACH_ONLY").is_some(),
            log_path: default_log_path(),
        }
    }
}

/// Build the launch argv.
///
/// Prefer the checkout's own venv entry point over `uv run langgraph`: `uv run`
/// **forks** the real server as a grandchild, so killing our direct child leaves
/// an orphaned server holding the port (observed in P6.2). Invoking the venv
/// binary directly keeps it a single process we actually own.
fn launch_command_for(project_dir: &Path, port: u16, wsl: Option<&WslTarget>) -> Vec<String> {
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
        argv.push(format!(
            "cd {dir} && exec .venv/bin/langgraph dev --host 0.0.0.0 --port {port} --no-reload",
            dir = wsl.dir,
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
    ]);
    argv
}

/// Read the WSL configuration from the environment.
///
/// `MINIME_BACKEND_WSL=1` (or `true`) uses WSL's default distro; any other value
/// is taken as the distro name. The checkout path inside the distro comes from
/// `MINIME_BACKEND_WSL_DIR`, defaulting to `~/Mini-Me`.
fn resolve_wsl_target() -> Option<WslTarget> {
    let raw = std::env::var("MINIME_BACKEND_WSL").ok()?;
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("0") || raw.eq_ignore_ascii_case("false") {
        return None;
    }
    let distro = if raw.eq_ignore_ascii_case("1") || raw.eq_ignore_ascii_case("true") {
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
            anyhow::bail!(
                "no backend at {} and attach-only mode is on",
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
