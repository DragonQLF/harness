# Harness — Arquitetura Técnica

> Documento original fornecido antes do início da implementação. Preservado verbatim como referência.
> Para desvios e decisões tomadas durante a construção, ver docs/DECISIONS.md.

Implementação. O documento de arquitetura descreve *o que* o sistema faz; este descreve *como está construído*.

Stack: Tauri (Rust) + webview.

---

## 0. As três regras

Tudo o resto deriva daqui.

**1. O backend é a única fonte de verdade.** O frontend não tem estado próprio, só uma projeção do que recebeu. Não decide nada, não valida nada, não guarda nada que não venha do Rust.

**2. As dependências apontam para dentro.** O domínio não sabe que existe git, nem contentores, nem API de modelos, nem Tauri. Quem sabe disso são as bordas.

**3. Uma só coisa escreve estado.** Um único loop possui o estado do mundo. Tudo o resto envia mensagens.

A regra 2 é a que impede a separação de módulos de apodrecer, e a 3 é a que impede as corridas.

---

## 1. Onde as coisas correm mal

Vale a pena nomear os modos de falha antes da solução, porque quase todos parecem inofensivos no início.

**Módulos organizados por substantivo.** `kanban/`, `agentes/`, `memoria/`, `git/` — parece limpo, e três semanas depois todos importam todos. Em Rust um ciclo entre crates nem compila, e a saída fácil é fundir tudo numa crate gigante. A separação tem de ser por *direção de dependência*, não por assunto.

**Frontend como segunda fonte de verdade.** Arrastas um cartão, o React atualiza local para parecer rápido, o backend recusa a transição, e agora tens duas realidades. Piora com múltiplas janelas.

**Lock mantido através de um `.await`.** O padrão óbvio em Tauri é `State<Mutex<App>>`. Assim que uma operação assíncrona acontecer com o lock na mão, bloqueias tudo — e num sistema com agentes de longa duração isso acontece no primeiro dia.

**Comandos de longa duração.** Um agente corre minutos. Se isso for um comando Tauri, a UI fica pendurada à espera de uma resposta que demora. Comandos são para respostas imediatas; trabalho longo é tarefa + eventos.

**IO bloqueante no runtime async.** `git2` e sistema de ficheiros são síncronos. Chamados diretamente num contexto tokio, param o executor. Tudo isso vai para `spawn_blocking`.

**Streaming de tokens direto para a IPC.** Emitir um evento por token afoga o webview. Precisa de agregação por janela de tempo.

**Orquestrador-deus.** Um módulo que conhece cartões, agentes, git, sandbox, custo e memória. É o destino natural se não houver regra que o impeça.

---

## 2. Estrutura de crates

Workspace Cargo. A regra de dependência é imposta pelo compilador — não é convenção, é estrutura.

```
crates/
  domain/      tipos, máquina de estados, invariantes.  Zero IO.
  ports/       traits: GitPort, SandboxPort, ModelPort, StorePort, ClockPort
  engine/      orquestrador: consome intents, aplica a máquina, emite eventos
  adapters/
    git/       git2 + worktrees
    sandbox/   contentores
    model/     ver secção 6 — a bifurcação
    store/     JSONL, ficheiros markdown
  app/         shell Tauri: IPC, janelas, ciclo de vida
```

| Crate | Pode depender de |
|---|---|
| `domain` | nada |
| `ports` | `domain` |
| `engine` | `domain`, `ports` |
| `adapters/*` | `domain`, `ports` |
| `app` | tudo — é o único sítio onde se faz a ligação |

**`engine` nunca depende de `adapters`.** Recebe implementações dos traits por injeção. É isto que torna a máquina de estados testável sem git, sem contentor e sem modelo — e a máquina de estados é a peça mais importante do sistema.

**`domain` sem IO é inegociável.** Assim que lá entrar um `std::fs`, deixa de ser testável em milissegundos e a disciplina cai.

---

## 3. Concorrência: um loop, sem locks

O estado do mundo vive numa só tarefa. Nada de `Mutex` partilhado.

```
                    ┌──────────────┐
  IPC (frontend) ──►│              │
  timers         ──►│  mpsc queue  │──► loop do engine ──► estado
  runs de agente ──►│              │         │
                    └──────────────┘         ├──► StorePort (append log)
                                             └──► broadcast ──► frontend
```

