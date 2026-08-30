#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

/// A theme-tinted icon. `ink` is required: GPUI does not inherit `text_color` into an `Svg`'s
/// paint colour, so an icon built without it renders invisibly.
pub(crate) fn app_icon(path: &'static str, ink: u32, size: Option<f32>) -> impl IntoElement {
    app_icon_at(path, ink, size.unwrap_or(ui::IconSize::Medium.px()))
}

pub(crate) fn app_icon_at(path: &'static str, ink: u32, size: f32) -> impl IntoElement {
    svg()
        .path(path)
        .w(px(size))
        .h(px(size))
        .flex_none()
        .text_color(rgb(ink))
}

/// A small caps-ish section heading.
pub(crate) fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}

/// [`section_label`] for a heading only known at runtime.
pub(crate) fn section_label_owned(text: String) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}

/// The glyph and colour standing for a file's kind, keyed off its extension.
pub(crate) fn file_mark(path: &std::path::Path) -> (&'static str, u32) {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" | "tsv" | "xlsx" | "xls" | "parquet" | "feather" => {
            ("icons/file-table.svg", theme::success())
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => {
            ("icons/file-image.svg", theme::running())
        }
        "py" | "r" | "jl" | "sh" | "js" | "ts" | "rs" | "sql" => {
            ("icons/file-code.svg", theme::accent())
        }
        "ipynb" => ("icons/file-notebook.svg", theme::accent()),
        "json" | "yaml" | "yml" | "toml" | "xml" => ("icons/file-data.svg", theme::warning()),
        "html" | "htm" => ("icons/file-web.svg", theme::warning()),
        "md" | "txt" | "rst" => ("icons/file-text.svg", theme::text_muted()),
        "log" | "out" | "err" => ("icons/file-log.svg", theme::text_faint()),
        "pdf" | "docx" | "doc" | "typ" => ("icons/file-doc.svg", theme::error()),
        "zip" | "gz" | "tar" | "tgz" | "7z" => ("icons/file-archive.svg", theme::text_muted()),
        "db" | "sqlite" | "sqlite3" | "duckdb" => ("icons/file-db.svg", theme::success()),
        _ => ("icons/file-blank.svg", theme::text_muted()),
    }
}

/// The card every popup menu is drawn on. Must `occlude` and swallow the left press, or a click
/// falls through to whatever is underneath (the row it's anchored to, or the transcript below it).
pub(crate) fn menu_card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(190.))
        .py_1()
        .rounded_md()
        .bg(rgb(theme::elevated()))
        .border_1()
        .border_color(rgb(theme::border_strong()))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, |_event, _window, cx| {
            cx.stop_propagation();
        })
}

/// One line of the activity trace: a tool call, or a delegation.
pub(crate) fn step_line(label: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(theme::text_muted()))
        .text_xs()
        .child(format!("· {label}"))
}

pub(crate) fn scrollbar(handle: &gpui::ScrollHandle) -> Option<impl IntoElement> {
    let overflow = handle.max_offset().height;
    let viewport = handle.bounds().size.height;
    if overflow <= px(0.) || viewport <= px(0.) {
        return None;
    }
    let content = viewport + overflow;
    let thumb = (viewport * (viewport / content)).max(px(28.));
    let travel = viewport - thumb;
    let progress = (-handle.offset().y / overflow).clamp(0.0, 1.0);

    Some(
        div()
            .absolute()
            .invisible()
            .group_hover(SCROLL_GROUP, |style| style.visible())
            .top(travel * progress)
            .right(px(2.))
            .w(px(6.))
            .h(thumb)
            .rounded_full()
            .bg(rgb(theme::border_strong())),
    )
}

/// A visible scrollbar for GPUI's variable-height `List`, which tracks offset in `ListState`
/// rather than a `ScrollHandle`.
pub(crate) fn list_scrollbar(state: &ListState) -> Option<impl IntoElement> {
    let overflow = state.max_offset_for_scrollbar().height;
    let viewport = state.viewport_bounds().size.height;
    if overflow <= px(0.) || viewport <= px(0.) {
        return None;
    }
    let content = viewport + overflow;
    let thumb = (viewport * (viewport / content)).max(px(28.));
    let travel = viewport - thumb;
    let progress = (-state.scroll_px_offset_for_scrollbar().y / overflow).clamp(0.0, 1.0);

    Some(
        div()
            .absolute()
            .invisible()
            .group_hover(SCROLL_GROUP, |style| style.visible())
            .top(travel * progress)
            .right(px(2.))
            .w(px(6.))
            .h(thumb)
            .rounded_full()
            .bg(rgb(theme::border_strong())),
    )
}

/// Thumb width for a rail showing `viewport` of `viewport + overflow` content. Floored at 28px so
/// it stays grabbable, ceilinged at `viewport` so a very narrow rail can't produce negative travel.
pub(crate) fn horizontal_thumb_width(viewport: gpui::Pixels, overflow: gpui::Pixels) -> gpui::Pixels {
    let content = viewport + overflow;
    (viewport * (viewport / content)).max(px(28.)).min(viewport)
}

