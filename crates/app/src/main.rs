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
mod gallery;
mod markdown;
mod menu;
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
mod workspace;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use futures::StreamExt;
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, AnimationExt as _, App, Application,
    Bounds, ClipboardItem, Context, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

use composer::{Composer, ComposerEvent};
use protocol::{AgentRef, ApprovalRequest, Bucket, Decision, Project, TurnEvent};
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

/// [`section_label`] for a heading only known at runtime.
fn section_label_owned(text: String) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}

/// One row of a picker: a label, a tick when it is the current choice, and an optional note.
///
/// Shared so every picker in this window looks the same and states the same thing the same way —
/// the theme list, the model list and the per-specialist list had drifted into three shapes.
fn picker_row(
    label: impl Into<SharedString>,
    selected: bool,
    note: Option<String>,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    div()
        .id(SharedString::from(format!("row-{label}")))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .w_full()
        .min_w_0()
        .px_2()
        .py_1()
        .rounded_md()
        .when(selected, |row| row.bg(rgb(theme::accent_soft())))
        .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
        .child(
            ui::Label::new(label)
                .colour(if selected {
                    theme::text()
                } else {
                    theme::text_muted()
                })
                .ellipsis(),
        )
        .children(note.map(|note| {
            // Muted, not red: a missing key is a thing to do next, not a thing done wrong.
            ui::Label::new(note)
                .colour(theme::warning())
                .size(ui::Size::Compact)
        }))
}

/// A small caps-ish section heading for the side panel.
///
/// **Faint, not accent.** A heading is not something you can click, and the accent is the app's
/// one signal that you can — `OUTPUTS`, `SUGGESTED NEXT` and `THE SPECIALISTS` shouting in the
/// brand colour is most of why the window read as busy. Moving these to `text_faint` was the
/// single largest change in the redesign and it is one line.
fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}

