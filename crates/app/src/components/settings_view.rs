#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    /// The main Settings pane: fields for the current section, or the Setup page.
    pub(crate) fn settings_pane(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let provider = settings::provider(&self.draft.provider);
        let needs_base_url = provider.is_some_and(|p| p.needs_base_url);

        let section = self.settings_section;
        if section == Section::Setup {
            return self.preferences_window(self.setup_pane(cx), self.setup_actions(cx), cx);
        }

        let mut pane = div().flex().flex_col().w_full().min_w_0().gap_3();
        if section == Section::Appearance {
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
                if settings::secret(&name).is_some() {
                    " · stored"
                } else {
                    " · not set"
                }
            } else {
                ""
            };
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
                            // tab_index orders Tab traversal across the page's fields.
                            .tab_index(tab as isize)
                            .in_focus(|style| style.border_color(rgb(theme::accent())))
                            .child(composer.clone()),
                    ),
            );
        }

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
                        // Reapply the saved theme: closing without saving discards the preview.
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

    /// The five providers as selectable pills.
    pub(crate) fn provider_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_row().flex_wrap().w_full().gap_1();
        for spec in &settings::PROVIDERS {
            let selected = spec.id == self.draft.provider;
            row = row.child(
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
                        // Staged, not applied: confirm before switching provider/model.
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

    /// The current provider's models, filterable and scrollable; the field below stays editable.
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
            .pr(px(SCROLL_GUTTER))
            .gap_px()
            .max_h(px(150.))
            .overflow_y_scroll()
            .track_scroll(&self.model_scroll);

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
                    .when(selected, |row| row.bg(rgb(theme::accent_soft())))
                    .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
                    .child(
                        // `.ellipsis()` on the label, not `truncate()` on the row: truncating
                        // the flex item itself gives it zero intrinsic width and renders "…".
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
            .child(self.filter_field(self.model_filter.clone(), cx))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .child(list)
                    .children(scrollbar(&self.model_scroll)),
            )
            .when(shown == 0, |panel| {
                panel.child(
                    ui::Label::new("No model matches that.")
                        .muted()
                        .size(ui::Size::Compact),
                )
            })
    }

    /// Every theme palette as a swatch row, with live hover preview and a gallery to install more.
    pub(crate) fn theme_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

        let mut list = div()
            .id("theme-rows")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
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
                                cx.new(|_| Hint {
                                    text: "remove this installed file and every palette in it"
                                        .into(),
                                })
                                .into()
                            })
                            .child(
                                ui::Button::new(
                                    SharedString::from(format!("remove-theme-{remove_name}")),
                                    "remove",
                                )
                                .size(ui::Size::Compact)
                                .on_click(cx.listener(
                                    move |workbench, _event, _window, cx| {
                                        // Stop propagation so this doesn't also select the row.
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
                        settings::apply_theme(&workbench.draft);
                        workbench.open_picker = None;
                        cx.notify();
                    })),
            );
        }

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
                    // `min_w_0()` required: without it an unbroken path sets the popup's
                    // minimum width and pushes everything else off the right edge.
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

    /// Every project (from conversations and folders) plus the way to create a new one.
    pub(crate) fn project_list(&self, starting_new: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let choose = move |workbench: &mut Self, project: Option<String>, cx: &mut Context<Self>| {
            if starting_new {
                let mut project = project;
                if let Some(name) = project.as_deref() {
                    match workspace::create_project(name) {
                        // Use the folder name actually created, not the raw typed text —
                        // `project_folder` rewrites characters a path cannot hold.
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

        if !typed.is_empty() && !names.iter().any(|name| name == &typed) {
            let created = typed.clone();
            list = list.child(
                picker_row(
                    format!("New project “{typed}”"),
                    false,
                    Some("creates the folder".into()),
                )
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    choose(workbench, Some(created.clone()), cx);
                })),
            );
        }

        list = list.child(
            picker_row(UNGROUPED_PROJECT_LABEL, current.is_none(), None).on_click(
                cx.listener(move |workbench, _event, _window, cx| choose(workbench, None, cx)),
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
                    choose(workbench, Some(chosen.clone()), cx);
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

    /// Per-specialist model overrides, listed from the live subagent registry.
    pub(crate) fn subagent_models(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    /// Models a specialist can be pointed at, grouped by provider, plus the way back to default.
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

        let query = self.model_filter.read(cx).text().to_string();
        for provider in settings::PROVIDERS {
            let mut models: Vec<(i32, String)> = catalogue::models_for(&provider, &self.catalogue)
                .into_iter()
                .filter_map(|model| match_score(&query, &model).map(|score| (score, model)))
                .collect();
            if !query.trim().is_empty() {
                models.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
            }
            if models.is_empty() {
                continue;
            }
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

    /// The Setup pane: one row per preflight check, each carrying its fix actions and output log.
    pub(crate) fn setup_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = div().flex().flex_col().w_full().min_w_0().gap_3();

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
                        .child(
                            div()
                                .text_color(rgb(theme::text_muted()))
                                .text_xs()
                                .child(format!("{} · {}", report.location, report.execution)),
                        )
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

        if let Some(fix) = &self.running_fix {
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
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .child(
                            div()
                                .flex_grow()
                                .min_w_0()
                                .text_color(rgb(theme::text()))
                                .text_sm()
                                .child(if fix.done {
                                    format!(
                                        "{} — {}",
                                        fix.label,
                                        if fix.ok { "done" } else { "failed" }
                                    )
                                } else if fix.stopping {
                                    format!("{} — stopping…", fix.label)
                                } else {
                                    format!("{}…", fix.label)
                                }),
                        )
                        .when(!fix.done, |header| {
                            header.child(
                                ui::Button::new("stop-fix", "Stop")
                                    .tone(ui::Tone::Danger)
                                    .size(ui::Size::Compact)
                                    .disabled(fix.stopping || !fix.cancel.armed())
                                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                                        workbench.stop_fix(cx);
                                    })),
                            )
                        }),
                );
            if let Some(link) = &fix.link {
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
                        .child(if fix.done {
                            "The command printed nothing. The sidecar log below may have more."
                        } else {
                            "starting…"
                        }),
                );
            }
            let mut log = log.child(output);
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
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(theme::text_muted()))
                .text_xs()
                .child(format!("App log: {}", app_log_path().display())),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .text_color(rgb(theme::text_muted()))
                .text_xs()
                .child(format!(
                    "Update log: {}",
                    std::env::temp_dir().join("mini-me-desktop-update.log").display()
                )),
        )
    }

    /// The buttons for the Setup page. Re-check is its Save.
    pub(crate) fn setup_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
