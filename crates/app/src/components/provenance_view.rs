#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

// --- citation / link resolution ---

/// Where a source's `link` should point: the paper's own page on Semantic Scholar.
///
/// `api.semanticscholar.org/<id>` redirects to the paper page for both a corpus id and a DOI.
pub(crate) fn scholar_link(
    source: &protocol::Source,
    verdict: Option<&references::Verdict>,
    repair: Option<&references::Repair>,
) -> Option<String> {
    let existing = link_for(source);

    // Built from the search result's own `corpusId`; nothing composed it, so nothing to check.
    if existing.as_deref().is_some_and(references::is_corpus_link) {
        return existing;
    }

    // The work the registry says this citation describes. Checked, so usable.
    if let Some(repair) = repair {
        return Some(format!("https://api.semanticscholar.org/DOI:{}", repair.doi));
    }

    // The citation's own DOI, only once verified: an unconfirmed DOI can resolve to someone
    // else's real paper instead of failing, so it must not be linked until checked.
    if matches!(verdict, Some(references::Verdict::Confirmed)) {
        if let Some(doi) = existing.as_deref().and_then(references::doi_in) {
            return Some(format!("https://api.semanticscholar.org/DOI:{doi}"));
        }
    }

    // A link with no identifier (e.g. a thesis repository) is kept as-is; one with an
    // unverified DOI is dropped rather than shown as if resolved.
    match existing.as_deref().and_then(references::doi_in) {
        Some(_) => None,
        None => existing,
    }
}

/// The link to actually use for a source: the backend's own field, else the URL in the citation.
pub(crate) fn link_for(source: &protocol::Source) -> Option<String> {
    source
        .link
        .clone()
        .or_else(|| first_url(&source.citation))
}

/// The URL written into the citation text, when it contradicts the structured one.
///
/// `None` when they agree, when either is missing, or when they differ only in ways that are
/// harmless (trailing slash, `http` vs `https`, `dx.doi.org` vs `doi.org`).
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
    out.trim().trim_end_matches(['.', ',', ';', ' ']).trim().to_string()
}

// --- export formats ---

/// This conversation's citations as BibTeX a reference manager will import.
///
/// Every entry is `@misc` with the raw citation text in `note`: splitting a prose citation into
/// structured fields risks a confidently wrong split, while `note` is always faithful.
pub(crate) fn bibliography(sources: &[protocol::Source], origins: &[references::Origin]) -> String {
    let mut out = String::new();
    for (at, source) in sources.iter().enumerate() {
        let citation = source.citation.trim();
        if citation.is_empty() {
            continue;
        }
        // Braces/backslashes are BibTeX syntax and would truncate the entry if left in.
        let safe = citation.replace('\\', "\\\\").replace(['{', '}'], "");
        out.push_str(&format!("@misc{{minime{},\n  note = {{{safe}}},\n", at + 1));
        if let Some(url) = link_for(source) {
            out.push_str(&format!("  url = {{{url}}},\n"));
        }
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

/// The provenance graph as a Mermaid `flowchart`.
///
/// Mermaid renders in GitHub, Quarto, Obsidian and Typst, and stays readable as plain text
/// otherwise. `-->` marks a causal (delegated) edge, `-.->` an arrival-order edge.
pub(crate) fn mermaid(graph: &provenance::Graph) -> String {
    let mut out = String::from("flowchart TD\n");
    for (at, node) in graph.nodes.iter().enumerate() {
        let label = match node.visits {
            1 => node.name.replace('_', " "),
            visits => format!("{} ×{visits}", node.name.replace('_', " ")),
        };
        // Quoted so a name with a bracket or space cannot end the node early.
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
/// SVG rather than a PNG screenshot: it's plain text, needs no rasteriser, and scales cleanly
/// for a manuscript figure. Colours are baked from the live theme at export time.
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

// --- drawing primitives ---

/// The colour that stands for how much an edge can be trusted.
pub(crate) fn edge_ink(kind: provenance::Edge) -> u32 {
    match kind {
        provenance::Edge::Delegated => theme::text_muted(),
        provenance::Edge::Then => theme::text_faint(),
        provenance::Edge::Returned => theme::accent(),
    }
}

/// Stroke a quadratic as a dashed line, since `PathBuilder::stroke` has no dash setting.
pub(crate) fn paint_dashed_curve(
    window: &mut Window,
    start: gpui::Point<gpui::Pixels>,
    control: gpui::Point<gpui::Pixels>,
    finish: gpui::Point<gpui::Pixels>,
    weight: f32,
    colour: u32,
) {
    /// Samples along the curve.
    const STEPS: usize = 48;
    /// Samples per dash and per gap (a 4/4 pattern, in curve parameter rather than pixels).
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

/// What each line in the graph means, drawn the way it is drawn (a stroke sample, not a swatch).
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
                        // A dashed sample without a canvas: gaps are background-coloured blocks
                        // laid over the line.
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

// --- cross-file utilities (used by chat.rs) ---

/// What kind of work a specialist does, as a colour: research-flavoured, data-flavoured, or none.
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

/// The specialists a turn consulted, in order, with consecutive repeats of the same one collapsed.
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
    /// The provenance modal: what was consulted, in what order, and where it doubled back.
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

        // Turn filter only applies to the graph — the timeline is already one row per turn.
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
    pub(crate) fn provenance_timeline(&self) -> gpui::Div {
        let mut body = div().flex().flex_col().w_full().min_w_0().gap_4();
        // Scaled against the longest turn span in the conversation, not each turn's own span,
        // so bars are comparable across rows; gaps between turns are excluded from the scale.
        let scale = self.provenance.scale() as f32;
        for (index, turn) in self.provenance.turns.iter().enumerate() {
            if turn.invocations.is_empty() {
                continue;
            }
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
                                // Indent shows a nested delegation as called by another specialist.
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
                                        // Zero-width single-chunk invocations still get a sliver.
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

    /// The graph: nodes are specialists, edges are the transitions between them, drawn as arcs.
    pub(crate) fn provenance_graph(&self) -> gpui::Div {
        let graph = self.provenance.graph_of(self.provenance_turn);

        // Nodes are real elements (for text layout) and edges are painted beside them on a
        // canvas, so both need to agree on geometry — hence these shared constants.
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
                weight: match edge.kind {
                    provenance::Edge::Returned => 2.,
                    _ => 1.5,
                } + (edge.count.saturating_sub(1) as f32 * 0.8).min(3.),
                colour: edge_ink(edge.kind),
                // Solid = causal (delegated); everything else is arrival order.
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
                    // twice the intended bow.
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

                    // Arrowhead direction is the curve's tangent at the endpoint: `finish - control`.
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
                        // Full width so every node ends at the same x, matching the edge gutter.
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
                    // Fixed width: the arcs' bow distances are measured against it.
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
