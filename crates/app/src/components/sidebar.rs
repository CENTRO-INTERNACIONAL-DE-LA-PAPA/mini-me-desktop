#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

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
                row("menu-new-project", "New project".into()),
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

impl Workbench {
    /// The whole sidebar: header, view toggle, search, conversation/project list, settings button.
    pub(crate) fn sidebar_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.sidecar.thread_id();
        let draft_project = current.is_none().then(|| self.sidecar.project()).flatten();
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
            .p_2()
            .gap_1();

        if self.sidebar_view == SidebarView::Conversations
            && draft_project.is_none()
            && current.is_none()
            && query.trim().is_empty()
        {
            list = list.child(ui::ListRow::new("draft-conversation", "Untitled Conversation").selected(true));
        }

        if matched.is_empty() && self.sidebar_view == SidebarView::Conversations {
            list = list.child(
                div()
                    .p_2()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .when(!self.conversations_loaded, |row| {
                        row.child(
                            ui::Spinner::new("loading-conversations")
                                .colour(theme::text_muted()),
                        )
                    })
                    .child(if !self.conversations_loaded {
                        "Loading your conversations…"
                    } else if self.conversations.is_empty() {
                        "Conversations you start will appear here."
                    } else {
                        "Nothing matches that."
                    }),
            );
        }

        // Grouped by project, ungrouped last, alphabetical — order stays stable as work moves
        // between projects instead of reshuffling the list.
        let mut grouped: std::collections::BTreeMap<Option<String>, Vec<&protocol::Conversation>> =
            std::collections::BTreeMap::new();
        if self.conversation_query.read(cx).text().trim().is_empty() {
            for name in &self.folder_projects {
                grouped.entry(Some(name.clone())).or_default();
            }
        }
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
        let projects_only = self.sidebar_view == SidebarView::Projects;
        let show_headings = projects_only;

