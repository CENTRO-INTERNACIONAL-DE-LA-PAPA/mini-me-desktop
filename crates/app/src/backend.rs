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
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Where the agent's files and shell commands run: always this machine.
///
/// Upstream Mini-Me executes inside a remote LangSmith sandbox. For a local-first
/// desktop app that is infrastructure we neither need nor want (docs §10/§11), so this
/// app runs the agent's code on the host (or, on Windows, inside WSL) unconditionally.
/// What makes that safe is the approval gate — every `execute` call stops and asks
/// (docs §19).

/// Find a directory that ships with the app.
///
/// Three places, in order:
///
/// 1. **An environment override**, for anything unusual.
/// 2. **Next to the executable** — how a *packaged* build is laid out
///    (`mini-me-desktop.exe` beside `mini-me/`, `scripts/`, `vendor/`). Checked before
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

/// Where this repo's helper scripts live, resolved the same way as [`bundled_backend_dir`].
///
/// `setup-wsl.sh` is the provisioning script the Setup pane offers to run, and it has
/// to be named as a path the *backend's* shell can reach — inside WSL that means
/// `/mnt/c/…`, which is what [`BackendConfig::setup_script`] does with this.
fn scripts_dir() -> PathBuf {
    resource("MINIME_SCRIPTS_DIR", "scripts")
}

