//! As conversas: quais existem, que sessão nativa cada uma continua, e qual
//! delas tem um turno a decorrer.
//!
//! Segue o mesmo princípio do `registry.rs` — um loop possui o estado, ninguém
//! partilha — mas isto tem ciclo de vida, e é por isso que está separado. Uma
//! conversa sobrevive à troca de projecto; um `CancellationToken` tem dono e
//! tem momento de morte. As duas coisas mudam pelas mesmas razões e ao mesmo
//! tempo (um turno começa numa conversa, acaba numa conversa), portanto têm de
//! ter o mesmo dono: com dois donos separados haveria um instante em que a
//! conversa já não existe e o token dela ainda sim.
//!
//! O relógio também é do actor. Antes cada chamador carimbava a hora e mandava
//! o número — duas escritas quase simultâneas podiam chegar por ordem inversa
//! ao carimbo que traziam. Aqui a ordem da fila **é** a ordem do tempo.
//!
//! As **palavras** não estão aqui (decisão #34): a transcrição é um
//! `RunLogPort` por conversa. O índice diz qual a sessão e qual o ficheiro; o
//! ficheiro tem o texto.

use std::collections::HashMap;

use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use harness_app::conversations::{Conversation, ConversationIndex};
use harness_app::paths::{self, AppPaths};
use harness_ports::ClockPort;

use crate::workspace::SystemClock;

const QUEUE_CAPACITY: usize = 128;

