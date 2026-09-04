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
mod commands;
mod dataverse;
mod discovery;
mod gallery;
mod markdown;
mod preflight;
mod protocol;
mod provenance;
mod references;
mod settings;
mod sidecar;
mod subagent;
mod theme;
mod update;
mod workspace;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt;

use protocol::{AgentRef, Bucket, TurnEvent};
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

impl SidebarMenu {
    pub(crate) fn rows(&self) -> Vec<MenuRow> {
        let row = |id, label: String| MenuRow {
            id,
            label,
            danger: false,
        };
        let danger = |id, label: String| MenuRow {
            id,
            label,
            danger: true,
        };
        match self {
            Self::New => vec![
                row("menu-new-conversation", "New conversation".into()),
                row("menu-new-project", "New project…".into()),
            ],
            Self::Conversation(_) => vec![
                row("menu-rename", "Rename".into()),
                danger("menu-delete", "Delete".into()),
            ],
            Self::Project { name, .. } => vec![
                row("menu-new-here", format!("New conversation in {name}")),
                row("menu-open-folder", "Open folder".into()),
                danger("menu-delete-project", "Delete project".into()),
            ],
        }
    }
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

/// What `with_attachments` sent the coordinator, minus the blockquote it may have prepended.
///
/// The coordinator and three subagent prompts still need that blockquote — it is how they learn
/// where an attached file landed — so it stays in what `start_turn_as` submits. It no longer
/// belongs in the *transcript*, though: the path it names is the conversation's workspace, which
/// is the same on every turn, and repeating it mid-conversation was noise where the researcher
/// was reading what they actually typed. The chat header now says it once, instead (§267).
fn without_attached_blockquote(prompt: &str) -> &str {
    const PREFIX: &str = "> Attached files (already saved in the sandbox working directory): ";
    match prompt.strip_prefix(PREFIX).and_then(|rest| rest.find("\n\n").map(|at| &rest[at + 2..])) {
        Some(typed) => typed,
        None => prompt,
    }
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

/// The same first-turn attachments, with the prompt reference the sidecar must replace after the
/// backend assigns the thread id and before it starts the model run.
fn attachments_for_turn(attachments: &[Attachment]) -> Vec<workspace::PendingAttachment> {
    attachments
        .iter()
        .filter(|attachment| !attachment.adopted)
        .map(|attachment| workspace::PendingAttachment {
            source: attachment.source.clone(),
            reference: attachment.reference.clone(),
        })
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

pub(crate) fn bibliography(sources: &[protocol::Source], origins: &[references::Origin]) -> String {
    let mut out = String::new();
    for (at, source) in sources.iter().enumerate() {
        let citation = source.citation.trim();
        if citation.is_empty() {
            continue;
        }
        // Braces and backslashes are BibTeX's own syntax; left in they would truncate the entry
        // at the first one and take the rest of the file with it.
        let safe = citation.replace('\\', "\\\\").replace(['{', '}'], "");
        out.push_str(&format!("@misc{{minime{},\n  note = {{{safe}}},\n", at + 1));
        if let Some(url) = link_for(source) {
            out.push_str(&format!("  url = {{{url}}},\n"));
        }
        // A disagreement travels with the entry rather than being resolved here. A reference
        // manager shows `annote`, and someone importing forty references should not have to come
        // back to this window to find out which two were doubtful.
        //
        // **And which ones nothing checked.** This is the copy that leaves the app — into Zotero,
        // into a manuscript, into a colleague's inbox — so it is the one place the distinction
        // most needs to survive. The panel can be re-read; an exported `.bib` is on its own, and
        // the note has to travel with the entry it belongs to (docs §185).
        match (disputed_link(source), origins.get(at)) {
            (Some(written), _) => out.push_str(&format!(
                "  annote = {{unverified: the citation text gives {written}}},\n"
            )),
            (None, Some(origin)) if origin.needs_a_human() => out.push_str(
                "  annote = {unverified: this reference came from the model, not from a search — \
                 confirm it before citing},\n",
            ),
            (None, _) => {}
        }
        out.push_str("}\n\n");
    }
    out
}


/// Where a source's `link` should point: the paper's own page on Semantic Scholar.
///
/// Asked for directly — *"when I press it I am redirected to the paper in semantic scholar not to
/// the article in the main page where the article was published"* — and it is the better default
/// anyway: the publisher's landing page is often a paywall, while the Semantic Scholar record
/// carries the abstract, the citation graph and whatever open-access copy exists.
///
/// `api.semanticscholar.org/<id>` 301-redirects to the paper page for **both** id forms, verified
/// against the live service:
///
/// ```text
/// CorpusID:45447591                     → /paper/117e16e7774ff0616b461a075feadcee7a33d793
/// DOI:10.1016/0304-3878(92)90044-a      → /paper/bbec167725ba916adafcaa221f934b759e2cd131
/// ```
///
/// In preference order: the corpus id the search itself returned; the DOI the registry says this
/// citation describes; the DOI the citation carries, when that one checked out. Failing all three
/// — a thesis in a university repository, say — whatever link the source came with, because a
/// working link to the right document beats a Semantic Scholar page that does not exist.
pub(crate) fn scholar_link(
    source: &protocol::Source,
    verdict: Option<&references::Verdict>,
    repair: Option<&references::Repair>,
) -> Option<String> {
    let existing = link_for(source);

    // Trustworthy by construction: built from the `corpusId` in the search result this paper came
    // from. Nothing composed it, so there is nothing to check.
    if existing.as_deref().is_some_and(references::is_corpus_link) {
        return existing;
    }

    // The work the registry says this citation describes. Checked, so usable.
    if let Some(repair) = repair {
        return Some(format!("https://api.semanticscholar.org/DOI:{}", repair.doi));
    }

    // The citation's own DOI — **only once it has been verified**.
    //
    // This is the line that shipped wrong, and it made things worse rather than merely failing.
    // It used to wrap *any* DOI, verified or not, as `api.semanticscholar.org/DOI:<doi>`. An
    // invented DOI is a real DOI belonging to somebody else, so instead of 404ing it resolved —
    // cleanly, to a real Semantic Scholar page. A researcher clicking `link` on a paper about
    // potato late blight was taken to one about recombination in the mammalian germ line, with no
    // warning, because the row renders before the check returns.
    //
    // Routing through Semantic Scholar removed the one accidental safeguard a bad DOI had: that
    // it often did not resolve at all. So the guard has to be explicit. An unverified identifier
    // written by a model is not a link; it is a claim awaiting a check.
    if matches!(verdict, Some(references::Verdict::Confirmed)) {
        if let Some(doi) = existing.as_deref().and_then(references::doi_in) {
            return Some(format!("https://api.semanticscholar.org/DOI:{doi}"));
        }
    }

    // A link that carries no identifier at all — a thesis in a university repository — is the
    // model's, and unverifiable, but at least it is not dressed up as a resolved paper. Kept only
    // when there is no DOI in it to be wrong about.
    match existing.as_deref().and_then(references::doi_in) {
        Some(_) => None,
        None => existing,
    }
}


/// The link to actually use for a source.
///
/// The backend's own field first, and the URL inside the citation only when there is no field.
/// See [`protocol::Source`] for why that order is not arbitrary: one is what Semantic Scholar
/// returned, the other is what the model wrote down.
pub(crate) fn link_for(source: &protocol::Source) -> Option<String> {
    source
        .link
        .clone()
        .or_else(|| first_url(&source.citation))
}


/// The URL written into the citation text, when it contradicts the structured one.
///
/// `None` when they agree, when either is missing, or when they differ only in the ways URLs
/// harmlessly differ — a trailing slash, `http` against `https`, or the `doi.org` host spelled
/// with `dx.`. Those are the same resolver and flagging them would train people to ignore this.
pub(crate) fn disputed_link(source: &protocol::Source) -> Option<String> {
    let structured = source.link.as_deref()?;
    let written = first_url(&source.citation)?;
    let normalise = |url: &str| {
        url.trim_end_matches('/')
            .replace("http://", "https://")
            .replace("://dx.doi.org/", "://doi.org/")
            .to_ascii_lowercase()
    };
    (normalise(structured) != normalise(&written)).then_some(written)
}


/// A citation with its URL removed, and the punctuation left tidy.
///
/// So the prose can be read as prose and the link shown once, on its own line, where it cannot
/// wrap into something that looks mistyped.
pub(crate) fn without_url(citation: &str) -> String {
    let Some(url) = first_url(citation) else {
        return citation.trim().to_string();
    };
    let Some(at) = citation.find(&url) else {
        return citation.trim().to_string();
    };
    let mut out = String::with_capacity(citation.len());
    out.push_str(&citation[..at]);
    out.push_str(&citation[at + url.len()..]);
    // "…potato. https://doi.org/10.1/x." leaves "…potato. ." behind.
    out.trim().trim_end_matches(['.', ',', ';', ' ']).trim().to_string()
}


/// The provenance graph as a Mermaid `flowchart`.
///
/// Mermaid because it is the one diagram format a researcher can already paste somewhere useful:
/// GitHub, Quarto, Obsidian and Typst all render it, and it stays readable as text if none of
/// them are to hand. The link styles carry the same distinction the drawing does — `-->` is
/// causal, `-.->` is arrival order — so a diagram pasted into a methods section does not quietly
/// lose the hedge that makes it honest.
pub(crate) fn mermaid(graph: &provenance::Graph) -> String {
    let mut out = String::from("flowchart TD\n");
    for (at, node) in graph.nodes.iter().enumerate() {
        let label = match node.visits {
            1 => node.name.replace('_', " "),
            visits => format!("{} ×{visits}", node.name.replace('_', " ")),
        };
        // Quotes around the label so a name with a bracket or a space in it cannot end the node.
        out.push_str(&format!("    n{at}[\"{}\"]\n", label.replace('"', "'")));
    }
    for edge in &graph.edges {
        let arrow = match edge.kind {
            provenance::Edge::Delegated => "-->",
            _ => "-.->",
        };
        let label = match (edge.kind, edge.count) {
            (provenance::Edge::Delegated, 1) => String::new(),
            (kind, 1) => format!("|{}|", kind.label()),
            (kind, count) => format!("|{} ×{count}|", kind.label()),
        };
        out.push_str(&format!(
            "    n{} {arrow}{label} n{}\n",
            edge.from, edge.to
        ));
    }
    out
}


/// The provenance graph as a standalone SVG.
///
/// **SVG rather than the PNG the design asks for.** Rasterising what is on screen would mean a
/// screenshot API gpui 0.2.2 does not expose, or a PNG encoder — a compressor and a CRC — written
/// by hand or pulled in as a dependency, on a build that has to succeed on a colleague's Windows
/// machine with nothing installed. SVG needs none of that: it is text, this function is the
/// generator, and a vector figure is what a journal asks for anyway. It also survives being
/// scaled into a poster, which a 760px raster does not.
///
/// Colours are baked from the live palette at the moment of export, because the file leaves the
/// app and cannot ask a theme anything later.
pub(crate) fn provenance_svg(graph: &provenance::Graph) -> String {
    const ROW: f32 = 58.;
    const LEFT: f32 = 16.;
    const NAME_WIDTH: f32 = 260.;
    const GUTTER: f32 = 236.;

    let height = ROW * graph.nodes.len() as f32 + 24.;
    let width = LEFT + NAME_WIDTH + GUTTER;
    let ink = |colour: u32| format!("#{colour:06x}");

    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\" font-family=\"sans-serif\">\n\
         <rect width=\"{width}\" height=\"{height}\" fill=\"{}\"/>\n",
        ink(theme::background())
    );

    let anchor = LEFT + NAME_WIDTH;
    for edge in &graph.edges {
        let from = 12. + (edge.from as f32 + 0.5) * ROW;
        let to = 12. + (edge.to as f32 + 0.5) * ROW;
        let span = (edge.to as f32 - edge.from as f32).abs();
        let bow = (24. + 50. * (span - 1.).max(0.)).min(GUTTER - 30.) * 2.;
        let weight = match edge.kind {
            provenance::Edge::Returned => 2.,
            _ => 1.5,
        } + (edge.count.saturating_sub(1) as f32 * 0.8).min(3.);
        let dashes = match edge.kind {
            provenance::Edge::Delegated => String::new(),
            _ => " stroke-dasharray=\"4 4\"".to_string(),
        };
        out.push_str(&format!(
            "<path d=\"M {anchor} {from} Q {} {} {anchor} {to}\" fill=\"none\" stroke=\"{}\" \
             stroke-width=\"{weight}\"{dashes}/>\n",
            anchor + bow,
            (from + to) / 2.,
            ink(edge_ink(edge.kind)),
        ));
    }

    for (at, node) in graph.nodes.iter().enumerate() {
        let y = 12. + (at as f32 + 0.5) * ROW;
        let x = LEFT + 14. * node.depth.min(3) as f32;
        out.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{y}\" r=\"5.5\" fill=\"{}\"/>\n",
            x + 5.5,
            ink(theme::accent())
        ));
        // `escape` rather than the name straight in: a specialist called `a<b` would otherwise
        // open a tag and the rest of the file would not parse.
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" fill=\"{}\" font-size=\"13\">{}</text>\n",
            x + 20.,
            y - 1.,
            ink(theme::text()),
            escape_xml(&node.name.replace('_', " "))
        ));
        let spans: Vec<String> = node.spans.iter().map(|ms| duration_label(*ms)).collect();
        let note = match node.visits {
            1 => spans.join(", "),
            visits => format!("visited {visits} times · {}", spans.join(", ")),
        };
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" fill=\"{}\" font-size=\"11\">{}</text>\n",
            x + 20.,
            y + 14.,
            ink(theme::text_faint()),
            escape_xml(&note)
        ));
    }

    out.push_str("</svg>\n");
    out
}


