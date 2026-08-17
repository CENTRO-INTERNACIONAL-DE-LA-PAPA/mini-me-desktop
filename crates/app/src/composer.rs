//! A single-line text composer for the chat pane.
//!
//! GPUI ships **no text-input widget** — only the primitives (focus, key
//! actions, IME plumbing, `shape_line`) — so an input has to be assembled by
//! hand: cursor motion, selection, clipboard, grapheme-aware boundaries, and a
//! custom `Element` that lays out the line and paints the caret.
//!
//! This is adapted from `crates/gpui/examples/input.rs` in the Zed repository
//! (<https://github.com/zed-industries/zed>), the upstream of the `gpui 0.2.2`
//! crate we depend on. That code is **Apache-2.0**; the notice is retained in
//! NOTICE at the repo root. Changes made here:
//!
//! - `Enter` submits, emitting [`ComposerEvent::Submit`] with the trimmed text
//!   and clearing the field, so the parent view owns what a turn *means*.
//! - Cross-platform bindings: `ctrl-` as well as `cmd-` (the example is
//!   mac-only, and our primary dev machine is Windows).
//! - Dark-theme placeholder colour, and the caret uses the Mini-Me accent.
//! - Rewritten without let-chains, which need edition 2024; we build on 2021.
//! - Submission is ignored while empty, so Enter can't post a blank turn.
//!
//! Multi-line since §55: `shape_line` lays out exactly one line, so the element shapes one
//! per `\n` itself and hit-tests per line. `shift-enter` inserts a break; Enter still sends.

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, rgba, App, Bounds, ClipboardItem,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle,
    Focusable, GlobalElementId, KeyBinding, LayoutId, LineFragment, LineWrapper, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme;

actions!(
    composer,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Submit,
        Newline,
    ]
);

/// Key bindings the composer needs. Scoped to the `Composer` key context so
/// `enter` and friends only apply while the field is focused.
pub fn key_bindings() -> Vec<KeyBinding> {
    let ctx = Some("Composer");
    let mut bindings = vec![
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("enter", Submit, ctx),
        // Shift-Enter for a line break. Enter still sends, because sending is what a
        // chat field is for and rebinding it would surprise everyone — but a prompt
        // carrying a script, a table or a list could not be pasted or typed at all
        // before this (docs §55).
        KeyBinding::new("shift-enter", Newline, ctx),
    ];
    // Bind both modifiers: `cmd` on macOS, `ctrl` everywhere else. Registering
    // both is harmless — only one exists on a given keyboard.
    for modifier in ["cmd", "ctrl"] {
        bindings.push(KeyBinding::new(&format!("{modifier}-a"), SelectAll, ctx));
        bindings.push(KeyBinding::new(&format!("{modifier}-c"), Copy, ctx));
        bindings.push(KeyBinding::new(&format!("{modifier}-v"), Paste, ctx));
        bindings.push(KeyBinding::new(&format!("{modifier}-x"), Cut, ctx));
    }
    bindings
}

/// What the composer tells its parent.
pub enum ComposerEvent {
    /// The user pressed Enter on non-empty text.
    Submit(String),
}

pub struct Composer {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// Last shaped line + bounds, cached by the element so mouse hit-testing
    /// and IME rect queries can map coordinates back to string offsets.
    /// One shaped line per `\n`-separated line, with the byte offset it starts at.
    ///
    /// Was a single `ShapedLine`. A prompt carrying a script or a list has to be typeable,
    /// and that means the field has to be genuinely multi-line — caret, selection and
    /// hit-testing included, not just a `\n` the renderer swallows (docs §55).
    last_layout: Vec<(usize, ShapedLine)>,
    last_bounds: Option<Bounds<Pixels>>,
    /// The line height the last frame drew with, so mouse rows can be worked out.
    line_height: Pixels,
    /// The topmost visual row actually painted.
    ///
    /// The field stops growing at [`MAX_VISIBLE_LINES`], so past that it has to *move* instead —
    /// otherwise the caret walks off the bottom and the researcher is typing into a box that
    /// shows them the beginning of what they wrote. Every screen ↔ offset conversion has to
    /// subtract it, which is why it is remembered here rather than recomputed per query.
    first_row: usize,
    is_selecting: bool,
    /// Whether Enter on an *empty* field still counts as a submission. False for the
    /// chat composer (an empty prompt is nothing to send); true for the command
    /// palette, where Enter means "activate the selected command", not "send this
    /// text" — so it has to fire before the user has typed anything.
    submits_empty: bool,
    /// Render the content as asterisks. For API-key fields in Settings.
    masked: bool,
    /// While a turn is in flight the field is read-only.
    disabled: bool,
}

