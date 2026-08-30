//! Os projectos: os que existem, um novo, e o do próprio Relay.
//!
//! Nenhuma destas precisa de um quadro resolvido — são as que respondem antes
//! de haver projecto sobre que agir, que é precisamente porque estão à parte.

use std::path::Path;
use std::sync::Arc;

use harness_ports::{ToolCall, ToolReply};

use super::text;
use crate::workspace::Workspace;

pub(super) async fn list_projects(ws: &Arc<Workspace>) -> ToolReply {
        let projects = ws.projects().await;
        if projects.is_empty() {
            return ToolReply::ok(
                "There are no projects yet. create_project makes one (a git repository with a \
                 board); most questions do not need one.",
            );
        }
        let mut out = String::new();
        for project in projects {
            let live = Path::new(&project.path).is_dir();
            out.push_str(&format!(
                "- {} (id {}) at {}{}{}\n",
                project.name,
                project.id,
                project.path,
                if project.paused { " — paused" } else { "" },
                if live { "" } else { " — folder is missing" }
            ));
        }
        return ToolReply::ok(out);
}

pub(super) async fn create_project(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(name) = text(&call.input, "name") else {
            return ToolReply::refused("create_project needs a name");
        };
        let Some(parent) = text(&call.input, "parent_path") else {
            return ToolReply::refused(
                "create_project needs parent_path: the folder to create the project inside. Ask \
                 the operator where it should live rather than guessing.",
            );
        };
        return match ws.create_project(&parent, &name).await {
            Ok(project) => ToolReply::ok(format!(
                "created {} (id {}) at {} — a git repository with an empty board",
                project.name, project.id, project.path
            )),
            Err(e) => ToolReply::refused(format!("could not create that project: {e}")),
        };
}

/// Pedir para trabalhar no próprio Relay não devia ser respondido com "vá
/// registar um repositório primeiro". O Director faz o que o ecrã de Projectos
/// faria, e o operador vê a mesma folha de permissões de qualquer forma.
pub(super) async fn work_on_relay(ws: &Arc<Workspace>) -> ToolReply {
        match crate::commands::project::ensure_mirror(ws).await {
            Ok(project) => ToolReply::ok(format!(
                "{} is now Relay's own source, at {}. Cards for the app go there,                      including the ones you make from accepted proposals, and read_docs reads its docs/.",
                project.name, project.path
            )),
            Err(e) => ToolReply::refused(e),
        }
}