pub(crate) fn horizontal_scroll_metrics(handle: &gpui::ScrollHandle) -> Option<HorizontalScrollMetrics> {
    let overflow = handle.max_offset().width;
    let viewport = handle.bounds().size.width;
    if overflow <= px(0.) || viewport <= px(0.) {
        return None;
    }
    let thumb = horizontal_thumb_width(viewport, overflow);
    let travel = viewport - thumb;
    let progress = (-handle.offset().x / overflow).clamp(0.0, 1.0);
    Some(HorizontalScrollMetrics {
        overflow,
        viewport,
        thumb,
        travel,
        progress,
    })
}

/// Convert a dragged thumb position into GPUI's negative content offset.
pub(crate) fn horizontal_drag_offset(
    pointer_x: gpui::Pixels,
    track_left: gpui::Pixels,
    grab_x: gpui::Pixels,
    travel: gpui::Pixels,
    overflow: gpui::Pixels,
) -> gpui::Pixels {
    if travel <= px(0.) {
        return px(0.);
    }
    let thumb_left = (pointer_x - track_left - grab_x).clamp(px(0.), travel);
    -(overflow * (thumb_left / travel))
}

/// Images in one group, everything else in another, each keeping its listing order.
pub(crate) fn split_images(
    outputs: &[workspace::Output],
) -> (Vec<workspace::Output>, Vec<workspace::Output>) {
    outputs
        .iter()
        .cloned()
        .partition(|output| output.kind == workspace::Kind::Figure)
}

impl Render for Hint {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::overlay()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            .text_color(rgb(theme::text()))
            .text_xs()
            .child(self.text.clone())
    }
}

impl Workbench {
    /// A visible, clickable, draggable horizontal scrollbar for one gallery rail. The whole
    /// track is a hit target, not just the thumb: clicking off-thumb jumps toward that position.
    pub(crate) fn horizontal_scrollbar(
        &self,
        id: String,
        handle: &gpui::ScrollHandle,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let metrics = horizontal_scroll_metrics(handle)?;
        let track_left = handle.bounds().origin.x;
        let thumb_left = metrics.travel * metrics.progress;
        let dragged = handle.clone();

        Some(
            div()
                .id(SharedString::from(format!("gallery-scrollbar-{id}")))
                .absolute()
                .bottom(px(0.))
                .left(px(0.))
                .w(metrics.viewport)
                .h(px(12.))
                .hover(|style| style.cursor_pointer())
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(thumb_left)
                        .h(px(8.))
                        .w(metrics.thumb)
                        .rounded_full()
                        .bg(rgb(theme::border_strong())),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |workbench, event: &gpui::MouseDownEvent, _window, cx| {
                        let local_x =
                            (event.position.x - track_left).clamp(px(0.), metrics.viewport);
                        let grab_x = if local_x >= thumb_left
                            && local_x <= thumb_left + metrics.thumb
                        {
                            local_x - thumb_left
                        } else {
                            metrics.thumb / 2.
                        };
                        let offset_x = horizontal_drag_offset(
                            event.position.x,
                            track_left,
                            grab_x,
                            metrics.travel,
                            metrics.overflow,
                        );
                        let offset_y = dragged.offset().y;
                        dragged.set_offset(gpui::point(offset_x, offset_y));
                        workbench.gallery_scroll_drag = Some(GalleryScrollDrag {
                            handle: dragged.clone(),
                            track_left,
                            grab_x,
                            travel: metrics.travel,
                            overflow: metrics.overflow,
                        });
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ),
        )
    }

    /// The generic right-click context menu; item set and enabled state come from `open.target`.
    pub(crate) fn context_menu(&self, open: menu::ContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let target = open.target;
        let mut panel = menu_card();

        for &item in open.items() {
            let enabled = self.menu_item_enabled(item, target, cx);
            let shortcut = item.shortcut(target);
            panel = panel.child(
                div()
                    .id(SharedString::from(format!("menu-{}", item.label())))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(if enabled {
                        theme::text()
                    } else {
                        theme::text_faint()
                    }))
                    .when(enabled, |row| {
                        row.hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer())
                            .on_click(cx.listener(move |workbench, _event, window, cx| {
                                workbench.run_menu_item(item, target, window, cx);
                            }))
                    })
                    .child(item.label())
                    .child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .text_xs()
                            .child(shortcut),
                    ),
            );
        }

        gpui::deferred(gpui::anchored().position(open.at).snap_to_window().child(
            panel.on_mouse_down_out(cx.listener(
                |workbench, event: &gpui::MouseDownEvent, _window, cx| {
                    // A right-click elsewhere re-opens the menu at the new spot; closing here too
                    // would race that handler and could leave no menu at all.
                    if event.button == gpui::MouseButton::Right {
                        return;
                    }
                    workbench.context_menu = None;
                    cx.notify();
                },
            )),
        ))
    }

    /// The draggable edge between two panes. Drag state is tracked on the root, not this strip,
    /// so it keeps following the pointer even once it outruns the 4px strip itself.
    pub(crate) fn pane_divider(&self, edge: Divider, cx: &mut Context<Self>) -> impl IntoElement {
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

    /// The bordered box a filter/search composer sits in, shared so every instance gets the same
    /// focus-ring behaviour (the composer is a child entity, so the wrapper tracks its handle).
    pub(crate) fn filter_field(&self, field: Entity<Composer>, cx: &App) -> impl IntoElement {
        div()
            .track_focus(&field.focus_handle(cx))
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .min_w_0()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .in_focus(|style| style.border_color(rgb(theme::accent())))
            .child(field)
    }
}
