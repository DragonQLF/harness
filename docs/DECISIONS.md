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

## Redesign v4 — multi-projeto, appdata e UI nova (2026-08-23)

Correspondente ao ficheiro de design `Harness v4.dc.html`. O que mudou e porque.

### 7. Multi-projeto: um engine por projeto
O design v4 introduz um seletor de projetos. O backend tinha um único
"workspace" sintético dentro de appdata. Agora existe um registo de projetos
(`projects.json`) e o `Workspace` (src-tauri) mantém um `ProjectRuntime` por
projeto — store, run log, git e engine próprios. Todos arrancam no `setup` para
que a Overview consiga contar trabalho sem se visitar cada quadro. Todos os
comandos IPC passaram a receber `projectId`.

Adotar uma pasta que não é repositório git é recusado, excepto se estiver vazia
(nesse caso é inicializada). Transformar a pasta de alguém em repositório não é
decisão nossa.

### 8. Worktrees saem de dentro do repositório
Antes: `<repo>/.harness/worktrees/<card>`. Um `git add -A` dentro de um run via
worktrees irmãs. Agora: `<appdata>/worktrees/<projeto>/<card>`. O `CliGit` recebe
a raiz das worktrees no construtor.

### 9. Bug real: o ciclo de aprovação estava partido
O `ApprovalRouter` inventava o seu próprio id (`apr-N`) e ignorava o do adapter.
A UI recebia o id do sidecar via `RunEvent::ApprovalRequested` e respondia com
esse — nunca correspondia, e cada pedido expirava (300s) como negado. Resolvido
mudando `Approver` para receber uma `ApprovalRequest` completa, com o
`request_id` cunhado pelo adapter a atravessar todo o percurso. Há teste.

### 10. Custo e turnos passam a ser persistidos
`RunOutcome::Completed` trazia `cost_usd` e era descartado no `finish_run`. Agora
`Event::RunFinished` carrega `cost_usd` e `turns`, o `Card` acumula-os, e a
Overview/Board/Sessions mostram gasto real em vez de estimativa. Novos campos são
`#[serde(default)]` — logs antigos continuam a reproduzir.

### 11. Transcrições de run em disco (`RunLogPort`)
As linhas de output viviam só em memória (últimas 40). Agora cada run tem
`projects/<id>/runs/<run>.jsonl` e o ecrã Sessions relê a transcrição depois de
reiniciar.

### 12. Perfis de agente são a política
`AgentProfile` ganhou modelo, capacidades, orçamento, `worktree`
(per-card/shared/none) e `reviewer` (director/you/nobody). São resolvidos num
`RunProfile` no momento em que o run começa; o engine deixa de ter política.
`Reviewer::Nobody` fecha o cartão sozinho, `Human` deixa-o em Review.

### 13. Sidecar vs CLI passa a ser decidido por run
`SwitchingAgent` consulta as settings a cada run, por isso o toggle em Settings
aplica-se imediatamente em vez de exigir reinício.

### 14. Empacotamento: nada vive na source depois de compilado
O `sidecar_script()` antigo caía no `CARGO_MANIFEST_DIR` — inútil numa app
instalada. Agora: `index.mjs` e `package.json` viajam como recursos do bundle,
são copiados para `<appdata>/sidecar` no arranque, e Settings instala lá as
dependências (`npm install`). Em desenvolvimento usa-se a checkout diretamente.
`agents.json` saiu de dentro do repositório workspace (onde era commitado pelo
`git add -A`) para a raiz de appdata.

### 15. Encerramento graciosos (dívida da secção 7 fechada)
`CloseRequested` cancela os runs ativos e espera pelos commits `wip:` antes de
destruir a janela, quando "Commit on close" está ligado.

### 16. `crates/app`: uma crate para o que não precisa de janela
O `cdylib` do Tauri não corre testes unitários no Windows
(`STATUS_ENTRYPOINT_NOT_FOUND`: falta o WebView2Loader ao lado do binário de
teste). Toda a lógica pura — paths, settings, perfis, router de aprovações,
registo de projetos, checks, métricas derivadas — foi para `crates/app`, testada
normalmente. O `src-tauri` fica só com cola: comandos IPC, engines por projeto e
staging do sidecar. O router de aprovações fala com a janela através de um trait
`Notifier`.