/// The five characters that would otherwise be markup.
pub(crate) fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}


/// The colour that stands for how much an edge can be trusted.
///
/// Shared by the arcs and the legend so the two cannot describe different pictures — which they
/// did while each carried its own `match`.
pub(crate) fn edge_ink(kind: provenance::Edge) -> u32 {
    match kind {
        provenance::Edge::Delegated => theme::text_muted(),
        provenance::Edge::Then => theme::text_faint(),
        // The one edge that gets the accent, because these returns are what §73 asked the
        // feature to make visible.
        provenance::Edge::Returned => theme::accent(),
    }
}


/// What kind of work a specialist does, as a colour.
///
/// **Two colours, and `None` for anything else.** The design is explicit that a colour per
/// specialist is a legend nobody memorises; the distinction worth carrying is between work that
/// goes and reads, and work that touches the data. Matched on the name because that is all the
/// chip has — a heuristic, and one whose worst outcome is a chip in the ordinary text colour
/// rather than a wrong claim. A specialist this does not recognise is left uncoloured on purpose:
/// guessing which of two kinds a new one is would be the mistake.
pub(crate) fn specialist_ink(name: &str) -> Option<u32> {
    let name = name.to_ascii_lowercase();
    if ["search", "research", "literature", "paper", "citation", "theor"]
        .iter()
        .any(|mark| name.contains(mark))
    {
        return Some(theme::running());
    }
    if ["data", "analy", "clean", "profil", "stat"]
        .iter()
        .any(|mark| name.contains(mark))
    {
        return Some(theme::success());
    }
    None
}


