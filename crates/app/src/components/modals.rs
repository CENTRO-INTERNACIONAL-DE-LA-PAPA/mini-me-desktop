#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    // ---- provider / delete / about ----

    /// Confirms switching providers, showing key and base-URL requirements.
    pub(crate) fn provider_modal(
        &self,
        spec: &'static settings::Provider,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let leaving = settings::provider(&self.draft.provider).map_or("none", |from| from.label);
        let has_key = settings::secret(&format!("llm:{}", spec.id)).is_some();
        let needs_url = spec.needs_base_url && self.draft.base_url.trim().is_empty();

        let mut body = div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap_3()
            .child(ui::Label::new(format!(
                "Turns will run on {} instead of {leaving}, and be billed to that account.",
                spec.label
            )));

        if !has_key {
            body = body.child(
                ui::Label::new(format!(
                    "No API key is stored for {}. Keys are kept per provider, so one added for \
                     another provider does not count — paste it below after confirming, or this \
                     one cannot run a turn.",
                    spec.label
                ))
                .colour(theme::warning()),
            );
        }
        if needs_url {
            body = body.child(
                ui::Label::new(
                    "This provider needs its base URL — for OpenRouter that is \
                     https://openrouter.ai/api/v1. Without it the request has no address to go to.",
                )
                .colour(theme::warning()),
            );
        }
        if has_key && !needs_url {
            body = body.child(
                ui::Label::new(format!("A key for {} is already stored.", spec.label))
                    .colour(theme::success()),
            );
        }

        body = body.child(
            ui::Label::new(format!("The model will be set to {}.", spec.suggested_model))
                .muted()
                .size(ui::Size::Compact),
        );

        ui::Modal::new("provider-confirmation", format!("Switch to {}?", spec.label))
            .width(560.)
            .focus(&self.delete_focus)
            .body(body)
            .actions(
                ui::actions()
                    .child(div().flex_grow())
                    .child(ui::Button::new("provider-cancel", "Cancel").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.confirming_provider = None;
                            cx.notify();
                        },
                    )))
                    .child(
                        ui::Button::new("provider-confirm", "Switch provider")
                            .tone(ui::Tone::Accent)
                            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                workbench.confirming_provider = None;
                                workbench.draft.provider = spec.id.to_string();
                                workbench.refresh_models(cx);
                                workbench.set_field(Field::ModelId, spec.suggested_model, cx);
                                cx.notify();
                            })),
                    ),
            )
            .footer(
                ui::Label::new("Nothing is billed until you save and ask a question.")
                    .muted()
                    .size(ui::Size::Compact),
            )
    }

    /// Confirms deleting a conversation or an entire project.
    pub(crate) fn delete_modal(&self, target: &DeleteTarget, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .children(self.delete_interrupts_work.then(|| {
                        ui::Label::new(
                            "Background work here still says it is running. Deleting now may lose \
                             whatever it has not finished writing — and a task that has been \
                             running far longer than it should is usually one that has stopped \
                             without saying so.",
                        )
                        .colour(theme::accent())
                        .size(ui::Size::Compact)
                    }))
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

    /// The About box: the specialist team, data sources, this build, and the Asta citation.
    pub(crate) fn about_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let specialists = workspace::subagents();

        let mut team = div().flex().flex_col().w_full().min_w_0().gap_2();
        if specialists.is_empty() {
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
            .child(section_label("THIS BUILD"))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .child(ui::Label::new(build_stamp()).colour(theme::accent()))
                    .child(
                        ui::Label::new(
                            "Include this line when reporting a problem, with the two log files                              named in Setup.",
                        )
                        .muted()
                        .size(ui::Size::Compact),
                    )
                    .child(
                        ui::Label::new(match &self.update {
                            Some(standing) => update::describe(standing, &self.install),
                            None => "checking for a newer build…".to_string(),
                        })
                        .muted()
                        .size(ui::Size::Compact),
                    )
                    .children(self.update_action(cx)),
            )
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

    /// The update control inside the About modal, or nothing when already up to date.
    pub(crate) fn update_action(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let offer = match (&self.update, &self.install) {
            (Some(update::Standing::Behind(release)), update::Layout::Packaged(_)) => release,
            _ => return None,
        };
        let line = match &self.taking {
            Some(update::Fetch::Progress(so_far, total)) => {
                let percent = if *total == 0 {
                    0
                } else {
                    (so_far.saturating_mul(100) / total).min(100)
                };
                return Some(
                    ui::Label::new(format!("downloading {} — {percent}%", offer.tag))
                        .muted()
                        .size(ui::Size::Compact)
                        .into_any_element(),
                );
            }
            Some(update::Fetch::Ready(root, integrity)) => {
                let checked = match integrity {
                    update::Integrity::Digest => "checked against the digest GitHub published",
                    update::Integrity::SizeOnly => "checked by length only — no digest was published",
                };
                return Some(
                    ui::Label::new(format!(
                        "{} is downloaded and {checked}. Press Restart to Update in the status \
                         bar. It is waiting at {}.",
                        offer.tag,
                        root.display()
                    ))
                    .muted()
                    .size(ui::Size::Compact)
                    .into_any_element(),
                );
            }
            Some(update::Fetch::Failed(reason)) => Some(reason.clone()),
            None => None,
        };
        let label = format!("Try {} again", offer.tag);
        let mut column = div().flex().flex_col().w_full().min_w_0().gap_1();
        if let Some(reason) = line {
            column = column.child(
                ui::Label::new(format!("could not download it: {reason}"))
                    .muted()
                    .size(ui::Size::Compact),
            );
        }
        Some(
            column
                .child(
                    ui::Button::new("take-update", label)
                        .size(ui::Size::Compact)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.take_update(cx);
                        })),
                )
                .into_any_element(),
        )
    }

    // ---- discovery ----

    /// The discovery run's search tree: experiment nodes connected by their branching.
    pub(crate) fn discovery_tree(&self, view: &DiscoveryView, cx: &mut Context<Self>) -> impl IntoElement {
        let placed = discovery::layout(&view.experiments);
        let (width, height) = discovery::canvas(&placed);
        let mut canvas = div()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(height));

        for (parent, child) in discovery::edges(&view.experiments) {
            let Some(from) = placed.iter().find(|node| node.at == parent) else {
                continue;
            };
            let Some(to) = placed.iter().find(|node| node.at == child) else {
                continue;
            };
            for segment in discovery::elbow(from, to) {
                canvas = canvas.child(
                    div()
                        .absolute()
                        .left(px(segment.left))
                        .top(px(segment.top))
                        .w(px(segment.width.max(1.0)))
                        .h(px(segment.height.max(1.0)))
                        .bg(rgb(theme::border_strong())),
                );
            }
        }

        for node in &placed {
            let Some(experiment) = view.experiments.get(node.at) else {
                continue;
            };
            let (x, y) = discovery::centre(node);
            let (fill, ink, border) = self.node_colours(experiment);
            let chosen = view.selected == Some(node.at);
            let at = node.at;
            canvas = canvas.child(
                div()
                    .id(SharedString::from(format!("exp-node-{}", experiment.id)))
                    .absolute()
                    .left(px(x - discovery::NODE / 2.0))
                    .top(px(y - discovery::NODE / 2.0))
                    .w(px(discovery::NODE))
                    .h(px(discovery::NODE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(fill))
                    .border_2()
                    .border_color(rgb(if chosen {
                        theme::accent()
                    } else if experiment.surprising {
                        theme::warning()
                    } else {
                        border
                    }))
                    .text_color(rgb(ink))
                    .text_xs()
                    .child(experiment.number.to_string())
                    .hover(|style| style.cursor_pointer())
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        let closing = workbench
                            .discovery_open
                            .as_ref()
                            .is_some_and(|view| view.selected == Some(at));
                        workbench.select_experiment(if closing { None } else { Some(at) }, cx);
                    })),
            );
        }

        div()
            .id("discovery-tree")
            .w_full()
            .min_w_0()
            .max_h(px(if view.expanded { 460. } else { 300. }))
            .overflow_scroll()
            .child(canvas)
    }

    /// Every experiment, ranked by how far it moved a belief.
    pub(crate) fn discovery_list(&self, view: &DiscoveryView, cx: &mut Context<Self>) -> impl IntoElement {
        let order = ranked(&view.experiments, view.loudest_first);

        let mut list = div()
            .id("discovery-list")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_px()
            .max_h(px(if view.expanded { 520. } else { 240. }))
            .overflow_y_scroll();

        for at in order {
            let experiment = &view.experiments[at];
            let (_, _, accent) = self.node_colours(experiment);
            let belief = match (experiment.prior, experiment.posterior) {
                (Some(prior), Some(posterior)) => {
                    format!("{} → {}", prior.name(), posterior.name())
                }
                _ => String::new(),
            };
            let score = match experiment.surprise {
                Some(_) => format!(
                    "{:.3} {}",
                    experiment.magnitude(),
                    experiment.direction().label()
                ),
                None => "—".to_string(),
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("exp-row-{}", experiment.id)))
                    .flex()
                    .flex_row()
                    .items_start()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(view.selected == Some(at), |row| {
                        row.bg(rgb(theme::hover_over(theme::surface())))
                    })
                    .hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .child(
                        div()
                            .flex_none()
                            .w(px(24.))
                            .text_xs()
                            .text_color(rgb(accent))
                            .child(experiment.number.to_string()),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_xs()
                            .text_color(rgb(theme::text()))
                            .child(protocol::clip(&experiment.hypothesis, Self::HYPOTHESIS_CHARS)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(theme::text_muted()))
                            .child(belief),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(theme::text_muted()))
                            .child(score),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.select_experiment(Some(at), cx);
                    })),
            );
        }
        list
    }

    /// One experiment, opened: its belief shift, hypothesis, analysis, review and figures.
    pub(crate) fn discovery_detail(
        &self,
        view: &DiscoveryView,
        experiment: &discovery::Experiment,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut detail = div()
            .id("discovery-detail")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_2()
            .max_h(px(if view.expanded { 520. } else { 240. }))
            .overflow_y_scroll();

        let shift = match (experiment.prior, experiment.posterior) {
            (Some(prior), Some(posterior)) => {
                format!("{} → {}", prior.describe(), posterior.describe())
            }
            _ => "no belief recorded".to_string(),
        };
        detail = detail.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .min_w_0()
                .gap_2()
                .flex_none()
                .child(
                    ui::Label::new(format!("Experiment {}", experiment.number))
                        .colour(theme::text()),
                )
                .child(div().flex_grow())
                .child(
                    ui::Label::new(match experiment.surprise {
                        Some(_) => format!(
                            "{} {:.3}",
                            experiment.direction().label(),
                            experiment.magnitude()
                        ),
                        None => "not scored".to_string(),
                    })
                    .colour(if experiment.surprising {
                        theme::warning()
                    } else {
                        theme::text_muted()
                    })
                    .size(ui::Size::Compact),
                )
                .child(
                    ui::Button::new("discovery-back", "All experiments")
                        .size(ui::Size::Compact)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.select_experiment(None, cx);
                        })),
                ),
        );
        detail = detail.child(
            ui::Label::new(shift)
                .muted()
                .size(ui::Size::Compact),
        );
        // Reported separately from the surprise number: the service's flag is its own judgment,
        // not a threshold derived from the magnitude above it.
        if experiment.surprising {
            detail = detail.child(
                ui::Label::new("The service flagged this one as surprising.")
                    .colour(theme::warning())
                    .size(ui::Size::Compact),
            );
        }

        for (heading, body) in [
            ("HYPOTHESIS", &experiment.hypothesis),
            ("ANALYSIS", &experiment.analysis),
            ("REVIEW", &experiment.review),
        ] {
            if body.trim().is_empty() {
                continue;
            }
            let mut rendered = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .flex_none()
                .gap_1()
                .child(section_label(heading));
            for block in markdown::parse(body) {
                rendered = rendered.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .child(markdown_block(&block, None)),
                );
            }
            detail = detail.child(rendered);
        }

        let known = view.figures.get(&experiment.id);
        match figure_state(known, view.fetching.as_deref() == Some(experiment.id.as_str())) {
            Figures::Ready => {
                let paths = known.expect("Ready means the paths are there");
                detail = detail.child(
                    div()
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .gap_1()
                        .child(section_label("FIGURES"))
                        .children(paths.iter().enumerate().map(|(at, path)| {
                            let opening = path.clone();
                            div()
                                .id(SharedString::from(format!("fig-{}-{at}", experiment.id)))
                                .relative()
                                .flex()
                                .flex_row()
                                .w_full()
                                .flex_none()
                                .h(px(260.))
                                .rounded_md()
                                .overflow_hidden()
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .child(
                                    img(path.clone())
                                        .w_full()
                                        .h_full()
                                        .object_fit(gpui::ObjectFit::Contain),
                                )
                                .hover(|style| style.cursor_pointer())
                                .on_click(move |_event, _window, _cx| {
                                    if let Err(error) = workspace::open(&opening) {
                                        tracing::warn!(%error, "could not open a figure");
                                    }
                                })
                        })),
                );
            }
            Figures::Nothing => {
                detail = detail.child(
                    ui::Label::new("No figures — this experiment drew none.")
                        .muted()
                        .size(ui::Size::Compact),
                );
            }
            Figures::Fetching => {
                detail = detail.child(
                    ui::Label::new("Fetching this experiment's figures…")
                        .muted()
                        .size(ui::Size::Compact),
                );
            }
            Figures::Unread => {
                detail = detail.child(
                    ui::Label::new("Figures have not been read for this experiment.")
                        .muted()
                        .size(ui::Size::Compact),
                );
            }
        }
        detail
    }

    /// A finished discovery run: the search as a tree, and its experiments ranked.
    pub(crate) fn discovery_modal(&self, view: &DiscoveryView, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div().flex().flex_col().w_full().min_w_0().gap_3();

        let scored = view
            .experiments
            .iter()
            .filter(|experiment| experiment.surprise.is_some())
            .count();
        let flagged = view
            .experiments
            .iter()
            .filter(|experiment| experiment.surprising)
            .count();
        let failed = view
            .experiments
            .iter()
            .filter(|experiment| experiment.is_finished() && !experiment.succeeded())
            .count();
        body = body.child(
            ui::Label::new(if view.loading {
                "Reading the run…".to_string()
            } else {
                let mut parts = vec![format!(
                    "{} experiment{}",
                    view.experiments.len(),
                    if view.experiments.len() == 1 { "" } else { "s" }
                )];
                if !view.complete {
                    parts.push("still running".to_string());
                }
                if failed > 0 {
                    parts.push(format!("{failed} failed"));
                }
                parts.push(format!("{scored} scored"));
                parts.push(format!("{flagged} flagged surprising by the service"));
                parts.join(" · ")
            })
            .muted()
            .size(ui::Size::Compact),
        );

        if let Some(error) = &view.error {
            body = body.child(
                ui::Label::new(error.clone())
                    .colour(theme::error())
                    .size(ui::Size::Compact),
            );
        }

        if !view.experiments.is_empty() {
            body = body
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .gap_2()
                        .child(div().flex_grow())
                        .child(
                            ui::Button::new(
                                "discovery-sort",
                                if view.loudest_first {
                                    "Biggest shift first"
                                } else {
                                    "Smallest shift first"
                                },
                            )
                            .size(ui::Size::Compact)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                if let Some(view) = workbench.discovery_open.as_mut() {
                                    view.loudest_first = !view.loudest_first;
                                }
                                cx.notify();
                            })),
                        )
                        .child(
                            ui::Button::new(
                                "discovery-expand",
                                if view.expanded { "Shrink" } else { "Full screen" },
                            )
                            .size(ui::Size::Compact)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                if let Some(view) = workbench.discovery_open.as_mut() {
                                    view.expanded = !view.expanded;
                                }
                                cx.notify();
                            })),
                        ),
                )
                .child(self.discovery_tree(view, cx))
                .child(match view.selected.and_then(|at| view.experiments.get(at)) {
                    Some(experiment) => {
                        self.discovery_detail(view, experiment, cx).into_any_element()
                    }
                    None => self.discovery_list(view, cx).into_any_element(),
                });
        } else if !view.loading && view.error.is_none() {
            body = body.child(
                ui::Label::new(
                    "This run recorded no experiments. Its status and any failure are in its own \
                     folder, under `discovery/`.",
                )
                .muted()
                .size(ui::Size::Compact),
            );
        }

        ui::Modal::new("discovery-results", if view.name.is_empty() {
            "Discovery run".to_string()
        } else {
            view.name.clone()
        })
        .width(if view.expanded { 1180. } else { 760. })
        .focus(&self.delete_focus)
        .body(body)
        .actions(
            ui::actions()
                .child(div().flex_grow())
                .child(
                    ui::Button::new("discovery-close", "Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.discovery_open = None;
                            cx.notify();
                        },
                    )),
                ),
        )
        .footer(
            ui::Label::new(
                "Every hypothesis and number here was produced by an AI system. Have a \
                 subject-matter expert check anything you intend to rely on.",
            )
            .muted()
            .size(ui::Size::Compact),
        )
    }

    /// The budget gate before a discovery run starts: cost, balance, and what to explore.
    pub(crate) fn approval_modal(&self, approval: &Approval, cx: &mut Context<Self>) -> impl IntoElement {
        let experiments = approval.experiments;
        let available = approval.cost.as_ref().and_then(|cost| cost.available);
        let over_budget = !affordable(experiments, available);

        let mut body = div().flex().flex_col().w_full().min_w_0().gap_3();

        if !approval.draft.name.is_empty() {
            body = body.child(ui::Label::new(approval.draft.name.clone()));
        }
        body = body.child(
            ui::Label::new(
                "AutoDiscovery writes its own hypotheses, runs code for each one and reports \
                 which results most changed its beliefs. It has not started.",
            )
            .muted()
            .size(ui::Size::Compact),
        );

        if !approval.draft.datasets.is_empty() {
            body = body.child(
                ui::Label::new(format!("Over {}", approval.draft.datasets.join(", ")))
                    .muted()
                    .size(ui::Size::Compact),
            );
        }

        body = body.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(section_label("EXPERIMENTS TO RUN"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .children(BUDGET_PRESETS.iter().map(|&preset| {
                            ui::Button::new(
                                SharedString::from(format!("budget-{preset}")),
                                preset.to_string(),
                            )
                            .size(ui::Size::Compact)
                            .tone(if preset == experiments {
                                ui::Tone::Accent
                            } else {
                                ui::Tone::Quiet
                            })
                            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                if let Some(approval) = workbench.approving.as_mut() {
                                    approval.experiments = preset;
                                }
                                cx.notify();
                            }))
                        }))
                        .child(
                            ui::Button::new("budget-down", "−")
                                .size(ui::Size::Compact)
                                .disabled(experiments <= 1)
                                .on_click(cx.listener(|workbench, _event, _window, cx| {
                                    if let Some(approval) = workbench.approving.as_mut() {
                                        approval.experiments =
                                            approval.experiments.saturating_sub(1).max(1);
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            ui::Button::new("budget-up", "+")
                                .size(ui::Size::Compact)
                                .disabled(experiments >= MAX_BUDGET)
                                .on_click(cx.listener(|workbench, _event, _window, cx| {
                                    if let Some(approval) = workbench.approving.as_mut() {
                                        approval.experiments =
                                            (approval.experiments + 1).min(MAX_BUDGET);
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                // `available` rather than `granted`: submitting moves credits to `pending`
                // straight away, so the grant overstates what is left by whatever is in flight.
                .child(
                    ui::Label::new(cost_line(experiments, available))
                    .colour(if over_budget {
                        theme::error()
                    } else {
                        theme::text_muted()
                    })
                    .size(ui::Size::Compact),
                ),
        );

        body = body.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(section_label("WHAT TO EXPLORE"))
                .child(self.filter_field(self.intent_field.clone(), cx))
                .child(
                    ui::Label::new(
                        "Steer it — \u{201c}focus on how rainfall relates to yield\u{201d}. Naming the \
                         answer you expect wastes what this tool is for.",
                    )
                    .muted()
                    .size(ui::Size::Compact),
                ),
        );

        if let Some(error) = &approval.error {
            body = body.child(
                ui::Label::new(error.clone())
                    .colour(theme::error())
                    .size(ui::Size::Compact),
            );
        }

        ui::Modal::new("discovery-approval", "Start this discovery run?")
            .width(560.)
            .focus(&self.delete_focus)
            .body(body)
            .actions(
                ui::actions()
                    .child(div().flex_grow())
                    .child(
                        ui::Button::new("discovery-reject", "Not now")
                            .disabled(approval.submitting)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                if let Some(approval) = workbench.approving.take() {
                                    workbench.declined.insert(approval.draft.run_id);
                                    workbench.status =
                                        "the discovery run is drafted and unstarted".into();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::Button::new(
                            "discovery-approve",
                            if approval.submitting {
                                "Starting…".to_string()
                            } else {
                                format!("Run {experiments} and spend {experiments}")
                            },
                        )
                        .tone(ui::Tone::Accent)
                        .disabled(
                            approval.submitting
                                || over_budget
                                || !ready_to_submit(approval.cost.as_ref()),
                        )
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.approve_discovery(cx);
                        })),
                    ),
            )
            .footer(
                ui::Label::new(if over_budget {
                    "That is more than the credits left.".to_string()
                } else if !ready_to_submit(approval.cost.as_ref()) {
                    "Checking the cost with the service…".to_string()
                } else {
                    "One credit per experiment. Nothing is spent until you press.".to_string()
                })
                .muted()
                .size(ui::Size::Compact),
            )
    }

    // ---- datasets ----

    /// Every dataset, in a searchable list.
    pub(crate) fn datasets_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        ui::Modal::new("datasets", format!("Datasets · {}", self.datasets.len()))
            .width(720.)
            .focus(&self.delete_focus)
            .body(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(self.filter_field(self.datasets_filter.clone(), cx))
                    .child(
                        div()
                            .id("all-datasets")
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .max_h(px(480.))
                            .overflow_y_scroll()
                            .child(self.datasets_section(None, cx))
                            .when(self.datasets.is_empty(), |list| {
                                list.child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .p_2()
                                        .text_color(rgb(theme::warning()))
                                        .text_xs()
                                        .child(
                                            "This turn's datasets arrived without the identifiers \
                                             needed to open or check them. The titles are in the \
                                             Outputs panel, and dataverse_search.json in this \
                                             conversation's folder has the full records.",
                                        ),
                                )
                            }),
                    ),
            )
            .actions(
                ui::actions().child(div().flex_grow()).child(
                    ui::Button::new("datasets-close", "Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.datasets_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
            .footer(
                ui::Label::new(
                    "Only datasets whose files are all public can be downloaded. Restricted ones \
                     stay listed — open the page to request access from CIP.",
                )
                .muted()
                .size(ui::Size::Compact),
            )
    }

    /// The dataset list, capped for the panel and whole for the modal.
    pub(crate) fn datasets_section(&self, limit: Option<usize>, cx: &mut Context<Self>) -> impl IntoElement {
        let query = match limit {
            Some(_) => String::new(),
            None => self.datasets_filter.read(cx).text().to_string(),
        };
        let matching: Vec<&protocol::Dataset> = self
            .datasets
            .iter()
            .filter(|dataset| {
                let haystack = format!(
                    "{} {} {}",
                    dataset.title,
                    dataset.authors.join(" "),
                    dataset.persistent_id
                );
                match_score(&query, &haystack).is_some()
            })
            .collect();

        let mut section = div().flex().flex_col().gap_2();
        for dataset in matching.into_iter().take(limit.unwrap_or(usize::MAX)) {
            section = section.child(self.dataset_row(dataset, cx));
        }
        section
    }

    /// One dataset: what it is, where it came from, and whether it can be had.
    pub(crate) fn dataset_row(&self, dataset: &protocol::Dataset, cx: &mut Context<Self>) -> impl IntoElement {
        let page = dataset.page();
        let id = dataset.persistent_id.clone();
        let access = self.dataset_access.get(&id);
        let downloading = self.downloading.contains(&id);
        let downloaded = self.downloaded.get(&id).cloned();

        // Two separate click targets: only the title/byline opens the page, so the download
        // button's own click never also triggers the browser.
        let mut row = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1();

        let mut opener = div()
            .id(SharedString::from(format!("dataset-{id}")))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .p_2()
            .rounded_lg()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text()))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(dataset.title.clone()),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(match dataset.authors.first() {
                        Some(author) if dataset.authors.len() > 1 => {
                            format!("{} et al. · {}", author, dataset.persistent_id)
                        }
                        Some(author) => format!("{author} · {}", dataset.persistent_id),
                        None => dataset.persistent_id.clone(),
                    }),
            );

        if let Some(url) = page.clone() {
            opener = opener
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style
                        .bg(rgb(fill))
                        .text_color(rgb(theme::ink_on(fill)))
                        .cursor_pointer()
                })
                .on_click(move |_event, _window, _cx| {
                    if let Err(error) = workspace::browse(&url) {
                        tracing::warn!(%error, "could not open a dataset");
                    }
                });
        }
        row = row.child(opener);

        if downloaded.is_none() && !downloading {
            if let Some(Ok(access)) = access {
                if access.refusal().is_none() {
                    let offer = access.offer();
                    let wanted = dataset.clone();
                    row = row.child(
                        div().px_2().pb_1().child(
                            ui::Button::new(SharedString::from(format!("get-{id}")), offer)
                                .tone(ui::Tone::Accent)
                                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                    cx.stop_propagation();
                                    workbench.download_dataset(wanted.clone(), cx);
                                })),
                        ),
                    );
                }
            }
        }
        row
    }

    // ---- documents (library) ----

    /// The researcher's own library, in a searchable list.
    pub(crate) fn documents_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        ui::Modal::new("documents", format!("Library · {}", self.documents.len()))
            .width(720.)
            .focus(&self.delete_focus)
            .body(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(self.filter_field(self.documents_filter.clone(), cx))
                    .child(
                        div()
                            .id("all-documents")
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .max_h(px(480.))
                            .overflow_y_scroll()
                            .child(self.documents_section(cx)),
                    ),
            )
            .actions(
                ui::actions().child(div().flex_grow()).child(
                    ui::Button::new("documents-close", "Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.documents_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
            .footer(
                ui::Label::new(
                    "Press a document to open it. Ask the pdf_librarian subagent to search this \
                     library by meaning rather than by filename.",
                )
                .muted()
                .size(ui::Size::Compact),
            )
    }

    /// The filtered document list.
    pub(crate) fn documents_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.documents_filter.read(cx).text().to_string();
        let matching: Vec<&protocol::Document> = self
            .documents
            .iter()
            .filter(|document| {
                let haystack = format!(
                    "{} {} {}",
                    document.title,
                    document.tags.join(" "),
                    document.summary
                );
                match_score(&query, &haystack).is_some()
            })
            .collect();

        let mut section = div().flex().flex_col().gap_1();
        if matching.is_empty() && !query.trim().is_empty() {
            return section.child(
                ui::Label::new("No document matches that.")
                    .muted()
                    .size(ui::Size::Compact),
            );
        }
        for document in matching {
            section = section.child(self.document_row(document));
        }
        section
    }

    /// One indexed document: what it is, what it is about, and where it lives.
    pub(crate) fn document_row(&self, document: &protocol::Document) -> impl IntoElement {
        let openable = workspace::local_path(
            &document.path,
            self.thread_workspace().as_deref(),
        );
        let mut row = div()
            .id(SharedString::from(format!("doc-{}", document.path)))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .p_2()
            .rounded_lg()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text()))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .child(document.title.clone()),
            );

        if !document.tags.is_empty() {
            row = row.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::accent()))
                    .text_xs()
                    .child(document.tags.join(" · ")),
            );
        }
        if !document.summary.is_empty() {
            row = row.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(document.summary.clone()),
            );
        }
        row = row.child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(theme::text_faint()))
                .text_size(px(11.))
                .child(match (&document.doi, document.page_count) {
                    (Some(doi), Some(pages)) => format!("{doi} · {pages} pages · {}", document.path),
                    (Some(doi), None) => format!("{doi} · {}", document.path),
                    (None, Some(pages)) => format!("{pages} pages · {}", document.path),
                    (None, None) => document.path.clone(),
                }),
        );

        if let Some(file) = openable {
            row = row
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style
                        .bg(rgb(fill))
                        .text_color(rgb(theme::ink_on(fill)))
                        .cursor_pointer()
                })
                .on_click(move |_event, _window, _cx| {
                    if let Err(error) = workspace::open(&file) {
                        tracing::warn!(%error, "could not open a document");
                    }
                });
        }
        row
    }

    // ---- sources (references) ----

    /// Every reference, in a searchable, scrollable list.
    pub(crate) fn sources_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let unverified = self.unverified_sources();
        ui::Modal::new("sources", format!("Sources · {}", self.sources.len()))
            .width(720.)
            .focus(&self.delete_focus)
            .body(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(self.filter_field(self.sources_filter.clone(), cx))
                    .child(
                        div()
                            .id("all-sources")
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_w_0()
                            .gap_1()
                            .max_h(px(480.))
                            .overflow_y_scroll()
                            .child(self.sources_section(None, cx)),
                    ),
            )
            .actions(
                ui::actions().child(div().flex_grow()).child(
                    ui::Button::new("sources-close", "Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.sources_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
            .footer(
                ui::Label::new(match unverified {
                    0 => "Every reference here came from a search or was checked against a \
                          registry."
                        .to_string(),
                    n => format!(
                        "{n} of these came from the model rather than from a search — confirm \
                         them before citing."
                    ),
                })
                .muted()
                .size(ui::Size::Compact),
            )
    }

    /// The reference list, capped for the panel and whole for the modal.
    pub(crate) fn sources_section(&self, limit: Option<usize>, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .when(!self.sources.is_empty(), |section| {
                section
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .child(section_label_owned(match self.unverified_sources() {
                        0 => format!("SOURCES · {}", self.sources.len()),
                        n => format!("SOURCES · {} · {n} UNVERIFIED", self.sources.len()),
                    }))
            });

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

        let query = match limit {
            Some(_) => String::new(),
            None => self.sources_filter.read(cx).text().to_string(),
        };
        let matching: Vec<(usize, &protocol::Source)> = self
            .sources
            .iter()
            .enumerate()
            // Numbered before filtering, so `[3]` keeps meaning the third reference of the
            // answer rather than being renumbered by the filter.
            .filter(|(_, source)| match_score(&query, &source.citation).is_some())
            .collect();
        let showing = limit.unwrap_or(matching.len());
        for (at, source) in matching.into_iter().take(showing) {
            let verdict = self.checked.get(&source.citation);
            // `None` is not looked up yet; `Some(None)` is looked up and the registry has
            // nothing. Kept unflattened so the match below can tell the three states apart.
            let looked_up = self.repaired.get(&source.citation);
            let repair = looked_up.cloned().flatten();
            let link = scholar_link(source, verdict, repair.as_ref());
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
                .when_some(link.clone(), |row, url| {
                    row.hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::browse(&url) {
                            tracing::warn!(%error, "could not open a source");
                        }
                    })
                })
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
            // Falls back to saying where the citation came from, so every case has some note.
            let note = note.or_else(|| {
                references::origin(verdict, looked_up.map(Option::is_some))
                    .note()
                    .map(|text| (theme::warning(), text.to_string()))
            });
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

        if showing == 0 && !query.trim().is_empty() {
            section = section.child(
                ui::Label::new("No reference matches that.")
                    .muted()
                    .size(ui::Size::Compact),
            );
        }

        let hidden = self.sources.len().saturating_sub(showing);
        if hidden > 0 {
            section = section.child(
                div()
                    .id("open-all-sources")
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
                    .text_color(rgb(theme::accent()))
                    .hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .child(ui::Label::new(format!("+{hidden} more")).inherit().size(ui::Size::Compact))
                    .child(
                        ui::Label::new("open all")
                            .inherit()
                            .size(ui::Size::Compact),
                    )
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.sources_open = true;
                        cx.notify();
                    })),
            );
        }
        section
    }

    // ---- commands ----

    /// Everything this conversation ran, newest first.
    pub(crate) fn commands_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let commands = self.thread_commands();
        let escaped = commands.iter().filter(|command| command.escaped()).count();

        let mut body = div().flex().flex_col().w_full().min_w_0().gap_2().child(
            ui::Label::new(
                "Every command this conversation ran. A path shown in the accent colour was \
                 watched appearing while the command ran, so the command wrote it; a faint one \
                 was only mentioned and may have been read. Either way a command can write \
                 somewhere it never names, and nothing here can see that.",
            )
            .muted()
            .size(ui::Size::Compact),
        );

        for command in commands.iter().rev() {
            let mut row = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .p_2()
                .rounded_lg()
                .bg(rgb(theme::surface()));

            let mut heading = command.at.clone();
            if let Some(seconds) = command.seconds {
                heading.push_str(&format!(" · {seconds:.1}s"));
            }
            match command.exit {
                Some(0) | None => {}
                Some(code) => heading.push_str(&format!(" · exit {code}")),
            }
            row = row.child(
                ui::Label::new(heading)
                    .colour(if command.failed() {
                        theme::error()
                    } else {
                        theme::text_faint()
                    })
                    .size(ui::Size::Compact),
            );

            row = row.child(
                div()
                    .w_full()
                    .min_w_0()
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(rgb(theme::text()))
                    .child(command.text.clone()),
            );
            if command.clipped {
                row = row.child(
                    ui::Label::new("(clipped — the full command is in the backend log)")
                        .muted()
                        .size(ui::Size::Compact),
                );
            }

            for path in &command.outside {
                let (verb, tone) = if command.wrote.contains(path) {
                    ("wrote, outside this conversation", theme::accent())
                } else {
                    ("named but not written", theme::text_faint())
                };
                row = row.child(
                    ui::Label::new(format!("{verb}: {path}"))
                        .colour(tone)
                        .size(ui::Size::Compact),
                );
            }
            body = body.child(row);
        }

        let title = if escaped > 0 {
            format!("What ran · {} · {escaped} outside", commands.len())
        } else {
            format!("What ran · {}", commands.len())
        };
        match &self.collecting {
            Some(Ok(collected)) => {
                for (path, name) in &collected.brought {
                    body = body.child(
                        ui::Label::new(format!("brought in as {name} — from {path}"))
                            .colour(theme::accent())
                            .size(ui::Size::Compact),
                    );
                }
                for (path, reason) in &collected.refused {
                    body = body.child(
                        ui::Label::new(format!("left where it was: {path} — {reason}"))
                            .muted()
                            .size(ui::Size::Compact),
                    );
                }
            }
            Some(Err(error)) => {
                body = body.child(
                    ui::Label::new(format!("could not bring them in: {error}"))
                        .colour(theme::error())
                        .size(ui::Size::Compact),
                );
            }
            None => {}
        }
        if let Some(Ok(collected)) = &self.collecting {
            if collected.brought.is_empty() && collected.refused.is_empty() {
                body = body.child(
                    ui::Label::new(collected_sentence(collected))
                        .muted()
                        .size(ui::Size::Compact),
                );
            }
        }

        // Offered only when a command was watched writing somewhere — never for a path that was
        // merely named, which may be the researcher's own input.
        let written = files_left_outside(&commands).len();
        let mut actions = ui::actions().child(div().flex_grow());
        if written > 0 {
            actions = actions.child(
                ui::Button::new(
                    "collect-outside",
                    if self.collect_in_flight {
                        "Bringing them in…".to_string()
                    } else {
                        format!("Copy {written} file{} into this conversation", if written == 1 { "" } else { "s" })
                    },
                )
                .tone(ui::Tone::Accent)
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.collect_outside(cx);
                })),
            );
        }

        ui::Modal::new("commands", title)
            .width(820.)
            .focus(&self.delete_focus)
            .body(body)
            .actions(
                actions.child(
                    ui::Button::new("commands-close", "Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.commands_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
    }
}
