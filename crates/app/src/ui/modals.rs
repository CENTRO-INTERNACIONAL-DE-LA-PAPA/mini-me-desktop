// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    pub(crate) fn context_menu(&self, open: menu::ContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let target = open.target;
        let mut popup = ui::Menu::new(open.at)
            // A right-click elsewhere re-opens this menu at the new spot, and that handler is
            // the only one that should decide whether it closes.
            .ignore_right_click(true);

        for &item in open.items() {
            let enabled = self.menu_item_enabled(item, target, cx);
            popup = popup.item(
                ui::MenuItem::new(SharedString::from(format!("menu-{}", item.label())), item.label())
                    .trailing(item.shortcut(target))
                    .disabled(!enabled)
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.run_menu_item(item, target, window, cx);
                    })),
            );
        }

        // Clicking anywhere else closes it, which is the only way out most people look for.
        popup.on_dismiss(cx.listener(|workbench, _event, _window, cx| {
            workbench.context_menu = None;
            cx.notify();
        }))
    }
}


impl Workbench {
    /// The irreversible scope, in the centre of the window rather than squeezed into a row.
    ///
    /// Conversation deletion now includes its saved outputs, and project deletion includes every
    /// conversation plus the complete project folder. The old inline "delete / keep" row had no
    /// room to say either fact; confirmation without the consequence is only a second click
    /// (§155).
    /// Confirm a provider change, and say what it will actually mean.
    ///
    /// **Which provider is selected decides which account gets billed**, and until §186 the only
    /// thing that said so was which pill was lit. That was enough to lose an afternoon: a turn ran
    /// against a provider the researcher had not chosen, and the first news of it was an
    /// out-of-credits page belonging to somebody else's API — *"this is weird, I set OpenRouter
    /// and I have credits."*
    ///
    /// So the modal states the three facts a person needs before pressing anything, and it reads
    /// them **from the keychain and the settings**, not from what the panel happens to show:
    ///
    /// 1. Whether a key is stored **for the provider being moved to**. Keys are filed per
    ///    provider (`llm:<id>`), so one pasted while another pill was selected belongs to that
    ///    one and is invisible here — which is exactly how the key goes missing.
    /// 2. That a custom endpoint needs its base URL, since without it the request has no address
    ///    and the backend falls back to OpenAI's.
    /// 3. Which model id it is about to be set to, because changing the provider changes that
    ///    too, and doing it silently is how a valid id turns into one that does not exist.
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

