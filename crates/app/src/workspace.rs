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
/// Task ids of background runs the researcher has already been told about.
///
/// **In a file, because the question is "since when".** The launch sweep (§243) asks the backend
/// for every unfinished run across recent conversations and collects the ones that have since
/// finished — and "finished" stays true forever. With only the in-memory `swept` flag guarding it,
/// a run that completed on Thursday was collected, announced and banner-ed on Friday, Saturday and
/// every launch after: *"since yesterday data voyager already finished the analysis so its weird to
/// see the modal everytime I open it."*
///
/// Ids rather than a timestamp. A clock comparison needs the run's finish time, the app's last
/// launch time and two zones to agree; a set of ids needs none of that and answers exactly the
/// question being asked — has this one been mentioned?
///
/// Newline-delimited and read best-effort: an unreadable file means "nothing announced yet", which
/// re-announces at worst and never *loses* a result.
fn announced_path() -> PathBuf {
    crate::settings::data_dir().join("announced-runs.txt")
}

/// The runs already announced. Empty when the file is missing, which is the first-launch case.
pub fn announced_runs() -> std::collections::HashSet<String> {
    std::fs::read_to_string(announced_path())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Record that these runs have been mentioned, so the next launch stays quiet about them.
///
/// Capped, because this file grows once per background run forever and nothing prunes it. The cap
/// keeps the newest, which is the only end that matters: a run old enough to fall off the list is
/// one no sweep will surface again, because the sweep only looks at recent conversations.
pub fn remember_announced(ids: &[String]) {
    const KEEP: usize = 500;
    if ids.is_empty() {
        return;
    }
    let mut known = announced_runs();
    let already = ids.iter().all(|id| known.contains(id));
    if already {
        return;
    }
    known.extend(ids.iter().cloned());
    let path = announced_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = known.into_iter().collect();
    lines.sort();
    if lines.len() > KEEP {
        let drop = lines.len() - KEEP;
        lines.drain(..drop);
    }
    if let Err(error) = std::fs::write(&path, lines.join("\n")) {
        // Not fatal: the cost is announcing the same run twice, not losing it.
        tracing::warn!(%error, path = %path.display(), "could not record announced runs");
    }
}

/// The figures already decoded for one experiment, in this conversation's folder.
///
/// **Looked for before asking for them.** The poll route decodes every experiment's plots when a
/// run finishes (§263), so for a completed run they are already here — and a request per click was
/// a wait for something we owned, which is the same answer §261 gave about the experiments list.
///
/// Sorted, so `figure-01` comes before `figure-02` rather than in whatever order the directory
/// happens to yield.
pub fn discovery_figures(
    conversation: &std::path::Path,
    run_id: &str,
    experiment_id: &str,
) -> Vec<PathBuf> {
    // Both ids come from a payload, so neither is trusted into a path.
    let unsafe_id = |id: &str| id.is_empty() || id.contains(['/', '\\', '.']);
    if unsafe_id(run_id) || unsafe_id(experiment_id) {
        return Vec::new();
    }
    let dir = conversation.join("discovery").join(run_id).join(experiment_id);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg")
                })
        })
        .collect();
    found.sort();
    found
}

/// A finished discovery run's own record, written by the poll route into the conversation's folder.
///
/// **Read this before asking the service.** `persist_discovery_outputs` writes
/// `discovery/<run_id>.json` the moment a run reaches a terminal state — it has to, because
/// uploaded datasets expire after seven days and the service is not an archive (§247). So for a
/// finished run the experiments are already on disk, and re-fetching them through the sandbox on
/// every open was a delay for something we owned: *"Are we fetching it every time? why we dont just
/// download it a show it?"* (§261).
///
/// `None` when the file is absent or unreadable, which is the ordinary case for a run still
/// producing — its record does not exist yet. The caller falls back to the service.
pub fn discovery_record(conversation: &std::path::Path, run_id: &str) -> Option<serde_json::Value> {
    // The run id reaches this from an artifact, so it is checked rather than trusted: a path
    // separator or a `..` in it would read a file from somewhere else entirely.
    if run_id.is_empty() || run_id.contains(['/', '\\', '.']) {
        return None;
    }
    let path = conversation.join("discovery").join(format!("{run_id}.json"));
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

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

impl Subagent {
    /// Whether this specialist reaches Asta.
    ///
    /// **Read off the registry rather than from a list kept here.** Three of the shipped
    /// specialists say so in their own descriptions — *"Conducts research using Asta tools"*,
    /// *"using the Asta Theorizer pipeline"*, *"Run the Asta DataVoyager pipeline"* — and that
    /// text arrives in `subagents.json`, written by the backend from the factory call that
    /// actually built the coordinator.
    ///
    /// A hardcoded `["academic_researcher", "hypothesis_generator", "data_voyager"]` would be
    /// the exact thing §55 built this file to avoid: a copy in the client that drifts the first
    /// time upstream renames one, failing silently. Here the failure mode is a specialist whose
    /// description stops mentioning Asta, which is visible in the file.
    pub fn uses_asta(&self) -> bool {
        self.description.to_ascii_lowercase().contains("asta")
    }
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

/// Whether a directory name is a generated thread id rather than something a person typed.
///
/// The one thing that tells a project folder from an ungrouped conversation's folder, because
/// both sit directly under the workspace root. A thread id is a UUID and a project name is
/// whatever a researcher called their work, so the shape is the discriminator — and it is the
/// same predicate the Outputs panel uses to strip a leading UUID from a folder label (§152).
pub fn looks_like_thread_id(component: &str) -> bool {
    component.len() == 36
        && component
            .chars()
            .enumerate()
            .all(|(at, character)| match at {
                8 | 13 | 18 | 23 => character == '-',
                _ => character.is_ascii_hexdigit(),
            })
}

/// Every project that has a folder, whether or not a conversation is filed under it yet.
///
/// **This is what makes an empty project possible**, and it is not a second registry — §105 made
/// a project a real directory, so the directory *is* the project and reading it is reading the
/// thing itself. §106's rule that a project is "a name some conversation is filed under" was
/// right about not keeping a list in settings and wrong about the only evidence: naming a project
/// created the folder and then showed nothing, because the sidebar could only see projects
/// through conversations (§167).
///
/// Excludes generated thread folders and anything hidden. Files are skipped, which is what keeps
/// `subagents.json` out of the sidebar.
pub fn projects() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root()) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| !name.starts_with('.') && !looks_like_thread_id(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Make a project's folder, so it exists before anything is filed into it.
///
/// Returns the sanitised name the folder actually carries — which is what the rest of the app
/// must use, since `project_folder` may have rewritten characters a path cannot hold.
pub fn create_project(name: &str) -> Result<String> {
    let folder = project_folder(name)
        .with_context(|| format!("{name:?} does not make a valid project folder"))?;
    let path = root().join(&folder);
    std::fs::create_dir_all(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    Ok(folder)
}

/// Where one conversation's files live, inside its project if it has one.
pub fn thread_dir_in(project: Option<&str>, thread_id: &str) -> PathBuf {
    match project.and_then(project_folder) {
        Some(folder) => root().join(folder).join(thread_id),
        None => root().join(thread_id),
    }
}

/// Where one background worker's files landed, inside the conversation that started it.
///
/// **A worker runs on its own LangGraph thread but writes inside its parent's folder** — the
/// overlay composes `[conversation_thread, worker_thread]` when the two differ
/// (`LazyLangsmithSandbox.__init__`), which is what §151 verified on a live run: plots appeared
/// at `<task>/guinea_pig_eda_output/plots/…` rather than in a sibling directory nobody would
/// think to open.
///
/// Falls back to the conversation's own folder when the worker wrote nothing of its own, because
/// a button that opens a directory which does not exist is worse than one that opens the parent
/// and lets somebody look.
pub fn worker_dir(conversation: &Path, worker_thread: &str) -> PathBuf {
    let own = conversation.join(worker_thread);
    if own.is_dir() {
        own
    } else {
        conversation.to_path_buf()
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

/// Delete one conversation's managed folder, and its now-empty project folder if applicable.
///
/// This deliberately changes §58's original decision to leave files behind. Once projects became
/// real folders (§105), keeping a deleted conversation's directory made Explorer contradict the
/// sidebar and made an empty project look alive. The confirmation dialog now names the files, so
/// the safe promise is one operation that removes both records rather than an undocumented orphan.
pub fn delete_thread(project: Option<&str>, thread_id: &str) -> Result<bool> {
    delete_thread_at(&root(), project, thread_id)
}

/// Delete a project's whole managed folder after all of its server conversations are gone.
///
/// The whole project, not merely the known thread directories: a researcher can add notes beside
/// those directories in Explorer, and a button labelled "Delete project" must either warn that
/// the folder goes too or leave a project-shaped orphan. The modal supplies that warning.
pub fn delete_project(project: &str) -> Result<bool> {
    delete_project_at(&root(), project)
}

/// A server-provided thread id may name one child and nothing else.
///
/// `thread_dir_in` is also used for reads, where a malformed id can only fail to find something.
/// Deletion is different: accepting `..`, a separator, or a Windows drive prefix here could turn
/// one confirmed conversation into a recursive delete outside Mini-Me's workspace (§154).
fn thread_segment(thread_id: &str) -> Result<&str> {
    let thread_id = thread_id.trim();
    let mut components = Path::new(thread_id).components();
    let one_normal = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if thread_id.is_empty()
        || thread_id.contains(['/', '\\'])
        || matches!(thread_id, "." | "..")
        || !one_normal
    {
        anyhow::bail!("refusing to delete an invalid conversation id: {thread_id:?}");
    }
    Ok(thread_id)
}

fn delete_thread_at(base: &Path, project: Option<&str>, thread_id: &str) -> Result<bool> {
    let thread_id = thread_segment(thread_id)?;
    let project = project.and_then(project_folder);
    let parent = project
        .as_ref()
        .map(|folder| base.join(folder))
        .unwrap_or_else(|| base.to_path_buf());

    // A project directory can be replaced by a junction or symlink in Explorer. Descending
    // through it would make `base/project/thread` look scoped while it actually names somewhere
    // else. Refuse the automatic cleanup; the server deletion still reports the exact error.
    if project.is_some()
        && std::fs::symlink_metadata(&parent)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        anyhow::bail!(
            "refusing to delete through the linked project folder {}",
            parent.display()
        );
    }

    let target = parent.join(thread_id);
    if !target.exists() && std::fs::symlink_metadata(&target).is_err() {
        return Ok(false);
    }
    remove_managed_tree(&target)?;

    // Never remove the workspace root. A named project's empty directory, however, is the
    // project according to §106; leaving it behind is the stale state this operation fixes.
    if project.is_some() {
        let _ = std::fs::remove_dir(&parent);
    }
    Ok(true)
}

fn delete_project_at(base: &Path, project: &str) -> Result<bool> {
    let folder = project_folder(project)
        .with_context(|| format!("{project:?} does not make a valid project folder"))?;
    let target = base.join(folder);
    if !target.exists() && std::fs::symlink_metadata(&target).is_err() {
        return Ok(false);
    }
    remove_managed_tree(&target)?;
    Ok(true)
}

/// Remove a real tree, or unlink a tree-shaped shortcut without following it.
///
/// Windows junctions matter here: scientific projects are often moved to OneDrive and linked
/// back. Recursive deletion must never walk through one and erase a directory outside the
/// workspace merely because the link itself sits inside it (§154).
fn remove_managed_tree(path: &Path) -> Result<()> {
    let link = std::fs::symlink_metadata(path)
        .with_context(|| format!("reading {} before deletion", path.display()))?;
    if link.file_type().is_symlink() {
        let points_to_directory = std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir());
        let result = if points_to_directory {
            std::fs::remove_dir(path)
        } else {
            std::fs::remove_file(path)
        };
        return result.with_context(|| format!("unlinking {}", path.display()));
    }
    if !link.is_dir() {
        anyhow::bail!("{} is not a conversation directory", path.display());
    }
    std::fs::remove_dir_all(path).with_context(|| format!("deleting {}", path.display()))
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

// No `images`. It listed the figures in a directory, oldest first, and existed because the
// transcript showed figures and nothing else. The transcript now shows every output a turn
// produced, so its one caller reads `outputs` — which finds the same files, carries what it
// already read about them, and does not need a second directory walk with a second sort rule.

/// One file a conversation produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub path: PathBuf,
    pub name: String,
    /// What it is, in the researcher's terms — the grouping key in the panel.
    pub kind: Kind,
    pub bytes: u64,
    /// When it was last written.
    ///
    /// Carried rather than discarded: [`outputs`] reads it to sort and used to throw it away, so
    /// every caller that wanted chronological order — or wanted to know whether its cached
    /// measurement of this file was still current — went back to the filesystem for a number
    /// that had already been read.
    pub modified: std::time::SystemTime,
}

/// Above this, an attached file is referenced where it lies rather than copied.
///
/// A researcher's PDF is a megabyte and belongs with the conversation; their genome table is forty
/// gigabytes and does not. The copy also happens on the thread that paints the window, so an
/// unbounded one is a frozen app as much as a full disk.
pub const ADOPT_LIMIT: u64 = 512 * 1024 * 1024;

/// Copy an attached file into the conversation's own folder, and say where it landed.
///
/// # Why attachments are copied at all
///
/// They were not, until `pdf_librarian` ran for the first time and the claims recorder said its
/// index pointed at `/mnt/c/Users/…/Downloads/Graph-neural-networks.pdf` (§227). That file is real
/// today. It is in a folder people empty. A conversation reopened next month has a library index,
/// a citation and an analysis all naming a path that resolves to nothing, and no way to tell that
/// from a paper nobody read.
///
/// Copying makes the attachment part of the conversation the same way its outputs are: it travels
/// with the folder, it appears in Outputs, and `execute` can reach it without leaving the
/// workspace — which is what §160's rule asks for and could not deliver while the input lived
/// somewhere else.
///
/// # What it will not do
///
/// **Never overwrite.** A name already taken by identical bytes is reused — attaching the same
/// paper twice should not litter — and a name taken by *different* bytes gets a suffix. Silently
/// replacing a file a previous turn produced would be the worst outcome available here.
pub fn adopt(folder: &Path, source: &Path) -> Result<PathBuf> {
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".to_string());
    std::fs::create_dir_all(folder)
        .with_context(|| format!("creating {}", folder.display()))?;

    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name.clone(), String::new()),
    };
    let wanted = std::fs::metadata(source)
        .with_context(|| format!("reading {}", source.display()))?
        .len();

    for attempt in 0..100 {
        let candidate = folder.join(match attempt {
            0 => name.clone(),
            n => format!("{stem}-{}{extension}", n + 1),
        });
        match std::fs::metadata(&candidate) {
            // Free.
            Err(_) => {
                std::fs::copy(source, &candidate)
                    .with_context(|| format!("copying {} in", source.display()))?;
                return Ok(candidate);
            }
            // Taken. Same size is treated as the same file: a byte comparison of two large
            // tables costs more than the collision it would avoid, and the suffix is harmless
            // when it is wrong.
            Ok(existing) if existing.len() == wanted => return Ok(candidate),
            Ok(_) => continue,
        }
    }
    anyhow::bail!("{name} already exists a hundred times over in this conversation")
}

/// The path this app can open, for a path the *agent* wrote down.
///
/// The two live on opposite sides of WSL, and `backend::wsl_path` only goes one way. A document
/// the librarian indexed is recorded however it saw it — relative to its working directory, or
/// absolute as `/mnt/c/…` — and neither opens in Explorer as written.
///
/// Returns `None` for anything that is not a local file, so a URL in `IndexedPaper.path` (which
/// its schema explicitly allows) does not become a launch of a file that is not there.
pub fn local_path(recorded: &str, thread: Option<&Path>) -> Option<PathBuf> {
    let recorded = recorded.trim();
    if recorded.is_empty() || recorded.contains("://") || recorded.starts_with("doi:") {
        return None;
    }
    let path = PathBuf::from(recorded);
    // `C:\Users\x` on Windows, `/home/piero/a.pdf` on Linux: openable exactly as written.
    if path.is_absolute() {
        return Some(path);
    }
    // A root with no drive letter, which on Windows is what *every* POSIX path is — the branch is
    // unreachable on Unix, where a root is always absolute, so it needs no `cfg!`.
    //
    // It must not reach the join below. `Path::join` **replaces** its base when the argument has a
    // root rather than extending it, so the conversation folder would be silently discarded and
    // `/home/piero/papers/a.pdf` would come back as `C:\home\piero\papers\a.pdf`: a row that
    // lights up and opens nothing (§267).
    if path.has_root() {
        return windows_path_for(recorded);
    }
    // Relative, which is what the skills ask for and what an adopted attachment produces: it is
    // relative to the conversation's own folder.
    thread.map(|dir| dir.join(recorded))
}

/// The Windows path for a rooted POSIX path the agent wrote down, or `None` when there is not one.
///
/// Only `/mnt/<drive>/…` crosses over: it came from `C:\…` and has to go back. Everything else
/// rooted is WSL's own filesystem or the sandbox's, which Explorer cannot open at all — and `None`
/// is the honest answer, because the caller uses it to decide whether the row lights up. A row
/// that lights up and opens nothing is worse than one that does not light up.
///
/// **No `cfg!` inside**, so both answers are testable on either platform. That is the point rather
/// than a nicety: this conversion lived behind `cfg!(windows)` at its only call site and therefore
/// had *no test at all* — it cannot run on the machine it is written on. The same blind spot let
/// `is_absolute()` reach a release build, and it is false on Windows for `/home/x` (§267).
fn windows_path_for(recorded: &str) -> Option<PathBuf> {
    let rest = recorded.strip_prefix("/mnt/")?;
    let mut chars = rest.chars();
    let (Some(drive), Some('/')) = (chars.next(), chars.next()) else {
        return None;
    };
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    // Both bytes were checked as ASCII above, so this is a character boundary.
    Some(PathBuf::from(format!(
        "{}:\\{}",
        drive.to_ascii_uppercase(),
        rest[2..].replace('/', "\\")
    )))
}

/// The folder inside a conversation where its own records are kept.
///
/// Named once because two readers look in it — `commands` and `claims` — and the producers spell
/// it in Python (`ledger.RECORD_DIR`). Three copies of a folder name is how §278 started.
const RECORD_DIR: &str = ".mini-me";

/// One command a conversation ran, as the overlay recorded it.
///
/// The producer is `overlay/minime_local/ledger.py`, and the shape is pinned by a fixture it
/// generates from its own code (`crates/app/tests/fixtures/command-record.jsonl`) — §264's
/// discipline, applied to the one record written by a Python file that ships beside this binary
/// rather than by the backend.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// When it ran, as the overlay wrote it. Kept as text: it is shown, never compared.
    pub at: String,
    /// The command, already clipped by the producer if it was enormous.
    pub text: String,
    /// Whether the producer clipped it, so the app can say so rather than imply the whole thing.
    pub clipped: bool,
    /// Its exit code. `None` when the producer could not read one.
    pub exit: Option<i64>,
    /// How long it took, in seconds.
    pub seconds: Option<f64>,
    /// **Absolute paths the command named that lie outside this conversation.**
    ///
    /// Named, not written — the producer is explicit about this and so is the UI that shows it. A
    /// command can write somewhere it never names, and nothing here can see that.
    pub outside: Vec<String>,
    /// The subset of [`Self::outside`] the command is **known** to have written.
    ///
    /// Decided by the producer from the file's own mtime against the window the command ran in, so
    /// it is a fact about the file rather than a guess about the string. This is the only list
    /// anything may act on: `pd.read_csv('/tmp/input.csv')` names a file the researcher owns, and
    /// treating a named path as output is how a tidy-up steals somebody's data.
    pub wrote: Vec<String>,
}

