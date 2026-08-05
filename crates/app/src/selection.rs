//! Selecting text in the transcript, and copying it out.
//!
//! A researcher reading an answer wants the paragraph, the citation, the number. Until now
//! the transcript was unselectable: the only way to get text out of it was to find the
//! conversation on disk. This module is the missing half.
//!
//! **The plan said this was impossible.** It recorded that "GPUI 0.2.2 cannot do text
//! selection — this is the one thing the framework genuinely makes hard." That was wrong, and
//! checking cost one `grep`: [`gpui::TextLayout`] exposes `index_for_position` and
//! `position_for_index` (`elements/text.rs:483` and `:517`), which is a hit-test and its
//! inverse — everything selection needs. What GPUI genuinely does not provide is *selection
//! state and painting*: nothing under `gpui/src/elements/` mentions the word, and
//! `InteractiveText` offers click and hover indices only. So that part is here.
//!
//! # How it fits together
//!
//! The transcript is a tree of divs — headings, paragraphs, list items, table cells — each
//! holding a [`gpui::StyledText`]. Rewriting that into one big custom element would mean
//! re-implementing Markdown layout, so instead every run of text is wrapped in a
//! [`Selectable`], which:
//!
//! 1. lays out and paints its inner `StyledText` unchanged, so styling is untouched;
//! 2. registers its [`gpui::TextLayout`] in a shared [`Spans`] registry, under an index
//!    assigned in document order;
//! 3. paints the part of the selection that falls inside it — *before* the glyphs, or the
//!    quad would cover the text it is meant to highlight.
//!
//! Mouse handlers on the transcript consult that registry to turn a pixel into a
//! [`Spot`] — which span, which byte. A drag from one span to another therefore selects
//! everything between them, because the registry is ordered.
//!
//! The registry is rebuilt every frame: layouts move when the window resizes or the
//! transcript scrolls, and a stale rectangle is a selection drawn in the wrong place.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    fill, point, px, rgba, App, Bounds, Element, GlobalElementId, InspectorElementId, LayoutId,
    Pixels, Point, SharedString, StyledText, TextLayout, Window,
};

/// One end of a selection: which run of text, and a byte offset into it.
///
/// Ordered by span first, so comparing two spots says which comes first in the transcript —
/// that is what lets a drag upwards select the same text as a drag downwards.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Spot {
    pub span: usize,
    pub offset: usize,
}

/// A selection in progress, or a finished one.
#[derive(Clone, Copy, Default, Debug)]
pub struct Selection {
    /// Where the drag started. Stays put while the pointer moves.
    anchor: Option<Spot>,
    /// Where the pointer is now. May be before the anchor.
    head: Option<Spot>,
    /// Whether the button is still down. A click with no drag has anchor == head, which is
    /// an empty selection and paints nothing.
    dragging: bool,
}

impl Selection {
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// Start a drag, discarding whatever was selected before.
    pub fn begin(&mut self, at: Spot) {
        self.anchor = Some(at);
        self.head = Some(at);
        self.dragging = true;
    }

    pub fn extend(&mut self, to: Spot) {
        if self.anchor.is_some() {
            self.head = Some(to);
        }
    }

    pub fn finish(&mut self) {
        self.dragging = false;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The two ends in document order, or `None` when nothing is selected.
    pub fn ordered(&self) -> Option<(Spot, Spot)> {
        let (anchor, head) = (self.anchor?, self.head?);
        if anchor == head {
            return None;
        }
        Some(if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        })
    }

    /// Which bytes of `span` are selected, if any.
    ///
    /// `len` is that span's own length, used to mean "to the end" for the spans a selection
    /// passes straight through.
    fn range_in(&self, span: usize, len: usize) -> Option<Range<usize>> {
        let (start, end) = self.ordered()?;
        if span < start.span || span > end.span {
            return None;
        }
        let from = if span == start.span { start.offset } else { 0 };
        let to = if span == end.span { end.offset } else { len };
        let (from, to) = (from.min(len), to.min(len));
        (from < to).then_some(from..to)
    }
}

