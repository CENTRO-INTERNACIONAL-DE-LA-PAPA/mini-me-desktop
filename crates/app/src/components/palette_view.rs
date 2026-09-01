// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

/// One row of a picker: a label, a tick when it is the current choice, and an optional note.
///
/// Shared so every picker in this window looks the same and states the same thing the same way —
/// the theme list, the model list and the per-specialist list had drifted into three shapes.
pub(crate) fn picker_row(
    label: impl Into<SharedString>,
    selected: bool,
    note: Option<String>,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    div()
        .id(SharedString::from(format!("row-{label}")))
        .flex()
        // **A column, because the note was eating the name.** Both sat in one row competing for
        // width, and since the label is the one that ellipsises, `gpt-4.1 · OpenAI — billed
        // separately` rendered as `gpt-4.` — reported as *"I cannot read the complete model
        // name"*. Stacking gives the id the full width it needs, which matters more now that a
        // gateway's ids look like `meta-llama/llama-3.3-70b-instruct` (docs §188).
        .flex_col()
        // **No `items_start`.** It sets the cross axis to content width, and `Label::ellipsis`
        // grows to fill a width it then truncates to — so with content width there was nothing to
        // fill and every row rendered as a bare "…", reported as *"I can't select models for the
        // subagents"*. Stretch is the default and is what a full-width row wants (§59, §190).
        .gap_0p5()
        .w_full()
        .min_w_0()
        .px_2()
        .py_1()
        .rounded_md()
        .when(selected, |row| row.bg(rgb(theme::accent_soft())))
        // **Inherited, not set on the label.** A vivid hover fill needs its ink to flip, and
        // `ui::Label::colour` writes the colour onto the element itself — which a parent's hover
        // refinement cannot override (the same rule that stops `text_color` reaching an SVG,
        // §157). So the row states the resting colour and the hover restates both together, which
        // is the only arrangement where the two can disagree (docs §189).
        .text_color(rgb(if selected {
            theme::text()
        } else {
            theme::text_muted()
        }))
        .hover(|style| {
            let fill = theme::hover_over(theme::elevated());
            style
                .bg(rgb(fill))
                .text_color(rgb(theme::ink_on(fill)))
                .cursor_pointer()
        })
        // **No `ellipsis`, and this one was settled by comparison rather than by reasoning.**
        // The provider headings two lines away are the same `Label` without it and render their
        // text correctly, so the truncate path — `overflow_hidden` + `text_ellipsis` — is the
        // difference, and it collapsed every model name to a bare "…" through three attempted
        // fixes (§59, §190, §192). A row that is already a column has somewhere to put a long id:
        // it wraps. That is worse than truncating and enormously better than showing nothing.
        .child(ui::Label::new(label).inherit())
        .children(note.map(|note| {
            // Muted, not red: a missing key is a thing to do next, not a thing done wrong.
            ui::Label::new(note)
                .colour(theme::warning())
                .size(ui::Size::Compact)
        }))
}


impl Workbench {
    /// The floating list a [`Picker`] shows: its filter field, then its rows.
    pub(crate) fn picker_popup(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
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
            Picker::Project => self.project_list(false, cx).into_any_element(),
            Picker::NewProject => self.project_list(true, cx).into_any_element(),
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
}


impl Workbench {
    /// The palette overlay: a query field over a filtered command list.
    ///
    /// Rendered as the root's last child so it paints above the panes; it is
    /// `absolute`, so it takes no part in the three-pane flex layout.
    pub(crate) fn palette(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            // Same reason as the preview backdrop: an overlay that does not occlude leaves the
            // window under it clickable, so choosing a command could also press whatever the row
            // happened to be drawn over (docs §163).
            .occlude()
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
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .border_t_1()
                            .border_color(rgb(theme::border()))
                            .text_color(rgb(theme::text_muted()))
                            .text_xs()
                            .child("↑↓ select ·")
                            .child(app_icon("icons/enter.svg", theme::text_muted(), None))
                            .child("run · esc close"),
                    ),
            )
    }
}

