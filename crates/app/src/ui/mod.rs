//! Everything on screen: the app's views (this module's direct children, split out of
//! main.rs — each adds `impl Workbench` methods and/or free helper functions that build GPUI
//! element trees) and the shared component library the views are built from ([`components`]).

pub(crate) mod common;
pub(crate) mod sidebar;
pub(crate) mod chat;
pub(crate) mod chat_input;
pub(crate) mod gallery_view;
pub(crate) mod provenance_view;
pub(crate) mod settings_view;
pub(crate) mod palette_view;
pub(crate) mod modals;
pub(crate) mod status_bar;

pub(crate) mod components;
pub use components::*;