/// Every run of selectable text laid out this frame, in document order.
///
/// Keyed rather than pushed, because the order things are painted in is not something to
/// depend on: the key *is* the document position, assigned while the transcript is built.
#[derive(Default)]
pub struct Spans {
    entries: BTreeMap<usize, Entry>,
}

struct Entry {
    text: SharedString,
    layout: TextLayout,
}

impl Spans {
    /// Forget the previous frame. Called as the transcript is rebuilt — bounds from the last
    /// frame would put the highlight where the text no longer is.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn insert(&mut self, span: usize, text: SharedString, layout: TextLayout) {
        self.entries.insert(span, Entry { text, layout });
    }

    /// The spot nearest a point on screen.
    ///
    /// A point inside a span's box is that span. A point in the gap between paragraphs, or
    /// past the last one, belongs to the vertically nearest span — which is what makes a drag
    /// that leaves the text still extend the selection instead of stopping dead.
    pub fn spot_at(&self, position: Point<Pixels>) -> Option<Spot> {
        let mut nearest: Option<(Pixels, usize, &Entry)> = None;
        for (&span, entry) in &self.entries {
            let bounds = entry.layout.bounds();
            let distance = if position.y < bounds.top() {
                bounds.top() - position.y
            } else if position.y > bounds.bottom() {
                position.y - bounds.bottom()
            } else {
                px(0.)
            };
            if distance == px(0.) && bounds.left() <= position.x && position.x <= bounds.right() {
                return Some(Spot {
                    span,
                    offset: offset_in(entry, position),
                });
            }
            if nearest.is_none_or(|(best, _, _)| distance < best) {
                nearest = Some((distance, span, entry));
            }
        }
        let (_, span, entry) = nearest?;
        Some(Spot {
            span,
            offset: offset_in(entry, position),
        })
    }

    /// The first and last spot in the transcript, for "select everything".
    pub fn whole(&self) -> Option<(Spot, Spot)> {
        let (&first, _) = self.entries.iter().next()?;
        let (&last, entry) = self.entries.iter().next_back()?;
        Some((
            Spot {
                span: first,
                offset: 0,
            },
            Spot {
                span: last,
                offset: entry.text.len(),
            },
        ))
    }

    /// The selected text, ready for the clipboard.
    ///
    /// Spans are joined with newlines because each one is a paragraph, a heading, a list item
    /// or a table cell — separate blocks in the original Markdown, and running them together
    /// would produce a wall of text nobody can paste into a paper.
    pub fn selected_text(&self, selection: &Selection) -> Option<String> {
        let (start, end) = selection.ordered()?;
        let mut parts = Vec::new();
        for (&span, entry) in self.entries.range(start.span..=end.span) {
            if let Some(range) = selection.range_in(span, entry.text.len()) {
                // A span whose bytes moved under us — the transcript changed mid-drag —
                // must not panic on a slice that is no longer a char boundary.
                if entry.text.is_char_boundary(range.start)
                    && entry.text.is_char_boundary(range.end)
                {
                    parts.push(entry.text[range].to_string());
                }
            }
        }
        (!parts.is_empty()).then(|| parts.join("\n"))
    }
}

/// Where in a span a point falls.
///
/// `index_for_position` reports `Err` for a point past the end of a line, carrying the
/// nearest index — which is the answer we want, not a failure.
fn offset_in(entry: &Entry, position: Point<Pixels>) -> usize {
    let offset = match entry.layout.index_for_position(position) {
        Ok(offset) | Err(offset) => offset,
    };
    offset.min(entry.text.len())
}

/// Shared between the transcript's mouse handlers and every [`Selectable`] in it.
#[derive(Clone, Default)]
pub struct Transcript(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    selection: Selection,
    spans: Spans,
    /// Handed out as the transcript is built, so document order and span index agree.
    next_span: usize,
}

