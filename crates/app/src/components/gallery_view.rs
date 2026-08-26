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


/// How many characters of a filename fit across a tile at `text_xs`.
///
/// Measured rather than truncated by the layout, for the reason [`Workbench::output_grid_tile`]
/// gives: `Label::ellipsis` collapses to a bare `…` without a flex parent to grow within (§59).
/// Roughly 6px per character at 12px type, less the tile's own padding.
pub(crate) fn name_chars(tile: f32) -> usize {
    (((tile - 16.) / 6.) as usize).max(8)
}


/// How many tiles the grid draws, and how many images the last one stands in for.
///
/// **The scrimmed tile counts among the hidden**, because it is covered: eight images in four
/// tiles reads `+5` — three pictures you can see, five you cannot — which is what the phone
/// gallery the researcher pointed at shows for the same eight. `total - tiles` gives `+4` and
/// looks perfectly reasonable in review; it is only wrong beside the thing it is imitating. One
/// function so the grid and its test cannot hold two versions of the rule.
pub(crate) fn image_grid_shape(total: usize) -> (usize, usize) {
    let shown = total.min(IMAGE_GRID_TILES);
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


/// Name the folder the agent chose, not the generated background-thread directory above it.
///
/// The screenshot in §152 devoted its useful width to a 36-character UUID common to every row.
/// That component is app bookkeeping; removing only a leading UUID leaves `eda/plots`, the
/// researcher's information, while the unshortened path remains the grouping identity above.
///
/// `worker` is whoever produced the files, when the app knows — from the folder for a background
/// worker (§199), from the backend's own record for a specialist (§201). It takes the leading
/// position either way: the UUID's, when there was one, so nothing is lost by removing it; and
/// otherwise ahead of the folder the agent chose. Either way the heading reads as a path of work
/// — `background worker / plots`, `exploratory data analysis / plots`. `None` keeps §152's
/// behaviour, which is what a conversation with no record still gets.
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


/// The worker thread a file sits under, when it sits under one.
///
/// The **only** attribution this client can make without guessing. A background worker runs on
/// its own thread and writes into a folder named after it, so the folder *is* the record of who
/// produced the file. Specialists consulted inside the conversation share the conversation's
/// thread and its one directory, and nothing on the wire says which of them wrote a given file —
/// so nothing here claims to know. That restraint is `provenance.rs`'s own rule from §73: a
/// provenance record that quietly guesses is worse than none, because it will be believed.
pub(crate) fn producing_thread(output: &workspace::Output) -> Option<&str> {
    let first = std::path::Path::new(&output.name).components().next()?;
    let name = first.as_os_str().to_str()?;
    if workspace::looks_like_thread_id(name) {
        Some(name)
    } else {
        None
    }
}


/// Outputs split by who produced them: the conversation's own first, then one group per
/// other author, in the order their first file appears.
///
/// **Ahead of the image/other split, not after it.** §152 put every image in one grid because
/// images are what a person opens the panel to look at. That was right within one body of work
/// and wrong across two: a researcher looking at *"15 images"* was looking at the conversation's
/// plots and a worker's plots in one tray, with nothing saying where the boundary was (§199). A
/// background worker is already a separate run with its own job row and its own folder; its
/// figures are a separate body of work for the same reason.
///
/// Two sources of truth, in this order, each exact within its own domain (§201):
///
/// 1. **The folder**, for a background worker — its own thread, its own directory, true by
///    construction and true even for a conversation reopened years later.
/// 2. **The manifest**, for everything else — what `overlay/minime_local/authorship.py` wrote
///    down as each file was produced.
///
/// The folder wins where both speak, because inside a worker's run the manifest records that
/// worker's *own* coordinator and would rename `background worker` to `coordinator` — technically
/// true of the inner graph and useless to the person reading the panel.
/// A file that records what a search returned, rather than a result of the research.
///
/// Kept out of the transcript's file cards only. Both are real outputs a researcher may want —
/// they are just not things to read mid-conversation.
pub(crate) fn is_search_record(output: &workspace::Output) -> bool {
    matches!(
        output.path.file_name().and_then(|name| name.to_str()),
        Some("papers.json") | Some("dataverse_search.json")
    )
}


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
    // The conversation's own files lead even when someone else wrote first: they are what the
    // researcher asked for directly, and a delegation is the detour under it.
    groups.sort_by_key(|(owner, _)| owner.is_some());
    groups
}