        // The two that stop a turn, said here rather than discovered several minutes into one.
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
                    .child(
                        ui::Button::new("provider-cancel").text("Cancel").on_click(cx.listener(
                            |workbench, _event, _window, cx| {
                                workbench.confirming_provider = None;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        ui::Button::new("provider-confirm")
                            .text("Switch provider")
                            .style(ui::ButtonStyle::Primary)
                            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                workbench.confirming_provider = None;
                                workbench.draft.provider = spec.id.to_string();
                                // A different provider has a different catalogue, and the one on
                                // screen a moment ago belonged to the provider being left.
                                workbench.refresh_models(cx);
                                // A model that exists for the provider just chosen, rather than
                                // leaving one that does not.
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
}



impl Workbench {
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
                        ui::Button::new("delete-cancel").text("Cancel").on_click(cx.listener(
                            |workbench, _event, _window, cx| {
                                workbench.confirming_delete = None;
                                workbench.restore_focus = true;
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        ui::Button::new("delete-confirm")
                            .text(action)
                            .style(ui::ButtonStyle::Danger)
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
}


impl Workbench {
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
    pub(crate) fn about_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(ui::Label::new("THE SPECIALISTS").colour(theme::text_faint()).size(ui::Size::Compact))
            .child(team)
            .child(ui::Label::new("WHERE THE DATA COMES FROM").colour(theme::text_faint()).size(ui::Size::Compact))
            .child(sources)
            .child(ui::Label::new("THIS BUILD").colour(theme::text_faint()).size(ui::Size::Compact))
            // **Because a tester's report is unusable without it.** The app has never shown its
            // own version anywhere: not in the window, not in the log, not in the About page. It
            // logged the *backend* checkout's commit as its very first line (§115) and said nothing
            // about itself — so "it doesn't work" from a second machine could be any of 183
            // commits, and the first question back would always be the same one (§213).
            //
            // Selectable, like the citation below, because the whole point is pasting it into a
            // message.
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
                    // Where the version already is, because that is where someone goes to ask
                    // "what am I running" — and "is there a newer one" is the same question with
                    // one more word. A separate pane for it would be a pane nobody opens.
                    .child(
                        ui::Label::new(match &self.update {
                            Some(standing) => update::describe(standing, &self.install),
                            // Said out loud, so the gap between launching and answering does not
                            // read as "there is nothing to report".
                            None => "checking for a newer build…".to_string(),
                        })
                        .muted()
                        .size(ui::Size::Compact),
                    )
                    .children(self.update_action(cx)),
            )
            .child(ui::Label::new("WHERE CODE RUNS").colour(theme::text_faint()).size(ui::Size::Compact))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(ui::Label::new(execution.0).colour(theme::accent()))
                    .child(ui::Label::new(execution.1).muted().size(ui::Size::Compact)),
            )
            .child(ui::Label::new("CITING THIS WORK").colour(theme::text_faint()).size(ui::Size::Compact))
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
                ui::Button::new("about-close").text("Close").on_click(cx.listener(
                    |workbench, _event, _window, cx| {
                        workbench.about_open = false;
                        workbench.restore_focus = true;
                        cx.notify();
                    },
                )),
            ))
    }
}


impl Workbench {
    /// The approval card: the command, verbatim, and the two decisions.
    ///
    /// Deliberately shows the command rather than a summary. Host execution means this
    /// runs on the researcher's own machine with their permissions, and the only
    /// meaningful review is of the actual text (docs §19).
    pub(crate) fn approval_card(&self, request: &ApprovalRequest, cx: &mut Context<Self>) -> impl IntoElement {
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
                    ui::Button::new("approve").text("Approve")
                        .style(ui::ButtonStyle::Primary)
                        .on_click(
                            cx.listener(|workbench, _event, _window, cx| {
                                workbench.decide(true, cx)
                            }),
                        ),
                )
                .child(ui::Button::new("reject").text("Reject").on_click(
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
                    ui::Button::new("approve-turn").text("Approve the rest of this turn")
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
                    ui::Button::new("approve-conversation")
                    .text("Approve everything in this conversation")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.approve_conversation = true;
                        workbench.decide(true, cx);
                    })),
                ),
        )
    }
}


impl Workbench {
    /// The search, drawn as the tree it is.
    ///
    /// **A tree and not the force-directed graph the service's own view shows.** Same data, and
    /// the reasons are in `discovery.rs`: a spring layout settles differently every frame in a
    /// panel that is rebuilt on every stream event, and depth is the one thing a blob cannot
    /// show — how far the search kept refining one line of enquiry.
    ///
    /// Edges are three axis-aligned pieces each. gpui draws rectangles, and elbows are exact
    /// where a rotated div would be approximate.
    pub(crate) fn discovery_tree(&self, view: &DiscoveryView, cx: &mut Context<Self>) -> impl IntoElement {
        let placed = discovery::layout(&view.experiments);
        let (width, height) = discovery::canvas(&placed);
        let mut canvas = div()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(height));

        // Connectors first, so a node is never drawn under its own edge.
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
                    // The server's own `is_surprising` gets its own mark rather than being folded
                    // into the colour: loudness is a number we banded, and that flag is the
                    // service's judgment. Two different claims, drawn differently.
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
                        // Pressing the open one closes it, back to the ranked list.
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
}


impl Workbench {
    /// Every experiment, ranked by how far it moved a belief.
    ///
    /// The order is the reason to read this at all: the point of a discovery run is the handful of
    /// results that changed the picture, and creation order buries them among the ones that did
    /// not. Ranked on `|surprise|`, so an experiment that moved a belief hard *against* its
    /// hypothesis ranks as high as one that confirmed it — which is the interesting case.
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
                // Magnitude and direction as separate columns, the way the service's own table
                // does it — the sign lives in `surprise` and is not derived from the beliefs.
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
}


impl Workbench {
    /// One experiment, opened.
    ///
    /// The order is the service's own and `interpreting-results.md` asks for it: the belief shift,
    /// the hypothesis, the analysis, then the review. Code is not shown — it is in the persisted
    /// `.json`, and a researcher reading results is not reading Python.
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
                    ui::Button::new("discovery-back").text("All experiments")
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
        // The service's own flag, said in words. It is not a threshold on the number above it —
        // the probe had a 0.67 shift the service called unsurprising — so the two are reported
        // side by side rather than one derived from the other.
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
            // **Rendered, not printed.** The service writes real Markdown here — `###` headings,
            // `- ` lists, `**bold**` labels — and a raw dump of it was on screen: *"The modal of
            // autodiscovery doesnt render well markdown."* `markdown_block` is the transcript's own
            // renderer, and `None` for the selection registry is the mode the file preview already
            // uses: the same blocks, not part of a conversation (§266).
            let mut rendered = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .flex_none()
                .gap_1()
                .child(ui::Label::new(heading).colour(theme::text_faint()).size(ui::Size::Compact));
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