impl Transcript {
    /// Start a fresh frame: forget the last one's rectangles and hand out indices from zero
    /// again. The selection itself survives, because it belongs to the user.
    pub fn begin_frame(&self) {
        let mut state = self.0.borrow_mut();
        state.spans.clear();
        state.next_span = 0;
    }

    /// The next span index, in document order.
    pub fn claim(&self) -> usize {
        let mut state = self.0.borrow_mut();
        let span = state.next_span;
        state.next_span += 1;
        span
    }

    pub fn selection(&self) -> Selection {
        self.0.borrow().selection
    }

    pub fn update(&self, change: impl FnOnce(&mut Selection)) {
        change(&mut self.0.borrow_mut().selection);
    }

    /// Point-to-spot in one borrow, so a caller cannot hold the registry while mutating.
    pub fn spot_at(&self, position: Point<Pixels>) -> Option<Spot> {
        self.0.borrow().spans.spot_at(position)
    }

    pub fn selected_text(&self) -> Option<String> {
        let state = self.0.borrow();
        state.spans.selected_text(&state.selection)
    }

    pub fn select_all(&self) {
        let mut state = self.0.borrow_mut();
        if let Some((start, end)) = state.spans.whole() {
            state.selection.anchor = Some(start);
            state.selection.head = Some(end);
            state.selection.dragging = false;
        }
    }
}

/// A run of transcript text that can be selected.
///
/// Wraps [`StyledText`] rather than replacing it: the inner element keeps doing the layout,
/// the shaping and the highlight runs, and this adds only the registration and the highlight
/// quads. Markdown styling is therefore unaffected by construction, which is the point —
/// bold, links and inline code all still work because nothing about them changed.
pub struct Selectable {
    span: usize,
    text: SharedString,
    styled: StyledText,
    transcript: Transcript,
}

impl Selectable {
    pub fn new(transcript: &Transcript, text: impl Into<SharedString>, styled: StyledText) -> Self {
        Self {
            span: transcript.claim(),
            text: text.into(),
            styled,
            transcript: transcript.clone(),
        }
    }
}