/// The specialists a turn consulted, in order, with a run of the same one collapsed.
///
/// `a → a → b` is one visit to `a` then one to `b`; `a → b → a` keeps both visits to `a`, because
/// coming *back* to a specialist after another is the loop the whole provenance feature exists to
/// show (§73). Only consecutive repeats collapse.
pub(crate) fn consulted(agents: &[AgentTrace]) -> Vec<String> {
    let mut path: Vec<String> = Vec::new();
    for agent in agents {
        if path.last().map(String::as_str) != Some(agent.name.as_str()) {
            path.push(agent.name.clone());
        }
    }
    path
}

/// The `+N` glyph, sized to the tile it sits on.
pub(crate) fn media_scrim_size(tile: f32) -> f32 {
    (tile / 4.).max(18.)
}


/// How many characters of a filename fit across a tile at `text_xs`.
///
/// Measured rather than truncated by the layout, for the reason [`Workbench::output_grid_tile`]
/// gives: `Label::ellipsis` collapses to a bare `…` without a flex parent to grow within (§59).
/// Roughly 6px per character at 12px type, less the tile's own padding.
pub(crate) fn name_chars(tile: f32) -> usize {
    (((tile - 16.) / 6.) as usize).max(8)
}


/// How many tiles the grid draws, and how many images the last one stands in for.
///
/// **The scrimmed tile counts among the hidden**, because it is covered: eight images in four
/// tiles reads `+5` — three pictures you can see, five you cannot — which is what the phone
/// gallery the researcher pointed at shows for the same eight. `total - tiles` gives `+4` and
/// looks perfectly reasonable in review; it is only wrong beside the thing it is imitating. One
/// function so the grid and its test cannot hold two versions of the rule.
pub(crate) fn image_grid_shape(total: usize) -> (usize, usize) {
    let shown = total.min(IMAGE_GRID_TILES);
    let hidden = if total > IMAGE_GRID_TILES {
        total - (IMAGE_GRID_TILES - 1)
    } else {
        0
    };
    (shown, hidden)
}


