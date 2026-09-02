//! Conversations: the operator talking to a profile, and the persistence that
//! lets them pick the thread back up after a restart.
//!
//! This replaces the hand-rolled block that used to live in `workspace.rs`. It
//! sits at workspace level, not in a project engine, for the reason recorded in
//! decision #19: one Director watches every board, and the engine has no notion
//! of a conversation. Nothing here touches the board directly — that goes
//! through `director_tools`, which reuses the same engine commands the UI does.
//!
//! Two things make a conversation survive:
//!
//! - the **native Claude session id**, captured from the SDK and handed back as
//!   `resume_session` on the next message, so the model keeps its own history;
//! - the **run log**, the same `RunLogPort` every run transcript already uses,
//!   so the words are readable even when the native session is gone.
//!
//! There is deliberately no second copy of the transcript: the index says which
//! session and which file, and the file holds the words.

use std::path::PathBuf;
use std::sync::Arc;

use harness_agent_claude::ClaudeCliAgent;
use harness_agent_sidecar::SidecarAgent;
use harness_app::agents;
use harness_app::conversations::Conversation;
use harness_app::director::{self, ChatContext, Speaker};
use harness_domain::{CardId, RunId};
use harness_engine::RunUpdate;
use harness_ports::{
    AgentPort, RunEvent, RunLogLine, RunLogPort, RunOutcome, RunSpec, ToolRunner,
};
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::sidecar;
use crate::workspace::{SwitchingAgent, SystemClock};
use crate::workspace::Workspace;
use harness_ports::ClockPort;

/// Relay tools that only read, navigate, or write into our own layer.
/// Granted outright, because none of them changes a board — see decisions #29
/// and #76. Everything that *changes* a board is absent on purpose, so it
/// reaches the approver. The mirror tools (`self_report`, `read_docs`,
/// `propose_improvement`) count our own history and file proposals; a proposal
/// is not an action on the world until the operator accepts it.
pub(crate) const READ_ONLY_TOOLS: [&str; 7] = [
    "mcp__harness__open_screen",
    "mcp__harness__read_diff",
    "mcp__harness__list_projects",
    "mcp__harness__record_decision",
    "mcp__harness__self_report",
    "mcp__harness__read_docs",
    "mcp__harness__propose_improvement",
];

/// What `chat_queue` did with a message.
///
/// One shape for two outcomes on purpose: the composer does not know whether
/// a turn is still running by the time the call lands, and the answer to that
/// is the backend's. `queue_id` set means it went into a turn already in
/// flight and the model has not read it yet; `None` means there was no turn,
/// so it became one — an ordinary message, drawn as one.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Queued {
    pub queue_id: Option<String>,
    pub conversation: Conversation,
}

/// Publish an event under a conversation's id, in the same shape any run uses
/// — which is what lets the UI keep one typed listener for both.
fn publish_chat(ws: &Workspace, conversation_id: &str, project_id: &str, event: RunEvent) {
    let _ = ws.app_handle().emit(
        crate::events::ENGINE_RUN,
        RunUpdate {
            project_id: project_id.to_string(),
            card_id: CardId::new(conversation_id.to_string()),
            run_id: RunId(conversation_id.to_string()),
            ts_ms: SystemClock.now_millis(),
            event,
        },
    );
}

/// Write an event into the transcript and put it on screen. The two always go
/// together for anything that is not ephemeral, and doing one without the
/// other is how a thread ends up disagreeing with itself after a reload.
fn note_chat(ws: &Workspace, conversation_id: &str, project_id: &str, event: RunEvent) {
    ws.append_chat_line(
        conversation_id,
        RunLogLine {
            ts_ms: SystemClock.now_millis(),
            event: event.clone(),
        },
    );
    publish_chat(ws, conversation_id, project_id, event);
}