enum Msg {
    List {
        include_archived: bool,
        reply: oneshot::Sender<Vec<Conversation>>,
    },
    Get {
        id: String,
        reply: oneshot::Sender<Option<Conversation>>,
    },
    ResumeTarget {
        profile_id: String,
        reply: oneshot::Sender<Option<Conversation>>,
    },
    /// Insere uma conversa já montada. Quem a monta é o chamador, que é quem
    /// sabe validar o perfil e o projecto.
    Insert {
        conversation: Box<Conversation>,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    Select {
        id: String,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    Rename {
        id: String,
        title: String,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    SetArchived {
        id: String,
        archived: bool,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    Remove {
        id: String,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    Pin {
        id: String,
        project_id: Option<String>,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    UnpinProject {
        project_id: String,
        reply: oneshot::Sender<()>,
    },
    RecordMessage {
        id: String,
        message: String,
        reply: oneshot::Sender<Result<Conversation, String>>,
    },
    RecordSession {
        id: String,
        session_id: String,
        reply: oneshot::Sender<()>,
    },
    RecordCost {
        id: String,
        cost_usd: Option<f64>,
        reply: oneshot::Sender<()>,
    },
    RecordResumeFailure {
        id: String,
        reply: oneshot::Sender<()>,
    },
    RegisterTurn {
        conversation_id: String,
        token: CancellationToken,
        reply: oneshot::Sender<()>,
    },
    FinishTurn {
        conversation_id: String,
        reply: oneshot::Sender<Option<CancellationToken>>,
    },
}

struct Conversations {
    app: AppHandle,
    paths: AppPaths,
    index: ConversationIndex,
    /// O token do turno que cada conversa tem no ar. Sem isto, um turno que
    /// nunca emite `done` deixa o operador sem saída.
    turns: HashMap<String, CancellationToken>,
}

impl Conversations {
    fn save(&self) -> Result<(), String> {
        paths::write_json(&self.paths.conversations_file(), &self.index)
    }

    /// A lista mudou sem a UI pedir; ela desenha estado do backend, portanto é
    /// o backend que diz quando ele mexeu. Só os três registos que o turno
    /// escreve sozinho anunciam: os restantes chegam por um comando IPC, e
    /// quem o chamou já recebe a linha de volta na resposta.
    fn publish(&self) {
        let _ = self.app.emit("chat://conversations", self.index.list(false));
    }

    /// Guardar o que ninguém pediu: o erro vai para o `stderr` porque não há
    /// chamador à espera de o ver.
    fn save_quietly(&self) {
        if let Err(e) = self.save() {
            eprintln!("could not save the conversation index: {e}");
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        while let Some(msg) = rx.recv().await {
            let now = SystemClock.now_millis();
            match msg {
                Msg::List {
                    include_archived,
                    reply,
                } => {
                    let _ = reply.send(self.index.list(include_archived));
                }
                Msg::Get { id, reply } => {
                    let _ = reply.send(self.index.get(&id).cloned());
                }
                Msg::ResumeTarget { profile_id, reply } => {
                    let _ = reply.send(self.index.resume_target(&profile_id).cloned());
                }
                Msg::Insert {
                    conversation,
                    reply,
                } => {
                    let created = self.index.insert(*conversation);
                    let _ = reply.send(self.save().map(|()| created));
                }
                Msg::Select { id, reply } => {
                    let answer = self.index.select(&id).and_then(|()| {
                        self.save().map(|()| {
                            self.index
                                .get(&id)
                                .cloned()
                                .expect("select found it a line ago")
                        })
                    });
                    let _ = reply.send(answer);
                }
                Msg::Rename { id, title, reply } => {
                    let answer = self.index.rename(&id, &title, now).and_then(|c| self.save().map(|()| c));
                    let _ = reply.send(answer);
                }
                Msg::SetArchived {
                    id,
                    archived,
                    reply,
                } => {
                    let answer = self.index.set_archived(&id, archived, now).and_then(|c| self.save().map(|()| c));
                    let _ = reply.send(answer);
                }
                Msg::Remove { id, reply } => {
                    let answer = self.index.remove(&id).and_then(|c| self.save().map(|()| c));
                    let _ = reply.send(answer);
                }
                Msg::Pin {
                    id,
                    project_id,
                    reply,
                } => {
                    let entry = self.index.conversations.iter_mut().find(|c| c.id == id);
                    let answer = match entry {
                        Some(entry) => {
                            entry.project_id = project_id;
                            entry.updated_ms = now;
                            Ok(entry.clone())
                        }
                        None => Err(format!("no conversation {id}")),
                    }
                    .and_then(|c| self.save().map(|()| c));
                    let _ = reply.send(answer);
                }
                Msg::UnpinProject { project_id, reply } => {
                    self.index.unpin_project(&project_id);
                    self.save_quietly();
                    let _ = reply.send(());
                }
                Msg::RecordMessage {
                    id,
                    message,
                    reply,
                } => {
                    let answer = self.index.record_message(&id, &message, now).and_then(|c| self.save().map(|()| c));
                    let _ = reply.send(answer);
                }
                Msg::RecordSession {
                    id,
                    session_id,
                    reply,
                } => {
                    // Só quando muda de facto: um `session_id` repetido chega
                    // duas vezes por turno (no init e no resultado), e anunciar
                    // as duas seria ruído no ecrã.
                    if self
                        .index
                        .record_session(&id, &session_id, now)
                        .unwrap_or(false)
                    {
                        self.save_quietly();
                        self.publish();
                    }
                    let _ = reply.send(());
                }
                Msg::RecordCost {
                    id,
                    cost_usd,
                    reply,
                } => {
                    self.index.record_cost(&id, cost_usd, now);
                    self.save_quietly();
                    self.publish();
                    let _ = reply.send(());
                }
                Msg::RecordResumeFailure { id, reply } => {
                    self.index.record_resume_failure(&id, now);
                    self.save_quietly();
                    self.publish();
                    let _ = reply.send(());
                }
                Msg::RegisterTurn {
                    conversation_id,
                    token,
                    reply,
                } => {
                    self.turns.insert(conversation_id, token);
                    let _ = reply.send(());
                }
                Msg::FinishTurn {
                    conversation_id,
                    reply,
                } => {
                    let _ = reply.send(self.turns.remove(&conversation_id));
                }
            }
        }
    }
}

/// A ponta pública. Cada método é uma ida e volta ao actor.
#[derive(Clone)]
pub struct ConversationsHandle {
    tx: mpsc::Sender<Msg>,
}

impl ConversationsHandle {
    pub fn spawn(app: AppHandle, paths: AppPaths, index: ConversationIndex) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let state = Conversations {
            app,
            paths,
            index,
            turns: HashMap::new(),
        };
        tauri::async_runtime::spawn(state.run(rx));
        Self { tx }
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Result<T, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(make(reply_tx))
            .await
            .map_err(|_| "the conversation index is not running".to_string())?;
        reply_rx
            .await
            .map_err(|_| "the conversation index dropped the reply".to_string())
    }

    pub async fn list(&self, include_archived: bool) -> Vec<Conversation> {
        self.ask(|reply| Msg::List {
            include_archived,
            reply,
        })
        .await
        .unwrap_or_default()
    }

    pub async fn get(&self, id: &str) -> Option<Conversation> {
        self.ask(|reply| Msg::Get {
            id: id.to_string(),
            reply,
        })
        .await
        .ok()
        .flatten()
    }

    /// A conversa a reabrir quando a app arranca.
    pub async fn resume_target(&self, profile_id: &str) -> Option<Conversation> {
        self.ask(|reply| Msg::ResumeTarget {
            profile_id: profile_id.to_string(),
            reply,
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn insert(&self, conversation: Conversation) -> Result<Conversation, String> {
        self.ask(|reply| Msg::Insert {
            conversation: Box::new(conversation),
            reply,
        })
        .await?
    }

    pub async fn select(&self, id: &str) -> Result<Conversation, String> {
        self.ask(|reply| Msg::Select {
            id: id.to_string(),
            reply,
        })
        .await?
    }

    pub async fn rename(&self, id: &str, title: &str) -> Result<Conversation, String> {
        self.ask(|reply| Msg::Rename {
            id: id.to_string(),
            title: title.to_string(),
            reply,
        })
        .await?
    }

    pub async fn set_archived(&self, id: &str, archived: bool) -> Result<Conversation, String> {
        self.ask(|reply| Msg::SetArchived {
            id: id.to_string(),
            archived,
            reply,
        })
        .await?
    }

    pub async fn remove(&self, id: &str) -> Result<Conversation, String> {
        self.ask(|reply| Msg::Remove {
            id: id.to_string(),
            reply,
        })
        .await?
    }

    pub async fn pin(
        &self,
        id: &str,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        self.ask(|reply| Msg::Pin {
            id: id.to_string(),
            project_id,
            reply,
        })
        .await?
    }

    pub async fn unpin_project(&self, project_id: &str) {
        let _ = self
            .ask(|reply| Msg::UnpinProject {
                project_id: project_id.to_string(),
                reply,
            })
            .await;
    }

    pub async fn record_message(&self, id: &str, message: &str) -> Result<Conversation, String> {
        self.ask(|reply| Msg::RecordMessage {
            id: id.to_string(),
            message: message.to_string(),
            reply,
        })
        .await?
    }

    pub async fn record_session(&self, id: &str, session_id: &str) {
        let _ = self
            .ask(|reply| Msg::RecordSession {
                id: id.to_string(),
                session_id: session_id.to_string(),
                reply,
            })
            .await;
    }

    pub async fn record_cost(&self, id: &str, cost_usd: Option<f64>) {
        let _ = self
            .ask(|reply| Msg::RecordCost {
                id: id.to_string(),
                cost_usd,
                reply,
            })
            .await;
    }

    pub async fn record_resume_failure(&self, id: &str) {
        let _ = self
            .ask(|reply| Msg::RecordResumeFailure {
                id: id.to_string(),
                reply,
            })
            .await;
    }

    /// Uma conversa tem um turno no ar: guarda o token que o cancela.
    pub async fn register_turn(&self, conversation_id: &str, token: CancellationToken) {
        let _ = self
            .ask(|reply| Msg::RegisterTurn {
                conversation_id: conversation_id.to_string(),
                token,
                reply,
            })
            .await;
    }

    /// Tira o token do turno (None se a conversa não tinha nenhum). Tirá-lo é
    /// ao mesmo tempo como o stop o encontra e como o fim do turno o limpa.
    pub async fn finish_turn(&self, conversation_id: &str) -> Option<CancellationToken> {
        self.ask(|reply| Msg::FinishTurn {
            conversation_id: conversation_id.to_string(),
            reply,
        })
        .await
        .ok()
        .flatten()
    }
}