pub(crate) fn output_folder_groups(outputs: &[workspace::Output]) -> Vec<OutputFolderGroup<'_>> {
    let mut groups: Vec<OutputFolderGroup<'_>> = Vec::new();
    for output in outputs {
        let folder = std::path::Path::new(&output.name)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .to_path_buf();
        if let Some(group) = groups.iter_mut().find(|group| group.folder == folder) {
            group.outputs.push(output);
        } else {
            groups.push(OutputFolderGroup {
                folder,
                outputs: vec![output],
            });
        }
    }
    groups
}


/// Name the folder the agent chose, not the generated background-thread directory above it.
///
/// The screenshot in §152 devoted its useful width to a 36-character UUID common to every row.
/// That component is app bookkeeping; removing only a leading UUID leaves `eda/plots`, the
/// researcher's information, while the unshortened path remains the grouping identity above.
///
/// `worker` is whoever produced the files, when the app knows — from the folder for a background
/// worker (§199), from the backend's own record for a specialist (§201). It takes the leading
/// position either way: the UUID's, when there was one, so nothing is lost by removing it; and
/// otherwise ahead of the folder the agent chose. Either way the heading reads as a path of work
/// — `background worker / plots`, `exploratory data analysis / plots`. `None` keeps §152's
/// behaviour, which is what a conversation with no record still gets.
pub(crate) fn output_folder_label(folder: &std::path::Path, worker: Option<&str>) -> String {
    let mut components: Vec<String> = folder
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    let removed_thread = components
        .first()
        .is_some_and(|component| workspace::looks_like_thread_id(component));
    if removed_thread {
        components.remove(0);
    }
    if let Some(name) = worker {
        components.insert(0, name.to_string());
    }
    if components.is_empty() {
        if removed_thread {
            "Background task files".to_string()
        } else {
            "Conversation files".to_string()
        }
    } else {
        components.join(" / ")
    }
}


