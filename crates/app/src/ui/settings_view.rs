// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    /// The five providers, as pills rather than a cycle button.
    ///
    /// Five fit on one row, so there is no reason to make someone click through them —
    /// and a cycle button hides four of the five, which is the same complaint the theme
    /// list just answered (docs §58).
    pub(crate) fn provider_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
                    .child(spec.label)
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        // **Staged, not applied.** Picking a pill used to change the provider and
                        // the model id on the spot, silently, and the only thing that told you
                        // which account a turn would bill was which pill happened to be lit.
                        // Asked for after a turn ran against the wrong one: *"a modal that
                        // confirms the user when he sets the providers"* (docs §186).
                        if spec.id == workbench.draft.provider {
                            return;
                        }
                        workbench.confirming_provider = Some(spec);
                        cx.notify();
                    })),
            );
        }
        row
    }
}


impl Workbench {
    /// The provider's models, as a scrollable list that fills the field.
    ///
    /// Curated, not a catalogue, and the field below stays editable — a list here can only
    /// ever be out of date, and a provider shipping a model the day after a release must
    /// not make the app unusable.
    pub(crate) fn model_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.field_text_or(Field::ModelId, &self.draft.model_id, cx);
        let models = settings::provider(&self.draft.provider)
            .map(|spec| catalogue::models_for(spec, &self.catalogue))
            .unwrap_or_default();

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

        // Fuzzy, the same scorer as every other list here, so `deepseek` finds
        // `deepseek/deepseek-r1` and `kimi` finds `moonshotai/kimi-k2`.
        let query = self.model_filter.read(cx).text().to_string();
        let mut ranked: Vec<(i32, String)> = models
            .into_iter()
            .filter_map(|model| match_score(&query, &model).map(|score| (score, model)))
            .collect();
        if !query.trim().is_empty() {
            ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        }
        let shown = ranked.len();

