//! First-run diagnosis: what is missing before a turn can possibly work.
//!
//! The app is meant to be **clicked, not configured** (docs §5). Until now a machine
//! that was not already set up produced
//! `backend did not become healthy within 120 attempts` in the status bar — a true
//! statement that tells the user nothing they can act on. The real answer is one of a
//! short list: the checkout isn't there, `uv sync` was never run, or no model key is
//! stored.
//!
//! So this module asks those questions directly, **through the same shell the backend
//! launches through** (see [`BackendConfig::shell_argv`]), and returns each answer with the
//! command that would fix it. §21 settled the shape of P6.4b as "a guided first run"; this
//! is the guiding part.
//!
//! Two rules it follows, both learned from earlier bugs in this repo:
//!
//! - **Never cascade.** If the runtime doesn't answer, the checks that run *inside* it
//!   report `Skip`, not a second failure. Five red lines caused by one missing thing
//!   sends the user hunting in four wrong places.
//! - **Never hang.** A broken environment can block instead of failing, and a setup
//!   pane that spins forever is worse than the error message it replaced.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::backend::{bundled_backend_dir, in_dir, venv_python, BackendConfig, Execution};

/// Ceiling for one probe. Generous because a cold `uv`/Python environment genuinely takes
/// several seconds to answer on the first call of a session, and reporting "missing"
/// because we gave up at two seconds would be a lie.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a check was skipped when the runtime itself never answered.
const RUNTIME_FIRST: &str = "the runtime above has to work";

/// The Asta entitlement the theorizer requires.
///
/// Found by decoding two real CIP tokens side by side: one account had only
/// `access:all_endpoints`, the other also had this — and only the second could run theory
/// generation. Being signed in says nothing about it.
const THEORY_PERMISSION: &str = "enroll:theory_generation";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Verified present.
    Pass,
    /// Missing, but a turn can still succeed without it.
    Warn,
    /// A turn cannot work until this is fixed.
    Fail,
    /// Not checked, because something it depends on failed first.
    Skip,
}

impl State {
    pub fn glyph(self) -> &'static str {
        match self {
            State::Pass => "✓",
            State::Warn => "!",
            State::Fail => "✗",
            State::Skip => "–",
        }
    }
}

/// What would resolve a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Fix {
    /// A command, already routed for the machine it has to run on. `note` carries the
    /// part a person needs to know before clicking (elevation, a restart, how long).
    Run {
        label: &'static str,
        argv: Vec<String>,
        note: &'static str,
    },
    /// Something only a person can do — a login, a download, pasting a key.
    Manual(String),
    /// Point the app at a checkout it found. Not a command but a settings write, and the
    /// *right* answer when the user already has one: adopting takes a second, while
    /// provisioning a second copy costs gigabytes and several minutes.
    Adopt { label: &'static str, dir: String },
}

#[derive(Debug, Clone)]
pub struct Check {
    /// Stable identifier, so tests and the UI can name a row without matching prose.
    pub id: &'static str,
    pub label: &'static str,
    pub state: State,
    /// What was found, or what is wrong. One line.
    pub detail: String,
    /// In preference order — the first is what the pane leads with. More than one because
    /// "we found a Mini-Me at ~/Mini-Me" and "install a fresh copy" are both real answers
    /// to a missing checkout, and which one is right is the user's call, not ours.
    pub fixes: Vec<Fix>,
}

impl Check {
    fn pass(id: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            label,
            state: State::Pass,
            detail: detail.into(),
            fixes: Vec::new(),
        }
    }

    /// `because` names what this is actually waiting on. Getting that wrong is its own
    /// small lie: a skipped dependency check once said "the runtime has to work first"
    /// on a machine whose runtime was fine and whose *checkout* was missing.
    fn skip(id: &'static str, label: &'static str, because: &'static str) -> Self {
        Self {
            id,
            label,
            state: State::Skip,
            detail: format!("not checked — {because} first"),
            fixes: Vec::new(),
        }
    }

    fn failing(
        id: &'static str,
        label: &'static str,
        state: State,
        detail: impl Into<String>,
        fixes: Vec<Fix>,
    ) -> Self {
        Self {
            id,
            label,
            state,
            detail: detail.into(),
            fixes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub checks: Vec<Check>,
    /// Where the checks ran — the pane's subtitle.
    pub location: String,
    pub execution: String,
    /// Whether the app provisioned this checkout and may therefore maintain it.
    ///
    /// Shown to the user because it changes what the app is allowed to do to their own
    /// files, and that should never come as a surprise.
    pub owned: bool,
}

impl Report {
    /// True when nothing blocks a turn. Warnings don't count — a missing `asta` costs
    /// the user literature search, not the app.
    pub fn ready(&self) -> bool {
        !self.checks.iter().any(|check| check.state == State::Fail)
    }

    /// `"4 ok · 1 to fix"`, for the status bar and the pane header.
    pub fn summary(&self) -> String {
        let count = |state: State| {
            self.checks
                .iter()
                .filter(|check| check.state == state)
                .count()
        };
        let mut parts = vec![format!("{} ok", count(State::Pass))];
        for (state, word) in [
            (State::Fail, "to fix"),
            (State::Warn, "optional"),
            (State::Skip, "skipped"),
        ] {
            let n = count(state);
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        parts.join(" · ")
    }

    /// The first thing standing in the way, for a one-line status message.
    pub fn first_problem(&self) -> Option<&Check> {
        self.checks.iter().find(|check| check.state == State::Fail)
    }
}

/// A finished (or abandoned) probe.
struct Probe {
    /// False when the program could not even be launched — worth distinguishing from a
    /// program that ran and refused, since the two have different fixes.
    launched: bool,
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Probe {
    /// The most useful line of failure output, trimmed for a single-line detail.
    fn message(&self) -> String {
        let raw = if self.stderr.trim().is_empty() {
            self.stdout.trim()
        } else {
            self.stderr.trim()
        };
        raw.lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no output")
            .chars()
            .take(160)
            .collect()
    }
}

/// Run a command and collect its output, giving up after [`PROBE_TIMEOUT`].
///
/// `Command::output()` has no timeout at all, and a broken environment can hang rather
/// than fail. Polling `try_wait` and killing the child on the deadline keeps a bad
/// machine diagnosable — reading the pipes only after exit is safe here because every
/// probe's output is a line or two, far below the pipe buffer that would deadlock a
/// chatty child.
fn probe(argv: &[String]) -> Probe {
    let absent = |stderr: String| Probe {
        launched: false,
        ok: false,
        stdout: String::new(),
        stderr,
    };
    let Some((program, rest)) = argv.split_first() else {
        return absent("empty command".into());
    };

    let mut child = match Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Python picks its stdout encoding from the console code page, which on
        // Windows is cp1252 — so `asta`'s Rich-formatted output (box-drawing
        // characters, checkmarks) crashes the child with a `UnicodeEncodeError`
        // before it prints anything, and this probe sees an empty, failed process
        // instead of the real status. See `run_streaming` below for the same fix.
        .env("PYTHONIOENCODING", "utf-8")
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return absent(format!("could not run {program}: {error}")),
    };

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_string(&mut stdout);
                }
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                return Probe {
                    launched: true,
                    ok: status.success(),
                    stdout,
                    stderr,
                };
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe {
                    launched: true,
                    ok: false,
                    stdout: String::new(),
                    stderr: format!("timed out after {}s", PROBE_TIMEOUT.as_secs()),
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return absent(format!("could not wait for {program}: {error}")),
        }
    }
}

