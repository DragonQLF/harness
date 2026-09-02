//! As ferramentas que mexem num quadro: os cartões, os seus estados, e o diff
//! que um deles deixou.
//!
//! Todas chegam já com o `ProjectRuntime` resolvido — quem escolhe o projecto é
//! o `mod.rs`, uma vez, porque a regra de qual é (o nomeado, senão o afixado)
//! é a mesma para todas e escrita duas vezes seria duas regras.

use std::path::Path;
use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, Status};
use harness_ports::{GitPort, ToolCall, ToolReply, WorktreePath};

use super::text;
use crate::workspace::{ProjectRuntime, Workspace};

fn column(raw: &str) -> Option<Status> {
    Some(match raw {
        "later" | "backlog" => Status::Backlog,
        "ready" => Status::Ready,
        "running" | "working" => Status::Running,
        "review" => Status::Review,
        "done" => Status::Done,
        _ => return None,
    })
}

/// Where a new card is born, as the two flags `create_card_inner` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Birth {
    start: bool,
    ready: bool,
}

/// Read the column a card was asked for. The Director asked for `later` and
/// got `ready`, then had to call `move_card` — two permission sheets for one
/// action, twice in one session. `None` for a column that cannot hold a new
/// card; an absent column keeps the old default, which was `ready`.
fn birth(asked: Option<&str>, start_flag: bool) -> Option<Birth> {
    let landing = match asked {
        None => Status::Ready,
        Some(raw) => match column(raw) {
            Some(s @ (Status::Backlog | Status::Ready | Status::Running)) => s,
            // Review and Done are where a run leaves a card, never where one
            // starts: a card born there has no run and no diff behind it.
            _ => return None,
        },
    };
    let start = start_flag || landing == Status::Running;
    Some(Birth {
        start,
        ready: start || landing == Status::Ready,
    })
}

pub(super) async fn create_card(
    ws: &Arc<Workspace>,
    project_id: &str,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(title) = text(&call.input, "title") else {
            return ToolReply::refused("create_card needs a title");
        };
        let agent = text(&call.input, "agent_id")
            .unwrap_or_else(|| harness_app::agents::DEFAULT_WORKER.to_string());
        let Some(profile) = ws.agent_exact(&agent).await else {
            return ToolReply::refused(format!(
                "there is no agent called {agent}. The crew is configured on the Agents screen."
            ));
        };
        if !profile.can_take_work() {
            return ToolReply::refused(format!(
                "{} cannot be given cards{}",
                profile.name,
                if profile.paused {
                    " — it is paused"
                } else {
                    " — task execution is turned off on its profile"
                }
            ));
        }
        let flag = call
            .input
            .get("start")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let Some(Birth { start, ready }) =
            birth(text(&call.input, "column").as_deref(), flag)
        else {
            return ToolReply::refused(
                "create_card takes column as later, ready or running. A card cannot be born \
                 in review or done — those are where a run leaves it.",
            );
        };
        // A proposal is a finding and a card is a unit of reviewable work, so
        // the honest ratio is many-to-one: four accepted findings that turn out
        // to be the same defect belong in one diff. `proposal_id` stays as the
        // singular spelling of the same thing.
        let wanted = proposal_ids(call);
        // Checked BEFORE the card exists. Linking after creation would leave a
        // card behind on a typo, and the caller would have to clean up a board
        // to fix an inbox.
        if let Err(refusal) = unlinkable(ws, &wanted) {
            return ToolReply::refused(refusal);
        }
        match crate::commands::board::create_card_inner(
            ws, project_id, &title, &agent, start, ready,
        )
        .await
        {
            Ok(created) => {
                // Closing the loop on an accepted proposal: this is the
                // only place the card he was given permission to make can
                // be tied back to the permission, and without the tie the
                // acceptance would be raised at him for ever.
                let acted: Vec<String> = wanted
                    .iter()
                    .filter_map(|id| {
                        ws.record_proposal_action(id, project_id, created.card_id.as_str())
                            .map(|p| p.id)
                    })
                    .collect();
                ToolReply::ok(format!(
                    "created {} for {agent}{where_}{}{}",
                    created.card_id,
                    if created.run_id.is_some() {
                        " and started it"
                    } else if ready {
                        ", ready to start"
                    } else {
                        ", in later"
                    },
                    match acted.len() {
                        0 => String::new(),
                        // Say what actually happened: the card is what stops
                        // the acceptance being raised, and the card has not
                        // been reviewed yet. Calling it "carried out" here
                        // would be the app claiming an effect it has not had.
                        1 => format!(
                            " — carrying out the accepted proposal {}, which stops being \
                             raised now that a card holds it",
                            acted[0]
                        ),
                        n => format!(
                            " — carrying out {n} accepted proposals ({}), which stop being \
                             raised now that a card holds them",
                            acted.join(", ")
                        ),
                    }
                ))
            }
            Err(e) => ToolReply::refused(e),
        }
}