/// Say something to a turn that is already running.
///
/// This does not interrupt and does not start a second turn: the message goes
/// into the run's inbox, the sidecar hands it to the SDK's input stream, and
/// the model reads it at its next natural read — so a correction lands *during*
/// the work instead of after it.
///
/// Two paths end in an ordinary turn instead. There may be no turn in flight
/// at all — the composer asked a moment too late — or the turn may end between
/// the look-up and the push. Neither is an error worth showing: the message
/// simply becomes the next turn, which is what the operator meant either way.
pub async fn queue(
    ws: &Arc<Workspace>,
    conversation_id: String,
    text: String,
    attachments: Vec<String>,
    effort: Option<String>,
) -> Result<Queued, String> {
    for file in &attachments {
        if !PathBuf::from(file).is_file() {
            return Err(format!("{file} is not a file on this machine"));
        }
    }
    let message = director::with_attachments(&text, &attachments);
    if message.is_empty() {
        return Err("nothing to send".to_string());
    }

    let queued = match ws.live_chat_turn(&conversation_id).await {
        Some(turn) => turn.queue.push(&message).ok(),
        None => None,
    };
    let Some(queued) = queued else {
        let conversation = send(ws, Some(conversation_id), text, attachments, effort).await?;
        return Ok(Queued {
            queue_id: None,
            conversation,
        });
    };

    // Written down before it is delivered, and marked as exactly that. If
    // Relay dies now, the thread reopens with the message still there and
    // still saying the model never saw it — see the queueing notes in
    // `harness_app::chatqueue`.
    let conversation = ws
        .conversation(&conversation_id)
        .await
        .ok_or_else(|| format!("no conversation {conversation_id}"))?;
    note_chat(
        ws,
        &conversation_id,
        conversation.project_id.as_deref().unwrap_or_default(),
        RunEvent::UserQueued {
            queue_id: queued.id.clone(),
            text: message.clone(),
        },
    );
    let conversation = ws.record_chat_message(&conversation_id, &message).await?;
    Ok(Queued {
        queue_id: Some(queued.id),
        conversation,
    })
}

/// Ao reabrir, ver se algum turno continuou sem nós.
///
/// Um socket em `run-sockets` é um sidecar que sobreviveu — a Relay anterior
/// morreu a meio de um turno e o trabalho seguiu sem ela. Liga-se a cada um,
/// confere-se que é mesmo daquela conversa, e o que ele escreveu entretanto
/// entra no fio a partir de onde a transcrição ficou.
///
/// Nada disto é um erro quando não encontra ninguém: o caso normal de um
/// arranque é não haver socket nenhum, e aí não se levanta nem se escreve nada.
pub async fn reattach_all(ws: &Arc<Workspace>) {
    // Ao contrário: pergunta-se a cada conversa onde estaria o seu socket, em
    // vez de se ler o nome do ficheiro para descobrir de quem é.
    //
    // O nome deixou de o poder dizer. Era a chave do run à letra
    // (`chat-<id>.sock`), o que dava caminhos de 124 bytes que nenhum `bind`
    // aceita — ver `harness_ports::sockets` — e agora é um resumo. Um resumo
    // não se desfaz, portanto a pergunta tinha de mudar de direcção. Fica
    // melhor do que estava: uma conversa apagada já não deixa aqui um socket
    // órfão a ser interrogado.
    let dir = ws.paths.run_sockets_dir();
    for conversation in ws.conversations(true).await {
        let key = format!("chat-{}", conversation.id);
        if !harness_ports::sockets::path_for(&dir, &key).exists() {
            continue;
        }
        if let Err(e) = send_message(
            ws,
            Some(conversation.id.clone()),
            String::new(),
            Vec::new(),
            false,
            None,
            true,
        )
        .await
        {
            eprintln!("could not reattach {}: {e}", conversation.id);
        }
    }
}

/// Send one message. Returns as soon as the run is under way: the answer
/// arrives on the `engine://run` channel, keyed by the conversation id.
pub async fn send(
    ws: &Arc<Workspace>,
    conversation_id: Option<String>,
    text: String,
    attachments: Vec<String>,
    effort: Option<String>,
) -> Result<Conversation, String> {
    send_message(ws, conversation_id, text, attachments, true, effort, false).await
}

