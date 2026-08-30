//! Everything Relay knows how to do that does not need a window.
//!
//! The Tauri shell is a thin adapter over this crate: app-data layout, operator
//! settings, agent profiles, approval routing and the numbers the UI shows all
//! live here, so they can be tested without a webview.

pub mod agents;
pub mod browsers;
pub mod allow;
pub mod approvals;
pub mod attachments;
/// A fila de um turno vivo mudou-se para os portos, porque um run de cartão
/// também precisa de uma e o engine não alcança este crate. Reexportada com o
/// nome antigo: quem a importava não tem nada a ver com a mudança.
pub use harness_ports::queue as chatqueue;
pub mod conversations;
pub mod checks;
pub mod code;
pub mod curator;
pub mod devdocs;
pub mod director;
pub mod grants;
pub mod inbox;
pub mod insights;
pub mod memory;
pub mod paths;
pub mod projects;
pub mod runstats;
pub mod catalog;
pub mod mirror;
pub mod providers;
pub mod vocabulary;
pub mod selfreport;
pub mod settings;