/// The Mini-Me source this app runs.
///
/// **`mini-me/` in this repository, tracked.** *"from now I want a mono repo in mini me desktop.
/// I dont want to depende on a secod repo anymmore."*
///
/// It is also what makes updates work at all. The backend used to be fetched from its own
/// repository, which is private: WSL has no credentials for it, so `git fetch` either hung waiting
/// for a sign-in dialog (§131) or failed fast and left the checkout on last month's commit while
/// every log line looked healthy (§134). Shipping the source here replaces a network call needing
/// credentials with a file copy needing nothing — `git pull` on this repo *is* the backend update.
///
/// `MINIME_BUNDLED_BACKEND` overrides it. The old `vendor/Mini-Me` fallback (a clone of the
/// separate private repo, populated by a since-removed `scripts/bundle-backend.sh`) is gone —
/// `mini-me/` has been the only layout since the monorepo move, confirmed dead weight rather
/// than a real fallback (nothing in this repository still produces or reads `vendor/Mini-Me`).
pub(crate) fn bundled_backend_dir() -> Option<PathBuf> {
    // The variable names the checkout itself, not the directory holding it — someone overriding
    // it is pointing at a specific copy.
    if let Some(dir) = std::env::var_os("MINIME_BUNDLED_BACKEND") {
        let dir = PathBuf::from(dir);
        return dir.join("langgraph.json").is_file().then_some(dir);
    }
    let dir = resource("MINIME_SOURCE_DIR", "mini-me");
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

/// Whether [`wsl_path`] produces a path the distro could actually open.
///
/// The one shape that survives translation looking perfectly fine and fails anyway is a
/// Windows UNC path. `\\nas\shared\yield.csv` has no drive letter, so `wsl_path` falls
/// through to its pass-through arm and hands the agent `//nas/shared/yield.csv` — a path
/// that exists in no Linux filesystem. The turn then fails minutes later with
/// `FileNotFoundError`, naming neither the share nor the reason, and the researcher has no
/// way to guess that the network drive was the problem.
///
/// Said at the moment of the drop instead, it is a sentence they can act on: copy it to the
/// machine first. Mapping the share inside the distro is the other fix and is not one to
/// suggest to someone who does not code.
pub(crate) fn wsl_can_open(path: &Path) -> bool {
    !path.to_string_lossy().replace('\\', "/").starts_with("//")
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
    /// Command + args that must finish *before* [`Self::launch_command`] — mirroring the
    /// bundled source, installing what the lock names, generating the config. Run only on the
    /// spawn path, and awaited without a health budget over it (see [`LaunchPlan`]).
    pub prepare_command: Option<Vec<String>>,
    /// When set, never spawn — just talk to a backend someone else is running.
    pub attach_only: bool,
    /// File the sidecar's stdout/stderr is written to.
    pub log_path: PathBuf,
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
    /// Build the configuration used at real startup, reading Settings.
    pub fn load() -> Self {
        let settings = crate::settings::Settings::load();
        let mut config = Self::with_recorded_dir(&settings);
        // Settings lose to an explicit environment variable, which is the debugging
        // escape hatch, but win over the built-in default.
        if std::env::var_os("MINIME_BACKEND_PORT").is_none() {
            config.port = settings.backend_port;
        }
        // The launch command embeds both the port and the execution environment, so it is
        // rebuilt rather than patched.
        let plan = launch_plan_for(
            &config.project_dir,
            config.port,
            config.wsl.as_ref(),
            settings.approve_execute,
            settings.async_subagents,
            config.owned,
            Some(&settings.model_spec()),
        );
        config.launch_command = plan.serve;
        config.prepare_command = plan.prepare;
        config.default_model = Some(settings.model_spec());
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
        let plan = launch_plan_for(&project_dir, port, wsl.as_ref(), true, false, owned, None);
        Self {
            port,
            launch_command: plan.serve,
            prepare_command: plan.prepare,
            project_dir,
            wsl,
            attach_only: std::env::var_os("MINIME_BACKEND_ATTACH_ONLY").is_some(),
            log_path: default_log_path(),
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

/// The two halves of a launch: what has to finish first, and the server itself.
///
/// They were one shell line — `cd DIR && <prepare> exec langgraph dev` — and a single process.
/// That made the readiness budget for *booting a server* (60 seconds, see
/// [`BackendSupervisor::wait_until_healthy`]) also the budget for *installing its dependencies*,
/// which is a download measured in hundreds of megabytes whenever the lock moves. Separating
/// them is the whole fix: the install is awaited without a stopwatch and reported while it runs,
/// and only then is anything asked to answer a health check.
#[derive(Clone, Debug)]
pub struct LaunchPlan {
    /// Run to completion before [`LaunchPlan::serve`], and only when the backend is not already
    /// up. `None` when there is nothing to do.
    pub prepare: Option<Vec<String>>,
    /// The long-running server.
    pub serve: Vec<String>,
}

#[cfg(test)]
impl LaunchPlan {
    /// The `bash -lc` script of the preparation step, or empty when there is none.
    fn prepare_script(&self) -> String {
        self.prepare
            .as_ref()
            .and_then(|argv| argv.last())
            .cloned()
            .unwrap_or_default()
    }

    /// The `bash -lc` script that starts the server.
    fn serve_script(&self) -> String {
        self.serve.last().cloned().unwrap_or_default()
    }

    /// Both halves, for assertions about something that must appear *somewhere* in the launch —
    /// and, more usefully, about something that must appear **nowhere** in it.
    fn both(&self) -> String {
        format!("{}\n{}", self.prepare_script(), self.serve_script())
    }
}

/// Build the launch argv.
///
/// Prefer the checkout's own venv entry point over `uv run langgraph`: `uv run`
/// **forks** the real server as a grandchild, so killing our direct child leaves
/// an orphaned server holding the port (observed in P6.2). Invoking the venv
/// binary directly keeps it a single process we actually own.
// These inputs cross separate launch boundaries (filesystem, WSL, execution policy,
// approval, concurrency and ownership), and the call sites deliberately spell every
// choice out. Bundling them would hide the security-relevant defaults just to satisfy
// a numeric style limit (the explicit-boundary rule from docs §41 and §96).
#[allow(clippy::too_many_arguments)]
fn launch_plan_for(
    project_dir: &Path,
    port: u16,
    wsl: Option<&WslTarget>,
    approve_execute: bool,
    async_subagents: bool,
    owned: bool,
    // The `"provider::model_id"` the researcher chose, for calls that arrive with no run
    // config. See `model_env` — without it the backend reaches for an OpenAI default.
    default_model: Option<&str>,
) -> LaunchPlan {
    if let Some(wsl) = wsl {
        // The `wsl.exe … bash -lc` wrapper is identical for both halves of the plan; only the
        // script differs. Built once so the two can never disagree about which distro they mean.
        let mut wrapper = vec!["wsl.exe".to_string()];
        if let Some(distro) = &wsl.distro {
            wrapper.push("-d".into());
            wrapper.push(distro.clone());
        }
        // Go through a login shell so PATH/uv are set up as the user's own shell
        // would have them, and `exec` so the shell is *replaced* by the server —
        // otherwise killing our child leaves the real process behind.
        //
        // Bind 0.0.0.0, not 127.0.0.1: WSL2's localhost forwarding reliably
        // reaches services bound to all interfaces, while loopback-only binds
        // are not always visible from Windows.
        wrapper.push("--".into());
        wrapper.push("bash".into());
        wrapper.push("-lc".into());
        // Host execution needs a couple of variables set *inside* the distro, so they go
        // in the command line rather than on `wsl.exe`'s own environment.
        let mut exports = String::new();
        for (name, value) in execution_env(true, approve_execute)
            .into_iter()
            .chain(feature_env(async_subagents))
            .chain(model_env(default_model))
        {
            exports.push_str(&format!("{name}={} ", shell_quote(&value)));
        }
        // The config we generate from upstream's just before launch (docs §30). `&&`, so a
        // generator failure stops the launch instead of silently starting a server whose
        // coordinator holds tools pointing at a graph nobody serves.
        //
        // **Generated on every launch, not only when background work is on (§303).** This was
        // gated on `async_subagents`, and `make_config.py` writes *two* things: the `background`
        // graph, and — the only place in the product that does — the `checkpointer` key.
        // Upstream's `langgraph.json` has none. So with background work off, which is the
        // **default**, `langgraph dev` ran with no checkpointer at all and conversations were
        // never written to disk. A researcher's laptop listed threads out of
        // `.langgraph_ops.pckl` and had nothing behind any of them.
        //
        // Nothing about durable storage belongs behind a preview feature flag, and the two are
        // now unbound: `MINIME_ASYNC_SUBAGENTS` still decides whether background work is *on*
        // (`async_agents.install` returns early without it), so declaring the graph with the flag
        // off costs one import at startup and enables nothing.
        //
        // **The trade this makes, stated.** `&&` now applies to every launch rather than to the
        // async ones only, so a generator failure stops the backend for everyone. That is
        // deliberate: `make_config.py` fails when the checkout has no `graphs` object or when
        // upstream has grown a `background` graph of its own, and a backend that cannot be
        // configured correctly must not start quietly — starting quietly without persistence is
        // the whole of §303. The failure lands in the sidecar log, where Setup already points.
        // **Joined with `&&`, and run before the server rather than in front of it.** Each step
        // below either tolerates its own failure internally (`|| true`) or is one the backend
        // must not start without, so `&&` says exactly the right thing: the mirror and the
        // checkpointer nudge cannot stop a launch, while a failed dependency install or a failed
        // config generation can and should. See [`sync_dependencies_command`] for what happened
        // when this shared a process — and a 60-second health budget — with `langgraph dev`.
        let mut prepare: Vec<String> = Vec::new();
        // First, so the generated config is written over a checkout that is already current.
        // Only for a checkout the app owns: someone who pointed us at their own clone gets to
        // keep it — same rule the version pin had, and the only part of it worth keeping.
        if owned {
            if let Some(bundled) = bundled_backend_dir() {
                prepare.push(sync_source_command(&wsl_path(&bundled), &wsl.dir));
                // Immediately after the mirror, which is what brings the new `uv.lock` in, and
                // before anything reads the environment it describes.
                prepare.push(sync_dependencies_command(&wsl.dir));
            }
        }
        // Durable conversation storage, for installs that were provisioned before it existed.
        // New ones get it from `setup-wsl.sh`; this is how the researchers already using the
        // app stop paying for a pickle store without having to be told about one (docs §96).
        // After the sync, because `uv sync` prunes anything the lock does not name.
        if owned {
            prepare.push(ensure_checkpointer_command());
        }
        // Run every launch, not gated on any feature: the generator writes the `checkpointer`
        // key upstream's own `langgraph.json` has none of, and it is what makes conversations
        // survive a restart at all (docs §303).
        prepare.push(generate_config_command(".venv/bin/python"));
        let config_flag = format!(" --config {GENERATED_CONFIG}");

        // `quote_path`, not `shell_quote`: the default is `~/Mini-Me`, and quoting the tilde
        // would stop it expanding. A configured dir with a space in it used to split into a
        // bogus command.
        let dir = quote_path(&wsl.dir);
        let prepare = (!prepare.is_empty()).then(|| {
            let mut argv = wrapper.clone();
            argv.push(format!("cd {dir} && {}", join_prepare_steps(&prepare)));
            argv
        });
        let mut serve = wrapper;
        serve.push(format!(
            "cd {dir} && {exports}exec .venv/bin/langgraph dev --host 0.0.0.0 \
             --port {port}{config_flag} --no-reload --no-browser --n-jobs-per-worker {jobs}",
            jobs = JOBS_PER_WORKER,
        ));
        return LaunchPlan { prepare, serve };
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
    // Nothing to prepare on the host path: it launches a checkout the researcher maintains
    // themselves, and mirroring or syncing someone else's working clone is not ours to do.
    LaunchPlan {
        prepare: None,
        serve: argv,
    }
}

/// Open a log for appending, keeping one previous file when it grows past the cap.
///
/// **Because `File::create` truncates, and that is how the evidence keeps disappearing.** Both
/// logs this app writes were opened that way, and both destroyed the run before them:
///
/// - a researcher's backend log held a single line — a later spawn had wiped the failing one, and
///   the answer to "why did it exit" was gone before anyone could read it;
/// - an app log arrived with timestamps out of order (16:22:20 above 16:22:09), because two app
///   instances had each truncated the same file and overwritten the other's region. Read as one
///   process it says something impossible.
///
/// Appending fixes both: concurrent writers interleave instead of clobbering, and a spawn cannot
/// erase the one before it. The cap is what makes that safe to leave on — past `LOG_MAX_BYTES` the
/// file is rolled to `<name>.old`, so there is always at least one full previous run to read and
/// never unbounded growth on a machine nobody administers (§305).
pub fn open_log_appending(path: &std::path::Path) -> std::io::Result<File> {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > LOG_MAX_BYTES) {
        // Best-effort: a failed roll must not cost the log itself, which is the whole point.
        let _ = std::fs::rename(path, path.with_extension("old"));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

/// How large a log may grow before the previous one is rolled aside.
///
/// Eight megabytes holds many runs of an ordinarily quiet backend and is small enough that two of
/// them are unremarkable in `%TEMP%`.
const LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// How long the preparation step may run before the app stops waiting on it.
///
/// Not a health budget — it is a stuck-detector. The step it covers legitimately takes minutes
/// (a full `uv sync` moved 430 MB on a researcher's machine at 440 KiB/s), and killing a working
/// download is exactly the failure this whole change exists to remove. Half an hour is long
/// enough that reaching it means something is wrong, and short enough that the app does not hang
/// forever if it is.
const PREPARE_MAX: Duration = Duration::from_secs(30 * 60);

/// The last thing the preparation step wrote, for showing the researcher that it is working.
///
/// Read from `from` — the offset the step's banner ended at — so this never reports the tail of
/// a previous run as current progress. Bounded: the log is appended to across launches and may
/// be megabytes, and this runs twice a second.
fn last_progress_line(path: &Path, from: u64) -> Option<String> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    const TAIL: u64 = 8 * 1024;

    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    // `from` wins over the tail window, so a step that has written nothing reads nothing — an
    // explicit `len <= from` guard above this was removed for being unreachable: seeking to
    // `from` on a file that has not grown past it already yields no bytes, and a branch no
    // mutation can reach is protection in appearance only.
    file.seek(SeekFrom::Start(from.max(len - len.min(TAIL))))
        .ok()?;
    let mut buf = Vec::new();
    file.by_ref().take(TAIL).read_to_end(&mut buf).ok()?;

    String::from_utf8_lossy(&buf)
        // `\r` as well as `\n`: a progress bar redraws in place, and a line that was never
        // terminated is still the most recent thing that happened.
        .split(['\n', '\r'])
        .map(|line| strip_ansi(line).trim().to_string())
        .rfind(|line| !line.is_empty())
        .map(|line| clip(&line, 90))
}

/// Drop the escape sequences a colouring tool writes, so they do not reach a UI label.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI sequences end at the first letter; that is enough for anything `uv` emits.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Shorten to fit a status line, counting characters rather than bytes.
fn clip(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }
    line.chars()
        .take(max.saturating_sub(1))
        .chain(['…'])
        .collect()
}

/// What the researcher reads while the preparation step runs.
///
/// The elapsed clock is the part that matters: without it a slow step and a wedged one look
/// identical, and the researcher's own report of this bug was *"It doesnt answer"*.
fn prepare_status(last: Option<&str>, elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let clock = if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    };
    match last {
        Some(line) => format!("{line} · {clock}"),
        None => format!("preparing the backend… {clock}"),
    }
}

/// Epoch milliseconds for the spawn banner.
///
/// Not a formatted date: this app carries no date library, and the banner's job is to separate one
/// spawn from the next and line up against `provenance::now_ms`, which is the same clock. A reader
/// comparing it to the tracing timestamps around it needs an ordering, not a calendar.
fn now_stamp() -> String {
    crate::provenance::now_ms().to_string()
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
/// The import check makes the common case one fast subprocess: after the first launch the
/// install never runs again. `|| true` throughout, because a backend that starts with the old
/// store is strictly better than one that does not start.
fn ensure_checkpointer_command() -> String {
    "{ .venv/bin/python -c 'import langgraph.checkpoint.sqlite' 2>/dev/null \
     || uv pip install langgraph-checkpoint-sqlite ; } >/dev/null 2>&1 || true"
        .to_string()
}

/// The graph id the background worker is served under.
///
/// Must match `BACKGROUND_GRAPH_ID` in `backend/local/async_agents.py` — the
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
///
/// Everything the server imports, mirrored from `mini-me/` — and nothing else.
///
/// `.venv` is built inside the distro and must never be copied over; `frontend/` is the web app.
const SOURCE_DIRS: [&str; 2] = ["backend", "skills"];
const SOURCE_FILES: [&str; 7] = [
    "langgraph.json",
    "pyproject.toml",
    "uv.lock",
    "conftest.py",
    "deepagents.toml",
    "mcp.json",
    ".python-version",
];

/// Bring the backend checkout up to the source this app ships, every launch.
///
/// **This replaces a `git fetch` that could never work.** The backend used to be updated by
/// fetching its own repository from inside WSL. Mini-Me is private, WSL holds no credentials for
/// it, and so the fetch either hung waiting for a sign-in nobody was watching (§131) or failed
/// fast and left the checkout a month behind while every log line looked healthy (§134). A merged,
/// pulled, verified fix took four test cycles to reach the machine, and never did on its own.
///
/// The source now ships in this repository, so the update is a file copy: no network, no token,
/// no second remote. **`git pull` on the app is the backend update**, which is what the
/// researcher asked for — *"so we dont need to pull and copy the backend"*.
///
/// The same lesson §25 records: a copy taken at
/// provisioning time goes stale, and the failure it produces is a fix that silently never runs.
///
/// # Why it is safe to run unconditionally
///
/// Each directory is staged beside its target and swapped in only once the copy has fully
/// succeeded, so an unreachable source — a Windows drive that is not mounted, which is exactly
/// what the in-distro copy exists to survive — leaves the working checkout untouched rather than
/// deleted. Stdout is discarded and **stderr is not**: a mirror that failed silently would be
/// this week's bug wearing a new coat.
///
/// `uv sync` runs only when `uv.lock` actually changed, compared against a stamp written after
/// the last successful sync. Without it a dependency added upstream would surface as an
/// ImportError at boot, which names the wrong problem.
fn sync_source_command(source: &str, backend_dir: &str) -> String {
    let dir = quote_path(backend_dir);
    let src = shell_quote(source.trim_end_matches('/'));

    // **Written out, one command per name, with no shell variable anywhere.** The first version
    // of this was a `for d in backend skills` loop, and on a real Windows machine `$d` arrived
    // *empty* — so `rm -rf {dir}/$d` was `rm -rf {dir}/` and every launch deleted the backend
    // checkout, `.venv` and conversation database included. The log said
    // `cp: cannot create directory '.../backend/..new'`, and `..new` is `.$d.new` with nothing
    // in the middle (docs §147).
    //
    // Why a loop cannot be trusted here is not fully explained, and that is exactly why this does
    // not use one: the same machine loses variables assigned inside `wsl bash -lc` when a command
    // is typed by hand too. Two names and seven files do not need iteration, and a literal name
    // cannot expand to nothing.
    let mut steps: Vec<String> = Vec::new();
    for name in SOURCE_DIRS {
        // Staged beside the target and swapped in only once the copy has succeeded, so an
        // unreachable source — an unmounted Windows drive — leaves the working checkout intact.
        // Every path here is a literal, so `rm -rf` can never be handed the directory itself.
        steps.push(format!(
            "[ -d {src}/{name} ] && rm -rf {dir}/.{name}.new \
             && cp -r {src}/{name} {dir}/.{name}.new \
             && rm -rf {dir}/{name} && mv {dir}/.{name}.new {dir}/{name}"
        ));
    }
    for name in SOURCE_FILES {
        steps.push(format!("[ -f {src}/{name} ] && cp {src}/{name} {dir}/"));
    }
    // Guarded on the checkout still being a checkout. Mirroring into a directory that has lost
    // its `pyproject.toml` is how a half-populated tree gets treated as current — and the whole
    // chain is `|| true`, so without this the damage stays silent until `cd` fails four commands
    // later with a message about the wrong thing.
    format!(
        "{{ [ -f {dir}/pyproject.toml ] || echo \
         'mini-me: the backend checkout looks incomplete — run Setup' >&2; \
         {steps}; }} >/dev/null || true",
        steps = steps.join("; "),
    )
}

/// Join the preparation steps so a failure in one actually stops the rest.
///
/// # Why each step is wrapped in its own group
///
/// `&&` and `||` have **equal precedence in the shell and associate left to right**, so a plain
/// `a && b && c` join does not mean what it looks like when the steps end in `|| true`:
///
/// ```text
/// { echo mirror; } || true && false || true && echo "generate ran anyway"
/// ```
///
/// prints `generate ran anyway` and exits **0**. The `|| true` belonging to the *overlay* step
/// rescues the *install* step that failed just before it, because what it is actually attached
/// to is the whole accumulated left-hand side. Measured, not reasoned about — the joined script
/// was printed and read, and the install's own missing `|| true` turned out to guarantee
/// nothing at all.
///
/// Bracing each step binds its fallback to itself: a best-effort step still cannot stop the
/// launch, and a step without a fallback finally can. This is the join, and
/// `a_failing_step_is_not_rescued_by_the_next_ones_fallback` runs it rather than reading it.
fn join_prepare_steps(steps: &[String]) -> String {
    steps
        .iter()
        .map(|step| format!("{{ {step}; }}"))
        .collect::<Vec<_>>()
        .join(" && ")
}

/// Install what the lock names, when the lock has moved.
///
/// # Why this is its own command, and not a step inside the mirror
///
/// It used to be the last step of [`sync_source_command`], which ends `>/dev/null || true`, on
/// the same shell line as `exec langgraph dev`. Both of those were wrong, and a researcher hit
/// them together the first time they opened v0.3.34:
///
/// - **One process.** The health poll starts when the process spawns, so its 60-second budget
///   covered the install. A lock change pulls ~430 MB — measured on a real machine at 440 KiB/s,
///   about eleven minutes — so the app gave up, the researcher pressed Restart, and the restart
///   killed the download that was in flight. `uv` keeps what finished, so each attempt got
///   further; none of them ever reached the 240 MB file at the end. The app was unusable and the
///   only visible symptom was that it "doesn't answer".
/// - **`|| true` over `>/dev/null`.** A failed install was indistinguishable from a successful
///   one, and surfaced later as an ImportError naming the wrong problem (F.1).
///
/// So: run to completion before the server is asked to exist, and **fail loudly**. The mirror
/// around it stays best-effort — an unmounted Windows drive must not stop a backend that is
/// already installed — but a dependency set that could not be installed is not a backend, and
/// starting one anyway is how §303 happened.
///
/// The stamp is written only after `uv sync` succeeds, so an interrupted install is retried on
/// the next launch rather than remembered as done.
fn sync_dependencies_command(backend_dir: &str) -> String {
    let dir = quote_path(backend_dir);
    // `echo` before the work, not after: this is the line the researcher reads while waiting,
    // and a message that only appears on completion explains a wait that has already ended.
    format!(
        "cmp -s {dir}/uv.lock {dir}/.mini-me-lock \
         || {{ echo 'mini-me: dependencies changed — installing them now. On a slow \
         connection this can take several minutes; the app is not stuck.' >&2; \
         (cd {dir} && uv sync --extra dev) && cp {dir}/uv.lock {dir}/.mini-me-lock; }}"
    )
}

/// Run the config generator, relative to the checkout root the launch has already `cd`ed into.
///
/// `backend/local/` is part of the checkout now (mirrored by [`sync_source_command`] along with
/// the rest of `backend/`), so there is no separate copy to resolve or prefer — the generator is
/// always the one the server itself will import.
fn generate_config_command(python: &str) -> String {
    format!("{python} \"backend/local/make_config.py\" .")
}

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
/// Separate from [`execution_env`] on purpose: the two settings are independent, and
/// keeping them apart is what lets either change without touching the other.
fn feature_env(async_subagents: bool) -> Vec<(String, String)> {
    if !async_subagents {
        return Vec::new();
    }
    vec![("MINIME_ASYNC_SUBAGENTS".to_string(), "1".to_string())]
}

/// The environment host execution needs. `for_wsl` selects how the workspace path is spelled.
fn execution_env(for_wsl: bool, approve: bool) -> Vec<(String, String)> {
    // Where a turn's files land. Chosen by the *app* rather than left to the backend's
    // default of `~/.mini-me/workspaces`, which inside WSL is a place a Windows researcher
    // cannot reach — see `workspace.rs` for why that one decision is what makes outputs
    // findable, downloadable and renderable at all.
    let workspace_root = crate::workspace::root();
    let workspace = if for_wsl {
        wsl_path(&workspace_root)
    } else {
        workspace_root.to_string_lossy().into_owned()
    };

    vec![
        (
            "MINIME_APPROVE_EXECUTE".to_string(),
            if approve { "1" } else { "0" }.to_string(),
        ),
        (crate::workspace::WORKSPACE_ENV.to_string(), workspace),
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
    ///
    /// Always host execution now — the remote sandbox option was removed.
    pub fn execution_label(&self) -> &'static str {
        "host (local)"
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

    /// Path to the backend's local-execution module, as the backend's own shell would open
    /// it — used by the preflight "local execution" check to confirm the checkout actually
    /// has it (see `preflight.rs`).
    pub fn local_execution_module(&self) -> String {
        format!("{}/backend/local/__init__.py", self.backend_dir().trim_end_matches('/'))
    }

    /// The provisioning command: `bash …/setup-wsl.sh <checkout>`, spelled for the
    /// backend's shell. Re-running it is safe — the script never overwrites a checkout
    /// or a `.env`.
    ///
    /// When a backend copy ships with the app, its path is passed in so the script
    /// provisions from it instead of cloning. That is the difference between an install
    /// a scientist can complete and one that stops at a GitHub token prompt, because
    /// Mini-Me is a private repository — the reason the backend is bundled as `mini-me/`
    /// in this repository rather than fetched at provision time.
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

    /// Whether the backend could open this path at all, once spelled its way.
    ///
    /// Asked before a dropped file becomes part of a question, because the alternative is a
    /// turn that runs for a minute and then reports a missing file (see [`wsl_can_open`]).
    /// Only WSL can fail this: a backend on this host reads the path the researcher's own
    /// file manager gave us.
    pub fn can_open(&self, path: &Path) -> bool {
        self.wsl.is_none() || wsl_can_open(path)
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

        let log = open_log_appending(&self.config.log_path).with_context(|| {
            format!(
                "could not open the sidecar log at {}",
                self.config.log_path.display()
            )
        })?;
        // **A banner, so two spawns in one file can be told apart.** Without it an appended log
        // reads as one confusing run; with it the reader can find the last `spawning` line and
        // know everything below it belongs to that attempt (§305).
        {
            use std::io::Write as _;
            let mut banner = log.try_clone().context("could not dup the sidecar log")?;
            let _ = writeln!(
                banner,
                "\n===== {} spawning {} on port {} =====",
                now_stamp(),
                self.config.location(),
                self.config.port
            );
        }
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
        // A usable stored or CLI-cached token beats a freshly minted one. Asta access tokens last
        // **seven days** (measured: `exp - iat` = 604800), while `--refresh` costs about ten
        // seconds on the startup path. `mint_asta_token` checks `exp` before paying that cost;
        // the name survives because refreshing is still its final fallback (§131/§145).
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
            for (name, value) in execution_env(false, self.config.approve_execute)
                .into_iter()
                .chain(feature_env(self.config.async_subagents))
                .chain(model_env(self.config.default_model.as_deref()))
            {
                command.env(name, value);
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

    /// Run the launch's preparation step to completion, reporting what it is doing.
    ///
    /// # Why this is awaited and not merely started
    ///
    /// Preparation is mostly instant — a file mirror and two guarded no-ops — right up until the
    /// lock moves, and then it is a several-hundred-megabyte download. There is no budget over
    /// it on purpose: the alternative was the 60-second health budget, which a researcher hit on
    /// the first launch of v0.3.34 and could not get past, because every retry killed the
    /// transfer that was in flight (see [`sync_dependencies_command`]).
    ///
    /// `progress` is called about twice a second with the last line the step wrote, so a
    /// ten-minute install reads as `Downloading nvidia-nccl-cu13 (240.7MiB) · 3m10s` rather than
    /// as a frozen window. The cap that remains is [`PREPARE_MAX`], which exists only so a
    /// genuinely wedged step ends in a sentence the researcher can act on.
    async fn prepare<P: FnMut(&str)>(&mut self, progress: &mut P) -> Result<()> {
        let Some(argv) = self.config.prepare_command.clone() else {
            return Ok(());
        };
        let (program, rest) = argv
            .split_first()
            .context("prepare_command must not be empty")?;

        let log = open_log_appending(&self.config.log_path).with_context(|| {
            format!(
                "could not open the sidecar log at {}",
                self.config.log_path.display()
            )
        })?;
        // Where this step's output starts, so `last_progress_line` reads what *it* wrote rather
        // than the tail of whatever ran before.
        let from = {
            use std::io::Write as _;
            let mut banner = log.try_clone().context("could not dup the sidecar log")?;
            let _ = writeln!(
                banner,
                "\n===== {} preparing {} =====",
                now_stamp(),
                self.config.location()
            );
            let _ = banner.flush();
            std::fs::metadata(&self.config.log_path).map_or(0, |meta| meta.len())
        };
        let log_err = log.try_clone().context("could not dup the sidecar log")?;

        tracing::info!(program = %program, "preparing the backend");
        let mut child = Command::new(program)
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .with_context(|| {
                format!(
                    "could not start the backend's preparation step in {}. Check that WSL is \
                     running (`wsl --status`).",
                    self.config.location()
                )
            })?;

        let started = Instant::now();
        loop {
            match child
                .try_wait()
                .context("could not poll the preparation step")?
            {
                Some(status) if status.success() => {
                    tracing::info!(seconds = started.elapsed().as_secs(), "backend prepared");
                    return Ok(());
                }
                // **Loud, where it used to be `>/dev/null || true`.** A dependency set that
                // could not be installed surfaces here, naming itself, instead of as an
                // ImportError at boot naming the wrong problem (F.1).
                Some(status) => anyhow::bail!(
                    "preparing the backend failed with {status}. The last thing it wrote was: \
                     {last}\n\nThe full output is in {log}. You can run it by hand with:\n    \
                     wsl bash -lc \"cd {dir} && uv sync --extra dev\"",
                    last = last_progress_line(&self.config.log_path, from)
                        .unwrap_or_else(|| "nothing".into()),
                    log = self.config.log_path.display(),
                    dir = self.config.wsl.as_ref().map_or("<backend>", |wsl| &wsl.dir),
                ),
                None => {}
            }
            if started.elapsed() > PREPARE_MAX {
                let _ = child.kill();
                anyhow::bail!(
                    "the backend's preparation step has run for {minutes} minutes without \
                     finishing. The last thing it wrote was: {last}\n\nIt is most likely still \
                     downloading. Run it in a terminal where you can watch it:\n    \
                     wsl bash -lc \"cd {dir} && uv sync --extra dev\"",
                    minutes = PREPARE_MAX.as_secs() / 60,
                    last = last_progress_line(&self.config.log_path, from)
                        .unwrap_or_else(|| "nothing".into()),
                    dir = self.config.wsl.as_ref().map_or("<backend>", |wsl| &wsl.dir),
                );
            }
            progress(&prepare_status(
                last_progress_line(&self.config.log_path, from).as_deref(),
                started.elapsed(),
            ));
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Ensure *something* healthy is listening: attach if it is already up,
    /// otherwise spawn and wait. Returns a status string for the UI.
    pub async fn ensure_running(&mut self, client: &LangGraphClient) -> Result<Started> {
        self.ensure_running_with(client, &mut |_| {}).await
    }

    /// As [`Self::ensure_running`], but reporting what the preparation step is doing.
    ///
    /// Separate rather than a parameter on the one method because three of the four callers have
    /// nowhere to put a progress line — they run before there is a turn to attach it to. The one
    /// that does is the turn itself, which is also the one a researcher is sitting in front of.
    /// Generic over the callback rather than taking `&mut dyn FnMut`: a trait object is `!Send`
    /// whatever it holds, which would have made *every* caller's future `!Send`. Three of them
    /// are spawned on the runtime and need `Send`; the fourth is the turn, whose closure holds
    /// `RefCell`s and can never be `Send`. Generics let each call site be judged on what it
    /// actually passes.
    pub async fn ensure_running_with<P: FnMut(&str)>(
        &mut self,
        client: &LangGraphClient,
        progress: &mut P,
    ) -> Result<Started> {
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
            // **The one case the source mirror cannot reach.** It runs as part of the launch
            // command, so a server left over from a previous session has already imported
            // whatever it imported and nothing here can change that — and `langgraph dev`
            // survives the app closing, so this is the usual case rather than an edge one.
            // Said plainly, with what to do about it, because the researcher just ran `git pull`
            // and has every reason to believe the new code is running (docs §130).
            tracing::warn!(
                "attached to a backend that was already running — it is on the code it started \
                 with, not what this app now ships. To pick up a backend change, close this app \
                 and run: wsl bash -lc \"pkill -f 'langgraph dev'\""
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
        // **Before `start`, and not on the same line as it.** Everything the server needs to
        // exist — the mirrored source, the installed dependencies, the generated config — is
        // finished and checked here, so the budget below covers only what it is named for.
        self.prepare(progress).await?;
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
    /// Spawned by this app, so it is running the backend checkout this app shipped.
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
    /// Stop the backend this app started, and any `langgraph dev` left in the distro.
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
            return;
        }
        // Nothing of ours to reap, but the researcher may still be attached to one someone
        // else's session left behind — which is the case that needs this most.
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
    let refresh = if refresh { " --refresh" } else { "" };
    let argv = config.shell_argv(&format!("asta auth print-token --raw{refresh} 2>/dev/null"));
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
            ("ui/components/button.rs", include_str!("ui/components/button.rs")),
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

    /// **Conversations are saved whether or not background work is on (§303).**
    ///
    /// The bug this pins was invisible from either side on its own. `make_config.py` writes two
    /// unrelated things — the `background` graph and the **only** `checkpointer` key in the
    /// product — and the launch passed `--config` only when `async_subagents` was true.
    /// `async_subagents` defaults to **false**. So an ordinary install ran upstream's
    /// `langgraph.json`, which declares no checkpointer, and `langgraph dev` kept every
    /// conversation in memory. A researcher's laptop listed threads out of `.langgraph_ops.pckl`
    /// with nothing behind any of them, and the four checks that exist all measure whether the
    /// *package* is installed — which it was.
    ///
    /// Asserted on the **launch argv** and not on `make_config.py`'s output, because the output
    /// was always correct: `the_generated_config_survives_being_run_as_a_script` passed
    /// throughout. What nothing asked was whether the launch uses it.
    #[test]
    fn conversations_are_saved_with_background_work_off() {
        let with_background_off = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            true,
            // The default, and the whole point: this is the ordinary install.
            false,
            true,
            None,
        );
        let serve = with_background_off.serve_script();
        let prepare = with_background_off.prepare_script();
        let command = with_background_off.both();
        let command = &command;

        assert!(
            serve.contains(&format!("--config {GENERATED_CONFIG}")),
            "a launch with background work off must still pass the generated config, or nothing \
             configures a checkpointer and conversations are never written: {serve}"
        );
        // **In the preparation step, which finishes before the server is spawned.** That
        // ordering used to be one `&&` on a shared command line; it is now the structure of the
        // launch itself, and `the_install_is_not_racing_the_health_check` is what holds it.
        assert!(
            prepare.contains("backend/local/make_config.py\" ."),
            "and the config has to be generated before it can be passed: {prepare}"
        );

        // **The feature stays off.** Unbinding the two must not switch background work on for
        // everybody: `async_agents.install` returns early unless this is set, so declaring the
        // graph costs one import and enables nothing (§114 is what a runaway would cost).
        assert!(
            !command.contains("MINIME_ASYNC_SUBAGENTS"),
            "background work must not be enabled by fixing persistence: {command}"
        );
    }

    /// The install is offered on a checkout the app provisioned, and never on someone else's.
    ///
    /// The rule that keeps this app welcome on a developer's own clone: it may run destructive
    /// or environment-changing commands only where it owns the environment (`resolve_project_dir`).
    /// Installing a package is exactly that kind of change, so it is gated on `owned` — and this
    /// pins the gate, because the cost of getting it wrong is silent and lands on somebody else's
    /// virtualenv (docs §96).
    #[test]
    fn the_checkpointer_install_is_gated_on_owning_the_checkout() {
        let launch = |owned: bool| {
            launch_plan_for(
                Path::new("/tmp/mini-me"),
                2024,
                Some(&WslTarget {
                    distro: None,
                    dir: "~/Mini-Me".into(),
                }),
                true,
                false,
                owned,
                None,
            )
            .prepare_script()
        };

        let ours = launch(true);
        assert!(ours.contains("langgraph-checkpoint-sqlite"), "{ours}");
        // Guarded by an import check, so the common case is one fast subprocess and the install
        // runs exactly once in the life of an installation.
        assert!(
            ours.contains("import langgraph.checkpoint.sqlite"),
            "{ours}"
        );
        // And it can never stop the backend starting: a server on the old store beats no server.
        assert!(ours.contains("|| true"), "{ours}");

        let theirs = launch(false);
        assert!(
            !theirs.contains("langgraph-checkpoint-sqlite"),
            "a checkout we do not own must be left alone: {theirs}"
        );
    }

    /// The generated config must extend upstream's, not replace it — and the generator must run
    /// **the way the launch command runs it**: as a script, with nothing arranged on `sys.path`.
    ///
    /// That last clause is the whole point. The first version of this test imported
    /// `make_config` as a module with its package root on `sys.path`, which passed while
    /// production was failing: the launch invokes `.venv/bin/python backend/local/make_config.py
    /// .`, and Python then puts the *script's own directory* on the path — `backend/local/`, not
    /// `backend/` above it. A `from backend.local import ...` at the top of the file therefore
    /// raised `ModuleNotFoundError`, the generator exited non-zero, and the `&&` in the launch
    /// expression stopped the backend from starting at all (docs §98).
    ///
    /// So this shells out to the file by path, exactly as `generate_config_command` does. A test
    /// that exercises a different invocation than production is not testing production.
    ///
    /// Skipped rather than failed when `python3` is absent, and it says so — a test that quietly
    /// covers nothing is one nobody notices has stopped (docs §81).
    #[test]
    fn the_generated_config_survives_being_run_as_a_script() {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../mini-me/backend/local/make_config.py");
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
    fn translates_windows_paths_for_wsl() {
        // The overlay lives in this repo — on Windows that means the Windows
        // filesystem, while the interpreter that imports it runs inside the distro.
        assert_eq!(
            wsl_path(Path::new(r"C:\Users\piero\mini-me-desktop\overlay")),
            "/mnt/c/Users/piero/mini-me-desktop/overlay"
        );
        assert_eq!(
            wsl_path(Path::new(r"D:\repos\overlay")),
            "/mnt/d/repos/overlay"
        );
        // A POSIX path is already what WSL wants.
        assert_eq!(
            wsl_path(Path::new("/home/piero/overlay")),
            "/home/piero/overlay"
        );
    }

    #[test]
    fn a_packaged_build_finds_its_files_beside_the_executable() {
        let _env = env_lock::hold();
        // A shipped copy must never reach back into a source tree that only exists on the
        // machine it was built on. `CARGO_MANIFEST_DIR` is baked in at compile time, so
        // without this the packaged app would look for its scripts under whatever path
        // the build machine happened to use.
        let exe = std::env::current_exe().expect("test binary path");
        let beside = exe.parent().expect("a parent").join("scripts");
        let _ = std::fs::remove_dir_all(&beside);

        std::env::remove_var("MINIME_SCRIPTS_DIR");
        let from_repo = resource("MINIME_SCRIPTS_DIR", "scripts");
        assert!(
            from_repo.ends_with("scripts") && !from_repo.starts_with(exe.parent().unwrap()),
            "with nothing beside the exe it falls back to the repo: {}",
            from_repo.display()
        );

        std::fs::create_dir_all(&beside).expect("packaged layout");
        assert_eq!(
            resource("MINIME_SCRIPTS_DIR", "scripts"),
            beside,
            "a directory beside the executable wins"
        );

        // An explicit override still beats both.
        std::env::set_var("MINIME_SCRIPTS_DIR", "/somewhere/else");
        assert_eq!(
            resource("MINIME_SCRIPTS_DIR", "scripts"),
            PathBuf::from("/somewhere/else")
        );
        std::env::remove_var("MINIME_SCRIPTS_DIR");
        let _ = std::fs::remove_dir_all(&beside);
    }

    #[test]
    fn a_joined_path_reads_like_a_path() {
        // Resolved as `crates/app/../../scripts`, and that spelling was showing up verbatim
        // in the log line and the Setup pane.
        assert_eq!(
            normalized(PathBuf::from("/repo/crates/app/../../scripts")),
            PathBuf::from("/repo/scripts")
        );
        // A `..` that would escape the root has nowhere to go and must stay put rather
        // than silently rewriting the path to something else.
        assert_eq!(normalized(PathBuf::from("/..")), PathBuf::from("/.."));
        assert_eq!(
            normalized(PathBuf::from("relative/../scripts")),
            PathBuf::from("scripts")
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
        let mut config = BackendConfig {
            wsl: Some(WslTarget {
                distro: Some("Ubuntu".into()),
                dir: "~/Mini-Me".into(),
            }),
            ..Default::default()
        };
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
        let config = BackendConfig {
            wsl: Some(WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            ..Default::default()
        };
        let command = config.setup_script();
        // The source now ships in this repository (`mini-me/`), so provisioning always has one
        // to copy from and the script never reaches GitHub for it. This assertion used to be
        // `starts_with("bash '")` — true only while a developer tree had no bundled copy, which
        // stopped being the case the moment the backend moved in here.
        assert!(command.contains("MINIME_BUNDLED_SOURCE="), "{command}");
        assert!(command.contains("bash '"), "{command}");
        assert!(command.contains("setup-wsl.sh"), "{command}");
        assert!(command.ends_with("~/'Mini-Me'"), "{command}");
    }

    /// The backend source is found in this repository, without an environment variable.
    ///
    /// **The point of the monorepo, as an assertion.** Provisioning and updates both hang off
    /// `bundled_backend_dir()`; if it stops finding `mini-me/`, the app silently falls back to
    /// cloning a *private* repository that WSL cannot authenticate to — which is the failure
    /// that cost §131 and §134, and it presents as a backend that simply never changes.
    #[test]
    fn the_backend_source_ships_in_this_repository() {
        let _env = env_lock::hold();
        std::env::remove_var("MINIME_BUNDLED_BACKEND");
        std::env::remove_var("MINIME_SOURCE_DIR");
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

    /// **A second launch does not erase the first one's log.**
    ///
    /// Both logs were opened with `File::create`, which truncates. Two failures came from that and
    /// neither looked like a logging problem:
    ///
    /// - a researcher's backend log held one line, so "why did it exit with 15" had no evidence
    ///   left to read — a later spawn had wiped it;
    /// - an app log arrived with timestamps out of order, 16:22:20 printed above 16:22:09, because
    ///   two app instances had each truncated the same file and overwritten the other's region.
    ///   Read as one process it describes something that cannot happen, and it was read that way.
    ///
    /// The cap is what makes appending safe to leave on: past `LOG_MAX_BYTES` the file rolls to
    /// `.old`, so a previous run always survives and nothing grows without bound (§305).
    #[test]
    fn a_second_launch_keeps_what_the_first_one_wrote() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!("minime-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("sidecar.log");

        {
            let mut first = open_log_appending(&path).expect("first open");
            writeln!(first, "the failing run said this").expect("write");
        }
        {
            let mut second = open_log_appending(&path).expect("second open");
            writeln!(second, "and then it was spawned again").expect("write");
        }

        let kept = std::fs::read_to_string(&path).expect("read");
        assert!(
            kept.contains("the failing run said this"),
            "the second open erased the evidence: {kept:?}"
        );
        assert!(kept.contains("and then it was spawned again"), "{kept:?}");

        // **Past the cap, one previous file survives — the run is not simply dropped.** A rotation
        // that deleted instead of renaming would be the same defect wearing a limit.
        std::fs::write(&path, vec![b'x'; (LOG_MAX_BYTES + 1) as usize]).expect("grow");
        {
            let mut after = open_log_appending(&path).expect("open after the cap");
            writeln!(after, "fresh").expect("write");
        }
        let rolled = path.with_extension("old");
        assert!(rolled.is_file(), "the oversized log must roll aside, not vanish");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read").trim(),
            "fresh",
            "the live log starts clean once it has rolled"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// **The release check accepts what the packager ships.**
    ///
    /// `release.sh` refused a correct bundle: it required `vendor/Mini-Me/langgraph.json`, the
    /// pre-monorepo path, and kept requiring it after §283 moved the backend to `mini-me/` and
    /// left `vendor/` as an empty compatibility directory. So the gate said *"the bundle cannot
    /// install itself"* about a bundle that installs fine, and the fix it printed rebuilt exactly
    /// the same thing.
    ///
    /// It stayed hidden because **CI never runs `release.sh`** — `release.yml` calls `package.sh`
    /// and uploads the result — so every tagged release worked while the documented local path
    /// was broken. Only someone releasing by hand ever met it, and that is the person least able
    /// to tell a stale check from a real one.
    ///
    /// The same shape as §283 twice over: two scripts each correct about its own job, nothing
    /// comparing them. So this compares them.
    #[test]
    fn the_release_check_accepts_the_layout_the_packager_writes() {
        let scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
        let packager = std::fs::read_to_string(scripts.join("package.sh")).expect("package.sh");
        let releaser = std::fs::read_to_string(scripts.join("release.sh")).expect("release.sh");

        // What the packager writes, read from the assignment rather than restated.
        assert!(
            packager.contains("BACKEND_DEST=\"$OUT/mini-me\""),
            "package.sh no longer writes mini-me/ — this test is checking the wrong thing"
        );
        assert!(
            releaser.contains("$BUNDLE/mini-me/langgraph.json"),
            "release.sh must accept the layout package.sh ships, or a correct bundle is refused"
        );

        // **And `vendor/` alone must not satisfy it.** `package.sh` creates an empty `vendor/`
        // with only a README so that a pre-§283 installer still accepts the download. A check
        // that treated the directory's existence as "the backend is here" would pass on a bundle
        // carrying no backend at all — which is the failure §283 shipped for a fortnight.
        assert!(
            !releaser.contains("[ -d \"$BUNDLE/vendor\" ]"),
            "an empty vendor/ is a compatibility shim, never evidence of a backend"
        );

        // Both names the installed app accepts, kept in step with `update::BUNDLE_BACKENDS`.
        for backend in crate::update::BUNDLE_BACKENDS {
            assert!(
                releaser.contains(backend),
                "release.sh never mentions {backend}, which the app accepts"
            );
        }
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
            "package.sh must stamp the bundle so setup-wsl.sh can compare builds"
        );

        let setup = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/setup-wsl.sh"),
        )
        .expect("the setup script is in this repo");
        assert!(
            setup.contains(".bundled-backend"),
            "setup-wsl.sh must read the stamp, or the bundle updates and the machine does not"
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
            wsl: None,
            project_dir: PathBuf::from("/opt/backend"),
            ..Default::default()
        };
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
    fn provisioning_keeps_a_windows_checkout_clean_for_git_inside_wsl() {
        let script = include_str!("../../../scripts/setup-wsl.sh");
        assert!(
            script.contains("git -C \"$DIR\" config core.autocrlf input"),
            "a copied Windows worktree needs a checkout-local CRLF policy inside WSL"
        );
        assert!(
            script.contains("git -C \"$DIR\" add --renormalize -- ."),
            "Git's cached clean filter must be refreshed after the policy changes"
        );
        assert!(
            !script.contains("git -C \"$DIR\" reset --hard"),
            "line-ending repair must not overwrite real work copied with a developer checkout"
        );
    }

    #[test]
    fn git_input_treats_a_windows_crlf_worktree_as_clean_inside_wsl() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: git is not on PATH");
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("minime-crlf-policy-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp repository");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&dir)
                .args(args)
                .output()
                .expect("git runs")
        };
        assert!(run(&["init", "-q"]).status.success());
        assert!(run(&["config", "user.email", "test@example.org"])
            .status
            .success());
        assert!(run(&["config", "user.name", "test"]).status.success());
        assert!(run(&["config", "core.autocrlf", "false"]).status.success());
        std::fs::write(dir.join("tracked.txt"), b"one\ntwo\n").expect("an LF source file");
        assert!(run(&["add", "tracked.txt"]).status.success());
        assert!(run(&["commit", "-qm", "base"]).status.success());

        // The bytes a clean Git-for-Windows checkout carries. Without the Windows global
        // policy, the Git process inside WSL sees both lines as edited.
        std::fs::write(dir.join("tracked.txt"), b"one\r\ntwo\r\n").expect("a CRLF worktree");
        assert!(
            !run(&["status", "--porcelain"]).stdout.is_empty(),
            "the fixture must reproduce the dirty checkout before testing the fix"
        );
        assert!(run(&["config", "core.autocrlf", "input"]).status.success());
        assert!(run(&["add", "--renormalize", "--", "."]).status.success());
        let status = run(&["status", "--porcelain"]);
        assert!(status.status.success());
        assert!(
            status.stdout.is_empty(),
            "the same CRLF bytes should be clean under the policy setup-wsl installs: {}",
            String::from_utf8_lossy(&status.stdout)
        );
        let _ = std::fs::remove_dir_all(&dir);
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
    fn local_execution_reaches_the_interpreter_inside_wsl() {
        let _env = env_lock::hold();
        // Pinned, or this test reads whichever `Documents` the machine running it has.
        // A Windows-shaped path on purpose: the distro cannot open `C:\…`, so the
        // translation to `/mnt/c/…` is the part that has to work.
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe {
            std::env::set_var(
                crate::workspace::WORKSPACE_ENV,
                r"C:\Users\Researcher\Documents\Mini-Me",
            )
        };
        let argv = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: Some("Ubuntu".into()),
                dir: "~/Mini-Me".into(),
            }),
            true,
            true,
            true,
            None,
        );
        let command = argv.serve_script();
        let command = &command;
        // Assignments must land *before* `exec`, or the server never sees them.
        let exec_at = command.find("exec ").expect("an exec");
        for assignment in [
            "MINIME_APPROVE_EXECUTE='1'",
            // The workspace the *app* chose, spelled the way the distro can open it —
            // this is what puts the researcher's outputs somewhere Explorer can reach
            // and the chat can render (docs §42).
            "MINIME_LOCAL_WORKSPACE='/mnt/c/Users/Researcher/Documents/Mini-Me'",
        ] {
            let at = command
                .find(assignment)
                .unwrap_or_else(|| panic!("{assignment} missing from: {command}"));
            assert!(at < exec_at, "{assignment} lands after exec: {command}");
        }
    }

    #[test]
    fn the_background_graph_id_is_the_same_on_both_sides() {
        // Two files name this id: here, `make_config.py` (which registers the graph) and
        // `async_agents.py` (whose tool points at it). They only had a comment saying they
        // must agree — and a disagreement fails when the coordinator first delegates,
        // mid-task and in front of the user, rather than at startup. Now it is checked.
        //
        // Reading the sources rather than importing them keeps this a plain unit test; the
        // Python is not ours to run from here.
        let local = normalized(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mini-me/backend/local"),
        );
        for file in ["make_config.py", "async_agents.py"] {
            let path = local.join(file);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                source.contains(&format!("BACKGROUND_GRAPH_ID = \"{BACKGROUND_GRAPH_ID}\"")),
                "{} does not declare BACKGROUND_GRAPH_ID = {BACKGROUND_GRAPH_ID:?}",
                path.display()
            );
        }
    }

    /// The backend source is mirrored from this repository on every launch.
    ///
    /// **This is what a `git pull` has to be enough for.** It replaces a `git fetch` against
    /// Mini-Me's own remote, which is private and which WSL has no credentials for — so the
    /// update either hung waiting for a sign-in (§131) or failed fast and left the checkout a
    /// month behind while every log line read healthy (§134). A merged and verified fix took four
    /// test cycles to reach the machine and never arrived on its own.
    #[test]
    fn a_config_less_graph_build_never_reaches_for_a_provider_we_have_no_key_for() {
        // `backend/models.py` falls back to `openai::gpt-5.4` when `MINIME_DEFAULT_MODEL` is
        // unset, and this app deliberately keeps provider keys **out** of the environment. So
        // every call that builds the graph without a run config — `GET /threads/{id}/state`,
        // which the client polls while watching a background task — constructed an OpenAI client
        // with no key and returned 500. A background run finished and its result was unreadable
        // (docs §148).
        let _env = env_lock::hold();
        let wsl = WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        };
        let named = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            true,
            false,
            true,
            Some("anthropic::claude-sonnet-4-5"),
        );
        // Both, because the assertion that matters is the negative one below: a key must be
        // absent from every command the launch runs, not merely from the one being read.
        let command = named.both();
        let command = &command;
        assert!(
            command.contains("MINIME_DEFAULT_MODEL='anthropic::claude-sonnet-4-5'"),
            "{command}"
        );

        // The name and nothing else. A key on the backend's environment is readable by the
        // agent's own `execute` tool, which is the whole reason they ride in the run request.
        assert!(!command.contains("API_KEY="), "{command}");

        // A spec that is not a spec is not exported: half a variable would send the backend to a
        // provider named after the whole string, which fails later and less clearly than the
        // default it replaced.
        assert!(model_env(None).is_empty());
        assert!(model_env(Some("claude-sonnet-4-5")).is_empty());
        assert!(model_env(Some("   ")).is_empty());
    }

    #[test]
    fn every_launch_brings_the_backend_source_forward() {
        let _env = env_lock::hold();
        let wsl = WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        };
        let ours = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            true,
            false,
            true,
            None,
        );
        let command = ours.prepare_script();
        let command = &command;

        // Before the server — which is now a fact about *where* it runs rather than about two
        // offsets into one string. The server command does no mirroring at all.
        assert!(
            command.contains("rm -rf ~/'Mini-Me'/.backend.new"),
            "the mirror: {command}"
        );
        assert!(
            !ours.serve_script().contains(".backend.new"),
            "the server must not be mirroring source over itself: {}",
            ours.serve_script()
        );

        // **No shell variable, anywhere in the mirror.** A `for d in …` loop here arrived with
        // `$d` empty on a real Windows machine, which made `rm -rf {dir}/$d` into `rm -rf {dir}/`
        // — every launch deleted the checkout, `.venv` and conversation database included, and
        // the researcher saw only `exit code: 127` (docs §147). Two names do not need iteration.
        let mirror_end = command.find("cmp -s").expect("the lock check ends the mirror");
        let mirror_text = &command[..mirror_end];
        assert!(
            !mirror_text.contains('$'),
            "a variable in the mirror can expand to nothing: {mirror_text}"
        );

        // Staged beside the target and swapped in only once the copy succeeded. An unreachable
        // source — a Windows drive that is not mounted, the case the in-distro copies exist to
        // survive — must leave the working checkout alone, not delete it.
        let staged = command.find(".backend.new").expect("staged copy");
        let removed = command.find("&& rm -rf ~/'Mini-Me'/backend ").expect("swap");
        assert!(staged < removed, "the live copy is removed before the new one exists");

        // And the `rm -rf` targets are named files, never a computed path that could reduce to
        // the checkout root. This is the assertion that would have caught the deletion.
        assert!(
            !command.contains("rm -rf ~/'Mini-Me'/ ") && !command.contains("rm -rf ~/'Mini-Me';"),
            "the mirror can remove the checkout itself: {command}"
        );

        // `uv sync` only when the lock actually moved: otherwise every launch pays for it.
        assert!(command.contains("cmp -s"), "{command}");
        assert!(command.contains("uv sync --extra dev"), "{command}");

        // Never fatal, and **stderr is not discarded**: a mirror that failed silently would be
        // this week's bug in a new coat (§134).
        assert!(command.contains("|| true"), "{command}");
        assert!(!command.contains("2>/dev/null } "), "{command}");

        // A checkout somebody else owns is theirs. Same rule the version pin had, and the only
        // part of it worth keeping.
        let theirs = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            true,
            false,
            false,
            None,
        );
        assert!(
            !theirs.both().contains("for d in backend skills"),
            "{theirs:?}"
        );
    }

    #[test]
    fn background_work_registers_its_graph_before_the_server_starts() {
        let _env = env_lock::hold();
        let wsl = WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        };
        let argv = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            true,
            true,
            true,
            None,
        );
        // The generator runs *before* the server — if it fails the launch must stop, not start
        // a coordinator holding tools that point at a graph nobody serves.
        //
        // **How that is enforced changed, so this changed with it.** It was one `&&` on a shared
        // command line, checked here by comparing two offsets. The generator is now the last
        // step of the preparation command, whose steps are joined with `&&` and whose exit
        // status is checked in Rust before anything is spawned —
        // `a_failed_preparation_never_starts_a_server` is the test for that half.
        assert!(
            argv.prepare_script().contains("make_config.py"),
            "{:?}",
            argv.prepare
        );
        assert!(
            argv.prepare_script().contains(" && "),
            "the preparation steps must be joined so a failing one stops the rest: {}",
            argv.prepare_script()
        );
        let command = argv.serve_script();
        let command = &command;
        assert!(command.contains("exec .venv/bin/langgraph"), "{command}");
        assert!(
            command.contains("--config .mini-me-desktop.langgraph.json"),
            "{command}"
        );
        // Registering the graph is only half of it: without this variable
        // `async_agents.install` never installs the middleware, so the coordinator has no
        // `start_async_task` and quietly delegates to a normal subagent instead — which blocks
        // the chat, exactly what the feature exists to avoid. That was the first live result.
        assert!(command.contains("MINIME_ASYNC_SUBAGENTS='1'"), "{command}");

        // **With the feature off, only the feature is off.**
        //
        // This block used to read *"the launch is exactly what it always was"* and assert that
        // neither `make_config` nor `--config` appeared. It was faithful to its intent and the
        // intent was the defect: "what it always was" included **no checkpointer**, because
        // `make_config.py` is the only thing that writes that key and upstream's `langgraph.json`
        // does not have one. Every install with background work off — the default — kept its
        // conversations in memory and lost them on restart (§303).
        //
        // The two assertions are inverted rather than deleted, so the file records that this
        // pairing was once believed correct. What stays untouched is the third: unbinding
        // persistence from the feature must not switch the feature on.
        let plain = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&wsl),
            true,
            false,
            true,
            None,
        );
        let plain = plain.both();
        let plain = &plain;
        assert!(
            plain.contains("make_config"),
            "the config is generated whether or not background work is on: {plain}"
        );
        assert!(
            plain.contains("--config"),
            "and passed, or no checkpointer is ever configured: {plain}"
        );
        assert!(!plain.contains("MINIME_ASYNC_SUBAGENTS"), "{plain}");
    }

    /// **Installing the dependencies is not on the clock that boots the server.**
    ///
    /// This is the whole of the change, stated as a shape: `uv sync` is in the preparation
    /// command and the server is in the other one. While they shared a shell line they also
    /// shared [`BackendSupervisor::wait_until_healthy`]'s 60-second budget — and a lock change
    /// pulls hundreds of megabytes.
    ///
    /// The researcher's own report was *"It doesnt answer"*. Their log holds the proof: two
    /// spawn banners five minutes apart, the second re-listing exactly the two files the first
    /// had not finished (`nvidia-nccl-cu13` at 240.7 MiB and `xgboost` at 54.9 MiB). Every
    /// restart killed the transfer in flight, so no attempt could ever reach the end of the
    /// queue, and no amount of waiting or retrying would have helped.
    #[test]
    fn the_install_is_not_racing_the_health_check() {
        let _env = env_lock::hold();
        let plan = launch_plan_for(
            Path::new("/tmp/mini-me"),
            2024,
            Some(&WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            true,
            false,
            true,
            None,
        );

        let prepare = plan.prepare_script();
        let serve = plan.serve_script();

        assert!(
            prepare.contains("uv sync --extra dev"),
            "the install belongs to the step that is waited on: {prepare}"
        );
        assert!(
            !serve.contains("uv sync"),
            "the server command must not be able to start an install: {serve}"
        );
        assert!(serve.contains("exec .venv/bin/langgraph dev"), "{serve}");
        assert!(
            !prepare.contains("langgraph dev"),
            "the preparation step must not start a server: {prepare}"
        );

        // **And it is not silenced.** The install used to sit inside a `>/dev/null || true`
        // wrapper, so a failure was indistinguishable from success and surfaced later as an
        // ImportError naming the wrong thing (F.1). The mirror around it stays best-effort.
        // Asked of the builder's own output rather than of a slice of the joined script: the
        // step contains `&&` inside a subshell, so splitting the plan on `&&` cut it in half and
        // the assertions below were reading a fragment. Checking the two together also pins that
        // the plan uses this command verbatim rather than a re-spelling of it.
        let install = sync_dependencies_command("~/Mini-Me");
        assert!(
            prepare.contains(&install),
            "the plan must run the install command as built: {prepare}"
        );
        let install = install.as_str();
        assert!(
            !install.contains("|| true"),
            "a failed install must stop the launch: {install}"
        );
        assert!(
            !install.contains(">/dev/null"),
            "a failed install must leave something to read: {install}"
        );
        // The researcher reads this while they wait, so it has to be written before the work
        // rather than after it.
        assert!(
            install.contains("the app is not stuck"),
            "the wait needs to explain itself: {install}"
        );
        // Stamped only on success, so an interrupted install is retried rather than remembered
        // as done — which is what turned one long download into an unbounded number of them.
        let stamped = install.find(".mini-me-lock;").expect("the stamp");
        assert!(
            install.find("uv sync").expect("the sync") < stamped,
            "{install}"
        );
    }

    /// A step that failed is not rescued by the next step's fallback.
    ///
    /// **Run, not read.** The joined script is shell, and the bug this pins is a shell
    /// precedence rule: `&&` and `||` bind equally and associate leftwards, so the `|| true`
    /// ending a best-effort step attaches to everything before it — including a failed install.
    /// Asserting that the install command contains no `|| true` was true and useless; the
    /// script still exited 0. Only executing it says which.
    #[test]
    fn a_failing_step_is_not_rescued_by_the_next_ones_fallback() {
        let run = |steps: &[&str]| {
            let script =
                join_prepare_steps(&steps.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&script)
                .output()
                .expect("sh")
                .status
                .success()
        };

        // The shape of a real launch: a best-effort mirror, the install, a best-effort overlay
        // copy, and the config generator.
        assert!(
            !run(&[
                "{ echo mirror; } >/dev/null || true",
                "false",
                "{ echo overlay; } >/dev/null 2>&1 || true",
                "true",
            ]),
            "a failed install must stop the launch"
        );

        // And the best-effort steps keep their fallbacks: an unmounted Windows drive must not
        // stop a backend that is already installed.
        assert!(
            run(&[
                "{ false; } >/dev/null || true",
                "true",
                "{ false; } >/dev/null 2>&1 || true",
                "true",
            ]),
            "a best-effort step that failed must not stop the launch"
        );

        // The generator is last, and it is not best-effort: a config that could not be written
        // must not become a server started without one (§303).
        assert!(
            !run(&["{ echo mirror; } >/dev/null || true", "true", "false"]),
            "a failed config generator must stop the launch"
        );
    }

    /// A preparation step that failed never becomes a server that half works.
    ///
    /// The other half of `background_work_registers_its_graph_before_the_server_starts`: that
    /// one pins the shell `&&` joining the steps, this one pins that their exit status is
    /// actually read. Before the split it was one process, so the shell enforced both; now the
    /// second half is Rust and needs its own test.
    #[tokio::test]
    async fn a_failed_preparation_never_starts_a_server() {
        let log = std::env::temp_dir().join(format!(
            "mini-me-prepare-{}.log",
            crate::provenance::now_ms()
        ));
        let _ = std::fs::remove_file(&log);

        // The lock covers reading the environment and nothing more. Held across the awaits
        // below it would be a `std` guard on a future that the runtime may resume anywhere,
        // which is the one thing this lock must never become.
        let _env = env_lock::hold();
        let config = BackendConfig {
            log_path: log.clone(),
            attach_only: false,
            // Nothing listens here, so `ensure_running` cannot attach and has to take the spawn
            // path.
            port: 1,
            // Stands in for a `uv sync` that could not reach the network.
            prepare_command: Some(vec![
                "sh".into(),
                "-c".into(),
                "echo 'Downloading nvidia-nccl-cu13 (240.7MiB)'; \
                 echo 'error: failed to fetch' >&2; exit 1"
                    .into(),
            ]),
            // If this ever runs, the test has failed: it would hold the process for ten minutes.
            launch_command: vec!["sh".into(), "-c".into(), "sleep 600".into()],
            ..BackendConfig::default()
        };
        let ok = BackendConfig {
            log_path: log.clone(),
            prepare_command: Some(vec!["sh".into(), "-c".into(), "exit 0".into()]),
            ..BackendConfig::default()
        };
        drop(_env);

        let mut supervisor = BackendSupervisor::new(config);
        let client = LangGraphClient::new("http://127.0.0.1:1");
        let error = supervisor
            .ensure_running_with(&client, &mut |_| {})
            .await
            .expect_err("a failed preparation must stop the launch");
        let error = format!("{error:#}");

        assert!(
            supervisor.child.is_none(),
            "the server must not be spawned when its dependencies could not be installed"
        );
        // The message has to carry what actually went wrong and what to do about it — the
        // failure this replaces produced no message at all.
        assert!(error.contains("error: failed to fetch"), "{error}");
        assert!(error.contains("uv sync --extra dev"), "{error}");

        // And a step that succeeds is simply not in the way.
        let mut supervisor = BackendSupervisor::new(ok);
        supervisor
            .prepare(&mut |_| {})
            .await
            .expect("a preparation step that succeeds is not an error");

        let _ = std::fs::remove_file(&log);
    }

    /// While it runs, the app says what it is doing — and how long it has been doing it.
    ///
    /// Without the clock a slow step and a wedged one look identical, which is how a working
    /// eleven-minute download got reported as *"It doesnt answer"* and restarted four times.
    #[test]
    fn the_wait_says_what_it_is_waiting_for() {
        assert_eq!(
            prepare_status(None, Duration::from_secs(9)),
            "preparing the backend… 9s"
        );
        assert_eq!(
            prepare_status(
                Some("Downloading nvidia-nccl-cu13 (240.7MiB)"),
                Duration::from_secs(191)
            ),
            "Downloading nvidia-nccl-cu13 (240.7MiB) · 3m11s"
        );

        // Read from the offset the step started at, so the tail of a *previous* launch is never
        // reported as current progress. The log is appended to across launches (§305), which is
        // exactly what makes this necessary.
        let path = std::env::temp_dir().join(format!(
            "mini-me-progress-{}.log",
            crate::provenance::now_ms()
        ));
        std::fs::write(&path, "old run: Application started up\n").expect("write");
        let from = std::fs::metadata(&path).expect("stat").len();
        assert_eq!(last_progress_line(&path, from), None);

        // **The case that distinguishes the offset from reading the whole file.** The step has
        // started and written only a blank line; the honest answer is "nothing yet", not the
        // last line of the run before it. Without this the offset could be dropped entirely and
        // every assertion here would still pass — which is what the first version of this test
        // did (`the_endpoint_table_was_actually_found` exists for the same reason).
        std::fs::write(&path, "old run: Application started up\n\n\n").expect("write");
        assert_eq!(
            last_progress_line(&path, from),
            None,
            "a blank line from this step must not surface the previous run's tail"
        );

        std::fs::write(
            &path,
            "old run: Application started up\nResolved 259 packages\n\u{1b}[2mDownloading \
             xgboost (54.9MiB)\u{1b}[0m\n",
        )
        .expect("append");
        assert_eq!(
            last_progress_line(&path, from).as_deref(),
            Some("Downloading xgboost (54.9MiB)"),
            "the newest line, with its colouring stripped"
        );

        // A partial line still counts: a progress bar redraws in place and never sends a newline,
        // and "nothing has happened" is the one thing this must not say while it is happening.
        std::fs::write(&path, "Resolved 259 packages\rDownloaded 12 of 259").expect("write");
        assert_eq!(
            last_progress_line(&path, 0).as_deref(),
            Some("Downloaded 12 of 259")
        );
        let _ = std::fs::remove_file(&path);

        assert_eq!(clip("abc", 90), "abc");
        assert_eq!(clip(&"x".repeat(200), 5).chars().count(), 5);
    }

    #[test]
    fn quotes_paths_that_contain_spaces() {
        // "Documents\My Repos\..." is entirely normal on Windows, and an unquoted
        // assignment would silently split into a bogus command.
        let quoted = shell_quote("/mnt/c/Users/a b/overlay");
        assert_eq!(quoted, "'/mnt/c/Users/a b/overlay'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
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
        // Lifted out of `backend/local/__init__.py` by source rather than imported: importing
        // `backend.local` first runs `backend/__init__.py`, which needs dotenv/langchain/langgraph
        // installed — this function itself needs nothing but the standard library.
        let local_init = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../mini-me/backend/local/__init__.py");
        let read = |dir: &std::path::Path| -> String {
            let source = std::fs::read_to_string(&local_init)
                .expect("backend/local is beside the crate");
            let start = source
                .find("def _checkout_version")
                .expect("_checkout_version is gone from backend/local/__init__.py");
            let end = source
                .find("\ndef install")
                .expect("the next function");
            let script = format!(
                "from pathlib import Path\n{}\nprint(_checkout_version({dir:?}))",
                &source[start..end],
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
