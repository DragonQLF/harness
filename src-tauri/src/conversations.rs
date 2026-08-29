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
use std::sync::Arc;

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
    RecordVersion {
        id: String,
        version: String,
        reply: oneshot::Sender<()>,
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
        turn: Turn,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Espreitar sem tirar: quem escreve a meio de um turno quer falar com ele,
    /// não acabá-lo.
    LiveTurn {
        conversation_id: String,
        reply: oneshot::Sender<Option<Turn>>,
    },
    FinishTurn {
        conversation_id: String,
        /// `None` acaba o que lá estiver — é o que o operador pede quando
        /// carrega em parar. Um turno a acabar-se a si próprio diz qual é.
        only: Option<u64>,
        reply: oneshot::Sender<Option<Turn>>,
    },
}

/// O turno que uma conversa tem no ar: como o parar, e onde aterra o que o
/// operador escrever enquanto ele corre.
///
/// As duas coisas nascem e morrem juntas — a fila só existe enquanto houver um
/// turno que a leia — portanto têm o mesmo dono, pela mesma razão que o token
/// já vivia aqui e não ao lado.
#[derive(Clone)]
pub struct Turn {
    /// Quem este turno é, e não apenas de que conversa. Sem isto, acabar um
    /// turno é acabar "o turno desta conversa" — que pode já ser outro.
    pub id: u64,
    pub token: CancellationToken,
    pub queue: Arc<harness_app::chatqueue::Queue>,
}

impl Turn {
    pub fn new(token: CancellationToken, queue: Arc<harness_app::chatqueue::Queue>) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            token,
            queue,
        }
    }
}

/// Que turno está no ar, por conversa. As duas regras que impedem dois turnos
/// na mesma conversa vivem aqui, e não dentro do laço do actor, porque são a
/// parte que se pode pôr à prova.
#[derive(Default)]
struct Turns(HashMap<String, Turn>);

impl Turns {
    /// Recusado, não substituído. Um `insert` por cima deixava o turno
    /// anterior a correr sem ninguém a segurar-lhe o token: vivo, invisível e
    /// impossível de parar — e a segunda sessão que o operador julga estar a
    /// ver é essa.
    fn register(&mut self, conversation_id: String, turn: Turn) -> Result<(), String> {
        match self.0.get(&conversation_id) {
            Some(live) if !live.token.is_cancelled() => {
                Err("this conversation already has a turn in flight".to_string())
            }
            _ => {
                self.0.insert(conversation_id, turn);
                Ok(())
            }
        }
    }

    fn live(&self, conversation_id: &str) -> Option<Turn> {
        self.0.get(conversation_id).cloned()
    }

    /// Um turno que acabou só se tira a si próprio (`only`). Tirar "o turno
    /// desta conversa" tirava o seguinte, e era assim que um turno vivo ficava
    /// órfão. O stop do operador não nomeia nenhum: leva o que lá estiver.
    fn finish(&mut self, conversation_id: &str, only: Option<u64>) -> Option<Turn> {
        match only {
            Some(id) if self.0.get(conversation_id).map(|t| t.id) != Some(id) => None,
            _ => self.0.remove(conversation_id),
        }
    }
}