        for (project, conversations) in ordered {
            // The Conversations tab is ungrouped work only; a project's conversations live under
            // its own heading in the Projects tab instead.
            if !projects_only && project.is_some() {
                continue;
            }
            if projects_only && project.is_none() {
                continue;
            }
            if show_headings {
                let heading = project
                    .clone()
                    .unwrap_or_else(|| UNGROUPED_PROJECT_LABEL.to_string());
                list = list.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .group(SharedString::from(format!("head-{heading}")))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .flex_grow()
                                .min_w_0()
                                .child(app_icon(
                                    "icons/folder.svg",
                                    theme::text(),
                                    Some(ui::IconSize::Small.px()),
                                ))
                                .child(
                                    div()
                                        .text_color(rgb(theme::text()))
                                        .text_xs()
                                        .child(heading.to_uppercase()),
                                ),
                        )
                        // Only a named project gets a `⋮`: "Ungrouped Conversations" is the
                        // workspace root, not a project, and cannot be deleted or started "in".
                        .when_some(project.clone(), |header, name| {
                            header.child(self.sidebar_menu_button(
                                format!("head-menu-{name}"),
                                SidebarMenu::Project {
                                    conversations: self
                                        .conversations
                                        .iter()
                                        .filter(|conversation| {
                                            conversation.project.as_deref() == Some(name.as_str())
                                        })
                                        .cloned()
                                        .collect(),
                                    name,
                                },
                                cx,
                            ))
                        }),
                );
            }
            if projects_only {
                let draft_here = draft_project.as_deref() == project.as_deref() && project.is_some();
                if !conversations.is_empty() || draft_here {
                    let mut nested = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .ml_2()
                        .pl_2()
                        .border_l_1()
                        .border_color(rgb(theme::border()));
                    if draft_here {
                        nested = nested.child(
                            ui::ListRow::new("draft-conversation", "Untitled Conversation")
                                .selected(true),
                        );
                    }
                    for conversation in conversations {
                        nested = nested.child(self.conversation_row(conversation, current.as_deref(), cx));
                    }
                    list = list.child(nested);
                }
                continue;
            }
            for conversation in conversations {
                list = list.child(self.conversation_row(conversation, current.as_deref(), cx));
            }
        }

        list = list.child(match self.sidebar_view {
            SidebarView::Conversations => ui::IconTextButton::new(
                "sidebar-new-conversation",
                "icons/plus.svg",
                "New Conversation",
            )
            .margin(false)
            .full_width(true)
            .on_click(cx.listener(|workbench, _event, window, cx| {
                workbench.run_sidebar_menu(&SidebarMenu::New, "menu-new-conversation", window, cx);
            })),
            SidebarView::Projects => ui::IconTextButton::new(
                "sidebar-new-project",
                "icons/plus.svg",
                "New Project",
            )
            .margin(false)
            .full_width(true)
            .on_click(cx.listener(|workbench, _event, window, cx| {
                workbench.run_sidebar_menu(&SidebarMenu::New, "menu-new-project", window, cx);
            })),
        });

        div()
            .flex()
            .flex_col()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_none()
            .m_2()
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
                    .child(
                        div()
                            .id("home")
                            .text_size(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|style| style.cursor_pointer())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.new_thread_in(None, cx);
                            }))
                            .child("Mini-Me App"),
                    )
                    .child(
                        ui::IconButton::new("toggle-left-sidebar", "icons/sidebar-simple-left.svg")
                            .icon_size(ui::IconSize::Small.px())
                            .ink(theme::text())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.sidebar_open = !workbench.sidebar_open;
                                workbench.remember_panels();
                                cx.notify();
                            })),
                    ),
            )
            .child(self.sidebar_view_toggle(cx))
            .child(
                div()
                    .flex_none()
                    .m_2()
                    .mt_0()
                    .px_2p5()
                    .py_1p5()
                    .rounded_md()
                    .rounded_t_none()
                    .bg(rgb(theme::surface()))
                    .border_1()
                    .text_color(rgb(theme::text_muted()))
                    .border_color(rgb(theme::border()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .child(app_icon("icons/magnifying-glass.svg", theme::text_muted(), Some(ui::IconSize::Small.px())))
                            .child(self.conversation_query.clone()),
                    )
            )
            .child(list)
            .child(
                ui::IconTextButton::new("open-settings", "icons/gear-six.svg", "Settings")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.run_command(Command::OpenSettings, cx);
                    })),
            )
    }

    /// Switches the list below between every conversation and just the projects.
    pub(crate) fn sidebar_view_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |label: &'static str, view: SidebarView, active: bool| {
            div()
                .id(SharedString::from(label))
                .flex_grow()
                .items_center()
                .justify_center()
                .py_1()
                .text_sm()
                .text_color(rgb(if active { theme::accent() } else { theme::text_muted() }))
                .text_center()
                .when(active, |tab| tab.bg(rgb(theme::accent_soft())))
                .child(label)
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    workbench.sidebar_view = view;
                    cx.notify();
                }))
        };
        div()
            .flex_none()
            .flex()
            .flex_row()
            .gap_1()
            .mx_2()
            .mt_2()
            .rounded_md()
            .rounded_b_none()
            .border_1()
            .border_b_0()
            .border_color(rgb(theme::border()))
            .hover(|style| style.text_color(rgb(theme::text())).cursor_pointer())
            .bg(rgb(theme::background()))
            .child(tab(
                "Conversations",
                SidebarView::Conversations,
                self.sidebar_view == SidebarView::Conversations,
            ))
            .child(tab(
                "Projects",
                SidebarView::Projects,
                self.sidebar_view == SidebarView::Projects,
            ))
    }

    /// One row of the conversation list — shared by the flat and per-project nested views.
    fn conversation_row(
        &self,
        conversation: &protocol::Conversation,
        current: Option<&str>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let thread_id = conversation.thread_id.clone();
        let selected = current == Some(thread_id.as_str());
        let renaming = self.renaming.as_deref() == Some(thread_id.as_str());

        if renaming {
            return div()
                .w_full()
                .min_w_0()
                .px_2()
                .py_1()
                .border_1()
                .border_color(rgb(theme::accent()))
                .child(self.rename_editor.clone())
                .into_any_element();
        }

        if self
            .deleting
            .as_ref()
            .is_some_and(|target| target.contains_thread(&thread_id))
        {
            return div()
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
                )
                .into_any_element();
        }

        let open = thread_id.clone();
        ui::ListRow::new(SharedString::from(format!("conv-{thread_id}")), conversation.title.clone())
            .selected(selected)
            .trailing(self.sidebar_menu_button(
                format!("row-menu-{thread_id}"),
                SidebarMenu::Conversation(conversation.clone()),
                cx,
            ))
            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                workbench.open_conversation(open.clone(), cx);
            }))
            .into_any_element()
    }

    /// The `⋮` that opens a [`SidebarMenu`] popup.
    pub(crate) fn sidebar_menu_button(
        &self,
        id: String,
        menu: SidebarMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(id))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(20.))
            .rounded_md()
            .text_color(rgb(theme::text_faint()))
            .text_sm()
            .hover(|style| {
                style
                    .bg(rgb(theme::accent_soft()))
                    .text_color(rgb(theme::text()))
                    .cursor_pointer()
            })
            .child("⋮")
            .on_click(cx.listener(move |workbench, event: &gpui::ClickEvent, _window, cx| {
                // Must stop propagation, or the row/heading underneath also fires (opening the
                // conversation, or launching Explorer).
                cx.stop_propagation();
                let at = match event {
                    gpui::ClickEvent::Mouse(click) => click.up.position,
                    _ => gpui::point(px(120.), px(160.)),
                };
                workbench.sidebar_menu = Some((menu.clone(), gpui::point(at.x, at.y + px(6.))));
                cx.notify();
            }))
    }

    /// The floating `⋮`/New popup menu itself, anchored at a point.
    pub(crate) fn sidebar_menu_element(
        &self,
        open: SidebarMenu,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut panel = menu_card();
        for row in open.rows() {
            let chosen = open.clone();
            let id = row.id;
            panel = panel.child(
                div()
                    .id(id)
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(if row.danger {
                        theme::error()
                    } else {
                        theme::text()
                    }))
                    .hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer())
                    .child(row.label)
                    .on_click(cx.listener(move |workbench, _event, window, cx| {
                        workbench.sidebar_menu = None;
                        workbench.run_sidebar_menu(&chosen, id, window, cx);
                    })),
            );
        }

        gpui::deferred(
            gpui::anchored().position(at).snap_to_window().child(
                panel.on_mouse_down_out(cx.listener(|workbench, _event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.sidebar_menu = None;
                    cx.notify();
                })),
            ),
        )
    }
}