impl gpui::IntoElement for Selectable {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Selectable {
    type RequestLayoutState = ();
    type PrepaintState = Vec<gpui::PaintQuad>;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let (layout_id, ()) = self.styled.request_layout(id, inspector, window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<gpui::PaintQuad> {
        // Inner first: until it has prepainted, its `TextLayout` has no bounds and every
        // query on it panics.
        self.styled
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        let layout = self.styled.layout().clone();
        self.transcript
            .0
            .borrow_mut()
            .spans
            .insert(self.span, self.text.clone(), layout.clone());

        let selection = self.transcript.selection();
        let Some(range) = selection.range_in(self.span, self.text.len()) else {
            return Vec::new();
        };
        highlight(&layout, range, bounds)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        quads: &mut Vec<gpui::PaintQuad>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Behind the glyphs. Painted after, an opaque-enough quad would hide the very words
        // it is supposed to be highlighting.
        for quad in quads.drain(..) {
            window.paint_quad(quad);
        }
        self.styled
            .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
    }
}

/// One rectangle per visual line a selected range covers.
///
/// A single box from the first character to the last would swallow whole lines in between,
/// so each wrapped line gets its own. The middle lines run to the right edge of the element
/// rather than to the end of their text: that is how every editor draws a selected line
/// break, and it is the only honest way to show that the break itself is included.
fn highlight(
    layout: &TextLayout,
    range: Range<usize>,
    bounds: Bounds<Pixels>,
) -> Vec<gpui::PaintQuad> {
    let (Some(from), Some(to)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    // The theme's accent at low alpha, so selection follows the palette instead of being
    // one hard-coded orange. `rgba` takes 0xRRGGBBAA and the theme stores 0xRRGGBB.
    let tint = rgba((crate::theme::accent() << 8) | 0x38);
    rows_between(from, to, layout.line_height(), bounds)
        .into_iter()
        .map(|row| fill(row, tint))
        .collect()
}

/// The rectangles covering a selection that runs from `from` to `to`.
///
/// Split out from [`highlight`] because a `TextLayout` cannot be built without a window, and
/// this is the part that is easy to get wrong and impossible to eyeball: which line gets a
/// partial rectangle and which runs to the edge.
fn rows_between(
    from: Point<Pixels>,
    to: Point<Pixels>,
    line_height: Pixels,
    bounds: Bounds<Pixels>,
) -> Vec<Bounds<Pixels>> {
    // Counted, not accumulated: stepping `y += line_height` until it reaches `to.y` compares
    // f32s for a bound it may overshoot by an epsilon, and a selection that drops or doubles
    // its last line is the kind of bug that only shows up on the one paragraph that wraps.
    let rows = if line_height > px(0.) {
        (((to.y - from.y) / line_height).round() as i32).max(0)
    } else {
        0
    };
    let mut quads = Vec::new();
    for row in 0..=rows {
        let top = from.y + line_height * row as f32;
        let left = if row == 0 { from.x } else { bounds.left() };
        let right = if row == rows { to.x } else { bounds.right() };
        // A selection ending exactly at a line start leaves nothing to draw on that line.
        if right <= left {
            continue;
        }
        quads.push(Bounds::from_corners(
            point(left, top),
            point(right, top + line_height),
        ));
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(span: usize, offset: usize) -> Spot {
        Spot { span, offset }
    }

    #[test]
    fn a_click_without_a_drag_selects_nothing() {
        // Otherwise every click in the transcript would paint a stray sliver and arm a copy
        // that returns one character.
        let mut selection = Selection::default();
        selection.begin(spot(2, 7));
        assert_eq!(selection.ordered(), None);
        assert_eq!(selection.range_in(2, 40), None);
    }

    #[test]
    fn dragging_upwards_selects_the_same_text_as_dragging_down() {
        let mut down = Selection::default();
        down.begin(spot(1, 3));
        down.extend(spot(3, 5));

        let mut up = Selection::default();
        up.begin(spot(3, 5));
        up.extend(spot(1, 3));

        assert_eq!(down.ordered(), up.ordered());
        // The span in the middle is covered end to end, not skipped.
        assert_eq!(down.range_in(2, 12), Some(0..12));
        assert_eq!(up.range_in(2, 12), Some(0..12));
    }

    #[test]
    fn only_the_selected_part_of_the_first_and_last_span_is_included() {
        let mut selection = Selection::default();
        selection.begin(spot(1, 4));
        selection.extend(spot(3, 2));
        assert_eq!(
            selection.range_in(1, 10),
            Some(4..10),
            "first span, from the click"
        );
        assert_eq!(
            selection.range_in(3, 10),
            Some(0..2),
            "last span, up to the pointer"
        );
        // Outside the drag entirely.
        assert_eq!(selection.range_in(0, 10), None);
        assert_eq!(selection.range_in(4, 10), None);
    }

    #[test]
    fn an_offset_past_the_end_of_a_span_is_clamped_rather_than_panicking() {
        // The transcript grows while a turn streams, so a spot recorded a frame ago can name
        // a byte that no longer exists. Slicing on it would panic in the paint path.
        let mut selection = Selection::default();
        selection.begin(spot(0, 0));
        selection.extend(spot(0, 999));
        assert_eq!(selection.range_in(0, 5), Some(0..5));
    }

    #[test]
    fn a_selection_within_one_span_stays_within_it() {
        let mut selection = Selection::default();
        selection.begin(spot(7, 2));
        selection.extend(spot(7, 6));
        assert_eq!(selection.range_in(7, 20), Some(2..6));
        assert_eq!(selection.range_in(6, 20), None);
        assert_eq!(selection.range_in(8, 20), None);
    }

    #[test]
    fn clearing_forgets_both_ends() {
        let mut selection = Selection::default();
        selection.begin(spot(0, 0));
        selection.extend(spot(1, 1));
        assert!(selection.ordered().is_some());
        selection.clear();
        assert_eq!(selection.ordered(), None);
        assert!(!selection.dragging());
    }

    /// A paragraph 200px wide, lines 20px tall, starting at (10, 100).
    fn para() -> Bounds<Pixels> {
        Bounds::from_corners(point(px(10.), px(100.)), point(px(210.), px(160.)))
    }

    #[test]
    fn a_selection_on_one_line_is_one_rectangle_between_the_two_points() {
        let rows = rows_between(
            point(px(40.), px(100.)),
            point(px(90.), px(100.)),
            px(20.),
            para(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left(), px(40.));
        assert_eq!(rows[0].right(), px(90.));
        assert_eq!(rows[0].top(), px(100.));
        assert_eq!(rows[0].bottom(), px(120.));
    }

    #[test]
    fn a_wrapped_selection_runs_to_the_edge_on_every_line_but_the_last() {
        // Three lines: the first from the click to the right edge, the middle one full
        // width, the last from the left edge to the pointer. Drawing one box from first
        // character to last would paint over both lines in between.
        let rows = rows_between(
            point(px(60.), px(100.)),
            point(px(80.), px(140.)),
            px(20.),
            para(),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[0].left(), rows[0].right()), (px(60.), px(210.)));
        assert_eq!((rows[1].left(), rows[1].right()), (px(10.), px(210.)));
        assert_eq!((rows[2].left(), rows[2].right()), (px(10.), px(80.)));
        // Stacked, with no gap and no overlap.
        assert_eq!(rows[0].bottom(), rows[1].top());
        assert_eq!(rows[1].bottom(), rows[2].top());
    }

    #[test]
    fn a_selection_ending_at_the_start_of_a_line_does_not_draw_a_sliver() {
        // Dragging to the very start of the next line: the last row has zero width, and a
        // zero-width quad still shows as a 1px artefact against the text.
        let rows = rows_between(
            point(px(60.), px(100.)),
            point(px(10.), px(120.)),
            px(20.),
            para(),
        );
        assert_eq!(rows.len(), 1, "only the first line has anything to paint");
        assert_eq!((rows[0].left(), rows[0].right()), (px(60.), px(210.)));
    }

    #[test]
    fn a_zero_line_height_does_not_loop_forever() {
        // Only reachable if the layout reports nothing, but an infinite loop here would
        // freeze the window rather than draw the wrong thing.
        let rows = rows_between(
            point(px(20.), px(100.)),
            point(px(90.), px(140.)),
            px(0.),
            para(),
        );
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn indices_are_handed_out_in_document_order() {
        // Span index *is* document position — the ordering that makes a cross-paragraph
        // drag mean anything. A frame that forgot to reset would keep counting upward and
        // every selection would land in the wrong paragraph.
        let transcript = Transcript::default();
        transcript.begin_frame();
        assert_eq!(
            (transcript.claim(), transcript.claim(), transcript.claim()),
            (0, 1, 2)
        );
        transcript.begin_frame();
        assert_eq!(transcript.claim(), 0);
    }

    #[test]
    fn a_new_frame_keeps_the_selection_but_drops_the_rectangles() {
        // The user's selection has to survive a re-render — a streaming turn re-renders
        // constantly — while last frame's bounds must not, or the highlight is drawn where
        // the text used to be.
        let transcript = Transcript::default();
        transcript.begin_frame();
        transcript.update(|selection| {
            selection.begin(spot(0, 1));
            selection.extend(spot(0, 4));
        });
        transcript.begin_frame();
        assert_eq!(
            transcript.selection().ordered(),
            Some((spot(0, 1), spot(0, 4)))
        );
        // Nothing registered yet this frame, so there is nothing to copy — and asking to
        // select everything finds no spans and leaves the user's selection alone.
        assert_eq!(transcript.selected_text(), None);
        transcript.select_all();
        assert_eq!(
            transcript.selection().ordered(),
            Some((spot(0, 1), spot(0, 4)))
        );
    }
}