### 17. Engine partido em três ficheiros
`lib.rs` (actor, tipos, persistência), `runs.rs` (ciclo de vida dos runs),
`director.rs` (revisão + chat), `tests.rs`. Os ports passaram a `Arc<dyn ...>` —
os quatro parâmetros genéricos desapareceram.

### 18. Frontend reconstruído sobre o design v4
Novo `src/`: tokens em `styles/theme.css`, IPC tipado em `lib/ipc.ts`, um único
store em `state/store.tsx`, componentes e ecrãs separados. Decisão relevante: o
frontend **não** reaplica eventos de domínio (o `applyEnvelope` antigo tinha um
bug — usava `card_id` como `run_id`). Um evento agenda um novo snapshot; a
verdade continua a ser só do backend.

### 18b. Correção: transcrição em vez de interpretação
A primeira versão do frontend era uma reinterpretação em classes CSS — parecida,
mas não igual ao ficheiro de design. Refeita como **transcrição**: os estilos
inline do `Harness v4.dc.html` foram copiados elemento a elemento, e o
`theme.css` ficou só com tokens, keyframes, reset e as classes `hv-*` que
substituem os atributos `style-hover` do design. Consequências:

- a nav passou a ter a linha do Director, secções "This project"/"Records" e o
  cartão de gasto diário no fundo, como no design (antes era uma lista simples);
- a Home ganhou o cartão escuro (`--ink`) com saudação e gasto, a barra de
  intenção separada, "Waiting on you" e "In progress";
- o perfil de agente passou a ser a **gaveta** que sobe (era uma página);
- a página Code desenha o grafo de commits em SVG com as mesmas pistas do design
  (`LANES`); para isso o adapter git passou a marcar cada commit com
  `on_default`, e a página classifica cada linha (main/branch/root/merge/tail).

## POR DECIDIR — atualizado

| # | Questão | Estado |
|---|---|---|
| 3 | JSONL vs SQLite | JSONL mantido; compaction continua pendente |
| 4 | Uma ou várias janelas | Uma; o seletor de projetos substitui a necessidade |
| 8 | Custo do Director | O custo da revisão fica na transcrição do run, não soma ao cartão |
| 9 | Sandbox | Continua adiado: permission modes + worktree isolada |

## Dívida técnica conhecida (atualizada)

- Compaction do event log (o botão do design não existe).
- Sem diff viewer dentro da UI: o Director lê o diff, a pessoa abre a worktree.
- Grafo de commits desenhado como lista, não como as pistas com curvas do design.
- Os projetos pausados são respeitados no `start_run`, mas não param runs a meio.

## Um só Director, e git local sem remoto (2026-08-23)

### 19. O Director é um só, ao nível do workspace
Estava modelado por projeto — vivia dentro do engine — mas a UI apresenta-o
acima das secções de projeto, diz "watching · all projects" e conta diffs
"across N projects". A implementação estava a ditar o conceito, e na véspera
juntei-lhe um segundo `ask_director` ao nível do workspace: ficaram dois
Directors com dois prompts que não se conheciam.

Corrigido: **uma identidade, dois âmbitos**.

- Conversa: `crates/app/src/director.rs` (`ask_prompt`) recebe um resumo de
  **todos** os quadros, com o projeto aberto marcado; corre com `cwd` no projeto
  aberto, por isso pode ler código exactamente quando isso faz sentido. Testado
  na crate `app` — é construção de string, não precisa de janela.
- Revisão de diff: continua dentro do engine do projeto (`run_director_review`),
  porque é lá que estão a worktree e o board.

Removido do engine: `director_chat`, `Msg::DirectorChat`, o handle e o teste
respectivo. O engine deixou de ter noção de conversa. O comando IPC
`director_chat` desapareceu; ficou `director_ask(text, project_id?)`.

Também: o stream da conversa passou a usar `harness_engine::RunUpdate` em vez de
um `serde_json::json!` montado à mão — o listener do frontend é tipado contra
esse shape e o outro ia divergir.

### 20. Git local basta — não é preciso remoto
Pergunta do operador: "porque é que só funciona com repositórios?". A resposta é
que a isolação **é** o git (worktree por cartão), o diff é o que se revê e o
commit é o desfazer. Mas nunca foi preciso um **remoto**: `git init` local,
commits locais, worktrees locais; nada sai da máquina sem uma aprovação
explícita para `git push`. Isso agora está dito na UI (ecrã de primeira
utilização) e visível no ecrã Code, que mostra `local only` quando não há
`origin` (`CliGit::remote`).