        // **The plots the experiment actually drew.** They exist only in the per-experiment
        // response, so they arrive after the text and the pane has to distinguish three states:
        // asking, none, and here. Conflating the first two makes an experiment with no figure look
        // permanently stuck (§257).
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
                        .child(ui::Label::new("FIGURES").colour(theme::text_faint()).size(ui::Size::Compact))
                        .children(paths.iter().enumerate().map(|(at, path)| {
                            let opening = path.clone();
                            div()
                                .id(SharedString::from(format!("fig-{}-{at}", experiment.id)))
                                // **The output gallery's exact shape, and not a near-miss.** The
                                // first version added `min_w_0` to a block `div` — gpui's default
                                // display — so the `img`'s own `w_full` had nothing definite to
                                // resolve against and rendered at zero width. The bordered box
                                // appeared, empty, while the same file drew fine in the panel.
                                // §88 and §59 are both about exactly this, one layer up (§263).
                                .relative()
                                .flex()
                                .flex_row()
                                .w_full()
                                .flex_none()
                                // A fixed height with `Contain`: a scree plot cropped to a square
                                // is a scree plot you cannot read, and §152 already settled that
                                // for the output gallery.
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
                                // Opens it full size in the researcher's own viewer, because a
                                // 260px band is for recognising a plot and not for reading one.
                                .on_click(move |_event, _window, _cx| {
                                    if let Err(error) = workspace::open(&opening) {
                                        tracing::warn!(%error, "could not open a figure");
                                    }
                                })
                        })),
                );
            }
            Figures::Nothing => {
                // Asked, and this experiment genuinely produced none. Said out loud so it does not
                // read as a pane that failed to finish loading.
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
}