impl Command {
    /// Whether this one named something outside the conversation, written or merely mentioned.
    pub fn escaped(&self) -> bool {
        !self.outside.is_empty()
    }

    /// Whether this one is **known** to have written outside the conversation.
    pub fn left_files(&self) -> bool {
        !self.wrote.is_empty()
    }

    /// Whether it failed. `None` counts as not failed: an unreadable exit code is not evidence.
    pub fn failed(&self) -> bool {
        self.exit.is_some_and(|code| code != 0)
    }
}

/// Every command this conversation ran, oldest first.
///
/// A malformed line is skipped rather than failing the read. The record is a diagnostic written by
/// a process that may have been killed mid-write, and a half-written last line must not cost the
/// researcher the other four hundred.
pub fn commands(conversation: &Path) -> Vec<Command> {
    let path = conversation.join(RECORD_DIR).join("commands.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    decode_commands(&text)
}

/// Every string in a JSON array, ignoring anything that is not one.
fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Split out from [`commands`] so the shape can be tested against the producer's own fixture.
pub fn decode_commands(text: &str) -> Vec<Command> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|value| Command {
            at: value["at"].as_str().unwrap_or_default().to_string(),
            text: value["command"].as_str().unwrap_or_default().to_string(),
            clipped: value["clipped"].as_bool().unwrap_or(false),
            exit: value["exit"].as_i64(),
            seconds: value["seconds"].as_f64(),
            outside: strings(&value["outside"]),
            wrote: strings(&value["wrote"]),
        })
        .collect()
}

/// One structured answer a subagent gave, beside what the conversation's folder actually held.
///
/// The producer is `mini-me/backend/middleware/claims.py`, and the shape is pinned by a fixture it
/// generates from its own code (`crates/app/tests/fixtures/claim-record.jsonl`).
///
/// **It records and does not block**, deliberately — the enforcement rules worth writing are the
/// ones that come from failures actually seen. Which means nothing here ever stopped a turn, and
/// everything here is a thing that already happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    /// When the answer came back, as the producer wrote it. Text: it is shown, never compared.
    pub at: String,
    /// Which subagent answered — `pdf_librarian`, `dataverse_explorer`, and so on.
    pub source: String,
    /// The `response_format` it answered with. Shown because it says *what kind* of claim this is.
    pub schema: String,
    /// Whether any rule covers this schema at all.
    ///
    /// **Not "nothing was missing".** A schema with no path rule produces an empty [`Self::missing`]
    /// and has been examined by nobody, and the two are the same silence unless something keeps
    /// them apart. Reading it as "verified" is the failure the field exists to prevent, which is
    /// why an absent value here defaults to `false` rather than to `true`.
    pub checked: bool,
    /// How many paths the answer named.
    pub claimed: u64,
    /// The ones that name nothing in the conversation's folder. **This is the accusation.**
    pub missing: Vec<String>,
    /// Paths that are real but elsewhere — the researcher's own Downloads folder, typically.
    ///
    /// Not an accusation: those files exist. A durability warning, because they do not travel with
    /// the conversation and the Outputs panel cannot show them. The first version of the recorder
    /// called these "missing" and it read as *this file does not exist*, which was false.
    pub outside: Vec<String>,
    /// How many datasets a dataverse run recommended. `None` when it was never that question.
    ///
    /// `Some(0)` and `None` are different facts: a run that recommended nothing is what a
    /// researcher saw twice while a broken tool argument went unnoticed for weeks.
    pub datasets: Option<u64>,
    /// Recommended `persistent_id`s that appear nowhere in what the search returned.
    pub unsearched: Vec<String>,
    /// Why a check could not be made — set only when one was attempted and failed.
    ///
    /// A check that could not run and a check that found nothing are the same silence in a log,
    /// and the whole dataverse comparison failed on every turn for two days because of it.
    pub note: Option<String>,
}