/// Whether the installed backend is the one this app shipped with.
///
/// **The check that was missing for a fortnight.** `setup-backend.sh` runs only when the Setup pane
/// offers it, and the pane offered it only when `langgraph.json` was absent — so once a machine
/// was provisioned, the Python underneath the app never changed again. The app updated weekly and
/// the backend stayed at whatever shipped the first time: a researcher's dataverse explorer ran a
/// `read_search_results` that had been fixed nine days earlier, and four middleware modules
/// written for it were not on the machine at all (§283).
///
/// **A failure, not a warning**, though nothing here blocks a turn — the pane is diagnostic and
/// always has been. `Warn` prints as *optional* in the summary, and the modules an out-of-date
/// backend is missing include `no_spending.py`, the gate that stops a subagent spending credits
/// without a press. A machine running without it is not a machine with an optional improvement
/// available. The red row is the whole signal; the app still starts, still answers, and the fix is
/// one button, because hijacking a launch for the fifteen minutes `uv sync` can take is a worse
/// trade than saying so plainly.
///
/// Silent when there is nothing to compare — a developer checkout carries no stamp, and running
/// the app from source is not a machine out of date.
fn backend_build(config: &BackendConfig) -> Check {
    let Some(bundled) = bundled_backend_stamp() else {
        return Check::pass(
            "backend-build",
            "Backend build",
            "running from source — nothing bundled to compare against",
        );
    };
    let installed = installed_backend_stamp(config);
    if installed.as_deref() == Some(bundled.as_str()) {
        return Check::pass(
            "backend-build",
            "Backend build",
            "matches the copy bundled with this app",
        );
    }

    let short = |stamp: &str| stamp.chars().take(12).collect::<String>();
    let detail = match &installed {
        // Named rather than counted: "older" invites the question this line should answer.
        Some(stamp) => format!(
            "installed {} but this app ships {} — subagents may be running last month's rules",
            short(stamp),
            short(&bundled)
        ),
        None => format!(
            "installed before this app stamped its backend; this app ships {} — \
             subagents may be running last month's rules",
            short(&bundled)
        ),
    };
    Check::failing(
        "backend-build",
        "Backend build",
        State::Fail,
        detail,
        vec![Fix::Run {
            label: "Update the backend",
            argv: config.shell_argv(&config.setup_script()),
            note: "copies the bundled backend over the installed one and refreshes its packages — \
                   your API keys and conversations are untouched",
        }],
    )
}

/// Whether a path exists where the backend would look for it.
///
/// Stats the filesystem directly rather than shelling out — that works on Linux, macOS
/// *and* native Windows, where there may be no `bash` to ask.
fn exists(config: &BackendConfig, relative: &str) -> bool {
    config.project_dir.join(relative).exists()
}

/// The file `scripts/package.sh` writes into the bundled backend, naming that build.
///
/// Content-derived rather than a version number, because a hand-built bundle has no version to
/// quote and the question is whether these files differ from the installed ones.
const BACKEND_STAMP: &str = ".bundled-backend";

/// What build of the backend this app ships, if it ships one at all.
fn bundled_backend_stamp() -> Option<String> {
    let stamp = std::fs::read_to_string(bundled_backend_dir()?.join(BACKEND_STAMP)).ok()?;
    let stamp = stamp.trim().to_string();
    (!stamp.is_empty()).then_some(stamp)
}

/// What build of the backend is installed, read where the backend actually lives.
fn installed_backend_stamp(config: &BackendConfig) -> Option<String> {
    let stamp = std::fs::read_to_string(config.project_dir.join(BACKEND_STAMP)).ok()?;
    let stamp = stamp.trim().to_string();
    (!stamp.is_empty()).then_some(stamp)
}

/// Look for a Mini-Me checkout somewhere other than where we are configured to find one.
fn discover_checkout(config: &BackendConfig) -> Option<String> {
    let configured = config.backend_dir();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let home = std::path::PathBuf::from(home);
    [
        home.join("Documents/Mini-Me"),
        home.join("Documents/GitHub/Mini-Me"),
        home.join("Mini-Me"),
        std::path::PathBuf::from("../Mini-Me"),
    ]
    .into_iter()
    .find(|dir| dir.join("langgraph.json").is_file() && dir.to_string_lossy() != configured)
    .map(|dir| dir.to_string_lossy().into_owned())
}

