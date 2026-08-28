//! O registo: os perfis de agente e os projectos que o operador adicionou.
//!
//! A premissa da arquitectura é uma só — **um loop possui o estado, ninguém
//! partilha, não há locks**. O engine possui o `Board` assim; este ficheiro dá
//! o mesmo dono ao resto. O estado vive dentro da tarefa que corre
//! `Registry::run`, e toda a gente fala com ele por mensagem, com a resposta a
//! voltar por um `oneshot`.
//!
//! Por isso é que a persistência também é dele: quem muda a lista é quem a
//! escreve, no mesmo passo. Não há janela entre a mutação e o ficheiro em que
//! outro leitor veja uma coisa e o disco outra.
//!
//! O que **não** vive aqui é o I/O demorado — canonicalizar caminhos, `git
//! init`, criar worktrees. Isso corre no chamador, antes de mandar a mensagem,
//! porque um actor bloqueado segundos a fio deixa de ser um dono e passa a ser
//! uma fila (é a mesma razão por que o engine resolve worktrees fora do loop).

use tokio::sync::{mpsc, oneshot};

use harness_app::agents::{self, AgentProfile};
use harness_app::paths::{self, AppPaths};
use harness_app::projects::Project;

/// Fundo suficiente para as rajadas do arranque (um `warm_all` pergunta pelos
/// projectos uma vez por cada engine que levanta) sem crescer sem conta.
const QUEUE_CAPACITY: usize = 128;

/// O que a vigia ao repositório do Relay encontrou da última vez.
///
/// Vive aqui, e não atrás de um lock no `Workspace`, porque o espelho é um
/// projecto e este é o dono dos projectos — e porque, ao contrário do
/// `settings` e do `inbox`, os dois leitores dele (`chat::send` e `bootstrap`)
/// são `async` e podem esperar pela fila. Era o único dos quatro que podia
/// mudar sem obrigar a uma cópia síncrona ao lado (#87).
pub struct Finding {
    /// O parágrafo inteiro, tal como o `chat.rs` o põe no prompt.
    pub said: String,
    /// O mesmo achado como dados, mais a metade dirigida ao Director.
    pub warning: harness_app::mirror::MirrorWarning,
}

