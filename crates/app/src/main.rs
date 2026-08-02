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
mod composer;
mod markdown;
mod preflight;
mod protocol;
mod settings;
mod sidecar;
mod theme;
mod workspace;

use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt;
use gpui::{
    actions, div, img, prelude::*, px, rgb, size, App, Application, Bounds, ClipboardItem, Context,
    Entity, Focusable, FontStyle, FontWeight, HighlightStyle, KeyBinding, SharedString, StyledText,
    Window, WindowBounds, WindowOptions,
};

use composer::{Composer, ComposerEvent};
use theme::*;
use protocol::{AgentRef, ApprovalRequest, Bucket, Decision, Project, TurnEvent};
use sidecar::Sidecar;

// ---- Palette (placeholder; align with the web app's tokens in P6.3) --------

/// Prefilled into the composer on first launch so Enter alone proves the round
/// trip; the user can clear or replace it.
const SEED_PROMPT: &str = "In one short paragraph, what is your role as the Mini-Me coordinator?";

/// A small caps-ish section heading for the side panel.
fn section_label(text: &'static str) -> impl IntoElement {
    div().text_color(rgb(ACCENT)).text_xs().child(text)
}

/// One line of the activity trace: a tool call, or a delegation.
fn step_line(label: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(TEXT_MUTED))
        .text_xs()
        .child(format!("· {label}"))
}

/// Consecutive identical steps folded into one line with a count.
///
/// An agent hunting for a file emits `glob` eight times in a row, and eight identical lines
/// carry exactly as much information as one — while costing eight lines of the answer's
/// screen space. `glob ×8` says the same thing and reads as one glance.
///
/// Only *consecutive* runs are folded. `read_file ×3, ls ×2, read_file ×3` is a different
/// story from `read_file ×6`, and flattening the order would erase it.
fn fold_steps(steps: &[String]) -> Vec<String> {
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

/// A labelled, bulleted list of spine entries.
fn spine_list(label: &'static str, items: &[String], bullet: &'static str) -> impl IntoElement {
    let mut list = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(section_label(label));
    for item in items {
        list = list.child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(div().flex_none().text_color(rgb(TEXT_MUTED)).text_sm().child(bullet))
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .text_color(rgb(TEXT))
                        .text_sm()
                        .child(item.clone()),
                ),
        );
    }
    list
}

actions!(
    workbench,
    [TogglePalette, PaletteNext, PalettePrev, PaletteDismiss, ToggleSettings]
);

/// The editable fields in Settings, in the order they are shown.
///
/// Secret fields never display what is stored — the keychain is write-only from here, and
/// the panel says "stored" or "not set" beside them. A field left blank on save keeps
/// whatever is already in the keychain; that is what lets someone change their model
/// without re-pasting a key.
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
            Field::ModelId => "Model",
            Field::BaseUrl => "Base URL",
            Field::ApiKey => "API key",
            Field::AstaToken => "Asta token",
            Field::AstaApiKey => "Asta API key",
            Field::Port => "Backend port",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Field::ModelId => "model id",
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
    ];
    for modifier in ["cmd", "ctrl"] {
        bindings.push(KeyBinding::new(&format!("{modifier}-p"), TogglePalette, None));
        bindings.push(KeyBinding::new(&format!("{modifier}-,"), ToggleSettings, None));
    }
    bindings
}

/// One command-palette entry.
///
/// Deliberately a closed enum rather than a registry of closures: the whole point of
/// the palette is that every action is also reachable another way, so there is no
/// dynamic set to register.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Command {
    RunTurn,
    NewThread,
    RefreshSpine,
    ExpandTraces,
    CollapseTraces,
    CopyLastAnswer,
    OpenSettings,
    OpenSetup,
    Quit,
}

