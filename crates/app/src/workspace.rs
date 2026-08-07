//! Where the researcher's outputs land, and how the app reaches them.
//!
//! The backend writes every file a turn produces into one directory per thread
//! (`minime_local/workspace.py:workspace_root`), and until now that was
//! `~/.mini-me/workspaces` **inside the WSL distro** — a place a Windows researcher cannot
//! reach without knowing what `\\wsl.localhost` means. Since ~98% of users are on Windows
//! and none of them are expected to code, files they cannot find are files that do not
//! exist.
//!
//! So the app now chooses that directory instead of letting the backend default, and
//! chooses one on the **Windows** side: `Documents\Mini-Me`. Three things fall out of that
//! single decision — outputs are in Explorer where a scientist expects them, "download
//! everything" becomes a button that opens a folder rather than a zip to build and unpack,
//! and the app can read the plots a turn produced and show them in the chat.
//!
//! The cost is that writes cross WSL's 9p mount. That is genuinely slow for *many small*
//! files — it is why the backend venv is deliberately kept inside the distro — but a
//! turn's outputs are a handful of CSVs, figures and reports, and being able to find them
//! is worth more than the milliseconds.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// The variable the backend reads for this (`minime_local/workspace.py:44`).
pub const WORKSPACE_ENV: &str = "MINIME_LOCAL_WORKSPACE";

/// Image kinds worth putting in the transcript.
///
/// `img()` decodes these; SVG is deliberately absent, because gpui renders it through a
/// different element that wants a monochrome-tintable asset rather than a plot.
const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// The directory holding one subdirectory per thread.
pub fn root() -> PathBuf {
    if let Some(dir) = std::env::var_os(WORKSPACE_ENV) {
        return PathBuf::from(dir);
    }
    // `Documents`, not `LOCALAPPDATA` like the backend checkout: this is the researcher's
    // own work, the one thing here they will want to open, copy and send to a colleague.
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        return PathBuf::from(home).join("Documents").join("Mini-Me");
    }
    PathBuf::from("Mini-Me")
}

/// A specialist the coordinator can be told to use by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    pub name: String,
    pub description: String,
}

/// The file the backend overlay writes its subagent list into.
const REGISTRY: &str = "subagents.json";

/// What can be named in a `/subagent` command.
///
/// Read from a file the backend overlay writes when it assembles the coordinator, rather than
/// hardcoded here. §55 asked for that specifically: a copy in the client would drift the first
/// time upstream renamed a subagent, and the failure mode is a command that silently does
/// nothing. See `overlay/minime_local/registry.py` for why it is a file and not an endpoint —
/// `langgraph.json` mounts its HTTP app by file path, which bypasses the import hook the
/// overlay patches through.
///
/// Empty until the backend has assembled a coordinator at least once. That is a real gap and
/// the caller has to say so rather than showing an empty picker as though there were no
/// specialists.
pub fn subagents() -> Vec<Subagent> {
    let Ok(text) = std::fs::read_to_string(root().join(REGISTRY)) else {
        return Vec::new();
    };
    parse_registry(&text)
}

/// Separated from the read so the shape can be tested without a filesystem.
pub(crate) fn parse_registry(text: &str) -> Vec<Subagent> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    // The version is checked rather than trusted: a client reading a newer shape would offer
    // commands that do nothing, which is worse than offering none.
    if value.get("format").and_then(serde_json::Value::as_u64) != Some(1) {
        return Vec::new();
    }
    value
        .get("subagents")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name")?.as_str()?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    Some(Subagent {
                        name: name.to_string(),
                        description: entry
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One path segment for a project name, or `None` for "no project".
///
/// **This must agree exactly with `workspace_project` in `overlay/minime_local/workspace.py`.**
/// The backend writes a turn's outputs into the folder *it* computes; the app looks in the folder
/// *this* computes. Disagree by one character and the researcher's figures are written somewhere
/// the app will never show them — the §89 failure with a longer fuse. There is a test that runs
/// both and compares them, because two implementations of one rule in two languages is exactly
/// the shape this project keeps getting wrong (docs §105).
pub fn project_folder(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| matches!(c, ' ' | '.' | '_'));
    let clipped: String = trimmed.chars().take(96).collect();
    (!clipped.is_empty()).then_some(clipped)
}

/// Where one conversation's files live, inside its project if it has one.
pub fn thread_dir_in(project: Option<&str>, thread_id: &str) -> PathBuf {
    match project.and_then(project_folder) {
        Some(folder) => root().join(folder).join(thread_id),
        None => root().join(thread_id),
    }
}

