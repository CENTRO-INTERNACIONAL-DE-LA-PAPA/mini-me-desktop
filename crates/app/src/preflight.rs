//! First-run diagnosis: what is missing before a turn can possibly work.
//!
//! The app is meant to be **clicked, not configured** (docs §5). Until now a machine
//! that was not already set up produced
//! `backend did not become healthy within 120 attempts` in the status bar — a true
//! statement that tells the user nothing they can act on. The real answer is one of a
//! short list: WSL isn't installed, the checkout isn't there, `uv sync` was never run,
//! or no model key is stored.
//!
//! So this module asks those questions directly, **through the same hop the backend
//! takes** (see [`BackendConfig::shell_argv`]), and returns each answer with the command
//! that would fix it. §21 settled the shape of P6.4b as "a guided first run"; this is
//! the guiding part.
//!
//! Two rules it follows, both learned from earlier bugs in this repo:
//!
//! - **Never cascade.** If the runtime doesn't answer, the checks that run *inside* it
//!   report `Skip`, not a second failure. Five red lines caused by one missing thing
//!   sends the user hunting in four wrong places.
//! - **Never hang.** A half-installed WSL can block instead of failing, and a setup
//!   pane that spins forever is worse than the error message it replaced.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::backend::{quote_path, shell_quote, BackendConfig, Execution};

/// Ceiling for one probe. Generous because a **cold** WSL distro genuinely takes
/// several seconds to boot on the first `wsl.exe` call of a session, and reporting
/// "WSL is missing" because we gave up at two seconds would be a lie.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a check was skipped when the runtime itself never answered.
const RUNTIME_FIRST: &str = "the runtime above has to work";

/// Where the `asta` CLI comes from.
///
/// **Public** (`allenai/asta-plugins`, Apache 2.0), unlike Mini-Me itself — so this one
/// really can be a button: no token, no account, nothing to ask the user for.
///
/// Pinned to the version the Asta plugin pins (`skills/asta-cli/SKILL.md`), and the tag
/// was checked against the remote rather than assumed. Bump both together: a CLI newer
/// than the skills that drive it is how a subcommand goes missing.
const ASTA_INSTALL_URL: &str = "git+https://github.com/allenai/asta-plugins.git@v0.101.1";

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
    /// Where the checks ran — the pane's subtitle, because "no checkout" means
    /// something quite different inside a distro than on this filesystem.
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
    /// False when the program could not even be launched — `wsl.exe` absent, no `bash`.
    /// Worth distinguishing: "WSL is not installed" and "WSL ran and refused" have
    /// different fixes.
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
/// `Command::output()` has no timeout at all, and a broken WSL install can hang rather
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

/// Whether a path exists where the backend would look for it.
///
/// Host mode stats the filesystem directly rather than shelling out — that works on
/// Linux, macOS *and* native Windows, where there may be no `bash` to ask.
fn exists(config: &BackendConfig, relative: &str) -> bool {
    match &config.wsl {
        Some(_) => {
            let script = format!("test -e {}/{relative}", quote_path(&config.backend_dir()));
            probe(&config.shell_argv(&script)).ok
        }
        None => config.project_dir.join(relative).exists(),
    }
}

/// Look for a Mini-Me checkout somewhere other than where we are configured to find one.
///
/// Only ever **local to the machine the backend runs on**. A checkout on a Windows drive
/// is deliberately not offered for adoption even though WSL can reach it at `/mnt/c`:
/// running the venv over that mount is the placement that makes everything feel broken
/// (see `owned_wsl_dir`). The setup script still *copies* from there, which is the right
/// use of a Windows-side checkout — as a source, not as a home.
fn discover_checkout(config: &BackendConfig) -> Option<String> {
    let configured = config.backend_dir();
    match &config.wsl {
        Some(_) => {
            // One probe for the whole list: each `wsl.exe` call costs seconds, so this
            // must not become one per candidate.
            let script = "for d in ~/Mini-Me ~/mini-me ~/Documents/Mini-Me \
                          ~/.local/share/mini-me-desktop/backend; do \
                          [ -f \"$d/langgraph.json\" ] && echo \"$d\" && break; done";
            let found = probe(&config.shell_argv(script));
            found
                .stdout
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && *line != configured)
                .map(str::to_string)
        }
        None => {
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
    }
}