/// The worker thread a file sits under, when it sits under one.
///
/// The **only** attribution this client can make without guessing. A background worker runs on
/// its own thread and writes into a folder named after it, so the folder *is* the record of who
/// produced the file. Specialists consulted inside the conversation share the conversation's
/// thread and its one directory, and nothing on the wire says which of them wrote a given file —
/// so nothing here claims to know. That restraint is `provenance.rs`'s own rule from §73: a
/// provenance record that quietly guesses is worse than none, because it will be believed.
pub(crate) fn producing_thread(output: &workspace::Output) -> Option<&str> {
    let first = std::path::Path::new(&output.name).components().next()?;
    let name = first.as_os_str().to_str()?;
    if workspace::looks_like_thread_id(name) {
        Some(name)
    } else {
        None
    }
}


/// Outputs split by who produced them: the conversation's own first, then one group per
/// other author, in the order their first file appears.
///
/// **Ahead of the image/other split, not after it.** §152 put every image in one grid because
/// images are what a person opens the panel to look at. That was right within one body of work
/// and wrong across two: a researcher looking at *"15 images"* was looking at the conversation's
/// plots and a worker's plots in one tray, with nothing saying where the boundary was (§199). A
/// background worker is already a separate run with its own job row and its own folder; its
/// figures are a separate body of work for the same reason.
///
/// Two sources of truth, in this order, each exact within its own domain (§201):
///
/// 1. **The folder**, for a background worker — its own thread, its own directory, true by
///    construction and true even for a conversation reopened years later.
/// 2. **The manifest**, for everything else — what `overlay/minime_local/authorship.py` wrote
///    down as each file was produced.
///
/// The folder wins where both speak, because inside a worker's run the manifest records that
/// worker's *own* coordinator and would rename `background worker` to `coordinator` — technically
/// true of the inner graph and useless to the person reading the panel.
/// A file that records what a search returned, rather than a result of the research.
///
/// Kept out of the transcript's file cards only. Both are real outputs a researcher may want —
/// they are just not things to read mid-conversation.
pub(crate) fn is_search_record(output: &workspace::Output) -> bool {
    matches!(
        output.path.file_name().and_then(|name| name.to_str()),
        Some("papers.json") | Some("dataverse_search.json")
    )
}


pub(crate) fn by_producer(
    outputs: &[workspace::Output],
    tasks: &[protocol::AsyncTask],
    wrote: &std::collections::HashMap<String, String>,
) -> Vec<(Option<String>, Vec<workspace::Output>)> {
    let mut groups: Vec<(Option<String>, Vec<workspace::Output>)> = Vec::new();
    for output in outputs {
        let by = match producing_thread(output) {
            Some(thread) => produced_by(Some(thread), tasks),
            None => wrote
                .get(&workspace::normalise_separators(&output.name))
                .map(|agent| agent.replace('_', " ")),
        };
        match groups.iter_mut().find(|(owner, _)| *owner == by) {
            Some((_, produced)) => produced.push(output.clone()),
            None => groups.push((by, vec![output.clone()])),
        }
    }
    // The conversation's own files lead even when someone else wrote first: they are what the
    // researcher asked for directly, and a delegation is the detour under it.
    groups.sort_by_key(|(owner, _)| owner.is_some());
    groups
}


/// Who produced a group of files, in the researcher's words rather than the engine's.
///
/// `None` is the conversation's own thread, and stays unlabelled: those files are the unmarked
/// case, and spending a heading on *"from this conversation"* would name the default everywhere
/// to say something only where it is not true.
///
/// A thread with no matching task is still *some* worker — the folder proves it — so it says so
/// without naming one. That is the state after a reload whose snapshot carried no `async_tasks`,
/// and it is the difference between "we don't know which" and "nobody".
pub(crate) fn produced_by(thread: Option<&str>, tasks: &[protocol::AsyncTask]) -> Option<String> {
    let thread = thread?;
    Some(
        tasks
            .iter()
            .find(|task| task.thread_id == thread)
            // Underscores are the graph's spelling of a name, not a person's — the road strip and
            // the jobs list both already say `background worker`, and a third spelling of the
            // same specialist in a third panel is how one worker reads as two.
            .map(|task| task.agent_name.replace('_', " "))
            .unwrap_or_else(|| "a background task".to_string()),
    )
}