struct Conversations {
    app: AppHandle,
    paths: AppPaths,
    index: ConversationIndex,
    /// O turno que cada conversa tem no ar. Sem isto, um turno que nunca emite
    /// `done` deixa o operador sem saída — e não haveria onde pousar uma
    /// mensagem escrita a meio dele.
    turns: Turns,
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
                Msg::RecordVersion { id, version, reply } => {
                    self.index.record_version(&id, &version);
                    self.save_quietly();
                    let _ = reply.send(());
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
                    turn,
                    reply,
                } => {
                    let _ = reply.send(self.turns.register(conversation_id, turn));
                }
                Msg::LiveTurn {
                    conversation_id,
                    reply,
                } => {
                    let _ = reply.send(self.turns.live(&conversation_id));
                }
                Msg::FinishTurn {
                    conversation_id,
                    only,
                    reply,
                } => {
                    let _ = reply.send(self.turns.finish(&conversation_id, only));
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
            turns: Turns::default(),
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

    /// Fica registado que esta conversa já foi informada desta versão, para o
    /// aviso ser dito uma vez e não a cada turno.
    pub async fn record_version(&self, id: &str, version: &str) {
        let _ = self
            .ask(|reply| Msg::RecordVersion {
                id: id.to_string(),
                version: version.to_string(),
                reply,
            })
            .await;
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

    /// Uma conversa tem um turno no ar: guarda como o parar e onde lhe falar.
    /// `Err` quando já lá estava um — e aí o turno novo não deve arrancar.
    pub async fn register_turn(&self, conversation_id: &str, turn: Turn) -> Result<(), String> {
        self.ask(|reply| Msg::RegisterTurn {
            conversation_id: conversation_id.to_string(),
            turn,
            reply,
        })
        .await
        .unwrap_or_else(|_| Err("the conversation store is gone".to_string()))
    }

    /// O turno em curso, sem lhe mexer.
    pub async fn live_turn(&self, conversation_id: &str) -> Option<Turn> {
        self.ask(|reply| Msg::LiveTurn {
            conversation_id: conversation_id.to_string(),
            reply,
        })
        .await
        .ok()
        .flatten()
    }

    /// Tira o turno (None se a conversa não tinha nenhum). Tirá-lo é ao mesmo
    /// tempo como o stop o encontra e como o fim do turno o limpa. `only` diz
    /// qual: um turno a limpar-se a si próprio nomeia-se, o stop leva o que
    /// estiver lá.
    pub async fn finish_turn(&self, conversation_id: &str, only: Option<u64>) -> Option<Turn> {
        self.ask(|reply| Msg::FinishTurn {
            conversation_id: conversation_id.to_string(),
            only,
            reply,
        })
        .await
        .ok()
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn() -> Turn {
        Turn::new(
            CancellationToken::new(),
            harness_app::chatqueue::Queue::new("c"),
        )
    }

    /// O bug: `chat_send` chamado com um turno ainda a correr punha um segundo
    /// processo a retomar a mesma sessão do Claude. Os dois escreviam para o
    /// mesmo transcript, o segundo refazia trabalho que o primeiro já tinha
    /// feito, e o modelo — a ler uma árvore que se mexeu debaixo dele —
    /// relatava uma sessão que não existe.
    #[test]
    fn a_second_turn_is_refused_while_the_first_is_alive() {
        let mut turns = Turns::default();
        let first = turn();
        assert!(turns.register("c1".into(), first.clone()).is_ok());
        assert!(turns.register("c1".into(), turn()).is_err());
        // E o primeiro continua a ser o que lá está: recusar não é substituir.
        assert_eq!(turns.live("c1").map(|t| t.id), Some(first.id));
    }

    #[test]
    fn a_cancelled_turn_does_not_hold_the_conversation() {
        let mut turns = Turns::default();
        let stopped = turn();
        turns.register("c1".into(), stopped.clone()).unwrap();
        stopped.token.cancel();
        let next = turn();
        assert!(turns.register("c1".into(), next.clone()).is_ok());
        assert_eq!(turns.live("c1").map(|t| t.id), Some(next.id));
    }

    /// A outra metade: acabar o turno tirava "o turno desta conversa", que a
    /// meio de uma troca já podia ser o seguinte — e deixava esse vivo e sem
    /// registo, que é como um turno órfão nasce.
    #[test]
    fn a_finished_turn_only_takes_itself() {
        let mut turns = Turns::default();
        let old = turn();
        turns.register("c1".into(), old.clone()).unwrap();
        old.token.cancel();
        let current = turn();
        turns.register("c1".into(), current.clone()).unwrap();

        assert!(turns.finish("c1", Some(old.id)).is_none());
        assert_eq!(turns.live("c1").map(|t| t.id), Some(current.id));
        assert_eq!(turns.finish("c1", Some(current.id)).map(|t| t.id), Some(current.id));
    }

    /// O stop do operador não nomeia nenhum: quer acabar o que estiver no ar.
    #[test]
    fn the_stop_takes_whatever_is_there() {
        let mut turns = Turns::default();
        let live = turn();
        turns.register("c1".into(), live.clone()).unwrap();
        assert_eq!(turns.finish("c1", None).map(|t| t.id), Some(live.id));
        assert!(turns.live("c1").is_none());
    }
}
