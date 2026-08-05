//! The right-click menu.
//!
//! Selecting text (§62) gave the transcript something worth copying, and then asked the
//! reader to know that `ctrl-c` is how. A right-click is what everyone tries first, so it has
//! to work — and in the composer it has to offer Cut and Paste too, because that is what a
//! text field's menu contains everywhere else.
//!
//! GPUI ships no menu widget, the same way it ships no text input (see [`crate::composer`]).
//! What it does ship is [`gpui::anchored`], which positions a child at a point in the window
//! and keeps it inside the frame, and [`gpui::deferred`], which paints it after everything
//! else so it is not clipped by the pane it opened over. Those two are the whole trick; the
//! rest is a list of rows and the discipline of only offering what will actually work.
//!
//! **Nothing here decides what an item does.** Each entry names one method that already
//! exists and is already reachable by keyboard — the menu is a second door onto the same
//! room, not a second implementation. Every item is greyed out rather than hidden when it
//! would do nothing: a menu whose rows move between right-clicks is a menu you cannot learn.

use gpui::{Pixels, Point};

/// Where the click landed, which decides what the menu can offer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// The conversation. Read-only, so there is nothing to cut or paste into.
    Transcript,
    /// The prompt field.
    Composer,
}

/// One row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Copy,
    Cut,
    Paste,
    SelectAll,
    /// The whole of the last answer, without selecting it first — by far the most common
    /// thing anyone wants out of a transcript.
    CopyLastAnswer,
}

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::Copy => "Copy",
            Item::Cut => "Cut",
            Item::Paste => "Paste",
            Item::SelectAll => "Select all",
            Item::CopyLastAnswer => "Copy last answer",
        }
    }

    /// The keyboard equivalent, shown so the menu teaches the shortcut instead of replacing
    /// it. `ctrl` reads correctly on Windows and Linux; macOS shows the symbol it expects.
    pub fn shortcut(self, target: Target) -> &'static str {
        match (self, target) {
            (Item::Copy, _) => modifier("C"),
            (Item::Cut, _) => modifier("X"),
            (Item::Paste, _) => modifier("V"),
            // Two different bindings on purpose: `ctrl-a` belongs to the field being typed
            // in, so the transcript's "everything" is shifted out of its way (docs §62).
            (Item::SelectAll, Target::Composer) => modifier("A"),
            (Item::SelectAll, Target::Transcript) => modifier("Shift-A"),
            (Item::CopyLastAnswer, _) => "",
        }
    }
}

fn modifier(key: &'static str) -> &'static str {
    // `&'static str` rather than a formatted String so a row costs no allocation per frame.
    // The match is exhaustive over the keys actually used above.
    if cfg!(target_os = "macos") {
        match key {
            "C" => "⌘C",
            "X" => "⌘X",
            "V" => "⌘V",
            "A" => "⌘A",
            "Shift-A" => "⇧⌘A",
            other => other,
        }
    } else {
        match key {
            "C" => "Ctrl+C",
            "X" => "Ctrl+X",
            "V" => "Ctrl+V",
            "A" => "Ctrl+A",
            "Shift-A" => "Ctrl+Shift+A",
            other => other,
        }
    }
}

/// An open menu: where it is, and what it offers.
#[derive(Clone, Debug)]
pub struct ContextMenu {
    /// Window coordinates of the click, which is where the corner of the menu goes.
    pub at: Point<Pixels>,
    pub target: Target,
}

impl ContextMenu {
    pub fn new(at: Point<Pixels>, target: Target) -> Self {
        Self { at, target }
    }

    /// The rows, in order.
    ///
    /// Cut and Paste are absent from the transcript rather than greyed: the conversation is
    /// not editable and never will be, so offering them at all would be a promise about the
    /// wrong thing. Within a field they are always present and greyed when unavailable,
    /// because there they are only *temporarily* out of reach.
    pub fn items(&self) -> &'static [Item] {
        match self.target {
            Target::Transcript => &[Item::Copy, Item::SelectAll, Item::CopyLastAnswer],
            Target::Composer => &[Item::Cut, Item::Copy, Item::Paste, Item::SelectAll],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_only_transcript_is_not_offered_cut_or_paste() {
        let menu = ContextMenu::new(Point::default(), Target::Transcript);
        assert!(!menu.items().contains(&Item::Cut));
        assert!(!menu.items().contains(&Item::Paste));
        assert!(menu.items().contains(&Item::Copy));
    }

    #[test]
    fn a_text_field_is_offered_the_whole_set() {
        let menu = ContextMenu::new(Point::default(), Target::Composer);
        for item in [Item::Cut, Item::Copy, Item::Paste, Item::SelectAll] {
            assert!(menu.items().contains(&item), "{item:?}");
        }
        // Copying the last answer is about the conversation, not about this field.
        assert!(!menu.items().contains(&Item::CopyLastAnswer));
    }

    #[test]
    fn select_all_shows_the_binding_that_actually_applies_there() {
        // The two are deliberately different keys, and a menu that showed the same one in
        // both places would teach the wrong shortcut for the transcript (docs §62).
        let composer = Item::SelectAll.shortcut(Target::Composer);
        let transcript = Item::SelectAll.shortcut(Target::Transcript);
        assert_ne!(composer, transcript);
        assert!(transcript.to_lowercase().contains("shift"), "{transcript}");
        assert!(!composer.to_lowercase().contains("shift"), "{composer}");
    }

    #[test]
    fn every_item_is_labelled_and_only_the_answer_row_lacks_a_shortcut() {
        for target in [Target::Transcript, Target::Composer] {
            for &item in ContextMenu::new(Point::default(), target).items() {
                assert!(!item.label().is_empty(), "{item:?}");
                let shortcut = item.shortcut(target);
                if item == Item::CopyLastAnswer {
                    assert!(shortcut.is_empty(), "{item:?} has no binding to advertise");
                } else {
                    assert!(!shortcut.is_empty(), "{item:?} in {target:?}");
                }
            }
        }
    }
}
