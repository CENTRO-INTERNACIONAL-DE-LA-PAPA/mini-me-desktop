// Every component starts from the same `use` block, copied from `main.rs` when the split
// happened, so most files import more than they need. Quietened rather than hand-trimmed
// nine times over — but `dead_code` is deliberately NOT allowed here: these modules are
// nothing but render methods, and one nobody calls is a feature that stopped being drawn.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{sidebar::*, chat::*, gallery_view::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

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


/// One line of the activity trace: a tool call, or a delegation.
pub(crate) fn step_line(label: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .text_color(rgb(theme::text_muted()))
        .text_xs()
        .child(format!("· {label}"))
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

/// How many lines of a text file's own content a thumbnail reads.
const TEXT_PREVIEW_LINES: usize = 12;

/// Extensions read as plain text for a thumbnail, rather than shown as a bare glyph.
///
/// An allowlist rather than "try to read it and see what comes back": `workspace::head` reads
/// line by line and a binary file's first few bytes are not reliably invalid UTF-8 — a PDF's
/// own header and cross-reference table are plain ASCII — so sniffing content risks a tile full
/// of PDF syntax rather than the icon PDFs still get. Real PDF rendering is a separate feature;
/// this one is scoped to files a person would call "text".
const TEXT_PREVIEW_EXTENSIONS: &[&str] = &[
    "md", "txt", "py", "js", "jsx", "ts", "tsx", "json", "yaml", "yml", "toml", "rs", "go",
    "java", "kt", "c", "cc", "cpp", "h", "hpp", "cs", "rb", "php", "swift", "css", "scss",
    "html", "htm", "xml", "sh", "bash", "zsh", "ps1", "r", "sql", "log", "ini", "cfg", "conf",
    "csv", "tsv", "env", "md", "rst", "tex", "bib", "makefile", "dockerfile", "gradle", "pom", "vbs", "lua", "pl"
];

/// The first few lines of a file's own content, for a thumbnail — `None` for anything not on
/// [`TEXT_PREVIEW_EXTENSIONS`] or that fails to read as text (a spreadsheet saved with the wrong
/// extension, say).
///
/// Lives here, not in `gallery_view.rs` or `chat.rs`, because both the Pinboard's tiles and the
/// chat's attachment tiles use it — one shared function neither file owns, the same shape the
/// sources-rendering split settled on.
pub(crate) fn text_preview_lines(output: &workspace::Output) -> Option<Vec<String>> {
    let extension = output
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !TEXT_PREVIEW_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let text = workspace::head(&output.path, TEXT_PREVIEW_LINES).ok()?;
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    if lines.is_empty() {
        return None;
    }
    Some(lines)
}

/// Truncate one preview line from the front — the opposite end from [`distinguishing_tail`],
/// because a line of code or prose reads left to right and its start is what identifies it,
/// unlike a filename sharing a long prefix with its siblings.
fn truncate_line_head(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars || max_chars == 0 {
        return line.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", line.chars().take(keep).collect::<String>())
}

/// The preview's own font size. Named so [`preview_chars`] can stay in step with it — the
/// column count this whole tile is about got left behind once already when this shrank from 9px
/// to 6px and the width estimate below did not follow, so lines cut off with the tile's right
/// half still empty.
const PREVIEW_FONT_PX: f32 = 6.;

/// How many monospace characters fit one preview line at a tile's width.
///
/// `0.6` is the rough advance-width-to-em-size ratio for the monospace stack `code_font()`
/// picks per platform (Consolas, Menlo, DejaVu Sans Mono) — a fixed pixel divisor tuned for one
/// font size silently stops matching reality the moment the size changes, which is exactly what
/// left this tile filling only its left half.
fn preview_chars(tile: f32) -> usize {
    (((tile - 8.) / (PREVIEW_FONT_PX * 0.6)) as usize).max(6)
}

/// A small monospace snippet of a text file's own content, sized to fill a tile's media box —
/// the same slot an image thumbnail fills, so a `.py` or `.md` reads as "here is the file" the
/// way a plot already does, rather than a bare glyph standing in for it.
pub(crate) fn text_preview_tile(lines: &[String], tile: f32) -> gpui::AnyElement {
    let chars_per_line = preview_chars(tile);
    let mut block = div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .p_1()
        .gap(px(1.))
        .font(ui::code_font())
        .text_color(rgb(theme::text_muted()))
        .text_size(px(PREVIEW_FONT_PX));
    for line in lines {
        block = block.child(div().min_w_0().child(truncate_line_head(line, chars_per_line)));
    }
    block.into_any_element()
}
