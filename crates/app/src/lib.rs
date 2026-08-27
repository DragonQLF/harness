//! Everything Relay knows how to do that does not need a window.
//!
//! The Tauri shell is a thin adapter over this crate: app-data layout, operator
//! settings, agent profiles, approval routing and the numbers the UI shows all
//! live here, so they can be tested without a webview.

pub mod agents;
pub mod allow;
pub mod approvals;
pub mod conversations;
pub mod checks;
pub mod curator;
pub mod devdocs;
pub mod director;
pub mod inbox;
pub mod insights;
pub mod memory;
pub mod paths;
pub mod projects;
pub mod catalog;
pub mod providers;
pub mod selfreport;
pub mod settings;
