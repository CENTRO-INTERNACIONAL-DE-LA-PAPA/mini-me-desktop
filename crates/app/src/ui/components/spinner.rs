//! A moving mark, for any wait the researcher is looking at.
//!
//! **Extracted because the app had exactly one and it was in the wrong places.** The status bar
//! span it came from is shown only while a turn streams or a setup fix runs — so the two longest
//! waits in the app, the fifteen seconds of graph construction at startup (§176) and the pause
//! while a conversation opens, were a still window with a sentence on it. A still window reads as
//! a hang, which is the most common reason someone kills an app that was working.
//!
//! Braille frames rather than an SVG or a rotation: no asset to ship, no font dependency, and it
//! reads as motion at any size.

use gpui::{div, prelude::*, rgb, AnimationExt as _, App, ElementId, IntoElement, SharedString, Window};

use crate::theme;

#[derive(IntoElement)]
pub struct Spinner {
    id: SharedString,
    colour: u32,
}

impl Spinner {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            colour: theme::accent(),
        }
    }

    pub fn colour(mut self, colour: u32) -> Self {
        self.colour = colour;
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        const FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];
        div()
            .flex_none()
            .text_color(rgb(self.colour))
            .text_sm()
            .with_animation(
                ElementId::from(self.id),
                gpui::Animation::new(std::time::Duration::from_millis(1200)).repeat(),
                |label, delta| {
                    let frame = (delta * FRAMES.len() as f32) as usize;
                    label.child(FRAMES[frame.min(FRAMES.len() - 1)])
                },
            )
    }
}