O loop é sequencial: retira uma mensagem, valida contra a máquina de estados, aplica, persiste, difunde. Não há acesso concorrente ao estado, logo não há corridas na transição de cartões — que é onde elas iriam doer.

Trabalho longo nunca corre dentro do loop. É `tokio::spawn`, com um handle guardado, e comunica de volta pela mesma fila. O loop mantém-se responsivo mesmo com seis agentes a trabalhar.

**Cancelamento.** Cada run recebe um `CancellationToken`. Teto de gasto, override, shutdown — tudo cancela pelo mesmo mecanismo, e o run tem de garantir o commit `wip:` no caminho de cancelamento. Se isso não estiver escrito no dia um, nunca funciona.

**Backpressure.** A fila é limitada. Cheia, o produtor espera. Ilimitada esconde problemas até rebentar a memória.

---

## 4. IPC: intents para dentro, eventos para fora

Duas direções, com formas diferentes de propósito.

**Intents** (frontend → backend, comandos Tauri): `move_card`, `override_card`, `send_message`, `create_workspace`. Devolvem apenas aceite/rejeitado, nunca o resultado do trabalho. Retorno imediato.

**Eventos** (backend → frontend, broadcast): `card_changed`, `agent_output`, `director_comment`, `cost_updated`. É a única forma de o frontend saber o que se passa.

**`snapshot`** — o comando que a maioria esquece. Ao montar, ao reabrir uma janela, ao recuperar de uma falha de IPC, o frontend pede o estado completo e substitui o que tinha. Sem isto, um evento perdido deixa a UI errada para sempre e ninguém percebe porquê.

Numeração de eventos: cada um leva um número de sequência. O frontend deteta buracos e pede snapshot. Barato de fazer, e sem isso a divergência é invisível.

**UI otimista, com reconciliação.** Arrastas um cartão, a UI move-o imediatamente e marca-o como pendente. O evento confirma ou reverte. O estado local só existe entre o gesto e a confirmação — nunca como verdade.

**Streaming.** Saída de agentes agregada por janela (~100ms) antes de emitir. Um evento por token afoga a webview.

**Segredos nunca atravessam a IPC.** Chaves ficam no Rust. O frontend nunca vê valores, só estado de configuração ("presente" / "em falta").

---

## 5. Ports

```rust
trait GitPort {
    fn create_worktree(&self, card: CardId, base: &str) -> Result<WorktreePath>;
    fn commit(&self, wt: &WorktreePath, msg: &str, trailers: Trailers) -> Result<Sha>;
    fn rebase_onto(&self, wt: &WorktreePath, target: &str) -> Result<RebaseOutcome>;
    fn remove_worktree(&self, wt: &WorktreePath) -> Result<()>;
}

trait SandboxPort {
    fn spawn(&self, spec: SandboxSpec) -> Result<SandboxHandle>;
    fn exec(&self, h: &SandboxHandle, cmd: Command) -> Result<Output>;
    fn kill(&self, h: SandboxHandle) -> Result<()>;
}
// Permissões e hooks do Claude Code NÃO são isolamento — ver secção 7.

trait AgentPort {
    async fn run(&self, spec: RunSpec, tx: Sender<RunEvent>, cancel: CancellationToken)
        -> Result<RunOutcome>;
    async fn resume(&self, session: SessionId, prompt: String, tx: Sender<RunEvent>,
                    cancel: CancellationToken) -> Result<RunOutcome>;
}

trait StorePort {
    fn append_event(&self, e: &Event) -> Result<()>;
    fn read_memory(&self, ws: &WorkspaceId, path: &str) -> Result<String>;
    fn write_memory(&self, ws: &WorkspaceId, path: &str, content: &str) -> Result<()>;
}

trait ClockPort { fn now(&self) -> DateTime<Utc>; }
```

`ClockPort` parece exagero até se tentar testar leases, keep-alive de cache e timers de inatividade. Sem relógio injetável, esses testes esperam em tempo real ou não existem.

Todas as implementações envolvem IO síncrono em `spawn_blocking`.

---

## 6. Claude Code como runtime

**Decidido.** O loop de agente, as ferramentas, o acesso ao ficheiro, as sessões e os subagentes vêm do Claude Code. O harness é orquestrador e superfície, não motor.

### O que deixa de ser preciso construir