impl Claim {
    /// Whether the workspace contradicts this answer: a file that is not there, or a dataset the
    /// search never returned. The only thing here strong enough to colour a line.
    pub fn contradicted(&self) -> bool {
        !self.missing.is_empty() || !self.unsearched.is_empty()
    }

    /// Whether it leaned on a file from outside this conversation, which will not travel with it.
    pub fn used_outside(&self) -> bool {
        !self.outside.is_empty()
    }

    /// Whether nothing looked at this answer at all — see [`Self::checked`].
    pub fn unexamined(&self) -> bool {
        !self.checked
    }
}

/// Every claim this conversation's subagents recorded, oldest first.
///
/// Same tolerance as [`commands`]: a malformed line is skipped rather than costing the researcher
/// the others, because this is a diagnostic written by a process that may have been killed.
pub fn claims(conversation: &Path) -> Vec<Claim> {
    let path = conversation.join(RECORD_DIR).join("claims.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    decode_claims(&text)
}

/// Split out from [`claims`] so the shape can be tested against the producer's own fixture.
pub fn decode_claims(text: &str) -> Vec<Claim> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|value| Claim {
            at: value["at"].as_str().unwrap_or_default().to_string(),
            source: value["source"].as_str().unwrap_or_default().to_string(),
            schema: value["schema"].as_str().unwrap_or_default().to_string(),
            // Absent means nobody looked. Defaulting the other way would turn a truncated line
            // into a clean bill of health, which is the one answer this must never invent.
            checked: value["checked"].as_bool().unwrap_or(false),
            claimed: value["claimed"].as_u64().unwrap_or(0),
            missing: strings(&value["missing"]),
            outside: strings(&value["outside"]),
            datasets: value["datasets"].as_u64(),
            unsearched: strings(&value["unsearched"]),
            note: value["note"].as_str().map(str::to_string),
        })
        .collect()
}

/// Every dataset this conversation's searches returned, in the order they were found.
///
/// **The API's answer, not the model's account of it.** Until §290 the datasets panel rendered
/// `DataVerseSearchResults.datasets` — seven fields per row, each one retyped by a language model
/// out of a file it had just read — and on a real turn six of six `persistent_id`s were composed
/// rather than copied. A researcher was one click from pasting a fabricated DOI into a paper.
///
/// A row here comes from `dataverse_search.json`, which `middleware/dataverse_first.py` writes
/// from what the search actually returned. A fabricated identifier has no row to appear in.
///
/// The shape is pinned by a fixture the producer generates from its own code
/// (`crates/app/tests/fixtures/dataverse-search.json`).
pub fn datasets(conversation: &Path) -> Vec<crate::protocol::Dataset> {
    let path = conversation.join("dataverse_search.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    decode_datasets(&text)
}

/// Split out from [`datasets`] so the shape can be tested against the producer's own fixture.
///
/// A row with no identifier is kept rather than dropped: the producer emits one when it meets a
/// record whose layout it does not know, and a search that quietly lost three of eighteen rows is
/// the failure mode that would be hardest to notice.
pub fn decode_datasets(text: &str) -> Vec<crate::protocol::Dataset> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(rows) = value.as_array() else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| crate::protocol::Dataset {
            title: row["title"].as_str().unwrap_or_default().to_string(),
            persistent_id: row["persistent_id"].as_str().unwrap_or_default().to_string(),
            // `None` rather than an empty string, because the row's own `link()` falls back to
            // building one from the identifier and an empty string is not a URL.
            link: row["link"]
                .as_str()
                .map(str::trim)
                .filter(|link| !link.is_empty())
                .map(str::to_string),
            description: row["description"].as_str().unwrap_or_default().to_string(),
            authors: strings(&row["authors"]),
            file_count: row["file_count"].as_u64(),
            repository: row["repository"]
                .as_str()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        })
        .collect()
}

/// What this conversation's searches said they found, beside what they returned.
///
/// **A count with no denominator is not an answer.** Until §299 the MCP read `total_count` to
/// decide when to stop paging and never returned it, so twenty-nine rows on screen were
/// indistinguishable from a thorough search of a twenty-nine dataset corpus. A researcher
/// choosing what to cite has to be able to see which one they are looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchTotals {
    /// What Dataverse reported matched the query. `0` means no search could say — which is a
    /// third state, not zero matches, and is why [`Self::denominator`] answers `None` for it.
    pub total: u64,
    /// How many records the conversation's folder actually holds.
    pub kept: u64,
    /// Whether every match was retrieved. False for a capped search, a failed page, or a
    /// deployment that reports no total at all.
    pub complete: bool,
}

impl SearchTotals {
    /// The number to show after the count, when one can honestly be shown.
    ///
    /// `None` when no search reported a total, so the panel says `29` rather than `29 of 0` —
    /// the second is a lie about the corpus, and the more confident-looking of the two. Also
    /// `None` when the total is not larger than what is held: a denominator equal to the count is
    /// noise, and one smaller than it reads as a bug.
    pub fn denominator(&self) -> Option<u64> {
        (self.total > 0 && self.total > self.kept).then_some(self.total)
    }
}

/// Read what the searches reported, or nothing if they reported nothing.
pub fn search_totals(conversation: &Path) -> SearchTotals {
    let path = conversation
        .join(RECORD_DIR)
        .join("dataverse_search.meta.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return SearchTotals::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return SearchTotals::default();
    };
    SearchTotals {
        total: value["total_count"].as_u64().unwrap_or(0),
        kept: value["kept"].as_u64().unwrap_or(0),
        complete: value["complete"].as_bool().unwrap_or(false),
    }
}

/// A bounded view of everything a conversation wrote.
///
/// `truncated` is deliberately part of the result rather than a log line. The person looking at
/// the Outputs panel is the one who needs to know that the folder contains more than the app is
/// showing; silently stopping at a safety limit would recreate §117 with a larger threshold.
pub struct OutputListing {
    pub groups: Vec<(Kind, Vec<Output>)>,
    pub truncated: bool,
}

/// Enough depth for the named output folders analysis tools normally create, without walking a
/// virtualenv or a copied dataset tree forever. Four means files remain visible through
/// `turn/analysis/tables/final/file.csv`; anything deeper is still reachable through Explorer.
const MAX_OUTPUT_DEPTH: usize = 4;
/// Bound directory entries as well as files: a tree of thousands of empty folders is just as
/// capable of freezing a render-time scan as a tree of thousands of artifacts (plan §117).
const MAX_OUTPUT_ENTRIES: usize = 2_048;
const MAX_OUTPUT_FILES: usize = 512;

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
    // No `label`. The panel used to head each group with "Figures" / "Data" / "Documents", and
    // the redesign replaced those headings with a glyph on each row — three words of chrome per
    // group, in a 330px column, to say what the icon beside the filename already said. The kind
    // still decides the *order* files appear in, which is the part that was doing work.

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
    output_listing(dir).groups
}

/// What the backend writes down about who produced each file, inside the conversation's folder.
///
/// Dot-prefixed, so [`collect_outputs`] already skips it: the record of what made the files never
/// turns up as one of them.
pub const AUTHORSHIP: &str = ".authorship.jsonl";

/// Who wrote each file, as the backend recorded it at the time.
///
/// **Read rather than inferred.** §199 could name a background worker, because a worker runs on
/// its own thread and writes into a folder named after it. The specialists a conversation
/// consults share one thread and one directory, so the client had nothing to go on and correctly
/// said nothing. `overlay/minime_local/authorship.py` now writes the fact down as it happens —
/// the delegation's own name, and the interval of the command that produced the file — and this
/// reads it back (§201).
///
/// One JSON object per line, appended. **The last line for a path wins**, which is what a
/// filesystem does anyway: a file rewritten by a second specialist belongs to the second one. A
/// malformed line is skipped rather than failing the read, because a truncated final line is what
/// a crash mid-append leaves and losing one attribution is better than losing all of them.
///
/// Keys are the same relative paths [`Output::name`] carries, with separators normalised —
/// the manifest is written with forward slashes and Windows lists files with backslashes, which
/// is not a difference a researcher should be able to see.
pub fn authorship(dir: &Path) -> std::collections::HashMap<String, String> {
    let mut who = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(dir.join(AUTHORSHIP)) else {
        return who;
    };
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let path = record.get("path").and_then(serde_json::Value::as_str);
        let agent = record.get("agent").and_then(serde_json::Value::as_str);
        if let (Some(path), Some(agent)) = (path, agent) {
            let agent = agent.trim();
            if !path.is_empty() && !agent.is_empty() {
                who.insert(normalise_separators(path), agent.to_string());
            }
        }
    }
    who
}

/// One spelling for a relative path, whichever platform listed it.
pub fn normalise_separators(path: &str) -> String {
    path.replace('\\', "/")
}

/// The same grouped files as [`outputs`], plus whether its documented safety bounds hid any.
///
/// The ordinary callers only need the files. The Outputs panel uses this fuller answer so a
/// bounded walk never pretends it was exhaustive (plan §117).
pub fn output_listing(dir: &Path) -> OutputListing {
    let mut found = Vec::new();
    let mut entries_seen = 0;
    let mut truncated = false;
    collect_outputs(dir, dir, 0, &mut found, &mut entries_seen, &mut truncated);

    // Newest first: the file someone wants is nearly always the one just written.
    found.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut groups: Vec<(Kind, Vec<Output>)> = Vec::new();
    for kind in [Kind::Figure, Kind::Data, Kind::Document, Kind::Other] {
        let items: Vec<Output> = found
            .iter()
            .filter(|output| output.kind == kind)
            .cloned()
            .collect();
        if !items.is_empty() {
            groups.push((kind, items));
        }
    }
    OutputListing { groups, truncated }
}