/// Who produced a group of files, in the researcher's words rather than the engine's.
///
/// `None` is the conversation's own thread, and stays unlabelled: those files are the unmarked
/// case, and spending a heading on *"from this conversation"* would name the default everywhere
/// to say something only where it is not true.
///
/// A thread with no matching task is still *some* worker — the folder proves it — so it says so
/// without naming one. That is the state after a reload whose snapshot carried no `async_tasks`,
/// and it is the difference between "we don't know which" and "nobody".
pub(crate) fn produced_by(thread: Option<&str>, tasks: &[protocol::AsyncTask]) -> Option<String> {
    let thread = thread?;
    Some(
        tasks
            .iter()
            .find(|task| task.thread_id == thread)
            // Underscores are the graph's spelling of a name, not a person's — the road strip and
            // the jobs list both already say `background worker`, and a third spelling of the
            // same specialist in a third panel is how one worker reads as two.
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


/// Keep the distinguishing tail when a filename itself is too long for a thumbnail.
///
/// `Label::ellipsis()` correctly protects layout (§59), but its trailing ellipsis preserves the
/// shared prefix and removes the useful suffix in §152. Shortening the string from the leading
/// edge before layout means the extension and differentiating part survive even in a 140px tile.
pub(crate) fn distinguishing_tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars == 0 {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("…{}", text.chars().skip(count - keep).collect::<String>())
}


/// Shorten an `a / b / c` heading to fit, giving up the middle rather than either end.
///
/// **[`distinguishing_tail`] keeps the wrong end for these.** §152 chose tail-keeping because its
/// labels shared a long *prefix* and differed at the end — the right rule for a filename. §201 then
/// put the producing worker's name at the *head*, which inverts it: `background worker / outputs /
/// tables` came out as `…d worker / outputs / tables`, throwing away the one word the attribution
/// exists to show. Spotted in a screenshot of the feature working (§208).
///
/// So both ends survive and the middle gives way. If it still will not fit, the **head** is kept
/// whole and the tail is trimmed — the producer outranks the leaf folder, because a heading that
/// cannot say who made these files is the heading §201 replaced.
pub(crate) fn shorten_path_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        return label.to_string();
    }
    let segments: Vec<&str> = label.split(" / ").collect();
    let (Some(head), Some(tail)) = (segments.first(), segments.last()) else {
        return distinguishing_tail(label, max_chars);
    };
    if segments.len() < 2 {
        // One segment: no middle to drop, so §152's rule is still the best available.
        return distinguishing_tail(label, max_chars);
    }
    let spacer = if segments.len() > 2 { " / … / " } else { " / " };
    let joined = format!("{head}{spacer}{tail}");
    if joined.chars().count() <= max_chars {
        return joined;
    }
    let room = max_chars.saturating_sub(head.chars().count() + spacer.chars().count());
    // Below four characters a trimmed tail is all ellipsis and no information; better to fall back
    // than to print `… / … / …s`.
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
    ///
    /// `at` and `count` come from the [`Preview`] rather than being recomputed, so the arrows, the
    /// `3 / 8` counter and the highlighted filmstrip tile can never disagree about which file is
    /// showing — the §158 rule about one calculation, applied to three affordances that all mean
    /// "this one".
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
                // **A box with both dimensions set, and `Contain` inside it.** `max_w_full` alone
                // left the height to the natural size of the file, so a tall plot resolved larger
                // than the space the flex row gave it and was clipped at *both* ends — the top of
                // a stacked bar chart cut off, with dead space underneath. `Contain` in a bounded
                // box letterboxes instead, which is the one arrangement that cannot crop.
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
                // Read a bounded prefix. A 200 MB CSV would otherwise be pulled into
                // memory and laid out as one paragraph, on the UI thread.
                match workspace::head(&output.path, 400) {
                    Ok(text) if output.name.ends_with(".md") => {
                        for parsed in markdown::parse(&text) {
                            body = body.child(markdown_block(&parsed, None));
                        }
                    }
                    Ok(text) if is_delimited(&output.name) => {
                        // Rainbow columns, the trick the `rainbow-csv` editor extensions
                        // use: colour by column index so the eye can follow one field
                        // down the rows. Without column *layout* — which GPUI 0.2.2 does
                        // not have — colour is the only thing that makes a wide CSV
                        // readable at all (docs §50).
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

        // The body, flanked by the arrows, so a step is a click where the eye already is rather
        // than a trip to a toolbar. Only when there is somewhere to go: a lone file gets no
        // arrows at all, because a control that does nothing is worse than an absent one (§158).
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
            // **Painting over something is not the same as being in front of it.** Without this,
            // the workbench under the dim stayed live: a click landed on the modal *and* on
            // whatever happened to be beneath it, so opening a figure could also hit a button in
            // the transcript. `occlude` blocks the mouse from everything behind this hitbox, which
            // is what makes the dim mean what it looks like it means (docs §163).
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            // The dim is the affordance: it says the workbench is still there and that
            // clicking away comes back to it.
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
                    // Clicks inside the panel are the panel's business. Click handlers fire on
                    // the bubble phase — innermost first — so stopping here after a control has
                    // run is what keeps the backdrop's close-on-click from firing too. Without
                    // it every arrow press closed the modal it was trying to step through.
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
                // Clicking the dimmed backdrop closes it, the way every modal does.
                workbench.preview = None;
                cx.notify();
            }))
    }
}