/// Move a conversation's folder into a different project, or out of one.
///
/// **Moves rather than copies**, which is what a person expects of "move to project" and what
/// keeps the app and Explorer telling the same story. Only safe while no turn is running — the
/// backend holds this path open for the length of a turn — so the caller checks that first.
///
/// Absent source is not an error: a conversation that has produced nothing yet has no directory,
/// and filing it should still work.
pub fn move_thread(from: Option<&str>, to: Option<&str>, thread_id: &str) -> Result<()> {
    let source = thread_dir_in(from, thread_id);
    let destination = thread_dir_in(to, thread_id);
    if source == destination || !source.is_dir() {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    // Refuse rather than merge: two directories for one conversation means the app has already
    // lost track of where its files are, and silently folding them together would hide that.
    if destination.exists() {
        anyhow::bail!(
            "{} already exists — move or remove it first",
            destination.display()
        );
    }
    std::fs::rename(&source, &destination)
        .with_context(|| format!("moving {} to {}", source.display(), destination.display()))?;
    // An empty project folder left behind reads as a project that still exists.
    if let Some(parent) = source.parent() {
        if parent != root() {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// Turn a report's title into a filename a person would recognise in a folder listing.
///
/// Runs of anything that is not a letter or digit become one underscore, and the case is kept —
/// so *"EDA Report: Simulated Potato Field Trials"* becomes
/// `EDA_Report_Simulated_Potato_Field_Trials.md`, which is what the agent itself proposed when
/// asked where the file was. Matching that spelling matters: the answer in the transcript and the
/// file on disk should be the same name.
///
/// Windows is the target, so this also has to survive `\ / : * ? " < > |` — a title with a colon
/// in it is the common case, not the exotic one.
pub fn report_filename(title: &str) -> String {
    let mut name = String::new();
    let mut pending = false;
    for character in title.chars() {
        if character.is_alphanumeric() {
            if pending && !name.is_empty() {
                name.push('_');
            }
            pending = false;
            name.push(character);
        } else {
            pending = true;
        }
    }
    if name.is_empty() {
        name.push_str("Report");
    }
    // Long titles happen, and Windows' path limit is not generous.
    let clipped: String = name.chars().take(96).collect();
    format!("{}.md", clipped.trim_end_matches('_'))
}

/// Write a report beside the conversation's other outputs, and say where it went.
///
/// Skips the write when the file already holds exactly this text. A `values` snapshot arrives
/// many times during a turn and carries every report each time, so without this the same file
/// would be rewritten on every frame — and its modification time, which [`images`] sorts by and
/// a researcher reads, would keep jumping to now.
pub fn save_report(dir: &Path, title: &str, markdown: &str) -> Result<PathBuf> {
    let path = dir.join(report_filename(title));
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == markdown {
            return Ok(path);
        }
    }
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating {} for a report", dir.display()))?;
    std::fs::write(&path, markdown).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Every image in `dir`, oldest first.
///
/// Sorted by modification time so a turn's figures appear in the order they were drawn,
/// which for a plotting script is the order the analysis went in. Returns empty for a
/// directory that does not exist yet — the common case, since the backend creates it on
/// first write.
pub fn images(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            if !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().map(|(_, path)| path).collect()
}

/// One file a conversation produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub path: PathBuf,
    pub name: String,
    /// What it is, in the researcher's terms — the grouping key in the panel.
    pub kind: Kind,
    pub bytes: u64,
}

/// The kinds of output worth telling apart.
///
/// Deliberately about *what a researcher does with it*, not about file format: a figure is
/// something you look at, data is something you open in a spreadsheet or load in a script,
/// a document is something you read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Figure,
    Data,
    Document,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Figure => "Figures",
            Kind::Data => "Data",
            Kind::Document => "Documents",
            Kind::Other => "Other files",
        }
    }

    fn of(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if IMAGE_EXTENSIONS.contains(&extension.as_str()) || extension == "svg" {
            Kind::Figure
        } else if matches!(
            extension.as_str(),
            "csv" | "tsv" | "xlsx" | "json" | "parquet"
        ) {
            Kind::Data
        } else if matches!(extension.as_str(), "md" | "txt" | "pdf" | "docx" | "html") {
            Kind::Document
        } else {
            Kind::Other
        }
    }
}

