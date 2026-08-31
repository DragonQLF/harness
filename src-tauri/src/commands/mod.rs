//! The IPC surface. The frontend holds no truth: it sends these intents and
//! renders the snapshots and events that come back.
//!
//! Um ficheiro por dono, e o nome do comando é o nome da função — mudar de
//! módulo não muda nada do lado da janela.

pub mod approvals;
pub mod board;
pub mod chat;
pub mod codex;
pub mod code;
pub mod crew;
pub mod inbox;
pub mod project;
pub mod sessions;
pub mod stats;
pub mod system;
pub mod updates;