impl Workbench {
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
}


impl Workbench {
    /// The set along the bottom of the modal: a counter, then a sideways strip to choose from.
    ///
    /// **This is the half the researcher asked for by name** — *"we can click and scroll at the
    /// bottom so the user can select which picture to see."* `None` for a lone file: a filmstrip
    /// of one is a decoration that implies there is somewhere to go.
    ///
    /// Tile ids carry the file's own index, so GPUI keeps each element's identity as the selection
    /// moves and the strip does not lose its scroll position on every step.
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
                    // The outline is the whole selection signal, so it is two pixels of accent
                    // against one of border: a one-pixel difference in colour alone did not read
                    // at thumbnail size on the Windows pass (§158's sibling complaint).
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
                    // A non-image in the set still needs a tile, or the counter and the strip
                    // disagree about how many there are.
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
}


impl Workbench {
    /// A file a turn produced, in the transcript, under the answer that produced it.
    ///
    /// **Why here and not only in the panel.** A produced file used to appear as a name and a
    /// size in a 330px column on the far side of the window. So the answer would say "I cleaned
    /// the dataset and removed 14 duplicate plots", and whether that dataset now had 1,204 rows
    /// or 40 was a separate trip to a separate place — which is exactly the check a researcher
    /// should be able to make without leaving the sentence that prompted it.
    ///
    /// A table shows its first rows, a figure shows itself, anything else shows its header. All
    /// three open the existing preview modal on click.
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
            // One step off the transcript's own background, whichever way the palette runs.
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
                        // Capped, not scaled to the pane: a 2000px figure would otherwise push
                        // the transcript's width around as it loads.
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
                        // Said, not silently dropped: a table shown four columns wide when it
                        // has eleven is a table someone will read as complete.
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
}


impl Workbench {
    pub(crate) fn output_gallery_scroll(&self, key: &str) -> gpui::ScrollHandle {
        self.output_gallery_scrolls
            .borrow_mut()
            .entry(key.to_string())
            .or_default()
            .clone()
    }
}


impl Workbench {
    /// A visible, clickable and draggable horizontal scrollbar for one gallery rail.
    ///
    /// The first version only painted the thumb. That was enough to imply an interaction and
    /// then break it: a mouse-first Windows user naturally grabbed the bar shown on screen and
    /// nothing happened. The whole 12px track is now a hit target; clicking outside the thumb
    /// jumps toward that position and holding the mouse continues the drag (docs §158).
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
}