Ao verificar isto apareceram duas lacunas reais:

- **Identidade de committer.** `ensure_workspace` só configurava `user.name` /
  `user.email` quando criava o commit inicial. Um repositório já existente numa
  máquina sem git config global falhava no primeiro commit de um agente — e
  falhava tarde, no fim do run. `add_project` passa a verificar
  `has_committer_identity()` (que conta a identidade herdada) e, só se não houver
  nenhuma, escreve uma **local** ao repositório.
- **Commit falhado em silêncio (bug).** `runs.rs` fazia
  `let _ = git.commit(...)`. Se o commit falhasse, o run reportava sucesso, o
  Director revia um diff vazio e nada explicava porquê. Agora o erro vai para o
  log do run como `Notice`, o cartão fica em Review à espera da pessoa, e a
  revisão automática é saltada. Com teste: um `GitPort` que recusa commits deixa
  o cartão em Review, sem `last_review`, com a razão no log.

Desvio ao plano: não há toast a dizer que a identidade local foi escrita — o que
importa é que os commits passam a funcionar, e o autor fica visível no ecrã Code.
Fica registado aqui em vez de na UI.

### 21. `cargo build` não produz uma app que corre (armadilha)
Passei duas instruções erradas ao operador: "faz `pnpm build` e depois
`cargo build`". Um binário construído por `cargo build` — debug **ou** release —
continua a apontar o webview para `build.devUrl`. Sem o Vite a correr, a janela
mostra a página de erro do Edge (`ERR_CONNECTION_REFUSED`), que foi exactamente
o que aconteceu. Só `tauri build` embute o `dist/` no binário.

Registado no README: usar sempre `pnpm tauri dev` ou `pnpm tauri build`
(`--no-bundle` para saltar os instaladores).

### 22. Porta de desenvolvimento fixa e própria
O webview em dev aponta para uma porta, e se essa porta estiver ocupada por
outra coisa a janela carrega a UI errada. Duas defesas:

- `strictPort: true` no Vite (já existia): a porta ocupada faz o Vite falhar em
  vez de saltar para a seguinte;
- a porta saiu do default 1420 do Tauri para **1751**, porque 1420 é o que todos
  os projetos Tauri usam e dois ao mesmo tempo colidiam.

Vive em dois sítios que têm de concordar — `PORT` em `vite.config.ts` e
`build.devUrl` em `tauri.conf.json` — com comentário a dizê-lo em ambos. Numa
app construída não há porta nenhuma: o frontend é servido pelo protocolo
`tauri://localhost`.

### 23. Janelas de consola a piscar (bug)
Abrir o separador Code fazia aparecer e desaparecer várias janelas de terminal.
Causa: o adapter de git lançava `git` com um `std::process::Command` simples, e
no Windows cada processo assim aloca uma consola. Um ecrã como o Code corre uma
dúzia de comandos (branches, languages, commits, activity, remote, …), logo uma
dúzia de piscas. O ecrã Agents fazia o mesmo, via `recent_commits`.

Resolvido com um único `git_command()` no adapter que junta `CREATE_NO_WINDOW`.
Os outros sítios que lançam processos já o tinham (sidecar, adapter da CLI,
checks) ou querem uma janela de propósito (abrir um terminal). `explorer` é uma
app gráfica e não aloca consola.

## O Director como assistente (2026-08-23)

### 24. "reading the board..." para sempre (bug no adapter)
O `drive()` do sidecar registava o `done` e **continuava a ler** o stdout. O
processo node fica vivo à espera de outro comando, portanto o stdout nunca
fecha: o future do run nunca resolvia, o `Done` nunca era publicado e a UI
ficava com o spinner eternamente. Um `break` no `done` resolve — e como já não
esperamos que o processo morra, passou a haver `kill_on_drop(true)` mais um
`kill()` explícito, senão ficava um node órfão por run.

### 25. Streaming a sério: deltas e raciocínio
`includePartialMessages: true` no SDK dá `stream_event` com
`content_block_delta`. O sidecar reencaminha `text_delta` como `delta` e
`thinking_delta` como `thinking`. Novos `RunEvent::Delta` / `Thinking`, marcados
como **efémeros**: aparecem ao vivo, não entram no log do run — o `Text` final é
o registo. A UI mostra o raciocínio no lugar do "reading the board…", e a
transcrição de uma sessão ganha uma linha viva antes de a definitiva chegar.

