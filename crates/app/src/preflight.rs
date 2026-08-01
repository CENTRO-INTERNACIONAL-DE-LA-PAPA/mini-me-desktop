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

use crate::backend::{quote_path, BackendConfig, Execution};

/// Ceiling for one probe. Generous because a **cold** WSL distro genuinely takes
/// several seconds to boot on the first `wsl.exe` call of a session, and reporting
/// "WSL is missing" because we gave up at two seconds would be a lie.
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a check was skipped when the runtime itself never answered.
const RUNTIME_FIRST: &str = "the runtime above has to work";

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
}

#[derive(Debug, Clone)]
pub struct Check {
    /// Stable identifier, so tests and the UI can name a row without matching prose.
    pub id: &'static str,
    pub label: &'static str,
    pub state: State,
    /// What was found, or what is wrong. One line.
    pub detail: String,
    pub fix: Option<Fix>,
}

impl Check {
    fn pass(id: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            label,
            state: State::Pass,
            detail: detail.into(),
            fix: None,
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
            fix: None,
        }
    }

    fn failing(
        id: &'static str,
        label: &'static str,
        state: State,
        detail: impl Into<String>,
        fix: Fix,
    ) -> Self {
        Self {
            id,
            label,
            state,
            detail: detail.into(),
            fix: Some(fix),
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
        self.checks
            .iter()
            .find(|check| check.state == State::Fail)
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
            let script = format!(
                "test -e {}/{relative}",
                quote_path(&config.backend_dir())
            );
            probe(&config.shell_argv(&script)).ok
        }
        None => config.project_dir.join(relative).exists(),
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
            if in_wsl {
                format!("a distro answered ({})", config.location())
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
                    argv: vec!["wsl.exe".into(), "--install".into(), "-d".into(), "Ubuntu".into()],
                    note: "asks for admin rights; may need a restart",
                },
            )
        } else {
            (
                "wsl.exe was not found — WSL is not installed".to_string(),
                Fix::Run {
                    label: "Install WSL",
                    argv: vec!["wsl.exe".into(), "--install".into()],
                    note: "asks for admin rights, then needs a restart",
                },
            )
        };
        checks.push(Check::failing(
            "runtime",
            "WSL2 runtime",
            State::Fail,
            detail,
            fix,
        ));
    } else {
        checks.push(Check::failing(
            "runtime",
            "Shell",
            State::Fail,
            format!("no usable bash — {}", runtime.message()),
            Fix::Manual(
                "The backend needs a POSIX shell. On Windows that means WSL: unset \
                 MINIME_BACKEND_WSL to use it (docs §21)."
                    .into(),
            ),
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
            checks.push(Check::failing(
                "checkout",
                "Mini-Me backend",
                State::Fail,
                format!("no langgraph.json in {}", config.backend_dir()),
                Fix::Run {
                    label: "Provision the backend",
                    argv: config.shell_argv(&config.setup_script()),
                    note: "clones the repo and installs Python deps — several minutes",
                },
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
                Fix::Run {
                    label: "Sync dependencies",
                    argv: config.shell_argv(&format!(
                        "cd {} && uv sync --extra dev",
                        quote_path(&config.backend_dir())
                    )),
                    note: "pulls PyMC and friends on a cold venv — several minutes",
                },
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
    if let Some(overlay) = config.overlay_for_backend() {
        if can_probe {
            let marker = format!("{}/sitecustomize.py", overlay.trim_end_matches('/'));
            let found = if in_wsl {
                probe(&config.shell_argv(&format!("test -f {}", quote_path(&marker)))).ok
            } else {
                std::path::Path::new(&marker).is_file()
            };
            if found {
                checks.push(Check::pass(
                    "overlay",
                    "Host execution overlay",
                    format!("reachable at {overlay}"),
                ));
            } else {
                checks.push(Check::failing(
                    "overlay",
                    "Host execution overlay",
                    State::Fail,
                    format!("the backend cannot see {overlay}"),
                    Fix::Manual(format!(
                        "Host execution would not take effect and the backend would try \
                         the remote sandbox instead. Put this repo on a local drive, or \
                         set MINIME_OVERLAY_DIR to a path reachable from {}.",
                        if in_wsl { "the distro" } else { "the backend" }
                    )),
                ));
            }
        } else {
            checks.push(Check::skip("overlay", "Host execution overlay", RUNTIME_FIRST));
        }
    } else if matches!(config.execution, Execution::Sandbox) {
        checks.push(Check::failing(
            "overlay",
            "Host execution overlay",
            State::Warn,
            "off — the agent's commands go to the remote sandbox",
            Fix::Manual(
                "That needs LANGSMITH_API_KEY. Turn on \"Run code on this machine\" in \
                 Settings to use the local path instead."
                    .into(),
            ),
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
            checks.push(Check::pass(
                "asta",
                "Asta CLI",
                found.stdout.trim().lines().next().unwrap_or("found").to_string(),
            ));
        } else {
            checks.push(Check::failing(
                "asta",
                "Asta CLI",
                State::Warn,
                "not on the backend's PATH",
                Fix::Manual(
                    "Literature search and the theorizer need it. Install it where the \
                     backend runs, then store ASTA_TOKEN and ASTA_API_KEY in Settings."
                        .into(),
                ),
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
            Fix::Manual("Open Settings (ctrl-,) and paste your key — it goes into the OS keychain, never into a file.".into()),
        ));
    }

    Report {
        checks,
        location: config.location(),
        execution: config.execution_label().to_string(),
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
        }
    }

    #[test]
    fn a_missing_checkout_is_reported_with_the_command_that_fixes_it() {
        let report = inspect(&config(), true);
        let checkout = report
            .checks
            .iter()
            .find(|check| check.id == "checkout")
            .expect("a checkout row");
        assert_eq!(checkout.state, State::Fail);
        assert!(!report.ready(), "a missing checkout blocks every turn");

        // The point of the pane: the fix is a command, not advice.
        let Some(Fix::Run { argv, .. }) = &checkout.fix else {
            panic!("expected a runnable fix, got {:?}", checkout.fix);
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
            assert!(check.fix.is_none(), "{id} should offer no fix while skipped");
        }
        // And the one real problem is still nameable in a single line.
        assert_eq!(report.first_problem().map(|c| c.id), Some("runtime"));
    }

    #[test]
    fn a_missing_key_is_the_only_thing_wrong_on_a_provisioned_machine() {
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

    #[test]
    fn the_summary_counts_every_state() {
        let report = inspect(&config(), false);
        let summary = report.summary();
        assert!(summary.contains("ok"), "{summary}");
        assert!(summary.contains("to fix"), "{summary}");
    }
}
