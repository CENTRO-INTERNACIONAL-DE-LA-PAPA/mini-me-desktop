//! The small set of controls the rest of the app is built from.
//!
//! # Why this exists
//!
//! GPUI ships primitives, not components. So every control in this app was written out
//! longhand at its call site — `div().px_3().py_1().border_1().border_color(…)
//! .text_color(…).text_sm().hover(…)` — once per button, **forty-four times**. Each copy is
//! an independent chance to omit one line, and the same omissions kept happening:
//!
//! - `flex_none` / `min_h_0` missing or misplaced: four layout bugs (§40, §48, §51, §53).
//! - Actions placed *inside* a scroll area, so the buttons scrolled away: three (§40, §41, §52).
//! - `.truncate()` on the flex item instead of an inner box, which collapses the element to
//!   its ellipsis and nothing else: §59.
//! - `rounded_md` simply forgotten: §58 rounded the corners of the app and missed **eleven**
//!   bordered buttons, which stayed square for two months in the pane every new user sees.
//!
//! Twice the correct pattern was already a few lines above the mistake. So this is not a
//! knowledge problem and no amount of writing the lesson down has fixed it — it has been
//! written down three times. A value you construct cannot forget a property the way a recipe
//! you retype can, and that is the whole argument for this module.
//!
//! # What it is not
//!
//! It is **not a design system** and does not introduce a single new visual decision. Every
//! colour, padding and radius here was read out of the call sites it replaces, so migrating a
//! button is meant to change nothing on screen — except where a site was missing a property
//! it should always have had.
//!
//! [`Modal`] is the other half, and the other bug class: **actions inside the scroll area**,
//! three times (§40, §41, §52). It is not a style — it is a *shape*. The body scrolls, the
//! header and the actions do not, and because they are separate slots there is no way to put a
//! Save button somewhere it can scroll out of reach.
//!
//! It also deliberately does not try to cover the **twenty-three borderless clickables** —
//! sidebar entries, menu rows, gallery cards. Those are rows with their own layout, not
//! buttons wearing a different hat, and forcing one type over both is how a component set
//! starts growing flags nobody can keep straight.
//!
//! # One file per component
//!
//! Each control below used to be one section of a single `ui.rs`. Split out so that adding or
//! reading one control's shape doesn't require scrolling past every other one — `button.rs` is
//! the whole story of [`Button`], nothing else.

mod button;
mod chip;
mod code_font;
mod dropdown;
mod hint;
mod icon;
mod label;
mod list_row;
mod menu;
mod modal;
mod nav;
mod scrollbar;
mod spinner;
mod toggle;
mod search_bar;

pub use button::{Alignment, Button, ButtonStyle};
pub use chip::Chip;
pub use code_font::code_font;
pub use dropdown::{picker_popup, Dropdown};
pub use hint::Hint;
pub use icon::{Icon, IconSize};
pub use label::{Label, Size};
pub use list_row::ListRow;
pub use menu::{menu_card, Menu, MenuItem};
pub use modal::{actions, Modal};
pub use nav::{nav_rail, NavEntry};
pub use scrollbar::{list_scrollbar, scrollbar};
pub use search_bar::SearchBar;
pub use spinner::Spinner;
pub use toggle::{setting_row, Toggle};

use gpui::{App, ClickEvent, Window};

/// A click handler, in the shape `div().on_click` wants it.
pub(crate) type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