impl Workbench {
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
                    // Said out loud, because a tree that grows between two openings is otherwise
                    // indistinguishable from one that was drawn wrong.
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
                        // Named in words rather than an arrow glyph. Our researchers are not
                        // developers, and §199's rule is that an affordance nobody can name is one
                        // they conclude does not exist.
                        .child(
                            ui::Button::new("discovery-sort")
                            .text(if view.loudest_first {
                                "Biggest shift first"
                            } else {
                                "Smallest shift first"
                            })
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                if let Some(view) = workbench.discovery_open.as_mut() {
                                    view.loudest_first = !view.loudest_first;
                                }
                                cx.notify();
                            })),
                        )
                        .child(
                            ui::Button::new("discovery-expand")
                            .text(if view.expanded { "Shrink" } else { "Full screen" })
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
        // Wide enough to read an analysis in when expanded, and a list-scanning width otherwise.
        // 1180 rather than "the whole window": `ui::Modal` centres on a fixed width, and prose
        // running the full width of a 4K display is unreadable for the opposite reason.
        .width(if view.expanded { 1180. } else { 760. })
        .focus(&self.delete_focus)
        .body(body)
        .actions(
            ui::actions()
                .child(div().flex_grow())
                .child(
                    ui::Button::new("discovery-close").text("Close").on_click(cx.listener(
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
}


impl Workbench {
    /// The budget gate.
    ///
    /// **A modal, deliberately, and §244 is the argument for it rather than against.** That section
    /// refused a modal for a background run that had already finished: nothing was pending, the
    /// researcher had somewhere to go, and a modal would have been a toll booth in front of work
    /// they could get on with. This is the other kind of thing. Nothing proceeds until it is
    /// answered, it has three outcomes rather than one, and the wrong one spends credits that do
    /// not come back. A banner is for something already true; a modal is for something that cannot
    /// happen without you.
    ///
    /// Three things it must say, and the order is the order: what will run, what it costs against
    /// what is left, and how to change it before agreeing.
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

        // --- the budget, which is the price ------------------------------------------------
        body = body.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(ui::Label::new("EXPERIMENTS TO RUN").colour(theme::text_faint()).size(ui::Size::Compact))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap_1()
                        .children(BUDGET_PRESETS.iter().map(|&preset| {
                            ui::Button::new(SharedString::from(format!("budget-{preset}")))
                                .text(preset.to_string())
                                .toggle(true)
                                .active(preset == experiments)
                            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                if let Some(approval) = workbench.approving.as_mut() {
                                    approval.experiments = preset;
                                }
                                cx.notify();
                            }))
                        }))
                        .child(
                            ui::Button::new("budget-down").text("−")
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
                            ui::Button::new("budget-up").text("+")
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
                // The cost and the balance in one sentence, because they are one question. And
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

        // --- the one field worth changing at the gate --------------------------------------
        body = body.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(ui::Label::new("WHAT TO EXPLORE").colour(theme::text_faint()).size(ui::Size::Compact))
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
                        ui::Button::new("discovery-reject").text("Not now")
                            .disabled(approval.submitting)
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                if let Some(approval) = workbench.approving.take() {
                                    // Remembered, so the next snapshot does not ask again. The
                                    // run stays drafted and unspent, which is what "not now"
                                    // means — nothing is deleted.
                                    workbench.declined.insert(approval.draft.run_id);
                                    workbench.status =
                                        "the discovery run is drafted and unstarted".into();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        ui::Button::new("discovery-approve")
                        .text(if approval.submitting {
                            "Starting…".to_string()
                        } else {
                            format!("Run {experiments} and spend {experiments}")
                        })
                        .style(ui::ButtonStyle::Primary)
                        // Not while a press is in flight, and not for a budget the balance cannot
                        // cover: the service would refuse it, and letting someone press a button
                        // that fails is worse than not offering it.
                        // Unpressable until the token is in hand. A press that could only fail is
                        // worse than a button that says "not yet" by being disabled (§252).
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
}


impl Workbench {
    /// Every dataset, in a list you scroll and search — the treatment the references got.
    ///
    /// *"we can search the datasets in a modal and click to be redirected to the pages."* The
    /// panel could not do this before because the client kept datasets as bare truncated titles,
    /// which is how five distinct records from one multi-site study rendered as five identical
    /// rows (see [`protocol::Dataset`]).
    pub(crate) fn datasets_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        ui::Modal::new(
            "datasets",
            format!(
                "Datasets · {}",
                datasets_heading(self.datasets.len(), self.search_totals)
            ),
        )
            .width(720.)
            .focus(&self.delete_focus)
            .body(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    // Outside the scroll region, so it cannot scroll away from what it filters.
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
                            // A heading that opens onto nothing is indistinguishable from a
                            // heading that does nothing. If the bucket has titles but the
                            // structured records did not decode, say so here instead.
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
            .actions({
                // Named in words, and only when it has something to do — the same rule the copy
                // button follows (§279): a control that says "Download 0" is one somebody presses
                // to find out what it means.
                let picked = self.picked_datasets().len();
                let mut actions = ui::actions().child(div().flex_grow());
                if picked > 0 {
                    actions = actions.child(
                        ui::Button::new("datasets-download-picked")
                        .text(format!(
                            "Download {picked} dataset{} into this conversation",
                            if picked == 1 { "" } else { "s" }
                        ))
                        .style(ui::ButtonStyle::Primary)
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.download_picked(cx);
                        })),
                    );
                }
                actions.child(ui::Button::new("datasets-close").text("Close").on_click(cx.listener(
                    |workbench, _event, _window, cx| {
                        workbench.datasets_open = false;
                        workbench.restore_focus = true;
                        cx.notify();
                    },
                )))
            })
            .footer(
                ui::Label::new(
                    "Only datasets whose files are all public can be downloaded. Restricted ones \
                     stay listed — open the page to request access from CIP.",
                )
                .muted()
                .size(ui::Size::Compact),
            )
    }
}


impl Workbench {
    /// The dataset list, capped for the panel and whole for the modal.
    ///
    /// One function rather than two, for the reason `sources_section` is one: a compact list and
    /// a full one are the same rows with a different count, and written separately the download
    /// gate ends up in one of them (docs §194).
    pub(crate) fn datasets_section(&self, limit: Option<usize>, cx: &mut Context<Self>) -> impl IntoElement {
        let query = match limit {
            Some(_) => String::new(),
            None => self.datasets_filter.read(cx).text().to_string(),
        };
        // Scored over title, authors and identifier together, so `andrade 0F9T62` and `israel`
        // both find what a researcher would expect them to.
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

        // **The agent's picks first, and only that.** Its reading is worth something — it read
        // the descriptions — but it is a *sort*, not a filter: everything the search returned
        // stays on the list, because the researcher is the one choosing (§290).
        let mut ordered = matching;
        ordered.sort_by_key(|dataset| !self.was_recommended(dataset));

        let mut section = div().flex().flex_col().gap_2();
        for dataset in ordered.into_iter().take(limit.unwrap_or(usize::MAX)) {
            section = section.child(self.dataset_row(dataset, cx));
        }
        section
    }
}