fn collect_outputs(
    root: &Path,
    dir: &Path,
    depth: usize,
    found: &mut Vec<Output>,
    entries_seen: &mut usize,
    truncated: &mut bool,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if *entries_seen >= MAX_OUTPUT_ENTRIES || found.len() >= MAX_OUTPUT_FILES {
            *truncated = true;
            break;
        }
        *entries_seen += 1;

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // A symlinked directory can lead outside the conversation or back to an ancestor. The
        // panel is an index of files the turn wrote here, not a general filesystem crawler.
        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        let Some(base_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Dotfiles and tool caches are the agent's business, not the researcher's. Apply the
        // existing top-level rule at every depth now that §117 makes those depths visible.
        //
        // `memories/` joins them: it is where the agent keeps its own instructions between turns,
        // and `memories\instructions.txt` showing up in a panel headed FILES invites a researcher
        // to open, edit or delete a file that is not theirs and whose loss changes how the agent
        // behaves (§173).
        // `provenance.json` joins them: it is the app's own record of which specialists ran, written
        // beside the conversation's files so the road strip survives a reload. A researcher reading
        // a panel headed FILES has no use for it and every reason to think it is an output they
        // asked for — *"I think we shouldn't show the provenance json file"* (§204).
        if base_name.starts_with('.')
            || base_name == "__pycache__"
            || base_name == "memories"
            || base_name == crate::provenance::FILENAME
        {
            continue;
        }

        if file_type.is_dir() {
            if depth < MAX_OUTPUT_DEPTH {
                collect_outputs(root, &path, depth + 1, found, entries_seen, truncated);
            } else {
                *truncated = true;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Some(relative) = path.strip_prefix(root).ok() else {
            continue;
        };
        let Some(name) = relative.to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        found.push(Output {
            kind: Kind::of(&path),
            path,
            name,
            bytes: metadata.len(),
            modified,
        });
    }
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

/// What a file is, past its name and its size.
///
/// # Why only two kinds
///
/// The design asks each file row to carry "the real shape of the file" and gives three examples:
/// `1,204 rows · 418 KB`, `1600 × 900 · 92 KB`, and `6 pages · 8 references`. The first two are
/// derivable here and are; the third is not, and is therefore absent.
///
/// A PDF's page count lives in its page tree, and reading that means either a PDF parser — a
/// dependency on a machine where `cargo build` is already the riskiest step a colleague performs
/// (see this crate's `Cargo.toml`, which argues the point for `flate2` and `keyring`) — or one of
/// the folklore heuristics: counting `/Type /Page`, which double-counts `/Pages` nodes and misses
/// anything in an object stream, or grepping `/Count`, which finds the first of several. Both
/// produce a plausible number that is sometimes wrong, shown in a panel a researcher is meant to
/// trust. Reference counts are not in the file at all in any recoverable form.
///
/// So a PDF says its size, which is true, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A delimited table: data rows (the header is not one) and columns.
    Table { rows: u64, columns: usize },
    /// Pixel dimensions.
    Image { width: u32, height: u32 },
    /// Nothing beyond the size — either the kind carries no shape, or reading it failed.
    Plain,
}

impl Shape {
    /// The sub-line, with the size on the end. `Plain` is the size alone.
    pub fn describe(self, bytes: u64) -> String {
        let size = human_size(bytes);
        match self {
            Shape::Table { rows, columns } => {
                format!("{} rows · {columns} cols · {size}", thousands(rows))
            }
            Shape::Image { width, height } => format!("{width} × {height} · {size}"),
            Shape::Plain => size,
        }
    }
}

/// Past this, the shape is not worth the read.
///
/// A 400 MB export would be counted line by line on the thread that draws the window. The panel
/// says the size instead, which is the honest answer to "how big is this" anyway.
const SHAPE_BUDGET: u64 = 64 * 1024 * 1024;

/// Measure a file, or [`Shape::Plain`] if it has no measurable shape or cannot be read.
///
/// Never an error: this decorates a row in a panel. A file mid-write, on a disconnected drive,
/// or in an encoding we cannot read should cost that row its sub-line, not the panel.
pub fn shape(path: &Path, bytes: u64) -> Shape {
    if bytes > SHAPE_BUDGET {
        return Shape::Plain;
    }
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => table(path, b','),
        "tsv" => table(path, b'\t'),
        extension if IMAGE_EXTENSIONS.contains(&extension) => {
            std::fs::read(path).map(|data| image_shape(&data)).unwrap_or(Shape::Plain)
        }
        _ => Shape::Plain,
    }
}

/// Count the records and fields of a delimited file.
///
/// **Quote-aware**, which is not fussiness: a delimiter inside `"Cusco, Peru"` counted as a
/// column boundary makes the column count wrong for exactly the files a researcher exports from
/// a spreadsheet, and a newline inside a quoted field makes the row count wrong the same way.
/// A number in this panel is read as a fact about the data.
pub(crate) fn count_records(data: &[u8], delimiter: u8) -> Shape {
    let mut rows: u64 = 0;
    let mut columns = 0usize;
    let mut fields = 1usize;
    let mut quoted = false;
    let mut started = false;
    let mut index = 0usize;
    while index < data.len() {
        let byte = data[index];
        if quoted {
            // `""` inside a quoted field is an escaped quote, not the end of one.
            if byte == b'"' {
                if data.get(index + 1) == Some(&b'"') {
                    index += 2;
                    continue;
                }
                quoted = false;
            }
            started = true;
            index += 1;
            continue;
        }
        match byte {
            b'"' => {
                quoted = true;
                started = true;
            }
            b'\r' => {}
            b'\n' => {
                if started {
                    // The first record is the header, so it is a column count, not a row.
                    if columns == 0 {
                        columns = fields;
                    } else {
                        rows += 1;
                    }
                }
                fields = 1;
                started = false;
            }
            byte if byte == delimiter => {
                fields += 1;
                started = true;
            }
            _ => started = true,
        }
        index += 1;
    }
    // A last line with no trailing newline is still a record.
    if started {
        if columns == 0 {
            columns = fields;
        } else {
            rows += 1;
        }
    }
    if columns == 0 {
        return Shape::Plain;
    }
    Shape::Table { rows, columns }
}

fn table(path: &Path, delimiter: u8) -> Shape {
    std::fs::read(path)
        .map(|data| count_records(&data, delimiter))
        .unwrap_or(Shape::Plain)
}

/// The delimiter a file's extension implies, for the two this app previews.
fn delimiter_of(path: &Path) -> Option<u8> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => Some(b','),
        "tsv" => Some(b'\t'),
        _ => None,
    }
}

/// One record split into fields, honouring quotes.
///
/// The same rules as [`count_records`] — a delimiter inside quotes is a character, `""` is an
/// escaped quote — because a preview whose columns disagreed with the column count printed above
/// it would be two views of one file contradicting each other on screen.
pub(crate) fn split_record(line: &str, delimiter: u8) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted => {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            c if !quoted && c as u32 == delimiter as u32 => {
                fields.push(std::mem::take(&mut field));
            }
            '\r' | '\n' if !quoted => {}
            c => field.push(c),
        }
    }
    fields.push(field);
    fields
}

/// The first few rows of a delimited file — header first — for the card in the transcript.
///
/// `None` for anything that is not a table we can split. Bounded by `rows`, and the read is
/// bounded too: [`head`] stops after that many lines rather than pulling a 400 MB export into
/// memory to show three rows of it.
///
/// A quoted field containing a newline will be cut short here, because the read is line-based
/// while [`count_records`] is byte-based. That costs a preview a cell; it does not affect the
/// row and column counts, which are the numbers anyone acts on.
pub fn table_preview(path: &Path, rows: usize) -> Option<Vec<Vec<String>>> {
    let delimiter = delimiter_of(path)?;
    let text = head(path, rows).ok()?;
    let found: Vec<Vec<String>> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| split_record(line, delimiter))
        .collect();
    // A header on its own is not a preview of anything.
    (found.len() > 1).then_some(found)
}

/// Pixel dimensions, straight out of the header.
///
/// Four formats, four headers, no decoder — the bytes that carry width and height sit within the
/// first few dozen of each file and are documented in each specification. `img()` already decodes
/// these to draw them; this only needs the numbers, and reaching for an image crate to get two
/// integers would add a dependency tree to a build that has to succeed on a colleague's Windows
/// machine with nothing installed.
pub(crate) fn image_shape(data: &[u8]) -> Shape {
    let be = |at: usize| -> Option<u32> {
        let slice: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(u32::from_be_bytes(slice))
    };
    let le16 = |at: usize| -> Option<u32> {
        let slice: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
        Some(u16::from_le_bytes(slice) as u32)
    };

    // PNG: an 8-byte signature, then the IHDR chunk whose first two fields are the dimensions.
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        if let (Some(width), Some(height)) = (be(16), be(20)) {
            return Shape::Image { width, height };
        }
    }
    // GIF: the logical screen descriptor, little-endian, right after `GIF87a`/`GIF89a`.
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        if let (Some(width), Some(height)) = (le16(6), le16(8)) {
            return Shape::Image { width, height };
        }
    }
    // WebP: a RIFF container whose VP8 flavour decides where the size lives.
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        match data.get(12..16) {
            // Lossy: a 3-byte start code, then 14-bit width and height.
            Some(b"VP8 ") => {
                if let (Some(width), Some(height)) = (le16(26), le16(28)) {
                    return Shape::Image {
                        width: width & 0x3fff,
                        height: height & 0x3fff,
                    };
                }
            }
            // Lossless: 14-bit each, packed across four bytes, and stored one less than actual.
            Some(b"VP8L") => {
                if let Some(bits) = data.get(21..25) {
                    let packed = u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]]);
                    return Shape::Image {
                        width: (packed & 0x3fff) + 1,
                        height: ((packed >> 14) & 0x3fff) + 1,
                    };
                }
            }
            // Extended: 24-bit each, little-endian, also stored one less than actual.
            Some(b"VP8X") => {
                if let Some(bits) = data.get(24..30) {
                    let width = u32::from_le_bytes([bits[0], bits[1], bits[2], 0]) + 1;
                    let height = u32::from_le_bytes([bits[3], bits[4], bits[5], 0]) + 1;
                    return Shape::Image { width, height };
                }
            }
            _ => {}
        }
    }
    // JPEG: no fixed offset. Walk the marker segments to the frame header, whose payload
    // begins with precision, height, width.
    if data.starts_with(b"\xff\xd8") {
        let mut at = 2usize;
        while at + 3 < data.len() {
            if data[at] != 0xff {
                at += 1;
                continue;
            }
            let marker = data[at + 1];
            // Padding and the standalone markers carry no length field to skip over.
            if marker == 0xff {
                at += 1;
                continue;
            }
            if matches!(marker, 0xd8 | 0x01) || (0xd0..=0xd7).contains(&marker) {
                at += 2;
                continue;
            }
            let length = match data.get(at + 2..at + 4) {
                Some(bytes) => u16::from_be_bytes([bytes[0], bytes[1]]) as usize,
                None => break,
            };
            // Every SOFn *except* the four that are not frame headers: DHT, JPG, DAC, DNL.
            let is_frame = (0xc0..=0xcf).contains(&marker)
                && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
            if is_frame {
                if let Some(bytes) = data.get(at + 5..at + 9) {
                    return Shape::Image {
                        width: u16::from_be_bytes([bytes[2], bytes[3]]) as u32,
                        height: u16::from_be_bytes([bytes[0], bytes[1]]) as u32,
                    };
                }
                break;
            }
            if length < 2 {
                break;
            }
            at += 2 + length;
        }
    }
    Shape::Plain
}

/// `1204` → `1,204`.
///
/// A four-digit row count is read as a four-digit number either way; a seven-digit one is not.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (seen, digit) in digits.chars().enumerate() {
        if seen > 0 && (digits.len() - seen).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
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

/// Open a URL in the researcher's browser.
///
/// **Separate from [`open`], which must not be handed one.** That function conjures a missing
/// directory before launching the file manager — a deliberate fix from §50 — so calling it with
/// `https://doi.org/…` would create a folder called `https:` in the working directory and then
/// point Explorer at it. Same three platform launchers, none of the filesystem behaviour.
///
/// Only `http` and `https`. A citation is a line of text the *model* wrote, so it is untrusted
/// input reaching a process launcher: `file://` would open anything on the disk, and on Windows
/// `explorer.exe` will act on a UNC path or a shell verb given the chance. Anything else is
/// refused rather than passed along.
pub fn browse(url: &str) -> Result<()> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        anyhow::bail!("{url:?} is not an http(s) URL");
    }
    let launcher = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(launcher)
        .arg(url)
        .spawn()
        .with_context(|| format!("could not open {url}"))?;
    Ok(())
}

#[cfg(test)]
mod worker_tests {
    use super::*;

    #[test]
    fn a_worker_opens_its_own_folder_or_the_conversation_that_started_it() {
        let root = std::env::temp_dir().join(format!("minime-worker-{}", std::process::id()));
        let conversation = root.join("conv-1");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&conversation).expect("a conversation folder");

        // Nothing of its own yet: the parent is the honest answer, because a button that opens a
        // directory which does not exist is worse than one that opens the folder above it.
        assert_eq!(worker_dir(&conversation, "worker-9"), conversation);

        // Once it has written something, its own folder — the shape §151 verified on a live run,
        // where the worker's files landed inside the conversation rather than beside it.
        let own = conversation.join("worker-9");
        std::fs::create_dir_all(&own).expect("a worker folder");
        assert_eq!(worker_dir(&conversation, "worker-9"), own);

        // A file of that name is not a folder to open, and must not be offered as one.
        let decoy = conversation.join("worker-8");
        std::fs::write(&decoy, "not a directory").expect("write");
        assert_eq!(worker_dir(&conversation, "worker-8"), conversation);