/// The next turn, started from inside the one that just ended, carrying what
/// that one never read. Boxed rather than `async`: the recursion is what makes
/// the guarantee — a message never vanishes because a run finished first — and
/// a boxed future is what makes the recursion finite.
fn carried_turn(
    ws: Arc<Workspace>,
    conversation_id: String,
    text: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Conversation, String>> + Send>> {
    Box::pin(async move {
        send_message(&ws, Some(conversation_id), text, Vec::new(), false, None, false).await
    })
}

/// `record` is false for a turn carrying messages that were queued against the
/// run before it: their words are already in the transcript, and writing them
/// again would say them twice.
async fn send_message(
    ws: &Arc<Workspace>,
    conversation_id: Option<String>,
    text: String,
    attachments: Vec<String>,
    record: bool,
    effort: Option<String>,
    attach: bool,
) -> Result<Conversation, String> {
    // A file that is not there is worse than no file: the model would go
    // looking and report a failure the operator caused. Refuse now, by name.
    for file in &attachments {
        if !PathBuf::from(file).is_file() {
            return Err(format!("{file} is not a file on this machine"));
        }
    }
    let message = director::with_attachments(&text, &attachments);
    if message.is_empty() && !attach {
        return Err("nothing to send".to_string());
    }

    // Which conversation, and which profile is speaking.
    let conversation = match conversation_id {
        Some(id) => ws
            .conversation(&id)
            .await
            .ok_or_else(|| format!("no conversation {id}"))?,
        None => ws.open_conversation(None, None).await?,
    };
    let profile = ws
        .agent_exact(&conversation.profile_id)
        .await
        .ok_or_else(|| format!("no agent profile called {}", conversation.profile_id))?;
    if !profile.can_chat() {
        return Err(if profile.paused {
            format!("{} is paused", profile.name)
        } else {
            format!("{} is not set up for conversations", profile.name)
        });
    }

    let settings = ws.settings();
    // Every board it watches, with the pinned one marked: that is what makes
    // one Director rather than one per project.
    let briefs = ws.project_briefs(conversation.project_id.as_deref()).await?;

    // Reading code only makes sense inside a project, and only the one this
    // conversation is pinned to.
    let repo = match conversation.project_id.as_deref() {
        Some(id) => ws.project(id).await,
        None => None,
    }
    .filter(|p| PathBuf::from(&p.path).is_dir());
    let cwd = repo
        .as_ref()
        .map(|p| PathBuf::from(&p.path))
        .unwrap_or_else(|| ws.paths.root().to_path_buf());

    let crew: Vec<(String, String)> = ws
        .agents()
        .await
        .into_iter()
        .filter(|a| a.id != profile.id && a.can_take_work())
        .map(|a| (a.id, a.title))
        .collect();

    let resume_session = conversation.resumes().map(str::to_string);
    // global.md: small, always in the prompt on a fresh session.
    let global_memory =
        harness_app::memory::global_for(ws.paths.root()).unwrap_or_default();
    // The rules `record_decision` already wrote for the open board. Written
    // since the tool existed and read by nothing until now, which is why the
    // operator's own standing rule about not being asked twice never reached
    // the turn it was written for.
    let decisions = conversation
        .project_id
        .as_deref()
        .and_then(|id| {
            harness_app::memory::decisions_from(&ws.paths.project_memory_decisions(id))
        })
        .unwrap_or_default();
    let outside_work = ws.outside_work().await;
    // Permission the operator granted between turns, on a screen he cannot
    // see. Read here rather than pushed at him when he clicks: the turn is
    // where he can act on it, and a resumed session has no other way to learn.
    // Tomadas, não lidas: quem as pede fica sem elas. Numa reatação não há
    // prompt nenhum para as levar, portanto pedi-las era deitá-las fora — a
    // operadora aprovava uma proposta e o Director nunca saberia dela.
    let accepted_proposals = if attach {
        Vec::new()
    } else {
        ws.accepted_proposals()
    };
    // What his own reviewer decided while nobody was talking to him. Taken,
    // not read: this is the news, and the boards below carry the state.
    // A versão só se diz quando mudou. Numa sessão retomada nada mais lho
    // contaria: as ferramentas aparecem-lhe na lista sem explicação, e deduzir
    // uma actualização pelo efeito é a pior maneira de a saber.
    let running = env!("CARGO_PKG_VERSION");
    // Pela mesma razão: marcar a versão como vista sem a dizer a ninguém fazia
    // com que ela nunca fosse dita.
    let new_version = (!attach && conversation.seen_version.as_deref() != Some(running))
        .then_some(running);
    if new_version.is_some() {
        ws.record_chat_version(&conversation.id, running).await;
    }
    let prompt = director::chat_prompt(
        &ChatContext {
            speaker: Speaker {
                name: &profile.name,
                title: &profile.title,
                brief: &profile.brief,
                is_director: profile.id == agents::DIRECTOR_ID,
                can_delegate: profile.can_delegate,
                expected_output: &profile.expected_output,
            },
            user_name: &settings.user_name,
            projects: &briefs,
            repo: repo.as_ref().map(|p| p.name.as_str()),
            // Identity and history live in the session being resumed; sending
            // them again would have it start the conversation over.
            resumed: resume_session.is_some(),
            crew: &crew,
            global_memory: &global_memory,
            decisions: &decisions,
            // Only what the last look found; the look itself runs at startup
            // and at the close, never on a turn the operator is waiting for.
            outside_work: outside_work.as_deref(),
            accepted_proposals: &accepted_proposals,
            new_version,
        },
        &message,
    );

    // The operator's own turn goes into the transcript first, so a conversation
    // reads as a conversation rather than as a list of answers.
    let conversation = if record && !attach {
        let now = SystemClock.now_millis();
        ws.append_chat_line(
            &conversation.id,
            RunLogLine {
                ts_ms: now,
                event: RunEvent::UserMessage {
                    text: message.clone(),
                },
            },
        );
        ws.record_chat_message(&conversation.id, &message).await?
    } else {
        conversation
    };

    // Relay's own tools. The mutating ones are not in `allowed_tools`, so the
    // SDK sends each call through the approver first: the operator sees "the
    // Director wants to move c_7b30" before anything moves.
    let delegating = profile.can_delegate;
    let tools: ToolRunner = crate::director_tools::runner(
        ws,
        conversation.project_id.clone(),
        delegating,
        profile.id.clone(),
    );

    let mut allowed_tools = profile.allowed_tools();
    allowed_tools.extend(READ_ONLY_TOOLS.iter().map(|t| t.to_string()));
    allowed_tools.sort();
    allowed_tools.dedup();

    // Where anything said while this turn runs lands. Built per turn: a queue
    // that outlived its run would hold messages for a model that is no longer
    // listening.
    let inbox = harness_app::chatqueue::Queue::new(&conversation.id);

    // Resolved before the spec because both halves need it: the sidecar takes
    // its grants on the port, the Codex app server takes its MCP servers per
    // thread and so reads them off the run. Same value, handed to whichever
    // asks — the alternative was a Codex conversation with no connectors and
    // no sign of why.
    let granted = harness_app::grants::for_profile(ws.paths.root(), &profile);
    // Whether a dollar figure from this thread is a price at all. Asked of the
    // profile and not of the number: a cancelled Anthropic turn also reports
    // nothing, and that is priced work with no total yet.
    let priced = profile.backend.meters_cost() && profile.resolved_provider(&settings).is_none();

    let spec = RunSpec {
        backend: profile.backend,
        provider: profile.resolved_provider(&settings),
        prompt,
        cwd,
        model: profile.model.clone(),
        allowed_tools: Some(allowed_tools),
        max_budget_usd: profile.resolved_budget(),
        // `dontAsk` denies anything outside `allowed_tools` without ever
        // consulting the approver, which would make the board tools dead
        // (decision #27). `manual` routes them to the operator instead.
        permission_mode: Some("manual".to_string()),
        approver: Some(ws.router.approver_for(
            conversation.project_id.as_deref().unwrap_or("workspace"),
        )),
        resume_session: resume_session.clone(),
        // Esta conversa, e só esta. É por aqui que uma Relay nova reencontra um
        // turno que ficou a andar sem ela — e o prefixo mantém-na longe da
        // chave de um cartão, que é trabalho de outro agente: ligar-se ao
        // socket errado seria adoptá-lo, escrever-lhe os eventos nesta conversa
        // e responder-lhe às aprovações em nome dela.
        run_key: Some(format!("chat-{}", conversation.id)),
        from_seq: None,
        attach_only: attach,
        tools: Some(tools),
        // The whole point of the queue: this run can be spoken to while it
        // works. Only a conversation carries one — nobody is typing at a card.
        inbox: Some(Arc::clone(&inbox) as harness_ports::Inbox),
        // The operator watches it think while it works, so give it room to.
        thinking_tokens: Some(4000),
        // A conversation acts through Relay's own tools; no fan-out, and
        // nothing to report — its work is the conversation itself.
        subagents: false,
        report_work: false,
        // O porto desta conversa também as leva, e para o sidecar tanto dá —
        // são as mesmas. Aqui é por causa do Codex, cujo servidor as recebe
        // por thread e portanto as lê do run.
        grants: granted.clone(),
        // Como este perfil escreve. Só pega numa sessão nova: numa retomada
        // o estilo é o que ela trouxe de origem, e é por isso que mudá-lo
        // não mexe na conversa que está no ecrã.
        output_style: profile.output_style.clone(),
        // Quanto pensa. Ao contrário do estilo, prende-se ao pedido e não ao
        // prompt de sistema — é o que permite mudá-lo a meio de uma conversa
        // e a mensagem seguinte já sair no nível novo, sem sessão nova. Por
        // isso viaja no run e não no perfil.
        effort,
    };

    // What this profile was granted, resolved now rather than held anywhere:
    // an approval that landed a minute ago is in the profile, so the next turn
    // carries it. Nothing is inherited — a profile with no grants gets exactly
    // the isolated run it got before any of this existed.
    let agent = crate::chat::granted_agent_for(ws, granted);
    let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(64);
    // Registered by conversation id so the operator has a stop: a turn that
    // never emits `done` must not leave them without an exit — and, since the
    // composer stays live, somewhere for what they type meanwhile to land.
    let token = CancellationToken::new();
    let token_for_reaper = token.clone();
    let turn = crate::conversations::Turn::new(token.clone(), Arc::clone(&inbox));
    let turn_id = turn.id;
    // Refused rather than stacked. Two turns on one conversation are two
    // processes resuming the same Claude session, both writing to the same
    // transcript: the second redoes work the first already did, and the model
    // — reading a tree that moved under it — reports a second session that
    // does not exist.
    //
    // The words are already in the thread by now, so a refusal here cannot end
    // in an error: it ends where a message written mid-turn was always meant
    // to go, which is into the turn that is running. Nothing of this run has
    // started, so there is nothing to unwind.
    if ws.register_chat_turn(&conversation.id, turn).await.is_err() {
        if let Some(live) = ws.live_chat_turn(&conversation.id).await {
            let _ = live.queue.push(&message);
        }
        return Ok(conversation);
    }

    // A guarda acima acabou de dizer que esta conversa não tem turno vivo. Mas
    // "sem turno vivo do lado da Relay" deixou de querer dizer "sem trabalho a
    // andar": desde a reatação, um sidecar pode ter continuado sozinho e estar
    // à espera de que alguém se volte a ligar a ele. Por isso a limpeza é a
    // segunda escolha e não a primeira — se há socket, quem decide é o porto,
    // que se liga e confere a chave antes de adoptar seja o que for.
    //
    // Sem socket é um resto do tempo dos canos, e esse ainda é o do #108: um
    // processo agarrado à sessão que ninguém lê. Esse limpa-se, como antes.
    if let Some(session) = resume_session.clone() {
        // Pelo mesmo caminho que o adaptador o construiria — são o mesmo
        // ficheiro, e uma segunda maneira de o soletrar era esta pergunta a
        // responder sempre "não há socket".
        let socket = harness_ports::sockets::path_for(
            &ws.paths.run_sockets_dir(),
            &format!("chat-{}", conversation.id),
        );
        if !socket.exists() {
            let swept =
                tokio::task::spawn_blocking(move || harness_app::strays::reap_session(&session))
                    .await
                    .unwrap_or(0);
            if swept > 0 {
                eprintln!("swept {swept} stray process(es) still holding this session");
            }
        }
    }

    let fut = agent.run(spec, ev_tx, token);

    let app = ws.app_handle();
    let ws = Arc::clone(ws);
    let conversation_id = conversation.id.clone();
    let project_id = conversation.project_id.clone().unwrap_or_default();
    let resumed = resume_session.is_some();

    tauri::async_runtime::spawn(async move {
        // Um turno registado que morre sem se desregistar deixa a conversa
        // trancada: o `register` recusa o seguinte, e a partir daí tudo o que o
        // operador escreve vai para a fila de um morto — que foi exactamente o
        // que se viu, a mesma resposta três vezes seguidas ao longo de dez
        // minutos. O `finish` no fim do corpo não chega, porque um `panic` ou
        // um `abort` não passam por ele.
        //
        // Isto passa. Cancelar o token é bastante: o `Turns::register` já trata
        // um token cancelado como um turno acabado e ocupa-lhe o lugar, portanto
        // o pior caso deixa de ser uma conversa trancada para sempre e passa a
        // ser um turno a menos.
        let _reaper = TurnReaper(Some(token_for_reaper));
        // Did this run actually reach a live session? A resume of a session
        // that no longer exists comes back with no `started`, no text and an
        // error result — so this is what separates "the thread continued" from
        // "there was nothing to continue".
        let answered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Published in the same shape as any run and keyed by the conversation
        // id, so the UI has one typed listener for both.
        let publish = {
            let app = app.clone();
            let conversation_id = conversation_id.clone();
            let project_id = project_id.clone();
            move |event: RunEvent| {
                let _ = app.emit(
                    crate::events::ENGINE_RUN,
                    RunUpdate {
                        project_id: project_id.clone(),
                        card_id: CardId::new(conversation_id.clone()),
                        run_id: RunId(conversation_id.clone()),
                        ts_ms: SystemClock.now_millis(),
                        event,
                    },
                );
            }
        };

        let seen = std::sync::Arc::clone(&answered);
        // O troço de raciocínio que está a ser escrito, à espera de assentar.
        let mut thought = String::new();
        let forward = async {
            while let Some(ev) = ev_rx.recv().await {
                // The session id is the whole reason this conversation can be
                // continued tomorrow. It arrives twice — on init and on the
                // result — and either is worth keeping.
                match &ev {
                    RunEvent::Started { session_id } => {
                        seen.store(true, std::sync::atomic::Ordering::Relaxed);
                        ws.record_chat_session(&conversation_id, session_id).await;
                    }
                    RunEvent::Text { .. } | RunEvent::Delta { .. } => {
                        seen.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    RunEvent::Done {
                        session_id,
                        cost_usd,
                        ..
                    } => {
                        if let Some(sid) = session_id {
                            ws.record_chat_session(&conversation_id, sid).await;
                        }
                        ws.record_chat_cost(&conversation_id, *cost_usd, priced).await;
                    }
                    _ => {}
                }
                // A lista do `/` chega por evento efémero e some-se com a
                // sessão. Guardada aqui, sobrevive ao reinício.
                if let RunEvent::Commands { commands } = &ev {
                    ws.remember_slash_commands(commands);
                }
                // O raciocínio chega em fatias efémeras e desaparecia com a
                // sessão: recarregada a conversa, o modelo parecia não ter
                // pensado nada. As fatias juntam-se aqui e assentam como um
                // `Thought` — que se guarda — assim que chega qualquer outra
                // coisa. Por troço e não por turno: um turno pensa, age, e
                // pensa outra vez, e um bloco só punha o raciocínio ao lado de
                // trabalho que aconteceu depois dele.
                match &ev {
                    // Só o raciocínio *deste* agente. Um subagente escreve no
                    // mesmo fluxo, e sem esta pergunta o pensamento dele era
                    // colado ao do Director.
                    RunEvent::Thinking { text, parent_tool_use_id: None } => {
                        thought.push_str(text)
                    }
                    // E nada do que um subagente faz fecha um pensamento que
                    // não é dele. Era isto que cortava as frases do Director a
                    // meio da palavra, uma vez por cada coisa que um filho
                    // fazia enquanto ele escrevia.
                    _ if ev.from_subagent() => {}
                    _ if !thought.trim().is_empty() => {
                        let sealed = RunEvent::Thought {
                            text: std::mem::take(&mut thought),
                        };
                        ws.append_chat_line(
                            &conversation_id,
                            RunLogLine {
                                ts_ms: SystemClock.now_millis(),
                                event: sealed.clone(),
                            },
                        );
                        publish(sealed);
                    }
                    _ => thought.clear(),
                }
                // Deltas are for the live view only; the `Text` that follows is
                // what the transcript keeps (decision #25).
                if !ev.is_ephemeral() {
                    ws.append_chat_line(
                        &conversation_id,
                        RunLogLine {
                            ts_ms: SystemClock.now_millis(),
                            event: ev.clone(),
                        },
                    );
                }
                publish(ev);
            }
        };

        let (result, _) = tokio::join!(fut, forward);

        // The UI clears its thinking state on the last event, so one always
        // goes out.
        match result {
            Ok(RunOutcome::Completed { .. }) => {}
            Ok(RunOutcome::Cancelled) => {
                let event = RunEvent::Done {
                    session_id: None,
                    cost_usd: None,
                    turns: None,
                    result: None,
                    error: None,
                };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: event.clone(),
                    },
                );
                publish(event);
            }
            Ok(RunOutcome::Failed { message, .. }) | Err(message) => {
                // A resume that could not be honoured is the one failure worth
                // explaining rather than just reporting: the thread above is
                // still readable, but the model no longer remembers it.
                //
                // Mas só quando foi *a sessão* que faltou. Isto dizia apenas
                // "falhou durante um resume", e por isso um socket que não
                // ligava — um problema de processo, com a sessão inteira no
                // disco — desligava a conversa da sua própria história para
                // sempre. Ver `conversations::session_was_lost`, e a
                // `harness_ports::sockets` para o bug que fazia isto disparar
                // em todos os runs de macOS.
                let unanswered = resumed && !answered.load(std::sync::atomic::Ordering::Relaxed);
                let lost = unanswered && harness_app::conversations::session_was_lost(&message);
                if unanswered && !lost {
                    let notice = RunEvent::Notice {
                        text: format!(
                            "this turn never reached the model ({message}). The conversation \
                             keeps its session — try again, and if it keeps failing the run \
                             above says what could not be started."
                        ),
                    };
                    ws.append_chat_line(
                        &conversation_id,
                        RunLogLine {
                            ts_ms: SystemClock.now_millis(),
                            event: notice.clone(),
                        },
                    );
                    publish(notice);
                }
                if lost {
                    ws.record_chat_resume_failure(&conversation_id).await;
                    let notice = RunEvent::Notice {
                        text: format!(
                            "the Claude session for this conversation could not be resumed \
                             ({message}). Everything above is still here to read, but the model \
                             has lost its own memory of it — your next message starts a new \
                             session."
                        ),
                    };
                    ws.append_chat_line(
                        &conversation_id,
                        RunLogLine {
                            ts_ms: SystemClock.now_millis(),
                            event: notice.clone(),
                        },
                    );
                    publish(notice);
                }
                let event = RunEvent::Failed { message };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: event.clone(),
                    },
                );
                publish(event);
            }
        }

        // The turn is over, however it ended: done, failed or cancelled.
        // Deregistered only now, after its last event has gone out — a turn
        // taken off the list earlier would leave a window in which a message
        // finds no turn to join and none of its events have arrived either, so
        // the screen would start a fresh turn while still believing the old
        // one was running. Taking it is also what shuts the inbox, and what
        // comes back is everything the run never read. Empty when the operator
        // stopped it, because `stop_turn` took it first and dropped the queue
        // on purpose.
        let undelivered = match ws.finish_chat_turn(&conversation_id, Some(turn_id)).await {
            Some(turn) => turn.queue.close(),
            None => Vec::new(),
        };
        // Saiu pela porta: já não há nada para o guarda apanhar, e cancelar o
        // token agora só confundiria quem o lê a seguir.
        _reaper.disarm();

        // A message typed while the turn was ending must not vanish because it
        // lost the race. Its words are already in the transcript, so all that
        // changes here is the mark — from queued to read — and then it becomes
        // the next turn, in the order it was typed.
        if !undelivered.is_empty() {
            for message in &undelivered {
                let event = RunEvent::UserRead {
                    queue_id: message.id.clone(),
                };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: event.clone(),
                    },
                );
                publish(event);
            }
            let carried = undelivered
                .iter()
                .map(|m| m.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if let Err(e) =
                carried_turn(Arc::clone(&ws), conversation_id.clone(), carried).await
            {
                let notice = RunEvent::Notice {
                    text: format!(
                        "what you sent while that turn was running could not be started as \
                         a turn of its own ({e}). It is still above — send it again."
                    ),
                };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: notice.clone(),
                    },
                );
                publish(notice);
            }
        }
    });

    Ok(conversation)
}