/// Everything a conversation produced, grouped and newest first within each group.
///
/// Reads the directory rather than the agent's own artifact list, for the same reason
/// plots are diffed off disk (§42): a file written by a script inside `execute` registers
/// no artifact, and those are most of them.
pub fn outputs(dir: &Path) -> Vec<(Kind, Vec<Output>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, Output)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            // Dotfiles are the agent's business, not the researcher's.
            if name.starts_with('.') {
                return None;
            }
            Some((
                metadata.modified().ok()?,
                Output {
                    kind: Kind::of(&path),
                    path,
                    name,
                    bytes: metadata.len(),
                },
            ))
        })
        .collect();
    // Newest first: the file someone wants is nearly always the one just written.
    found.sort_by(|a, b| b.0.cmp(&a.0));

    let mut groups: Vec<(Kind, Vec<Output>)> = Vec::new();
    for kind in [Kind::Figure, Kind::Data, Kind::Document, Kind::Other] {
        let items: Vec<Output> = found
            .iter()
            .filter(|(_, output)| output.kind == kind)
            .map(|(_, output)| output.clone())
            .collect();
        if !items.is_empty() {
            groups.push((kind, items));
        }
    }
    groups
}

/// A size a person can read at a glance.
pub fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes.div_ceil(KB))
    } else {
        format!("{bytes} B")
    }
}

/// The first `lines` lines of a text file.
///
/// Bounded on purpose. A dataset can be hundreds of megabytes, and a preview that reads
/// the whole thing would pull it into memory and lay it out on the UI thread — the file
/// most worth previewing being exactly the one that would freeze the window.
pub fn head(path: &Path, lines: usize) -> Result<String> {
    use std::io::{BufRead, BufReader};

    let file =
        std::fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut text = String::new();
    let mut buffer = String::new();
    for taken in 0..lines {
        buffer.clear();
        // Not UTF-8 (a spreadsheet export, say) is a preview problem, not a crash.
        match reader.read_line(&mut buffer) {
            Ok(0) => break,
            Ok(_) => text.push_str(&buffer),
            Err(error) => {
                if taken == 0 {
                    return Err(error)
                        .with_context(|| format!("could not read {}", path.display()));
                }
                break;
            }
        }
    }
    Ok(text)
}