impl gpui::EventEmitter<ComposerEvent> for Composer {}

impl Composer {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: Vec::new(),
            last_bounds: None,
            line_height: px(20.),
            first_row: 0,
            is_selecting: false,
            submits_empty: false,
            masked: false,
            disabled: false,
        }
    }

    /// The current text. Read by the command palette to filter on every keystroke.
    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_submits_empty(&mut self, submits_empty: bool) {
        self.submits_empty = submits_empty;
    }

    /// Hide what is typed — API keys should not sit on screen in plain text.
    pub fn set_masked(&mut self, masked: bool) {
        self.masked = masked;
    }

    /// What the element should draw: the content, or asterisks standing in for it.
    ///
    /// Masks **byte for byte**, not character for character, so the mask has exactly the
    /// same length as the content. Cursor and selection are byte offsets into the string
    /// being shaped, and a mask of a different length would put the caret in the wrong
    /// place — or panic on a boundary. Keys are ASCII in practice, so the count is exact;
    /// for anything multi-byte the mask is simply a little longer, which for a secret is
    /// no loss.
    fn display_content(&self) -> SharedString {
        if self.masked && !self.content.is_empty() {
            return SharedString::from("*".repeat(self.content.len()));
        }
        self.content.clone()
    }

    /// Prefill the field (used to seed the first prompt).
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.marked_range = None;
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        self.submit_now(cx);
    }

    /// A line break, without sending.
    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        // Straight through the same path typing takes, so the caret, the selection and
        // the undo-shaped behaviour of a replaced selection all stay consistent.
        self.replace_text_in_range(None, "\n", window, cx);
    }

    /// Submit the current text, if any. Exposed so a Send button can do exactly
    /// what Enter does without depending on where focus happens to be.
    pub fn submit_now(&mut self, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let text = self.content.trim().to_string();
        if text.is_empty() && !self.submits_empty {
            return;
        }
        self.content = SharedString::default();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(ComposerEvent::Submit(text));
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all_text(cx);
    }

    /// Whether anything is selected.
    ///
    /// A right-click menu has to know before it offers Cut and Copy: an item that looks
    /// available and does nothing is worse than one that is visibly greyed out.
    pub fn has_selection(&self) -> bool {
        !self.selected_range.is_empty()
    }

    /// Whether the field accepts edits — false while a turn is running.
    pub fn is_editable(&self) -> bool {
        !self.disabled
    }

    pub fn select_all_text(&mut self, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    /// Copy the selection, reporting whether there was anything to copy.
    pub fn copy_to_clipboard(&mut self, cx: &mut Context<Self>) -> bool {
        if self.selected_range.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.content[self.selected_range.clone()].to_string(),
        ));
        true
    }

    pub fn cut_to_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_to_clipboard(cx) || self.disabled {
            return;
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    pub fn paste_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        // Line breaks are kept. They were flattened to spaces while this field really was
        // one line, and that line outlived §55's multi-line rewrite — so pasting the very
        // thing §55 existed for, a script or a table, silently ran it all together. `\r\n`
        // is normalised because a Windows clipboard is full of it and a stray `\r` shapes
        // as a box.
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.replace_text_in_range(None, &text, window, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        self.paste_from_clipboard(window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_to_clipboard(cx) {
            // Hand `ctrl-c` on. Focus lives here almost all the time, so consuming the
            // shortcut with nothing selected would make copying out of the transcript
            // impossible without first clicking somewhere to move focus — which is not
            // something a reader should have to know (docs §62).
            cx.propagate();
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.cut_to_clipboard(window, cx);
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        if self.last_layout.is_empty() {
            return 0;
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        // Which visual line the pointer is on, then where along it. Offset by the scroll, or a
        // click in a field showing rows 12–19 would land in rows 0–7.
        let row = self.first_row
            + ((position.y - bounds.top()) / self.line_height).floor() as usize;
        let row = row.min(self.last_layout.len().saturating_sub(1));
        let (start, line) = &self.last_layout[row];
        start + line.closest_index_for_x(position.x - bounds.left())
    }

    /// Where a byte offset sits on screen, as (line index, x within the line).
    fn position_for_offset(&self, offset: usize) -> Option<(usize, Pixels)> {
        for (row, (start, line)) in self.last_layout.iter().enumerate() {
            let end = start + line.len();
            // `<=` so the caret at the very end of a line lands on that line rather
            // than falling through to the next one.
            if offset <= end {
                return Some((row, line.x_for_index(offset.saturating_sub(*start))));
            }
        }
        self.last_layout.last().map(|(start, line)| {
            (
                self.last_layout.len() - 1,
                line.x_for_index(offset.saturating_sub(*start)),
            )
        })
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for Composer {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let (start_row, start_x) = self.position_for_offset(range.start)?;
        let (end_row, end_x) = self.position_for_offset(range.end)?;
        // The IME panel wants one rectangle; for a marked range spanning lines the
        // sensible answer is the first line's, which is where the caret is.
        let top = bounds.top() + self.line_height * (start_row as f32 - self.first_row as f32);
        let bottom = top + self.line_height;
        let end_x = if end_row == start_row { end_x } else { start_x };
        Some(Bounds::from_corners(
            point(bounds.left() + start_x, top),
            point(bounds.left() + end_x, bottom),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let _ = bounds.localize(&point)?;
        let utf8_index = self.index_for_mouse_position(point);
        Some(self.offset_to_utf16(utf8_index))
    }
}

/// Lays out the composer's single line and paints the caret/selection.
struct ComposerElement {
    composer: Entity<Composer>,
}

struct PrepaintState {
    lines: Vec<(usize, ShapedLine)>,
    cursor: Option<PaintQuad>,
    /// One rectangle per line a selection covers — a selection spanning three lines is
    /// three quads, not one box swallowing the text between them.
    selection: Vec<PaintQuad>,
    /// The row drawn at the top of the box — see [`Composer::first_row`].
    first_row: usize,
}

/// Room kept clear at the right edge, so the caret has somewhere to be.
///
/// **The caret is painted after the last glyph.** A row wrapped at exactly the box width therefore
/// leaves it nowhere to go: it lands on the border, a couple of pixels outside the text, which
/// reads as the text having run off the side — *"the bar is beyond the text"* (§202). Wrapping one
/// character earlier costs nothing and keeps the thing you are typing with inside the thing you
/// are typing in.
const CARET_ROOM: Pixels = px(4.);

/// The width text may occupy inside a box this wide, or `None` if there is no room to speak of.
///
/// Shared by the height calculation and the shaping, because a wrap width they disagreed about is
/// a box whose height is measured for one layout and painted with another.
fn wrappable(width: Pixels) -> Option<Pixels> {
    let usable = width - CARET_ROOM;
    if usable > px(0.) {
        Some(usable)
    } else {
        None
    }
}

/// How tall the field is allowed to grow before it simply stops.
///
/// A pasted script should make the composer bigger, not eat the transcript. Eight lines is
/// enough to see a short command in full and still leave the conversation on screen.
const MAX_VISIBLE_LINES: usize = 8;

/// The byte range of every **visual** row the text occupies at a given width.
///
/// **A line the researcher did not break still has to break.** §55 made the field multi-line by
/// splitting on `\n` and shaping one line per segment, with no wrap width — so a long paragraph
/// typed without pressing shift-enter was one row, shaped at its natural width and clipped at the
/// field's right edge. The text and the caret both went out the side, and the box stayed one line
/// tall while it happened: *"when I write a long text the box doesn't increase in height, which
/// causes I cannot see what I'm typing"* (docs §200). Nobody writing a research question types
/// their own line breaks, so the only line that mattered was the one case this never handled.
///
/// Wrapping is done in the **string** domain and each row is then shaped on its own, which keeps
/// every existing mechanism — caret `x_for_index`, per-row hit testing, one selection quad per
/// row — working unchanged on a longer list. `shape_text` would have returned wrapped lines with
/// their own coordinate space and asked all four of those to be rewritten at once.
///
/// `breaks` is asked, per hard line, for the offsets **within that line** where a new row starts;
/// an empty answer means the line does not wrap. Passed in rather than measured here so the part
/// that can be wrong on its own — turning break points into ranges over the whole string — is
/// testable without a window, and so the first frame (which has no width yet) can simply say
/// "nowhere".
fn row_ranges(text: &str, mut breaks: impl FnMut(&str) -> Vec<usize>) -> Vec<Range<usize>> {
    let mut rows: Vec<Range<usize>> = Vec::new();
    let mut offset = 0usize;
    // Hard breaks first: they are the researcher's own, and the wrapper skips `\n` entirely.
    for segment in text.split('\n') {
        let end = offset + segment.len();
        let mut start = offset;
        for at in breaks(segment) {
            let at = offset + at;
            // Bounded on both sides: a boundary at or before where this row began would make an
            // empty range, and one at the very end would give the field a blank final row it
            // never earned.
            if at > start && at < end {
                rows.push(start..at);
                start = at;
            }
        }
        rows.push(start..end);
        // +1 for the newline itself, so offsets keep matching the real string.
        offset = end + 1;
    }
    rows
}

/// Ask the text system where a line has to break, at a known width.
///
/// `None` — or a width of zero — means "don't wrap", which is the first frame's answer: the width
/// comes from the previous frame's bounds and there isn't one yet.
fn wrap_at<'a>(
    wrapper: &'a mut LineWrapper,
    wrap_width: Option<Pixels>,
) -> impl FnMut(&str) -> Vec<usize> + 'a {
    move |segment: &str| match wrap_width {
        Some(width) if width > px(0.) => {
            // Bound to a `let`: `wrap_line` borrows the fragments for as long as the iterator
            // lives, so a temporary built in the call expression would not outlive the collect.
            let fragments = [LineFragment::text(segment)];
            wrapper
                .wrap_line(&fragments, width)
                .map(|boundary| boundary.ix)
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Slice global text runs down to one line's byte range.
///
/// Runs exist to carry the IME's underline for marked text, and they are expressed over
/// the whole string — so each line needs its own view of them or CJK input would underline
/// the wrong characters.
fn runs_for(runs: &[TextRun], start: usize, end: usize) -> Vec<TextRun> {
    let mut sliced = Vec::new();
    let mut cursor = 0usize;
    for run in runs {
        let run_start = cursor;
        let run_end = cursor + run.len;
        cursor = run_end;
        let from = run_start.max(start);
        let to = run_end.min(end);
        if from < to {
            sliced.push(TextRun {
                len: to - from,
                ..run.clone()
            });
        }
    }
    sliced
}

impl IntoElement for ComposerElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ComposerElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // Height follows the content, capped — counted here rather than in prepaint because
        // layout has to know the size before anything is shaped.
        //
        // **Measured against the previous frame's width**, which is the one thing in this
        // function that looks like a shortcut and is not. Wrapping needs a width; the width is
        // only known once layout has run; so a truthful height needs either last frame's number
        // or `request_measured_layout`. The second means giving up the `width: 100%` +
        // `flex_grow` + `min_width` triple below, and this file has paid for that combination
        // three times over (§72, §88, §97, §99) with fields that collapsed to a 10px sliver.
        // The cost of the first is one frame of stale height while a window is being dragged
        // wider, which the next frame corrects — and a resize is a stream of frames.
        let (content, wrap_width) = {
            let composer = self.composer.read(cx);
            let content = composer.display_content();
            (
                if content.is_empty() {
                    composer.placeholder.clone()
                } else {
                    content
                },
                composer
                    .last_bounds
                    .and_then(|bounds| wrappable(bounds.size.width)),
            )
        };
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let lines = {
            let mut wrapper = cx.text_system().line_wrapper(text_style.font(), font_size);
            row_ranges(&content, wrap_at(&mut wrapper, wrap_width)).len()
        };
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        // **And grow, which is the half that was missing.** `width: 100%` only means anything
        // when the parent's width is definite; inside the theme picker's popup it resolved to
        // nothing, so the field's border collapsed to a sliver while its placeholder painted
        // straight out the side. §72 fixed one box that way and the same bug came back on both
        // of them (docs §88). `flex_grow` needs no definite parent — it takes whatever free
        // space the row has — so the two together hold in either kind of container.
        style.flex_grow = 1.0;
        // **A floor, because a text field is never legitimately narrower than this.** The two
        // above are both *derived* — a percentage of the parent, and a share of the parent's
        // spare room — so both evaluate to nothing when an ancestor is content-sized, which is
        // what a real window measured: 0.0px for one field and 38.4px for another (docs §99).
        // This is the only width here that does not ask anything of an ancestor, and it turns
        // the worst case from an invisible control into a small one.
        style.min_size.width = px(120.).into();
        style.size.height = (window.line_height() * lines.min(MAX_VISIBLE_LINES) as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let composer = self.composer.read(cx);
        let content = composer.display_content();
        let selected_range = composer.selected_range.clone();
        let cursor = composer.cursor_offset();
        let style = window.text_style();

        // What the box actually measures, which nobody has ever read.
        //
        // Three fixes have shipped against a field that renders ~10px wide with its placeholder
        // spilling out the side (§72, §88, and a third diagnosis §92 refuted), and not one of
        // them started from this number. A taffy replay says the collapse needs a content-sized
        // ancestor this tree does not appear to contain; the only way to tell whether real gpui
        // agrees is to look. `prepaint` already receives `bounds` — the measurement was one line
        // away the whole time (docs §97).
        //
        // Both branches speak, because §81 paid three times for the lesson that a component
        // which only reports failure is indistinguishable from one that was never reached. The
        // narrow case warns; setting `MINIME_LAYOUT_DEBUG` reports every field, so "no warning"
        // can be confirmed as "measured and fine" rather than assumed.
        {
            let width = f32::from(bounds.size.width);
            let field = composer.placeholder.clone();
            if width < 40. {
                tracing::warn!(
                    width,
                    field = %field,
                    "a text field was laid out too narrow to use — docs §92"
                );
            } else if std::env::var_os("MINIME_LAYOUT_DEBUG").is_some() {
                tracing::info!(width, field = %field, "text field width");
            }
        }

        let (display_text, text_color) = if content.is_empty() {
            (
                composer.placeholder.clone(),
                rgb(theme::text_muted()).into(),
            )
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        // An IME pre-edit (marked) range is underlined so the user can see what
        // is still being composed — essential for CJK input.
        let runs = if let Some(marked_range) = composer.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        // One shaped line per *visual* row — the researcher's own breaks and the ones the width
        // forced. GPUI's `shape_line` is exactly one line, so both kinds of splitting are ours.
        let mut lines: Vec<(usize, ShapedLine)> = Vec::new();
        {
            let mut wrapper = cx.text_system().line_wrapper(style.font(), font_size);
            row_ranges(&display_text, wrap_at(&mut wrapper, wrappable(bounds.size.width)))
        }
        .into_iter()
        .for_each(|range| {
            let shaped = window.text_system().shape_line(
                SharedString::from(display_text[range.clone()].to_string()),
                font_size,
                &runs_for(&runs, range.start, range.end),
                None,
            );
            lines.push((range.start, shaped));
        });

        let position = |target: usize| -> (usize, Pixels) {
            for (row, (start, line)) in lines.iter().enumerate() {
                let end = start + line.len();
                if target > end {
                    continue;
                }
                // **At a wrap, the caret belongs to the row the next character is on.** Row N's
                // end and row N+1's start are the *same* offset when the break was forced by the
                // width rather than typed — a hard newline consumes a byte and these two differ by
                // one. Without the distinction the caret sits at the right-hand end of the row
                // *above* the character it precedes, which is the same "it went off the side" it
                // was supposed to fix (§202).
                if target == end {
                    if let Some((next_start, next_line)) = lines.get(row + 1) {
                        if *next_start == end {
                            return (row + 1, next_line.x_for_index(0));
                        }
                    }
                }
                return (row, line.x_for_index(target.saturating_sub(*start)));
            }
            match lines.last() {
                Some((start, line)) => (
                    lines.len().saturating_sub(1),
                    line.x_for_index(target.saturating_sub(*start)),
                ),
                None => (0, px(0.)),
            }
        };

        // Which row sits at the top of the box.
        //
        // The field grows to [`MAX_VISIBLE_LINES`] and then stops, so beyond that the *window*
        // over the rows has to follow the caret or the box would show the first eight lines of
        // something being typed on the twentieth. Clamped to the last full screenful, so the
        // final row never floats above empty space.
        //
        // Read from `position` rather than worked out again, so the row the box scrolls to and the
        // row the caret is drawn on cannot disagree at a wrap boundary.
        let caret_row = position(cursor).0;
        let first_row = if lines.len() <= MAX_VISIBLE_LINES {
            0
        } else {
            caret_row
                .saturating_sub(MAX_VISIBLE_LINES - 1)
                .min(lines.len() - MAX_VISIBLE_LINES)
        };

        let row_top = |row: usize| bounds.top() + line_height * (row as f32 - first_row as f32);

        let (selection, cursor) = if selected_range.is_empty() {
            let (row, x) = position(cursor);
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + x, row_top(row)),
                        gpui::size(px(2.), line_height),
                    ),
                    rgb(theme::accent()),
                )),
            )
        } else {
            // A rectangle per covered line. One box from start to end would paint over
            // the text on every line in between.
            let (first_row, start_x) = position(selected_range.start);
            let (last_row, end_x) = position(selected_range.end);
            let mut quads = Vec::new();
            for row in first_row..=last_row {
                let (_, line) = &lines[row.min(lines.len().saturating_sub(1))];
                let from = if row == first_row { start_x } else { px(0.) };
                let to = if row == last_row {
                    end_x
                } else {
                    // To the end of the text on that line, plus a little, so a selected
                    // line break is visible rather than invisible.
                    line.width + px(4.)
                };
                quads.push(fill(
                    Bounds::from_corners(
                        point(bounds.left() + from, row_top(row)),
                        point(bounds.left() + to, row_top(row) + line_height),
                    ),
                    rgba(0xe8703a40),
                ));
            }
            (quads, None)
        };

        PrepaintState {
            lines,
            cursor,
            selection,
            first_row,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.composer.read(cx).focus_handle.clone();
        // Route OS keyboard/IME input at these bounds to the composer entity.
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.composer.clone()),
            cx,
        );
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }
        let line_height = window.line_height();
        // Clipped to the box it belongs to.
        //
        // A separate defect from the width, and conflated with it three times: the text is
        // shaped with no wrap width and painted at `bounds.origin`, so a field that measures
        // wrong does not truncate — it draws its content straight across whatever is beside it.
        // That is why a 10px box appeared to contain a full-length placeholder. With the mask, a
        // future layout mistake becomes "text visibly cut off", which is a bug report someone
        // can act on, instead of "text floating over unrelated UI" (docs §97).
        let first_row = prepaint.first_row;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            for (row, (_, line)) in prepaint.lines.iter().enumerate() {
                let origin = point(
                    bounds.origin.x,
                    bounds.origin.y + line_height * (row as f32 - first_row as f32),
                );
                line.paint(origin, line_height, window, cx).unwrap();
            }
        });

        // Caret only when focused. (Nested `if`s, not a let-chain: those need
        // edition 2024 and this crate is on 2021.)
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.composer.update(cx, |composer, _cx| {
            composer.last_layout = std::mem::take(&mut prepaint.lines);
            composer.last_bounds = Some(bounds);
            composer.line_height = line_height;
            // Hit-testing and the IME rect both convert between rows and screen y, and a scrolled
            // field that forgot how far it had scrolled would put the caret eight lines from
            // where the click was.
            composer.first_row = first_row;
        });
    }
}