        let _ = std::fs::remove_dir_all(&root);
    }
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
            .env("PYTHONIOENCODING", "utf-8")
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
            .env("PYTHONIOENCODING", "utf-8")
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

    #[test]
    fn a_project_folder_is_told_apart_from_a_conversations_own() {
        // Both sit directly under the workspace root, so the shape of the name is the only
        // discriminator — and getting it wrong would list every ungrouped conversation as a
        // project heading (§167).
        assert!(looks_like_thread_id("019ff651-0cd7-71c1-9f17-5fc9250b10d1"));
        assert!(looks_like_thread_id("019FF651-0CD7-71C1-9F17-5FC9250B10D1"));
        // A name a researcher would type, however UUID-ish it looks.
        assert!(!looks_like_thread_id("Late blight"));
        assert!(!looks_like_thread_id("2026-08-12-trial"));
        assert!(!looks_like_thread_id("019ff651-0cd7-71c1-9f17-5fc9250b10d"), "35 characters");
        assert!(!looks_like_thread_id("019ff651_0cd7_71c1_9f17_5fc9250b10d1"), "wrong separator");
        assert!(!looks_like_thread_id("019ff651-0cd7-71c1-9f17-5fc9250b10dZ"), "not hex");
    }

    #[test]
    fn a_named_project_exists_as_a_folder_before_anything_is_filed_into_it() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-empty-project-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&base).expect("workspace");
        // `root()` reads an environment variable, and so does half of `backend`'s suite. The
        // shared lock is what keeps two tests from redirecting the workspace out from under each
        // other — a failure that would look like this feature not working.
        let _env = crate::backend::env_lock::hold();
        let previous = std::env::var_os(WORKSPACE_ENV);
        // SAFETY: the lock above makes this the only thread touching the variable; restored below.
        unsafe { std::env::set_var(WORKSPACE_ENV, &base) };

        // Nothing yet.
        assert!(projects().is_empty());

        // Naming one creates the directory, and the directory is what the sidebar reads: this is
        // the whole of §167. `create_project` reports the name the folder actually carries.
        assert_eq!(create_project("Late blight").unwrap(), "Late blight");
        assert_eq!(projects(), vec!["Late blight".to_string()]);

        // A conversation's own folder sits beside it at the root and is not a project.
        std::fs::create_dir_all(base.join("019ff651-0cd7-71c1-9f17-5fc9250b10d1")).expect("thread");
        // Nor is a file, which is what keeps `subagents.json` out of the sidebar.
        std::fs::write(base.join("subagents.json"), b"{}").expect("registry");
        assert_eq!(projects(), vec!["Late blight".to_string()]);

        // A name a path cannot hold is rewritten, and the rewritten one is what comes back — so
        // the metadata and the folder cannot end up spelling the same project two ways.
        let folder = create_project("Q1/Q2").unwrap();
        assert_eq!(folder, "Q1_Q2");
        assert!(projects().contains(&folder));

        match previous {
            Some(value) => unsafe { std::env::set_var(WORKSPACE_ENV, value) },
            None => unsafe { std::env::remove_var(WORKSPACE_ENV) },
        }
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn deleting_a_conversation_deletes_its_files_but_not_its_neighbours() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-delete-one-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let first = base.join("Late blight").join("thread-one");
        let second = base.join("Late blight").join("thread-two");
        std::fs::create_dir_all(first.join("eda_outputs")).expect("first conversation");
        std::fs::write(first.join("eda_outputs/plot.png"), b"plot").expect("first output");
        std::fs::create_dir_all(&second).expect("second conversation");
        std::fs::write(second.join("notes.md"), b"notes").expect("second output");

        assert!(delete_thread_at(&base, Some("Late blight"), "thread-one").unwrap());
        assert!(!first.exists(), "the confirmed conversation goes");
        assert!(second.join("notes.md").is_file(), "its neighbour survives");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn deleting_the_last_conversation_removes_its_empty_project_folder() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-delete-last-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let project = base.join("Late blight");
        std::fs::create_dir_all(project.join("thread-one")).expect("conversation");

        assert!(delete_thread_at(&base, Some("Late blight"), "thread-one").unwrap());
        assert!(!project.exists(), "an empty project is not left behind");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn deleting_a_project_removes_its_whole_folder_and_nothing_beside_it() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-delete-project-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let doomed = base.join("Late blight");
        let kept = base.join("Yield trials").join("thread-three");
        std::fs::create_dir_all(doomed.join("thread-one/plots")).expect("first conversation");
        std::fs::create_dir_all(doomed.join("thread-two")).expect("second conversation");
        // A project delete names the whole folder, including files a researcher placed beside
        // conversation directories; the modal must therefore say exactly that (§154).
        std::fs::write(doomed.join("project-notes.md"), b"notes").expect("project note");
        std::fs::create_dir_all(&kept).expect("neighbour project");

        assert!(delete_project_at(&base, "Late blight").unwrap());
        assert!(!doomed.exists(), "the project folder goes");
        assert!(kept.is_dir(), "the neighbouring project survives");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_malformed_conversation_id_cannot_escape_the_workspace_during_deletion() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-delete-escape-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let outside = base.with_extension("outside");
        std::fs::create_dir_all(&base).expect("workspace");
        std::fs::create_dir_all(&outside).expect("outside sentinel");

        for hostile in ["..", "../outside", r"..\outside", "a/b", r"a\b", ""] {
            assert!(
                delete_thread_at(&base, None, hostile).is_err(),
                "accepted {hostile:?}"
            );
        }
        assert!(
            outside.is_dir(),
            "nothing outside the workspace was touched"
        );

        std::fs::remove_dir_all(&base).ok();
        std::fs::remove_dir_all(&outside).ok();
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
mod authorship_tests {
    use super::*;