impl Command {
    const ALL: [Command; 9] = [
        Command::RunTurn,
        Command::NewThread,
        Command::RefreshSpine,
        Command::ExpandTraces,
        Command::CollapseTraces,
        Command::CopyLastAnswer,
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
fn match_score(query: &str, label: &str) -> Option<i32> {
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

/// Render one Markdown block as an element.
///
/// Emphasis becomes a `HighlightStyle` run rather than a nested element, which is how GPUI
/// wants inline styling: one shaped line per block, with ranges carrying the differences.
fn markdown_block(block: &markdown::Block) -> gpui::AnyElement {
    use markdown::{Block, Emphasis};

    let styled = |inlines: &markdown::Inlines, base: u32| {
        let highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = inlines
            .styles
            .iter()
            .map(|(range, emphasis)| {
                let style = match emphasis {
                    Emphasis::Strong => HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                    Emphasis::Italic => HighlightStyle {
                        font_style: Some(FontStyle::Italic),
                        ..Default::default()
                    },
                    // No monospace family is bundled yet, so code is marked by colour.
                    // Honest and legible; a real code face is a follow-up.
                    Emphasis::Code => HighlightStyle {
                        color: Some(rgb(ACCENT).into()),
                        ..Default::default()
                    },
                    Emphasis::Link => HighlightStyle {
                        color: Some(rgb(ACCENT).into()),
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Emphasis::Url => HighlightStyle {
                        color: Some(rgb(TEXT_MUTED).into()),
                        ..Default::default()
                    },
                };
                (range.clone(), style)
            })
            .collect();
        div().w_full().min_w_0().text_color(rgb(base)).child(
            StyledText::new(inlines.text.clone()).with_highlights(highlights),
        )
    };

    match block {
        Block::Heading { level, inlines } => {
            let element = styled(inlines, TEXT);
            // Only two sizes: an answer is not a document, and six heading levels of
            // typography would be noise.
            if *level <= 2 {
                element.text_lg().into_any_element()
            } else {
                element.into_any_element()
            }
        }
        Block::Paragraph(inlines) => styled(inlines, TEXT).into_any_element(),
        Block::ListItem { marker, inlines } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(TEXT_MUTED))
                    .child(marker.clone()),
            )
            .child(styled(inlines, TEXT))
            .into_any_element(),
        Block::Code { text, .. } => div()
            .w_full()
            .min_w_0()
            .p_2()
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER))
            .text_color(rgb(TEXT))
            .text_sm()
            .child(text.clone())
            .into_any_element(),
        Block::Table { header, rows } => {
            // Equal-width columns via `flex_1`, rather than measuring content. GPUI has no
            // table layout and measuring text before shaping is not something this app can
            // do honestly; even columns are predictable and never collapse a column to
            // nothing, which is what a naive proportional split does to a long cell.
            let columns = block.columns();
            let cell = |inlines: &markdown::Inlines, bold: bool| {
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .child(styled(inlines, if bold { TEXT } else { TEXT_MUTED }))
                    .when(bold, |row| row.font_weight(FontWeight::BOLD))
            };
            // Pad short rows so columns stay aligned when the source is ragged.
            let padded = |row: &Vec<markdown::Inlines>| {
                let mut cells: Vec<markdown::Inlines> = row.clone();
                cells.resize_with(columns, Default::default);
                cells
            };

            let mut table = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .border_1()
                .border_color(rgb(BORDER));
            if !header.is_empty() {
                let mut head = div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .bg(rgb(SURFACE))
                    .border_b_1()
                    .border_color(rgb(BORDER));
                for value in padded(header) {
                    head = head.child(cell(&value, true));
                }
                table = table.child(head);
            }
            for (index, row) in rows.iter().enumerate() {
                let mut line = div().flex().flex_row().w_full().min_w_0();
                // A hairline between rows, but not under the last one — the table's own
                // border already closes it.
                if index + 1 < rows.len() {
                    line = line.border_b_1().border_color(rgb(BORDER));
                }
                for value in padded(row) {
                    line = line.child(cell(&value, false));
                }
                table = table.child(line);
            }
            table.into_any_element()
        }
        Block::Rule => div()
            .w_full()
            .border_b_1()
            .border_color(rgb(BORDER))
            .into_any_element(),
    }
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
/// Turn dropped files into a prompt the researcher can edit before sending.
///
/// **Loaded into the composer, never sent.** Dropping a file is a clumsy gesture — it
/// happens by accident — and the same rule already governs the suggestion cards: the app
/// prepares the question, the person asks it (docs §12).
///
/// Directories are named as directories, because "analyse this folder of readings" is a
/// real request and the agent can list it itself.
fn prompt_for_dropped(paths: &[String], directories: &[bool]) -> String {
    match paths {
        [] => String::new(),
        [one] => {
            if directories.first().copied().unwrap_or(false) {
                format!("Have a look at the files in {one} and tell me what is there.")
            } else {
                format!("Analyse the data in {one}. Start by describing what it contains.")
            }
        }
        many => format!(
            "Analyse these files together:\n{}",
            many.iter()
                .map(|path| format!("- {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

/// The first `https://` URL in a line of command output.
///
/// Stops at whitespace and trims the punctuation a sentence tends to leave attached, so
/// "open https://example.org/x." yields the URL without the full stop.
fn first_url(line: &str) -> Option<String> {
    let at = line.find("https://")?;
    let url: String = line[at..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    // A trailing colon is not punctuation you would expect to matter — until you see the
    // real line, `gio: <url>: Operation not supported`, where it is what separates the URL
    // from the error. Safe to strip: a colon is meaningful *inside* a URL (a port), never
    // at the end of one.
    let url = url.trim_end_matches(['.', ',', ':', ';', ')', ']', '"', '\'']);
    // A bare scheme is not a link.
    (url.len() > "https://".len()).then(|| url.to_string())
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
        std::process::Command::new("open").arg(url).spawn().map(|_| ())
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

/// A single chat message in the transcript, plus the agent activity behind it.
struct Message {
    role: &'static str,
    body: String,
    /// Coordinator-level steps (tool calls, delegations), in the order they happened.
    steps: Vec<String>,
    /// One group per subagent invocation.
    agents: Vec<AgentTrace>,
    /// Whether the coordinator's own steps are showing.
    ///
    /// Open while the turn runs, because during a two-minute wait the steps *are* the only
    /// sign of progress; closed once it ends, because then the answer is the point. Zed's
    /// agent panel converged on the same shape, and the pattern has a name — expand live,
    /// collapse on completion (docs §47).
    steps_expanded: bool,
    /// Figures this turn drew, shown inline beneath the answer.
    ///
    /// Found by diffing the thread's workspace across the turn rather than reported by the
    /// agent: a plot is usually written by a `matplotlib` script inside `execute`, which
    /// registers no artifact and tells the client nothing. The file appearing on disk is
    /// the only signal there is (docs §42).
    plots: Vec<std::path::PathBuf>,
}

impl Message {
    fn new(role: &'static str, body: String) -> Self {
        Self {
            role,
            body,
            steps: Vec::new(),
            agents: Vec::new(),
            steps_expanded: true,
            plots: Vec::new(),
        }
    }

    /// Nothing happened here worth keeping. A turn that produced only tool calls
    /// still has activity, so "empty body" alone is not enough to drop a message —
    /// that would throw away the only record of a purely delegated turn.
    fn is_silent(&self) -> bool {
        self.body.is_empty() && self.steps.is_empty() && self.agents.is_empty()
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
    done: bool,
    ok: bool,
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
    setup_open: bool,
    report: Option<preflight::Report>,
    checking: bool,
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
    /// The researcher's past conversations, newest first.
    conversations: Vec<protocol::Conversation>,
    /// A name to give the current conversation once its thread exists.
    pending_title: Option<String>,
    /// The thread whose name is being edited, if any.
    renaming: Option<String>,
    /// The field that edits it. One shared editor rather than one per row — only one
    /// name can be edited at a time, and a Composer per conversation would be an entity
    /// per row for a list that can run to hundreds.
    rename_editor: Entity<Composer>,
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
    fn new(sidecar: Arc<Sidecar>, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            let mut composer = Composer::new(cx, "Ask Mini-Me…  (Enter to send)");
            composer.set_text(SEED_PROMPT, cx);
            composer
        });
        // The composer only reports *that* text was submitted; deciding it means
        // "run a coordinator turn" stays here.
        cx.subscribe(&composer, |workbench, _composer, event, cx| match event {
            ComposerEvent::Submit(text) => workbench.start_turn(text.clone(), cx),
        })
        .detach();

        // Renaming a conversation. Submit commits the new name; the sidebar row is
        // replaced by this field while it is in force.
        let rename_editor = cx.new(|cx| Composer::new(cx, "Name this conversation"));
        cx.subscribe(&rename_editor, |workbench, _editor, event, cx| match event {
            ComposerEvent::Submit(text) => workbench.commit_rename(text.clone(), cx),
        })
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

        let mut workbench = Self {
            project: None,
            buckets: Vec::new(),
            jobs: Vec::new(),
            tasks: Vec::new(),
            transcript: Vec::new(),
            sidecar,
            status: "idle — type a prompt and press Enter".to_string(),
            streaming: false,
            error: None,
            composer,
            palette_open: false,
            palette_selected: 0,
            palette_query,
            settings_open: false,
            draft: settings::Settings::load(),
            fields,
            settings_note: String::new(),
            setup_open: false,
            report: None,
            checking: false,
            running_fix: None,
            pending_approval: None,
            approve_rest_of_turn: false,
            approve_conversation: false,
            conversations: Vec::new(),
            pending_title: None,
            renaming: None,
            rename_editor,
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

        // Populate the spine if a backend is already listening. This does not
        // start one — see `Sidecar::fetch_project`.
        workbench.refresh_project(cx);
        // And the researcher's own history, so the window opens on their work rather
        // than on a blank slate that implies there is none.
        workbench.refresh_conversations(cx);
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
            job.kind.expected()
        );
        let mut updates = self.sidecar.watch_job(job);
        cx.spawn(async move |this, cx| {
            while let Some(update) = updates.next().await {
                let carry_on = this.update(cx, |workbench, cx| {
                    let finished = update.is_finished();
                    let label = update.kind.label();
                    let succeeded = update.succeeded();
                    if let Some(tracked) =
                        workbench.jobs.iter_mut().find(|k| k.task_id == update.task_id)
                    {
                        tracked.status = update.status.clone();
                    }
                    if finished {
                        workbench.status = if succeeded {
                            format!("{label} finished — its results are in the sandbox")
                        } else {
                            format!("{label} ended: {}", update.status)
                        };
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
    fn track_task(&mut self, task: protocol::AsyncTask, cx: &mut Context<Self>) {
        if let Some(existing) = self.tasks.iter_mut().find(|t| t.task_id == task.task_id) {
            // The snapshot knows the status the coordinator last recorded; the *watcher*
            // knows whether it is stopped at the gate right now. Never let a stale
            // snapshot erase a pending approval the user is looking at.
            if existing.pending.is_none() && !existing.is_finished() {
                existing.status = task.status;
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
                    if let Some(tracked) =
                        workbench.tasks.iter_mut().find(|t| t.task_id == update.task_id)
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
                        workbench.status =
                            "a background task is waiting for your approval".into();
                    } else if finished {
                        workbench.status = if succeeded {
                            "a background task finished".into()
                        } else {
                            "a background task stopped".into()
                        };
                        workbench.collect_plots();
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
        // One decision per held action, in order — the agent validates the count.
        let decisions: Vec<Decision> = request
            .actions
            .iter()
            .map(|_| {
                if approve {
                    Decision::Approve
                } else {
                    Decision::Reject {
                        message: "The researcher declined to run this command.".to_string(),
                    }
                }
            })
            .collect();
        let thread_id = task.thread_id.clone();
        task.status = "running".into();
        self.sidecar.decide_task(thread_id, decisions);
        self.status = if approve {
            "background task approved — running…"
        } else {
            "background task rejected"
        }
        .into();
        cx.notify();
    }

    /// A file was dropped on the window.
    ///
    /// The one thing the web app cannot do: the researcher's data is already on this
    /// machine, and this is the whole distance between "here is my CSV" and an analysis —
    /// no upload, no copy, no bucket.
    fn files_dropped(&mut self, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        if self.streaming {
            self.status = "finish this turn before adding files".into();
            cx.notify();
            return;
        }
        // Translated to the backend's view of the filesystem — on Windows the agent runs
        // inside WSL, where `C:\…` is `/mnt/c/…`.
        let translated: Vec<String> = paths
            .iter()
            .map(|path| self.sidecar.path_for_backend(path))
            .collect();
        let directories: Vec<bool> = paths.iter().map(|path| path.is_dir()).collect();

        let prompt = prompt_for_dropped(&translated, &directories);
        self.composer
            .update(cx, |composer, cx| composer.set_text(prompt, cx));
        self.restore_focus = true;
        self.status = match paths.len() {
            1 => format!(
                "added {} — edit the question and press Enter",
                paths[0]
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| translated[0].clone())
            ),
            n => format!("added {n} files — edit the question and press Enter"),
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
                    if first && blocked {
                        workbench.setup_open = true;
                        workbench.settings_open = false;
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
        self.setup_open = true;
        self.settings_open = false;
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

    /// Run a fix on the user's behalf, streaming its output into the pane.
    ///
    /// Re-checks automatically when it finishes, so a successful install turns its own
    /// row green without the user having to work out that they should press Re-check.
    fn start_fix(&mut self, label: String, argv: Vec<String>, cx: &mut Context<Self>) {
        if self.running_fix.as_ref().is_some_and(|fix| !fix.done) {
            return;
        }
        self.status = format!("running: {label}");
        self.running_fix = Some(RunningFix {
            label,
            link: None,
            lines: Vec::new(),
            done: false,
            ok: false,
        });
        let mut events = self.sidecar.run_fix(argv);
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
                            fix.lines.push(format!("— {note}"));
                            // Credentials are read into the backend's environment when it
                            // *starts*, so signing in while it runs changes nothing until
                            // it is restarted. Saying so is the difference between a fix
                            // that works and one that looks broken — signing in from this
                            // pane and then watching the same failure is exactly what
                            // happened the first time (docs §32).
                            if ok && fix.label.contains("Sign in") {
                                fix.lines.push(
                                    "— Close and reopen the app: the backend reads your \
                                     Asta sign-in when it starts."
                                        .into(),
                                );
                            }
                            workbench.status = format!(
                                "{}: {note}",
                                if ok { "done" } else { "failed" }
                            );
                            // Re-check on success so the row the user just fixed goes
                            // green by itself.
                            if ok {
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
        if self.streaming || prompt.trim().is_empty() {
            return;
        }
        self.streaming = true;
        self.error = None;
        self.status = "starting…".into();
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(true, cx));
        // Name the conversation after the first thing asked. A sidebar of "New
        // conversation" is a sidebar of nothing, and every chat app auto-titles for
        // exactly this reason; the researcher can rename it whenever they like.
        let first_turn = self.transcript.is_empty();
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

    fn apply(&mut self, event: TurnEvent, cx: &mut Context<Self>) {
        match event {
            TurnEvent::Status(status) => self.status = status,
            TurnEvent::Token(text) => {
                if let Some(last) = self.transcript.last_mut() {
                    last.body.push_str(&text);
                }
            }
            // Activity attaches to the in-flight assistant message, so it sits with
            // the answer it produced instead of in a panel the user has to correlate.
            TurnEvent::Step { agent, label } => {
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
                if let Some(message) = self.transcript.last_mut() {
                    trace_for(message, &agent).push_text(&text);
                }
            }
            // Each `values` event is a *whole* snapshot, so replace rather than
            // merge. The spine rides along in the same payload, which keeps the
            // mission current without another HTTP round trip.
            TurnEvent::Snapshot(snapshot) => {
                if let Some(project) = snapshot.project {
                    self.project = Some(merge_spine(self.project.as_ref(), project));
                }
                if !snapshot.buckets.is_empty() {
                    self.buckets = snapshot.buckets;
                }
                for job in snapshot.jobs {
                    self.track_job(job, cx);
                }
                for task in snapshot.tasks {
                    self.track_task(task, cx);
                }
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
    fn refresh_conversations(&mut self, cx: &mut Context<Self>) {
        let mut updates = self.sidecar.list_conversations();
        cx.spawn(async move |this, cx| {
            if let Some(conversations) = updates.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.conversations = conversations;
                    cx.notify();
                });
            }
        })
        .detach();
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
        self.buckets.clear();
        self.tasks.clear();
        self.jobs.clear();
        self.error = None;
        self.approve_conversation = false;
        self.approve_tasks.clear();
        self.status = "opening…".into();

        let mut messages = self.sidecar.open_conversation(thread_id);
        cx.spawn(async move |this, cx| {
            if let Some(messages) = messages.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    for (role, body) in messages {
                        // Roles come back as the two the transcript renders; anything
                        // else was filtered out server-side by `decode_stored_message`.
                        let role = if role == "you" { "you" } else { "mini-me" };
                        workbench.transcript.push(Message::new(role, body));
                    }
                    // Figures this conversation produced are still on disk, so they can
                    // be shown again — history the transcript alone cannot carry.
                    workbench.collect_plots();
                    workbench.status = "done".into();
                    workbench.refresh_project(cx);
                    cx.notify();
                });
            }
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

    /// The thread's own output directory, or `None` before the first turn creates one.
    fn thread_workspace(&self) -> Option<std::path::PathBuf> {
        self.sidecar
            .thread_id()
            .map(|thread_id| workspace::thread_dir(&thread_id))
    }

    /// Every figure currently in this thread's workspace.
    fn workspace_images(&self) -> Vec<std::path::PathBuf> {
        self.thread_workspace()
            .map(|dir| workspace::images(&dir))
            .unwrap_or_default()
    }

    /// Attach any figure not already on screen to the newest answer.
    ///
    /// A diff rather than a report, because nothing reports it: a figure is written by a
    /// plotting script inside `execute`, which registers no artifact (docs §42).
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
            .flat_map(|message| message.plots.iter().cloned())
            .collect();
        let drawn: Vec<_> = self
            .workspace_images()
            .into_iter()
            .filter(|path| !shown.contains(path))
            .collect();
        if drawn.is_empty() {
            return;
        }
        if let Some(message) = self
            .transcript
            .iter_mut()
            .rev()
            .find(|message| message.role == "mini-me")
        {
            message.plots.extend(drawn);
        }
    }

    fn finish_turn(&mut self, cx: &mut Context<Self>) {
        self.collect_plots();
        // The thread id does not exist until the turn has run, which is why the title
        // waits until here rather than being set when the prompt was typed.
        if let (Some(title), Some(thread_id)) = (self.pending_title.take(), self.sidecar.thread_id())
        {
            self.sidecar.rename_conversation(thread_id, title);
        }
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
        self.composer
            .update(cx, |composer, cx| composer.set_disabled(false, cx));
        // A turn can change the spine — the mission is derived from the first
        // question, and completed/pending shift as work lands.
        self.refresh_project(cx);
    }

    /// The conversation list.
    ///
    /// The backend has stored every thread since the first launch; the app simply never
    /// asked, so a 64px rail with a decorative glyph was all there was and every session
    /// looked like the first one. Past work was not lost — it was unreachable, which for
    /// the researcher is the same thing (docs §48).
    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.sidecar.thread_id();

        let mut list = div()
            .id("conversations")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .overflow_y_scroll()
            .p_1()
            .gap_px();

        if self.conversations.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(TEXT_FAINT))
                    .text_xs()
                    .child("Conversations you start will appear here."),
            );
        }

        for conversation in &self.conversations {
            let thread_id = conversation.thread_id.clone();
            let selected = current.as_deref() == Some(thread_id.as_str());
            let renaming = self.renaming.as_deref() == Some(thread_id.as_str());

            // Renaming happens in place, in the row itself — the pattern every chat app
            // uses, and the one that keeps the name next to the thing being named.
            if renaming {
                list = list.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .child(self.rename_editor.clone()),
                );
                continue;
            }

            let open = thread_id.clone();
            let rename = thread_id.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("conv-{thread_id}")))
                    .group(SharedString::from(format!("conv-group-{thread_id}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .when(selected, |row| row.bg(rgb(ACCENT_SOFT)))
                    // Every row reacts to the pointer. A list that does not respond to
                    // the cursor does not read as a list of *buttons*.
                    .hover(|style| style.bg(rgb(RAISED)).cursor_pointer())
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .truncate()
                            .text_color(rgb(if selected { TEXT } else { TEXT_MUTED }))
                            .text_xs()
                            .child(conversation.title.clone()),
                    )
                    .child(
                        // Hidden until the row is hovered, so the list stays a list of
                        // names rather than a wall of controls.
                        div()
                            .id(SharedString::from(format!("rename-{thread_id}")))
                            .flex_none()
                            .invisible()
                            .group_hover(
                                SharedString::from(format!("conv-group-{thread_id}")),
                                |style| style.visible(),
                            )
                            .px_1()
                            .text_color(rgb(TEXT_FAINT))
                            .text_xs()
                            .hover(|style| style.text_color(rgb(ACCENT)).cursor_pointer())
                            .child("rename")
                            .on_click(cx.listener(move |workbench, _event, window, cx| {
                                workbench.start_rename(rename.clone(), window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.open_conversation(open.clone(), cx);
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .w(px(220.))
            .h_full()
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(div().text_color(rgb(ACCENT)).child("◎"))
                    .child(
                        div()
                            .id("new-conversation")
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(BORDER_STRONG))
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .hover(|style| {
                                style
                                    .text_color(rgb(ACCENT))
                                    .border_color(rgb(ACCENT))
                                    .cursor_pointer()
                            })
                            .child("New")
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.run_command(Command::NewThread, cx);
                            })),
                    ),
            )
            .child(list)
    }

    fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // `min_w_0` is what makes long assistant text *wrap* instead of running off
        // the right edge: a flex item defaults to min-width:auto, so its content
        // width becomes its floor and a long paragraph widens the pane instead of
        // flowing down.
        // `id` + `overflow_y_scroll` is what lets a long transcript scroll; GPUI
        // keeps the scroll offset keyed on that id across re-renders.
        let mut col = div()
            .id("transcript")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3();

        if self.transcript.is_empty() {
            col = col.child(
                div()
                    .text_color(rgb(TEXT_MUTED))
                    .child("No turns yet. Press Run to stream one from the local sidecar."),
            );
        }
        for (index, message) in self.transcript.iter().enumerate() {
            let label_color = if message.role == "you" { TEXT_MUTED } else { ACCENT };
            let has_activity = !message.steps.is_empty() || !message.agents.is_empty();
            // An empty assistant body means we're still waiting on the first token —
            // unless a trace is already showing what's going on, which says more.
            let body = if message.body.is_empty() && self.streaming && !has_activity {
                "…".to_string()
            } else {
                message.body.clone()
            };
            let mut block = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_color(rgb(label_color))
                        .text_sm()
                        .child(message.role),
                );
            // The trace goes *above* the answer, because that is the order it
            // happened in and because the answer should be the last thing read.
            if has_activity {
                block = block.child(self.activity_block(index, message, cx));
            }
            if !body.is_empty() {
                // The user's own text is shown as typed — they wrote it, and reinterpreting
                // their asterisks would be presumptuous. Assistant text is Markdown.
                if message.role == "you" {
                    block = block.child(div().w_full().text_color(rgb(TEXT)).child(body));
                } else {
                    let mut rendered = div().flex().flex_col().w_full().min_w_0().gap_2();
                    for parsed in markdown::parse(&body) {
                        rendered = rendered.child(markdown_block(&parsed));
                    }
                    block = block.child(rendered);
                }
            }
            // Figures last: the answer explains them, so it should be read first.
            for (plot, path) in message.plots.iter().enumerate() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let opened = path.clone();
                block = block.child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(
                            // Capped, not scaled to the pane: a 2000px figure would
                            // otherwise push the transcript's width around as it loads.
                            img(path.clone())
                                .max_w_full()
                                .max_h(px(420.))
                                .object_fit(gpui::ObjectFit::Contain),
                        )
                        .child(
                            div()
                                .id(("plot", index * 64 + plot))
                                .text_color(rgb(TEXT_MUTED))
                                .text_xs()
                                .hover(|style| style.cursor_pointer())
                                .child(format!("{name} — click to open"))
                                .on_click(move |_event, _window, _cx| {
                                    // The figure at full size, in whatever the researcher
                                    // normally views images with.
                                    if let Err(error) = workspace::open(&opened) {
                                        tracing::warn!(%error, "could not open a figure");
                                    }
                                }),
                        ),
                );
            }
            col = col.child(block);
        }

        let mut pane = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .h_full()
            .child(col);
        // Above the composer, so the decision sits where the user's attention already
        // is and cannot be scrolled out of view.
        if let Some(request) = &self.pending_approval {
            pane = pane.child(self.approval_card(request, cx));
        }
        pane.child(self.composer_row(cx)).child(self.status_bar(cx))
    }

    /// Answer the pending approval and pump the continuation into the same turn.
    fn decide(&mut self, approve: bool, cx: &mut Context<Self>) {
        let Some(request) = self.pending_approval.take() else {
            return;
        };
        // Exactly one decision per held action, in the order they were presented —
        // the agent validates the count and errors out if it disagrees.
        let decisions: Vec<Decision> = request
            .actions
            .iter()
            .map(|_| {
                if approve {
                    Decision::Approve
                } else {
                    Decision::Reject {
                        message: "The researcher declined to run this command.".to_string(),
                    }
                }
            })
            .collect();
        self.status = if approve { "approved — running…" } else { "rejected" }.into();

        let mut events = self.sidecar.resume(decisions);
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

    /// The approval card: the command, verbatim, and the two decisions.
    ///
    /// Deliberately shows the command rather than a summary. Host execution means this
    /// runs on the researcher's own machine with their permissions, and the only
    /// meaningful review is of the actual text (docs §19).
    fn approval_card(&self, request: &ApprovalRequest, cx: &mut Context<Self>) -> impl IntoElement {
        let card = div()
            .flex()
            .flex_col()
            // Natural height, never stretched and never squeezed. Without this the card
            // grew with the command and pushed its own buttons — and the composer — off
            // the bottom of the window, which is exactly the review it exists to force.
            .flex_none()
            .w_full()
            .min_w_0()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(rgb(ACCENT))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .text_color(rgb(ACCENT))
                    .text_xs()
                    .child("RUN THIS ON YOUR MACHINE?"),
            );

        // The command scrolls; the decision does not. An agent-written script runs to
        // hundreds of lines, and the whole point of this gate is that Approve and Reject
        // stay reachable no matter how long the thing being approved is.
        let mut commands = div()
            .id("approval-commands")
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .max_h(px(260.))
            .overflow_y_scroll();

        for action in &request.actions {
            if !action.description.is_empty() {
                commands = commands.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .text_color(rgb(TEXT_MUTED))
                        .text_xs()
                        .child(action.description.clone()),
                );
            }
            commands = commands.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .p_2()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(TEXT))
                    .text_sm()
                    .child(action.detail.clone()),
            );
        }

        card.child(commands).child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    div()
                        .id("approve")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .text_color(rgb(ACCENT))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Approve")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.decide(true, cx)
                        })),
                )
                .child(
                    div()
                        .id("reject")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Reject")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.decide(false, cx)
                        })),
                )
                // Bounded to *this turn*, and nothing is persisted. A permanent
                // "always allow" is how a security gate becomes a habit: the tenth
                // identical dialog in one analysis is not read, it is dismissed, and
                // then neither is the eleventh — which is the one that mattered.
                // Approving the rest of one task is a decision someone can actually
                // hold in their head, and it expires on its own.
                .child(
                    div()
                        .id("approve-turn")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Approve the rest of this turn")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_rest_of_turn = true;
                            workbench.decide(true, cx);
                        })),
                )
                // The wider grant, asked for because one analysis is a dozen commands
                // across several turns and nobody reads the twelfth dialog. It covers
                // background workers too — they are where the clicking is worst, since
                // there is no one watching the panel. Still bounded: "New thread" or
                // closing the app ends it, nothing is written to disk, and the status bar
                // says so for as long as it holds (docs §41).
                .child(
                    div()
                        .id("approve-conversation")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Approve everything in this conversation")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_conversation = true;
                            workbench.decide(true, cx);
                        })),
                ),
        )
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

    fn move_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.palette_commands(cx).len();
        if count == 0 {
            return;
        }
        // Wrap, so `up` from the first row lands on the last.
        let current = self.palette_selected.min(count - 1) as isize;
        self.palette_selected = (current + delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    fn activate_palette(&mut self, cx: &mut Context<Self>) {
        let Some(command) = self
            .palette_commands(cx)
            .get(self.palette_selected)
            .copied()
        else {
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
        self.settings_note.clear();
        // Both live in the right-hand slot, so opening one closes the other. Setup's
        // "Settings" button is the usual route here — you go there to paste the key it
        // told you was missing.
        self.setup_open = false;
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
            if let Some((_, first)) = self.fields.first() {
                let focus = first.focus_handle(cx);
                window.focus(&focus);
            }
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
        let key_name = self.draft.key_name();
        let secrets: Vec<(String, String)> = self
            .fields
            .iter()
            .filter(|(field, _)| field.is_secret())
            .filter_map(|(field, composer)| {
                let value = composer.read(cx).text().trim().to_string();
                if value.is_empty() {
                    return None;
                }
                let name = field.secret_name().map(str::to_string).unwrap_or_else(|| key_name.clone());
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
        cx.notify();
    }

    /// The Settings pane, in place of the artifacts panel.
    fn settings_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = settings::provider(&self.draft.provider);
        let provider_label = provider.map(|p| p.label).unwrap_or("unknown");
        let needs_base_url = provider.is_some_and(|p| p.needs_base_url);
        let key_name = self.draft.key_name();

        let mut pane = div()
            .id("settings")
            .flex()
            .flex_col()
            .w(px(420.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(SURFACE))
            .border_l_1()
            .border_color(rgb(BORDER))
            .p_4()
            .gap_3()
            .child(section_label("SETTINGS"))
            // Clicking cycles: five providers do not need a dropdown, and a dropdown in
            // GPUI is a whole widget we would have to build.
            .child(
                div()
                    .id("provider")
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .text_color(rgb(TEXT))
                    .text_sm()
                    .hover(|style| style.border_color(rgb(ACCENT)).cursor_pointer())
                    .child(format!("Provider: {provider_label}  ⟳"))
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        let current = settings::PROVIDERS
                            .iter()
                            .position(|p| p.id == workbench.draft.provider)
                            .unwrap_or(0);
                        let next = &settings::PROVIDERS[(current + 1) % settings::PROVIDERS.len()];
                        workbench.draft.provider = next.id.to_string();
                        // Suggest a model that exists for the provider just chosen,
                        // rather than leaving one that does not.
                        let suggested = next.suggested_model.to_string();
                        if let Some((_, composer)) = workbench
                            .fields
                            .iter()
                            .find(|(field, _)| *field == Field::ModelId)
                        {
                            let composer = composer.clone();
                            composer.update(cx, |composer, cx| composer.set_text(suggested, cx));
                        }
                        cx.notify();
                    })),
            );

        for (field, composer) in &self.fields {
            if *field == Field::BaseUrl && !needs_base_url {
                continue;
            }
            let status = if field.is_secret() {
                let name = field.secret_name().map(str::to_string).unwrap_or_else(|| key_name.clone());
                // Presence only — the value itself is never read back into the UI.
                if settings::secret(&name).is_some() {
                    " · stored"
                } else {
                    " · not set"
                }
            } else {
                ""
            };
            pane = pane.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .child(format!("{}{status}", field.label())),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .p_2()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .child(composer.clone()),
                    ),
            );
        }

        // Toggles, as rows rather than checkboxes — a row is one element and reads the
        // same way.
        for (label, value, toggle) in [
            (
                "Run code on this machine",
                self.draft.local_execution,
                0usize,
            ),
            ("Ask before every command", self.draft.approve_execute, 1),
            // Preview API, and it needs the generated graph config — so opt-in, and
            // labelled by what it does rather than by what it is called upstream.
            (
                "Let work run in the background",
                self.draft.async_subagents,
                2,
            ),
        ] {
            pane = pane.child(
                div()
                    .id(SharedString::from(format!("toggle-{toggle}")))
                    .w_full()
                    .p_2()
                    .border_1()
                    .border_color(rgb(if value { ACCENT } else { BORDER }))
                    .text_color(rgb(if value { TEXT } else { TEXT_MUTED }))
                    .text_sm()
                    .hover(|style| style.cursor_pointer())
                    .child(format!("{} {label}", if value { "☑" } else { "☐" }))
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        match toggle {
                            0 => {
                                workbench.draft.local_execution =
                                    !workbench.draft.local_execution
                            }
                            1 => {
                                workbench.draft.approve_execute =
                                    !workbench.draft.approve_execute
                            }
                            _ => {
                                workbench.draft.async_subagents =
                                    !workbench.draft.async_subagents
                            }
                        }
                        cx.notify();
                    })),
            );
        }

        // What is still missing, before the user finds out from a failed turn.
        let has_key = settings::secret(&key_name).is_some()
            || !self.field_text(Field::ApiKey, cx).is_empty();
        for problem in self.draft.problems(has_key) {
            pane = pane.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(ERROR))
                    .text_xs()
                    .child(problem),
            );
        }

        if !self.settings_note.is_empty() {
            pane = pane.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(TEXT_MUTED))
                    .text_xs()
                    .child(self.settings_note.clone()),
            );
        }

        pane.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    div()
                        .id("save-settings")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .text_color(rgb(ACCENT))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Save")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.save_settings(cx)
                        })),
                )
                .child(
                    div()
                        .id("close-settings")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Close")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.settings_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(TEXT_MUTED))
                .text_xs()
                .child(format!(
                    "Keys live in your OS keychain, never in a file. Settings: {}",
                    settings::settings_path().display()
                )),
        )
    }

    /// The Setup pane: one row per check, each carrying the command that fixes it.
    ///
    /// Deliberately not a wizard. A wizard assumes it knows the order things went wrong
    /// in; a checklist just says what is true, which is also what makes it useful the
    /// *second* time — when one thing broke on a machine that used to work.
    fn setup_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = div()
            .id("setup")
            .flex()
            .flex_col()
            .w(px(420.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(SURFACE))
            .border_l_1()
            .border_color(rgb(BORDER))
            .p_4()
            .gap_3()
            .child(section_label("SETUP"));

        match &self.report {
            None => {
                pane = pane.child(
                    div()
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .child("Checking this machine…"),
                );
            }
            Some(report) => {
                pane = pane.child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_color(rgb(if report.ready() { TEXT_MUTED } else { ERROR }))
                                .text_sm()
                                .child(if self.checking {
                                    "Re-checking…".to_string()
                                } else if report.ready() {
                                    format!("Ready to run · {}", report.summary())
                                } else {
                                    format!("Not ready yet · {}", report.summary())
                                }),
                        )
                        // Where the checks ran, because "no checkout" means something
                        // different inside a distro than on this filesystem.
                        .child(
                            div()
                                .text_color(rgb(TEXT_MUTED))
                                .text_xs()
                                .child(format!("{} · {}", report.location, report.execution)),
                        )
                        // Whether the app may maintain that directory. Said out loud
                        // because it decides what the app is allowed to do to the user's
                        // own files, and that must never be a surprise.
                        .child(
                            div().text_color(rgb(TEXT_MUTED)).text_xs().child(if report.owned {
                                "Installed and maintained by this app."
                            } else {
                                "Your own checkout — the app runs it but never modifies it."
                            }),
                        ),
                );

                for check in &report.checks {
                    let color = match check.state {
                        preflight::State::Pass => TEXT_MUTED,
                        preflight::State::Warn => ACCENT,
                        preflight::State::Fail => ERROR,
                        preflight::State::Skip => BORDER,
                    };
                    let mut row = div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .pl_2()
                        .border_l_1()
                        .border_color(rgb(color))
                        .child(
                            div()
                                .text_color(rgb(if check.state == preflight::State::Pass {
                                    TEXT
                                } else {
                                    color
                                }))
                                .text_sm()
                                .child(format!("{} {}", check.state.glyph(), check.label)),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_color(rgb(TEXT_MUTED))
                                .text_xs()
                                .child(check.detail.clone()),
                        );

                    for fix in &check.fixes {
                    match fix {
                        preflight::Fix::Run { label, argv, note } => {
                            let command = preflight::display_argv(argv);
                            let busy = self.running_fix.as_ref().is_some_and(|fix| !fix.done);
                            row = row
                                // The note is not decoration: "asks for admin rights, then
                                // needs a restart" is the difference between a user who
                                // waits and a user who thinks it broke.
                                .child(div().text_color(rgb(TEXT_MUTED)).text_xs().child(*note))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap_2()
                                        .child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "run-{}",
                                                    check.id
                                                )))
                                                .px_3()
                                                .py_1()
                                                .border_1()
                                                .border_color(rgb(if busy { BORDER } else { ACCENT }))
                                                .text_color(rgb(if busy { TEXT_MUTED } else { ACCENT }))
                                                .text_sm()
                                                .hover(|style| style.cursor_pointer())
                                                .child(*label)
                                                .on_click(cx.listener({
                                                    let argv = argv.clone();
                                                    let label = label.to_string();
                                                    move |workbench, _event, _window, cx| {
                                                        workbench.start_fix(
                                                            label.clone(),
                                                            argv.clone(),
                                                            cx,
                                                        );
                                                    }
                                                })),
                                        )
                                        // Kept alongside the button: someone who would
                                        // rather run it themselves — or send it to whoever
                                        // administers the machine — should not have to
                                        // retype it.
                                        .child(
                                            div()
                                                .id(SharedString::from(format!("copy-{}", check.id)))
                                                .px_3()
                                                .py_1()
                                                .border_1()
                                                .border_color(rgb(BORDER))
                                                .text_color(rgb(TEXT_MUTED))
                                                .text_sm()
                                                .hover(|style| style.cursor_pointer())
                                                .child("Copy ⧉")
                                                .on_click(cx.listener({
                                                    let command = command.clone();
                                                    move |workbench, _event, _window, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(
                                                                command.clone(),
                                                            ),
                                                        );
                                                        workbench.status =
                                                            "command copied".into();
                                                        cx.notify();
                                                    }
                                                })),
                                        ),
                                );
                        }
                        preflight::Fix::Adopt { label, dir } => {
                            row = row.child(
                                div()
                                    .id(SharedString::from(format!("adopt-{}", check.id)))
                                    .px_3()
                                    .py_1()
                                    .border_1()
                                    .border_color(rgb(ACCENT))
                                    .text_color(rgb(ACCENT))
                                    .text_sm()
                                    .hover(|style| style.cursor_pointer())
                                    .child(*label)
                                    .on_click(cx.listener({
                                        let dir = dir.clone();
                                        move |workbench, _event, _window, cx| {
                                            workbench.adopt_checkout(dir.clone(), cx);
                                        }
                                    })),
                            );
                        }
                        preflight::Fix::Manual(instruction) => {
                            row = row.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_color(rgb(TEXT_MUTED))
                                    .text_xs()
                                    .child(instruction.clone()),
                            );
                        }
                    }
                    }
                    pane = pane.child(row);
                }
            }
        }

        // What the running fix is printing. Shown *below* the checks so the list stays in
        // one place, and only while there is something to show.
        if let Some(fix) = &self.running_fix {
            // The actions sit **outside** the scrolling log. They were inside it, and the
            // box — a flex child, so shrinkable — squeezed until "Open the sign-in page"
            // was cut in half and unreadable. A button you cannot read is worse than no
            // button: the user knows something is there and cannot use it.
            let mut log = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .flex_none()
                .gap_2()
                .p_2()
                .border_1()
                .border_color(rgb(if !fix.done {
                    ACCENT
                } else if fix.ok {
                    BORDER
                } else {
                    ERROR
                }))
                .child(
                    div()
                        .text_color(rgb(TEXT))
                        .text_sm()
                        .child(if fix.done {
                            format!("{} — {}", fix.label, if fix.ok { "done" } else { "failed" })
                        } else {
                            format!("{}…", fix.label)
                        }),
                );
            // A sign-in page to open. Prominent, and above the log, because while this is
            // showing the command is *blocked* waiting for the user to visit it — and the
            // CLI's own attempt to open it failed inside the distro.
            if let Some(link) = &fix.link {
                // The code, big and on its own line. It is what the sign-in page asks for,
                // and inside the full URL it is the first thing to be clipped.
                if let Some(code) = device_code(link) {
                    log = log.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_color(rgb(ACCENT))
                            .text_lg()
                            .child(code),
                    );
                }
                log = log.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .id("open-signin")
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(ACCENT))
                                .text_color(rgb(ACCENT))
                                .text_sm()
                                .hover(|style| style.cursor_pointer())
                                .child("Open the sign-in page")
                                .on_click(cx.listener({
                                    let link = link.clone();
                                    move |workbench, _event, _window, cx| {
                                        workbench.status = match open_in_browser(&link) {
                                            Ok(()) => "opened the sign-in page in your browser"
                                                .to_string(),
                                            Err(error) => {
                                                format!("could not open a browser: {error}")
                                            }
                                        };
                                        cx.notify();
                                    }
                                })),
                        )
                        // A copy, for a machine where opening a browser from here fails —
                        // the code in that URL is short-lived, so retyping it is not an
                        // option.
                        .child(
                            div()
                                .id("copy-signin")
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(BORDER))
                                .text_color(rgb(TEXT_MUTED))
                                .text_sm()
                                .hover(|style| style.cursor_pointer())
                                .child("Copy ⧉")
                                .on_click(cx.listener({
                                    let link = link.clone();
                                    move |workbench, _event, _window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            link.clone(),
                                        ));
                                        workbench.status = "sign-in link copied".into();
                                        cx.notify();
                                    }
                                })),
                        ),
                );
            }
            let mut output = div()
                .id("fix-output")
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .flex_none()
                .max_h(px(200.))
                .overflow_y_scroll();
            for line in &fix.lines {
                output = output.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(TEXT_MUTED))
                        .text_xs()
                        .child(line.clone()),
                );
            }
            if fix.lines.is_empty() {
                output = output.child(
                    div()
                        .text_color(rgb(TEXT_MUTED))
                        .text_xs()
                        .child("starting…"),
                );
            }
            pane = pane.child(log.child(output));
        }

        pane.child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(
                    div()
                        .id("recheck")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .text_color(rgb(ACCENT))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child(if self.checking { "Checking…" } else { "Re-check" })
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.run_preflight(cx)
                        })),
                )
                .child(
                    div()
                        .id("setup-to-settings")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Settings")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.setup_open = false;
                            workbench.open_settings(None, cx);
                        })),
                )
                .child(
                    div()
                        .id("close-setup")
                        .px_3()
                        .py_1()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .hover(|style| style.cursor_pointer())
                        .child("Close")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.setup_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(TEXT_MUTED))
                .text_xs()
                .child(format!("Sidecar log: {}", self.sidecar.log_path())),
        )
    }

    fn run_command(&mut self, command: Command, cx: &mut Context<Self>) {
        match command {
            // Same path as Enter in the composer and as the Send button.
            Command::RunTurn => self
                .composer
                .update(cx, |composer, cx| composer.submit_now(cx)),
            Command::NewThread => {
                if self.streaming {
                    self.status = "can't start a new thread mid-turn".into();
                    return;
                }
                self.sidecar.reset_thread();
                self.transcript.clear();
                self.buckets.clear();
                self.error = None;
                // Blanket approval is scoped to the conversation, so it ends with it —
                // together with every per-task grant, whose tasks belonged to that
                // conversation too. This is the line that makes the button's wording true.
                self.approve_conversation = false;
                self.approve_tasks.clear();
                // The conversation just left should appear in the list.
                self.refresh_conversations(cx);
                // The spine is thread-independent — the mission survives, so say so
                // rather than letting the panel look stale.
                self.status = "new thread — the project spine is kept".into();
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
                        self.status = "last answer copied".into();
                    }
                    None => self.status = "no answer to copy yet".into(),
                }
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
    }

    /// The palette overlay: a query field over a filtered command list.
    ///
    /// Rendered as the root's last child so it paints above the panes; it is
    /// `absolute`, so it takes no part in the three-pane flex layout.
    fn palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let commands = self.palette_commands(cx);
        let selected = self.palette_selected.min(commands.len().saturating_sub(1));

        let mut list = div().flex().flex_col().w_full().min_w_0();
        if commands.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(TEXT_MUTED))
                    .text_sm()
                    .child("No matching command."),
            );
        }
        for (index, command) in commands.iter().enumerate() {
            let is_selected = index == selected;
            let command = *command;
            list = list.child(
                div()
                    .id(SharedString::from(format!("command-{index}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .when(is_selected, |row| row.bg(rgb(BORDER)))
                    .hover(|style| style.bg(rgb(BORDER)).cursor_pointer())
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(if is_selected { TEXT } else { TEXT_MUTED }))
                            .child(command.label()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .child(command.hint()),
                    )
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.close_palette(window, cx);
                        workbench.run_command(command, cx);
                        cx.notify();
                    })),
            );
        }

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            // The context the palette's own keys are bound to. It wraps the query
            // field, so those bindings win while the palette has focus.
            .key_context("Palette")
            .on_action(cx.listener(|workbench, _: &PaletteNext, _window, cx| {
                workbench.move_palette_selection(1, cx)
            }))
            .on_action(cx.listener(|workbench, _: &PalettePrev, _window, cx| {
                workbench.move_palette_selection(-1, cx)
            }))
            .on_action(cx.listener(|workbench, _: &PaletteDismiss, window, cx| {
                workbench.close_palette(window, cx)
            }))
            .child(
                div()
                    .mt(px(96.))
                    .w(px(520.))
                    .flex()
                    .flex_col()
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .child(self.palette_query.clone()),
                    )
                    .child(list)
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .child("↑↓ select · ⏎ run · esc close"),
                    ),
            )
    }

    /// The agent activity trace for one turn: coordinator steps as one-liners, then
    /// a collapsible group per subagent.
    ///
    /// This exists because a delegated turn is otherwise *silent*: the coordinator
    /// emits only a `task` tool call while a subagent does the real work, so the user
    /// sees a frozen window and then an answer with no account of where it came from
    /// (plan §15).
    fn activity_block(
        &self,
        message_index: usize,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut block = div().flex().flex_col().w_full().min_w_0().gap_1();

        // The coordinator's own steps, behind the same disclosure the subagent groups have
        // had all along. Flat and unbounded, they ran to twenty lines of `read_file`, `ls`
        // and `glob` and pushed the actual answer off the screen (docs §47).
        let folded = fold_steps(&message.steps);
        if !folded.is_empty() {
            let count = message.steps.len();
            block = block.child(
                div()
                    .id(SharedString::from(format!("steps-{message_index}")))
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(TEXT_MUTED))
                    .text_xs()
                    .hover(|style| style.cursor_pointer())
                    .child(format!(
                        "{} {count} {}",
                        if message.steps_expanded { "▾" } else { "▸" },
                        if count == 1 { "step" } else { "steps" },
                    ))
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if let Some(message) = workbench.transcript.get_mut(message_index) {
                            message.steps_expanded = !message.steps_expanded;
                        }
                        cx.notify();
                    })),
            );
            if message.steps_expanded {
                for step in &folded {
                    block = block.child(step_line(step));
                }
            }
        }

        for (trace_index, trace) in message.agents.iter().enumerate() {
            let steps = if trace.steps.len() == 1 {
                "1 step".to_string()
            } else {
                format!("{} steps", trace.steps.len())
            };
            let header = format!(
                "{} {} · {steps} · {} chars",
                if trace.expanded { "▾" } else { "▸" },
                trace.name,
                trace.text.chars().count(),
            );

            let mut group = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .pl_2()
                .border_l_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        // Unique per (turn, trace) so GPUI keeps each group's click
                        // state to itself.
                        .id(SharedString::from(format!(
                            "trace-{message_index}-{trace_index}"
                        )))
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(ACCENT))
                        .text_xs()
                        .hover(|style| style.cursor_pointer())
                        .child(header)
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            if let Some(message) = workbench.transcript.get_mut(message_index) {
                                if let Some(trace) = message.agents.get_mut(trace_index) {
                                    trace.expanded = !trace.expanded;
                                }
                            }
                            cx.notify();
                        })),
                );

            if trace.expanded {
                for step in &fold_steps(&trace.steps) {
                    group = group.child(step_line(step));
                }
                // Not the raw stream: a subagent's answer often arrives as one JSON
                // object, which is unreadable as a trace line.
                let preview = protocol::summarize_agent_result(&trace.text);
                if !preview.is_empty() {
                    group = group.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .child(preview),
                    );
                }
            }
            block = block.child(group);
        }

        block
    }

    /// The input row: the text field plus a Send affordance.
    fn composer_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (send_label, send_color) = if self.streaming {
            ("Streaming…", TEXT_MUTED)
        } else {
            ("Send ⏎", ACCENT)
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .p_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(self.composer.clone())
            .child(
                div()
                    .id("send-turn")
                    .flex_none()
                    .px_3()
                    .py_1()
                    .border_1()
                    .border_color(rgb(send_color))
                    .text_color(rgb(send_color))
                    .text_sm()
                    .child(send_label)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        // Same path as Enter. Calling the entity directly rather
                        // than dispatching an action keeps this working regardless
                        // of where focus is when the button is clicked.
                        workbench
                            .composer
                            .update(cx, |composer, cx| composer.submit_now(cx));
                    })),
            )
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), ERROR),
            None => (self.status.clone(), TEXT_MUTED),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .child(
                div()
                    .flex_grow()
                    .min_w_0()
                    .text_color(rgb(status_color))
                    .text_sm()
                    .child(status_text),
            )
            // A blanket grant that is in force must never be invisible — and must be
            // revocable without starting a new conversation, or "just this once" becomes
            // permanent by inconvenience. Click to hand the gate back.
            .when(self.approve_conversation, |bar| {
                bar.child(
                    div()
                        .id("revoke-approval")
                        .flex_none()
                        .px_2()
                        .border_1()
                        .border_color(rgb(ACCENT))
                        .text_color(rgb(ACCENT))
                        .text_xs()
                        .hover(|style| style.cursor_pointer())
                        .child("approving everything — click to stop")
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_conversation = false;
                            workbench.approve_tasks.clear();
                            workbench.status = "asking before each command again".into();
                            cx.notify();
                        })),
                )
            })
            // Say where the agent's code runs. When that is the user's own machine
            // it should be visible without opening a log (docs §18).
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(if self.sidecar.execution() == "host (local)" {
                        ACCENT
                    } else {
                        TEXT_MUTED
                    }))
                    .text_xs()
                    .child(self.sidecar.execution()),
            )
            // Discoverability: a palette nobody knows the shortcut for is a palette
            // nobody opens.
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(TEXT_MUTED))
                    .text_xs()
                    .child("ctrl-p commands"),
            )
            .child(
                div()
                    .text_color(rgb(TEXT_MUTED))
                    .text_sm()
                    .child(self.sidecar.base_url().to_string()),
            )
    }

    /// The project spine: mission, what's done, what's queued, what's suggested.
    fn artifacts_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("spine")
            .flex()
            .flex_col()
            .w(px(320.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(SURFACE))
            .border_l_1()
            .border_color(rgb(BORDER))
            .p_4()
            .gap_4()
            .child(section_label("RESEARCH PROJECT"));

        let Some(project) = &self.project else {
            // No spine yet, but a run may already be producing outputs — still show
            // them rather than an empty panel.
            return panel
                .child(
                    div()
                        .text_color(rgb(TEXT_MUTED))
                        .text_sm()
                        .child("No project loaded yet. Run a turn — the mission is derived from your first question."),
                )
                .child(self.jobs_section(cx))
                .child(self.outputs_section());
        };

        panel = panel.child(if project.mission.is_empty() {
            div()
                .text_color(rgb(TEXT_MUTED))
                .text_sm()
                .child("No mission yet — it comes from your first question.")
        } else {
            div()
                .w_full()
                .text_color(rgb(TEXT))
                .child(project.mission.clone())
        });

        if !project.completed.is_empty() {
            panel = panel.child(spine_list("COMPLETED", &project.completed, "✓"));
        }
        if !project.pending.is_empty() {
            panel = panel.child(spine_list("PENDING", &project.pending, "○"));
        }

        // Advisory only: shown so the user can choose to ask for one. Nothing here
        // auto-runs — org policy is human-gated.
        if !project.suggestions.is_empty() {
            let mut suggestions = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("SUGGESTED NEXT"));
            for (index, suggestion) in project.suggestions.iter().enumerate() {
                let prompt = suggestion.prompt.clone();
                suggestions = suggestions.child(
                    div()
                        .id(("suggestion", index))
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .p_2()
                        .border_1()
                        .border_color(rgb(BORDER))
                        .hover(|style| style.border_color(rgb(ACCENT)).cursor_pointer())
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(TEXT))
                                .text_sm()
                                .child(suggestion.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(TEXT_MUTED))
                                .text_xs()
                                .child(suggestion.rationale.clone()),
                        )
                        // Clicking *loads* the prompt into the composer; it never
                        // runs it. Suggestions are advisory and org policy is
                        // human-gated, so the user still presses Enter.
                        .on_click(cx.listener(move |workbench, _event, window, cx| {
                            if workbench.streaming || prompt.is_empty() {
                                return;
                            }
                            workbench.composer.update(cx, |composer, cx| {
                                composer.set_text(prompt.clone(), cx);
                            });
                            // Drop it from the list: it is in the composer now, and
                            // leaving a duplicate to click is just confusing.
                            if let Some(project) = workbench.project.as_mut() {
                                project.suggestions.retain(|s| s.prompt != prompt);
                            }
                            let focus = workbench.composer.focus_handle(cx);
                            window.focus(&focus);
                            workbench.status = "suggestion loaded — press Enter to run it".into();
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(suggestions);
        }

        if project.completed.is_empty() && project.pending.is_empty() {
            panel = panel.child(
                div()
                    .text_color(rgb(TEXT_MUTED))
                    .text_xs()
                    .child("Completed and pending work will appear here as the project grows."),
            );
        }

        panel.child(self.jobs_section(cx)).child(self.outputs_section())
    }

    /// Long jobs still running, and the ones that finished this session.
    ///
    /// The theorizer and DataVoyager return a task id immediately and finish minutes
    /// later, so without this the answer to "is it still going?" was nothing at all —
    /// and, worse, nobody was collecting the result (docs §29).
    fn jobs_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = div().flex().flex_col().gap_2().pt_2();
        if self.jobs.is_empty() && self.tasks.is_empty() {
            return section;
        }
        section = section.child(section_label("BACKGROUND JOBS"));

        // Background workers first, because one of them may be *stopped waiting for you* —
        // and until this existed that task simply hung, since the gate it hit runs on its
        // own thread and nothing in the UI could answer it (docs §31).
        for task in &self.tasks {
            let (mark, colour) = if task.needs_approval() {
                ("⏸", ACCENT)
            } else if !task.is_finished() {
                ("◐", RUNNING)
            } else if task.succeeded() {
                ("✓", SUCCESS)
            } else {
                ("✗", ERROR)
            };
            let mut row = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .pl_2()
                .border_l_1()
                .border_color(rgb(colour))
                .child(
                    div()
                        .text_color(rgb(TEXT))
                        .text_sm()
                        .child(format!("{mark} {}", task.agent_name.replace('_', " "))),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        // The failure the server recorded, in place of the bare word
                        // "error" — which is all this said while two rounds went into
                        // guessing what had actually happened (docs §38).
                        .text_color(rgb(if task.error.is_some() { ERROR } else { TEXT_MUTED }))
                        .text_xs()
                        .child(match (&task.error, task.needs_approval()) {
                            (Some(error), _) => error.clone(),
                            (None, true) => "waiting for your approval".to_string(),
                            // What it is *doing*, not just that it is doing something —
                            // "running" for ten minutes tells a researcher nothing about
                            // whether to wait (docs §42).
                            (None, false) => match (&task.activity, task.is_finished()) {
                                (Some(activity), false) => format!("{} · {activity}", task.status),
                                _ => task.status.clone(),
                            },
                        }),
                )
                .when(!task.description.is_empty(), |row| {
                    row.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .child(task.description.clone()),
                    )
                });

            if let Some(request) = &task.pending {
                let task_id = task.task_id.clone();
                // Capped and scrollable, for the same reason the foreground card is: a
                // background worker writes long scripts, and a command tall enough to push
                // Approve out of the panel is a gate the researcher cannot answer.
                let mut commands = div()
                    .id(SharedString::from(format!("bg-commands-{task_id}")))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w_full()
                    .min_w_0()
                    .max_h(px(200.))
                    .overflow_y_scroll();
                for action in &request.actions {
                    // The command verbatim, exactly as the foreground card shows it: this
                    // runs on the researcher's own machine, and the only meaningful review
                    // is of the actual text (docs §19).
                    commands = commands.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_none()
                            .p_2()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_color(rgb(TEXT))
                            .text_xs()
                            .child(action.detail.clone()),
                    );
                }
                row = row.child(commands);
                row = row.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("bg-approve-{task_id}")))
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(ACCENT))
                                .text_color(rgb(ACCENT))
                                .text_sm()
                                .hover(|style| style.cursor_pointer())
                                .child("Approve")
                                .on_click(cx.listener({
                                    let task_id = task_id.clone();
                                    move |workbench, _event, _window, cx| {
                                        workbench.decide_task(task_id.clone(), true, cx);
                                    }
                                })),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("bg-reject-{task_id}")))
                                .px_3()
                                .py_1()
                                .border_1()
                                .border_color(rgb(BORDER))
                                .text_color(rgb(TEXT_MUTED))
                                .text_sm()
                                .hover(|style| style.cursor_pointer())
                                .child("Reject")
                                .on_click(cx.listener({
                                    let task_id = task_id.clone();
                                    move |workbench, _event, _window, cx| {
                                        workbench.decide_task(task_id.clone(), false, cx);
                                    }
                                })),
                        ),
                );
                // A background worker asks once per command over several minutes. Without
                // this the researcher has to sit on the panel and answer each one, which
                // defeats the entire point of handing the work to the background.
                //
                // Both blanket grants appear here, and the conversation-wide one is worded
                // *identically* to the chat's. They are two gates — the coordinator asks
                // below the composer, a worker asks in this panel — and which one appears
                // depends on who happened to need permission. A grant offered in one place
                // and not the other reads as the button moving around at random (docs §44).
                for (suffix, label, conversation_wide) in [
                    ("task", "Approve the rest of this task", false),
                    ("conv", "Approve everything in this conversation", true),
                ] {
                    row = row.child(
                        div()
                            .id(SharedString::from(format!("bg-approve-{suffix}-{task_id}")))
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_color(rgb(TEXT_MUTED))
                            .text_xs()
                            .hover(|style| style.cursor_pointer())
                            .child(label)
                            .on_click(cx.listener({
                                let task_id = task_id.clone();
                                move |workbench, _event, _window, cx| {
                                    if conversation_wide {
                                        workbench.approve_conversation = true;
                                    } else {
                                        workbench.approve_tasks.insert(task_id.clone());
                                    }
                                    workbench.decide_task(task_id.clone(), true, cx);
                                }
                            })),
                    );
                }
            }
            section = section.child(row);
        }
        for job in &self.jobs {
            let (mark, colour) = if !job.is_finished() {
                ("◐", RUNNING)
            } else if job.succeeded() {
                ("✓", SUCCESS)
            } else {
                ("✗", ERROR)
            };
            let detail = if job.is_finished() {
                job.status.clone()
            } else {
                // Say how long it usually takes. A spinner with no expectation attached
                // is indistinguishable from a hang.
                format!("running · usually {}", job.kind.expected())
            };
            section = section.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_1()
                    .pl_2()
                    .border_l_1()
                    .border_color(rgb(colour))
                    .child(
                        div()
                            .text_color(rgb(TEXT))
                            .text_sm()
                            .child(format!("{mark} {}", job.kind.label())),
                    )
                    .child(div().text_color(rgb(TEXT_MUTED)).text_xs().child(detail))
                    .when(!job.question.is_empty(), |row| {
                        row.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_color(rgb(TEXT_MUTED))
                                .text_xs()
                                .child(job.question.clone()),
                        )
                    }),
            );
        }
        section
    }

    /// Research outputs from the current run, grouped by kind.
    ///
    /// Fed by the `values` stream event, so it fills in as a turn produces papers,
    /// datasets, theories and reports — not only at the end.
    fn outputs_section(&self) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(section_label("OUTPUTS"));

        // Everything this conversation wrote, in one folder the researcher already owns.
        // This *is* "download all the documents": the files are in their own Documents
        // directory (`workspace.rs`), so there is nothing to package — the ask was only
        // ever for a way to get at them.
        if let Some(dir) = self.thread_workspace() {
            section = section.child(
                div()
                    .id("open-workspace")
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(ACCENT))
                    .text_color(rgb(ACCENT))
                    .text_xs()
                    .hover(|style| style.cursor_pointer())
                    .child("Open this conversation's files")
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&dir) {
                            tracing::warn!(%error, "could not open the workspace folder");
                        }
                    }),
            );
        }

        // The files themselves, grouped by what a researcher would do with them. The
        // buckets below are what the *agent* declared it produced; this is what is
        // actually on disk, which is a superset and the thing they asked to see.
        let files = self
            .thread_workspace()
            .map(|dir| workspace::outputs(&dir))
            .unwrap_or_default();
        for (kind, items) in &files {
            section = section.child(
                div()
                    .pt_1()
                    .text_color(rgb(TEXT_FAINT))
                    .text_xs()
                    .child(kind.label()),
            );
            for output in items {
                let opened = output.path.clone();
                section = section.child(
                    div()
                        .id(SharedString::from(format!("file-{}", output.name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .px_1()
                        .hover(|style| style.bg(rgb(RAISED)).cursor_pointer())
                        .child(
                            div()
                                .flex_grow()
                                .min_w_0()
                                .truncate()
                                .text_color(rgb(TEXT))
                                .text_xs()
                                .child(output.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgb(TEXT_FAINT))
                                .text_xs()
                                .child(workspace::human_size(output.bytes)),
                        )
                        .on_click(move |_event, _window, _cx| {
                            // Opens in whatever the researcher normally uses for that
                            // kind of file — a CSV in Excel, a figure in the viewer.
                            if let Err(error) = workspace::open(&opened) {
                                tracing::warn!(%error, "could not open an output file");
                            }
                        }),
                );
            }
        }

        if self.buckets.is_empty() && files.is_empty() {
            return section.child(
                div()
                    .text_color(rgb(TEXT_MUTED))
                    .text_xs()
                    .child("Papers, datasets, theories and reports show up here as a turn produces them."),
            );
        }

        for bucket in &self.buckets {
            // Show a bounded number of titles — a literature search can return
            // dozens, and the count already conveys the scale.
            const MAX_SHOWN: usize = 4;
            let mut group = div().flex().flex_col().gap_1().child(
                div()
                    .text_color(rgb(TEXT))
                    .text_sm()
                    .child(format!("{} · {}", bucket.name, bucket.items.len())),
            );
            for item in bucket.items.iter().take(MAX_SHOWN) {
                group = group.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(TEXT_MUTED))
                        .text_xs()
                        .child(item.clone()),
                );
            }
            if bucket.items.len() > MAX_SHOWN {
                group = group.child(
                    div()
                        .text_color(rgb(TEXT_MUTED))
                        .text_xs()
                        .child(format!("+{} more", bucket.items.len() - MAX_SHOWN)),
                );
            }
            section = section.child(group);
        }

        section
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.restore_focus {
            self.restore_focus = false;
            let composer = self.composer.focus_handle(cx);
            window.focus(&composer);
        }

        // `relative` so the palette's `absolute` overlay is positioned against the
        // window rather than the page origin.
        let root = div()
            // An id makes the root a drop target; without one the platform's file-drop
            // event has nowhere to land.
            .id("workbench")
            .relative()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::toggle_settings))
            // Anywhere on the window, not a designated strip: someone dragging a file has
            // their eyes on the file, not on a target.
            .on_drop(cx.listener(
                |workbench, paths: &gpui::ExternalPaths, _window, cx| {
                    workbench.files_dropped(paths.paths(), cx);
                },
            ))
            .child(self.rail(cx))
            .child(self.chat_pane(cx));

        // One right-hand pane at a time: Setup wins over Settings, because the only
        // reason it is open is that something is stopping a turn.
        let root = if self.setup_open {
            root.child(self.setup_pane(cx))
        } else if self.settings_open {
            root.child(self.settings_pane(cx))
        } else {
            root.child(self.artifacts_panel(cx))
        };

        if self.palette_open {
            root.child(self.palette(cx))
        } else {
            root
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
                TurnEvent::Token(text) => message.body.push_str(&text),
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
                        message.steps.push(format!("awaiting approval: {}", action.tool));
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let (message, outputs) = decode_capture(DELEGATED_TURN, |status| {
            statuses.push(status.to_string())
        });

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
            panic!("expected exactly one subagent group, got {}", message.agents.len());
        };
        assert_eq!(trace.name, "academic_researcher");
        assert!(trace.ns.starts_with("tools:"), "{}", trace.ns);
        assert_eq!(trace.steps, vec!["search_paper_by_title"]);

        // Its answer was a JSON object, so the trace shows the readable part.
        let preview = protocol::summarize_agent_result(&trace.text);
        assert!(preview.starts_with("The canonical DESeq2 paper"), "{preview}");
        assert!(preview.ends_with("· 1 sources"), "{preview}");

        // The coordinator's answer still arrives, and the outputs panel still fills:
        // subagent frames must not be mistaken for either.
        assert!(message.body.contains("Genome Biology"), "{}", message.body);
        assert_eq!(
            outputs.iter().map(|b| (b.name, b.items.len())).collect::<Vec<_>>(),
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
            hits.into_iter().map(|(_, _, label)| label).collect::<Vec<_>>()
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
        let real = "gio: https://auth0.allenai.org/activate?user_code=DPMW-BJCG: Operation not supported";
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
        // Nothing to open.
        assert_eq!(first_url("Waiting for authentication…"), None);
        assert_eq!(first_url("https://"), None, "a bare scheme is not a link");
        assert_eq!(first_url("http://example.org"), None, "https only");
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

    #[test]
    fn a_dropped_file_becomes_a_question_the_backend_can_act_on() {
        // The path has to be spelled the way the *agent* would open it. On Windows the
        // agent lives inside WSL, so a prompt naming `C:\…` would send it looking for a
        // file that does not exist there — and the researcher would have no idea why.
        let _env = backend::env_lock::hold();
        let mut config = backend::BackendConfig::default();
        config.wsl = Some(backend::WslTarget {
            distro: None,
            dir: "~/Mini-Me".into(),
        });
        let translated =
            config.path_for_backend(std::path::Path::new(r"C:\Users\LENOVO\Documents\yield.csv"));
        assert_eq!(translated, "/mnt/c/Users/LENOVO/Documents/yield.csv");

        let prompt = prompt_for_dropped(&[translated.clone()], &[false]);
        assert!(prompt.contains(&translated), "{prompt}");
        assert!(!prompt.contains('\\'), "no Windows path survives: {prompt}");

        // A directory is a different request from a file.
        let folder = prompt_for_dropped(&["/mnt/c/readings".into()], &[true]);
        assert!(folder.contains("files in"), "{folder}");

        // Several files are one question about all of them, not several questions.
        let many = prompt_for_dropped(
            &["/mnt/c/a.csv".into(), "/mnt/c/b.csv".into()],
            &[false, false],
        );
        assert!(many.contains("/mnt/c/a.csv") && many.contains("/mnt/c/b.csv"), "{many}");
        assert_eq!(many.matches("- ").count(), 2, "{many}");

        assert!(prompt_for_dropped(&[], &[]).is_empty());
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
}

/// Assemble the model routing the backend expects from settings plus the keychain.
fn model_choice(user_settings: &settings::Settings) -> Option<protocol::ModelChoice> {
    let provider = settings::provider(&user_settings.provider)?;
    Some(protocol::ModelChoice {
        spec: user_settings.model_spec(),
        provider: user_settings.provider.clone(),
        api_key: settings::secret(&user_settings.key_name()),
        base_url: if provider.needs_base_url && !user_settings.base_url.trim().is_empty() {
            Some(user_settings.base_url.trim().to_string())
        } else {
            None
        },
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

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
            prompts.push(SEED_PROMPT);
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

    Application::new().run(move |cx: &mut App| {
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
                |_window, cx| cx.new(|cx| Workbench::new(sidecar.clone(), cx)),
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
