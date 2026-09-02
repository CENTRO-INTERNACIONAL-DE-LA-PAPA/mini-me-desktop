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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

impl IconSize {
    pub const fn px(self) -> f32 {
        match self {
            IconSize::Small => 18.0,
            IconSize::Medium => 20.0,
            IconSize::Large => 22.0,
        }
    }
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
            size: IconSize::Medium,
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
        gpui::svg()
            .path(self.path)
            .w(gpui::px(self.size.px()))
            .h(gpui::px(self.size.px()))
            .flex_none()
            .text_color(rgb(self.colour))
    }
}