        for (_, model) in ranked {
            let selected = model == current;
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
                    .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
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
                        workbench.set_field(Field::ModelId, &model, cx);
                        cx.notify();
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            // Above the rows, because a filter under the thing it filters is one nobody finds.
            .child(self.filter_field(self.model_filter.clone(), cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(list)
                    .children(ui::scrollbar(&self.model_scroll)),
            )
            // **Said, not implied by an empty box.** A filter matching nothing and a provider
            // that returned nothing look identical otherwise, and only one of them is fixed by
            // typing less.
            .when(shown == 0, |panel| {
                panel.child(
                    ui::Label::new("No model matches that.")
                        .muted()
                        .size(ui::Size::Compact),
                )
            })
    }
}


impl Workbench {
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
    pub(crate) fn theme_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The same fuzzy scorer as everywhere else, so `mocha` finds Catppuccin Mocha.
        let query = self.theme_filter.read(cx).text().to_string();
        let mut matched: Vec<(i32, settings::ThemeEntry)> = settings::available_theme_entries()
            .into_iter()
            .filter_map(|entry| {
                match_score(&query, &entry.name).map(|score| (score, entry))
            })
            .collect();
        if !query.trim().is_empty() {
            matched.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
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

        for (_, entry) in matched {
            let settings::ThemeEntry {
                name,
                palette,
                source,
            } = entry;
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

            let remove_name = name.clone();
            let actions = div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap_2()
                .child(swatch)
                .when_some(source, |actions, path| {
                    actions.child(
                        div()
                            .id(SharedString::from(format!("remove-theme-hint-{remove_name}")))
                            .tooltip(|_window, cx| {
                                cx.new(|_| ui::Hint {
                                    text: "remove this installed file and every palette in it"
                                        .into(),
                                })
                                .into()
                            })
                            .child(
                                ui::Button::new(SharedString::from(format!(
                                    "remove-theme-{remove_name}"
                                )))
                                .text("remove")
                                .on_click(cx.listener(
                                    move |workbench, _event, _window, cx| {
                                        // The theme row itself selects and closes the picker.
                                        // Removing is a second action nested inside that row, so
                                        // it must not also select the file it just deleted.
                                        cx.stop_propagation();
                                        workbench.uninstall_theme(
                                            path.clone(),
                                            remove_name.clone(),
                                            cx,
                                        );
                                    },
                                )),
                            ),
                    )
                });

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
                    .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
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
                    .child(actions)
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
            .child(ui::Label::new("GET MORE").colour(theme::text_faint()).size(ui::Size::Compact))
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
                    .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
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
                    .children(ui::scrollbar(&self.theme_scroll)),
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
}


impl Workbench {
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
    pub(crate) fn project_list(&self, starting_new: bool, cx: &mut Context<Self>) -> impl IntoElement {
        // The one difference between the two modes, named once. `file_in_project` moves the open
        // conversation's folder; `new_thread_in` starts a fresh one and moves nothing — which is
        // what "New project" has to mean when there may be no open conversation to move.
        let choose = move |workbench: &mut Self, project: Option<String>, cx: &mut Context<Self>| {
            if starting_new {
                // **The folder first, and it is what makes the project real.** `new_thread_in`
                // only sets where the *next* turn will write, and until that turn happens there
                // is no thread, no metadata and nothing for the sidebar to show — which is
                // exactly what naming a project used to look like: nothing (§167). Creating the
                // directory is creating the project, because §105 made them the same thing.
                let mut project = project;
                if let Some(name) = project.as_deref() {
                    match workspace::create_project(name) {
                        // **The name the folder actually got**, not the one that was typed.
                        // `project_folder` rewrites characters a path cannot hold, so keeping
                        // the raw text would file conversations under `Q1/Q2` while the
                        // directory is `Q1_Q2` — and the sidebar, which reads both, would show
                        // the one project twice under two spellings.
                        Ok(folder) => {
                            project = Some(folder);
                            workbench.folder_projects = workspace::projects();
                        }
                        Err(error) => {
                            workbench.error = Some(format!("{error:#}"));
                            workbench.open_picker = None;
                            cx.notify();
                            return;
                        }
                    }
                }
                workbench.new_thread_in(project, cx);
            } else {
                workbench.file_in_project(project, cx);
            }
            workbench.open_picker = None;
        };
        let typed = self.project_query.read(cx).text().trim().to_string();
        let current = self.sidecar.project();
        // Both sources, for the same reason the sidebar uses both: a project with a folder and
        // no conversations yet is a project you should be able to file into (§167).
        let mut names: Vec<String> = self
            .conversations
            .iter()
            .filter_map(|conversation| conversation.project.clone())
            .chain(self.folder_projects.iter().cloned())
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
                    format!("Create New Project “{typed}”"),
                    false,
                    None,
                )
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    choose(workbench, Some(created.clone()), cx);
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
}


impl Workbench {
    /// A model per specialist, under the coordinator's.
    ///
    /// **The specialists do genuinely different work**, and one model for all ten is either an
    /// expensive way to grep or a cheap way to write a paper. Literature search wants a long
    /// context and cheap tokens across many calls; a report wants the best prose available; data
    /// cleaning wants neither and runs dozens of times.
    ///
    /// The list is the live registry (§76), so it cannot name a specialist the backend does not
    /// have — and when the registry is empty it says why rather than showing nothing.
    pub(crate) fn subagent_models(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let specialists = workspace::subagents();
        let mut rows = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(ui::Label::new("PER SPECIALIST").colour(theme::text_faint()).size(ui::Size::Compact));

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
}


impl Workbench {
    /// The models one specialist can be pointed at, plus the way back to the default.
    ///
    /// Every provider's models, not just the current one's: pointing literature search at a
    /// cheap long-context model from another provider is the main reason to want this at all.
    /// The key for that provider has to be stored, so the row says when it is not — a turn that
    /// fails inside a subagent several minutes in is the worst place to discover it (§104).
    pub(crate) fn subagent_model_list(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
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

        // **The same filter the coordinator's list has**, asked for after the grouped list made
        // the problem plain: four providers' catalogues stacked under headings is more rows than
        // before, not fewer. Only one picker is open at a time, so one field serves both — they
        // ask the same question of the same catalogue (docs §192).
        let query = self.model_filter.read(cx).text().to_string();
        for provider in settings::PROVIDERS {
            // The same live list the coordinator's picker uses, so a specialist can be pointed at
            // anything the gateway actually carries rather than at four names written here.
            let mut models: Vec<(i32, String)> = catalogue::models_for(&provider, &self.catalogue)
                .into_iter()
                .filter_map(|model| match_score(&query, &model).map(|score| (score, model)))
                .collect();
            if !query.trim().is_empty() {
                models.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
            }
            // A provider whose whole catalogue was filtered out contributes no heading either,
            // or the list becomes a column of company names with nothing under them.
            if models.is_empty() {
                continue;
            }
            // **The company's name once, over its models** — the shape Mini-Me's own web panel
            // uses (`<optgroup label={provider.name}>`) and the one Zed's provider page uses, and
            // both are right for the same reason: repeating "OpenAI — billed separately" on every
            // row spent the width the model id needed, to say a thing that is true of the whole
            // group (docs §191).
            let keyed = settings::secret(&format!("llm:{}", provider.id)).is_some();
            let note = specialist_note(&provider, &self.draft.provider, keyed);
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .child(
                        ui::Label::new(provider.label)
                            .colour(theme::text())
                            .size(ui::Size::Compact),
                    )
                    .children(note.map(|note| {
                        ui::Label::new(note)
                            .colour(theme::warning())
                            .size(ui::Size::Compact)
                    })),
            );
            // **A provider with no key offers nothing to press.**
            //
            // It used to list its whole catalogue with `— no key stored` beside the company name,
            // and a researcher scrolling 400 models past a heading picked one and got a 429 from a
            // billing page they had never seen, minutes later, inside a background worker. The
            // same model is very often present *twice* — `gpt-4.1` under OpenAI and
            // `openai/gpt-4.1` under OpenRouter — so the trap is not exotic: one of the two works,
            // they differ by a prefix, and only one of them is paid for (§212).
            //
            // The heading stays, and says what to do. Hiding the provider entirely would leave a
            // researcher who *has* an OpenAI account with no clue why it is not offered.
            if !keyed {
                list = list.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .pb_2()
                        .text_xs()
                        .text_color(rgb(theme::text_muted()))
                        .child(format!(
                            "{} {} here once a {} key is stored — add one under API key above.",
                            models.len(),
                            if models.len() == 1 { "model" } else { "models" },
                            provider.label
                        )),
                );
                continue;
            }
            for (_, model) in models {
                let spec = format!("{}::{}", provider.id, model);
                let selected = chosen.as_deref() == Some(spec.as_str());

                let picked = name.clone();
                let value = spec.clone();
                list = list.child(
                    picker_row(model.clone(), selected, None)
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
        // The filter above the rows, so it is seen before the scrolling starts.
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(self.filter_field(self.model_filter.clone(), cx))
            .child(list)
            .into_any_element()
    }
}


impl Workbench {
    pub(crate) fn settings_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let provider = settings::provider(&self.draft.provider);
        let needs_base_url = provider.is_some_and(|p| p.needs_base_url);

        // A centred modal, not a column. As a column it took 420px off the chat for as
        // long as it was open, and settings are something you visit and leave — the same
        // argument that makes Zed's fifty pickers modal rather than panels (docs §51).
        let section = self.settings_section;

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
                    .unwrap_or_else(|| format!("llm:{}", self.key_target));
                // Presence only — the value itself is never read back into the UI.
                if settings::secret(&name).is_some() {
                    " · stored"
                } else {
                    " · not set"
                }
            } else {
                ""
            };
            // **Every provider at once, above the field.** Both references converge here: the
            // web panel lists all providers with a Connected badge, and Zed's page gives each its
            // own key row — because a key belongs to a *company*, not to whichever provider a
            // conversation happens to be running on. Picking one here retargets the field and
            // nothing else: the coordinator does not move, and no confirmation is owed (§191).
            if *field == Field::ApiKey {
                let mut chips = div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .w_full()
                    .min_w_0();
                for spec in settings::PROVIDERS {
                    let chosen = spec.id == self.key_target;
                    let has_key = settings::secret(&format!("llm:{}", spec.id)).is_some();
                    chips = chips.child(
                        div()
                            .id(SharedString::from(format!("key-target-{}", spec.id)))
                            .flex_none()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(if chosen {
                                theme::accent()
                            } else {
                                theme::border()
                            }))
                            .when(chosen, |chip| chip.bg(rgb(theme::accent_soft())))
                            .text_color(rgb(if chosen {
                                theme::text()
                            } else {
                                theme::text_muted()
                            }))
                            .text_xs()
                            .hover(|style| {
                                let fill = theme::hover_over(theme::elevated());
                                style
                                    .bg(rgb(fill))
                                    .text_color(rgb(theme::ink_on(fill)))
                                    .cursor_pointer()
                            })
                            // A tick where a key is filed, so "which of these am I missing" is
                            // one glance rather than five clicks.
                            .child(format!("{}{}", if has_key { "✓ " } else { "" }, spec.label))
                            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                workbench.key_target = spec.id.to_string();
                                cx.notify();
                            })),
                    );
                }
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
                                .child("Keys — one per company, set them in any order"),
                        )
                        .child(chips),
                );
            }
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
                    "Ask before every command",
                    "Pause and show each command, so nothing runs without you seeing it.",
                    self.draft.approve_execute,
                    0usize,
                ),
                // Preview API, and it needs the generated graph config — so opt-in, and
                // labelled by what it does rather than by what it is called upstream.
                (
                    "Let work run in the background",
                    "Long jobs keep going while you carry on asking questions.",
                    self.draft.async_subagents,
                    1,
                ),
                // Named for what it puts on screen, not for the reader it is aimed at: "developer
                // mode" would be a claim about who deserves it, and the researcher checking a
                // citation before submission is exactly who needs it most (§301).
                (
                    "Show what ran",
                    "Adds a line to Outputs naming the commands this conversation ran, and \
                     whether any of them wrote outside its folder.",
                    self.draft.run_record,
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
                            0 => workbench.draft.approve_execute = !workbench.draft.approve_execute,
                            1 => workbench.draft.async_subagents = !workbench.draft.async_subagents,
                            _ => workbench.draft.run_record = !workbench.draft.run_record,
                        }
                        cx.notify();
                    }),
                ),
            ));
        }

        if section == Section::Backend {
            pane = pane.child(ui::setting_row(
                "Run setup checks",
                "Re-run the install checks and fixes shown on first launch.",
                ui::Button::new("open-onboarding").text("Run setup checks").style(
                    ui::ButtonStyle::Secondary,
                ).on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.open_onboarding(cx);
                })),
            ));
        }

        let actions = ui::actions()
            .child(
                ui::Button::new("save-settings")
                    .text("Save")
                    .style(ui::ButtonStyle::Primary)
                    .on_click(
                        cx.listener(|workbench, _event, _window, cx| workbench.save_settings(cx)),
                    ),
            )
            .child(
                ui::Button::new("close-settings").text("Close").on_click(cx.listener(
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
}

