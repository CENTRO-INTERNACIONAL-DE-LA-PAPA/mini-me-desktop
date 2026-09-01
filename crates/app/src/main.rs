//! Mini-Me Desktop — GPUI entry point and root workbench view.
//!
//! P6.3. The three-pane workbench (rail / chat / artifacts) streams a **real
//! coordinator turn** from the local Python sidecar: `Sidecar` spawns and
//! health-checks the backend, assistant tokens land in the transcript as they
//! arrive over SSE, and the **agent activity trace** shows what subagents are doing
//! while they do it instead of leaving a silent gap (plan §15c).
//!
//! Built against the published `gpui 0.2.2` (see crates/app/Cargo.toml). Markdown
//! rendering and the command palette are still open.

mod backend;
mod catalogue;
mod components;
mod composer;
mod dataverse;
mod discovery;
mod gallery;
mod markdown;
mod menu;
mod notify;
mod preflight;
mod protocol;
mod provenance;
mod references;
mod selection;
mod settings;
mod sidecar;
mod subagent;
mod theme;
mod ui;
mod update;
mod workspace;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt;
use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, AssetSource, Bounds, ClipboardItem, Context, Entity, Focusable,
    KeyBinding, ListAlignment, ListState, SharedString,
    Window, WindowBounds,
    WindowOptions,
};

use components::common::horizontal_drag_offset;
use components::common::app_icon;
use components::provenance_view::{link_for, provenance_svg};
use composer::{Composer, ComposerEvent};
use protocol::{AgentRef, ApprovalRequest, Bucket, Project, TurnEvent};
use sidecar::Sidecar;

// ---- Palette (placeholder; align with the web app's tokens in P6.3) --------

/// What `--stream` asks when no `--prompt` is given: a question every backend can answer, so a
/// headless check exercises the whole round trip without depending on the researcher's data.
///
/// It used to be **prefilled into the composer** as well, from P6.0, when the app could not yet be
/// trusted to reach the backend at all and Enter-with-no-typing was the fastest proof it did. That
/// stopped being a scaffold and became litter: every launch opened with a stranger's question
/// already typed, to be deleted before the real one could be asked (docs §87).
const CHECK_PROMPT: &str = "In one short paragraph, what is your role as the Mini-Me coordinator?";

/// The reference the Allen Institute asks for when work uses Asta.
///
/// Held as a constant so the About box and anything else that needs it cannot disagree, and
/// written out in full rather than linked: a researcher pasting this into a manuscript should not
/// have to open a browser to finish the job.
const ASTA_CITATION: &str = "AstaBench: Rigorous Benchmarking of AI Agents with a Scientific \
     Research Suite. arXiv:2510.21652 — https://arxiv.org/abs/2510.21652";

/// The root workspace said as a useful place rather than as the absence of organisation.
///
/// `None` remains the metadata value and the files remain directly under `Documents/Mini-Me`;
/// this is only the researcher-facing name requested in §154, so it cannot become a second
/// project registry or collide with a real folder of the same name.
const UNGROUPED_PROJECT_LABEL: &str = "Ungrouped Conversations";
const ICON_PATHS: [&str; 31] = [
    "icons/settings.svg",
    "icons/conversations.svg",
    "icons/research.svg",
    "icons/road.svg",
    "icons/enter.svg",
    "icons/attach.svg",
    "icons/file-table.svg",
    "icons/file-image.svg",
    "icons/file-code.svg",
    "icons/file-notebook.svg",
    "icons/file-data.svg",
    "icons/file-web.svg",
    "icons/file-text.svg",
    "icons/file-log.svg",
    "icons/file-doc.svg",
    "icons/file-archive.svg",
    "icons/file-db.svg",
    "icons/file-blank.svg",
    "icons/folder.svg",
    "icons/agent-ellipse.svg",
    "icons/binoculars.svg",
    "icons/book-open-text.svg",
    "icons/broom.svg",
    "icons/chat-circle-dots.svg",
    "icons/gear-six.svg",
    "icons/magnifying-glass.svg",
    "icons/paper-plane-right.svg",
    "icons/pencil.svg",
    "icons/plus.svg",
    "icons/sidebar-simple-left.svg",
    "icons/sidebar-simple-right.svg",
];

/// The four small UI icons, compiled into the executable rather than read beside it.
///
/// Windows installs do not preserve a source-tree-relative assets directory. GPUI still needs an
/// [`AssetSource`] to resolve `svg().path(...)`, so embedding the hand-authored files makes the
/// packaged and development builds follow the same path (docs §157).
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
            "icons/conversations.svg" => {
                Some(include_bytes!("../assets/icons/conversations.svg"))
            }
            "icons/research.svg" => Some(include_bytes!("../assets/icons/research.svg")),
            "icons/road.svg" => Some(include_bytes!("../assets/icons/road.svg")),
            "icons/enter.svg" => Some(include_bytes!("../assets/icons/enter.svg")),
            "icons/attach.svg" => Some(include_bytes!("../assets/icons/attach.svg")),
            "icons/file-table.svg" => Some(include_bytes!("../assets/icons/file-table.svg")),
            "icons/file-image.svg" => Some(include_bytes!("../assets/icons/file-image.svg")),
            "icons/file-code.svg" => Some(include_bytes!("../assets/icons/file-code.svg")),
            "icons/file-notebook.svg" => Some(include_bytes!("../assets/icons/file-notebook.svg")),
            "icons/file-data.svg" => Some(include_bytes!("../assets/icons/file-data.svg")),
            "icons/file-web.svg" => Some(include_bytes!("../assets/icons/file-web.svg")),
            "icons/file-text.svg" => Some(include_bytes!("../assets/icons/file-text.svg")),
            "icons/file-log.svg" => Some(include_bytes!("../assets/icons/file-log.svg")),
            "icons/file-doc.svg" => Some(include_bytes!("../assets/icons/file-doc.svg")),
            "icons/file-archive.svg" => Some(include_bytes!("../assets/icons/file-archive.svg")),
            "icons/file-db.svg" => Some(include_bytes!("../assets/icons/file-db.svg")),
            "icons/file-blank.svg" => Some(include_bytes!("../assets/icons/file-blank.svg")),
            "icons/agent-ellipse.svg" => Some(include_bytes!("../assets/icons/agent-ellipse.svg")),
            "icons/binoculars.svg" => Some(include_bytes!("../assets/icons/binoculars.svg")),
            "icons/book-open-text.svg" => Some(include_bytes!("../assets/icons/book-open-text.svg")),
            "icons/broom.svg" => Some(include_bytes!("../assets/icons/broom.svg")),
            "icons/chat-circle-dots.svg" => Some(include_bytes!("../assets/icons/chat-circle-dots.svg")),
            "icons/gear-six.svg" => Some(include_bytes!("../assets/icons/gear-six.svg")),
            "icons/magnifying-glass.svg" => Some(include_bytes!("../assets/icons/magnifying-glass.svg")),
            "icons/paper-plane-right.svg" => Some(include_bytes!("../assets/icons/paper-plane-right.svg")),
            "icons/pencil.svg" => Some(include_bytes!("../assets/icons/pencil.svg")),
            "icons/plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "icons/sidebar-simple-left.svg" => Some(include_bytes!("../assets/icons/sidebar-simple-left.svg")),
            "icons/sidebar-simple-right.svg" => Some(include_bytes!("../assets/icons/sidebar-simple-right.svg")),
            "icons/folder.svg" => Some(include_bytes!("../assets/icons/folder.svg")),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICON_PATHS
            .iter()
            .filter(|asset| path.is_empty() || asset.starts_with(path))
            .copied()
            .map(SharedString::from)
            .collect())
    }
}














/// One edge of the provenance graph, reduced to what the canvas needs to draw it.
///
/// Not `Arc` — that name is `std::sync::Arc` in this module, and the sidecar is behind one.
struct EdgeArc {
    from: f32,
    to: f32,
    /// Rows skipped, which decides how far the curve bows out.
    span: f32,
    weight: f32,
    colour: u32,
    dashed: bool,
}







/// Extensions a research run actually writes, for [`named_files`].
///
/// **Deliberately narrower than `file_mark`'s.** That one maps whatever exists on disk to an icon
/// and can afford a catch-all; this one decides whether a word in an *answer* is a claim about a
/// file, and a wrong yes puts a correction under a sentence that was fine. So `.sh`, `.js` and
/// `.rs` are absent: an answer is far likelier to mention one in passing than to have written it.
const CLAIMABLE: [&str; 20] = [
    "png", "jpg", "jpeg", "gif", "webp", "svg", "pdf", "csv", "tsv", "xlsx", "parquet", "json",
    "txt", "md", "html", "typ", "zip", "db", "sqlite", "ipynb",
];

/// Filenames an answer names, in the order it names them.
///
/// The turn tells the researcher what it produced, and until now nothing compared that to the
/// folder. Two failed attempts reported plots on disk that were not there, and a later answer
/// listed ten filenames the panel could not show (§42). The prompt already says *"NEVER invent
/// findings, numbers, or charts"* — measured at zero compliance, which is what a rule with no
/// check is worth.
///
/// Basenames only: an answer may write `outputs/plots/a.png` for a file the workspace holds at a
/// different depth, and the question is whether the file exists, not whether the model recited its
/// path correctly.
fn named_files(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '`' | '"' | '\'' | ',' | ';' | '<' | '>' | '|' | '*')) {
        // Trailing sentence punctuation is not part of a name; a leading bullet is not either.
        let token = token.trim_matches(|c: char| matches!(c, '.' | ':' | '!' | '?' | '·' | '-' | '#'));
        let Some(name) = token.rsplit(['/', '\\']).next() else {
            continue;
        };
        let Some((stem, extension)) = name.rsplit_once('.') else {
            continue;
        };
        if !CLAIMABLE.contains(&extension.to_ascii_lowercase().as_str()) {
            continue;
        }
        // A stem has to look like a name. `0.96` fails on the extension already; this catches
        // the rest of the numeric and single-character noise.
        if stem.len() < 2 || !stem.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let name = name.to_string();
        if !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

/// A menu opened from a control in the sidebar rather than by right-clicking.
///
/// **Why a menu and not more inline chips.** Each row carried `rename` and `✕` revealed on hover,
/// and each project heading carried `+` and `✕`. Four controls, all of them one or two characters
/// wide, all of them appearing only when the pointer is already on top of them — so the way to
/// find out what a row can do was to hover it and read two abbreviations. A `⋮` is one target in
/// a fixed place whose contents are words, which is the shape every list of this kind uses.
///
/// The `New` variant is the same idea aimed the other way: one button whose menu says what the
/// two kinds of new thing are, rather than a button that silently means only one of them.
#[derive(Clone, Debug)]
enum SidebarMenu {
    New,
    Conversation(protocol::Conversation),
    Project {
        name: String,
        conversations: Vec<protocol::Conversation>,
    },
}

/// One row of a sidebar menu: a label, and whether it is the destructive one.
struct MenuRow {
    id: &'static str,
    label: String,
    danger: bool,
}

/// Which of the two sidebar lists is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum SidebarView {
    #[default]
    Conversations,
    Projects,
}




/// A scrollbar for a `ScrollHandle`, or nothing when there is nothing to scroll.
///
/// `overflow_y_scroll` draws no scrollbar at all, so a long transcript looked *cut off*
/// rather than scrollable — which is why the approval card read as broken (§40) and why
/// nobody found the buttons below the fold in Settings (§52). A wheel that works is not an
/// affordance; a visible bar is.
///
/// Positioned absolutely, so the caller's container must be `relative()`. `bounds()` is
/// only meaningful after the first paint, hence the zero check: on frame one there is
/// simply no bar, and from frame two there is.
/// Room to leave on the right of anything [`scrollbar`] is drawn over.
///
/// The bar is `right: 2px` and `6px` wide, so it owns the last 8; the extra 4 keep a row's
/// border and the thumb from touching. Stated once and used by both sides, because the
/// alternative is a number in the scrollbar and a different number in each thing it overlaps —
/// which is how the theme rows ended up with a thumb sitting on their swatches (docs §100).
pub const SCROLL_GUTTER: f32 = 12.;

/// The group name a scrollbar watches, so the thumb appears only while the pointer is over the
/// region it scrolls.
///
/// **A scrollbar is a control, and a control that is always drawn is furniture.** The transcript's
/// sat permanently against the right edge, close enough to the text that a long line ran under it.
/// Revealed on hover it is still findable — the pointer is already there when you reach for it —
/// and gone the rest of the time (§173).
const SCROLL_GROUP: &str = "scroll-region";

/// How far the conversation sits from its own edge.
///
/// Matches the composer below it, which is `m_2` outside a `p_2` box — so the question a
/// researcher types and the answer they read start on the same x. 16px was the list's padding
/// before §174 found that half of it never applied.
const TRANSCRIPT_INSET: f32 = 16.;

/// How many references the side panel lists before offering the rest in one press.
///
/// Four, the same count the image gallery shows before its `+N` tile (§152). Enough to see whose
/// work this is; few enough that the files below stay on screen.
const SOURCES_IN_PANEL: usize = 4;

/// How tall the background-jobs list grows before it scrolls inside itself.
///
/// The number the model picker, the theme rows and the approval card already use, so the app has
/// one scroller height rather than four — §100's argument about stating a measurement once,
/// applied to heights instead of gutters.
const JOBS_BODY_HEIGHT: f32 = 260.;

/// How much of a job's question the panel prints.
///
/// Roughly three lines at the panel's width. The question's job in this row is to tell two
/// concurrent analyses apart, and a first clause does that; the rest of it is instructions to the
/// analyst, which belong to the turn that sent them.
const JOB_QUESTION_CHARS: usize = 120;

/// A finished discovery run, open for reading.
///
/// Holds the experiments rather than re-reading them each frame: the fetch is a request to the
/// service through the sandbox, and a modal that re-issued it on every notify would hammer a
/// route to redraw a picture that has not changed.
#[derive(Clone, Debug)]
struct DiscoveryView {
    run_id: String,
    name: String,
    experiments: Vec<discovery::Experiment>,
    /// Which experiment is open, as an index into `experiments`. `None` shows the ranked list.
    selected: Option<usize>,
    /// Figures decoded to disk, keyed by `experiment_id`.
    ///
    /// Per experiment and fetched on open, because `rich_outputs` is `null` in the listing and
    /// ~458KB in the per-experiment response (§247). An absent key means "not asked yet"; an empty
    /// vec means "asked, and this one has none" — a distinction the panel has to draw, or an
    /// experiment with no plot looks like one still loading.
    figures: std::collections::HashMap<String, Vec<std::path::PathBuf>>,
    /// The experiment whose figures are in flight, so the pane can say so once.
    fetching: Option<String>,
    /// True while the fetch is in flight, so an empty tree reads as "loading" and not "nothing".
    loading: bool,
    /// Whether the modal is expanded to fill the window.
    ///
    /// Asked for: *"maybe we can make an option to full-screen for the ui modal of autodiscovery as
    /// it have interesting reading for the user."* An analysis and a review are several paragraphs
    /// of real prose, and 760px is a width for scanning a list, not for reading one (§266).
    expanded: bool,
    /// Which way the ranked list is ordered.
    ///
    /// Loudest first by default, because the point of a discovery run is the handful of results that
    /// changed the picture. The other direction answers a real question too — *what did it try that
    /// moved nothing?* — which is why it is a toggle rather than a fixed order.
    loudest_first: bool,
    /// Whether the run has stopped producing experiments, from `has_job_completed`.
    ///
    /// Read rather than inferred from the count: `n_experiments` is what was *requested*, and a
    /// run that failed early has fewer without still being in progress. The service's own flag is
    /// the only thing that knows the difference.
    complete: bool,
    error: Option<String>,
}

/// A discovery run waiting for its budget to be approved, and what the researcher has changed.
///
/// Held rather than read from the snapshot each frame because two of its fields are *being edited*.
/// A modal that re-derived the budget from the artifact on every notify would discard the number
/// the researcher just set — and the number is the price.
#[derive(Clone, Debug)]
struct Approval {
    draft: protocol::Draft,
    /// Experiments to run, which is the cost in credits. Starts at whatever the run was drafted
    /// with and moves only when the researcher moves it.
    experiments: u32,
    /// What the service says it will cost and what is left, once that has come back. `None` while
    /// the request is in flight.
    cost: Option<protocol::DraftCost>,
    /// A submit that did not work, in the backend's own words — which say whether anything was
    /// charged.
    error: Option<String>,
    /// True between the press and the answer, so the button cannot be pressed twice. Spending
    /// twice is the one double-submit in this app that cannot be undone.
    submitting: bool,
}

/// Budgets the modal offers in one press.
///
/// 15 is the default because the researcher who owns the credits said so; the others bracket it.
/// Presets rather than a text field: a typed number is how somebody spends 150 credits meaning to
/// spend 15, and the four here cover exploring, a normal run, and a thorough one.
const BUDGET_PRESETS: [u32; 4] = [5, 15, 30, 50];

/// The most a single press of `+` can add, and the ceiling the service itself enforces.
const MAX_BUDGET: u32 = 500;

/// The budget the gate opens with, from whatever the agent drafted.
///
/// Clamped, so the number on screen is always one the service will accept — a modal offering to
/// spend 0 or 900 credits is offering a press that fails.
fn opening_budget(drafted: u32) -> u32 {
    drafted.clamp(1, MAX_BUDGET)
}

/// Whether this budget can actually be spent.
///
/// `None` for the balance means the lookup did not answer, and that must not block the decision:
/// refusing to let somebody spend because a *balance request* failed is the wrong failure, and the
/// service will refuse an unaffordable submit anyway. Only a known, smaller balance disables the
/// button.
fn affordable(experiments: u32, available: Option<u32>) -> bool {
    match available {
        Some(left) => experiments <= left,
        None => true,
    }
}

/// The experiments in reading order, as indices.
///
/// **Ties broken by creation order**, so flipping the direction twice returns the list to exactly
/// where it was. Three of the five experiments in one real run reported the same `0.690`, so equal
/// scores are the common case rather than an edge one, and a sort that reshuffled them would make
/// the toggle look like it was doing something else.
/// One sentence for what a press of "bring them in" actually did.
///
/// **Never silence, and never a bare zero.** `brought: 0, refused: 0` is a result with no
/// information in it, and it is exactly what a researcher got when the files had been swept from
/// `/tmp` between the command and the press: the button appeared to do nothing at all.
fn collected_sentence(collected: &protocol::Collected) -> String {
    let brought = collected.brought.len();
    let refused = collected.refused.len();
    if brought == 0 && refused == 0 {
        // The backend's own explanation, which it sends precisely so this case can be described.
        return if collected.note.is_empty() {
            "nothing to bring in".to_string()
        } else {
            collected.note.clone()
        };
    }
    let mut said = if brought == 0 {
        "brought nothing in".to_string()
    } else {
        format!(
            "brought {brought} file{} into this conversation",
            if brought == 1 { "" } else { "s" }
        )
    };
    if refused > 0 {
        // The count *and* the first reason: "2 left out" tells nobody what to do next.
        let (_, reason) = &collected.refused[0];
        said.push_str(&format!(
            " · {refused} left where {} — {reason}",
            if refused == 1 { "it was" } else { "they were" }
        ));
    }
    said
}

/// The distinct files this conversation's commands were watched writing outside it.
///
/// **Files, not commands.** The button says how many will be copied, and one command can write
/// three — counting commands would have made the label a lie the first time a script produced a
/// figure per variable, which is the ordinary case here.
///
/// Deduplicated in order: a file rewritten by a later command is one file, and it appears where it
/// was first produced, which is the order a person remembers making them in.
fn files_left_outside(commands: &[workspace::Command]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for command in commands {
        for path in &command.wrote {
            if !found.contains(path) {
                found.push(path.clone());
            }
        }
    }
    found
}

/// Which message carries the offer to fetch files written outside this conversation.
///
/// `messages` is `(is_from_the_researcher, named_nothing_missing)` per transcript row, which is
/// everything the rule needs and nothing it does not — a free function so the placement can be
/// tested without a window, the same reason `commands_summary` is one.
///
/// **Newest flagged answer first.** That is the message the researcher is looking at when they
/// go hunting for a plot, and it is where §279's button should have been all along instead of
/// two clicks inside a diagnostic modal nobody opens (§301).
///
/// **And the newest answer when nothing is flagged, which is not a fallback but the case that
/// matters.** A script that writes `plot.png` next to itself names no path, so `ledger.outside`
/// — which reads absolute paths out of the *command text* — records nothing, and no message ever
/// gets a note. The files are still there and still fetchable, because `wrote` is decided by the
/// file's own mtime rather than by the string. Anchoring the offer to the note would have hidden
/// it in exactly the situation that produced eight orphaned figures.
fn place_recovery_offer(stray: &[String], messages: &[(bool, bool)]) -> Option<usize> {
    if stray.is_empty() {
        return None;
    }
    let answers = || {
        messages
            .iter()
            .enumerate()
            .filter(|(_, (from_researcher, _))| !from_researcher)
    };
    answers()
        .filter(|(_, (_, nothing_missing))| !nothing_missing)
        .next_back()
        .or_else(|| answers().next_back())
        .map(|(index, _)| index)
}

/// The offer beside §175's note, and the caveat that has to travel with it.
///
/// **Two different lists, and the wording may not blur them.** The note above is built from names
/// the *answer* used, parsed out of prose. This button acts on absolute paths a command was
/// watched writing, decided by the file's own mtime. A name in one is not evidence of a path in
/// the other — they can overlap completely, partly, or not at all — so the count here is the
/// button's own and never the note's.
///
/// When the button can fetch fewer than the note named, that gap is stated. Pressing a control
/// that says eight and produces six is how §272's *"I pressed it and nothing happened"* starts,
/// and the difference is not a failure: a name nothing was seen writing is a name nothing knows
/// where to look for.
fn recovery_offer(named: usize, recoverable: usize) -> Option<(String, Option<String>)> {
    if recoverable == 0 {
        return None;
    }
    let label = format!(
        "Copy {recoverable} file{} into this conversation",
        if recoverable == 1 { "" } else { "s" }
    );
    // Only when the two counts disagree. Said under every offer, it would be a sentence about
    // mtime windows under a button that already did exactly what it promised.
    let caveat = (recoverable < named).then(|| {
        "only files a command was watched writing can be fetched — a name nothing was seen \
         writing is one nothing knows where to look for"
            .to_string()
    });
    Some((label, caveat))
}

/// Whether the Outputs panel has nothing at all to say.
///
/// **A turn that ran commands has something to say even with no files**, and that is not an edge
/// case — it is the case this whole record exists for. A command that wrote everything to `/tmp`
/// leaves `files == 0`, and the first version of the panel checked only files and buckets, so it
/// went silent in exactly the situation §160 describes (§277).
///
/// **And with `run_record` off, that silence comes back — on purpose, and only here.** Both lines
/// are hidden by default now (§301), so a turn that wrote everything to `/tmp` really does leave
/// this panel with nothing, exactly as before §277. What makes that acceptable is that the thing
/// §277 was protecting is no longer in this panel: the offer to fetch those files sits on the
/// answer that named them, in the transcript, and reads `Workbench::stray` — which no setting
/// gates. The panel going quiet costs a diagnostic line; it cannot cost anybody their figures.
fn outputs_are_empty(files: usize, buckets: usize, commands: usize, claims: usize) -> bool {
    files == 0 && buckets == 0 && commands == 0 && claims == 0
}

/// The one line the Outputs panel shows about what a conversation ran, and whether it is loud.
///
/// A free function so the wording is testable without a window — the same reason `ranked` is one
/// (§266). What it says is the whole feature: a count nobody can act on is furniture, and the
/// phrase that matters is the one naming files the Outputs panel cannot show.
fn commands_summary(commands: &[workspace::Command]) -> (String, bool) {
    let escaped = commands.iter().filter(|command| command.escaped()).count();
    let failed = commands.iter().filter(|command| command.failed()).count();

    // The number first, because the number is what is being scanned for.
    let mut summary = format!(
        "{} command{}",
        commands.len(),
        if commands.len() == 1 { "" } else { "s" }
    );
    if failed > 0 {
        summary.push_str(&format!(" · {failed} failed"));
    }
    // **Two claims of different strength, and the line makes exactly the one it has earned.**
    //
    // `wrote` is decided from the file's own mtime against the command's window, so it is a fact
    // about the file. `outside` is a path appearing in the command's text, which may equally have
    // been read — `pd.read_csv('/tmp/input.csv')` names a file the researcher owns. Saying "wrote"
    // about that would be §252's mistake, and saying only "named" about a file we watched appear
    // would be the opposite failure: burying the finding in a hedge.
    let left = commands.iter().filter(|command| command.left_files()).count();
    if left > 0 {
        summary.push_str(&format!(" · {left} wrote a file outside this conversation"));
    } else if escaped > 0 {
        summary.push_str(&format!(" · {escaped} named a file outside this conversation"));
    }
    (summary, escaped > 0)
}

/// The one line the Outputs panel shows about what this conversation's subagents *claimed*.
///
/// A free function for the same reason `commands_summary` is one: the wording is the feature, and
/// it has to be assertable without a window.
///
/// **One clause, and it is the strongest one earned.** The line is scanned, not read; four clauses
/// is a line nobody finishes, which is how §116's diagnostic stopped being read. Everything else is
/// in the modal, one row per answer.
fn claims_summary(claims: &[workspace::Claim]) -> (String, bool) {
    let contradicted = claims.iter().filter(|claim| claim.contradicted()).count();
    let blind = claims.iter().filter(|claim| claim.note.is_some()).count();
    let elsewhere = claims.iter().filter(|claim| claim.used_outside()).count();
    let unexamined = claims.iter().filter(|claim| claim.unexamined()).count();

    let mut summary = format!(
        "{} subagent answer{}",
        claims.len(),
        if claims.len() == 1 { "" } else { "s" }
    );
    if contradicted > 0 {
        // The accusation, and the only one that colours the line: a file that is not there, or a
        // `persistent_id` composed from memory — which is a citation a researcher would paste.
        summary.push_str(&format!(" · {contradicted} claimed something that isn't there"));
    } else if blind > 0 {
        // Distinct from finding nothing wrong, and the distinction is the point: the dataverse
        // comparison failed on every turn for two days and the log looked exactly like success.
        summary.push_str(&format!(" · {blind} could not be checked"));
    } else if elsewhere > 0 {
        summary.push_str(&format!(
            " · {elsewhere} used a file from outside this conversation"
        ));
    } else if unexamined > 0 {
        // Not a fault — no rule covers those schemas. Said out loud anyway, because an unexamined
        // answer and a verified one are the same silence, and silence reads as verified.
        summary.push_str(&format!(" · {unexamined} with nothing to check"));
    } else if claims
        .iter()
        .any(|claim| claim.claimed > 0 || claim.datasets.is_some())
    {
        // Earned, not assumed: every answer was examined, at least one had something to examine,
        // and none of it was missing. Stated rather than left to a bare count, because leaving the
        // good news implicit is what makes the bad news invisible.
        summary.push_str(" · everything they named is there");
    }
    (summary, contradicted > 0)
}

/// One persistent identifier with its scheme removed, lowercased.
///
/// The same normalisation the backend's claims check applies, and for the same reason: Dataverse
/// hands the same dataset back as `doi:10.21223/P3/X`, as `10.21223/P3/X`, and as a resolver URL,
/// and a mark that only matched one spelling would leave the agent's own choice unmarked (§288).
fn bare_persistent_id(identifier: &str) -> String {
    let cleaned = identifier.trim().trim_matches('"');
    let lowered = cleaned.to_ascii_lowercase();
    for prefix in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "https://hdl.handle.net/",
        "http://hdl.handle.net/",
        "doi:",
        "hdl:",
    ] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            return rest.trim_matches('/').to_string();
        }
    }
    lowered.trim_matches('/').to_string()
}

/// The ticked datasets that can still be fetched, in the order the list shows them.
///
/// A free function for the reason `commands_summary` is one: the button's label is a promise about
/// how many files will arrive, and a promise is worth testing without a window. The first version
/// of its test re-implemented this filter and therefore agreed with itself whatever the code did —
/// which is the shape of test §294 threw away a few hours earlier.
///
/// Three sets, and the arithmetic between them is the part that goes wrong: a pick whose file has
/// already landed, or whose download is in flight, is not another file.
fn still_fetchable(
    datasets: &[protocol::Dataset],
    picked: &std::collections::HashSet<String>,
    downloaded: &std::collections::HashMap<String, String>,
    downloading: &std::collections::HashSet<String>,
) -> Vec<protocol::Dataset> {
    datasets
        .iter()
        .filter(|dataset| picked.contains(&dataset.persistent_id))
        .filter(|dataset| !downloaded.contains_key(&dataset.persistent_id))
        .filter(|dataset| !downloading.contains(&dataset.persistent_id))
        .cloned()
        .collect()
}

/// How many datasets to say there are, with a denominator when one was reported.
///
/// A free function so the wording is testable without a window, and so the panel heading and the
/// modal title read the same — two derivations of "29 of 4,000" is two chances to disagree about
/// what the researcher is looking at.
fn datasets_heading(shown: usize, totals: workspace::SearchTotals) -> String {
    match totals.denominator() {
        // **"of" and not "/"**, because this is a sentence a researcher reads once and acts on.
        Some(total) => format!("{shown} of {total}"),
        None => shown.to_string(),
    }
}

fn ranked(experiments: &[discovery::Experiment], loudest_first: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..experiments.len()).collect();
    order.sort_by(|&a, &b| {
        let (first, second) = (experiments[a].magnitude(), experiments[b].magnitude());
        if loudest_first {
            second.total_cmp(&first)
        } else {
            first.total_cmp(&second)
        }
        .then_with(|| experiments[a].order.cmp(&experiments[b].order))
    });
    order
}

/// What the detail pane knows about one experiment's figures.
///
/// Four states, and the reason they are four is that three of them look identical if you collapse
/// them. `rich_outputs` is absent from the experiments listing and only arrives per experiment, so
/// "not asked", "asking" and "asked, there are none" are genuinely different things — and an
/// experiment that drew no plot must not read as a pane still loading (§257).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Figures {
    /// Decoded and on disk.
    Ready,
    /// Fetched, and this experiment produced none.
    Nothing,
    /// The request is in flight.
    Fetching,
    /// Nobody has asked yet.
    Unread,
}

/// Which of the four states applies, from what the view holds.
///
/// Pure, so the distinction is testable without a window — it is the part that regresses, because
/// the tempting simplification is `if paths.is_empty()`.
fn figure_state(known: Option<&Vec<std::path::PathBuf>>, fetching: bool) -> Figures {
    match known {
        Some(paths) if !paths.is_empty() => Figures::Ready,
        Some(_) => Figures::Nothing,
        None if fetching => Figures::Fetching,
        None => Figures::Unread,
    }
}

/// Whether the gate has what it needs to submit: a one-shot approval token from the draft lookup.
///
/// Pure so the enable rule is testable. Split out because "the button is pressable" and "the press
/// will work" have to be the same condition — a modal that offers a press it knows will be refused
/// is a modal that teaches people to press twice.
fn ready_to_submit(cost: Option<&protocol::DraftCost>) -> bool {
    cost.is_some_and(|cost| !cost.approval.trim().is_empty())
}

/// The cost and the balance as one sentence, because they are one question.
///
/// `available` and never `granted`: submitting moves credits to `pending` immediately, so the grant
/// overstates what is left by however much is already in flight (§247).
fn cost_line(experiments: u32, available: Option<u32>) -> String {
    let unit = if experiments == 1 {
        "experiment"
    } else {
        "experiments"
    };
    match available {
        Some(left) => format!("{experiments} {unit} · {experiments} of {left} credits"),
        None => format!("{experiments} {unit} · one credit each"),
    }
}

/// How many background things are in each state.
///
/// Exists for the folded heading, which is the whole reason folding is safe: collapsed, this
/// summary is the *only* thing on screen about work that is still moving, and a fold that hid a
/// worker stopped at an approval gate without saying so would put back the hang §31 removed.
///
/// Counted over both lists because a researcher does not have two mental categories here. A
/// LangGraph background subagent and an Asta task are different objects to this client — different
/// endpoints, different polls, different rows — and identical to the person waiting on them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct JobTally {
    waiting: usize,
    running: usize,
    failed: usize,
    done: usize,
}

impl JobTally {
    fn of(tasks: &[protocol::AsyncTask], jobs: &[protocol::Job]) -> Self {
        let mut tally = Self::default();
        for task in tasks {
            // Approval first: a task at the gate is `interrupted`, which is *not* terminal and
            // not running either — it is stopped, waiting for a person.
            if task.needs_approval() {
                tally.waiting += 1;
            } else if !task.is_finished() {
                tally.running += 1;
            } else if task.succeeded() {
                tally.done += 1;
            } else {
                tally.failed += 1;
            }
        }
        for job in jobs {
            if !job.is_finished() {
                tally.running += 1;
            } else if job.succeeded() {
                tally.done += 1;
            } else {
                tally.failed += 1;
            }
        }
        tally
    }

    /// `1 waiting for you · 2 running`, most urgent first, silent about states that are empty.
    ///
    /// States are named rather than totalled. `3 jobs` tells a folded reader nothing they can act
    /// on, and the reason to fold is not having to unfold.
    fn summary(self) -> String {
        let mut parts = Vec::new();
        // "for you", not "for approval". The panel is read by researchers who do not write code,
        // and the sentence has to say whose move it is.
        if self.waiting > 0 {
            parts.push(format!("{} waiting for you", self.waiting));
        }
        if self.running > 0 {
            parts.push(format!("{} running", self.running));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.done > 0 {
            parts.push(format!("{} done", self.done));
        }
        parts.join(" · ")
    }

    /// The colour of the most urgent state in it.
    ///
    /// The accent for a gate, because the accent is this app's one signal that something is
    /// yours to act on (§199) — and a summary is the only thing a folded section can signal with.
    fn colour(self) -> u32 {
        if self.waiting > 0 {
            theme::accent()
        } else if self.running > 0 {
            theme::running()
        } else if self.failed > 0 {
            theme::error()
        } else {
            theme::text_faint()
        }
    }
}


/// The geometry shared by painting and dragging a gallery scrollbar.
///
/// It has to be one calculation. The first Windows pass found a painted thumb that could not be
/// dragged at all; letting its hit-testing use a second set of numbers would be the same defect
/// one layer later (docs §158).
#[derive(Clone, Copy, Debug)]
struct HorizontalScrollMetrics {
    overflow: gpui::Pixels,
    viewport: gpui::Pixels,
    thumb: gpui::Pixels,
    travel: gpui::Pixels,
    progress: f32,
}




/// Outputs that share the directory the agent chose share one visual gallery.
///
/// §143 deliberately retained each relative path while making nested work visible. §152 found
/// that rendering those paths as independent rows flattened the useful structure straight back
/// out. Keep the full parent as the identity so two separate runs' `plots/` folders never merge.
struct OutputFolderGroup<'a> {
    folder: PathBuf,
    outputs: Vec<&'a workspace::Output>,
}

/// How many image tiles the panel shows before the last one becomes a count.
///
/// Four, and 2×2, because that is the arrangement the researcher pointed at: a phone's photo
/// grid, where the fourth tile carries `+5` rather than the grid growing. §152's complaint was
/// never that the thumbnails were too small — it was that a productive run claimed the whole
/// panel before anyone had chosen a figure to look at.
const IMAGE_GRID_TILES: usize = 4;

/// Tiles per row. Two, which with [`IMAGE_GRID_TILES`] makes the 2×2 the researcher pointed at.
const GRID_COLUMNS: usize = 2;

/// The gap between tiles, matching `gap_2`. Named because the heading is sized from it.
const GRID_GAP: f32 = 8.;

/// Tile width in the 330px Outputs panel, and in the transcript.
///
/// **Fixed, not a fraction.** A grid of `flex_1` tiles is as wide as whatever holds it, which in
/// the transcript is the whole conversation — one folder of files claimed a band wider than the
/// answer that produced it. Two fixed tiles make the block `2 × tile + gap` and no wider, which is
/// how the phone gallery being imitated stays a block you flick past rather than a wall (§164).
/// How many characters a folder heading gets in the research panel.
///
/// The heading box is [`GRID_TILE_COMPACT`] × [`GRID_COLUMNS`] + [`GRID_GAP`] = 304px, and
/// `click to open all` takes about a hundred of them — so roughly 32 characters at
/// [`ui::Size::Compact`]. It was 28, chosen before headings carried a producer's name and two
/// characters short of fitting `background worker / … / tables` (§208). `ui::Label` here has no
/// `.ellipsis()` (§193), so this number is the only thing keeping the text inside the box.
const PANEL_HEADING_CHARS: usize = 32;

/// The same, for a heading under an answer in the transcript, where the box is 408px.
const TRANSCRIPT_HEADING_CHARS: usize = 40;

const GRID_TILE_COMPACT: f32 = 148.;
const GRID_TILE_ROOMY: f32 = 200.;

/// A tile's media area, as a fraction of its width.
///
/// Landscape rather than square: the figures are matplotlib plots, which are wider than tall, and
/// a square tile showing a `Contain`ed plot is mostly empty box.
const GRID_TILE_ASPECT: f32 = 0.7;



/// The height of the image area in the preview, and the modal's own size.
///
/// Explicit rather than "as tall as the picture": the modal has a header above and a filmstrip
/// below, and an image sized from the file pushed both out of a bounded panel. 380 + the header +
/// the strip sits inside [`PREVIEW_MAX_HEIGHT`] with room to spare, so the layout cannot depend on
/// what the agent happened to plot.
const PREVIEW_IMAGE_HEIGHT: f32 = 380.;

/// The body's own ceiling, so it scrolls instead of growing the panel.
///
/// A flex child with `overflow_y_scroll` needs a bounded height to scroll *within*; unbounded, it
/// resolves to its content and the clipping happens somewhere else — which is how a plot ended up
/// cut at the top. 440 leaves the image box its 380 plus the 24 of padding around it, and a long
/// CSV scrolls inside the same frame.
const PREVIEW_BODY_HEIGHT: f32 = 440.;

/// Wide enough to read a plot's axis labels. Was 760, which was chosen when the preview was a
/// table of CSV rows and is narrow for a figure with five rotated category names on the x axis.
const PREVIEW_WIDTH: f32 = 880.;

/// Leaves the workbench visible at the edges — it is a modal, not a screen (docs §49).
const PREVIEW_MAX_HEIGHT: f32 = 640.;


/// Ink for text drawn on a dark scrim over a picture.
///
/// Deliberately **not** a theme role. The scrim beneath it is a fixed dark wash in both palettes,
/// so a role that followed the theme would put near-black text on it in the light one. The colour
/// belongs to the scrim, not to the page — the same reason the modal's own backdrop is a literal.
const SCRIM_INK: u32 = 0xf5f5f5;

/// A file open in the preview, and the set the researcher can step through from it.
///
/// **Why a set and not a file.** The preview held one `Output`, so it had nothing to go "next"
/// to: choosing between eight figures meant closing the modal, finding the next thumbnail, and
/// opening it again. Holding the group it was opened from is what makes the arrows, the counter
/// and the filmstrip possible, and all three are the same fact rendered three ways.
struct Preview {
    /// Never empty — see [`Preview::opening`], which is the only way to build one.
    items: Vec<workspace::Output>,
    at: usize,
}

impl Preview {
    /// Open `items` at `at`, or `None` when there is nothing to show.
    ///
    /// The emptiness check is here rather than at the call sites because `current()` indexes,
    /// and an empty preview would be a panic reachable from a click on a folder whose files were
    /// deleted between the scan and the click — which on this project's own evidence is not a
    /// hypothetical (§159's reproduction was deleted mid-diagnosis).
    fn opening(items: Vec<workspace::Output>, at: usize) -> Option<Self> {
        (!items.is_empty()).then(|| {
            let at = at.min(items.len() - 1);
            Self { items, at }
        })
    }

    /// One file, with nothing to step to. What a non-image row still opens.
    fn single(output: workspace::Output) -> Option<Self> {
        Self::opening(vec![output], 0)
    }

    fn current(&self) -> &workspace::Output {
        // `at` is clamped on construction and only ever moved by `step`, which wraps.
        &self.items[self.at]
    }

    /// Move `by` places, wrapping at both ends.
    ///
    /// Wrapping rather than stopping: the counter says which of how many, so there is no risk of
    /// mistaking the end for a broken button, and a researcher comparing the first and last plot
    /// of a series should not have to travel back through six.
    fn step(&mut self, by: isize) {
        let count = self.items.len() as isize;
        if count <= 1 {
            return;
        }
        let at = self.at as isize + by;
        self.at = at.rem_euclid(count) as usize;
    }
}


/// One mouse-held gallery thumb.
struct GalleryScrollDrag {
    handle: gpui::ScrollHandle,
    track_left: gpui::Pixels,
    grab_x: gpui::Pixels,
    travel: gpui::Pixels,
    overflow: gpui::Pixels,
}










/// Approve, or decline with the same sentence every time.
fn decision_for(approve: bool) -> protocol::Decision {
    if approve {
        protocol::Decision::Approve
    } else {
        protocol::Decision::Reject {
            message: "The researcher declined to run this command.".to_string(),
        }
    }
}

/// The status bar's one-line answer to "what is happening, and how far through".
///
/// Free of `self` so the rule can be checked without a window — the lesson §203 and §205 both cost
/// a round trip to learn.
///
/// **`done + 1` is the step being worked on**, not `done`: with two of four finished, the third is
/// the one running, and "step 2 of 4" tells a researcher the wrong thing about where the work is.
fn summary_for(tasks: &[protocol::AsyncTask], plan: &[protocol::Todo]) -> Option<String> {
    let worker = tasks
        .iter()
        .filter(|task| !task.is_finished() && !task.todos.is_empty())
        .max_by_key(|task| protocol::plan_progress(&task.todos).map(|(done, _)| done));
    if let Some(task) = worker {
        let (done, total) = protocol::plan_progress(&task.todos)?;
        let name = task.agent_name.replace('_', " ");
        return Some(match &task.activity {
            Some(activity) => format!("{name} · step {} of {total} · {activity}", done + 1),
            None => format!("{name} · step {} of {total}", done + 1),
        });
    }
    let (done, total) = protocol::plan_progress(plan)?;
    // Every step done: say nothing rather than sit at "step 3 of 2" for the rest of the session.
    if done == total {
        return None;
    }
    Some(format!("step {} of {total}", done + 1))
}




/// A one-line tooltip.
///
/// GPUI wants a whole view for a tooltip, so this is the smallest one that renders text —
/// and having it means a control can be an icon without becoming a guess.
struct Hint {
    text: SharedString,
}






actions!(
    workbench,
    [
        TogglePalette,
        PaletteNext,
        PalettePrev,
        PaletteDismiss,
        ToggleSettings,
        Dismiss,
        CopySelection,
        SelectAllTranscript
    ]
);

/// The editable fields in Settings, in the order they are shown.
///
/// Secret fields never display what is stored — the keychain is write-only from here, and
/// the panel says "stored" or "not set" beside them. A field left blank on save keeps
/// whatever is already in the keychain; that is what lets someone change their model
/// without re-pasting a key.
/// Which edge is being dragged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Divider {
    Sidebar,
    Panel,
}

/// Which choice a popup is currently open for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Picker {
    Theme,
    Model,
    /// Which project the open conversation is filed under.
    Project,
    /// Which project a *new* conversation should start in. Same list, same "New project “…”"
    /// row; only what choosing does differs, so naming a project is one gesture either way.
    NewProject,
    /// A model for one specialist, by its index in the registry.
    ///
    /// The index rather than the name because a `Picker` is `Copy` and lives in a field that is
    /// compared on every frame; the name is one lookup away and the registry does not reorder
    /// within a session.
    Subagent(usize),
}

/// A page of the preferences window.
///
/// Setup is one of these rather than a pane of its own. It used to live in the right-hand
/// slot, which meant opening it *closed the research panel* and cost the chat 420px for as
/// long as it was open — and it is the same kind of thing as the rest of this window: something
/// you visit, change, and leave. Zed puts every one of these behind one nav rail, and the
/// screenshots of it are what settled the shape (docs §68).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Section {
    Appearance,
    #[default]
    Model,
    Research,
    Backend,
    Setup,
}

impl Section {
    /// In rail order.
    const ALL: [Section; 5] = [
        Section::Appearance,
        Section::Model,
        Section::Research,
        Section::Backend,
        Section::Setup,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Model => "Model",
            Section::Research => "Research",
            Section::Backend => "Backend",
            Section::Setup => "Setup",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Appearance => "appearance",
            Section::Model => "model",
            Section::Research => "research",
            Section::Backend => "backend",
            Section::Setup => "setup",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    ModelId,
    BaseUrl,
    ApiKey,
    AstaToken,
    AstaApiKey,
    Port,
}

impl Field {
    const ALL: [Field; 6] = [
        Field::ModelId,
        Field::BaseUrl,
        Field::ApiKey,
        Field::AstaToken,
        Field::AstaApiKey,
        Field::Port,
    ];

    fn label(self) -> &'static str {
        match self {
            Field::ModelId => "Or type any model id",
            Field::BaseUrl => "Base URL",
            Field::ApiKey => "API key",
            Field::AstaToken => "Asta token",
            Field::AstaApiKey => "Asta API key",
            Field::Port => "Backend port",
        }
    }

    /// Which page of the preferences window this field appears on.
    fn section(self) -> Section {
        match self {
            Field::ModelId | Field::BaseUrl | Field::ApiKey => Section::Model,
            Field::AstaToken | Field::AstaApiKey => Section::Research,
            Field::Port => Section::Backend,
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Field::ModelId => "e.g. a model released after this build",
            Field::BaseUrl => "https://… (custom providers only)",
            Field::ApiKey => "paste to set — stored in the OS keychain",
            Field::AstaToken => "paste to set",
            Field::AstaApiKey => "paste to set",
            Field::Port => "2024",
        }
    }

    fn is_secret(self) -> bool {
        matches!(self, Field::ApiKey | Field::AstaToken | Field::AstaApiKey)
    }

    /// The keychain entry a secret field writes to. `None` for the provider key, whose
    /// name depends on the provider currently chosen.
    fn secret_name(self) -> Option<&'static str> {
        match self {
            Field::AstaToken => Some("ASTA_TOKEN"),
            Field::AstaApiKey => Some("ASTA_API_KEY"),
            _ => None,
        }
    }
}

/// Bindings the workbench itself owns. `ctrl-p`/`cmd-p` is deliberately *not*
/// scoped to a key context: it has to open the palette while the chat composer has
/// focus, which is where focus almost always is.
fn workbench_key_bindings() -> Vec<KeyBinding> {
    let palette = Some("Palette");
    let mut bindings = vec![
        KeyBinding::new("escape", PaletteDismiss, palette),
        KeyBinding::new("down", PaletteNext, palette),
        KeyBinding::new("up", PalettePrev, palette),
        // Escape everywhere else. Bound with **no** key context, because the composer
        // almost always has focus and a binding scoped to the workbench would never be
        // reached from there — the reason Escape did nothing to a modal (docs §58).
        //
        // It also **outranks** the palette binding above, which is the opposite of what this
        // comment used to claim. `Keymap::binding_enabled` scores a context-less binding at
        // `contexts.len()` — deeper than any predicate can match — and actions stop propagation
        // during the bubble phase, so `PaletteDismiss` is never reached (docs §84). The palette
        // is therefore closed by `dismiss` like every other overlay, and the binding above is
        // kept only for the arrow keys it sits beside.
        KeyBinding::new("escape", Dismiss, None),
    ];
    for modifier in ["cmd", "ctrl"] {
        bindings.push(KeyBinding::new(
            &format!("{modifier}-p"),
            TogglePalette,
            None,
        ));
        bindings.push(KeyBinding::new(
            &format!("{modifier}-,"),
            ToggleSettings,
            None,
        ));
        // Also unscoped, and for the same reason Escape is: focus lives in the composer,
        // so a workbench-scoped binding would never be reached. The composer's own
        // `ctrl-c` is more specific and still wins — it just declines the action when it
        // has nothing selected, which is what lets a transcript selection be copied
        // without first clicking somewhere to move focus (docs §62).
        bindings.push(KeyBinding::new(
            &format!("{modifier}-c"),
            CopySelection,
            None,
        ));
        bindings.push(KeyBinding::new(
            &format!("{modifier}-shift-a"),
            SelectAllTranscript,
            None,
        ));
    }
    bindings
}

/// Which face of the provenance record is showing.
///
/// One modal, two views, one dataset (docs §74). They are not alternatives so much as two
/// distances: the timeline is what happened, in order, with durations; the graph is the shape that
/// falls out of it once invocations collapse into kinds. The timeline earns its keep on the first
/// conversation, the graph on the tenth — when the loop is the thing worth seeing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ProvenanceView {
    Timeline,
    Graph,
}

/// One command-palette entry.
///
/// Deliberately a closed enum rather than a registry of closures: the whole point of
/// the palette is that every action is also reachable another way, so there is no
/// dynamic set to register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Command {
    RunTurn,
    NewThread,
    RefreshSpine,
    ExpandTraces,
    CollapseTraces,
    CopyLastAnswer,
    CopySelected,
    SelectWhole,
    SpecialistInBackground,
    RestartBackend,
    RenderReport,
    FileInProject,
    OpenAbout,
    OpenProvenance,
    OpenSettings,
    OpenSetup,
    Quit,
}

impl Command {
    const ALL: [Command; 17] = [
        Command::RunTurn,
        Command::NewThread,
        Command::RefreshSpine,
        Command::ExpandTraces,
        Command::CollapseTraces,
        Command::CopyLastAnswer,
        Command::CopySelected,
        Command::SelectWhole,
        Command::SpecialistInBackground,
        Command::RestartBackend,
        Command::RenderReport,
        Command::FileInProject,
        Command::OpenAbout,
        Command::OpenProvenance,
        Command::OpenSettings,
        Command::OpenSetup,
        Command::Quit,
    ];

    fn label(self) -> &'static str {
        match self {
            Command::RunTurn => "Run turn",
            Command::NewThread => "New thread",
            Command::RefreshSpine => "Refresh project spine",
            Command::ExpandTraces => "Expand agent activity",
            Command::CollapseTraces => "Collapse agent activity",
            Command::CopyLastAnswer => "Copy last answer",
            Command::CopySelected => "Copy selected text",
            Command::SelectWhole => "Select the whole conversation",
            Command::SpecialistInBackground => "Run the named specialist in the background",
            Command::RestartBackend => "Restart the backend",
            Command::RenderReport => "Save the latest report as a PDF",
            Command::FileInProject => "Put this conversation in a project",
            Command::OpenAbout => "About Mini-Me",
            Command::OpenProvenance => "Show this conversation's provenance",
            Command::OpenSettings => "Settings",
            Command::OpenSetup => "Setup & diagnostics",
            Command::Quit => "Quit",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Command::RunTurn => "send what is in the composer",
            Command::NewThread => "start a fresh conversation",
            Command::RefreshSpine => "reload mission, completed and pending",
            Command::ExpandTraces => "open every subagent group",
            Command::CollapseTraces => "close every subagent group",
            Command::CopyLastAnswer => "to the clipboard",
            Command::CopySelected => "what you dragged over in the transcript (ctrl-c)",
            Command::SelectWhole => "every message, ready to copy (ctrl-shift-a)",
            Command::SpecialistInBackground => "sends the /name in the composer, without waiting",
            Command::RestartBackend => "after updating the app — reloads its Python overlay",
            Command::RenderReport => "typeset with citations, into this conversation's folder",
            Command::FileInProject => "its folder moves there too, so Explorer matches",
            Command::OpenAbout => {
                "what the specialists do, where the data comes from, how to cite it"
            }
            Command::OpenProvenance => "which specialists were consulted, and in what order",
            Command::OpenSettings => "model, keys, execution (ctrl-,)",
            Command::OpenSetup => "check what the backend still needs",
            Command::Quit => "close the window and the sidecar",
        }
    }
}

/// Score how well `query` matches `label`, or `None` when it doesn't match.
///
/// A plain subsequence test is too loose to *filter* on: "nt" also matches
/// "ru**n** **t**urn" and "expa**n**d ac**t**ivity". So matches are **ranked** instead
/// of hidden — a hit at the start of a word counts for much more than one mid-word,
/// and a hit adjacent to the previous one counts for more again. The list stays
/// sorted, so "nt" puts "New thread" under the cursor while still showing the rest.
pub(crate) fn match_score(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.to_lowercase().chars().collect();
    let query = query.to_lowercase();

    let mut score = 0;
    let mut cursor = 0;
    let mut previous_end = None;
    for needle in query.chars().filter(|c| !c.is_whitespace()) {
        let at = cursor + label[cursor..].iter().position(|c| *c == needle)?;
        let starts_a_word = at == 0 || label[at - 1] == ' ' || label[at - 1] == '-';
        score += if starts_a_word { 8 } else { 1 };
        if previous_end == Some(at) {
            score += 4;
        }
        previous_end = Some(at + 1);
        cursor = at + 1;
    }
    Some(score)
}



/// Fold an incoming project spine into what is already on screen.
///
/// **Suggestions survive a spine that doesn't mention them.** Upstream recomputes them
/// opportunistically — `ProjectSpineMiddleware.abefore_agent` derives them from whatever
/// artifacts the thread has, and emits a payload with mission and completed work even when
/// it produces none. Treating each snapshot as authoritative therefore *erased* the
/// suggestions mid-turn: they appeared, the user started reading one, the answer arrived,
/// and the card vanished before it could be clicked (reported 2026-07-31).
///
/// Advisory content is different from state: a payload without suggestions means "no new
/// advice", not "the advice is withdrawn". Everything else — mission, completed, pending —
/// is authoritative and replaces.
/// Put file paths into the composer, and write nothing else.
///
/// **Loaded into the composer, never sent.** Dropping a file is a clumsy gesture — it
/// happens by accident — and the same rule already governs the suggestion cards: the app
/// prepares the question, the person asks it (docs §12).
///
/// **And it prepares only the part it knows.** §28 filled the composer with a whole
/// question — *"Analyse the data in …. Start by describing what it contains."* — which is a
/// guess about the research, made by the only participant who has not seen the data. Asked
/// to remove it: *"let's avoid that so the user can have flexibility in his query"* (§180).
/// The path is the one thing here the app knows and the researcher would rather not type;
/// what to do with it is theirs.
///
/// So the paths go in and the caret ends after them, wherever they are:
///
/// - Nothing typed yet — the paths first, then a blank line to carry on writing under.
/// - Something typed — the paths underneath it, after a blank line, leaving every word alone.
///
/// Directories need no special case now that no sentence is written about them. The agent
/// can list one itself, and "analyse this folder of readings" is the researcher's sentence
/// to write.
/// One file the researcher attached, waiting to go with the next question.
///
/// **Two strings, because the reader and the agent need different ones.** The chip shows a
/// filename; the turn carries a path. Writing the path into the composer served both badly — the
/// researcher saw three wrapped lines of `/mnt/c/Users/LENOVO/Documents/Mini-Me/01a01ae5-27e8-…/`
/// where a name would do, and could not remove one without editing text they had not typed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Attachment {
    /// The file's own name, which is what a person recognises.
    label: String,
    /// Where it came from, kept so a file that could not be copied yet can be copied later.
    source: std::path::PathBuf,
    /// Whether it is already inside the conversation's folder.
    adopted: bool,
    /// What the agent is told. `./name` for a file copied into the conversation — the workspace
    /// *is* the agent's working directory — and the absolute path for one that had to stay put.
    reference: String,
}

/// The blockquote the backend has always described and this app never sent.
///
/// `backend/prompts.py:188` tells the coordinator that a message may begin with
///
/// > Attached files (already saved in the sandbox working directory): `./<name>`
///
/// three subagent prompts tell their specialists to read the paths out of it, and
/// `backend/project.py:_strip_attached_files_blockquote` drops it before seeding the mission. All
/// of that was written for the web frontend. The desktop app put bare paths on their own lines, so
/// the format four prompts agree on arrived from one client only — and the mission seed for every
/// desktop conversation that began with a file started with a path (docs §231).
///
/// `None` when nothing is attached, so a plain question stays a plain question.
fn attached_blockquote(attachments: &[Attachment]) -> Option<String> {
    if attachments.is_empty() {
        return None;
    }
    let listed = attachments
        .iter()
        .map(|attachment| format!("`{}`", attachment.reference))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "> Attached files (already saved in the sandbox working directory): {listed}"
    ))
}

/// The sources of every attachment that is not yet inside the conversation's folder.
///
/// Pure so the rule is testable without a window: a file already copied in must not be copied
/// again, and one that was not must not be forgotten.
fn awaiting_adoption(attachments: &[Attachment]) -> Vec<std::path::PathBuf> {
    attachments
        .iter()
        .filter(|attachment| !attachment.adopted)
        .map(|attachment| attachment.source.clone())
        .collect()
}

/// The turn to send: the blockquote, then what they typed.
fn with_attachments(typed: &str, attachments: &[Attachment]) -> String {
    match attached_blockquote(attachments) {
        None => typed.trim().to_string(),
        Some(quote) => format!("{quote}\n\n{}", typed.trim()),
    }
}

/// What a per-specialist model row has to say about the provider it would actually run on.
///
/// **A flat list of five providers' models, and only one thing told them apart.** The row used to
/// be annotated *only* when the provider had no key — on the argument that a missing key is the
/// thing a researcher has to act on before the choice can work. True, and it optimised for *can
/// this run* while the question that mattered was *whose account pays*.
///
/// Here is what that cost. With the coordinator on `custom` (which is how OpenRouter is reached),
/// the same dropdown offers:
///
/// - `gpt-4.1` — the `openai` provider. OpenAI direct, billed to an OpenAI account.
/// - `openai/gpt-4o-mini` — the `custom` provider. OpenRouter, billed to an OpenRouter account.
///
/// A slash. That was the entire visible difference. A researcher with credits on OpenRouter set
/// `academic_researcher` to `gpt-4.1`, saw no warning *because they did have an OpenAI key*, and
/// the next literature search — which delegates to exactly that specialist — died several minutes
/// in on an exhausted account they had not chosen to use (docs §187).
///
/// So every row that leaves the coordinator's provider now says so, keyed or not. `None` only for
/// the provider already running the conversation, where there is nothing to warn about and a note
/// on every row would be noise.
fn specialist_note(
    model_provider: &settings::Provider,
    coordinator: &str,
    has_key: bool,
) -> Option<String> {
    if model_provider.id == coordinator {
        return None;
    }
    Some(if has_key {
        // The consequence, not the mechanism: "a different provider" is a fact about
        // configuration, "billed separately" is a fact about money.
        format!("{} — billed separately", model_provider.label)
    } else {
        format!("{} — no key stored", model_provider.label)
    })
}

/// What a recorded theme name should become once a palette file has been removed.
///
/// `None` when the name still resolves — either it was a built-in all along, or the deleted file
/// was only *overriding* one and the bundled palette underneath has taken its place. `Some` names
/// the default, the one palette guaranteed to exist.
///
/// A function rather than a line inside the handler because it has to be asked twice, of two
/// strings that are not the same: the palette on screen, and the one `settings.toml` records.
/// Those drift apart the moment somebody previews a theme, and only the second one survives Esc.
fn theme_after_removal(name: &str, survivors: &[(String, theme::Theme)]) -> Option<String> {
    let survives = survivors
        .iter()
        .any(|(candidate, _)| candidate.eq_ignore_ascii_case(name));
    (!survives).then(|| theme::DEFAULT_NAME.to_string())
}

/// A dropped path as a person would name it: the filename, or the whole path if it has none.
fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The first URL in a line of text.
///
/// Stops at whitespace and trims the punctuation a sentence tends to leave attached, so
/// "open https://example.org/x." yields the URL without the full stop.
///
/// Two callers with the same problem: a device sign-in URL inside `asta auth login`'s output, and
/// the link inside a citation the agent wrote — `Smith et al. (2021). Late blight…
/// https://doi.org/10.1234/x.` Both are one line of someone else's prose with a link somewhere in
/// it. `http://` as well as `https://` for the second: plenty of older DOIs and institutional
/// repositories are still published that way, and a source row that silently would not open
/// because of the scheme is indistinguishable from one with no link at all.
fn first_url(line: &str) -> Option<String> {
    let at = match (line.find("https://"), line.find("http://")) {
        (Some(secure), Some(plain)) => secure.min(plain),
        (found, None) | (None, found) => found?,
    };
    let url: String = line[at..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    // A trailing colon is not punctuation you would expect to matter — until you see the
    // real line, `gio: <url>: Operation not supported`, where it is what separates the URL
    // from the error. Safe to strip: a colon is meaningful *inside* a URL (a port), never
    // at the end of one.
    let url = url.trim_end_matches(['.', ',', ':', ';', ')', ']', '"', '\'']);
    // A bare scheme is not a link — measured against *the scheme this one has*. A fixed floor
    // does not work with two schemes: `"http://".len()` lets a bare `https://` through, and
    // `"https://".len()` rejects the real `http://x.org`.
    let scheme = if url.starts_with("https://") {
        "https://"
    } else {
        "http://"
    };
    (url.len() > scheme.len()).then(|| url.to_string())
}

/// The device code out of a sign-in URL (`…?user_code=KFDM-BQQG`).
///
/// Worth pulling out on its own because the URL is a single unbreakable word: it cannot
/// wrap, so in a 420px pane it runs off the edge and the code — the one part the user has
/// to read and type on the page — is exactly what gets clipped.
fn device_code(url: &str) -> Option<String> {
    let code: String = url
        .split_once("user_code=")?
        .1
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    (!code.is_empty()).then_some(code)
}

/// Open a URL in the **host's** browser.
///
/// Deliberately not routed through `shell_argv`: that would run inside the WSL distro,
/// which is exactly where it does not work — `asta auth login` already tries there and
/// reports `gio: … Operation not supported`. The browser is on Windows, so this runs on
/// Windows.
fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // The empty argument is `start`'s title parameter. Without it, a quoted URL is
        // taken *as* the title and nothing opens.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }
}

/// Whether a turn failure is really the machine not being set up.
///
/// Matching on message text is not elegant, and it is the honest option here: these
/// strings are raised by `BackendSupervisor` in this same crate, the test below pins
/// them, and the alternative — a typed error threaded through the whole streaming
/// path — would be a lot of plumbing to decide which pane to open.
///
/// Attach-only is deliberately *not* in the list: that mode is opt-in and its message
/// already names its own fix, so a setup pane would be answering a question nobody asked.
fn looks_like_a_setup_failure(message: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "no langgraph.json",
        "failed to launch the backend",
        "failed to spawn the backend",
        "backend exited during startup",
        "did not become healthy",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

fn merge_spine(previous: Option<&Project>, incoming: Project) -> Project {
    let mut merged = incoming;
    if merged.suggestions.is_empty() {
        if let Some(previous) = previous {
            merged.suggestions = previous.suggestions.clone();
        }
    }
    merged
}

/// The unit a centred confirmation is about.
///
/// The project variant owns its complete conversation list at the moment it opens. Building it
/// from the sidebar's *filtered* rows would make a search silently spare conversations that the
/// modal just said were going away (§155).
#[derive(Clone, Debug)]
enum DeleteTarget {
    Conversation(protocol::Conversation),
    Project {
        name: String,
        conversations: Vec<protocol::Conversation>,
    },
}

impl DeleteTarget {
    fn thread_ids(&self) -> Vec<String> {
        match self {
            Self::Conversation(conversation) => vec![conversation.thread_id.clone()],
            Self::Project { conversations, .. } => conversations
                .iter()
                .map(|conversation| conversation.thread_id.clone())
                .collect(),
        }
    }

    fn contains_thread(&self, thread_id: &str) -> bool {
        match self {
            Self::Conversation(conversation) => conversation.thread_id == thread_id,
            Self::Project { conversations, .. } => conversations
                .iter()
                .any(|conversation| conversation.thread_id == thread_id),
        }
    }

    fn files(&self) -> sidecar::DeleteFiles {
        match self {
            Self::Conversation(conversation) => sidecar::DeleteFiles::Conversation {
                project: conversation.project.clone(),
                thread_id: conversation.thread_id.clone(),
            },
            Self::Project { name, .. } => sidecar::DeleteFiles::Project { name: name.clone() },
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Self::Conversation(_) => "conversation",
            Self::Project { .. } => "project",
        }
    }
}

/// What the sidebar may do after asking the backend to delete confirmed work.
///
/// Kept separate from the async UI so the dangerous rule is testable: absence of a successful
/// answer means the durable thread may still exist, therefore its row must stay (§154).
enum DeleteResolution {
    Remove { files_error: Option<String> },
    Keep(String),
}

fn resolve_delete(
    noun: &str,
    result: Option<anyhow::Result<sidecar::DeleteOutcome>>,
) -> DeleteResolution {
    match result {
        Some(Ok(outcome)) => DeleteResolution::Remove {
            files_error: outcome.files_error,
        },
        Some(Err(error)) => {
            DeleteResolution::Keep(format!("couldn't delete the {noun}: {error:#}"))
        }
        None => DeleteResolution::Keep(format!(
            "couldn't confirm deletion — the {noun} is still shown"
        )),
    }
}

/// Whether a project still exists after a deletion result has been applied.
///
/// §106 defines existence from conversation metadata, not from a remembered selection and not
/// from a possibly locked folder. Keeping this tiny rule outside the callback makes the
/// last-conversation boundary testable — the boundary that resurrected projects in §154.
fn project_exists(conversations: &[protocol::Conversation], name: &str) -> bool {
    conversations
        .iter()
        .any(|conversation| conversation.project.as_deref() == Some(name))
}

/// A single chat message in the transcript, plus the agent activity behind it.
struct Message {
    role: &'static str,
    body: String,
    /// Coordinator-level steps (tool calls, delegations), in the order they happened.
    steps: Vec<String>,
    /// One group per subagent invocation.
    agents: Vec<AgentTrace>,
    /// Files this answer named that the conversation's folder does not hold.
    ///
    /// Recomputed as outputs settle rather than fixed when the turn ends, because a background
    /// worker can still be writing — a name that is missing at second one and present at second
    /// three was never a false claim, and flagging it would be its own kind of lie (§175).
    unverified: Vec<String>,
    /// Whether the coordinator's own steps are showing.
    ///
    /// Open while the turn runs, because during a two-minute wait the steps *are* the only
    /// sign of progress; closed once it ends, because then the answer is the point. Zed's
    /// agent panel converged on the same shape, and the pattern has a name — expand live,
    /// collapse on completion (docs §47).
    steps_expanded: bool,
    /// `body` parsed into blocks, kept beside it rather than recomputed.
    ///
    /// It used to be parsed **in `render`**, which meant every message in the conversation was
    /// re-parsed on every frame — sixty times a second, for text that had not changed since it
    /// arrived. Now a message parses when its body changes: once for a finished one, and once
    /// per token for the single message still streaming.
    ///
    /// Empty for `you` messages, which are shown as typed — reinterpreting someone's asterisks
    /// would be presumptuous, and §14 settled that.
    blocks: Vec<markdown::Block>,
    /// Whether the reader stopped this turn before it finished.
    ///
    /// Shown, because a cut-off answer and a complete one are otherwise the same thing on
    /// screen — and the difference decides whether it can be trusted (docs §63).
    stopped: bool,
    /// Files this turn produced, shown inline beneath the answer.
    ///
    /// Found by diffing the thread's workspace across the turn rather than reported by the
    /// agent: a plot is usually written by a `matplotlib` script inside `execute`, which
    /// registers no artifact and tells the client nothing. The file appearing on disk is
    /// the only signal there is (docs §42).
    ///
    /// **Every output, not only figures.** This held images alone, so a turn that produced a
    /// cleaned dataset said so in prose and showed nothing — the researcher had to go and find
    /// it in the side panel to learn whether it had 40 rows or 40,000. The diff never cared what
    /// kind of file it was; only the renderer did.
    outputs: Vec<workspace::Output>,
}

impl Message {
    fn new(role: &'static str, body: String) -> Self {
        let blocks = Self::parse(role, &body);
        Self {
            role,
            body,
            blocks,
            steps: Vec::new(),
            agents: Vec::new(),
            steps_expanded: true,
            stopped: false,
            outputs: Vec::new(),
            unverified: Vec::new(),
        }
    }

    fn parse(role: &'static str, body: &str) -> Vec<markdown::Block> {
        if role == "you" || body.is_empty() {
            Vec::new()
        } else {
            markdown::parse(body)
        }
    }

    /// Append streamed text, keeping the parsed blocks in step.
    ///
    /// The only way the body grows, so the cache cannot be left stale by a caller that forgot
    /// to refresh it.
    fn push_body(&mut self, text: &str) {
        self.body.push_str(text);
        self.blocks = Self::parse(self.role, &self.body);
    }

    /// Nothing happened here worth keeping. A turn that produced only tool calls
    /// still has activity, so "empty body" alone is not enough to drop a message —
    /// that would throw away the only record of a purely delegated turn.
    fn is_silent(&self) -> bool {
        // A stopped turn counts as content even with nothing in it: "you stopped this" is
        // the whole record of what happened, and pruning it would leave a question that
        // appears never to have been answered for no stated reason (docs §63).
        self.body.is_empty() && self.steps.is_empty() && self.agents.is_empty() && !self.stopped
    }

    /// The rendered words Select All should copy when this row is off screen (docs §156).
    fn selection_text(&self) -> String {
        use markdown::Block;

        if self.role == "you" {
            return self.body.clone();
        }
        let mut runs = Vec::new();
        for block in &self.blocks {
            match block {
                Block::Heading { inlines, .. }
                | Block::Paragraph(inlines)
                | Block::ListItem { inlines, .. }
                | Block::Quote { inlines, .. } => runs.push(inlines.text.clone()),
                Block::Code { text, .. } => runs.push(text.clone()),
                Block::Table { header, rows } => {
                    runs.extend(header.iter().map(|cell| cell.text.clone()));
                    runs.extend(rows.iter().flat_map(|row| row.iter().map(|cell| cell.text.clone())));
                }
                // These have never registered selectable transcript text.
                Block::Image { .. } | Block::Rule => {}
            }
        }
        runs.join("\n")
    }
}

/// Live trace of one subagent invocation.
struct AgentTrace {
    /// The namespace from [`AgentRef`] — the grouping key, unique per invocation.
    ns: String,
    name: String,
    steps: Vec<String>,
    text: String,
    expanded: bool,
}

/// Most text one trace keeps. A trace is a tail-followed log, and a research turn
/// can stream far more subagent text than the answer it produces, so when a group
/// overflows we drop from the *front*: the newest work is what the user is watching.
const MAX_TRACE_CHARS: usize = 4_000;

impl AgentTrace {
    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
        let overflow = self.text.chars().count().saturating_sub(MAX_TRACE_CHARS);
        if overflow > 0 {
            let kept: String = self.text.chars().skip(overflow).collect();
            self.text = format!("…{kept}");
        }
    }
}

/// Find (or start) the trace group for a subagent invocation.
fn trace_for<'a>(message: &'a mut Message, agent: &AgentRef) -> &'a mut AgentTrace {
    if let Some(index) = message.agents.iter().position(|trace| trace.ns == agent.ns) {
        return &mut message.agents[index];
    }
    message.agents.push(AgentTrace {
        ns: agent.ns.clone(),
        name: agent.name.clone(),
        steps: Vec::new(),
        text: String::new(),
        // A subagent that just started is what is happening *now*, so it opens
        // expanded; the turn ending collapses everything so the answer stays primary.
        expanded: true,
    });
    message.agents.last_mut().expect("just pushed")
}

/// How deeply nested a delegation is: 0 for one the coordinator made itself.
///
/// The namespace is a `|`-joined path (`NS_SEP`, docs §75), so depth is a segment count.
fn depth(ns: &str) -> usize {
    ns.split('|').count().saturating_sub(1)
}

/// A duration in the units a person reads it in.
///
/// Sub-second work is reported in milliseconds because that is what distinguishes a cache hit
/// from a real call; anything longer is seconds, where a hundred milliseconds is noise.
fn duration_label(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.)
    } else {
        format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

/// A prompt reduced to a heading: one line, bounded.
///
/// A `/subagent` turn is a paragraph of instruction to the coordinator, and a timeline row headed
/// by all of it would be a wall of near-identical text.
fn one_line(prompt: &str) -> String {
    let flattened = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    match flattened.char_indices().nth(90) {
        Some((at, _)) => format!("{}…", &flattened[..at]),
        None => flattened,
    }
}

/// Root view: the three-pane research workbench.
/// A setup command the app is running for the user, and its output so far.
struct RunningFix {
    label: String,
    /// A sign-in URL the command printed, if any. See [`Workbench::open_link`] — the
    /// command runs *inside the distro*, which has no browser, so the link has to be
    /// opened out here on the host.
    link: Option<String>,
    /// The last [`FIX_LOG_LINES`] lines. Capped because `uv sync` prints hundreds and
    /// the pane is 420px wide — the tail is the part that matters.
    lines: Vec<String>,
    /// What *we* have to say about the run — the verdict, and what to do next. Kept apart
    /// from `lines` because it is not output: pushing "— finished" in with the command's own
    /// lines is why a fix that printed nothing showed a box containing one dash instead of
    /// admitting there was nothing to show (docs §60).
    notes: Vec<String>,
    /// Which check this fix belongs to, so a re-check can tell whether it worked.
    check_id: &'static str,
    done: bool,
    ok: bool,
    /// The way to stop it, for as long as there is a process to stop (docs §172).
    cancel: preflight::Cancel,
    /// Set once Stop has been pressed, so the pane can say *stopping* rather than claiming a
    /// finish it has not seen. §168's rule: **stopped** is only true once the command has
    /// actually exited, and that arrives as `FixEvent::Finished` like any other ending.
    stopping: bool,
}

/// How much of a fix's output the pane keeps.
const FIX_LOG_LINES: usize = 200;

struct Workbench {
    /// The project spine from `GET /project`. `None` until the first fetch lands
    /// (or if the backend isn't up yet) — the panel says so rather than lying.
    project: Option<Project>,
    /// Research outputs, from the latest `values` snapshot of the current run.
    buckets: Vec<Bucket>,
    /// Long jobs (theorizer, DataVoyager) started by a turn and still being watched.
    /// Kept for the whole session so a finished one stays visible rather than vanishing.
    jobs: Vec<protocol::Job>,
    /// Work handed to a background Mini-Me, and whether it is stopped at the gate.
    tasks: Vec<protocol::AsyncTask>,
    transcript: Vec<Message>,
    /// Reports already written to disk this session, so each is announced once rather than on
    /// every `values` frame of the turn that produced it.
    saved_reports: std::collections::HashSet<std::path::PathBuf>,
    /// The reports this conversation has produced, bodies included, newest last.
    reports: Vec<protocol::Report>,
    /// Whole citations, for a rendered report's bibliography. Not the panel's truncated ones.
    sources: Vec<protocol::Source>,
    /// Datasets the explorer recommended, whole. See [`protocol::Dataset`].
    /// The rows the panel shows: **what the search returned**, read from the conversation's
    /// own `dataverse_search.json`.
    ///
    /// Until §290 this was the model's structured answer — seven fields per row retyped by a
    /// language model out of a file it had just read — and on a real turn six of six
    /// `persistent_id`s were composed rather than copied. A researcher was one click from pasting
    /// a fabricated DOI into a paper.
    datasets: Vec<protocol::Dataset>,
    /// The model's own answer, kept as the fallback for a run that wrote no file — a sandboxed
    /// deployment, or a conversation from before the file existed. Never preferred over it.
    recommended_datasets: Vec<protocol::Dataset>,
    /// The identifiers the model put forward, bare, for marking rows it chose.
    ///
    /// **A mark on a row, never the row itself.** An identifier that is not in the search has
    /// nothing to mark, which is the whole point: it cannot be rendered, so it cannot be cited.
    recommended_ids: Vec<String>,
    /// The datasets the researcher has ticked, by `persistent_id`.
    ///
    /// **Theirs, not the agent's.** The agent's opinion is a mark and a sort (§290); this is the
    /// selection, and the whole flow the researcher described is *"select the dois, download one
    /// or many and then ask the app to analyze whatever we want"* (§297). Cleared when the
    /// conversation changes, and a pick drops itself once its file has landed.
    dataset_picks: std::collections::HashSet<String>,
    /// What this conversation's searches reported finding — the denominator, when there is one.
    search_totals: workspace::SearchTotals,
    /// Documents the librarian indexed. See [`protocol::Document`].
    documents: Vec<protocol::Document>,
    /// What checking each reference against Crossref found, keyed by its citation text.
    ///
    /// **Keyed by the citation, not by position and not by DOI.** Not by position because a turn
    /// can add sources while a check is running, and a verdict shown against the wrong reference
    /// is worse than none. Not by DOI because the references that most need an answer are the
    /// ones that have no DOI at all — which is the shape a fabricated citation takes.
    checked: HashMap<String, references::Verdict>,
    /// What the registry had for each bad citation, once it has been asked.
    ///
    /// `None` records **asked, and there is no such work** — which is not an absence of
    /// information, it is the most important thing this feature can tell anyone. A reference
    /// whose DOI is unregistered *and* whose text matches nothing in the registry was not
    /// mis-transcribed; it does not describe a paper that exists.
    repaired: HashMap<String, Option<references::Repair>>,

    /// How many references are still being resolved.
    ///
    /// A count rather than a flag, because sources arrive across several turns and a second
    /// batch can start while the first is still going.
    resolving: usize,
    /// Whether the About window is showing.
    about_open: bool,
    /// Whether the provenance window is showing, and which of its two views.
    provenance_open: bool,
    provenance_view: ProvenanceView,
    /// Which turn the graph is filtered to, or `None` for the whole conversation.
    provenance_turn: Option<usize>,
    /// What this conversation consulted, and in what order (docs §73–§75).
    ///
    /// Held here rather than derived from `transcript` because the transcript deliberately does
    /// not survive a reload: `conversation_messages` returns role and text only, since the
    /// activity trace was assembled from a stream that is over. This record is written to the
    /// thread's directory as turns finish, which makes it the one part of a turn's activity that
    /// can be reopened — and "track his work by conversation" is the whole request.
    provenance: provenance::Record,
    sidecar: Arc<Sidecar>,
    /// Status line text (backend/stream progress, not model output).
    status: String,
    /// True while a turn is in flight — gates the run button.
    streaming: bool,
    /// Set when the last turn failed, rendered in the status line.
    error: Option<String>,
    /// The text field. Owns its own focus/selection state.
    composer: Entity<Composer>,
    /// Command palette: open flag, the highlighted row, and its own query field.
    /// The query is a second `Composer`, kept alive across opens so its
    /// subscriptions are registered once rather than per-open.
    palette_open: bool,
    palette_selected: usize,
    palette_query: Entity<Composer>,
    /// Settings pane: open flag, the draft being edited, and one field per input.
    settings_open: bool,
    draft: settings::Settings,
    fields: Vec<(Field, Entity<Composer>)>,
    /// What the last save did, shown in the pane. Never contains a secret.
    settings_note: String,
    /// Setup pane: open flag, the last report, and whether checks are in flight.
    /// `None` before the first run — the pane says "checking…" rather than "all clear".
    /// Which page of the preferences window is showing.
    settings_section: Section,
    report: Option<preflight::Report>,
    checking: bool,
    /// Set when a fix has just succeeded, so the re-check it triggered is compared against
    /// the row it was meant to fix. See [`Workbench::judge_finished_fix`].
    judge_after_recheck: bool,
    /// A fix the app is running for the user: what it is, and what it has printed so far.
    /// `Some` means a command is live, which is what disables the other buttons.
    running_fix: Option<RunningFix>,
    /// A run paused at the approval gate: the command it wants to run, awaiting a
    /// decision. While this is set the turn is *open*, not finished.
    pending_approval: Option<ApprovalRequest>,
    /// The user approved everything remaining in *this* turn. Never persisted, and reset
    /// by [`Workbench::finish_turn`] — see the button's comment for why it is bounded.
    approve_rest_of_turn: bool,
    /// The user approved everything for the rest of this **conversation**, foreground and
    /// background alike.
    ///
    /// Turn scope was too small in practice: one analysis is a dozen commands across
    /// several turns, and a researcher who must click Approve twelve times stops reading
    /// by the third. This is the wider grant they asked for — still **never persisted**,
    /// still gone on "New thread" or a restart, and announced in the status bar the whole
    /// time it is in force, so it cannot be in effect without being visible (docs §41).
    approve_conversation: bool,
    /// Where this build stands against the newest published one, once GitHub has been asked.
    ///
    /// `None` until the answer arrives, which is the state the About page renders as "checking".
    /// The check runs once per launch and never blocks anything: an app that will not open because
    /// github.com is unreachable would be a worse failure than the one it exists to fix.
    update: Option<update::Standing>,
    /// How far a *taken* update has got, once the button has been pressed.
    ///
    /// Separate from `update` because they answer different questions — "is there a newer build"
    /// survives a failed download, and a failed download must not erase the fact that one exists.
    taking: Option<update::Fetch>,
    /// The researcher sent the update chip away for this session.
    ///
    /// Not persisted, and deliberately: an update dismissed forever is an install that never
    /// updates, which is the thing this whole flow exists to prevent. It comes back next launch,
    /// which is also when there is something new to say.
    update_dismissed: bool,
    /// The record of what this conversation ran is open.
    ///
    /// A modal rather than a panel section: the list is long by nature, and the question it answers
    /// — *what did that turn actually do* — is asked occasionally and read closely, which is the
    /// opposite of what a permanently-visible section is for.
    commands_open: bool,
    /// The record of what this conversation's subagents *claimed* is open.
    ///
    /// A second modal rather than a tab inside the first, because they answer different questions
    /// about different actors: `WHAT RAN` is the shell, this is what a subagent said it produced.
    /// Folding them together would make the shorter one — usually this one — the harder to find.
    claims_open: bool,
    /// The delete being confirmed would interrupt background work that says it is still running.
    ///
    /// Carried to the confirmation modal so the sentence that asks can say so. Not a refusal:
    /// see `ask_to_delete` for what refusing cost.
    delete_interrupts_work: bool,
    /// What came back from the last press of "Bring them in", or that one is in flight.
    ///
    /// Kept so the modal can report *what happened to each file* rather than a count. A partial
    /// result is the normal case — `/tmp` is swept — and "3 of 5" is not something anyone can act
    /// on.
    collecting: Option<Result<protocol::Collected, String>>,
    collect_in_flight: bool,
    /// Absolute paths this conversation's commands were **watched writing** outside it.
    ///
    /// Cached because the offer to fetch them is drawn beside a transcript message, and
    /// `transcript_message` runs per visible row inside a virtualized list — reading the command
    /// ledger off disk there would put a file read in the scroll path. Refreshed by
    /// `check_file_claims`, which already walks the workspace and already re-runs as outputs
    /// settle, so this costs one more read at the point the answer is being checked anyway.
    stray: Vec<String>,
    /// Which transcript message carries the offer, if any.
    ///
    /// **One offer per conversation, never one per message.** The files are the conversation's,
    /// not any single turn's — `collect_outside` fetches all of them whichever button is pressed —
    /// so repeating the control under every flagged answer would be three buttons that do the
    /// same thing. It sits on the newest message that named a missing file, which is where the
    /// researcher is already looking, and on the newest answer otherwise (see
    /// `Self::place_recovery_offer` for why the second case has to exist at all).
    recovery_on: Option<usize>,
    /// Whether this install is an unzipped bundle or a `cargo build` inside a checkout.
    ///
    /// Read once at startup rather than per render, and kept beside the standing because the two
    /// together decide what the About page may offer — a source build is told about a new release
    /// and pointedly not offered a button that would unzip over its worktree.
    install: update::Layout,
    /// Filters the *installed* theme list. With a hundred palettes installed, a list you
    /// can only scroll is a list you cannot use.
    theme_filter: Entity<Composer>,
    /// Narrows the model picker. Necessary rather than a nicety: a gateway's catalogue runs to
    /// several hundred ids, and a scroll box is not a way to find `deepseek` among them (§188).
    model_filter: Entity<Composer>,
    /// Filter for the project picker, which doubles as the field a new project is named in.
    project_query: Entity<Composer>,
    theme_scroll: gpui::ScrollHandle,
    model_scroll: gpui::ScrollHandle,
    /// What the gallery search box holds, and what it found.
    gallery_query: Entity<Composer>,
    gallery_results: Vec<gallery::Listing>,
    gallery_note: String,
    /// Measured variable-height rows and their scroll position. `uniform_list` would assign a
    /// one-line question and a two-page answer the same height (docs §156).
    transcript_list: ListState,
    /// Selected transcript text, and the span registry a drag hit-tests against.
    /// See [`selection`] — the registry is rebuilt every frame, the selection is not.
    text_selection: selection::Transcript,
    /// An open right-click menu, if any.
    context_menu: Option<menu::ContextMenu>,
    /// Projects that have a folder, including ones nothing is filed under yet.
    ///
    /// Read alongside the conversation list rather than per frame: the sidebar renders on every
    /// frame and this is a directory listing, which has no business on the render thread.
    folder_projects: Vec<String>,
    /// A conversation is being fetched. Its own flag rather than a status string, because the
    /// status bar is prose and prose cannot be asked a question (§177).
    opening: bool,
    /// The agent graph has not finished building. §176 measured that wait at fifteen seconds on
    /// a real machine, which is far too long to leave a window looking idle.
    warming: bool,
    /// An open sidebar `⋮` or `New` menu, and where its corner goes.
    sidebar_menu: Option<(SidebarMenu, gpui::Point<gpui::Pixels>)>,
    /// Which of the two sidebar lists — Conversations or Projects — is showing.
    sidebar_view: SidebarView,
    /// Which row of the `/name` picker is chosen. Reset on every keystroke.
    subagent_selected: usize,
    /// An open choice popup: which choice, and where its trigger was clicked.
    open_picker: Option<(Picker, gpui::Point<gpui::Pixels>)>,
    /// Pane widths, in pixels, and which edge is being dragged.
    ///
    /// Both were fixed numbers. 240px of conversation list is generous on a laptop and mean on
    /// a 4K monitor, and the person who knows which is the one looking at it.
    sidebar_width: f32,
    panel_width: f32,
    dragging: Option<Divider>,
    /// The preferences window's own focus, for pages that have no field to put it in.
    settings_focus: gpui::FocusHandle,
    /// The provenance window's own focus. It has no field at all, and §71 is the reason it
    /// needs one anyway: focus left on an element the open pane no longer renders means key
    /// bindings — Escape among them — simply stop arriving.
    provenance_focus: gpui::FocusHandle,
    /// The About window's own focus, for the same reason (docs §71).
    about_focus: gpui::FocusHandle,
    /// The delete warning's focus. It has buttons but no text field, so leaving focus on the
    /// sidebar row it covers would make Escape depend on an element hidden behind the modal.
    delete_focus: gpui::FocusHandle,
    /// Recent outcomes, newest last, each fading on its own timer.
    ///
    /// The status bar holds exactly one line, so an outcome worth reading — "copied 12 lines",
    /// "settings saved" — was routinely overwritten by the next thing that happened before
    /// anyone looked at it. These stack instead. Deliberately **only** for things a person
    /// did: streaming progress still goes to the status bar, because a toast per token would
    /// be a wall of them.
    toasts: Vec<SharedString>,
    panel_scroll: gpui::ScrollHandle,
    /// One horizontal position per output folder gallery.
    ///
    /// A single handle would make scrolling one folder move every other folder too. The full
    /// folder path plus its surface owns the state, matching §152's rule that each agent-chosen
    /// folder is one independent photo-like collection.
    output_gallery_scrolls: std::cell::RefCell<HashMap<String, gpui::ScrollHandle>>,
    /// The gallery thumb currently held by the mouse, if any.
    ///
    /// Kept separately from pane resizing because both are drags but their units differ: pane
    /// dividers follow window pixels directly, while a gallery thumb maps a short track onto a
    /// wider hidden content range (docs §158).
    gallery_scroll_drag: Option<GalleryScrollDrag>,
    /// The palette on screen right now, which is not always the saved one: the picker
    /// applies as you point at it so a theme can be judged by looking at it.
    applied_theme: String,
    /// Whether the conversation sidebar is showing. A researcher deep in one thread
    /// wants the screen, not the list.
    sidebar_open: bool,
    /// Whether the research panel on the right is showing.
    panel_open: bool,
    /// Whether the road strip down the left of the chat is showing.
    road_open: bool,
    /// What each output file turned out to be, keyed by path and stamped with the modification
    /// time it was measured at.
    ///
    /// The panel redraws on every frame — every streamed token, every scroll — and measuring a
    /// CSV means reading it. Without this, a turn that produced a 20 MB dataset would re-read
    /// that dataset sixty times a second on the thread drawing the window.
    ///
    /// A `RefCell` because the panel renders from `&self`, and a cache that could only be filled
    /// from `&mut self` would have to be refreshed by `render` for a panel that may not even be
    /// open. Never borrowed across a call that could re-enter.
    shapes: std::cell::RefCell<HashMap<PathBuf, (std::time::SystemTime, workspace::Shape)>>,
    /// The first rows of each table, for the cards in the transcript. Same key, same reason as
    /// [`Self::shapes`]; `None` records "looked, and it is not a table we can split", so a
    /// Markdown file is not re-opened on every frame to find that out again.
    #[allow(clippy::type_complexity)]
    previews:
        std::cell::RefCell<HashMap<PathBuf, (std::time::SystemTime, Option<Vec<Vec<String>>>)>>,
    /// What the sidebar's search box holds. Empty means "show everything".
    conversation_query: Entity<Composer>,
    /// A file being previewed in the centre, if any — and the set it can be stepped through.
    preview: Option<Preview>,
    /// The researcher's past conversations, newest first.
    conversations: Vec<protocol::Conversation>,
    /// How the app got hold of the backend it is talking to. `None` until it has one.
    backend_start: Option<backend::Started>,
    /// Whether the list has ever come back. `false` means *loading*, which is not the same as
    /// empty — and saying "conversations you start will appear here" over a list that is merely
    /// still arriving is a claim the researcher has none (docs §79).
    conversations_loaded: bool,
    /// A name to give the current conversation once its thread exists.
    pending_title: Option<String>,
    /// The thread whose name is being edited, if any.
    renaming: Option<String>,
    /// Whether the mission at the top of the research panel is being edited in place.
    editing_mission: bool,
    /// The coordinator's own plan for this conversation, when it wrote one. See [`protocol::Todo`].
    plan: Vec<protocol::Todo>,
    /// Who wrote each file, as the backend recorded it — see [`workspace::authorship`].
    authorship: std::collections::HashMap<String, String>,
    /// The manifest's size and mtime when it was last read, so a frame that changed nothing
    /// costs one `stat` rather than a parse.
    authorship_stamp: Option<(std::time::SystemTime, u64)>,
    /// The conversation or project whose delete control opened the centred warning.
    ///
    /// This used to be an inline yes/no row. It could not say that saved files now go too, and a
    /// project delete needs a count and a path; destructive scope belongs where it can be read
    /// before acting (§155).
    confirming_delete: Option<DeleteTarget>,
    /// A provider the researcher has clicked but not yet confirmed — see [`Workbench::provider_modal`].
    confirming_provider: Option<&'static settings::Provider>,
    /// What each provider last said it offers — see [`catalogue`]. Read from disk at launch and
    /// replaced in place when a refresh lands, so the picker never blocks on the network.
    catalogue: catalogue::Catalogue,
    /// Whose key the API-key field is about to set.
    ///
    /// **Separate from the coordinator's provider on purpose.** A specialist may run on a second
    /// provider — the request path has always sent a key per provider (`extra_keys`) — but the
    /// field wrote to whichever provider was *selected*, so filing an Anthropic key meant
    /// switching to Anthropic, pasting, saving, and switching back, with §186's confirmation
    /// interrupting each hop. Asked after meeting exactly that: *"do I have the ability to select
    /// the models for the subagents using independent API keys?"* (docs §191).
    key_target: String,
    /// Whether the whole reference list is open over the workbench.
    sources_open: bool,
    /// The datasets modal, which the Outputs heading opens.
    datasets_open: bool,
    /// The library modal, which the Outputs heading opens.
    documents_open: bool,
    /// Files chosen or dropped, waiting to go with the next question.
    attachments: Vec<Attachment>,
    /// Attachments sent before this conversation had a folder, to copy in once it does.
    pending_adoption: Vec<std::path::PathBuf>,
    /// Whether this launch has already collected runs that finished unattended (§243).
    swept: bool,
    /// Runs collected this launch, still to be told about. Cleared when the researcher opens one
    /// or dismisses the banner.
    collected_runs: Vec<(String, protocol::Job)>,
    /// Whether this window is the one the researcher is looking at.
    ///
    /// Read by `notify_if_away`, and the whole reason a toast is not noise: the banner (§244) and
    /// the jobs row already speak to somebody with the window open. Starts `true`, because a window
    /// that has just been opened is the one in front of them.
    window_active: bool,
    /// A finished discovery run being read. See [`DiscoveryView`].
    discovery_open: Option<DiscoveryView>,
    /// A discovery run whose budget is waiting on the researcher. See [`Approval`].
    ///
    /// The one modal in this app that guards money, which is why it is a modal at all: §244 argued
    /// a banner beats a modal for something already true, and this is the opposite — nothing
    /// proceeds until it is answered and the wrong answer cannot be taken back.
    approving: Option<Approval>,
    /// Runs the researcher declined, so rejecting one does not reopen it on the next snapshot.
    ///
    /// Kept in memory only. The artifact still says `awaiting_approval`, because that is the
    /// truth — the run is drafted and unspent — and a rejection is "not now", not a deletion.
    declined: std::collections::HashSet<String>,
    /// The intent being edited in the approval modal. The one descriptive field worth changing at
    /// the gate, because it is what the run spends its experiments on.
    intent_field: Entity<Composer>,
    /// Whether `BACKGROUND JOBS` is unfolded. Starts open, and reopens by itself the moment a
    /// worker stops at the approval gate — the researcher's press is respected everywhere except
    /// where it would hide a question addressed to them (§245).
    jobs_expanded: bool,
    /// Where the jobs list is scrolled to, so the offset survives the rebuild every stream event
    /// causes. Without a handle of its own the list would jump back to the top on each tick.
    jobs_scroll: gpui::ScrollHandle,
    /// Narrows the open reference list. Only the modal reads it — the panel's four are a
    /// preview, and filtering something that shows four of seventeen would be a filter whose
    /// result you cannot see (§197).
    sources_filter: Entity<Composer>,
    datasets_filter: Entity<Composer>,
    documents_filter: Entity<Composer>,
    /// What the files API said about each dataset, by `persistent_id`.
    ///
    /// Absent means *not asked yet*, and the same three-state distinction `repaired` needs
    /// applies: a dataset still being checked must not render the message meant for one that came
    /// back restricted. `Err` carries the reason so a row can say why it has no button rather
    /// than quietly having none.
    dataset_access: HashMap<String, Result<dataverse::Access, String>>,
    /// Datasets whose access is in flight, so the check is not started twice.
    checking_access: HashSet<String>,
    /// Downloads in flight, by `persistent_id`.
    downloading: HashSet<String>,
    /// Where a finished download landed, so the row can say so instead of offering again.
    downloaded: HashMap<String, String>,
    /// The confirmed target awaiting its backend and filesystem results.
    ///
    /// Optimistically removing the row made a failed or interrupted request look successful
    /// until restart, when the durable conversation — and therefore its project — returned
    /// (§154). Keep it visible as pending until the server has actually deleted it.
    deleting: Option<DeleteTarget>,
    /// The field that edits it. One shared editor rather than one per row — only one
    /// name can be edited at a time, and a Composer per conversation would be an entity
    /// per row for a list that can run to hundreds.
    rename_editor: Entity<Composer>,
    /// The field that edits the mission, live in the panel.
    mission_editor: Entity<Composer>,
    /// Background tasks whose remaining commands are pre-approved, by task id.
    ///
    /// Separate from the turn grant because a background worker has no turn to belong to:
    /// it runs on its own thread for minutes, and its gate would otherwise need an answer
    /// per command from someone who has gone back to work.
    approve_tasks: std::collections::HashSet<String>,
    /// Set when focus must return to the composer but no `Window` is at hand — an
    /// entity subscription doesn't get one. `render` does, so it settles the debt
    /// there. Without this, activating a command with Enter would leave focus on a
    /// field that is no longer rendered and typing would go nowhere.
    restore_focus: bool,
}

impl Workbench {
    fn new(sidecar: Arc<Sidecar>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Opens empty. The placeholder says what to do, which is all a first launch needs.
        let composer = cx.new(|cx| {
            let mut composer = Composer::new(
                cx,
                "Ask Mini-Me…  (Enter to send, Shift-Enter for a new line)",
            );
            // A research question is a paragraph, so this field is genuinely several rows and needs
            // arrow-up and arrow-down to walk them (§204).
            composer.set_multiline(true);
            composer
        });
        // The composer only reports *that* text was submitted; deciding it means
        // "run a coordinator turn" stays here.
        cx.subscribe(&composer, |workbench, _composer, event, cx| match event {
            ComposerEvent::Submit(text) => workbench.submitted(text.clone(), cx),
        })
        .detach();
        // Observed as well as subscribed: the `/name` picker filters on every keystroke, and
        // without this the list would only refresh on the next unrelated render.
        cx.observe(&composer, |workbench, _composer, cx| {
            workbench.subagent_selected = 0;
            cx.notify();
        })
        .detach();

        // Filtering installed themes, as you type — this one is local, so every keystroke
        // is free.
        let theme_filter = cx.new(|cx| Composer::new(cx, "Filter themes"));
        let model_filter = cx.new(|cx| Composer::new(cx, "Filter models"));
        let documents_filter = cx.new(|cx| Composer::new(cx, "Filter by title, tag or summary"));
        cx.observe(&documents_filter, |_workbench, _field, cx| cx.notify())
            .detach();
        let datasets_filter = cx.new(|cx| Composer::new(cx, "Filter by title, author or DOI"));
        cx.observe(&datasets_filter, |_workbench, _field, cx| cx.notify())
            .detach();
        let sources_filter = cx.new(|cx| Composer::new(cx, "Filter by author, title or year"));
        let intent_field = cx.new(|cx| {
            Composer::new(cx, "What should the search focus on? (not the answer you expect)")
        });
        cx.observe(&intent_field, |_workbench, _field, cx| cx.notify())
            .detach();

        // Told rather than sampled. Reading `is_window_active()` inside `render` would work and
        // would tie a fact about the OS to how often we happen to draw; this fires exactly when it
        // changes, which is what a notification decision needs.
        cx.observe_window_activation(window, |workbench, window, _cx| {
            workbench.window_active = window.is_window_active();
        })
        .detach();
        cx.observe(&sources_filter, |_workbench, _field, cx| cx.notify())
            .detach();
        cx.observe(&model_filter, |_workbench, _field, cx| cx.notify())
            .detach();
        let project_query = cx.new(|cx| Composer::new(cx, "Find or name a project"));
        cx.observe(&project_query, |_workbench, _field, cx| cx.notify())
            .detach();
        cx.observe(&theme_filter, |_workbench, _field, cx| cx.notify())
            .detach();

        // Searching Zed's theme gallery. Submits rather than filtering as you type: each
        // keystroke would be an HTTP request to somebody else's server.
        let gallery_query = cx.new(|cx| Composer::new(cx, "Search Zed's theme gallery"));
        cx.subscribe(&gallery_query, |workbench, _query, event, cx| match event {
            ComposerEvent::Submit(text) => workbench.search_gallery(text.clone(), cx),
        })
        .detach();

        // Filtering the conversation list. Never submits — it filters as you type, which
        // is what "fast" means for a list this small.
        let conversation_query = cx.new(|cx| Composer::new(cx, "Search conversations"));
        cx.observe(&conversation_query, |_workbench, _query, cx| cx.notify())
            .detach();

        // Renaming a conversation. Submit commits the new name; the sidebar row is
        // replaced by this field while it is in force.
        let rename_editor = cx.new(|cx| Composer::new(cx, "Name this conversation"));
        cx.subscribe(
            &rename_editor,
            |workbench, _editor, event, cx| match event {
                ComposerEvent::Submit(text) => workbench.commit_rename(text.clone(), cx),
            },
        )
        .detach();

        // Editing the mission, in the panel where it is read. Same shape as renaming, and for the
        // same reason: the field replaces the text it is editing rather than opening somewhere
        // else, so the researcher is looking at the thing they are changing.
        let mission_editor = cx.new(|cx| {
            let mut editor = Composer::new(cx, "What is this project trying to find out?");
            // Capped at 500 characters by the backend, which is still four or five rows.
            editor.set_multiline(true);
            // Enter on an empty field means "clear the mission", which the backend accepts and
            // which is the only way back to the derived one. Without this, emptying the field and
            // pressing Enter would do nothing and look broken.
            editor.set_submits_empty(true);
            editor
        });
        cx.subscribe(
            &mission_editor,
            |workbench, _editor, event, cx| match event {
                ComposerEvent::Submit(text) => workbench.commit_mission(text.clone(), cx),
            },
        )
        .detach();

        let palette_query = cx.new(|cx| {
            let mut query = Composer::new(cx, "Type a command…");
            // Enter in the palette means "run the highlighted command", which has to
            // work before anything is typed.
            query.set_submits_empty(true);
            query
        });
        cx.subscribe(&palette_query, |workbench, _query, event, cx| match event {
            ComposerEvent::Submit(_) => workbench.activate_palette(cx),
        })
        .detach();
        // Re-filter as the user types: editing the query notifies the *query*, and
        // without observing it the list would only refresh on the next unrelated
        // render.
        cx.observe(&palette_query, |workbench, _query, cx| {
            workbench.palette_selected = 0;
            cx.notify();
        })
        .detach();

        // One field per input. Created up front so each keeps its own focus and
        // selection state for the life of the window.
        let fields: Vec<(Field, Entity<Composer>)> = Field::ALL
            .into_iter()
            .map(|field| {
                let composer = cx.new(|cx| {
                    let mut composer = Composer::new(cx, field.placeholder());
                    composer.set_masked(field.is_secret());
                    composer
                });
                (field, composer)
            })
            .collect();

        // Read once. It used to be read twice in this literal and the panel states would have
        // made it three — one file, opened three times, in a constructor.
        let stored = settings::Settings::load();

        // Which folder this executable sits in, decided once. `current_exe` failing is not a
        // reason to fail the launch — it just means no update is ever offered, which is the safe
        // direction for a decision about replacing files.
        let install = std::env::current_exe()
            .as_deref()
            .map_or(update::Layout::Source, update::layout);
        tracing::info!(install = ?install, "the shape of this install");

        let mut workbench = Self {
            update: None,
            taking: None,
            update_dismissed: false,
            commands_open: false,
            claims_open: false,
            delete_interrupts_work: false,
            collecting: None,
            collect_in_flight: false,
            stray: Vec::new(),
            recovery_on: None,
            install,
            project: None,
            buckets: Vec::new(),
            jobs: Vec::new(),
            tasks: Vec::new(),
            transcript: Vec::new(),
            saved_reports: std::collections::HashSet::new(),
            reports: Vec::new(),
            sources: Vec::new(),
            datasets: Vec::new(),
            recommended_datasets: Vec::new(),
            recommended_ids: Vec::new(),
            dataset_picks: std::collections::HashSet::new(),
            search_totals: workspace::SearchTotals::default(),
            documents: Vec::new(),
            checked: HashMap::new(),
            repaired: HashMap::new(),
            resolving: 0,
            about_open: false,
            provenance_open: false,
            provenance_view: ProvenanceView::Timeline,
            provenance_turn: None,
            provenance: provenance::Record::default(),
            sidecar,
            status: "idle — type a prompt and press Enter".to_string(),
            streaming: false,
            error: None,
            composer,
            palette_open: false,
            palette_selected: 0,
            palette_query,
            settings_open: false,
            draft: stored.clone(),
            fields,
            settings_note: String::new(),
            settings_section: Section::default(),
            report: None,
            checking: false,
            judge_after_recheck: false,
            running_fix: None,
            pending_approval: None,
            approve_rest_of_turn: false,
            approve_conversation: false,
            theme_filter,
            model_filter,
            project_query,
            theme_scroll: gpui::ScrollHandle::new(),
            model_scroll: gpui::ScrollHandle::new(),
            gallery_query,
            gallery_results: Vec::new(),
            gallery_note: String::new(),
            transcript_list: ListState::new(0, ListAlignment::Top, px(240.)),
            text_selection: selection::Transcript::default(),
            context_menu: None,
            folder_projects: Vec::new(),
            opening: false,
            warming: false,
            sidebar_menu: None,
            sidebar_view: SidebarView::default(),
            subagent_selected: 0,
            open_picker: None,
            settings_focus: cx.focus_handle(),
            provenance_focus: cx.focus_handle(),
            about_focus: cx.focus_handle(),
            delete_focus: cx.focus_handle(),
            sidebar_width: 240.,
            panel_width: 320.,
            dragging: None,
            toasts: Vec::new(),
            panel_scroll: gpui::ScrollHandle::new(),
            output_gallery_scrolls: std::cell::RefCell::new(HashMap::new()),
            gallery_scroll_drag: None,
            applied_theme: stored.theme.clone(),
            sidebar_open: stored.sidebar_open,
            panel_open: stored.panel_open,
            road_open: stored.road_open,
            shapes: std::cell::RefCell::new(HashMap::new()),
            previews: std::cell::RefCell::new(HashMap::new()),
            conversation_query,
            preview: None,
            conversations: Vec::new(),
            conversations_loaded: false,
            backend_start: None,
            pending_title: None,
            renaming: None,
            editing_mission: false,
            plan: Vec::new(),
            authorship: std::collections::HashMap::new(),
            authorship_stamp: None,
            confirming_delete: None,
            confirming_provider: None,
            catalogue: catalogue::load(),
            key_target: stored.provider.clone(),
            sources_open: false,
            datasets_open: false,
            documents_open: false,
            attachments: Vec::new(),
            pending_adoption: Vec::new(),
            swept: false,
            collected_runs: Vec::new(),
            window_active: true,
            discovery_open: None,
            approving: None,
            declined: std::collections::HashSet::new(),
            intent_field,
            jobs_expanded: true,
            jobs_scroll: gpui::ScrollHandle::new(),
            sources_filter,
            datasets_filter,
            documents_filter,
            dataset_access: HashMap::new(),
            checking_access: HashSet::new(),
            downloading: HashSet::new(),
            downloaded: HashMap::new(),
            deleting: None,
            rename_editor,
            mission_editor,
            approve_tasks: std::collections::HashSet::new(),
            restore_focus: false,
        };
        // Fill the editable fields from what is stored, and open Settings on a fresh
        // install instead of letting the first turn fail against a backend with no key.
        let draft = workbench.draft.clone();
        for (field, composer) in &workbench.fields {
            let value = match field {
                Field::ModelId => draft.model_id.clone(),
                Field::BaseUrl => draft.base_url.clone(),
                Field::Port => draft.backend_port.to_string(),
                // Secrets are never read back out of the keychain into the UI.
                _ => continue,
            };
            composer.update(cx, |composer, cx| composer.set_text(value, cx));
        }
        let has_key = settings::secret(&draft.key_name()).is_some();
        if !draft.problems(has_key).is_empty() {
            workbench.settings_open = true;
            workbench.settings_note =
                "Add a model key to get started — it goes into your OS keychain.".to_string();
        }

        // Check the machine on every launch, and let the *first* report decide which pane
        // the user lands on. A missing key is a Settings problem; a missing WSL or backend
        // is a Setup problem, and Setup has to win — pasting a key into an app that cannot
        // start its backend fixes nothing, and the first thing a new user sees should be
        // the thing actually standing in their way.
        workbench.run_preflight(cx);

        // A launch opens at the workspace root. The previous project used to be a second project
        // registry in settings.toml, despite §106 defining a project solely by the conversations
        // filed under it. That stale value resurrected an empty project and silently filed the
        // morning's first conversation inside it (§154). Opening a saved conversation or using a
        // project heading's `+` remains the explicit way to enter one.
        workbench.sidecar.set_project(None);
        // Populate the spine if a backend is already listening. This does not
        // start one — see `Sidecar::fetch_project`.
        workbench.refresh_project(cx);
        // Start the backend now and list the history once it answers. Both matter for the
        // first ten seconds: the sidebar was empty until the researcher had already asked
        // something, which made the app look as if it had never been used (docs §50).
        workbench.warm_up(cx);
        workbench
    }

    /// Start watching a long job the turn left running, if it isn't already watched.
    ///
    /// A `values` snapshot repeats every artifact it knows about on every frame, so this
    /// is called many times for the same job — keyed on the task id, and already-finished
    /// jobs are recorded without spawning a poller that would have nothing to wait for.
    fn track_job(&mut self, job: protocol::Job, cx: &mut Context<Self>) {
        if let Some(existing) = self.jobs.iter_mut().find(|k| k.task_id == job.task_id) {
            // Trust the snapshot for a status we don't have yet, but never let it walk a
            // finished job back to running.
            if !existing.is_finished() {
                existing.status = job.status;
            }
            return;
        }
        let finished = job.is_finished();
        self.jobs.push(job.clone());
        if finished {
            return;
        }

        self.status = format!(
            "{} running in the background ({}) — you can keep working",
            job.kind.label(),
            job.kind.expected(job.size)
        );
        let mut updates = self.sidecar.watch_job(job);
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                let carry_on = this.update(cx, |workbench, cx| {
                    let finished = update.is_finished();
                    let label = update.kind.label();
                    let succeeded = update.succeeded();
                    if let Some(tracked) = workbench
                        .jobs
                        .iter_mut()
                        .find(|k| k.task_id == update.task_id)
                    {
                        tracked.status = update.status.clone();
                    }
                    if finished {
                        workbench.status = if succeeded {
                            format!("{label} finished — its results are in the sandbox")
                        } else {
                            format!("{label} ended: {}", update.status)
                        };
                        // **Write the ending down, not just the beginning.** §258 recorded
                        // `running` at approval — correctly, it was — and nothing ever recorded
                        // the end. So every launch after read `running` from the artifact, drew
                        // `running · usually 25–40 min` for a run that had finished hours before,
                        // and corrected it on the first poll: the same flicker, forever (§261).
                        if update.kind == protocol::JobKind::Discovery {
                            workbench.record_discovery_status(
                                update.task_id.clone(),
                                &update.status,
                                cx,
                            );
                        }
                        // The one message that can reach somebody who left. A forty-minute run
                        // that ends while they are in Excel is exactly what the roadmap's
                        // notification item was for.
                        workbench.notify_if_away(
                            &if succeeded {
                                format!("{label} finished")
                            } else {
                                format!("{label} stopped")
                            },
                            &if update.question.is_empty() {
                                "Its results are in its conversation.".to_string()
                            } else {
                                update.question.clone()
                            },
                        );
                        // The route wrote the outcome into the sandbox as it reported it,
                        // so the spine and outputs have something new to show.
                        workbench.refresh_project(cx);
                    }
                    cx.notify();
                });
                if carry_on.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Start watching a background worker's thread, if it isn't already watched.
    ///
    /// `owner` is the conversation whose snapshot carried this task. Passed in rather than looked
    /// up, because the two call sites are the only places that know it for certain and the answer
    /// has to survive the researcher moving on to another conversation (docs §159). Stamped
    /// *before* the watcher is armed, so the poll — which mutates only status, pending, error and
    /// activity — carries it for the task's whole life.
    fn track_task(&mut self, owner: &str, mut task: protocol::AsyncTask, cx: &mut Context<Self>) {
        task.owner = owner.to_string();
        if let Some(existing) = self.tasks.iter_mut().find(|t| t.task_id == task.task_id) {
            // The snapshot knows the status the coordinator last recorded; the *watcher*
            // knows whether it is stopped at the gate right now. Never let a stale
            // snapshot erase a pending approval the user is looking at.
            if existing.pending.is_none() && !existing.is_finished() {
                existing.status = task.status;
            }
            // A task already being watched keeps the owner it was first seen with: re-stamping
            // would reintroduce the drift this argument exists to prevent, on any later snapshot
            // that arrives from somewhere else.
            if existing.owner.is_empty() {
                existing.owner = task.owner;
            }
            return;
        }
        let finished = task.is_finished();
        self.tasks.push(task.clone());
        if finished {
            return;
        }

        let mut updates = self.sidecar.watch_task(task);
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                let carry_on = this.update(cx, |workbench, cx| {
                    let finished = update.is_finished();
                    let waiting = update.needs_approval();
                    let succeeded = update.succeeded();
                    let task_id = update.task_id.clone();
                    // Read before `update` is moved into the tracked slot below.
                    let worker = update.agent_name.replace('_', " ");
                    if let Some(tracked) = workbench
                        .tasks
                        .iter_mut()
                        .find(|t| t.task_id == update.task_id)
                    {
                        *tracked = update;
                    }
                    // Already granted, for this task or for the whole conversation:
                    // answer it rather than asking again. The command still lands in the
                    // card, so what ran stays reviewable — this removes the interruption,
                    // not the record.
                    if waiting
                        && (workbench.approve_conversation
                            || workbench.approve_tasks.contains(&task_id))
                    {
                        workbench.decide_task(task_id, true, cx);
                        cx.notify();
                        return;
                    }
                    if waiting {
                        workbench.status = "a background task is waiting for your approval".into();
                        // **More deserving of a toast than a finished run.** This one is stopped
                        // and cannot continue: §31 is the record of such a task hanging with
                        // nothing on screen able to answer it, and a researcher who stepped away
                        // has no way to learn it is their turn.
                        workbench.notify_if_away(
                            "A background task needs your approval",
                            &worker,
                        );
                        // Unfold the panel section that holds the Approve button. The fold is the
                        // researcher's to set, but a folded section is the one state in which a
                        // question addressed to them is invisible — so the gate appearing opens
                        // it, and a press after that is respected (§245).
                        workbench.jobs_expanded = true;
                    } else if finished {
                        workbench.status = if succeeded {
                            "a background task finished".into()
                        } else {
                            "a background task stopped".into()
                        };
                        workbench.notify_if_away(
                            if succeeded {
                                "A background task finished"
                            } else {
                                "A background task stopped"
                            },
                            &worker,
                        );
                        workbench.collect_plots();
                        workbench.settle_outputs(cx);
                        workbench.refresh_project(cx);
                    }
                    cx.notify();
                });
                if carry_on.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Answer a background worker's approval request.
    fn decide_task(&mut self, task_id: String, approve: bool, cx: &mut Context<Self>) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) else {
            return;
        };
        let Some(request) = task.pending.take() else {
            return;
        };
        // One answer per held action, in order — the agent validates the count — and each carries
        // the id of the interrupt that raised it, which is what lets several specialists be
        // answered at once (§215).
        let answers = protocol::Answer::all(&request, decision_for(approve));
        let thread_id = task.thread_id.clone();
        // **The task's own owner, not the conversation on screen.** Answering an approval is the
        // moment a background worker is told where to write, and it happens whenever the
        // researcher gets to it — by then they may have pressed New thread or opened something
        // else. Sending the open conversation put a worker's figures into a conversation that
        // never asked for them (docs §159). Unknown stays unknown: `None` sends no key, and the
        // backend falls back to the sibling folder it used before, which is at least visible.
        let owner = task.owning_conversation().map(str::to_string);
        if owner.is_none() {
            tracing::warn!(
                task = %task_id,
                worker = %thread_id,
                "answering a background task whose owning conversation was never recorded — \
                 its files may land beside the conversation instead of inside it"
            );
        }
        task.status = "running".into();
        self.sidecar.decide_task(thread_id, owner, answers);
        self.status = if approve {
            "background task approved — running…"
        } else {
            "background task rejected"
        }
        .into();
        cx.notify();
    }

    /// Open the platform's file chooser, and add whatever comes back.
    ///
    /// **The affordance dragging never had.** §28 accepted a drop anywhere on the window and
    /// then said so in one line of the empty state — which vanishes the moment a conversation
    /// has anything in it, leaving the feature invisible for the whole rest of the session.
    /// Dragging is also the harder gesture on the platform this app is for: it needs Explorer
    /// and a *not*-maximised window side by side, which is not how anyone works.
    ///
    /// Files only. `can_select_mixed_files_and_dirs` is `false` on Windows — the
    /// `FOS_PICKFOLDERS` flag toggles the dialog between the two rather than widening it — so
    /// asking for both would silently give a folder picker to someone looking for a CSV.
    /// Dragging still accepts a folder, which is the gesture that suits one anyway.
    fn choose_files(&mut self, cx: &mut Context<Self>) {
        if self.streaming {
            self.status = "finish this turn before adding files".into();
            cx.notify();
            return;
        }
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            // Three outcomes worth telling apart: paths, a cancel, and a picker that would
            // not open. Only the last is a problem, and it is the one a silent `_ => {}`
            // would turn into a button that does nothing.
            match chosen.await {
                Ok(Ok(Some(paths))) => {
                    let _ = this.update(cx, |workbench, cx| workbench.add_files(&paths, cx));
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    let _ = this.update(cx, |workbench, cx| {
                        workbench.error = Some(format!("could not open the file chooser: {error}"));
                        cx.notify();
                    });
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    /// Put files into the question being written — dropped on the window, or chosen.
    ///
    /// The one thing the web app cannot do: the researcher's data is already on this
    /// machine, and this is the whole distance between "here is my CSV" and an analysis —
    /// no upload, no copy, no bucket.
    fn add_files(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        if self.streaming {
            self.status = "finish this turn before adding files".into();
            cx.notify();
            return;
        }
        // Checked before anything is written into the composer, because the alternative is a
        // turn that runs for a minute and then reports a missing file. A share the agent
        // cannot reach is worth one sentence now rather than a puzzle later (§179).
        let (usable, unreachable): (Vec<_>, Vec<_>) = paths
            .iter()
            .partition(|path| self.sidecar.can_open(path.as_path()));
        // Named, never counted: "1 of 3 added" leaves the researcher hunting for which one,
        // and which one is the only actionable part of the sentence.
        let skipped = unreachable
            .iter()
            .map(|path| file_label(path))
            .collect::<Vec<_>>()
            .join(", ");
        if usable.is_empty() {
            self.error = Some(format!(
                "{skipped} is on a network share the agent cannot open — copy it to this \
                 computer first"
            ));
            cx.notify();
            return;
        }

        // **Copied into the conversation first.** An attachment used to be referenced where it
        // lay, so `pdf_librarian` indexed `…/Downloads/Graph-neural-networks.pdf` and the claims
        // recorder said so (§227). Downloads is a folder people empty; a conversation reopened
        // next month would hold a library index, a citation and an analysis all naming a path
        // that resolves to nothing. Copying makes the input part of the conversation the same way
        // its outputs are.
        //
        // Falls back to referencing in place — with the reason said out loud — rather than
        // refusing. A researcher who dropped a file wants to ask about it, and an attachment that
        // does not persist is far better than one that does not arrive.
        let folder = self.thread_workspace();
        let mut adopted: Vec<std::path::PathBuf> = Vec::new();
        let mut left_where_they_are: Vec<String> = Vec::new();
        for path in &usable {
            let Some(folder) = folder.as_ref() else {
                adopted.push(path.to_path_buf());
                continue;
            };
            let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if size > workspace::ADOPT_LIMIT {
                left_where_they_are.push(file_label(path));
                adopted.push(path.to_path_buf());
                continue;
            }
            match workspace::adopt(folder, path) {
                Ok(copy) => adopted.push(copy),
                Err(error) => {
                    tracing::warn!(%error, "could not copy an attachment in");
                    left_where_they_are.push(file_label(path));
                    adopted.push(path.to_path_buf());
                }
            }
        }

        // **Beside the composer, not inside it.** The path used to be written into the text, which
        // meant a researcher looking at three wrapped lines of
        // `/mnt/c/Users/…/Mini-Me/01a01ae5-27e8-…/New Phytologist - 2013 - …pdf` where a filename
        // would do, and no way to drop one without editing something they had not typed (§231).
        //
        // `./name` for a file that was copied in, because the conversation's folder *is* the
        // agent's working directory and that is the form four backend prompts ask for. The
        // absolute path for one that had to stay put — it is outside the workspace, so a relative
        // reference would resolve to nothing.
        for path in &adopted {
            let label = file_label(path);
            let inside = folder
                .as_ref()
                .is_some_and(|dir| path.parent() == Some(dir.as_path()));
            let reference = if inside {
                format!("./{label}")
            } else {
                self.sidecar.path_for_backend(path)
            };
            if self
                .attachments
                .iter()
                .any(|held| held.reference == reference)
            {
                continue;
            }
            self.attachments.push(Attachment {
                label,
                source: path.to_path_buf(),
                adopted: inside,
                reference,
            });
        }
        self.restore_focus = true;
        // Says what to do next, because the composer no longer does. With the prepared
        // question gone (§180) there is a path sitting in the field and nothing asking
        // anything, and one line is cheaper than leaving a researcher to infer it.
        self.status = match usable.len() {
            1 => format!(
                "added {} — say what you want done with it",
                file_label(usable[0])
            ),
            n => format!("added {n} files — say what you want done with them"),
        };
        // Assigned rather than only set, so a second add clears the first one's warning. A
        // stale "left out yield.csv" beside a composer that no longer mentions it is worse
        // than the older, already-seen error this replaces.
        self.error = if !unreachable.is_empty() {
            Some(format!(
                "left out {skipped} — on a network share the agent cannot open"
            ))
        } else if !left_where_they_are.is_empty() {
            // Not a failure, and said anyway: the file is in the turn either way, but it will
            // not travel with the conversation, and the researcher is the only one who can
            // decide whether that matters.
            Some(format!(
                "{} stayed where it is rather than being copied in — move or delete it and this \
                 conversation loses it",
                left_where_they_are.join(", ")
            ))
        } else {
            None
        };
        cx.notify();
    }

    /// Run the first-run checks and show the result.
    ///
    /// The keychain lookup happens **here**, on the main thread, and travels as a bool —
    /// the Linux keychain client panics if called from a Tokio worker (docs §22).
    fn run_preflight(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        let has_key = settings::secret(&settings::Settings::load().key_name()).is_some();
        let mut results = self.sidecar.preflight(has_key);
        cx.spawn(async move |this, cx| {
            if let Some(report) = results.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.checking = false;
                    // Name the *first* blocker rather than a count: "2 to fix" still
                    // leaves the user opening the pane to find out what.
                    workbench.status = match report.first_problem() {
                        Some(check) => format!("setup: {} — {}", check.label, check.detail),
                        None => format!("setup: {}", report.summary()),
                    };
                    // Only the first report may open a pane by itself. After that the user
                    // has seen the state of things, and a background re-check yanking the
                    // pane open under their cursor would be rude.
                    let first = workbench.report.is_none();
                    let blocked = !report.ready();
                    workbench.report = Some(report);
                    if std::mem::take(&mut workbench.judge_after_recheck) {
                        workbench.judge_finished_fix();
                    }
                    if first && blocked {
                        // The first report is the guided first run: open the window on the
                        // page that says what is wrong.
                        workbench.settings_section = Section::Setup;
                        workbench.settings_open = true;
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Show the Setup pane, re-checking as it opens — a stale report is worse than none,
    /// because the whole point is to reflect what the machine is like *now*.
    fn open_setup(&mut self, cx: &mut Context<Self>) {
        self.settings_section = Section::Setup;
        self.settings_open = true;
        self.run_preflight(cx);
    }

    /// Point the app at a checkout the user already has.
    ///
    /// Recorded as **not owned**, which is the whole point: the app will run this backend
    /// but will never `git checkout` or re-sync it, because that would destroy work in a
    /// clone somebody else is responsible for.
    fn adopt_checkout(&mut self, dir: String, cx: &mut Context<Self>) {
        let mut settings = settings::Settings::load();
        settings.backend_dir = dir.clone();
        settings.backend_dir_owned = false;
        match settings.save() {
            Ok(()) => {
                self.draft = settings;
                // The launch command is built at startup from this path, so it cannot
                // take effect until the app restarts — say so plainly instead of leaving
                // the user to wonder why the row is still red.
                self.status = format!("using {dir} — restart the app to launch it");
                self.settings_note = format!("Backend set to {dir}. Restart to use it.");
            }
            Err(error) => self.status = format!("could not save that choice: {error:#}"),
        }
        self.run_preflight(cx);
    }



    /// Report the outcome of something the user did.
    ///
    /// Goes to the status bar *and* to a toast that lingers past the next status change. Use
    /// it for results — copied, saved, stopped, deleted — never for progress.
    fn say(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.status = text.to_string();
        self.toasts.push(text);
        // Bounded, so a burst cannot fill the window with its own history.
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        // Each toast retires on its own timer rather than all of them on a shared one, so a
        // second message does not cut the first one's time short.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(4))
                .await;
            let _ = this.update(cx, |workbench, cx| {
                if !workbench.toasts.is_empty() {
                    workbench.toasts.remove(0);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }


    /// Open or close a choice popup, remembering where its trigger was.
    ///
    /// The position comes from the click rather than from the trigger's bounds: the same thing
    /// the right-click menu does (§64), and it needs no element to measure.
    fn toggle_picker(
        &mut self,
        picker: Picker,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_picker = match self.open_picker {
            Some((open, _)) if open == picker => None,
            _ => Some((picker, at)),
        };
        cx.notify();
    }


    /// Open the right-click menu at a point, over whatever was clicked.
    fn open_context_menu(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        target: menu::Target,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(menu::ContextMenu::new(at, target));
        cx.notify();
    }

    /// Whether an item would actually do something if it were clicked.
    ///
    /// Drives the greying, and is checked again when the item runs — the two must agree, so
    /// they read the same state rather than each deciding for themselves.
    fn menu_item_enabled(&self, item: menu::Item, target: menu::Target, cx: &App) -> bool {
        match (item, target) {
            (menu::Item::Copy, menu::Target::Transcript) => {
                self.text_selection.selected_text().is_some()
            }
            (menu::Item::Copy, menu::Target::Composer)
            | (menu::Item::Cut, menu::Target::Composer) => {
                let composer = self.composer.read(cx);
                composer.has_selection() && (item == menu::Item::Copy || composer.is_editable())
            }
            // Not whether the clipboard has anything — reading it on every frame to grey a
            // row is a syscall for a cosmetic, and a paste of nothing is harmless.
            (menu::Item::Paste, _) => self.composer.read(cx).is_editable(),
            (menu::Item::SelectAll, menu::Target::Composer) => {
                !self.composer.read(cx).text().is_empty()
            }
            (menu::Item::SelectAll, menu::Target::Transcript) => !self.transcript.is_empty(),
            (menu::Item::CopyLastAnswer, _) => self
                .transcript
                .iter()
                .any(|message| message.role == "mini-me" && !message.body.is_empty()),
            // Cut in the transcript is never offered; see `ContextMenu::items`.
            (menu::Item::Cut, menu::Target::Transcript) => false,
        }
    }

    fn run_menu_item(
        &mut self,
        item: menu::Item,
        target: menu::Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        match (item, target) {
            (menu::Item::Copy, menu::Target::Transcript) => self.copy_selected_text(cx),
            (menu::Item::SelectAll, menu::Target::Transcript) => self.select_whole_transcript(cx),
            (menu::Item::CopyLastAnswer, _) => self.run_command(Command::CopyLastAnswer, cx),
            (menu::Item::Copy, menu::Target::Composer) => {
                self.composer.update(cx, |composer, cx| {
                    composer.copy_to_clipboard(cx);
                });
            }
            (menu::Item::Cut, menu::Target::Composer) => {
                self.composer.update(cx, |composer, cx| {
                    composer.cut_to_clipboard(window, cx);
                });
            }
            (menu::Item::Paste, _) => {
                self.composer.update(cx, |composer, cx| {
                    composer.paste_from_clipboard(window, cx);
                });
            }
            (menu::Item::SelectAll, menu::Target::Composer) => {
                self.composer.update(cx, |composer, cx| {
                    composer.select_all_text(cx);
                });
            }
            (menu::Item::Cut, menu::Target::Transcript) => {}
        }
        // A menu item that edited or selected the prompt should leave the caret where the
        // user can carry on typing.
        if target == menu::Target::Composer {
            self.composer.read(cx).focus_handle(cx).focus(window);
        }
        cx.notify();
    }



    /// What each row does. **Nothing new lives here** — every arm calls a method the sidebar
    /// already had, which is the rule `menu.rs` states for the right-click menu and the reason
    /// this change is a rearrangement rather than a feature with its own behaviour.
    fn run_sidebar_menu(
        &mut self,
        open: &SidebarMenu,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (open, id) {
            (SidebarMenu::New, "menu-new-conversation") => self.new_thread_in(None, cx),
            (SidebarMenu::New, "menu-new-project") => {
                // The project picker already knows how to name one that does not exist yet —
                // typing offers `New project “…”` as its first row. `NewProject` only changes
                // what choosing does: start a conversation there, rather than move the open one.
                self.open_picker = Some((Picker::NewProject, gpui::point(px(24.), px(120.))));
                self.project_query.update(cx, |query, cx| query.set_text("", cx));
                cx.notify();
            }
            (SidebarMenu::Conversation(conversation), "menu-rename") => {
                self.start_rename(conversation.thread_id.clone(), window, cx)
            }
            (SidebarMenu::Conversation(conversation), "menu-delete") => {
                self.request_delete(DeleteTarget::Conversation(conversation.clone()), window, cx)
            }
            (SidebarMenu::Project { name, .. }, "menu-new-here") => {
                self.new_thread_in(Some(name.clone()), cx)
            }
            (SidebarMenu::Project { name, .. }, "menu-open-folder") => {
                if let Some(dir) =
                    workspace::project_folder(name).map(|folder| workspace::root().join(folder))
                {
                    if let Err(error) = workspace::open(&dir) {
                        tracing::warn!(%error, "could not open a project");
                    }
                }
            }
            (
                SidebarMenu::Project {
                    name,
                    conversations,
                },
                "menu-delete-project",
            ) => self.request_delete(
                DeleteTarget::Project {
                    name: name.clone(),
                    conversations: conversations.clone(),
                },
                window,
                cx,
            ),
            _ => {}
        }
    }


    /// Copy the selected transcript text.
    ///
    /// Reached from `ctrl-c` when the composer declines it, so the ordinary shortcut works on
    /// the transcript without the user having to click somewhere first to move focus.
    fn copy_selection(&mut self, _: &CopySelection, _window: &mut Window, cx: &mut Context<Self>) {
        self.copy_selected_text(cx);
    }

    /// The work behind [`Self::copy_selection`], without the `Window` an action handler is
    /// handed — so the command palette, which has none, can run it too.
    fn copy_selected_text(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.text_selection.selected_text() else {
            // Nothing selected here either. Say nothing rather than claiming a copy — an
            // empty clipboard reported as success is worse than silence.
            return;
        };
        let lines = text.lines().count();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.say(
            format!(
                "copied {lines} line{} from the transcript",
                if lines == 1 { "" } else { "s" }
            ),
            cx,
        );
    }

    /// Select the whole transcript.
    ///
    /// `ctrl-shift-a`, not `ctrl-a`: that one belongs to the composer, where it selects the
    /// prompt being typed, and taking it would break the field people use constantly to fix
    /// what they are writing.
    fn select_all_transcript(
        &mut self,
        _: &SelectAllTranscript,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_whole_transcript(cx);
    }

    fn select_whole_transcript(&mut self, cx: &mut Context<Self>) {
        let text = self.transcript.iter().map(Message::selection_text)
            .filter(|message| !message.is_empty()).collect::<Vec<_>>().join("\n");
        self.text_selection.select_all(text);
        // Only reports a count once there is something to count: on the very first frame the
        // registry is still empty, and "selected 0 messages" would be a lie about a feature
        // rather than a fact about the conversation.
        self.status = match self.text_selection.selected_text() {
            Some(text) => format!("selected {} lines — ctrl-c to copy", text.lines().count()),
            None => "there is nothing in the transcript to select yet".into(),
        };
        cx.notify();
    }

    /// Whether the named row is still not passing, according to the latest report.
    fn still_unfixed(&self, check_id: &str) -> bool {
        self.report.as_ref().is_some_and(|report| {
            report
                .checks
                .iter()
                .any(|check| check.id == check_id && check.state != preflight::State::Pass)
        })
    }

    /// What colour the app's own remarks about a fix should be.
    ///
    /// Read from the current report rather than stored, so "restart Windows" is amber while
    /// it is still true and goes quiet by itself once the row it names turns green.
    fn fix_tone(&self, fix: &RunningFix) -> u32 {
        if !fix.done {
            theme::text_muted()
        } else if !fix.ok {
            theme::error()
        } else if self.still_unfixed(fix.check_id) {
            theme::warning()
        } else {
            theme::text_muted()
        }
    }

    /// Say so when a fix reported success and changed nothing.
    ///
    /// A machine with WSL but no distro ran `wsl --install -d Ubuntu`, which installed the
    /// WSL runtime, exited 0 — and left the distro unregistered until Windows restarts. The
    /// pane said "Install Ubuntu — done" directly above the same red row it had started
    /// from, which reads as a bug in the app rather than as a step the user still has to
    /// take (docs §60).
    ///
    /// The verdict is drawn from the re-check, never from the command's own words: `wsl.exe`
    /// speaks the system language — the machine this came from printed *"Descargando:
    /// Subsistema de Windows para Linux"* — so matching on "restart" would have failed for
    /// exactly the user who needed it.
    fn judge_finished_fix(&mut self) {
        let check_id = match &self.running_fix {
            Some(fix) if fix.ok => fix.check_id,
            _ => return,
        };
        if !self.still_unfixed(check_id) {
            return;
        }
        let Some(fix) = self.running_fix.as_mut() else {
            return;
        };
        // Only the runtime row installs WSL itself, and only Windows reboots for it.
        if check_id == "runtime" && cfg!(windows) {
            fix.notes.push(
                "— That installed WSL, but Windows has to restart before a distro can \
                 start. Restart this machine, open the app again, and this row should be \
                 green."
                    .into(),
            );
        } else {
            fix.notes.push(
                "— It reported success but the check still fails. The output above is the \
                 best clue; the sidecar log below has the rest."
                    .into(),
            );
        }
    }

    /// Run a fix on the user's behalf, streaming its output into the pane.
    ///
    /// Re-checks automatically when it finishes, so a successful install turns its own
    /// row green without the user having to work out that they should press Re-check.
    /// Stop the repair that is running, if one is.
    ///
    /// **Says `stopping`, not `stopped`.** The only honest report of a stop is the command
    /// actually exiting, which arrives as `FixEvent::Finished` on the same channel as any other
    /// ending — so this asks, marks the pane, and waits to be told. §146 refused a Stop button
    /// precisely because the version that flips a label without owning a process claims the
    /// machine stopped changing when it has not.
    ///
    /// A repair that finished between the click and this call is **stopped**, not an error:
    /// `Cancel::stop` reports there was nothing to signal, and there being nothing left to stop
    /// is the outcome the button was pressed for.
    fn stop_fix(&mut self, cx: &mut Context<Self>) {
        let Some(fix) = self.running_fix.as_mut() else {
            return;
        };
        if fix.done || fix.stopping {
            return;
        }
        fix.stopping = true;
        if fix.cancel.stop() {
            fix.notes.push("— asked to stop; waiting for the command to exit".into());
            self.status = "stopping the repair…".into();
        } else {
            // Nothing was armed: it had already exited and the Finished event is on its way.
            fix.notes.push("— it had already finished".into());
            self.status = "the repair had already finished".into();
        }
        cx.notify();
    }

    fn start_fix(
        &mut self,
        label: String,
        argv: Vec<String>,
        check_id: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.running_fix.as_ref().is_some_and(|fix| !fix.done) {
            return;
        }
        self.status = format!("running: {label}");
        let (events, cancel) = self.sidecar.run_fix(argv);
        self.running_fix = Some(RunningFix {
            label,
            link: None,
            lines: Vec::new(),
            notes: Vec::new(),
            check_id,
            done: false,
            ok: false,
            cancel: cancel.clone(),
            stopping: false,
        });
        let mut events = events;
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let update = this.update(cx, |workbench, cx| {
                    let Some(fix) = workbench.running_fix.as_mut() else {
                        return;
                    };
                    match event {
                        sidecar::FixEvent::Line(line) => {
                            // `asta auth login` prints its device-activation URL and then
                            // tries to open it with `gio`, which fails inside WSL: no
                            // browser there. Catching the URL is what lets the app open it
                            // on the host, where the browser is (docs §32c).
                            if fix.link.is_none() {
                                fix.link = first_url(&line);
                            }
                            fix.lines.push(line);
                            if fix.lines.len() > FIX_LOG_LINES {
                                fix.lines.remove(0);
                            }
                        }
                        sidecar::FixEvent::Finished { ok, note } => {
                            fix.done = true;
                            fix.ok = ok;
                            fix.notes.push(format!("— {note}"));
                            // Credentials are read into the backend's environment when it
                            // *starts*, so signing in while it runs changes nothing until
                            // it is restarted. Saying so is the difference between a fix
                            // that works and one that looks broken — signing in from this
                            // pane and then watching the same failure is exactly what
                            // happened the first time (docs §32).
                            if ok && fix.label.contains("Sign in") {
                                fix.notes.push(
                                    "— Close and reopen the app: the backend reads your \
                                     Asta sign-in when it starts."
                                        .into(),
                                );
                            }
                            workbench.status =
                                format!("{}: {note}", if ok { "done" } else { "failed" });
                            // Re-check on success so the row the user just fixed goes
                            // green by itself — and so that a fix which succeeded without
                            // fixing anything gets found out. See `judge_finished_fix`.
                            if ok {
                                workbench.judge_after_recheck = true;
                                workbench.run_preflight(cx);
                            }
                        }
                    }
                    cx.notify();
                });
                if update.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Pull the project spine in the background and swap it in when it arrives.
    fn refresh_project(&self, cx: &mut Context<Self>) {
        let mut results = self.sidecar.fetch_project();
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = results.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match outcome {
                        Ok(project) => {
                            workbench.project =
                                Some(merge_spine(workbench.project.as_ref(), project))
                        }
                        // A missing spine is not worth interrupting the user for —
                        // the panel already shows an honest placeholder.
                        Err(error) => tracing::debug!(%error, "could not load the project spine"),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Kick off one coordinator turn and pump its events into the transcript.
    fn start_turn(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.start_turn_as(prompt, subagent::Dispatch::default(), cx);
    }

    /// Ask the current provider what it offers, if nobody has asked today.
    ///
    /// **Called when the Model pane opens, not on a timer.** A background poll would spend a
    /// researcher's key on a request they cannot see, and the only moment the answer matters is
    /// the moment somebody is looking at the list. Opening the pane is that moment.
    ///
    /// Silent on failure by design. This is a nicety on top of a curated list that already works:
    /// offline, rate-limited or a gateway that does not serve `/models` all mean *"keep the list
    /// you have"*, and an error toast for a thing nobody asked for is noise.
    fn refresh_models(&mut self, cx: &mut Context<Self>) {
        let Some(spec) = settings::provider(&self.draft.provider) else {
            return;
        };
        let Some((url, auth)) = catalogue::endpoint(spec.id, &self.draft.base_url) else {
            return;
        };
        let now = provenance::now_ms();
        if !self
            .catalogue
            .get(spec.id)
            .is_none_or(|listing| listing.is_stale(now))
        {
            return;
        }
        // Read here, on the main thread — the keychain is not safe to touch from a Tokio worker.
        let key = settings::secret(&format!("llm:{}", spec.id));
        let mut done = self.sidecar.refresh_models(
            spec.id.to_string(),
            url,
            auth,
            key,
            now,
        );
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = done.next().await {
                let _ = this.update(cx, |workbench, cx| match outcome {
                    Ok((provider, count)) => {
                        // Re-read rather than patch: the file is the record, and another provider
                        // may have been written while this one was in flight.
                        workbench.catalogue = catalogue::load();
                        tracing::info!(%provider, count, "model list refreshed");
                        cx.notify();
                    }
                    Err(error) => tracing::debug!(%error, "could not refresh the model list"),
                });
            }
        })
        .detach();
    }

    /// The one thing stopping a turn from reaching the provider that was actually chosen.
    ///
    /// Read from the **saved** settings rather than from `self.draft`, because the draft is what
    /// somebody is halfway through editing and the request is built from what was saved. Only
    /// the two failures that silently redirect a turn — a missing key and a custom provider with
    /// no endpoint — because a wrong *model id* fails loudly, from the provider you picked, in a
    /// sentence that names the model.
    fn provider_blocker(&self) -> Option<String> {
        let stored = settings::Settings::load();
        let has_key = settings::secret(&stored.key_name()).is_some();
        if let Some(blocker) = stored.misdirects_a_turn(has_key) {
            return blocker.into();
        }
        // **And the specialists.** §186's gate read the coordinator alone, so an override to an
        // unkeyed provider sailed through and failed minutes later inside a worker, billed to an
        // account the researcher had never opened (§212). Refused here for the same reason and in
        // the same place: before anything is spent.
        //
        // The keychain read stays on this thread, which is why `unkeyed_specialists` takes the
        // lookup rather than doing it — `secret` is not async-safe.
        stored
            .unkeyed_specialists(|id| settings::secret(&format!("llm:{id}")).is_some())
            .into_iter()
            .next()
    }

    /// Start a turn, choosing how a `/name` command should reach its specialist.
    fn start_turn_as(
        &mut self,
        prompt: String,
        dispatch: subagent::Dispatch,
        cx: &mut Context<Self>,
    ) {
        if self.streaming || prompt.trim().is_empty() {
            return;
        }

        // **Refuse rather than fall through to somebody else's account.** `problems()` has always
        // been computed and, in `main`'s own words, *"warned, not fatal"* — logged at launch and
        // shown in the pane. A turn ran regardless, and the consequence was not a clear failure:
        // with no key for the chosen provider, `run_request_body` omits `__llm_keys` **entirely**,
        // and `base_url` lives inside that block. The backend then builds a bare OpenAI client,
        // picks up whatever `OPENAI_API_KEY` the distro happens to hold, and bills a provider
        // nobody selected. Reported as *"this is weird, I set OpenRouter and I have credits"* —
        // with an out-of-credits page for OpenAI (docs §186).
        if let Some(problem) = self.provider_blocker() {
            self.error = Some(format!("{problem} — Settings › Model"));
            self.settings_section = Section::Model;
            self.settings_open = true;
            cx.notify();
            return;
        }
        // `/name …` names a specialist. Resolved *before* anything is sent, because the failure
        // this guards against is silent: sent as prose, `/eda-subagent do the thing` is a
        // ten-minute wait for a turn that was never delegated (§55, §76).
        let prompt = match subagent::parse(&prompt) {
            None => prompt,
            Some(command) => match self.resolve_subagent(&command, dispatch, cx) {
                Some(turn) => turn,
                None => return,
            },
        };
        // **After the specialist is resolved, and after every early return.**
        //
        // Order first: `subagent::parse` needs the prompt to *begin* with `/name`, so a blockquote
        // prepended above would hide the command and send it as prose — the ten-minute
        // never-delegated turn of §55 and §76, reachable by attaching a file.
        //
        // Placement second: `provider_blocker` and `resolve_subagent` both refuse and return, and
        // taking the list before them would drop a researcher's attachments on a turn that never
        // ran. They are cleared here, where the turn is certain to go.
        let prompt = with_attachments(&prompt, &self.attachments);
        // **A file attached before the conversation existed is copied in afterwards.**
        // `thread_workspace()` is `None` until the backend assigns a thread id on the first turn,
        // so "new conversation, attach, ask" — the ordinary flow, and the one §228 was written for
        // — silently skipped the copy. The turn that follows carries the absolute path, which the
        // agent reads perfectly well; what was missing is the file being *kept* (docs §236).
        self.pending_adoption
            .extend(awaiting_adoption(&self.attachments));
        self.attachments.clear();
        self.streaming = true;
        self.error = None;
        self.status = "starting…".into();
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(true, cx));
        // Name the conversation after the first thing asked. A sidebar of "New
        // conversation" is a sidebar of nothing, and every chat app auto-titles for
        // exactly this reason; the researcher can rename it whenever they like.
        let first_turn = self.transcript.is_empty();
        // Open a row in the provenance record for this question. The prompt sent is what is
        // recorded, not what was typed — for a `/name` command those differ, and what reached the
        // coordinator is what the work responded to.
        self.provenance
            .begin_turn(prompt.clone(), provenance::now_ms());
        self.transcript.push(Message::new("you", prompt.clone()));
        // The assistant message — text *and* activity — streams into this entry.
        self.transcript.push(Message::new("mini-me", String::new()));
        if first_turn {
            self.pending_title = Some(protocol::title_from_prompt(&prompt));
        }

        let mut events = self.sidecar.submit(prompt);
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                // `Err` here means the view is gone (window closed) — stop pumping.
                if this
                    .update(cx, |workbench, cx| {
                        workbench.apply(event, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    /// Stop the backend and start it again.
    ///
    /// The app *attaches* to a healthy backend rather than replacing it, which is right for
    /// speed and wrong after an update: the Python overlay lives in that process's memory, so a
    /// newly-pulled app kept talking to a server holding the previous one — with no symptom
    /// except a feature that did nothing (docs §79).
    fn restart_backend(&mut self, cx: &mut Context<Self>) {
        if self.streaming {
            self.say("can't restart the backend mid-turn", cx);
            return;
        }
        self.status = "restarting the backend…".into();
        let mut done = self.sidecar.restart_backend();
        cx.spawn(async move |this, cx| {
            let outcome = done.next().await;
            let _ = this.update(cx, |workbench, cx| {
                match outcome {
                    Some(Ok(status)) => {
                        workbench.backend_start = Some(status);
                        workbench.say("backend restarted", cx)
                    }
                    Some(Err(error)) => workbench.say(format!("restart failed: {error:#}"), cx),
                    None => workbench.say("restart reported nothing back", cx),
                }
                // Everything read from the backend is now a fresh process's answer, including
                // the specialist list the overlay writes as it assembles a coordinator.
                workbench.conversations_loaded = false;
                workbench.refresh_conversations(cx);
                workbench.run_preflight(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Stop the turn in flight.
    ///
    /// Closes the turn here *and* asks the backend to abandon the run. Only aborting our own
    /// stream would leave the graph running with nobody reading it — `on_disconnect` defaults
    /// to `continue` — which for an agent that spends tokens per step is the expensive kind
    /// of silence (docs §63).
    ///
    /// Whatever already streamed in stays. It is real work, the reader may well want it, and
    /// deleting the thing they were reading is not what stop means anywhere else.
    fn stop_turn(&mut self, cx: &mut Context<Self>) {
        let told_backend = self.sidecar.cancel_turn();
        self.streaming = false;
        self.pending_approval = None;
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(false, cx));
        if let Some(message) = self.transcript.last_mut() {
            if message.role == "mini-me" {
                message.stopped = true;
                // The steps are why it was still going; leave them open on a stopped turn so
                // the reason is on screen rather than one click away.
                message.steps_expanded = true;
            }
        }
        if let Some(last) = self.transcript.len().checked_sub(1) {
            self.invalidate_transcript_message(last);
        }
        // Said differently in the two cases because they are different: one stopped the run,
        // the other only stopped us watching it.
        let outcome = if told_backend {
            "turn stopped"
        } else {
            "stopped watching — the run had not reported an id yet, so the backend may still \
             be finishing it"
        };
        self.say(outcome, cx);
    }

    /// Enter, in the composer.
    ///
    /// While a name is still being typed, Enter **completes** rather than sends — the way
    /// completion works in a shell, and the reason two Enters is the natural rhythm here: one to
    /// settle the specialist, one to send the request. It cannot send by accident, because a
    /// half-typed name is never a real one.
    fn submitted(&mut self, text: String, cx: &mut Context<Self>) {
        if subagent::completing(&text) {
            let agents = workspace::subagents();
            let query = subagent::parse(&text).map(|c| c.name).unwrap_or_default();
            let matched = subagent::ranked(&query, &agents);
            if let Some(chosen) =
                matched.get(self.subagent_selected.min(matched.len().saturating_sub(1)))
            {
                self.choose_subagent(&chosen.name, cx);
                return;
            }
            // Nothing matched. Fall through, so `start_turn` refuses by name and suggests —
            // silence here would look like a key that does nothing.
        }
        self.start_turn(text, cx);
    }

    /// Put a chosen name in the composer, ready for the request.
    ///
    /// The trailing space is the point: it closes the picker and puts the caret where the
    /// sentence continues.
    fn choose_subagent(&mut self, name: &str, cx: &mut Context<Self>) {
        let filled = format!("/{name} ");
        self.composer
            .update(cx, |composer, cx| composer.set_text(filled, cx));
        self.subagent_selected = 0;
        cx.notify();
    }

    /// Copy in the files that were attached before this conversation had a folder.
    ///
    /// Runs once the turn has finished, which is the first moment `thread_workspace()` can answer.
    /// Failures are logged and dropped: the turn already ran and the agent already read the file
    /// from where it was, so nothing here is worth interrupting a researcher over.
    fn adopt_pending(&mut self, cx: &mut Context<Self>) {
        if self.pending_adoption.is_empty() {
            return;
        }
        let Some(folder) = self.thread_workspace() else {
            return;
        };
        let mut kept = 0usize;
        for source in std::mem::take(&mut self.pending_adoption) {
            let size = std::fs::metadata(&source).map(|meta| meta.len()).unwrap_or(0);
            if size > workspace::ADOPT_LIMIT {
                continue;
            }
            match workspace::adopt(&folder, &source) {
                Ok(_) => kept += 1,
                Err(error) => tracing::warn!(%error, "could not copy an attachment in later"),
            }
        }
        if kept > 0 {
            tracing::info!(kept, "copied attachments into the conversation");
            // So Outputs shows them beside everything else the turn produced.
            self.refresh_project(cx);
        }
    }




    /// Turn a `/name …` into the turn to send, or refuse and say why.
    ///
    /// Every rejection leaves the prompt where it was, so nothing typed is lost to a typo.
    fn resolve_subagent(
        &mut self,
        command: &subagent::Command,
        dispatch: subagent::Dispatch,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let agents = workspace::subagents();
        if agents.is_empty() {
            // The registry is written when the backend assembles a coordinator, so before the
            // first turn there is genuinely nothing to check against. Saying that is better than
            // rejecting a name that may well be correct.
            self.say(
                "no specialist list yet — ask one ordinary question first, then /name works",
                cx,
            );
            return None;
        }
        if !subagent::known(&command.name, &agents) {
            // Name the nearest thing rather than only the mistake: the names the request
            // imagined are not the ones the backend uses, so "did you mean" is the useful half.
            let nearest = subagent::ranked(&command.name, &agents)
                .first()
                .map(|agent| format!(" — did you mean /{}?", agent.name))
                .unwrap_or_default();
            self.say(
                format!("no specialist called \"{}\"{nearest}", command.name),
                cx,
            );
            return None;
        }
        if command.prompt.trim().is_empty() {
            self.say(format!("say what {} should do", command.name), cx);
            return None;
        }
        Some(subagent::turn(&command.name, &command.prompt, dispatch))
    }

    fn apply(&mut self, event: TurnEvent, cx: &mut Context<Self>) {
        match event {
            TurnEvent::Status(status) => self.status = status,
            // Recorded by the sidecar as it passes; nothing here needs it, and putting a
            // uuid in the status line would only push out something a person can read.
            TurnEvent::Started { .. } => {}
            TurnEvent::Token(text) => {
                if let Some(last) = self.transcript.last_mut() {
                    last.push_body(&text);
                }
            }
            // Activity attaches to the in-flight assistant message, so it sits with
            // the answer it produced instead of in a panel the user has to correlate.
            TurnEvent::Step { agent, label } => {
                if let Some(agent) = &agent {
                    self.note_provenance(agent);
                }
                if let Some(message) = self.transcript.last_mut() {
                    match agent {
                        None => message.steps.push(label),
                        Some(agent) => trace_for(message, &agent).steps.push(label),
                    }
                }
            }
            // The run is holding a command at the gate. Keep the turn open and show it.
            TurnEvent::Approval(request) => {
                let commands = request.actions.len();
                self.pending_approval = Some(request);
                // Already decided for this turn: answer without asking again. The command
                // is still recorded in the trace, so what ran remains reviewable — this
                // removes the interruption, not the record.
                if self.approve_rest_of_turn || self.approve_conversation {
                    self.status = if self.approve_conversation {
                        "approved (rest of conversation) — running…".into()
                    } else {
                        "approved (rest of turn) — running…".into()
                    };
                    self.decide(true, cx);
                    return;
                }
                self.status = if commands == 1 {
                    "waiting for your approval".into()
                } else {
                    format!("waiting for your approval ({commands} commands)")
                };
                // The composer stays disabled: this turn is still running, it is just
                // paused on a question for the user.
            }
            TurnEvent::SubagentToken { agent, text } => {
                self.note_provenance(&agent);
                if let Some(message) = self.transcript.last_mut() {
                    trace_for(message, &agent).push_text(&text);
                }
            }
            // Each `values` event is a *whole* snapshot, so replace rather than
            // merge. The spine rides along in the same payload, which keeps the
            // mission current without another HTTP round trip.
            TurnEvent::Snapshot(snapshot) => {
                self.save_reports(&snapshot.reports, cx);
                if !snapshot.reports.is_empty() {
                    self.reports = snapshot.reports.clone();
                }
                if !snapshot.sources.is_empty() {
                    self.sources = snapshot.sources.clone();
                    // Verified as it arrives, not when someone thinks to ask.
                    self.resolve_sources(cx);
                }
                if !snapshot.datasets.is_empty() {
                    // What the model *chose*, which is a different claim from what the search
                    // *found*. Kept apart deliberately — §289.
                    self.recommended_ids = snapshot
                        .datasets
                        .iter()
                        .map(|dataset| bare_persistent_id(&dataset.persistent_id))
                        .filter(|id| !id.is_empty())
                        .collect();
                    self.recommended_datasets = snapshot.datasets.clone();
                }
                self.reload_datasets();
                if !snapshot.documents.is_empty() {
                    self.documents = snapshot.documents.clone();
                }
                if let Some(project) = snapshot.project {
                    self.project = Some(merge_spine(self.project.as_ref(), project));
                }
                if !snapshot.buckets.is_empty() {
                    self.buckets = snapshot.buckets;
                }
                // **Replaced, not merged.** A plan is a whole statement about the current
                // intention: the model rewrites the list to reorder or drop a step, so keeping
                // the old items when a shorter list arrives would show work the agent has
                // abandoned. Guarded on non-empty for the opposite reason — a frame that carries
                // no `todos` at all is silent about the plan, not a claim there isn't one (§209).
                if !snapshot.todos.is_empty() {
                    self.plan = snapshot.todos;
                }
                // The conversation this stream belongs to, and so the owner of any worker it
                // launched. Safe to read here and nowhere else: `apply` only runs mid-turn, and
                // both `New thread` and opening another conversation refuse while streaming — so
                // the open thread cannot have moved by the time this line runs.
                let owner = self.sidecar.thread_id().unwrap_or_default();
                self.adopt_background_work(
                    &snapshot.drafts,
                    snapshot.jobs,
                    snapshot.tasks,
                    &owner,
                    cx,
                );
            }
            TurnEvent::Done => {
                self.streaming = false;
                self.finish_turn(cx);
                self.status = "done".into();
                if let Some(last) = self.transcript.last() {
                    if last.body.is_empty() {
                        self.status = "done — but no assistant text arrived".into();
                    }
                }
            }
            TurnEvent::Error(message) => {
                self.streaming = false;
                self.finish_turn(cx);
                self.status = "failed".into();
                // A failure to *start* is a setup problem, not a turn problem, and
                // "backend did not become healthy within 120 attempts" tells the user
                // nothing they can act on. Open the diagnosis instead of the log path.
                if looks_like_a_setup_failure(&message) {
                    self.error = Some(format!("{message} — see Setup for what is missing"));
                    self.open_setup(cx);
                    return;
                }
                // Point at the sidecar log: backend-side failures (a missing key,
                // a bad graph import) surface there, not in the HTTP error.
                self.error = Some(format!(
                    "{message} — sidecar log: {}",
                    self.sidecar.log_path()
                ));
            }
        }
    }

    /// A turn ended (either way): collapse its activity trace, drop the assistant
    /// placeholder if nothing at all arrived, and hand the field back to the user.
    /// Reload the conversation list from the backend.
    ///
    /// Cheap, and called whenever a turn ends or a name changes, because a sidebar that
    /// is only correct at launch is worse than none: it teaches the researcher to distrust
    /// it, and then they stop looking.
    /// Ask once, at launch, whether a newer build exists.
    ///
    /// **Not on a button.** A button nobody presses is a check that never runs, and the person
    /// who most needs the answer is the one who does not know to look for it. This is the quiet
    /// half: it asks, records, and shows the answer where the version already is. Nothing pops up
    /// and nothing is downloaded — pressing to take an update stays a separate, deliberate act.
    fn check_for_update(&mut self, cx: &mut Context<Self>) {
        let mut answer = self.sidecar.check_for_update();
        cx.spawn(async move |workbench, cx| {
            let Some(standing) = answer.next().await else {
                return;
            };
            let _ = workbench.update(cx, |workbench, cx| {
                match &standing {
                    update::Standing::Behind(release) => tracing::info!(
                        running = %env!("CARGO_PKG_VERSION"),
                        published = %release.tag,
                        "a newer build is published"
                    ),
                    // Nothing else to do here; the fetch is started below, once the standing is
                    // recorded, because `take_update` reads it off `self`.
                    // Logged at `warn` rather than swallowed: a check that has been silently
                    // failing for a month looks exactly like an app that is up to date.
                    update::Standing::Unknown(reason) => {
                        tracing::warn!(%reason, "could not check for a newer build");
                    }
                    other => tracing::info!(standing = ?other, "checked for a newer build"),
                }
                // Read off the *answer*, not off the field: taking it from `workbench.update`
                // reads the previous value, which is `None` on the only check that ever runs — so
                // the download would never start and nothing would say why.
                let behind = matches!(standing, update::Standing::Behind(_));
                workbench.update = Some(standing);
                // **Downloaded without being asked**, which is what "Restart to Update" requires:
                // a button offering a restart has to have something staged to restart into. Ten
                // megabytes, once per published release, on a machine that just fetched a graph.
                // Nothing is *installed* without a press — that is the part that matters.
                if behind && matches!(workbench.install, update::Layout::Packaged(_)) {
                    workbench.take_update(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }


    /// The update that is downloaded and ready to install, if there is one.
    ///
    /// **One reader, used by both the chip and the press.** They used to ask different questions:
    /// the chip wanted a staged download on a packaged install, and the press *also* required the
    /// standing to still be `Behind`. Any state satisfying the first and not the second would draw
    /// a button that returned without doing anything and without saying why — §259's shape, in the
    /// one place where the symptom is "I pressed it and nothing happened".
    fn ready_update(&self) -> Option<(std::path::PathBuf, std::path::PathBuf, String)> {
        let Some(update::Fetch::Ready(staged, _)) = &self.taking else {
            return None;
        };
        let update::Layout::Packaged(install) = &self.install else {
            return None;
        };
        let Some(update::Standing::Behind(release)) = &self.update else {
            return None;
        };
        Some((install.clone(), staged.clone(), release.tag.clone()))
    }

    /// Restart into the update that is already downloaded.
    ///
    /// The last thing this process does. A helper is spawned that waits for this window to close,
    /// moves the folders and starts the new build — see `update::swap_script` for why the order is
    /// what it is. **If the helper cannot even start, the app stays open and says so**, because
    /// quitting after a failed spawn would look exactly like a successful update that lost the app.
    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Some((install, staged, tag)) = self.ready_update() else {
            // Not reachable through the chip, which asks the same question — kept because a
            // keyboard path or a stale click could still land here.
            tracing::warn!("restart was pressed with nothing staged to restart into");
            return;
        };
        let plan = update::Swap::plan(std::process::id(), &install, &staged, &tag);
        tracing::info!(
            pid = plan.pid,
            install = %plan.install.display(),
            staged = %plan.staged.display(),
            log = %plan.log.display(),
            "restarting into a new build"
        );
        match update::begin_swap(&plan) {
            Ok(()) => {
                // The helper is waiting on this pid, so the last useful act is to stop existing.
                self.status = "restarting into the new build…".into();
                cx.notify();
                cx.quit();
            }
            Err(reason) => {
                tracing::warn!(%reason, "could not start the updater");
                self.taking = Some(update::Fetch::Failed(reason.clone()));
                self.error = Some(format!("could not restart to update: {reason}"));
                cx.notify();
            }
        }
    }


    /// Download the published build and stage it beside this one.
    ///
    /// Only reachable when the standing is `Behind` **and** the install is a bundle, so the two
    /// refusals in `update.rs` gate the button rather than being re-argued here.
    fn take_update(&mut self, cx: &mut Context<Self>) {
        let Some(update::Standing::Behind(release)) = self.update.clone() else {
            return;
        };
        let update::Layout::Packaged(install) = self.install.clone() else {
            return;
        };
        if matches!(self.taking, Some(update::Fetch::Progress(..))) {
            return; // already going; a second press must not start a second download
        }
        tracing::info!(tag = %release.tag, bytes = release.size, "taking an update");
        self.taking = Some(update::Fetch::Progress(0, release.size));
        let mut steps = self.sidecar.take_update(release, install);
        cx.spawn(async move |workbench, cx| {
            while let Some(step) = steps.next().await {
                let done = !matches!(step, update::Fetch::Progress(..));
                let updated = workbench.update(cx, |workbench, cx| {
                    match &step {
                        update::Fetch::Ready(root, integrity) => tracing::info!(
                            staged = %root.display(),
                            checked = ?integrity,
                            "an update is downloaded and verified"
                        ),
                        update::Fetch::Failed(reason) => {
                            tracing::warn!(%reason, "could not take the update");
                        }
                        update::Fetch::Progress(..) => {}
                    }
                    workbench.taking = Some(step);
                    cx.notify();
                });
                // The window has gone. Stop rather than keep decoding a download nobody waits for.
                if updated.is_err() || done {
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Bring the backend up at launch, then show the history it has.
    fn warm_up(&mut self, cx: &mut Context<Self>) {
        self.status = "starting the agent…".into();
        // Alongside the backend rather than after it: the two are unrelated, and an update check
        // that waited for a graph to build would be a check that never ran on a slow launch.
        self.check_for_update(cx);
        let mut ready = self.sidecar.warm_up();
        cx.spawn(async move |this, cx| {
            let status = ready.next().await;
            let Some(status) = status else {
                return;
            };
            // Populate names as soon as the server can answer. Graph construction is unrelated to
            // `/threads/search`; making the list wait for it would fix the first-click pause by
            // creating the same pause in the sidebar instead (docs §176).
            let graph = this.update(cx, |workbench, cx| {
                workbench.status = "loading research tools…".into();
                workbench.warming = true;
                // Remembered, not just announced. Whether this app started the backend decides
                // whether it is running this app's overlay, and the status line is gone by the
                // time that matters (docs §80).
                workbench.backend_start = Some(status);
                workbench.refresh_conversations(cx);
                workbench.refresh_project(cx);
                cx.notify();
                workbench.sidecar.warm_graph()
            });
            let Ok(mut graph) = graph else {
                return;
            };
            let outcome = graph.next().await;
            let _ = this.update(cx, |workbench, cx| {
                workbench.warming = false;
                match outcome {
                    Some(Ok(())) => workbench.status = status.label().into(),
                    Some(Err(error)) => {
                        // Startup remains usable: a dependency may be temporarily unreachable,
                        // and the first real turn will surface its contextual error. What must not
                        // happen is the status bar claiming the graph is ready, or waiting forever.
                        tracing::warn!(%error, "agent graph did not finish warming at startup");
                        workbench.status = "backend started; research tools are not ready".into();
                    }
                    None => {
                        workbench.status = "backend started; research tools are not ready".into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
        // Also ask straight away: a backend left running from a previous session answers
        // immediately, and waiting on the spawn would hide the list for no reason.
        self.refresh_conversations(cx);
    }

    /// Ask the backend once per launch whether any background run finished unattended.
    ///
    /// Polling a run's route is what makes `routes/artifacts.py` write its charts and metrics into
    /// the conversation's folder, so this *is* the collection — not a notification about it.
    fn sweep_finished_jobs(&mut self, cx: &mut Context<Self>) {
        if self.swept {
            return;
        }
        let mut answer = self.sidecar.sweep_finished_jobs();
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match outcome {
                        // Could not ask. Leave `swept` false so the next refresh tries again.
                        None => {}
                        Some(collected) => {
                            workbench.swept = true;
                            // **Announced once, ever.** "Finished" stays true forever, so the
                            // sweep re-collects the same completed run on every launch — and
                            // without this it re-announced it too, every time (§250).
                            let announced = workspace::announced_runs();
                            let fresh: Vec<(String, protocol::Job)> = collected
                                .into_iter()
                                .filter(|(_, job)| !announced.contains(&job.task_id))
                                .collect();
                            if !fresh.is_empty() {
                                workspace::remember_announced(
                                    &fresh
                                        .iter()
                                        .map(|(_, job)| job.task_id.clone())
                                        .collect::<Vec<_>>(),
                                );
                                // Kept rather than only announced: the status line is a strip at
                                // the bottom of the window that the next thing to happen
                                // overwrites, and this is the one message the app has that the
                                // researcher cannot arrive at any other way (§244).
                                workbench.collected_runs = fresh;
                                // Their outputs just landed on disk.
                                workbench.refresh_project(cx);
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn refresh_conversations(&mut self, cx: &mut Context<Self>) {
        // Read from disk rather than from `self.draft`, which is the Settings pane's editing
        // buffer — the same argument `remember_panels` makes. The migration must be decided by
        // what is *stored*, because that is what survives to the next launch.
        let adopt = !settings::Settings::load().adopted_untagged;
        // **Read here, not in the answer.** Projects are directories on this machine and the
        // backend has nothing to do with them, but hanging the read off a successful HTTP reply
        // meant a cold launch — where the first refresh reliably fires before the server is up,
        // as `list_conversations` itself documents — showed no project headings at all until some
        // later refresh happened to succeed. An empty project would simply not be there on the
        // launch after it was created (§167).
        self.folder_projects = workspace::projects();
        // **Collected here because this is the call that already runs at launch and keeps
        // retrying.** A run finishing unattended is not collected by anything else (§243), and the
        // first attempt reliably loses the race with the starting server — so it is tried again on
        // each refresh until the search actually answers, then never again this launch.
        self.sweep_finished_jobs(cx);
        let mut updates = self.sidecar.list_conversations(adopt);
        cx.spawn(async move |this, cx| {
            if let Some(answer) = updates.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.conversations = answer.conversations;
                    // Again on the answer, because a turn may have created a project folder
                    // while this request was in flight. Cheap: one `read_dir` of a directory
                    // holding a handful of entries.
                    workbench.folder_projects = workspace::projects();
                    // Only on a real answer. A failed fetch sends nothing, so the list keeps
                    // saying "loading" rather than claiming the researcher has none — a
                    // backend that is still booting will answer the next refresh.
                    workbench.conversations_loaded = true;
                    if answer.scanned {
                        workbench.remember_adoption();
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Record that §90's pre-tag scan has run, so it can never run again.
    ///
    /// Deliberately *not* conditional on having adopted anything: an installation with no
    /// untagged threads is exactly the one that must stop scanning, and it is the one where
    /// deleting every conversation used to bring the leftovers back (docs §166).
    fn remember_adoption(&self) {
        let mut stored = settings::Settings::load();
        if stored.adopted_untagged {
            return;
        }
        stored.adopted_untagged = true;
        if let Err(error) = stored.save() {
            // The scan did run; all that failed is remembering it. Worth a log because the
            // consequence is a repeat scan, which is the defect this whole field exists for.
            tracing::warn!(%error, "could not record that the pre-tag scan has run");
        }
    }

    /// Reopen a past conversation: switch threads and rebuild the transcript.
    fn open_conversation(&mut self, thread_id: String, cx: &mut Context<Self>) {
        if self.streaming {
            self.status = "can't switch conversations mid-turn".into();
            return;
        }
        if self.sidecar.thread_id().as_deref() == Some(thread_id.as_str()) {
            return;
        }
        // Clear what belongs to the conversation being left. The spine is
        // thread-independent, so it stays — same rule as `New thread`.
        self.transcript.clear();
        self.reset_transcript_list();
        // Read back from the thread being opened, below. Cleared first so a failure to load
        // shows the new conversation as having no record rather than the previous one's.
        self.provenance = provenance::Record::default();
        self.text_selection.clear_document();
        self.buckets.clear();
        self.tasks.clear();
        self.jobs.clear();
        self.plan.clear();
        // The record of who wrote what belongs to the conversation being left. Cleared with the
        // stamp, or the next frame would see an unchanged `None` and keep the old map.
        self.authorship.clear();
        self.authorship_stamp = None;
        self.error = None;
        self.approve_conversation = false;
        self.approve_tasks.clear();
        self.status = "opening…".into();
        self.opening = true;

        // Adopt the project it is filed under *before* the fetch, so `thread_workspace` — which
        // the figures and the provenance record are read from — is looking in the right folder
        // by the time they land.
        let filed = self
            .conversations
            .iter()
            .find(|conversation| conversation.thread_id == thread_id)
            .and_then(|conversation| conversation.project.clone());
        self.sidecar.set_project(filed);
        self.project = None;
        // Kept before the id is handed to the sidecar: any worker in this conversation's state
        // belongs to *this* conversation, and that is the fact `decide_task` needs later.
        let owner = thread_id.clone();
        let mut messages = self.sidecar.open_conversation(thread_id);
        cx.spawn(async move |this, cx| {
            if let Some((messages, snapshot)) = messages.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    // The user can leave — a new thread, a different conversation — while this
                    // fetch is still in flight. Landing it anyway would push a stranger's
                    // messages into whatever is open by the time the response arrives.
                    if workbench.sidecar.thread_id().as_deref() != Some(owner.as_str()) {
                        return;
                    }
                    for (role, body) in messages {
                        // Roles come back as the two the transcript renders; anything
                        // else was filtered out server-side by `decode_stored_message`.
                        let role = if role == "you" { "you" } else { "mini-me" };
                        workbench.transcript.push(Message::new(role, body));
                    }
                    // Datasets likewise: the search results are a file in this conversation's
                    // folder, so reopening it shows what the searches found rather than nothing
                    // until the next turn happens to answer.
                    workbench.recommended_ids.clear();
                    workbench.recommended_datasets.clear();
                    // A tick is about *these* rows. Carrying it across would offer to fetch a
                    // dataset the researcher chose while reading a different search.
                    workbench.dataset_picks.clear();
                    workbench.reload_datasets();
                    // Figures this conversation produced are still on disk, so they can
                    // be shown again — history the transcript alone cannot carry.
                    workbench.collect_plots();
                    // Same argument, for the same reason: the record of what was consulted is
                    // on disk because the stream it came from is over (docs §73).
                    if let Some(dir) = workbench.thread_workspace() {
                        workbench.provenance = provenance::load(&dir);
                    }
                    // **And pick up any long run still going.** A theorizer or DataVoyager task
                    // lives on Asta's own service, keyed by a task id the thread's artifacts
                    // carry — so closing the window never stopped the work, only our watching of
                    // it. The state we just fetched for the messages already holds those ids,
                    // and `track_job` re-arms the poll that persists the result (docs §102).
                    if let Some(snapshot) = snapshot {
                        if !snapshot.buckets.is_empty() {
                            workbench.buckets = snapshot.buckets;
                        }
                        // **The reference list survives a reload too.** Restoring `buckets` and
                        // not `sources` left the panel with the plain bucket rendering — a name,
                        // a count and `+13 more` — while everything §185 to §195 built sat on
                        // `self.sources`: the unverified count, the provenance note, the link,
                        // the row you can press. Reported as *"when I reload the conversation I
                        // cannot see the interaction of sources"*, and the two lists looked
                        // similar enough that the difference read as the feature being broken
                        // rather than as a second renderer (docs §196).
                        if !snapshot.sources.is_empty() {
                            workbench.sources = snapshot.sources;
                            // Checked on reopen as on arrival, or a conversation returned to is
                            // a conversation whose citations are all silently unverified.
                            workbench.resolve_sources(cx);
                        }
                        // **Not `workbench.datasets`.** §290 pointed the panel at the search's
                        // own file; this line went on overwriting it with the model's retyped
                        // list every time a conversation was reopened, so the live path and the
                        // reopen path rendered different things and only one of them came from
                        // Dataverse. What the model chose is a mark and a sort, and nothing else.
                        if !snapshot.datasets.is_empty() {
                            workbench.recommended_ids = snapshot
                                .datasets
                                .iter()
                                .map(|dataset| bare_persistent_id(&dataset.persistent_id))
                                .filter(|id| !id.is_empty())
                                .collect();
                            workbench.recommended_datasets = snapshot.datasets;
                            workbench.reload_datasets();
                        }
                        if !snapshot.documents.is_empty() {
                            workbench.documents = snapshot.documents;
                        }
                        if !snapshot.reports.is_empty() {
                            workbench.reports = snapshot.reports;
                        }
                        // Restored with them, and for §196's reason: a conversation reopened
                        // mid-plan showing no plan reads as the feature being broken.
                        if !snapshot.todos.is_empty() {
                            workbench.plan = snapshot.todos;
                        }
                        if let Some(project) = snapshot.project {
                            workbench.project =
                                Some(merge_spine(workbench.project.as_ref(), project));
                        }
                        // The same call the streaming path makes, and the point of it being a
                        // call: this site used to handle jobs and tasks and quietly ignore
                        // `drafts` (§259).
                        workbench.adopt_background_work(
                            &snapshot.drafts,
                            snapshot.jobs,
                            snapshot.tasks,
                            &owner,
                            cx,
                        );
                    }
                    workbench.opening = false;
                    workbench.status = "done".into();
                    workbench.refresh_project(cx);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Open the centred warning for a conversation or a whole project.
    fn request_delete(
        &mut self,
        target: DeleteTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.deleting.is_some() {
            return;
        }
        let current_is_targeted = self
            .sidecar
            .thread_id()
            .is_some_and(|thread_id| target.contains_thread(&thread_id));
        if self.streaming && current_is_targeted {
            self.say(
                format!(
                    "can't delete this {} while its turn is running",
                    target.noun()
                ),
                cx,
            );
            return;
        }
        // **A warning now, not a refusal.** A background worker can still be writing beneath the
        // conversation directory after the foreground turn ends, and deleting that tree underneath
        // it can lose the remainder of its work — so the modal says so, in the sentence that asks.
        //
        // It used to refuse outright, and that was worse than the thing it prevented: a task that
        // never reaches a terminal state locks the conversation **forever**. It happened while the
        // app was being shown to a colleague — a paper search that had said "running" for over an
        // hour, and no way to remove the conversation at all. A guard with no way past it is a
        // guard that eventually holds the wrong thing (§278).
        self.delete_interrupts_work =
            current_is_targeted && self.tasks.iter().any(|task| !task.is_finished());
        self.confirming_delete = Some(target);
        window.focus(&self.delete_focus);
        cx.notify();
    }

    /// Carry out exactly what the modal named: durable threads first, managed files second.
    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.confirming_delete.take() else {
            return;
        };
        if self.deleting.is_some() {
            return;
        }
        let noun = target.noun();
        self.status = format!("deleting {noun}…");
        self.deleting = Some(target.clone());
        let mut deleted = self
            .sidecar
            .delete_conversations(target.thread_ids(), target.files());
        cx.spawn(async move |this, cx| {
            let result = deleted.next().await;
            let _ = this.update(cx, |workbench, cx| {
                workbench.deleting = None;
                match resolve_delete(target.noun(), result) {
                    DeleteResolution::Remove { files_error } => {
                        let removed = target.thread_ids();
                        workbench
                            .conversations
                            .retain(|conversation| !removed.contains(&conversation.thread_id));
                        // The folder went with them, so the heading must too. Re-read rather
                        // than removing by name: deleting a conversation can empty a project and
                        // take its folder as well (§155), and that is the same fact (§167).
                        workbench.folder_projects = workspace::projects();
                        // If it was the open one, leave a genuinely ungrouped empty slate rather
                        // than a transcript and project whose thread no longer exists (§154).
                        let open_was_removed = workbench
                            .sidecar
                            .thread_id()
                            .is_some_and(|thread_id| target.contains_thread(&thread_id));
                        let active_project_was_removed = workbench
                            .sidecar
                            .project()
                            .is_some_and(|name| !project_exists(&workbench.conversations, &name));
                        if open_was_removed {
                            workbench.sidecar.reset_thread();
                            workbench.transcript.clear();
                            workbench.provenance = provenance::Record::default();
                            workbench
                                .text_selection
                                .update(|selection| selection.clear());
                            workbench.buckets.clear();
                            workbench.tasks.clear();
                            workbench.jobs.clear();
                        }
                        if open_was_removed || active_project_was_removed {
                            // Also covers an empty "new conversation here" slate whose thread has
                            // not been created yet. Leaving only its project key alive would make
                            // the next question recreate the project just deleted (§155).
                            workbench.sidecar.set_project(None);
                            workbench.project = None;
                            workbench.refresh_project(cx);
                        }
                        match files_error {
                            None => workbench.say(format!("{} deleted", target.noun()), cx),
                            Some(error) => {
                                // The irreversible server half succeeded, so restoring the row
                                // would be another lie. Keep the recoverable folder and say where
                                // synchronization stopped instead (§155).
                                workbench.error = Some(format!(
                                    "The {} was deleted, but its saved folder remains: {error}",
                                    target.noun()
                                ));
                                workbench.say(
                                    format!(
                                        "{} deleted; its saved folder could not be removed",
                                        target.noun()
                                    ),
                                    cx,
                                );
                            }
                        }
                    }
                    DeleteResolution::Keep(error) => {
                        // Keep the row because the backend kept the conversation. Claiming
                        // success here is the precise defect that only restart exposed. A project
                        // batch can have succeeded partly before one request failed, so refresh
                        // instead of assuming our captured list is still authoritative (§155).
                        workbench.error = Some(error);
                        workbench.status = format!("{} was not fully deleted", target.noun());
                        workbench.refresh_conversations(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Begin renaming a conversation, with its current name in the field.
    fn start_rename(&mut self, thread_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .conversations
            .iter()
            .find(|conversation| conversation.thread_id == thread_id)
            .map(|conversation| conversation.title.clone())
            .unwrap_or_default();
        self.renaming = Some(thread_id);
        self.rename_editor.update(cx, |editor, cx| {
            editor.set_text(&current, cx);
        });
        // Focus the field, or the researcher clicks "rename" and types into the composer.
        self.rename_editor.read(cx).focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Commit the new name — locally first, so the list never lags the typing.
    fn commit_rename(&mut self, title: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self.renaming.take() else {
            return;
        };
        let title = title.trim().to_string();
        if !title.is_empty() {
            if let Some(conversation) = self
                .conversations
                .iter_mut()
                .find(|conversation| conversation.thread_id == thread_id)
            {
                conversation.title = title.clone();
            }
            self.sidecar.rename_conversation(thread_id, title);
        }
        self.restore_focus = true;
        cx.notify();
    }

    /// Re-read who wrote what, but only when the record itself has moved.
    ///
    /// The manifest is append-only and grows by a line per file, so parsing it on every frame of
    /// a streaming answer would be real work for an answer that changes a few times a turn. Size
    /// **and** mtime, because an append that lands inside one filesystem timestamp tick still
    /// changes the length.
    fn refresh_authorship(&mut self) {
        let Some(dir) = self.thread_workspace() else {
            self.authorship.clear();
            self.authorship_stamp = None;
            return;
        };
        let stamp = std::fs::metadata(dir.join(workspace::AUTHORSHIP))
            .ok()
            .and_then(|meta| meta.modified().ok().map(|at| (at, meta.len())));
        if stamp == self.authorship_stamp {
            return;
        }
        self.authorship_stamp = stamp;
        self.authorship = workspace::authorship(&dir);
    }

    /// Begin editing the mission, with the current one in the field.
    fn start_mission_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .project
            .as_ref()
            .map(|project| project.mission.clone())
            .unwrap_or_default();
        self.editing_mission = true;
        self.mission_editor.update(cx, |editor, cx| {
            editor.set_text(&current, cx);
        });
        // Same reason as `start_rename`: without this the researcher presses the mission and
        // types into the composer at the bottom of the window.
        self.mission_editor.read(cx).focus_handle(cx).focus(window);
        cx.notify();
    }

    /// Save a hand-edited mission, and show what the store actually kept.
    ///
    /// **Not optimistic**, unlike renaming a conversation. A name is ours and lives on the thread;
    /// a mission is the backend's, which caps it at 500 characters and collapses whitespace before
    /// storing it — and it is read back into the coordinator's system prompt on the next turn
    /// (`backend/middleware/project.py`). Showing the typed text and letting the stored text
    /// differ would mean the panel disagreed with what the agent is actually working to.
    fn commit_mission(&mut self, mission: String, cx: &mut Context<Self>) {
        if !self.editing_mission {
            return;
        }
        self.editing_mission = false;
        self.restore_focus = true;
        let mission = mission.trim().to_string();
        let mut saved = self.sidecar.set_mission(mission);
        self.status = "saving the mission…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = saved.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match outcome {
                        // Only the spine's own fields: `PATCH /project` cannot return live
                        // suggestions — its docstring says so, because they are derived from a
                        // running thread's artifacts — and taking its empty list wholesale would
                        // clear the advice on screen as a side effect of an unrelated edit.
                        Ok(project) => {
                            workbench.project =
                                Some(merge_spine(workbench.project.as_ref(), project));
                            workbench.status = "mission saved".into();
                        }
                        Err(error) => {
                            // Said out loud, and the panel keeps the mission the backend still
                            // holds. A silent failure here is the worst kind: the researcher
                            // believes the agent has been redirected and it has not.
                            workbench.status = format!("could not save the mission: {error}");
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Note that a specialist produced something, now.
    ///
    /// Called from every frame that carries an [`AgentRef`], which is what makes the interval an
    /// *arrival* interval — narrower than the execution it stands for, and honest about it
    /// (docs §74). Cheap by construction: a scan of one turn's invocations, which is single
    /// digits even on a heavily delegated question.
    fn note_provenance(&mut self, agent: &AgentRef) {
        self.provenance
            .observe(&agent.ns, &agent.name, provenance::now_ms());
    }

    /// Put every report this turn has produced on disk, beside the figures and the data.
    ///
    /// **Because otherwise there is no report.** A report artifact is `{title, markdown}` living
    /// in the run's state — unlike a figure, which a plotting script writes to the workspace, or a
    /// dataset, which is a file by nature. So the agent would say "the report is in the Outputs
    /// panel", which was true, and the researcher would open the thread's folder and find seven
    /// files, none of them the report (docs §89).
    ///
    /// Called on every `values` snapshot, which is often; [`workspace::save_report`] skips a write
    /// whose content is already there, so the cost is a read per report per frame and the file's
    /// timestamp stays honest.
    fn save_reports(&mut self, reports: &[protocol::Report], cx: &mut Context<Self>) {
        if reports.is_empty() {
            return;
        }
        let Some(dir) = self.thread_workspace() else {
            return;
        };
        for report in reports {
            match workspace::save_report(&dir, &report.title, &report.markdown) {
                Ok(path) => {
                    if self.saved_reports.insert(path.clone()) {
                        // Said once per report, because a file appearing silently in a folder is
                        // not something anyone notices — and not finding it was the whole
                        // complaint.
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.say(format!("saved {name}"), cx);
                    }
                }
                Err(error) => tracing::warn!(%error, "could not save a report"),
            }
        }
    }

    /// Typeset the newest report into this conversation's folder.
    ///
    /// The markdown is already on disk beside it — that is what `save_reports` fixed — so this is
    /// the second half of the same answer: *"how do we render it as a PDF?"* Through the backend,
    /// which has done it since before this app existed (`backend/routes/rendering.py`) and does it
    /// with the figures resolved and the citations laid out.
    fn render_report(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.reports.last().cloned() else {
            self.say("no report in this conversation yet", cx);
            return;
        };
        let Some(dir) = self.thread_workspace() else {
            self.say("no conversation folder yet", cx);
            return;
        };
        let into = dir.join(workspace::report_filename(&report.title));
        self.say(format!("rendering {}…", report.title), cx);
        let mut rendered = self.sidecar.render_report(
            report.title.clone(),
            report.markdown.clone(),
            // Whole. This used to map each source down to its citation, under a comment claiming
            // the template took a list of strings — it takes a list of objects, and the mismatch
            // was a 502 on every report that had a source to cite (§141). The `link` it now
            // carries is the one the backend supplied, so the bibliography's DOIs resolve.
            self.sources.clone(),
            self.used_asta(),
            into,
        );
        cx.spawn(async move |this, cx| {
            if let Some(result) = rendered.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match result {
                        Ok(path) => {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            workbench.say(format!("saved {name}"), cx);
                            // Opened, because a PDF is made to be looked at and the folder is
                            // one more step between the researcher and the thing they asked for.
                            if let Err(error) = workspace::open(&path) {
                                tracing::warn!(%error, "could not open the rendered report");
                            }
                        }
                        // Surfaced whole: a Typst compile fails for reasons that are in the
                        // message, and "could not render" alone would waste the only useful part.
                        Err(error) => workbench.error = Some(format!("{error:#}")),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Persist this conversation's record, if there is a conversation to persist it under.
    ///
    /// A failure is reported in the status line rather than swallowed. This is the researcher's
    /// record of their own enquiry, and a provenance file that silently stopped being written
    /// would be discovered weeks later, as a gap — which is precisely the failure §81 spent four
    /// attempts on. It does not interrupt the turn, which has already succeeded.
    fn save_provenance(&mut self) {
        let Some(dir) = self.thread_workspace() else {
            return;
        };
        if self.provenance.is_empty() {
            return;
        }
        if let Err(error) = provenance::save(&dir, &self.provenance) {
            tracing::warn!(%error, "could not write the provenance record");
            self.error = Some(format!(
                "could not save this conversation's provenance: {error}"
            ));
        }
    }

    /// The thread's own output directory, or `None` before the first turn creates one.
    fn thread_workspace(&self) -> Option<std::path::PathBuf> {
        let project = self.sidecar.project();
        self.sidecar
            .thread_id()
            .map(|thread_id| workspace::thread_dir_in(project.as_deref(), &thread_id))
    }

    /// Attach any output not already on screen to the newest answer.
    ///
    /// A diff rather than a report, because nothing reports it: a figure is written by a
    /// plotting script inside `execute`, which registers no artifact (docs §42), and so is the
    /// cleaned CSV beside it.
    ///
    /// Diffed against **what the transcript already shows**, not against a snapshot taken
    /// when the turn began. A background worker finishes on its own schedule — often
    /// between turns, sometimes minutes after the turn that started it — and a
    /// start-of-turn snapshot simply missed those (docs §43). This way the call is safe to
    /// make from anywhere, as often as we like.
    fn collect_plots(&mut self) {
        let shown: std::collections::HashSet<_> = self
            .transcript
            .iter()
            .flat_map(|message| message.outputs.iter().map(|output| output.path.clone()))
            .collect();
        let mut produced: Vec<workspace::Output> = self
            .thread_workspace()
            .map(|dir| workspace::outputs(&dir))
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(_, items)| items)
            .filter(|output| !shown.contains(&output.path))
            .collect();
        // Oldest first, so a turn's files read in the order they were written — which for an
        // analysis script is the order the work went in. Sorted on the stamp rather than by
        // reversing what `outputs` returned: that is grouped by kind *and then* newest-first, so
        // reversing it would have put every figure after every stray file rather than putting
        // anything in chronological order.
        produced.sort_by_key(|output| output.modified);
        if produced.is_empty() {
            return;
        }
        if let Some(message) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|message| message.role == "mini-me")
        {
            message.outputs.extend(produced);
        }
    }

    /// Re-read files after a writer has reported completion and evict any image decode cached
    /// while that writer still had the file open.
    ///
    /// The Outputs panel scans on every paint, which made a just-created PNG visible *before* it
    /// was necessarily complete. GPUI correctly cached that first decode failure by path, but a
    /// later paint asked for the same path and received the same failure forever; restarting the
    /// application was the only thing that cleared it. Two bounded follow-up passes cover the
    /// Windows/WSL filesystem hand-off without turning the whole workspace into a permanent
    /// polling loop (docs §158).
    /// Compare what each answer *said* it wrote against what the folder holds.
    ///
    /// **The claim was never checked against the data.** An answer would list ten filenames and
    /// the Outputs panel would show none of them; twice a turn reported plots saved after the
    /// command that would have written them failed (§42). The prompt forbids inventing charts,
    /// which is a rule with nothing behind it — this is the something.
    ///
    /// Recomputed over **every** assistant message, not just the last, and re-run as outputs
    /// settle. The workspace only grows, so a name that was missing when the turn ended and is
    /// present two seconds later stops being flagged on its own. That self-correction is the
    /// reason this is cheap to be wrong about in one direction and not the other.
    ///
    /// Matched on basename: an answer may write `outputs/plots/a.png` for a file the workspace
    /// holds at another depth, and the question is whether the file exists — not whether the
    /// model recited its path correctly.
    fn check_file_claims(&mut self) -> bool {
        let present: std::collections::HashSet<String> = self
            .thread_workspace()
            .map(|dir| workspace::outputs(&dir))
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(_kind, items)| items)
            .filter_map(|output| {
                std::path::Path::new(&output.name)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.to_ascii_lowercase())
            })
            .collect();

        // **A name the researcher introduced is not a claim the agent made.** Dropped files are
        // read where they lie rather than copied in (§13), so an answer discussing `kiwi.csv`
        // from the desktop would otherwise be flagged for not holding a file that was never
        // supposed to be here.
        let from_researcher: std::collections::HashSet<String> = self
            .transcript
            .iter()
            .filter(|message| message.role == "you")
            .flat_map(|message| named_files(&message.body))
            .map(|name| name.to_ascii_lowercase())
            .collect();

        // **A file a run has not finished writing is not a file that is missing.** DataVoyager and
        // the theorizer submit and return immediately; their outputs land at
        // `analysis/<task_id>.md` when the run reaches a terminal state, twenty to forty minutes
        // later. So the answer that says *"the results will appear in the Analysis panel when the
        // run completes"* was flagged for not holding a file it had just explained was forthcoming
        // — the one shape of false alarm this note cannot afford, since §175's whole argument is
        // that it reports a check rather than a verdict (docs §240).
        //
        // Matched on the task id, which is what the filename is built from, and only while the job
        // is unfinished: once it completes the file must be there, and if it is not, that is worth
        // exactly the warning this note gives.
        let awaited: Vec<String> = self
            .jobs
            .iter()
            .filter(|job| !job.is_finished())
            .map(|job| job.task_id.to_ascii_lowercase())
            .filter(|id| !id.is_empty())
            .collect();

        let mut changed = false;
        for index in 0..self.transcript.len() {
            if self.transcript[index].role == "you" {
                continue;
            }
            let missing: Vec<String> = named_files(&self.transcript[index].body)
                .into_iter()
                .filter(|name| {
                    let name = name.to_ascii_lowercase();
                    !present.contains(&name)
                        && !from_researcher.contains(&name)
                        && !awaited.iter().any(|id| name.contains(id.as_str()))
                })
                .collect();
            if self.transcript[index].unverified != missing {
                self.transcript[index].unverified = missing;
                changed = true;
            }
        }

        // **The offer moves with the note, and exists without one.** Refreshed here rather than
        // at render because both inputs are already in hand: the per-message `unverified` lists
        // were just recomputed above, and the ledger read is the same kind of cost as the
        // workspace walk this function opens with.
        self.stray = files_left_outside(&self.thread_commands());
        let placed = place_recovery_offer(
            &self.stray,
            &self
                .transcript
                .iter()
                .map(|message| (message.role == "you", message.unverified.is_empty()))
                .collect::<Vec<_>>(),
        );
        if self.recovery_on != placed {
            self.recovery_on = placed;
            changed = true;
        }
        changed
    }

    fn settle_outputs(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for delay in [250, 1_000] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay))
                    .await;
                if this
                    .update(cx, |workbench, cx| {
                        workbench.collect_plots();
                        if workbench.check_file_claims() {
                            // The note changes a row's height, and a virtualized list caches
                            // heights until told otherwise (§156).
                            workbench.invalidate_all_transcript_messages();
                        }
                        let figures: Vec<std::path::PathBuf> = workbench
                            .thread_workspace()
                            .map(|dir| workspace::outputs(&dir))
                            .unwrap_or_default()
                            .into_iter()
                            .flat_map(|(_, items)| items)
                            .filter(|output| output.kind == workspace::Kind::Figure)
                            .map(|output| output.path)
                            .collect();
                        for path in figures {
                            gpui::ImageSource::from(path).remove_asset(cx);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn finish_turn(&mut self, cx: &mut Context<Self>) {
        self.collect_plots();
        self.check_file_claims();
        self.settle_outputs(cx);
        // Written here, and only here, for the same reason the title is: the thread id does not
        // exist until the turn has run, so there is no directory to write into before this point.
        // A turn stopped or failed still gets recorded — what was consulted before it stopped is
        // part of the enquiry, and §63 already settled that a cut-off turn is worth keeping.
        self.save_provenance();
        // The thread id does not exist until the turn has run, which is why the title
        // waits until here rather than being set when the prompt was typed.
        if let (Some(title), Some(thread_id)) =
            (self.pending_title.take(), self.sidecar.thread_id())
        {
            self.sidecar.rename_conversation(thread_id, title);
        }
        // And the same reason attachments wait: the folder they belong in did not exist when they
        // were chosen. Now it does.
        self.adopt_pending(cx);
        self.refresh_conversations(cx);
        self.pending_approval = None;
        // Blanket approval expires with the turn it was given for. Carrying it into the
        // next question would turn a bounded decision into a permanent one, which is
        // exactly what the button is worded to avoid.
        self.approve_rest_of_turn = false;
        // While a turn runs the trace is the only sign of progress; once the answer
        // is there, the answer is the point.
        if let Some(message) = self.transcript.last_mut() {
            for trace in &mut message.agents {
                trace.expanded = false;
            }
            message.steps_expanded = false;
        }
        if self
            .transcript
            .last()
            .is_some_and(|message| message.role == "mini-me" && message.is_silent())
        {
            self.transcript.pop();
        }
        self.invalidate_all_transcript_messages();
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(false, cx));
        // A turn can change the spine — the mission is derived from the first
        // question, and completed/pending shift as work lands.
        self.refresh_project(cx);
    }


    /// Look for themes in Zed's gallery.
    fn search_gallery(&mut self, query: String, cx: &mut Context<Self>) {
        if query.trim().is_empty() {
            return;
        }
        self.gallery_note = "searching…".into();
        let mut results = self.sidecar.search_themes(query);
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = results.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    match outcome {
                        Ok(found) => {
                            workbench.gallery_note = if found.is_empty() {
                                "no themes matched".into()
                            } else {
                                format!("{} themes", found.len())
                            };
                            workbench.gallery_results = found;
                        }
                        // Most likely a proxy or no network, which is a normal state on a
                        // work laptop and not a reason for anything to look broken.
                        Err(error) => workbench.gallery_note = error,
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Download one, then show it in the list above.
    fn install_theme(&mut self, id: String, cx: &mut Context<Self>) {
        self.gallery_note = format!("installing {id}…");
        let mut done = self.sidecar.install_theme(id);
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = done.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.gallery_note = match outcome {
                        Ok(names) => format!("installed {} palettes", names.len()),
                        Err(error) => error,
                    };
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Remove the JSON file behind an installed palette and keep the live theme valid.
    ///
    /// One Zed file can carry a whole family, so the picker removes the file rather than
    /// pretending one palette inside it is independently installed. If the file supplied the
    /// current palette, a built-in with the same name is revealed and reapplied; if no such name
    /// remains, both the draft and the trigger move to the default. Leaving the deleted name in
    /// either place would make Settings display a choice the next launch cannot load (docs §181).
    fn uninstall_theme(&mut self, path: PathBuf, name: String, cx: &mut Context<Self>) {
        let removed = settings::available_theme_entries()
            .iter()
            .filter(|entry| entry.source.as_deref() == Some(path.as_path()))
            .count();

        match settings::uninstall_theme_file(&path) {
            Ok(()) => {
                let survivors = settings::available_themes();
                if theme_after_removal(&self.applied_theme, &survivors).is_some() {
                    self.applied_theme = theme::DEFAULT_NAME.to_string();
                    self.draft.theme = theme::DEFAULT_NAME.to_string();
                }
                // Also matters when the removed file overrode a built-in: the name remains, but
                // its palette has changed back to the bundled one and the next frame must too.
                settings::apply_theme(&self.draft);

                // **And `settings.toml` too, now rather than on Save.** Everything else in this
                // pane is a draft, and dismissing it reloads the file on the stated grounds that
                // *"an unsaved palette was a look, not a change"*. Deleting a file is not a look.
                // Leaving the removed name on disk meant Esc restored a palette whose JSON was
                // gone: the dropdown read `Catppuccin Mocha` over a window painted in the
                // default, and no restart cleared it.
                //
                // Checked against the stored name rather than the live one — they differ the
                // moment somebody previews a theme before removing another — and written through
                // a fresh load rather than by saving `self.draft`, which may be holding model or
                // key edits they have not chosen to keep.
                let mut stored = settings::Settings::load();
                if let Some(replacement) = theme_after_removal(&stored.theme, &survivors) {
                    stored.theme = replacement;
                    if let Err(error) = stored.save() {
                        tracing::warn!(%error, "could not write the removed theme out of settings");
                    }
                }
                self.gallery_note = if removed > 1 {
                    format!("removed {removed} palettes from the installed family")
                } else {
                    format!("removed {name}")
                };
            }
            Err(error) => self.gallery_note = format!("could not remove {name}: {error:#}"),
        }
        cx.notify();
    }



    /// Put text into one of the Settings fields.
    fn set_field(&mut self, field: Field, text: &str, cx: &mut Context<Self>) {
        let Some((_, composer)) = self.fields.iter().find(|(name, _)| *name == field) else {
            return;
        };
        let composer = composer.clone();
        let text = text.to_string();
        composer.update(cx, |composer, cx| composer.set_text(text, cx));
    }

    /// What a Settings field currently holds, falling back when it is empty.
    fn field_text_or(&self, field: Field, fallback: &str, cx: &App) -> String {
        let text = self
            .fields
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, composer)| composer.read(cx).text().to_string())
            .unwrap_or_default();
        if text.trim().is_empty() {
            fallback.to_string()
        } else {
            text
        }
    }



    /// File the open conversation under a project, moving its folder to match.
    ///
    /// **The folder moves first, and a failure stops there.** If the metadata were written first
    /// and the move then failed, the app would believe the conversation lives somewhere its files
    /// are not — which is the §89 shape again, and worse here because it would look like the
    /// files had been deleted.
    fn file_in_project(&mut self, project: Option<String>, cx: &mut Context<Self>) {
        self.open_picker = None;
        self.project_query
            .update(cx, |query, cx| query.set_text("", cx));
        let Some(thread_id) = self.sidecar.thread_id() else {
            return;
        };
        if self.streaming {
            self.say(
                "can't move a conversation mid-turn — its folder is in use",
                cx,
            );
            return;
        }
        let from = self.sidecar.project();
        if from == project {
            return;
        }
        if let Err(error) = workspace::move_thread(from.as_deref(), project.as_deref(), &thread_id)
        {
            self.error = Some(format!("{error:#}"));
            cx.notify();
            return;
        }
        self.sidecar.set_project(project.clone());
        self.say(
            match &project {
                Some(name) => format!("filed under {name}"),
                None => "filed under Ungrouped Conversations".to_string(),
            },
            cx,
        );
        self.sidecar.set_thread_project(thread_id, project);
        // The spine belongs to the project now, so moving between them changes which one the
        // panel shows. Cleared first: a stale mission above a new project's empty list reads as
        // that project having inherited the old one's work (docs §109).
        self.project = None;
        self.refresh_project(cx);
        self.refresh_conversations(cx);
    }

    /// Start a fresh conversation in a named project.
    ///
    /// The `+` beside a project heading. Same as `New thread` but without the step of starting
    /// somewhere else and filing afterwards — which is a folder move for something that had not
    /// needed to be anywhere yet.
    fn new_thread_in(&mut self, project: Option<String>, cx: &mut Context<Self>) {
        if self.streaming {
            self.say("can't start a new thread mid-turn", cx);
            return;
        }
        self.sidecar.reset_thread();
        self.sidecar.set_project(project.clone());
        self.project = None;
        self.refresh_project(cx);
        self.transcript.clear();
        // A conversation fetch started before this can still land after it — see the guard
        // in `open_conversation`'s completion — but the screen itself must not keep showing
        // "opening…" for a conversation that was just left (§262).
        self.opening = false;
        // A new conversation is a new enquiry. The one just left keeps its own record on disk,
        // where reopening it will find it.
        self.provenance = provenance::Record::default();
        self.text_selection.update(|selection| selection.clear());
        self.buckets.clear();
        self.tasks.clear();
        self.jobs.clear();
        self.plan.clear();
        // The record of who wrote what belongs to the conversation being left. Cleared with the
        // stamp, or the next frame would see an unchanged `None` and keep the old map.
        self.authorship.clear();
        self.authorship_stamp = None;
        self.error = None;
        // Blanket approval is scoped to the conversation, so it ends with it — together with
        // every per-task grant, whose tasks belonged to that conversation too.
        self.approve_conversation = false;
        self.approve_tasks.clear();
        self.refresh_conversations(cx);
        self.status = match project {
            Some(name) => format!("new conversation in {name}"),
            None => "new conversation in Ungrouped Conversations".into(),
        };
        cx.notify();
    }


















    /// The first rows of a table, measured at most once per version of the file.
    ///
    /// Cached beside the shape and for the same reason: this renders on every frame of a
    /// streaming answer, and a preview that re-read the file each time would be doing disk I/O
    /// sixty times a second on the thread drawing the window.
    fn preview_of(&self, output: &workspace::Output, rows: usize) -> Option<Vec<Vec<String>>> {
        if let Some(entry) = self.previews.borrow().get(&output.path) {
            if entry.0 == output.modified {
                return entry.1.clone();
            }
        }
        let found = workspace::table_preview(&output.path, rows);
        self.previews
            .borrow_mut()
            .insert(output.path.clone(), (output.modified, found.clone()));
        found
    }


    /// The provenance turn that produced the assistant message at `index`, if it can be known.
    ///
    /// **Matched from the end, not the start.** Reopening a conversation loads its messages from
    /// the server and its record from disk, and the two have different lengths on purpose: the
    /// activity trace does not survive a reload (§46) while the record does. Counting forwards
    /// would then pair message three with turn three and be wrong by however many turns the
    /// reload dropped. Both grow at the tail, so aligning the tails is the pairing that holds.
    fn turn_for(&self, index: usize) -> Option<&provenance::Turn> {
        let after = self
            .transcript
            .iter()
            .skip(index + 1)
            .filter(|message| message.role != "you")
            .count();
        let at = self.provenance.turns.len().checked_sub(after + 1)?;
        self.provenance.turns.get(at)
    }




    /// Resolve every source this conversation has gathered, without being asked.
    ///
    /// **No button.** Verifying a citation is work the app can do and the researcher cannot —
    /// it takes a network call per reference and a title comparison, and the answer is the same
    /// every time. Putting it behind a control asked them to request a check on data we had
    /// already decided to show them, which is the wrong way round: either it is worth verifying,
    /// in which case do it, or it is not, in which case do not offer it.
    ///
    /// Runs in the background as sources arrive, only for citations not already answered, so a
    /// turn that adds a reference resolves that one rather than all fourteen again.
    ///
    /// **What leaves the machine**, since this now happens on its own: a DOI, and — for a
    /// reference whose DOI is wrong or absent — the citation text, which is a reference to
    /// published work. Both go to `crossref.org` and nowhere else. Never the question, never the
    /// conversation, never a file.
    fn resolve_sources(&mut self, cx: &mut Context<Self>) {
        let mut wanted: Vec<(String, Option<String>, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for source in &self.sources {
            if self.checked.contains_key(&source.citation)
                || !seen.insert(source.citation.clone())
            {
                continue;
            }
            let link = link_for(source);
            // A corpus-id link needs no registry call: it was built from the id in the search
            // result (`overlay/minime_local/sources.py`), so there is nothing composed in it to
            // be wrong. Settled here, and settled as the *strongest* verdict rather than as
            // "nothing to check".
            if link.as_deref().is_some_and(references::is_corpus_link) {
                self.checked
                    .insert(source.citation.clone(), references::Verdict::FromSearch);
                continue;
            }
            wanted.push((
                source.citation.clone(),
                link.as_deref().and_then(references::doi_in),
                source.citation.clone(),
            ));
        }
        if wanted.is_empty() {
            return;
        }

        self.resolving += wanted.len();
        let mut results = self.sidecar.resolve_references(wanted);
        cx.spawn(async move |this, cx| {
            while let Some((key, verdict, repair)) = results.next().await {
                if this
                    .update(cx, |workbench, cx| {
                        workbench.resolving = workbench.resolving.saturating_sub(1);
                        workbench.checked.insert(key.clone(), verdict);
                        // Recorded either way. A row that stayed blank after being looked up
                        // looked exactly like one never looked up, which is how "found nothing"
                        // and "did nothing" became the same thing on screen.
                        workbench.repaired.insert(key, repair);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            // Nothing is announced. A toast per turn saying the references checked out is noise;
            // the row says what it found, and only a problem is worth an eye.
            let _ = this.update(cx, |workbench, cx| {
                workbench.resolving = 0;
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether an Asta-backed specialist actually ran in this conversation.
    ///
    /// **What the report footer's attribution should be decided by.** That footer reads
    /// *"Academic literature search performed using Asta tools (Allen Institute for AI)"*, and
    /// the backend's default test for it is `len(sources) > 0` — the number of citation objects
    /// the *model* emitted. Those are different claims. A run in which the model wrote five
    /// references from memory produces five sources, no Asta call, and a report crediting AI2
    /// for it (docs §119).
    ///
    /// Attribution is a claim about what happened, so it is answered from the record of what
    /// happened: the specialists the provenance record saw run, crossed with the ones the
    /// backend's own registry describes as using Asta.
    ///
    /// Conservative in the one direction that matters. An unreadable or absent registry means no
    /// specialist is known to use Asta, so nothing is credited — a missing acknowledgement is a
    /// thing a researcher can add, and a false one is a thing they have to retract.
    fn used_asta(&self) -> bool {
        let asta: std::collections::HashSet<String> = workspace::subagents()
            .into_iter()
            .filter(workspace::Subagent::uses_asta)
            .map(|subagent| subagent.name)
            .collect();
        if asta.is_empty() {
            return false;
        }
        self.provenance
            .road()
            .iter()
            .any(|stage| asta.contains(&stage.name))
    }

    /// Write the graph beside the conversation's own files, and open it.
    fn save_provenance_svg(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.thread_workspace() else {
            self.say("ask something first — there is no folder to save into yet", cx);
            return;
        };
        let graph = self.provenance.graph_of(self.provenance_turn);
        // Named for the turn it shows, so exporting turn 2 and then turn 3 gives two files
        // rather than one overwritten one.
        let name = match self.provenance_turn {
            Some(at) => format!("provenance-turn-{}.svg", at + 1),
            None => "provenance.svg".to_string(),
        };
        let path = dir.join(&name);
        if let Err(error) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&path, provenance_svg(&graph)))
        {
            self.say(format!("could not save {name}: {error}"), cx);
            return;
        }
        self.say(format!("saved {name} beside this conversation's files"), cx);
        if let Err(error) = workspace::open(&path) {
            tracing::warn!(%error, "could not open the provenance drawing");
        }
    }


    /// Fold the road, and remember that it is folded.
    fn toggle_road(&mut self, cx: &mut Context<Self>) {
        self.road_open = !self.road_open;
        self.remember_panels();
        cx.notify();
    }

    /// Persist which panels are open.
    ///
    /// Re-read from disk and written back rather than saving `self.draft`, which is the *Settings
    /// pane's* editing buffer: someone with half-typed changes in that pane who then folds a panel
    /// must not have those changes committed by the fold.
    fn remember_panels(&self) {
        let mut stored = settings::Settings::load();
        if stored.sidebar_open == self.sidebar_open
            && stored.panel_open == self.panel_open
            && stored.road_open == self.road_open
        {
            return;
        }
        stored.sidebar_open = self.sidebar_open;
        stored.panel_open = self.panel_open;
        stored.road_open = self.road_open;
        if let Err(error) = stored.save() {
            // Not a toast. The panel *did* fold; all that failed is remembering it for next
            // time, and a modal about that would be louder than the thing it reports.
            tracing::warn!(%error, "could not remember which panels are open");
        }
    }



    fn sync_transcript_list(&self) {
        let wanted = self.transcript.len() + usize::from(self.streaming);
        let present = self.transcript_list.item_count();
        if wanted > present {
            self.transcript_list.splice(present..present, wanted - present);
        } else if wanted < present {
            self.transcript_list.splice(wanted..present, 0);
        }
        // GPUI requires a splice when a measured row changes height. Only the in-flight answer
        // changes token by token, so finished rows stay cached (§156).
        if self.streaming && !self.transcript.is_empty() {
            let last = self.transcript.len() - 1;
            self.transcript_list.splice(last..last + 1, 1);
        }
    }

    fn invalidate_transcript_message(&self, index: usize) {
        if index < self.transcript_list.item_count() {
            self.transcript_list.splice(index..index + 1, 1);
        }
    }

    fn reset_transcript_list(&self) {
        self.transcript_list.reset(self.transcript.len());
    }

    fn invalidate_all_transcript_messages(&self) {
        let count = self.transcript.len().min(self.transcript_list.item_count());
        for index in 0..count {
            self.transcript_list.splice(index..index + 1, 1);
        }
    }

    /// Whether anything the researcher is waiting for is in flight.
    ///
    /// **One question, five sources.** The status bar's mark used to be shown for two of them —
    /// a streaming turn and a running setup fix — so the app was visibly busy exactly when it was
    /// least likely to be mistaken for stuck, and perfectly still through the fifteen-second graph
    /// build at launch (§176) and the pause while a conversation loads. Those are the waits that
    /// read as a hang (§177).
    fn is_waiting(&self) -> bool {
        self.streaming
            || self.running_fix.as_ref().is_some_and(|fix| !fix.done)
            || self.warming
            || self.opening
            || !self.conversations_loaded
    }


    /// Answer the pending approval and pump the continuation into the same turn.
    fn decide(&mut self, approve: bool, cx: &mut Context<Self>) {
        let Some(request) = self.pending_approval.take() else {
            return;
        };
        // Exactly one answer per held action, in the order they were presented — the agent
        // validates the count and errors out if it disagrees.
        let answers = protocol::Answer::all(&request, decision_for(approve));
        self.status = if approve {
            "approved — running…"
        } else {
            "rejected"
        }
        .into();

        let mut events = self.sidecar.resume(answers);
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |workbench, cx| {
                        workbench.apply(event, cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }


    // ---- Command palette ------------------------------------------------------

    /// The commands matching the current query, best first.
    fn palette_commands(&self, cx: &App) -> Vec<Command> {
        let query = self.palette_query.read(cx).text().to_string();
        let mut scored: Vec<(i32, usize, Command)> = Command::ALL
            .into_iter()
            .enumerate()
            .filter_map(|(index, command)| {
                match_score(&query, command.label()).map(|score| (score, index, command))
            })
            .collect();
        // Declaration order breaks ties, so an empty query lists the commands in the
        // order they are written rather than an arbitrary one.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, _, command)| command).collect()
    }

    fn toggle_palette(&mut self, _: &TogglePalette, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette_open {
            self.close_palette(window, cx);
            return;
        }
        self.palette_open = true;
        self.palette_selected = 0;
        self.palette_query
            .update(cx, |query, cx| query.set_text("", cx));
        let focus = self.palette_query.focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    /// Close the palette and hand focus back to the composer — otherwise focus would
    /// be left on a field that is no longer rendered and typing would go nowhere.
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        self.palette_query
            .update(cx, |query, cx| query.set_text("", cx));
        let focus = self.composer.focus_handle(cx);
        window.focus(&focus);
        cx.notify();
    }

    /// Which row is chosen, and what it would run.
    ///
    /// **One function for all three callers.** The row that was drawn as chosen, the row the
    /// arrow keys move from, and the command Enter runs each used to clamp `palette_selected`
    /// their own way — and the activation path did not clamp at all. So whenever the index
    /// outran a filtered list, the palette highlighted the last row and Enter ran either the
    /// wrong command or, past the end, nothing whatsoever (docs §69).
    fn palette_choice(&self, commands: &[Command]) -> Option<(usize, Command)> {
        let index = self.palette_selected.min(commands.len().checked_sub(1)?);
        Some((index, commands[index]))
    }

    fn move_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let commands = self.palette_commands(cx);
        let Some((current, _)) = self.palette_choice(&commands) else {
            return;
        };
        // Wrap, so `up` from the first row lands on the last.
        self.palette_selected =
            (current as isize + delta).rem_euclid(commands.len() as isize) as usize;
        cx.notify();
    }

    fn activate_palette(&mut self, cx: &mut Context<Self>) {
        let commands = self.palette_commands(cx);
        let Some((_, command)) = self.palette_choice(&commands) else {
            // Said out loud. This branch used to return in silence, which is
            // indistinguishable from a command that ran and did nothing — and that is
            // exactly how it was reported (docs §69).
            self.status = "no command matches what you typed".into();
            self.palette_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        };
        self.palette_open = false;
        self.palette_query
            .update(cx, |query, cx| query.set_text("", cx));
        self.restore_focus = true;
        self.run_command(command, cx);
        cx.notify();
    }

    // ---- Settings ---------------------------------------------------------------

    /// Escape: close the innermost thing that is open.
    ///
    /// One at a time and inside-out, which is what the key means everywhere else: from a
    /// file preview it returns to Settings if that was open behind it, not to nothing.
    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        // Innermost first, the rule §58 settled: a menu open over a modal closes the menu.
        if self.context_menu.take().is_some() {
            cx.notify();
            return;
        }
        if self.sidebar_menu.take().is_some() {
            cx.notify();
            return;
        }
        if self.open_picker.take().is_some() {
            cx.notify();
            return;
        }
        // Above the preview because it paints above it, and here at all because the scoped
        // `PaletteDismiss` binding cannot fire — see `workbench_key_bindings`. The footer has
        // promised "esc close" the whole time (docs §84).
        if self.palette_open {
            self.close_palette(window, cx);
            return;
        }
        if self.confirming_delete.take().is_some() {
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.sources_open {
            self.sources_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.datasets_open {
            self.datasets_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.documents_open {
            self.documents_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.commands_open {
            self.commands_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.claims_open {
            self.claims_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.confirming_provider.take().is_some() {
            // Escape leaves the provider as it was: this modal exists precisely so the change
            // needs a deliberate press, and dismissing is not one.
            cx.notify();
            return;
        }
        if self.preview.take().is_some() {
            cx.notify();
            return;
        }
        if self.renaming.take().is_some() {
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.editing_mission {
            // Escape abandons the edit and leaves the stored mission alone — nothing was sent
            // until Enter, so there is nothing to undo.
            self.editing_mission = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.about_open {
            self.about_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.provenance_open {
            self.provenance_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        if self.settings_open {
            // Same as Close: an unsaved palette was a look, not a change.
            let saved = settings::Settings::load();
            self.applied_theme = saved.theme.clone();
            settings::apply_theme(&saved);
            self.settings_open = false;
            self.restore_focus = true;
            cx.notify();
        }
    }

    fn toggle_settings(&mut self, _: &ToggleSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.settings_open = false;
            self.restore_focus = true;
            cx.notify();
            return;
        }
        self.open_settings(Some(window), cx);
    }

    /// Load the stored settings into the draft and show the pane.
    ///
    /// Secret fields open **empty**: what is in the keychain is never read back into the
    /// UI. The row says "stored" or "not set" instead, and leaving a field blank on save
    /// keeps whatever is already there — so changing your model does not mean re-pasting
    /// your key.
    fn open_settings(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        self.draft = settings::Settings::load();
        // The *live* palette, not the saved one. Reloading the whole draft used to reset
        // the screen to whatever was last saved the instant the pane opened, so clicking
        // the mark at top-left threw away the theme being looked at (docs §50).
        self.draft.theme = self.applied_theme.clone();
        self.settings_note.clear();
        // Opened by the keyboard or the palette rather than from Setup, so it lands on the
        // page most people came for. Reaching Setup is now one click in the rail.
        if self.settings_section == Section::Setup {
            self.settings_section = Section::Model;
        }
        // The one moment a fresh list is worth a request: somebody is about to read it.
        self.refresh_models(cx);
        let values: Vec<(Field, String)> = self
            .fields
            .iter()
            .map(|(field, _)| {
                let value = match field {
                    Field::ModelId => self.draft.model_id.clone(),
                    Field::BaseUrl => self.draft.base_url.clone(),
                    Field::Port => self.draft.backend_port.to_string(),
                    _ => String::new(),
                };
                (*field, value)
            })
            .collect();
        for ((_, composer), (_, value)) in self.fields.iter().zip(values) {
            composer.update(cx, |composer, cx| composer.set_text(value, cx));
        }
        self.settings_open = true;
        if let Some(window) = window {
            self.focus_settings_page(window, cx);
        }
        cx.notify();
    }

    fn field_text(&self, field: Field, cx: &App) -> String {
        self.fields
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, composer)| composer.read(cx).text().trim().to_string())
            .unwrap_or_default()
    }

    /// Write the draft: settings to `settings.toml`, secrets to the OS keychain.
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        self.draft.model_id = self.field_text(Field::ModelId, cx);
        self.draft.base_url = self.field_text(Field::BaseUrl, cx);
        let port = self.field_text(Field::Port, cx);
        if let Ok(port) = port.parse::<u16>() {
            self.draft.backend_port = port;
        } else if !port.is_empty() {
            self.settings_note = format!("{port:?} is not a port number — not saved.");
            cx.notify();
            return;
        }

        let mut stored = Vec::new();
        // Only non-empty fields are written, so a blank field means "leave it alone"
        // rather than "delete my key".
        let key_name = format!("llm:{}", self.key_target);
        let secrets: Vec<(String, String)> = self
            .fields
            .iter()
            .filter(|(field, _)| field.is_secret())
            .filter_map(|(field, composer)| {
                let value = composer.read(cx).text().trim().to_string();
                if value.is_empty() {
                    return None;
                }
                let name = field
                    .secret_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| key_name.clone());
                Some((name, value))
            })
            .collect();
        for (name, value) in &secrets {
            match settings::set_secret(name, value) {
                Ok(()) => stored.push(name.clone()),
                Err(error) => {
                    // Say which keychain failed, not the value.
                    self.settings_note = format!("Could not store {name}: {error:#}");
                    cx.notify();
                    return;
                }
            }
        }

        if let Err(error) = self.draft.save() {
            self.settings_note = format!("Could not write settings: {error:#}");
            cx.notify();
            return;
        }

        // Clear the secret fields once written — nothing is gained by leaving a key on
        // screen, and the row now reports it as stored.
        for (field, composer) in &self.fields {
            if field.is_secret() {
                composer.update(cx, |composer, cx| composer.set_text("", cx));
            }
        }

        // The model takes effect on the next turn, because the backend resolves it per
        // request. The port and execution locality are baked into the launch command, so
        // those need a restart — say so rather than letting the user wonder.
        self.sidecar.set_model(model_choice(&self.draft));
        let mut note = String::from("Saved.");
        if !stored.is_empty() {
            note.push_str(&format!(" Stored: {}.", stored.join(", ")));
        }
        note.push_str(" Model applies to the next turn; port and execution need a restart.");
        self.settings_note = note;
        // A toast as well as the note: the note lives inside a window the user is about to
        // close, and "did that save?" is the question this whole pane exists to answer.
        self.say(
            if stored.is_empty() {
                "settings saved".to_string()
            } else {
                format!("settings saved — stored {}", stored.join(", "))
            },
            cx,
        );
        // Saving is finishing. Leaving the window up after it put a note inside a pane the user
        // had already decided to leave, so the only visible answer to "did that work?" was a
        // sentence they had to go back and look for — which is exactly what the toast above
        // exists to avoid (docs §88).
        self.settings_open = false;
        self.restore_focus = true;
        cx.notify();
    }

    /// The Settings pane, in place of the artifacts panel.
    /// The preferences window: rail on the left, the chosen page on the right.
    ///
    /// Every page goes through here, so the frame, the scrolling and the pinned actions are
    /// decided once. `ui::Modal` is what enforces that the actions cannot end up inside the
    /// part that scrolls.
    fn preferences_window(
        &self,
        body: impl IntoElement,
        actions: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let current = self.settings_section;
        let mut rail = ui::nav_rail();
        for section in Section::ALL {
            rail = rail.child(
                ui::NavEntry::new(section.id(), section.label(), section == current).on_click(
                    cx.listener(move |workbench, _event, window, cx| {
                        workbench.settings_section = section;
                        // The field that had focus may not exist on the new page, and focus on
                        // an unrendered element stops key bindings arriving (docs §71).
                        workbench.focus_settings_page(window, cx);
                        // Landing on Setup should show what is true *now*: a stale report is
                        // the one thing worse than none (the reason `open_setup` re-checks).
                        if section == Section::Setup {
                            workbench.run_preflight(cx);
                        }
                        cx.notify();
                    }),
                ),
            );
        }

        let mut footer = div().flex().flex_col().gap_1();
        // What is still missing, before the user finds out from a failed turn. Shown on every
        // page, because the page you are on is rarely the one with the problem.
        let has_key = settings::secret(&self.draft.key_name()).is_some()
            || !self.field_text(Field::ApiKey, cx).is_empty();
        for problem in self.draft.problems(has_key) {
            footer = footer.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::error()))
                    .text_xs()
                    .child(problem),
            );
        }
        if !self.settings_note.is_empty() {
            footer = footer.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(self.settings_note.clone()),
            );
        }
        footer = footer.child(
            div()
                .text_color(rgb(theme::text_faint()))
                .text_xs()
                .child(format!(
                    "Keys live in your OS keychain, never in a file. {}",
                    settings::settings_path().display()
                )),
        );

        ui::Modal::new("settings", "SETTINGS")
            .focus(&self.settings_focus)
            // Wider than the 520px column it replaces: the rail takes 150 of it, and the
            // Setup page has a check, a reason and two buttons to fit on a line.
            .width(760.)
            .nav(rail)
            .body(body)
            .actions(actions)
            .footer(footer)
            .into_any_element()
    }

    /// Put the keyboard somewhere that exists on the page being shown.
    ///
    /// It used to focus `fields.first()` unconditionally — which is the Model page's first
    /// field. On Appearance or Setup that element is not rendered at all, and focus sitting on
    /// something that is not on screen means **key bindings stop arriving**: Escape did nothing
    /// until you clicked a page that happened to contain that field (docs §71).
    ///
    /// Pages with no fields focus the window itself, so Escape always has somewhere to come
    /// from.
    fn focus_settings_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let section = self.settings_section;
        let field = self
            .fields
            .iter()
            .find(|(field, _)| field.section() == section)
            .map(|(_, composer)| composer.focus_handle(cx));
        window.focus(&field.unwrap_or_else(|| self.settings_focus.clone()));
    }




    fn run_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            // Same path as Enter in the composer and as the Send button.
            Command::RunTurn => self
                .composer
                .update(cx, |composer, cx| composer.submit_now(cx)),
            Command::NewThread => {
                // Ordinary New always means the root workspace. Starting within a project has
                // its own explicit `+` beside that heading; inheriting the open/remembered project
                // made conversations start already filed and revived deleted headings (§154).
                //
                // `new_thread_in` clears `tasks` and `jobs` as well, which is the §159 fix: this
                // path used to leave the previous conversation's pending approvals on screen and
                // clickable, and answering one then named the wrong conversation as the worker's
                // owner. Both routes now clear the same state because there is only one route.
                self.new_thread_in(None, cx);
            }
            Command::RefreshSpine => {
                self.refresh_project(cx);
                self.status = "refreshing the project spine…".into();
            }
            Command::ExpandTraces => self.set_all_traces_expanded(true),
            Command::CollapseTraces => self.set_all_traces_expanded(false),
            Command::CopyLastAnswer => {
                let answer = self
                    .transcript
                    .iter()
                    .rev()
                    .find(|message| message.role == "mini-me" && !message.body.is_empty())
                    .map(|message| message.body.clone());
                match answer {
                    Some(answer) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(answer));
                        self.say("last answer copied", cx);
                    }
                    None => self.status = "no answer to copy yet".into(),
                }
            }
            // Both reachable by keyboard already; here so they can be *found*, which for a
            // reader who has never met this app is the difference between a feature that
            // exists and one that does not.
            // Whether work blocks is a property of the work, not something to encode in
            // punctuation — so background dispatch is a command rather than a `/name!` syntax
            // nobody would discover (docs §77).
            Command::RestartBackend => self.restart_backend(cx),
            Command::SpecialistInBackground => {
                let typed = self.composer.read(cx).text().trim().to_string();
                if subagent::parse(&typed).is_none() {
                    self.say("type /name and what it should do first", cx);
                    return;
                }
                self.composer
                    .update(cx, |composer, cx| composer.set_text("", cx));
                self.start_turn_as(typed, subagent::Dispatch::Background, cx);
            }
            Command::CopySelected => self.copy_selected_text(cx),
            Command::SelectWhole => self.select_whole_transcript(cx),
            Command::RenderReport => self.render_report(cx),
            Command::FileInProject => {
                if self.sidecar.thread_id().is_none() {
                    self.say(
                        "ask something first — there is no conversation to file yet",
                        cx,
                    );
                    return;
                }
                // Anchored under the sidebar, where the projects it is about are listed.
                self.open_picker = Some((Picker::Project, gpui::point(px(24.), px(120.))));
                cx.notify();
            }
            Command::OpenAbout => {
                self.about_open = true;
                cx.notify();
            }
            Command::OpenProvenance => {
                self.provenance_open = true;
                cx.notify();
            }
            Command::OpenSettings => self.open_settings(None, cx),
            Command::OpenSetup => self.open_setup(cx),
            Command::Quit => cx.quit(),
        }
    }

    fn set_all_traces_expanded(&mut self, expanded: bool) {
        for message in &mut self.transcript {
            message.steps_expanded = expanded;
            for trace in &mut message.agents {
                trace.expanded = expanded;
            }
        }
        self.invalidate_all_transcript_messages();
    }




    /// One line for the status bar: what is being worked on, and how far through.
    ///
    /// **An unfinished worker outranks the conversation's own plan**, because a worker is the thing
    /// running while nobody is looking — the conversation's plan belongs to a turn the researcher
    /// is already watching. With several workers the busiest wins; a column of them belongs in the
    /// panel, not in a single line.
    ///
    /// `None` when there is nothing with a plan, which leaves the bar exactly as it was. This line
    /// adds a denominator to a wait; it does not become another thing always on screen.
    fn work_summary(&self) -> Option<String> {
        summary_for(&self.tasks, &self.plan)
    }







    /// Long jobs still running, and the ones that finished this session.
    ///
    /// The theorizer and DataVoyager return a task id immediately and finish minutes
    /// later, so without this the answer to "is it still going?" was nothing at all —
    /// and, worse, nobody was collecting the result (docs §29).
    ///
    /// **Foldable, and bounded when open.** Asked for directly: *"we need to make the ui of
    /// background jobs plegable and scrolable. when we have a lot of text like for data voyager
    /// its disruptive at sight."* A job row carries the question it was launched with, and a
    /// DataVoyager question is a paragraph *by design* — the four rules in `subagents.py` make it
    /// name the datasets, name the methods and ask for the numbers — so one row could fill the
    /// column and push `OUTPUTS` and `SOURCES` off the bottom of it. Three bounds, cheapest
    /// first: the question is clipped to [`JOB_QUESTION_CHARS`], the list scrolls within
    /// [`JOBS_BODY_HEIGHT`], and the section folds to a single line.
    ///
    /// **What folds is the noise, not the question being asked of you.** A worker stopped at the
    /// approval gate is pinned above the scroller, because its Approve button is the one control
    /// in this section that something is *waiting on* — the rule the card itself follows (§40),
    /// one level out. The gate also opens the section when it appears, so a fold can hide it only
    /// by a deliberate press; folded, the heading still reads `1 waiting for you`.
    ///
    /// **Two call sites, one per frame.** `artifacts_contents` renders this either from its
    /// no-spine early return or from the full panel, never both, so the fixed element ids below
    /// cannot collide the way two sibling `datasets-heading`s once did.
    /// Raise a desktop notification, but only for somebody who is not looking.
    ///
    /// The one thing in this app that can reach a researcher who switched to Excel. Three callers,
    /// all of them the same kind of event: long work that ended, and long work that stopped needing
    /// a decision. Suppressed when the window is active, because the banner and the jobs row are
    /// already saying it there and a toast on top would be the third voice.
    fn notify_if_away(&self, title: &str, body: &str) {
        if !notify::worth_interrupting(self.window_active) {
            return;
        }
        notify::toast(title, body);
    }

    /// Record a discovery run's status when a poll observes it changing.
    ///
    /// Sends no budget: `n_experiments` was written at approval and is not this call's business —
    /// a status update that also restated the budget would be a second chance to get it wrong.
    fn record_discovery_status(&mut self, run_id: String, status: &str, cx: &mut Context<Self>) {
        let mut answer = self
            .sidecar
            .discovery_status_changed(run_id.clone(), status.to_string());
        cx.spawn(async move |_workbench, _cx| {
            if let Some(Err(error)) = answer.next().await {
                tracing::warn!(
                    %error,
                    run = %run_id,
                    "could not record a discovery run's status; the row will be corrected by a poll"
                );
            }
        })
        .detach();
    }

    /// Tell the conversation's own record that an approved run is now running.
    ///
    /// Fire-and-forget with a warning on failure: the run is already paying for itself on Asta and
    /// nothing here can undo that, so a state update that does not land must not look like a failed
    /// submit. The cost of losing it is a row missing after a reload, which is what §258 was.
    fn record_discovery_started(
        &mut self,
        run_id: String,
        experiments: u32,
        status: &str,
        cx: &mut Context<Self>,
    ) {
        let mut answer =
            self.sidecar
                .discovery_started(run_id.clone(), experiments, status.to_string());
        cx.spawn(async move |_workbench, _cx| {
            if let Some(Err(error)) = answer.next().await {
                tracing::warn!(
                    %error,
                    run = %run_id,
                    "could not record that a discovery run started; it is running regardless"
                );
            }
        })
        .detach();
    }

    /// Open a finished discovery run and fetch its experiments.
    ///
    /// One request for the whole tree — §247 established that the experiments endpoint returns
    /// every node complete, so the graph does not cost a call per node. Figures are not fetched
    /// here; they live only in the per-experiment response and are worth ~458KB each.
    fn open_discovery(&mut self, run_id: String, name: String, cx: &mut Context<Self>) {
        tracing::info!(run = %run_id, "opening a discovery run");
        // **Ours first.** The poll route writes `discovery/<run_id>.json` into this conversation's
        // folder the moment a run finishes, because the service forgets its datasets after a week
        // (§247). So a finished run is already on disk, and asking the service again on every open
        // was a wait for something we owned (§261).
        let stored = self
            .thread_workspace()
            .and_then(|folder| workspace::discovery_record(&folder, &run_id));
        if let Some(record) = stored {
            let experiments = discovery::decode_experiments(&record);
            if !experiments.is_empty() {
                tracing::info!(
                    run = %run_id,
                    experiments = experiments.len(),
                    "read a discovery run from this conversation's folder"
                );
                self.discovery_open = Some(DiscoveryView {
                    run_id,
                    name,
                    experiments,
                    selected: None,
                    expanded: false,
                    loudest_first: true,
                    figures: std::collections::HashMap::new(),
                    fetching: None,
                    loading: false,
                    // The file only exists once the run reached a terminal state, so its presence
                    // *is* the completeness — no need to ask.
                    complete: true,
                    error: None,
                });
                cx.notify();
                return;
            }
        }

        self.discovery_open = Some(DiscoveryView {
            run_id: run_id.clone(),
            name,
            experiments: Vec::new(),
            selected: None,
            expanded: false,
            loudest_first: true,
            figures: std::collections::HashMap::new(),
            fetching: None,
            loading: true,
            complete: false,
            error: None,
        });
        let mut answer = self.sidecar.discovery_run(run_id.clone());
        cx.spawn(async move |workbench, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = workbench.update(cx, |workbench, cx| {
                    let Some(view) = workbench.discovery_open.as_mut() else {
                        return;
                    };
                    // A different run may have been opened while this was in flight.
                    if view.run_id != run_id {
                        return;
                    }
                    view.loading = false;
                    match outcome {
                        Ok(payload) => {
                            view.experiments = discovery::decode_experiments(&payload);
                            view.complete = discovery::finished(&payload);
                            tracing::info!(
                                run = %run_id,
                                experiments = view.experiments.len(),
                                "read a discovery run"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(%error, run = %run_id, "could not read a discovery run");
                            view.error = Some(error);
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// Open an experiment, or go back to the list.
    ///
    /// One path for the node, the row and the back button, so opening an experiment always fetches
    /// its figures — the first version had three call sites and only text to show, which is how the
    /// figures route came to exist without a caller (§257).
    fn select_experiment(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let wanted = self.discovery_open.as_mut().and_then(|view| {
            view.selected = at;
            at.and_then(|at| view.experiments.get(at))
                .map(|experiment| experiment.id.clone())
        });
        if let Some(experiment_id) = wanted {
            self.fetch_figures(experiment_id, cx);
        }
        cx.notify();
    }

    /// Fetch one experiment's figures, once.
    ///
    /// Called when an experiment is opened rather than when the run is read: this is the expensive
    /// endpoint, and fetching all of them up front would be megabytes for plots nobody looked at.
    fn fetch_figures(&mut self, experiment_id: String, cx: &mut Context<Self>) {
        let Some(view) = self.discovery_open.as_mut() else {
            return;
        };
        // Already have them, or already asking. An empty vec counts as having them.
        if view.figures.contains_key(&experiment_id) || view.fetching.as_deref() == Some(&experiment_id) {
            return;
        }
        let run_id = view.run_id.clone();

        // **On disk already, for a finished run.** The poll route decodes every experiment's plots
        // when the run ends (§263), so the ordinary case needs no request at all — which is what
        // was asked for: *"maybe we can improve this logic."*
        let stored = self.thread_workspace().map(|folder| {
            workspace::discovery_figures(&folder, &run_id, &experiment_id)
        });
        if let Some(paths) = stored.filter(|paths| !paths.is_empty()) {
            tracing::info!(
                run = %run_id,
                experiment = %experiment_id,
                figures = paths.len(),
                "read an experiment's figures from this conversation's folder"
            );
            if let Some(view) = self.discovery_open.as_mut() {
                view.figures.insert(experiment_id, paths);
            }
            cx.notify();
            return;
        }

        let Some(view) = self.discovery_open.as_mut() else {
            return;
        };
        view.fetching = Some(experiment_id.clone());

        let mut answer = self
            .sidecar
            .discovery_figures(run_id.clone(), experiment_id.clone());
        cx.spawn(async move |workbench, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = workbench.update(cx, |workbench, cx| {
                    let Some(view) = workbench.discovery_open.as_mut() else {
                        return;
                    };
                    if view.run_id != run_id {
                        return; // a different run was opened while this was in flight
                    }
                    if view.fetching.as_deref() == Some(experiment_id.as_str()) {
                        view.fetching = None;
                    }
                    match outcome {
                        Ok(paths) => {
                            tracing::info!(
                                run = %run_id,
                                experiment = %experiment_id,
                                figures = paths.len(),
                                "decoded an experiment's figures"
                            );
                            // Recorded even when empty, so "none" stops looking like "loading".
                            view.figures.insert(experiment_id.clone(), paths);
                        }
                        Err(error) => {
                            // Deliberately not recorded: an absent key means "ask again", and
                            // caching a failure as an empty list would turn a transient error into
                            // an experiment that permanently drew nothing (§260).
                            tracing::warn!(
                                %error,
                                experiment = %experiment_id,
                                "could not decode an experiment's figures"
                            );
                            view.error = Some(format!("could not read figures: {error}"));
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// The colours a node is drawn in, from how far it moved a belief.
    ///
    /// Returns `(fill, ink, border)`. Three bands rather than a ramp, because the scale of
    /// `surprise` is not documented and a gradient over it would imply precision nobody has.
    fn node_colours(&self, experiment: &discovery::Experiment) -> (u32, u32, u32) {
        // A pure mapping from the band to three colours. The *decision* — that status outranks the
        // score — lives in `discovery::loudness`, where it can be tested without a window.
        match discovery::loudness(experiment) {
            discovery::Loudness::Running => {
                (theme::surface(), theme::text_muted(), theme::running())
            }
            discovery::Loudness::Failed => {
                (theme::surface(), theme::text_muted(), theme::error())
            }
            discovery::Loudness::Loud => {
                let fill = theme::accent();
                (fill, theme::ink_on(fill), fill)
            }
            discovery::Loudness::Middling => (
                theme::surface(),
                theme::text(),
                theme::border_strong(),
            ),
            discovery::Loudness::Quiet => {
                (theme::surface(), theme::text_faint(), theme::border())
            }
        }
    }


    /// How much of an experiment's hypothesis a list row shows.
    ///
    /// The hypothesis is a sentence the service wrote to be read whole, so the row shows enough to
    /// tell two apart and the detail below shows all of it.
    const HYPOTHESIS_CHARS: usize = 90;




    /// Adopt the background work a snapshot describes: drafts, long jobs, and workers.
    ///
    /// **One method, because there are two places a snapshot arrives and they drifted.** A live
    /// turn's `values` event is one; opening a conversation and reading its stored state is the
    /// other. The streaming path called `open_approval` and the opening path did not, so a drafted
    /// run was offered — or, after §258, repaired — only while a turn was running. Reopen the
    /// conversation and the drafted run was decoded, dropped from the job list because
    /// `awaiting_approval` is not a job, and adopted by nobody: *"I still cant see the ui for the
    /// background job"* (§259).
    ///
    /// Drafts first, because a drafted run is not among the jobs — it is a question, and one that
    /// must not be buried under the rows of things already running.
    fn adopt_background_work(
        &mut self,
        drafts: &[protocol::Draft],
        jobs: Vec<protocol::Job>,
        tasks: Vec<protocol::AsyncTask>,
        owner: &str,
        cx: &mut Context<Self>,
    ) {
        self.open_approval(drafts, cx);
        for job in jobs {
            self.track_job(job, cx);
        }
        for task in tasks {
            // Into the provenance record as well as the Jobs panel. A background worker runs on
            // its own LangGraph thread, so none of its events reach this conversation's stream —
            // the `async_tasks` map is the only trace on this side, and the record had never been
            // told about it. Which is why the graph showed nothing for work a researcher had
            // explicitly handed off (docs §111).
            self.provenance.observe_background(
                &format!("async:{}", task.task_id),
                &task.agent_name,
                provenance::now_ms(),
            );
            self.track_task(owner, task, cx);
        }
    }

    /// Open the budget gate for a run that is drafted and unspent.
    ///
    /// Called from a snapshot rather than from a press, because the researcher never asked for
    /// this modal — the agent drafted a run and something has to ask them whether to pay for it.
    /// Ignores a run they already declined, and never replaces a gate already on screen: a second
    /// draft arriving mid-decision must not move the button under the pointer.
    fn open_approval(&mut self, drafts: &[protocol::Draft], cx: &mut Context<Self>) {
        if self.approving.is_some() {
            return;
        }
        let Some(draft) = drafts
            .iter()
            .find(|draft| !self.declined.contains(&draft.run_id))
        else {
            return;
        };
        // **Ask the service before asking the researcher.** The gate used to open immediately and
        // fill in the cost when it arrived, which was fine until a run could already have been
        // approved — and one can, because the artifact saying `awaiting_approval` is a turn's
        // record and approving is not a turn. Offering to spend credits on a finished run is worse
        // than a second of delay, and the answer brings the cost and the token with it, so the
        // modal now opens complete or not at all (§258).
        let draft = draft.clone();
        let mut answer = self.sidecar.discovery_draft(draft.run_id.clone());
        cx.spawn(async move |workbench, cx| {
            let Some(outcome) = answer.next().await else {
                return;
            };
            let _ = workbench.update(cx, |workbench, cx| {
                // Something else may have opened a gate while this was in flight.
                if workbench.approving.is_some() {
                    return;
                }
                let cost = match outcome {
                    Ok(cost) => cost,
                    Err(error) => {
                        // Cannot tell whether it was approved, so do not ask for money. The draft
                        // keeps its status and the next snapshot tries again.
                        tracing::warn!(
                            %error,
                            run = %draft.run_id,
                            "could not check a drafted run; not offering the gate"
                        );
                        return;
                    }
                };
                if cost.submitted {
                    // Already running or finished. Adopt it as a job so it appears where a
                    // researcher expects, and never ask about its budget again.
                    tracing::info!(
                        run = %draft.run_id,
                        "a drafted run had already been approved; tracking it instead of asking"
                    );
                    workbench.declined.insert(draft.run_id.clone());
                    // What the service says it *is*, not an assumption that it is running. A
                    // finished run adopted as "running" showed "usually 25–40 min" until the first
                    // poll corrected it, on a row somebody was already reading (§260).
                    let status = if cost.status.is_empty() {
                        "running".to_string()
                    } else {
                        cost.status.clone()
                    };
                    workbench.track_job(
                        protocol::Job {
                            kind: protocol::JobKind::Discovery,
                            task_id: draft.run_id.clone(),
                            question: draft.name.clone(),
                            context_id: None,
                            status: status.clone(),
                            size: Some(u64::from(cost.experiments)),
                        },
                        cx,
                    );
                    // And correct the record, so the next launch does not have to ask again.
                    workbench.record_discovery_started(
                        draft.run_id.clone(),
                        cost.experiments.max(1),
                        &status,
                        cx,
                    );
                    cx.notify();
                    return;
                }

                tracing::info!(
                    run = %draft.run_id,
                    experiments = draft.experiments,
                    "asking the researcher to approve a discovery budget"
                );
                // Whatever the agent drafted, clamped into what the service will accept — so the
                // number on screen is always one that can actually be submitted.
                let experiments = opening_budget(draft.experiments);
                workbench.intent_field.update(cx, |field, cx| {
                    field.set_text(draft.intent.clone(), cx);
                });
                workbench.approving = Some(Approval {
                    draft: draft.clone(),
                    experiments,
                    cost: Some(cost),
                    error: None,
                    submitting: false,
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Ask the backend what this run costs, and for the token that authorises submitting it.
    ///
    /// Called when the gate opens and again after a failed submit. The modal renders without the
    /// answer — the run's own experiment count is the price either way — so a slow lookup delays
    /// the balance and the press, never the reading.
    fn refresh_approval(&mut self, run_id: String, cx: &mut Context<Self>) {
        let mut answer = self.sidecar.discovery_draft(run_id.clone());
        cx.spawn(async move |workbench, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = workbench.update(cx, |workbench, cx| {
                    let Some(approval) = workbench.approving.as_mut() else {
                        return;
                    };
                    // The gate may have been answered and reopened for a different run while this
                    // was in flight; a balance belonging to another run would be a wrong number
                    // next to a price, and a token for another run would be refused.
                    if approval.draft.run_id != run_id {
                        return;
                    }
                    match outcome {
                        Ok(cost) => approval.cost = Some(cost),
                        Err(error) => {
                            tracing::warn!(%error, "could not read the discovery credit balance");
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Spend the credits. The only place in this app that does.
    fn approve_discovery(&mut self, cx: &mut Context<Self>) {
        let Some(approval) = self.approving.as_mut() else {
            return;
        };
        if approval.submitting {
            return; // the press already landed; a second one would be a second charge
        }
        approval.submitting = true;
        approval.error = None;
        let run_id = approval.draft.run_id.clone();
        let experiments = approval.experiments;
        let kind = approval.draft.name.clone();
        let intent = self.intent_field.read(cx).text().trim().to_string();
        // The token the draft lookup issued. Without it the submit is refused — which is the point,
        // and also why a modal opened before the lookup answered cannot be pressed (§252).
        let Some(approval) = approval
            .cost
            .as_ref()
            .map(|cost| cost.approval.clone())
            .filter(|token| !token.is_empty())
        else {
            approval.submitting = false;
            approval.error = Some(
                "still checking the cost with the service — try again in a moment".to_string(),
            );
            cx.notify();
            return;
        };
        tracing::info!(run = %run_id, experiments, "approving a discovery run");

        let mut answer = self
            .sidecar
            .submit_discovery(run_id.clone(), approval, experiments, intent);
        cx.spawn(async move |workbench, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = workbench.update(cx, |workbench, cx| {
                    match outcome {
                        Ok(()) => {
                            workbench.approving = None;
                            workbench.status =
                                format!("{kind} running in the background ({} experiments)", experiments);
                            // Start watching it immediately rather than waiting for the next
                            // turn's snapshot: the researcher just paid for it, and a run with no
                            // row is a run they cannot see.
                            workbench.track_job(
                                protocol::Job {
                                    kind: protocol::JobKind::Discovery,
                                    task_id: run_id.clone(),
                                    question: kind.clone(),
                                    context_id: None,
                                    status: "running".to_string(),
                                    size: Some(u64::from(experiments)),
                                },
                                cx,
                            );
                            // **And write it down.** The line above is why the row appears in
                            // *this* session; this is why it is still there after a reload. The
                            // artifact is written by a turn and approval is not a turn, so without
                            // this it says `awaiting_approval` forever and `decode_jobs` — which
                            // skips that by design — drops a run that is genuinely working (§258).
                            workbench.record_discovery_started(
                                run_id.clone(),
                                experiments,
                                "running",
                                cx,
                            );
                        }
                        Err(error) => {
                            tracing::warn!(%error, run = %run_id, "a discovery submit failed");
                            let still_open = workbench
                                .approving
                                .as_ref()
                                .is_some_and(|open| open.draft.run_id == run_id);
                            if let Some(approval) = workbench.approving.as_mut() {
                                approval.submitting = false;
                                approval.error = Some(error);
                                // The token is gone from this side's point of view whether or not
                                // the backend spent it, so the modal re-fetches one. Without this
                                // a recoverable failure — a configuration change that did not
                                // save — left the next press answering "this submit carries no
                                // valid approval", which is a dead end rather than a retry (§255).
                                approval.cost = None;
                            }
                            if still_open {
                                workbench.refresh_approval(run_id.clone(), cx);
                            }
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }





    /// Research outputs from the current run, grouped by kind.
    ///
    /// Fed by the `values` stream event, so it fills in as a turn produces papers,
    /// datasets, theories and reports — not only at the end.
    /// What this conversation has cited, numbered.
    ///
    /// `[n]` in the accent because a source is something you can act on — it opens. The number is
    /// the same one the agent's prose uses, so `[3]` in an answer and `[3]` here are the same
    /// paper without the researcher having to match titles.
    ///
    /// Whole strings, not split into title and venue: `Snapshot::sources` carries a citation as
    /// one line of the agent's own text, in whatever form it wrote it. Splitting it into
    /// `Plant Pathology · 2021 · CIP Dataverse` would mean parsing prose into fields and being
    /// confidently wrong about some of them — a bibliography that quietly mis-attributes is worse
    /// than one that is merely plain.
    /// Where each source came from, positionally — one entry per `self.sources`.
    ///
    /// **One function, three readers.** The header's count, the note under a row and the `annote`
    /// in an exported `.bib` all have to mean the same thing by "unverified", and three
    /// re-derivations of that from two maps is three chances to drift. Positional so the export
    /// can zip it against the same slice it is already walking.
    fn source_origins(&self) -> Vec<references::Origin> {
        self.sources
            .iter()
            .map(|source| {
                references::origin(
                    self.checked.get(&source.citation),
                    self.repaired.get(&source.citation).map(Option::is_some),
                )
            })
            .collect()
    }

    /// How many references nothing has confirmed — the number a subject-matter expert owns.
    fn unverified_sources(&self) -> usize {
        self.source_origins()
            .into_iter()
            .filter(|origin| origin.needs_a_human())
            .count()
    }

    /// Open the dataset list, and start asking the server about each one.
    ///
    /// Checked on open rather than on arrival: a turn can recommend dozens, and asking about
    /// datasets nobody has looked at would be a burst of requests to CIP for nothing. Opening the
    /// list is the moment the answers become worth having.
    fn open_datasets(&mut self, cx: &mut Context<Self>) {
        // Both halves, because "the modal is not working" has two causes and they need telling
        // apart: the press never arriving, and the press arriving with nothing to show.
        tracing::info!(
            datasets = self.datasets.len(),
            "opening the dataset list"
        );
        self.datasets_open = true;
        for id in self
            .datasets
            .iter()
            .map(|dataset| dataset.persistent_id.clone())
            .collect::<Vec<_>>()
        {
            self.check_access(id, cx);
        }
        cx.notify();
    }

    /// Ask the server what a dataset holds, once, when the row first needs to know.
    ///
    /// **Not from `file_access_summary`.** That field exists on `DataVerseFindings` and is prose a
    /// model wrote; the search results carry no access field at all. Whether a researcher may
    /// have these files is the server's answer, and gating a download on a sentence would be the
    /// same mistake as trusting a citation nobody checked (docs §223).
    fn check_access(&mut self, persistent_id: String, cx: &mut Context<Self>) {
        if self.dataset_access.contains_key(&persistent_id)
            || !self.checking_access.insert(persistent_id.clone())
        {
            return;
        }
        let mut answer = self.sidecar.dataset_access(persistent_id.clone());
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.checking_access.remove(&persistent_id);
                    workbench.dataset_access.insert(persistent_id, outcome);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Fetch a dataset into this conversation's folder.
    ///
    /// Which is the sandbox's working directory as well, so the archive is both something the
    /// researcher can open in Explorer and something the analysis subagents can read — the whole
    /// reason this is a button rather than a tool the model calls.
    /// Tick or untick one dataset.
    fn toggle_dataset_pick(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.dataset_picks.remove(&id) {
            self.dataset_picks.insert(id);
        }
        cx.notify();
    }

    /// The ticked datasets that can still be fetched, **in the order the list shows them**.
    ///
    /// Read off `self.datasets` rather than out of the set, so the button's count and the order
    /// things arrive both match what is on screen. A pick whose file already landed, or whose
    /// download is in flight, is not offered again — the count would otherwise promise work that
    /// will not happen.
    fn picked_datasets(&self) -> Vec<protocol::Dataset> {
        still_fetchable(
            &self.datasets,
            &self.dataset_picks,
            &self.downloaded,
            &self.downloading,
        )
    }

    /// Fetch every ticked dataset.
    ///
    /// One call per dataset rather than a batch route: `download_dataset` already guards its own
    /// id against a second start, reports its own failure, and refreshes Outputs when a file
    /// lands. A batch would have to reimplement all three and would report one outcome for
    /// several files, which is the shape §279 had to undo for the copy button.
    fn download_picked(&mut self, cx: &mut Context<Self>) {
        let wanted = self.picked_datasets();
        if wanted.is_empty() {
            return;
        }
        // Said once, up front, because the per-file statuses that follow overwrite each other and
        // the researcher pressed one button.
        self.say(
            format!(
                "fetching {} dataset{} into this conversation",
                wanted.len(),
                if wanted.len() == 1 { "" } else { "s" }
            ),
            cx,
        );
        for dataset in wanted {
            self.dataset_picks.remove(&dataset.persistent_id);
            self.download_dataset(dataset, cx);
        }
    }

    fn download_dataset(&mut self, dataset: protocol::Dataset, cx: &mut Context<Self>) {
        let Some(folder) = self.thread_workspace() else {
            self.status = "Start a conversation before downloading — the file needs a folder to \
                           land in."
                .to_string();
            cx.notify();
            return;
        };
        let id = dataset.persistent_id.clone();
        if !self.downloading.insert(id.clone()) {
            return;
        }
        self.status = format!("Downloading {}…", dataset.title);
        let mut answer = self.sidecar.download_dataset(id.clone(), folder);
        cx.spawn(async move |this, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.downloading.remove(&id);
                    match outcome {
                        Ok(path) => {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_else(|| path.display().to_string());
                            workbench.status =
                                format!("{name} is in this conversation's folder");
                            workbench.downloaded.insert(id, name);
                            // So Outputs shows it beside everything else the turn produced.
                            workbench.refresh_project(cx);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "a dataset download failed");
                            workbench.status = format!("Could not download: {error}");
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }









    /// What a file turned out to be, measured at most once per version of it.
    ///
    /// Keyed on modification time, so a dataset the agent rewrites is re-measured and one it
    /// leaves alone is not. A file that has vanished between the directory listing and this call
    /// reports its size only, which is what the row falls back to anyway.
    fn shape_of(&self, output: &workspace::Output) -> workspace::Shape {
        if let Some((seen, shape)) = self.shapes.borrow().get(&output.path) {
            if *seen == output.modified {
                return *shape;
            }
        }
        let shape = workspace::shape(&output.path, output.bytes);
        self.shapes
            .borrow_mut()
            .insert(output.path.clone(), (output.modified, shape));
        shape
    }

    /// Copy the files this conversation's commands wrote outside it back into it.
    ///
    /// **A press, and only ever a copy.** Automatic would be wrong twice over: a named path can be
    /// the researcher's own input, and a script often reads back what it just wrote. The backend
    /// decides *which* files from its own record — this sends no paths, because a request that
    /// could name one would be a file-copier pointed at anything.
    fn collect_outside(&mut self, cx: &mut Context<Self>) {
        // **Logged before anything can fail**, so "I pressed it and nothing happened" stops being
        // indistinguishable from "the press never arrived". §272 cost four rounds to learn that an
        // absent message and an absent action look identical from the other end of a report.
        let waiting = self.thread_commands();
        tracing::info!(
            commands = waiting.len(),
            files = files_left_outside(&waiting).len(),
            in_flight = self.collect_in_flight,
            // **The folder this side read**, so it can be compared with the one the backend names
            // in its answer. The app counted files written outside and the backend counted none,
            // which means the two are reading different records — and neither count can say that.
            read_from = ?self.thread_workspace(),
            "asked to bring outside files into the conversation"
        );
        if self.collect_in_flight {
            return;
        }
        self.collect_in_flight = true;
        self.collecting = None;
        cx.notify();

        let mut answer = self.sidecar.collect_outside();
        cx.spawn(async move |workbench, cx| {
            if let Some(outcome) = answer.next().await {
                let _ = workbench.update(cx, |workbench, cx| {
                    workbench.collect_in_flight = false;
                    // **Say it out loud.** `say` puts the outcome in the status bar *and* in a
                    // toast that outlives the next status change — it has existed for exactly this
                    // since §41, and the first version of this button used none of it, so a press
                    // that worked and a press that did nothing looked identical.
                    match &outcome {
                        Ok(collected) => {
                            tracing::info!(
                                brought = collected.brought.len(),
                                refused = collected.refused.len(),
                                note = %collected.note,
                                "brought files into the conversation"
                            );
                            workbench.say(collected_sentence(collected), cx);
                        }
                        Err(error) => {
                            tracing::warn!(%error, "could not bring the files in");
                            workbench.say(format!("could not bring the files in: {error}"), cx);
                        }
                    }
                    workbench.collecting = Some(outcome);
                    cx.notify();
                });
            }
        })
        .detach();
    }



    /// The commands this conversation recorded, oldest first.
    fn thread_commands(&self) -> Vec<workspace::Command> {
        self.thread_workspace()
            .map(|dir| workspace::commands(&dir))
            .unwrap_or_default()
    }

    /// Re-read the datasets this conversation's searches returned.
    ///
    /// **The file wins whenever there is one.** The model's answer is kept only for a run that
    /// wrote none — a sandboxed deployment, or a conversation from before the file existed — so
    /// that switching to the search's own answer never empties a panel that used to have rows.
    pub(crate) fn reload_datasets(&mut self) {
        self.search_totals = self
            .thread_workspace()
            .map(|dir| workspace::search_totals(&dir))
            .unwrap_or_default();
        let found = self
            .thread_workspace()
            .map(|dir| workspace::datasets(&dir))
            .unwrap_or_default();
        self.datasets = if found.is_empty() {
            self.recommended_datasets.clone()
        } else {
            found
        };
    }

    /// Whether the agent put this row forward, matched on the bare identifier.
    pub(crate) fn was_recommended(&self, dataset: &protocol::Dataset) -> bool {
        let id = bare_persistent_id(&dataset.persistent_id);
        !id.is_empty() && self.recommended_ids.contains(&id)
    }

    /// The claims this conversation's subagents recorded, oldest first.
    pub(crate) fn thread_claims(&self) -> Vec<workspace::Claim> {
        self.thread_workspace()
            .map(|dir| workspace::claims(&dir))
            .unwrap_or_default()
    }

}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.restore_focus {
            self.restore_focus = false;
            let composer = self.composer.focus_handle(cx);
            window.focus(&composer);
        }
        // Once per frame, and only actually read when the record has changed. Both the panel and
        // every transcript block need it, and doing it in each would be a file read per message
        // per frame — the mistake `shape_of` is cached to avoid.
        self.refresh_authorship();

        // `relative` so the palette's `absolute` overlay is positioned against the
        // window rather than the page origin.
        // The panels sit in a row; the status bar spans the **window** beneath them.
        //
        // It used to live inside the chat pane, so it was only as wide as the chat and its
        // controls slid left and right whenever a panel was collapsed. A status bar that
        // moves is one you have to look for. Zed's runs the full width for the same
        // reason, and its buttons are always in the same place (docs §53).
        let mut body = div()
            .flex()
            .flex_row()
            .flex_grow()
            // Without this the row refuses to shrink below its content and pushes the
            // status bar off the bottom — the flex default that has now cost four
            // separate bugs (§40, §48, §51).
            .min_h_0()
            .w_full()
            .when(self.sidebar_open, |body| {
                body.child(self.rail(cx))
                    .child(self.divider(Divider::Sidebar, cx))
            })
            .when(!self.sidebar_open, |body| {
                body.child(
                    div()
                        .id("toggle-left-sidebar")
                        .child(
                            app_icon(
                                "icons/sidebar-simple-left.svg",
                                theme::text(),
                                Some(ui::IconSize::Small.px())
                            )
                        )
                        .w(px(30.))
                        .h(px(30.))
                        .bg(rgb(theme::surface()))
                        .m_2()
                        .mt_3()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .flex_none()
                        .p_4()
                        .flex()
                        .rounded_lg()
                        .items_center()
                        .justify_center()
                        .hover(|style| style.cursor_pointer())
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.sidebar_open = !workbench.sidebar_open;
                            workbench.remember_panels();
                            cx.notify();
                        })),
                    )
            })
            // **Its own card, not a strip inside the conversation's.** It lived inside the chat
            // pane's border, so the two read as one panel with a notch cut out of it while the
            // sidebar and the research panel each sat on their own — *"the conversation panel is
            // colliding with the road"*. Same treatment as its neighbours now (§173).
            //
            // Not before the first question: an empty road beside an empty transcript is a frame
            // around nothing, and the empty state has its own things to say.
            .when(!self.transcript.is_empty(), |body| {
                body.child(self.road_strip(cx))
            })
            .child(self.chat_pane(cx));

        // The right-hand slot belongs to the research panel alone. Setup used to take it,
        // which meant diagnosing a problem hid the outputs you were diagnosing it about.
        body = if self.panel_open {
            body.child(self.divider(Divider::Panel, cx))
                .child(self.artifacts_panel(cx))
        } else {
            body.child(
                div()
                    .id("toggle-right-panel")
                    .child(app_icon(
                        "icons/sidebar-simple-right.svg",
                        theme::text(),
                        Some(ui::IconSize::Small.px()),
                    ))
                    .w(px(30.))
                    .h(px(30.))
                    .bg(rgb(theme::surface()))
                    .m_2()
                    .mt_4()
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .flex_none()
                    .p_3()
                    .flex()
                    .rounded_lg()
                    .items_center()
                    .justify_center()
                    .hover(|style| style.cursor_pointer())
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.panel_open = !workbench.panel_open;
                        workbench.remember_panels();
                        cx.notify();
                    })),
            )
        };

        let root = div()
            // An id makes the root a drop target; without one the platform's file-drop
            // event has nowhere to land.
            .id("workbench")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme::background()))
            .text_color(rgb(theme::text()))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_move(
                cx.listener(|workbench, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some(drag) = workbench.gallery_scroll_drag.as_ref() {
                        // A release outside the narrow track may not deliver its mouse-up to the
                        // thumb. The move event still tells us the button is no longer held, so
                        // end the drag here as well instead of letting the next click move a rail
                        // the researcher is no longer touching (docs §158).
                        if !event.dragging() {
                            workbench.gallery_scroll_drag = None;
                            cx.notify();
                            return;
                        }
                        let offset_x = horizontal_drag_offset(
                            event.position.x,
                            drag.track_left,
                            drag.grab_x,
                            drag.travel,
                            drag.overflow,
                        );
                        let offset_y = drag.handle.offset().y;
                        drag.handle.set_offset(gpui::point(offset_x, offset_y));
                        cx.notify();
                        return;
                    }
                    let Some(edge) = workbench.dragging else {
                        return;
                    };
                    // Clamped so a pane can be made narrow but never vanish: a panel dragged
                    // to nothing is one the user has no handle left to drag back.
                    let width = match edge {
                        Divider::Sidebar => f32::from(event.position.x),
                        Divider::Panel => {
                            f32::from(window.viewport_size().width - event.position.x)
                        }
                    };
                    let width = width.clamp(160., 640.);
                    match edge {
                        Divider::Sidebar => workbench.sidebar_width = width,
                        Divider::Panel => workbench.panel_width = width,
                    }
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    if workbench.dragging.take().is_some()
                        || workbench.gallery_scroll_drag.take().is_some()
                    {
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::select_all_transcript))
            // Anywhere on the window, not a designated strip: someone dragging a file has
            // their eyes on the file, not on a target.
            .on_drop(
                cx.listener(|workbench, paths: &gpui::ExternalPaths, _window, cx| {
                    workbench.add_files(paths.paths(), cx);
                }),
            )
            .child(body)
            .child(self.status_bar(cx))
            .child(self.toasts(cx));

        // Settings floats rather than displacing a panel, so opening it no longer costs
        // the chat 420px for as long as it is open.
        let root = if self.settings_open {
            root.child(self.settings_pane(cx))
        } else {
            root
        };

        let root = if self.about_open {
            root.child(self.about_modal(cx))
        } else {
            root
        };

        let root = if self.provenance_open {
            root.child(self.provenance_modal(cx))
        } else {
            root
        };

        // The preview floats over the workbench but under destructive confirmation and the
        // palette: it is a thing you open, look at, and dismiss, not a place you navigate to
        // (docs §49, §155).
        let root = match &self.preview {
            Some(preview) => root.child(self.preview_modal(
                preview.current().clone(),
                &preview.items,
                preview.at,
                cx,
            )),
            None => root,
        };

        let root = if self.sources_open {
            root.child(self.sources_modal(cx))
        } else {
            root
        };

        let root = if self.datasets_open {
            root.child(self.datasets_modal(cx))
        } else {
            root
        };

        let root = if self.documents_open {
            root.child(self.documents_modal(cx))
        } else {
            root
        };

        let root = if self.commands_open {
            root.child(self.commands_modal(cx))
        } else {
            root
        };

        let root = if self.claims_open {
            root.child(self.claims_modal(cx))
        } else {
            root
        };

        // The budget gate, mounted before the confirmations below it. Nothing else in this app
        // guards money, and the researcher did not ask for it — a drafted run did.
        let root = match self.discovery_open.clone() {
            Some(view) => root.child(self.discovery_modal(&view, cx)),
            None => root,
        };

        let root = match self.approving.clone() {
            Some(approval) => root.child(self.approval_modal(&approval, cx)),
            None => root,
        };

        let root = match &self.confirming_delete {
            Some(target) => root.child(self.delete_modal(target, cx)),
            None => root,
        };

        // Above Settings, and that is not cosmetic: the pill that raises it lives *inside* the
        // Settings pane, which mounts later and would otherwise draw straight over it.
        let root = match self.confirming_provider {
            Some(spec) => root.child(self.provider_modal(spec, cx)),
            None => root,
        };

        let root = if self.palette_open {
            root.child(self.palette(cx))
        } else {
            root
        };

        let root = root.children(self.picker_popup(cx));

        // Both menus last, and `deferred` inside, so they paint over every pane they might open
        // across instead of being clipped by the one they opened in.
        let root = match &self.sidebar_menu {
            Some((open, at)) => root.child(self.sidebar_menu_element(open.clone(), *at, cx)),
            None => root,
        };
        match &self.context_menu {
            Some(open) => root.child(self.context_menu(open.clone(), cx)),
            None => root,
        }
    }
}

/// Decode a whole captured SSE stream into the transcript state it would produce.
///
/// Shared by `--replay` and the fixture test, so both exercise the same path the
/// window does: frame → decode → transcript, with nothing simulated in between.
fn decode_capture(raw: &[u8], mut on_status: impl FnMut(&str)) -> (Message, Vec<Bucket>) {
    let mut frames = protocol::SseDecoder::default();
    let mut turn = protocol::TurnDecoder::default();
    let mut message = Message::new("mini-me", String::new());
    let mut outputs: Vec<Bucket> = Vec::new();

    // One push: the framer handles the split, exactly as it does off the socket.
    for frame in frames.push(raw) {
        for event in turn.push(&frame) {
            match event {
                // A replay rebuilds the transcript, and the transcript has no use for the
                // run's id — only a live turn does, to be able to stop it.
                TurnEvent::Started { .. } => {}
                TurnEvent::Token(text) => message.push_body(&text),
                TurnEvent::Step { agent, label } => match agent {
                    None => message.steps.push(label),
                    Some(agent) => trace_for(&mut message, &agent).steps.push(label),
                },
                TurnEvent::SubagentToken { agent, text } => {
                    trace_for(&mut message, &agent).push_text(&text);
                }
                TurnEvent::Snapshot(snapshot) => {
                    if !snapshot.buckets.is_empty() {
                        outputs = snapshot.buckets;
                    }
                }
                TurnEvent::Approval(request) => {
                    for action in &request.actions {
                        message
                            .steps
                            .push(format!("awaiting approval: {}", action.tool));
                    }
                }
                TurnEvent::Status(status) => on_status(&status),
                TurnEvent::Error(error) => on_status(&format!("error: {error}")),
                TurnEvent::Done => {}
            }
        }
    }
    (message, outputs)
}

/// Replay a captured SSE stream and print what the transcript would show. No
/// backend, no window, no tokens spent.
///
/// The activity trace is the one feature whose input is 500 events of a real
/// delegation, so being able to re-run a saved capture is the difference between
/// testing it and paying for a research turn every time the decoder changes.
fn replay(path: &str) -> anyhow::Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("could not read {path}"))?;
    let (message, outputs) = decode_capture(&raw, |status| println!("status   : {status}"));

    println!("\n--- activity ---");
    for step in &message.steps {
        println!("· {step}");
    }
    for trace in &message.agents {
        println!(
            "▾ {} · {} step(s) · {} chars   [{}]",
            trace.name,
            trace.steps.len(),
            trace.text.chars().count(),
            trace.ns,
        );
        for step in &trace.steps {
            println!("    · {step}");
        }
        println!("    {}", protocol::summarize_agent_result(&trace.text));
    }
    println!("\n--- outputs ---");
    for bucket in &outputs {
        println!("{} · {}", bucket.name, bucket.items.len());
    }
    println!("\n--- assistant text ---\n{}", message.body.trim());

    anyhow::ensure!(
        !message.steps.is_empty() || !message.agents.is_empty(),
        "the capture decoded no activity at all — did `stream_subgraphs` get dropped?"
    );
    Ok(())
}

// The behavior suite stays immediately after the UI implementation it exercises, while
// the CLI-only launch helpers remain at the bottom of the executable. Moving this large
// module past startup code would create merge churn without changing test visibility
// (the source-order lesson recorded in docs §118).
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    fn collected(kind: protocol::JobKind, thread: &str) -> (String, protocol::Job) {
        (
            thread.to_string(),
            protocol::Job {
                kind,
                task_id: "20840df8-a8e8-4ab0-a65c-1b1824961955".into(),
                question: "SOC modelling".into(),
                context_id: None,
                status: "completed".into(),
                size: None,
            },
        )
    }

    /// §244: one run gets a definite press, because a single action has to mean something.
    /// A conversation must never become undeletable.
    ///
    /// The guard used to *refuse* while any task was unfinished, and a task that never reaches a
    /// terminal state locks the conversation for good. It happened in front of a colleague: a
    /// paper search stuck on "running" for over an hour, and no way to remove the thread at all.
    ///
    /// Deleting under a live worker is a real risk and the modal says so — but a warning can be
    /// read and acted on, where a refusal with no way past it cannot (§278).
    #[test]
    fn unfinished_work_warns_rather_than_locking_the_conversation() {
        let source = include_str!("main.rs");
        let ask = source
            .split("fn request_delete")
            .nth(1)
            .expect("the delete guard")
            .split("\n    fn ")
            .next()
            .expect("its body");

        assert!(
            ask.contains("self.confirming_delete = Some(target)"),
            "the confirmation must be reachable"
        );
        assert!(
            !ask.contains("while its background work is running"),
            "unfinished background work must no longer refuse the delete outright"
        );
        assert!(
            ask.contains("delete_interrupts_work"),
            "it must still be carried to the modal, so the sentence that asks can warn"
        );
        // A turn actually streaming *is* still refused: that one ends on its own, in seconds.
        assert!(
            ask.contains("while its turn is running"),
            "a live foreground turn is a different case and still blocks"
        );
    }

    /// A press must never be silent, and a zero must never be bare.
    ///
    /// The button reported `brought=0 refused=0` and drew nothing — so a press that worked and a
    /// press that did nothing looked identical, which is how a working feature reads as broken
    /// (§279). The backend now sends its own sentence for that case precisely so it can be shown.
    #[test]
    fn every_outcome_has_a_sentence_including_nothing_at_all() {
        let nothing = protocol::Collected {
            brought: Vec::new(),
            refused: Vec::new(),
            note: "1 file(s) were written outside this conversation, and none is still there"
                .to_string(),
        };
        assert_eq!(collected_sentence(&nothing), nothing.note, "the reason, not a zero");

        // Even with no note the press says *something*.
        let mute = protocol::Collected::default();
        assert!(!collected_sentence(&mute).is_empty());

        let one = protocol::Collected {
            brought: vec![("/tmp/results.csv".into(), "results.csv".into())],
            refused: Vec::new(),
            note: String::new(),
        };
        assert_eq!(collected_sentence(&one), "brought 1 file into this conversation");

        // A partial result names the count **and** the first reason: "2 left out" tells nobody
        // what to do next.
        let partial = protocol::Collected {
            brought: vec![("/tmp/a.csv".into(), "a.csv".into())],
            refused: vec![("/tmp/b.csv".into(), "it is no longer where the command left it".into())],
            note: String::new(),
        };
        let said = collected_sentence(&partial);
        assert!(said.contains("brought 1 file"), "{said}");
        assert!(said.contains("1 left where it was"), "{said}");
        assert!(said.contains("no longer where the command left it"), "the reason too: {said}");
    }

    /// The button counts **files**, not commands.
    ///
    /// One command can write three — a figure per variable is the ordinary case here — so counting
    /// commands would have made "Copy 1 file into this conversation" a lie the first time a real
    /// analysis ran.
    #[test]
    fn what_would_be_copied_is_counted_in_files() {
        let commands = workspace::decode_commands(
            "{\"command\":\"a\",\"outside\":[\"/tmp/x.png\",\"/tmp/y.png\"],\"wrote\":[\"/tmp/x.png\",\"/tmp/y.png\"]}\n\
             {\"command\":\"b\",\"outside\":[\"/tmp/x.png\"],\"wrote\":[\"/tmp/x.png\"]}\n\
             {\"command\":\"c\",\"outside\":[\"/tmp/read.csv\"],\"wrote\":[]}",
        );
        let files = files_left_outside(&commands);
        // Two commands, three `wrote` entries, two distinct files — and the read is not among them.
        assert_eq!(files, vec!["/tmp/x.png".to_string(), "/tmp/y.png".to_string()]);
        assert!(!files.contains(&"/tmp/read.csv".to_string()), "a path only named is never copied");
    }

    /// Nothing confirmed written means nothing to offer, even with paths named.
    #[test]
    fn a_conversation_that_only_read_outside_files_offers_nothing() {
        let commands = workspace::decode_commands(
            "{\"command\":\"python3 read.py\",\"outside\":[\"/tmp/theirs.csv\"],\"wrote\":[]}",
        );
        assert!(files_left_outside(&commands).is_empty());
    }

    /// **The case that produced eight orphaned figures, as a test.**
    ///
    /// A researcher asked for plots. The agent wrote them beside the script it ran, in a folder
    /// that was not this conversation's, and the answer named them. The note appeared. No button
    /// did, because §279's control lived at the bottom of the WHAT RAN modal — and none of that
    /// is visible from any single unit, which is why this asserts the *placement* rather than
    /// either half of it (§301).
    #[test]
    fn the_offer_sits_on_the_answer_that_named_the_missing_files() {
        let stray = vec!["/tmp/work/missingness.png".to_string()];
        // (from_researcher, named_nothing_missing) — ask, answer, ask, flagged answer.
        let transcript = [(true, true), (false, true), (true, true), (false, false)];
        assert_eq!(place_recovery_offer(&stray, &transcript), Some(3));

        // **Newest, when several answers are flagged.** The oldest is not where anyone is
        // looking, and one conversation gets one offer however many notes it collected.
        let twice = [(true, true), (false, false), (true, true), (false, false)];
        assert_eq!(place_recovery_offer(&stray, &twice), Some(3));

        // Nothing to fetch, nothing to press — whatever the notes say. A note can outlive the
        // file it named, and a button that fetches nothing is §272's silent press.
        assert_eq!(place_recovery_offer(&[], &transcript), None);
    }

    /// **A script that writes beside itself names no path, so no message is ever flagged.**
    ///
    /// `ledger::outside` reads absolute paths out of the *command text*; `plt.savefig("a.png")`
    /// inside `analysis.py` puts none there. But `wrote` is decided by the file's own mtime, so
    /// the file is known and fetchable while no note exists anywhere. Anchoring the offer to the
    /// note would hide it precisely here — which is the reported failure, not a corner case.
    #[test]
    fn files_written_with_no_note_anywhere_are_still_offered() {
        let stray = vec!["/tmp/work/correlation_heatmap.png".to_string()];
        let unflagged = [(true, true), (false, true), (true, true), (false, true)];
        assert_eq!(place_recovery_offer(&stray, &unflagged), Some(3));

        // Never on the researcher's own message, even when it is the newest thing said.
        let researcher_last = [(false, true), (true, true)];
        assert_eq!(place_recovery_offer(&stray, &researcher_last), Some(0));
        assert_eq!(place_recovery_offer(&stray, &[(true, true)]), None);
    }

    /// **The button counts its own list, and says so when the two disagree.**
    ///
    /// The note is parsed from prose; the button acts on mtime-verified paths. Promising eight
    /// and delivering six is how "I pressed it and nothing happened" starts (§272).
    #[test]
    fn the_offer_never_promises_more_than_it_can_fetch() {
        assert_eq!(recovery_offer(8, 0), None, "nothing fetchable, nothing offered");

        let (label, caveat) = recovery_offer(6, 6).expect("six named, six fetchable");
        assert_eq!(label, "Copy 6 files into this conversation");
        assert!(caveat.is_none(), "the counts agree — no explanation is owed");

        let (label, caveat) = recovery_offer(8, 6).expect("eight named, six fetchable");
        assert_eq!(label, "Copy 6 files into this conversation", "6, never 8");
        assert!(
            caveat.expect("the gap is explained").contains("watched writing"),
            "the caveat has to say what the button can see"
        );

        // One file is "file", and the caveat still appears when it is one of several named.
        let (label, caveat) = recovery_offer(3, 1).expect("one fetchable");
        assert_eq!(label, "Copy 1 file into this conversation");
        assert!(caveat.is_some());
    }

    /// **The agent's pick has to match the search's row, or the mark never appears.**
    ///
    /// Dataverse hands the same dataset back as `doi:10.21223/P3/X`, as `10.21223/P3/X` and as a
    /// resolver URL. The model answers with whichever it read. Comparing verbatim is what made
    /// six real recommendations look fabricated (§288) — here it would leave every row unmarked
    /// while the list looked fine, which is quieter and no better.
    #[test]
    fn a_recommendation_is_matched_however_it_is_spelled() {
        let rows = workspace::decode_datasets(include_str!(
            "../tests/fixtures/dataverse-search.json"
        ));
        let found = bare_persistent_id(&rows[0].persistent_id);
        assert_eq!(found, "10.21223/p3/hjlujz");

        for spelling in [
            "doi:10.21223/P3/HJLUJZ",
            "10.21223/P3/HJLUJZ",
            "https://doi.org/10.21223/P3/HJLUJZ",
            "  DOI:10.21223/p3/hjlujz  ",
        ] {
            assert_eq!(bare_persistent_id(spelling), found, "{spelling} names the same dataset");
        }

        // And a different dataset stays different — a normaliser that collapsed everything would
        // mark every row and mean nothing.
        assert_ne!(bare_persistent_id("doi:10.21223/P3/3AIN78"), found);
        assert_eq!(bare_persistent_id(""), "");
    }

    /// A row the producer could not map is still a row, and must not be mistaken for a match.
    #[test]
    fn a_row_with_no_identifier_is_never_marked_as_chosen() {
        let rows = workspace::decode_datasets(include_str!(
            "../tests/fixtures/dataverse-search.json"
        ));
        let unmapped = rows.last().expect("the layout nobody has met");
        assert_eq!(unmapped.persistent_id, "");
        // `was_recommended` guards on this: an empty id must not match an empty entry in the
        // recommendation list and light up a row the agent never named.
        assert!(bare_persistent_id(&unmapped.persistent_id).is_empty());
    }

    /// The panel must not go silent in the situation the record exists for.
    ///
    /// A command that wrote everything to `/tmp` leaves the conversation folder empty, so `files`
    /// is 0 — and the first version returned early on `files == 0 && buckets.is_empty()`, hiding
    /// `WHAT RAN` in precisely the case §160 describes. A researcher pressed the button, the file
    /// went outside, and the panel said nothing (§277).
    #[test]
    fn a_turn_that_wrote_nothing_here_still_has_something_to_say() {
        assert!(
            !outputs_are_empty(0, 0, 1, 0),
            "no files, no artifacts, one command — the panel must still show what ran, because \
             the missing files are the point"
        );
        // The genuinely empty case stays empty: a conversation gains no furniture for nothing.
        assert!(outputs_are_empty(0, 0, 0, 0));
        // And any one of the four is enough on its own.
        assert!(!outputs_are_empty(1, 0, 0, 0));
        assert!(!outputs_are_empty(0, 1, 0, 0));
        // Including a subagent answer with no files behind it — which is the whole finding when a
        // worker reports success over four empty folders (§207a).
        assert!(!outputs_are_empty(0, 0, 0, 1));
    }

    /// What the Outputs panel says about what this conversation's subagents claimed.
    ///
    /// Read from the recorder's own fixture, so the wording is asserted against the shapes that
    /// actually occur rather than against ones invented here.
    #[test]
    fn the_claims_line_leads_with_the_strongest_thing_it_has_earned() {
        let fixture = include_str!("../tests/fixtures/claim-record.jsonl");
        let claims = workspace::decode_claims(fixture);
        let (summary, loud) = claims_summary(&claims);

        assert!(summary.starts_with("5 subagent answers"), "{summary}");
        // Two answers are contradicted — a missing index and an invented `persistent_id` — and
        // that outranks the unreadable check and the borrowed PDF also present in this fixture.
        assert!(summary.contains("2 claimed something that isn't there"), "{summary}");
        assert!(loud, "a claim the workspace contradicts is drawn in the accent colour");
    }

    /// **An answer nothing looked at must not read as an answer that was verified.**
    ///
    /// This is `checked` earning its place. A schema with no path rule produces an empty `missing`
    /// list, so a line that reported only findings would say "3 subagent answers" and mean
    /// "3 verified" to every reader — the silence `NO_PATHS` exists to break.
    #[test]
    fn nothing_to_check_is_said_out_loud_rather_than_left_as_silence() {
        let unexamined = workspace::decode_claims(
            "{\"source\":\"hypothesis_generator\",\"schema\":\"HypothesisOutput\",\"checked\":false}",
        );
        let (summary, loud) = claims_summary(&unexamined);
        assert_eq!(summary, "1 subagent answer · 1 with nothing to check");
        assert!(!loud, "not a fault — no rule covers that schema");

        // And the genuinely clean case says so rather than leaving the good news implicit.
        let clean = workspace::decode_claims(
            "{\"source\":\"data_voyager\",\"checked\":true,\"claimed\":4}",
        );
        assert_eq!(claims_summary(&clean).0, "1 subagent answer · everything they named is there");

        // A check that could not run is neither of those, and outranks both.
        let blind = workspace::decode_claims(
            "{\"source\":\"dataverse_explorer\",\"checked\":true,\"datasets\":2,\
              \"note\":\"dataverse_search.json could not be read\"}",
        );
        let (said, shouted) = claims_summary(&blind);
        assert_eq!(said, "1 subagent answer · 1 could not be checked");
        assert!(!shouted, "it is not an accusation; nothing was compared");
    }

    /// **A count with no denominator is not an answer, and `29 of 0` is worse than either.**
    ///
    /// The MCP read `total_count` to decide when to stop paging and never returned it, so
    /// twenty-nine rows read exactly like a thorough search of a twenty-nine dataset corpus
    /// (§299). Now that it does, the panel and the modal say so — and both say it the same way,
    /// because the wording lives in one function.
    #[test]
    fn the_heading_says_of_how_many_only_when_something_said_so() {
        use workspace::SearchTotals;

        let partial = SearchTotals { total: 4000, kept: 29, complete: false };
        assert_eq!(datasets_heading(29, partial), "29 of 4000");

        // Nothing reported a total. `29 of 0` would be a claim about the corpus, and a
        // confident-looking one — a deployment that cannot count is not a corpus of nothing.
        let unknown = SearchTotals { total: 0, kept: 29, complete: false };
        assert_eq!(datasets_heading(29, unknown), "29");
        assert_eq!(unknown.denominator(), None);

        // The whole corpus. A denominator equal to the count is noise, not reassurance.
        let whole = SearchTotals { total: 29, kept: 29, complete: true };
        assert_eq!(datasets_heading(29, whole), "29");

        // And a total smaller than what is on screen — several searches this turn, accumulated
        // past any single query's match count. Showing "40 of 29" would read as a bug.
        let accumulated = SearchTotals { total: 29, kept: 40, complete: false };
        assert_eq!(datasets_heading(40, accumulated), "40");
    }

    /// **The count on the button is the number of files that will arrive.**
    ///
    /// Read off the list rather than out of the set, so a tick on a dataset that has since been
    /// fetched, or is being fetched, stops being counted. Otherwise the label promises work that
    /// will not happen — which is `files_left_outside`'s lesson (§279) on a different control.
    #[test]
    fn the_selection_counts_only_what_can_still_be_fetched() {
        let rows = workspace::decode_datasets(include_str!(
            "../tests/fixtures/dataverse-search.json"
        ));
        let ids: Vec<String> = rows
            .iter()
            .map(|dataset| dataset.persistent_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        assert!(ids.len() >= 3, "the fixture has enough rows to select among");

        let picked: std::collections::HashSet<String> = ids.iter().cloned().collect();
        let downloaded: std::collections::HashMap<String, String> =
            [(ids[0].clone(), "a.csv".to_string())].into_iter().collect();
        let downloading: std::collections::HashSet<String> =
            [ids[1].clone()].into_iter().collect();

        let offered = still_fetchable(&rows, &picked, &downloaded, &downloading);
        let offered_ids: Vec<&String> = offered.iter().map(|d| &d.persistent_id).collect();

        assert_eq!(
            offered.len(),
            ids.len() - 2,
            "one already here and one in flight are not two more files"
        );
        assert!(!offered_ids.contains(&&ids[0]) && !offered_ids.contains(&&ids[1]));
        // And order follows the list, because that is the order the rows are read in.
        assert_eq!(offered_ids.first(), Some(&&ids[2]));

        // Nothing ticked is nothing offered — the button must not appear at all.
        assert!(still_fetchable(&rows, &Default::default(), &downloaded, &downloading).is_empty());
    }

    /// A row with no identifier cannot be ticked, because there is nothing to tick it by.
    ///
    /// The producer emits one when it meets a Dataverse layout it does not know (§290). It renders
    /// — losing a row silently is worse — but it must not join a selection keyed by identifier,
    /// where an empty key would collide with every other unmapped row.
    #[test]
    fn an_unidentified_dataset_is_never_part_of_a_selection() {
        let rows = workspace::decode_datasets(include_str!(
            "../tests/fixtures/dataverse-search.json"
        ));
        let unmapped = rows.last().expect("the layout nobody has met");
        assert_eq!(unmapped.persistent_id, "");

        // The tick lives under the same condition as the download button, and that condition needs
        // an identifier to ask the access route about at all.
        let selectable: Vec<&protocol::Dataset> = rows
            .iter()
            .filter(|dataset| !dataset.persistent_id.is_empty())
            .collect();
        assert_eq!(selectable.len(), rows.len() - 1);
    }

    /// A real file in the researcher's own Downloads folder is not a missing file.
    ///
    /// The recorder called these "missing" once and it read as *this file does not exist*, which
    /// was false. The line has to be able to say the true thing — they will not travel with the
    /// conversation — without making the accusation.
    #[test]
    fn a_borrowed_file_is_a_durability_warning_and_not_an_accusation() {
        let borrowed = workspace::decode_claims(
            "{\"source\":\"pdf_librarian\",\"checked\":true,\"claimed\":1,\
              \"outside\":[\"/mnt/c/Users/LENOVO/Downloads/gnn.pdf\"]}",
        );
        let (summary, loud) = claims_summary(&borrowed);
        assert!(summary.contains("1 used a file from outside this conversation"), "{summary}");
        assert!(!loud, "the file is real; only its location is worth saying");
    }

    /// What the Outputs panel says about a conversation's commands.
    ///
    /// A free function so the *wording* is testable, because the wording is the feature: the phrase
    /// that matters names files the panel below it cannot show, and it has to say **named** rather
    /// than **wrote** — the producer sees paths in a command's text and nothing more.
    #[test]
    fn the_summary_says_named_and_never_says_wrote() {
        let fixture = include_str!("../tests/fixtures/command-record.jsonl");
        let commands = workspace::decode_commands(fixture);
        let (summary, loud) = commands_summary(&commands);

        assert!(summary.starts_with("4 commands"), "{summary}");
        assert!(summary.contains("1 failed"), "{summary}");
        // One command is *confirmed* to have written outside, so the line says so — that is a fact
        // about a file, established from its mtime, not a reading of the command's text.
        assert!(summary.contains("1 wrote a file outside this conversation"), "{summary}");
        assert!(loud, "something landed outside, so the line is drawn in the accent colour");
    }

    /// When nothing is *confirmed* written, the line must retreat to the weaker, true claim.
    ///
    /// `pd.read_csv('/tmp/input.csv')` names a file the researcher owns. Saying the command "wrote"
    /// it would be §252's mistake — a sentence claiming more than the code can know — and this is
    /// the sentence where it would be made.
    #[test]
    fn a_path_only_named_is_never_described_as_written() {
        let named_only = workspace::decode_commands(
            "{\"command\":\"python3 read.py\",\"exit\":0,\"outside\":[\"/tmp/theirs.csv\"],\"wrote\":[]}",
        );
        let (summary, loud) = commands_summary(&named_only);
        assert!(summary.contains("named a file outside"), "{summary}");
        assert!(
            !summary.contains("wrote"),
            "nothing confirmed it was written, so the line must not say so: {summary}"
        );
        assert!(loud, "still worth the accent colour — it is still worth looking at");
    }

    /// A quiet conversation says a quiet thing, and is not drawn as a warning.
    #[test]
    fn nothing_outside_is_not_an_alarm() {
        let commands = workspace::decode_commands(
            "{\"command\":\"ls\",\"exit\":0,\"outside\":[]}\n{\"command\":\"pwd\",\"exit\":0,\"outside\":[]}",
        );
        let (summary, loud) = commands_summary(&commands);
        assert_eq!(summary, "2 commands");
        assert!(!loud);

        // And one command is not "1 commands".
        let one = workspace::decode_commands("{\"command\":\"ls\",\"exit\":0,\"outside\":[]}");
        assert_eq!(commands_summary(&one).0, "1 command");
    }

    #[test]
    fn one_collected_run_is_named_and_openable() {
        let runs = [collected(protocol::JobKind::Analysis, "01a0215f-c66b-7461-96f2-595a168fa8f8")];
        // The label names what finished rather than counting it.
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].1.kind.label(), "Data analysis");
        assert_eq!(runs[0].0, "01a0215f-c66b-7461-96f2-595a168fa8f8");
    }

    /// With several, no single thread is the right destination — the sidebar is.
    #[test]
    fn several_collected_runs_are_counted_rather_than_opened() {
        let runs = [
            collected(protocol::JobKind::Analysis, "aaa"),
            collected(protocol::JobKind::Theorizer, "bbb"),
        ];
        assert!(runs.len() > 1);
        // Two different threads, so "open it" would have to pick one arbitrarily.
        assert_ne!(runs[0].0, runs[1].0);
    }

    /// The theorizer goes through the identical path, so its label has to work too.
    #[test]
    fn a_collected_theorizer_run_is_named_by_its_own_kind() {
        let (_, job) = collected(protocol::JobKind::Theorizer, "ccc");
        assert_eq!(job.kind.label(), "Theorizer");
        assert!(job.is_finished());
    }

    /// Only unfinished ones are worth a request; a terminal job has already been collected.
    #[test]
    fn a_finished_run_is_not_swept_again() {
        let base = protocol::Job {
            kind: protocol::JobKind::Analysis,
            task_id: "t".into(),
            question: "q".into(),
            context_id: None,
            status: String::new(),
            size: None,
        };
        for status in ["completed", "failed", "canceled", "error"] {
            let job = protocol::Job { status: status.into(), ..base.clone() };
            assert!(job.is_finished(), "{status} must not be swept");
        }
        for status in ["working", "submitted", "running", "input-required"] {
            let job = protocol::Job { status: status.into(), ..base.clone() };
            assert!(!job.is_finished(), "{status} must be swept");
        }
    }

    /// §240: the answer explained the file was forthcoming and was flagged for not holding it.
    #[test]
    fn a_file_a_running_job_has_not_written_yet_is_not_missing() {
        let task = "4c290c71-be43-421a-8273-2f98dcc7b331";
        let named = format!("analysis/{task}.md");

        let running = protocol::Job {
            kind: protocol::JobKind::Analysis,
            task_id: task.to_string(),
            question: "SOC modelling".into(),
            context_id: None,
            status: "working".into(),
            size: None,
        };
        assert!(!running.is_finished());
        // The rule the filter applies: the filename carries the task id of an unfinished job.
        assert!(named.to_ascii_lowercase().contains(&running.task_id.to_ascii_lowercase()));

        // And once it finishes, the exemption lapses — a file that should be there and is not is
        // exactly what §175's note is for.
        let done = protocol::Job {
            status: "completed".into(),
            ..running.clone()
        };
        assert!(done.is_finished());
    }

    /// A file named after some *other* run is still checked.
    #[test]
    fn an_unrelated_filename_is_not_exempted_by_a_running_job() {
        let running = protocol::Job {
            kind: protocol::JobKind::Analysis,
            task_id: "4c290c71-be43-421a-8273-2f98dcc7b331".into(),
            question: "q".into(),
            context_id: None,
            status: "working".into(),
            size: None,
        };
        for other in ["eda_distributions.png", "analysis/deadbeef-0000.md", "final_report.md"] {
            assert!(
                !other.to_ascii_lowercase().contains(&running.task_id.to_ascii_lowercase()),
                "{other} must still be checked"
            );
        }
    }

    /// §236: `thread_workspace()` is `None` until the backend assigns a thread id on the first
    /// turn, so "new conversation, attach, ask" — the ordinary flow — copied nothing.
    #[test]
    fn a_file_attached_before_the_conversation_existed_is_copied_in_later() {
        let waiting = awaiting_adoption(&[
            attached("SOC_Covariables_TrainValV5.csv", "/mnt/c/Users/x/Downloads/SOC_Covariables_TrainValV5.csv"),
            attached("SOC_Covariables_TESTV5.csv", "/mnt/c/Users/x/Downloads/SOC_Covariables_TESTV5.csv"),
        ]);
        assert_eq!(waiting.len(), 2, "both were sent from outside the folder");
    }

    #[test]
    fn a_file_already_inside_the_conversation_is_not_copied_twice() {
        assert!(awaiting_adoption(&[attached("yield.csv", "./yield.csv")]).is_empty());
    }

    #[test]
    fn a_mixed_batch_queues_only_the_ones_that_are_outside() {
        let waiting = awaiting_adoption(&[
            attached("copied.csv", "./copied.csv"),
            attached("huge.tab", "/mnt/d/genomes/huge.tab"),
        ]);
        assert_eq!(waiting.len(), 1);
        assert!(waiting[0].ends_with("huge.tab"));
    }

    #[test]
    fn nothing_attached_queues_nothing() {
        assert!(awaiting_adoption(&[]).is_empty());
    }


    /// The search records stay on disk and out of the conversation.
    #[test]
    fn a_search_record_is_not_a_transcript_card() {
        let record = |name: &str| workspace::Output {
            path: std::path::PathBuf::from("/w/thread").join(name),
            name: name.to_string(),
            kind: workspace::Kind::Data,
            bytes: 0,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };
        assert!(is_search_record(&record("papers.json")));
        assert!(is_search_record(&record("dataverse_search.json")));
        // Everything the research actually produced still shows.
        assert!(!is_search_record(&record("eda_distributions.png")));
        assert!(!is_search_record(&record("cleaned.csv")));
        assert!(!is_search_record(&record("final_report.md")));
        // Not by extension, and not by a name that merely contains one of them.
        assert!(!is_search_record(&record("my_papers.json")));
    }
    use super::*;
    use crate::components::{chat::*, common::*, gallery_view::*, provenance_view::*};

    #[gpui::test]
    fn a_long_transcript_builds_only_rows_near_the_viewport(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::cell::Cell;
        use std::rc::Rc;

        struct MeasuredList { state: ListState, built: Rc<Cell<usize>> }
        impl Render for MeasuredList {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                let built = self.built.clone();
                gpui::list(self.state.clone(), move |index, _window, _cx| {
                    built.set(built.get() + 1);
                    // Variable by design: `uniform_list` is invalid for transcript rows (§156).
                    div().h(px(if index % 7 == 0 { 180. } else { 36. })).into_any_element()
                }).w_full().h_full()
            }
        }

        let built = Rc::new(Cell::new(0));
        let state = ListState::new(500, ListAlignment::Top, px(240.));
        let cx = cx.add_empty_window();
        cx.draw(gpui::point(px(0.), px(0.)), size(px(900.), px(600.)), |_, cx| {
            cx.new(|_| MeasuredList { state, built: built.clone() })
        });
        // The removed eager loop constructed all 500. Count the same deterministic unit on both
        // sides instead of noisy wall time from a headless debug build (docs §156).
        println!("transcript row construction: eager 500, virtual {}", built.get());
        assert!(built.get() < 100, "virtualization built {} rows", built.get());
    }

    #[test]
    fn select_all_uses_rendered_words_for_an_unpainted_markdown_message() {
        let message = Message::new(
            "mini-me",
            "# Result\n\nThe **measured** value is 42.\n\n```text\ncopy me\n```".into(),
        );
        assert_eq!(message.selection_text(), "Result\nThe measured value is 42.\ncopy me");
    }

    #[test]
    fn outputs_in_the_same_agent_folder_become_one_gallery() {
        let task = "019fe9f6-9126-7710-a806-35d5e09170a4";
        let names = [
            PathBuf::from(task).join("guinea_pig_eda_output/plots/health.png"),
            PathBuf::from(task).join("guinea_pig_eda_output/plots/yield.png"),
            PathBuf::from(task).join("guinea_pig_eda_output/tables/summary.csv"),
        ];
        let outputs: Vec<workspace::Output> = names
            .into_iter()
            .map(|name| workspace::Output {
                path: name.clone(),
                name: name.to_string_lossy().into_owned(),
                kind: workspace::Kind::Other,
                bytes: 1,
                modified: std::time::SystemTime::UNIX_EPOCH,
            })
            .collect();

        let groups = output_folder_groups(&outputs);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].outputs.len(), 2);
        assert_eq!(groups[1].outputs.len(), 1);
        assert_eq!(
            output_folder_label(&groups[0].folder, None),
            "guinea_pig_eda_output / plots"
        );
        // The same folder, once the app knows whose thread that UUID is: the worker takes the
        // UUID's place rather than vanishing with it, so the heading reads as a path of work.
        assert_eq!(
            output_folder_label(&groups[0].folder, Some("background worker")),
            "background worker / guinea_pig_eda_output / plots"
        );
        // And a specialist inside the conversation, which has no UUID to replace: the name goes
        // ahead of the folder rather than nowhere (§201).
        assert_eq!(
            output_folder_label(std::path::Path::new("plots"), Some("report writer")),
            "report writer / plots"
        );
    }

    /// A worker with a plan, so the status-bar line can be checked without a window.
    fn worker_with(status: &str, todos: &[(&str, &str)], activity: Option<&str>) -> protocol::AsyncTask {
        protocol::AsyncTask {
            task_id: "t-1".into(),
            thread_id: "th-1".into(),
            agent_name: "background_worker".into(),
            status: status.into(),
            description: String::new(),
            pending: None,
            error: None,
            activity: activity.map(str::to_owned),
            todos: todos
                .iter()
                .map(|(content, status)| protocol::Todo {
                    content: (*content).to_string(),
                    status: (*status).to_string(),
                })
                .collect(),
            owner: String::new(),
        }
    }

    /// The one-line glance version of a long run (§209).
    #[test]
    fn the_status_line_counts_the_step_being_worked_on() {
        let plan = [
            ("Generate the dataset", "completed"),
            ("Clean the columns", "completed"),
            ("Build the model", "in_progress"),
            ("Write the report", "pending"),
        ];

        // Two done, so the one being worked on is the third — `done + 1`, not `done`. A researcher
        // reading "step 2 of 4" while the third is running has been told the wrong thing.
        let task = worker_with("running", &plan, Some("execute"));
        assert_eq!(
            summary_for(&[task], &[]),
            Some("background worker · step 3 of 4 · execute".to_string())
        );

        // No activity yet: the count still stands on its own.
        let quiet = worker_with("running", &plan, None);
        assert_eq!(
            summary_for(&[quiet], &[]),
            Some("background worker · step 3 of 4".to_string())
        );

        // **A finished worker says nothing.** Its row already carries a tick and a button; keeping
        // it in the status bar would leave a stale count there for the rest of the session.
        let done = worker_with("success", &plan, None);
        assert_eq!(summary_for(&[done], &[]), None);

        // Nor does a worker that never wrote a plan — there is no denominator to offer.
        let planless = worker_with("running", &[], Some("execute"));
        assert_eq!(summary_for(&[planless], &[]), None);
    }

    #[test]
    fn a_conversations_own_plan_is_the_fallback_and_stops_when_it_is_done() {
        let plan: Vec<protocol::Todo> = [("Search the literature", "completed"), ("Synthesise", "pending")]
            .iter()
            .map(|(content, status)| protocol::Todo {
                content: (*content).to_string(),
                status: (*status).to_string(),
            })
            .collect();
        assert_eq!(summary_for(&[], &plan), Some("step 2 of 2".to_string()));

        // A worker outranks it: the worker is what runs while nobody is looking.
        let task = worker_with("running", &[("Do the thing", "in_progress")], Some("ls"));
        assert_eq!(
            summary_for(&[task], &plan),
            Some("background worker · step 1 of 1 · ls".to_string())
        );

        // Every step done: the line goes away rather than sitting at "step 3 of 2".
        let finished: Vec<protocol::Todo> = plan
            .iter()
            .map(|todo| protocol::Todo {
                content: todo.content.clone(),
                status: "completed".into(),
            })
            .collect();
        assert_eq!(summary_for(&[], &finished), None);
        assert_eq!(summary_for(&[], &[]), None);
    }

    /// The heading that cut off the very thing §201 added (§208).
    #[test]
    fn a_folder_heading_gives_up_its_middle_before_either_end() {
        // The screenshot's case, verbatim: 35 characters into a 32-character budget.
        let real = "background worker / outputs / tables";
        assert_eq!(
            distinguishing_tail(real, PANEL_HEADING_CHARS),
            "…round worker / outputs / tables",
            "what §152's rule did: the producer's name is the part that goes"
        );
        assert_eq!(
            shorten_path_label(real, PANEL_HEADING_CHARS),
            "background worker / … / tables",
            "both ends survive, the middle gives way"
        );

        // Already short enough: untouched, ellipsis and all.
        assert_eq!(
            shorten_path_label("background worker", PANEL_HEADING_CHARS),
            "background worker"
        );
        assert_eq!(
            shorten_path_label("eda / plots", PANEL_HEADING_CHARS),
            "eda / plots"
        );

        // Two segments have no middle to drop, so the tail is trimmed and the head — the producer —
        // is kept whole.
        let two = "exploratory data analysis / a_very_long_output_folder";
        let shortened = shorten_path_label(two, PANEL_HEADING_CHARS);
        assert!(
            shortened.starts_with("exploratory data analysis / "),
            "{shortened}"
        );
        assert!(shortened.chars().count() <= PANEL_HEADING_CHARS, "{shortened}");

        // A head that cannot fit on its own falls back rather than printing all punctuation.
        let hopeless = "an_extremely_long_single_component_with_no_separators_at_all";
        assert_eq!(
            shorten_path_label(hopeless, 12),
            distinguishing_tail(hopeless, 12),
            "one segment keeps §152's rule"
        );
        assert!(shorten_path_label(hopeless, 12).chars().count() <= 12);

        // Whatever the budget, the answer fits it. This is the only thing holding the text inside
        // a fixed-width box, since the label has no ellipsis of its own (§193).
        for max in 6..48 {
            let out = shorten_path_label(real, max);
            assert!(out.chars().count() <= max, "{max}: {out}");
        }
    }

    /// One task, one thread, one name — the only attribution available without guessing.
    #[test]
    fn files_under_a_worker_thread_are_named_after_the_worker() {
        let thread = "019fe9f6-9126-7710-a806-35d5e09170a4";
        let outputs: Vec<workspace::Output> = [
            PathBuf::from("summary.csv"),
            PathBuf::from(thread).join("plots/yield.png"),
        ]
        .into_iter()
        .map(|name| workspace::Output {
            path: name.clone(),
            name: name.to_string_lossy().into_owned(),
            kind: workspace::Kind::Other,
            bytes: 1,
            modified: std::time::SystemTime::UNIX_EPOCH,
        })
        .collect();

        let tasks = vec![protocol::AsyncTask {
            task_id: "t-1".into(),
            thread_id: thread.into(),
            agent_name: "background_worker".into(),
            status: "success".into(),
            description: String::new(),
            pending: None,
            error: None,
            activity: None,
            todos: Vec::new(),
            owner: String::new(),
        }];

        let groups = by_producer(&outputs, &tasks, &std::collections::HashMap::new());
        assert_eq!(
            groups.len(),
            2,
            "conversation and worker are two bodies of work"
        );
        assert_eq!(groups[0].0, None, "the conversation's own files lead");
        assert_eq!(groups[1].0.as_deref(), Some("background worker"));

        assert_eq!(produced_by(None, &tasks), None);
        assert_eq!(
            produced_by(Some(thread), &tasks).as_deref(),
            Some("background worker"),
        );
        // A reload that carried no task list still knows a worker wrote these, and says only that.
        assert_eq!(
            produced_by(Some(thread), &[]).as_deref(),
            Some("a background task"),
        );
        assert_eq!(images_heading(1, None), "1 image");
        assert_eq!(
            images_heading(5, Some("background worker")),
            "5 images from background worker",
        );
    }

    /// The half §199 could not do without the backend writing it down (§201).
    #[test]
    fn the_manifest_names_the_specialist_the_folder_cannot() {
        let thread = "019fe9f6-9126-7710-a806-35d5e09170a4";
        let outputs: Vec<workspace::Output> = [
            PathBuf::from("plots").join("yield.png"),
            PathBuf::from("notes.md"),
            PathBuf::from(thread).join("worker.csv"),
        ]
        .into_iter()
        .map(|name| workspace::Output {
            path: name.clone(),
            name: name.to_string_lossy().into_owned(),
            kind: workspace::Kind::Other,
            bytes: 1,
            modified: std::time::SystemTime::UNIX_EPOCH,
        })
        .collect();

        let tasks = vec![protocol::AsyncTask {
            task_id: "t-1".into(),
            thread_id: thread.into(),
            agent_name: "background_worker".into(),
            status: "success".into(),
            description: String::new(),
            pending: None,
            error: None,
            activity: None,
            todos: Vec::new(),
            owner: String::new(),
        }];

        // Forward slashes, as the backend writes them — matched against a name Windows spells
        // with backslashes. The two must not be able to disagree.
        let mut wrote = std::collections::HashMap::new();
        wrote.insert(
            "plots/yield.png".to_string(),
            "exploratory_data_analysis".to_string(),
        );
        // Recorded inside the worker's own run, where the manifest sees *its* coordinator. The
        // folder outranks it, or `background worker` would be renamed to `coordinator`.
        wrote.insert(
            format!("{thread}/worker.csv"),
            "coordinator".to_string(),
        );

        let groups = by_producer(&outputs, &tasks, &wrote);
        let named: Vec<Option<&str>> = groups.iter().map(|(by, _)| by.as_deref()).collect();
        assert_eq!(
            named,
            vec![
                None,
                Some("exploratory data analysis"),
                Some("background worker")
            ],
            "the conversation's own files lead, then one group per author"
        );
        assert_eq!(groups[0].1.len(), 1, "notes.md has no record and stays unlabelled");
        assert_eq!(groups[0].1[0].name, "notes.md");
    }

    /// `n` outputs, alternating image / not, named so a failure says which one moved.
    fn sample_outputs(kinds: &[workspace::Kind]) -> Vec<workspace::Output> {
        kinds
            .iter()
            .enumerate()
            .map(|(at, kind)| {
                let name = format!("file-{at}");
                workspace::Output {
                    path: PathBuf::from(&name),
                    name,
                    kind: *kind,
                    bytes: 1,
                    modified: std::time::SystemTime::UNIX_EPOCH,
                }
            })
            .collect()
    }

    #[test]
    fn images_and_other_files_are_two_groups_that_keep_their_order() {
        use workspace::Kind::{Data, Document, Figure};
        // The researcher's own boundary: "I want to group images and in another group other
        // files." A folder of seven plots and one summary CSV used to put the CSV in the middle
        // of the strip you flick through looking for a figure.
        let outputs = sample_outputs(&[Figure, Data, Figure, Document, Figure]);
        let (images, others) = split_images(&outputs);
        assert_eq!(
            images.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            ["file-0", "file-2", "file-4"]
        );
        assert_eq!(
            others.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            ["file-1", "file-3"],
            "listing order has to survive the split, or the panel reshuffles"
        );

        // Neither group is invented: a run with no figures gets no image grid at all.
        let (none, all) = split_images(&sample_outputs(&[Data, Document]));
        assert!(none.is_empty());
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn the_last_visible_tile_counts_exactly_the_images_it_hides() {
        // The `+N` arithmetic, which is off-by-one bait: with four tiles and eight images the
        // fourth tile is *shown*, so it stands in for the other five — not four, and not six.
        // WhatsApp's own grid is the reference the researcher pointed at, and it reads `+5`.
        // (tiles drawn, images the last one stands in for)
        assert_eq!(image_grid_shape(8), (4, 5), "eight images, four tiles");
        assert_eq!(image_grid_shape(5), (4, 2));
        // Exactly the cap, and under it: nothing is hidden, so no tile carries a count.
        assert_eq!(image_grid_shape(4), (4, 0));
        assert_eq!(image_grid_shape(3), (3, 0));
        assert_eq!(image_grid_shape(1), (1, 0));
        assert_eq!(image_grid_shape(0), (0, 0));

        // Every image is either visible or counted, at every size. This is the property the
        // off-by-one broke: with `total - tiles` the fourth picture was neither.
        for total in 0..40usize {
            let (shown, hidden) = image_grid_shape(total);
            let visible = if hidden > 0 { shown - 1 } else { shown };
            assert_eq!(visible + hidden, total, "{total} images went unaccounted for");
        }
    }

    #[test]
    fn every_sidebar_menu_offers_what_its_control_is_about() {
        let conversation = protocol::Conversation {
            thread_id: "t-1".into(),
            title: "Kiwi grading".into(),
            project: Some("Late blight".into()),
            updated_at: String::new(),
        };
        let labels = |menu: &SidebarMenu| -> Vec<String> {
            menu.rows().into_iter().map(|row| row.label).collect()
        };

        // The `New` button names both kinds of new thing. Before this, creating a project meant
        // opening a conversation first and filing it afterwards — a route with no button (§165).
        assert_eq!(
            labels(&SidebarMenu::New),
            ["New conversation", "New project…"]
        );

        // A row offers exactly what its two hover chips did, as words.
        assert_eq!(
            labels(&SidebarMenu::Conversation(conversation.clone())),
            ["Rename", "Delete"]
        );

        // A heading names the project it acts on, so a menu floating over the list still says
        // which one it belongs to.
        let project = SidebarMenu::Project {
            name: "Late blight".into(),
            conversations: vec![conversation],
        };
        // **`Open folder` sits between them, and the order is the point.** The two rows either
        // side create and destroy; this one only looks. Putting a plain action next to a
        // destructive one is how a menu gets misread in a hurry, so the safe row separates them.
        assert_eq!(
            labels(&project),
            ["New conversation in Late blight", "Open folder", "Delete project"]
        );

        // Exactly one destructive row per menu, and never the first — the row a mis-aimed click
        // lands on should not be the irreversible one.
        for menu in [SidebarMenu::New, project] {
            let rows = menu.rows();
            assert!(!rows[0].danger, "the first row is destructive");
            assert!(rows.iter().filter(|row| row.danger).count() <= 1);
        }
    }

    #[test]
    fn an_answer_naming_files_is_read_without_inventing_claims() {
        // The real answer from the run that prompted this, trimmed. Every one of these is a
        // claim the panel could not show (§42).
        let answer = "EDA completed with the exploratory subagent.\n\nArtifacts:\n\
            · hola_dummy_dataset.csv\n· hola_eda_numeric_summary.csv\n\
            · outputs/plots/hola_eda_overview.png\n· `hola_eda_findings.txt`\n";
        assert_eq!(
            named_files(answer),
            [
                "hola_dummy_dataset.csv",
                "hola_eda_numeric_summary.csv",
                // The path is stripped: what matters is whether the file exists, not whether the
                // model recited the directory correctly.
                "hola_eda_overview.png",
                "hola_eda_findings.txt",
            ]
        );
    }

    #[test]
    fn prose_that_merely_looks_like_a_filename_is_not_a_claim() {
        // **A false positive is worse than the bug.** It puts a correction under a sentence that
        // was fine, and a warning that cries wolf is one nobody reads the day it is right.
        for innocent in [
            "Strongest numeric relationship: annual_income vs monthly_spend = 0.96",
            "Conversion is rare: 3.1%, so the dataset is imbalanced",
            "See https://doi.org/10.21223/P3/MO4PSJ for the dataset",
            "Missingness in annual_income (7.81%), income_band (7.81%)",
            "I ran it with uv, then re-ran setup-wsl.sh and main.rs compiled",
            "e.g. the third column",
            "version 2.7.11",
        ] {
            assert!(
                named_files(innocent).is_empty(),
                "invented a claim in: {innocent}"
            );
        }

        // A number with a real extension is still not a name — `4.png` could be a file, but a
        // stem of one character in running prose is noise far more often than it is an artifact.
        assert!(named_files("figure 4.png").is_empty());
        assert_eq!(named_files("fig4.png"), ["fig4.png"]);
    }

    #[test]
    fn a_file_named_twice_is_reported_once_and_punctuation_is_not_part_of_its_name() {
        assert_eq!(
            named_files("I wrote summary.csv. Then I updated summary.csv!"),
            ["summary.csv"]
        );
        assert_eq!(named_files("saved to (results.png),"), ["results.png"]);
        assert_eq!(named_files("**plot.png**"), ["plot.png"]);
    }

    #[test]
    fn a_grid_stays_a_block_rather_than_spanning_the_conversation() {
        // The complaint this exists for: *"the grouping occupies too much space in the
        // conversation (too wide)."* A grid of `flex_1` tiles is as wide as whatever holds it, so
        // the width has to come from the tiles. Two fixed tiles plus one gap, and nothing about
        // the window or the panel enters into it.
        let width = |tile: f32| tile * GRID_COLUMNS as f32 + GRID_GAP;

        // The panel is roughly 330px inside its padding, so the compact block has to fit that.
        assert!(width(GRID_TILE_COMPACT) <= 320., "{}", width(GRID_TILE_COMPACT));
        // And the transcript block is close to the phone gallery it imitates — about 415px in the
        // screenshot the researcher sent — not the full width of the conversation.
        let roomy = width(GRID_TILE_ROOMY);
        assert!((400.0..=440.0).contains(&roomy), "{roomy}");

        // Two columns, four tiles: the 2×2 that makes `+N` land on the bottom-right.
        assert_eq!(IMAGE_GRID_TILES % GRID_COLUMNS, 0);
        assert_eq!(IMAGE_GRID_TILES / GRID_COLUMNS, 2, "two rows, not three");

        // A name is shortened to something that actually fits, and never to nothing — §59's bare
        // `…` is what happens when the layout is asked to do this instead.
        assert!(name_chars(GRID_TILE_COMPACT) >= 20, "{}", name_chars(GRID_TILE_COMPACT));
        assert!(name_chars(GRID_TILE_ROOMY) > name_chars(GRID_TILE_COMPACT));
        assert_eq!(name_chars(0.), 8, "a floor, so a name is never cut to nothing");
        // The tail is what tells two summaries apart, and the result is exactly as long as the
        // tile allows — 22 characters for a 148px one, ellipsis included.
        let shortened = distinguishing_tail(
            "kiwi_quality_summary_statistics.csv",
            name_chars(GRID_TILE_COMPACT),
        );
        assert_eq!(shortened, "…ummary_statistics.csv");
        assert_eq!(shortened.chars().count(), name_chars(GRID_TILE_COMPACT));
        assert!(shortened.ends_with(".csv"), "the extension has to survive");
    }

    #[test]
    fn stepping_through_a_preview_wraps_and_never_leaves_the_set() {
        use workspace::Kind::Figure;
        let outputs = sample_outputs(&[Figure, Figure, Figure]);
        let mut preview = Preview::opening(outputs.clone(), 1).expect("three files");
        assert_eq!(preview.current().name, "file-1");

        preview.step(1);
        assert_eq!(preview.current().name, "file-2");
        // Past the end comes back to the start. The counter says which of how many, so wrapping
        // cannot be mistaken for a dead button — and comparing the first plot of a series with
        // the last should not mean travelling back through the middle.
        preview.step(1);
        assert_eq!(preview.current().name, "file-0");
        preview.step(-1);
        assert_eq!(preview.current().name, "file-2");

        // An index beyond the set is clamped rather than panicking: a click can arrive after the
        // files behind it were moved or deleted, which has happened on this project's own
        // evidence (§159).
        let clamped = Preview::opening(outputs, 99).expect("still three files");
        assert_eq!(clamped.current().name, "file-2");

        // Nothing to show is `None`, not an empty preview that panics on `current()`.
        assert!(Preview::opening(Vec::new(), 0).is_none());

        // A lone file has nowhere to step, and asking must not move it anywhere.
        let mut single =
            Preview::single(sample_outputs(&[Figure]).remove(0)).expect("one file");
        single.step(1);
        single.step(-1);
        assert_eq!(single.current().name, "file-0");
        assert_eq!(single.items.len(), 1);
    }

    #[test]
    fn a_thumbnail_names_the_file_instead_of_its_shared_uuid_prefix() {
        let relative = PathBuf::from("019fe9f6-9126-7710-a806-35d5e09170a4")
            .join("guinea_pig_eda_output/plots/health_by_activity_box.png");
        let output = workspace::Output {
            path: relative.clone(),
            name: relative.to_string_lossy().into_owned(),
            kind: workspace::Kind::Figure,
            bytes: 1,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };

        assert_eq!(output_filename(&output), "health_by_activity_box.png");
        let long = distinguishing_tail("shared-prefix-but-the-useful-name-is-at-the-end.csv", 24);
        assert!(long.starts_with('…'), "{long}");
        assert!(long.ends_with("name-is-at-the-end.csv"), "{long}");
        assert_eq!(long.chars().count(), 24);
    }

    #[test]
    fn dragging_a_gallery_thumb_reaches_every_hidden_file() {
        // A 200px thumb journey represents 800px of hidden content. The pointer keeps the
        // same 20px grip inside the thumb, and positions beyond either end clamp instead of
        // exposing blank space (docs §158).
        let offset = |pointer| {
            horizontal_drag_offset(px(pointer), px(100.), px(20.), px(200.), px(800.))
        };
        assert_eq!(offset(0.), px(0.));
        assert_eq!(offset(120.), px(0.));
        assert_eq!(offset(220.), px(-400.));
        assert_eq!(offset(400.), px(-800.));
    }

    #[test]
    fn a_gallery_thumb_never_grows_wider_than_the_rail_it_sits_in() {
        // A wide rail: the thumb is proportional, and there is room to drag it.
        let wide = horizontal_thumb_width(px(400.), px(800.));
        assert!(wide > px(28.) && wide < px(400.), "{wide:?}");

        // A long rail: proportional would be a few pixels, so the 28px floor applies.
        assert_eq!(horizontal_thumb_width(px(300.), px(9_000.)), px(28.));

        // A rail narrower than that floor is the case the floor alone gets wrong. The thumb has
        // to stop at the track width, because `travel = viewport - thumb` going negative paints
        // it outside the track and leaves it undraggable — a control that reads as broken rather
        // than as absent.
        for narrow in [1., 10., 27.9] {
            let thumb = horizontal_thumb_width(px(narrow), px(500.));
            assert_eq!(thumb, px(narrow), "a {narrow}px rail");
            assert!(px(narrow) - thumb >= px(0.), "travel went negative at {narrow}");
        }
    }

    #[test]
    fn a_failed_delete_keeps_the_conversation_visible() {
        assert!(matches!(
            resolve_delete(
                "conversation",
                Some(Err(anyhow::anyhow!("backend unavailable")))
            ),
            DeleteResolution::Keep(message) if message.contains("backend unavailable")
        ));
        assert!(matches!(
            resolve_delete("conversation", None),
            DeleteResolution::Keep(message) if message.contains("still shown")
        ));
        assert!(matches!(
            resolve_delete(
                "conversation",
                Some(Ok(sidecar::DeleteOutcome { files_error: None }))
            ),
            DeleteResolution::Remove { files_error: None }
        ));
    }

    #[test]
    fn a_file_cleanup_failure_does_not_resurrect_a_deleted_conversation() {
        // HTTP succeeded, so the durable conversation is gone. A locked Explorer folder is a
        // recoverable orphan to report, not grounds to put back a row that can no longer open.
        assert!(matches!(
            resolve_delete(
                "conversation",
                Some(Ok(sidecar::DeleteOutcome {
                    files_error: Some("folder is open".into())
                }))
            ),
            DeleteResolution::Remove {
                files_error: Some(message)
            } if message == "folder is open"
        ));
    }

    #[test]
    fn deleting_a_project_targets_every_conversation_not_only_a_filtered_row() {
        let conversations = vec![
            protocol::Conversation {
                thread_id: "one".into(),
                project: Some("Late blight".into()),
                title: "Visible in search".into(),
                updated_at: String::new(),
            },
            protocol::Conversation {
                thread_id: "two".into(),
                project: Some("Late blight".into()),
                title: "Hidden by search".into(),
                updated_at: String::new(),
            },
        ];
        let target = DeleteTarget::Project {
            name: "Late blight".into(),
            conversations,
        };
        assert_eq!(target.thread_ids(), vec!["one", "two"]);
        assert!(target.contains_thread("two"));
    }

    #[test]
    fn deleting_a_projects_last_conversation_ends_the_active_project() {
        let other = protocol::Conversation {
            thread_id: "other".into(),
            project: Some("Yield trials".into()),
            title: "Other work".into(),
            updated_at: String::new(),
        };
        assert!(project_exists(std::slice::from_ref(&other), "Yield trials"));
        assert!(
            !project_exists(std::slice::from_ref(&other), "Late blight"),
            "an empty project cannot survive as an active selection"
        );
    }

    #[test]
    fn every_ui_icon_is_embedded_rather_than_read_from_beside_the_executable() {
        // The half a test can actually settle: the bytes are *in* the binary, so a Windows
        // install with no source-tree-relative assets directory resolves them exactly as
        // `cargo run` does. `include_bytes!` makes a missing file a build failure, and this
        // makes a path declared in `ICON_PATHS` but never wired into `load` a test failure.
        let assets = Assets;
        assert_eq!(assets.list("icons/").unwrap().len(), ICON_PATHS.len());

        // **Every mark `file_mark` can return is a real, loadable asset.** This is the assertion
        // worth having: the mapping is a `match` over extensions, and a new arm naming an icon
        // nobody added would draw nothing at all — the §157 failure again, one layer along. A
        // count would only have said the number changed.
        for name in [
            "a.csv", "b.png", "c.py", "d.ipynb", "e.json", "f.html", "g.md", "h.log", "i.pdf",
            "j.zip", "k.sqlite", "l.unheard-of", "no-extension",
        ] {
            let (icon, _ink) = file_mark(std::path::Path::new(name));
            assert!(ICON_PATHS.contains(&icon), "{name} draws undeclared {icon}");
            assert!(assets.load(icon).unwrap().is_some(), "{icon} is not embedded");
        }
        for path in ICON_PATHS {
            let bytes = assets.load(path).unwrap().expect("declared icon is loadable");
            let source = std::str::from_utf8(&bytes).expect("hand-authored SVG is UTF-8");
            assert!(source.contains("viewBox=\"0 0 24 24\""), "{path} has no common canvas");
        }
        assert!(assets.load("icons/missing.svg").unwrap().is_none());

        // **Not asserted: that they are tintable.** The original test read `currentColor` out of
        // the file and called that tintable. GPUI never reads it — it rasterises the SVG and
        // multiplies by `style.text.color`, so whether an icon appears is decided entirely by
        // the element's own colour and not by anything in these bytes. That assertion passed
        // just as happily when all four icons rendered nothing at all, which is the state this
        // PR arrived in. What replaces it is `app_icon` taking `ink` as an argument, so the
        // compiler refuses a call site that forgets (docs §157).
    }

    #[test]
    fn csv_columns_get_distinct_colours_from_the_live_palette() {
        // The live palette is global, so a test that changes it must not run beside one
        // that reads it. §197 fixed this for `theme.rs`'s own tests and could not reach
        // these three files, because the lock lived in a private test module.
        let _theme = crate::theme::theme_lock::hold();
        assert!(is_delimited("papas.csv"));
        assert!(is_delimited("MODELO.TSV"), "case is not a format");
        assert!(!is_delimited("informe.md"));

        // Adjacent columns must differ, or the colouring does nothing for the eye.
        for column in 0..12 {
            assert_ne!(
                column_colour(column),
                column_colour(column + 1),
                "columns {column} and {} share a colour",
                column + 1
            );
        }
        // And they follow the theme, so a light palette does not get dark-theme inks.
        theme::apply(&theme::PAPER);
        assert_eq!(column_colour(0), theme::PAPER.text);
        theme::apply(&theme::MINI_ME_DARK);
        assert_eq!(column_colour(0), theme::MINI_ME_DARK.text);
    }

    #[test]
    fn repeated_steps_fold_but_the_order_survives() {
        // Straight from a screenshot: the coordinator hunting for a file it could not
        // find. Twenty lines of this pushed the answer off the screen (docs §47).
        let hunting: Vec<String> = ["read_file", "read_file", "read_file", "ls", "ls", "ls"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(fold_steps(&hunting), vec!["read_file ×3", "ls ×3"]);

        // Only *consecutive* runs fold. Going back to a tool after using another one is a
        // different story from using it six times, and collapsing both to one line would
        // erase the difference.
        let alternating: Vec<String> = ["glob", "read_file", "glob"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(fold_steps(&alternating), vec!["glob", "read_file", "glob"]);

        // A lone step keeps its plain label — "×1" is noise.
        assert_eq!(fold_steps(&["execute".to_string()]), vec!["execute"]);
        assert!(fold_steps(&[]).is_empty());
    }

    /// A real delegated turn, reduced to fit the repo (see the fixture's header).
    /// Replaying it is what proves the trace works on *measured* wire data rather
    /// than on shapes hand-written from the docs.
    const DELEGATED_TURN: &[u8] = include_bytes!("../tests/fixtures/delegated-turn.sse");

    #[test]
    fn a_real_delegated_turn_produces_one_named_trace_with_its_steps() {
        let mut statuses = Vec::new();
        let (message, outputs) =
            decode_capture(DELEGATED_TURN, |status| statuses.push(status.to_string()));

        // The coordinator's own line: one delegation, announced once, labelled from
        // arguments that arrived across 60 fragments.
        assert_eq!(
            message.steps,
            vec![
                "delegating to academic_researcher — Find the canonical DESeq2 paper. Return a concise citation…"
            ]
        );

        // One group, named by the backend, with the subagent's real tool call in it.
        let [trace] = message.agents.as_slice() else {
            panic!(
                "expected exactly one subagent group, got {}",
                message.agents.len()
            );
        };
        assert_eq!(trace.name, "academic_researcher");
        assert!(trace.ns.starts_with("tools:"), "{}", trace.ns);
        assert_eq!(trace.steps, vec!["search_paper_by_title"]);

        // Its answer was a JSON object, so the trace shows the readable part.
        let preview = protocol::summarize_agent_result(&trace.text);
        assert!(
            preview.starts_with("The canonical DESeq2 paper"),
            "{preview}"
        );
        assert!(preview.ends_with("· 1 sources"), "{preview}");

        // The coordinator's answer still arrives, and the outputs panel still fills:
        // subagent frames must not be mistaken for either.
        assert!(message.body.contains("Genome Biology"), "{}", message.body);
        assert_eq!(
            outputs
                .iter()
                .map(|b| (b.name, b.items.len()))
                .collect::<Vec<_>>(),
            vec![("sources", 1)]
        );

        // Sandbox provisioning reaches the status line — the first turn on a cold
        // thread waits on it, and without this the window looks stuck.
        assert!(
            statuses.iter().any(|status| status == "Creating sandbox…"),
            "{statuses:?}"
        );
    }

    #[test]
    fn the_same_real_turn_lands_in_the_provenance_record() {
        // The cross-layer proof: the record is fed by the *real* decoder on *measured* wire data,
        // not by hand-written events. If `AgentRef.ns` or `lc_agent_name` ever changes shape, this
        // fails here rather than as an empty modal weeks later — which is exactly how the subagent
        // registry went wrong three times (docs §78–§81).
        let mut frames = protocol::SseDecoder::default();
        let mut turn = protocol::TurnDecoder::default();
        let mut record = provenance::Record::default();
        record.begin_turn("Find the canonical DESeq2 paper", 0);
        // A counter, not the clock: arrival order is what the capture fixes, and a test that
        // depended on wall-clock timing would be a test that fails on a slow machine.
        let mut tick = 0u64;
        for frame in frames.push(DELEGATED_TURN) {
            for event in turn.push(&frame) {
                tick += 1;
                match event {
                    TurnEvent::Step {
                        agent: Some(agent), ..
                    }
                    | TurnEvent::SubagentToken { agent, .. } => {
                        record.observe(&agent.ns, &agent.name, tick);
                    }
                    _ => {}
                }
            }
        }

        let [invocation] = record.turns[0].invocations.as_slice() else {
            panic!(
                "expected one invocation, got {:?}",
                record.turns[0].invocations
            );
        };
        assert_eq!(invocation.name, "academic_researcher");
        assert!(invocation.ns.starts_with("tools:"), "{}", invocation.ns);
        // A real interval, not a point: the invocation streamed across many frames.
        assert!(
            invocation.last_seen > invocation.first_seen,
            "{invocation:?}"
        );
        // One kind, visited once, and nothing invented around it.
        let graph = record.graph_of(None);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.nodes[0].name, "academic_researcher");
        assert_eq!(graph.nodes[0].visits, 1);
        assert!(
            graph.edges.is_empty(),
            "one specialist has nothing to point at"
        );
    }

    #[test]
    fn a_context_less_binding_outranks_a_scoped_one() {
        // Why Escape never closed the palette, proven against gpui rather than reasoned about.
        //
        // `escape` is bound twice: to `PaletteDismiss` in the `Palette` context, and to `Dismiss`
        // with no context at all. The comment beside them used to say the scoped one wins because
        // it is "more specific". It does not: `Keymap::binding_enabled` scores a context-less
        // binding at `contexts.len()`, which is deeper than any predicate can match, and matched
        // bindings are sorted deepest-first. `Dismiss` is dispatched, actions stop propagation
        // during the bubble phase (`window.rs`: "Actions stop propagation by default"), and
        // `PaletteDismiss` is never reached.
        //
        // Pinned here so a gpui bump that changes the rule is caught by a failing test rather
        // than by a key that quietly stops working.
        let keymap = gpui::Keymap::new(workbench_key_bindings());
        let stack = [
            gpui::KeyContext::try_from("Palette").expect("a valid context"),
            gpui::KeyContext::try_from("Composer").expect("a valid context"),
        ];
        let (matched, _pending) =
            keymap.bindings_for_input(&[gpui::Keystroke::parse("escape").unwrap()], &stack);
        let first = matched.first().expect("escape matches something");
        assert!(
            first.action().partial_eq(&Dismiss),
            "the unscoped Dismiss is dispatched first, so the palette must close from `dismiss`"
        );
    }

    /// A stale index must never make the highlighted row and the Enter key disagree.
    ///
    /// Three call sites used to clamp `palette_selected` three ways, and the activation path
    /// did not clamp at all — so past the end of a filtered list the palette drew the last row
    /// as chosen and Enter did nothing at all (docs §69).
    #[test]
    fn the_row_drawn_as_chosen_is_the_one_enter_runs() {
        let commands = [Command::OpenSettings];
        for stale in [0usize, 1, 8, 999] {
            let clamped = stale.min(commands.len() - 1);
            assert_eq!(
                clamped, 0,
                "index {stale} should clamp into a one-item list"
            );
            assert_eq!(commands[clamped], Command::OpenSettings);
        }
        // And an empty list chooses nothing rather than panicking on `commands[0]`.
        let empty: Vec<Command> = Vec::new();
        assert_eq!(empty.len().checked_sub(1), None);
    }

    #[test]
    fn the_palette_ranks_matches_rather_than_hiding_them() {
        // "nt" should find "New thread" — initials across words is how you actually
        // type in a palette.
        let ranked = |query: &str| {
            let mut hits: Vec<(i32, usize, &str)> = Command::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(index, command)| {
                    match_score(query, command.label()).map(|score| (score, index, command.label()))
                })
                .collect();
            hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            hits.into_iter()
                .map(|(_, _, label)| label)
                .collect::<Vec<_>>()
        };
        // "nt" is ambiguous — "ruN Turn" matches too — so the test is about *rank*,
        // which is what Enter acts on.
        assert_eq!(ranked("nt")[0], "New thread");
        assert_eq!(ranked("quit"), vec!["Quit"]);
        // Case-insensitive, and spaces in the query are ignored.
        assert_eq!(ranked("RUN T")[0], "Run turn");
        // An empty query lists everything, in declaration order.
        assert_eq!(ranked("").len(), Command::ALL.len());
        assert_eq!(ranked("")[0], "Run turn");
        // Out-of-order letters must not match at all.
        assert!(ranked("tnur").is_empty());
        assert!(ranked("zzz").is_empty());
    }

    #[test]
    fn a_spine_without_suggestions_does_not_erase_them() {
        let with_advice = Project {
            mission: "M".into(),
            completed: vec!["a".into()],
            pending: vec![],
            suggestions: vec![protocol::Suggestion {
                title: "Look for the dataset".into(),
                rationale: "You have the paper".into(),
                prompt: "find the dataset".into(),
            }],
        };
        // Mid-turn snapshot: newer mission and completed work, no advice.
        let mid_turn = Project {
            mission: "M2".into(),
            completed: vec!["a".into(), "b".into()],
            pending: vec!["c".into()],
            suggestions: vec![],
        };
        let merged = merge_spine(Some(&with_advice), mid_turn);
        // State replaces...
        assert_eq!(merged.mission, "M2");
        assert_eq!(merged.completed.len(), 2);
        assert_eq!(merged.pending, vec!["c".to_string()]);
        // ...but the card the user was about to click survives.
        assert_eq!(merged.suggestions.len(), 1);
        assert_eq!(merged.suggestions[0].prompt, "find the dataset");

        // Fresh advice always wins over old advice.
        let replacement = Project {
            suggestions: vec![protocol::Suggestion {
                title: "New".into(),
                rationale: String::new(),
                prompt: "new".into(),
            }],
            ..Default::default()
        };
        assert_eq!(
            merge_spine(Some(&with_advice), replacement).suggestions[0].prompt,
            "new"
        );

        // Nothing to carry over is fine.
        assert!(merge_spine(None, Project::default()).suggestions.is_empty());
    }

    #[test]
    fn secret_fields_are_masked_and_named() {
        // A field that looks like a secret but is not masked would put an API key on
        // screen; one that is masked but has nowhere to go would silently discard it.
        for field in Field::ALL {
            assert!(!field.label().is_empty());
            assert!(!field.placeholder().is_empty());
            match field {
                Field::ApiKey => {
                    assert!(field.is_secret());
                    // The provider key's entry name depends on the provider chosen, so it
                    // is resolved at save time rather than being fixed here.
                    assert!(field.secret_name().is_none());
                }
                Field::AstaToken | Field::AstaApiKey => {
                    assert!(field.is_secret());
                    assert!(field.secret_name().is_some());
                }
                _ => assert!(!field.is_secret(), "{}", field.label()),
            }
        }
    }

    #[test]
    fn the_sign_in_link_is_picked_out_of_the_cli_output() {
        // The real line, verbatim: `asta auth login` prints the device-activation URL and
        // then fails to open it, because there is no browser inside the distro.
        let real =
            "gio: https://auth0.allenai.org/activate?user_code=DPMW-BJCG: Operation not supported";
        assert_eq!(
            first_url(real).as_deref(),
            Some("https://auth0.allenai.org/activate?user_code=DPMW-BJCG"),
            "the trailing colon and message must not come along"
        );

        // Sentence punctuation is not part of the link.
        assert_eq!(
            first_url("Visit https://example.org/a.").as_deref(),
            Some("https://example.org/a")
        );
        assert_eq!(
            first_url("see (https://example.org/b)").as_deref(),
            Some("https://example.org/b")
        );
        // A citation, which is the second caller: one line of the agent's prose with a DOI in it.
        assert_eq!(
            first_url("Smith et al. (2021). Late blight. https://doi.org/10.1234/x.").as_deref(),
            Some("https://doi.org/10.1234/x")
        );
        // `http://`, because plenty of older DOIs and institutional repositories still publish
        // that way and a source row that would not open is indistinguishable from one with no
        // link at all.
        assert_eq!(
            first_url("archived at http://hdl.handle.net/10568/1").as_deref(),
            Some("http://hdl.handle.net/10568/1")
        );
        // Whichever comes first in the line, not whichever scheme is checked first.
        assert_eq!(
            first_url("http://a.org and https://b.org").as_deref(),
            Some("http://a.org")
        );
        assert_eq!(
            first_url("https://a.org and http://b.org").as_deref(),
            Some("https://a.org")
        );

        // Nothing to open.
        assert_eq!(first_url("Waiting for authentication…"), None);
        // A bare scheme is not a link — and the guard has to be against *its own* scheme. A
        // fixed floor of `"http://".len()` would let this one through, which is exactly what
        // adding the second scheme did until this line caught it.
        assert_eq!(first_url("https://"), None, "a bare scheme is not a link");
        assert_eq!(first_url("http://"), None, "either scheme");
    }

    #[test]
    fn the_device_code_is_pulled_out_of_the_sign_in_url() {
        // A URL is one unbreakable word, so in a 420px pane it runs off the edge — and the
        // code is the part that gets clipped, which is the part the user has to type.
        // Both real lines seen from the CLI.
        assert_eq!(
            device_code("https://auth0.allenai.org/activate?user_code=KFDM-BQQG").as_deref(),
            Some("KFDM-BQQG")
        );
        assert_eq!(
            device_code("https://auth0.allenai.org/activate?user_code=DPMW-BJCG&x=1").as_deref(),
            Some("DPMW-BJCG"),
            "a following parameter must not come along"
        );
        assert_eq!(device_code("https://example.org/plain"), None);
        assert_eq!(device_code("https://example.org/a?user_code="), None);
    }

    /// The loop §73 asked for: two specialists, and a return across a turn boundary.
    fn a_loop() -> provenance::Record {
        let mut record = provenance::Record::default();
        record.begin_turn("find papers", 0);
        record.observe("tools:a", "academic_researcher", 100);
        record.observe("tools:a", "academic_researcher", 1_200);
        record.begin_turn("theorise", 1_300);
        record.observe("tools:b", "theorizer", 1_400);
        record.observe("tools:b", "theorizer", 2_000);
        record.begin_turn("and check that", 2_100);
        record.observe("tools:c", "academic_researcher", 2_200);
        record.observe("tools:c", "academic_researcher", 2_800);
        record
    }

    #[test]
    fn the_graph_exports_carry_the_hedge_the_drawing_does() {
        let graph = a_loop().graph_of(None);

        let diagram = mermaid(&graph);
        assert!(diagram.starts_with("flowchart TD\n"));
        // A revisited specialist is one node saying twice, exactly as on screen.
        assert!(diagram.contains("[\"academic researcher ×2\"]"), "{diagram}");
        // **The distinction survives the export.** A diagram pasted into a methods section that
        // drew observed order as a causal arrow would be a stronger claim than the record makes.
        assert!(diagram.contains("-.->"), "{diagram}");
        assert!(diagram.contains("came back to"), "{diagram}");
        // Every node referenced by an edge is one the diagram declared.
        for line in diagram.lines().filter(|line| line.contains("->")) {
            for token in line.split_whitespace().filter(|t| t.starts_with('n')) {
                assert!(
                    diagram.contains(&format!("    {token}[")),
                    "{token} is used but never declared\n{diagram}"
                );
            }
        }

        let drawing = provenance_svg(&graph);
        assert!(drawing.starts_with("<svg xmlns="));
        assert!(drawing.ends_with("</svg>\n"));
        // One dashed path per cross-turn edge, and the arcs are quadratics.
        assert_eq!(drawing.matches("stroke-dasharray").count(), graph.edges.len());
        assert!(drawing.contains(" Q "), "{drawing}");
        // Tags open and close in pairs — the cheapest check that this parses at all.
        assert_eq!(drawing.matches("<text").count(), graph.nodes.len() * 2);

        // A name carrying markup must not become markup. `escape_xml` is the only thing between
        // a subagent name and a file that will not open.
        let mut hostile = provenance::Record::default();
        hostile.begin_turn("q", 0);
        hostile.observe("tools:a", "a<b & \"c\"", 10);
        let escaped = provenance_svg(&hostile.graph_of(None));
        assert!(escaped.contains("a&lt;b &amp; &quot;c&quot;"), "{escaped}");
        assert!(!escaped.contains("a<b"), "{escaped}");

        // An empty record exports an empty diagram rather than something malformed.
        let nothing = provenance::Record::default().graph_of(None);
        assert_eq!(mermaid(&nothing), "flowchart TD\n");
        assert!(provenance_svg(&nothing).ends_with("</svg>\n"));
    }

    /// A source as the backend sends it: prose, and optionally a structured link.
    fn cited(citation: &str, link: Option<&str>) -> protocol::Source {
        protocol::Source {
            citation: citation.to_string(),
            link: link.map(str::to_string),
        }
    }

    #[test]
    fn an_unverified_doi_never_becomes_a_link() {
        // The real failure, reported from a live run: a citation about potato late blight linked
        // to a paper on recombination in the mammalian germ line. The model had invented a DOI,
        // and an invented DOI is a real DOI belonging to somebody else — so routing it through
        // Semantic Scholar made it *resolve* instead of 404ing. The one accidental safeguard a
        // bad DOI had was removed by the thing meant to improve it.
        let invented = cited(
            "Lindqvist-Kreuze, H., & Perez, W. G. (2010). Field resistance to Phytophthora \
             infestans in native Andean potato landraces. Euphytica, 174(2), 217-227. \
             https://doi.org/10.1007/s10681-010-0147-6",
            None,
        );

        // Unchecked: no link at all. Not a plausible one.
        assert_eq!(scholar_link(&invented, None, None), None);
        // Checked and wrong: still no link.
        assert_eq!(
            scholar_link(
                &invented,
                Some(&references::Verdict::Mismatch {
                    found: "Recombination in the mammalian germ line".into()
                }),
                None
            ),
            None
        );
        assert_eq!(
            scholar_link(&invented, Some(&references::Verdict::Unregistered), None),
            None
        );

        // Checked and right: the link appears, through Semantic Scholar.
        assert_eq!(
            scholar_link(&invented, Some(&references::Verdict::Confirmed), None).as_deref(),
            Some("https://api.semanticscholar.org/DOI:10.1007/s10681-010-0147-6")
        );

        // Repaired: the registry's DOI, not the citation's.
        assert_eq!(
            scholar_link(
                &invented,
                Some(&references::Verdict::Unregistered),
                Some(&references::Repair {
                    doi: "10.1007/s10681-009-0053-y".into(),
                    title: "The real paper".into()
                })
            )
            .as_deref(),
            Some("https://api.semanticscholar.org/DOI:10.1007/s10681-009-0053-y")
        );

        // A corpus id from the search needs no check — nothing composed it.
        let from_search = cited(
            "Monteros-Altamirano, Á. (2021). Late blight resistance of Ecuadorian landraces.",
            Some("https://api.semanticscholar.org/CorpusID:237744014"),
        );
        assert_eq!(
            scholar_link(&from_search, None, None).as_deref(),
            Some("https://api.semanticscholar.org/CorpusID:237744014")
        );

        // A link with no DOI in it — a thesis in a repository — is kept: unverifiable, but not
        // dressed up as a resolved paper.
        let thesis = cited(
            "de Haan, S. (2009). Potato Diversity at Height. PhD thesis, Wageningen University.",
            Some("https://library.wur.nl/WebQuery/wurpubs/399292"),
        );
        assert_eq!(
            scholar_link(&thesis, None, None).as_deref(),
            Some("https://library.wur.nl/WebQuery/wurpubs/399292")
        );
    }

    #[test]
    fn the_structured_link_wins_and_a_disagreement_is_said_out_loud() {
        // The whole point of decoding `link`: the DOI in the prose is written by the model, the
        // field is what Semantic Scholar returned. When only one exists, use it.
        let field_only = cited("Hijmans & Spooner (2001).", Some("https://doi.org/10.2307/3558433"));
        assert_eq!(
            link_for(&field_only).as_deref(),
            Some("https://doi.org/10.2307/3558433")
        );
        assert_eq!(disputed_link(&field_only), None, "nothing to disagree with");

        let prose_only = cited("Smith (2021). https://doi.org/10.1111/ppa.13400", None);
        assert_eq!(
            link_for(&prose_only).as_deref(),
            Some("https://doi.org/10.1111/ppa.13400"),
            "with no field, the prose is all there is"
        );

        // Both present and pointing at different papers — the case that sent someone to the
        // wrong article. The field is used, and the discrepancy is reported rather than hidden.
        let disagreeing = cited(
            "Hijmans & Spooner (2001). Am. J. Bot. https://doi.org/10.2307/3558457",
            Some("https://doi.org/10.2307/3558433"),
        );
        assert_eq!(
            link_for(&disagreeing).as_deref(),
            Some("https://doi.org/10.2307/3558433")
        );
        assert_eq!(
            disputed_link(&disagreeing).as_deref(),
            Some("https://doi.org/10.2307/3558457")
        );

        // Differences that are not disagreements. Flagging these would teach people to ignore
        // the warning, which costs more than the warning is worth.
        for same in [
            "https://doi.org/10.1111/ppa.13400/",
            "http://doi.org/10.1111/ppa.13400",
            "https://dx.doi.org/10.1111/ppa.13400",
            "https://doi.org/10.1111/PPA.13400",
        ] {
            let pair = cited(
                &format!("Smith (2021). {same}"),
                Some("https://doi.org/10.1111/ppa.13400"),
            );
            assert_eq!(disputed_link(&pair), None, "{same} is the same paper");
        }

        // The prose loses its URL so it can be read as prose, and the trailing full stop that
        // the URL was carrying goes with it.
        assert_eq!(
            without_url("Smith (2021). Late blight. https://doi.org/10.1/x."),
            "Smith (2021). Late blight"
        );
        assert_eq!(without_url("No link here."), "No link here.");
    }

    #[test]
    fn bibtex_is_importable_and_invents_nothing() {
        // Every source verified, so the only annotations below are the ones the entries earn.
        let verified = [references::Origin::Search; 3];
        let entries = bibliography(
            &[
                cited(
                    "Smith, J. et al. (2021). Late blight resistance. Plant Pathology 70(4). \
                     https://doi.org/10.1111/ppa.13400",
                    None,
                ),
                cited("CIP Dataverse: Andean potato trials, 2019", None),
                cited("   ", None),
            ],
            &verified,
        );

        // Two entries, not three: a blank source is not a reference.
        assert_eq!(entries.matches("@misc{").count(), 2);
        // Keys are distinct, or a reference manager silently keeps one of them.
        assert!(entries.contains("@misc{minime1,"));
        assert!(entries.contains("@misc{minime2,"));
        // Verbatim in `note`, with nothing split into author/title/year — a mis-split citation
        // does not look broken in a manuscript, it looks like a citation with the wrong author.
        assert!(entries.contains("note = {Smith, J. et al. (2021). Late blight resistance."));
        assert!(!entries.contains("author ="), "no field is inferred");
        // The URL is the one part extractable without interpretation.
        assert!(entries.contains("url = {https://doi.org/10.1111/ppa.13400}"));
        // A source with no link gets no empty `url`, which would import as a broken one.
        let second = entries.split("@misc{minime2,").nth(1).expect("the entry");
        assert!(!second.contains("url ="));

        // BibTeX's own syntax cannot come out of a citation and truncate the file. A stray
        // brace ends an entry early and takes every entry after it.
        let hostile = bibliography(
            &[cited("A title with {braces} and a \\command", None)],
            &verified,
        );
        assert_eq!(hostile.matches('{').count(), hostile.matches('}').count());
        assert!(!hostile.contains("{braces}"));
        assert!(hostile.contains("\\\\command"));

        // A doubtful reference carries its doubt into the reference manager. Somebody importing
        // forty of these should not have to come back here to find out which two to check.
        let doubtful = bibliography(
            &[cited(
                "Hijmans & Spooner (2001). https://doi.org/10.2307/3558457",
                Some("https://doi.org/10.2307/3558433"),
            )],
            &verified,
        );
        assert!(doubtful.contains("url = {https://doi.org/10.2307/3558433}"));
        assert!(doubtful.contains("annote = {unverified:"), "{doubtful}");

        assert!(bibliography(&[], &[]).is_empty(), "nothing to copy is empty");

        // **An unverified reference says so in the file that leaves the app.** The panel can be
        // re-read; a `.bib` in somebody's Zotero is on its own, and it is the copy that ends up
        // in a manuscript (docs §185).
        let recalled = bibliography(
            &[cited("Barrera et al. (2016). Andean tuber diversity.", None)],
            &[references::Origin::Unconfirmed],
        );
        assert!(recalled.contains("annote = {unverified:"), "{recalled}");
        assert!(recalled.contains("not from a search"), "{recalled}");

        // A reference that came out of a search carries no such note — the whole value of the
        // mark is that it appears on the ones that need a person.
        let searched = bibliography(
            &[cited("Barrera et al. (2016). Andean tuber diversity.", None)],
            &[references::Origin::Search],
        );
        assert!(!searched.contains("annote ="), "{searched}");

        // An origin list shorter than the sources must not annotate the wrong entry, or say
        // nothing about one it has no answer for. `get` returning `None` means "no claim".
        let ragged = bibliography(
            &[
                cited("First, unverified.", None),
                cited("Second, no origin recorded.", None),
            ],
            &[references::Origin::Unconfirmed],
        );
        assert_eq!(ragged.matches("annote =").count(), 1, "{ragged}");
        let second = ragged.split("@misc{minime2,").nth(1).expect("the entry");
        assert!(!second.contains("annote ="), "{second}");
    }

    #[test]
    fn a_dropped_file_is_named_the_way_the_agent_opens_it() {
        // The path has to be spelled the way the *agent* would open it. On Windows the agent lives
        // inside WSL, so a reference naming `C:\…` would send it looking for a file that does not
        // exist there — and the researcher would have no idea why.
        let _env = backend::env_lock::hold();
        let config = backend::BackendConfig {
            wsl: Some(backend::WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            ..Default::default()
        };
        let translated =
            config.path_for_backend(std::path::Path::new(r"C:\Users\LENOVO\Documents\yield.csv"));
        assert_eq!(translated, "/mnt/c/Users/LENOVO/Documents/yield.csv");
        assert!(!translated.contains('\\'), "no Windows path survives: {translated}");
    }

    fn attached(label: &str, reference: &str) -> Attachment {
        Attachment {
            label: label.to_string(),
            source: std::path::PathBuf::from(format!("/downloads/{label}")),
            adopted: reference.starts_with("./"),
            reference: reference.to_string(),
        }
    }

    /// §231: the format four backend prompts agree on, which this client never sent.
    #[test]
    fn attachments_reach_the_agent_as_the_blockquote_the_prompts_describe() {
        let one = with_attachments(
            "please also index this paper",
            &[attached("Tesitelova-2013.pdf", "./Tesitelova-2013.pdf")],
        );
        assert_eq!(
            one,
            "> Attached files (already saved in the sandbox working directory): \
             `./Tesitelova-2013.pdf`\n\nplease also index this paper"
        );
        // `backend/project.py` seeds the mission by dropping every line starting with `>`, so the
        // question has to survive that on its own line.
        let kept: Vec<&str> = one.lines().filter(|line| !line.starts_with('>')).collect();
        assert_eq!(kept.join("").trim(), "please also index this paper");
    }

    #[test]
    fn several_files_are_one_blockquote_and_not_one_each() {
        let many = with_attachments(
            "compare these",
            &[attached("a.csv", "./a.csv"), attached("b.csv", "./b.csv")],
        );
        assert_eq!(many.lines().filter(|l| l.starts_with('>')).count(), 1);
        assert!(many.contains("`./a.csv`, `./b.csv`"));
    }

    /// A file too large to copy in, or one whose copy failed, is outside the workspace — so a
    /// relative reference would resolve to nothing.
    #[test]
    fn a_file_left_where_it_lies_is_named_absolutely() {
        let out = with_attachments(
            "profile this",
            &[attached("huge.tab", "/mnt/d/genomes/huge.tab")],
        );
        assert!(out.contains("`/mnt/d/genomes/huge.tab`"), "{out}");
    }

    #[test]
    fn a_question_with_nothing_attached_is_sent_unchanged() {
        assert_eq!(with_attachments("what is late blight?", &[]), "what is late blight?");
        assert!(attached_blockquote(&[]).is_none());
        // §28's rule survives: no question is invented on the researcher's behalf.
        assert!(!with_attachments("", &[attached("a.csv", "./a.csv")])
            .to_ascii_lowercase()
            .contains("analyse"));
    }

    /// **The order that matters.** `subagent::parse` needs the prompt to *begin* with `/name`, so
    /// the blockquote must be prepended after the command is resolved — otherwise attaching a file
    /// turns a delegated turn into prose, which is §55 and §76's ten-minute silent failure.
    #[test]
    fn a_specialist_command_would_not_survive_the_blockquote_going_first() {
        let typed = "/pdf_librarian index it";
        assert!(subagent::parse(typed).is_some(), "a command on its own");
        let quoted = with_attachments(typed, &[attached("a.pdf", "./a.pdf")]);
        assert!(
            subagent::parse(&quoted).is_none(),
            "which is why `start_turn_as` resolves the specialist first: {quoted}"
        );
    }

    #[test]
    fn removing_a_theme_rewrites_the_name_that_survives_pressing_escape() {
        // The failure this guards: remove the palette you are using, press Esc, and the
        // dismiss path reloads `settings.toml` on the stated grounds that "an unsaved palette
        // was a look, not a change". Deleting a file is not a look — so the dropdown came back
        // reading a theme whose JSON was gone, over a window painted in the default, and no
        // restart cleared it.
        let survivors: Vec<(String, theme::Theme)> = theme::THEMES
            .iter()
            .map(|(name, palette)| ((*name).to_string(), *palette))
            .collect();

        // A name the removal took with it has to be rewritten, wherever it was recorded.
        assert_eq!(
            theme_after_removal("Catppuccin Mocha", &survivors).as_deref(),
            Some(theme::DEFAULT_NAME)
        );

        // A built-in is never rewritten — including when the deleted file was only *overriding*
        // one, which is the case where the name survives and the palette underneath changes.
        assert_eq!(theme_after_removal(theme::DEFAULT_NAME, &survivors), None);
        assert_eq!(theme_after_removal("Bench", &survivors), None);
        // Matched the way the picker matches, or a theme saved in another case is rewritten
        // out from under someone who never removed it.
        assert_eq!(theme_after_removal("bench", &survivors), None);

        // The replacement must itself be loadable, or this trades one dead name for another.
        let replacement = theme_after_removal("gone", &survivors).expect("a replacement");
        assert!(
            survivors
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(&replacement)),
            "{replacement} is not a theme that exists"
        );
    }

    #[test]
    fn a_specialist_pointed_at_another_provider_says_whose_account_pays() {
        let openai = settings::provider("openai").expect("a shipped provider");
        let custom = settings::provider("custom").expect("a shipped provider");

        // The exact row that cost an afternoon: coordinator on `custom` (which is how OpenRouter
        // is reached), specialist offered `gpt-4.1` from the `openai` list, and a key stored for
        // OpenAI — so the old rule said nothing at all, and the turn ran on the wrong account.
        let note = specialist_note(openai, "custom", true).expect("annotated");
        assert!(note.contains("OpenAI"), "{note}");
        assert!(note.contains("billed separately"), "{note}");

        // Still said when there is no key, and the two messages are different: one is a thing to
        // fix before it works, the other is a thing to know before it costs.
        let unkeyed = specialist_note(openai, "custom", false).expect("annotated");
        assert!(unkeyed.contains("no key stored"), "{unkeyed}");
        assert_ne!(note, unkeyed);

        // The provider already running the conversation needs no note — its models are billed
        // exactly where every other turn is, and a line on every row is noise.
        assert_eq!(specialist_note(custom, "custom", true), None);
        assert_eq!(specialist_note(openai, "openai", true), None);
        // Including when that provider has no key: the coordinator's own missing key is
        // §186's problem, refused at the turn, and repeating it on twenty rows helps nobody.
        assert_eq!(specialist_note(custom, "custom", false), None);
    }

    #[test]
    fn a_file_on_a_network_share_is_refused_before_the_turn_rather_than_during_it() {
        // `wsl_path` has no drive letter to work with here, so it passes the path through
        // and the agent receives `//nas/shared/yield.csv` — which exists in no Linux
        // filesystem. Left alone, that surfaces a minute into a turn as `FileNotFoundError`,
        // naming neither the share nor the reason.
        let _env = backend::env_lock::hold();
        let wsl = backend::BackendConfig {
            wsl: Some(backend::WslTarget {
                distro: None,
                dir: "~/Mini-Me".into(),
            }),
            ..Default::default()
        };
        let share = std::path::Path::new(r"\\nas\shared\yield.csv");
        assert!(!wsl.can_open(share));
        assert!(wsl.can_open(std::path::Path::new(r"C:\Users\LENOVO\yield.csv")));
        // A mapped drive letter is fine: WSL mounts those under /mnt like any other.
        assert!(wsl.can_open(std::path::Path::new(r"Z:\shared\yield.csv")));

        // Only WSL can fail this. A backend on this host opens exactly the path the
        // researcher's own file manager handed us, network share or not.
        // Windows deliberately defaults the Python backend to WSL, so `Default` is not a host
        // fixture on the platform this test is meant to protect. Name the boundary explicitly;
        // otherwise the assertion depends on which OS runs the suite (§179).
        let host = backend::BackendConfig {
            wsl: None,
            ..Default::default()
        };
        assert!(host.can_open(share));
        assert!(host.can_open(std::path::Path::new("/home/p/yield.csv")));
    }

    #[test]
    fn a_backend_that_never_starts_opens_setup_rather_than_naming_a_log_file() {
        // These are the exact strings `BackendSupervisor` raises. Pinned here because
        // the routing decision reads them: if one is reworded and this is not, the app
        // silently goes back to showing "did not become healthy" and nothing else.
        for message in [
            "no langgraph.json under /home/x — set MINIME_BACKEND_DIR to the Mini-Me checkout",
            "failed to launch the backend in WSL (default distro) ~/Mini-Me",
            "failed to spawn the backend (uv). If the LangGraph CLI is missing…",
            "backend exited during startup with exit status: 1",
            "backend did not become healthy within 120 attempts",
        ] {
            assert!(looks_like_a_setup_failure(message), "{message}");
        }
        // A model or graph error is not a setup problem — the sidecar log is the right
        // pointer for those, and hijacking the pane would hide it.
        for message in [
            "stream failed: 500 Internal Server Error",
            "no backend at http://127.0.0.1:2024 and attach-only mode is on",
            "the run paused but no thread was recorded",
        ] {
            assert!(!looks_like_a_setup_failure(message), "{message}");
        }
    }

    #[test]
    fn every_command_is_labelled_and_hinted() {
        // A palette row with an empty label is invisible but still selectable, which
        // is worse than a missing command.
        for command in Command::ALL {
            assert!(!command.label().is_empty(), "{command:?}");
            assert!(!command.hint().is_empty(), "{command:?}");
        }
    }

    #[test]
    fn a_purely_delegated_turn_is_not_discarded_as_empty() {
        // The web client filters tool-call-only assistant messages out of the
        // transcript, which is precisely why a delegation there renders as nothing.
        // Activity has to count as content or we would reproduce the same silence.
        let mut message = Message::new("mini-me", String::new());
        assert!(message.is_silent());
        message.steps.push("delegating to report_writer".into());
        assert!(!message.is_silent());
    }

    #[test]
    fn a_trace_keeps_the_newest_text_when_it_overflows() {
        let mut trace = AgentTrace {
            ns: "tools:a".into(),
            name: "academic_researcher".into(),
            steps: Vec::new(),
            text: String::new(),
            expanded: true,
        };
        // Multi-byte on purpose: the cap counts characters, so a naive byte slice
        // would split one and panic.
        trace.push_text(&"á".repeat(MAX_TRACE_CHARS));
        trace.push_text("tail");
        assert!(trace.text.ends_with("tail"));
        assert!(trace.text.starts_with('…'));
        assert!(trace.text.chars().count() <= MAX_TRACE_CHARS + 1);
    }

    /// §245: what the heading says when the section is folded shut.
    ///
    /// Both lists at once, because a LangGraph worker and an Asta task are one category to the
    /// person waiting and two objects to this client.
    #[test]
    fn the_folded_heading_names_every_state_that_has_something_in_it() {
        let mut gated = worker_with("interrupted", &[], None);
        gated.pending = Some(protocol::ApprovalRequest { actions: Vec::new() });
        let tasks = vec![
            gated,
            worker_with("running", &[], None),
            worker_with("success", &[], None),
            worker_with("error", &[], None),
        ];
        let analysis = protocol::Job {
            kind: protocol::JobKind::Analysis,
            task_id: "t".into(),
            question: "q".into(),
            context_id: None,
            status: "working".into(),
            size: None,
        };
        let jobs = vec![
            analysis.clone(),
            protocol::Job { status: "completed".into(), ..analysis.clone() },
            protocol::Job { status: "failed".into(), ..analysis },
        ];

        let tally = JobTally::of(&tasks, &jobs);
        assert_eq!(
            tally,
            JobTally { waiting: 1, running: 2, failed: 2, done: 2 }
        );
        assert_eq!(
            tally.summary(),
            "1 waiting for you · 2 running · 2 failed · 2 done"
        );
        // A gate outranks everything else for the colour, because it is the only state that is
        // the researcher's move.
        assert_eq!(tally.colour(), theme::accent());
    }

    /// An `interrupted` worker is neither finished nor running — it is stopped, waiting.
    #[test]
    fn a_worker_at_the_gate_counts_as_waiting_not_running() {
        let mut gated = worker_with("interrupted", &[], None);
        gated.pending = Some(protocol::ApprovalRequest { actions: Vec::new() });
        assert!(!gated.is_finished(), "interrupted is not terminal");
        let tally = JobTally::of(std::slice::from_ref(&gated), &[]);
        assert_eq!(tally.waiting, 1);
        assert_eq!(tally.running, 0);
        assert_eq!(tally.summary(), "1 waiting for you");
    }

    /// Nothing to say, and the heading says nothing — rather than `0 running`.
    #[test]
    fn an_empty_tally_has_an_empty_summary() {
        let tally = JobTally::default();
        assert_eq!(tally.summary(), "");
        assert_eq!(tally.colour(), theme::text_faint());
    }

    /// Running beats failed and done for the colour: it is the state that is still changing.
    #[test]
    fn the_heading_colour_follows_the_most_urgent_state() {
        let running = JobTally { running: 1, failed: 3, done: 9, ..Default::default() };
        assert_eq!(running.colour(), theme::running());
        let failed = JobTally { failed: 1, done: 9, ..Default::default() };
        assert_eq!(failed.colour(), theme::error());
        let done = JobTally { done: 1, ..Default::default() };
        assert_eq!(done.colour(), theme::text_faint());
    }

    /// §245: a DataVoyager question is a paragraph by design, and the row is one column wide.
    #[test]
    fn a_long_job_question_is_clipped_for_the_panel() {
        // The shape `subagents.py` asks the coordinator to write: datasets, methods, numbers.
        let question = "Using SOC_MgHa as the response and the 113 covariate columns as \
             predictors, fit random forest, gradient boosting and ridge regression, report \
             cross-validated and held-out R², RMSE and the ten most important covariates, and \
             run it.";
        assert!(question.chars().count() > JOB_QUESTION_CHARS, "{question}");

        let shown = protocol::clip(question, JOB_QUESTION_CHARS);
        assert!(shown.chars().count() <= JOB_QUESTION_CHARS + 1, "{shown}");
        assert!(shown.ends_with('…'), "{shown}");
        // Cut between words, not through one.
        assert!(!shown.contains(" …"), "{shown}");
        assert!(question.starts_with(shown.trim_end_matches('…')), "{shown}");

        // Short enough to say in full is said in full, with no ellipsis to imply otherwise.
        let short = "Does rainfall predict yield?";
        assert_eq!(protocol::clip(short, JOB_QUESTION_CHARS), short);

        // And the stored question is untouched — it is also a query parameter the theorizer's
        // poll route sends back.
        let job = protocol::Job {
            kind: protocol::JobKind::Analysis,
            task_id: "t".into(),
            question: question.to_string(),
            context_id: None,
            status: "working".into(),
            size: None,
        };
        assert_eq!(job.question, question);
    }

    /// §248: the number on screen must always be one the service will accept.
    #[test]
    fn the_gate_opens_on_a_budget_that_can_actually_be_submitted() {
        assert_eq!(opening_budget(15), 15);
        // A draft with no budget recorded, or a nonsense one, still opens somewhere pressable.
        assert_eq!(opening_budget(0), 1);
        assert_eq!(opening_budget(9_999), MAX_BUDGET);
        assert_eq!(opening_budget(MAX_BUDGET), MAX_BUDGET);
        // And every preset is inside the bounds, so a single press cannot make it unsubmittable.
        for preset in BUDGET_PRESETS {
            assert_eq!(opening_budget(preset), preset, "{preset}");
        }
    }

    /// A failed *balance* lookup must not stop somebody spending their own credits.
    #[test]
    fn an_unknown_balance_does_not_block_the_decision() {
        assert!(affordable(15, None));
        assert!(affordable(500, None));
        // A known balance does gate it, exactly and inclusively.
        assert!(affordable(15, Some(15)));
        assert!(!affordable(16, Some(15)));
        assert!(!affordable(1, Some(0)));
    }

    /// The sentence a researcher reads before spending. `available`, never `granted`.
    #[test]
    fn the_cost_line_states_the_price_against_what_is_left() {
        assert_eq!(cost_line(15, Some(495)), "15 experiments · 15 of 495 credits");
        // Singular, because "1 experiments" is the kind of detail that makes a money dialog look
        // untrustworthy.
        assert_eq!(cost_line(1, Some(495)), "1 experiment · 1 of 495 credits");
        // No balance yet: say the rate rather than implying a number nobody confirmed.
        assert_eq!(cost_line(15, None), "15 experiments · one credit each");
        assert!(!cost_line(15, Some(495)).contains("500"), "the grant is not the balance");
    }

    /// §252: the press must be impossible, not merely doomed, until the token is in hand.
    #[test]
    fn the_gate_cannot_be_pressed_without_an_approval_token() {
        assert!(!ready_to_submit(None), "no lookup has answered yet");

        let mut cost = protocol::DraftCost {
            status: String::new(),
            submitted: false,
            approval: String::new(),
            experiments: 15,
            available: Some(495),
            intent: "steer it".into(),
        };
        assert!(!ready_to_submit(Some(&cost)), "answered, but with no token");
        cost.approval = "   ".into();
        assert!(!ready_to_submit(Some(&cost)), "whitespace is not a token");
        cost.approval = "tok_abc".into();
        assert!(ready_to_submit(Some(&cost)));

        // The token is orthogonal to affordability: both have to hold.
        assert!(affordable(15, cost.available));
        assert!(!affordable(500, cost.available));

        // §258: a run the service says is already started issues no token at all, so the gate
        // cannot be pressed for it even if something managed to open one.
        let started = protocol::DraftCost {
            status: "completed".into(),
            submitted: true,
            approval: String::new(),
            experiments: 5,
            available: Some(495),
            intent: String::new(),
        };
        assert!(!ready_to_submit(Some(&started)));
    }

    /// §257: "asking", "none" and "not asked" are three different things, and the tempting
    /// simplification — `if paths.is_empty()` — collapses them into one.
    #[test]
    fn an_experiment_with_no_figure_does_not_look_like_one_still_loading() {
        let one = vec![std::path::PathBuf::from("/w/discovery/r/node_2_0/figure-01.png")];

        // Decoded and on disk.
        assert_eq!(figure_state(Some(&one), false), Figures::Ready);
        // Fetched, and this one drew nothing. Recorded as an empty vec on purpose.
        assert_eq!(figure_state(Some(&Vec::new()), false), Figures::Nothing);
        // In flight.
        assert_eq!(figure_state(None, true), Figures::Fetching);
        // Nobody asked. Distinct from `Fetching`, because a failed fetch lands here and a pane
        // that still said "fetching…" would be lying.
        assert_eq!(figure_state(None, false), Figures::Unread);

        // A present answer wins over a stale in-flight flag: the paths are what matters.
        assert_eq!(figure_state(Some(&one), true), Figures::Ready);
        assert_eq!(figure_state(Some(&Vec::new()), true), Figures::Nothing);
    }

    /// §259: a snapshot arrives in two places, and one of them ignored `drafts`.
    ///
    /// The general form of the last four defects — a value produced correctly and consumed in one
    /// of the two places that should consume it. Asserted against the source because that is where
    /// the property lives: a second reader of a snapshot is a second chance to forget a field.
    ///
    /// The needles are assembled at runtime so this test does not match itself.
    #[test]
    fn every_snapshot_is_adopted_through_one_path() {
        let source = include_str!("main.rs");
        let jobs = concat!("snapshot", ".jobs");
        let tasks = concat!("snapshot", ".tasks");
        let drafts = concat!("snapshot", ".drafts");

        // Nothing walks them itself; the shared method does.
        assert!(
            !source.contains(&format!("for job in {jobs}")),
            "a snapshot's jobs are adopted through `adopt_background_work`, not inline"
        );
        assert!(
            !source.contains(&format!("for task in {tasks}")),
            "a snapshot's tasks are adopted through `adopt_background_work`, not inline"
        );

        // And every reader of one reads all three, which is what the two sites disagreed about.
        let reads = |needle: &str| source.matches(needle).count();
        assert_eq!(
            reads(jobs),
            reads(drafts),
            "a place that reads a snapshot's jobs must also read its drafts"
        );
        assert_eq!(reads(jobs), reads(tasks));
        assert!(reads(jobs) >= 2, "both the streaming and the opening path");
    }

    /// §260: a failed figure fetch must not be remembered as an experiment that drew nothing.
    #[test]
    fn a_failed_figure_fetch_is_not_an_answer() {
        // The three answers the pane can hold, and the fourth state is *no key at all* — which is
        // what a failure leaves behind, so the next open asks again.
        let none: Option<&Vec<std::path::PathBuf>> = None;
        assert_eq!(figure_state(none, false), Figures::Unread);
        assert_eq!(figure_state(none, true), Figures::Fetching);
        assert_eq!(figure_state(Some(&Vec::new()), false), Figures::Nothing);

        // `Nothing` is cached and `Unread` is not, so the two must never be produced by the same
        // input — which is the property that broke when a failure returned an empty list.
        assert_ne!(
            figure_state(Some(&Vec::new()), false),
            figure_state(none, false)
        );
    }

    /// §265: the notification exists for three events, and a helper called from two of them is a
    /// toast that arrives depending on which kind of work you left running.
    ///
    /// A join test, not a component test — which is the whole lesson of §264 and the six defects
    /// before it. The needle is assembled at runtime so this does not match itself.
    #[test]
    fn every_kind_of_finished_background_work_can_reach_somebody_who_left() {
        let source = include_str!("main.rs");
        let call = concat!("notify", "_if_away(");

        // Three call sites: a long job ending, a worker ending, a worker asking for a decision.
        let sites = source.matches(call).count();
        assert!(
            sites >= 3,
            "expected a call for a finished job, a finished worker and a worker at the gate; \
             found {sites}"
        );

        // And the decision lives in one place rather than being re-derived per site.
        assert_eq!(
            source.matches(concat!("worth", "_interrupting(")).count(),
            1,
            "the suppress-when-looking rule belongs in `notify_if_away` alone"
        );
    }

    /// §266: a sort a researcher can reverse, and reverse back.
    #[test]
    fn flipping_the_sort_twice_returns_the_same_order() {
        let experiments = discovery::decode_experiments(
            &serde_json::from_str::<serde_json::Value>(include_str!(
                "../tests/fixtures/autodiscovery-experiments.json"
            ))
            .expect("the probe"),
        );
        let loud = ranked(&experiments, true);
        let quiet = ranked(&experiments, false);

        // Biggest shift first, and the other way round.
        let magnitude = |at: usize| experiments[at].magnitude();
        assert!(magnitude(loud[0]) >= magnitude(loud[loud.len() - 1]));
        assert!(magnitude(quiet[0]) <= magnitude(quiet[quiet.len() - 1]));
        // Every experiment appears exactly once either way.
        let mut sorted = loud.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..experiments.len()).collect::<Vec<_>>());
        assert_eq!(ranked(&experiments, true), loud, "the sort is stable");

        // Ties keep creation order, so the toggle is reversible rather than a reshuffle. Three of
        // one real run's five experiments reported the same score, so this is the common case.
        let tied: Vec<discovery::Experiment> = (0..4)
            .map(|at| {
                let mut copy = experiments[0].clone();
                copy.order = at as u32;
                copy.surprise = Some(0.690);
                copy
            })
            .collect();
        assert_eq!(ranked(&tied, true), [0, 1, 2, 3]);
        assert_eq!(ranked(&tied, false), [0, 1, 2, 3]);

        // And an empty run sorts to nothing rather than panicking.
        assert!(ranked(&[], true).is_empty());
    }
}

/// Assemble the model routing the backend expects from settings plus the keychain.
fn model_choice(user_settings: &settings::Settings) -> Option<protocol::ModelChoice> {
    let provider = settings::provider(&user_settings.provider)?;
    // Only the specialists actually pointed somewhere else. An entry equal to the coordinator's
    // spec is not an override, and sending it would make the settings file's shape visible in
    // every request for no effect.
    let subagents: std::collections::BTreeMap<String, String> = user_settings
        .subagents
        .iter()
        .filter(|(_, spec)| !spec.trim().is_empty() && **spec != user_settings.model_spec())
        .map(|(name, spec)| (name.clone(), spec.trim().to_string()))
        .collect();
    // A key for every *other* provider those overrides reach. Read here, on the main thread,
    // for the same reason the coordinator's is: the keychain is not async-safe (see `secret_env`).
    let mut extra_keys = std::collections::BTreeMap::new();
    for spec in subagents.values() {
        let Some((provider_id, _)) = spec.split_once("::") else {
            continue;
        };
        if provider_id == user_settings.provider || extra_keys.contains_key(provider_id) {
            continue;
        }
        if let Some(key) = settings::secret(&format!("llm:{provider_id}")) {
            extra_keys.insert(provider_id.to_string(), key);
        }
    }
    Some(protocol::ModelChoice {
        spec: user_settings.model_spec(),
        provider: user_settings.provider.clone(),
        api_key: settings::secret(&user_settings.key_name()),
        base_url: if provider.needs_base_url && !user_settings.base_url.trim().is_empty() {
            Some(user_settings.base_url.trim().to_string())
        } else {
            None
        },
        subagents,
        extra_keys,
    })
}

/// `--set-secret NAME [VALUE]` writes one credential to the OS keychain and exits.
///
/// The settings *panel* is the real interface, but this exists so the machine can be set
/// up headlessly — which is exactly how the whole backend path is tested on a box with no
/// display. An empty value forgets the entry.
fn set_secret_from_args(args: &[String]) -> Option<i32> {
    let at = args.iter().position(|arg| arg == "--set-secret")?;
    let Some(name) = args.get(at + 1) else {
        eprintln!("--set-secret needs a name, e.g. ASTA_TOKEN or llm:anthropic");
        return Some(2);
    };
    let value = args.get(at + 2).map(String::as_str).unwrap_or("");
    match settings::set_secret(name, value) {
        Ok(()) => {
            // Never echo the value.
            println!(
                "{name}: {}",
                if value.trim().is_empty() {
                    "cleared"
                } else {
                    "stored in the OS keychain"
                }
            );
            Some(0)
        }
        Err(error) => {
            eprintln!("could not store {name}: {error:#}");
            if let Err(status) = settings::keychain_status() {
                eprintln!("keychain: {status:#}");
            }
            Some(1)
        }
    }
}

/// What this build is, in one line a researcher can paste into a message.
///
/// `CARGO_PKG_VERSION` alone would not distinguish two builds a fortnight apart from the same
/// version, and that fortnight is where this project lives. The commit is stamped at build time by
/// the release workflow; a local `cargo run` has none, and says so rather than implying a release.
fn build_stamp() -> String {
    match option_env!("MINIME_BUILD_COMMIT") {
        Some(commit) if !commit.trim().is_empty() => {
            format!("Mini-Me Desktop {} ({})", env!("CARGO_PKG_VERSION"), commit.trim())
        }
        _ => format!(
            "Mini-Me Desktop {} (built from source)",
            env!("CARGO_PKG_VERSION")
        ),
    }
}

/// Where the app writes its own log, beside the one it keeps for the backend.
///
/// **Because for six months it kept none.** `backend.rs` has always written the sidecar's output to
/// a file, and the app's own tracing went to stderr only — which for a windowed program means a
/// console nobody kept. Twice in one session a diagnostic was added, the researcher was asked to
/// grep for it, and the answer was empty because the console output had never been captured; the
/// second time that empty answer was mistaken for evidence and sent a diagnosis down the wrong path
/// (§206). A log a person has to remember to record is a log that is not there when it matters.
pub fn app_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("mini-me-desktop-app.log")
}

/// A log destination that is the console *and* a file.
///
/// Both, not either: the console is what a developer watches live, and the file is what survives to
/// be read afterwards. Cloneable and shared behind a mutex because `MakeWriter` hands out a fresh
/// writer per event.
#[derive(Clone)]
struct Tee(Option<std::sync::Arc<std::sync::Mutex<std::fs::File>>>);

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // The console first and its result is the one returned: a full disk or a locked file must
        // never cost the line someone is watching arrive.
        let written = std::io::stderr().write(buf)?;
        if let Some(file) = &self.0 {
            if let Ok(mut file) = file.lock() {
                let _ = file.write_all(buf);
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(file) = &self.0 {
            if let Ok(mut file) = file.lock() {
                let _ = file.flush();
            }
        }
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Tee {
    type Writer = Tee;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn main() {
    // **Truncated, not appended.** The whole point is that a researcher told to read this file is
    // reading *this* launch. An appended log would have answered the question we actually asked
    // with lines from a run three days earlier, which is the failure one step removed.
    let log = std::fs::File::create(app_log_path()).ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        // No escape codes: the console is still perfectly readable without them, and the file is
        // meant to be grepped and pasted.
        .with_ansi(false)
        .with_writer(Tee(log.map(|file| {
            std::sync::Arc::new(std::sync::Mutex::new(file))
        })))
        .init();
    // First two lines, and they name what everything after them is about: which build this is, and
    // where the record of it will be. The backend's own version has been logged since §115; the
    // app's never was (§213).
    tracing::info!(build = %build_stamp(), "starting");
    tracing::info!(path = %app_log_path().display(), "app log");

    // The researcher's palette, before the first frame — so the window never opens in
    // one theme and repaints into another.
    settings::apply_theme(&settings::Settings::load());

    // `--replay <capture>` needs no backend at all, so it runs before one is
    // configured.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = set_secret_from_args(&args) {
        std::process::exit(code);
    }
    if let Some(path) = args.iter().position(|a| a == "--replay") {
        let Some(capture) = args.get(path + 1) else {
            eprintln!("--replay needs a path to a captured SSE stream");
            std::process::exit(2);
        };
        if let Err(error) = replay(capture) {
            eprintln!("\nreplay: FAIL — {error:#}");
            std::process::exit(1);
        }
        println!("\nreplay: PASS");
        return;
    }

    // `--local` / `--sandbox` override `MINIME_EXECUTION_BACKEND`; see
    // `resolve_execution`. Last one wins if both are given.
    let execution_override = args.iter().rev().find_map(|arg| match arg.as_str() {
        "--local" => Some(true),
        "--sandbox" => Some(false),
        _ => None,
    });
    let config = backend::BackendConfig::with_execution_override(execution_override);
    tracing::info!(
        location = %config.location(),
        url = %config.base_url(),
        execution = config.execution_label(),
        "backend sidecar configured"
    );
    if matches!(config.execution, backend::Execution::Local { .. }) {
        // Loud on purpose: this is the setting that lets model-written commands touch
        // the user's own machine (docs §18).
        tracing::warn!("host execution is ON — the agent runs commands on this machine");
    }
    if !config.looks_like_backend_repo() {
        tracing::warn!(
            dir = %config.project_dir.display(),
            "no langgraph.json found — set MINIME_BACKEND_DIR to the Mini-Me checkout"
        );
    }
    // The model choice comes from settings; the key comes from the OS keychain and goes
    // straight into each run request, so no key is ever written to a file or an
    // environment variable (docs §20).
    let user_settings = settings::Settings::load();
    let model = model_choice(&user_settings);
    for problem in user_settings.problems(model.as_ref().is_some_and(|m| m.api_key.is_some())) {
        // Warned, not fatal: the app still opens, which is where the user fixes it.
        tracing::warn!(%problem, "settings incomplete");
    }
    // `--preflight` prints the same checks the Setup pane shows, and exits non-zero when
    // something blocks a turn. No window, so it works over SSH — and it is how the pane's
    // content is verified on a machine that cannot open one.
    if args.iter().any(|a| a == "--preflight") {
        let has_key = model.as_ref().is_some_and(|m| m.api_key.is_some());
        let report = preflight::inspect(&config, has_key);
        println!("where    : {} · {}", report.location, report.execution);
        for check in &report.checks {
            println!(
                "{} {:<22} {}",
                check.state.glyph(),
                check.label,
                check.detail
            );
            for fix in &check.fixes {
                match fix {
                    preflight::Fix::Run { label, argv, note } => {
                        println!("    fix  : {label} ({note})");
                        println!("    run  : {}", preflight::display_argv(argv));
                    }
                    preflight::Fix::Adopt { label, dir } => {
                        println!("    fix  : {label} — {dir}");
                    }
                    preflight::Fix::Manual(instruction) => {
                        println!("    fix  : {instruction}");
                    }
                }
            }
        }
        println!("\npreflight: {}", report.summary());
        std::process::exit(if report.ready() { 0 } else { 1 });
    }

    let sidecar =
        Arc::new(Sidecar::new(config, model).expect("failed to build the sidecar runtime"));

    // `--check-backend [--stream]` exercises the sidecar without a window, so the
    // client/backend contract can be verified on a headless machine.
    if args.iter().any(|a| a == "--check-backend") {
        // `--stream` runs the seed prompt; `--prompt "…"` runs your own, which is how
        // a delegating turn (and so the activity trace) gets verified headlessly.
        // Repeating `--prompt` runs several turns **on one thread**, which is how
        // conversation continuity gets checked without a window.
        let mut prompts: Vec<&str> = Vec::new();
        for (at, arg) in args.iter().enumerate() {
            if arg == "--prompt" {
                if let Some(prompt) = args.get(at + 1) {
                    prompts.push(prompt);
                }
            }
        }
        if prompts.is_empty() && args.iter().any(|a| a == "--stream") {
            prompts.push(CHECK_PROMPT);
        }
        let outcome = sidecar.check(&prompts);
        let failed = match &outcome {
            Ok(()) => {
                println!("\nbackend check: PASS");
                false
            }
            Err(error) => {
                eprintln!("\nbackend check: FAIL — {error:#}");
                true
            }
        };
        // `process::exit` skips destructors, which would leak the spawned
        // backend. Drop the sidecar (shutting the child down) *before* exiting.
        drop(sidecar);
        if failed {
            std::process::exit(1);
        }
        return;
    }

    Application::new().with_assets(Assets).run(move |cx: &mut App| {
        // Without these the composer receives no editing keys at all — GPUI
        // dispatches actions, and nothing binds to them by default.
        cx.bind_keys(composer::key_bindings());
        cx.bind_keys(workbench_key_bindings());

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Workbench::new(sidecar.clone(), window, cx)),
            )
            .expect("failed to open window");

        // Focus the composer so the user can type immediately on launch.
        window
            .update(cx, |workbench, window, cx| {
                let composer = workbench.composer.focus_handle(cx);
                window.focus(&composer);
            })
            .expect("failed to focus the composer");

        cx.activate(true);
    });
}