impl Workbench {
    /// A capped grid of outputs, with the last visible tile counting the rest.
    ///
    /// **One renderer for images and for files**, because the researcher asked for the same
    /// treatment on both and the difference is only what a tile draws inside itself. §153's
    /// sideways strip is gone: it spanned the whole transcript, one folder of seven files claimed
    /// a band of the conversation wider than the answer above it, and the phone gallery it was
    /// being compared against is a compact block you flick past. Their words: *"the grouping
    /// occupies too much space in the conversation (too wide) … less invasive and functions the
    /// same."*
    ///
    /// Fixed-width tiles rather than a fraction of the container, which is what makes it narrow:
    /// two per row means the block is exactly `2 × tile + gap` and stops there, whatever the panel
    /// or the window is doing.
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
                // The count rides on the *last visible* tile, and only when something is behind
                // it. Clicking it opens that file; the rest are then one arrow away, which is
                // what makes a capped grid honest rather than lossy.
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
}


impl Workbench {
    /// One tile: a picture for a figure, a glyph and a name for anything else.
    ///
    /// **No filename on an image tile.** The picture identifies itself, the modal's header names
    /// it, and a caption under every thumbnail was half of what made the old strip feel like
    /// furniture. A data file is the opposite case — one CSV looks exactly like another — so those
    /// tiles carry the name and the shape, which is the only thing that tells them apart.
    ///
    /// The name is shortened **here**, in Rust, rather than by asking the layout to truncate it.
    /// `Label::ellipsis` needs a flex parent to grow within (§59), and a tile is a column of
    /// fixed width — get that wrong and every name renders as a bare `…`, which is exactly what
    /// §153's tiles did in the panel.
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
                        // `Contain`, not `Cover`: a photo crops acceptably and a chart does not.
                        // Cropping the axes off a plot makes the thumbnail useless for choosing
                        // between seven of them, which is the only job it has.
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
}


impl Workbench {
    /// One file on its own row. `by` names the worker that produced it, when one did.
    ///
    /// A lone file gets no gallery heading to carry its attribution, so it carries it on the line
    /// that already describes the file — otherwise a worker that wrote exactly one report would
    /// be the one case §199 still left anonymous.
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
                    // The filename is the distinguishing tail. The parent folder has its own
                    // gallery heading, so repeating its UUID here recreates §152 exactly.
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
}


impl Workbench {
    /// The project spine: mission, what's done, what's queued, what's suggested.
    /// The panel's card, with the scrolling contents inside it and a bar beside them.
    ///
    /// Split from the contents because the scrollbar must sit *outside* the scrolling
    /// element — inside, it would scroll along with what it measures.
    pub(crate) fn artifacts_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_col()
            .w(px(self.panel_width))
            .flex_none()
            .h_full()
            .m_1()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .group(SCROLL_GROUP)
            .child(self.artifacts_contents(cx))
            .children(scrollbar(&self.panel_scroll))
    }
}


impl Workbench {
    /// The mission, and the way to change it.
    ///
    /// **It had never been changeable.** The mission is seeded server-side from the first human
    /// message of a project and then rendered here as plain text, so a researcher whose opening
    /// question was a warm-up — or whose project turned out to be about something else — had no
    /// way to say so: the panel showed a sentence they could not edit, and the coordinator was
    /// reading that same sentence into its system prompt on every turn
    /// (`backend/middleware/project.py`). Reported as *"I cannot modify the project mission"*
    /// (§199). The route to change it had existed the whole time and this client had never
    /// called it — see [`protocol::LangGraphClient::set_mission`].
    ///
    /// Editing happens in place, as renaming a conversation does, and for the same reason: the
    /// field replaces the text it is about, so the researcher is looking at what they are
    /// changing rather than at a copy of it in a dialog.
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
                        // What it costs to be wrong, said before the press rather than after:
                        // this sentence is read by the coordinator on every turn, so it is not a
                        // label on the work — it is an instruction to it.
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
}


impl Workbench {
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
            // `MISSION`, not `RESEARCH PROJECT`. The panel *is* the research project — saying so
            // at the top of it spends the widest heading in the column on a word that names the
            // container rather than its first section.
            //
            // The heading carries the edit control rather than the mission carrying a hover-only
            // one. Our researchers are not developers, and *"I cannot modify the project mission"*
            // was said about a panel where the text was in fact the button — an affordance that
            // only exists once the pointer is already on it cannot be the answer to someone who
            // has concluded there isn't one (§199).
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
        // The same block whether or not a spine has arrived: with no project there is nothing to
        // *read*, but there is still something to *write*, and a researcher who knows what this
        // project is for should be able to say so before the first question rather than having
        // one derived from it (§199).
        panel = panel.child(self.mission_block(&mission, cx));