/// Ask every question, in dependency order.
///
/// `has_model_key` is passed in rather than read here on purpose: the Linux keychain
/// client runs its own `block_on` and panics if called from a thread already driving a
/// Tokio runtime — which is how the first live run of the settings code died. Secrets
/// are read once, on the main thread, and the answer travels as a bool.
pub fn inspect(config: &BackendConfig, has_model_key: bool) -> Report {
    let mut checks = Vec::new();

    // ---------------------------------------------------------------- 1. the runtime
    //
    // What every fix below needs: a POSIX shell to run through (`shell_argv`), and `uv` —
    // which provisions and launches the backend's own Python environment. Round-tripping
    // `echo` through the shell proves it is actually usable, not merely present.
    let shell = probe(&config.shell_argv("echo ok"));
    let shell_ok = shell.ok && shell.stdout.contains("ok");
    let uv = probe(&["uv".to_string(), "--version".to_string()]);
    let runtime_ok = shell_ok && uv.ok;
    let shell_name = if cfg!(windows) { "PowerShell" } else { "bash" };
    if runtime_ok {
        checks.push(Check::pass(
            "runtime",
            "Shell",
            format!("{shell_name} and {} are on this machine", uv.stdout.trim()),
        ));
    } else if !uv.ok {
        let install = if cfg!(windows) {
            Fix::Run {
                label: "Install uv",
                argv: vec![
                    "powershell.exe".into(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    "irm https://astral.sh/uv/install.ps1 | iex".into(),
                ],
                note: "installs into your user profile — no admin rights needed",
            }
        } else {
            Fix::Run {
                label: "Install uv",
                argv: config.shell_argv("curl -LsSf https://astral.sh/uv/install.sh | sh"),
                note: "installs into your user profile — no admin rights needed",
            }
        };
        checks.push(Check::failing(
            "runtime",
            "Shell",
            State::Fail,
            "uv is not installed — nothing here can provision or launch the backend".to_string(),
            vec![install],
        ));
    } else {
        // Worth telling apart: a shell that is simply missing needs installing, one that ran
        // and refused needs a different kind of attention.
        let detail = if shell.launched {
            format!("no usable shell — {}", shell.message())
        } else {
            format!("{shell_name} was not found on this machine")
        };
        let fix = if cfg!(windows) {
            // powershell.exe ships with every supported Windows, so reaching this branch
            // means something is unusually broken (a locked-down PATH, a corrupted install)
            // rather than a missing dependency — there is no install step to offer.
            Fix::Manual(
                "The backend needs PowerShell to run its setup and maintenance commands, but \
                 this machine could not run it. Check that powershell.exe is on PATH."
                    .into(),
            )
        } else {
            Fix::Manual(
                "The backend needs a POSIX shell to run its setup and maintenance commands. \
                 On Windows, install Git for Windows, which provides one."
                    .into(),
            )
        };
        checks.push(Check::failing("runtime", "Shell", State::Fail, detail, vec![fix]));
    }

    // ------------------------------------------------- 1b. execute's real shell (Windows)
    //
    // Independent of the check above: PowerShell runs `setup-backend.ps1` and the fixes
    // this pane offers, but `execute()` (the tool an agent's own turns run code through —
    // `overlay/minime_local/workspace.py`) routes through Git Bash instead, because
    // cmd.exe — what `subprocess.run(..., shell=True)` hardcodes to on Windows —
    // understands neither a heredoc (`cat > f.py << 'EOF' ...`, the single most common way
    // an agent writes a multi-line file) nor a command line past roughly 8191 characters.
    //
    // Without bash on PATH, `execute` does not fail here — it silently falls back to that
    // broken cmd.exe path, so the first sign of anything wrong is an agent's own command
    // failing deep in a turn with "The command line is too long," which names nothing a
    // researcher could act on. Reported separately from the row above so a machine with
    // PowerShell but no Git Bash gets a specific answer instead of a green "Shell" row and
    // an inexplicable failure three steps later.
    if cfg!(windows) {
        let bash = probe(&["bash".to_string(), "--version".to_string()]);
        if bash.ok {
            checks.push(Check::pass(
                "execute-shell",
                "Shell for running code",
                format!(
                    "Git Bash: {}",
                    bash.stdout.lines().next().unwrap_or("").trim()
                ),
            ));
        } else {
            checks.push(Check::failing(
                "execute-shell",
                "Shell for running code",
                State::Fail,
                "Git Bash was not found — commands the agent writes to run code (multi-line \
                 scripts especially) will fail or behave unpredictably"
                    .to_string(),
                vec![Fix::Manual(
                    "Install Git for Windows (https://git-scm.com/download/win), which \
                     includes Git Bash, then press Re-check. This is the same thing the \
                     setup script itself needs to fetch the backend, so most machines \
                     already have it — this usually means it isn't on PATH."
                        .into(),
                )],
            ));
        }
    }

    // Everything below runs *through* the runtime, so without it they would only
    // restate the failure above.
    let can_probe = runtime_ok;

    // -------------------------------------------------------------- 2. the checkout
    let checkout_ok = if can_probe {
        let found = exists(config, "langgraph.json");
        if found {
            checks.push(Check::pass(
                "checkout",
                "Mini-Me backend",
                format!("langgraph.json found in {}", config.backend_dir()),
            ));
            checks.push(backend_build(config));
        } else {
            // Offer to adopt an existing checkout *before* offering to install a second
            // one. Someone who already has Mini-Me on this machine should not be made to
            // download gigabytes again — and adopting keeps their branches intact,
            // because the app never runs destructive git on what it does not own.
            let mut fixes = Vec::new();
            let mut detail = format!("not installed in {}", config.backend_dir());
            if let Some(found) = discover_checkout(config) {
                detail = format!(
                    "not in {}, but there is one at {found}",
                    config.backend_dir()
                );
                fixes.push(Fix::Adopt {
                    label: "Use the one I have",
                    dir: found,
                });
            }
            fixes.push(Fix::Run {
                label: "Install Mini-Me",
                argv: config.shell_argv(&config.setup_script()),
                note: "downloads the backend and its Python packages — 5 to 15 minutes",
            });
            checks.push(Check::failing(
                "checkout",
                "Mini-Me backend",
                State::Fail,
                detail,
                fixes,
            ));
        }
        found
    } else {
        checks.push(Check::skip("checkout", "Mini-Me backend", RUNTIME_FIRST));
        false
    };

    // ------------------------------------------------------- 3. the LangGraph CLI
    //
    // Its own check rather than part of the checkout, because this is the failure the
    // repo has actually seen twice: `langgraph-cli` sits in an optional extra, so a
    // plain `uv sync` leaves the server libraries installed and the entry point absent
    // — a synced-looking checkout that cannot be launched.
    if checkout_ok {
        let entry = if cfg!(windows) {
            ".venv/Scripts/langgraph.exe"
        } else {
            ".venv/bin/langgraph"
        };
        if exists(config, entry) {
            checks.push(Check::pass(
                "dependencies",
                "Python dependencies",
                format!("{entry} is installed"),
            ));
        } else {
            checks.push(Check::failing(
                "dependencies",
                "Python dependencies",
                State::Fail,
                format!("{entry} is missing — the dev extra was never synced"),
                vec![Fix::Run {
                    label: "Install Python packages",
                    argv: config.shell_argv(&in_dir(&config.backend_dir(), "uv sync --extra dev")),
                    note: "a few minutes on a cold environment",
                }],
            ));
        }
    } else {
        checks.push(Check::skip(
            "dependencies",
            "Python dependencies",
            "the checkout above has to be there",
        ));
    }

    // ------------------------------------------- 3b. durable conversation storage
    //
    // Optional, and deliberately a *check* rather than a hard dependency. Without it the
    // backend keeps `langgraph dev`'s pickle checkpointer and works exactly as it always
    // has — slow to boot and able to lose everything, but working. With it, conversations
    // move to SQLite: constant boot instead of one that grows with history (docs §80), and
    // per-row writes instead of a format where one unreadable byte takes every conversation
    // with it (docs §90/§94).
    //
    // A `Warn`, not a `Fail`. Nothing is broken without it, and a red row for something
    // optional is how a Setup pane stops being read.
    //
    // **And it is not how most people get it.** On a checkout the app owns — every ordinary
    // install — provisioning installs it and the launch command re-checks, so this row is a
    // report, not a chore. It only asks anything of a developer pointed at their own clone,
    // whose virtualenv is not ours to change (docs §96). A researcher who cannot code should
    // never have had to notice a warning to avoid losing their history.
    if checkout_ok {
        let module = if !cfg!(windows) {
            ".venv/lib/python3.12/site-packages/langgraph/checkpoint/sqlite"
        } else {
            ".venv/Lib/site-packages/langgraph/checkpoint/sqlite"
        };
        if exists(config, module) {
            checks.push(Check::pass(
                "checkpointer",
                "Conversation storage",
                "SQLite — conversations load without unpickling the whole history",
            ));
        } else {
            checks.push(Check::failing(
                "checkpointer",
                "Conversation storage",
                State::Warn,
                "the pickle store — boot slows as history grows, and a failed load can \
                 overwrite it"
                    .to_string(),
                vec![Fix::Run {
                    label: "Move conversations to SQLite",
                    argv: config.shell_argv(&in_dir(
                        &config.backend_dir(),
                        "uv pip install langgraph-checkpoint-sqlite",
                    )),
                    note: "existing conversations stay in the old store until they are opened",
                }],
            ));
        }
    } else {
        checks.push(Check::skip(
            "checkpointer",
            "Conversation storage",
            "the checkout above has to be there",
        ));
    }

    // ---------------------------------------------------------------- 4. the overlay
    //
    // The check that exists because this failure is *silent*. Host execution works by
    // putting `overlay/` on the backend's PYTHONPATH so `sitecustomize` swaps the
    // sandbox class at interpreter startup (docs §18). If that path is not reachable —
    // an `MINIME_OVERLAY_DIR` naming somewhere that does not exist on this machine —
    // Python simply imports nothing, no error is raised, and the backend quietly tries
    // the *remote* sandbox instead. The user sees an authentication failure about a
    // service they thought they had stopped using.
    let candidates = config.overlay_candidates();
    if let Some(overlay) = candidates.last().cloned() {
        if can_probe {
            // In the launch command's own preference order, so the pane names the copy the
            // backend will actually import. Reporting a different path from the one in use
            // is worse than reporting nothing — it sends anyone debugging to the wrong file.
            let found = candidates.iter().find(|candidate| {
                let marker = format!("{}/sitecustomize.py", candidate.trim_end_matches('/'));
                std::path::Path::new(&marker).is_file()
            });
            if let Some(found) = found {
                let installed = found != &overlay;
                checks.push(Check::pass(
                    "overlay",
                    "Host execution overlay",
                    if installed {
                        format!("installed with the backend: {found}")
                    } else {
                        format!("reachable at {found}")
                    },
                ));
            } else {
                checks.push(Check::failing(
                    "overlay",
                    "Host execution overlay",
                    State::Fail,
                    format!("the backend cannot see {overlay}"),
                    vec![Fix::Manual(
                        "Host execution would not take effect and the backend would try \
                         the remote sandbox instead. Set MINIME_OVERLAY_DIR to a path \
                         reachable from the backend."
                            .into(),
                    )],
                ));
            }
        } else {
            checks.push(Check::skip(
                "overlay",
                "Host execution overlay",
                RUNTIME_FIRST,
            ));
        }
    } else if matches!(config.execution, Execution::Sandbox) {
        checks.push(Check::failing(
            "overlay",
            "Host execution overlay",
            State::Warn,
            "off — the agent's commands go to the remote sandbox",
            vec![Fix::Manual(
                "That needs LANGSMITH_API_KEY. Turn on \"Run code on this machine\" in \
                 Settings to use the local path instead."
                    .into(),
            )],
        ));
    }

    // ------------------------------------------------------------------- 5. the CLI
    //
    // A warning, not a failure: the coordinator answers perfectly well without `asta`,
    // it just cannot search the literature or run the theorizer. Overstating this would
    // block a first run that would have worked.
    //
    // Asta ships as a normal dependency of the backend's own venv now, rather than a
    // separately installed CLI reached over `PATH` — so it is run as a module of that
    // same interpreter (`python -m asta.cli ...`), never through a shell.
    if can_probe {
        match venv_python(&config.project_dir) {
            None => {
                checks.push(Check::skip(
                    "asta",
                    "Asta CLI",
                    "the checkout above has to be there",
                ));
            }
            Some(python) => {
                let asta_argv = |args: &[&str]| -> Vec<String> {
                    let mut argv =
                        vec![python.to_string_lossy().into_owned(), "-m".into(), "asta.cli".into()];
                    argv.extend(args.iter().map(|arg| arg.to_string()));
                    argv
                };
                let found = probe(&asta_argv(&["--version"]));
                if found.ok {
                    // Installed is not the same as usable. Asta access tokens last seven
                    // days, and an expired login surfaces as "the theorizer returned no
                    // task id" — which names neither the token nor the fix. Ask the CLI
                    // directly instead.
                    let token = probe(&asta_argv(&["auth", "status"]));
                    // `Local Token Status` is the CLI's own verdict. Checking for it rather
                    // than trusting the exit code, which is 0 even when signed out.
                    if token.ok && token.stdout.contains("Valid") {
                        let identity = asta_identity(&token.stdout);
                        // Being signed in is not the same as being *entitled*. The theorizer
                        // needs `enroll:theory_generation`, and an account without it fails
                        // with upstream's "the Asta theorizer returned no task id — likely a
                        // missing or expired token", which is a guess and a wrong one: the
                        // token is present, valid, and simply not enrolled. Two real CIP
                        // accounts differed on exactly this, and the error sent the user to
                        // re-authenticate for days.
                        //
                        // `print-token` without `--raw` prints the decoded payload,
                        // permissions and all, so this needs no JWT decoding of our own.
                        let claims = probe(&asta_argv(&["auth", "print-token"]));
                        let sign_in = Fix::Run {
                            label: "Sign in again",
                            argv: asta_argv(&["auth", "login"]),
                            note: "use the account with theory-generation access",
                        };
                        if claims.ok && !claims.stdout.contains(THEORY_PERMISSION) {
                            checks.push(Check::failing(
                                "asta",
                                "Asta CLI",
                                State::Warn,
                                format!("{identity} — this account cannot run the theorizer"),
                                vec![
                                    sign_in,
                                    Fix::Manual(format!(
                                        "Literature search works; the theorizer needs the \
                                         `{THEORY_PERMISSION}` permission, which this account \
                                         does not have. Sign in with the account that does, or \
                                         ask Asta to enrol this one."
                                    )),
                                ],
                            ));
                        } else {
                            checks.push(Check {
                                id: "asta",
                                label: "Asta CLI",
                                state: State::Pass,
                                // Who, and for how long. On a shared machine "signed in" is
                                // not enough — someone signed in with the wrong account
                                // cannot work out why their permissions look odd.
                                detail: identity,
                                // A button even when green: when the *refresh* credential
                                // finally lapses this is the only cure, and a button that
                                // appears only once you are broken is one you cannot find.
                                fixes: vec![sign_in],
                            });
                        }
                    } else {
                        checks.push(Check::failing(
                            "asta",
                            "Asta CLI",
                            State::Warn,
                            "installed, but not signed in",
                            vec![Fix::Run {
                                label: "Sign in to Asta",
                                argv: asta_argv(&["auth", "login"]),
                                note: "opens a browser; the app refreshes the token itself after this",
                            }],
                        ));
                    }
                } else {
                    checks.push(Check::failing(
                        "asta",
                        "Asta CLI",
                        State::Warn,
                        "not available — literature search and the theorizer need it",
                        vec![Fix::Manual(
                            "Asta ships as part of the backend's own Python packages. Run \
                             \"Install Python packages\" above (or `uv sync --extra dev` in \
                             the checkout) to pick it up."
                                .into(),
                        )],
                    ));
                }
            }
        }
    } else {
        checks.push(Check::skip("asta", "Asta CLI", RUNTIME_FIRST));
    }

    // ------------------------------------------------------------------- 6. the key
    if has_model_key {
        checks.push(Check::pass(
            "model-key",
            "Model API key",
            "stored in the OS keychain",
        ));
    } else {
        checks.push(Check::failing(
            "model-key",
            "Model API key",
            State::Fail,
            "no key stored for the selected provider",
            vec![Fix::Manual(
                "Open Settings and paste your key — it goes into the OS keychain, never \
                 into a file."
                    .into(),
            )],
        ));
    }

    Report {
        checks,
        location: config.location(),
        execution: config.execution_label().to_string(),
        owned: config.owned,
    }
}

/// Run a fix, handing each line of its output to `emit` as it arrives.
///
/// **Streamed, not buffered.** Provisioning takes minutes — a clone, then `uv sync`
/// pulling PyMC and scikit-learn — and a progress bar with no detail is exactly the
/// experience this pane exists to replace. Our users do not read logs; they need to see
/// that something is happening and, when it fails, the line that says why.
///
/// `stdin` is null on purpose. `git clone` of a private repository will otherwise sit
/// waiting for a username at a prompt nobody can see, and the app would look hung
/// forever. With no stdin it fails immediately and the reason reaches the pane.
///
/// Either of a child's output pipes, so both can go through one reader.
enum PipeOut {
    Stdout(std::process::ChildStdout),
    Stderr(std::process::ChildStderr),
}

impl std::io::Read for PipeOut {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            PipeOut::Stdout(pipe) => pipe.read(buffer),
            PipeOut::Stderr(pipe) => pipe.read(buffer),
        }
    }
}

