//! A folha de permissões: o que está à espera de resposta, e a resposta.
//!
//! O `always` é o único sítio da app onde uma resposta se torna regra, e a
//! regra é derivada do pedido pendente e nunca do que a janela mandou — senão
//! a janela podia alargar aquilo a que está a responder.

use std::sync::Arc;

use tauri::State;

use harness_app::approvals::PendingApproval;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[tauri::command]
pub async fn approvals_pending(ws: Shared<'_>) -> Result<Vec<PendingApproval>, String> {
    Ok(ws.router.pending_list())
}

/// Answer a permission request. `always` records a standing allowance — scoped
/// to this call, not to the bare tool name: agreeing to one `git push` must
/// never authorise every shell command. Some calls cannot be scoped safely (a
/// chained shell command), and those are allowed once and asked about again.
#[tauri::command]
pub async fn respond_approval(
    request_id: String,
    allow: bool,
    always: bool,
    ws: Shared<'_>,
) -> Result<Option<String>, String> {
    let mut recorded = None;
    if allow && always {
        // The tool and input come from the pending request, never from the
        // caller, so the UI cannot widen what it is answering about.
        if let Some(pending) = ws
            .router
            .pending_list()
            .into_iter()
            .find(|p| p.request_id == request_id)
        {
            let mut settings = ws.settings();
            let rule = settings.allow_always(&pending.tool, &pending.input);
            match rule {
                Some(rule) => {
                    ws.set_settings(settings)?;
                    recorded = Some(rule.label());
                }
                // Nothing safe to remember: allow this one and keep asking.
                None => recorded = None,
            }
        }
    }
    ws.router.resolve(&request_id, allow)?;
    Ok(recorded)
}