/// Stop the turn a conversation has in flight. No-op when there is none.
///
/// Stop means stop, and that includes the queue: anything the operator typed
/// while the turn ran is dropped rather than delivered into a turn they have
/// just asked to end. Nothing is thrown away — the messages stay in the thread
/// exactly as they were written down, still saying the model never read them,
/// and sending one again is one click.
/// Cancela o token do turno se o run desaparecer sem se despedir.
///
/// Existe por causa do `Drop`, e é a única coisa que corre num caminho que
/// ninguém escreveu: um `panic` dentro da tarefa, ou a tarefa a ser deitada
/// fora. Não desregista o turno — isso é uma chamada a um actor e o `Drop` não
/// pode esperar por nada — mas cancelar chega, porque um turno cancelado deixa
/// de contar como vivo para o `register`.
struct TurnReaper(Option<CancellationToken>);

impl TurnReaper {
    fn disarm(mut self) {
        self.0 = None;
    }
}

impl Drop for TurnReaper {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            token.cancel();
        }
    }
}

pub async fn stop_turn(ws: &Workspace, conversation_id: &str) {
    let Some(turn) = ws.finish_chat_turn(conversation_id, None).await else {
        return;
    };
    let dropped = turn.queue.close();
    turn.token.cancel();
    if dropped.is_empty() {
        return;
    }
    let project_id = ws
        .conversation(conversation_id)
        .await
        .and_then(|c| c.project_id)
        .unwrap_or_default();
    let notice = RunEvent::Notice {
        text: if dropped.len() == 1 {
            "stopped before the message you queued was read — it is above, unsent.".to_string()
        } else {
            format!(
                "stopped before the {} messages you queued were read — they are above, unsent.",
                dropped.len()
            )
        },
    };
    note_chat(ws, conversation_id, &project_id, notice);
}