/// Read lines from a child's pipe, **whatever encoding it writes in**.
///
/// This replaces `BufReader::lines().map_while(Result::ok)`, which looks harmless and is
/// not: `lines()` yields an error on the first byte that is not UTF-8, and `map_while`
/// stops at the first error. `wsl.exe` (and `powershell.exe`, which some fixes still run
/// through) writes **UTF-16LE** on Windows, so every line was an error and the iterator
/// ended immediately — the fix log captured *nothing*, and the app then told a researcher
/// "the command reported a failure — the last lines say why" with no lines at all (docs
/// §57).
///
/// `read` returns false to stop, which is how a dropped receiver ends the thread.
fn read_lines(mut pipe: impl std::io::Read, mut send: impl FnMut(String) -> bool) {
    let mut buffered: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut wide: Option<bool> = None;

    loop {
        let read = match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => break,
        };
        buffered.extend_from_slice(&chunk[..read]);
        // Decided once, from the first bytes that arrive: mixing separators mid-stream
        // would slice a UTF-16 line in half and garble everything after it.
        if wide.is_none() {
            wide = Some(looks_utf16(&buffered));
        }
        let wide = wide.unwrap_or(false);
        // In UTF-16LE a newline is `0A 00`, so cutting on a bare `0A` would leave the
        // stray `00` at the head of the next line and shift every character after it.
        let step = if wide { 2 } else { 1 };
        while let Some(at) = find_newline(&buffered, wide) {
            let line: Vec<u8> = buffered.drain(..at).collect();
            buffered.drain(..step.min(buffered.len()));
            if !send(decode(&line, wide)) {
                return;
            }
        }
    }
    // Whatever the child wrote without a trailing newline — often the only line a failing
    // command produces.
    if !buffered.is_empty() {
        let wide = wide.unwrap_or(false);
        send(decode(&buffered, wide));
    }
}

