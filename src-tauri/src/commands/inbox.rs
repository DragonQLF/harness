//! A caixa de entrada do operador: propostas que o Director arquivou.
//!
//! Aceitar é **permissão, não é trabalho** (#98): não nasce cartão nenhum,
//! não se toca em quadro nenhum, e o Director recebe a permissão como facto no
//! turno seguinte para agir sobre ela — ou não.

use std::sync::Arc;

use tauri::State;

use harness_app::inbox::Proposal;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[tauri::command]
pub async fn inbox_list(ws: Shared<'_>) -> Result<Vec<Proposal>, String> {
    Ok(ws.inbox().proposals)
}

/// Accept a proposal: permission, not work. Nothing is created and nothing is
/// assigned — the Director is told in his next turn and acts on it himself.
#[tauri::command]
pub async fn inbox_accept(proposal_id: String, ws: Shared<'_>) -> Result<Proposal, String> {
    ws.accept_proposal(&proposal_id)
}

#[tauri::command]
pub async fn inbox_dismiss(proposal_id: String, ws: Shared<'_>) -> Result<Proposal, String> {
    ws.dismiss_proposal(&proposal_id)
}