/// `15 images`, or `5 images from background worker`.
pub(crate) fn images_heading(count: usize, by: Option<&str>) -> String {
    let plural = if count == 1 { "" } else { "s" };
    match by {
        Some(who) => format!("{count} image{plural} from {who}"),
        None => format!("{count} image{plural}"),
    }
}


/// Keep the distinguishing tail when a filename itself is too long for a thumbnail.
///
/// `Label::ellipsis()` correctly protects layout (§59), but its trailing ellipsis preserves the
/// shared prefix and removes the useful suffix in §152. Shortening the string from the leading
/// edge before layout means the extension and differentiating part survive even in a 140px tile.
pub(crate) fn distinguishing_tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars == 0 {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("…{}", text.chars().skip(count - keep).collect::<String>())
}


/// Shorten an `a / b / c` heading to fit, giving up the middle rather than either end.
///
/// **[`distinguishing_tail`] keeps the wrong end for these.** §152 chose tail-keeping because its
/// labels shared a long *prefix* and differed at the end — the right rule for a filename. §201 then
/// put the producing worker's name at the *head*, which inverts it: `background worker / outputs /
/// tables` came out as `…d worker / outputs / tables`, throwing away the one word the attribution
/// exists to show. Spotted in a screenshot of the feature working (§208).
///
/// So both ends survive and the middle gives way. If it still will not fit, the **head** is kept
/// whole and the tail is trimmed — the producer outranks the leaf folder, because a heading that
/// cannot say who made these files is the heading §201 replaced.
pub(crate) fn shorten_path_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_string();
    }
    let segments: Vec<&str> = label.split(" / ").collect();
    let (Some(head), Some(tail)) = (segments.first(), segments.last()) else {
        return distinguishing_tail(label, max_chars);
    };
    if segments.len() < 2 {
        // One segment: no middle to drop, so §152's rule is still the best available.
        return distinguishing_tail(label, max_chars);
    }
    let spacer = if segments.len() > 2 { " / … / " } else { " / " };
    let joined = format!("{head}{spacer}{tail}");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let room = max_chars.saturating_sub(head.chars().count() + spacer.chars().count());
    // Below four characters a trimmed tail is all ellipsis and no information; better to fall back
    // than to print `… / … / …s`.
    if room >= 4 {
        return format!("{head}{spacer}{}", distinguishing_tail(tail, room));
    }
    distinguishing_tail(label, max_chars)
}


pub(crate) fn output_filename(output: &workspace::Output) -> String {
    let name = std::path::Path::new(&output.name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&output.name);
    distinguishing_tail(name, 36)
}


/// Whether a file is column-separated, and so worth colouring by column.
pub(crate) fn is_delimited(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".csv") || name.ends_with(".tsv")
}


/// The colour for one CSV column.
///
/// Cycles the theme's own roles rather than inventing a rainbow: colours already checked
/// against every surface for contrast, so a wide table stays readable in every palette —
/// including the light one, where a fixed rainbow would wash out.
pub(crate) fn column_colour(column: usize) -> u32 {
    const WHEEL: [fn() -> u32; 6] = [
        theme::text,
        theme::accent,
        theme::running,
        theme::success,
        theme::warning,
        theme::text_muted,
    ];
    WHEEL[column % WHEEL.len()]()
}


/// Consecutive identical steps folded into one line with a count.
///
/// An agent hunting for a file emits `glob` eight times in a row, and eight identical lines
/// carry exactly as much information as one — while costing eight lines of the answer's
/// screen space. `glob ×8` says the same thing and reads as one glance.
///
/// Only *consecutive* runs are folded. `read_file ×3, ls ×2, read_file ×3` is a different
/// story from `read_file ×6`, and flattening the order would erase it.
pub(crate) fn fold_steps(steps: &[String]) -> Vec<String> {
    let mut folded: Vec<(String, usize)> = Vec::new();
    for step in steps {
        match folded.last_mut() {
            Some((label, count)) if label == step => *count += 1,
            _ => folded.push((step.clone(), 1)),
        }
    }
    folded
        .into_iter()
        .map(|(label, count)| {
            if count > 1 {
                format!("{label} ×{count}")
            } else {
                label
            }
        })
        .collect()
}