| Desenhado por nós | Já existe |
|---|---|
| loop de tool use, retries, parsing | agent loop do Claude Code |
| arquivo de transcritos | sessões em JSONL no disco |
| consultas laterais visíveis em thread | mensagens de subagente no stream, com `parent_tool_use_id` a permitir reconstruir a árvore de aninhamento |
| teto de gasto por cartão | `--max-budget-usd` |
| comentário do Diretor em formato estruturado | `--output-format json` + `--json-schema` |
| skills | skills nativas, invocáveis com `/nome` no prompt |
| definições de agente | subagentes |

### Hooks — a espinha de tudo o resto

`PreToolUse`, `PostToolUse`, `Stop`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, registados com matchers.

São o ponto de integração mais importante, por duas razões:

**Telemetria sem parsing de prosa.** Cada uso de ferramenta gera um evento estruturado. O log da secção 13 do documento de arquitetura alimenta-se daqui, não de interpretar output.

**Enforcement que sobrevive a tudo.** Hooks correm antes de qualquer verificação de permissões, incluindo em `bypassPermissions`. É onde vive a zona congelada e o bloqueio de leitura de `.env` e caminhos de credenciais — a garantia mantém-se mesmo com permissões frouxas.

### Permissões

`allowedTools` explícito + `permissionMode: dontAsk`. Regras de negação com âmbito (`Bash(rm *)`) valem em todos os modos.

**Armadilha:** subagentes herdam `bypassPermissions`, `acceptEdits` e `auto` do pai e **não podem sobrepor-se por subagente**. Um pai permissivo concede silenciosamente o mesmo acesso a tudo o que gerar.

### Determinismo do prefixo

`--bare` não carrega os hooks de `~/.claude` nem o `.mcp.json` do projeto. Para a disciplina de cache isto é essencial: sem ele, configuração local do utilizador entra no prefixo e fragmenta-o de máquina para máquina. É recomendado para chamadas programáticas e está previsto tornar-se o default de `-p`.

### O que continua a ser nosso

Kanban e máquina de estados · workspaces · worktrees, commits e trailers · árvore de memória e encaminhamento do Diretor · agregação de custo entre runs · log de eventos · **limite de profundidade de fan-out**.

Esse último não vem de graça: subagentes podem gerar subagentes a qualquer profundidade. A regra de profundidade 1 tem de ser imposta por um hook que bloqueie a ferramenta de spawn quando já há um pai.

### Multi-modelo

Extensão futura, e fora do nosso código: o Claude Code aponta para outros fornecedores por variável de ambiente. Não é trabalho de arquitetura, é configuração do processo.

---

## 6b. A bifurcação que resta: como o Rust fala com o Claude Code

O Agent SDK é Python e TypeScript. O backend é Rust. Há duas formas.

### A — CLI como subprocesso

`claude -p --output-format stream-json --bare`, parsing do JSONL que sai do stdout. Rust puro, sem runtime adicional, uma dependência a menos no instalador.

Custo: aprovação de ferramentas não é interativa — defines permissões à cabeça e não podes decidir a meio. Hooks configurados por ficheiro de settings, não por callback.

### B — sidecar Node com o SDK TypeScript

O Tauri lança um sidecar; o Rust fala com ele por stdio. Ganhas objetos de mensagem tipados, **callbacks de aprovação de ferramenta em tempo real** e hooks como funções em vez de configuração.

Custo: Node no instalador, mais um processo a supervisionar, fronteira de serialização extra.

**A pergunta que decide:** precisas de decidir sobre uma ferramenta *enquanto* o agente corre? Se o bloqueio determinístico e a zona congelada se resolverem com regras estáticas, A chega. Se quiseres que o Diretor intervenha a meio de um run, B é o caminho.

O `AgentPort` é o mesmo nos dois casos — a decisão é adiável se a regra de dependência for respeitada.

---

## 7. Especificidades do Tauri

**Encerramento gracioso.** `on_window_event` com `CloseRequested` → `api.prevent_close()`, mostrar o ecrã de progresso, cancelar todos os runs, esperar pelos commits `wip:`, e só então fechar. É exatamente o comportamento que o conceito pede, e o Tauri suporta-o diretamente.

**Timer de inatividade.** Deteção no Rust, a partir do último intent recebido. No frontend seria enganado por separadores em segundo plano e não sobrevive a um reload.