/// Every proposal id the call names, singular spelling first, deduplicated and
/// in the order given. Both spellings are accepted so nothing that already
/// passes `proposal_id` breaks.
fn proposal_ids(call: &ToolCall) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: String| {
        let id = id.trim().to_string();
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    };
    if let Some(one) = text(&call.input, "proposal_id") {
        push(one);
    }
    if let Some(many) = call.input.get("proposal_ids").and_then(|v| v.as_array()) {
        for v in many {
            if let Some(s) = v.as_str() {
                push(s.to_string());
            }
        }
    }
    out
}

/// Refuse by name rather than dropping in silence: an id that cannot be linked
/// is either a typo or a proposal already answered by another card, and both
/// are worth stopping for. Says which one and why.
fn unlinkable(ws: &crate::workspace::Workspace, wanted: &[String]) -> Result<(), String> {
    unlinkable_in(&ws.inbox().proposals, wanted)
}

/// The decision itself, over plain data so it can be tested without a
/// workspace, a disk or a board.
fn unlinkable_in(
    proposals: &[harness_app::inbox::Proposal],
    wanted: &[String],
) -> Result<(), String> {
    if wanted.is_empty() {
        return Ok(());
    }
    for id in wanted {
        match proposals.iter().find(|p| &p.id == id) {
            None => return Err(format!("there is no proposal {id} in the inbox")),
            Some(p) if p.status != harness_app::inbox::ProposalStatus::Accepted => {
                return Err(format!(
                    "proposal {id} is {:?}, not accepted — only an accepted proposal can be \
                     carried out by a card",
                    p.status
                ));
            }
            Some(p) => {
                if let Some(held) = &p.card_id {
                    return Err(format!(
                        "proposal {id} is already carried out by card {held}"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Dizer-lhe mais uma coisa a meio do turno. Não interrompe: entra na fila do
/// run e o modelo lê-a na leitura seguinte (#103).
pub(super) async fn message_agent(
    runtime: &ProjectRuntime,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("that needs a card_id");
        };
        let Some(said) = text(&call.input, "text") else {
            return ToolReply::refused("that needs something to say");
        };
        match runtime.engine.message_run(CardId::new(card_id.clone()), said).await {
            Ok(_) => ToolReply::ok(format!(
                "said to {card_id}{where_}; it reads that at its next read, without stopping"
            )),
            Err(e) => ToolReply::refused(e),
        }
}

/// Corrigir um cartão mal escrito, em vez de o deitar fora e perder o id, o
/// histórico, a sessão e as dependências que lhe apontam. O domínio recusa-o
/// depois de o cartão ter corrido.
pub(super) async fn edit_card(
    runtime: &ProjectRuntime,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("edit_card needs a card_id");
        };
        let Some(title) = text(&call.input, "title") else {
            return ToolReply::refused("edit_card needs the title it should have instead");
        };
        match runtime
            .engine
            .execute(Command::EditCard {
                card_id: CardId::new(card_id.clone()),
                title: title.clone(),
            })
            .await
        {
            Ok(_) => ToolReply::ok(format!("{card_id} now reads \"{title}\"{where_}")),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn move_card(
    ws: &Arc<Workspace>,
    runtime: &ProjectRuntime,
    project_id: &str,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("move_card needs a card_id");
        };
        let Some(to) = text(&call.input, "to").and_then(|t| column(&t)) else {
            return ToolReply::refused(
                "move_card needs `to` as one of: later, ready, running, review, done",
            );
        };
        // Moving into `running` means starting a run, not just relabelling.
        if to == Status::Running {
            return match crate::commands::board::start_run_inner(
                ws,
                project_id,
                CardId::new(card_id.clone()),
                None,
            )
            .await
            {
                Ok(_) => ToolReply::ok(format!("{card_id} is running now{where_}")),
                Err(e) => ToolReply::refused(e),
            };
        }
        match runtime
            .engine
            .execute(Command::MoveCard {
                card_id: CardId::new(card_id.clone()),
                to,
            })
            .await
        {
            Ok(_) => ToolReply::ok(format!("moved {card_id} to {to:?}{where_}")),
            Err(e) => ToolReply::refused(format!(
                "that move is not allowed: {e}. The board only permits the steps in order, \
                 or an override with a reason."
            )),
        }
}

/// O veredicto da revisão é qual destas duas ele chama — não há um segundo
/// relato em JSON ao lado (#102).
pub(super) async fn review_card(
    ws: &Arc<Workspace>,
    runtime: &ProjectRuntime,
    project_id: &str,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("that needs a card_id");
        };
        let reason = text(&call.input, "reason").unwrap_or_default();
        let approving = call.name == "approve_card";
        if !approving && reason.is_empty() {
            return ToolReply::refused("sending a card back needs a reason the agent can act on");
        }
        // The same gate the operator's own Approve goes through. The Director
        // is not a second authority on whether a red build is acceptable — and
        // it cannot see the game it just approved, which is exactly how six
        // defects shipped green.
        if approving {
            if let Err(why) = crate::commands::board::refuse_approval_over_red_checks(
                &ws.paths,
                project_id,
                &card_id,
            ) {
                return ToolReply::refused(why);
            }
        }
        let cmd = if approving {
            Command::ApproveCard {
                card_id: CardId::new(card_id.clone()),
                by: Actor::Director,
                reason: reason.clone(),
                hunks: Vec::new(),
            }
        } else {
            Command::RejectCard {
                card_id: CardId::new(card_id.clone()),
                reason: reason.clone(),
                by: Actor::Director,
                hunks: Vec::new(),
            }
        };
        match runtime.engine.execute(cmd).await {
            Ok(_) => ToolReply::ok(if approving {
                format!("approved {card_id}{where_}")
            } else {
                format!("sent {card_id} back to ready{where_}")
            }),
            Err(e) => ToolReply::refused(format!("that card cannot be reviewed now: {e}")),
        }
}

pub(super) async fn delete_card(runtime: &ProjectRuntime, call: &ToolCall) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("delete_card needs a card_id");
        };
        let reason = text(&call.input, "reason").unwrap_or_else(|| "deleted".to_string());
        match runtime
            .engine
            .execute(Command::DiscardCard {
                card_id: CardId::new(card_id.clone()),
                reason,
            })
            .await
        {
            Ok(_) => ToolReply::ok(format!("deleted {card_id} and removed its worktree")),
            Err(e) => ToolReply::refused(format!(
                "cannot delete {card_id}: {e}. A running card has to be stopped first."
            )),
        }
}

pub(super) async fn read_diff(runtime: &ProjectRuntime, call: &ToolCall) -> ToolReply {
        let Some(card_id) = text(&call.input, "card_id") else {
            return ToolReply::refused("read_diff needs a card_id");
        };
        let snap = match runtime.engine.snapshot().await {
            Ok(s) => s,
            Err(e) => return ToolReply::refused(e),
        };
        let Some(session) = snap.sessions.iter().find(|s| s.card_id.as_str() == card_id) else {
            return ToolReply::refused(format!(
                "{card_id} has no worktree, so nothing has been written for it yet"
            ));
        };
        let git = Arc::clone(&runtime.git);
        let base = runtime.project.base_branch.clone();
        let against = base.clone();
        let worktree = WorktreePath(Path::new(&session.worktree).to_path_buf());
        let diff = tauri::async_runtime::spawn_blocking(move || {
            git.diff_summary(&worktree, &against)
        })
        .await;
        match diff {
            Ok(Ok(text)) if !text.trim().is_empty() => ToolReply::ok(text),
            Ok(Ok(_)) => ToolReply::ok(format!("{card_id} changed nothing against {base}")),
            Ok(Err(e)) => ToolReply::refused(format!("could not read that diff: {e}")),
            Err(e) => ToolReply::refused(format!("could not read that diff: {e}")),
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_app::inbox::{Proposal, ProposalStatus};

    fn call_with(input: serde_json::Value) -> ToolCall {
        ToolCall { name: "create_card".into(), input }
    }

    fn proposal(id: &str, status: ProposalStatus, card: Option<&str>) -> Proposal {
        Proposal {
            id: id.into(),
            created_ms: 0,
            title: String::new(),
            observation: String::new(),
            proposal: String::new(),
            status,
            card_id: card.map(|c| c.to_string()),
            project_id: None,
        }
    }

    /// Both spellings mean the same thing, and naming one twice is one link,
    /// not two.
    #[test]
    fn both_spellings_are_gathered_in_order_without_repeats() {
        let call = call_with(serde_json::json!({
            "proposal_id": "prp_one",
            "proposal_ids": ["prp_two", "prp_one", "  ", "prp_three"],
        }));
        assert_eq!(proposal_ids(&call), vec!["prp_one", "prp_two", "prp_three"]);
        // The singular alone still behaves exactly as it always did.
        let old = call_with(serde_json::json!({"proposal_id": "prp_only"}));
        assert_eq!(proposal_ids(&old), vec!["prp_only"]);
        // And a card that carries no proposal names none.
        assert!(proposal_ids(&call_with(serde_json::json!({}))).is_empty());
    }

    /// Silence is the failure being fixed: a bad id must be refused BY NAME,
    /// never dropped, because a dropped link leaves the operator's acceptance
    /// being raised for ever with nothing to show why.
    #[test]
    fn a_proposal_that_cannot_be_linked_is_refused_by_name() {
        let inbox = vec![
            proposal("prp_open", ProposalStatus::Accepted, None),
            proposal("prp_taken", ProposalStatus::Accepted, Some("c_1fab")),
            proposal("prp_no", ProposalStatus::Dismissed, None),
        ];
        assert!(unlinkable_in(&inbox, &[]).is_ok());
        assert!(unlinkable_in(&inbox, &["prp_open".into()]).is_ok());

        let missing = unlinkable_in(&inbox, &["prp_ghost".into()]).unwrap_err();
        assert!(missing.contains("prp_ghost"), "{missing}");

        let taken = unlinkable_in(&inbox, &["prp_taken".into()]).unwrap_err();
        assert!(taken.contains("prp_taken") && taken.contains("c_1fab"), "{taken}");

        let dismissed = unlinkable_in(&inbox, &["prp_no".into()]).unwrap_err();
        assert!(dismissed.contains("prp_no"), "{dismissed}");

        // One bad id in a good list still refuses the whole call, so a card is
        // never created against a half-linked set.
        let mixed =
            unlinkable_in(&inbox, &["prp_open".into(), "prp_ghost".into()]).unwrap_err();
        assert!(mixed.contains("prp_ghost"), "{mixed}");
    }

    #[test]
    fn column_names_match_the_board() {
        assert_eq!(column("later"), Some(Status::Backlog));
        assert_eq!(column("backlog"), Some(Status::Backlog));
        assert_eq!(column("working"), Some(Status::Running));
        assert_eq!(column("done"), Some(Status::Done));
        assert_eq!(column("sideways"), None);
    }

    /// The two-approvals bug: a card asked for in `later` must be born there,
    /// with no `move_card` behind it.
    #[test]
    fn a_card_is_born_in_the_column_that_was_asked_for() {
        assert_eq!(birth(Some("later"), false), Some(Birth { start: false, ready: false }));
        assert_eq!(birth(Some("backlog"), false), Some(Birth { start: false, ready: false }));
        assert_eq!(birth(Some("ready"), false), Some(Birth { start: false, ready: true }));
        // `running` is a run starting, not a label.
        assert_eq!(birth(Some("running"), false), Some(Birth { start: true, ready: true }));
        // No column at all keeps what every caller got before this existed.
        assert_eq!(birth(None, false), Some(Birth { start: false, ready: true }));
        // The old `start` flag still wins over any column that is not running.
        assert_eq!(birth(Some("later"), true), Some(Birth { start: true, ready: true }));
        // Review and Done are where a run leaves a card, not where one starts.
        assert_eq!(birth(Some("review"), false), None);
        assert_eq!(birth(Some("done"), false), None);
        assert_eq!(birth(Some("sideways"), false), None);
    }
}

/// Dizer o que tem de estar feito antes de um cartão poder começar.
///
/// O `depends_on` existe no domínio desde sempre e o `SetDependencies` também;
/// o Director é que não tinha por onde lhes chamar. Sem isto, planear cinco
/// cartões deixava o operador a ser o escalonador — a arrancar cada um na
/// ordem certa à mão, que é exactamente o "explicar outra vez a seguir a cada
/// passo" de que ele se queixou.
pub(super) async fn set_dependencies(
    runtime: &ProjectRuntime,
    where_: &str,
    call: &ToolCall,
) -> ToolReply {
    let Some(card_id) = text(&call.input, "card_id") else {
        return ToolReply::refused("that needs a card_id");
    };
    let depends_on: Vec<String> = call
        .input
        .get("depends_on")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // Um cartão à espera de si próprio nunca arranca, e o quadro não teria como
    // o dizer depois — recusa-se aqui, onde ainda há a quem dizê-lo.
    if depends_on.iter().any(|d| d == &card_id) {
        return ToolReply::refused(format!(
            "{card_id} cannot depend on itself; it would never be startable"
        ));
    }

    match runtime
        .engine
        .execute(Command::SetDependencies {
            card_id: CardId::new(card_id.clone()),
            depends_on: depends_on.iter().cloned().map(CardId::new).collect(),
        })
        .await
    {
        Ok(_) if depends_on.is_empty() => {
            ToolReply::ok(format!("{card_id} now depends on nothing{where_}"))
        }
        Ok(_) => ToolReply::ok(format!(
            "{card_id} waits for {}{where_}",
            depends_on.join(", ")
        )),
        Err(e) => ToolReply::refused(format!("could not set those dependencies: {e}")),
    }
}
