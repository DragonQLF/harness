# Decision & Deviation Log

Registo de tudo o que desviou do documento original (docs/SPEC-ORIGINAL.md) e das
decisões tomadas em conjunto durante a construção. Ordem cronológica.

## Desvios ao spec

### 1. `git` CLI em vez de `git2` — crates/adapters/git
O spec pedia `git2 + worktrees`. Implementado com o executável `git` via subprocesso
(`CliGit`). Motivos: zero problemas de build no Windows, comportamento idêntico ao que
o utilizador vê no terminal. Continua escondido atrás do `GitPort` — trocar para git2
tocaria só esta crate.

### 2. `serde` dentro do `domain`
O spec dizia "domain depende de nada". Decidido explicitamente: serialização não é IO;
permitido `serde` derive no domínio para os eventos irem ao log JSONL e à IPC sem uma
camada de DTO duplicada. Nada com syscalls entra no domain.

### 3. `AgentPort` dyn-compatível (refutação à assinatura literal da secção 5)
`async fn` em trait não gera `dyn AgentPort`. Como o spec queria adiar A/B por trás do
trait, a assinatura usa boxed futures (`Pin<Box<dyn Future>>`) — implementações
trocáveis em runtime sem generics a propagarem-se.

### 4. `--bare` removido (contradiz a secção 6)
O spec recomendava `--bare` pelo determinismo do prefixo. Verificou-se empiricamente que
`--bare` **salta o carregamento das credenciais OAuth** — login por subscrição falha em
modo headless ("Not logged in"). Testes A/B provaram: sem `--bare` funciona, com falha.
Removido. O determinismo fica para um futuro `claude setup-token` (POR DECIDIR #7).

### 5. Secção 6b resolvida: **opção B — sidecar Node com Agent SDK**
Era a pergunta aberta #1. Escolhido B: processo Node (`sidecar/index.mjs`) hospeda
`@anthropic-ai/claude-agent-sdk`, protocolo JSON-lines por stdio. O `canUseTool` do SDK
pausa o agente e encaminha pedidos de aprovação para a UI (modal Permitir/Negar).
Subscrição Claude confirmada a funcionar através do SDK (smoke test com custo > 0 e
resultado correto). O CLI adapter (`model-claude`) mantém-se como alternativa.

### 6. Modos de permissão explorados empiricamente
A CLI enumera: `acceptEdits, auto, bypassPermissions, manual, dontAsk, plan`.
Perfil escolhido: workers = `acceptEdits` + allowlist com âmbito
(`Read Edit Write Glob Grep Bash(git *)`); director/chat = `dontAsk` + só-leitura.
`bypassPermissions`/`auto` deliberadamente fora do default (subagentes herdam-nos
silenciosamente — armadilha apontada pelo próprio spec). Modo é configurável por run.

## Adições fora do spec

- **Estado de auth na UI**: chip de estado (`agent_status`: CLI encontrado /
  credenciais presentes) + botão que abre terminal real com `claude` interativo para `/login`.
- **Captura de session_id** por run + botão "agent terminal" que faz `claude --resume <sid>`
  dentro da worktree do cartão — entrar na sessão do agente.
- **Director**: (a) revisor automático — quando um run termina OK, o engine faz commit com
  trailers, extrai o diff e lança um segundo run "director" cujo veredito JSON
  aprova (→ Done) ou rejeita com razão (→ Ready); (b) chat lateral interativo com o
  director, com contexto do quadro.
- **Recuperação de crash**: no arranque, cartões ficados em `Running` são marcados como
  run falhado automaticamente (replay do log).
- **Janela frameless** com titlebar custom (drag, minimizar/maximizar/fechar) e sidebar
  de chat; layout shell + stage + aside.

## POR DECIDIR — estado atual

| # | Questão | Estado |
|---|---|---|
| 1 | CLI vs sidecar | **Resolvido: B (sidecar)** |
| 2 | Contentores/sandbox | Adiado conscientemente; hoje = permission modes + cwd/worktree |
| 3 | JSONL vs SQLite | JSONL mantido; snapshot compaction pendente |
| 4 | Uma ou várias janelas | Uma, por agora |
| 5 | Granularidade RunEvent | Mensagens completas do stream (não por-token); agregador só se houver token streaming |
| 6 | Construções nativas vs próprias | Híbrido de facto: SDK sessions/resume nativos; skills/subagents próprios ainda não |
| 7 | Auth | Login interativo OAuth funciona headless; `setup-token` continua como opção futura |

## Dívida técnica conhecida

- Encerramento gracioso (secção 7): fechar a janela a meio de um run mata filhos sem
  commit `wip:` — o cancelamento in-app faz commit, o close da janela não espera.
- Hooks (telemetria estruturada, zona congelada, limite de profundidade de fan-out):
  não registados; enforcement atual = permission modes apenas.
- Custo agregado entre runs, timer de inatividade, drag&drop, diff viewer,
  inspector do event log: pendentes no UI/backend.