/// The gutter glyph for a list item at a given depth.
///
/// Only bullets change. A numbered item keeps the number the author wrote — renumbering it, or
/// swapping it for a bullet because it happens to be nested, would change what the answer says.
pub(crate) fn nested_marker(marker: &str, depth: usize) -> String {
    if marker.ends_with('.') {
        return marker.to_string();
    }
    match depth {
        0 => "·".to_string(),
        1 => "‣".to_string(),
        _ => "–".to_string(),
    }
}


/// The glyph and colour that stand for a file's kind.
///
/// Finer than [`workspace::Kind`], which groups by what a researcher *does* with a file and is
/// the right grouping for the panel's sections. Here a PDF and a Markdown note want telling
/// apart at a glance even though both are things you read.
///
/// Four colours, from the palette's status roles rather than a new set — the same argument as
/// the provenance chips: a colour per file type is a legend nobody memorises.
pub(crate) fn file_mark(path: &std::path::Path) -> (&'static str, u32) {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" | "tsv" | "xlsx" | "xls" | "parquet" | "feather" => {
            ("icons/file-table.svg", theme::success())
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => {
            ("icons/file-image.svg", theme::running())
        }
        "py" | "r" | "jl" | "sh" | "js" | "ts" | "rs" | "sql" => {
            ("icons/file-code.svg", theme::accent())
        }
        "ipynb" => ("icons/file-notebook.svg", theme::accent()),
        "json" | "yaml" | "yml" | "toml" | "xml" => ("icons/file-data.svg", theme::warning()),
        "html" | "htm" => ("icons/file-web.svg", theme::warning()),
        "md" | "txt" | "rst" => ("icons/file-text.svg", theme::text_muted()),
        "log" | "out" | "err" => ("icons/file-log.svg", theme::text_faint()),
        "pdf" | "docx" | "doc" | "typ" => ("icons/file-doc.svg", theme::error()),
        "zip" | "gz" | "tar" | "tgz" | "7z" => ("icons/file-archive.svg", theme::text_muted()),
        "db" | "sqlite" | "sqlite3" | "duckdb" => ("icons/file-db.svg", theme::success()),
        _ => ("icons/file-blank.svg", theme::text_muted()),
    }
}


/// Images in one group, everything else in another, each keeping its listing order.
///
/// **The boundary the researcher asked for**, in their words: *"I want to group images and in
/// another group other files."* §152's gallery grouped by the folder the agent chose, which was
/// right about structure and wrong about kind — a folder holding seven plots and a summary CSV
/// put the CSV in the middle of the strip, and the strip is the thing you flick through looking
/// for a figure.
///
/// `Kind::Figure` is the test rather than the extension, so this cannot disagree with the
/// thumbnail renderer about what an image is: both ask the same enum.
pub(crate) fn split_images(
    outputs: &[workspace::Output],
) -> (Vec<workspace::Output>, Vec<workspace::Output>) {
    outputs
        .iter()
        .cloned()
        .partition(|output| output.kind == workspace::Kind::Figure)
}


/// How wide the thumb is for a rail showing `viewport` of `viewport + overflow` content.
pub(crate) fn horizontal_thumb_width(viewport: f32, overflow: f32) -> f32 {
    let content = viewport + overflow;
    (viewport * (viewport / content)).max(28.).min(viewport)
}


/// Convert a dragged thumb position into a negative content offset.
pub(crate) fn horizontal_drag_offset(
    pointer_x: f32,
    track_left: f32,
    grab_x: f32,
    travel: f32,
    overflow: f32,
) -> f32 {
    if travel <= 0. {
        return 0.;
    }
    let thumb_left = (pointer_x - track_left - grab_x).clamp(0., travel);
    -(overflow * (thumb_left / travel))
}