/// Open a folder in the platform's file manager.
///
/// The whole of "download everything the agent made": the files are already sitting in the
/// researcher's own Documents, so there is nothing to package — only somewhere to point.
pub fn open(path: &Path) -> Result<()> {
    // Only conjure a *missing* directory. Calling this on a file that exists — which is
    // every "open outside" click — made `create_dir_all` fail with AlreadyExists, and the
    // error return meant Explorer was never launched at all (docs §50).
    if !path.exists() {
        std::fs::create_dir_all(path)
            .with_context(|| format!("could not create {}", path.display()))?;
    }

    let mut command = if cfg!(windows) {
        // Through `explorer.exe` directly rather than `cmd /c start`, which would flash a
        // console window on a machine where this app is the only thing the user opened.
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(path);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };

    // Explorer returns a non-zero exit code even when it succeeds, so the status is not
    // worth checking — only whether the process started at all.
    command
        .spawn()
        .with_context(|| format!("could not open {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod project_tests {
    use super::*;

    /// The Rust and Python sanitisers must produce byte-identical names.
    ///
    /// **This is the test that matters most in this file.** The backend writes a turn's outputs
    /// into the folder it computes from `configurable.__workspace_project__`; the app looks in the
    /// folder *it* computes from the same string. One character of disagreement and a
    /// researcher's figures land somewhere the app will never look — §89's failure, with a longer
    /// fuse and no error anywhere.
    ///
    /// Two implementations of one rule in two languages is a shape this project has got wrong
    /// before (§100: a scrollbar width in one file and a layout in another). It cannot be written
    /// once here, so it is checked instead.
    #[test]
    fn the_rust_and_python_project_names_agree() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");

        let names = [
            "Late blight",
            "Late blight/2026",
            "  ../../etc  ",
            "",
            "   ",
            r#"Q1:"yield"<x>|v2"#,
            "café ñandú",
            "___",
            "...",
            "a.b.c",
            &"A".repeat(200),
            "trailing_",
            "-leading",
        ];

        // The sanitiser lifted out of the module, so importing it does not pull in deepagents.
        let source = std::fs::read_to_string(overlay.join("minime_local/workspace.py"))
            .expect("the overlay is beside the crate");
        let start = source
            .find("def workspace_project()")
            .expect("the function");
        let end = source
            .find("def workspace_thread(")
            .expect("the next function");
        let script = format!(
            "import json,sys\nWORKSPACE_PROJECT_KEY='__workspace_project__'\n\
             def _configurable(): return {{'__workspace_project__': sys.argv[1]}}\n{}\n\
             print(json.dumps(workspace_project()))",
            &source[start..end]
        );

        for name in names {
            let out = std::process::Command::new("python3")
                .arg("-c")
                .arg(&script)
                .arg(name)
                .output()
                .expect("running the python sanitiser");
            assert!(
                out.status.success(),
                "python failed for {name:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let python: String = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
                .expect("json string");
            let rust = project_folder(name).unwrap_or_default();
            assert_eq!(rust, python, "disagreed on {name:?}");
        }
    }

    #[test]
    fn a_project_name_can_never_escape_the_workspace() {
        // It becomes a path segment, and it is a thing a person types.
        for hostile in ["../..", "/etc/passwd", r"..\..\Windows", "a/b"] {
            let folder = project_folder(hostile).unwrap_or_default();
            assert!(!folder.contains('/'), "{hostile:?} -> {folder:?}");
            assert!(!folder.contains('\\'), "{hostile:?} -> {folder:?}");
            assert!(!folder.starts_with('.'), "{hostile:?} -> {folder:?}");
        }
        // Nothing usable is nothing, not a folder called "_".
        assert_eq!(project_folder("   "), None);
        assert_eq!(project_folder("..."), None);
        assert_eq!(project_folder(""), None);
    }

    #[test]
    fn an_ungrouped_conversation_keeps_the_path_it_always_had() {
        // Every conversation that predates projects stays exactly where it is — the answer to
        // "what happens to my existing work" is "nothing" (docs §105).
        assert_eq!(thread_dir_in(None, "t-1"), root().join("t-1"));
        assert_eq!(thread_dir_in(Some("   "), "t-1"), root().join("t-1"));
        assert_eq!(
            thread_dir_in(Some("Late blight"), "t-1"),
            root().join("Late blight").join("t-1")
        );
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn a_title_becomes_the_filename_the_agent_itself_proposed() {
        // The transcript said the report could be saved as
        // `EDA_Report_Simulated_Potato_Field_Trials.md`. The file on disk should carry that same
        // name, or the answer and the folder disagree about what happened (docs §89).
        assert_eq!(
            report_filename("EDA Report: Simulated Potato Field Trials"),
            "EDA_Report_Simulated_Potato_Field_Trials.md"
        );
    }

    #[test]
    fn characters_windows_refuses_never_reach_the_path() {
        // Windows is the target, and a colon in a report title is the common case. Every one of
        // `\ / : * ? " < > |` has to be gone, not escaped.
        let name = report_filename(r#"Q1/Q2: "yield" <draft> | v2*?"#);
        assert!(
            !name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']),
            "{name}"
        );
        assert_eq!(name, "Q1_Q2_yield_draft_v2.md");
    }

    #[test]
    fn a_title_of_nothing_usable_still_produces_a_file() {
        assert_eq!(report_filename("***"), "Report.md");
        assert_eq!(report_filename(""), "Report.md");
    }

    #[test]
    fn rewriting_the_same_report_leaves_the_file_alone() {
        // A `values` snapshot arrives many times per turn and carries every report each time.
        // Rewriting on each one would keep resetting a timestamp a researcher reads — and that
        // `images` sorts by.
        let dir = std::env::temp_dir().join(format!("mini-me-report-{}", std::process::id()));
        let first = save_report(&dir, "Trial Report", "# Yield\n").expect("first write");
        let stamp = std::fs::metadata(&first).unwrap().modified().unwrap();
        let again = save_report(&dir, "Trial Report", "# Yield\n").expect("second write");
        assert_eq!(first, again);
        assert_eq!(
            std::fs::metadata(&again).unwrap().modified().unwrap(),
            stamp
        );
        // Changed content does land.
        save_report(&dir, "Trial Report", "# Yield\n\nRevised.\n").expect("third write");
        assert!(std::fs::read_to_string(&first).unwrap().contains("Revised"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_images_are_collected_and_in_the_order_they_were_written() {
        let dir =
            std::env::temp_dir().join(format!("minime-workspace-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");

        // A report and a dataset are outputs too, but they are not something the chat can
        // render — only figures belong inline.
        for name in ["informe.md", "papas.csv", "notes.txt"] {
            std::fs::write(dir.join(name), b"x").expect("write");
        }
        for name in ["a_first.png", "b_second.JPG", "c_third.webp"] {
            std::fs::write(dir.join(name), b"x").expect("write");
            // Distinct mtimes, or the sort has nothing to order by.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let found: Vec<String> = images(&dir)
            .iter()
            .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
            .collect();
        assert_eq!(found, vec!["a_first.png", "b_second.JPG", "c_third.webp"]);

        // A directory that does not exist is the normal state before the first write.
        assert!(images(&dir.join("nothing-here")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outputs_are_grouped_by_what_the_researcher_would_do_with_them() {
        let dir = std::env::temp_dir().join(format!("minime-outputs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");
        for name in [
            "plot_yield.png",
            "papas.csv",
            "informe.md",
            "model.pkl",
            ".hidden",
        ] {
            std::fs::write(dir.join(name), b"xx").expect("write");
        }

        let groups = outputs(&dir);
        let labels: Vec<&str> = groups.iter().map(|(kind, _)| kind.label()).collect();
        // Figures first: they are the outputs someone wants to *see*.
        assert_eq!(labels, vec!["Figures", "Data", "Documents", "Other files"]);

        let names: Vec<&str> = groups
            .iter()
            .flat_map(|(_, items)| items.iter().map(|o| o.name.as_str()))
            .collect();
        assert!(names.contains(&"plot_yield.png"));
        assert!(names.contains(&"papas.csv"));
        // Dotfiles are the agent's business, not the researcher's.
        assert!(!names.contains(&".hidden"), "{names:?}");

        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact bytes `overlay/minime_local/registry.py` writes, with the real names from
    /// `backend/subagents.py` — so a rename upstream shows up here as a failing test rather
    /// than as a command that quietly does nothing.
    #[test]
    fn the_subagent_registry_is_read_as_the_overlay_writes_it() {
        let written = r#"{
          "format": 1,
          "subagents": [
            {"name": "academic_researcher", "description": "Conducts research using Asta tools."},
            {"name": "exploratory_data_analysis", "description": "Performs EDA."}
          ]
        }"#;
        let found = parse_registry(written);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "academic_researcher");
        assert!(found[0].description.starts_with("Conducts research"));
        assert_eq!(found[1].name, "exploratory_data_analysis");
    }

    /// The cross-language contract, tested against a file the Python side really wrote.
    ///
    /// Generated by running `minime_local.registry.record` over the reference checkout's own
    /// `backend.subagents.subagents` — so this asserts the *seam*, not my idea of it. Every bug
    /// this week lived in a seam (§71), and this one has a parser on one side of it and a
    /// different language on the other.
    #[test]
    fn the_fixture_the_overlay_actually_wrote_parses() {
        let written = include_str!("../tests/fixtures/subagent-registry.json");
        let found = parse_registry(written);
        assert_eq!(found.len(), 10, "{found:#?}");
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        // The three the request imagined, under the names the backend actually uses.
        assert!(names.contains(&"exploratory_data_analysis"), "{names:?}");
        assert!(names.contains(&"academic_researcher"), "{names:?}");
        assert!(names.contains(&"report_writer"), "{names:?}");
        assert!(
            found.iter().all(|s| !s.description.is_empty()),
            "every specialist has something to say about itself"
        );
    }

    #[test]
    fn a_registry_from_a_future_release_is_declined_rather_than_guessed_at() {
        // Offering commands read out of a shape we do not understand is worse than offering
        // none: every one of them would look available and do nothing.
        let ahead = r#"{"format": 2, "subagents": [{"name": "academic_researcher"}]}"#;
        assert!(parse_registry(ahead).is_empty());
    }

    #[test]
    fn junk_and_absence_both_give_nothing_rather_than_panicking() {
        // This file is written by another process while this one reads it. Every bad shape has
        // to be survivable, because the alternative is the window dying on a truncated write.
        for text in [
            "",
            "not json",
            "{}",
            r#"{"format":1}"#,
            r#"{"format":1,"subagents":[]}"#,
        ] {
            assert!(parse_registry(text).is_empty(), "{text:?}");
        }
        // A nameless entry is not nameable, and a missing description is merely unhelpful.
        let ragged = r#"{"format":1,"subagents":[{"name":"  "},{"name":"report_writer"}]}"#;
        let found = parse_registry(ragged);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "report_writer");
        assert_eq!(found[0].description, "");
    }

    #[test]
    fn the_root_is_somewhere_the_researcher_can_find() {
        // Explicit configuration always wins, which is what lets the backend and the app
        // agree on one directory rather than each guessing.
        let previous = std::env::var_os(WORKSPACE_ENV);
        // SAFETY: single-threaded test setup; restored below.
        unsafe { std::env::set_var(WORKSPACE_ENV, "/tmp/somewhere-else") };
        assert_eq!(root(), PathBuf::from("/tmp/somewhere-else"));
        assert_eq!(
            thread_dir_in(None, "abc-123"),
            PathBuf::from("/tmp/somewhere-else").join("abc-123")
        );
        match previous {
            Some(value) => unsafe { std::env::set_var(WORKSPACE_ENV, value) },
            None => unsafe { std::env::remove_var(WORKSPACE_ENV) },
        }
    }
}
