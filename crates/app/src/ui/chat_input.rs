//! The chat input: the composer row itself, the specialist it is currently addressed to, and
//! the hover menu that changes it. Split out on its own because all three are one interaction —
//! typing, seeing who will answer, and switching who that is — not three unrelated controls that
//! happened to live at the bottom of `chat.rs`.
//!
//! Every component starts from the same `use` block, copied from `main.rs` when the split
//! happened, so most files import more than they need. Quietened rather than hand-trimmed
//! nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
//! nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, sidebar::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

impl Workbench {
    /// The input row: the text field plus a Send affordance.
    pub(crate) fn composer_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Three states, which is what every shipped chat composer converged on: a filled
        // circular button that sends, the same button greyed when there is nothing to
        // send, and a stop control while a turn streams. Empty-means-disabled is the
        // near-universal rule, and a send/stop toggle in the composer is how the running
        // state is expressed without adding a second control (docs §52).
        let has_text = !self.composer.read(cx).text().trim().is_empty();
        let (send_icon, send_style, hint) = if self.streaming {
            ("icons/stop-circle.svg", ui::ButtonStyle::Danger, "Stop this turn")
        } else if has_text {
            ("icons/paper-plane-right.svg", ui::ButtonStyle::Primary, "Send")
        } else {
            (
                "icons/paper-plane-right.svg",
                ui::ButtonStyle::SecondaryWhite,
                "Type a question first",
            )
        };
        let send_disabled = !has_text && !self.streaming;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_none()
            .m_2()
            .px_2()
            .py_1()
            .rounded_lg()
            .text_sm()
            // The composer reads as one field with a control inside it, rather than a
            // text box sitting next to an unrelated button.
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            // Which field has the keyboard is otherwise invisible — there is a caret, and it
            // is two pixels wide. `in_focus` rather than `focus` because the thing with the
            // focus is a child entity, not this box.
            .track_focus(&self.composer.focus_handle(cx))
            // .in_focus(|style| style.border_color(rgb(theme::accent())))
            // **Feedback for a gesture that had none.** A file dragged over the window changed
            // nothing on screen, so there was no way to tell the app would take it until you
            // let go. Lit here rather than over the whole window because this is where the
            // file lands: it becomes part of the question, and the drop does not send it.
            //
            // A style refinement rather than a flag on `Workbench`, deliberately. gpui clears
            // `active_drag` on `FileDropEvent::Exited` but dispatches that event to no element,
            // so a flag set on enter would have no way to learn the drag left the window and
            // would stay lit until the next drop.
            .drag_over::<gpui::ExternalPaths>(|style, _paths, _window, _cx| {
                style
                    .bg(rgb(theme::accent_soft()))
                    .border_color(rgb(theme::accent()))
            })
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    workbench.open_context_menu(event.position, menu::Target::Composer, cx);
                }),
            )
            .child(
                ui::Button::new("attach-file")
                    .icon(ui::Icon::new("icons/plus.svg"))
                    .style(ui::ButtonStyle::SecondaryWhite)
                    .border(false)
                    .tooltip("Add a file from this computer")
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.choose_files(cx);
                    })),
            )
            .child(self.composer.clone())
            .child(
                ui::Button::new("send-turn")
                    .icon(ui::Icon::new(send_icon))
                    .style(send_style)
                    .border(false)
                    .disabled(send_disabled)
                    .tooltip(hint)
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

    /// The composer row and the agent indicator above it, sharing one `.relative()` box.
    ///
    /// The indicator used to anchor to the transcript's own scroll box, several elements higher
    /// up the tree — right when nothing else stood between them, and wrong the moment
    /// `collected_banner` or `attachment_chips` rendered, since neither shifts where the
    /// transcript box's own bottom edge is. Anchored to *this* box instead, the indicator sits
    /// against the composer's own top edge no matter what else is showing above it (§263).
    pub(crate) fn composer_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .min_w_0()
            .children(self.agent_menu(cx))
            .child(self.composer_row(cx))
    }
}


