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
                // The ellipsis is a promise: this one asks for a name before anything happens.
                row("menu-new-project", "New project…".into()),
            ],
            Self::Conversation(_) => vec![
                row("menu-rename", "Rename".into()),
                danger("menu-delete", "Delete".into()),
            ],
            Self::Project { name, .. } => vec![
                row("menu-new-here", format!("New conversation in {name}")),
                danger("menu-delete-project", "Delete project".into()),
            ],
        }
    }
}


impl Workbench {
    /// The draggable edge between two panes.
    ///
    /// Four pixels wide with a resize cursor, and it does not move anything itself: it records
    /// *which* edge is being dragged, and the root's mouse-move does the arithmetic. Tracking
    /// the drag on the root rather than on this strip is what keeps it working when the pointer
    /// outruns four pixels, which it does immediately.
    pub(crate) fn divider(&self, edge: Divider, cx: &mut Context<Self>) -> impl IntoElement {
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
}


impl Workbench {
    /// The `⋮` and `New` menus, drawn where their control is.
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


impl Workbench {
    /// The `⋮` that opens one of those menus.
    ///
    /// Always drawn, never revealed on hover: the inline chips this replaces were invisible until
    /// the pointer was already on the row, so the only way to learn what a row could do was to
    /// point at it and decode two abbreviations.
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
                // **The row underneath must not also fire.** A conversation row opens that
                // conversation on click and a project heading opens its folder in Explorer, so
                // without this, asking a row what it can do would switch conversations, and
                // asking a heading would launch a file manager (§163, one layer in).
                cx.stop_propagation();
                // Anchored to the pointer rather than to the button's bounds, which GPUI does not
                // hand a click handler. Nudged down so the menu opens below the control it came
                // from instead of on top of it.
                let at = match event {
                    gpui::ClickEvent::Mouse(click) => click.up.position,
                    _ => gpui::point(px(120.), px(160.)),
                };
                workbench.sidebar_menu = Some((menu.clone(), gpui::point(at.x, at.y + px(6.))));
                cx.notify();
            }))
    }
}


impl Workbench {
    /// Switches the list below between every conversation and just the projects.
    ///
    /// Basic on purpose: two labels, one highlighted, click to switch. Creation is being
    /// redone separately, so this does not yet try to be more than a toggle.
    pub(crate) fn sidebar_view_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |label: &'static str, view: SidebarView, active: bool| {
            div()
                .id(SharedString::from(label))
                .flex_grow()
                .items_center()
                .justify_center()
                .py_1()
                .rounded_md()
                .text_xs()
                .text_color(rgb(if active { theme::text() } else { theme::text_muted() }))
                .when(active, |tab| tab.bg(rgb(theme::elevated())))
                .hover(|style| style.text_color(rgb(theme::text())).cursor_pointer())
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
            .p(px(2.))
            .rounded_md()
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

    /// The conversation list.
    ///
    /// The backend has stored every thread since the first launch; the app simply never
    /// asked, so a 64px rail with a decorative glyph was all there was and every session
    /// looked like the first one. Past work was not lost — it was unreachable, which for
    /// the researcher is the same thing (docs §48).
    pub(crate) fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                    // The mark goes where the researcher is already looking. A sentence alone
                    // said the same thing and said it motionlessly, which is what a hung window
                    // also looks like (§177).
                    .when(!self.conversations_loaded, |row| {
                        row.child(
                            // Muted, matching the sentence beside it: this reports a state, and
                            // the accent in this app means "act on me".
                            ui::Spinner::new("loading-conversations")
                                .colour(theme::text_muted()),
                        )
                    })
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
        // Seeded with every project that has a folder, so one nothing is filed under yet still
        // gets a heading. Naming a project used to create the folder and show nothing at all —
        // the sidebar could only see a project through a conversation (§167).
        //
        // Not while a search is running: a filter is a way to find work, and an empty project
        // matches nothing, so leaving them in would make searching look broken.
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
        // A named project always gets its heading now, even when it is the only group. The
        // heading is no longer decoration: it owns New here, Open folder and Delete project, so
        // hiding it would hide the only project-delete affordance (§155). Ungrouped work alone
        // still needs no heading because it is not a project and has no project folder to delete.
        let projects_only = self.sidebar_view == SidebarView::Projects;
        let show_headings = projects_only
            || ordered.len() > 1
            || ordered
                .iter()
                .any(|(project, _conversations)| project.is_some());

