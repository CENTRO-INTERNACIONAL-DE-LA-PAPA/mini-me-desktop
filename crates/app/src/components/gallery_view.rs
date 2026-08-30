#![allow(dead_code, unused_imports)]

use crate::*;
use crate::components::{common::*, sidebar::*, chat::*, provenance_view::*, settings_view::*, palette_view::*, modals::*, status_bar::*};
use gpui::{
    actions, div, img, prelude::*, px, relative, rgb, size, svg, App, Application, AssetSource,
    Bounds, ClipboardItem, Context, Div, Entity, Focusable, FontStyle, FontWeight, HighlightStyle,
    KeyBinding, ListAlignment, ListState, SharedString, StyledText, Window, WindowBounds, WindowOptions,
};

/// The `+N` glyph, sized to the tile it sits on.
pub(crate) fn media_scrim_size(tile: f32) -> f32 {
    (tile / 4.).max(18.)
}


/// How many characters of a filename fit across a tile at `text_xs` (measured, not layout-truncated).
pub(crate) fn name_chars(tile: f32) -> usize {
    (((tile - 16.) / 6.) as usize).max(8)
}


/// How many tiles the grid draws, and how many images the last tile stands in for.
pub(crate) fn image_grid_shape(total: usize) -> (usize, usize) {
    let shown = total.min(IMAGE_GRID_TILES);
    // The scrimmed tile counts among the hidden, since the overlay covers it too.
    let hidden = if total > IMAGE_GRID_TILES {
        total - (IMAGE_GRID_TILES - 1)
    } else {
        0
    };
    (shown, hidden)
}


pub(crate) fn output_folder_groups(outputs: &[workspace::Output]) -> Vec<OutputFolderGroup<'_>> {
    let mut groups: Vec<OutputFolderGroup<'_>> = Vec::new();
    for output in outputs {
        let folder = std::path::Path::new(&output.name)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""))
            .to_path_buf();
        if let Some(group) = groups.iter_mut().find(|group| group.folder == folder) {
            group.outputs.push(output);
        } else {
            groups.push(OutputFolderGroup {
                folder,
                outputs: vec![output],
            });
        }
    }
    groups
}


/// Heading for a folder: the producing worker's name (if known) ahead of the agent's own
/// path, with a leading thread-id folder component dropped.
pub(crate) fn output_folder_label(folder: &std::path::Path, worker: Option<&str>) -> String {
    let mut components: Vec<String> = folder
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    let removed_thread = components
        .first()
        .is_some_and(|component| workspace::looks_like_thread_id(component));
    if removed_thread {
        components.remove(0);
    }
    if let Some(name) = worker {
        components.insert(0, name.to_string());
    }
    if components.is_empty() {
        if removed_thread {
            "Background task files".to_string()
        } else {
            "Conversation files".to_string()
        }
    } else {
        components.join(" / ")
    }
}


/// The worker thread a file sits under, when its path starts with a thread-id folder.
pub(crate) fn producing_thread(output: &workspace::Output) -> Option<&str> {
    let first = std::path::Path::new(&output.name).components().next()?;
    let name = first.as_os_str().to_str()?;
    if workspace::looks_like_thread_id(name) {
        Some(name)
    } else {
        None
    }
}


/// Whether this output is a raw search-results file, kept out of the transcript's file cards.
pub(crate) fn is_search_record(output: &workspace::Output) -> bool {
    matches!(
        output.path.file_name().and_then(|name| name.to_str()),
        Some("papers.json") | Some("dataverse_search.json")
    )
}


/// Outputs grouped by producer: the conversation's own files first, then one group per
/// other author, in the order their first file appears.
pub(crate) fn by_producer(
    outputs: &[workspace::Output],
    tasks: &[protocol::AsyncTask],
    wrote: &std::collections::HashMap<String, String>,
) -> Vec<(Option<String>, Vec<workspace::Output>)> {
    let mut groups: Vec<(Option<String>, Vec<workspace::Output>)> = Vec::new();
    for output in outputs {
        let by = match producing_thread(output) {
            Some(thread) => produced_by(Some(thread), tasks),
            None => wrote
                .get(&workspace::normalise_separators(&output.name))
                .map(|agent| agent.replace('_', " ")),
        };
        match groups.iter_mut().find(|(owner, _)| *owner == by) {
            Some((_, produced)) => produced.push(output.clone()),
            None => groups.push((by, vec![output.clone()])),
        }
    }
    // The conversation's own files lead even when another author wrote first.
    groups.sort_by_key(|(owner, _)| owner.is_some());
    groups
}


/// Human-readable name of the worker behind a thread id, `None` for the conversation's own.
pub(crate) fn produced_by(thread: Option<&str>, tasks: &[protocol::AsyncTask]) -> Option<String> {
    let thread = thread?;
    Some(
        tasks
            .iter()
            .find(|task| task.thread_id == thread)
            .map(|task| task.agent_name.replace('_', " "))
            .unwrap_or_else(|| "a background task".to_string()),
    )
}


/// `15 images`, or `5 images from background worker`.
pub(crate) fn images_heading(count: usize, by: Option<&str>) -> String {
    let plural = if count == 1 { "" } else { "s" };
    match by {
        Some(who) => format!("{count} image{plural} from {who}"),
        None => format!("{count} image{plural}"),
    }
}


/// Truncate a string from the front, keeping the tail (extension/differentiator) intact.
pub(crate) fn distinguishing_tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars == 0 {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("…{}", text.chars().skip(count - keep).collect::<String>())
}