/// The agent used for conversations: the sidecar or the command line, decided
/// per run so the Settings toggle applies immediately (decision #13).
pub fn agent_for(ws: &Workspace) -> Arc<dyn AgentPort> {
    granted_agent_for(ws, harness_ports::Grants::default())
}

/// The same port, carrying what one agent was granted.
///
/// Grants hang off the **port**, not off the `RunSpec`, because a conversation
/// builds its own port and serves exactly one profile. The engine holds a
/// single port shared by every card run, so this cannot reach worker runs
/// without the engine passing the grants through — see `DEBT.md`.
///
/// The CLI adapter ignores them: `claude --print` has no equivalent of the
/// SDK's `plugins` option, and inventing one would mean writing into the
/// operator's `~/.claude`, which is the thing this design exists to avoid.
pub fn granted_agent_for(ws: &Workspace, grants: harness_ports::Grants) -> Arc<dyn AgentPort> {
    let script = sidecar::script_in(ws.sidecar_dir());
    Arc::new(SwitchingAgent {
        sidecar: Arc::new(
            SidecarAgent::new("node", script.clone())
                .with_grants(grants)
                .with_runs_dir(ws.paths.run_sockets_dir()),
        ),
        cli: Arc::new(ClaudeCliAgent::new("claude")),
        // Grants reach Codex through the run rather than through the port: the
        // app server takes its MCP servers per thread, so the adapter reads
        // `spec.grants` and needs nothing built into it.
        codex: Arc::new(
            harness_agent_codex::CodexAgent::new("codex").with_home(&ws.paths.codex_home()),
        ),
        settings: Arc::clone(&ws.settings),
    })
}

/// Read a conversation back from disk.
pub fn transcript(ws: &Workspace, conversation_id: &str) -> Result<Vec<RunLogLine>, String> {
    RunLogPort::read(ws.chat_log(), conversation_id).map_err(|e| e.to_string())
}

/// What the thread spent, counted over the whole transcript rather than over
/// whatever the screen happens to have loaded. The arithmetic is in
/// `harness_app::conversations`; this only reads the file and hands it over.
///
/// `profile_model` is what the profile is configured to run on, used only when
/// the transcript is old enough never to have named a model itself.
pub fn totals(
    ws: &Workspace,
    conversation_id: &str,
    cost_usd: f64,
    priced: bool,
    profile_model: Option<&str>,
) -> Result<harness_app::conversations::ConversationTotals, String> {
    let lines = transcript(ws, conversation_id)?;
    Ok(harness_app::conversations::totals(
        &lines,
        cost_usd,
        priced,
        profile_model,
    ))
}