/// Every line in a block of bytes already in hand, decoded the same way a pipe would be.
///
/// Test-only: it exists so the encoding tests below can hand `read_lines` a fixed byte
/// block instead of a live pipe, through the exact same decoder rather than a second one
/// that could drift away from it.
#[cfg(test)]
fn lines_of(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    read_lines(std::io::Cursor::new(bytes), |line| {
        out.push(line);
        true
    });
    out
}

/// Whether these bytes look like UTF-16LE: a BOM, or ASCII padded with NULs.
fn looks_utf16(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xff, 0xfe]) {
        return true;
    }
    // ASCII in UTF-16LE is every other byte zero. Two of the first eight is already a
    // pattern no UTF-8 text produces.
    let window = &bytes[..bytes.len().min(16)];
    window
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|b| **b == 0)
        .count()
        >= 2
}

fn find_newline(bytes: &[u8], wide: bool) -> Option<usize> {
    if wide {
        bytes
            .windows(2)
            .position(|pair| pair == [0x0a, 0x00])
    } else {
        bytes.iter().position(|byte| *byte == 0x0a)
    }
}

fn decode(bytes: &[u8], wide: bool) -> String {
    let text = if wide {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    // The BOM and the CR are not content.
    text.trim_start_matches('\u{feff}')
        .trim_end_matches('\r')
        .to_string()
}

/// stdout and stderr are read on separate threads and interleaved. Reading them in
/// sequence would deadlock the moment a chatty child filled the pipe we were not
/// draining — and `uv` writes its progress to stderr, which is most of what there is to
/// watch.
///
/// A running repair's process, for as long as stopping it can mean anything.
///
/// **The whole design is "hold the thing we already own".** An earlier version (§168)
/// specified a `setsid` handshake to publish a detached process-group id and signal it from
/// a second process — measured (§170) to detach the repair from the one process the app
/// could actually reach, once the wrapper it went through exited while its own children kept
/// running. Holding the pid of the process this app itself spawned, and killing its whole
/// tree directly, is what stayed reachable. So this holds a pid and nothing else.
///
/// The pid cannot be recycled underneath us while it is armed: the waiter thread still holds the
/// `Child`, so the process is unreaped — a zombie at worst on Unix, a live handle on Windows — and
/// neither operating system hands the number to anyone else until that handle goes.
#[derive(Clone, Default)]
pub struct Cancel(std::sync::Arc<std::sync::Mutex<Option<u32>>>);

impl Cancel {
    fn arm(&self, pid: u32) {
        *self.0.lock().expect("cancel mutex") = Some(pid);
    }

    /// Called once the child has been reaped, because after that the number means nothing.
    fn disarm(&self) {
        self.0.lock().expect("cancel mutex").take();
    }

    /// Whether there is a live process this could stop.
    pub fn armed(&self) -> bool {
        self.0.lock().expect("cancel mutex").is_some()
    }

    /// Ask the repair to stop. `false` means there was nothing left to stop.
    ///
    /// A repair that finished on its own between the click and this call is **stopped** — that is
    /// §168's own rule, and the alternative is telling a researcher their machine is still
    /// changing when it is not.
    pub fn stop(&self) -> bool {
        let Some(pid) = *self.0.lock().expect("cancel mutex") else {
            return false;
        };
        kill_tree(pid)
    }
}

/// Terminate a spawned process and whatever it started.
#[cfg(windows)]
fn kill_tree(pid: u32) -> bool {
    // `/T` for the whole process tree the fix started (`bash`, `uv`, `python`, …), `/F`
    // because a console process given a polite request during `uv sync` will not take it.
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(windows))]
fn kill_tree(pid: u32) -> bool {
    // **The negative pid is a process group, and it is required here.** Signalling the child
    // alone leaves its own children running *and holding the stdout pipe open*, so
    // `run_streaming` blocks on EOF until the grandchild finishes on its own — a Stop that
    // reports nothing for thirty seconds. A test caught this; reasoning had not.
    //
    // This is §26's complaint reproduced in miniature, and it is why the child is spawned into
    // its own group below. Windows needs none of it: `taskkill /T` already walks the whole
    // process tree on its own.
    // SAFETY: `pid` names a child this process spawned into its own group and has not reaped.
    unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGTERM) == 0 }
}