/// Shorten an `a / b / c` heading to fit by eliding the middle, keeping head and tail intact.
pub(crate) fn shorten_path_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_string();
    }
    let segments: Vec<&str> = label.split(" / ").collect();
    let (Some(head), Some(tail)) = (segments.first(), segments.last()) else {
        return distinguishing_tail(label, max_chars);
    };
    if segments.len() < 2 {
        return distinguishing_tail(label, max_chars);
    }
    let spacer = if segments.len() > 2 { " / … / " } else { " / " };
    let joined = format!("{head}{spacer}{tail}");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let room = max_chars.saturating_sub(head.chars().count() + spacer.chars().count());
    // Below four characters a trimmed tail is all ellipsis and no information.
    if room >= 4 {
        return format!("{head}{spacer}{}", distinguishing_tail(tail, room));
    }
    distinguishing_tail(label, max_chars)
}


pub(crate) fn output_filename(output: &workspace::Output) -> String {
    let name = std::path::Path::new(&output.name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&output.name);
    distinguishing_tail(name, 36)
}


impl Workbench {
    /// The file open in the centre, with the set it belongs to along the bottom.
    pub(crate) fn preview_modal(
        &self,
        output: workspace::Output,
        set: &[workspace::Output],
        at: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut body = div()
            .id("preview-body")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_grow()
            .max_h(px(PREVIEW_BODY_HEIGHT))
            .overflow_y_scroll()
            .p_3()
            .gap_2();

        match output.kind {
            workspace::Kind::Figure => {
                // Bounded box with `Contain`: letterboxes rather than cropping a tall plot.
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(PREVIEW_IMAGE_HEIGHT))
                        .child(
                            img(output.path.clone())
                                .w_full()
                                .h_full()
                                .object_fit(gpui::ObjectFit::Contain),
                        ),
                );
            }
            _ => {
                // Bounded read: a large file would otherwise be pulled fully into memory.
                match workspace::head(&output.path, 400) {
                    Ok(text) if output.name.ends_with(".md") => {
                        for parsed in markdown::parse(&text) {
                            body = body.child(markdown_block(&parsed, None));
                        }
                    }
                    Ok(text) if is_delimited(&output.name) => {
                        // Colour by column index so the eye can follow a field down rows,
                        // since GPUI has no column layout to align a wide CSV otherwise.
                        let delimiter = if output.name.ends_with(".tsv") {
                            '\t'
                        } else {
                            ','
                        };
                        for (row, line) in text.lines().enumerate() {
                            let mut cells = div().flex().flex_row().flex_wrap().w_full().gap_2();
                            for (column, cell) in line.split(delimiter).enumerate() {
                                cells = cells.child(
                                    div()
                                        .flex_none()
                                        .text_color(rgb(column_colour(column)))
                                        // The header row is what you read first.
                                        .when(row == 0, |cell| cell.font_weight(FontWeight::BOLD))
                                        .text_xs()
                                        .child(cell.trim().to_string()),
                                );
                            }
                            body = body.child(cells);
                        }
                    }
                    Ok(text) => {
                        for line in text.lines() {
                            body = body.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .child(line.to_string()),
                            );
                        }
                    }
                    Err(error) => {
                        body = body.child(
                            div()
                                .text_color(rgb(theme::error()))
                                .text_xs()
                                .child(format!("{error:#}")),
                        );
                    }
                }
            }
        }

        // Arrows only when there is somewhere to go; a lone file gets none.
        let framed = if set.len() > 1 {
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .min_w_0()
                .flex_grow()
                .overflow_hidden()
                .child(self.preview_arrow("preview-prev", "‹", -1, cx))
                .child(body)
                .child(self.preview_arrow("preview-next", "›", 1, cx))
                .into_any_element()
        } else {
            body.into_any_element()
        };

        let opened = output.path.clone();
        div()
            .id("preview-backdrop")
            .absolute()
            .inset_0()
            // Blocks mouse input to whatever sits behind this hitbox, or a click could
            // also hit a button in the transcript underneath the modal.
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(if theme::is_light(&theme::current()) {
                gpui::rgba(0x33333366)
            } else {
                gpui::rgba(0x00000099)
            })
            .child(
                div()
                    .id("preview")
                    .flex()
                    .flex_col()
                    .w(px(PREVIEW_WIDTH))
                    .max_h(px(PREVIEW_MAX_HEIGHT))
                    .bg(rgb(theme::overlay()))
                    .border_1()
                    .border_color(rgb(theme::border_strong()))
                    // Stop the click here or it bubbles to the backdrop's close-on-click.
                    .on_click(|_event, _window, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .flex_none()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(rgb(theme::border()))
                            .child(
                                div()
                                    .flex_grow()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(rgb(theme::text()))
                                    .text_sm()
                                    .child(output.name.clone()),
                            )
                            .child(
                                div()
                                    .id("preview-open")
                                    .rounded_md()
                                    .flex_none()
                                    .px_2()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .hover(|style| {
                                        style.text_color(rgb(theme::accent())).cursor_pointer()
                                    })
                                    .child("open outside")
                                    .on_click(move |_event, _window, _cx| {
                                        if let Err(error) = workspace::open(&opened) {
                                            tracing::warn!(%error, "could not open a file");
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .id("preview-close")
                                    .rounded_md()
                                    .flex_none()
                                    .px_2()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .hover(|style| {
                                        style.text_color(rgb(theme::accent())).cursor_pointer()
                                    })
                                    .child("✕")
                                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                                        workbench.preview = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(framed)
                    .children(self.preview_filmstrip(set, at, cx)),
            )
            .on_click(cx.listener(|workbench, _event, _window, cx| {
                workbench.preview = None;
                cx.notify();
            }))
    }
    /// One step-through arrow beside the previewed file.
    pub(crate) fn preview_arrow(
        &self,
        id: &'static str,
        glyph: &'static str,
        by: isize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(34.))
            .h(px(34.))
            .mx_1()
            .rounded_full()
            .bg(rgb(theme::elevated()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_color(rgb(theme::text()))
            .text_size(px(19.))
            .hover(|style| {
                style
                    .bg(rgb(theme::accent_soft()))
                    .border_color(rgb(theme::accent()))
                    .cursor_pointer()
            })
            .child(glyph)
            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                if let Some(preview) = workbench.preview.as_mut() {
                    preview.step(by);
                    cx.notify();
                }
            }))
    }
    /// The set along the bottom of the modal: a counter, then a sideways strip to choose from.
    pub(crate) fn preview_filmstrip(
        &self,
        set: &[workspace::Output],
        at: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if set.len() < 2 {
            return None;
        }
        let key = "preview-filmstrip";
        let scroll = self.output_gallery_scroll(key);

        let mut strip = div()
            .id(SharedString::from(format!("{key}-rail")))
            .flex()
            .flex_row()
            .gap_1()
            .w_full()
            .min_w_0()
            .pb_3()
            .overflow_x_scroll()
            .track_scroll(&scroll);
        for (index, output) in set.iter().enumerate() {
            let selected = index == at;
            let is_image = output.kind == workspace::Kind::Figure;
            let (glyph, ink) = file_mark(&output.path);
            strip = strip.child(
                div()
                    .id(SharedString::from(format!("{key}-{index}")))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(64.))
                    .h(px(48.))
                    .overflow_hidden()
                    .rounded_md()
                    .border_2()
                    .border_color(rgb(if selected {
                        theme::accent()
                    } else {
                        theme::border()
                    }))
                    .bg(rgb(theme::surface()))
                    .hover(|style| style.border_color(rgb(theme::accent_hover())).cursor_pointer())
                    .when(is_image, |tile| {
                        tile.child(
                            img(output.path.clone())
                                .w_full()
                                .h_full()
                                .object_fit(gpui::ObjectFit::Cover),
                        )
                    })
                    .when(!is_image, |tile| {
                        tile.child(app_icon_at(glyph, ink, 18.))
                    })
                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                        if let Some(preview) = workbench.preview.as_mut() {
                            preview.at = index;
                            cx.notify();
                        }
                    })),
            );
        }

        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_none()
                .w_full()
                .min_w_0()
                .px_3()
                .pt_2()
                .border_t_1()
                .border_color(rgb(theme::border()))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .justify_center()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(format!("{} of {}", at + 1, set.len())),
                )
                .child(
                    div()
                        .relative()
                        .w_full()
                        .min_w_0()
                        .child(strip)
                        .children(self.horizontal_scrollbar(key.to_string(), &scroll, cx)),
                ),
        )
    }
    /// One file as a card under the answer. `by` names the worker that produced it, when one did.
    pub(crate) fn output_card(
        &self,
        key: usize,
        output: &workspace::Output,
        by: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        /// Enough to see the shape of the table without becoming a table.
        const PREVIEW_ROWS: usize = 4;
        /// Past this the cells are too narrow to read in a chat pane.
        const PREVIEW_COLUMNS: usize = 4;

        let (glyph, ink) = file_mark(&output.path);
        let shape = self.shape_of(output);
        let described = match by {
            Some(who) => format!("from {who} · {}", shape.describe(output.bytes)),
            None => shape.describe(output.bytes),
        };
        let opened = output.path.clone();
        let revealed = output.path.parent().map(std::path::Path::to_path_buf);
        let previewed = output.clone();

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme::border()))
            .child(
                app_icon_at(glyph, ink, 15.),
            )
            .child(
                ui::Label::new(output.name.clone())
                    .size(ui::Size::Compact)
                    .ellipsis(),
            )
            .child(
                div()
                    .flex_grow()
                    .min_w_0()
                    .text_color(rgb(theme::text_faint()))
                    .text_size(px(12.))
                    .child(described),
            )
            .child(
                div()
                    .id(("open-output", key))
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_size(px(12.))
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                    .child("Open ⧉")
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&opened) {
                            tracing::warn!(%error, "could not open an output");
                        }
                    }),
            )
            .children(revealed.map(|folder| {
                div()
                    .id(("reveal-output", key))
                    .flex_none()
                    .text_color(rgb(theme::text_muted()))
                    .text_size(px(12.))
                    .hover(|style| style.text_color(rgb(theme::accent())).cursor_pointer())
                    .child("Reveal")
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&folder) {
                            tracing::warn!(%error, "could not open an output's folder");
                        }
                    })
            }));

        let mut card = div()
            .id(("output-card", key))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(if theme::is_light(&theme::current()) {
                theme::elevated()
            } else {
                theme::surface()
            }))
            .hover(|style| style.border_color(rgb(theme::border_strong())).cursor_pointer())
            .child(header);

        match output.kind {
            workspace::Kind::Figure => {
                card = card.child(
                    div().p_2().child(
                        // Capped height so a large figure can't push the transcript width around.
                        img(output.path.clone())
                            .max_w_full()
                            .max_h(px(420.))
                            .object_fit(gpui::ObjectFit::Contain),
                    ),
                );
            }
            _ => {
                if let Some(rows) = self.preview_of(output, PREVIEW_ROWS) {
                    let columns = rows
                        .iter()
                        .map(Vec::len)
                        .max()
                        .unwrap_or(0)
                        .min(PREVIEW_COLUMNS);
                    let mut table = div().flex().flex_col().w_full().min_w_0().px_3().py_2();
                    for (at, record) in rows.iter().enumerate() {
                        let heading = at == 0;
                        let mut line = div()
                            .flex()
                            .flex_row()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .py_1()
                            .when(heading, |line| {
                                line.border_b_1().border_color(rgb(theme::border()))
                            });
                        for cell in record.iter().take(columns) {
                            line = line.child(
                                div()
                                    .flex_grow()
                                    .flex_basis(relative(1. / columns as f32))
                                    .min_w_0()
                                    .text_color(rgb(if heading {
                                        theme::text_faint()
                                    } else {
                                        theme::text_muted()
                                    }))
                                    .text_size(px(12.))
                                    .overflow_hidden()
                                    .child(cell.clone()),
                            );
                        }
                        // Say the count rather than silently dropping the extra columns.
                        if record.len() > columns {
                            line = line.child(
                                div()
                                    .flex_none()
                                    .text_color(rgb(theme::text_faint()))
                                    .text_size(px(12.))
                                    .child(format!("+{}", record.len() - columns)),
                            );
                        }
                        table = table.child(line);
                    }
                    card = card.child(table);

                    if let workspace::Shape::Table { rows: total, .. } = shape {
                        card = card.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .px_3()
                                .pb_2()
                                .text_color(rgb(theme::text_faint()))
                                .text_size(px(12.))
                                .child(format!(
                                    "first {} of {} rows · click to open the whole table",
                                    rows.len().saturating_sub(1),
                                    workspace::thousands(total)
                                )),
                        );
                    }
                }
            }
        }

        card.on_click(cx.listener(move |workbench, _event, _window, cx| {
            workbench.preview = Preview::single(previewed.clone());
            cx.notify();
        }))
    }
    pub(crate) fn output_gallery_scroll(&self, key: &str) -> gpui::ScrollHandle {
        self.output_gallery_scrolls
            .borrow_mut()
            .entry(key.to_string())
            .or_default()
            .clone()
    }

    /// A capped grid of outputs, with the last visible tile counting the rest.
    pub(crate) fn output_grid(
        &self,
        scope: &str,
        heading: String,
        items: &[workspace::Output],
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tile = if compact {
            GRID_TILE_COMPACT
        } else {
            GRID_TILE_ROOMY
        };
        let (shown, hidden) = image_grid_shape(items.len());

        let mut grid = div().flex().flex_col().gap_2().flex_none();
        for row_start in (0..shown).step_by(GRID_COLUMNS) {
            let mut row = div().flex().flex_row().gap_2().flex_none();
            for at in row_start..(row_start + GRID_COLUMNS).min(shown) {
                // The overflow count rides on the last visible tile only.
                let more = (hidden > 0 && at + 1 == shown).then_some(hidden);
                row = row.child(self.output_grid_tile(
                    format!("output-tile-{scope}-{at}"),
                    items,
                    at,
                    tile,
                    more,
                    cx,
                ));
            }
            grid = grid.child(row);
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_none()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w(px(tile * GRID_COLUMNS as f32 + GRID_GAP))
                    .child(ui::Label::new(heading).size(ui::Size::Compact))
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::text_faint()))
                            .text_xs()
                            .child(if hidden > 0 {
                                "click to open all".to_string()
                            } else {
                                "click to open".to_string()
                            }),
                    ),
            )
            .child(grid)
    }
    /// One tile: a picture for a figure, a glyph and a name for anything else.
    pub(crate) fn output_grid_tile(
        &self,
        id: String,
        set: &[workspace::Output],
        at: usize,
        tile: f32,
        more: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let output = &set[at];
        let opening = set.to_vec();
        let media = tile * GRID_TILE_ASPECT;
        let (glyph, ink) = file_mark(&output.path);
        let shape = self.shape_of(output);
        let is_image = output.kind == workspace::Kind::Figure;

        let inside = if is_image {
            div()
                .relative()
                .w_full()
                .h(px(media))
                .flex_none()
                .child(
                    img(output.path.clone())
                        .w_full()
                        .h_full()
                        // `Contain`, not `Cover`: cropping the axes off a plot makes it unreadable.
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .when_some(more, |media, more| {
                    media.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(gpui::rgba(0x000000a6))
                            .text_color(rgb(SCRIM_INK))
                            .text_size(px(media_scrim_size(tile)))
                            .child(format!("+{more}")),
                    )
                })
                .into_any_element()
        } else {
            div()
                .relative()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .w_full()
                .h(px(media))
                .flex_none()
                .px_2()
                .child(
                    app_icon_at(glyph, ink, 24.),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text()))
                        .text_xs()
                        .child(distinguishing_tail(
                            &output_filename(output),
                            name_chars(tile),
                        )),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(11.))
                        .child(shape.describe(output.bytes)),
                )
                .when_some(more, |media, more| {
                    media.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(gpui::rgba(0x000000a6))
                            .text_color(rgb(SCRIM_INK))
                            .text_size(px(media_scrim_size(tile)))
                            .child(format!("+{more}")),
                    )
                })
                .into_any_element()
        };

        div()
            .id(SharedString::from(id))
            .flex()
            .flex_col()
            .flex_none()
            .w(px(tile))
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(if theme::is_light(&theme::current()) {
                theme::elevated()
            } else {
                theme::surface()
            }))
            .hover(|style| style.border_color(rgb(theme::accent())).cursor_pointer())
            .child(inside)
            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                workbench.preview = Preview::opening(opening.clone(), at);
                cx.notify();
            }))
    }
    /// One file on its own row. `by` names the worker that produced it, when one did.
    pub(crate) fn output_panel_row(
        &self,
        id: String,
        output: &workspace::Output,
        by: Option<&str>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let shown = output.clone();
        let shape = self.shape_of(output).describe(output.bytes);
        let shape = match by {
            Some(who) => format!("from {who} · {shape}"),
            None => shape,
        };
        let (glyph, ink) = file_mark(&output.path);
        div()
            .id(SharedString::from(id))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .p_2()
            .rounded_lg()
            .bg(rgb(theme::elevated()))
            .hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer())
            .child(
                app_icon_at(glyph, ink, 15.),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .min_w_0()
                    .child(
                        ui::Label::new(output_filename(output))
                            .size(ui::Size::Compact)
                            .ellipsis(),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .text_size(px(11.))
                            .child(shape),
                    ),
            )
            .on_click(cx.listener(move |workbench, _event, _window, cx| {
                workbench.preview = Preview::single(shown.clone());
                cx.notify();
            }))
    }
    // ---- project panel ----

    /// The panel's card: mission, plan, jobs, outputs, with a scrollbar beside the contents.
    pub(crate) fn artifacts_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .w(px(self.panel_width))
            .flex_none()
            .h_full()
            .m_2()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .group(SCROLL_GROUP)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_start()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .child(
                        ui::IconButton::new("toggle-right-panel", "icons/sidebar-simple-right.svg")
                            .icon_size(ui::IconSize::Small.px())
                            .ink(theme::text())
                            .on_click(cx.listener(|workbench, _event, _window, cx| {
                                workbench.panel_open = false;
                                workbench.remember_panels();
                                cx.notify();
                            })),
                    ),
            )
            .child(self.artifacts_contents(cx))
            .children(scrollbar(&self.panel_scroll))
    }
    /// The mission, and the way to change it in place.
    pub(crate) fn mission_block(&self, mission: &str, cx: &mut Context<Self>) -> Div {
        let block = div().flex().flex_col().w_full().min_w_0().gap_1();

        if self.editing_mission {
            return block
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(theme::accent()))
                        .child(self.mission_editor.clone()),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(
                            "Enter to save · Esc to cancel. Mini-Me reads this on every turn.",
                        ),
                );
        }

        block.child(
            div()
                .id("mission")
                .w_full()
                .min_w_0()
                .px_2()
                .py_1()
                .rounded_md()
                .hover(|style| {
                    style
                        .bg(rgb(theme::hover_over(theme::surface())))
                        .cursor_pointer()
                })
                .on_click(cx.listener(|workbench, _event, window, cx| {
                    workbench.start_mission_edit(window, cx)
                }))
                .when(mission.is_empty(), |empty| {
                    empty
                        .text_color(rgb(theme::text_muted()))
                        .text_sm()
                        .child("No mission yet — press to write one, or it comes from your first question.")
                })
                .when(!mission.is_empty(), |set| {
                    set.text_color(rgb(theme::text())).child(mission.to_string())
                }),
        )
    }
    pub(crate) fn artifacts_contents(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div()
            .id("spine")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_grow()
            .overflow_y_scroll()
            .track_scroll(&self.panel_scroll)
            .p_4()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .min_w_0()
                    .child(section_label("MISSION"))
                    .when(!self.editing_mission, |heading| {
                        heading.child(
                            div()
                                .id("edit-mission")
                                .px_1()
                                .rounded_sm()
                                .text_xs()
                                .text_color(rgb(theme::accent()))
                                .hover(|style| {
                                    style
                                        .bg(rgb(theme::hover_over(theme::surface())))
                                        .cursor_pointer()
                                })
                                .child("Edit")
                                .on_click(cx.listener(|workbench, _event, window, cx| {
                                    workbench.start_mission_edit(window, cx)
                                })),
                        )
                    }),
            );

        let mission = self
            .project
            .as_ref()
            .map(|project| project.mission.clone())
            .unwrap_or_default();
        panel = panel.child(self.mission_block(&mission, cx));

        let Some(project) = &self.project else {
            // No spine yet, but a run may already be producing outputs — show those.
            return panel
                .child(self.plan_section(cx))
                .child(self.jobs_section(cx))
                .child(self.outputs_section(cx))
                .child(self.sources_section(Some(SOURCES_IN_PANEL), cx));
        };

        if !project.completed.is_empty() {
            panel = panel.child(spine_list("COMPLETED", &project.completed, "✓"));
        }
        if !project.pending.is_empty() {
            panel = panel.child(spine_list("PENDING", &project.pending, "○"));
        }

        // Advisory only: loads into the composer, never auto-runs.
        if !project.suggestions.is_empty() {
            let mut suggestions = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("SUGGESTED NEXT"));
            for (index, suggestion) in project.suggestions.iter().enumerate() {
                let prompt = suggestion.prompt.clone();
                suggestions = suggestions.child(
                    div()
                        .id(("suggestion", index))
                        .flex()
                        .flex_col()
                        .w_full()
                        .min_w_0()
                        .gap_1()
                        .p_2()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .hover(|style| style.border_color(rgb(theme::accent())).cursor_pointer())
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(theme::text()))
                                .text_sm()
                                .child(suggestion.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_color(rgb(theme::text_muted()))
                                .text_xs()
                                .child(suggestion.rationale.clone()),
                        )
                        // Loads the prompt into the composer; never runs it directly.
                        .on_click(cx.listener(move |workbench, _event, window, cx| {
                            if workbench.streaming || prompt.is_empty() {
                                return;
                            }
                            workbench.composer.update(cx, |composer, cx| {
                                composer.set_text(prompt.clone(), cx);
                            });
                            // Drop it from the list now that it is in the composer.
                            if let Some(project) = workbench.project.as_mut() {
                                project.suggestions.retain(|s| s.prompt != prompt);
                            }
                            let focus = workbench.composer.focus_handle(cx);
                            window.focus(&focus);
                            workbench.status = "suggestion loaded — press Enter to run it".into();
                            cx.notify();
                        })),
                );
            }
            panel = panel.child(suggestions);
        }

        if project.completed.is_empty() && project.pending.is_empty() {
            panel = panel.child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Completed and pending work will appear here as the project grows."),
            );
        }

        panel
            .child(self.plan_section(cx))
            .child(self.jobs_section(cx))
            .child(self.outputs_section(cx))
            .child(self.sources_section(Some(SOURCES_IN_PANEL), cx))
    }
    /// An agent's own plan, as a checklist with the step it is on marked. Nothing is derived
    /// or estimated — no percentage, no bar — only what the agent itself wrote down. `busy` is
    /// what the running step is doing right now. Empty plan, empty element.
    pub(crate) fn plan_list(&self, todos: &[protocol::Todo], busy: Option<&str>) -> Div {
        let mut list = div().flex().flex_col().w_full().min_w_0().gap_1();
        if todos.is_empty() {
            return list;
        }
        for (at, todo) in todos.iter().enumerate() {
            let ink = if todo.is_done() {
                theme::text_muted()
            } else if todo.is_running() {
                theme::text()
            } else {
                theme::text_faint()
            };
            let mut row = div()
                .id(("plan-step", at))
                .flex()
                .flex_row()
                .items_start()
                .w_full()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(if todo.is_running() {
                            theme::accent()
                        } else if todo.is_done() {
                            theme::success()
                        } else {
                            theme::text_faint()
                        }))
                        .child(todo.mark()),
                )
                .child(
                    div()
                        .flex_grow()
                        .min_w_0()
                        .text_xs()
                        .text_color(rgb(ink))
                        .child(todo.content.clone()),
                );
            if todo.is_running() {
                if let Some(busy) = busy {
                    row = row.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(theme::text_faint()))
                            .child(busy.to_string()),
                    );
                }
            }
            list = list.child(row);
        }
        list
    }
    /// The coordinator's working plan for this conversation, when it wrote one. Kept after the
    /// turn ends, since a finished plan is the record of what the answer involved.
    pub(crate) fn plan_section(&self, _cx: &mut Context<Self>) -> Div {
        let mut section = div().flex().flex_col().gap_2().w_full().min_w_0();
        let Some((done, total)) = protocol::plan_progress(&self.plan) else {
            return section;
        };
        section = section
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(section_label("PLAN"))
                    .child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::text_faint()))
                            .text_xs()
                            .child(format!("{done} of {total}")),
                    ),
            )
            .child(self.plan_list(&self.plan, None));
        section
    }

    /// Long jobs still running, plus the ones that finished this session.
    pub(crate) fn jobs_section(&self, cx: &mut Context<Self>) -> Div {
        let mut section = div().flex().flex_col().gap_2().pt_2();
        if self.jobs.is_empty() && self.tasks.is_empty() {
            return section;
        }

        let tally = JobTally::of(&self.tasks, &self.jobs);
        section = section.child(
            div()
                .id("jobs-heading")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .min_w_0()
                .gap_2()
                .py_1()
                .rounded_md()
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style
                        .bg(rgb(fill))
                        .text_color(rgb(theme::ink_on(fill)))
                        .cursor_pointer()
                })
                .text_color(rgb(theme::text_faint()))
                .text_xs()
                .child(format!(
                    "{} BACKGROUND JOBS",
                    if self.jobs_expanded { "▾" } else { "▸" }
                ))
                .child(
                    // Named states rather than a total, so folded still says what needs action.
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(tally.colour()))
                        .child(tally.summary()),
                )
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.jobs_expanded = !workbench.jobs_expanded;
                    cx.notify();
                })),
        );
        if !self.jobs_expanded {
            return section;
        }

        // Background workers waiting for approval are shown first.
        let (waiting, working): (Vec<_>, Vec<_>) =
            self.tasks.iter().partition(|task| task.needs_approval());
        for task in waiting {
            section = section.child(self.task_row(task, cx));
        }

        let mut body = div()
            .id("jobs-body")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_2()
            // Room for the thumb painted over this by the wrapper below.
            .pr(px(SCROLL_GUTTER))
            .max_h(px(JOBS_BODY_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.jobs_scroll)
            // Stop the wheel event here so it doesn't also scroll the panel underneath.
            .when(self.jobs_scroll.max_offset().height > px(0.), |body| {
                body.on_scroll_wheel(|_event, _window, cx| cx.stop_propagation())
            });
        for task in working {
            body = body.child(self.task_row(task, cx));
        }
        for job in &self.jobs {
            body = body.child(self.job_row(job, cx));
        }

        section.child(
            // The bar sits outside the element it measures, so it doesn't scroll with it.
            div()
                .relative()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .child(body)
                .children(scrollbar(&self.jobs_scroll)),
        )
    }
    /// One background worker: what it is, what it is doing, its plan, its gate, its files.
    pub(crate) fn task_row(&self, task: &protocol::AsyncTask, cx: &mut Context<Self>) -> Div {
        let (mark, colour) = if task.needs_approval() {
            ("⏸", theme::accent())
        } else if !task.is_finished() {
            ("◐", theme::running())
        } else if task.succeeded() {
            ("✓", theme::success())
        } else {
            ("✗", theme::error())
        };
        let mut row = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_none()
            .gap_1()
            .pl_2()
            .border_l_1()
            .border_color(rgb(colour))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(theme::text()))
                            .text_sm()
                            .child(format!("{mark} {}", task.agent_name.replace('_', " "))),
                    )
                    .children(protocol::plan_progress(&task.todos).map(|(done, total)| {
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(theme::text_faint()))
                            .child(format!("{done} of {total}"))
                    })),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(if task.error.is_some() {
                        theme::error()
                    } else if task.needs_approval() {
                        theme::warning()
                    } else {
                        theme::text_muted()
                    }))
                    .text_xs()
                    .child(match (&task.error, task.needs_approval()) {
                        (Some(error), _) => error.clone(),
                        (None, true) => "waiting for your approval".to_string(),
                        (None, false) => match (&task.activity, task.is_finished()) {
                            (Some(activity), false) => format!("{} · {activity}", task.status),
                            _ => task.status.clone(),
                        },
                    }),
            )
            .when(!task.description.is_empty(), |row| {
                row.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(task.description.clone()),
                )
            })
            .when(!task.todos.is_empty(), |row| {
                row.child(self.plan_list(&task.todos, task.activity.as_deref()))
            });

        if let Some(request) = &task.pending {
            let task_id = task.task_id.clone();
            // Capped and scrollable so a long command can't push Approve off-screen.
            let mut commands = div()
                .id(SharedString::from(format!("bg-commands-{task_id}")))
                .flex()
                .flex_col()
                .gap_1()
                .w_full()
                .min_w_0()
                .max_h(px(200.))
                .overflow_y_scroll();
            for action in &request.actions {
                commands = commands.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_none()
                        .p_2()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .text_color(rgb(theme::text()))
                        .text_xs()
                        .child(action.detail.clone()),
                );
            }
            row = row.child(commands);
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        ui::Button::new(
                            SharedString::from(format!("bg-approve-{task_id}")),
                            "Approve",
                        )
                        .tone(ui::Tone::Accent)
                        .on_click(cx.listener({
                            let task_id = task_id.clone();
                            move |workbench, _event, _window, cx| {
                                workbench.decide_task(task_id.clone(), true, cx);
                            }
                        })),
                    )
                    .child(
                        ui::Button::new(
                            SharedString::from(format!("bg-reject-{task_id}")),
                            "Reject",
                        )
                        .on_click(cx.listener({
                            let task_id = task_id.clone();
                            move |workbench, _event, _window, cx| {
                                workbench.decide_task(task_id.clone(), false, cx);
                            }
                        })),
                    ),
            );
            // Blanket grants so the researcher isn't asked once per command.
            for (suffix, label, conversation_wide) in [
                ("task", "Approve the rest of this task", false),
                ("conv", "Approve everything in this conversation", true),
            ] {
                row = row.child(
                    ui::Button::new(
                        SharedString::from(format!("bg-approve-{suffix}-{task_id}")),
                        label,
                    )
                    .size(ui::Size::Compact)
                    .on_click(cx.listener({
                        let task_id = task_id.clone();
                        move |workbench, _event, _window, cx| {
                            if conversation_wide {
                                workbench.approve_conversation = true;
                            } else {
                                workbench.approve_tasks.insert(task_id.clone());
                            }
                            workbench.decide_task(task_id.clone(), true, cx);
                        }
                    })),
                );
            }
        }

        // Opens the folder directly rather than composing a turn to ask for it.
        if task.succeeded() {
            if let Some(dir) = self
                .thread_workspace()
                .map(|conversation| workspace::worker_dir(&conversation, &task.thread_id))
            {
                row = row.child(
                    div().mt_1().child(
                        // Names the specialist since several may run at once.
                        ui::Chip::new(
                            SharedString::from(format!("task-files-{}", task.task_id)),
                            format!("Show what {} produced", task.agent_name.replace('_', " ")),
                        )
                        .on_click(move |_event, _window, _cx| {
                            if let Err(error) = workspace::open(&dir) {
                                tracing::warn!(%error, "could not open a worker's folder");
                            }
                        }),
                    ),
                );
            }
        }
        row
    }
    /// One long-running job: the theorizer, or a DataVoyager analysis. Polled only, no controls.
    pub(crate) fn job_row(&self, job: &protocol::Job, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let (mark, colour) = if !job.is_finished() {
            ("◐", theme::running())
        } else if job.succeeded() {
            ("✓", theme::success())
        } else {
            ("✗", theme::error())
        };
        let detail = if job.is_finished() {
            job.status.clone()
        } else {
            // A bare spinner reads as a hang; say the expected duration instead.
            format!("running · usually {}", job.kind.expected(job.size))
        };
        // The one job row with something to open: a finished discovery run.
        let readable = job.kind == protocol::JobKind::Discovery && job.succeeded();
        let mut row = div()
            .id(SharedString::from(format!("job-row-{}", job.task_id)))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_none()
            .gap_1()
            .pl_2()
            .border_l_1()
            .border_color(rgb(colour))
            .child(
                div()
                    .text_color(rgb(theme::text()))
                    .text_sm()
                    .child(format!("{mark} {}", job.kind.label())),
            )
            .child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(detail),
            )
            .when(!job.question.is_empty(), |row| {
                row.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        // Clipped: enough to tell concurrent analyses apart, not the full question.
                        .child(protocol::clip(&job.question, JOB_QUESTION_CHARS)),
                )
            });

        if readable {
            let run_id = job.task_id.clone();
            let name = job.question.clone();
            row = row
                // Said as well as coloured; this is the only way into the run.
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::accent()))
                        .text_xs()
                        .child("Show what it found"),
                )
                .rounded_md()
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style.bg(rgb(fill)).cursor_pointer()
                })
                .on_click(cx.listener(move |workbench, _event, _window, cx| {
                    workbench.open_discovery(run_id.clone(), name.clone(), cx);
                }));
        }
        row
    }
    /// One line for what this conversation ran, shown only when something did.
    pub(crate) fn commands_line(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let commands = self.thread_commands();
        if commands.is_empty() {
            return None;
        }
        let (summary, escaped) = commands_summary(&commands);
        let tone = if escaped {
            theme::accent()
        } else {
            theme::text_muted()
        };

        Some(
            div()
                .id("what-ran")
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .p_1()
                .rounded_md()
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style.bg(rgb(fill)).cursor_pointer()
                })
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.commands_open = true;
                    cx.notify();
                }))
                .child(section_label("WHAT RAN"))
                .child(
                    ui::Label::new(summary)
                        .colour(tone)
                        .size(ui::Size::Compact),
                )
                .into_any_element(),
        )
    }
    /// What's actually on disk for this conversation, grouped by producer and folder.
    pub(crate) fn outputs_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let listing = self
            .thread_workspace()
            .map(|dir| workspace::output_listing(&dir));
        let files = listing
            .as_ref()
            .map(|listing| listing.groups.as_slice())
            .unwrap_or_default();
        let count: usize = files.iter().map(|(_, items)| items.len()).sum();
        // Restore file order before regrouping by parent folder; metadata only, no file reads.
        let ordered_outputs: Vec<workspace::Output> = files
            .iter()
            .flat_map(|(_, items)| items.iter().cloned())
            .collect();

        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(rgb(theme::border()));

        // Computed before the empty-section guard below, since count==0 is exactly when it matters.
        let ran = self.commands_line(cx);

        if outputs_are_empty(count, self.buckets.len(), usize::from(ran.is_some())) {
            return section;
        }

        section = section.children(ran);

        if count > 0 {
            section = section.child(section_label_owned(format!("FILES · {count}")));
        }

        if listing.as_ref().is_some_and(|listing| listing.truncated) {
            // The scan is bounded (a workspace may contain a virtualenv); say when it truncates.
            section = section.child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Showing a bounded view. Open the folder to see the rest."),
            );
        }

        // Images first, grouped by producer; a CSV is opened to check something, a deliberate act.
        for (band, (worker, produced)) in by_producer(&ordered_outputs, &self.tasks, &self.authorship)
            .into_iter()
            .enumerate()
        {
            let (images, others) = split_images(&produced);
            if !images.is_empty() {
                section = section.child(self.output_grid(
                    &format!("panel-{band}"),
                    images_heading(images.len(), worker.as_deref()),
                    &images,
                    true,
                    cx,
                ));
            }
            for (at, group) in output_folder_groups(&others).iter().enumerate() {
                if let [output] = group.outputs.as_slice() {
                    // A lone file stays a row rather than a one-tile grid.
                    section = section.child(self.output_panel_row(
                        format!("panel-output-{}", output.name),
                        output,
                        worker.as_deref(),
                        cx,
                    ));
                } else {
                    // Still folder-grouped; only the image grid above groups by kind instead.
                    section = section.child(self.output_grid(
                        &format!("panel-{band}-{at}"),
                        shorten_path_label(
                            &output_folder_label(&group.folder, worker.as_deref()),
                            PANEL_HEADING_CHARS,
                        ),
                        &group.outputs.iter().map(|o| (*o).clone()).collect::<Vec<_>>(),
                        true,
                        cx,
                    ));
                }
            }
        }

        // A way out of the panel to the whole folder, beyond the scan's bounds; dashed and last.
        if let Some(dir) = self.thread_workspace() {
            section = section.child(
                div()
                    .id("open-workspace")
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(theme::border_strong()))
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .hover(|style| {
                        style
                            .text_color(rgb(theme::accent()))
                            .border_color(rgb(theme::accent()))
                            .cursor_pointer()
                    })
                    .child(if cfg!(windows) {
                        "Open the folder in Explorer"
                    } else {
                        "Open the folder"
                    })
                    .on_click(move |_event, _window, _cx| {
                        if let Err(error) = workspace::open(&dir) {
                            tracing::warn!(%error, "could not open the workspace folder");
                        }
                    }),
            );
        }

        for bucket in &self.buckets {
            // Bounded titles shown; the count already conveys the scale.
            const MAX_SHOWN: usize = 4;
            // Only when the structured list arrived; an older backend still renders as plain text.
            let openable = matches!(bucket.name, "datasets" | "libraries")
                && !bucket.items.is_empty();
            // Count from the structured lists, not the raw bucket length.
            let (label, count, rows) = match bucket.name {
                "libraries" if !self.documents.is_empty() => (
                    "library",
                    self.documents.len(),
                    self.documents
                        .iter()
                        .take(MAX_SHOWN)
                        .map(|document| document.title.clone())
                        .collect::<Vec<String>>(),
                ),
                "datasets" if !self.datasets.is_empty() => (
                    bucket.name,
                    self.datasets.len(),
                    self.datasets
                        .iter()
                        .take(MAX_SHOWN)
                        .map(|dataset| {
                            dataset
                                .persistent_id
                                .strip_prefix("doi:")
                                .unwrap_or(&dataset.persistent_id)
                                .to_string()
                        })
                        .collect(),
                ),
                _ => (
                    bucket.name,
                    bucket.items.len(),
                    bucket.items.iter().take(MAX_SHOWN).cloned().collect(),
                ),
            };
            let mut heading = div()
                // Per-bucket id, since a shared id across sibling elements breaks gpui's click routing.
                .id(SharedString::from(format!("bucket-{}", bucket.name)))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                // Full-width hover target rather than a bare text strip.
                .w_full()
                .min_w_0()
                .px_2()
                .py_1()
                .rounded_md()
                .text_color(rgb(theme::text()))
                .text_sm()
                .child(format!("{label} · {count}"));
            if openable {
                heading = heading
                    // Said as well as coloured; this is the only way into the dataset list.
                    .child(
                        ui::Label::new("open all")
                            .inherit()
                            .size(ui::Size::Compact),
                    )
                    .hover(|style| {
                        let fill = theme::hover_over(theme::surface());
                        style
                            .bg(rgb(fill))
                            .text_color(rgb(theme::ink_on(fill)))
                            .cursor_pointer()
                    })
                    .on_click({
                        let which = bucket.name;
                        cx.listener(move |workbench, _event, _window, cx| match which {
                            "libraries" => {
                                workbench.documents_open = true;
                                cx.notify();
                            }
                            _ => workbench.open_datasets(cx),
                        })
                    });
            }
            let mut group = div().flex().flex_col().gap_1().child(heading);
            for item in rows {
                group = group.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(item),
                );
            }
            if count > MAX_SHOWN {
                group = group.child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(format!("+{} more", count - MAX_SHOWN)),
                );
            }
            section = section.child(group);
        }

        section
    }
}