/// This conversation's citations as BibTeX a reference manager will import.
///
/// # Why every entry is `@misc` with a `note`
///
/// A source arrives as **one line of the agent's prose** — `Smith, J. et al. (2021). Late blight
/// resistance in Andean potato. Plant Pathology 70(4).` BibTeX wants `author`, `title`, `year`,
/// `journal` as separate fields, and splitting that sentence into them means a parser that is
/// right about most citations and confidently wrong about the rest. A mis-split reference does
/// not look broken in a manuscript; it looks like a citation, with the wrong author on it.
///
/// So the whole string goes in `note`, which is what `note` is for, and the URL — the one part
/// that can be extracted without interpretation — goes in `url`. Every entry is importable, every
/// entry is verbatim, and nothing is attributed to anyone the agent did not name. A researcher
/// fills in the fields their journal wants, which they were going to check anyway (org policy:
/// *validate AI-generated content with subject matter experts*).
fn bibliography(sources: &[protocol::Source]) -> String {
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
        if let Some(written) = disputed_link(source) {
            out.push_str(&format!(
                "  annote = {{unverified: the citation text gives {written}}},\n"
            ));
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
fn scholar_link(
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
fn link_for(source: &protocol::Source) -> Option<String> {
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
fn disputed_link(source: &protocol::Source) -> Option<String> {
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
fn without_url(citation: &str) -> String {
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
fn mermaid(graph: &provenance::Graph) -> String {
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
fn provenance_svg(graph: &provenance::Graph) -> String {
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
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

/// The colour that stands for how much an edge can be trusted.
///
/// Shared by the arcs and the legend so the two cannot describe different pictures — which they
/// did while each carried its own `match`.
fn edge_ink(kind: provenance::Edge) -> u32 {
    match kind {
        provenance::Edge::Delegated => theme::text_muted(),
        provenance::Edge::Then => theme::text_faint(),
        // The one edge that gets the accent, because these returns are what §73 asked the
        // feature to make visible.
        provenance::Edge::Returned => theme::accent(),
    }
}

/// Stroke a quadratic as a dashed line.
///
/// **Hand-rolled because there is no dash setting.** `PathBuilder::stroke` takes a width and
/// nothing else, so a dashed curve has to be built out of short solid ones. The curve is walked at
/// a fixed parameter step and alternate runs are emitted, which gives even-looking dashes on the
/// gentle arcs this draws.
///
/// Why bother: solid versus dashed is the *only* thing separating "one specialist delegated to the
/// other", which is true by construction, from "one was seen before the other", which is an
/// inference. Drawing both as solid lines and explaining the difference in a paragraph underneath
/// puts the hedge somewhere the reader has already stopped looking.
fn paint_dashed_curve(
    window: &mut Window,
    start: gpui::Point<gpui::Pixels>,
    control: gpui::Point<gpui::Pixels>,
    finish: gpui::Point<gpui::Pixels>,
    weight: f32,
    colour: u32,
) {
    /// Samples along the curve. Enough that a dash is a dash rather than a chord across a bend.
    const STEPS: usize = 48;
    /// Samples per dash, and per gap — the 4/4 pattern the design asks for, in curve parameter
    /// rather than in pixels.
    const DASH: usize = 3;

    let at = |t: f32| -> gpui::Point<gpui::Pixels> {
        let inverse = 1. - t;
        gpui::point(
            px(inverse * inverse * f32::from(start.x)
                + 2. * inverse * t * f32::from(control.x)
                + t * t * f32::from(finish.x)),
            px(inverse * inverse * f32::from(start.y)
                + 2. * inverse * t * f32::from(control.y)
                + t * t * f32::from(finish.y)),
        )
    };

    let mut step = 0;
    while step < STEPS {
        let last = (step + DASH).min(STEPS);
        let mut dash = gpui::PathBuilder::stroke(px(weight));
        dash.move_to(at(step as f32 / STEPS as f32));
        for point in step + 1..=last {
            dash.line_to(at(point as f32 / STEPS as f32));
        }
        if let Ok(path) = dash.build() {
            window.paint_path(path, gpui::rgb(colour));
        }
        step = last + DASH;
    }
}

/// What each line in the graph means, drawn the way it is drawn.
///
/// A sample of the actual stroke rather than a coloured word: the reader is being asked to tell
/// solid from dashed at 1.5px, and a legend that only names the colours does not help with that.
fn graph_legend() -> impl IntoElement {
    let mut rows = div().flex().flex_row().flex_wrap().gap_4().w_full().min_w_0();
    for (kind, meaning) in [
        (provenance::Edge::Delegated, "delegated to — causal"),
        (provenance::Edge::Then, "then, within a turn — order only"),
        (provenance::Edge::Returned, "came back to, in a later turn"),
    ] {
        let colour = edge_ink(kind);
        let dashed = kind != provenance::Edge::Delegated;
        let weight = if kind == provenance::Edge::Returned {
            2.
        } else {
            1.5
        };
        rows = rows.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .flex_none()
                .child(
                    div()
                        .flex_none()
                        .w(px(26.))
                        .h(px(weight))
                        .bg(rgb(colour))
                        // A dashed sample, without a canvas: the gaps are drawn as a row of
                        // background-coloured blocks over the line.
                        .when(dashed, |sample| {
                            sample
                                .bg(rgb(theme::overlay()))
                                .border_t(px(weight))
                                .border_dashed()
                                .border_color(rgb(colour))
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .child(meaning),
                ),
        );
    }
    rows
}

/// What kind of work a specialist does, as a colour.
///
/// **Two colours, and `None` for anything else.** The design is explicit that a colour per
/// specialist is a legend nobody memorises; the distinction worth carrying is between work that
/// goes and reads, and work that touches the data. Matched on the name because that is all the
/// chip has — a heuristic, and one whose worst outcome is a chip in the ordinary text colour
/// rather than a wrong claim. A specialist this does not recognise is left uncoloured on purpose:
/// guessing which of two kinds a new one is would be the mistake.
fn specialist_ink(name: &str) -> Option<u32> {
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
fn consulted(agents: &[AgentTrace]) -> Vec<String> {
    let mut path: Vec<String> = Vec::new();
    for agent in agents {
        if path.last().map(String::as_str) != Some(agent.name.as_str()) {
            path.push(agent.name.clone());
        }
    }
    path
}

/// The glyph and colour that stand for a file's kind.
///
/// Finer than [`workspace::Kind`], which groups by what a researcher *does* with a file and is
/// the right grouping for the panel's sections. Here a PDF and a Markdown note want telling
/// apart at a glance even though both are things you read.
///
/// Four colours, from the palette's status roles rather than a new set — the same argument as
/// the provenance chips: a colour per file type is a legend nobody memorises.
fn file_mark(path: &std::path::Path) -> (&'static str, u32) {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" | "tsv" | "xlsx" | "parquet" => ("▤", theme::success()),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => ("▩", theme::running()),
        "pdf" => ("▦", theme::error()),
        _ => ("▤", theme::warning()),
    }
}

/// One line of the activity trace: a tool call, or a delegation.
fn step_line(label: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(theme::text_muted()))
        .text_xs()
        .child(format!("· {label}"))
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

fn scrollbar(handle: &gpui::ScrollHandle) -> Option<impl IntoElement> {
    let overflow = handle.max_offset().height;
    let viewport = handle.bounds().size.height;
    if overflow <= px(0.) || viewport <= px(0.) {
        return None;
    }
    let content = viewport + overflow;
    // Floored, so a very long transcript still leaves something big enough to see.
    let thumb = (viewport * (viewport / content)).max(px(28.));
    let travel = viewport - thumb;
    let progress = (-handle.offset().y / overflow).clamp(0.0, 1.0);

    Some(
        div()
            .absolute()
            .top(travel * progress)
            .right(px(2.))
            .w(px(6.))
            .h(thumb)
            .rounded_full()
            .bg(rgb(theme::border_strong())),
    )
}

/// A one-line tooltip.
///
/// GPUI wants a whole view for a tooltip, so this is the smallest one that renders text —
/// and having it means a control can be an icon without becoming a guess.
struct Hint {
    text: SharedString,
}

impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::overlay()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            .text_color(rgb(theme::text()))
            .text_xs()
            .child(self.text.clone())
    }
}

/// Whether a file is column-separated, and so worth colouring by column.
fn is_delimited(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".csv") || name.ends_with(".tsv")
}

/// The colour for one CSV column.
///
/// Cycles the theme's own roles rather than inventing a rainbow: colours already checked
/// against every surface for contrast, so a wide table stays readable in every palette —
/// including the light one, where a fixed rainbow would wash out.
fn column_colour(column: usize) -> u32 {
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
    let mut list = div().flex().flex_col().gap_1().child(section_label(label));
    for item in items {
        list = list.child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
                        .text_sm()
                        .child(bullet),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .text_color(rgb(theme::text()))
                        .text_sm()
                        .child(item.clone()),
                ),
        );
    }
    list
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

/// Render one Markdown block as an element.
///
/// Emphasis becomes a `HighlightStyle` run rather than a nested element, which is how GPUI
/// wants inline styling: one shaped line per block, with ranges carrying the differences.
/// The gutter glyph for a list item at a given depth.
///
/// Only bullets change. A numbered item keeps the number the author wrote — renumbering it, or
/// swapping it for a bullet because it happens to be nested, would change what the answer says.
fn nested_marker(marker: &str, depth: usize) -> String {
    if marker.ends_with('.') {
        return marker.to_string();
    }
    match depth {
        0 => "·".to_string(),
        1 => "‣".to_string(),
        _ => "–".to_string(),
    }
}

/// Render one Markdown block.
///
/// `selectable` is the transcript's span registry when this block is part of a conversation,
/// and `None` when it is not — the file preview renders the same blocks, and a drag there
/// must not run through the transcript's spans as if the two were one document.
fn markdown_block(
    block: &markdown::Block,
    selectable: Option<&selection::Transcript>,
) -> gpui::AnyElement {
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
                        color: Some(rgb(theme::accent()).into()),
                        ..Default::default()
                    },
                    Emphasis::Link => HighlightStyle {
                        color: Some(rgb(theme::accent()).into()),
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    Emphasis::Url => HighlightStyle {
                        color: Some(rgb(theme::text_muted()).into()),
                        ..Default::default()
                    },
                };
                (range.clone(), style)
            })
            .collect();
        let text = StyledText::new(inlines.text.clone()).with_highlights(highlights);
        let element = div().w_full().min_w_0().text_color(rgb(base));
        match selectable {
            Some(transcript) => element.child(selection::Selectable::new(
                transcript,
                inlines.text.clone(),
                text,
            )),
            None => element.child(text),
        }
    };

    match block {
        Block::Heading { level, inlines } => {
            let element = styled(inlines, theme::text());
            // Only two sizes: an answer is not a document, and six heading levels of
            // typography would be noise.
            if *level <= 2 {
                element.text_lg().into_any_element()
            } else {
                element.into_any_element()
            }
        }
        Block::Paragraph(inlines) => styled(inlines, theme::text()).into_any_element(),
        Block::ListItem {
            marker,
            inlines,
            depth,
        } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .gap_2()
            // Indent per level. Capped at four because past that the text column is
            // narrower than the gutter, and a plan nested five deep is a plan nobody reads.
            .pl(px(16. * (*depth).min(4) as f32))
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    // A different glyph per level, so nesting survives a screenshot and a
                    // reader who cannot see the indentation of a wrapped line. A numbered
                    // item keeps its own number at any depth.
                    .child(nested_marker(marker, *depth)),
            )
            .child(styled(inlines, theme::text()))
            .into_any_element(),
        Block::Quote { depth, inlines } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .pl(px(12. * (*depth).min(3) as f32))
            // A rule down the left, which is what a quote looks like everywhere. The text is
            // muted, because a quote is something the answer is *referring* to.
            .border_l_2()
            .border_color(rgb(theme::border_strong()))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .pl_3()
                    .child(styled(inlines, theme::text_muted())),
            )
            .into_any_element(),
        Block::Image { alt, url } => div()
            .flex()
            .flex_row()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(div().flex_none().child("🖼"))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    // Named, not fetched. See [`markdown::Block::Image`]: the path lives in
                    // the distro and figures the agent really produced are already shown
                    // below, found on the host (§42). Saying which file it meant is the
                    // useful part; pretending to display it would not be.
                    .child(if alt.trim().is_empty() {
                        url.clone()
                    } else {
                        format!("{alt} — {url}")
                    }),
            )
            .into_any_element(),
        Block::Code { text, .. } => {
            let block = div()
                .w_full()
                .min_w_0()
                .p_2()
                .bg(rgb(theme::surface()))
                .border_1()
                .border_color(rgb(theme::border()))
                .text_color(rgb(theme::text()))
                .text_sm()
                // Nothing bundled — a stack ending at a face Windows always has. See
                // `ui::code_font`.
                .font(ui::code_font());
            // Selectable like any other run, and arguably the one that matters most: a
            // snippet is written to be copied.
            match selectable {
                Some(transcript) => block
                    .child(selection::Selectable::new(
                        transcript,
                        text.clone(),
                        StyledText::new(text.clone()),
                    ))
                    .into_any_element(),
                None => block.child(text.clone()).into_any_element(),
            }
        }
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
                    .child(styled(
                        inlines,
                        if bold {
                            theme::text()
                        } else {
                            theme::text_muted()
                        },
                    ))
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
                .border_color(rgb(theme::border()));
            if !header.is_empty() {
                let mut head = div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .bg(rgb(theme::surface()))
                    .border_b_1()
                    .border_color(rgb(theme::border()));
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
                    line = line.border_b_1().border_color(rgb(theme::border()));
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
            .border_color(rgb(theme::border()))
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
    /// Filters the *installed* theme list. With a hundred palettes installed, a list you
    /// can only scroll is a list you cannot use.
    theme_filter: Entity<Composer>,
    /// Filter for the project picker, which doubles as the field a new project is named in.
    project_query: Entity<Composer>,
    theme_scroll: gpui::ScrollHandle,
    model_scroll: gpui::ScrollHandle,
    /// What the gallery search box holds, and what it found.
    gallery_query: Entity<Composer>,
    gallery_results: Vec<gallery::Listing>,
    gallery_note: String,
    /// Scroll positions we draw scrollbars from. GPUI keeps the offset itself; these let
    /// us *read* it, which is what a visible bar needs.
    transcript_scroll: gpui::ScrollHandle,
    /// Selected transcript text, and the span registry a drag hit-tests against.
    /// See [`selection`] — the registry is rebuilt every frame, the selection is not.
    text_selection: selection::Transcript,
    /// An open right-click menu, if any.
    context_menu: Option<menu::ContextMenu>,
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
    /// A file being previewed in the centre, if any.
    preview: Option<workspace::Output>,
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
    /// The conversation or project whose delete control opened the centred warning.
    ///
    /// This used to be an inline yes/no row. It could not say that saved files now go too, and a
    /// project delete needs a count and a path; destructive scope belongs where it can be read
    /// before acting (§155).
    confirming_delete: Option<DeleteTarget>,
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
        // Opens empty. The placeholder says what to do, which is all a first launch needs.
        let composer = cx.new(|cx| {
            Composer::new(
                cx,
                "Ask Mini-Me…  (Enter to send, Shift-Enter for a new line)",
            )
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

        let mut workbench = Self {
            project: None,
            buckets: Vec::new(),
            jobs: Vec::new(),
            tasks: Vec::new(),
            transcript: Vec::new(),
            saved_reports: std::collections::HashSet::new(),
            reports: Vec::new(),
            sources: Vec::new(),
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
            project_query,
            theme_scroll: gpui::ScrollHandle::new(),
            model_scroll: gpui::ScrollHandle::new(),
            gallery_query,
            gallery_results: Vec::new(),
            gallery_note: String::new(),
            transcript_scroll: gpui::ScrollHandle::new(),
            text_selection: selection::Transcript::default(),
            context_menu: None,
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
            confirming_delete: None,
            deleting: None,
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
            job.kind.expected()
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

    /// The bordered box a filter composer sits in.
    ///
    /// One helper because the theme popup and the gallery both want it, and because it is the
    /// only place a *focus ring* has anywhere to attach: the composer is a child entity, so
    /// the wrapper has to track its handle and light up with `in_focus`.
    fn filter_field(&self, field: Entity<Composer>, cx: &App) -> impl IntoElement {
        div()
            .track_focus(&field.focus_handle(cx))
            // Stated, not inherited. The gallery's box was built by hand and looked identical
            // in the source, and it came out a quarter the width with its placeholder spilling
            // out the side — it was relying on flex stretch, and the two boxes did not agree
            // about whether they got it (docs §72).
            //
            // A **flex row**, because `w_full` alone was not enough and §72 came back: a `div` is
            // `Display::Block` by default in gpui, so the field inside had no row to fill and its
            // own `width: 100%` had nothing definite to resolve against (docs §88).
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .in_focus(|style| style.border_color(rgb(theme::accent())))
            .child(field)
    }

    /// The draggable edge between two panes.
    ///
    /// Four pixels wide with a resize cursor, and it does not move anything itself: it records
    /// *which* edge is being dragged, and the root's mouse-move does the arithmetic. Tracking
    /// the drag on the root rather than on this strip is what keeps it working when the pointer
    /// outruns four pixels, which it does immediately.
    fn divider(&self, edge: Divider, cx: &mut Context<Self>) -> impl IntoElement {
        let id = match edge {
            Divider::Sidebar => "divider-sidebar",
            Divider::Panel => "divider-panel",
        };
        div()
            .id(id)
            .flex_none()
            .w(px(4.))
            .h_full()
            .when(self.dragging == Some(edge), |bar| {
                bar.bg(rgb(theme::accent()))
            })
            .hover(|style| {
                style
                    .bg(rgb(theme::border_strong()))
                    .cursor(gpui::CursorStyle::ResizeLeftRight)
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(
                    move |workbench, _event: &gpui::MouseDownEvent, _window, cx| {
                        workbench.dragging = Some(edge);
                        cx.notify();
                    },
                ),
            )
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

    /// The stack of recent outcomes, above the status bar.
    fn toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack = div()
            .absolute()
            .right_4()
            .bottom_10()
            .flex()
            .flex_col()
            .gap_2()
            .items_end();
        for (index, toast) in self.toasts.iter().enumerate() {
            stack = stack.child(
                div()
                    .id(SharedString::from(format!("toast-{index}")))
                    .max_w(px(360.))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(theme::elevated()))
                    .border_1()
                    .border_color(rgb(theme::border_strong()))
                    .text_color(rgb(theme::text()))
                    .text_sm()
                    .hover(|style| style.cursor_pointer())
                    // Clicking one dismisses it: four seconds is right for a glance and wrong
                    // for a message you have already read.
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if index < workbench.toasts.len() {
                            workbench.toasts.remove(index);
                        }
                        cx.notify();
                    }))
                    .child(toast.clone()),
            );
        }
        stack
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

    /// The floating list a [`Picker`] shows: its filter field, then its rows.
    fn picker_popup(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let (picker, at) = self.open_picker?;
        let panel = match picker {
            // `theme_list` brings its own filter field; adding a second one here put two
            // identical boxes in the popup.
            Picker::Theme => self.theme_list(cx).into_any_element(),
            Picker::Model => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(self.model_list(cx))
                .into_any_element(),
            Picker::Subagent(index) => self.subagent_model_list(index, cx).into_any_element(),
            Picker::Project => self.project_list(cx).into_any_element(),
        };
        Some(
            ui::picker_popup(
                at,
                // **A flex column with a stated width, not a bare div.** Measured on a real
                // window: the two fields inside this popup came out at 0.0px and 38.4px while
                // the sidebar's and the composer's were 204 and 533. A `div()` is
                // `Display::Block` with `width: auto` in gpui, so it did not carry the popup's
                // declared 320px down, and every `w_full` beneath it was a percentage of
                // nothing (docs §99).
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(panel)
                    .on_mouse_down_out(cx.listener(
                        |workbench, _event: &gpui::MouseDownEvent, _window, cx| {
                            workbench.open_picker = None;
                            cx.notify();
                        },
                    )),
            )
            .into_any_element(),
        )
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

    fn context_menu(&self, open: menu::ContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let target = open.target;
        let mut panel = div()
            .flex()
            .flex_col()
            .min_w(px(190.))
            .py_1()
            .rounded_md()
            .bg(rgb(theme::elevated()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            // Swallow the press so the click that chooses an item does not also land on the
            // transcript underneath and start a fresh selection there.
            .on_mouse_down(gpui::MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            });

        for &item in open.items() {
            let enabled = self.menu_item_enabled(item, target, cx);
            let shortcut = item.shortcut(target);
            panel = panel.child(
                div()
                    .id(SharedString::from(format!("menu-{}", item.label())))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(if enabled {
                        theme::text()
                    } else {
                        theme::text_faint()
                    }))
                    .when(enabled, |row| {
                        row.hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer())
                            .on_click(cx.listener(move |workbench, _event, window, cx| {
                                workbench.run_menu_item(item, target, window, cx);
                            }))
                    })
                    .child(item.label())
                    .child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .text_xs()
                            .child(shortcut),
                    ),
            );
        }

        // Clicking anywhere else closes it, which is the only way out most people look for.
        gpui::deferred(gpui::anchored().position(open.at).snap_to_window().child(
            panel.on_mouse_down_out(cx.listener(
                |workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    // A right-click elsewhere re-opens the menu at the new spot, and
                    // that handler is the only one that should decide. Closing here as
                    // well would race it, and which one won would depend on paint
                    // order — sometimes leaving no menu at all.
                    if event.button == gpui::MouseButton::Right {
                        return;
                    }
                    workbench.context_menu = None;
                    cx.notify();
                },
            )),
        ))
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
        self.text_selection.select_all();
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
        self.running_fix = Some(RunningFix {
            label,
            link: None,
            lines: Vec::new(),
            notes: Vec::new(),
            check_id,
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

    /// The `/name` picker, shown above the composer.
    ///
    /// Above it for the same reason the approval card is (§40): that is where attention already
    /// is, and it cannot be scrolled away from. A plain flex child rather than a floating popup —
    /// no position to measure, and it behaves like part of the composer, which is what it is.
    fn subagent_picker(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let text = self.composer.read(cx).text().to_string();
        if !subagent::completing(&text) {
            return None;
        }
        let agents = workspace::subagents();
        let query = subagent::parse(&text).map(|c| c.name).unwrap_or_default();
        let matched = subagent::ranked(&query, &agents);

        let mut list = div()
            .id("subagent-picker")
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .min_w_0()
            .mx_2()
            .max_h(px(220.))
            .overflow_y_scroll()
            .rounded_lg()
            .bg(rgb(theme::elevated()))
            .border_1()
            .border_color(rgb(theme::border_strong()));

        if agents.is_empty() {
            // The registry is written when the backend assembles a coordinator, so before the
            // first turn there is nothing to offer. Say which, rather than showing an empty box.
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(
                        "No specialist list yet. It is written when the backend builds a \
                         coordinator, so ask one ordinary question first — and if you just \
                         updated the app, restart the backend (ctrl-p → Restart).",
                    ),
            );
        } else if matched.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(format!("No specialist matches \"{query}\".")),
            );
        }

        let selected = self.subagent_selected.min(matched.len().saturating_sub(1));
        for (index, agent) in matched.iter().enumerate() {
            let chosen = agent.name.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("subagent-{}", agent.name)))
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .gap_px()
                    .when(index == selected, |row| row.bg(rgb(theme::accent_soft())))
                    .hover(|style| style.bg(rgb(theme::overlay())).cursor_pointer())
                    .child(
                        ui::Label::new(format!("/{}", agent.name))
                            .colour(if index == selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            })
                            .ellipsis(),
                    )
                    // The description is not decoration: none of these names says what it does,
                    // and the request's own guesses show that nobody can be expected to know.
                    .child(
                        ui::Label::new(agent.description.clone())
                            .colour(theme::text_faint())
                            .size(ui::Size::Compact)
                            .ellipsis(),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.choose_subagent(&chosen, cx);
                    })),
            );
        }
        Some(list.into_any_element())
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
                    // Into the provenance record as well as the Jobs panel. A background worker
                    // runs on its own LangGraph thread, so none of its events reach this
                    // conversation's stream — the `async_tasks` map is the only trace on this
                    // side, and the record had never been told about it. Which is why the graph
                    // showed nothing for work a researcher had explicitly handed off
                    // (docs §111).
                    self.provenance.observe_background(
                        &format!("async:{}", task.task_id),
                        &task.agent_name,
                        provenance::now_ms(),
                    );
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
    /// Bring the backend up at launch, then show the history it has.
    fn warm_up(&mut self, cx: &mut Context<Self>) {
        self.status = "starting the agent…".into();
        let mut ready = self.sidecar.warm_up();
        cx.spawn(async move |this, cx| {
            let status = ready.next().await;
            let _ = this.update(cx, |workbench, cx| {
                if let Some(status) = status {
                    workbench.status = status.label().into();
                    // Remembered, not just announced. Whether this app started the backend
                    // decides whether the backend is running this app's overlay, and the
                    // status line is gone by the time that matters (docs §80).
                    workbench.backend_start = Some(status);
                }
                workbench.refresh_conversations(cx);
                workbench.refresh_project(cx);
                cx.notify();
            });
        })
        .detach();
        // Also ask straight away: a backend left running from a previous session answers
        // immediately, and waiting on the spawn would hide the list for no reason.
        self.refresh_conversations(cx);
    }

    fn refresh_conversations(&mut self, cx: &mut Context<Self>) {
        let mut updates = self.sidecar.list_conversations();
        cx.spawn(async move |this, cx| {
            if let Some(conversations) = updates.next().await {
                let _ = this.update(cx, |workbench, cx| {
                    workbench.conversations = conversations;
                    // Only on a real answer. A failed fetch sends nothing, so the list keeps
                    // saying "loading" rather than claiming the researcher has none — a
                    // backend that is still booting will answer the next refresh.
                    workbench.conversations_loaded = true;
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
        // Read back from the thread being opened, below. Cleared first so a failure to load
        // shows the new conversation as having no record rather than the previous one's.
        self.provenance = provenance::Record::default();
        self.text_selection.update(|selection| selection.clear());
        self.buckets.clear();
        self.tasks.clear();
        self.jobs.clear();
        self.error = None;
        self.approve_conversation = false;
        self.approve_tasks.clear();
        self.status = "opening…".into();

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
        let mut messages = self.sidecar.open_conversation(thread_id);
        cx.spawn(async move |this, cx| {
            if let Some((messages, snapshot)) = messages.next().await {
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
                        if let Some(project) = snapshot.project {
                            workbench.project =
                                Some(merge_spine(workbench.project.as_ref(), project));
                        }
                        for job in snapshot.jobs {
                            workbench.track_job(job, cx);
                        }
                        for task in snapshot.tasks {
                            workbench.track_task(task, cx);
                        }
                    }
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
        if current_is_targeted && self.tasks.iter().any(|task| !task.is_finished()) {
            // A background worker can still be writing beneath the conversation directory after
            // the foreground turn ends. Deleting that tree underneath it would recreate the
            // folder or lose the remainder of its work; wait for the task's terminal state.
            self.say(
                format!(
                    "can't delete this {} while its background work is running",
                    target.noun()
                ),
                cx,
            );
            return;
        }
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

    fn finish_turn(&mut self, cx: &mut Context<Self>) {
        self.collect_plots();
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
        // The same scorer the command palette uses, so "pap" finds "Rendimiento de papa"
        // and typing feels the way Zed's file finder does rather than like a substring
        // match that misses the obvious (docs §49).
        let query = self.conversation_query.read(cx).text().to_string();
        let mut ranked: Vec<(i32, &protocol::Conversation)> = self
            .conversations
            .iter()
            .filter_map(|conversation| {
                match_score(&query, &conversation.title).map(|score| (score, conversation))
            })
            .collect();
        if !query.trim().is_empty() {
            ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        }
        let matched: Vec<&protocol::Conversation> = ranked
            .into_iter()
            .map(|(_, conversation)| conversation)
            .collect();

        let mut list = div()
            .id("conversations")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .overflow_y_scroll()
            .p_1()
            .gap_px();

        if matched.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(if !self.conversations_loaded {
                        // The backend takes seconds to boot from cold, and this list is
                        // the first thing anyone looks at.
                        "Loading your conversations…"
                    } else if self.conversations.is_empty() {
                        "Conversations you start will appear here."
                    } else {
                        "Nothing matches that."
                    }),
            );
        }

        // Grouped by project, ungrouped last.
        //
        // A heading per project rather than an indent or a colour: the sidebar is scanned, and a
        // name is the only marker that survives being glanced at. The order is alphabetical with
        // ungrouped work pinned to the bottom, so the list does not reshuffle as work moves
        // between projects — a sidebar that reorders itself is one nobody builds a memory of
        // (docs §106, §154).
        let mut grouped: std::collections::BTreeMap<Option<String>, Vec<&protocol::Conversation>> =
            std::collections::BTreeMap::new();
        for conversation in &matched {
            grouped
                .entry(conversation.project.clone())
                .or_default()
                .push(conversation);
        }
        let mut ordered: Vec<(Option<String>, Vec<&protocol::Conversation>)> =
            grouped.into_iter().collect();
        ordered.sort_by(|a, b| match (&a.0, &b.0) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, _) => std::cmp::Ordering::Greater,
            (_, None) => std::cmp::Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        });
        // A named project always gets its heading now, even when it is the only group. The
        // heading is no longer decoration: it owns New here, Open folder and Delete project, so
        // hiding it would hide the only project-delete affordance (§155). Ungrouped work alone
        // still needs no heading because it is not a project and has no project folder to delete.
        let show_headings = ordered.len() > 1
            || ordered
                .iter()
                .any(|(project, _conversations)| project.is_some());

        for (project, conversations) in ordered {
            if show_headings {
                let heading = project
                    .clone()
                    .unwrap_or_else(|| UNGROUPED_PROJECT_LABEL.to_string());
                let opening = project.clone();
                let starting = project.clone();
                // All conversations in the project, not merely the rows that survived the
                // sidebar search. A filter is a way to find work, never a deletion boundary.
                let deleting_project = project.clone().map(|name| DeleteTarget::Project {
                    conversations: self
                        .conversations
                        .iter()
                        .filter(|conversation| {
                            conversation.project.as_deref() == Some(name.as_str())
                        })
                        .cloned()
                        .collect(),
                    name,
                });
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .pt_2()
                        .pb_1()
                        .group(SharedString::from(format!("head-{heading}")))
                        .child(
                            div()
                                .id(SharedString::from(format!("project-{heading}")))
                                .flex_grow()
                                .min_w_0()
                                .hover(|style| style.cursor_pointer())
                                // Clicking the name opens the folder — the whole reason a
                                // project is a directory rather than a label (docs §105).
                                .on_click(move |_event, _window, _cx| {
                                    let dir = match &opening {
                                        Some(name) => workspace::project_folder(name)
                                            .map(|folder| workspace::root().join(folder)),
                                        None => Some(workspace::root()),
                                    };
                                    if let Some(dir) = dir {
                                        if let Err(error) = workspace::open(&dir) {
                                            tracing::warn!(%error, "could not open a project");
                                        }
                                    }
                                })
                                .child(section_label_owned(heading.to_uppercase())),
                        )
                        .when_some(deleting_project, |header, target| {
                            let id = match &target {
                                DeleteTarget::Project { name, .. } => {
                                    SharedString::from(format!("delete-project-{name}"))
                                }
                                DeleteTarget::Conversation(_) => unreachable!(),
                            };
                            header.child(
                                div()
                                    .id(id)
                                    .flex_none()
                                    .px_1()
                                    .rounded_md()
                                    .text_color(rgb(theme::text_faint()))
                                    .text_xs()
                                    .invisible()
                                    .group_hover(
                                        SharedString::from(format!("head-{heading}")),
                                        |style| style.visible(),
                                    )
                                    .hover(|style| {
                                        style.text_color(rgb(theme::error())).cursor_pointer()
                                    })
                                    .child("✕")
                                    .tooltip(move |_window, cx| {
                                        cx.new(|_| Hint {
                                            text: "delete project and saved files".into(),
                                        })
                                        .into()
                                    })
                                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                                        workbench.request_delete(target.clone(), window, cx);
                                    })),
                            )
                        })
                        // Asked for directly: starting work in a project should not mean
                        // starting it somewhere else and then filing it (docs §107). Revealed
                        // on hover, like the rename and delete controls on the rows below.
                        .child(
                            div()
                                .id(SharedString::from(format!("new-in-{heading}")))
                                .flex_none()
                                .px_1()
                                .rounded_md()
                                .text_color(rgb(theme::text_faint()))
                                .text_xs()
                                .invisible()
                                .group_hover(
                                    SharedString::from(format!("head-{heading}")),
                                    |style| style.visible(),
                                )
                                .hover(|style| {
                                    style.text_color(rgb(theme::accent())).cursor_pointer()
                                })
                                .child("+")
                                .tooltip(move |_window, cx| {
                                    cx.new(|_| Hint {
                                        text: "new conversation here".into(),
                                    })
                                    .into()
                                })
                                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                    workbench.new_thread_in(starting.clone(), cx);
                                })),
                        ),
                );
            }
            for conversation in conversations {
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
                            .border_color(rgb(theme::accent()))
                            .child(self.rename_editor.clone()),
                    );
                    continue;
                }

                // The row stays until the backend confirms deletion. Removing it optimistically
                // is what made a failed request look successful until launch brought both the
                // conversation and its derived project heading back (§154).
                if self
                    .deleting
                    .as_ref()
                    .is_some_and(|target| target.contains_thread(&thread_id))
                {
                    list = list.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .min_w_0()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(theme::elevated()))
                            .child(
                                ui::Label::new("Deleting this conversation…")
                                    .muted()
                                    .size(ui::Size::Compact)
                                    .ellipsis(),
                            ),
                    );
                    continue;
                }

                let open = thread_id.clone();
                let rename = thread_id.clone();
                let remove = DeleteTarget::Conversation((*conversation).clone());
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
                        .rounded_md()
                        .when(selected, |row| row.bg(rgb(theme::accent_soft())))
                        // Every row reacts to the pointer. A list that does not respond to
                        // the cursor does not read as a list of *buttons*.
                        .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                        .child(
                            ui::Label::new(conversation.title.clone())
                                .colour(if selected {
                                    theme::text()
                                } else {
                                    theme::text_muted()
                                })
                                .size(ui::Size::Compact)
                                .ellipsis(),
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
                                .text_color(rgb(theme::text_faint()))
                                .text_xs()
                                .hover(|style| {
                                    style.text_color(rgb(theme::accent())).cursor_pointer()
                                })
                                .child("rename")
                                .on_click(cx.listener(move |workbench, _event, window, cx| {
                                    workbench.start_rename(rename.clone(), window, cx);
                                })),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("delete-{thread_id}")))
                                .flex_none()
                                .invisible()
                                .group_hover(
                                    SharedString::from(format!("conv-group-{thread_id}")),
                                    |style| style.visible(),
                                )
                                .px_1()
                                .rounded_md()
                                .text_color(rgb(theme::text_faint()))
                                .text_xs()
                                .hover(|style| {
                                    style.text_color(rgb(theme::error())).cursor_pointer()
                                })
                                .child("✕")
                                .on_click(cx.listener(move |workbench, _event, window, cx| {
                                    workbench.request_delete(remove.clone(), window, cx);
                                })),
                        )
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            workbench.open_conversation(open.clone(), cx);
                        })),
                );
            }
        }

        div()
            .flex()
            .flex_col()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_none()
            // A rounded card on the window background, the way Zed's panels sit, rather
            // than a full-bleed slab meeting the next panel at a hairline (docs §50).
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
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
                    .border_color(rgb(theme::border()))
                    .child(
                        div()
                            .id("open-settings")
                            .text_color(rgb(theme::accent()))
                            .hover(|style| {
                                style
                                    .text_color(rgb(theme::accent_hover()))
                                    .cursor_pointer()
                            })
                            .child("◎")
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.run_command(Command::OpenSettings, cx);
                            })),
                    )
                    .child(
                        // Not a `ui::Button`: it brightens its border *and* its text on
                        // hover, which no other button does, and it uses `border_strong`.
                        // One call site is not worth a flag on the shared type.
                        div()
                            .id("new-conversation")
                            .rounded_md()
                            .px_2()
                            .py_1()
                            .border_1()
                            .border_color(rgb(theme::border_strong()))
                            .text_color(rgb(theme::text_muted()))
                            .text_xs()
                            .hover(|style| {
                                style
                                    .text_color(rgb(theme::accent()))
                                    .border_color(rgb(theme::accent()))
                                    .cursor_pointer()
                            })
                            .child("New")
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.run_command(Command::NewThread, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .m_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(theme::background()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .child(self.conversation_query.clone()),
            )
            .child(list)
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

    /// The five providers, as pills rather than a cycle button.
    ///
    /// Five fit on one row, so there is no reason to make someone click through them —
    /// and a cycle button hides four of the five, which is the same complaint the theme
    /// list just answered (docs §58).
    fn provider_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_row().flex_wrap().w_full().gap_1();
        for spec in &settings::PROVIDERS {
            let selected = spec.id == self.draft.provider;
            row = row.child(
                // A selectable pill, not a button: it has a *chosen* state with its own
                // background, and "which one of these is picked" is a different control from
                // "press this to do a thing".
                div()
                    .id(SharedString::from(format!("provider-{}", spec.id)))
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if selected {
                        theme::accent()
                    } else {
                        theme::border()
                    }))
                    .when(selected, |pill| pill.bg(rgb(theme::accent_soft())))
                    .text_color(rgb(if selected {
                        theme::text()
                    } else {
                        theme::text_muted()
                    }))
                    .text_xs()
                    .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                    .child(spec.label)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.draft.provider = spec.id.to_string();
                        // Suggest a model that exists for the provider just chosen, rather
                        // than leaving one that does not.
                        workbench.set_field(Field::ModelId, spec.suggested_model, cx);
                        cx.notify();
                    })),
            );
        }
        row
    }

    /// The provider's models, as a scrollable list that fills the field.
    ///
    /// Curated, not a catalogue, and the field below stays editable — a list here can only
    /// ever be out of date, and a provider shipping a model the day after a release must
    /// not make the app unusable.
    fn model_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.field_text_or(Field::ModelId, &self.draft.model_id, cx);
        let models = settings::provider(&self.draft.provider)
            .map(|spec| spec.models)
            .unwrap_or(&[]);

        let mut list = div()
            .id("model-rows")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            // Same gutter as every other list a scrollbar is drawn over (docs §100).
            .pr(px(SCROLL_GUTTER))
            .gap_px()
            // Capped, because `custom` could list anything and a long list would push the
            // API-key field out of the modal.
            .max_h(px(150.))
            .overflow_y_scroll()
            .track_scroll(&self.model_scroll);

        for model in models {
            let selected = *model == current;
            list = list.child(
                div()
                    .id(SharedString::from(format!("model-{model}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    // Background only, no border: a border on the selected row alone made
                    // it taller than its neighbours, so the list jumped as you moved down.
                    .when(selected, |row| row.bg(rgb(theme::accent_soft())))
                    .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                    .child(
                        // The label truncates, not the row. `truncate` on the flex item
                        // itself gave it zero intrinsic width, so every model rendered as
                        // a bare "…" (docs §59).
                        ui::Label::new(model.to_string())
                            .colour(if selected {
                                theme::accent()
                            } else {
                                theme::text_muted()
                            })
                            .size(ui::Size::Compact)
                            .ellipsis(),
                    )
                    .when(selected, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_color(rgb(theme::accent()))
                                .text_xs()
                                .child("✓"),
                        )
                    })
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.set_field(Field::ModelId, model, cx);
                        cx.notify();
                    })),
            );
        }

        div()
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .child(list)
            .children(scrollbar(&self.model_scroll))
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

    /// Every palette at once, each showing what it looks like.
    ///
    /// The cycle button was wrong twice over: the only way to find a palette was to click
    /// through all of them, and there was no way to see what existed. Zed shows the whole
    /// list and previews on hover, so a theme is judged by looking rather than by reading
    /// its name (docs §50).
    ///
    /// GPUI 0.2.2 has hover *styling* but no hover *event*, so a true live preview would
    /// need a custom element. The swatch does the same job in miniature and is arguably
    /// better here: every theme is visible side by side, rather than one at a time.
    fn theme_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The same fuzzy scorer as everywhere else, so `mocha` finds Catppuccin Mocha.
        let query = self.theme_filter.read(cx).text().to_string();
        let mut matched: Vec<(i32, String, theme::Theme)> = settings::available_themes()
            .into_iter()
            .filter_map(|(name, palette)| {
                match_score(&query, &name).map(|score| (score, name, palette))
            })
            .collect();
        if !query.trim().is_empty() {
            matched.sort_by_key(|(score, _, _)| std::cmp::Reverse(*score));
        }

        // Capped and scrollable: four built-ins fit, a hundred installed palettes do not,
        // and a list that grows without bound pushes Save off the modal (docs §58).
        let mut list = div()
            .id("theme-rows")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            // Room for the thumb, which is painted over this by the wrapper below. Without it
            // the bar sits on the rows' right border and the last colour swatch (docs §100).
            .pr(px(SCROLL_GUTTER))
            .gap_1()
            .max_h(px(260.))
            .overflow_y_scroll()
            .track_scroll(&self.theme_scroll);

        for (_, name, palette) in matched {
            let selected = name.eq_ignore_ascii_case(&self.applied_theme);
            let chosen = name.clone();
            let previewed = name.clone();

            // Enough of the palette to tell warm from cool and light from dark at a
            // glance, which is what someone is actually choosing between.
            let mut swatch = div().flex().flex_row().flex_none().gap_px();
            for colour in [
                palette.background,
                palette.surface,
                palette.accent,
                palette.text,
                palette.error,
            ] {
                swatch = swatch.child(
                    div()
                        .w(px(12.))
                        .h(px(12.))
                        .rounded_sm()
                        .bg(rgb(colour))
                        .border_1()
                        .border_color(rgb(palette.border)),
                );
            }

            list = list.child(
                div()
                    .id(SharedString::from(format!("theme-{name}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(if selected {
                        theme::accent()
                    } else {
                        theme::border()
                    }))
                    .when(selected, |row| row.bg(rgb(theme::accent_soft())))
                    .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                    // The live preview: pointing at a theme applies it to the whole
                    // window, and leaving puts back whatever was chosen. GPUI does have a
                    // hover *event* — `InteractiveElement::on_hover` — so this needed no
                    // custom element after all (docs §52).
                    .on_hover(cx.listener(move |workbench, hovering: &bool, _window, cx| {
                        let showing = if *hovering {
                            previewed.clone()
                        } else {
                            workbench.applied_theme.clone()
                        };
                        let palette = settings::available_themes()
                            .into_iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(&showing))
                            .map(|(_, palette)| palette);
                        if let Some(palette) = palette {
                            theme::apply(&palette);
                            cx.notify();
                        }
                    }))
                    .child(
                        ui::Label::new(name.clone())
                            .colour(if selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            })
                            .ellipsis(),
                    )
                    .child(swatch)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.draft.theme = chosen.clone();
                        workbench.applied_theme = chosen.clone();
                        // Immediately, so the choice is judged by the window it changes.
                        settings::apply_theme(&workbench.draft);
                        // And the picker closes: choosing is the thing it was opened to do, and
                        // a list that stays up over the window it just repainted hides the very
                        // change being judged (docs §88).
                        workbench.open_picker = None;
                        cx.notify();
                    })),
            );
        }

        // The gallery. Zed's theme extensions are pure data — the registry marks every
        // one `wasm_api_version: null` — so they can be fetched and read here, unlike the
        // language extensions this app genuinely cannot run (docs §52).
        let mut gallery = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .pt_2()
            .child(section_label("GET MORE"))
            .child(self.filter_field(self.gallery_query.clone(), cx));

        if !self.gallery_note.is_empty() {
            gallery = gallery.child(
                div()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(self.gallery_note.clone()),
            );
        }

        for listing in self.gallery_results.iter().take(12) {
            let id = listing.id.clone();
            // Author and source shown because these are other people's work under their
            // own licences, and a gallery that hides authorship is not a gallery.
            let by = listing.authors.first().cloned().unwrap_or_default();
            gallery = gallery.child(
                div()
                    .id(SharedString::from(format!("gallery-{}", listing.id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(theme::text()))
                                    .text_sm()
                                    .child(listing.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(theme::text_faint()))
                                    .text_xs()
                                    .child(format!("{by} · {} installs", listing.download_count)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::accent()))
                            .text_xs()
                            .child("install"),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.install_theme(id.clone(), cx);
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(self.filter_field(self.theme_filter.clone(), cx))
            // The scrollbar lives outside the scrolling list, in a relative wrapper.
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(list)
                    .children(scrollbar(&self.theme_scroll)),
            )
            .child(gallery)
            .child(
                div()
                    // `w_full` + `min_w_0` so the path *wraps*. Without them this line's
                    // intrinsic width — an unbreakable Windows path — became the popup's
                    // minimum, and a panel declared at 320px rendered at nearly 400, pushing
                    // the filter field and every swatch off the right-hand edge (docs §86).
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(format!(
                        "Or drop a Zed theme .json in {}.",
                        settings::themes_dir().display()
                    )),
            )
    }

    /// A file, shown in the middle of the window.
    ///
    /// The shape is Zed's picker: a centred panel floating over a dimmed workbench, which
    /// they use for all fifty-odd of their modals. It suits this exactly — opening a
    /// figure or a report is something you do, look at, and dismiss, not somewhere you
    /// navigate to and have to find your way back from (docs §49).
    /// Every project that exists, plus the way out of one.
    ///
    /// The list is derived from the conversations themselves rather than kept anywhere: a project
    /// is exactly "a name some conversation is filed under", so there is no separate registry to
    /// fall out of step with the sidebar (docs §106). Creating one is typing a name into the
    /// filter field and pressing the row that offers it — the same gesture as choosing an
    /// existing one, so there is no second mode to learn.
    fn project_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.project_query.read(cx).text().trim().to_string();
        let current = self.sidecar.project();
        let mut names: Vec<String> = self
            .conversations
            .iter()
            .filter_map(|conversation| conversation.project.clone())
            .collect();
        names.sort();
        names.dedup();

        let mut list = div()
            .id("project-rows")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .pr(px(SCROLL_GUTTER))
            .gap_px()
            .max_h(px(240.))
            .overflow_y_scroll();

        // Offered first, because naming a new one is the reason this list is usually open.
        if !typed.is_empty() && !names.iter().any(|name| name == &typed) {
            let created = typed.clone();
            list = list.child(
                picker_row(
                    format!("New project “{typed}”"),
                    false,
                    Some("creates the folder".into()),
                )
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    workbench.file_in_project(Some(created.clone()), cx);
                })),
            );
        }

        list = list.child(
            picker_row(UNGROUPED_PROJECT_LABEL, current.is_none(), None).on_click(
                cx.listener(|workbench, _event, _window, cx| workbench.file_in_project(None, cx)),
            ),
        );

        for name in names {
            if !typed.is_empty() && crate::match_score(&typed, &name).is_none() {
                continue;
            }
            let chosen = name.clone();
            list = list.child(
                picker_row(
                    name.clone(),
                    current.as_deref() == Some(name.as_str()),
                    None,
                )
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    workbench.file_in_project(Some(chosen.clone()), cx);
                })),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(self.filter_field(self.project_query.clone(), cx))
            .child(list)
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
        // A new conversation is a new enquiry. The one just left keeps its own record on disk,
        // where reopening it will find it.
        self.provenance = provenance::Record::default();
        self.text_selection.update(|selection| selection.clear());
        self.buckets.clear();
        self.tasks.clear();
        self.jobs.clear();
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

    /// A model per specialist, under the coordinator's.
    ///
    /// **The specialists do genuinely different work**, and one model for all ten is either an
    /// expensive way to grep or a cheap way to write a paper. Literature search wants a long
    /// context and cheap tokens across many calls; a report wants the best prose available; data
    /// cleaning wants neither and runs dozens of times.
    ///
    /// The list is the live registry (§76), so it cannot name a specialist the backend does not
    /// have — and when the registry is empty it says why rather than showing nothing.
    fn subagent_models(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let specialists = workspace::subagents();
        let mut rows = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(section_label("PER SPECIALIST"));

        if specialists.is_empty() {
            return rows.child(
                ui::Label::new(
                    "The specialists appear here once the backend has answered its first \
                     question. Until then they all use the model above.",
                )
                .muted()
                .size(ui::Size::Compact),
            );
        }

        for (index, specialist) in specialists.iter().enumerate() {
            // "Use default" rather than a repeat of the coordinator's model: the two are
            // different states. One follows whatever the coordinator becomes; the other is a
            // choice that happens to match today and would not move with it.
            let chosen = self
                .draft
                .subagents
                .get(&specialist.name)
                .map(|spec| spec.rsplit("::").next().unwrap_or(spec).to_string())
                .unwrap_or_else(|| "Use default".to_string());
            rows = rows.child(ui::setting_row(
                specialist.name.clone(),
                specialist.description.clone(),
                ui::Dropdown::new(
                    SharedString::from(format!("pick-subagent-{index}")),
                    chosen,
                )
                .open(matches!(self.open_picker, Some((Picker::Subagent(open), _)) if open == index))
                .on_click(cx.listener(move |workbench, event: &gpui::ClickEvent, _window, cx| {
                    workbench.toggle_picker(Picker::Subagent(index), event.position(), cx);
                })),
            ));
        }
        rows
    }

    /// The models one specialist can be pointed at, plus the way back to the default.
    ///
    /// Every provider's models, not just the current one's: pointing literature search at a
    /// cheap long-context model from another provider is the main reason to want this at all.
    /// The key for that provider has to be stored, so the row says when it is not — a turn that
    /// fails inside a subagent several minutes in is the worst place to discover it (§104).
    fn subagent_model_list(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let specialists = workspace::subagents();
        let Some(specialist) = specialists.get(index) else {
            return div().into_any_element();
        };
        let name = specialist.name.clone();
        let chosen = self.draft.subagents.get(&name).cloned();

        let mut list = div()
            .id(SharedString::from(format!("subagent-models-{index}")))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .pr(px(SCROLL_GUTTER))
            .gap_px()
            .max_h(px(260.))
            .overflow_y_scroll();

        let clearing = name.clone();
        list = list.child(
            picker_row("Use default", chosen.is_none(), None).on_click(cx.listener(
                move |workbench, _event, _window, cx| {
                    workbench.draft.subagents.remove(&clearing);
                    workbench.open_picker = None;
                    cx.notify();
                },
            )),
        );

        for provider in settings::PROVIDERS {
            for model in provider.models {
                let spec = format!("{}::{}", provider.id, model);
                let selected = chosen.as_deref() == Some(spec.as_str());
                // Named only when it would be a *second* provider to key, since that is the
                // thing a researcher has to act on before the choice can work.
                let missing = provider.id != self.draft.provider
                    && settings::secret(&format!("llm:{}", provider.id)).is_none();
                let note = missing.then(|| format!("{} — no key stored", provider.label));
                let picked = name.clone();
                let value = spec.clone();
                list = list.child(
                    picker_row(*model, selected, note)
                        .id(SharedString::from(format!("sa-{index}-{spec}")))
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            workbench
                                .draft
                                .subagents
                                .insert(picked.clone(), value.clone());
                            workbench.open_picker = None;
                            cx.notify();
                        })),
                );
            }
        }
        list.into_any_element()
    }

    /// The irreversible scope, in the centre of the window rather than squeezed into a row.
    ///
    /// Conversation deletion now includes its saved outputs, and project deletion includes every
    /// conversation plus the complete project folder. The old inline "delete / keep" row had no
    /// room to say either fact; confirmation without the consequence is only a second click
    /// (§155).
    fn delete_modal(&self, target: &DeleteTarget, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, body, action) = match target {
            DeleteTarget::Conversation(conversation) => {
                let path = workspace::thread_dir_in(
                    conversation.project.as_deref(),
                    &conversation.thread_id,
                );
                let body = div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(ui::Label::new(format!(
                        "This permanently deletes “{}”, its chat history, and every saved file it produced.",
                        conversation.title
                    )))
                    .child(
                        ui::Label::new(format!("Saved folder:\n{}", path.display()))
                            .muted()
                            .size(ui::Size::Compact),
                    )
                    .into_any_element();
                ("Delete conversation?", body, "Delete conversation")
            }
            DeleteTarget::Project {
                name,
                conversations,
            } => {
                let path = workspace::project_folder(name)
                    .map(|folder| workspace::root().join(folder))
                    .unwrap_or_else(workspace::root);
                let count = conversations.len();
                let body = div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(ui::Label::new(format!(
                        "This permanently deletes project “{name}”, {count} conversation{}, and the entire project folder.",
                        if count == 1 { "" } else { "s" }
                    )))
                    .child(
                        ui::Label::new(
                            "Files placed directly in the project folder are deleted too — not only files Mini-Me created.",
                        )
                        .colour(theme::warning()),
                    )
                    .child(
                        ui::Label::new(format!("Project folder:\n{}", path.display()))
                            .muted()
                            .size(ui::Size::Compact),
                    )
                    .into_any_element();
                ("Delete project?", body, "Delete project")
            }
        };

        ui::Modal::new("delete-confirmation", title)
            .width(560.)
            .focus(&self.delete_focus)
            .body(body)
            .actions(
                ui::actions()
                    .child(div().flex_grow())
                    .child(
                        ui::Button::new("delete-cancel", "Cancel").on_click(cx.listener(
                            |workbench, _event, _window, cx| {
                                workbench.confirming_delete = None;
                                workbench.restore_focus = true;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        ui::Button::new("delete-confirm", action)
                            .tone(ui::Tone::Danger)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.confirm_delete(cx);
                            })),
                    ),
            )
            .footer(
                ui::Label::new("There is no undo.")
                    .colour(theme::error())
                    .size(ui::Size::Compact),
            )
    }

    /// What this thing is, what the specialists do, and who to credit.
    ///
    /// Asked for after a look at the web app, which has one and this did not. Three jobs, and the
    /// third is not optional:
    ///
    /// 1. **Say what the specialists are.** Ten of them delegate to each other and a researcher
    ///    meets them one at a time, in a trace, mid-answer. A list is the cheapest orientation
    ///    there is.
    /// 2. **Say where the data comes from.** Asta, CIP Dataverse, AGROVOC and Crop Ontology are
    ///    other people's catalogues, and which one an answer leaned on changes how it should be
    ///    read.
    /// 3. **Credit Asta.** The Allen Institute asks that work using it cite AstaBench, and a tool
    ///    that makes their search easy to use while making the citation hard to find is taking
    ///    something without saying so. The reference is here, selectable, next to a note about
    ///    when it applies (docs §103).
    ///
    /// **The team list is read from the live registry**, not written here. §76 built that list
    /// precisely so a copy in the client could not drift the first time upstream renamed a
    /// specialist, and an About box that names agents the backend no longer has would be the
    /// same defect wearing a friendlier face.
    fn about_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let specialists = workspace::subagents();

        let mut team = div().flex().flex_col().w_full().min_w_0().gap_2();
        if specialists.is_empty() {
            // Said rather than left blank: an empty list looks like "there are none", and the
            // real reason is that the backend has not assembled a coordinator yet (docs §78).
            team = team.child(
                ui::Label::new(
                    "The specialist list appears once the backend has answered its first question.",
                )
                .muted()
                .size(ui::Size::Compact),
            );
        }
        for specialist in &specialists {
            team = team.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(ui::Label::new(specialist.name.clone()).colour(theme::accent()))
                    .child(
                        ui::Label::new(specialist.description.clone())
                            .muted()
                            .size(ui::Size::Compact),
                    ),
            );
        }

        let mut sources = div().flex().flex_col().w_full().min_w_0().gap_2();
        for (name, what) in [
            (
                "Asta",
                "Allen Institute for AI — federated academic literature search and citation \
                 tracing.",
            ),
            (
                "CIP Dataverse",
                "The International Potato Center's dataset catalogue, with persistent DOIs and \
                 full metadata.",
            ),
            (
                "AGROVOC",
                "FAO's multilingual agricultural vocabulary, used to normalise crop, soil and \
                 pest terminology.",
            ),
            (
                "Crop Ontology",
                "Standardised crop traits, genotypes and phenotypes, for comparability across \
                 studies.",
            ),
        ] {
            sources = sources.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(ui::Label::new(name).colour(theme::accent()))
                    .child(ui::Label::new(what).muted().size(ui::Size::Compact)),
            );
        }

        // **Where code runs, as this install is actually configured.** The web app's About says
        // every conversation runs in an isolated LangSmith sandbox. On this app that is usually
        // false: host execution is the default, because a local-first workbench shipping the
        // researcher's own files to a rented VM to be read was the wrong shape (docs §11). Saying
        // the reassuring thing regardless is the defect this repo has already reported upstream
        // in `guardrails.py`, and it would be worse to repeat it here, in the document that
        // explains the product.
        let execution = if self.sidecar.runs_locally() {
            (
                "Runs on this machine",
                "Python and shell code the agent writes execute here, with your permissions, in \
                 this conversation's folder under Documents\\Mini-Me. Commands that touch your \
                 system stop for your approval first.",
            )
        } else {
            (
                "Runs in an isolated sandbox",
                "Python and shell code the agent writes execute in a LangSmith sandbox rather \
                 than on this machine. Files it produces are copied back into this \
                 conversation's folder.",
            )
        };

        let body = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(ui::Label::new(
                "A research workbench. A coordinator delegates to specialists that search the \
                 literature, find datasets, clean and analyse tabular data, build models, and \
                 write the findings up.",
            ))
            .child(section_label("THE SPECIALISTS"))
            .child(team)
            .child(section_label("WHERE THE DATA COMES FROM"))
            .child(sources)
            .child(section_label("WHERE CODE RUNS"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(ui::Label::new(execution.0).colour(theme::accent()))
                    .child(ui::Label::new(execution.1).muted().size(ui::Size::Compact)),
            )
            .child(section_label("CITING THIS WORK"))
            .child(ui::Label::new(
                "Literature search is powered by Asta, from the Allen Institute for AI. If your \
                 work uses output produced with it, please cite AstaBench:",
            ))
            // Selectable, because a citation you cannot copy is a citation you will retype
            // wrongly. `ctrl-c` takes it once dragged over, like the transcript (docs §62).
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_l_2()
                    .border_color(rgb(theme::accent()))
                    .bg(rgb(theme::surface()))
                    .text_color(rgb(theme::text()))
                    .text_sm()
                    .child(selection::Selectable::new(
                        &self.text_selection,
                        ASTA_CITATION.to_string(),
                        StyledText::new(ASTA_CITATION),
                    )),
            )
            .child(
                ui::Label::new(
                    "Generative AI produced the analysis and prose in this app. Say so in \
                     anything you publish from it, and have a subject-matter expert check it.",
                )
                .muted()
                .size(ui::Size::Compact),
            );

        ui::Modal::new("about", "About Mini-Me")
            .width(640.)
            .focus(&self.about_focus)
            .body(body)
            .actions(ui::actions().child(div().flex_grow()).child(
                ui::Button::new("about-close", "Close").on_click(cx.listener(
                    |workbench, _event, _window, cx| {
                        workbench.about_open = false;
                        workbench.restore_focus = true;
                        cx.notify();
                    },
                )),
            ))
    }

    /// The record of this enquiry: what was consulted, in what order, and where it doubled back.
    ///
    /// Requested (docs §73) with one sentence as the specification — *"each scientist can track
    /// his work by conversation"* — and built as a modal for the reason §68 moved Setup into one:
    /// it is something you open, read and close, not a place you navigate to.
    fn provenance_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.provenance_view;
        let rail = ui::nav_rail()
            .child(
                ui::NavEntry::new(
                    "prov-timeline",
                    "Timeline",
                    view == ProvenanceView::Timeline,
                )
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.provenance_view = ProvenanceView::Timeline;
                    cx.notify();
                })),
            )
            .child(
                ui::NavEntry::new("prov-graph", "Graph", view == ProvenanceView::Graph).on_click(
                    cx.listener(|workbench, _event, _window, cx| {
                        workbench.provenance_view = ProvenanceView::Graph;
                        cx.notify();
                    }),
                ),
            );

        // Which turn the graph is showing. Only on the graph — the timeline is one row per turn
        // already, so filtering it to a turn would leave a chart of one bar.
        let rail = if view == ProvenanceView::Graph && self.provenance.turns.len() > 1 {
            let mut rail = rail.child(div().pt_3().child(section_label("TURNS"))).child(
                ui::NavEntry::new(
                    "prov-turn-all",
                    format!("All {}", self.provenance.turns.len()),
                    self.provenance_turn.is_none(),
                )
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.provenance_turn = None;
                    cx.notify();
                })),
            );
            for (at, turn) in self.provenance.turns.iter().enumerate() {
                rail = rail.child(
                    ui::NavEntry::new(
                        SharedString::from(format!("prov-turn-{at}")),
                        // The question, cut to a line. A turn numbered and not named is a row
                        // the reader has to count to identify.
                        SharedString::from(one_line(&turn.prompt)),
                        self.provenance_turn == Some(at),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.provenance_turn = Some(at);
                        cx.notify();
                    })),
                );
            }
            rail
        } else {
            rail
        };

        let body = if self.provenance.is_empty() {
            // Distinguished from "nothing happened": a conversation of plain questions has a
            // record and it is empty of delegations, which is a fact about the work rather than
            // a failure of the feature.
            div().flex().flex_col().gap_2().child(
                ui::Label::new("No specialist has been consulted in this conversation yet.")
                    .muted(),
            )
        } else {
            match view {
                ProvenanceView::Timeline => self.provenance_timeline(),
                ProvenanceView::Graph => self.provenance_graph(),
            }
        };

        ui::Modal::new("provenance", "Provenance")
            .width(760.)
            .focus(&self.provenance_focus)
            .nav(rail)
            .body(body)
            // The exports are what let a researcher put this record in a methods section, which
            // is the whole reason it is kept. Both are text: Mermaid renders in GitHub, Quarto,
            // Obsidian and Typst, and SVG is what a journal wants a figure in.
            .actions(
                ui::actions()
                    .child(
                        ui::Button::new("provenance-mermaid", "Copy as Mermaid")
                            .size(ui::Size::Compact)
                            .disabled(self.provenance.is_empty())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                let text = mermaid(&workbench.provenance.graph_of(workbench.provenance_turn));
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                                workbench.say("provenance copied as a Mermaid diagram", cx);
                            })),
                    )
                    .child(
                        ui::Button::new("provenance-svg", "Save as SVG")
                            .tone(ui::Tone::Accent)
                            .size(ui::Size::Compact)
                            .disabled(self.provenance.is_empty() || self.thread_workspace().is_none())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.save_provenance_svg(cx);
                            })),
                    )
                    .child(div().flex_grow())
                    .child(
                        ui::Button::new("provenance-close", "Close").on_click(cx.listener(
                            |workbench, _event, _window, cx| {
                                workbench.provenance_open = false;
                                workbench.restore_focus = true;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .footer(
                ui::Label::new(match self.thread_workspace() {
                    Some(dir) => format!("kept in {}", dir.join(provenance::FILENAME).display()),
                    None => "kept beside this conversation's files, once it has some".to_string(),
                })
                .muted()
                .size(ui::Size::Compact),
            )
    }

    /// One row per turn, one bar per invocation, on a scale shared by the whole conversation.
    ///
    /// **The scale is the point, and getting it wrong made the view worse than useless.** It
    /// first normalised each turn against its own span, which meant a turn with a single
    /// invocation always drew a full-width bar — so an 8-second lookup and a 32-second one came
    /// out pixel-identical and *looked* comparable. A chart whose bars carry no information is
    /// worse than no chart, because it will be read anyway.
    ///
    /// So the divisor is the longest **turn span** in the conversation. Spans rather than
    /// individual durations because a turn's bars are laid out inside it, and a scale smaller
    /// than the span would push later bars off the end. Gaps *between* turns stay out of it —
    /// those are however long the researcher took to read and type, and including them would
    /// squash every bar to a sliver.
    fn provenance_timeline(&self) -> gpui::Div {
        let mut body = div().flex().flex_col().w_full().min_w_0().gap_4();
        // Shared by every row — see `Record::scale` for why, and for what it replaced.
        let scale = self.provenance.scale() as f32;
        for (index, turn) in self.provenance.turns.iter().enumerate() {
            if turn.invocations.is_empty() {
                continue;
            }
            // Where this turn's clock starts. Offsets are measured from here and widths against
            // the conversation-wide `scale`, so bars compare between rows as well as within one.
            let start = turn
                .invocations
                .iter()
                .map(|invocation| invocation.first_seen)
                .min()
                .unwrap_or(turn.sent_at);

            let mut rows = div().flex().flex_col().w_full().min_w_0().gap_1();
            for invocation in &turn.invocations {
                let offset = invocation.first_seen.saturating_sub(start) as f32 / scale;
                let width =
                    invocation.last_seen.saturating_sub(invocation.first_seen) as f32 / scale;
                rows = rows.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .flex_none()
                                .w(px(190.))
                                .min_w_0()
                                // Depth shows a nested delegation for what it is: a specialist
                                // that was called by another specialist, not by the coordinator.
                                .pl(px(12. * depth(&invocation.ns) as f32))
                                .child(
                                    ui::Label::new(invocation.name.clone())
                                        .size(ui::Size::Compact)
                                        .ellipsis(),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_grow()
                                .min_w_0()
                                .h(px(12.))
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .my(px(5.))
                                        .rounded_sm()
                                        .bg(rgb(theme::border())),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .h_full()
                                        .left(relative(offset))
                                        .w(relative(width))
                                        // A single-chunk invocation has a zero-width interval
                                        // and would otherwise be drawn as nothing at all.
                                        .min_w(px(3.))
                                        .rounded_sm()
                                        .bg(rgb(theme::accent())),
                                ),
                        )
                        .child(
                            div().flex_none().w(px(56.)).child(
                                ui::Label::new(duration_label(
                                    invocation.last_seen.saturating_sub(invocation.first_seen),
                                ))
                                .muted()
                                .size(ui::Size::Compact),
                            ),
                        ),
                );
            }

            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(
                        ui::Label::new(format!("{}. {}", index + 1, one_line(&turn.prompt)))
                            .size(ui::Size::Compact)
                            .ellipsis(),
                    )
                    .child(rows),
            );
        }
        body.child(
            ui::Label::new(format!(
                "Every row is drawn to the same scale, where full width is {}. Bars are when \
                 tokens arrived, which is narrower than the work itself — so bars that overlap \
                 certainly ran together, while a gap only suggests one followed the other.",
                duration_label(scale as u64)
            ))
            .muted()
            .size(ui::Size::Compact),
        )
    }

    /// The graph: nodes are kinds, edges are the transitions between them, drawn.
    ///
    /// §73 sketched this as a second stage after a chain of chips, and the chain was built first.
    /// Shown, the verdict was immediate: *"the other image its not a graph."* Fair — a row of
    /// chips is a sentence about the work, and what was asked for is its shape.
    ///
    /// **Laid out vertically**, which is not the obvious choice and is the right one here. The
    /// specialists are named `exploratory_data_analysis` and `academic_researcher`; ten of those
    /// across a 570px modal is 57px each, so a horizontal row would either clip every label or
    /// need text painted into the canvas. A column gives each name the full width, grows to any
    /// number of specialists, and leaves the whole right-hand gutter for the edges.
    ///
    /// **Edges bow further right the further they travel**, so a transition that skips three
    /// nodes cannot be mistaken for one between neighbours and nested arcs stay separable. The
    /// arrowhead carries direction, which is what makes the return edge — the one this feature
    /// exists for — visible as an arc running back *up* the column.
    fn provenance_graph(&self) -> gpui::Div {
        let graph = self.provenance.graph_of(self.provenance_turn);

        // Two sides of one geometry. `canvas` cannot lay out text and a `div` cannot draw a
        // curve, so the nodes are real elements and the edges are painted beside them — which
        // means both have to agree on where a node is. One constant each, used by both.
        const ROW: f32 = 58.;
        const GUTTER: f32 = 236.;
        /// How far an arc bows out per row it skips, so nested arcs stay separable.
        const BOW_PER_ROW: f32 = 50.;

        let height = ROW * graph.nodes.len() as f32;
        let arcs: Vec<EdgeArc> = graph
            .edges
            .iter()
            .map(|edge| EdgeArc {
                from: (edge.from as f32 + 0.5) * ROW,
                to: (edge.to as f32 + 0.5) * ROW,
                span: (edge.to as f32 - edge.from as f32).abs(),
                // Heavier with each traversal, but bounded: a loop walked ten times should read
                // as heavier than one walked twice without becoming a blob.
                weight: match edge.kind {
                    provenance::Edge::Returned => 2.,
                    _ => 1.5,
                } + (edge.count.saturating_sub(1) as f32 * 0.8).min(3.),
                colour: edge_ink(edge.kind),
                // Solid means causal. Everything else is arrival order, and dashes are how a
                // reader is told which is which without reading the legend first.
                dashed: edge.kind != provenance::Edge::Delegated,
            })
            .collect();

        let edges = gpui::canvas(
            |_bounds, _window, _cx| {},
            move |bounds, _prepaint, window, _cx| {
                for arc in arcs {
                    let x = bounds.origin.x + px(4.);
                    let start = gpui::point(x, bounds.origin.y + px(arc.from));
                    let finish = gpui::point(x, bounds.origin.y + px(arc.to));
                    // A quadratic reaches half-way to its control point, so the control sits at
                    // twice the bow the arc should actually show. Bowing in proportion to the
                    // rows skipped is what keeps an arc over three rows outside one over two,
                    // rather than the two crossing where neither can be followed.
                    let bow = (24. + BOW_PER_ROW * (arc.span - 1.).max(0.)).min(GUTTER - 30.) * 2.;
                    let control = gpui::point(x + px(bow), (start.y + finish.y) / 2.);

                    if arc.dashed {
                        paint_dashed_curve(window, start, control, finish, arc.weight, arc.colour);
                    } else {
                        let mut line = gpui::PathBuilder::stroke(px(arc.weight));
                        line.move_to(start);
                        line.curve_to(finish, control);
                        if let Ok(path) = line.build() {
                            window.paint_path(path, gpui::rgb(arc.colour));
                        }
                    }

                    // The arrowhead, pointing the way the curve travels as it lands: for a
                    // quadratic that tangent is `finish - control`. Without it the graph shows
                    // that two specialists are related but not which way the work went, which is
                    // the entire question.
                    let (dx, dy) = (
                        f32::from(finish.x - control.x),
                        f32::from(finish.y - control.y),
                    );
                    let length = (dx * dx + dy * dy).sqrt().max(1.);
                    let (ux, uy) = (dx / length, dy / length);
                    let size = 7. + arc.weight;
                    let back = gpui::point(finish.x - px(ux * size), finish.y - px(uy * size));
                    let (wx, wy) = (uy * size * 0.45, ux * size * 0.45);
                    let mut head = gpui::PathBuilder::fill();
                    head.add_polygon(
                        &[
                            finish,
                            gpui::point(back.x - px(wx), back.y + px(wy)),
                            gpui::point(back.x + px(wx), back.y - px(wy)),
                        ],
                        true,
                    );
                    if let Ok(path) = head.build() {
                        window.paint_path(path, gpui::rgb(arc.colour));
                    }
                }
            },
        );

        // The stage still producing output, by the same rule the road strip uses.
        let running = self
            .streaming
            .then(|| {
                self.provenance
                    .road()
                    .into_iter()
                    .max_by_key(|stage| stage.last_seen)
            })
            .flatten()
            .map(|stage| stage.name);

        let mut column = div().flex().flex_col().flex_grow().min_w_0();
        for node in &graph.nodes {
            let is_running = running.as_deref() == Some(node.name.as_str());
            // `visited twice · 11s, 6s` — the visits and how long each produced output for.
            let mut note = match node.visits {
                1 => String::new(),
                2 => "visited twice".to_string(),
                visits => format!("visited {visits} times"),
            };
            let spans: Vec<String> = node.spans.iter().map(|ms| duration_label(*ms)).collect();
            if !spans.is_empty() {
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(&spans.join(", "));
            }

            column = column.child(
                div()
                    .h(px(ROW))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    // A nested delegation sits under the one that dispatched it.
                    .pl(px(14. * node.depth.min(3) as f32))
                    .child(
                        div()
                            .flex_none()
                            .size(px(11.))
                            .rounded_full()
                            .when(is_running, |dot| {
                                dot.border_2().border_color(rgb(theme::running()))
                            })
                            .when(!is_running, |dot| dot.bg(rgb(theme::accent()))),
                    )
                    .child(
                        // **Full width, so every node ends at the same x.** The edges are
                        // painted in a gutter that begins where this column stops, and the
                        // canvas has no way to ask how wide a name came out. With names at their
                        // natural width the arcs anchored to the gutter's edge and the nodes
                        // stopped wherever their text did — an arc floating in space, attached to
                        // nothing (docs §86). One shared right edge is what makes the two halves
                        // of this drawing agree.
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .min_w_0()
                            .child(
                                ui::Label::new(node.name.replace('_', " "))
                                    .colour(if is_running {
                                        theme::running()
                                    } else {
                                        theme::text()
                                    })
                                    .ellipsis(),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme::text_faint()))
                                    .text_size(px(11.))
                                    .child(note),
                            ),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .h(px(height))
                    .child(column)
                    // The gutter the arcs live in. Fixed, because the bow distances are measured
                    // against it.
                    .child(div().flex_none().w(px(GUTTER)).h(px(height)).child(edges)),
            )
            .child(graph_legend())
            .child(
                ui::Label::new(
                    "A solid line is causal: one specialist delegated to the other, which comes \
                     from the run\u{2019}s own structure and cannot be wrong. A dashed line is \
                     the order things were seen to arrive \u{2014} overlap proves two ran \
                     together, but a gap only suggests one followed the other. A thicker line was \
                     travelled more often.",
                )
                .muted()
                .size(ui::Size::Compact),
            )
    }

    fn preview_modal(&self, output: workspace::Output, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .id("preview-body")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_grow()
            .overflow_y_scroll()
            .p_3()
            .gap_2();

        match output.kind {
            workspace::Kind::Figure => {
                body = body.child(
                    img(output.path.clone())
                        .max_w_full()
                        .object_fit(gpui::ObjectFit::Contain),
                );
            }
            _ => {
                // Read a bounded prefix. A 200 MB CSV would otherwise be pulled into
                // memory and laid out as one paragraph, on the UI thread.
                match workspace::head(&output.path, 400) {
                    Ok(text) if output.name.ends_with(".md") => {
                        for parsed in markdown::parse(&text) {
                            body = body.child(markdown_block(&parsed, None));
                        }
                    }
                    Ok(text) if is_delimited(&output.name) => {
                        // Rainbow columns, the trick the `rainbow-csv` editor extensions
                        // use: colour by column index so the eye can follow one field
                        // down the rows. Without column *layout* — which GPUI 0.2.2 does
                        // not have — colour is the only thing that makes a wide CSV
                        // readable at all (docs §50).
                        let delimiter = if output.name.ends_with(".tsv") {
                            '\t'
                        } else {
                            ','
                        };
                        for (row, line) in text.lines().enumerate() {
                            let mut cells = div().flex().flex_row().flex_wrap().w_full().gap_2();
                            for (column, cell) in line.split(delimiter).enumerate() {
                                cells = cells.child(
                                    div()
                                        .flex_none()
                                        .text_color(rgb(column_colour(column)))
                                        // The header row is what you read first.
                                        .when(row == 0, |cell| cell.font_weight(FontWeight::BOLD))
                                        .text_xs()
                                        .child(cell.trim().to_string()),
                                );
                            }
                            body = body.child(cells);
                        }
                    }
                    Ok(text) => {
                        for line in text.lines() {
                            body = body.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .child(line.to_string()),
                            );
                        }
                    }
                    Err(error) => {
                        body = body.child(
                            div()
                                .text_color(rgb(theme::error()))
                                .text_xs()
                                .child(format!("{error:#}")),
                        );
                    }
                }
            }
        }

        let opened = output.path.clone();
        div()
            .id("preview-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            // The dim is the affordance: it says the workbench is still there and that
            // clicking away comes back to it.
            .bg(if theme::is_light(&theme::current()) {
                gpui::rgba(0x33333366)
            } else {
                gpui::rgba(0x00000099)
            })
            .child(
                div()
                    .id("preview")
                    .flex()
                    .flex_col()
                    .w(px(760.))
                    .max_h(px(620.))
                    .bg(rgb(theme::overlay()))
                    .border_1()
                    .border_color(rgb(theme::border_strong()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .flex_none()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(theme::border()))
                            .child(
                                div()
                                    .flex_grow()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(rgb(theme::text()))
                                    .text_sm()
                                    .child(output.name.clone()),
                            )
                            .child(
                                div()
                                    .id("preview-open")
                                    .rounded_md()
                                    .flex_none()
                                    .px_2()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .hover(|style| {
                                        style.text_color(rgb(theme::accent())).cursor_pointer()
                                    })
                                    .child("open outside")
                                    .on_click(move |_event, _window, _cx| {
                                        if let Err(error) = workspace::open(&opened) {
                                            tracing::warn!(%error, "could not open a file");
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .id("preview-close")
                                    .rounded_md()
                                    .flex_none()
                                    .px_2()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .hover(|style| {
                                        style.text_color(rgb(theme::accent())).cursor_pointer()
                                    })
                                    .child("✕")
                                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                                        workbench.preview = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(body),
            )
            .on_click(cx.listener(|workbench, _event, _window, cx| {
                // Clicking the dimmed backdrop closes it, the way every modal does.
                workbench.preview = None;
                cx.notify();
            }))
    }

    /// A file a turn produced, in the transcript, under the answer that produced it.
    ///
    /// **Why here and not only in the panel.** A produced file used to appear as a name and a
    /// size in a 330px column on the far side of the window. So the answer would say "I cleaned
    /// the dataset and removed 14 duplicate plots", and whether that dataset now had 1,204 rows
    /// or 40 was a separate trip to a separate place — which is exactly the check a researcher
    /// should be able to make without leaving the sentence that prompted it.
    ///
    /// A table shows its first rows, a figure shows itself, anything else shows its header. All
    /// three open the existing preview modal on click.
    fn output_card(
        &self,
        key: usize,
        output: &workspace::Output,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        /// Enough to see the shape of the table without becoming a table.
        const PREVIEW_ROWS: usize = 4;
        /// Past this the cells are too narrow to read in a chat pane.
        const PREVIEW_COLUMNS: usize = 4;

        let (glyph, ink) = file_mark(&output.path);
        let shape = self.shape_of(output);
        let opened = output.path.clone();
        let revealed = output.path.parent().map(std::path::Path::to_path_buf);
        let previewed = output.clone();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(ink))
                    .text_size(px(13.))
                    .child(glyph),
            )
            .child(
                ui::Label::new(output.name.clone())
                    .size(ui::Size::Compact)
                    .ellipsis(),
            )
            .child(
                div()
                    .flex_grow()
                    .min_w_0()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(12.))
                    .child(shape.describe(output.bytes)),
            )
            .child(
                div()
                    .id(("open-output", key))
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_size(px(12.))
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                    .child("Open ⧉")
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&opened) {
                            tracing::warn!(%error, "could not open an output");
                        }
                    }),
            )
            .children(revealed.map(|folder| {
                div()
                    .id(("reveal-output", key))
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_size(px(12.))
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                    .child("Reveal")
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&folder) {
                            tracing::warn!(%error, "could not open an output's folder");
                        }
                    })
            }));

        let mut card = div()
            .id(("output-card", key))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(theme::border()))
            // One step off the transcript's own background, whichever way the palette runs.
            .bg(rgb(if theme::is_light(&theme::current()) {
                theme::elevated()
            } else {
                theme::surface()
            }))
            .hover(|style| style.border_color(rgb(theme::border_strong())).cursor_pointer())
            .child(header);

        match output.kind {
            workspace::Kind::Figure => {
                card = card.child(
                    div().p_2().child(
                        // Capped, not scaled to the pane: a 2000px figure would otherwise push
                        // the transcript's width around as it loads.
                        img(output.path.clone())
                            .max_w_full()
                            .max_h(px(420.))
                            .object_fit(gpui::ObjectFit::Contain),
                    ),
                );
            }
            _ => {
                if let Some(rows) = self.preview_of(output, PREVIEW_ROWS) {
                    let columns = rows
                        .iter()
                        .map(Vec::len)
                        .max()
                        .unwrap_or(0)
                        .min(PREVIEW_COLUMNS);
                    let mut table = div().flex().flex_col().w_full().min_w_0().px_3().py_2();
                    for (at, record) in rows.iter().enumerate() {
                        let heading = at == 0;
                        let mut line = div()
                            .flex()
                            .flex_row()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .py_1()
                            .when(heading, |line| {
                                line.border_b_1().border_color(rgb(theme::border()))
                            });
                        for cell in record.iter().take(columns) {
                            line = line.child(
                                div()
                                    .flex_grow()
                                    .flex_basis(relative(1. / columns as f32))
                                    .min_w_0()
                                    .text_color(rgb(if heading {
                                        theme::text_faint()
                                    } else {
                                        theme::text_muted()
                                    }))
                                    .text_size(px(12.))
                                    .overflow_hidden()
                                    .child(cell.clone()),
                            );
                        }
                        // Said, not silently dropped: a table shown four columns wide when it
                        // has eleven is a table someone will read as complete.
                        if record.len() > columns {
                            line = line.child(
                                div()
                                    .flex_none()
                                    .text_color(rgb(theme::text_faint()))
                                    .text_size(px(12.))
                                    .child(format!("+{}", record.len() - columns)),
                            );
                        }
                        table = table.child(line);
                    }
                    card = card.child(table);

                    if let workspace::Shape::Table { rows: total, .. } = shape {
                        card = card.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .px_3()
                                .pb_2()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(12.))
                                .child(format!(
                                    "first {} of {} rows · click to open the whole table",
                                    rows.len().saturating_sub(1),
                                    workspace::thousands(total)
                                )),
                        );
                    }
                }
            }
        }

        card.on_click(cx.listener(move |workbench, _event, _window, cx| {
            workbench.preview = Some(previewed.clone());
            cx.notify();
        }))
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

    /// Who was consulted for this answer, how long it took, how many steps.
    ///
    /// The path reads `academic_researcher → theorizer → data_analysis · 19s · 4 steps`, which is
    /// the summary people were expanding the trace to reconstruct.
    fn answer_chips(&self, index: usize, message: &Message) -> impl IntoElement {
        /// Past this the row wraps into a paragraph and stops being a glance.
        const MAX_PILLS: usize = 6;

        let path = consulted(&message.agents);
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .w_full()
            .min_w_0();

        for (at, name) in path.iter().take(MAX_PILLS).enumerate() {
            if at > 0 {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_muted()))
                        .text_size(px(11.))
                        .child("→"),
                );
            }
            row = row.child(
                div()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .rounded_full()
                    .bg(rgb(theme::elevated()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .text_color(rgb(specialist_ink(name).unwrap_or(theme::text_muted())))
                    .text_size(px(11.))
                    .child(name.replace('_', " ")),
            );
        }
        if path.len() > MAX_PILLS {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(format!("+{}", path.len() - MAX_PILLS)),
            );
        }

        // Steps across the whole turn: the coordinator's own, plus every specialist's.
        let steps: usize = message.steps.len()
            + message
                .agents
                .iter()
                .map(|agent| agent.steps.len())
                .sum::<usize>();
        let mut note = String::new();
        if let Some(turn) = self.turn_for(index) {
            let span = turn
                .invocations
                .iter()
                .map(|invocation| invocation.last_seen)
                .max()
                .unwrap_or(turn.sent_at)
                .saturating_sub(turn.sent_at);
            if span >= 1_000 {
                note.push_str(&format!(" · {}", duration_label(span)));
            }
        }
        if steps > 0 {
            note.push_str(&format!(" · {steps} steps"));
        }
        if !note.is_empty() {
            row = row.child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(note),
            );
        }
        row
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

    /// What to do with a finished answer.
    fn export_row(&self, message: &Message, cx: &mut Context<Self>) -> impl IntoElement {
        let again = self
            .transcript
            .iter()
            .rev()
            .find(|earlier| earlier.role == "you")
            .map(|earlier| earlier.body.clone());
        let bibtex = bibliography(&self.sources);
        let answer = message.body.clone();

        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .pt_1()
            .child(
                ui::Button::new("export-pdf", "Save as PDF with references")
                    .tone(ui::Tone::Accent)
                    .size(ui::Size::Compact)
                    // Disabled rather than hidden, so the affordance is discoverable before
                    // there is a report to use it on.
                    .disabled(self.reports.is_empty())
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.render_report(cx);
                    })),
            )
            .child(
                ui::Button::new("export-bibtex", "Copy BibTeX")
                    .size(ui::Size::Compact)
                    .disabled(bibtex.is_empty())
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        let entries = bibtex.matches("@misc").count();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(bibtex.clone()));
                        workbench.say(format!("{entries} references copied as BibTeX"), cx);
                    })),
            )
            .child(
                ui::Button::new("export-rerun", "Re-run this turn")
                    .size(ui::Size::Compact)
                    .disabled(again.is_none() || self.streaming)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        // Into the composer, not straight to the backend. Re-running is a
                        // decision, and a question worth asking twice is usually worth editing
                        // first — the same rule every other suggestion here follows.
                        if let Some(prompt) = again.clone() {
                            workbench
                                .composer
                                .update(cx, |composer, cx| composer.set_text(prompt, cx));
                            workbench.restore_focus = true;
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(format!("{} words", answer.split_whitespace().count())),
            )
    }

    /// What an empty transcript says.
    ///
    /// It used to say one grey sentence. The replacement answers the two questions a researcher
    /// actually opens this window with — *where was I* and *what can this thing do* — using what
    /// the app already knows: their own recent conversations, and three things it is genuinely
    /// good at.
    ///
    /// **Nothing here runs anything.** Every starting move loads the composer and stops, which is
    /// the rule the project suggestions already follow and is org policy besides: the human
    /// decides what is asked.
    fn empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        /// Three, because a row of them has to stay readable in a narrow pane, and because a
        /// list of recent work long enough to scroll is the sidebar's job.
        const RECENT: usize = 3;

        let now = provenance::now_ms() as i64 / 1_000;

        let mut block = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            // Centred vertically: with nothing in the transcript there is no reading order to
            // preserve, and a page of prose pinned to the top of a tall window reads as a header.
            .justify_center()
            .gap_6()
            .px(px(60.))
            .py(px(34.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_color(rgb(theme::text()))
                            .text_size(px(22.))
                            .line_height(px(29.))
                            .child("What are you working on?"),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::text_muted()))
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .child(
                                "Ask below, or drop a file on this window. Everything a turn \
                                 produces is saved into your Documents folder.",
                            ),
                    ),
            );

        // Where they left off. Only conversations that have actually been used — a list whose
        // first card is an empty thread from a mis-click is a list nobody trusts.
        let recent: Vec<&protocol::Conversation> = self
            .conversations
            .iter()
            .filter(|conversation| Some(&conversation.thread_id) != self.sidecar.thread_id().as_ref())
            .take(RECENT)
            .collect();
        if !recent.is_empty() {
            let mut cards = div().flex().flex_row().gap_2().w_full().min_w_0();
            for conversation in recent {
                let thread_id = conversation.thread_id.clone();
                // What is in it, counted off disk rather than remembered — the same source the
                // research panel reads, so the two cannot disagree.
                let outputs: usize = workspace::outputs(&workspace::thread_dir_in(
                    conversation.project.as_deref(),
                    &conversation.thread_id,
                ))
                .iter()
                .map(|(_, items)| items.len())
                .sum();
                let when = protocol::how_long_ago(&conversation.updated_at, now);
                let note = match (outputs, when.is_empty()) {
                    (0, true) => String::new(),
                    (0, false) => when,
                    (1, true) => "1 output".to_string(),
                    (1, false) => format!("1 output · {when}"),
                    (many, true) => format!("{many} outputs"),
                    (many, false) => format!("{many} outputs · {when}"),
                };
                cards = cards.child(
                    div()
                        .id(SharedString::from(format!("resume-{thread_id}")))
                        .flex()
                        .flex_col()
                        .gap_1()
                        // Equal thirds, and `min_w_0` so a long title ellipsises instead of
                        // widening its own card past the other two.
                        .flex_grow()
                        .flex_basis(relative(0.33))
                        .min_w_0()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(theme::elevated()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .hover(|style| {
                            style
                                .border_color(rgb(theme::accent()))
                                .cursor_pointer()
                        })
                        .child(
                            ui::Label::new(conversation.title.clone())
                                .size(ui::Size::Compact)
                                .ellipsis(),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(11.))
                                .child(note),
                        )
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            workbench.open_conversation(thread_id.clone(), cx);
                        })),
                );
            }
            block = block.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(section_label("PICK UP WHERE YOU LEFT OFF"))
                    .child(cards),
            );
        }

        // Three things this is good at, in the researcher's words. Deliberately not a feature
        // list: each one is a sentence they could have typed themselves, and clicking it puts
        // exactly that in the composer for them to edit.
        const MOVES: [(&str, &str, &str); 3] = [
            (
                "◎",
                "Find datasets in CIP Dataverse on a topic",
                "Search CIP Dataverse for datasets about ",
            ),
            (
                "▤",
                "Summarise what the literature says, with references",
                "Summarise what the literature says about , with references.",
            ),
            (
                "▩",
                "Clean and profile a file I drop here",
                "Clean and profile the file I am about to drop, and tell me what is in it.",
            ),
        ];
        let mut moves = div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .min_w_0()
            .child(section_label("OR START SOMETHING"));
        for (at, (glyph, label, prompt)) in MOVES.into_iter().enumerate() {
            let leading = at == 0;
            moves = moves.child(
                div()
                    .id(SharedString::from(format!("start-{at}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    // The first is marked, not louder: one suggestion carrying the accent is a
                    // recommendation, three would be a menu shouting.
                    .when(leading, |row| {
                        row.bg(rgb(theme::accent_soft()))
                            .border_color(rgb(theme::accent()))
                    })
                    .when(!leading, |row| row.border_color(rgb(theme::border())))
                    .hover(|style| style.bg(rgb(theme::elevated())).cursor_pointer())
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::accent()))
                            .text_size(px(13.))
                            .child(glyph),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(theme::text()))
                            .text_size(px(13.))
                            .child(label),
                    )
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.composer.update(cx, |composer, cx| {
                            composer.set_text(prompt, cx);
                        });
                        // Focused, because the prompt is a stem they have to finish.
                        window.focus(&workbench.composer.focus_handle(cx));
                        cx.notify();
                    })),
            );
        }
        block.child(moves)
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

    /// The road: where this enquiry has been, down the left edge of the chat.
    ///
    /// **Why a strip and not the modal.** The provenance modal has held this since §75 and it is
    /// the wrong place for the question people actually ask, which is *where am I* — a question
    /// you have while the turn is running and will not interrupt it to open a window for. The
    /// modal answers *what happened*, afterwards, in detail. This answers the live one, and costs
    /// 172px to do it.
    ///
    /// Fed from [`provenance::Record`], which is already written on every frame that carries an
    /// agent ([`Self::note_provenance`]) — so nothing new is collected, something already
    /// collected is finally shown while it still matters.
    fn road_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const OPEN: f32 = 172.;
        const FOLDED: f32 = 38.;
        /// The dot's own size, and the gutter it is centred in.
        const DOT: f32 = 9.;
        const GUTTER: f32 = 12.;

        let stages = self.provenance.road();
        // The stage still producing output. Only meaningful while a turn is in flight: after it
        // ends, every stage has been seen and none is running. The *strongest true statement*
        // available — we know which invocation spoke most recently, and nothing else (§74).
        let running = self
            .streaming
            .then(|| stages.iter().max_by_key(|stage| stage.last_seen))
            .flatten()
            .map(|stage| stage.name.clone());

        let mut strip = div()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .w(px(if self.road_open { OPEN } else { FOLDED }))
            .pt(px(18.))
            .pb(px(14.))
            .when(self.road_open, |strip| strip.px(px(14.)).gap_3())
            .when(!self.road_open, |strip| strip.items_center().gap_2())
            // One step up from the pane's `background`, which is what makes it read as a rail
            // rather than as the transcript with something in the margin.
            .bg(rgb(theme::surface()))
            .border_r_1()
            .border_color(rgb(theme::border()));

        // Header: the name, and the chevron that folds it. Folded, the chevron is the whole
        // header — there is no room for a word and no need for one.
        strip = strip.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .flex_none()
                .when(self.road_open, |header| {
                    header.child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .text_size(px(11.))
                            .child("THE ROAD"),
                    )
                })
                .child(
                    div()
                        .id("fold-road")
                        .flex_none()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(12.))
                        .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                        .child(if self.road_open { "‹" } else { "›" })
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.toggle_road(cx);
                        })),
                ),
        );

        if stages.is_empty() {
            // Folded, an explanation would not fit and the empty gutter says it anyway.
            if self.road_open {
                strip = strip.child(
                    div()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .child("The specialists this enquiry consults appear here as it reaches them."),
                );
            }
            return strip;
        }

        let mut body = div().flex().flex_col().flex_grow().min_h_0().w_full();
        let last = stages.len().saturating_sub(1);
        for (at, stage) in stages.iter().enumerate() {
            let is_running = running.as_deref() == Some(stage.name.as_str());

            // The dot, and the connector that continues down to the next one. Both live in a
            // fixed-width gutter so every label starts on the same x whatever the dot is doing.
            let gutter = div()
                .flex()
                .flex_col()
                .items_center()
                .flex_none()
                .w(px(GUTTER))
                .child(
                    div()
                        .flex_none()
                        .size(px(DOT))
                        .rounded_full()
                        // Filled when it has been, ringed while it is. A ring is a shape that
                        // has not closed, which is the state it stands for.
                        .when(is_running, |dot| {
                            dot.border_2().border_color(rgb(theme::running()))
                        })
                        .when(!is_running, |dot| dot.bg(rgb(theme::accent()))),
                )
                .when(at < last, |gutter| {
                    gutter.child(
                        div()
                            .flex_grow()
                            .w(px(2.))
                            .min_h(px(14.))
                            .bg(rgb(theme::border_strong())),
                    )
                });

            let mut row = div()
                .flex()
                .flex_row()
                .items_start()
                .w_full()
                .min_w_0()
                .when(at < last, |row| row.min_h(px(46.)))
                .child(gutter);

            if self.road_open {
                // `visited twice · 11s` — the count and how long it was producing. Not
                // `6 found · Asta`: nothing on this side knows how many results a specialist
                // returned, or which of them Asta served.
                let note = if is_running {
                    format!("running · {}", duration_label(stage.busy_ms))
                } else if stage.visits > 1 {
                    format!(
                        "visited {} times · {}",
                        stage.visits,
                        duration_label(stage.busy_ms)
                    )
                } else {
                    duration_label(stage.busy_ms)
                };
                row = row.child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_grow()
                        .min_w_0()
                        .pl_2()
                        // Pulls the label's cap-height level with the dot beside it.
                        .mt(px(-3.))
                        .child(
                            ui::Label::new(stage.name.replace('_', " "))
                                .colour(if is_running { theme::running() } else { theme::text() })
                                .ellipsis(),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(11.))
                                .child(note),
                        ),
                );
            }
            body = body.child(row);
        }
        strip = strip.child(body);

        // Pinned to the bottom by the body's `flex_grow` above it.
        if self.road_open {
            strip = strip
                .child(
                    div().w_full().flex_none().child(
                        ui::Button::new("road-full-graph", "Full graph")
                            .tone(ui::Tone::Accent)
                            .size(ui::Size::Compact)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.provenance_view = ProvenanceView::Graph;
                                workbench.provenance_open = true;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .child("Written beside this conversation's files, so it survives a reload."),
                );
        }
        strip
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

    fn chat_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // `min_w_0` is what makes long assistant text *wrap* instead of running off
        // the right edge: a flex item defaults to min-width:auto, so its content
        // width becomes its floor and a long paragraph widens the pane instead of
        // flowing down.
        // `id` + `overflow_y_scroll` is what lets a long transcript scroll; GPUI
        // keeps the scroll offset keyed on that id across re-renders.
        // Last frame's span rectangles go now, before this frame registers its own: the
        // transcript moves under a scroll, a resize and every streamed token, and a highlight
        // painted from stale bounds is a highlight over the wrong words.
        self.text_selection.begin_frame();
        let mut col = div()
            .id("transcript")
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .overflow_y_scroll()
            .track_scroll(&self.transcript_scroll)
            .p_4()
            .gap_3()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.clear());
                    if let Some(spot) = workbench.text_selection.spot_at(event.position) {
                        workbench
                            .text_selection
                            .update(|selection| selection.begin(spot));
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(|workbench, event: &gpui::MouseMoveEvent, _window, cx| {
                    if !workbench.text_selection.selection().dragging() {
                        return;
                    }
                    if let Some(spot) = workbench.text_selection.spot_at(event.position) {
                        workbench
                            .text_selection
                            .update(|selection| selection.extend(spot));
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.finish());
                    cx.notify();
                }),
            )
            // Releasing outside the transcript has to end the drag too, or the selection
            // keeps following the pointer after the button is long since up.
            .on_mouse_up_out(
                gpui::MouseButton::Left,
                cx.listener(|workbench, _event: &gpui::MouseUpEvent, _window, cx| {
                    workbench
                        .text_selection
                        .update(|selection| selection.finish());
                    cx.notify();
                }),
            )
            // Deliberately leaves the selection alone: right-clicking a shade off the text
            // you just highlighted, in order to copy it, must not be what throws it away.
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Transcript, cx);
                }),
            );

        if self.transcript.is_empty() {
            col = col.child(self.empty_state(cx));
        }
        for (index, message) in self.transcript.iter().enumerate() {
            let asked = message.role == "you";
            let has_activity = !message.steps.is_empty() || !message.agents.is_empty();
            // An empty assistant body means we're still waiting on the first token —
            // unless a trace is already showing what's going on, which says more.
            // A placeholder while the first token is still coming, and *only* then — it is
            // not part of the body, so it is not parsed and never reaches the cache.
            let waiting = message.body.is_empty() && self.streaming && !has_activity;
            let body = message.body.clone();
            // **Side carries the role, so no label does.** Asked for by name after a side-by-side
            // with a chat client: questions ride right in a bubble, answers run full width on the
            // left as plain prose. The shape is doing the work a `you` / `mini-me` caption used to
            // do, and two signals for one fact is one more than the eye needs (docs §86).
            let mut block = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .when(asked, |block| block.items_end());
            // A one-line summary of the work, above the answer it produced. The collapsible
            // trace stays underneath for anyone who wants the detail — this replaces nothing,
            // it just means the common question ("who did this, and how long did it take")
            // no longer requires expanding anything.
            if !asked && !message.agents.is_empty() {
                block = block.child(self.answer_chips(index, message));
            }
            // The trace goes *above* the answer, because that is the order it
            // happened in and because the answer should be the last thing read.
            if has_activity {
                block = block.child(self.activity_block(index, message, cx));
            }
            if waiting {
                block = block.child(div().text_color(rgb(theme::text_muted())).child("…"));
            }
            if !body.is_empty() {
                // The user's own text is shown as typed — they wrote it, and reinterpreting
                // their asterisks would be presumptuous. Assistant text is Markdown.
                if asked {
                    block = block.child(
                        div()
                            // Capped rather than full width: a bubble that reaches both edges is
                            // not a bubble, and the ragged left edge is what makes a glance down
                            // the transcript separate questions from answers.
                            .max_w(relative(0.78))
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(rgb(theme::surface()))
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .text_color(rgb(theme::text()))
                            // Shown as typed — they wrote it, and reinterpreting their asterisks
                            // would be presumptuous (docs §14).
                            .child(selection::Selectable::new(
                                &self.text_selection,
                                body.clone(),
                                StyledText::new(body),
                            )),
                    );
                } else {
                    let mut rendered = div().flex().flex_col().w_full().min_w_0().gap_2();
                    // Parsed when the text arrived, not now. See `Message::blocks`.
                    for parsed in &message.blocks {
                        rendered =
                            rendered.child(markdown_block(parsed, Some(&self.text_selection)));
                    }
                    block = block.child(rendered);
                }
            }
            // Marked, not hidden. A truncated answer looks exactly like a finished one, and
            // whether it was cut off decides whether it can be relied on (docs §63).
            if message.stopped {
                block = block.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::warning()))
                        .text_xs()
                        .child("— you stopped this turn; the answer above is incomplete"),
                );
            }
            // Files last: the answer explains them, so it should be read first.
            for (at, output) in message.outputs.iter().enumerate() {
                block = block.child(self.output_card(index * 64 + at, output, cx));
            }
            // What to do with the answer, under the answer. All three exist already — the first
            // is a palette command, and a command nobody knows the name of is a feature nobody
            // has. Only under the *last* completed one: three buttons after every answer in a
            // twelve-turn conversation is a wall of chrome, and it is the latest answer a person
            // exports.
            if !asked
                && !message.body.is_empty()
                && index + 1 == self.transcript.len()
                && !self.streaming
            {
                block = block.child(self.export_row(message, cx));
            }
            col = col.child(block);
        }

        // What the turn is doing, kept at the bottom of the transcript while it runs.
        //
        // The trace still sits above the answer it produced — that is the order it happened in and
        // it stays with its own message. But during a two-minute delegation the trace scrolls up
        // out of view behind the streaming answer, and the one question a person has while waiting
        // is "is this still going". So the live line is pinned under the last message instead of
        // being hunted for inside it.
        if self.streaming {
            let elapsed = self
                .provenance
                .turns
                .last()
                .map(|turn| provenance::now_ms().saturating_sub(turn.sent_at))
                .filter(|elapsed| *elapsed >= 1_000)
                .map(|elapsed| format!(" · {}", duration_label(elapsed)))
                .unwrap_or_default();
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(format!("{}{elapsed}", self.status)),
            );
        }

        // Everything that is not the road: transcript, approval, picker, composer. Built as its
        // own column so the road can sit *beside* all of it rather than above the transcript
        // and below the composer.
        let mut column = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .h_full()
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .min_w_0()
                    .overflow_hidden()
                    .child(col)
                    .children(scrollbar(&self.transcript_scroll)),
            );
        // Above the composer, so the decision sits where the user's attention already
        // is and cannot be scrolled out of view.
        if let Some(request) = &self.pending_approval {
            column = column.child(self.approval_card(request, cx));
        }
        let column = column
            .children(self.subagent_picker(cx))
            .child(self.composer_row(cx));

        div()
            .flex()
            // A row now: the road, then everything else.
            .flex_row()
            .flex_grow()
            .min_w_0()
            .h_full()
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            // Not before the first question. An empty road beside an empty transcript is a
            // frame around nothing, and the empty state has its own things to say.
            .when(!self.transcript.is_empty(), |pane| {
                pane.child(self.road_strip(cx))
            })
            .child(column)
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
        self.status = if approve {
            "approved — running…"
        } else {
            "rejected"
        }
        .into();

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
            .m_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme::accent()))
            .bg(rgb(theme::surface()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        // The one heading in the app that keeps the accent. It is not a label
                        // for a surface, it is the question — and the thing being asked about
                        // is whether to run code on the researcher's own machine.
                        div()
                            .flex_none()
                            .text_color(rgb(theme::accent()))
                            .text_size(px(11.))
                            .child("RUN THIS ON YOUR MACHINE?"),
                    )
                    .child(
                        // **The tool, not the specialist.** The design names the subagent that
                        // asked; nothing in `ApprovalRequest` carries one. It could be inferred
                        // from whichever specialist spoke most recently — very likely right, and
                        // an inference stated as fact beside a security decision, which is the
                        // one place in this app that must not happen. The tool name is exact.
                        div()
                            .flex_none()
                            .text_color(rgb(theme::text_faint()))
                            .text_size(px(11.))
                            .child(match request.actions.len() {
                                0 | 1 => request
                                    .actions
                                    .first()
                                    .map(|action| action.tool.clone())
                                    .unwrap_or_default(),
                                many => format!("{many} commands"),
                            }),
                    ),
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
                        .text_color(rgb(theme::text_muted()))
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
                    .rounded_md()
                    // Sunk, not raised: the card is `surface`, so the command sitting on
                    // `background` reads as a thing quoted inside it.
                    .bg(rgb(theme::background()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .text_color(rgb(theme::text()))
                    // Monospaced, which is not decoration on this element. This is the text a
                    // researcher is being asked to actually review, and a proportional font hides
                    // the differences that matter in a shell command — spacing, `l` against `1`,
                    // where a quote opens and closes.
                    .font(ui::code_font())
                    .text_size(px(12.5))
                    .line_height(px(19.))
                    .child(action.detail.clone()),
            );
        }

        // What is knowable about the effect, and nothing more. The design's line reads "Reads 1
        // file, writes 1 file, in …" — which would mean deciding what an arbitrary shell command
        // touches, by reading it. A wrong "reads 1 file" beside a command that deletes a
        // directory is worse than no line, and this is the gate that exists because the agent's
        // `execute` runs with the researcher's own permissions.
        let effect = match self.thread_workspace() {
            Some(dir) => format!(
                "Runs on {} with your permissions, in {}.",
                self.sidecar.execution(),
                dir.display()
            ),
            None => format!(
                "Runs on {} with your permissions.",
                self.sidecar.execution()
            ),
        };

        card.child(commands)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(effect),
            )
            .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .w_full()
                .min_w_0()
                .child(
                    ui::Button::new("approve", "Approve")
                        .tone(ui::Tone::Accent)
                        .on_click(
                            cx.listener(|workbench, _event, _window, cx| {
                                workbench.decide(true, cx)
                            }),
                        ),
                )
                .child(ui::Button::new("reject", "Reject").on_click(
                    cx.listener(|workbench, _event, _window, cx| workbench.decide(false, cx)),
                ))
                // Bounded to *this turn*, and nothing is persisted. A permanent
                // "always allow" is how a security gate becomes a habit: the tenth
                // identical dialog in one analysis is not read, it is dismissed, and
                // then neither is the eleventh — which is the one that mattered.
                // Approving the rest of one task is a decision someone can actually
                // hold in their head, and it expires on its own.
                // Both grants pushed right and set `Compact`, so the row reads as two decisions
                // about *this* command and two ways to stop being asked. The design shows only
                // the wider one; neither is dropped, because the narrower grant is the safer
                // habit and removing it would leave "approve everything" as the only way out of
                // clicking — which is how a gate becomes a formality.
                .child(div().flex_grow())
                .child(
                    ui::Button::new("approve-turn", "Approve the rest of this turn")
                        .size(ui::Size::Compact)
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
                    ui::Button::new(
                        "approve-conversation",
                        "Approve everything in this conversation",
                    )
                    .size(ui::Size::Compact)
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
        if self.preview.take().is_some() {
            cx.notify();
            return;
        }
        if self.renaming.take().is_some() {
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

    /// The buttons for the Setup page. Re-check is its Save.
    fn setup_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        ui::actions()
            .child(
                ui::Button::new(
                    "recheck",
                    if self.checking {
                        "Checking…"
                    } else {
                        "Re-check"
                    },
                )
                .tone(ui::Tone::Accent)
                .on_click(
                    cx.listener(|workbench, _event, _window, cx| workbench.run_preflight(cx)),
                ),
            )
            // Beside Re-check because this is where someone comes when something is wrong,
            // and "restart it" is the second thing anyone tries after "check again".
            .child(
                ui::Button::new("restart-backend", "Restart backend").on_click(
                    cx.listener(|workbench, _event, _window, cx| workbench.restart_backend(cx)),
                ),
            )
            .child(
                ui::Button::new("close-setup", "Close").on_click(cx.listener(
                    |workbench, _event, _window, cx| {
                        workbench.settings_open = false;
                        workbench.restore_focus = true;
                        cx.notify();
                    },
                )),
            )
    }

    fn settings_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let provider = settings::provider(&self.draft.provider);
        let needs_base_url = provider.is_some_and(|p| p.needs_base_url);
        let key_name = self.draft.key_name();

        // A centred modal, not a column. As a column it took 420px off the chat for as
        // long as it was open, and settings are something you visit and leave — the same
        // argument that makes Zed's fifty pickers modal rather than panels (docs §51).
        let section = self.settings_section;
        // Setup is a page like any other, and brings its own content.
        if section == Section::Setup {
            return self.preferences_window(self.setup_pane(cx), self.setup_actions(cx), cx);
        }

        let mut pane = div().flex().flex_col().w_full().min_w_0().gap_3();
        if section == Section::Appearance {
            // The list has not changed — it moved into a popup and gained a trigger. Zed puts
            // every choice behind one, and the reason shows the moment there is more than a
            // handful: a hundred installed palettes is a hundred rows in a window with four
            // other settings in it. Hovering a row still previews it (§50); you just have to
            // open the list first.
            pane = pane.child(ui::setting_row(
                "Theme",
                "The palette the whole window uses. Hovering a row previews it.",
                ui::Dropdown::new("pick-theme", self.applied_theme.clone())
                    .open(matches!(self.open_picker, Some((Picker::Theme, _))))
                    .on_click(
                        cx.listener(|workbench, event: &gpui::ClickEvent, _window, cx| {
                            workbench.toggle_picker(Picker::Theme, event.position(), cx);
                        }),
                    ),
            ));
        }
        if section == Section::Model {
            let current = self.field_text_or(Field::ModelId, &self.draft.model_id, cx);
            pane = pane.child(self.provider_row(cx)).child(ui::setting_row(
                "Model",
                "Which model answers. Any id can be typed in the field below.",
                ui::Dropdown::new("pick-model", current)
                    .open(matches!(self.open_picker, Some((Picker::Model, _))))
                    .on_click(
                        cx.listener(|workbench, event: &gpui::ClickEvent, _window, cx| {
                            workbench.toggle_picker(Picker::Model, event.position(), cx);
                        }),
                    ),
            ));
            pane = pane.child(self.subagent_models(cx));
        }

        for (tab, (field, composer)) in self
            .fields
            .iter()
            .filter(|(field, _)| field.section() == section)
            .enumerate()
        {
            if *field == Field::BaseUrl && !needs_base_url {
                continue;
            }
            let status = if field.is_secret() {
                let name = field
                    .secret_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| key_name.clone());
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
                            .text_color(rgb(theme::text_muted()))
                            .text_xs()
                            .child(format!("{}{status}", field.label())),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .track_focus(&composer.focus_handle(cx))
                            // Tab walks the fields of the page in the order they are read.
                            // `track_focus` is what makes landing on this box mean landing in
                            // the field inside it, rather than on a div that swallows typing.
                            .tab_index(tab as isize)
                            .in_focus(|style| style.border_color(rgb(theme::accent())))
                            .child(composer.clone()),
                    ),
            );
        }

        // Each setting says what it does. Half of these are things a researcher has no reason
        // to have an opinion about until someone tells them — "Run code on this machine" is a
        // sentence about trust, not a preference, and the name alone never said so.
        for (label, description, value, toggle) in if section == Section::Backend {
            vec![
                (
                    "Run code on this machine",
                    "Commands run in your own WSL distro rather than a remote sandbox.",
                    self.draft.local_execution,
                    0usize,
                ),
                (
                    "Ask before every command",
                    "Pause and show each command, so nothing runs without you seeing it.",
                    self.draft.approve_execute,
                    1,
                ),
                // Preview API, and it needs the generated graph config — so opt-in, and
                // labelled by what it does rather than by what it is called upstream.
                (
                    "Let work run in the background",
                    "Long jobs keep going while you carry on asking questions.",
                    self.draft.async_subagents,
                    2,
                ),
            ]
        } else {
            Vec::new()
        } {
            pane = pane.child(ui::setting_row(
                label,
                description,
                ui::Toggle::new(SharedString::from(format!("toggle-{toggle}")), value).on_click(
                    cx.listener(move |workbench, _event, _window, cx| {
                        match toggle {
                            0 => workbench.draft.local_execution = !workbench.draft.local_execution,
                            1 => workbench.draft.approve_execute = !workbench.draft.approve_execute,
                            _ => workbench.draft.async_subagents = !workbench.draft.async_subagents,
                        }
                        cx.notify();
                    }),
                ),
            ));
        }

        let actions = ui::actions()
            .child(
                ui::Button::new("save-settings", "Save")
                    .tone(ui::Tone::Accent)
                    .on_click(
                        cx.listener(|workbench, _event, _window, cx| workbench.save_settings(cx)),
                    ),
            )
            .child(
                ui::Button::new("close-settings", "Close").on_click(cx.listener(
                    |workbench, _event, _window, cx| {
                        // Closing without saving puts the saved palette back — the preview was a
                        // look, not a change.
                        let saved = settings::Settings::load();
                        workbench.applied_theme = saved.theme.clone();
                        settings::apply_theme(&saved);
                        workbench.settings_open = false;
                        workbench.restore_focus = true;
                        cx.notify();
                    },
                )),
            );

        self.preferences_window(pane, actions, cx)
    }

    /// The Setup pane: one row per check, each carrying the command that fixes it.
    ///
    /// Deliberately not a wizard. A wizard assumes it knows the order things went wrong
    /// in; a checklist just says what is true, which is also what makes it useful the
    /// *second* time — when one thing broke on a machine that used to work.
    fn setup_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // A page inside the preferences window: no frame, no scroll container and no action
        // row of its own. `ui::Modal` owns all three, which is what stops Re-check ending up
        // inside the scrolling part again (§40, §41, §52).
        let mut pane = div().flex().flex_col().w_full().min_w_0().gap_3();

        // Said out loud, because it is invisible and load-bearing. The Python overlay lives in
        // the backend *process*, so a server left running by an earlier session may be running
        // an older copy than this app ships — and the only symptom is a feature that silently
        // does nothing, which is exactly how §78 and §79 both presented (docs §80).
        if self.backend_start == Some(backend::Started::Attached) {
            pane = pane.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme::warning()))
                    .text_color(rgb(theme::warning()))
                    .text_xs()
                    .child(
                        "This backend was already running when the app started, so it may be \
                         running an older version of the app's Python overlay. If something \
                         new does nothing, restart it below.",
                    ),
            );
        }

        match &self.report {
            None => {
                pane = pane.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
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
                                .text_color(rgb(if report.ready() {
                                    theme::text_muted()
                                } else {
                                    theme::error()
                                }))
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
                                .text_color(rgb(theme::text_muted()))
                                .text_xs()
                                .child(format!("{} · {}", report.location, report.execution)),
                        )
                        // Whether the app may maintain that directory. Said out loud
                        // because it decides what the app is allowed to do to the user's
                        // own files, and that must never be a surprise.
                        .child(div().text_color(rgb(theme::text_muted())).text_xs().child(
                            if report.owned {
                                "Installed and maintained by this app."
                            } else {
                                "Your own checkout — the app runs it but never modifies it."
                            },
                        )),
                );

                for check in &report.checks {
                    let color = match check.state {
                        preflight::State::Pass => theme::text_muted(),
                        preflight::State::Warn => theme::accent(),
                        preflight::State::Fail => theme::error(),
                        preflight::State::Skip => theme::border(),
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
                                    theme::text()
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
                                .text_color(rgb(theme::text_muted()))
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
                                    .child(
                                        div()
                                            .text_color(rgb(theme::text_muted()))
                                            .text_xs()
                                            .child(*note),
                                    )
                                    .child(
                                        ui::actions()
                                            .gap_2()
                                            .child(
                                                ui::Button::new(
                                                    SharedString::from(format!("run-{}", check.id)),
                                                    *label,
                                                )
                                                .tone(ui::Tone::Accent)
                                                .disabled(busy)
                                                .on_click(cx.listener({
                                                    let argv = argv.clone();
                                                    let label = label.to_string();
                                                    let check_id = check.id;
                                                    move |workbench, _event, _window, cx| {
                                                        workbench.start_fix(
                                                            label.clone(),
                                                            argv.clone(),
                                                            check_id,
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
                                                ui::Button::new(
                                                    SharedString::from(format!(
                                                        "copy-{}",
                                                        check.id
                                                    )),
                                                    "Copy ⧉",
                                                )
                                                .on_click(cx.listener({
                                                    let command = command.clone();
                                                    move |workbench, _event, _window, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(
                                                                command.clone(),
                                                            ),
                                                        );
                                                        workbench.say("command copied", cx);
                                                        cx.notify();
                                                    }
                                                })),
                                            ),
                                    );
                            }
                            preflight::Fix::Adopt { label, dir } => {
                                row = row.child(
                                    ui::Button::new(
                                        SharedString::from(format!("adopt-{}", check.id)),
                                        *label,
                                    )
                                    .tone(ui::Tone::Accent)
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
                                        .text_color(rgb(theme::text_muted()))
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
                .rounded_lg()
                .border_1()
                .border_color(rgb(if !fix.done {
                    theme::accent()
                } else if fix.ok {
                    theme::border()
                } else {
                    theme::error()
                }))
                .child(
                    div()
                        .text_color(rgb(theme::text()))
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
                            .text_color(rgb(theme::accent()))
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
                            ui::Button::new("open-signin", "Open the sign-in page")
                                .tone(ui::Tone::Accent)
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
                            ui::Button::new("copy-signin", "Copy ⧉").on_click(cx.listener({
                                let link = link.clone();
                                move |workbench, _event, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                    workbench.say("sign-in link copied", cx);
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
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(line.clone()),
                );
            }
            if fix.lines.is_empty() {
                output = output.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        // Said plainly when a *finished* fix produced nothing, because
                        // "the last lines say why" over an empty box is worse than
                        // admitting there are none (docs §57).
                        .child(if fix.done {
                            "The command printed nothing. The sidecar log below may have more."
                        } else {
                            "starting…"
                        }),
                );
            }
            let mut log = log.child(output);
            // Outside the scrolling box: the verdict and what to do next are the two things
            // that must not be scrolled out of sight by a chatty command.
            let tone = self.fix_tone(fix);
            for note in &fix.notes {
                log = log.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(tone))
                        .text_xs()
                        .child(note.clone()),
                );
            }
            pane = pane.child(log);
        }

        pane.child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(theme::text_muted()))
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
                // Ordinary New always means the root workspace. Starting within a project has
                // its own explicit `+` beside that heading; inheriting the open/remembered project
                // made conversations start already filed and revived deleted headings (§154).
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
    }

    /// The palette overlay: a query field over a filtered command list.
    ///
    /// Rendered as the root's last child so it paints above the panes; it is
    /// `absolute`, so it takes no part in the three-pane flex layout.
    fn palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let commands = self.palette_commands(cx);
        // The same choice the Enter key will make, so what is highlighted is what runs.
        let selected = self.palette_choice(&commands).map(|(index, _)| index);

        let mut list = div().flex().flex_col().w_full().min_w_0();
        if commands.is_empty() {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child("No matching command."),
            );
        }
        for (index, command) in commands.iter().enumerate() {
            let is_selected = Some(index) == selected;
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
                    .when(is_selected, |row| row.bg(rgb(theme::border())))
                    .hover(|style| style.bg(rgb(theme::border())).cursor_pointer())
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(if is_selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            }))
                            .child(command.label()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::text_muted()))
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
                    .bg(rgb(theme::surface()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .child(
                        div()
                            .p_2()
                            .border_b_1()
                            .border_color(rgb(theme::border()))
                            .child(self.palette_query.clone()),
                    )
                    .child(list)
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .border_t_1()
                            .border_color(rgb(theme::border()))
                            .text_color(rgb(theme::text_muted()))
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
                    .text_color(rgb(theme::text_muted()))
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
                .border_color(rgb(theme::border()))
                .child(
                    div()
                        // Unique per (turn, trace) so GPUI keeps each group's click
                        // state to itself.
                        .id(SharedString::from(format!(
                            "trace-{message_index}-{trace_index}"
                        )))
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::accent()))
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
                            .text_color(rgb(theme::text_muted()))
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
        // Three states, which is what every shipped chat composer converged on: a filled
        // circular button that sends, the same button greyed when there is nothing to
        // send, and a stop control while a turn streams. Empty-means-disabled is the
        // near-universal rule, and a send/stop toggle in the composer is how the running
        // state is expressed without adding a second control (docs §52).
        let has_text = !self.composer.read(cx).text().trim().is_empty();
        let (glyph, fill, ink, hint) = if self.streaming {
            ("■", theme::elevated(), theme::error(), "stop this turn")
        } else if has_text {
            ("↑", theme::accent(), theme::background(), "send")
        } else {
            (
                "↑",
                theme::elevated(),
                theme::text_faint(),
                "type a question first",
            )
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_none()
            .m_2()
            .p_2()
            .rounded_lg()
            // The composer reads as one field with a control inside it, rather than a
            // text box sitting next to an unrelated button.
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            // Which field has the keyboard is otherwise invisible — there is a caret, and it
            // is two pixels wide. `in_focus` rather than `focus` because the thing with the
            // focus is a child entity, not this box.
            .track_focus(&self.composer.focus_handle(cx))
            .in_focus(|style| style.border_color(rgb(theme::accent())))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Composer, cx);
                }),
            )
            .child(self.composer.clone())
            .child(
                div()
                    .id("send-turn")
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(30.))
                    .h(px(30.))
                    // Circular, so it reads as a control and not as a word in a box.
                    .rounded_full()
                    .bg(rgb(fill))
                    .text_color(rgb(ink))
                    .text_sm()
                    .tooltip({
                        let hint = hint.to_string();
                        move |_window, cx| {
                            cx.new(|_| Hint {
                                text: hint.clone().into(),
                            })
                            .into()
                        }
                    })
                    .when(has_text && !self.streaming, |button| {
                        button.hover(|style| style.bg(rgb(theme::accent_hover())).cursor_pointer())
                    })
                    .when(self.streaming, |button| {
                        button.hover(|style| style.cursor_pointer())
                    })
                    .child(glyph)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        if workbench.streaming {
                            workbench.stop_turn(cx);
                            return;
                        }
                        // Same path as Enter. Calling the entity directly rather than
                        // dispatching an action keeps this working regardless of where
                        // focus is when the button is clicked.
                        workbench
                            .composer
                            .update(cx, |composer, cx| composer.submit_now(cx));
                    })),
            )
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (status_text, status_color) = match &self.error {
            Some(error) => (error.clone(), theme::error()),
            None => (self.status.clone(), theme::text_muted()),
        };

        div()
            // Never squeezed. It is the last child of a column whose transcript grows, and
            // a flex child shrinks by default — which is how the toggles and the host
            // indicator ended up cut off at the bottom edge (docs §51).
            .flex_none()
            // A moving mark while anything is running. The first turn after launch spends
            // 20–40 seconds building the agent — MCP tool fetches, middleware, model
            // construction — and a still window during that reads as a hang, which is the
            // single most common reason someone kills an app that was working fine.
            .when(self.streaming || self.running_fix.is_some(), |bar| {
                bar.child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::accent()))
                        .text_sm()
                        .with_animation(
                            "working",
                            gpui::Animation::new(std::time::Duration::from_millis(1200)).repeat(),
                            |label, delta| {
                                // Four frames of a braille spinner: no font dependency,
                                // no SVG to ship, and it reads as motion at any size.
                                const FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
                                let frame = (delta * FRAMES.len() as f32) as usize;
                                label.child(FRAMES[frame.min(FRAMES.len() - 1)])
                            },
                        ),
                )
            })
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::surface()))
            .child(ui::Label::new(status_text).colour(status_color).ellipsis())
            // A blanket grant that is in force must never be invisible — and must be
            // revocable without starting a new conversation, or "just this once" becomes
            // permanent by inconvenience. Click to hand the gate back.
            .when(self.approve_conversation, |bar| {
                bar.child(
                    ui::Button::new("revoke-approval", "approving everything — click to stop")
                        .tone(ui::Tone::Accent)
                        .size(ui::Size::Chip)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_conversation = false;
                            workbench.approve_tasks.clear();
                            workbench.status = "asking before each command again".into();
                            cx.notify();
                        })),
                )
            })
            // Panel toggles. Both always present, so a closed panel is never a one-way
            // door — the commonest way a collapsible panel becomes a bug report.
            .child(
                div()
                    .id("toggle-sidebar")
                    .flex_none()
                    .text_color(rgb(if self.sidebar_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child("▤ conversations")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.sidebar_open = !workbench.sidebar_open;
                        workbench.remember_panels();
                        cx.notify();
                    })),
            )
            // The third of the same kind, between the two it belongs with — the road folds from
            // here as well as from its own chevron, because a strip folded to 38px hides its
            // chevron among the dots and the status bar is where the other two live.
            .child(
                div()
                    .id("toggle-road")
                    .flex_none()
                    .text_color(rgb(if self.road_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child("◧ road")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.toggle_road(cx);
                    })),
            )
            .child(
                div()
                    .id("toggle-panel")
                    .flex_none()
                    .text_color(rgb(if self.panel_open {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    }))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent_hover()))
                            .cursor_pointer()
                    })
                    .child("▥ research")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.panel_open = !workbench.panel_open;
                        workbench.remember_panels();
                        cx.notify();
                    })),
            )
            // Say where the agent's code runs. When that is the user's own machine
            // it should be visible without opening a log (docs §18).
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(if self.sidecar.execution() == "host (local)" {
                        theme::accent()
                    } else {
                        theme::text_muted()
                    }))
                    .text_xs()
                    .child(self.sidecar.execution()),
            )
            // Discoverability: a palette nobody knows the shortcut for is a palette
            // nobody opens.
            .child(
                div()
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("ctrl-p commands"),
            )
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child(self.sidecar.base_url().to_string()),
            )
    }

    /// The project spine: mission, what's done, what's queued, what's suggested.
    /// The panel's card, with the scrolling contents inside it and a bar beside them.
    ///
    /// Split from the contents because the scrollbar must sit *outside* the scrolling
    /// element — inside, it would scroll along with what it measures.
    fn artifacts_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .w(px(self.panel_width))
            .flex_none()
            .h_full()
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(self.artifacts_contents(cx))
            .children(scrollbar(&self.panel_scroll))
    }

    fn artifacts_contents(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("spine")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_grow()
            .overflow_y_scroll()
            .track_scroll(&self.panel_scroll)
            .p_4()
            .gap_4()
            // `MISSION`, not `RESEARCH PROJECT`. The panel *is* the research project — saying so
            // at the top of it spends the widest heading in the column on a word that names the
            // container rather than its first section.
            .child(section_label("MISSION"));

        let Some(project) = &self.project else {
            // No spine yet, but a run may already be producing outputs — still show
            // them rather than an empty panel.
            return panel
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .text_sm()
                        .child("No project loaded yet. Run a turn — the mission is derived from your first question."),
                )
                .child(self.jobs_section(cx))
                .child(self.outputs_section(cx))
                .child(self.sources_section());
        };

        panel = panel.child(if project.mission.is_empty() {
            div()
                .text_color(rgb(theme::text_muted()))
                .text_sm()
                .child("No mission yet — it comes from your first question.")
        } else {
            div()
                .w_full()
                .text_color(rgb(theme::text()))
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
                        .border_color(rgb(theme::border()))
                        .hover(|style| style.border_color(rgb(theme::accent())).cursor_pointer())
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(theme::text()))
                                .text_sm()
                                .child(suggestion.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(theme::text_muted()))
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
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Completed and pending work will appear here as the project grows."),
            );
        }

        panel
            .child(self.jobs_section(cx))
            .child(self.outputs_section(cx))
            .child(self.sources_section())
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
                ("⏸", theme::accent())
            } else if !task.is_finished() {
                ("◐", theme::running())
            } else if task.succeeded() {
                ("✓", theme::success())
            } else {
                ("✗", theme::error())
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
                        .text_color(rgb(theme::text()))
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
                        .text_color(rgb(if task.error.is_some() {
                            theme::error()
                        } else if task.needs_approval() {
                            theme::warning()
                        } else {
                            theme::text_muted()
                        }))
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
                            .text_color(rgb(theme::text_muted()))
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
                            .border_color(rgb(theme::border()))
                            .text_color(rgb(theme::text()))
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
                            ui::Button::new(
                                SharedString::from(format!("bg-approve-{task_id}")),
                                "Approve",
                            )
                            .tone(ui::Tone::Accent)
                            .on_click(cx.listener({
                                let task_id = task_id.clone();
                                move |workbench, _event, _window, cx| {
                                    workbench.decide_task(task_id.clone(), true, cx);
                                }
                            })),
                        )
                        .child(
                            ui::Button::new(
                                SharedString::from(format!("bg-reject-{task_id}")),
                                "Reject",
                            )
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
                        ui::Button::new(
                            SharedString::from(format!("bg-approve-{suffix}-{task_id}")),
                            label,
                        )
                        // `text_xs` in the original: these sit under the pair above and
                        // are the wider-scope variants of it, not peers.
                        .size(ui::Size::Compact)
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
                ("◐", theme::running())
            } else if job.succeeded() {
                ("✓", theme::success())
            } else {
                ("✗", theme::error())
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
                            .text_color(rgb(theme::text()))
                            .text_sm()
                            .child(format!("{mark} {}", job.kind.label())),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::text_muted()))
                            .text_xs()
                            .child(detail),
                    )
                    .when(!job.question.is_empty(), |row| {
                        row.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_color(rgb(theme::text_muted()))
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
    fn sources_section(&self) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!self.sources.is_empty(), |section| {
                section
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .child(section_label_owned(format!(
                        "SOURCES · {}",
                        self.sources.len()
                    )))
            });

        // A quiet line while the registry is being asked, and nothing at all once it is done.
        // There is no control here: see `Workbench::resolve_sources` for why verifying a citation
        // is not something to ask permission for every time.
        if self.resolving > 0 {
            section = section.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(11.))
                    .child(format!("checking {} references…", self.resolving)),
            );
        }

        for (at, source) in self.sources.iter().enumerate() {
            let verdict = self.checked.get(&source.citation);
            // **Three states, not two.** `None` is *not looked up yet*; `Some(None)` is *looked
            // up, and the registry has nothing*. Collapsing them with `.flatten()` — which this
            // did — made a reference still being resolved display the message meant for one that
            // came back empty, which is how a correctly cited Magurran 1988 was told it matched
            // nothing while its lookup was still in flight.
            //
            // This is the distinction the whole feature is about, reintroduced one call inside
            // it. Kept unflattened here so the match below can see all three.
            let looked_up = self.repaired.get(&source.citation);
            let repair = looked_up.cloned().flatten();
            // **Semantic Scholar, whichever identifier we ended up with.** Asked for directly:
            // *"when I press it I am redirected to the paper in semantic scholar not to the
            // article in the main page where the article was published."* `api.semanticscholar.org`
            // 301-redirects for both id forms — verified — so a corpus id and a DOI both land on
            // the paper's own page.
            let link = scholar_link(source, verdict, repair.as_ref());
            // The citation without its URL. A DOI written into a sentence wraps mid-token in a
            // 330px column, and a link that *looks* broken is one somebody retypes with a space
            // in it — but more to the point, the raw URL is not information a reader wants. The
            // word "link" is.
            let prose = without_url(&source.citation);

            let mut row = div()
                .id(SharedString::from(format!("source-{at}")))
                .flex()
                .flex_row()
                .items_start()
                .gap_2()
                .w_full()
                .min_w_0()
                .p_2()
                .rounded_lg()
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(theme::accent()))
                        .text_size(px(11.))
                        .child(format!("[{}]", at + 1)),
                );

            let mut body = div()
                .flex()
                .flex_col()
                .flex_grow()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_color(rgb(theme::text()))
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .child(prose),
                );

            if let Some(url) = link.clone() {
                body = body.child(
                    div()
                        .id(SharedString::from(format!("source-link-{at}")))
                        .flex_none()
                        .text_color(rgb(theme::accent()))
                        .text_size(px(12.))
                        .hover(|style| {
                            style
                                .text_color(rgb(theme::accent_hover()))
                                .cursor_pointer()
                        })
                        .child("link")
                        .on_click(move |_event, _window, _cx| {
                            if let Err(error) = workspace::browse(&url) {
                                tracing::warn!(%error, "could not open a source");
                            }
                        }),
                );
            }

            // **Only when something is wrong.** A line under every reference saying it checked
            // out is fourteen lines of reassurance nobody reads, and it buries the two that
            // matter. Silence here means verified.
            // Said while the check is still running, because the alternative is a reference that
            // looks finished and is not. The link is withheld until then (see `scholar_link`), and
            // a row with neither a link nor an explanation reads as a reference with nothing
            // wrong with it.
            let note = match (verdict, looked_up) {
                (None, _) if self.resolving > 0 => {
                    Some((theme::text_faint(), "checking this reference…".to_string()))
                }
                (Some(references::Verdict::Mismatch { found }), None) => Some((
                    theme::error(),
                    format!(
                        "the DOI in this citation belongs to a different paper ({found}) — \
                         looking for the right one"
                    ),
                )),
                (Some(references::Verdict::Mismatch { .. }), Some(Some(_))) => Some((
                    theme::warning(),
                    "the citation's own DOI named a different paper; this link is the work it \
                     describes"
                        .to_string(),
                )),
                (Some(references::Verdict::Unregistered), Some(Some(_))) => Some((
                    theme::warning(),
                    "the citation's own DOI is not registered; this link is the work it describes"
                        .to_string(),
                )),
                (Some(verdict), Some(None)) if verdict.is_problem() => Some((
                    theme::error(),
                    "this reference does not check out, and nothing in Crossref matches it — \
                     Crossref covers journal articles, so a book or thesis may not be there"
                        .to_string(),
                )),
                (Some(references::Verdict::NoIdentifier), Some(None)) => Some((
                    theme::warning(),
                    "no identifier, and nothing in Crossref matches this citation".to_string(),
                )),
                (Some(references::Verdict::Unreachable { why }), _) => Some((
                    theme::text_faint(),
                    format!("not checked ({why})"),
                )),
                _ => None,
            };
            if let Some((ink, text)) = note {
                body = body.child(
                    div()
                        .text_color(rgb(ink))
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .child(text),
                );
            }

            row = row.child(body);
            section = section.child(row);
        }
        section
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

    fn outputs_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // What is actually on disk, rather than the agent's own artifact list: a file written by
        // a script inside `execute` registers no artifact, and those are most of them.
        let listing = self
            .thread_workspace()
            .map(|dir| workspace::output_listing(&dir));
        let files = listing
            .as_ref()
            .map(|listing| listing.groups.as_slice())
            .unwrap_or_default();
        let count: usize = files.iter().map(|(_, items)| items.len()).sum();

        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(theme::border()));

        // **Nothing at all when there is nothing.** This used to promise which artifacts would
        // appear before the filesystem had any. The recursive scan now makes §117's subfolders
        // visible, but an empty section still says less and is right.
        if count == 0 && self.buckets.is_empty() {
            return section;
        }

        if count > 0 {
            section = section.child(section_label_owned(format!("FILES · {count}")));
        }

        if listing.as_ref().is_some_and(|listing| listing.truncated) {
            // The scan is intentionally bounded: an agent can create a virtualenv or unpack a
            // dataset under its workspace. Say when that protection bites, because a silent cap
            // would only turn §117's missing-folder defect into a missing-513th-file defect.
            section = section.child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Showing a bounded view. Open the folder to see the rest."),
            );
        }

        for (_, items) in files {
            for output in items {
                let shown = output.clone();
                let (glyph, ink) = file_mark(&output.path);
                section = section.child(
                    div()
                        .id(SharedString::from(format!("file-{}", output.name)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .p_2()
                        .rounded_lg()
                        // A fill instead of a border: thirty bordered rows in a 330px column is
                        // thirty horizontal lines, and the eye reads those as a table it is
                        // supposed to compare across.
                        .bg(rgb(theme::elevated()))
                        .hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer())
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgb(ink))
                                .text_size(px(13.))
                                .child(glyph),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_grow()
                                .min_w_0()
                                .child(
                                    ui::Label::new(output.name.clone())
                                        .size(ui::Size::Compact)
                                        .ellipsis(),
                                )
                                // The real shape of the file, not just how much of the disk it
                                // takes: `1,204 rows · 11 cols` is what decides whether it is the
                                // file you wanted. See `workspace::Shape` for why a PDF gets a
                                // size and no page count.
                                .child(
                                    div()
                                        .text_color(rgb(theme::text_faint()))
                                        .text_size(px(11.))
                                        .child(self.shape_of(output).describe(output.bytes)),
                                ),
                        )
                        .on_click(cx.listener(move |workbench, _event, _window, cx| {
                            // In the window first. Leaving the app to look at a 4 KB CSV
                            // is a context switch out of the work, and back is not free.
                            workbench.preview = Some(shown.clone());
                            cx.notify();
                        })),
                );
            }
        }

        // Everything this conversation wrote, in one folder the researcher already owns.
        // This *is* "download all the documents": the files are in their own Documents
        // directory (`workspace.rs`), so there is nothing to package — the ask was only
        // ever for a way to get at them.
        //
        // Dashed and last, because it is a way *out* of the panel rather than another row in it —
        // and it reaches anything beyond §143's deliberate scan bounds.
        if let Some(dir) = self.thread_workspace() {
            section = section.child(
                div()
                    .id("open-workspace")
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(theme::border_strong()))
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent()))
                            .border_color(rgb(theme::accent()))
                            .cursor_pointer()
                    })
                    .child(if cfg!(windows) {
                        "Open the folder in Explorer"
                    } else {
                        "Open the folder"
                    })
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&dir) {
                            tracing::warn!(%error, "could not open the workspace folder");
                        }
                    }),
            );
        }

        for bucket in &self.buckets {
            // Show a bounded number of titles — a literature search can return
            // dozens, and the count already conveys the scale.
            const MAX_SHOWN: usize = 4;
            let mut group = div().flex().flex_col().gap_1().child(
                div()
                    .text_color(rgb(theme::text()))
                    .text_sm()
                    .child(format!("{} · {}", bucket.name, bucket.items.len())),
            );
            for item in bucket.items.iter().take(MAX_SHOWN) {
                group = group.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(item.clone()),
                );
            }
            if bucket.items.len() > MAX_SHOWN {
                group = group.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
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
            .child(self.chat_pane(cx));

        // The right-hand slot belongs to the research panel alone. Setup used to take it,
        // which meant diagnosing a problem hid the outputs you were diagnosing it about.
        body = if self.panel_open {
            body.child(self.divider(Divider::Panel, cx))
                .child(self.artifacts_panel(cx))
        } else {
            body
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
                    if workbench.dragging.take().is_some() {
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
                    workbench.files_dropped(paths.paths(), cx);
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
            Some(output) => root.child(self.preview_modal(output.clone(), cx)),
            None => root,
        };

        let root = match &self.confirming_delete {
            Some(target) => root.child(self.delete_modal(target, cx)),
            None => root,
        };

        let root = if self.palette_open {
            root.child(self.palette(cx))
        } else {
            root
        };

        let root = root.children(self.picker_popup(cx));

        // Last, and `deferred` inside, so it paints over every pane it might open across
        // instead of being clipped by the one it opened in.
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
    use super::*;

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
        let entries = bibliography(&[
            cited(
                "Smith, J. et al. (2021). Late blight resistance. Plant Pathology 70(4). \
                 https://doi.org/10.1111/ppa.13400",
                None,
            ),
            cited("CIP Dataverse: Andean potato trials, 2019", None),
            cited("   ", None),
        ]);

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
        let hostile = bibliography(&[cited("A title with {braces} and a \\command", None)]);
        assert_eq!(hostile.matches('{').count(), hostile.matches('}').count());
        assert!(!hostile.contains("{braces}"));
        assert!(hostile.contains("\\\\command"));

        // A doubtful reference carries its doubt into the reference manager. Somebody importing
        // forty of these should not have to come back here to find out which two to check.
        let doubtful = bibliography(&[cited(
            "Hijmans & Spooner (2001). https://doi.org/10.2307/3558457",
            Some("https://doi.org/10.2307/3558433"),
        )]);
        assert!(doubtful.contains("url = {https://doi.org/10.2307/3558433}"));
        assert!(doubtful.contains("annote = {unverified:"), "{doubtful}");

        assert!(bibliography(&[]).is_empty(), "nothing to copy is empty");
    }

    #[test]
    fn a_dropped_file_becomes_a_question_the_backend_can_act_on() {
        // The path has to be spelled the way the *agent* would open it. On Windows the
        // agent lives inside WSL, so a prompt naming `C:\…` would send it looking for a
        // file that does not exist there — and the researcher would have no idea why.
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

        let prompt = prompt_for_dropped(std::slice::from_ref(&translated), &[false]);
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
        assert!(
            many.contains("/mnt/c/a.csv") && many.contains("/mnt/c/b.csv"),
            "{many}"
        );
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

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