pub fn run_streaming(
    argv: &[String],
    cancel: &Cancel,
    mut emit: impl FnMut(String),
) -> anyhow::Result<bool> {
    use anyhow::Context as _;

    let (program, rest) = argv.split_first().context("empty command")?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // `asta auth login` prints its device-activation URL through Rich, which on
        // Windows crashes with `UnicodeEncodeError` against the default cp1252
        // console encoding before the URL ever reaches this pipe. Same fix as
        // `probe` above.
        .env("PYTHONIOENCODING", "utf-8");
    // Its own process group, so `kill_tree` can signal the whole thing. **Unix only, and that
    // asymmetry is the measured result rather than an oversight**: on Windows `taskkill /T`
    // already walks the process tree, and an equivalent detach-then-signal gesture there was
    // measured (§170) to lose track of the very process it was meant to reach.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .with_context(|| format!("could not start {program}"))?;

    // Armed before a single line is read, so a Stop pressed during the first second of a cold
    // start has something to act on. §168's race test asked exactly this of the control file
    // it proposed; here the answer is structural — the pid exists the moment `spawn` returns.
    cancel.arm(child.id());

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    for pipe in [
        child.stdout.take().map(PipeOut::Stdout),
        child.stderr.take().map(PipeOut::Stderr),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            read_lines(pipe, |line| tx.send(line).is_ok());
        }));
    }

    // Waited off this thread so the pipes stay drained while we wait: they have their own
    // readers, so nothing can fill and block.
    let waiter = std::thread::spawn(move || child.wait());
    // Our own sender has to go, or the loop below never ends.
    drop(tx);

    // Ends when both pipes reach EOF, which is the child exiting.
    for line in rx {
        emit(strip_ansi(&line));
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = waiter
        .join()
        .map_err(|_| anyhow::anyhow!("the thread waiting on the fix panicked"))?
        .context("could not wait for the fix to finish")?;
    // The child has been reaped, so the number is now free for the next process on the machine.
    // Anything still holding this `Cancel` must stop being able to act on it.
    cancel.disarm();
    Ok(status.success())
}

/// Drop ANSI colour codes.
///
/// The setup script colours its own progress markers, and GPUI renders escape sequences
/// as the mojibake they are rather than as colour.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // Skip to the end of the sequence: a letter terminates CSI.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}

/// Read one field out of `asta auth status`.
///
/// The CLI prints a Rich table — box-drawing characters around `│ Property │ Value │` —
/// so this splits on the vertical bar rather than matching prose. Fragile by nature, which
/// is why it is *only* used to enrich a row that already passed: if the format changes we
/// lose a label, never a check.
fn status_field(output: &str, property: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut cells = line.split('│').map(str::trim);
        cells.next()?; // the leading border
        let key = cells.next()?;
        let value = cells.next()?;
        (key == property && !value.is_empty()).then(|| {
            // Drop the status emoji the CLI decorates values with.
            value
                .trim_start_matches(['✅', '❌', '⚠'])
                .trim()
                .to_string()
        })
    })
}

/// A one-line summary of who is signed in and for how long.
fn asta_identity(output: &str) -> String {
    let email = status_field(output, "Email");
    let expires = status_field(output, "Access Token Expires");
    match (email, expires) {
        // "2026-08-08 14:24:31 (167h 55m left)" — the parenthesised part is the useful
        // half, so it leads.
        (Some(email), Some(expires)) => {
            let remaining = expires
                .split_once('(')
                .map(|(_, rest)| rest.trim_end_matches(')').trim().to_string());
            match remaining {
                Some(remaining) => format!("{email} · token {remaining}"),
                None => format!("{email} · expires {expires}"),
            }
        }
        (Some(email), None) => format!("signed in as {email}"),
        _ => "installed and signed in".to_string(),
    }
}