/// The agent menu's own floating shell.
///
/// Built on [`ui::menu_card`] — the chrome-only lower half [`ui::Menu`] is itself built from —
/// rather than on `Menu` directly, because this needs the opposite of both things `Menu` bakes
/// in: open while the pointer is over the indicator rather than opened by a click, and pivoted
/// from its own *bottom-left* corner rather than its top-left, so it grows upward from wherever
/// it sits instead of downward past the composer underneath it. Kept private to this file, since
/// no other caller in the app wants either change — `Menu`'s own callers all want the shape it
/// already has.
#[derive(IntoElement)]
struct AgentMenu {
    items: Vec<gpui::AnyElement>,
}

impl AgentMenu {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_any_element());
        self
    }
}

impl RenderOnce for AgentMenu {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut card = ui::menu_card();
        for item in self.items {
            card = card.child(item);
        }
        gpui::deferred(
            gpui::anchored()
                // Relative to the nearest positioned ancestor — the indicator's own wrapper —
                // rather than the window, so no click position or other measurement is needed
                // to place it: it is always "right above wherever the indicator sits."
                .position_mode(gpui::AnchoredPositionMode::Local)
                // The point below becomes this corner of the popup rather than `Menu`'s default
                // top-left, so the popup's own bottom edge sits at the indicator's top edge and
                // it grows upward from there as rows are added, rather than downward from it.
                .anchor(gpui::Corner::BottomLeft)
                .position(gpui::point(px(0.), px(0.)))
                .child(card),
        )
    }
}

impl Workbench {
    /// The pill above the composer naming who a turn will go to, and — while the pointer is
    /// over it or the menu it opens — every specialist available to switch to instead.
    ///
    /// Replaces typing `/name` into an inline picker (§55) with hovering a name (§263). The
    /// choice lives in `current_subagent`, not in the composer's text — `start_turn_as` folds it
    /// into the same `/name` prefix `subagent::parse` already reads, so only *how* a specialist
    /// gets named changed, not what happens once one has been.
    pub(crate) fn agent_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let agents = workspace::subagents();
        let current = self
            .current_subagent
            .as_deref()
            .and_then(|name| agents.iter().find(|agent| agent.name == name));
        let current_display = current.map(|agent| subagent::display(&agent.name));

        let open = self.agent_pill_hovered || self.agent_menu_hovered;

        // The containing block `AgentMenu` anchors against below — its own top edge is that
        // popup's `(0, 0)`, and the popup's bottom edge is pinned there — so this wrapper must
        // be `.relative()`, and the trigger inside it must be the only thing that gives it a
        // size: the popup itself is `deferred`, out of flow, and does not.
        let mut indicator = div().relative().flex_none().child(
            div()
                .id("agent-indicator")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .flex_none()
                .px_3()
                .py_1()
                .rounded_md()
                .rounded_b_none()
                .bg(rgb(theme::surface()))
                .border_1()
                .border_b_0()
                .border_color(rgb(theme::border()))
                .hover(|style| style.cursor_pointer())
                .child(
                    ui::Icon::new("icons/agent-ellipse.svg")
                        .size(ui::IconSize::ExtraSmall)
                        .colour(
                            current_display
                                .as_ref()
                                .map(|(_, colour)| *colour)
                                .unwrap_or_else(theme::text_muted),
                        ),
                )
                .child(
                    ui::Label::new(
                        current_display
                            .as_ref()
                            .map(|(name, _)| name.clone())
                            .unwrap_or_else(|| "Auto".to_string()),
                    )
                    .colour(if current.is_some() {
                        theme::text()
                    } else {
                        theme::text_muted()
                    })
                    .size(ui::Size::Regular),
                )
                .on_hover(cx.listener(|workbench, hovering: &bool, _window, cx| {
                    workbench.agent_pill_hovered = *hovering;
                    cx.notify();
                })),
        );

        // Pinned to `composer_input`'s own top edge — its bottom-left corner, not its own
        // top-left, is the point given below (`Corner::BottomLeft`), so it grows upward from
        // there as the trigger's height changes rather than needing that height known in
        // advance. Never pushes the composer down either way: `Anchored` is itself out of flow,
        // and the trigger inside is its only sized content.
        if !open {
            return Some(anchor_above_composer(indicator).into_any_element());
        }