// The behavior suite stays immediately after the UI implementation it exercises, while
// the CLI-only launch helpers remain at the bottom of the executable. Moving this large
// module past startup code would create merge churn without changing test visibility
// (the source-order lesson recorded in docs §118).
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

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
             {\"command\":\"c\",\"outside\":[\"/tmp/read.csv\"],\"wrote\":[]}\n\
             {\"command\":\"python analysis.py\",\"outside\":[],\"wrote\":[\"/tmp/z.png\"]}",
        );
        let files = files_left_outside(&commands);
        // A duplicate, two named writes, and one relative write found only from cwd observation.
        assert_eq!(files, vec![
            "/tmp/x.png".to_string(),
            "/tmp/y.png".to_string(),
            "/tmp/z.png".to_string(),
        ]);
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
    /// that matters names files the panel below it cannot show. It says **wrote** only for a path
    /// the filesystem observation confirmed and **named** for the weaker command-text evidence.
    #[test]
    fn the_summary_uses_the_strongest_evidence_it_has() {
        let fixture = include_str!("../tests/fixtures/command-record.jsonl");
        let commands = workspace::decode_commands(fixture);
        let (summary, loud) = commands_summary(&commands);

        assert!(summary.starts_with("5 commands"), "{summary}");
        assert!(summary.contains("1 failed"), "{summary}");
        // One command is *confirmed* to have written outside, so the line says so — that is a fact
        // about a file, established from its mtime, not a reading of the command's text.
        assert!(summary.contains("2 wrote a file outside this conversation"), "{summary}");
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
    fn a_first_turn_hands_the_source_and_its_prompt_reference_to_the_sidecar() {
        let source = std::path::PathBuf::from(
            r"C:\Users\LENOVO\Documents\workshop mini-me\dataset.csv",
        );
        let reference = "/mnt/c/Users/LENOVO/Documents/workshop mini-me/dataset.csv";
        let attachment = Attachment {
            label: "dataset.csv".into(),
            source: source.clone(),
            adopted: false,
            reference: reference.into(),
        };

        assert_eq!(
            attachments_for_turn(&[attachment]),
            vec![workspace::PendingAttachment {
                source,
                reference: reference.into(),
            }]
        );
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
        let offset = |pointer: f32| {
            horizontal_drag_offset(pointer, 100., 20., 200., 800.)
        };
        assert_eq!(offset(0.), 0.);
        assert_eq!(offset(120.), 0.);
        assert_eq!(offset(220.), -400.);
        assert_eq!(offset(400.), -800.);
    }

    #[test]
    fn a_gallery_thumb_never_grows_wider_than_the_rail_it_sits_in() {
        // A wide rail: the thumb is proportional, and there is room to drag it.
        let wide = horizontal_thumb_width(400., 800.);
        assert!(wide > 28. && wide < 400., "{wide:?}");

        // A long rail: proportional would be a few pixels, so the 28px floor applies.
        assert_eq!(horizontal_thumb_width(300., 9_000.), 28.);

        // A rail narrower than that floor is the case the floor alone gets wrong. The thumb has
        // to stop at the track width, because `travel = viewport - thumb` going negative paints
        // it outside the track and leaves it undraggable — a control that reads as broken rather
        // than as absent.
        for narrow in [1., 10., 27.9] {
            let thumb = horizontal_thumb_width(narrow, 500.);
            assert_eq!(thumb, narrow, "a {narrow}px rail");
            assert!(narrow - thumb >= 0., "travel went negative at {narrow}");
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

    /// What lands in the transcript is what the researcher typed — the blockquote naming where
    /// the file went is for the coordinator, and the chat header says that once instead of §267
    /// repeating it inline on every attached turn.
    #[test]
    fn the_transcript_shows_what_was_typed_not_the_blockquote_sent_alongside_it() {
        let sent = with_attachments("profile this", &[attached("a.csv", "./a.csv")]);
        assert_eq!(without_attached_blockquote(&sent), "profile this");
        // Nothing attached: the blockquote was never prepended, so there is nothing to strip.
        assert_eq!(without_attached_blockquote("what is late blight?"), "what is late blight?");
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

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
                let app_menu = SubmenuBuilder::new(app, "Mini-Me Desktop")
                    .item(&PredefinedMenuItem::about(app, None, None)?)
                    .separator()
                    .item(&PredefinedMenuItem::quit(app, None)?)
                    .build()?;
                let edit_menu = SubmenuBuilder::new(app, "Edit")
                    .item(&PredefinedMenuItem::undo(app, None)?)
                    .item(&PredefinedMenuItem::redo(app, None)?)
                    .separator()
                    .item(&PredefinedMenuItem::cut(app, None)?)
                    .item(&PredefinedMenuItem::copy(app, None)?)
                    .item(&PredefinedMenuItem::paste(app, None)?)
                    .item(&PredefinedMenuItem::select_all(app, None)?)
                    .build()?;
                let menu = MenuBuilder::new(app).item(&app_menu).item(&edit_menu).build()?;
                app.set_menu(menu)?;
            }
            Ok(())
        })
        .manage(sidecar)
        .invoke_handler(tauri::generate_handler![
            commands::get_execution_label,
            commands::get_base_url,
            commands::get_settings,
            commands::save_settings,
            commands::get_secret,
            commands::set_secret_value,
            commands::get_providers,
            commands::search_themes,
            commands::install_theme,
            commands::list_installed_themes,
            commands::submit_turn,
            commands::resume_turn,
            commands::cancel_turn,
            commands::reset_thread,
            commands::get_thread_id,
            commands::set_project,
            commands::get_project,
            commands::fetch_project,
            commands::set_mission,
            commands::warm_up,
            commands::warm_graph,
            commands::restart_backend,
            commands::list_conversations,
            commands::open_conversation,
            commands::delete_conversations,
            commands::rename_conversation,
            commands::sweep_finished_jobs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the Tauri app");
}