### 26. Sem conectores emprestados
O modelo estava a falar de conectores MCP (Linear, Notion, Gmail) não
autorizados: vinham da configuração da conta do operador. Os runs do Harness
passaram a ser isolados — `settingSources: []`, `mcpServers` só o nosso,
`strictMcpConfig: true`.

### 27. O Director actua no quadro (feature)
Era um comentador: descrevia botões e pedia ao operador para escolher de um menu
que não conseguia mostrar. Agora tem ferramentas próprias, servidas por um MCP
in-process do SDK: `create_card`, `move_card`, `approve_card`, `reject_card`,
`read_diff` e `open_screen` — esta última navega a janela do operador.

O caminho é `tool_request` / `tool_response` pelo stdio, gémeo do fluxo de
aprovações, com um `ToolRunner` novo no `RunSpec`. Implementação em
`src-tauri/src/director_tools.rs`, que reutiliza os mesmos comandos do engine
que a UI usa.

Duas descobertas empíricas ao ligar isto:

- `mcpServers` como **array** faz o SDK nomear o servidor pelo índice
  (`mcp__0__move_card`); tem de ser um objecto com a chave (`mcp__harness__...`).
- `permissionMode: "dontAsk"` **nega** tudo o que não está em `allowedTools` sem
  consultar o `canUseTool`. O chat do Director passou a `"manual"`: leitura está
  em `allowedTools` (o SDK auto-aprova entradas simples — avisa-o ele mesmo), e
  cada acção no quadro passa pelo painel de permissões do operador.

### 28. Factos em vez de adivinhação, mas sem despejar o diff
O Director inventava o conteúdo de uma worktree. Agora cada cartão em revisão
leva no prompt **quantos ficheiros mudaram, quais (até quatro nomes) e +/- de
linhas** — nunca o patch. Para ler a mudança a sério chama `read_diff`.

### 29. Navegar não é uma acção que se pede autorização
Mostrar um ecrã não muda nada, e o operador pediu-o explicitamente: quando ele
diz "mostra-me o cartão", a janela deve ir lá, não deve aparecer um pedido de
permissão. `mcp__harness__open_screen` e `mcp__harness__read_diff` passaram para
o `allowed_tools` do chat do Director — o SDK auto-aprova entradas simples, logo
navegar e ler um diff acontecem sem interromper. Tudo o que **altera** o quadro
(criar, mover, aprovar, rejeitar, apagar) fica de fora do `allowed_tools` de
propósito, e portanto continua a passar pelo painel de permissões.

A instrução também saiu do prompt para a **descrição da ferramenta**: é lá que
diz "quando pedirem para ver algo, chama isto primeiro; apontar para o ecrã é a
resposta". O prompt ficou só com o que ele sabe e como se deve comportar.

### 30. Apagar cartões (feature em falta)
O Director dizia, com razão, que não conseguia apagar: o domínio não tinha o
conceito. Adicionado `Command::DiscardCard` / `Event::CardDiscarded` — o cartão
sai do board, o log guarda o facto e a razão. Recusado enquanto o cartão está a
correr: primeiro pára-se o run, senão apagava-se debaixo de um processo vivo.

O engine limpa a worktree ao processar o discard (ninguém mais o faria, o board
já se esqueceu do cartão). Existe como comando IPC (`discard_card`), como
ferramenta do Director (`delete_card`, que pede autorização por ser destrutiva) e
como ✕ em cada cartão do quadro, com confirmação quando há trabalho em revisão
que ninguém viu.

### 31. Streaming de raciocínio: depende do modelo
O encanamento está completo — `includePartialMessages`, `thinking_delta` →
`RunEvent::Thinking`, `maxThinkingTokens` configurável, efémero no log. Medido:
**haiku emite thinking deltas, sonnet e opus não** nesta versão do SDK/CLI. Como
o Director corre em Opus por defeito, o operador via um spinner sem conteúdo
durante os segundos em que ele trabalhava.

Em vez de fingir, o dock passou a mostrar o progresso que **todos** os modelos
dão: as chamadas de ferramenta ("reading the diff…", "opening the screen…"). O
texto continua a chegar em deltas para todos os modelos.