        for (project, conversations) in ordered {
            // The Projects tab lists projects, not the ungrouped-work bucket — that heading
            // exists to give the workspace root somewhere to open from, not to stand in for a
            // project that can be deleted.
            if projects_only && project.is_none() {
                continue;
            }
            if show_headings {
                let heading = project
                    .clone()
                    .unwrap_or_else(|| UNGROUPED_PROJECT_LABEL.to_string());
                let opening = project.clone();
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
                        // One `⋮` where four hover-revealed characters used to be. Only a named
                        // project gets one: "Ungrouped Conversations" is the workspace root, which
                        // is not a project and cannot be deleted or started "in" (§165).
                        .when_some(project.clone(), |header, name| {
                            header.child(self.sidebar_menu_button(
                                format!("head-menu-{name}"),
                                SidebarMenu::Project {
                                    // All conversations in the project, not merely the rows that
                                    // survived the sidebar search. A filter is a way to find
                                    // work, never a deletion boundary (§155).
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
                continue;
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
                        .hover(|style| style.bg(rgb(theme::hover_over(theme::elevated()))).cursor_pointer())
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
                        .child(self.sidebar_menu_button(
                            format!("row-menu-{thread_id}"),
                            SidebarMenu::Conversation(conversation.clone()),
                            cx,
                        ))
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
                            .child(app_icon("icons/settings.svg", theme::accent(), None))
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
                            .child("New"),
                            // The menu this used to open (conversation vs. project) is gone —
                            // creation is being redone, so this button is a placeholder until it
                            // has a new behaviour wired to it.
                    ),
            )
            .child(self.sidebar_view_toggle(cx))
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
}


impl Workbench {
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
    pub(crate) fn road_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        const OPEN: f32 = 172.;
        const FOLDED: f32 = 38.;
        /// The dot's own size, and the gutter it is centred in.
        const DOT: f32 = 9.;
        const GUTTER: f32 = 12.;
        /// How tall a stage's row is, which is the distance the connector has to span.
        ///
        /// Folded, the rail is the *whole* content — no labels, so the rows close up and the dots
        /// read as one strung line rather than as marks scattered down a margin.
        const ROW_OPEN: f32 = 46.;
        const ROW_FOLDED: f32 = 26.;

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
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
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
                // Folded, the chevron is the header's only child, and `justify_between` puts a
                // lone child at the start — so it sat against the left edge above a rail that
                // §169 had just centred. Same one-line fix as the stage rows below it.
                .when(!self.road_open, |header| header.justify_center())
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
                // **Stretch, not `items_start`.** The connector below the dot is `flex_grow`, and
                // it can only grow inside a gutter that has a height to grow into. `items_start`
                // aligns children to the top *and leaves them at their content height*, so the
                // gutter stood 23px — a 9px dot plus the connector's 14px minimum — while the row
                // beside it stood at 46px for a two-line label. The line stopped a third of the
                // way down and every dot hung under a stub (§169).
                .w_full()
                .min_w_0()
                // Folded, the gutter is the row's only child, so a full-width row left it against
                // the edge. The rail is the whole strip at 38px and belongs down its middle.
                .when(!self.road_open, |row| row.justify_center())
                .when(at < last, |row| {
                    row.min_h(px(if self.road_open { ROW_OPEN } else { ROW_FOLDED }))
                })
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
                        // Its own, now that the row stretches: the column fills the row height and
                        // its two lines stack at the top, where the row's `items_start` used to
                        // put them.
                        .items_start()
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
}