        let mut list = div()
            .id("agent-menu-list")
            .flex()
            .flex_col()
            .w(px(280.))
            .min_w_0()
            .max_h(px(220.))
            .overflow_y_scroll()
            // Tracked the same way `theme_list`/`model_list` track theirs, or a list taller
            // than `max_h` hit-tests rows against their pre-scroll layout.
            .track_scroll(&self.agent_menu_scroll)
            .on_hover(cx.listener(|workbench, hovering: &bool, _window, cx| {
                workbench.agent_menu_hovered = *hovering;
                cx.notify();
            }))
            .child(
                // Back to the coordinator — the only way to undo a choice now that picking
                // one no longer leaves anything in the composer to delete.
                div()
                    .id("agent-auto")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .hover(|style| style.bg(rgb(theme::background())).cursor_pointer())
                    .child(
                        ui::Icon::new("icons/agent-ellipse.svg")
                            .size(ui::IconSize::ExtraSmall)
                            .colour(theme::text_muted()),
                    )
                    .child(
                        ui::Label::new("Auto")
                            .colour(if current.is_none() {
                                theme::text()
                            } else {
                                theme::text_muted()
                            })
                            .ellipsis(),
                    )
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.choose_subagent(None, cx);
                    })),
            );

        if agents.is_empty() {
            // The registry is written when the backend assembles a coordinator, so before
            // the first turn there is nothing to offer. Say which, rather than showing an
            // empty box.
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
        }
        for agent in &agents {
            let chosen = agent.name.clone();
            let selected = current.is_some_and(|c| c.name == agent.name);
            let (label, colour) = subagent::display(&agent.name);
            list = list.child(
                div()
                    .id(SharedString::from(format!("agent-{}", agent.name)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .hover(|style| style.bg(rgb(theme::background())).cursor_pointer())
                    .child(
                        ui::Icon::new("icons/agent-ellipse.svg")
                            .size(ui::IconSize::ExtraSmall)
                            .colour(colour),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                ui::Label::new(label)
                                    .colour(if selected {
                                        theme::text()
                                    } else {
                                        theme::text_muted()
                                    })
                                    .ellipsis(),
                            )
                            // The description is not decoration: none of these names says
                            // what it does, and the request's own guesses show that
                            // nobody can be expected to know.
                            .child(
                                ui::Label::new(agent.description.clone())
                                    .colour(theme::text_muted())
                                    .size(ui::Size::Compact)
                                    .ellipsis(),
                            ),
                    )
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        workbench.choose_subagent(Some(chosen.clone()), cx);
                    })),
            );
        }

        indicator = indicator.child(AgentMenu::new().item(list));

        Some(anchor_above_composer(indicator).into_any_element())
    }
}

/// `composer_row`'s own `.m_2()` — 8px on every side, sitting *outside* its border/background
/// box. `composer_input`'s own top edge lands there, not on the composer's visible box, so
/// anchoring straight to it left the indicator floating 8px above the composer with a visible
/// gap. Named so the two can't quietly drift apart the next time either one changes.
const COMPOSER_MARGIN: f32 = 8.;

/// Positions an element's bottom-left corner at the composer's own visible top edge — inside
/// `composer_row`'s margin, not at `composer_input`'s box edge — so it grows upward from there,
/// `TRANSCRIPT_INSET` in from the left, regardless of the element's own height. Shared by both
/// the closed (trigger only) and open (trigger plus menu) shapes `agent_menu` returns, since both
/// anchor the same way.
fn anchor_above_composer(child: impl IntoElement) -> impl IntoElement {
    gpui::anchored()
        .position_mode(gpui::AnchoredPositionMode::Local)
        .anchor(gpui::Corner::BottomLeft)
        .position(gpui::point(px(TRANSCRIPT_INSET), px(COMPOSER_MARGIN)))
        .child(child)
}