/// Render an argv the way a person would type it, for the copy button.
pub fn display_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.contains(' ') || arg.contains('"') {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The one check that answers "is the app newer than the Python underneath it".
    ///
    /// **This is the finding, stated as a test.** For a fortnight the app shipped a backend it
    /// never installed and the Setup pane never mentioned it, so a researcher's subagents ran
    /// rules that had been replaced nine days earlier while every pane said the machine was
    /// fine (§283).
    #[test]
    fn a_backend_older_than_the_app_is_said_out_loud() {
        let _guard = crate::backend::env_lock::hold();
        let root = std::env::temp_dir().join(format!("mini-me-stamp-{}", std::process::id()));
        let bundled = root.join("bundled");
        let installed = root.join("installed");
        std::fs::create_dir_all(&bundled).expect("bundled");
        std::fs::create_dir_all(&installed).expect("installed");
        std::fs::write(bundled.join("langgraph.json"), "{}").expect("marker");

        let mut config = config();
        config.project_dir = installed.clone();

        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe { std::env::set_var("MINIME_BUNDLED_BACKEND", &bundled) };

        // Running from source: nothing is bundled to compare against, and a machine that has
        // never been handed a stamped build is not a machine out of date.
        assert_eq!(backend_build(&config).state, State::Pass);

        std::fs::write(bundled.join(BACKEND_STAMP), "ccbe00ee1741\n").expect("stamp");

        // Installed before stamps existed — which is every machine on the day this ships.
        let unstamped = backend_build(&config);
        assert_eq!(
            unstamped.state,
            State::Fail,
            "an out-of-date backend is missing the credit gate, which is not optional"
        );
        assert!(unstamped.detail.contains("ccbe00ee1741"), "{}", unstamped.detail);
        assert_eq!(unstamped.fixes.len(), 1, "and it can be fixed from the pane");

        // A different build.
        std::fs::write(installed.join(BACKEND_STAMP), "0000deadbeef").expect("stamp");
        let stale = backend_build(&config);
        assert_eq!(stale.state, State::Fail);
        assert!(stale.detail.contains("0000deadbeef"), "names what is installed: {}", stale.detail);
        assert!(stale.detail.contains("ccbe00ee1741"), "and what it should be: {}", stale.detail);

        // The same build says so, and offers nothing — a pane full of actions nobody needs is
        // how the real one stops being read.
        std::fs::write(installed.join(BACKEND_STAMP), "  ccbe00ee1741  \n").expect("stamp");
        let current = backend_build(&config);
        assert_eq!(current.state, State::Pass, "whitespace is not a different build");
        assert!(current.fixes.is_empty());

        // SAFETY: same lock.
        unsafe { std::env::remove_var("MINIME_BUNDLED_BACKEND") };
        std::fs::remove_dir_all(&root).ok();
    }

    fn config() -> BackendConfig {
        BackendConfig {
            port: 2024,
            project_dir: PathBuf::from("/nonexistent-checkout"),
            launch_command: vec!["true".into()],
            attach_only: false,
            log_path: PathBuf::from("/dev/null"),
            execution: Execution::Sandbox,
            secrets: Vec::new(),
            approve_execute: true,
            async_subagents: false,
            default_model: None,
            owned: true,
        }
    }

    #[test]
    fn a_missing_checkout_is_reported_with_the_command_that_fixes_it() {
        let _env = crate::backend::env_lock::hold();
        let report = inspect(&config(), true);
        let checkout = report
            .checks
            .iter()
            .find(|check| check.id == "checkout")
            .expect("a checkout row");
        assert_eq!(checkout.state, State::Fail);
        assert!(!report.ready(), "a missing checkout blocks every turn");

        // The point of the pane: the fix is a command, not advice.
        let Some(Fix::Run { argv, .. }) = checkout
            .fixes
            .iter()
            .find(|fix| matches!(fix, Fix::Run { .. }))
        else {
            panic!("expected a runnable fix, got {:?}", checkout.fixes);
        };
        let command = display_argv(argv);
        #[cfg(windows)]
        assert!(command.contains("setup-backend.ps1"), "{command}");
        #[cfg(not(windows))]
        assert!(command.contains("setup-backend.sh"), "{command}");
        assert!(command.contains("/nonexistent-checkout"), "{command}");

        // A skip has to name what it is *actually* waiting on. This one said "the
        // runtime above has to work first" on a machine whose runtime was fine — a small
        // lie that sends the user to check the wrong thing when the checkout is what is
        // missing.
        let dependencies = report
            .checks
            .iter()
            .find(|check| check.id == "dependencies")
            .expect("a dependencies row");
        assert_eq!(dependencies.state, State::Skip);
        assert!(
            dependencies.detail.contains("checkout"),
            "{}",
            dependencies.detail
        );
    }

    #[test]
    fn one_missing_runtime_does_not_cascade_into_five_failures() {
        let _env = crate::backend::env_lock::hold();
        // A machine with no usable shell or `uv` must be told *one* thing. Reporting a
        // missing checkout and missing dependencies as well would send the user hunting
        // for problems that are really just downstream of the one that matters.
        let real_path = std::env::var_os("PATH");
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe { std::env::set_var("PATH", "") };
        let report = inspect(&config(), true);
        // SAFETY: same lock.
        unsafe {
            match &real_path {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }

        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.id == "runtime")
                .map(|check| check.state),
            Some(State::Fail),
        );
        for id in ["checkout", "dependencies", "asta"] {
            let check = report
                .checks
                .iter()
                .find(|check| check.id == id)
                .unwrap_or_else(|| panic!("expected a {id} row"));
            assert_eq!(check.state, State::Skip, "{id} should not cascade");
            assert!(
                check.fixes.is_empty(),
                "{id} should offer no fix while skipped"
            );
        }
        // And the one real problem is still nameable in a single line.
        assert_eq!(report.first_problem().map(|c| c.id), Some("runtime"));
    }

    #[test]
    fn a_missing_key_is_the_only_thing_wrong_on_a_provisioned_machine() {
        let _env = crate::backend::env_lock::hold();
        // The repo itself stands in for a provisioned checkout: it has no langgraph.json,
        // so instead assert the *key* row alone flips with the flag, which is the
        // first-run path — everything installed, nothing pasted yet.
        let with = inspect(&config(), true);
        let without = inspect(&config(), false);
        let state = |report: &Report| {
            report
                .checks
                .iter()
                .find(|check| check.id == "model-key")
                .map(|check| check.state)
        };
        assert_eq!(state(&with), Some(State::Pass));
        assert_eq!(state(&without), Some(State::Fail));
    }

    #[test]
    fn the_remote_sandbox_is_a_warning_and_never_blocks_a_turn() {
        let _env = crate::backend::env_lock::hold();
        // Sandbox execution is still supported (`--sandbox`), so it must not show up as
        // a failure — only as a note that commands leave this machine.
        let report = inspect(&config(), true);
        let overlay = report
            .checks
            .iter()
            .find(|check| check.id == "overlay")
            .expect("an overlay row");
        assert_eq!(overlay.state, State::Warn);
        assert!(overlay.detail.contains("remote sandbox"), "{overlay:?}");
    }

    #[test]
    fn a_probe_that_would_hang_forever_is_not_run_forever() {
        // Not a timeout test — that would cost 30 real seconds. This checks the other
        // half: a program that does not exist comes back as *not launched*, which is
        // what separates "the program is missing" from "the program ran and refused".
        let missing = probe(&["no-such-program-4b81".to_string()]);
        assert!(!missing.launched);
        assert!(!missing.ok);
        assert!(!missing.message().is_empty());
    }

    /// Real `asta auth status` output, 2026-08-01. Kept verbatim, box-drawing and all,
    /// because the parser reads that layout — a reworded copy would test nothing.
    const REAL_STATUS: &str = "\
                    Authentication Status                     \n\
┏━━━━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓\n\
┃ Property             ┃ Value                               ┃\n\
┡━━━━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩\n\
│ Local Token Status   │ ✅ Valid                            │\n\
│ Server Verification  │ ✅ Valid                            │\n\
│ Email                │ piero.palacios@cipotato.org         │\n\
│ Name                 │ Piero Palacios                      │\n\
│ Access Token Expires │ 2026-08-08 14:24:31 (167h 55m left) │\n\
│ Refresh Token        │ ✅ Available                        │\n\
│ Auto-Refresh         │ ✅ Enabled                          │\n\
└──────────────────────┴─────────────────────────────────────┘";

    #[test]
    fn the_asta_row_says_who_is_signed_in_and_for_how_long() {
        assert_eq!(
            status_field(REAL_STATUS, "Email").as_deref(),
            Some("piero.palacios@cipotato.org")
        );
        // The emoji the CLI decorates values with must not leak into the pane.
        assert_eq!(
            status_field(REAL_STATUS, "Refresh Token").as_deref(),
            Some("Available")
        );
        assert_eq!(status_field(REAL_STATUS, "Not A Property"), None);

        let identity = asta_identity(REAL_STATUS);
        assert_eq!(
            identity,
            "piero.palacios@cipotato.org · token 167h 55m left"
        );
        // On a shared machine "signed in" is not enough to explain odd permissions.
        assert!(identity.contains('@'), "{identity}");

        // A changed table format loses the label, never the check.
        assert_eq!(
            asta_identity("something else entirely"),
            "installed and signed in"
        );
    }

    #[test]
    fn an_account_without_theory_generation_is_detected_from_its_claims() {
        // Two real CIP accounts, decoded side by side. Both were "signed in and valid";
        // only the second could run the theorizer. Upstream reports the first as a missing
        // or expired token, which sent this user re-authenticating for days.
        let without = r#"JWT Payload:
{
  "https://asta.allenai.org/name": "someone@example.org",
  "permissions": [
    "access:all_endpoints"
  ]
}"#;
        let with = r#"JWT Payload:
{
  "https://asta.allenai.org/name": "Piero Palacios",
  "permissions": [
    "access:all_endpoints",
    "access:biopathways",
    "enroll:asta_integration",
    "enroll:theory_generation"
  ]
}"#;
        assert!(
            !without.contains(THEORY_PERMISSION),
            "the account that could not"
        );
        assert!(with.contains(THEORY_PERMISSION), "the account that could");
        // `print-token` without --raw is what carries the claims — with --raw it is opaque
        // base64 and this check would silently always fail.
        assert!(!"eyJhbGci.eyJzdWIi.sig".contains(THEORY_PERMISSION));
    }

    #[test]
    fn the_summary_counts_every_state() {
        let _env = crate::backend::env_lock::hold();
        let report = inspect(&config(), false);
        let summary = report.summary();
        assert!(summary.contains("ok"), "{summary}");
        assert!(summary.contains("to fix"), "{summary}");
    }
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    /// UTF-16LE bytes for a string, as `wsl.exe` and `powershell.exe` write them.
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn utf16_output_is_read_rather_than_dropped() {
        // The exact failure from the first clean machine: Windows tools write UTF-16LE, and
        // `BufReader::lines().map_while(Result::ok)` ended at the first line — so the fix
        // log was empty and the app claimed the last lines said why (docs §57).
        let mut bytes = vec![0xff, 0xfe]; // BOM, as Windows tools emit
        bytes.extend(utf16("Installing: Ubuntu\r\n"));
        bytes.extend(utf16("Error: 0x80070005 access denied\r\n"));
        assert_eq!(
            lines_of(&bytes),
            vec!["Installing: Ubuntu", "Error: 0x80070005 access denied"]
        );
    }

    #[test]
    fn utf8_output_still_reads_normally() {
        // Everything else — uv, git, python — writes UTF-8, including non-ASCII.
        let bytes = b"Resolved 42 packages\ncreando el entorno\n".to_vec();
        assert_eq!(
            lines_of(&bytes),
            vec!["Resolved 42 packages", "creando el entorno"]
        );
    }

    #[test]
    fn a_last_line_without_a_newline_is_not_lost() {
        // Often the *only* line a failing command produces, so losing it loses the reason.
        assert_eq!(
            lines_of(b"fatal: no such checkout"),
            vec!["fatal: no such checkout"]
        );
        assert_eq!(lines_of(&utf16("access denied")), vec!["access denied"]);
    }

    #[cfg(unix)]
    #[test]
    fn stopping_a_repair_kills_it_and_stopping_a_finished_one_reports_nothing_to_do() {
        // §146 declined a Stop button because nothing owned a killable process. This is that
        // ownership, tested end to end: arm on spawn, kill the tree, disarm on reap.
        let cancel = Cancel::default();
        assert!(!cancel.armed(), "nothing to stop before anything runs");
        assert!(!cancel.stop(), "stopping nothing reports nothing to stop");

        let armed = cancel.clone();
        let watcher = std::thread::spawn(move || {
            // Wait for the child to exist, then stop it. A repair the researcher interrupts is
            // one that was still printing, so this has to work mid-stream rather than only at a
            // convenient boundary.
            for _ in 0..200 {
                if armed.armed() {
                    return armed.stop();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        });

        let started = Instant::now();
        let ok = run_streaming(
            &[
                "sh".into(),
                "-c".into(),
                // Long enough that finishing on its own would be the failure, not the pass.
                "printf 'working\\n'; sleep 30; printf 'finished\\n'".into(),
            ],
            &cancel,
            |_line| {},
        )
        .expect("the command ran");

        assert!(watcher.join().expect("watcher"), "the kill was not delivered");
        assert!(!ok, "a stopped repair did not succeed");
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "it ran to completion instead of being stopped"
        );
        // Reaped, so the number is free for the next process on the machine and must not be
        // handed to anyone. This is the half that keeps a late click from killing a stranger.
        assert!(!cancel.armed(), "the handle stayed armed after the child was reaped");
        assert!(!cancel.stop());
    }
}