impl Workbench {
    /// One dataset: what it is, where it came from, and whether it can be had.
    pub(crate) fn dataset_row(&self, dataset: &protocol::Dataset, cx: &mut Context<Self>) -> impl IntoElement {
        let page = dataset.page();
        let id = dataset.persistent_id.clone();
        let access = self.dataset_access.get(&id);
        let downloading = self.downloading.contains(&id);
        let downloaded = self.downloaded.get(&id).cloned();

        // **Two targets, so two shapes.** The whole row used to carry the page link, with the
        // download button sitting inside it — so the hover fill covered both, and pressing the
        // button opened the browser as well as downloading. *"the hover colour both the doi
        // redirect and the download data button. There must be a distinction there."*
        //
        // Right, and the fix is structural rather than a `stop_propagation`: a highlight is a
        // promise about what a press will do, and one that spans two different actions is a
        // wrong promise however the events are routed. Only the title and byline open the page,
        // and they are the only part that lights up.
        let mut row = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1();

        // Said on the row the agent chose, not by leaving the others out. The list is the
        // search's; this is the agent's opinion of it, and a reader can disagree.
        if self.was_recommended(dataset) {
            row = row.child(
                ui::Label::new("the agent put this one forward")
                    .colour(theme::accent())
                    .size(ui::Size::Compact),
            );
        }

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
                    // The identifier, always: it is what tells two records of one study apart,
                    // and what a researcher pastes into a citation.
                    .child(match dataset.authors.first() {
                        Some(author) if dataset.authors.len() > 1 => {
                            format!("{} et al. · {}", author, dataset.persistent_id)
                        }
                        Some(author) => format!("{author} · {}", dataset.persistent_id),
                        None => dataset.persistent_id.clone(),
                    }),
            );

        // Only when there is somewhere to go, so a row never lights up and then does nothing.
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