enum Msg {
    OutsideWork {
        reply: oneshot::Sender<Option<(String, harness_app::mirror::MirrorWarning)>>,
    },
    SetOutsideWork {
        found: Option<Finding>,
        reply: oneshot::Sender<()>,
    },
    Agents {
        reply: oneshot::Sender<Vec<AgentProfile>>,
    },
    SetAgents {
        next: Vec<AgentProfile>,
        reply: oneshot::Sender<Result<Vec<AgentProfile>, String>>,
    },
    AddAgentFromTemplate {
        template_id: String,
        reply: oneshot::Sender<Result<AgentProfile, String>>,
    },
    DuplicateAgent {
        agent_id: String,
        reply: oneshot::Sender<Result<AgentProfile, String>>,
    },
    RemoveAgent {
        agent_id: String,
        reply: oneshot::Sender<Result<Vec<AgentProfile>, String>>,
    },
    /// Reescreve o ficheiro dos agentes tal como está. Serve o arranque, que
    /// normaliza a tripulação lida do disco e quer o disco a concordar.
    SaveAgents {
        reply: oneshot::Sender<Result<(), String>>,
    },
    Projects {
        reply: oneshot::Sender<Vec<Project>>,
    },
    AddProject {
        project: Box<Project>,
        reply: oneshot::Sender<Result<Project, String>>,
    },
    UpdateProject {
        project: Box<Project>,
        reply: oneshot::Sender<Result<Project, String>>,
    },
    RemoveProject {
        id: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

struct Registry {
    paths: AppPaths,
    agents: Vec<AgentProfile>,
    projects: Vec<Project>,
    outside_work: Option<Finding>,
}

impl Registry {
    fn save_agents(&self) -> Result<(), String> {
        paths::write_json(&self.paths.agents_file(), &self.agents)?;
        self.materialise_skills();
        Ok(())
    }

    /// Write each agent's granted skills to its own directory, and sweep away
    /// what no agent claims any more.
    ///
    /// It happens here, in the one place that writes `agents.json`, for the
    /// reason this whole file exists: whoever changes the list is whoever
    /// writes it, in the same step. A skill revoked on the Agents screen is off
    /// disk before the reply comes back, so there is no window in which the
    /// profile says one thing and the directory a run loads says another.
    ///
    /// A failure here is not fatal and does not undo the save: the profile is
    /// the truth, and the next save writes the directory again. Losing the
    /// profile because a file could not be written would be the worse trade.
    fn materialise_skills(&self) {
        let root = self.paths.root();
        for profile in &self.agents {
            if let Err(e) = harness_app::grants::materialise(root, profile) {
                eprintln!("could not write {}'s skills: {e}", profile.id);
            }
        }
        let live: Vec<std::path::PathBuf> = self
            .agents
            .iter()
            .map(|a| harness_app::grants::agent_skills_dir(root, &a.id))
            .collect();
        if let Ok(entries) = std::fs::read_dir(root.join("skills")) {
            for entry in entries.flatten() {
                if !live.contains(&entry.path()) {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    fn save_projects(&self) -> Result<(), String> {
        paths::write_json(&self.paths.projects_file(), &self.projects)
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Msg>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                Msg::OutsideWork { reply } => {
                    let _ = reply.send(
                        self.outside_work
                            .as_ref()
                            .map(|f| (f.said.clone(), f.warning.clone())),
                    );
                }
                Msg::SetOutsideWork { found, reply } => {
                    self.outside_work = found;
                    let _ = reply.send(());
                }
                Msg::Agents { reply } => {
                    let _ = reply.send(self.agents.clone());
                }
                Msg::SetAgents { next, reply } => {
                    self.agents = agents::normalise(next);
                    let saved = self.save_agents().map(|()| self.agents.clone());
                    let _ = reply.send(saved);
                }
                Msg::AddAgentFromTemplate { template_id, reply } => {
                    let taken: Vec<String> =
                        self.agents.iter().map(|a| a.id.clone()).collect();
                    let created = match agents::from_template(&template_id, &taken) {
                        Some(created) => created,
                        None => {
                            let _ = reply.send(Err(format!(
                                "there is no template called {template_id}"
                            )));
                            continue;
                        }
                    };
                    self.agents.push(created.clone());
                    let _ = reply.send(self.save_agents().map(|()| created));
                }
                Msg::DuplicateAgent { agent_id, reply } => {
                    let original = self.agents.iter().find(|a| a.id == agent_id).cloned();
                    let Some(original) = original else {
                        let _ =
                            reply.send(Err(format!("no agent profile called {agent_id}")));
                        continue;
                    };
                    let taken: Vec<String> =
                        self.agents.iter().map(|a| a.id.clone()).collect();
                    let copy = agents::duplicate(&original, &taken);
                    self.agents.push(copy.clone());
                    let _ = reply.send(self.save_agents().map(|()| copy));
                }
                Msg::RemoveAgent { agent_id, reply } => {
                    if agent_id == agents::DIRECTOR_ID {
                        let _ = reply.send(Err(
                            "the Director cannot be removed: the review loop needs it"
                                .to_string(),
                        ));
                        continue;
                    }
                    if !self.agents.iter().any(|a| a.id == agent_id) {
                        let _ =
                            reply.send(Err(format!("no agent profile called {agent_id}")));
                        continue;
                    }
                    self.agents.retain(|a| a.id != agent_id);
                    let saved = self.save_agents().map(|()| self.agents.clone());
                    let _ = reply.send(saved);
                }
                Msg::SaveAgents { reply } => {
                    let _ = reply.send(self.save_agents());
                }
                Msg::Projects { reply } => {
                    let _ = reply.send(self.projects.clone());
                }
                Msg::AddProject { project, reply } => {
                    let project = *project;
                    self.projects.push(project.clone());
                    let _ = reply.send(self.save_projects().map(|()| project));
                }
                Msg::UpdateProject { project, reply } => {
                    let project = *project;
                    let slot = self.projects.iter_mut().find(|p| p.id == project.id);
                    let Some(slot) = slot else {
                        let _ = reply
                            .send(Err(format!("unknown project {}", project.id)));
                        continue;
                    };
                    *slot = project.clone();
                    // O Modo Espelho é uma casa só, não uma bandeira por
                    // projecto (#65): quem a reclama tira-a a quem a tinha.
                    if project.mirror {
                        harness_app::projects::only_mirror(&mut self.projects, &project.id);
                    }
                    let _ = reply.send(self.save_projects().map(|()| project));
                }
                Msg::RemoveProject { id, reply } => {
                    self.projects.retain(|p| p.id != id);
                    let _ = reply.send(self.save_projects());
                }
            }
        }
    }
}

/// A ponta pública. Cada método é uma ida e volta ao actor; nada muda de estado
/// deste lado.
#[derive(Clone)]
pub struct RegistryHandle {
    tx: mpsc::Sender<Msg>,
}

impl RegistryHandle {
    /// Levanta o actor com o que estava no disco. A normalização da tripulação
    /// acontece antes de entrar, para que o dono nasça já com o estado certo.
    pub fn spawn(paths: AppPaths, agents: Vec<AgentProfile>, projects: Vec<Project>) -> Self {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let registry = Registry {
            paths,
            agents,
            projects,
            outside_work: None,
        };
        tauri::async_runtime::spawn(registry.run(rx));
        Self { tx }
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Msg) -> Result<T, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(make(reply_tx))
            .await
            .map_err(|_| "the registry is not running".to_string())?;
        reply_rx
            .await
            .map_err(|_| "the registry dropped the reply".to_string())
    }

    // ---- agentes ----

    /// A tripulação. Um actor caído devolve uma lista vazia em vez de rebentar:
    /// só acontece a fechar, e nessa altura ninguém precisa de perfis.
    /// O achado, nas duas formas que os dois leitores querem: o parágrafo para
    /// o prompt do Director, os números para a janela.
    pub async fn outside_work(&self) -> Option<(String, harness_app::mirror::MirrorWarning)> {
        self.ask(|reply| Msg::OutsideWork { reply })
            .await
            .unwrap_or_default()
    }

    pub async fn set_outside_work(&self, found: Option<Finding>) {
        let _ = self.ask(|reply| Msg::SetOutsideWork { found, reply }).await;
    }

    pub async fn agents(&self) -> Vec<AgentProfile> {
        self.ask(|reply| Msg::Agents { reply })
            .await
            .unwrap_or_default()
    }

    /// O perfil de um id, caindo para um trabalhador quando é desconhecido.
    /// Certo para atribuir trabalho; errado para uma conversa, que tem de falar
    /// como o perfil que diz ser — ver `agent_exact`.
    pub async fn agent(&self, id: &str) -> Option<AgentProfile> {
        let agents = self.agents().await;
        agents::find(&agents, id).cloned()
    }

    /// O perfil de um id, ou nada.
    pub async fn agent_exact(&self, id: &str) -> Option<AgentProfile> {
        self.agents().await.into_iter().find(|a| a.id == id)
    }

    pub async fn set_agents(&self, next: Vec<AgentProfile>) -> Result<Vec<AgentProfile>, String> {
        self.ask(|reply| Msg::SetAgents { next, reply }).await?
    }

    /// Acrescenta um perfil a partir de um modelo. Os modelos são um menu: nada
    /// é instalado até isto ser chamado.
    pub async fn add_agent_from_template(
        &self,
        template_id: &str,
    ) -> Result<AgentProfile, String> {
        self.ask(|reply| Msg::AddAgentFromTemplate {
            template_id: template_id.to_string(),
            reply,
        })
        .await?
    }

    pub async fn duplicate_agent(&self, agent_id: &str) -> Result<AgentProfile, String> {
        self.ask(|reply| Msg::DuplicateAgent {
            agent_id: agent_id.to_string(),
            reply,
        })
        .await?
    }

    /// Remove um perfil. Todos são opcionais menos o Director, de que o ciclo
    /// de revisão precisa.
    pub async fn remove_agent(&self, agent_id: &str) -> Result<Vec<AgentProfile>, String> {
        self.ask(|reply| Msg::RemoveAgent {
            agent_id: agent_id.to_string(),
            reply,
        })
        .await?
    }

    pub async fn save_agents(&self) -> Result<(), String> {
        self.ask(|reply| Msg::SaveAgents { reply }).await?
    }

    // ---- projectos ----

    pub async fn projects(&self) -> Vec<Project> {
        self.ask(|reply| Msg::Projects { reply })
            .await
            .unwrap_or_default()
    }

    pub async fn project(&self, id: &str) -> Option<Project> {
        self.projects().await.into_iter().find(|p| p.id == id)
    }

    /// Regista um projecto já preparado. Preparar — validar a pasta, garantir o
    /// repositório, escolher o id — é do chamador; aqui só entra na lista.
    pub async fn add_project(&self, project: Project) -> Result<Project, String> {
        self.ask(|reply| Msg::AddProject {
            project: Box::new(project),
            reply,
        })
        .await?
    }

    pub async fn update_project(&self, project: Project) -> Result<Project, String> {
        self.ask(|reply| Msg::UpdateProject {
            project: Box::new(project),
            reply,
        })
        .await?
    }

    pub async fn remove_project(&self, id: &str) -> Result<(), String> {
        self.ask(|reply| Msg::RemoveProject {
            id: id.to_string(),
            reply,
        })
        .await?
    }
}