    /// The contract with `overlay/minime_local/authorship.py`, checked against the bytes it
    /// actually writes rather than against a struct both sides agree on in prose.
    #[test]
    fn the_last_writer_of_a_file_owns_it() {
        let dir = std::env::temp_dir().join(format!("minime-authorship-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(
            dir.join(AUTHORSHIP),
            concat!(
                r#"{"path": "plots/yield.png", "agent": "exploratory_data_analysis", "at": 1.0}"#,
                "\n",
                r#"{"path": "notes.md", "agent": "coordinator", "at": 2.0}"#,
                "\n",
                // Rewritten later by someone else. A filesystem lets that happen, so the record
                // has to as well — the newest line wins.
                r#"{"path": "notes.md", "agent": "report_writer", "at": 3.0}"#,
                "\n",
                // What a crash mid-append leaves. Skipped, rather than costing every line above.
                r#"{"path": "half-writ"#,
                "\n",
                // Neither field may be blank: an entry that names no author is not an author.
                r#"{"path": "orphan.csv", "agent": "  "}"#,
                "\n",
            ),
        )
        .expect("manifest");

        let who = authorship(&dir);
        assert_eq!(
            who.get("plots/yield.png").map(String::as_str),
            Some("exploratory_data_analysis")
        );
        assert_eq!(who.get("notes.md").map(String::as_str), Some("report_writer"));
        assert!(!who.contains_key("orphan.csv"));
        assert_eq!(who.len(), 2, "the torn line is skipped, not fatal");

        // No record at all is not an error — it is every conversation that ran before this
        // existed, and every one on a backend without the overlay armed.
        std::fs::remove_file(dir.join(AUTHORSHIP)).ok();
        assert!(authorship(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_spelling_of_a_path_whichever_platform_listed_it() {
        // The backend writes `plots/yield.png`; Windows lists `plots\yield.png`. Windows is
        // ~98% of these users, so this is the ordinary case, not the exotic one.
        assert_eq!(normalise_separators(r"plots\yield.png"), "plots/yield.png");
        assert_eq!(normalise_separators("plots/yield.png"), "plots/yield.png");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §263: the plots are decoded when the run finishes, so opening an experiment reads them.
    #[test]
    fn an_experiments_figures_are_found_on_disk_in_order() {
        let dir = std::env::temp_dir().join(format!("minime-figs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let run = "8e5d2eaa-f067-4193-9400-d555e4607c41";
        let inside = dir.join("discovery").join(run).join("node_2_0");
        std::fs::create_dir_all(&inside).expect("a temp dir");
        // Written out of order on purpose: the directory yields whatever it likes.
        for name in ["figure-02.png", "figure-01.png", "figure-03.jpg", "notes.txt"] {
            std::fs::write(inside.join(name), b"x").expect("write");
        }

        let found = discovery_figures(&dir, run, "node_2_0");
        let names: Vec<String> = found
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // Sorted, and only images: `figure-01` before `figure-02`, and no `notes.txt`.
        assert_eq!(names, ["figure-01.png", "figure-02.png", "figure-03.jpg"]);

        // An experiment with nothing decoded yet is empty, which is how the caller knows to ask.
        assert!(discovery_figures(&dir, run, "node_9_9").is_empty());

        // Both ids come from a payload, so neither is trusted into a path.
        for hostile in ["../../..", "..", "a/b", "x.y", ""] {
            assert!(discovery_figures(&dir, run, hostile).is_empty(), "{hostile}");
            assert!(discovery_figures(&dir, hostile, "node_2_0").is_empty(), "{hostile}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// §261: a finished run's experiments are already on disk, so opening it should not wait on
    /// the service.
    #[test]
    fn a_finished_runs_record_is_read_from_the_conversations_own_folder() {
        let dir = std::env::temp_dir().join(format!("minime-discovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("discovery")).expect("a temp dir");
        let run = "8e5d2eaa-f067-4193-9400-d555e4607c41";
        std::fs::write(
            dir.join("discovery").join(format!("{run}.json")),
            serde_json::json!({
                "run_id": run,
                "has_job_completed": true,
                "experiments": [{"experiment_id": "node_2_0", "id_in_run": 1, "hypothesis": "h"}]
            })
            .to_string(),
        )
        .expect("write");

        let record = discovery_record(&dir, run).expect("the stored run");
        assert_eq!(record["run_id"], run);
        assert_eq!(record["experiments"].as_array().map(Vec::len), Some(1));

        // A run still producing has no file yet, which is how the caller knows to ask the service.
        assert!(discovery_record(&dir, "11111111-2222-3333-4444-555555555555").is_none());

        // The run id comes from an artifact, so it is checked rather than trusted: a traversal
        // must not read a file from somewhere else.
        for hostile in ["../../etc/passwd", "..", "a/b", "a.json", ""] {
            assert!(discovery_record(&dir, hostile).is_none(), "{hostile}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// §250: the banner reappeared every launch for a run that finished the day before.
    #[test]
    fn a_run_is_only_ever_announced_once() {
        // The repo's idiom for an env override: one lock, one temp dir named after the process.
        let _env = crate::backend::env_lock::hold();
        let dir = std::env::temp_dir().join(format!("minime-announced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");
        // SAFETY: the lock above serialises every test that touches the environment.
        unsafe { std::env::set_var("MINIME_DATA_DIR", &dir) };

        assert!(
            announced_runs().is_empty(),
            "nothing announced on a first launch"
        );

        remember_announced(&["task-a".to_string(), "task-b".to_string()]);
        let known = announced_runs();
        assert!(known.contains("task-a"));
        assert!(known.contains("task-b"));

        // The whole point: the sweep re-collects the same finished run forever, because "finished"
        // stays true — so the second launch has to stay quiet about it.
        remember_announced(&["task-a".to_string()]);
        assert_eq!(announced_runs().len(), 2, "no duplicates, and nothing lost");

        remember_announced(&["task-c".to_string()]);
        assert_eq!(announced_runs().len(), 3);

        // Blank lines and stray whitespace are not ids.
        std::fs::write(dir.join("announced-runs.txt"), "\n  \ntask-a\n\n").expect("write");
        let reread = announced_runs();
        assert_eq!(reread.len(), 1);
        assert!(reread.contains("task-a"));

        // Nothing to record is a no-op, not an empty file.
        std::fs::remove_file(dir.join("announced-runs.txt")).expect("remove");
        remember_announced(&[]);
        assert!(!dir.join("announced-runs.txt").exists());
        // And a missing file means "nothing announced", which re-announces at worst.
        assert!(announced_runs().is_empty());

        unsafe { std::env::remove_var("MINIME_DATA_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_registry_says_which_specialists_reach_asta() {
        // The three real descriptions, verbatim from `backend/subagents.py` (2026-08-07). The
        // list is read rather than copied, so this test is about the *rule* holding against the
        // text upstream actually writes.
        let registry = parse_registry(
            &serde_json::json!({
                "format": 1,
                "subagents": [
                    {"name": "academic_researcher",
                     "description": "Conducts research using Asta tools (via MCP tools)."},
                    {"name": "hypothesis_generator",
                     "description": "Generate literature-grounded scientific theories and hypotheses for a research question using the Asta Theorizer pipeline."},
                    {"name": "data_voyager",
                     "description": "Run the Asta DataVoyager pipeline (`asta analyze-data`) to generate and test hypotheses."},
                    {"name": "report_writer",
                     "description": "Write a polished report from the findings and recommendations."},
                    {"name": "dataverse_explorer",
                     "description": "Searches and recommends datasets from CIP Dataverse."},
                ]
            })
            .to_string(),
        );
        let asta: Vec<&str> = registry
            .iter()
            .filter(|subagent| subagent.uses_asta())
            .map(|subagent| subagent.name.as_str())
            .collect();
        assert_eq!(
            asta,
            ["academic_researcher", "hypothesis_generator", "data_voyager"]
        );
        // The report writer produces the document the attribution goes *in*. Crediting Asta
        // because that ran would restore the bug in a new place.
        assert!(!registry[3].uses_asta());
        assert!(!registry[4].uses_asta(), "CIP Dataverse is not Asta");

        // An empty or unreadable registry credits nothing. A missing acknowledgement can be
        // added; a false one has to be retracted.
        assert!(parse_registry("{ not json").is_empty());
    }

    #[test]
    fn a_delimiter_inside_quotes_is_not_a_column() {
        // The case that makes a naive split wrong on precisely the files a researcher exports
        // from a spreadsheet: a place name with a comma in it.
        let csv = b"site,yield_t_ha,notes\n\"Cusco, Peru\",21.4,ok\n\"Puno, Peru\",18.9,ok\n";
        assert_eq!(
            count_records(csv, b','),
            Shape::Table {
                rows: 2,
                columns: 3
            }
        );

        // A newline inside a quoted field is not a record boundary either.
        let wrapped = b"a,b\n\"line one\nline two\",2\n";
        assert_eq!(
            count_records(wrapped, b','),
            Shape::Table {
                rows: 1,
                columns: 2
            }
        );

        // `""` is an escaped quote, so the field does not end there and the rest of the line is
        // still one field.
        let escaped = b"a,b\n\"he said \"\"yes\"\", then left\",2\n";
        assert_eq!(
            count_records(escaped, b','),
            Shape::Table {
                rows: 1,
                columns: 2
            }
        );

        // No trailing newline still counts the last record.
        assert_eq!(
            count_records(b"a,b\n1,2", b','),
            Shape::Table {
                rows: 1,
                columns: 2
            }
        );
        // A header and nothing else is a table with no rows, not a table with one.
        assert_eq!(
            count_records(b"a,b,c\n", b','),
            Shape::Table {
                rows: 0,
                columns: 3
            }
        );
        // Blank lines between records are not records.
        assert_eq!(
            count_records(b"a,b\n1,2\n\n\n3,4\n", b','),
            Shape::Table {
                rows: 2,
                columns: 2
            }
        );
        assert_eq!(count_records(b"", b','), Shape::Plain);
        // Tabs, for a .tsv, and a comma inside a field is then just a character.
        assert_eq!(
            count_records(b"a\tb\nCusco, Peru\t2\n", b'\t'),
            Shape::Table {
                rows: 1,
                columns: 2
            }
        );
    }

    #[test]
    fn a_preview_splits_the_same_way_the_count_does() {
        // The two must agree: a preview showing four columns under a sub-line saying three is
        // one file contradicting itself on screen.
        let fields = split_record("\"Cusco, Peru\",21.4,\"he said \"\"yes\"\"\"", b',');
        assert_eq!(fields, vec!["Cusco, Peru", "21.4", "he said \"yes\""]);
        assert_eq!(split_record("a\tb\tc", b'\t'), vec!["a", "b", "c"]);
        // A trailing delimiter is a real empty last field, not an absent one.
        assert_eq!(split_record("a,b,", b','), vec!["a", "b", ""]);

        let dir = std::env::temp_dir().join(format!("minime-preview-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");

        let csv = dir.join("yield.csv");
        std::fs::write(
            &csv,
            "site,yield_t_ha\n\"Cusco, Peru\",21.4\nPuno,18.9\nJunin,20.1\n",
        )
        .expect("write");
        let preview = table_preview(&csv, 3).expect("a table");
        assert_eq!(preview[0], vec!["site", "yield_t_ha"]);
        assert_eq!(preview[1], vec!["Cusco, Peru", "21.4"]);
        assert_eq!(preview.len(), 3, "bounded by the row budget");
        // The column count the preview shows and the one the sub-line states are the same.
        assert_eq!(
            count_records(&std::fs::read(&csv).expect("read"), b','),
            Shape::Table {
                rows: 3,
                columns: 2
            }
        );

        // A header with no data under it previews nothing rather than an empty table.
        let bare = dir.join("empty.csv");
        std::fs::write(&bare, "a,b,c\n").expect("write");
        assert!(table_preview(&bare, 4).is_none());

        // Not a delimited file at all.
        let note = dir.join("informe.md");
        std::fs::write(&note, "# Hola\n\nunas notas\n").expect("write");
        assert!(table_preview(&note, 4).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_images_dimensions_come_out_of_its_header() {
        // Real headers, byte for byte, rather than files written by an encoder we would then be
        // testing instead of ours.

        // PNG: signature, chunk length, "IHDR", then 1600 × 900.
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&1600u32.to_be_bytes());
        png.extend_from_slice(&900u32.to_be_bytes());
        assert_eq!(
            image_shape(&png),
            Shape::Image {
                width: 1600,
                height: 900
            }
        );

        // GIF: little-endian, and the byte order is the thing worth pinning.
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&640u16.to_le_bytes());
        gif.extend_from_slice(&480u16.to_le_bytes());
        assert_eq!(
            image_shape(&gif),
            Shape::Image {
                width: 640,
                height: 480
            }
        );

        // JPEG: a comment segment first, so the walk has to skip a length-carrying marker to
        // reach the frame header — and the frame stores *height before width*.
        let mut jpeg = b"\xff\xd8".to_vec();
        jpeg.extend_from_slice(b"\xff\xfe"); // COM
        jpeg.extend_from_slice(&6u16.to_be_bytes()); // length, including itself
        jpeg.extend_from_slice(b"hola");
        jpeg.extend_from_slice(b"\xff\xc0"); // SOF0
        jpeg.extend_from_slice(&17u16.to_be_bytes());
        jpeg.push(8); // precision
        jpeg.extend_from_slice(&768u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&1024u16.to_be_bytes()); // width
        assert_eq!(
            image_shape(&jpeg),
            Shape::Image {
                width: 1024,
                height: 768
            }
        );

        // WebP lossless stores each dimension one less than it is.
        let mut webp = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0, 0]); // chunk length + signature byte
        let packed: u32 = (255) | (99 << 14);
        webp.extend_from_slice(&packed.to_le_bytes());
        assert_eq!(
            image_shape(&webp),
            Shape::Image {
                width: 256,
                height: 100
            }
        );

        // Anything else, and anything truncated, says nothing rather than guessing.
        assert_eq!(image_shape(b"not an image at all"), Shape::Plain);
        assert_eq!(image_shape(&png[..12]), Shape::Plain);
        assert_eq!(image_shape(b""), Shape::Plain);
    }

    #[test]
    fn a_shape_reads_as_the_sub_line_the_panel_shows() {
        assert_eq!(
            Shape::Table {
                rows: 1204,
                columns: 11
            }
            .describe(428_032),
            "1,204 rows · 11 cols · 418 KB"
        );
        assert_eq!(
            Shape::Image {
                width: 1600,
                height: 900
            }
            .describe(94_208),
            "1600 × 900 · 92 KB"
        );
        // A PDF says its size and stops: page and reference counts are not derivable here.
        assert_eq!(Shape::Plain.describe(1_048_576), "1.0 MB");

        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn every_output_carries_when_it_was_written() {
        // What replaced `images`. The transcript shows a turn's files in the order they were
        // written, across kinds — so the stamp has to survive `outputs`, and sorting on it has
        // to give write order rather than the kind grouping the panel wants.
        let dir =
            std::env::temp_dir().join(format!("minime-workspace-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");

        // Deliberately alternating kinds: a figure, then data, then a figure again. Reversing
        // what `outputs` returns would put both figures together and lose this order entirely.
        for name in ["a_first.png", "b_second.csv", "c_third.webp", "d_last.md"] {
            std::fs::write(dir.join(name), b"x").expect("write");
            // Distinct mtimes, or the sort has nothing to order by.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let mut produced: Vec<Output> = outputs(&dir)
            .into_iter()
            .flat_map(|(_, items)| items)
            .collect();
        produced.sort_by_key(|output| output.modified);
        let names: Vec<&str> = produced.iter().map(|output| output.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["a_first.png", "b_second.csv", "c_third.webp", "d_last.md"]
        );

        // The panel's own order is the other one: grouped by kind, newest first inside a group.
        let grouped = outputs(&dir);
        assert_eq!(grouped[0].0, Kind::Figure);
        assert_eq!(grouped[0].1[0].name, "c_third.webp", "newest figure first");

        // A directory that does not exist is the normal state before the first write.
        assert!(outputs(&dir.join("nothing-here")).is_empty());
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
        let kinds: Vec<Kind> = groups.iter().map(|(kind, _)| *kind).collect();
        // Figures first: they are the outputs someone wants to *see*. The panel no longer heads
        // each group with a word, but it still lists them in this order, so the order is still
        // the thing worth pinning.
        assert_eq!(
            kinds,
            vec![Kind::Figure, Kind::Data, Kind::Document, Kind::Other]
        );

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

    #[test]
    fn outputs_inside_an_agents_named_folder_remain_visible() {
        let dir =
            std::env::temp_dir().join(format!("minime-nested-outputs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("eda_outputs/tables")).expect("nested output folders");
        std::fs::write(dir.join("eda_outputs/yield.png"), b"plot").expect("a nested plot");
        std::fs::write(dir.join("eda_outputs/tables/summary.csv"), b"clone,yield")
            .expect("a nested table");

        let found: Vec<Output> = outputs(&dir)
            .into_iter()
            .flat_map(|(_, items)| items)
            .collect();
        let names: Vec<&str> = found.iter().map(|output| output.name.as_str()).collect();
        assert!(
            names.contains(&if cfg!(windows) {
                "eda_outputs\\yield.png"
            } else {
                "eda_outputs/yield.png"
            }),
            "{names:?}"
        );
        assert!(
            names.contains(&if cfg!(windows) {
                "eda_outputs\\tables\\summary.csv"
            } else {
                "eda_outputs/tables/summary.csv"
            }),
            "{names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hidden_caches_stay_hidden_at_every_output_depth() {
        let dir =
            std::env::temp_dir().join(format!("minime-hidden-output-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("eda_outputs/.ipynb_checkpoints"))
            .expect("a notebook cache");
        std::fs::create_dir_all(dir.join("eda_outputs/__pycache__")).expect("a Python cache");
        std::fs::write(
            dir.join("eda_outputs/.ipynb_checkpoints/draft.csv"),
            b"hidden",
        )
        .expect("a cached notebook output");
        std::fs::write(dir.join("eda_outputs/__pycache__/analysis.pyc"), b"hidden")
            .expect("a cached Python output");
        std::fs::write(dir.join("eda_outputs/report.md"), b"visible").expect("a visible report");

        let names: Vec<String> = outputs(&dir)
            .into_iter()
            .flat_map(|(_, items)| items.into_iter().map(|output| output.name))
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].ends_with("report.md"), "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_agents_own_memory_is_not_offered_as_the_researchers_output() {
        let base = std::env::temp_dir().join(format!(
            "mini-me-memories-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(base.join("memories")).expect("agent memory");
        std::fs::write(base.join("memories/instructions.txt"), b"how to behave").expect("write");
        std::fs::create_dir_all(base.join("outputs")).expect("outputs");
        std::fs::write(base.join("outputs/summary.csv"), b"a,b\n1,2\n").expect("write");

        let names: Vec<String> = outputs(&base)
            .into_iter()
            .flat_map(|(_kind, items)| items)
            .map(|output| output.name)
            .collect();

        // A panel headed FILES invites a researcher to open, edit or delete what it lists, and
        // `memories/instructions.txt` is the agent's own state — losing it changes how the agent
        // behaves, and it was never theirs to manage (§173).
        assert!(
            !names.iter().any(|name| name.contains("instructions.txt")),
            "{names:?}"
        );
        assert!(
            names.iter().any(|name| name.ends_with("summary.csv")),
            "real output still appears: {names:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn a_deeper_output_tree_says_when_the_bounded_view_stops() {
        let dir =
            std::env::temp_dir().join(format!("minime-bounded-output-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let too_deep = dir.join("one/two/three/four/five");
        std::fs::create_dir_all(&too_deep).expect("a deep output tree");
        std::fs::write(too_deep.join("buried.csv"), b"hidden by bound")
            .expect("a deeply nested output");

        let listing = output_listing(&dir);
        assert!(listing.truncated);
        assert!(listing.groups.is_empty());
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
        // The same lock the projects test takes, and for the reason written there: `root()` reads
        // this variable and so does half of `backend`'s suite. Without it, every test running
        // concurrently sees the workspace redirected to `/tmp/somewhere-else` for as long as this
        // one holds it — one failure, a different name each time, never reproducible. It cost two
        // sightings and a release build to find (§267). The old comment here claimed
        // "single-threaded test setup"; `cargo test` runs test functions on a thread pool.
        let _env = crate::backend::env_lock::hold();
        let previous = std::env::var_os(WORKSPACE_ENV);
        // SAFETY: the lock above makes this the only thread touching the variable; restored below.
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

    /// A directory of this test's own. `adopt` reads no environment, so no lock is needed.
    fn scratch(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "mini-me-adopt-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch");
        base
    }

    /// §227: an attachment must become part of the conversation, not a pointer into Downloads.
    #[test]
    fn an_attachment_is_copied_into_the_conversation() {
        let home = scratch("copied");
        let downloads = home.join("Downloads");
        std::fs::create_dir_all(&downloads).expect("downloads");
        let source = downloads.join("Graph-neural-networks.pdf");
        std::fs::write(&source, b"%PDF-1.7 a paper").expect("the paper");

        let thread = home.join("Mini-Me").join("thread-1");
        let landed = adopt(&thread, &source).expect("adopted");

        assert_eq!(landed, thread.join("Graph-neural-networks.pdf"));
        assert_eq!(std::fs::read(&landed).expect("readable"), b"%PDF-1.7 a paper");
        // The original is untouched — this copies, it does not move somebody's file.
        assert!(source.exists());
    }

    /// Attaching the same paper twice should not litter the folder.
    #[test]
    fn the_same_file_attached_twice_is_the_same_copy() {
        let home = scratch("twice");
        let source = home.join("paper.pdf");
        std::fs::write(&source, b"same bytes").expect("write");
        let thread = home.join("thread");

        let first = adopt(&thread, &source).expect("first");
        let second = adopt(&thread, &source).expect("second");
        assert_eq!(first, second);
        assert_eq!(std::fs::read_dir(&thread).expect("listing").count(), 1);
    }

    /// **Never overwrite.** A different file with a name a turn already used gets a suffix.
    #[test]
    fn a_different_file_with_a_taken_name_does_not_replace_it() {
        let home = scratch("collision");
        let thread = home.join("thread");
        std::fs::create_dir_all(&thread).expect("thread");
        std::fs::write(thread.join("results.csv"), b"what a turn produced").expect("existing");

        let source = home.join("results.csv");
        std::fs::write(&source, b"a completely different table").expect("source");
        let landed = adopt(&thread, &source).expect("adopted");

        assert_eq!(landed, thread.join("results-2.csv"));
        assert_eq!(
            std::fs::read(thread.join("results.csv")).expect("readable"),
            b"what a turn produced",
            "the turn's own output must survive an attachment that shares its name"
        );
    }

    /// A name with no extension still gets a usable suffix rather than one inside the stem.
    #[test]
    fn a_file_without_an_extension_is_suffixed_too() {
        let home = scratch("noext");
        let thread = home.join("thread");
        std::fs::create_dir_all(&thread).expect("thread");
        std::fs::write(thread.join("README"), b"one").expect("existing");
        let source = home.join("README");
        std::fs::write(&source, b"a longer, different thing").expect("source");
        assert_eq!(adopt(&thread, &source).expect("adopted"), thread.join("README-2"));
    }

    /// A path the agent wrote has to come back across WSL before this app can open it.
    #[test]
    fn a_relative_document_resolves_against_the_conversation() {
        let thread = std::path::Path::new("/w/Mini-Me/thread-1");
        assert_eq!(
            local_path("Graph-neural-networks.pdf", Some(thread)),
            Some(thread.join("Graph-neural-networks.pdf"))
        );
        assert_eq!(local_path("papers/blight.pdf", Some(thread)),
            Some(thread.join("papers/blight.pdf")));
        // With no conversation there is nothing to resolve against, and guessing would open
        // whatever happens to sit beside the executable.
        assert_eq!(local_path("a.pdf", None), None);
    }

    /// `IndexedPaper.path` is documented as "sandbox path **or URL**".
    #[test]
    fn a_url_is_not_a_file_to_open() {
        let thread = std::path::Path::new("/w/thread");
        assert_eq!(local_path("https://example.org/paper.pdf", Some(thread)), None);
        assert_eq!(local_path("asta://doc/1", Some(thread)), None);
        assert_eq!(local_path("doi:10.1000/x", Some(thread)), None);
        assert_eq!(local_path("   ", Some(thread)), None);
    }

    /// Every key the producer writes is either read here or declared unread with a reason.
    ///
    /// **The half the first version of this fixture was missing.** The Python side asserted its own
    /// shape, so adding `wrote` failed a Python test and regenerating it made that pass — while the
    /// Rust decoder went on ignoring the field and 462 tests stayed green. That is §223 exactly: a
    /// producer carrying a field the client silently drops, for as long as the feature exists.
    ///
    /// A one-sided contract is not a contract. This is the other side.
    #[test]
    fn every_key_in_the_record_is_read_or_declared_unread() {
        /// Fields deliberately not read, and why. A reason of fewer than ten characters is not one.
        const UNREAD: &[(&str, &str)] = &[(
            "clipped",
            "read into Command::clipped, but listed here because the modal shows it rather than \
             the decoder using it",
        )];

        let fixture = include_str!("../tests/fixtures/command-record.jsonl");
        let first: serde_json::Value =
            serde_json::from_str(fixture.lines().next().expect("a line")).expect("json");
        let keys: Vec<&str> = first.as_object().expect("an object").keys().map(String::as_str).collect();

        // What the decoder demonstrably reads: change a field in the fixture and the value changes.
        let read = ["at", "command", "clipped", "exit", "seconds", "outside", "wrote"];
        for key in &keys {
            assert!(
                read.contains(key) || UNREAD.iter().any(|(name, _)| name == key),
                "the record carries `{key}` and nothing here reads it or says why not — \
                 regenerate with MINIME_WRITE_CONTRACT=1 and decide about it"
            );
        }
        for (_, reason) in UNREAD {
            assert!(reason.len() > 10, "a declared-unread field needs a real reason");
        }
        // And every field this claims to read is really in the record, so the list cannot rot.
        for field in read {
            assert!(keys.contains(&field), "`{field}` is claimed as read but is not in the record");
        }
    }

    /// The record, read from the fixture the producer generates from its own code.
    ///
    /// Every field is asserted, because the failure this shape exists to prevent is §223's: the
    /// producer carried nine fields, the client kept one, and four distinct things rendered
    /// identically for as long as the feature existed. Regenerate with
    /// `MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_ledger.py`.
    #[test]
    fn every_field_the_overlay_records_is_read_back() {
        let fixture = include_str!("../tests/fixtures/command-record.jsonl");
        let commands = decode_commands(fixture);
        assert_eq!(commands.len(), 4, "one line per command");

        let first = &commands[0];
        assert_eq!(first.at, "2026-08-25T09:14:03Z");
        assert!(first.text.starts_with("python3 -c"), "{}", first.text);
        assert_eq!(first.exit, Some(0));
        assert_eq!(first.seconds, Some(1.4));
        assert!(!first.clipped);
        assert!(!first.escaped(), "it stayed inside the conversation");
        assert!(!first.failed());

        // The one this whole record exists for.
        let escaped = &commands[1];
        assert_eq!(escaped.outside, vec!["/tmp/hist.png".to_string()]);
        assert!(escaped.escaped());
        assert!(!escaped.failed(), "landing outside is not the same as failing");

        let broken = &commands[2];
        assert_eq!(broken.exit, Some(127));
        assert!(broken.failed());
        assert!(!broken.escaped(), "and failing is not the same as landing outside");

        // **The distinction everything downstream rests on.** The second command named a path it
        // did not write — that is the read case, and nothing may act on it. The fourth is
        // confirmed written, and is the only kind a copy button may ever touch.
        assert!(escaped.escaped() && !escaped.left_files(), "named without writing");
        let created = &commands[3];
        assert_eq!(created.wrote, vec!["/tmp/late-blight.csv".to_string()]);
        assert!(created.left_files());
        assert_eq!(created.outside, created.wrote, "wrote is always a subset of named");
    }

    /// A record written by a process that may have been killed mid-write.
    #[test]
    fn a_half_written_line_does_not_cost_the_researcher_the_others() {
        let text = "{\"command\":\"echo one\",\"exit\":0}\n{\"command\":\"echo tw";
        let commands = decode_commands(text);
        assert_eq!(commands.len(), 1, "the whole line survives the broken one");
        assert_eq!(commands[0].text, "echo one");
        // And an absent field is a default rather than a panic.
        assert_eq!(commands[0].seconds, None);
        assert!(commands[0].at.is_empty());
    }

    /// An unreadable exit code is not evidence of failure.
    #[test]
    fn a_command_with_no_exit_code_is_not_called_failed() {
        let commands = decode_commands("{\"command\":\"echo\",\"exit\":null}");
        assert_eq!(commands[0].exit, None);
        assert!(!commands[0].failed(), "absence of a code is not a non-zero code");
    }

    /// No record at all is the ordinary case, not an error.
    #[test]
    fn a_conversation_that_ran_nothing_has_no_commands() {
        let base = scratch("no-commands");
        assert!(commands(&base).is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    /// Every key the recorder writes is either read here or declared unread with a reason.
    ///
    /// The other side of the contract, for the same reason the command record has one: a producer
    /// carrying a field the client silently drops is §223, and it survived there for as long as
    /// the feature existed. Regenerate with
    /// `MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_claims.py`.
    #[test]
    fn every_key_in_the_claim_record_is_read_or_declared_unread() {
        /// Fields deliberately not read, and why. A reason of fewer than ten characters is not one.
        const UNREAD: &[(&str, &str)] = &[];

        let fixture = include_str!("../tests/fixtures/claim-record.jsonl");
        let first: serde_json::Value =
            serde_json::from_str(fixture.lines().next().expect("a line")).expect("json");
        let keys: Vec<&str> = first
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();

        let read = [
            "at", "source", "schema", "checked", "claimed", "missing", "outside", "datasets",
            "unsearched", "note",
        ];
        for key in &keys {
            assert!(
                read.contains(key) || UNREAD.iter().any(|(name, _)| name == key),
                "the record carries `{key}` and nothing here reads it or says why not — \
                 regenerate with MINIME_WRITE_CONTRACT=1 and decide about it"
            );
        }
        for (_, reason) in UNREAD {
            assert!(reason.len() > 10, "a declared-unread field needs a real reason");
        }
        for field in read {
            assert!(keys.contains(&field), "`{field}` is claimed as read but is not in the record");
        }
    }

    /// Every shape the recorder can produce, read from the fixture it generates from its own code.
    #[test]
    fn every_field_the_recorder_writes_is_read_back() {
        let fixture = include_str!("../tests/fixtures/claim-record.jsonl");
        let claims = decode_claims(fixture);
        assert_eq!(claims.len(), 5, "one line per structured answer");

        // The clean answer, which is a line rather than a silence — that is what makes a *missing*
        // line visible, and a missing line is the run that never happened.
        let clean = &claims[0];
        assert_eq!(clean.at, "2026-08-26T11:04:12Z");
        assert_eq!(clean.source, "data_voyager");
        assert_eq!(clean.schema, "DataAnalysisResults");
        assert_eq!(clean.claimed, 4);
        assert!(clean.checked && !clean.contradicted() && !clean.used_outside());
        assert_eq!(clean.datasets, None, "it was never a dataverse question");
        assert_eq!(clean.note, None);

        // The finding: an index that is not there, beside a PDF that is — in the researcher's own
        // Downloads folder. Calling the second one missing was true only in a sense that read as
        // false, which is why they are two lists.
        let librarian = &claims[1];
        assert_eq!(librarian.missing, vec![".asta/documents".to_string()]);
        assert_eq!(
            librarian.outside,
            vec!["/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf".to_string()]
        );
        assert!(librarian.contradicted(), "a named file that is not there");
        assert!(librarian.used_outside(), "and a real one that will not travel");

        let unexamined = &claims[2];
        assert!(unexamined.unexamined());
        assert!(!unexamined.contradicted(), "nothing looked, so nothing is contradicted");
        assert_eq!(unexamined.claimed, 0);

        let invented = &claims[3];
        assert_eq!(invented.datasets, Some(3));
        assert_eq!(invented.unsearched, vec!["doi:10.21223/INVENTED".to_string()]);
        assert!(invented.contradicted(), "a citation composed from memory");
        assert_eq!(invented.note, None, "the check ran; there is nothing to explain");

        // And the one that is neither clean nor an accusation: the check could not be made.
        let blind = &claims[4];
        assert_eq!(blind.datasets, Some(2));
        assert!(blind.unsearched.is_empty(), "no accusation from a check that never happened");
        assert_eq!(blind.note.as_deref(), Some("dataverse_search.json could not be read"));
        assert!(!blind.contradicted());
    }

    /// A truncated line must not read as a verified one.
    ///
    /// `checked` absent means nobody looked. Defaulting it to `true` would turn the last,
    /// half-written line of a killed process into a clean bill of health — an answer this record
    /// is not allowed to invent.
    #[test]
    fn a_claim_with_no_verdict_is_not_read_as_verified() {
        let claims = decode_claims("{\"source\":\"pdf_librarian\"}");
        assert!(claims[0].unexamined());
        assert_eq!(claims[0].claimed, 0);
        assert_eq!(claims[0].datasets, None);
        assert!(claims[0].at.is_empty());
    }

    /// Every key the search writer produces is either read here or declared unread with a reason.
    #[test]
    fn every_key_in_a_dataset_row_is_read_or_declared_unread() {
        /// Fields deliberately not read, and why. A reason of fewer than ten characters is not one.
        const UNREAD: &[(&str, &str)] = &[(
            "raw",
            "the producer's untouched record, carried so a field nobody mapped is not lost and so \
             the claims check can still find an id under a name we have never met — the app \
             renders the mapped fields instead",
        )];

        let fixture = include_str!("../tests/fixtures/dataverse-search.json");
        let rows: serde_json::Value = serde_json::from_str(fixture).expect("json");
        let first = rows.as_array().expect("an array")[0].as_object().expect("an object");
        let keys: Vec<&str> = first.keys().map(String::as_str).collect();

        let read = [
            "title", "persistent_id", "link", "description", "authors", "file_count", "repository",
        ];
        for key in &keys {
            assert!(
                read.contains(key) || UNREAD.iter().any(|(name, _)| name == key),
                "a dataset row carries `{key}` and nothing here reads it or says why not — \
                 regenerate with MINIME_WRITE_CONTRACT=1 and decide about it"
            );
        }
        for (_, reason) in UNREAD {
            assert!(reason.len() > 10, "a declared-unread field needs a real reason");
        }
        for field in read {
            assert!(keys.contains(&field), "`{field}` is claimed as read but is not in a row");
        }
    }

    /// **The API's answer, read as the app will render it.**
    ///
    /// Every field asserted, because the failure this shape exists to prevent is §223's twin: the
    /// producer carried nine fields, the client kept one, and four distinct datasets rendered as
    /// four identical rows for as long as the feature existed.
    #[test]
    fn a_search_result_is_read_back_as_the_row_a_researcher_sees() {
        let fixture = include_str!("../tests/fixtures/dataverse-search.json");
        let rows = decode_datasets(fixture);
        assert_eq!(rows.len(), 4, "one row per search result, including the ones we cannot map");

        let full = &rows[0];
        assert_eq!(full.persistent_id, "doi:10.21223/P3/HJLUJZ");
        assert!(full.title.starts_with("Three new healthy"), "{}", full.title);
        assert_eq!(full.authors, vec!["Perez, Willmer".to_string(), "Gastelo, Manuel".to_string()]);
        assert_eq!(full.file_count, Some(3));
        assert_eq!(full.repository.as_deref(), Some("CIP Potato Breeding"));
        assert!(full.link.as_deref().is_some_and(|link| link.ends_with("HJLUJZ")));

        // Everything optional missing. The row still renders and still opens.
        let sparse = &rows[1];
        assert_eq!(sparse.persistent_id, "doi:10.21223/P3/3AIN78");
        assert_eq!(sparse.link, None, "an empty string is not a URL");
        assert_eq!(sparse.file_count, None);
        assert!(sparse.authors.is_empty());

        // Dataverse's native split form, put back together by the producer — the shape whose
        // joined id appears nowhere in the record, and which §288 mistook for a fabrication.
        assert_eq!(rows[2].persistent_id, "doi:10.21223/P3/CKYEB5");

        // A layout nobody has met. **Kept, not dropped**: a search that quietly lost three of
        // eighteen rows is the failure hardest to notice.
        assert_eq!(rows[3].persistent_id, "");
    }

    /// Junk and absence both give nothing rather than half a list.
    #[test]
    fn an_unreadable_search_file_is_no_datasets_rather_than_a_panic() {
        assert!(decode_datasets("not json").is_empty());
        assert!(decode_datasets("{\"content\": []}").is_empty(), "an object is not the array");
        assert!(decode_datasets("[]").is_empty());
        let base = scratch("no-datasets");
        assert!(datasets(&base).is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    /// No record at all is the ordinary case: most conversations never call a subagent.
    #[test]
    fn a_conversation_with_no_subagent_answers_has_no_claims() {
        let base = scratch("no-claims");
        assert!(claims(&base).is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    /// **The join, not the parts.** Python decides where the record goes; Rust decides where to
    /// look; and until this test nothing compared the two.
    ///
    /// That gap is §280 exactly — two notions of where a conversation is, each correct in its own
    /// file, both sides green. So this drives the overlay's own `append` through its own constants
    /// and then reads the result back with the app's own reader.
    #[test]
    fn the_record_python_writes_is_the_one_the_app_reads() {
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let base = scratch("claims-join");
        let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overlay");
        // The librarian's answer out of the producer's own fixture, rather than a shape retyped
        // here — a hand-written line would agree with itself and prove nothing about the producer.
        let line = include_str!("../tests/fixtures/claim-record.jsonl")
            .lines()
            .nth(1)
            .expect("the librarian's answer");

        let script = "import json,sys\n\
                      sys.path.insert(0, sys.argv[1])\n\
                      from minime_local import ledger\n\
                      ledger.append(sys.argv[2], json.loads(sys.argv[3]), name=ledger.CLAIMS_NAME)\n";
        let out = std::process::Command::new("python3")
            .env("PYTHONIOENCODING", "utf-8")
            .args(["-c", script])
            .arg(&overlay)
            .arg(&base)
            .arg(line)
            .output()
            .expect("python3 runs");
        assert!(
            out.status.success(),
            "the overlay could not write the record: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let found = claims(&base);
        assert_eq!(found.len(), 1, "the app looked where the overlay wrote");
        assert_eq!(found[0].source, "pdf_librarian");
        assert_eq!(found[0].missing, vec![".asta/documents".to_string()]);
        // And the two records in that folder stay separate files. If the command reader picked
        // this up, `WHAT RAN` would count subagent answers as commands — and every one of them
        // would show an empty command line.
        assert!(commands(&base).is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    /// On Linux that path *is* the file. On Windows it is on the other side of WSL, and
    /// `is_absolute()` is **false** for it — a root with no drive letter — so it fell through to
    /// the join and came back as `C:\home\piero\papers\a.pdf` (§267).
    #[test]
    fn an_absolute_path_is_taken_as_it_stands() {
        let resolved = local_path("/home/piero/papers/a.pdf", None);
        if cfg!(windows) {
            assert_eq!(resolved, None, "WSL's own filesystem does not open in Explorer");
        } else {
            assert_eq!(resolved, Some(std::path::PathBuf::from("/home/piero/papers/a.pdf")));
        }
    }

    /// The defect itself, stated as a property rather than a platform: `Path::join` **replaces**
    /// its base when the argument has a root, so a rooted path must never be resolved *inside* the
    /// conversation folder. This assertion holds on both platforms and fails on either if the
    /// order of the branches in `local_path` is ever reversed again.
    #[test]
    fn a_rooted_path_never_lands_inside_the_conversation() {
        let thread = std::path::Path::new("/w/Mini-Me/thread-1");
        for recorded in ["/root/work/report.pdf", "/home/piero/a.pdf", "/mnt/c/Users/x/a.pdf"] {
            let resolved = local_path(recorded, Some(thread));
            let inside = matches!(&resolved, Some(path) if path.starts_with(thread));
            assert!(!inside, "{recorded} resolved to {resolved:?}, inside the conversation");
        }
    }

    /// Runs on every platform, which is the whole reason `windows_path_for` is its own function:
    /// behind `cfg!(windows)` this conversion could not be tested on the machine it was written
    /// on, and so never was.
    #[test]
    fn a_mounted_drive_comes_back_as_a_drive() {
        assert_eq!(
            windows_path_for("/mnt/c/Users/x/Downloads/a.pdf"),
            Some(std::path::PathBuf::from("C:\\Users\\x\\Downloads\\a.pdf"))
        );
        // Uppercased: `D:` is how a drive is written, and Explorer accepts either.
        assert_eq!(windows_path_for("/mnt/d/data"), Some(std::path::PathBuf::from("D:\\data")));
        // Not a drive. WSL's own root, the sandbox's work dir, and `/mnt/wsl`, which is a real
        // mount point and not one letter. None of the three open from Windows.
        assert_eq!(windows_path_for("/home/piero/a.pdf"), None);
        assert_eq!(windows_path_for("/root/work/report.pdf"), None);
        assert_eq!(windows_path_for("/mnt/wsl/share"), None);
        // A bare mount root is not a document, and is not worth a case of its own.
        assert_eq!(windows_path_for("/mnt/c"), None);
    }
}