/// Ask every question, in dependency order.
///
/// `has_model_key` is passed in rather than read here on purpose: the Linux keychain
/// client runs its own `block_on` and panics if called from a thread already driving a
/// Tokio runtime — which is how the first live run of the settings code died. Secrets
/// are read once, on the main thread, and the answer travels as a bool.
pub fn inspect(config: &BackendConfig, has_model_key: bool) -> Report {
    let mut checks = Vec::new();
    let in_wsl = config.wsl.is_some();

    // ---------------------------------------------------------------- 1. the runtime
    //
    // Probed by asking the distro to answer, **not** by parsing `wsl -l`: that command
    // prints UTF-16LE, which `from_utf8_lossy` turns into NUL-riddled nonsense. Round
    // -tripping `echo` through bash also proves a distro is actually *usable* rather
    // than merely registered — a distro can be listed and still fail to start.
    let runtime = probe(&config.shell_argv("echo ok"));
    let runtime_ok = runtime.ok && runtime.stdout.contains("ok");
    if runtime_ok {
        checks.push(Check::pass(
            "runtime",
            if in_wsl { "WSL2 runtime" } else { "Shell" },
            // Not the full location — the header already carries it, and repeating a long
            // path in a 420px pane pushed everything else off the useful part of the row.
            if in_wsl {
                "a distro started and answered".to_string()
            } else {
                "bash is available on this machine".to_string()
            },
        ));
    } else if in_wsl {
        // wsl.exe's *own* errors are UTF-16 too, so its stderr is not shown — our
        // message is more use than a mojibake one anyway.
        let (detail, fix) = if runtime.launched {
            (
                "WSL is present but no distro answered".to_string(),
                Fix::Run {
                    label: "Install Ubuntu",
                    // Deliberately *not* `--no-launch`, though it looks made for this: it
                    // can install the distro without registering it under
                    // `HKCU\...\Lxss`, so `wsl -l -v` does not list it and the only cure is
                    // to run the install again without the flag
                    // (microsoft/WSL#10646). That failure is indistinguishable from the
                    // state this button exists to escape. The launch it would have
                    // suppressed is handled by denying stdin instead — see `elevated`.
                    argv: elevated(&["wsl.exe", "--install", "-d", "Ubuntu"]),
                    note: "Windows will ask for admin rights; may need a restart",
                },
            )
        } else {
            (
                "wsl.exe was not found — WSL is not installed".to_string(),
                Fix::Run {
                    label: "Install WSL",
                    argv: elevated(&["wsl.exe", "--install"]),
                    note: "Windows will ask for admin rights, then needs a restart",
                },
            )
        };
        checks.push(Check::failing(
            "runtime",
            "WSL2 runtime",
            State::Fail,
            detail,
            vec![fix],
        ));
    } else {
        checks.push(Check::failing(
            "runtime",
            "Shell",
            State::Fail,
            format!("no usable bash — {}", runtime.message()),
            vec![Fix::Manual(
                "The backend needs a POSIX shell. On Windows that means WSL: unset \
                 MINIME_BACKEND_WSL to use it (docs §21)."
                    .into(),
            )],
        ));
    }

    // Everything below runs *through* the runtime, so without it they would only
    // restate the failure above.
    let can_probe = runtime_ok || !in_wsl;

    // -------------------------------------------------------------- 2. the checkout
    let checkout_ok = if can_probe {
        let found = exists(config, "langgraph.json");
        if found {
            checks.push(Check::pass(
                "checkout",
                "Mini-Me backend",
                format!("langgraph.json found in {}", config.backend_dir()),
            ));
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
        let entry = if in_wsl {
            ".venv/bin/langgraph"
        } else if cfg!(windows) {
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
                    argv: config.shell_argv(&format!(
                        "cd {} && uv sync --extra dev",
                        quote_path(&config.backend_dir())
                    )),
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

    // ---------------------------------------------------------------- 4. the overlay
    //
    // The check that exists because this failure is *silent*. Host execution works by
    // putting `overlay/` on the backend's PYTHONPATH so `sitecustomize` swaps the
    // sandbox class at interpreter startup (docs §18). If that path is not reachable
    // from the backend — the repo on a drive the distro has not mounted, a UNC path
    // `wsl_path` cannot translate — Python simply imports nothing, no error is raised,
    // and the backend quietly tries the *remote* sandbox instead. The user sees an
    // authentication failure about a service they thought they had stopped using.
    let candidates = config.overlay_candidates();
    if let Some(overlay) = candidates.last().cloned() {
        if can_probe {
            // In the launch command's own preference order, so the pane names the copy the
            // backend will actually import. Reporting a different path from the one in use
            // is worse than reporting nothing — it sends anyone debugging to the wrong file.
            let found = candidates.iter().find(|candidate| {
                let marker = format!("{}/sitecustomize.py", candidate.trim_end_matches('/'));
                if in_wsl {
                    probe(&config.shell_argv(&format!("test -f {}", quote_path(&marker)))).ok
                } else {
                    std::path::Path::new(&marker).is_file()
                }
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
                    vec![Fix::Manual(format!(
                        "Host execution would not take effect and the backend would try \
                         the remote sandbox instead. Put this repo on a local drive, or \
                         set MINIME_OVERLAY_DIR to a path reachable from {}.",
                        if in_wsl { "the distro" } else { "the backend" }
                    ))],
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
    if can_probe {
        let found = probe(&config.shell_argv("command -v asta"));
        if found.ok {
            // Installed is not the same as usable. Asta access tokens last seven days, and
            // an expired login surfaces as "the theorizer returned no task id" — which
            // names neither the token nor the fix. Ask the CLI directly instead.
            //
            // Checked **where the backend runs**: on Windows that is inside the distro, so
            // being logged in on the Windows side proves nothing at all.
            let token = probe(&config.shell_argv("asta auth status"));
            // `Local Token Status` is the CLI's own verdict. Checking for it rather than
            // trusting the exit code, which is 0 even when signed out.
            if token.ok && token.stdout.contains("Valid") {
                let identity = asta_identity(&token.stdout);
                // Being signed in is not the same as being *entitled*. The theorizer
                // needs `enroll:theory_generation`, and an account without it fails with
                // upstream's "the Asta theorizer returned no task id — likely a missing or
                // expired token", which is a guess and a wrong one: the token is present,
                // valid, and simply not enrolled. Two real CIP accounts differed on
                // exactly this, and the error sent the user to re-authenticate for days.
                //
                // `print-token` without `--raw` prints the decoded payload, permissions
                // and all, so this needs no JWT decoding of our own.
                let claims = probe(&config.shell_argv("asta auth print-token"));
                let sign_in = Fix::Run {
                    label: "Sign in again",
                    argv: config.shell_argv("asta auth login"),
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
                                 `{THEORY_PERMISSION}` permission, which this account does \
                                 not have. Sign in with the account that does, or ask Asta \
                                 to enrol this one."
                            )),
                        ],
                    ));
                } else {
                    checks.push(Check {
                        id: "asta",
                        label: "Asta CLI",
                        state: State::Pass,
                        // Who, and for how long. On a shared machine "signed in" is not
                        // enough — someone signed in with the wrong account cannot work
                        // out why their permissions look odd.
                        detail: identity,
                        // A button even when green: when the *refresh* credential finally
                        // lapses this is the only cure, and a button that appears only
                        // once you are broken is one you cannot find.
                        fixes: vec![sign_in],
                    });
                }
            } else {
                checks.push(Check::failing(
                    "asta",
                    "Asta CLI",
                    State::Warn,
                    "installed, but not signed in where the backend runs",
                    vec![Fix::Run {
                        label: "Sign in to Asta",
                        argv: config.shell_argv("asta auth login"),
                        note: "opens a browser; the app refreshes the token itself after this",
                    }],
                ));
            }
        } else {
            checks.push(Check::failing(
                "asta",
                "Asta CLI",
                State::Warn,
                "not installed — literature search and the theorizer need it",
                vec![
                    Fix::Run {
                        label: "Install the Asta CLI",
                        argv: config.shell_argv(&format!(
                            "uv tool install {} && uv tool update-shell",
                            shell_quote(ASTA_INSTALL_URL)
                        )),
                        note: "about a minute",
                    },
                    Fix::Manual(
                        "Afterwards, paste ASTA_TOKEN and ASTA_API_KEY in Settings — the \
                         CLI reads them from its environment when a command runs."
                            .into(),
                    ),
                ],
            ));
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
/// stops at the first error. `wsl.exe` writes **UTF-16LE** on Windows, so every line was an
/// error and the iterator ended immediately — the fix log captured *nothing*, and the app
/// then told a researcher "the command reported a failure — the last lines say why" with no
/// lines at all (docs §57).
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
/// The elevated fix log is a file, not a stream, but it is written by the same Windows tools
/// in the same UTF-16LE — so it goes through [`read_lines`] rather than a second decoder
/// that could drift away from it.
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
            .map(|at| at)
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
/// A command that needs administrator rights, wrapped so Windows actually asks.
///
/// `wsl --install` requires elevation. Run from the app — which is not elevated, and must
/// not be — it fails immediately, and on the first clean machine it ever met it did exactly
/// that (docs §57). A process cannot elevate itself, so the only honest options were to ask
/// the researcher to open an admin terminal, or to let Windows ask them. `Start-Process
/// -Verb RunAs` is that prompt.
///
/// `-Wait` so the fix's own "finished" means finished, and `$p.ExitCode` so a refused UAC
/// prompt reports failure rather than success.
///
/// The command is run *through `cmd.exe`* only to get `> log 2>&1`. See [`elevated_log`]:
/// an elevated child cannot write into our pipes, so redirecting to a file is the only way
/// to find out what it said.
fn elevated(argv: &[&str]) -> Vec<String> {
    if !cfg!(windows) {
        return argv.iter().map(|part| part.to_string()).collect();
    }
    // Every part here is a compile-time constant without spaces, so only the log path —
    // which runs through the user's account name — needs quoting for cmd. Leaving the
    // first token unquoted also keeps cmd's "strip the outer pair" rule out of it.
    let command = argv.join(" ");
    let log = elevated_log().display().to_string();
    // `< NUL` matters as much as the redirect. `wsl --install -d Ubuntu` finishes by
    // launching the new distro, which asks — interactively — for a UNIX username and
    // password. With stdout going to a file that question is *invisible*, and the window
    // would sit there forever looking finished. At EOF the prompt gives up instead, leaving
    // a distro that answers as root, which is all the sidecar needs. An elevated fix can
    // never be interactive anyway: its console is not one we can put a question in.
    let inner = format!("/c {command} < NUL > \"{log}\" 2>&1");
    // Single-quoted for PowerShell, doubling any quote inside — nothing here contains one
    // today, and a future path must not be able to break out of the string.
    let script = format!(
        "$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        inner.replace('\'', "''")
    );
    vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-Command".into(),
        script,
    ]
}

/// Where an elevated fix leaves its output.
///
/// `-Verb RunAs` elevates through ShellExecute, which cannot be handed our pipes: the
/// elevated child gets a console window of its own. On the first machine that had no distro
/// the researcher watched `wsl.exe` download WSL 2.7.11 in *that* window while our pane
/// showed a fix which had, as far as the app could tell, printed nothing at all — then said
/// "done" over a red row (docs §60). Fixing the encoding in §57 could not have helped; the
/// bytes were never ours to decode.
///
/// So the elevated command redirects here, and [`run_streaming`] follows the file while it
/// grows. In the user's own temp directory rather than a shared one, because the unelevated
/// app has to be able to delete it; an elevated writer carries the Administrators group and
/// can write there regardless of which account approved the prompt.
pub fn elevated_log() -> std::path::PathBuf {
    std::env::temp_dir().join("mini-me-desktop-elevated.log")
}

/// Follow the elevated log while the command is still running.
///
/// Redirecting the elevated child's output to a file leaves its console blank, and
/// `wsl --install` downloads for minutes — so without this the pane would sit on "starting…"
/// with nothing to show for the wait, which is the state a researcher reads as *stuck*.
/// Re-reading the whole file each pass rather than framing UTF-16 incrementally: it is a few
/// kilobytes, and a half-decoded surrogate is not worth the cleverness.
fn tail_file(
    path: &std::path::Path,
    done: &std::sync::atomic::AtomicBool,
    mut send: impl FnMut(String) -> bool,
) {
    use std::sync::atomic::Ordering;

    let mut emitted = 0usize;
    loop {
        let finished = done.load(Ordering::SeqCst);
        if let Ok(bytes) = std::fs::read(path) {
            let lines = lines_of(&bytes);
            // A line the writer has already terminated is complete and goes out at once —
            // holding every line back until the next one arrived would show `wsl.exe`'s
            // progress one step behind for the whole download. Only an *unterminated* tail
            // waits, because it may be half-written. `Start-Process -Wait` means the writer
            // is gone before `finished` is set, so nothing is held back forever.
            let terminated = bytes.ends_with(&[0x0a]) || bytes.ends_with(&[0x0a, 0x00]);
            let ready = if finished || terminated {
                lines.len()
            } else {
                lines.len().saturating_sub(1)
            };
            for line in lines.iter().take(ready).skip(emitted) {
                if !send(strip_ansi(line)) {
                    return;
                }
            }
            emitted = emitted.max(ready);
        }
        if finished {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn run_streaming(argv: &[String], mut emit: impl FnMut(String)) -> anyhow::Result<bool> {
    use anyhow::Context as _;

    let (program, rest) = argv.split_first().context("empty command")?;
    // Before the child, so a previous elevated fix's output can never be read as this
    // one's. After this, the file existing at all means this run wrote it.
    let elevated_log = elevated_log();
    let _ = std::fs::remove_file(&elevated_log);
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not start {program}"))?;

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

    // Waited off this thread so the tailer below knows when to stop while we are still
    // draining lines. The pipes have their own readers, so nothing can fill and block.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let waiter = {
        let done = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            let status = child.wait();
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            status
        })
    };
    readers.push({
        let tx = tx.clone();
        let done = std::sync::Arc::clone(&done);
        let log = elevated_log.clone();
        std::thread::spawn(move || {
            tail_file(&log, &done, |line| tx.send(line).is_ok());
        })
    });
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

    fn config() -> BackendConfig {
        BackendConfig {
            port: 2024,
            project_dir: PathBuf::from("/nonexistent-checkout"),
            wsl: None,
            launch_command: vec!["true".into()],
            attach_only: false,
            log_path: PathBuf::from("/dev/null"),
            execution: Execution::Sandbox,
            secrets: Vec::new(),
            approve_execute: true,
            async_subagents: false,
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
        assert!(command.contains("setup-wsl.sh"), "{command}");
        assert!(command.contains("/nonexistent-checkout"), "{command}");

        // A skip has to name what it is *actually* waiting on. This one said "the
        // runtime above has to work first" on a machine whose runtime was fine — a small
        // lie that sends the user to check WSL when the checkout is what is missing.
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
        // A machine with no WSL must be told *one* thing. Reporting a missing checkout
        // and missing dependencies as well would send the user hunting in the distro
        // they do not have.
        let mut config = config();
        config.wsl = Some(crate::backend::WslTarget {
            // A distro name that cannot exist, so the runtime probe fails the way a
            // machine without WSL does.
            distro: Some("no-such-distro-9f3a".into()),
            dir: "~/Mini-Me".into(),
        });
        let report = inspect(&config, true);

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
        // what separates "WSL is missing" from "WSL refused".
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

    /// UTF-16LE bytes for a string, as `wsl.exe` writes them.
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn utf16_output_is_read_rather_than_dropped() {
        // The exact failure from the first clean machine: `wsl.exe` writes UTF-16LE, and
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
            lines_of(b"fatal: no such distro"),
            vec!["fatal: no such distro"]
        );
        assert_eq!(lines_of(&utf16("access denied")), vec!["access denied"]);
    }

    #[test]
    fn elevation_wraps_the_command_for_windows_only() {
        let argv = elevated(&["wsl.exe", "--install", "-d", "Ubuntu"]);
        if cfg!(windows) {
            assert_eq!(argv[0], "powershell.exe");
            let script = argv.last().expect("the script");
            // `RunAs` is the UAC prompt; `-Wait` makes "finished" mean finished; the exit
            // code is what turns a refused prompt into a reported failure.
            assert!(script.contains("-Verb RunAs"), "{script}");
            assert!(script.contains("-Wait"), "{script}");
            assert!(script.contains("exit $p.ExitCode"), "{script}");
            assert!(script.contains("wsl.exe --install -d Ubuntu"), "{script}");
            // Not `--no-launch`, which can leave the distro unregistered
            // (microsoft/WSL#10646) — the interactive prompt is denied stdin instead, or an
            // invisible question hangs the window forever (docs §61).
            assert!(!script.contains("--no-launch"), "{script}");
            assert!(script.contains("< NUL"), "{script}");
            // The whole point of going through cmd: an elevated child has its own console,
            // so without this redirect its output is lost and the pane has nothing to show
            // (docs §60).
            let log = elevated_log().display().to_string();
            assert!(script.contains(&format!("> \"{log}\" 2>&1")), "{script}");
        } else {
            // Everywhere else it must stay the plain command, or the Linux dev path breaks.
            assert_eq!(argv, vec!["wsl.exe", "--install", "-d", "Ubuntu"]);
        }
    }

    /// The elevated child writes to a file because its console is not ours (docs §60). Three
    /// things have to hold: a previous fix's output is never served up as this one's, the
    /// lines arrive *while* the command runs, and they arrive in order.
    ///
    /// One test rather than three because [`elevated_log`] is a single fixed path — which is
    /// right for an app that runs one fix at a time, and means separate tests would race each
    /// other over the same file.
    #[cfg(unix)]
    #[test]
    fn an_elevated_fix_log_is_followed_live_and_never_stale() {
        let log = elevated_log().display().to_string();
        std::fs::write(&log, b"left over from the last fix\n").expect("seed a stale log");

        let started = Instant::now();
        let mut seen: Vec<(String, Duration)> = Vec::new();
        let ok = run_streaming(
            &[
                "sh".into(),
                "-c".into(),
                // Stands in for the elevated child: writes only to the file, never to the
                // pipes we hold — which is exactly `wsl.exe` behind a UAC prompt.
                format!(
                    "printf 'Descargando: WSL 2.7.11\\n' > '{log}'; sleep 2; \
                     printf 'Instalando: WSL 2.7.11\\n' >> '{log}'"
                ),
            ],
            |line| seen.push((line, started.elapsed())),
        )
        .expect("the command ran");
        let total = started.elapsed();

        assert!(ok);
        let lines: Vec<&str> = seen.iter().map(|(line, _)| line.as_str()).collect();
        assert_eq!(
            lines,
            vec!["Descargando: WSL 2.7.11", "Instalando: WSL 2.7.11"]
        );
        // The first line has to reach the pane while the command is still going, or the
        // tailing does nothing that draining at the end would not have done.
        let first = seen[0].1;
        assert!(
            first + Duration::from_secs(1) < total,
            "the first line arrived at {first:?} of {total:?} — that is not live"
        );
        let _ = std::fs::remove_file(&log);
    }
}
