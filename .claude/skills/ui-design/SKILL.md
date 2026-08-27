---
name: ui-design
description: Build or edit mini-me-desktop's GPUI UI (crates/app/src) using its component library and theme roles. Use whenever you need to create a new element.
---

# mini-me-desktop UI

GPUI ships primitives, not components. This app's `ui.rs` and `theme.rs` exist
because the same handful of layout bugs kept getting rewritten at each call
site. **Reach for the existing pieces before writing raw `div()` chains.**

## Reference these files directly

- [`crates/app/src/ui.rs`](../../../crates/app/src/ui.rs) — the reusable
  controls and shared conventions (`Tone`, `Size`, `IconSize`) this app is
  built from. Skim what's actually defined there before building anything —
  this list grows over time, so don't rely on a fixed set of names.
- [`crates/app/src/theme.rs`](../../../crates/app/src/theme.rs) — the palette,
  as roles (`theme::accent()`, `theme::text_muted()`, `theme::border()`, …),
  live-swappable at runtime.
- [`crates/app/src/components/common.rs`](../../../crates/app/src/components/common.rs)
  — small shared helpers (`app_icon`, `section_label`) that don't rise to a
  full `ui.rs` type yet.

## Building a control

1. **Check `ui.rs` first.** Whatever shape you're about to hand-build — a
   bordered clickable, body text in a flex row, a floating dialog, an
   icon-only or icon+label button, a selectable list row — look for a type
   that already matches before writing a raw `div()` chain.
2. **Colours always come from `theme::*()`, never a literal hex.** These are
   atomic-backed getters, not consts — hardcoding a colour is invisible to
   theme switching and to the WCAG-AA tests in `theme.rs`.
3. **Tone communicates intent, not decoration.** `Tone::Accent` means "the
   action this pane exists for" — reserve it for the one actionable thing.
   Everything else is `Tone::Quiet` (the default). `Tone::Danger` is for a
   confirmed, irreversible action only.
4. **Icon sizing goes through `IconSize`** (`ui.rs`): `IconSize::Small.px()` /
   `Medium` / `Large`, not a magic `f32` literal at the call site.
5. If nothing in `ui.rs` fits, look at whether it's closer to a
   **borderless clickable row** (sidebar entries, menu rows, gallery cards) —
   those are intentionally left as one-off rows rather than forced into a
   bordered-button type, since a row with its own layout wearing a button's
   shape is how the component set drifts. Match the nearest existing row
   before inventing a new pattern, and only add a new type to `ui.rs` when
   the shape genuinely recurs.

## Design Guide

This is the Design Guide you should follow when building the UI. It will tell you which colors, size, 
spacing and components to choose when making the element. If uncertain, you should choose the default
option, defined by the (default) text. 

1. **Colors**:
  - Even though there are many colors in the theme, you should use only the following colors for your elements:
    - `theme::background()` for the main background
    - `theme::surface()` for background surfaces
    - `theme::accent()` for primary actions' text/outline
    - `theme::accent_soft()` mainly for primary button's backgrounds
    - `theme::text()` for main text
    - `theme::text_muted()` for secondary text
    - `theme::border()` for borders and dividers

2. **Text Sizes**:
  - You should only use the following text sizes for your elements:
    - `text_sm()` for base text (default)
    - `text_base()` for subtitles/secondary text 
    - `text_2xl()` for titles/large text (barely used)

3. **Icon Sizes**:
  - You should only use the following icon sizes for your elements:
    - `IconSize::Small` for small icons
    - `IconSize::Medium` for medium icons (default)
    - `IconSize::Large` for large icons

4. **Spacing**:
  - You should only use the following spacing (margin/padding) for your elements:
    - `p_1p5()`
    - `p_2()` (default)
    - `p_2p5()`
    - `p_3()`
    - `p_4()`
    - `p_6()`

### General Rules

These are the general design rules you should follow when building the UI.

- If you are creating a button with a `div()` chain (Meaning the already existing buttons are not enough for your use case), you should
  use a `py_1p5()` and a `px_2p5()` for the padding. 
- If a button has `theme::background()` as its background color, the hover background color should be `theme::surface()`. 
  If a button has `theme::surface()` as its background color, the hover background color should be `theme::background()`. Likewise,
- If a button has `theme::accent_soft()` as its background color, the background should stay the same on hover.
- A button with `theme::accent_soft()` as its background color should have `theme::accent()` as its text color and outline/border color.
- If a text has an icon next to it, the icon should be on the left side of the text and have a `gap_2()` between the icon and the text. Also, it should have `items_center()` to make sure the icon and text are aligned properly.
- Before placing an element inside an existing container, check what horizontal
  padding/margin its siblings already use and match it, rather than trusting the
  element's own default — a component's own margin can stack with the container's
  padding (use `ui::IconTextButton`'s `.margin(false)` / `.full_width(true)` to avoid
  that), and a heading with no horizontal padding of its own won't line up with rows
  that have theirs.