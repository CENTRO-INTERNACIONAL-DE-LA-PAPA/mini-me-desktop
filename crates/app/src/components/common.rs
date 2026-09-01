// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::components::{sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

/// A theme-tinted 14px icon, matching the text it replaces without changing control geometry.
///
/// **`ink` is required, and that is the whole point.** GPUI paints an SVG by rasterising it to a
/// mask and multiplying by `style.text.color` — and `Svg::paint` is literally
/// `self.path.as_ref().zip(style.text.color)`, so a `None` there paints *nothing at all*. That
/// colour is not inherited: `Interactivity::compute_style` starts from `Style::default()`, whose
/// `text.color` is `None`, and refines it with the element's **own** styles. A parent's
/// `.text_color(…)` never reaches the child (docs §157).
///
/// So the argument is not a convenience. Without it every icon here is invisible, and no test
/// that reads the SVG file can tell — which is why this is a parameter the compiler demands
/// rather than a rule written down.
pub(crate) fn app_icon(path: &'static str, ink: u32, size: Option<f32>) -> impl IntoElement {
    app_icon_at(path, ink, size.unwrap_or(ui::IconSize::Medium.px()))
}


/// The same, at a size the caller chooses — a file tile wants more than 14px.
pub(crate) fn app_icon_at(path: &'static str, ink: u32, size: f32) -> impl IntoElement {
    svg()
        .path(path)
        .w(px(size))
        .h(px(size))
        .flex_none()
        .text_color(rgb(ink))
}


/// [`section_label`] for a heading only known at runtime.
pub(crate) fn section_label_owned(text: String) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}


/// A small caps-ish section heading for the side panel.
///
/// **Faint, not accent.** A heading is not something you can click, and the accent is the app's
/// one signal that you can — `OUTPUTS`, `SUGGESTED NEXT` and `THE SPECIALISTS` shouting in the
/// brand colour is most of why the window read as busy. Moving these to `text_faint` was the
/// single largest change in the redesign and it is one line.
pub(crate) fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_faint()))
        .text_xs()
        .child(text)
}


/// The glyph and colour that stand for a file's kind.
///
/// Finer than [`workspace::Kind`], which groups by what a researcher *does* with a file and is
/// the right grouping for the panel's sections. Here a PDF and a Markdown note want telling
/// apart at a glance even though both are things you read.
///
/// Four colours, from the palette's status roles rather than a new set — the same argument as
/// the provenance chips: a colour per file type is a legend nobody memorises.
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


/// The card every popup menu is drawn on.
///
/// One definition because the discipline is easy to omit and invisible when it is: a menu must
/// `occlude`, or a click on a row also lands on whatever the menu was drawn over (§163), and it
/// must swallow the left press, or choosing an item starts a text selection in the transcript
/// underneath. The right-click menu learned both the hard way; a second menu written from scratch
/// beside it would have learned them again.
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
    // Floored, so a very long transcript still leaves something big enough to see.
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


/// How wide the thumb is for a rail showing `viewport` of `viewport + overflow` content.
///
/// Split out from the metrics only so it can be tested without a laid-out `ScrollHandle`; the
/// metrics still compute it exactly once, which is the property the type above exists to hold.
///
/// Two bounds, and the second matters as much as the first. The 28px floor keeps a thumb
/// grabbable on a long rail. The `viewport` ceiling keeps that floor from exceeding the track it
/// sits in: without it a rail narrower than 28px yields a *negative* `travel`, so the thumb is
/// painted to the left of its own track while `horizontal_drag_offset` refuses to move it — the
/// "looked interactive, wasn't" shape of §158, one case further out.
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
///
/// **The boundary the researcher asked for**, in their words: *"I want to group images and in
/// another group other files."* §152's gallery grouped by the folder the agent chose, which was
/// right about structure and wrong about kind — a folder holding seven plots and a summary CSV
/// put the CSV in the middle of the strip, and the strip is the thing you flick through looking
/// for a figure.
///
/// `Kind::Figure` is the test rather than the extension, so this cannot disagree with the
/// thumbnail renderer about what an image is: both ask the same enum.
pub(crate) fn split_images(
    outputs: &[workspace::Output],
) -> (Vec<workspace::Output>, Vec<workspace::Output>) {
    outputs
        .iter()
        .cloned()
        .partition(|output| output.kind == workspace::Kind::Figure)
}


/// A visible scrollbar for GPUI's variable-height list.
///
/// `list` stores its offset in [`ListState`], not a `ScrollHandle`. This keeps §40's visible
/// affordance without adding a second scroll container around the virtual list (docs §156).
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
    /// The bordered box a filter composer sits in.
    ///
    /// One helper because the theme popup and the gallery both want it, and because it is the
    /// only place a *focus ring* has anywhere to attach: the composer is a child entity, so
    /// the wrapper has to track its handle and light up with `in_focus`.
    pub(crate) fn filter_field(&self, field: Entity<Composer>, cx: &App) -> impl IntoElement {
        div()
            .track_focus(&field.focus_handle(cx))
            // Stated, not inherited. The gallery's box was built by hand and looked identical
            // in the source, and it came out a quarter the width with its placeholder spilling
            // out the side — it was relying on flex stretch, and the two boxes did not agree
            // about whether they got it (docs §72).
            //
            // A **flex row**, because `w_full` alone was not enough and §72 came back: a `div` is
            // `Display::Block` by default in gpui, so the field inside had no row to fill and its
            // own `width: 100%` had nothing definite to resolve against (docs §88).
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

