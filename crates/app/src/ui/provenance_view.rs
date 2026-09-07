// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, sidebar::*, chat::*, gallery_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

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
pub(crate) fn paint_dashed_curve(
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
pub(crate) fn graph_legend() -> impl IntoElement {
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


impl Workbench {
    /// The record of this enquiry: what was consulted, in what order, and where it doubled back.
    ///
    /// Requested (docs §73) with one sentence as the specification — *"each scientist can track
    /// his work by conversation"* — and built as a modal for the reason §68 moved Setup into one:
    /// it is something you open, read and close, not a place you navigate to.
    pub(crate) fn provenance_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            let mut rail = rail.child(div().pt_3().child(ui::Label::new("TURNS").colour(theme::text_faint()).size(ui::Size::Compact))).child(
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
                        ui::Button::new("provenance-mermaid")
                            .text("Copy as Mermaid")
                            .disabled(self.provenance.is_empty())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                let text = mermaid(&workbench.provenance.graph_of(workbench.provenance_turn));
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                                workbench.say("provenance copied as a Mermaid diagram", cx);
                            })),
                    )
                    .child(
                        ui::Button::new("provenance-svg")
                            .text("Save as SVG")
                            .style(ui::ButtonStyle::Primary)
                            .disabled(self.provenance.is_empty() || self.thread_workspace().is_none())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.save_provenance_svg(cx);
                            })),
                    )
                    .child(div().flex_grow())
                    .child(
                        ui::Button::new("provenance-close").text("Close").on_click(cx.listener(
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
}


impl Workbench {
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
    pub(crate) fn provenance_timeline(&self) -> gpui::Div {
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
}


impl Workbench {
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
    pub(crate) fn provenance_graph(&self) -> gpui::Div {
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
}

