//! One svg icon, sized and tinted.
//!
//! Every control that draws an icon — [`super::Button`], [`super::IconTextButton`] — used to
//! build its own `gpui::svg().path(…).w(px(…)).h(px(…)).text_color(…)` inline. One copy here
//! instead, so the four bugs a hand-copied `div` chain invites (a forgotten `flex_none`, a size
//! that doesn't match [`IconSize`]'s three fixed steps, …) have nowhere to happen.

use gpui::{prelude::*, rgb, App, IntoElement, Window};

use crate::theme;

/// The three icon sizes used app-wide. Not an arbitrary `f32`, so every icon in the app is one
/// of exactly three sizes rather than whatever pixel value a call site guessed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IconSize {
    ExtraSmall,
    #[default]
    Small,
    Medium,
    Large,
}

#[derive(IntoElement)]
pub struct Icon {
    path: &'static str,
    size: IconSize,
    colour: u32,
}

impl Icon {
    pub fn new(path: &'static str) -> Self {
        Self {
            path,
            size: IconSize::default(),
            colour: theme::text_muted(),
        }
    }

    pub fn size(mut self, size: IconSize) -> Self {
        self.size = size;
        self
    }

    pub fn colour(mut self, colour: u32) -> Self {
        self.colour = colour;
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let size_px;
        if self.size == IconSize::ExtraSmall {
            size_px = 12.0;
        } else if self.size == IconSize::Small {
            size_px = 18.0;
        } else if self.size == IconSize::Medium {
            size_px = 20.0;
        } else {
            size_px = 22.0;
        }

        gpui::svg()
            .path(self.path)
            .w(gpui::px(size_px))
            .h(gpui::px(size_px))
            .flex_none()
            .text_color(rgb(self.colour))
    }
}