**Sandbox no desktop — o ponto mais fraco, mas menos urgente.** Permissões e hooks do Claude Code dão **controlo de acesso**, não **isolamento**: um `PreToolUse` bloqueia caminhos e comandos de forma fiável, mas não contém um processo que já escapou. Para a v1, `allowedTools` restrito + negações com âmbito + cwd na worktree provavelmente chegam, e o contentor fica para depois. Isso muda o sandbox de bloqueador para dívida assumida — mas convém que seja decisão consciente e não esquecimento.

**SIGTERM aborta, não guarda.** Parar um run com SIGTERM aborta o turno em curso e termina a árvore de processos dos Bash em execução. Não há commit `wip:` nesse caminho. O encerramento gracioso tem de pedir paragem e esperar, e só usar SIGTERM como último recurso após timeout.

**Retenção de transcritos.** As sessões ficam em JSONL no disco, mas os transcritos de subagente são limpos ao fim de 30 dias por defeito. Se o arquivo servir de proveniência do brainstorm, o harness tem de copiar o que interessa para o workspace em vez de confiar na retenção.

**Preview do `_harness`.** Um segundo processo noutro porto, lançado pelo Rust, apresentado numa janela separada. Não é hot-reload da instância a correr — é uma instância nova ao lado.

**Múltiplas janelas.** O broadcast serve todas; cada uma pede o seu `snapshot` ao abrir. Se o estado vivesse no frontend, isto seria impossível.

---

## 8. Frontend

Apenas duas coisas: renderizar o estado recebido e emitir intents.

- Um único reducer, alimentado só por eventos e snapshots
- Nada de fetch, nada de lógica de negócio, nada de validação — se o backend rejeita, o backend explica
- Otimismo só entre gesto e confirmação
- Virtualização nas listas de saída de agentes; sessões longas produzem muito texto

Um teste que revela se a fronteira está bem: **desligar o backend e a UI deve ficar simplesmente parada**, não parcialmente funcional. Se continuar a funcionar em algum aspeto, tem estado que não devia ter.

---

## 9. Ordem de construção

1. `domain` — máquina de estados do cartão, com testes. Sem IO, sem UI, sem modelo.
2. `engine` + fila + `StorePort` em JSONL. Já dá para mover cartões e ver eventos a acumular.
3. `app` com IPC e um frontend mínimo. O ciclo completo a funcionar com um `AgentPort` falso.
4. `GitPort` — worktrees, commits, trailers.
5. `AgentPort` — Claude Code a sério, com hooks a alimentar o log.
6. `SandboxPort`.
7. Analista, telemetria, painéis.

A ordem não é arbitrária: cada passo é testável sem o seguinte. A máquina de estados com agentes reais em cima é impossível de depurar — os passos 1 a 3 existem para chegar a esse ponto com o núcleo já correto.

---

# POR DECIDIR

**1. Ponte Rust ↔ Claude Code: CLI ou sidecar Node** (secção 6b). Decide-se pela pergunta: é preciso aprovar ferramentas a meio de um run?

**2. Isolamento a sério, e quando.** Permissões não são sandbox. A v1 pode viver sem contentor; falta decidir se é dívida assumida ou requisito de lançamento.

**3. Persistência: JSONL puro ou SQLite?** O estado do Kanban tem de ser reconstruído no arranque a partir do log — com milhares de eventos fica lento. Snapshot periódico ou SQLite resolve. Adiável, não indefinidamente.

**4. Uma janela ou várias?** Afeta o broadcast e a gestão de foco para o keep-alive de cache.

**5. Formato do `RunEvent`.** O que atravessa a fronteira durante um run e com que granularidade. Agora tem uma base concreta: as mensagens do stream do Claude Code, filtradas e agregadas.

**6. Reusar as construções nativas ou manter as nossas?** Subagentes vs. `agents/*.md`, memória nativa vs. árvore Markdown, skills nativas vs. próprias. Reusar é muito menos trabalho e o modelo já conhece as convenções; manter as nossas dá controlo total sobre o prefixo. Provavelmente híbrido, mas precisa de ser escolhido peça a peça em vez de por omissão.

**7. Autenticação.** `claude setup-token` gera token de longa duração para scripts com subscrição; chave de API é a alternativa. Muda o modelo de custos para o utilizador final.