impl Render for Composer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_grow()
            .min_w_0()
            .key_context("Composer")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(div().flex_grow().min_w_0().child(ComposerElement {
                composer: cx.entity(),
            }))
    }
}

impl Focusable for Composer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: gpui::Font {
                family: "x".into(),
                features: Default::default(),
                fallbacks: None,
                weight: Default::default(),
                style: Default::default(),
            },
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn runs_are_sliced_to_each_line() {
        // "ab\ncd" with an IME marking "b\nc": a run before, the marked run, a run after.
        // Each line needs its own view of these or the underline lands on the wrong
        // characters — the reason this splitting exists at all.
        let runs = vec![run(1), run(3), run(1)];

        // First line is bytes 0..2: one whole run, one byte of the marked one.
        assert_eq!(
            runs_for(&runs, 0, 2)
                .iter()
                .map(|r| r.len)
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        // Second line is bytes 3..5: the tail of the marked run, then the last run.
        assert_eq!(
            runs_for(&runs, 3, 5)
                .iter()
                .map(|r| r.len)
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
        // A range past the end contributes nothing — never a zero-length run, which
        // `shape_line` treats as malformed.
        assert!(runs_for(&runs, 9, 12).is_empty());
        assert!(runs_for(&runs, 2, 2).is_empty());
    }

    /// Break every `every` bytes, standing in for what the text system measures.
    ///
    /// The pixel measurement is gpui's and needs a window; what this module can get wrong on its
    /// own is turning break points into ranges over the whole string, and that is what is tested.
    fn every(every: usize) -> impl FnMut(&str) -> Vec<usize> {
        move |segment: &str| (every..segment.len()).step_by(every).collect()
    }

    #[test]
    fn a_line_nobody_broke_still_becomes_rows() {
        // The defect: one long line was one row, shaped past the right edge and clipped, with the
        // caret out there with it (§200).
        let rows = row_ranges("abcdefghij", every(4));
        assert_eq!(rows, vec![0..4, 4..8, 8..10]);

        // Offsets stay in the *whole string's* terms across a hard break, newline included, or
        // the caret and the selection would be placed against the wrong text.
        let rows = row_ranges("abcdef\nghi", every(4));
        assert_eq!(rows, vec![0..4, 4..6, 7..10]);

        // Nothing to wrap: unchanged from §55, which is what the first frame and every short
        // prompt get.
        assert_eq!(row_ranges("one\ntwo", |_| Vec::new()), vec![0..3, 4..7]);
        assert_eq!(row_ranges("", |_| Vec::new()), vec![0..0]);
    }

    #[test]
    fn a_wrap_point_at_the_edge_does_not_add_an_empty_row() {
        // A break exactly at the end of the text, and one at the start: both would produce an
        // empty range, which draws as a blank line the researcher did not type and — worse —
        // moves every row below it down by one.
        assert_eq!(row_ranges("abcd", |_| vec![4]), vec![0..4]);
        assert_eq!(row_ranges("abcd", |_| vec![0]), vec![0..4]);
        assert_eq!(row_ranges("abcd", |_| vec![2, 2]), vec![0..2, 2..4]);
    }

    #[test]
    fn the_field_grows_with_its_lines_but_stops() {
        // A pasted script makes the composer taller, up to a point, and then stops rather than
        // eating the transcript. Past the cap the *window* over the rows follows the caret —
        // checked here because the element's own layout needs a Window.
        let rows = |text: &str, breaks: usize| row_ranges(text, every(breaks)).len();
        assert_eq!(rows("one line", 40), 1);
        assert_eq!(rows("one\ntwo\nthree", 40), 3);
        assert!(rows(&"x".repeat(400), 10) > MAX_VISIBLE_LINES);

        // The scroll rule from `prepaint`, stated where it can be checked: the caret's row is
        // always on screen, and the last screenful never floats above empty space.
        let first_row = |caret: usize, total: usize| {
            if total <= MAX_VISIBLE_LINES {
                0
            } else {
                caret
                    .saturating_sub(MAX_VISIBLE_LINES - 1)
                    .min(total - MAX_VISIBLE_LINES)
            }
        };
        assert_eq!(first_row(0, 3), 0, "a short field never scrolls");
        assert_eq!(first_row(0, 20), 0, "nor does one whose caret is at the top");
        assert_eq!(first_row(9, 20), 2, "the caret's row is the last one shown");
        assert_eq!(first_row(19, 20), 12, "and the bottom stops at the bottom");
    }

    #[test]
    fn the_caret_has_room_at_the_right_edge() {
        // Wrapping at the full box width leaves the caret — painted *after* the last glyph —
        // sitting on the border, which is what "the bar is beyond the text" was (§202).
        assert_eq!(wrappable(px(800.)), Some(px(796.)));
        // And a box too narrow to hold anything says so rather than asking for a wrap at zero,
        // which would break after every character.
        assert_eq!(wrappable(px(4.)), None);
        assert_eq!(wrappable(px(0.)), None);
    }

    /// Which row the caret is on, extracted from `prepaint` so the rule can be checked.
    ///
    /// `rows` is `(start, len)` per row — a shaped line's two load-bearing numbers.
    fn caret_row(rows: &[(usize, usize)], target: usize) -> usize {
        for (row, (start, len)) in rows.iter().enumerate() {
            let end = start + len;
            if target > end {
                continue;
            }
            if target == end {
                if let Some((next_start, _)) = rows.get(row + 1) {
                    if *next_start == end {
                        return row + 1;
                    }
                }
            }
            return row;
        }
        rows.len().saturating_sub(1)
    }

    #[test]
    fn the_caret_at_a_wrap_belongs_to_the_row_below() {
        // "hello " / "world": a *wrap*, so row 1 starts at the same offset row 0 ends at.
        let wrapped = [(0usize, 6usize), (6, 5)];
        assert_eq!(caret_row(&wrapped, 3), 0, "inside the first row");
        assert_eq!(
            caret_row(&wrapped, 6),
            1,
            "at the break, with the character it precedes"
        );
        assert_eq!(caret_row(&wrapped, 11), 1, "at the end of the text, and stays");

        // "hello" \n "world": a typed break, which consumes a byte — so the caret before the
        // newline stays on the row above it, where the person pressed the key.
        let typed = [(0usize, 5usize), (6, 5)];
        assert_eq!(caret_row(&typed, 5), 0, "before the newline, not after it");
        assert_eq!(caret_row(&typed, 6), 1);
    }
}