        let Some(project) = &self.project else {
            // No spine yet, but a run may already be producing outputs — still show
            // them rather than an empty panel.
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

        // Advisory only: shown so the user can choose to ask for one. Nothing here
        // auto-runs — org policy is human-gated.
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
                        // Clicking *loads* the prompt into the composer; it never
                        // runs it. Suggestions are advisory and org policy is
                        // human-gated, so the user still presses Enter.
                        .on_click(cx.listener(move |workbench, _event, window, cx| {
                            if workbench.streaming || prompt.is_empty() {
                                return;
                            }
                            workbench.composer.update(cx, |composer, cx| {
                                composer.set_text(prompt.clone(), cx);
                            });
                            // Drop it from the list: it is in the composer now, and
                            // leaving a duplicate to click is just confusing.
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
}


impl Workbench {
    /// Long jobs still running, and the ones that finished this session.
    ///
    /// The theorizer and DataVoyager return a task id immediately and finish minutes
    /// later, so without this the answer to "is it still going?" was nothing at all —
    /// and, worse, nobody was collecting the result (docs §29).
    /// An agent's own plan, as a checklist with the step it is on marked.
    ///
    /// **The agent's words, its order, its statuses.** Nothing is derived and nothing is estimated:
    /// no percentage, no bar, no remaining-time guess. §73's rule about provenance applies just as
    /// hard to progress — a number a researcher believes is worse than no number, and the only
    /// honest denominator is the one the agent wrote down itself.
    ///
    /// `busy` is what the running step is doing right now, which is the `activity` the watcher
    /// already reads. It rides the in-progress line rather than a row of its own, because a
    /// forty-second `execute` is a property of *that step*, not of the plan.
    ///
    /// Empty plan, empty element. `write_todos` is optional and the model skips it for simple
    /// requests, so a plan is a thing that sometimes exists — not a thing to fake a skeleton for
    /// (§178).
    pub(crate) fn plan_list(&self, todos: &[protocol::Todo], busy: Option<&str>) -> Div {
        let mut list = div().flex().flex_col().w_full().min_w_0().gap_1();
        if todos.is_empty() {
            return list;
        }
        for (at, todo) in todos.iter().enumerate() {
            // Done recedes, doing is the one you read, still-to-come is legible but quiet. All
            // three stay above the AA floor `theme` guarantees — "faint" is not permission to be
            // unreadable, and a scientist scanning a plan is reading, not glancing.
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
}


impl Workbench {
    /// The coordinator's plan for this conversation, when it wrote one.
    ///
    /// Its own section rather than a line in the spine, because the spine is the *project* —
    /// durable, surviving every turn — and this is the working plan for the question in flight.
    /// Filing them together would make an abandoned step look like a project commitment.
    ///
    /// Kept after the turn ends on purpose: a finished plan is the account of what the answer
    /// involved, and clearing it the moment the last token arrives would delete the explanation
    /// exactly when someone starts reading it.
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
            // The coordinator's plan gets no activity string: `activity` is read off a *worker's*
            // thread, and this conversation's current tool is already the road strip's job.
            .child(self.plan_list(&self.plan, None));
        section
    }
}


impl Workbench {
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
                // **No horizontal padding**, so `BACKGROUND JOBS` starts on the same x as `PLAN`
                // and `OUTPUTS` above and below it; the fill runs the width of the section
                // instead. Which is the shape that was asked for the last time a heading here
                // became pressable: *"a whole rectangle use the hover colour so I know that I can
                // click there."*
                .hover(|style| {
                    let fill = theme::hover_over(theme::surface());
                    style
                        .bg(rgb(fill))
                        .text_color(rgb(theme::ink_on(fill)))
                        .cursor_pointer()
                })
                .text_color(rgb(theme::text_faint()))
                .text_xs()
                // Drawn whether or not a pointer is near it. A disclosure whose only sign is a
                // hover is one a researcher concludes does not exist (§199) — and this is the
                // same `▾`/`▸` the transcript's step groups have used all along, so the fold in
                // the panel and the fold in the conversation are one gesture.
                .child(format!(
                    "{} BACKGROUND JOBS",
                    if self.jobs_expanded { "▾" } else { "▸" }
                ))
                .child(
                    // Named states, not a total. `3 jobs` folded is a number you have to unfold
                    // to act on, and not having to is the entire point of the fold.
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

        // Background workers first, because one of them may be *stopped waiting for you* —
        // and until this existed that task simply hung, since the gate it hit runs on its
        // own thread and nothing in the UI could answer it (docs §31).
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
            // Room for the thumb painted over this by the wrapper below (docs §100).
            .pr(px(SCROLL_GUTTER))
            .max_h(px(JOBS_BODY_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.jobs_scroll)
            // **The wheel drives one list at a time.** gpui's own scroll handler does not stop
            // propagation and `should_handle_scroll` is true for every hitbox under the pointer,
            // so an inner scroller and the panel it sits in both take the same delta from the
            // same event and slide together — twice the speed, in two places. The offset handler
            // is registered after this one and the bubble phase runs in reverse, so this fires
            // second and stops the panel without stopping the list.
            //
            // Only attached when there is something here to scroll, so a two-row section leaves
            // the wheel to the panel rather than swallowing it. `max_offset` is meaningless until
            // the first paint, which costs one frame's worth of double-scroll and no more — the
            // same deal [`scrollbar`] already takes for the thumb.
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
            // The bar sits *outside* the element it measures; inside, it would scroll along with
            // the thing it is reporting on.
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
}


impl Workbench {
    /// One background worker: what it is, what it is doing, its plan, its gate, its files.
    ///
    /// Split out of [`Self::jobs_section`] because a worker waiting for approval is rendered from
    /// a different place in the tree than one merely running — pinned above the scroller rather
    /// than inside it — and two copies of this would be two places for the Approve button to
    /// drift apart.
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
            // A child of a `max_h` flex column shrinks to fit it by default, which would squash
            // a worker's whole plan into the height of a line rather than letting it scroll.
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
                    // `step 4 of 7`, and nothing when the agent wrote no plan. The one number
                    // in this panel with a real denominator (§209).
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
                    // The failure the server recorded, in place of the bare word
                    // "error" — which is all this said while two rounds went into
                    // guessing what had actually happened (docs §38).
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
                        // What it is *doing*, not just that it is doing something —
                        // "running" for ten minutes tells a researcher nothing about
                        // whether to wait (docs §42).
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
            // **What turns six minutes of "running · execute" into something a person can
            // read.** Measured on a real run: eight approval rounds and 35–43 seconds per
            // command, with nothing on screen saying how much of it was left (§209).
            .when(!task.todos.is_empty(), |row| {
                row.child(self.plan_list(&task.todos, task.activity.as_deref()))
            });

        if let Some(request) = &task.pending {
            let task_id = task.task_id.clone();
            // Capped and scrollable, for the same reason the foreground card is: a
            // background worker writes long scripts, and a command tall enough to push
            // Approve out of the panel is a gate the researcher cannot answer.
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
                // The command verbatim, exactly as the foreground card shows it: this
                // runs on the researcher's own machine, and the only meaningful review
                // is of the actual text (docs §19).
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
            // A background worker asks once per command over several minutes. Without
            // this the researcher has to sit on the panel and answer each one, which
            // defeats the entire point of handing the work to the background.
            //
            // Both blanket grants appear here, and the conversation-wide one is worded
            // *identically* to the chat's. They are two gates — the coordinator asks
            // below the composer, a worker asks in this panel — and which one appears
            // depends on who happened to need permission. A grant offered in one place
            // and not the other reads as the button moving around at random (docs §44).
            for (suffix, label, conversation_wide) in [
                ("task", "Approve the rest of this task", false),
                ("conv", "Approve everything in this conversation", true),
            ] {
                row = row.child(
                    ui::Button::new(
                        SharedString::from(format!("bg-approve-{suffix}-{task_id}")),
                        label,
                    )
                    // `text_xs` in the original: these sit under the pair above and
                    // are the wider-scope variants of it, not peers.
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

        // **What it produced, one press away.** Asked for directly: *"when a background task
        // has a success, we should see a modal button the user can press… so the user doesn't
        // type it every time in the chatbox."* A finished worker's output is already on disk
        // — §151 verified plots landing at `<task>/…` inside the conversation's own folder —
        // and until now the only way to reach it was to compose a question and wait for a
        // turn to answer it (docs §198).
        //
        // **Opens the folder rather than sending a turn.** No model call, nothing billed,
        // and it is instant; the files are the result, not a description of them.
        if task.succeeded() {
            if let Some(dir) = self
                .thread_workspace()
                .map(|conversation| workspace::worker_dir(&conversation, &task.thread_id))
            {
                row = row.child(
                    div()
                        .id(SharedString::from(format!("task-files-{}", task.task_id)))
                        .flex_none()
                        .mt_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .hover(|style| {
                            let fill = theme::hover_over(theme::surface());
                            style
                                .bg(rgb(fill))
                                .text_color(rgb(theme::ink_on(fill)))
                                .cursor_pointer()
                        })
                        // Names the specialist, because several run at once (§43) and a row
                        // of identical buttons is one you have to count rows to use.
                        .child(format!(
                            "Show what {} produced",
                            task.agent_name.replace('_', " ")
                        ))
                        .on_click(move |_event, _window, _cx| {
                            if let Err(error) = workspace::open(&dir) {
                                tracing::warn!(%error, "could not open a worker's folder");
                            }
                        }),
                );
            }
        }
        row
    }
}


impl Workbench {
    /// One long-running job: the theorizer, or a DataVoyager analysis.
    ///
    /// No `cx` and no controls — a job is something this client polls, not a gate it can answer.
    /// What the row owes a reader is whether it is still going and roughly how long that takes.
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
            // Say how long it usually takes. A spinner with no expectation attached
            // is indistinguishable from a hang.
            format!("running · usually {}", job.kind.expected(job.size))
        };
        // A finished discovery run is the one job row with something to open: its results are a
        // tree of experiments, and the alternative is composing a question to ask about work the
        // app already has (the argument §198 made for the worker-files button).
        let readable = job.kind == protocol::JobKind::Discovery && job.succeeded();
        let mut row = div()
            // Always identified, whether or not it is pressable: `.id()` changes the element's
            // type, and branching on it would mean two incompatible return values for one row.
            .id(SharedString::from(format!("job-row-{}", job.task_id)))
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            // A child of a `max_h` flex column shrinks to fit it by default, which would squash
            // three rows into the height of one rather than letting them scroll.
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
                        // Clipped rather than wrapped whole. The full question is a paragraph the
                        // turn that launched it already holds; what this row needs it for is
                        // telling two concurrent analyses apart, and the first clause does that.
                        .child(protocol::clip(&job.question, JOB_QUESTION_CHARS)),
                )
            });

        if readable {
            let run_id = job.task_id.clone();
            let name = job.question.clone();
            row = row
                // Said as well as coloured, because a hover-only affordance is one a researcher
                // finds by accident — and this is the only way into a run they paid for.
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
}


impl Workbench {
    /// One line for what this conversation *ran*, beside what it produced.
    ///
    /// **This is the half §219 has been missing.** The provenance recorder records and does not
    /// block, deliberately — *"the rules worth enforcing are the ones that come from failures
    /// actually seen"* — and the roadmap has carried the same sentence about it since: *nothing has
    /// been read off it yet*. A record nobody reads is a record that is not working.
    ///
    /// Shown only when something ran, so an ordinary conversation gains no furniture. When a
    /// command named a file outside this conversation, the line says so in the accent colour and
    /// says how many — because that is §160's failure, and the whole point is that it is legible on
    /// the day it happens rather than a week later.
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
}


impl Workbench {
    pub(crate) fn outputs_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // What is actually on disk, rather than the agent's own artifact list: a file written by
        // a script inside `execute` registers no artifact, and those are most of them.
        let listing = self
            .thread_workspace()
            .map(|dir| workspace::output_listing(&dir));
        let files = listing
            .as_ref()
            .map(|listing| listing.groups.as_slice())
            .unwrap_or_default();
        let count: usize = files.iter().map(|(_, items)| items.len()).sum();
        // `output_listing` groups by file kind for ordering. The gallery's meaningful boundary
        // is instead the directory the agent chose (§152), so restore one ordered sequence before
        // grouping by parent. Cloning metadata only; no file is read here.
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

        // **Asked before the guard below, and that is the whole point.** The case where this line
        // matters most is a turn that wrote nothing *into the conversation* — because everything it
        // wrote went somewhere else. That is exactly `count == 0`, so the first version, which
        // added it after the early return, hid it in the one situation it exists for. A researcher
        // pressed the button, the file went to `/tmp`, and the panel stayed silent (§277).
        let ran = self.commands_line(cx);

        // **Nothing at all when there is nothing.** This used to promise which artifacts would
        // appear before the filesystem had any. The recursive scan now makes §117's subfolders
        // visible, but an empty section still says less and is right — and "nothing" now includes
        // having run nothing.
        if outputs_are_empty(count, self.buckets.len(), usize::from(ran.is_some())) {
            return section;
        }

        // Above `FILES`, because "what ran" is the question that explains why the file list is
        // shorter than expected — which is exactly §160's morning.
        section = section.children(ran);

        if count > 0 {
            section = section.child(section_label_owned(format!("FILES · {count}")));
        }

        if listing.as_ref().is_some_and(|listing| listing.truncated) {
            // The scan is intentionally bounded: an agent can create a virtualenv or unpack a
            // dataset under its workspace. Say when that protection bites, because a silent cap
            // would only turn §117's missing-folder defect into a missing-513th-file defect.
            section = section.child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Showing a bounded view. Open the folder to see the rest."),
            );
        }

        // Images first and together, then everything else — the two groups the researcher asked
        // for. Images lead because they are what a person opens the panel to look at; a CSV is
        // opened to *check* something, which is a deliberate act further down.
        //
        // "Together" is now bounded by who produced them (§199): one tray per body of work, not
        // one tray for the window.
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
                    // A lone file stays a row: it has the whole width for its name and shape, and
                    // a grid of one is a tile with nothing to compare it to.
                    section = section.child(self.output_panel_row(
                        format!("panel-output-{}", output.name),
                        output,
                        worker.as_deref(),
                        cx,
                    ));
                } else {
                    // Still folder-grouped, because two runs' `results/` directories are still two
                    // things — the image grid above is the only surface where kind outranks folder.
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

        // Everything this conversation wrote, in one folder the researcher already owns.
        // This *is* "download all the documents": the files are in their own Documents
        // directory (`workspace.rs`), so there is nothing to package — the ask was only
        // ever for a way to get at them.
        //
        // Dashed and last, because it is a way *out* of the panel rather than another row in it —
        // and it reaches anything beyond §143's deliberate scan bounds.
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
            // Show a bounded number of titles — a literature search can return
            // dozens, and the count already conveys the scale.
            const MAX_SHOWN: usize = 4;
            // **Datasets get a way in.** Their bucket items are titles truncated to 96
            // characters, which for five records of one multi-site study is five identical rows;
            // the modal has the identifier that tells them apart, the page link and the download
            // (docs §223). Only when the structured list actually arrived, so a bucket from an
            // older backend still renders as plain text rather than as a heading that does
            // nothing.
            let openable = matches!(bucket.name, "datasets" | "libraries")
                && !bucket.items.is_empty();
            // **What the researcher is counting, not what the payload wrapped.** `libraries` holds
            // one artifact per turn and `datasets` one entry per recommendation, so the bucket's
            // own length answered *how many envelopes* for the first and *how many datasets* for
            // the second — and `libraries · 1` beside two indexed papers read as the app losing
            // one (§232). The structured lists are what a person means by these words.
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
                // **Per bucket, not one id for all of them.** Every heading in this loop carried
                // `"datasets-heading"`, so with two buckets on screen two sibling elements shared
                // an element id — and gpui resolves interaction against that path. Whatever it
                // did with the collision, it was not "call the listener on the datasets one".
                .id(SharedString::from(format!("bucket-{}", bucket.name)))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                // **A block, not a strip.** The first version hovered a bare text div, so the
                // target was the width of the words and nothing announced it: *"if I not hover
                // datasets thin rectangle I will never know that there is a modal there."* The
                // same box `open-all-sources` uses — full width, padded, rounded — so the fill
                // lands on a shape a pointer will cross on its way past.
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
                    // Said as well as coloured. A hover-only affordance is one a researcher finds
                    // by accident, and this is the panel's only way into the dataset list.
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