        // The button appears only for a dataset the server says is entirely public, and its label
        // carries the size — so pressing it is never a surprise.
        if downloaded.is_none() && !downloading {
            if let Some(Ok(access)) = access {
                if access.refusal().is_none() {
                    let offer = access.offer();
                    let wanted = dataset.clone();
                    let picked = self.dataset_picks.contains(&id);
                    let pick_id = id.clone();
                    row = row.child(
                        div().px_2().pb_1().flex().flex_row().items_center().gap_2().child(
                            // The tick. Deliberately a word rather than a checkbox glyph: this
                            // list is read by someone deciding what to keep, and "selected" says
                            // what the state is without needing the convention explained.
                            div()
                                .id(SharedString::from(format!("pick-{pick_id}")))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(if picked {
                                    theme::accent()
                                } else {
                                    theme::border()
                                }))
                                .text_xs()
                                .text_color(rgb(if picked {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                }))
                                .hover(|style| {
                                    let fill = theme::hover_over(theme::surface());
                                    style.bg(rgb(fill)).cursor_pointer()
                                })
                                .child(if picked { "✓ selected" } else { "select" })
                                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                    cx.stop_propagation();
                                    workbench.toggle_dataset_pick(pick_id.clone(), cx);
                                })),
                        ).child(
                            ui::Button::new(SharedString::from(format!("get-{id}")))
                                .text(offer)
                                // Accent, because this is the action the list exists for and it
                                // must read as a control rather than as more of the row.
                                .style(ui::ButtonStyle::Primary)
                                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                    // Belt as well as braces: the opener is a sibling now, so
                                    // nothing is behind this — but a future nesting must not
                                    // silently reintroduce "download also opens the browser".
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
}


impl Workbench {
    /// The researcher's own library, in a list you can search.
    ///
    /// The third of these, after references (§194) and datasets (§223), and deliberately the same
    /// shape: a filter outside the scroll region and one section function serving both the panel
    /// and the modal. A library is the case that most wants searching — its whole purpose is to
    /// answer *"what do I have on this"* — and until now `LibraryArtifact` reached the client
    /// carrying titles, paths, summaries and tags, and the client kept none of it.
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
                    ui::Button::new("documents-close").text("Close").on_click(cx.listener(
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
}


impl Workbench {
    pub(crate) fn documents_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.documents_filter.read(cx).text().to_string();
        // Title, tags and summary together: a library is searched by what a paper is *about*, and
        // the summary is the only field that carries that.
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
            // Said, rather than left blank: a filter matching nothing and an empty library look
            // identical otherwise, and only one of them is fixed by typing less.
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
}


impl Workbench {
    /// One indexed document: what it is, what it is about, and where it lives.
    pub(crate) fn document_row(&self, document: &protocol::Document) -> impl IntoElement {
        // The whole row opens the file, because unlike a dataset there is no second action to
        // confuse it with (§225a) — and only when there is a file to open, so a URL-only entry
        // does not light up and then do nothing.
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
        // What the librarian recorded, verbatim. A researcher chasing a document the recorder
        // called missing needs the string it was looking for, not a prettier version of it.
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
}


impl Workbench {
    /// Every reference, in a list you scroll rather than a panel you fight.
    ///
    /// Asked for in these terms: *"a nice list that can scroll in y direction, like OS systems do
    /// in file explorers"* — and pointedly **not** the slider the images got. A figure is one
    /// thing you look at and the next is a different thing; a reference list is one object you
    /// read down. Paging through twenty-six citations one at a time would be the wrong gesture
    /// for the same reason paging through a folder would be (docs §194).
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
                    // The field sits outside the scroll region, so it cannot scroll away from
                    // the list it filters. `Modal::body` is itself a scroller, and the inner
                    // `max_h` means its content fits — so only the list moves.
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
                    ui::Button::new("sources-close").text("Close").on_click(cx.listener(
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
                    // The same count the panel header carries, from the same function, so the
                    // two cannot disagree about what "unverified" means (§185).
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
}


impl Workbench {
    /// The reference list, capped for the panel and whole for the modal.
    ///
    /// **One function rather than two, because they must agree.** A compact panel list and a full
    /// one are the same rows with a different count — and the moment they are written separately,
    /// the unverified mark or the link is in one and not the other (docs §194).
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
                    .child(
                        ui::Label::new(match self.unverified_sources() {
                            // **Counted where the eye lands, not only marked row by row.**
                            // Silence under a reference means "nothing wrong with this one", and
                            // until §185 it also meant "nothing checked this one" — so a
                            // researcher scanning fourteen citations had no way to know how many
                            // needed them. The header says how many, and the rows say which
                            // (docs §185).
                            0 => format!("SOURCES · {}", self.sources.len()),
                            n => format!("SOURCES · {} · {n} UNVERIFIED", self.sources.len()),
                        })
                        .colour(theme::text_faint())
                        .size(ui::Size::Compact),
                    )
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

        // Scored against the citation as written, which is what a researcher remembers: an
        // author's name, a year, a word from the title. The same fuzzy scorer as every other
        // filter here, so `2024 orchid` finds what you would expect it to.
        let query = match limit {
            Some(_) => String::new(),
            None => self.sources_filter.read(cx).text().to_string(),
        };
        let matching: Vec<(usize, &protocol::Source)> = self
            .sources
            .iter()
            .enumerate()
            // **Numbered before filtering.** `[3]` has to keep meaning the third reference of
            // the answer, or a filtered list renumbers the citations the prose points at.
            .filter(|(_, source)| match_score(&query, &source.citation).is_some())
            .collect();
        let showing = limit.unwrap_or(matching.len());
        for (at, source) in matching.into_iter().take(showing) {
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
                // **The whole row opens the paper, and lights up to say so.** Asked for after
                // §194 made the list long enough to read down: *"I would like to have a hover
                // colouring when I'm hovering a paper so when I click it I'll be redirected to
                // the web page."* A twelve-pixel word called `link` at the end of a four-line
                // citation is a target you aim at; the citation itself is the thing being
                // pointed at, so it should be the thing you press (docs §195).
                //
                // **Only when there is somewhere to go.** A reference nothing could resolve gets
                // no hover and no pointer, because a row that lights up and then does nothing is
                // worse than one that never offered (§185 marks those as unverified already).
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
            // **The holes the match above leaves.** Every arm answers *is this broken*, and
            // falling through means "nothing wrong" — which was also what a reference nothing had
            // checked looked like. `(NoIdentifier, None)` and `(Unregistered, None)` land here,
            // and so does a source with no verdict at all once resolution has stopped. Saying
            // where it came from is a different question, and one that has an answer in every
            // case (docs §185).
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

        // Said, rather than left as an empty panel: a filter matching nothing and a
        // conversation with no references look identical otherwise, and only one of them is
        // fixed by typing less.
        if showing == 0 && !query.trim().is_empty() {
            section = section.child(
                ui::Label::new("No reference matches that.")
                    .muted()
                    .size(ui::Size::Compact),
            );
        }

        // **The way in, and the count it hides.** A panel that lists twenty-six references in
        // full is a wall a researcher scrolls past to reach the files below it — the same problem
        // the images had before §152 grouped them behind one tile. The rest are one press away.
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
}


impl Workbench {
    /// Everything this conversation ran, newest first.
    ///
    /// **Newest first, unlike the file itself.** The record is written oldest-first because that is
    /// how it accumulates; it is read to answer "what did that just do", so the last command is the
    /// one being looked for.
    ///
    /// The heading states the limit rather than burying it in a docstring nobody here will read.
    /// The producer checks named paths and performs a bounded scan of the command's real cwd; if
    /// that scan stops early the row says so rather than turning an incomplete scan into silence.
    pub(crate) fn commands_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let commands = self.thread_commands();
        let escaped = commands.iter().filter(|command| command.escaped()).count();

        let mut body = div().flex().flex_col().w_full().min_w_0().gap_2().child(
            ui::Label::new(
                "Every command this conversation ran. A path shown in the accent colour was \
                 watched appearing while the command ran, so the command wrote it; a faint one \
                 was only mentioned and may have been read. Observation is bounded to the \
                 command's working directory; writes elsewhere or past its safety limit may not \
                 appear.",
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

            // The command as written. Monospace, because it is code and a proportional font makes
            // a shell pipeline unreadable at exactly the moment somebody is checking it carefully.
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
            if !command.cwd.is_empty() {
                row = row.child(
                    ui::Label::new(format!("working directory: {}", command.cwd))
                        .muted()
                        .size(ui::Size::Compact),
                );
            }

            // Two different sentences, because they are two different claims. A file watched to
            // appear during the command is a fact; a path merely mentioned may have been read.
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
            // A relative write found by the cwd scan does not appear in `outside`, because that
            // list means exactly "an absolute path named in the command text". Show both without
            // forcing one into the other; conflating them is the defect §301 closes.
            for path in command
                .wrote
                .iter()
                .filter(|path| !command.outside.contains(path))
            {
                row = row.child(
                    ui::Label::new(format!("wrote, outside this conversation: {path}"))
                        .colour(theme::accent())
                        .size(ui::Size::Compact),
                );
            }
            if command.scan_truncated {
                row = row.child(
                    ui::Label::new(
                        "working-directory scan stopped at its safety limit; later files may be absent",
                    )
                    .colour(theme::warning())
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
        // What happened last time the button was pressed, per file. A partial result is the
        // normal case — `/tmp` is swept — and a count would hide which ones did not make it.
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
        // And when both lists were empty, the backend's own sentence — otherwise the press draws
        // nothing and reads as broken, which is what it did.
        if let Some(Ok(collected)) = &self.collecting {
            if collected.brought.is_empty() && collected.refused.is_empty() {
                body = body.child(
                    ui::Label::new(collected_sentence(collected))
                        .muted()
                        .size(ui::Size::Compact),
                );
            }
        }

        // Offered only when a command was *watched writing* somewhere — never for a path that was
        // merely named, which may be the researcher's own input.
        let written = files_left_outside(&commands).len();
        let mut actions = ui::actions().child(div().flex_grow());
        if written > 0 {
            actions = actions.child(
                ui::Button::new("collect-outside")
                .text(if self.collect_in_flight {
                    "Bringing them in…".to_string()
                } else {
                    // Named in words: what the press will do, and to how many.
                    format!("Copy {written} file{} into this conversation", if written == 1 { "" } else { "s" })
                })
                .style(ui::ButtonStyle::Primary)
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
                    ui::Button::new("commands-close").text("Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.commands_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
    }

    /// Every answer a subagent gave, newest first, beside what the workspace held.
    ///
    /// Newest first for the same reason the command list is: this is read to answer "what did that
    /// just do", so the last answer is the one being looked for.
    ///
    /// The heading states the limit where the list is, rather than in a docstring nobody here will
    /// read: **this compares what was *said* against what is on disk, and nothing more.** A
    /// subagent that produced a file and described it wrongly looks the same as one that produced
    /// nothing, and neither is the same as one that lied about its findings — which nothing here
    /// can see at all.
    pub(crate) fn claims_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let claims = self.thread_claims();
        let contradicted = claims.iter().filter(|claim| claim.contradicted()).count();

        let mut body = div().flex().flex_col().w_full().min_w_0().gap_2().child(
            ui::Label::new(
                "What each subagent said it produced, beside what this conversation's folder \
                 actually holds. A path in the accent colour is one the workspace does not have; \
                 a faint one is real but sits outside this conversation, so it will not travel \
                 with it. This records and does not block — nothing here stopped a turn — and it \
                 compares names against disk, so a subagent that wrote a file and described it \
                 wrongly is invisible to it.",
            )
            .muted()
            .size(ui::Size::Compact),
        );

        for claim in claims.iter().rev() {
            let mut row = div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .p_2()
                .rounded_lg()
                .bg(rgb(theme::surface()));

            row = row.child(
                ui::Label::new(format!("{} · {}", claim.at, claim.source))
                    .colour(if claim.contradicted() {
                        theme::error()
                    } else {
                        theme::text_faint()
                    })
                    .size(ui::Size::Compact),
            );
            row = row.child(
                ui::Label::new(format!("answered with {}", claim.schema))
                    .muted()
                    .size(ui::Size::Compact),
            );

            // **Said before the lists, and separately.** A schema no rule covers produces no
            // missing files, and an empty list under a heading reads as "checked, all fine" — the
            // one answer this record is not allowed to imply.
            if claim.unexamined() {
                row = row.child(
                    ui::Label::new("nothing here checks this kind of answer")
                        .muted()
                        .size(ui::Size::Compact),
                );
            } else if claim.claimed > 0 {
                let verdict = if claim.missing.is_empty() {
                    format!("named {} path(s), all present", claim.claimed)
                } else {
                    format!(
                        "named {} path(s), {} not in this conversation",
                        claim.claimed,
                        claim.missing.len()
                    )
                };
                row = row.child(ui::Label::new(verdict).muted().size(ui::Size::Compact));
            }

            // A check that could not run. Neither clean nor an accusation, and it gets its own
            // sentence because in a log it is the same silence as success (§224).
            if let Some(note) = &claim.note {
                row = row.child(
                    // "nothing was compared", rather than "could not be checked", because one of
                    // the two notes is *recommended no datasets at all* — a fact about the run and
                    // not a failure of the check. Both are true under this framing.
                    ui::Label::new(format!("nothing was compared: {note}"))
                        .colour(theme::accent())
                        .size(ui::Size::Compact),
                );
            }

            if let Some(datasets) = claim.datasets {
                let verdict = if !claim.unsearched.is_empty() {
                    format!(
                        "recommended {datasets} dataset(s), {} absent from the search",
                        claim.unsearched.len()
                    )
                } else if claim.note.is_some() {
                    format!("recommended {datasets} dataset(s)")
                } else {
                    format!("recommended {datasets} dataset(s), all present in the search")
                };
                row = row.child(ui::Label::new(verdict).muted().size(ui::Size::Compact));
            }

            for path in &claim.missing {
                row = row.child(
                    ui::Label::new(format!("not in this conversation: {path}"))
                        .colour(theme::accent())
                        .size(ui::Size::Compact),
                );
            }
            // Real files, in the researcher's own folders. Faint, because calling these missing
            // once read as *this file does not exist*, which was false and cost the record its
            // credibility for a week.
            for path in &claim.outside {
                row = row.child(
                    ui::Label::new(format!("used from outside this conversation: {path}"))
                        .colour(theme::text_faint())
                        .size(ui::Size::Compact),
                );
            }
            // A citation composed from memory, which is the one thing here that leaves the app —
            // straight into a paper, if nobody says so.
            for identifier in &claim.unsearched {
                row = row.child(
                    ui::Label::new(format!("never returned by the search: {identifier}"))
                        .colour(theme::accent())
                        .size(ui::Size::Compact),
                );
            }
            body = body.child(row);
        }

        let title = if contradicted > 0 {
            format!(
                "What was claimed · {} · {contradicted} not borne out",
                claims.len()
            )
        } else {
            format!("What was claimed · {}", claims.len())
        };

        ui::Modal::new("claims", title)
            .width(820.)
            .focus(&self.delete_focus)
            .body(body)
            .actions(
                ui::actions().child(div().flex_grow()).child(
                    ui::Button::new("claims-close").text("Close").on_click(cx.listener(
                        |workbench, _event, _window, cx| {
                            workbench.claims_open = false;
                            workbench.restore_focus = true;
                            cx.notify();
                        },
                    )),
                ),
            )
    }
}
