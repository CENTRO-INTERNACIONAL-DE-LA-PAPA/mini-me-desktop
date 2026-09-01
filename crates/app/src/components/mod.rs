//! UI rendering split out of main.rs. Each submodule adds `impl Workbench` methods
//! and/or free helper functions that build GPUI element trees.

pub(crate) mod common;
pub(crate) mod sidebar;
pub(crate) mod chat;
pub(crate) mod gallery_view;
pub(crate) mod provenance_view;
pub(crate) mod settings_view;
pub(crate) mod palette_view;
pub(crate) mod modals;
pub(crate) mod status_bar;
