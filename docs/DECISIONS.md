# Decision & Deviation Log

Registo de tudo o que desviou do documento original (docs/SPEC-ORIGINAL.md) e das
decisões tomadas durante a construção. Os números são identificadores estáveis —
o texto refere-se a eles constantemente.

> **Regras deste ficheiro:** blocos são *append-only* — nunca re-anexar a cauda;
> cada decisão aparece exactamente uma vez, por ordem numérica. A dívida técnica
> vive em `docs/DEBT.md`, reescrito a cada passagem em vez de acumulado.

## Sessões

| Data | Decisões | Tema |
|---|---|---|
| — | 1–6 | Spec original: desvios registados e adições fora dele |
| 2026-08-23 | 7–18b | Redesign v4: multi-projeto, appdata, worktrees fora do repo, UI nova |
| 2026-08-23 | 19–31 | Um só Director, git local sem remoto, assistente geral, streaming |
| 2026-08-23 | 32–44 | Conversas persistentes, segurança das aprovações, perfis |
| 2026-08-24 | 45–49 | Revisão externa: corrida no shutdown, actor bloqueado pelo git, override para Running |
| 2026-08-24 | 50–56 | Compaction, ts-rs, memória mínima, dependências, fan-out, Triador/Analista, e2e |
| 2026-08-24 | 57 | Mensagem de commit com o título do cartão |
| 2026-08-24 | 58–60 | report_work e memória fora do repositório |
| 2026-08-24 | 62–65 | Modo Espelho: zona congelada, build como check, instalar com volta |
| 2026-08-24 | 66 | Pathguard guarda por omissão |
| 2026-08-24 | 67–69 | Modo Destacado e Voz (fase 1 desenhada; fase 2 atrás de uma semana de uso) |
| 2026-08-26 | 78–79 | O Director vê o próprio histórico: self_report, read_docs, caixa de entrada e fecho do dia |
| 2026-08-28 | 80 | Tailwind v3: os tokens deixam de ser custom properties e o inline sai das vistas |
| 2026-08-28 | 87 | O estado da shell sai de trás dos mutexes: dois actores novos, mensagens em vez de locks |
| 2026-08-28 | 88–89 | O aviso de trabalho fora do quadro ganha ecrã; a derivação do RightNow auditada |
| 2026-08-28 | 90–92 | O aviso leva os factos, sobrevive ao arranque, e a máquina de estados deixa de estar em duplicado |
| 2026-08-28 | 93–96 | Skills e MCP por agente em runtime: plugin do Relay, declaração em vez de comando, auto-elevação recusada |
| 2026-08-30 | 99–101 | O token deixa de ser uma unidade de render; Agentes ganha lugar na barra; as Definições ganham secções |
| 2026-08-30 | 102 | A revisão automática deixa de ser um segundo Director; o #98 reforma-se |
| 2026-08-30 | 103 | Os agentes falam uns com os outros a meio do trabalho |
| 2026-08-30 | 104 | Dois browsers por agente: um que não guarda nada e um que guarda |
| 2026-08-30 | 106 | A linha de comandos é uma skill, e o Director tem-na |
| 2026-08-30 | 107 | Um agente pode ficar cercado a um projecto; quem manda no quadro revê-o |
| 2026-08-30 | 105 | Passagem de estrutura: a casca reparte-se por dono e a política sai dela |
| 2026-08-30 | 108 | Uma execução leva consigo o que levantou; o turno vazio deixa de passar por resposta |
| 2026-08-30 | 109 | O trabalho de fundo passa a ver-se: nível e não arestas, fora do fio |
| 2026-08-31 | 110 | Um resto é um processo agarrado a uma sessão sem execução viva; descoberta em vez de lock |
| 2026-08-31 | 111 | O run sobrevive à Relay: socket em vez de cano, com identidade conferida |

> Nota: o número 63 não existe — houve um salto ao numerar o Modo Espelho.
> Não reutilizar; os números são estáveis mesmo quando errados.

## Decisões

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

### 32. O chat do Director era um caminho paralelo ao engine
`ask_director` no `workspace.rs` era uma cópia à mão do ciclo de reencaminhamento
do engine: lançava o agente, reencaminhava eventos, publicava `RunUpdate`, com um
`RunId` descartável por mensagem e **sem escrever em log nenhum**. A transcrição
existia apenas no array `chat` do React, limpo a cada troca de projeto. Ou seja: o
frontend era a fonte da verdade da conversa, exactamente o que a arquitectura diz
que não deve acontecer.

Corrigido criando `src-tauri/src/chat.rs` — um runner de conversas que substitui
aquele bloco. Continua ao nível do workspace (decisão #19: o engine não tem
noção de conversa), mas agora persiste. Não é uma camada nova: é o mesmo trabalho,
num sítio só.

### 33. O `session_id` perdia-se em três sítios ao mesmo tempo
A razão pela qual reiniciar o Harness matava a conversa não era uma: eram três.

1. `resume_session: None` fixo no código — cada mensagem abria uma sessão Claude
   nova. A conversa nunca foi contínua, nem dentro da mesma execução.
2. O `session_id` devolvido pelo SDK era descartado: o `match` do forward tratava
   `Delta/Thinking/Text/ToolUse/Failed` e deixava cair o resto em `_ => {}`, logo
   `Started { session_id }` nunca chegava; o `Done` era republicado com
   `session_id: None`.
3. Nada era persistido — sem índice, sem log de chat.

### 34. Índice de conversas, não uma segunda base de dados
`crates/app/src/conversations.rs`: id do Harness, `session_id` nativo do Claude,
perfil, projeto opcional, título, timestamps, arquivado. Puro — sem I/O e sem
relógio, o shell injecta ids e tempos. Persistido em `conversations.json` ao lado
do `settings.json`, com o mesmo `write_json` atómico.

As **palavras** não estão lá: uma conversa é um `RunLogPort` como qualquer outro
run (decisão #11), um JSONL por conversa em `<appdata>/conversations/`. Isso
implicou uma variante nova em `RunEvent`, `UserMessage`, para que o turno do
operador viva no mesmo ficheiro que a resposta — é aditiva, logo logs antigos
continuam a ler. Duas cópias da mesma transcrição era o que havia a evitar: o
índice diz qual a sessão e qual o ficheiro, o ficheiro tem o texto.

Empírico, do SDK (`sdk.d.ts`): `sessionId` **não** pode ser combinado com
`resume` sem `forkSession`, por isso nunca cunhamos o id — deixamos o SDK
cunhá-lo e guardamos o que vem. E `total_cost_usd` documenta
"resumed sessions start fresh", logo o custo por mensagem soma-se ao total da
conversa em vez de o substituir.

### 35. Um resume que falha diz-se, não se esconde
Se pedimos resume e o run falha **antes** de qualquer `session_id` chegar, a
sessão nativa desapareceu. Nesse caso: o `session_id` é limpo, `resume_failed`
fica marcado, e vai um `Notice` para a transcrição a dizer que o texto acima
continua legível mas o modelo já não se lembra dele. A alternativa — tentar o
mesmo id para sempre — falharia em silêncio a cada mensagem.

### 36. O Director deixa de ser um gestor de software
O prompt dizia "You are the Director of Harness... you never write code
yourself", e com zero projetos abria a conversa a perguntar que repositório
adicionar. Isso fazia dele um gestor de tickets, não um assistente.

`chat_prompt` agora: identidade geral (software, investigação, negócio, planos,
projetos pessoais), regra explícita de **responder** em vez de fabricar trabalho,
e sem projetos é dito que isso "não é um problema a resolver antes de ser útil" —
sugerir um projeto é uma oferta, não um pré-requisito. O mesmo construtor serve
um especialista em chat directo, com `Speaker` diferente, para não haver dois
sítios a decidir como abre uma conversa.

Numa sessão retomada o prompt é só a mensagem mais um refresh dos quadros: a
identidade já está na sessão, reenviá-la fazia o modelo começar de novo.

Migração: o `brief` que **nós** shipámos ("Own the board...") é substituído no
`normalise`; um brief que o operador editou fica intocado.

### 37. O Director sabia de todos os projetos mas só podia agir em um
Assimetria real, encontrada a responder a uma pergunta do operador: `ask_director`
construía um brief de **todos** os quadros, mas `director_tools` recebia um único
`project_id` (o aberto) e as ferramentas no sidecar não tinham argumento de
projeto nenhum. Lia em todo o lado, escrevia onde a pessoa estava, e não
conseguia criar projetos.

Agora cada ferramenta de quadro aceita `project_id` opcional (default: o projeto
a que a conversa está fixada) e existem `list_projects` e `create_project`. Ambas
as novas passam pelas regras de #29: listar é leitura, criar altera e portanto
pede autorização.

### 38. "Always allow" guardava o nome nu da ferramenta (bug de segurança)
`settings.allow_always(&pending.tool)` gravava `"Bash"`. Aprovar um
`git status` uma vez autorizava **todos** os comandos de shell para sempre. A
revisão de segurança da sessão anterior olhou para o `respond_approval` e
validou-o contra *spoofing* do nome da ferramenta, mas não viu o âmbito.

`crates/app/src/allow.rs`: uma regra é a ferramenta **e** o prefixo do comando
(`Bash(git push …)`). Três invariantes, com testes:

1. uma chamada que traz comando só é coberta por uma regra que nomeia comando —
   logo uma entrada `Bash` nua não cobre nada;
2. o prefixo tem de terminar em fronteira de palavra (`git push` não cobre
   `git pushall`);
3. um comando com metacaracteres de shell nunca é coberto **nem** gera regra, por
   isso `git status; rm -rf /` não entra ao colo do `git status`.

Ficheiros antigos continuam a carregar (deserializador aceita string ou objecto).
Decisão tomada com o operador: uma entrada de shell sem âmbito — precisamente o
que o bug escrevia — passa a **inerte** e aparece riscada como "revoked" nas
Settings, em vez de ser honrada. Uma permissão que ninguém deu conscientemente
não se herda.

### 39. Duas dívidas da revisão de segurança anterior, fechadas
- `remove_worktree` usava `Path::starts_with`, que compara componente a
  componente: `<esperado>/../../outro` passava. Agora ambos os lados são
  canonicalizados antes de comparar, e um caminho que não resolve é recusado.
- `open_terminal_in` fora do Windows fazia `argv.join(" ")` para
  `x-terminal-emulator -e`, o que deixaria um `session_id` com espaços partir-se
  ou injectar. Passa a argumentos separados, sem o invólucro `cmd /K` que ali não
  significa nada.

### 40. Perfis: dois modos, e templates que não se instalam sozinhos
`AgentProfile` ganhou `team`, `chat_enabled`, `tasks_enabled`, `max_concurrent`,
`skills`, `reports_to`, `can_delegate`, `expected_output`, `escalate_to` — todos
`#[serde(default)]`, logo um `agents.json` antigo carrega e comporta-se como
antes (falar e receber cartões ficam ligados por omissão).

Os dois modos são: **chat directo** (conversa persistente com o especialista) e
**trabalho atribuído** (um cartão, com a worktree, orçamento e revisor do
perfil). `can_delegate` decide se as ferramentas que alteram quadros existem
para aquele perfil.

`templates()` devolve doze perfis (Director, PM, Researcher, Designer, Senior
Engineer, Builder, Editor, SEO, Ads, Analytics, Finance, Compliance). São um
**menu**: só um `agent_create_from_template` explícito instala algum. Uma
instalação nova continua com três perfis, não doze. O Director continua
obrigatório; `agent_remove` recusa-o.

### 41. Um resultado de erro tinha exactamente a forma de um sucesso (bug)
Descoberto ao verificar #35 com o SDK a correr, em vez de assumir. Retomar uma
sessão que já não existe **não falha**: o SDK emite uma mensagem `result` normal
com `is_error: true`, `num_turns: 0`, custo 0 e texto nenhum — e só lança a
excepção *depois*, quando o nosso `case "result"` já fez `return`. Resultado
medido antes da correcção:

```
{"kind":"done","session_id":"00000000-dead-...","cost_usd":0,"turns":0,"result":null}
```

Ou seja: a conversa aparecia respondida com uma resposta vazia. E não era só no
chat — **qualquer** run com resultado de erro (orçamento excedido, max turns,
erro de API) era reportado como `Completed`, o cartão era commitado e o Director
revia um diff de nada.

Corrigido na origem: o sidecar lê `is_error`/`subtype`/`errors` da mensagem
`result` e envia um campo `error` novo no evento `done` (aditivo, com
`#[serde(default)]`); o adapter transforma um `done` com erro em
`RunOutcome::Failed`. Depois disto, o mesmo teste devolve a razão verdadeira:

```
"error":"No conversation found with session ID: 00000000-dead-..."
```

A detecção de resume perdido no `chat.rs` passou a olhar para *este* run — houve
`started` ou texto? — em vez de para o estado guardado, que numa retoma está
sempre preenchido, e portanto nunca detectaria nada.

Verificado ao vivo, dois processos separados (que é o que um restart é): a mesma
sessão retomada lembra-se do que foi dito no processo anterior; um chat novo
recebe um `session_id` diferente e não sabe nada; uma sessão morta diz porquê.

### 42. Sessões de cartão também sobrevivem ao restart
Pergunta do operador ao ler #41: "não é para isto que serve o encerramento
gracioso, o commit wip?". Não — são metades diferentes do mesmo problema:

- o **commit wip** guarda o *trabalho*: os ficheiros ficam em git, nada se perde;
- o **session_id** guarda a *memória* do agente sobre esse trabalho.

Sem o segundo, depois de reiniciar o Harness o run seguinte no mesmo cartão
começava do zero: relia tudo, redecidia tudo, pagava tudo outra vez, e podia
refazer de maneira diferente aquilo que já estava meio feito. E o botão "agent
terminal" (`claude --resume <sid>`) respondia "no agent session for this card
yet", porque o mapa `sessions` do engine só existia em memória.

Aplicado o mesmo padrão das conversas, mas no sítio próprio: o **log de eventos**,
que já é a fonte da verdade do quadro (precedente da #10, onde custo e turnos
passaram a ser persistidos em vez de descartados).

- `Event::RunStarted` passou a carregar `worktree` e `branch` — não são
  deriváveis depois: o modo vem do perfil no momento em que o run começa, e o
  perfil pode ter mudado desde então. Campos `#[serde(default)]`, logo um log
  antigo continua a reproduzir (há teste que lê linhas antigas reais).
- `Event::AgentSession { card_id, session_id }` novo, com
  `Command::RecordSession`. Escrito quando o agente reporta a sessão (no init e
  outra vez no resultado), e ignorado se já for a mesma.
- `Card` ganhou `session_id`, `worktree`, `branch`.
- O engine reconstrói o seu mapa `sessions` do log no arranque
  (`restore_sessions`). Fica no engine e não no `Card` porque o que falta ao
  domínio é o *relógio*: `started_ms` vem do `ts_ms` do evento guardado.

### 43. Bug encontrado ao reordenar: um cartão ficava Running sem run
`start_run` decidia `StartRun` (o que marca o cartão Running e persiste) e **só
depois** criava a worktree. Se a worktree falhasse, a função devolvia erro com o
cartão já marcado Running e sem run nenhum a correr — preso até ao próximo
restart, onde a recuperação de crash o marcava como falhado.

Como o log agora precisa da worktree no `RunStarted`, a ordem inverteu-se por
necessidade: resolve-se o checkout primeiro, só depois se registra o run. O bug
desapareceu de graça, e ficou com teste (`a_failed_worktree_leaves_the_card_alone`).

### 44. A worktree partilhada era destruída a cada restart (bug ao lado)
`CliGit::create_worktree` **remove e recria**: faz `worktree remove --force` e
`branch -D` antes de criar. Para uma worktree por cartão isso é o que se quer —
começa limpa. Para a **partilhada** é perda de dados: os commits daquele ramo
ficam inalcançáveis.

Dentro de uma sessão não acontecia, porque o engine guardava `shared_worktree`
em memória. Depois de reiniciar, esse campo voltava a `None` e o primeiro run
partilhado apagava o ramo com o trabalho todo.

Corrigido com um método novo no `GitPort`, `worktree_path(name)` — "onde é que
isto viveria" — para o engine poder adoptar um checkout existente em vez de o
reconstruir. Não há adivinhação por nomes de ramo, e há teste: dois engines
sobre o mesmo log, `create_worktree` chamado exactamente uma vez.

Nota de teste: os `FakeGit` passaram a ter uma raiz própria por instância. Com
uma raiz fixa partilhada, um teste via o checkout deixado por outro — e agora
que "já existe?" é uma pergunta com consequências, isso deixou de ser inócuo.

### 45. Dois `git commit` na mesma worktree ao fechar (bug)
O `shutdown` cancelava os tokens e **não esperava**: commitava o wip ele próprio
enquanto a tarefa do run — vendo o cancelamento — commitava também. Dois commits
concorrentes na mesma worktree: o segundo falha com `index.lock`, ou o primeiro
captura um estado a meio de uma escrita.

Corrigido na propriedade: quem commita é a tarefa do run, sempre foi ela que sabia
o *outcome*. O `shutdown` agora cancela e **espera** pelos handles (grace de 15s),
e não commita nada por si. Como a política `commit_wip_on_close = false` tem de
continuar a significar algo, cada run leva um `commit_on_cancel` partilhado: o
shutdown limpa-o antes de cancelar quando a política está desligada, e a tarefa
respeita-o no momento do commit. Um cancelamento *dentro* da app continua a
commitar — a bandeira só é limpa para o fecho.

Testes: um agente falso que dorme 200ms depois do cancelamento prova a ordem
(`agent-stopped` antes de `wip`, exatamente uma vez); e com a política desligada
nenhum commit acontece.

### 46. O actor parado segundos atrás do git (bug)
`create_worktree` (que faz `worktree remove --force` + `branch -D` +
`worktree add`), `remove_worktree` e o `diff_summary` da revisão corriam dentro do
loop do actor via `block_in_place`. Nesses segundos não entrava mensagem nenhuma:
nem snapshot, nem `cancel_run`, nem `RunDone` — e com a fila limitada, os
produtores bloqueavam. Não se conseguia cancelar um run enquanto outro criava
worktree.

Corrigido caso a caso:

- **Criar worktree** passou para `spawn_blocking`, com o resultado a voltar como
  mensagem nova (`Msg::WorktreeResolved`). O `start_run` ficou em duas fases: a
  primeira valida e despacha, a segunda (`launch_run`) recebe a worktree pronta e
  registra o run. A ordem da decisão #43 sobrevive — o checkout resolve-se antes
  do `StartRun` ser persistido — só que agora através de uma fronteira de
  mensagem. Como o mundo anda entre as duas fases, `launch_run` repete as
  verificações (cartão ainda sem run, limite do agente).
- **Remover worktree no discard** é destacado e esquecido: o cartão já saiu do
  quadro, ninguém devia esperar pelo `rm -rf`.
- **Diff da revisão** passou para dentro da tarefa que lança o Director; o actor
  só emite o aviso "director is reading the diff" e lança.
- O `persist` ficou como estava: um append a JSONL é rápido, e envolvê-lo em
  mensagens complicaria todos os caminhos por nada.

### 47. Um override podia pôr um cartão a correr sem run (bug)
O `OverrideCard` validava razão e estado diferente, e mais nada. Um override para
`Running` produzia um cartão que o domínio não conseguia representar: `DiscardCard`
recusa, `FinishRun` recusa (`RunMismatch` com `current_run = None`), `StartRun`
recusa (`NotReady`) — preso para sempre.

Agora recusado à entrada, com erro próprio (`DecisionError::CannotOverrideToRunning`,
"only starting a run puts a card in Running"). Só o `StartRun` põe um cartão a
correr; o override continua a servir todos os outros estados.

### 48. `max_concurrent` passou a limitar
Era guardado no perfil e mostrado na UI, sem efeito nenhum. Agora viaja no
`RunProfile` até ao engine, que conta os runs activos com o mesmo `agent_id` e
recusa acima do limite com erro legível ("builder is already working on 1 card;
its limit is 1"). Um perfil editado à mão com `0` conta como 1 — "zero em
paralelo" não é um limite, é um perfil pausado. Há teste: dois cartões, mesmo
agente, limite 1 — o segundo é recusado e o cartão fica Ready; limite 2, passa.

### 49. O diff do Review, ficheiro a ficheiro
Nota da revisão: "sem diff viewer dentro da UI". Já existia — `card_diff` traz o
patch e o Review coloria-o linha a linha — mas era um bloco único, e num diff de
vinte ficheiros isso é o mesmo que não estar lá. Agora o patch é dividido por
ficheiro: cabeçalho com caminho e `+n −m` do próprio ficheiro, colapsável, sticky
ao fazer scroll. Sem syntax highlighting a sério — primeiro existir, depois ser
bonito.

### 50. Compaction: um snapshot em vez de milhares de eventos
`Event::BoardSnapshot { cards }` novo — no replay, substitui o quadro inteiro,
portanto é só mais um evento para os logs antigos. `StorePort::compact` (default:
recusar) reescreve o ficheiro com exatamente os eventos dados; o `JsonlStore`
escreve num irmão e faz rename, logo um crash a meio deixa ou o log antigo ou o
novo, nunca metade de cada.

Quando: no arranque, se o log passou `EngineConfig::compact_at` (1000). Tudo o que
o log disse já está no board nesse momento; escreve-se o snapshot, trunca-se,
`last_seq` continua de onde estava. Falhar compaction não é fatal: fica o log
longo, que é só o custo antigo. A recuperação de crash atravessa snapshots sem
saber que existem — um cartão Running dentro do snapshot continua a ser marcado
como falhado pelo caminho de sempre. Há teste dos dois lados (store e engine).

### 51. ts-rs: os tipos TypeScript nascem do Rust
`src/lib/types.ts` tinha 467 linhas escritas à mão a duplicar structs Rust; cada
campo novo se escrevia duas vezes e a divergência era silenciosa. Agora 28 tipos
(`Card`, `Snapshot`, `AgentProfile`, `Settings`, `Project`, …) derivam `TS` e são
gerados para `src/lib/generated/` por `pnpm codegen`
(= `cargo test --workspace --test export_types`). Os testes de export vivem nas
crates donas dos tipos, não no `src-tauri`: os testes unitários do binário Tauri
não correm no Windows (decisão #16), logo tudo o que precisasse deles saiu de lá
— nada saiu, porque os wrappers que ficam no shell continuam à mão.

Duas decisões dentro da decisão:

- **u64 → number**, não bigint. O que atravessa a IPC é um número JSON (carimbos
  de milissegundos e contadores); bigint seria fiel ao Rust mas mentiroso sobre
  o fio. A normalização corre no passo de geração.
- **Envelope / RunUpdate / RunLogLine** continuam à mão: são unions achatadas de
  eventos onde a UI lê campos soltos, e gerar a union exata quebraria mais do
  que protege. Estão marcados como exceção deliberada no cabeçalho do ficheiro.

### 52. Memória curada: o piso
O desenho completo — charter por projeto, árvore `memory/areas/`, índices
gerados, Director curador — espera pelo teto. O piso é agora real:
`charter.md` na raiz do projeto e `global.md` na appdata, ambos lidos com teto
(4000/1500 caracteres, cortados numa fronteira de linha) e entram em todo o lado:
no prompt de cada run de worker, no chat do Director (global sempre; charter só
do projeto aberto, senão cada turno paga todos os quadros). Quem escreve é o
operador; curadoria automática fica para quando houver o que curar.

### 53. Dependências entre cartões: ordem, não conflito
`Card.depends_on`, escrito por `SetDependencies` (valida cartões existentes e
recusa ciclos, incluindo o trivial). `StartRun` recusa enquanto alguma
dependência estiver no quadro sem estar Done — erro legível ("waiting on:
c_x (Ready)"). Descartar uma dependência liberta os dependentes, e o próprio
descarte leva a nota ("…; frees c_y"), porque uma regra que acontece em silêncio
não se distingue de um bug. Na UI, o Review mostra a fila ordenada pelo Triador
(#55) e o campo viaja nos tipos gerados (#51).

### 54. Fan-out limitado a um nível, no canUseTool
O sidecar agora sabe se o seu spec permite subagentes (`subagents` no RunSpec:
workers sim, revisão e conversas não). Dentro do `canUseTool`, a ferramenta Task
é negada fora dessa política e, dentro dela, enquanto `childDepth > 0`. O
contador sobe quando um Task é aprovado e desce no hook PostToolUse; se o hook
nunca chegar a correr, o contador fica alto e os spawns continuam negados —
falha fechada, que é a direção certa para um limite.

### 55. Triador e Analista
- **Triador**: `insights::triage` ordena a fila de Review por risco mecânico —
  superfície do diff (ficheiros ×4, linhas/25) mais espera (2/hora, ×6 ao fim de
  um dia) — com razões verificáveis em texto. O comando `review_queue` junta as
  peças (worktrees + log) e o Review usa-o para ordenar os chips e mostrar o
  número. Ponderação de ficheiros protegidos fica para quando o operador puder
  nomeá-los.
- **Analista**: `analyst_ask` monta as tabelas já calculadas (stats + atividade
  por projeto, JSON exato), abre uma conversa do Director e entrega-as ao prompt
  de analista: interpretar sem recontar, citar evidência com ids de cartão,
  terminar em cinco correções no máximo. Corre sob pedido, não semanal — o
  agendador é infraestrutura nova e ninguém pediu cron ainda.

### 56. Verificação end-to-end: feita, com modelo a correr
Herdada de três sessões atrás. `src-tauri/examples/e2e_sidecar.rs` leva um
cartão de Ready a Review headless, com sidecar, SDK e modelo reais (haiku,
orçamento limitado, revisor humano para provar a fila e não o Director):

```
card status: Some(Review)
session survived: true
transcript written: true
commit subject: harness: work for card c_e2e
E2E PASS: Ready → running → Review, committed.
```

Corre só de propósito (`cargo run --release --example e2e_sidecar -p harness`),
porque custa dinheiro e precisa de login. Foi corrido nesta máquina, hoje, e
passou.

### 57. A mensagem de commit é o título do cartão
Pergunta do operador ao ler o fluxo de commits: "os programadores tratam commits
como história, a mensagem não ajudava?". Não ajudava — era literalmente
"harness: work for card c_e2e", um uuid disfarçado em `git log`.

Agora o assunto de um run concluído é `harness: <título do cartão>`, com uma
segunda linha em prosa (`harness card c_x, run abc12345, by builder`) e os
trailers intactos — os ids continuam exatamente onde as máquinas os procuram
(trailers, que é o que o ecrã Code lê para desenhar as pistas). Título vazio
cai no formato antigo. Os wip mantêm-se genéricos ("wip: interrupted run"):
são andaimes transitórios numa worktree que o próximo run recria, e alongar o
`GitPort` por isso não pagava.

Há teste: o commit de um cartão chamado "Fix the retry loop" chama-se
"harness: Fix the retry loop" e continua a carregar `Harness-Card`.

### 58. `report_work`: o agente conta, o engine commita
Ferramenta nova que só os workers recebem (`report_work { summary,
memory_notes }`). **Não é o agente a commitar**: a pós-condição continua
decidida em Rust. O que a chamada faz:

- `summary` espera num slot do run e torna-se o **corpo** do commit que o task
  já ia fazer; o assunto continua a vir do board.
- `memory_notes` vai para o log como `Event::WorkReported` — ao evento, nunca
  ao git. Memória dentro do repositório significaria uma cópia por worktree e
  conflitos de escrita entre cartões concorrentes, o pior sítio possível para
  um.

O caminho é o do resto do engine: a ferramenta envia `Msg::WorkReport`, o actor
valida (`Command::ReportWork`; vazio dos dois lados → `EmptyReport`) persiste, e
**só então** fecha o ack da chamada — "reported" significa gravado, não
enfileirado. A primeira versão tinha a corrida clássica: o send resolve ao
entrar na fila, o agente acabava, o task commitava antes de o actor processar o
relatório, e o corpo saía genérico. Foi o ack que a fechou.

Decisões dentro da decisão:

- **Duas chamadas: a última ganha**, documentado no comando ("an agent refining
  itself beats two summaries glued together"). Recusar a segunda puniria o agente
  por se corrigir; acumular em silêncio era o que o handoff proibiu.
- **Silêncio é normal e nomeado**: sem chamada, o commit sai com o corpo
  genérico de sempre e um `Notice` — "the agent did not report its work" — no
  transcript. Nada de parsing da resposta final; texto livre que *parece* um
  resumo é o #41 outra vez.
- A ferramenta viaja no `allowed_tools` do worker: escrita nossa, não do
  repositório; pedir autorização por cartão seria ruído.

### 59. A memória mora fora do repositório
`<appdata>/projects/<id>/memory/charter.md` passa a ser o local preferido — ao
lado de `runs/` e das conversas. A leitura aceita as duas casas: o diretório de
memória primeiro, o `charter.md` na raiz do repositório (#52) ainda conta, por
respeito às mãos que lá já escreveram. `add_project` escreve um charter de
arranque na criação — nunca inventado depois; um ficheiro vazio diz ao operador
onde escrever.

### 60. O Curador: desenhado, à espera de notas
Perfil novo em `templates()`, dono de `areas/`, semanal ou no shutdown, lendo
`WorkReported` só de cartões em `Done` (notas de trabalho rejeitado são factos
falsos à espera de sítio). Índices gerados por código a partir do frontmatter,
destruições pelo painel de aprovações. **Não implementado nesta passagem** — a
árvore sem notas reais é cerâmica antes do barro; os eventos já estão a
acumular.

### 61. A janela entre as duas fases tinha a sua própria corrida (bug)
Apontado pelo operador ao reler #46: entre o despacho da worktree e a chegada
do `WorktreeResolved`, o cartão não estava em lado nenhum — `check_run_start`
corria nas duas fases, mas só olhava para `runs`, que só recebe no fim. Dois
arranques para o mesmo cartão passavam os dois crivos; com PerCard, o segundo
`create_worktree` fazia `remove --force` + `branch -D` por cima do checkout
que o primeiro acabara de criar, e o agente do primeiro ficava a trabalhar numa
diretoria recriada debaixo dele.

Correção: `starting: HashMap<card_id → agent_id>`, inserido antes do despacho,
consultado pelos dois crivos, removido quando o run se registra — e nos
caminhos de falha também, com um detalhe que custou um teste falhado: o
**próprio** marcador não pode contar na fase 2, senão o cartão bloqueia-se a si
próprio ("a start is already under way" contra si mesmo). O set existe para as
mensagens *entre* fases; dentro de um handler o actor não intercala.

Consequências medidas pelos testes:

- duplo arranque do mesmo cartão → **uma** chamada a create_worktree, o segundo
  despacho recusado com "a start is already under way for this card";
- limite do agente durante a janela → o segundo é recusado **antes de
  construir** (o crivo conta o que está a arrancar, não só o que corre), logo
  nem sequer há órfão;
- cartão descartado a meio da construção → o `StartRun` é recusado e a checkout
  acabada de criar é **removida** (`abandon_start`, destacada como o discard);
  checkouts adotados nunca são nossos para apagar, e o flag `created` na
  mensagem distingue.

### 62. A zona congelada é uma comparação de caminhos (feito)
O build cobre o código; não cobre `agents.json` nem afins — um agente que edite
a equipa não levanta um único erro do compilador. A regra deixou de ser lista de
módulos e passou a caminho: **um run escreve dentro da sua worktree e em mais
lado nenhum**, decidido no `canUseTool` antes da fila de aprovações (uma recusa
aqui não é pergunta para o operador).

Vive em `sidecar/pathguard.mjs`, módulo puro sem SDK — testável offline com
`pnpm test:sidecar` (8 testes): canonicalização resolve e segue o que existe,
recusa o que não resolve (#39 de novo; nada de `starts_with` componente a
componente), fronteira de diretório incluída (`/wt/c1` não contém `/wt/c11`),
e qualquer string sob uma chave terminada em `path` é candidata — ferramenta
nova cai no guardo por omissão. Ferramentas de escrita apenas; leituras ficam
livres. A negação aparece no transcript com o caminho.

Limite honesto, dito em vez de escondido: o Bash continua regido pela allowlist
e pelas aprovações — confinar um shell de verdade é sandbox (decisão #2,
adiada). Isto fecha os caminhos estruturados; não finge fechar o shell.

### 64. Instalar com volta — feito
Detecção do artefacto pendente + botão explícito são a parte fácil. O que manda:

- binário anterior guardado antes de trocar;
- marca de "arranque em curso" escrita antes de lançar a nova, limpa quando o
  `setup` completa;
- ao arrancar, marca órfã → repor o binário guardado e dizer porquê.

Dois arranques falhados revertem sozinhos. No Windows há um detalhe que decide a
implementação: o exe em execução não se substitui — troca por rename (velho
guardado primeiro, novo no lugar) é o caminho conhecido e o que se seguiu.

**Implementado** em `src-tauri/src/update.rs`, com quatro testes próprios: a
dança completa swap-rollback (o binário velho volta, a razão nomeia o cartão),
instalação falhada que repõe o original em vez de deixar a app morta, lista de
pendentes que não mostra promessa sem binário, e marker-sem-backup que explica
em vez de brickar. O build verde agora **copia o binário para
`updates/<cartão>/`** — worktrees são destruídas pelo próximo run do cartão, e
o instalador nunca depende de uma sobreviver. Comandos IPC: `updates_list` e
`update_install` (recusado com agentes activos; shutdown gracioso antes do
relançamento). Falta o fio na UI: um banner que leia `updates_list` e um botão
para `update_install`.

### 65. O build como check do engine (feito)
Depois do commit num run do `_harness`, o engine corre `pnpm tauri build
--no-bundle` (o `cargo build` sozinho produz uma app que não corre — #21),
destacado na disciplina de #46, com o cartão "a compilar". Verde → Review com
artefacto em `<appdata>/updates/<card-id>/` marcado com o SHA; vermelho →
Review com o erro no transcript e artefacto nenhum — nunca há artefacto de um
build que falhou. Fora do orçamento do modelo, resultado como facto nosso e não
relato dele (#41). **Implementado (#65)**: o build é do engine, destacado, com o cartão "a compiling" no transcript. O que falta é só a instalação:
um build verde seria convidativo a instalar algo de que não há volta — e isso
é armadilha, não feature.

### 66. O pathguard guarda por omissão
A inversão que a revisão do guardo impôs: em vez de `WRITE_TOOLS.has(toolName)` —
quatro nomes conhecidos, tudo o mais passava sem verificação nenhuma, incluindo
ferramentas MCP de terceiros com um campo de caminho — agora **qualquer** ferramenta
com input contendo caminhos candidatos é inspectada. A lista explícita passou a ser
de isenção: leituras (`Read/Glob/Grep/NotebookRead/LS`) e as ferramentas nossas
(`mcp__harness__*`, que actuam na app com a sua própria história de aprovação,
#27–#29). Junto: separadores só unificados no Windows (`a\\b` em Linux é um nome
válido), e um cwd não-resolvível devolve razão distinta em vez de acusar a raiz do
run como caminho culpado. Teste novo: uma ferramenta MCP desconhecida com
`{path: "/etc/passwd"}` é recusada com o caminho na mensagem.

### 105. A casca reparte-se por dono, e a política que estava dentro dela sai
Passagem de estrutura, não de funcionalidade: **nenhum nome de comando, nenhum
nome de evento, nenhuma forma de resposta e nenhum formato em disco mudou**. O
que mudou foi onde as coisas vivem e o que se pode testar.

**A auditoria começou por confirmar o que já estava certo, e é a metade mais
importante do resultado.** O `lib.rs` são 240 linhas que constroem o builder,
registam plugins, estado e comandos e tratam do ciclo de vida — sem uma linha
de negócio. Os `#[tauri::command]` são, quase todos, três linhas: recebem, vão
buscar o estado, chamam, devolvem. As crates `domain`/`ports`/`engine`/`app`
não importam `tauri` em lado nenhum. A separação por portos e adaptadores
**já cumpria** o que uma reestruturação por *feature slices* teria ido buscar,
e converter `crates/` numa árvore `features/` era desfazer cento e quatro
decisões para chegar ao mesmo sítio por outro caminho. Não se fez, e o que se
aplicou foram os princípios — coesão, dono único, fronteira fina — dentro da
forma que já existe.

Onde o corte vertical **pagou** foi exactamente nos dois sítios onde a
arquitectura não chegava: os ficheiros que a casca acumulou porque nada os
obrigava a ter um assunto.

**O `director_tools.rs` era um `match` de mil linhas.** Passa a cinco módulos
por dono do que muda — `board`, `crew`, `grants`, `projects`, `knowledge` — com
o `mod.rs` a ficar com o guardo da delegação, a escolha do projecto e a tabela.
A mensagem "there is no agent called X. The crew is: …" estava escrita seis
vezes; é agora o `crew::slot_of`.

**O `commands/system.rs` era "tudo o que não é quadro nem projecto"**: doze
assuntos que não se conhecem. Passa a `crew`, `approvals`, `inbox`, `updates`
e um `system` de 266 linhas que fala da própria shell.

**E três pedaços de política saíram da caixa do Tauri**, que é onde o CLAUDE.md
diz que não devem estar e onde nenhum teste lhes chega: a passagem do Curador
(`curator::run`), a regra que decide se um endpoint é local e se a cache serve
(`catalog`), e a dobra dos números de um agente por vários projectos
(`insights`). Sete testes novos, e um deles apanhou um defeito que ninguém
tinha visto: o índice do Curador era construído pela ordem do `read_dir`, que é
a do sistema de ficheiros — duas passagens sem promoções podiam reescrevê-lo
por outra ordem.

**O `me: OnceLock<Weak<Workspace>>` desapareceu, e com ele um silêncio.** A
razão escrita ao lado dele dizia que `self: &Arc<Self>` partiria dois
chamadores internos; um deles já era `self: &Arc<Self>` desde o #87 e o outro é
chamado uma vez. Mudaram-se os três e nenhum dos trinta chamadores externos
precisou de tocar — um `State<'_, Arc<Workspace>>` resolve sozinho. O que isso
apaga é o `arc() -> Option`, que fazia o `spawn_runtime` escrever
`review: self.arc().as_ref().map(hook)`: um `None` ali era o engine a nascer
**sem revisão automática**, com os cartões a acumularem-se em Review e nada em
lado nenhum a dizer porquê. Inalcançável, sim — mas uma defesa a esconder uma
invariante em vez de a afirmar.

**Os nomes dos canais passam a constantes (`events.rs`).** Treze literais em
sete ficheiros, com o `engine://run` em quatro deles. Uma letra trocada não
parte nada de visível: o `emit` devolve `Ok`, o `listen` fica calado, o ecrã não
se mexe. Ficam de fora o `menu://picked`, que já era constante pelo mesmo
argumento, e os dois do splash, que são duas janelas do frontend a falar uma
com a outra.

**No frontend, três regras que estavam escritas duas vezes.** A lista de shells
do `allow.rs` estava em três cópias — a gerada, que o `ruleIsRevoked` usa bem, e
duas escritas à mão no `Chat.tsx`, que é onde se decide o que uma permissão
permanente cobre. O `toolName` era dois, e o do `events.ts` só tirava o prefixo
do *nosso* servidor MCP: uma ferramenta de um servidor concedido lia-se
`mcp__figma__export` no feed e `export` no chat. E o `Review.tsx` ainda decidia
a cor de um veredicto por `label.startsWith("Approved")`, quando a
`ActivityRow` traz um campo `approved` cujo próprio doc explica porque é que
ler o prefixo é errado.

**Doze *casts* saíram do redutor de eventos.** O `RunUpdate` e o `RunLogLine`
escritos à mão estavam atrás do `RunEvent`, e o código lia os campos que
faltavam com `(u as RunUpdate & { ok?: boolean }).ok` — o compilador a ser dito
para não olhar, num ficheiro cuja promessa é que se pode olhar. Os campos estão
declarados; os *casts* desapareceram por consequência.

**O `Misc.tsx` eram três ecrãs.** "Worktrees, Activity and Settings": três
valores do `View`, três destinos da barra lateral, zero estado partilhado. O
que os juntava eram quatro `const` de classes, que são agora do `ui.tsx`.

**O `ipc.ts` fica um ficheiro, e isso é uma decisão.** Repartido por
funcionalidade poupava importações e custava a única coisa que dá: a lista
completa do que a janela pode pedir, ao lado do `invoke_handler`. É por essa
lista que se vê que seis comandos registados no Rust não têm porta nenhuma —
e foi ela que os apontou. Ganhou os cabeçalhos que lhe faltavam.

**E dois testes novos atravessam a fronteira que nenhum compilador atravessa.**
Uma ferramenta declarada ao modelo no `sidecar/index.mjs` e sem braço no
despacho do Rust falha em silêncio: o modelo chama-a e ouve "Relay has no tool
called X", como se tivesse inventado o nome. É a classe de erro que partir o
`director_tools.rs` podia introduzir, e não havia nada a apanhá-la. Os dois
lêem o despacho e comparam-no com o que é declarado, nos dois sentidos.

**O que se recusou apagar, e é o achado mais útil da passagem.** O
`ProjectPage` são 474 linhas que nada importa desde o `6bc7309`. Não é código
morto: é um ecrã que perdeu a estrada. É o único sítio da app que mostra o
detalhe de um projecto e o único consumidor de `project_detail`,
`project_checks`, `project_set_checks` e `project_run_checks`. O README diz que
o ecrã de Código mostra "branches, languages and checks" — não mostra checks
nenhuns, e ninguém mostra. Apagá-lo tirava a única implementação de uma
funcionalidade documentada para arrumar um aviso do compilador.

**Também se recusou:** unificar os cinco valores em que o `custom()` do ecrã de
Agentes discorda do `AgentProfile::default()`. Não é descuido — um perfil que o
Director cria vai trabalhar, um que sai daquele botão é um rascunho sem
worktree e revisto pelo operador. A razão passou a estar escrita lá; o `tsc` já
garante que a *forma* não diverge.

## POR DECIDIR

| Questão | Estado |
|---|---|
| Sandbox / contentores | Adiado conscientemente: permission modes + worktree isolada + pathguard; confinar shell de verdade é outro trabalho (#2, #62) |
| Auth headless | Login OAuth funciona; `claude setup-token` fica como opção futura (#4, #7) |
| Granularidade RunEvent | Mensagens completas do stream; por-token só se houver necessidade real (#5) |
| Uma ou várias janelas | Uma; o seletor de projetos substituiu a necessidade até agora (#4 v4) |
| Instalar actualizações sem sair da app | Feito (#64, com rollback); o banner lê `updates_list` e instala via `update_install` |




## Modo Destacado e Voz (2026-08-24)

O princípio: **voz conduz, ecrã mostra.** Fala são ~150 palavras por minuto e é
linear; um diff não se ouve. Por isso os painéis vêm primeiro e valem sozinhos —
a voz acaba por ser só outra forma de invocar as mesmas ferramentas. A fase 2
não dispensa a fase 1: entre elas há "usar durante uma semana".

### 67. `show(what, monitor, placement)` substitui o `open_screen`
O acoplamento primeiro: os nomes de ecrã estão escritos à mão num `z.enum`
dentro do sidecar; renomear um ecrã parte a ferramenta em silêncio — o problema
que o ts-rs resolveu (#51), por resolver na navegação. O `what` passa a ser um
**painel nomeado pelo backend**, com o enum gerado do Rust como os tipos.

Um painel é uma janela própria sem barra lateral (diff, transcrição, lista,
quadro), criada em runtime via `available_monitors()` + `WebviewWindowBuilder`;
"põe o diff no ecrã da direita" é aritmética sobre posições físicas.
`monitor`: índice | `primary` | `current` (rato). `placement`: `full` | `left` |
`right` | `corner`. Sem monitor, usa o que não tem a janela principal; só
havendo um, sobrepõe com margem.

E fica registo: "o Director levou-me ao painel X" é um facto, e hoje o
`open_screen` emite `ui://navigate` direto do `director_tools.rs`, fora do log
de eventos. Passa a evento.

### 68. Painéis fecham-se sozinhos
Cartão apagado, run terminado há N minutos → painel fecha. Sem isto ficam
ecrãs cheios de janelas mortas e a feature vira estorvo. Cada painel pede o seu
snapshot ao abrir, como a janela principal (#18) — o broadcast já serve todas
as janelas, que é porque o estado vive no backend.

### 69. Voz: Moonshine + Kokoro + Silero, inglês para já — desenhada, atrás da fase 1
STT Moonshine (27MB+, bate Whisper Tiny/Small sendo menor), TTS Kokoro-82M
(Apache 2.0, ~6x tempo real em CPU), VAD Silero. Inglês primeiro: em português o
Kokoro só tem pt-BR (3 vozes) e o Moonshine ficava de fora; em inglês o total de
pesos é ~100MB. `SttPort`/`TtsPort` em ports, `adapters/voice` implementa, o
engine **não sabe que existe áudio** — este campo muda todos os meses. Pesos
fora do binário, descarga na primeira utilização com hash; a app funciona sem
eles. Áudio nunca atravessa a IPC.

O que decide se presta não são os modelos: é a **deteção de turno** (VAD mal
calibrado corta frases) e depois o **barge-in**. Comandos destrutivos falados
exigem confirmação falada, nunca por omissão. Enunciados curtos são a fraqueza
do Kokoro (<10–20 tokens): agrupar — "card 42 is running, assigned to builder"
soa melhor que "42 running". Antes de código: gravar dez comandos reais e ouvi-
los transcritos; meia hora que decide o resto.

**Estado: desenhado.** A implementação segue a ordem do handoff — show() com um
monitor, depois placement multi-monitor, depois ciclo de vida, depois *uma
semana de uso* antes de qualquer áudio. Não construir 1 e 6 ao mesmo tempo:
depurar VAD e colocação de janelas em simultâneo torna impossível saber qual
das duas está a estragar a experiência.



### 70. A conversa não tem travões — cinco, fechados
Uma sessão real do Director: não conseguiu criar cartões, a recusa falava do
Director em terceira pessoa, e ele decidiu construir fora do quadro. O padrão:
quando algo falha, nada chega ao operador e ele improvisa.

- **`can_delegate` na origem**: perfis gravados antes do campo existirem
  herdam `false` do default da struct — o Director ficava surdo por migração.
  `normalise` força-o a true para o Director (agir no quadro É o trabalho dele,
  #27; quem o quiser cego remove o perfil).
- **Recusas falam de configuração, nunca de papéis**: "this profile does not
  have delegation enabled" — o leitor pode ser o próprio Director.
- **`AskUserQuestion` deixou de desaparecer**: intercepção no `canUseTool`,
  aviso no transcript + negação com razão legível ("say what you need in text").
  Confirmação que faltava: aprovações sem resposta **têm** timeout de 30 min
  (`approvals.rs`, `WAIT`) — não esperam para sempre, mas meia hora de spinner
  era o bug visível.
- **Identidade e regra de parar no prompt**: quem é, o que o perfil pode, e
  "se uma ferramenta do quadro é recusada, diz e para" — trabalho fora de
  cartões não tem review, história nem custo.
- **Stop na conversa**: o turno corria com token descartável, incancelável.
  Agora registra-se por conversa (`chat_turns`), comando `chat_stop`, botão
  ■ stop junto ao composer enquanto `chatBusy`. E trocar de conversa já
  limpava o busy (rede de segurança que lá estava); a causa real do
  "thinking…" preso era a pergunta sem resposta acima.



### 71. O trabalho saiu da worktree — três falhas de uma sessão real
Cartão `c_19a1`: dez ficheiros escritos em `C:\Users\nandi\site\`, tecto de
orçamento rebentado, `$0.00` no cartão, retoma com worktree vazia. Três falhas
independentes; duas fechadas, uma decidida.

- **Bash no pathguard (#62 dizia-heurística; agora é código).** No Windows,
  git-bash reescreve `/Users/nandi/site/` para um caminho real — o buraco não
  era teórico. `classifyBash` varre o comando por absolutos fora da worktree:
  estilos Windows (`C:\`, `\\?\`) e POSIX (`/Users/…`, `/c/…`), recusando com o
  caminho nomeado. Declarado como heurística: confinamento de shell a sério é
  sandbox ao nível do SO (WSL2 ou contentor) — decisão ainda aberta em POR
  DECIDIR, agora com um caso concreto a empurrá-la.
- **Run falhado soma custo e turnos.** `RunOutcome::Failed` carrega
  `{message, cost_usd, turns}`; o sidecar preenche-os do mesmo `done` que
  reporta o erro, e o cartão soma seja qual for o desfecho. Tecto de cartão,
  tectos globais e Analista deixam de ler números falsos.
- **Uma linha que diz a verdade.** Um `done` com `error` renderiza como
  falha — nunca mais "done · 17 turns · $0.77" seguido de um notice a
  desmenti-lo.
- **"resumes 36e9afb4" → "Start continues session …"**: o cartão dizia que
  tinha sido retomado quando significava que será.
- **Por fazer:** ao retomar, verificar que a worktree tem o trabalho que a
  memória alega (uma linha no transcript poupava os 17 turnos); pausa-e-pergunta
  no corte de orçamento em vez de falhar.



### 72. Trabalho novo não nasce no projeto aberto
O `c_19a1` foi um site editorial a nascer em `harness/c_19a1` — o pinned_project
assume que tudo sem `project_id` pertence ao projeto aberto, e um mês de
"faz-me um site" deixa três sites e duas experiências num só histórico. Mover
depois custa: worktrees, cartões e a memória por vir ficam presos ao
repositório errado.

Duas linhas: no prompt do Director (com delegação), "antes de criares cartões,
pergunta se o trabalho pertence ao projeto aberto; coisa nova a construir ganha
projeto próprio — propõe com create_project e pergunta onde deve viver"; e na
recusa por falta de projeto, a terceira via dita em vez de escondida (nomear,
mandar abrir, **ou propor create_project** — que já exige `parent_path` e já
passa pelas aprovações). Propor, nunca criar.



### 73. Primeira sessão completa — sete achados
`c_19a1` fechou o ciclo: 5 runs, 50 turnos, $1.82, 13 ficheiros, aprovado e em
Done — com três runs desperdiçados no caminho.

- **Feito: adoptar em vez de destruir (#1).** Um checkout per-card existente é
  **adoptado**, nunca recriado — o `create_worktree` fazia `remove --force` +
  `branch -D` e levava o trabalho wip-committed do run anterior junto. Destruir
  só quando não há nada em disco. Teste: agente escreve `site/feed.xml`, falha
  por orçamento ($0.766/17 turnos somados ao cartão), run seguinte encontra o
  ficheiro e `create` foi chamado uma única vez.
- **Feito (#7):** o prompt do worker abre com "Harness commits for you — what
  it expects from you at the end is one call to report_work".
- **Desenhado, por fazer:** pausa-por-orçamento como estado próprio (#2 — o
  corte mata o processo, logo "pausar" é commitar + marcar + continuar com
  tecto novo; o botão pede o tecto antes de arrancar); revisão do Director
  visível em Sessions com veredicto no cartão (#3); RightNow a derivar do mesmo
  estado + sequência por evento contra buracos (#4); custo/turnos intercalares
  durante o run (#5); relógio de sessão a bater 1s enquanto corre (#6).



### 74. Pausa por orçamento — feito
`Card.budget_paused` + `SetBudgetPause`. Quando um run morre com "budget" no
erro, o engine marca o cartão (evento no log → actividade) e o `StartRun`
recusa com instruções: subir o orçamento do agente, voltar a carregar Start.
O `launch_run` limpa a bandeira quando o tecto novo do perfil cobre o já gasto
— sem tecto nenhum não conta como subido. O wip-commit de #73 manteve o
trabalho; a retoma adopta o checkout (#73/1); a sessão continua (#42).
Fechado: o único pendente do lote que ainda perdia quota.



### 75. A postura do Director no prompt
O trabalho de revisão que se fazia fora da app — ler o diff, comparar com o que
o cartão pedia, apanhar o desenhado-vs-feito — passou para dentro do prompt,
gated por delegação. Sete linhas de postura: verificar em vez de acreditar;
distinguir desenhado de feito; dizer o que falta sem lhe perguntarem; liderar
com dano; admitir erros antes de seguir; escrever decisões no momento (e anunciar
que as registou); nunca aprovar em silêncio — dizer o que verificou e o que não
conseguiu. Mais curto: cinco linhas que dizem o que fazer valem mais do que
trinta que descrevem o que aconteceu.

A escrita em `decisions/` continua limitada: o Director hoje não tem ferramenta
de escrita de ficheiros, e o prompt diz-lho honestamente ("say so aloud instead
of letting the decision die"). Ferramenta nova fica para o lote do Curador.



### 76. `record_decision` — o Director escreve as decisões
A limitação honesta do #75 está fechada: ferramenta nova no chat do Director
que grava a decisão **no momento**, em
`<appdata>/projects/<id>/memory/decisions/<data>-<slug>-NN.md` - datada,
append-only, fora de qualquer repositório (#59). Auto-aprovada como
`report_work`: escrita nossa na memória nossa, não no repo do operador. O
prompt já a anuncia e manda dizer que gravou.

Porque não "dar todas as ferramentas" ao Director: o trabalho fora de cartões
anula worktree, review, história e custo (#70) - mas uma nota de memória é da
camada nossa, reversível, e sem conflitos entre worktrees.



### 77. Curador v1 - o mecanico completo
Comando `curator_run(project_id)`: promove os `report_work` de cartoes em Done
para `<appdata>/projects/<id>/memory/areas/` (um ficheiro por promocao, com
card e seq no frontmatter), regenera `index.md` **a partir dos ficheiros que
existem** - codigo, nunca modelo - e grava a marca de agua
(`curator-state.json`) para nao promover duas vezes. Idempotente.

O que falta e o julgamento: contradições, obsolescência, reorganização entre
áreas. Isso corre sobre estes ficheiros num passe com modelo depois; nada do
que hoje foi escrito muda de formato quando ele chegar.



### 78. `self_report` e `read_docs` — o Director vê o próprio histórico
O #75 deu-lhe a postura ("distinguir desenhado de feito", "dizer o que falta")
sem lhe dar material nenhum para a cumprir: o `DEBT.md` e o `DECISIONS.md`
vivem no repositório do harness e ele não tinha como os ler. E quando uma
ferramenta lhe é recusada, esse facto morria na conversa — ninguém agregava
"bateu na mesma recusa doze vezes esta semana", que é exactamente o sinal que
geraria uma proposta de melhoria.

- **`self_report(days?)`** devolve, por janela (7 dias por omissão), contagens:
  recusas de ferramenta por ferramenta **e razão**, aprovações que expiraram sem
  resposta, runs falhados separando corte de orçamento de falha real,
  `commit_error`, `unreported`, e cartões que voltaram de Review para Ready.
  Contagens e um exemplo curto por padrão — quarenta recusas iguais são uma
  linha, não quarenta transcrições. A agregação é código sobre os logs que já
  existem (`events.jsonl`, transcrições de run e de conversa); **o modelo não
  calcula**, recebe a tabela pronta — mesmo princípio do Analista (#55).
- **Expirações passaram a ser um facto.** O router gravava timeout e recusa
  operador da mesma maneira (ambos respondem "não"). Agora, no momento em que
  os 30 minutos acabam (`approvals.rs`), uma linha vai para
  `<appdata>/approvals-expired.jsonl` — uma pergunta que ninguém viu é diferente
  de um não deliberado, e só assim sobrevive a um restart. Teste com relógio
  tokio parado prova os dois caminhos: expiração grava, clique em Deny não.
- **`read_docs(doc: debt|decisions, find?)`** lê `<repo do harness>/docs/`. O
  repositório do harness é o projecto com `mirror: true` (#65) — sem ele, a
  recusa diz honestamente que não há onde procurar. O DECISIONS já passa de 90KB,
  logo: cabeça limitada (14k caracteres) com aviso, e secções puxadas por
  `find` — número ("75") com fronteiras exactas ("#7" não arrasta "#75") ou
  palavras. Código divide as secções; o modelo nunca adivinha offsets.
- **Auto-aprovadas**, mesma justificação de `record_decision` (#76): leitura dos
  nossos dados, escrita na nossa caixa de entrada — nada de quadros, nada do
  repositório do operador. Nem exigem delegação: um perfil sem delegação pode
  olhar, só não pode actuar.

### 79. A caixa de entrada e o fecho do dia — propôr, nunca criar
Último elo do Modo Espelho: nota o padrão → propõe → o operador decide →
cartão no `_harness` → agente corrige → compila → o operador instala.

- **`propose_improvement(title, observation, proposal)`** escreve na caixa de
  entrada (`inbox.json`) e anuncia. É tudo: não cria cartão, não move nada.
  Repetir um título ainda aberto **reforça** a proposta existente em vez de
  empilhar cópias — doze recusas fortalecem uma proposta, não criam doze.
- **Aceitar é do operador**, no rail RightNow: `inbox_accept` cria o cartão no
  projecto com `mirror: true` — **nunca no projecto aberto** (#72). Recusado
  com instrução quando o harness não está registado como projecto.
- **O fecho do dia corre no shutdown, uma vez por dia** (`look_due`: 20h), com
  tecto próprio ($0.30) e relógio de parede (120s). Nunca a cada turno —
  padrões operacionais veem-se ao longo de semanas, e um modelo pedido para
  reflectir constantemente reflicte sobre nada. O prompt do dia
  (`daily_look_prompt`) diz-lhe para chamar `self_report`, conferir o
  `DEBT.md` antes de propor, e parar se não houver padrão ("um dia mau é
  tempo, não padrão").
- **O relógio é nosso, não dele.** Primeira versão da linha no prompt fixo
  dizia "no fecho do dia, olha…". Errado: ele não sabe que horas são — um
  ritual que não pode agendar é ruído em todos os outros turnos. O prompt fixo
  ficou só com a capacidade e o travão (vê a própria semana; propõe em vez de
  agir); o "fecho do dia" vive só no prompt agendado, que corre quando *nós*
  decidimos que o dia acabou. Há teste que proíbe horas no prompt fixo.
- **A revisão deixa rasto**: cria uma conversa real ("End-of-day review") com a
  transcrição normal — amanhã abre-se e lê-se *porquê* existe cada proposta,
  que é a auditabilidade do Modo Espelho inteiro.
- **Delimitado contra alguém à porta**: quem fecha a janela espera no máximo o
  relógio; propostas já escritas estão salvas porque se escrevem no momento da
  chamada, não no fim. Sem wip a commitar e sem fecho devido, a janela fecha
  como sempre fechou.

### 80. Tailwind v3 — os tokens são literais e o inline sai das vistas
O `PRODUCT.md` dizia, e era verdade até aqui, que "tokens and keyframes live in
`src/styles/theme.css`, every other style is inline so a screen can be read
beside the design". **Isto contradiz essa escolha de propósito.** O que a
justificava era poder ler um ecrã ao lado do ficheiro de desenho; o que ela
custava eram 778 objectos de estilo inline contra 124 `className`, e um inline
não tem `:hover`, `:focus-visible` nem `:disabled` — que é a razão por que
metade dos controlos deste app eram `<span onClick>` sem teclado.

- **Os tokens são valores literais no `tailwind.config.js`.** As 96
  declarações de custom property do `theme.css` passam a valores escritos. O
  valor base é o do tema **claro** e a variante `dark:` é a do escuro, porque é
  assim que o Tailwind lê um tema; o selector é o atributo que o `store.tsx` já
  escreve (`darkMode: ["selector", '[data-theme="dark"]']`), portanto o
  `applyTheme` não mudou uma linha. Os ~38 tokens que diferem entre temas
  passam a precisar de `dark:` em cada sítio onde são usados. É trabalho, era
  esperado, está feito.
- **Uma excepção, e só uma: o acento.** O operador pode escolher um acento nas
  definições, e o `applyTheme` escreve seis propriedades no elemento raiz em
  runtime. Essas seis ficam como `var(--accent, <literal>)` no config: o
  literal é o fallback e é o caso normal, e nenhuma folha de estilo as declara —
  só existem quando o operador escolhe. Sem isto o selector de acento passava a
  não fazer nada, o que seria mudar comportamento em nome de uma migração.
- **`src/styles/theme.css` foi eliminado.** No seu lugar fica
  `src/styles/app.css`, que é a folha de entrada do Tailwind e mais nada: as
  directivas, e o que não cabe numa `className` — o corpo, as barras de rolagem,
  a selecção, o cursor de texto, e o bloco global de
  `prefers-reduced-motion`. Zero tokens, zero utilitários soltos.
- **As 94 classes do desenho desapareceram.** As de layout (`.row`, `.chip`,
  `.tile`, `.stagger`, `.cols`, `.hv-*`) viraram classes nas vistas ou
  constantes com nome dentro do ficheiro que as usa; as que eram componentes
  viraram variantes no `ui.tsx` — `Card`, `Pill`, `Avatar`, `Meter`,
  `DiffBlocks` e `Glyph` aceitam agora um tom em vez de receberem cores por
  cima.
- **Um tom deixou de ser uma cor.** `TONE` e `STATUS_TONE` continuam no
  `types.ts` como o `DEBT.md` diz, mas cada tom passa a ser um conjunto de
  classes (`fg`, `soft`, `solid`, `line`, `edge`, `wash`), porque o Tailwind
  precisa do nome escrito em código para o gerar.
- **O movimento fica em CSS por omissão.** As Web Interface Guidelines preferem
  CSS a JavaScript, e girar, pulsar, piscar, aparecer e crescer ficaram em
  `animate-*`. O `motion` entrou só para o que o CSS não faz: um cartão que
  muda de coluna (é removido de uma coluna e montado noutra, e o CSS anima a
  montagem, não a viagem), a **saída** de painéis, folhas, avisos e do rail — que
  até aqui apareciam com animação e desapareciam num salto — e as sequências
  orquestradas do `.stagger`, com os mesmos atrasos que estavam escritos à mão.
  A preferência de movimento reduzido é respeitada dos dois lados: o bloco
  global no `app.css` e `<MotionConfig reducedMotion="user">`, mais
  `useReducedMotion()` no quadro, onde a resposta certa não é "não mexas" mas
  "diz-o de outra maneira" — o cartão que mudou lava-se de acento em vez de
  viajar.
- **Os SVG à mão passaram a `lucide-react`.** Trinta dos trinta e um; o
  trigésimo primeiro é o grafo de commits do ecrã de projecto, que é geometria
  vinda da história real e não um ícone. A fachada `Icon.*` ficou, e cada
  entrada guarda o tamanho e o peso de traço que o desenho lhe deu, convertidos
  para a grelha de 24 do lucide. As marcas dos agentes não são ícones: são
  identidade e ficaram exactamente como estavam.
- **Uma guarda impede a volta.** `pnpm check:styles`
  (`scripts/no-static-inline-style.mjs`) percorre a AST de cada `.tsx` e falha
  se algum `style={{ }}` for feito só de literais. Corre no workflow de
  release, ao lado do `tsc --noEmit`. Um objecto de estilo sem variável nenhuma
  é uma classe que ninguém escreveu; sem esta verificação voltam a ser duzentas
  dentro de um mês.

**O que ficou inline, e porquê.** Dezanove objectos, todos calculados: a
largura de uma barra em percentagem, a altura de um glifo que o chamador
escolheu, o avanço de uma linha da transcrição pela profundidade da chamada, o
atraso de uma barra pelo seu índice. Nenhum deles é uma classe disfarçada.

**Uma coisa que não foi corrigida, de propósito.** O `.tile:hover` do desenho
levanta um cartão do quadro com três valores crus (`#1e1d19`, `#33302b`) e não
tem par para o tema claro, portanto um cartão claro escurece ao passar por
cima. Está preservado tal e qual: é opinião de desenho e pertence ao brief da
v2, não a uma migração que promete não mudar pixels.

### 81. O guardo do Bash distingue leitura de escrita
Um `ls -R` de um caminho fora da worktree era recusado com *"runs may only
write inside their worktree"*. O `ls` não escreve. O `classifyBash` procurava
caminhos absolutos no comando inteiro e recusava sem olhar ao que o comando
fazia — ao contrário do `inspect`, que isenta `Read`, `Glob` e `Grep` desde o
#66. A incoerência custava o que se via: o Director não conseguia sequer olhar
para os dados do próprio Relay.

Passa a haver `READ_COMMANDS` (`ls`, `cat`, `head`, `tail`, `find`, `grep`,
`wc`, `stat`), a par do `READ_TOOLS` que já existia. O comando é cortado nos
separadores do shell (`;`, `&&`, `||`, `|`, `&`) e **cada segmento é julgado
sozinho**: `ls /fora | tee /fora` isenta o primeiro e guarda o segundo.

Quatro coisas devolvem a isenção, e são elas que impedem isto de ser um buraco:
qualquer `>` no segmento (cobre `>`, `>>`, `2>`, `&>`, `>(…)`), uma substituição
de comando (`$(…)` ou crases), uma atribuição à cabeça (`OUT=/fora cat x`), e um
`find` que executa (`-exec`, `-delete`, `-fprint` e afins) — o único leitor da
lista com escrita embutida. Sem elas, `cat x > /fora` passava por leitura.

Continua a ser heurística declarada, não fronteira: o #2 (sandbox) continua
adiado e isto não finge fechá-lo. Onze testes no `pathguard.test.mjs`, metade
deles escrita disfarçada de leitura.

### 82. `EditCard`: corrigir um cartão, mas só antes de correr
O domínio tinha `CreateCard`, `DiscardCard`, `SetDependencies`, `AssignAgent`,
`MoveCard` e `OverrideCard` — e nada que mudasse o texto. Um cartão mal escrito
só se corrigia apagando e recriando, o que perde id, histórico, sessão e as
dependências que apontam para ele. E **o título é o prompt que o agente
recebe**, portanto um cartão mal escrito é uma instrução mal escrita.

`Command::EditCard { card_id, title }`, com `Event::CardEdited`. A linha que o
torna seguro: **permitido só enquanto `runs == 0`**. Depois do primeiro run o
log, a transcrição e o assunto do commit já respondem ao título antigo;
reescrevê-lo faz o registo deixar de bater certo com o cartão. `runs` sobe
quando um run *arranca*, não quando acaba, portanto um cartão a correr também
está fechado. `DecisionError::AlreadyRan` diz o número de runs e o que fazer em
vez disso.

Não se acrescentou nada ao prompt do Director por causa disto: a ferramenta
`edit_card` traz a regra na própria descrição, que é onde ela é lida no momento
em que interessa.

### 83. O corpo de uma proposta vai para o cartão
Aceitar uma proposta criava o cartão a partir de `proposal.title` e mais nada.
A observação e o raciocínio — o corpo todo — morriam na caixa de entrada no
momento exacto em que o operador dizia que sim. Como o título é o prompt, **uma
proposta aceite chegava ao builder sem nenhuma das razões que a motivaram.**

O título de um cartão passa a poder ter corpo: a primeira linha é o pedido de
uma linha, e o corpo vem por baixo. `Proposal::as_card_text()` monta-o.

Isto obrigou a uma segunda decisão, pequena e obrigatória: `harness_domain::one_line`,
e os dois sítios que precisam de exactamente uma linha passam a pedi-la — o
assunto do commit (senão o #57 deixava de valer: o `git log` deixava de se ler
como trabalho) e a linha por cartão no prompt do Director. Tudo o resto vê o
texto inteiro, incluindo o prompt do agente, que é o ponto. A UI já corta o
título em três linhas (`line-clamp-3`), portanto não mudou nada no ecrã.

### 84. Uma ocorrência única basta para abrir uma proposta
A descrição do `propose_improvement` pedia contagens do `self_report`, portanto
lia-se como sendo só para padrões repetidos. Numa sessão real o Director
encontrou **quatro recusas de ferramenta**, cada uma um buraco de capacidade
genuíno, e **não usou a ferramenta** — porque uma ocorrência única não parecia
caber lá. Disse-o quando lhe perguntaram.

O texto da ferramenta passa a dizer que uma ocorrência única chega, e que as
contagens do `self_report` **reforçam** uma proposta em vez de serem requisito
para a abrir. Texto da ferramenta, não do prompt: é lido no momento da decisão
de a usar, e não gasta uma das ~150 instruções que o modelo segue com fiabilidade.

### 85. Uma regra de paragem no prompt do Director
O system prompt do Claude Code já traz ~50 instruções e os modelos seguem de
forma fiável 150-200 antes de a adesão degradar: acrescentar regras faz seguir
**menos** delas. A postura do #75 está a funcionar, portanto acrescenta-se
**uma** regra, e uma só:

> Quando uma ferramenta te é recusada, isso é um achado sobre a aplicação, não
> uma condição do teu turno. Antes de continuares: arquiva com
> `propose_improvement` e diz se a recusa está correcta ou se é defeito.

Duas metades, ambas necessárias. O **"antes de continuares"** é o que a faz
acontecer — sem isso arquiva no fim, ou não arquiva. E o **"diz se a recusa
está correcta"** é a distinção que faltava: na mesma sessão o `AskUserQuestion`
estava correctamente recusado (falta superfície na UI, não é defeito) e o
guardo do Bash estava errado (#81). Arquivados juntos e indistintos, ambos se
lêem como ruído.

Fica ao lado da regra de recusa que já existia ("diz e pára; não contornes"),
porque é o mesmo acontecimento. **Duas regras foram explicitamente recusadas**:
não sobre-explorar (resolve-se com âmbito no pedido, não no prompt) e dar
estimativas de tempo (é geral, não específico do Director). Há um teste que
falha se alguma delas voltar.

### 86. O quadro dá por si quando o código muda por fora
Quando alguém trabalha no repositório do Relay **sem passar pelo Relay** — o
operador num editor, um agente de infra, uma migração — o quadro não fica a
saber. Cartões que descrevem trabalho já feito continuam em Ready, o `DEBT.md`
que o Director lê fica desactualizado, e ele encontra comportamento que
contradiz o que julga saber sem forma de perceber porquê. Aconteceu nesta
própria passagem: nem os commits desta tarefa nem os da migração do Tailwind
passaram por cartão nenhum.

**A defesa não precisa de perceber o que mudou — basta detectar que mudou.** O
que a torna barata já existia: todo o commit nascido de um cartão leva o
trailer `Harness-Card` (#57), portanto **um commit sem esse trailer é, por
definição, trabalho que não passou pelo quadro**. Três chamadas de leitura ao
git (`commits_without_a_card`): os commits do intervalo, os que têm o trailer, e
os ficheiros de cada um.

O SHA da última vez fica em `<appdata>/mirror-watch.json` — fora do repositório
do operador, como tudo o que é do Relay. O repositório é encontrado pelo caminho
do Modo Espelho que já existe (`mirror_project`), não por um segundo.

Corre no arranque e no fecho do dia. **Nunca segura nenhum dos dois**: o git é
chamado numa thread bloqueante com prazo de 5s e um repositório lento ou avariado
é desistido em silêncio. Uma janela que demora mais a fechar por causa de uma
verificação de estado é pior bug do que um aviso perdido — o #79 gastou um tecto
duro de 180s a aprender isso. A primeira execução regista o SHA e não avisa nada:
despejar a história inteira na primeira vez que o Relay abre os olhos é ruído.

**O Director recebe e sinaliza; não decide.** O aviso diz quantos commits, que
ficheiros e desde quando, e manda-o dizer que cartões e que documentos vale a
pena reler — e **parar aí**. Não fecha cartões, não move nada, não reescreve
documentos. É a mesma postura da caixa de entrada (#79) e a mesma razão: uma
lista de ficheiros não é fundamento para decidir por quem é dono do trabalho.

### 87. Metade do estado da shell sai de trás dos mutexes; a outra metade fica, com razão escrita
A arquitectura tem uma premissa, e é uma só: **um loop possui o estado, ninguém
partilha, não há locks.** É o que torna as transições de cartão livres de
corridas — o engine possui o `Board` assim, e é por isso que ninguém precisa de
pensar em ordem quando lhe manda um comando.

O `Workspace` fazia o contrário, ao lado. Mil e trezentas linhas, oito campos
atrás de locks (dezoito ocorrências de `Mutex` no ficheiro), e o estado da app
inteira — quem são os agentes, quais os projectos,
que conversas existem, que turnos estão no ar — atrás de locks, **fora** do
actor, com as regras opostas às do vizinho.

Não é uma queixa de estética. É a origem da classe de bug que o #73/3 já
apanhou duas vezes: com duas fontes de verdade, uma avança, a outra não, e
ninguém sabe qual está certa. Iam continuar a ser caçadas à mão enquanto a
estrutura fosse esta.

**Dois actores novos, pelo mesmo padrão do `EngineHandle`**: um `enum Msg` com
um `oneshot` por pedido, um `ask()` que faz a ida e volta, e o estado a viver
dentro da tarefa que corre o loop.

- `registry.rs` — os perfis de agente e os projectos. Leituras e escritas
  simples, sem ciclo de vida; foi aqui que o padrão se estabeleceu.
- `conversations.rs` — o índice das conversas e os tokens dos turnos. Ficam no
  mesmo dono de propósito: um turno começa e acaba numa conversa, e com dois
  donos haveria um instante em que a conversa já não existe e o token dela ainda
  sim.

Três decisões dentro do padrão, que valem mais do que o padrão:

1. **A persistência é do dono.** Quem muda a lista é quem escreve o ficheiro, no
   mesmo passo do loop. Deixa de haver janela entre a mutação e o disco.
2. **O I/O demorado fica fora.** Canonicalizar caminhos, `git init`, clonar,
   levantar um engine — nada disso corre dentro do actor. Um actor bloqueado
   segundos a fio deixa de ser um dono e passa a ser uma fila; é a mesma lição
   do #45 (o actor bloqueado pelo git) e do `WorktreeResolved` no engine.
3. **O relógio é do dono das conversas.** Antes cada chamador carimbava
   `now_millis()` e mandava o número — duas escritas quase simultâneas podiam
   chegar ao índice por ordem inversa ao carimbo que traziam. Agora a ordem da
   fila **é** a ordem do tempo.

A consequência que se vê é que `agents()`, `projects()`, `runtime()`,
`conversation()` e o que deles depende são `async`. **Nenhum comando IPC mudou
de nome, de argumentos ou de forma de retorno**: o frontend continua a enviar
intenções e a desenhar snapshots, como o `PRODUCT.md` diz.

#### O que **não** se mexeu, e porquê

Estes dois foram avaliados e ficam. O objectivo era a premissa, não a contagem
de mutexes; forçá-los custaria mais do que devolve.

- **`settings`.** É lido em dois sítios que não podem esperar. O
  `AgentPort::run` do `SwitchingAgent` é um método de trait sem `async` — a
  assinatura é dyn-compatível por decisão explícita (#3) — e o guardo do fecho
  da janela em `lib.rs` tem de decidir o `prevent_close` **antes** de a função
  retornar, ou a janela vai-se embora. Um actor obrigaria a manter uma cópia
  síncrona ao lado, que é exactamente a segunda fonte de verdade que isto veio
  remover. E o `Settings` não é estado de quadro: não tem transições nem vistas
  derivadas, ninguém calcula o estado de um cartão a partir dele. Uma leitura
  atrasada escolhe o outro adapter num run; não põe dois painéis a discordar.
  É também partilhado com o `ApprovalRouter`, que vive na crate `app` e não
  conhece o Tauri.
- **`runtimes`.** Não é estado — é uma mesa de punhos para outros donos. Um
  `ProjectRuntime` guarda um `EngineHandle` (que é ele próprio um actor), o git,
  o store e o run log, todos portos com sincronização própria. Não há ali
  nenhum facto sobre o quadro que possa divergir de outro. Convertê-lo daria o
  mesmo desenho que já tem — perguntar, construir fora, pôr — com um canal em
  vez de um lock, e tiraria um número à contagem sem tirar nada ao problema.

  O que **existe** ali é uma corrida de construção, não de verdade: duas
  chamadas ao mesmo projecto frio podem levantar dois engines sobre o mesmo
  ficheiro de eventos. Já existia antes desta passagem. Ficou estreitada — o
  mapa passou a ser o árbitro (`entry().or_insert()`), portanto só um deles é
  usado e o perdedor morre com o handle — mas fechá-la a sério é *single
  flight*: o dono registar que alguém já está a construir e o segundo esperar.
  Isso muda comportamento, e esta passagem era de estrutura. Está no `DEBT.md`.

`inbox` ficou igualmente de fora, pela mesma razão do `settings`: o
`daily_look_due()` é lido do guardo síncrono do fecho da janela.

**Emenda (mesmo dia).** O `outside_work` também tinha ficado de fora, mas por
âmbito e não por argumento — e a diferença importa, porque era o único dos
quatro cujos dois leitores (`chat::send` e `bootstrap`) são `async` e podem
esperar pela fila. Passou para o `registry.rs`: o espelho é um projecto, e este
é o dono dos projectos. Ficam três campos atrás de um lock, cada um com o seu
motivo escrito acima.

`workspace.rs`: 1300 → 1204 linhas **no momento desta decisão** — o ficheiro
cresce a seguir com trabalho que nada tem a ver com esta passagem (as concessões
do #93–#97), portanto não se leia o número como uma promessa a manter; os campos do `Workspace` atrás de um lock
passam de oito (`settings`, `agents`, `projects`, `runtimes`, `conversations`,
`chat_turns`, `inbox`, `outside_work`) para quatro (`settings`, `runtimes`,
`inbox`, `outside_work`). `grep -c "Mutex"` desce de 18 para 11 — onze e não dez
porque uma das linhas é o comentário que explica porque é que o `settings` fica.

### 88. O aviso de trabalho fora do quadro ganha ecrã
O #86 detecta, escreve o aviso e entrega-o ao Director. Ao operador não
entregava nada: o `mirror://outside-work` era emitido e o frontend não tinha
escutador nenhum, portanto a única forma de o ver era ler o `stderr` ou reparar
que o Director estava a falar de commits que ninguém tinha pedido. É o
*"nothing is silently lost"* do `PRODUCT.md` a falhar pelo lado mais simples:
o aviso existia e não tinha superfície.

`events.onOutsideWork` no `ipc.ts`, secção **"Outside the board"** no RightNow,
ao lado das propostas e com a mesma postura — o backend descobriu, disse, e a
decisão é do operador.

Quatro decisões dentro disto:

1. **Não expira, e não exige que o operador estivesse a olhar.** O aviso entra
   na lista e fica; nada o tira dali senão o botão *Dismiss*. Entra também na
   contagem de "Waiting on you", que é o mesmo número que o strip de 44px mostra
   quando o rail está fechado — um aviso que o rail conta e o strip não seria um
   aviso que ninguém vê. E é uma **lista**, não um campo: um segundo aviso não
   apaga o primeiro, ainda que o backend só guarde o último.
2. **Só a metade que é para o operador.** O evento leva **uma string** — o
   parágrafo do `mirror::describe()` —, e esse parágrafo tem dois leitores: os
   factos (quantos commits, que ficheiros, desde quando) e, a seguir, o que o
   Director deve fazer, na segunda pessoa ("say which open cards…, do not close
   a card"). Pôr a segunda metade à frente do operador lê-se como uma ordem dada
   a ele, e o #86 é explícito no contrário. O rail mostra os factos e põe o resto
   atrás de *"what the Director was told"* — visível a pedido, porque saber o que
   ele recebeu é a auditabilidade, não ruído.
3. **O corte é de prosa, e assume-se.** `outsideWorkParts` procura uma marca
   literal; se a redacção do backend mudar, o corte não acontece e vê-se o aviso
   inteiro — **nunca menos do que hoje**. Não se conta nada a partir do texto:
   não há chips de `3 commits · 12 ficheiros` porque isso seria reler prosa para
   fabricar números que o backend já tinha e não mandou. O que fecha isto é o
   evento trazer o `OutsideWork` a par da frase, e é backend.
4. **A hora é de chegada, e diz-se assim.** O evento não traz id nem carimbo, só
   texto. O rail escreve *"seen HH:MM"* — a hora a que **este ecrã** recebeu —
   e não "detected", que seria inventar um facto sobre o repositório a partir de
   uma coisa que só se sabe sobre a janela.

**O que ficou por fazer, e é backend.** O emit do arranque é lançado no
`setup()` do `lib.rs`, antes de o webview existir: se o git responder depressa e
a janela demorar, o aviso é emitido para ninguém. O backend guarda-o
(`ws.outside_work()`, que o `chat.rs` já lê para o prompt) mas **não há comando
que o leia nem campo no `bootstrap`** — logo, nem o arranque nem um recarregar
da janela se recuperam. Um campo `outside_work` no `Bootstrap` fecha os dois e é
uma linha; não se construiu porque o âmbito desta passagem era `src/`. Está no
`DEBT.md`.

Nos ecrãs `board` e `agents` o rail está escondido de propósito, e lá o único
sinal é o toast de chegada — que passa. Junta-se à dívida que as propostas já
tinham pelo mesmo motivo.

### 89. A derivação do RightNow: auditada, e está correcta
O #73/3 mandava auditar a derivação interna do RightNow **só se** a divergência
persistisse depois da defesa por sequência de eventos e do #87. Auditou-se, e o
veredicto é que a derivação está certa — pela razão mais aborrecida possível:

- **O RightNow não tem um único `useMemo`.** Tudo o que mostra — `reviewing`,
  `runningCards`, `openProposals`, `doneToday`, `liveSpend`, `waiting`,
  `allQuiet` — é calculado inline a cada render, a partir do `snapshot`, do
  `activity` e das listas que o backend empurra. Não há array de dependências
  para estar errado.
- **O valor do contexto é um objecto novo a cada render** (`store.tsx` monta-o
  sem `useMemo`), portanto qualquer mudança de estado do provider chega a todos
  os consumidores. Não há aqui o caso clássico de um memo a segurar um valor
  velho.
- O `useEffect` das worktrees já depende de `snapshot?.last_seq`, ou seja
  re-lê a cada evento do motor.
- A contagem "Done today" bate com a fonte: `stats.done_today` conta
  `CardApproved` do dia no backend, e a lista filtra `kind === "review"` com
  rótulo a começar por "Approved" — que é o que o `insights.rs` escreve nos dois
  casos (Director e operador).

**Dois defeitos encontrados, e nenhum deles é derivação — são cache e relógio.**

1. **O diff em cache nunca era invalidado.** O rail lia o diff de um cartão em
   revisão com `if (!diffs[c.id]) loadCardDiff(c.id)`. O store diz na própria
   documentação que chamar outra vez é barato e que *"a re-run shows the new
   patch"* — e aquela guarda era exactamente o que impedia isso de acontecer. Um
   cartão devolvido, corrido outra vez e de volta a Review mostrava os `+/−` do
   run **anterior**, com a mesma confiança com que mostra os certos: números
   errados apresentados como certos, que é pior do que não os mostrar. Passa a
   ser chaveado por `card.runs` — que sobe quando um run *arranca* (#82) —, com
   um ref do par `(id, runs)` já lido para não re-ler o que não mudou. O
   `diffs` fica fora das dependências como estava, e por isso é que o efeito
   corria em ciclo se lá entrasse.
2. **O tick de um segundo estava no componente errado.** O `RightNowStrip` — a
   tira de 44px do rail fechado — tinha um intervalo de 1s com o comentário
   *"elapsed timers must breathe: a frozen number reads as 'frozen app'"*, e não
   mostra tempo nenhum: mostra uma contagem e as iniciais de quem está a
   trabalhar. O `duration(Date.now() - session.started_ms)` vive no rail
   **aberto**, que não tinha tick nenhum — só se mexia quando um token do stream
   fazia o store mudar, ou seja parava parada durante cada chamada de ferramenta
   longa, que é precisamente quando o operador olha para ele. O tick mudou-se
   para onde o número está.

Nada mais foi tocado. Um relatório que diz "auditei, está bem, e eis porquê"
vale mais do que uma alteração inventada — e as duas que se fizeram descrevem-se
sem recorrer a "por segurança".

### 90. O aviso leva os factos ao lado da frase
O #88 deixou-o dito com todas as letras: o evento `mirror://outside-work`
levava **uma string** — o parágrafo do `mirror::describe()` — e o `OutsideWork`
de onde ela nasceu nunca atravessava. O ecrã ficava com prosa onde queria
dados, e com um problema por cima: metade daquele parágrafo fala ao Director na
segunda pessoa ("say which open cards…, do not close a card"), e pôr isso à
frente do operador lê-se como uma ordem dada a ele. O #86 diz o contrário — o
Director sinaliza, o operador decide.

A defesa era cortar a frase numa marca literal (`outsideWorkParts`). Funcionava
e degradava com segurança — se a redacção mudasse via-se o aviso inteiro, nunca
menos —, mas não era dados, e o preço estava à vista: sem número de commits,
sem lista de ficheiros e sem `since_ms`, não havia chips, só um parágrafo.

**O corte muda-se para onde a redacção vive.** `mirror::FOR_DIRECTOR` passa a
ser a constante com a metade que fala ao Director, e o `describe()` passa a ser
"factos + `FOR_DIRECTOR`" — a mesma frase, no mesmo sítio, para o mesmo leitor.
O evento passa a levar um `MirrorWarning { work, for_director }`, com `work` a
ser o `OutsideWork` derivado em `TS` (`pnpm codegen`, nunca
`cargo test -p <crate> --test export_types`, que o `DEBT.md` regista como
corrompendo os tipos com `bigint`).

Três decisões dentro disto:

1. **O `said` não atravessa.** O parágrafo inteiro continua a existir e a ir
   para o prompt do Director (`chat.rs`), mas não vai para a janela: o ecrã
   nunca o mostrou inteiro e não passa a mostrar. Mandá-lo seria mandar duas
   vezes o mesmo — `said` é `factos + FOR_DIRECTOR` — e um campo que ninguém
   desenha é um convite a desenhá-lo. O `Workspace` guarda os dois juntos num
   `Finding`, porque os dois leitores querem metades do mesmo achado e nenhum
   deve reconstruir a do outro.
2. **A frase não se recalcula.** O `describe()` corre uma vez, no momento do
   olhar, e a idade que cita ("the oldest 3 hours ago") é relativa a esse
   instante. Voltar a escrevê-la ao montar o prompt daria uma frase diferente
   para o mesmo achado — mais correcta quanto à idade, e diferente da que o
   Director já tinha recebido. Fica como estava.
3. **O ecrã mostra o mesmo, como dados.** Chips (`3 commits · 12 files ·
   oldest 4h ago`), a lista de ficheiros já cortada pelo backend
   (`FILES_NAMED`) e o `and N more` contado do `files_total`. A idade sai do
   `since_ms` — que é um facto do repositório, não da janela — e quando o git
   não datou nada diz-se, tal como a frase dizia. Nada de novo: nem mais um
   conselho, nem menos um facto. O *"what the Director was told"* continua a
   ser exactamente a metade que ele leva, agora porque vem separada e não
   porque se acertou no corte.

Um teste garante o que a etiqueta promete: `describe()` **acaba** em
`FOR_DIRECTOR`, e os factos não repetem a instrução. Era isto que o corte de
prosa não podia garantir.

### 91. O aviso do arranque deixa de depender de quem estava a ouvir
O emit do arranque nasce dentro do `setup()` do `lib.rs`, **antes de a webview
existir**, e o `look_for_outside_work` só demora o que o git demorar. Git
rápido e janela lenta: o aviso é emitido para ninguém. Recarregar a janela
perde-o da mesma maneira. O backend guardava-o (`ws.outside_work()`) e o
`chat.rs` lia-o para o prompt do Director — o operador é que não tinha por onde
o pedir. O *"nothing is silently lost"* do `PRODUCT.md` a falhar não por o
aviso não existir, mas por não haver quem o vá buscar.

**Um campo `outside_work` no `Bootstrap`**, que é a chamada única que a UI faz
ao abrir e existe precisamente para não haver cascata no primeiro pintar. O
mesmo `MirrorWarning` que o evento leva, lido do mesmo sítio
(`outside_work_warning()`). Fecha os dois casos com um caminho só, e não
inventa um comando novo para uma coisa que já tem uma chamada.

O `look_for_outside_work` passa a **guardar antes de anunciar**. Uma janela que
ouve o evento e pergunta no mesmo fôlego não pode ser informada de que não há
nada a relatar.

**A chave de identidade são os factos, não a frase.** O mesmo achado chega
agora por dois caminhos e o operador não pode ver o aviso duas vezes. A chave é
`commits · since_ms · files_total · files` — o que o backend descobriu **sobre
o repositório**, igual venha por onde vier. A frase não serve: a idade que ela
cita é relativa ao instante em que foi escrita, e a mesma descoberta descrita
duas vezes daria duas frases diferentes. Hoje isso não acontece — o `describe()`
corre uma vez e é essa string que os dois caminhos carregam —, mas uma chave que
só funciona enquanto ninguém voltar a escrever a frase é a mesma fragilidade que
o #90 acabou de tirar do ecrã.

Duas consequências que se assumem:

- **O livro dos avisos já vistos é um `ref`, não a lista.** A decisão tem de
  ser tomada na chamada: um `setState` corre no render seguinte, tarde demais
  para dizer se o toast é devido. E *Dismiss* tira a entrada do rail sem
  esquecer a chave, portanto a cópia que chega pelo outro caminho não a põe de
  volta.
- **Um toast por aviso e por janela**, venha do evento ou do bootstrap. É a
  mesma afirmação que o rail faz com o *"seen HH:MM"*: **esta** janela acabou de
  saber disto. Recarregar volta a tocar uma vez por um aviso que ainda está
  aberto — que é um lembrete do que está no rail, não uma notícia nova.

### 92. Auditoria: o frontend continua sem verdade — com uma excepção, e fechou-se
O `PRODUCT.md` diz, nas restrições técnicas: *"The frontend holds no truth: it
sends intents and renders backend snapshots rather than replaying domain rules
in TypeScript."* Depois de um dia em que o `src/` levou a migração para
Tailwind (#80), a divisão do `store.tsx` em `store`/`events`/`chat` (#87) e a
superfície nova do #88, a regra foi auditada de ponta a ponta.

**O veredicto é que se aguenta, e a razão é estrutural**: os dois mecanismos que
a fazem cumprir-se existem e são usados. O `vocabulary.ts` é escrito pelo
`vocabulary.rs` a partir de **serializar os próprios enums**, portanto um id no
frontend é por construção um id que o backend parseia (`STATUS_ORDER`,
`STATUS_NAME`, `REVIEWERS`, `WORKTREE_MODES`, `MODELS`, `ALL_PERMISSIONS`); e o
resto dos tipos vem do ts-rs (#51). Nenhum comando IPC mudou de forma. O
`RightNow` não tem um único `useMemo` (#89) e o valor do contexto é um objecto
novo a cada render, portanto não há derivação a segurar valores velhos.

**Uma violação real, e é a única desta classe: a tabela de transições.** O
`Board.tsx` tinha `LEGAL: Record<Status, Status[]>` escrita à mão — cópia exacta
do `Status::LEGAL_MOVES` do `crates/domain`, que **não era exportado**. E não
era decoração: o `drop()` recusava a jogada e **retornava em silêncio**, sem o
backend chegar a ouvir falar dela, e o `canDrop` decidia que colunas se acendem.
Duas cópias de uma máquina de estados não falham como uma gralha — falham como
uma coluna que recusa um cartão que o motor aceitaria, ou que oferece um que ele
vai rejeitar, sem erro em lado nenhum.

Corrigido a favor do backend pelo mecanismo que já existia: o `vocabulary.rs`
passa a escrever `LEGAL_MOVES`, derivado de `Status::can_move_to` e não de uma
lista transcrita, e o `Board.tsx` lê-o. Comportamento idêntico — a tabela era a
mesma —, com um dono só. Um teste fecha os dois lados: tudo o que se oferece é
uma jogada legal, e o número de jogadas oferecidas é o número de jogadas que
existem.

**O que se encontrou e se anota em vez de corrigir**, porque corrigir no
frontend seria acrescentar a réplica em vez de a tirar:

- **Começar um cartão.** O `App.tsx` oferece *"Start:"* a qualquer cartão em
  `ready`; o motor exige mais três coisas (`crates/domain`: `!budget_paused`,
  e todas as `depends_on` em `Done`). O `Card` já traz os dois campos e o
  frontend não os lê. Escrever a regra em TypeScript seria replicá-la; o que
  fecha isto é o backend dizer se um cartão arranca, e porquê.
- **"Done today" tem duas definições.** O número no cabeçalho é o
  `stats.done_today` do `insights.rs`; a lista por baixo é
  `activity.filter(a => a.kind === "review" && a.label.startsWith("Approved"))`
  com a meia-noite do relógio do browser. Ou seja: um facto de domínio
  reconhecido **pela prosa inglesa de uma etiqueta** — a mesma classe do #90 —
  e uma segunda noção de "hoje" ao lado da que o backend usa
  (`day_index(ts_ms, tz_offset_minutes)`). O #89 verificou que hoje batem
  certo, e batem; o que não se pode garantir é que continuem a bater. Fecha-se
  com um discriminador na `ActivityRow`, e isso mexe no vocabulário dos filtros
  da Activity.
- **`ruleIsRevoked` no `types.ts`** repete a lista de shells do
  `allow.rs::is_inert` (`bash`, `shell`, `sh`, `powershell`). É uma regra de
  segurança em duplicado. O `bootstrap` já traz `revoked_allowances` calculado
  pelo backend — falta o ecrã das definições poder perguntar o mesmo por regra.
- **`NavRail` compara o gasto contra `settings?.daily_budget_usd ?? 10`.** O
  `10` é um tecto inventado no frontend. Hoje é **inalcançável** — o `App.tsx`
  só desenha o rail depois de `ready`, e `ready` só sobe depois de o bootstrap
  ter posto o `settings` —, portanto não é divergência a correr: é uma defesa
  morta à espera do dia em que passe a ser alcançável. Anotado por isso, e não
  corrigido, que seria mexer sem nada mudar.
- **O `RunUpdate` escrito à mão no `types.ts` está atrás do `RunEvent` do
  Rust**: `error`, `ok`, `detail`, `tool_use_id` e `parent_tool_use_id` existem
  no backend e chegam ao `events.ts` por *cast*. Não é regra de domínio
  replicada — é o espelho manual a ficar para trás, que o próprio ficheiro
  assume manter "loose on purpose".

Nada disto é derivação a divergir do snapshot: os contadores que o rail e o
quadro fazem com `filter(...).length` recalculam-se do `snapshot` a cada render
e não podem ficar velhos. São, quando muito, aritmética que o backend também
sabe — e essa é uma escolha de latência, não de verdade.

### 93. Skills e MCP por agente: a via é um plugin do Relay
Acrescentar uma skill ou um MCP a um agente exigia recompilar. O `mcpServers` já
era opção directa do SDK e o sidecar já a usava — para MCP faltava só um campo no
perfil. Para skills não havia caminho: o `settingSources: []` do #26 não descobre
nada, e a opção `skills` do SDK é um **filtro sobre o que foi descoberto**, com a
própria documentação a avisar que é *"a context filter, not a sandbox"*.

Foram avaliadas três vias, e as três foram **medidas** contra o SDK instalado
(0.3.239) em vez de deduzidas:

1. **`settingSources: ['project']`** — descobre, e traz atrás o
   `.claude/settings.json`, os hooks e o `.mcp.json` **do repositório alvo**.
   Configuração injectada sem aprovação nenhuma. Recusada sem experimentar.
2. **`CLAUDE_CONFIG_DIR` a apontar para uma pasta nossa, com
   `settingSources: ['user']`** — **funciona**: uma sonda com o SDK real
   descobriu `gadget-maker` a partir de um directório sintético, e o `intruder`
   plantado no `.claude` do repositório ficou de fora. Foi rejeitada por duas
   razões, nenhuma delas "não funciona":
   - é uma **variável de ambiente**, portanto vale para todo o processo e para
     tudo o que ele lançar, quando o que se quer é um valor por run;
   - move o sítio onde a CLI procura as **credenciais**. O nosso próprio
     `sidecar.rs::claude_status` diz onde elas vivem
     (`$CLAUDE_CONFIG_DIR/.credentials.json`), e apontar a variável para uma
     pasta do Relay deixa lá de as haver. Nesta máquina não se vê — o macOS
     guarda o token no Keychain —, mas no Windows e no Linux seria cada run a
     arrancar deslogado. Não se constrói em cima de uma via que só não parte
     no sistema operativo em que se testou.
3. **`plugins: [{ type: 'local', path, skipMcpDiscovery: true }]`** — a
   escolhida. É opção do SDK, é passada **por run**, nomeia **um** directório,
   e o `skipMcpDiscovery` impede esse directório de declarar servidores MCP por
   sua conta. O `settingSources: []` fica **exactamente como estava**: a
   isolação não se afrouxa, acrescenta-se uma lista explícita por cima dela.

Medido, com o SDK a correr: sem plugin, um run vê 51 comandos; com o plugin, vê
52, e o que entra é `relay:figma-export`. Nada mais. O `.claude/skills` do
repositório não entra em nenhuma das configurações.

**A isolação é o directório, não o filtro.** Cada agente tem
`<appdata>/skills/<agente>/`, e lá dentro está exactamente o que lhe foi
concedido. O filtro `skills` do SDK não separa dois agentes — ele próprio diz
que os ficheiros continuam em disco e alcançáveis por `Read`/`Bash` —, portanto
o que separa é o caminho não conter o que não foi concedido. Passa-se
`skills: 'all'`, que aqui lê-se "tudo o que foi concedido a este", e é também o
que liga a ferramenta `Skill` sem a pôr no `allowed_tools`.

**O disco é uma projecção do `agents.json`.** O `materialise` reescreve o
directório inteiro a partir do perfil, no `save_agents` do `registry.rs` — o
único sítio que escreve o ficheiro. Uma skill revogada no ecrã sai do disco
antes de a resposta voltar, e não há uma segunda contabilidade para divergir.
Como o arranque também chama `save_agents`, um `agents.json` editado à mão
passa a valer no run seguinte: **é isto que faz "sem recompilar" ser verdade**.

**O campo não se chama `skills`, e não podia.** O `AgentProfile` já tinha
`skills: Vec<String>` desde o #40, e é prosa que entra no brief ("planning",
"scoping"). Uma chave JSON não pode ter dois significados: ler
`["planning","scoping"]` como nomes de pacotes seria o Relay a procurar skills
que o operador nunca pediu. Os concedidos vivem em `granted_skills`, e o antigo
fica o que sempre foi.

**Onde isto não chega, e é fronteira e não esquecimento.** As concessões
penduram-se no **porto**, não no `RunSpec`: uma conversa constrói o seu porto e
serve um perfil só. O engine tem um `Arc<dyn AgentPort>` partilhado por todos os
runs de cartões, portanto os runs de trabalho não as levam sem o engine as
passar. Acrescentar campos ao `RunSpec` obriga a tocar nos dois literais de
`crates/engine` (`runs.rs:368`, `director.rs:74`), e esse directório estava
fechado nesta passagem. Fica no `DEBT.md`, com os dois sítios nomeados.

### 94. O modelo declara, o código instala
O operador diz "instala esta skill no Designer". O Director procura, lê a
documentação, e produz **uma declaração** — nome, fonte, agente, e o que traz.
Nunca um comando, nunca um script. A instalação é do Relay, a partir dos campos.

É o padrão que este código já usa três vezes: o `report_work` diz e o engine
commita (#58); o Analista interpreta e o código calcula (#55); o Curador promove
mecanicamente e o modelo julga (#60).

**A razão é concreta e não cerimonial.** Ele vai ler páginas web para descobrir
como se instala aquilo. Uma página que diga "acrescenta também este servidor"
torna-se uma instrução no momento em que o que sai dele é executado. Sendo uma
declaração revista pelo operador, a injecção aparece como uma **segunda folha de
aprovação**; sendo um comando, não aparece como nada.

Três ferramentas novas, todas fora do `allowed_tools` e portanto todas pelo
painel: `install_skill`, `add_mcp_server`, `revoke_grant`.

Decisões dentro da decisão:

- **A folha mostra a declaração, não a intenção.** O `summary` do pedido de
  aprovação passou a ser cunhado pelo **sidecar** (`summarizeUse`), que conhece
  a ferramenta pelo nome, em vez de pela renderização genérica chave-a-chave do
  adapter — que continua a existir como recurso para tudo o resto. O que o
  operador lê é *"add the MCP server "figma" to designer — npx -y figma-mcp;
  grants get_file, export_frame"*, e não *"o Director quer instalar algo"*. Uma
  declaração sem ferramentas nomeadas diz-o (`no tools declared`) em vez de
  parecer inofensiva.
- **A lista de ferramentas é declarada, não descoberta.** O Relay não consegue
  saber o que um servidor concede sem se ligar a ele, e ligar-se é executar o
  código que a aprovação existe para travar. Portanto a lista vem da
  documentação que o modelo leu, o operador aprova **essa** lista, e a
  confrontação com a realidade (`mcpServerStatus()` devolve as ferramentas
  verdadeiras) só é possível depois de o run já ter sido autorizado. Está no
  `DEBT.md` como o passo que falta.
- **O modelo nunca pede uma chave.** O `add_mcp_server` aceita **nomes** de
  variáveis de ambiente e nunca valores, pela mesma razão que o `add_endpoint`
  já não aceitava: a conversa é escrita para disco. O ecrã dos agentes é que
  tem os campos, e o Relay diz ao modelo quais faltam para ele mandar o
  operador lá.
- **O Relay escreve o frontmatter.** O `SKILL.md` nasce do `name` e da
  `description` declarados; um corpo que traga frontmatter próprio vê-o
  removido. Sem isto, o texto podia renomear-se para uma skill que o operador
  não aprovou. Há teste.
- **Um nome mau é recusado, não reparado.** O `paths::sanitize` conserta; aqui
  não se conserta, porque um nome consertado é um nome que o operador não viu.
  `../etc` é recusado pelo nome.
- **`harness` é reservado.** Um servidor concedido com esse nome substituiria as
  ferramentas de quadro com que o Director responde. Recusado na app, filtrado
  no adapter, e sobrescrito no sidecar — três fechaduras na mesma porta, porque
  uma configuração que perde silenciosamente o `move_card` é pior do que uma que
  perde um conector.

### 95. Auto-elevação não é uma aprovação difícil, é uma recusa
Dos três pedidos, dois são aprovação e um não é. Uma skill é markdown que entra
no prompt de outro agente: aprova-se, e **mostra-se a fonte**. Um MCP é código
arbitrário com as permissões desse agente: aprova-se, e **listam-se as
ferramentas**. Uma ferramenta é elevação de privilégio, e um agente que se
concede `Bash` a si próprio deixou de ter limites — não há resposta que o
operador possa dar que torne isso seguro, porque dá-la uma vez remove o que
voltaria a perguntar.

`grants::self_elevation_guard(tool, caller, target)`: recusa `grant_agent_tools`
e `add_mcp_server` apontados a quem os chama. Duas notas:

- **O MCP entra na regra.** O lote nomeava só as ferramentas, mas um servidor MCP
  é código com as permissões daquele agente: conceder-se um servidor é
  conceder-se ferramentas com um passo extra. Deixar essa metade aberta fecharia
  a porta e deixaria a janela.
- **Não é a regra do Director, é a de toda a gente.** Um especialista com
  `can_delegate` está exactamente na mesma linha; o `caller` é o perfil da
  conversa, não um id fixo. As skills ficam de fora: prosa no próprio prompt é a
  classe do `record_decision`, que os agentes já escrevem para si.

A regra vive no `grants.rs` e não no handler, para ser **uma lista que se lê e se
testa** em vez de um guardo lembrado em cada sítio — uma ferramenta acrescentada
depois sem o guardo é precisamente a falha que esta forma evita.

**E nenhuma das três se pode tornar permanente.** O `NEVER_STANDING` do #38 já
tinha o `grant_agent_tools`; ganhou o `install_skill` e o `add_mcp_server`, com
uma razão mais afiada do que a original: um "sim permanente" a estes instala a
próxima página sem ninguém a ler, que é exactamente a injecção que a declaração
foi desenhada para tornar visível.

**Um defeito ao lado, encontrado ao duplicar a superfície.** O `never_standing`
só era consultado no `covers`, não no `derive`. Ou seja: o operador marcava
"stop asking me about this", a regra aparecia nas Settings, e continuava a ser
perguntado — uma promessa no ecrã que nada cumpria. Já era assim para o
`grant_agent_tools`; com mais duas ferramentas na lista passava a ser três vezes
mais visível. O `derive` passa a devolver `None`, portanto não se escreve regra
nenhuma e o `respond_approval` não reporta nenhuma. Há teste dos dois lados.

### 96. O chão de 17 skills que ninguém concedeu
Ao correr a prova ponta a ponta com modelo, os dois agentes listaram as skills
concedidas **e mais dezassete** (`run`, `code-review`, `simplify`, `dataviz`,
`init`, …). A primeira leitura foi que o `skills: 'all'` tinha alargado o que um
run vê. Mediu-se, com `getContextUsage()`, em vez de se assumir:

| configuração | skills | tokens |
|---|---|---|
| como o Relay estava (sem `plugins`, sem `skills`) | 17, todas `source: "built-in"` | 2631 |
| com o plugin e `skills: 'all'` | 18 — a que entra é `relay:figma-export`, `source: "plugin"` | 2650 |
| `skills: []`, sem plugin | nenhuma | 0 |

Ou seja: o `skills: 'all'` **não alargou nada**. As dezassete são da própria CLI,
estavam em todos os runs antes disto existir, e o `settingSources: []` nunca as
tirou — o que ele tira, e continua a tirar, são as do `~/.claude` do operador
(nenhuma das duas que esta máquina tem apareceu).

**Não se desligou.** `skills: []` desligaria as dezassete e poupava 2631 tokens
por run, mas é uma mudança de comportamento que ninguém pediu, num sítio onde
não se sabe se alguma delas está a ser útil a um worker. Fica medido no
`DEBT.md` com os números, para ser uma decisão do operador e não um efeito
secundário desta passagem.

A prova está em `src-tauri/examples/grants_e2e.rs` e corre-se de propósito, como
o `e2e_sidecar` do #56, porque custa dinheiro:

```
designer loads …/skills/designer   → relay:figma-export (+ as 17 da CLI)
builder  loads …/skills/builder    → relay:rustfmt-house-style (+ as 17)
analyst  (servidor weather)        → mcp__weather__get_forecast, mcp__weather__wipe_disk
scribe   (sem concessões)          → NONE
GRANTS PASS: each agent saw exactly its own skill and its own server,
             and the repository's neither.
```

O `.claude/skills/intruder` e o `.mcp.json` plantados na worktree não apareceram
em run nenhum, e as skills do próprio operador — lidas do `~/.claude/skills`
real, não escritas à mão no teste — também não.

### 97. O teste que esperava por uma janela que já tinha fechado (bug)

Um teste do engine falhou seis vezes ao longo de semanas — as quatro últimas
sempre o mesmo, `a_failed_run_leaves_work_and_the_next_run_finds_it`, sempre
aos 30,0s, sempre a passar em 0,03s isolado. A leitura foi mudando: primeiro
"é flaky sob carga", depois "o `wait_for` de 30s é curto de mais para uma
máquina cheia", e por fim (`f56eeb3`) "há duas worker threads presas em
trabalho bloqueante e falta uma terceira para haver progresso", com
`worker_threads = 2` como reprodutor determinista.

As três estavam erradas, e a terceira refutava-se com o código que já lá
estava. O `wait_for` só entra em `panic` **depois** de `check().await`
**voltar**. Se o actor estivesse bloqueado, a sondagem não voltava e o teste
pendurava para sempre — nunca estouraria num prazo. Estourar exactamente aos
30,0s prova o contrário do que se concluiu: o actor estava vivo e a responder o
tempo todo. Instrumentado, respondeu a 93 sondagens em 2s enquanto "não
progredia", e o log mostrava o segundo run já com `RunStarted` **e**
`RunFinished`.

O que havia era o teste a esperar por `active_runs()` não vazio — um estado
**transitório**. O `WritesThenFailsAgent` morre no primeiro turno: com poucas
worker threads o segundo run nascia e acabava antes de a primeira sondagem
chegar, e a partir daí a lista de activos nunca mais enchia. Mais threads não
curavam nada; só faziam a sondagem apanhar a janela a tempo. É por isso que o
limiar parecia exacto — não era um recurso a faltar, era uma corrida a ganhar
ou a perder.

A correcção é esperar por um facto que **só acumula**: o `RunStarted` daquele
run no log de eventos. Um `wait_for` é um ciclo de sondagem, e um ciclo de
sondagem só pode observar estados monótonos; qualquer condição que se desligue
sozinha é uma corrida à espera de acontecer. Os outros `wait_for` do ficheiro
foram revistos um a um e estão bem — os que esperam por `active_runs()` usam
agentes que só morrem cancelados, portanto o estado cola.

Subir o `WAIT_BUDGET` continuaria a ser a correcção errada, agora por uma razão
mais forte do que antes: o que se esperava nunca mais ia acontecer.

**Um sítio bloqueante a sério, encontrado pelo caminho.** A auditoria que isto
obrigou a fazer não confirmou nenhum `std::sync::Mutex` com o guard a
atravessar um `await` — os quatro `.lock().unwrap()` do engine
(`director.rs:102` e `:125`, `runs.rs:486`, `lib.rs:791`) são todos temporários
que morrem no fim da própria instrução. Mas o `build_done` estacionava o
artefacto com `std::fs::create_dir_all`, `std::fs::copy` e `std::fs::write`
crus **dentro do loop do actor**: com o binário do orquestrador a pesar dezenas
de MB, a worker thread ficava presa na cópia. É a doença do #46 no caminho do
build. Foi para `spawn_blocking`. O actor continua a esperar por ele de
propósito — o manifesto tem de existir antes de o cartão ser anunciado — mas o
runtime deixa de parar com ele.

**Fica em aberto.** As duas falhas antigas (`a_shared_worktree_is_adopted_after_a_restart_not_rebuilt`
e `the_loser_of_the_agent_limit_never_builds`, uma vez cada) não são desta
forma: ambos esperam por estados que colam. Não têm reprodutor nem mensagem
guardada, e esta passagem não os explica.

### 98. Aceitar é permissão, não é trabalho — e o veredicto volta pela mesma estrada
Duas metades do mesmo canal. Corrige o #79 e o #83, que descrevem
comportamento que já não existe.

**Aceitar deixa de criar cartão.** O `inbox_accept` procurava o projecto
espelho, chamava `create_card_inner` com o texto da proposta e guardava o
`card_id` na proposta. O "sim" do operador era portanto uma **ordem**: cartão
nascido no `_harness`, atribuído ao worker por omissão, sem ele ter dito nada
sobre nenhuma das duas coisas. E numa máquina sem modo espelho recusava com
"não há onde pôr este cartão" — uma falha em forma de cartão para uma coisa que
não é um cartão. Nas palavras do operador: *"propose_improvement é o Director a
dizer que precisa de alguma coisa, e eu aceito — a partir daí ele pode agir."*

`InboxState::accept(id)` marca `Accepted` e mais nada: não recebe projecto, não
recebe cartão, não toca em quadro nenhum e não pode falhar por falta de sítio.

**O `card_id`/`project_id` ficam, com outro significado.** Deixam de ser "onde
o cartão nasceu ao aceitar" e passam a ser "o que o Director fez com isto
**depois**", preenchidos quando ele cria o cartão e passa o `proposal_id` novo
do `create_card`. É esse par que responde à única pergunta que o canal precisa
de fazer — já agiu? — e é o que preserva o registo verdadeiro de todas as
propostas aceites no tempo do comportamento antigo. Apagá-los seria perder
história de operador para não ganhar nada. Levam `#[serde(default)]`: um
`inbox.json` escrito antes ou depois desta forma tem de carregar, porque uma
proposta no disco de alguém não é coisa que uma mudança de formato deite fora.
Há teste com um ficheiro antigo literal.

**A permissão chega-lhe como facto, não como evento empurrado.** Estrada do
`outside_work`: guardada de um lado, buscada no turno do outro
(`ChatContext.accepted_proposals`). Sai da lista quando ele age (o
`proposal_id`) ou quando o operador a retira — o `dismiss` passa a aceitar uma
proposta aceite que ainda ninguém executou, porque uma permissão que não se
pode devolver não é uma permissão. O `truncate` conta uma aceitação por
executar como viva, senão a poda revogava-a em silêncio.

**Nos dois ramos do prompt.** Aceitar acontece **entre** turnos, num ecrã que
ele não vê, e o ramo retomado — o que corre numa conversa viva — `return`a
antes de tudo o resto. É a armadilha exacta do #91 e do aviso de versão. Teste
prende os dois ramos.

**A segunda metade: o veredicto da revisão automática não chegava a ninguém.**
O #12 fez do `reviewer` um campo de perfil (`director`/`you`/`nobody`), logo a
revisão automática é política configurável. O #19 removeu do engine o
`director_chat`, a `Msg::DirectorChat` e o handle — *"o engine deixou de ter
noção de conversa"* — e estava certo. Mas entre as duas ficou um buraco que
**ninguém decidiu**: o canal por onde o veredicto voltaria tinha sido apagado
por um refactor cujo objectivo era outro. Um agente acabava, a revisão corria,
o Director dava um parecer — e o Director da conversa nunca soube que o seu
próprio revisor tinha julgado alguma coisa. Não estava sequer no `DEBT.md`. O
silêncio foi consequência, não escolha. Foi o próprio Director que o
diagnosticou e o arquivou com `propose_improvement`, que é exactamente para o
que a caixa serve.

A correcção **não devolve ao engine noção nenhuma de conversa**, e isso é
inegociável. O engine continua a fazer o que já fazia: persistir `CardApproved`
/ `CardRejected` com o actor e a razão em cima. Zero linhas mudadas em
`crates/engine/src/director.rs`. Quem vem buscar é a casca: o laço de eventos
que já existe em `spawn_runtime` reconhece o facto e guarda-o
(`harness_app::verdicts`), e o `chat.rs` levanta-o no turno seguinte, ao lado do
`outside_work`.

- **Quem julgou já era tipado, e é isso que torna o travão honesto.** Só
  `Actor::Director` é notícia. `reviewer: you` deixa uma revisão `Actor::Human`
  porque foi o operador que leu o diff; `reviewer: nobody` fecha o cartão
  também como `Actor::Human`. Nenhum dos dois é um juízo dele, portanto nenhum
  dos dois lhe é dito. O actor do caminho `nobody` passou a estar assertado no
  engine, que é onde a distinção nasce.
- **Entregue uma vez e passa a passado** (`take`, não `read`), disciplina do
  `outside_work`: o quadro que ele recebe já carrega o estado permanente de
  cada cartão, e isto é a *notícia*. Repetido todos os turnos deixava de ser
  informação e passava a papel de parede. Um cartão julgado duas vezes guarda
  só o veredicto posterior, e guardam-se dez.
- **No disco, em `verdicts.json`, ao lado do `inbox.json`.** O momento mais
  provável para uma revisão acabar é um momento em que ninguém está a ouvir —
  o operador fecha a janela quando acabou o dia, e a revisão corre sozinha. Só
  em memória, o veredicto desaparecia antes de lhe poder ser dito, que é o bug
  original outra vez em ponto pequeno. **Persiste-se a remoção e não só a
  chegada:** se o `take` vivesse só em memória, um restart entre o `take` e a
  escrita seguinte dizia-lhe a mesma coisa duas vezes, e a disciplina toda é
  dizer uma vez. Escrita pelo `paths::write_json` (tmp + rename, como todos os
  outros), leitura pelo `read_json_or_default`: ficheiro ausente é uma
  instalação antiga, ficheiro ilegível é um dia mau, e ambos são uma caixa
  vazia. O Relay abrir sem veredictos está bem; o Relay recusar-se a abrir por
  causa deste ficheiro não estava.
- **A proposta aceite já era durável, e por construção.** Perguntou-se em vez
  de se assumir: aceitar é um **estado** escrito no `inbox.json`, não uma
  entrega, e o `awaiting_action` recalcula-o do ficheiro a cada turno. É essa a
  diferença honesta entre os dois factos que viajam nesta estrada — a permissão
  fica até ele agir, o veredicto diz-se uma vez. Há teste para o
  round-trip de cada um.
- **Campo irmão, não segundo mecanismo.** A forma de um veredicto não é a de
  uma proposta, portanto são dois campos do `ChatContext` na mesma estrada — e
  não duas estradas.

**Fica em aberto, e por decidir:** o revisor continua a receber um diff sem
contexto nenhum (nem carta do projecto, nem `memory/decisions/`, nem a
transcrição do run que está a julgar) e um resumo que os lockfiles afogam; e a
revisão não tem tecto de orçamento nem relógio, ao contrário do fecho do dia
(#79). Nenhuma das duas foi feita aqui. A estrada acomoda ambas sem se mexer:
são o *dentro* do run de revisão, e este canal só trata do que sai dele.

### 99. O token deixa de ser uma unidade de render, e o quadro deixa de se medir para mexer um número
O ecrã gaguejava a escrever, e não era o markdown nem o `motion`: era a
contabilidade. Um `delta` é um token, e cada token era um `setState`. Com o
objecto do contexto a ser construído de raiz a cada render do provider, isso
punha **toda** a app a re-renderizar — barra lateral, título, o quadro — dezenas
de vezes por segundo, para desenhar meia palavra. Quem pagava mais caro era o
chat, onde cada quadro remandava o markdown de *todos* os balões da conversa
pelo Streamdown para poder crescer o último, e o quadro, onde cada cartão é um
`motion.div` com `layout` e portanto se volta a medir a cada render.

**Os tokens juntam-se e assentam uma vez por quadro** (`state/chat.ts`,
`state/events.ts`). Nenhum modelo escreve mais depressa do que 60 fps, portanto
não se perde nada da escrita ao vivo — perde-se só o trabalho repetido atrás
dela. Qualquer evento que não seja um token esvazia primeiro o que está
pendente: um recibo de ferramenta à frente do texto que o anunciou trocaria a
ordem da transcrição, que é a única coisa que ela promete. Trocar de conversa
deita fora o que não assentou, ou os tokens da conversa que saiu do ecrã caíam
na que entrou.

**O valor do contexto passa a ser memoizado** (`state/store.tsx`). Sem isso o
lote por quadro não chegaria: continuava a ser um objecto novo, e um objecto
novo re-renderiza todos os `useStore` da app na mesma.

**Uma vez da conversa é `memo`, com comparação à mão** (`views/Chat.tsx`). Os
`blocks` são reconstruídos a cada quadro — os invólucros são novos, as mensagens
lá dentro é que não —, portanto compara-se por referência e o array de recibos
elemento a elemento. O `motion.div` foi para dentro do componente memoizado: no
pai, o `memo` só pouparia os filhos.

**O relógio dos segundos vai para onde o número está** (`views/Board.tsx`). Um
`useTicker` no quadro inteiro fazia uma medição de `layout` de todos os cartões
por segundo, para mexer um tempo decorrido. É agora um `<Elapsed>` com o seu
próprio intervalo.

**E o cartão a correr passa a mostrar o fim do que está a pensar, não o
princípio.** O buffer guarda os *últimos* 2000 caracteres e a linha cortava os
primeiros 40: enquanto o buffer não enchia mostrava a abertura de um pensamento
já passado, e depois de encher saltava letra a letra ao sabor do que caía à
frente. É o `tail` em `lib/format.ts`, que corta pelo fim e nunca a meio de uma
palavra.

**A memória traz um risco novo, e vem com o guardo que o cobre.** O objecto
construído de raiz era trivialmente correcto: não havia dependências para ficar
erradas. Memoizá-lo troca custo por um esquecimento possível, e um esquecimento
silencioso — um campo novo no `Store` fora da lista fica preso no valor com que
entrou, e nem o `tsc` nem os testes dizem nada. `scripts/store-deps.mjs` lê o
`useMemo` pelo AST e falha se um campo não contribuir para as dependências; está
no `check`, no CLAUDE.md e no workflow de release. Uma optimização que só se
consegue manter por atenção humana não se mantém.

**Rejeitado:** partir o estado de chat para um provider próprio, que era a
correcção estrutural — um token não faria o provider geral renderizar de todo.
Resolvia o mesmo e mais limpo, mas mexe em quem consome os eventos e em quem os
encaminha, e não é trabalho para a véspera de uma versão. Fica em
`docs/DEBT.md`.

**Também aqui, e é uma escolha de gosto e não de custo:** o escalonar das listas
era 0,37s de atraso sobre 0,38s de duração — a última linha assentava 0,77s
depois de a primeira começar. Um escalonar serve para dizer em que ordem as
coisas chegaram, e isso lê-se muito antes disso. A ordem manteve-se, o compasso
encolheu para cerca de metade (`lib/motion.ts`).

### 100. O ecrã de Agentes ganha o sexto lugar na barra, e a tripulação abre perfis
O `views.ts` reservava `agents` como sexto lugar da nav e deixava-o de fora
"until its design lands". O desenho aterrou há muito: o ecrã edita um perfil
inteiro — modelo, endpoint, ferramentas, skills, servidores MCP, a quem
responde, o orçamento, o revisor — e era, para várias dessas coisas, o *único*
sítio onde existem. Chegava-se lá pela paleta, ou clicando num membro da
tripulação que por acaso tivesse o chat desligado. Um ecrã que é o único sítio
onde uma definição existe não é um ecrã que se encontra por acaso.

**E a linha da tripulação passa a abrir o perfil dessa pessoa.** Abria
`openChat()` sem argumento, que abre a conversa *que já estava no ecrã* — clicar
no Scout dava a última conversa do Director. A barra lateral é a lista de quem
existe e do que pode fazer; falar com alguém é o separador Chat, e o perfil tem
lá o botão.

### 101. As Definições ganham secções, e o nome do operador ganha um campo
Eram oito painéis sem nome, um a seguir ao outro: para mudar uma coisa era
preciso ler todas as linhas até dar com ela. Os cabeçalhos não acrescentam
definição nenhuma — dizem a que assunto pertence o painel seguinte, que é o que
faltava para o poder saltar.

**O `user_name` passou a ter onde ser escrito.** O campo existe no `Settings`
desde sempre, o Home saúda com ele e a barra lateral assina com ele, e não havia
em lado nenhum maneira de lhe tocar: toda a gente era "Operator" para sempre. Em
branco volta ao valor por omissão do backend, que é o que "apagar o nome"
quer dizer — e não um nome vazio.

### 102. A revisão automática deixa de ser um segundo Director e passa a ser um turno do primeiro
O #12 fez do `reviewer` um campo do perfil e o engine passou a saber rever
sozinho: quando um run acabava e o revisor era o Director, o engine levantava
ali mesmo um Director novo — sessão nova, `permission_mode: dontAsk`, `inbox:
None` — que lia o diff, devolvia uma linha de JSON e movia o cartão.

Funcionava, e era um fantasma. Existiam dois Directors, só um deles era aquele
com quem o operador estava a falar, e o outro trabalhava onde ninguém o via.
Da cadeira do operador isso lê-se como trabalho a desaparecer da coluna Review
sem que nada aconteça — e o Director da conversa não sabia responder por uma
decisão que nunca tinha tomado. O #98 tratou metade do problema: fez o veredicto
chegar-lhe no turno seguinte. Tratou o silêncio, não o fantasma.

**O engine deixa de rever.** Passa a *pedir*, por um porto novo (`ReviewHook`):
diz que uma revisão é precisa, entrega quatro factos, e recebe um sim ou um não.
Continua sem saber que existem conversas (#19) — quem sabe isso é a casca.

**A casca corre-a na conversa do Director**, pelo `chat::queue`, que é o mesmo
caminho do compositor: se ele já tem um turno no ar, o pedido entra no inbox
desse turno e ele lê-o na leitura seguinte; se não tem, começa um turno normal.
Nos dois casos é a **sessão dele**, portanto mantém o fio que o operador estava
a seguir, e nos dois casos a resposta sai no `engine://run` com o id da
conversa — que é dizer, no ecrã.

**Não há canal de veredicto nenhum.** Ele tem `read_diff`, `approve_card` e
`reject_card`, e o veredicto é qual deles chama: uma chamada de ferramenta é um
evento do quadro com o nome dele em cima, onde o JSON era um segundo relato da
mesma decisão — e dois relatos de uma decisão podem discordar.

**Consequência aceite, e é uma mudança de comportamento:** `approve_card` não é
uma ferramenta de leitura, portanto passa pela folha de permissões. A revisão
deixa de ser silenciosa e passa a pedir uma confirmação. É mais um clique do que
antes e muito menos do que ler o diff; quem não quiser o clique tem a regra
permanente, que é onde essa escolha deve ser feita — uma vez, e à vista.

**Nada pega, o cartão espera.** Sem Director que possa conversar, ou com a
conversa a recusar, o gancho devolve `false` e o cartão fica em Review para o
operador. Melhor um cartão parado do que um cartão movido por algo que ele não
viu acontecer, que é precisamente o defeito que isto fecha.

**E o #98 reforma-se.** A premissa dele era "o veredicto não tinha para onde
ir". Passou a ter: vai para a conversa, como a revisão em si, à frente do
operador. Mantê-lo dizia ao Director "a tua revisão automática correu enquanto
não estavas a olhar" sobre uma decisão que ele acabara de tomar e de narrar —
o mesmo defeito de dois relatos, agora entre o prompt e a transcrição. Saem o
`verdicts.rs`, o campo do `ChatContext` e o ficheiro `verdicts.json`.

**Também sai o porto `director` do engine**, e com ele `director_model`,
`director_provider` e `director_allowed_tools` do `EngineConfig`: existiam só
para levantar o fantasma. A revisão corre agora com o modelo e as ferramentas do
perfil que a conversa já usa, que é a única resposta que nunca pode divergir do
que o operador vê no ecrã de Agentes.

**Fica por fazer, e é o passo seguinte:** um revisor especializado. Enquanto o
trabalho não se amontoar, o Director chega; quando chegar a altura, o `ReviewHook`
é exactamente o sítio onde outro nome se encaixa sem o engine dar por isso.

### 103. Os agentes falam uns com os outros a meio do trabalho
A fila de mensagens existia desde que o compositor deixou de bloquear durante um
turno: o que se escreve entra no stream de entrada do SDK e o modelo lê-o na
leitura seguinte — *durante* o trabalho, não depois dele. Só que essa fila era
do chat e de mais ninguém. Um run de cartão levava `inbox: None`, com a razão
escrita ao lado: "um cartão não é conversa de ninguém, não há compositor apontado
a ele, portanto não há nada para pôr em fila".

A razão estava certa e a conclusão não. Não há compositor apontado a um cartão,
mas há o Director — e a coisa que ele mais precisa de poder fazer é avisar um
agente que está a construir a coisa errada, agora, em vez de esperar que acabe
para lha mandar refazer.

**Os dois sentidos são a mesma fila.** `message_agent` põe na fila daquele run,
por um método novo do engine; `message_director` faz o caminho inverso por um
gancho (`MessageHook`), pela mesma razão que a revisão o faz — acaba numa
conversa, e o engine não sabe que existem conversas (#19). Nenhum dos dois
espera resposta: um agente parado à espera de uma pessoa é um agente que não
está a trabalhar, e a fila existe precisamente para que nenhum dos lados espere.

**A fila mudou-se para os portos.** Estava em `harness_app`, que fica *acima* do
engine, portanto o engine não lhe chegava. Não tem I/O nem relógio — é
contabilidade pura sobre um porto que os portos já definem —, por isso desce
para junto do porto que implementa, com o nome antigo reexportado para quem a
importava.

**Recusas em vez de silêncio.** Sem run não há caixa onde pôr a mensagem, e um
texto vazio não é uma mensagem: as duas são recusadas com uma frase, e há teste
para ambas. Uma mensagem que desaparece calada é o defeito que esta fila foi
escrita para não ter.

**Quem fala viaja com o que foi dito.** Quatro builders a trabalhar dariam
quatro mensagens sem dono, e a primeira pergunta dele seria sempre a mesma.

### 104. Dois browsers, e a diferença entre eles é o que fica guardado
Um agente que não vê uma página não consegue verificar nada que aconteça numa.
O `chrome-devtools-mcp` resolve isso e não precisa de máquina nova nenhuma no
Relay: já há concessões de MCP por agente, com aprovação, com a lista de
ferramentas declarada e com o painel que a mostra. O que faltava era a escolha
estar feita em vez de ser um formulário.

**São dois, e a diferença é uma só: se o que se passa no browser sobrevive ao
run.** O `--isolated` dá ao Chrome uma pasta temporária que é apagada ao fechar
— nada fica, ninguém está autenticado, e dois agentes ao mesmo tempo não se
pisam porque cada um tem a sua. O outro aponta para uma pasta que o Relay
guarda: os cookies ficam, que é o objectivo, e é também todo o seu perigo.

**Contra a suposição de partida, que era minha e estava errada:** por omissão o
`chrome-devtools-mcp` **não** é stateless. Sem `--isolated` reutiliza
`~/.cache/chrome-devtools-mcp/chrome-profile` e não a limpa entre runs. Quem
assumisse que o comportamento por defeito é seguro estaria a conceder o
persistente sem saber — por isso o Relay nomeia os dois e nunca oferece "o
default".

**Não é o Chrome do operador.** É um perfil dentro dos dados do Relay, vazio até
alguém entrar em alguma coisa nele. O que ele alcança é o que lá foi posto de
propósito, e é isso que torna "entra só no site que o Director precisa" uma
resposta a sério. Ligar a um Chrome já a correr é possível (`--browser-url`) e
**não** é oferecido como preset: essa entrega todas as sessões que o operador
tem abertas, e uma escolha dessas não deve caber num clique.

**O Chrome tranca a pasta do perfil.** Dois agentes com o "signed in" a correr
ao mesmo tempo dá um deles sem browser. É para um agente, e o ecrã di-lo em vez
de deixar descobrir.

**Nenhum é concedido por omissão.** Um browser é alcance, e o alcance concede-se
por agente, à mão, no ecrã de Agentes. A frase que descreve cada um vem do
backend e é a mesma que o teste prende — uma paráfrase no ecrã podia vir a
discordar do que o comando realmente faz.

**Fica por fazer:** `--allowed-url-pattern` / `--blocked-url-pattern` existem e
limitariam o persistente aos sítios que interessam. Não é sandbox — a própria
documentação o diz — mas é a diferença entre um agente que chega ao painel de
uma conta e um que chega ao email dela.

### 106. A linha de comandos passa a ser uma skill, e o Director tem-na
Um agente que não sabe que a máquina já tem uma ferramenta pede que se construa
outra. A `Shell` sempre esteve na lista de permissões e o Director sempre a
teve; o que faltava era ele saber que a tem, saber onde o guardo pára, e saber
que a pode passar adiante.

**Prosa, não permissão.** Uma skill entra no prompt e não concede alcance
nenhum — quem concede é a lista de ferramentas do perfil. O ecrã di-lo em voz
alta, e diz mais: se o agente tiver a skill e não tiver `Shell`, a linha por
baixo avisa que aquilo descreve uma ferramenta que ele não alcança. O engano
caro aqui é o inverso — pensar que conceder a skill dá terminal a alguém.

**O corpo diz onde o guardo pára, e há teste que o prende.** Ler fora da
worktree passa; escrever é recusado, e com ele a redirecção, a substituição de
comandos e o `find -exec`. Um agente que leia aqui que pode escrever fora ia
bater numa recusa sem perceber porquê, por isso o teste compara o texto com a
regra que o `pathguard.mjs` aplica de facto. Se uma das metades mudar, a outra
parte a compilação.

**O Director recebe-a pelo `normalise`**, com a mesma disciplina do
`can_delegate` logo acima: vem ligada, entra uma vez, e quem não a quiser
remove-a no ecrã de Agentes. A alternativa — esperar que a operadora carregue
num botão para o Director saber que tem terminal — deixava-o a recusar trabalho
que já podia fazer.

**Passar adiante está escrito na própria skill**, e não numa instrução à parte:
o texto que ele tem diz-lhe que outro agente a pode receber com `install_skill`
e que o que esse agente também vai precisar é da permissão `Shell`. A
consciência da ferramenta e a maneira de a distribuir viajam juntas, porque
separá-las é como se perde uma das duas.

### 107. Um agente pode ficar cercado a um projecto, e quem manda no quadro é quem o revê
O perfil `Project PM` dizia "Owns the board of one project" e o `AgentProfile`
não tinha campo nenhum sobre projectos. Era uma frase no brief: nada guardava
que quadro era o dele, nada o impedia de mexer noutro, e nada mandava o trabalho
daquele quadro para ele. O #104 mostrou que a máquina para isto já estava toda
lá — só faltava o facto.

**Um campo, e é uma cerca e não uma preferência.** Com `project` posto, as
ferramentas de quadro caem nele quando ninguém nomeia outro *e recusam* uma
chamada que nomeie outro. Sem as duas metades não é uma cerca: a primeira
sozinha é comodidade, e não dá a propriedade que interessa com muitos projectos
— saber que aquele agente não anda por fora.

**No sítio onde tudo passa.** A resolução do projecto é uma linha só no
`director_tools::run`; pôr a cerca em cada handler queria dizer que a ferramenta
seguinte lhe escapava calada.

**O Director nunca é cercado, e isso é forçado em três sítios.** Recebe o
trabalho acabado de todos os quadros: cercá-lo deixá-lo-ia a ler um diff e a ser
recusado ao aprová-lo, e a avaria pareceria a revisão estar partida em vez de
uma definição. O `normalise` limpa-lhe o campo ao carregar, o `owner_of`
exclui-o, e o ecrã explica-o em vez de oferecer o selector.

**Ser dono são três coisas ao mesmo tempo:** cercado àquele projecto, poder mexer
num quadro, e poder ter conversa — porque a revisão corre *como um turno* numa.
Faltando uma, não é dono, e o trabalho volta para o Director como sempre voltou.

**Um dono mal configurado não trava o cartão.** Volta para o Director, e ele é
*informado de porquê* no próprio pedido: a resposta a "porque estou eu a rever o
quadro da Ana" fica à frente dele, e não num ecrã de definições que ele não vê.

**Empates resolvem-se pelo id.** Dois agentes cercados ao mesmo quadro é uma
configuração que a operadora fez; escolher ao acaso faria o mesmo quadro ser
revisto por gente diferente de execução para execução.

**A mensagem do agente segue a mesma regra** (#103): quem reporta um bloqueio
chega a quem pode agir sobre ele, e não sempre à mesma pessoa.

### 108. Uma execução leva consigo o que levantou, e um turno que não correu não passa por resposta
Três mensagens seguidas ficaram sem resposta, sem erro e sem nada no ecrã: a
roda parava e a conversa seguia em branco. As respostas existiam — o CLI
escreveu-as todas no transcript — e nenhuma chegou à aplicação.

**O neto ficava órfão.** Há um sidecar por execução, e ao `done` a Rust
matava-o. O `child.kill()` manda um SIGKILL só ao node; o CLI do Claude é neto
dele. Ficava vivo, era adoptado pelo init — medido: o `ppid` passa a 1 — e vivo
continuava a segurar a sessão.

**Daí para a frente havia duas ideias sobre quem tinha a sessão.** A guarda das
duas execuções na mesma conversa não travava nada, porque para a Rust a anterior
tinha mesmo acabado. A mensagem seguinte retomava uma sessão que já era de
outro; o CLI novo fazia o que faz nesse caso — punha o pedido na fila do órfão e
saía sem correr turno nenhum. As `queue-operation` do transcript são isso, e são
também o que disfarçou a avaria: a fila entregou mesmo a mensagem. **Uma fila
entrega palavras; não devolve respostas.** Essas saíam pelo stream do órfão, que
já ninguém lia. As três mensagens caíram na mesma janela; à quarta, quarenta
minutos depois, o órfão já tinha morrido e a conversa voltou ao normal sozinha.

**O SIGKILL era também o que impedia o sidecar de se desmontar.** O `finally`
dele nunca chegava a correr. E mesmo que corresse não ganhava: medido, a
desmontagem do SDK leva 1 a 3 segundos a derrubar o CLI, contra um SIGKILL
imediato. Não é uma corrida que se ganhe com mais pressa — ganha-se mudando o
que se mata.

**Grupo próprio à nascença, grupo inteiro à morte.** O `process_group(0)` tinha
de vir primeiro, e não é detalhe: sem ele o sidecar corre no grupo da própria
Relay, e matar o grupo matava a aplicação. Com ele, o grupo é só deste run.

**No fim de todos os caminhos, e não nos dois que matavam à mão.** A limpeza
está depois do `drive`, num sítio só: os `break` do meio — cancelamento,
resultado, falha, erro de arranque — saíam todos por lá e três deles não matavam
nada, à espera do `kill_on_drop`, que só apanha o node.

**Rejeitado: esperar pelo filho.** Se o `done` só saísse com o processo já
morto, a guarda passava a assentar em verdade. Não há por onde: medido, tanto o
`q.return()` como esgotar o iterador resolvem em 0 ms com o CLI ainda vivo. O
SDK não dá sinal nenhum de "o processo acabou", e inventar um à custa de
sondagens era pôr a correcção a depender de adivinhar.

**E o turno vazio reprova.** Aquele `result` vinha `subtype: "success"`, custo
0, zero turnos e texto vazio; o teste que lá estava — herdado do resume de uma
sessão que não existe — só olhava para o `subtype`, e este era mesmo `success`.
Silêncio sem erro é a única coisa sobre a qual a operadora não pode agir. É a
rede por baixo da correcção: apanha o estado mau seja qual for a causa, incluindo
as que ainda não conhecemos.

**O turno é que separa, não o texto.** Um turno que só chamou ferramentas também
acaba sem texto e correu; é o `num_turns` que distingue os dois. Reprovar por
texto vazio calava trabalho verdadeiro.

### 109. O trabalho de fundo passa a ver-se
O #108 escondeu-se durante uma tarde por uma razão simples: um turno que
responde pode deixar um comando a correr por baixo, e não havia nada no ecrã a
dizer isso. A única pista era uma linha de resultado a dizer "running in the
background" — que rola para fora do ecrã como qualquer outra.

**Nível, não arestas.** O SDK dá as duas coisas: `task_started` e
`task_notification` como bookends, e `background_tasks_changed` com o conjunto
vivo inteiro a cada mudança. Toma-se o conjunto e substitui-se. Emparelhar
arestas quer dizer que uma perdida deixa um indicador a girar para sempre, e um
indicador preso é pior do que nenhum: passa a mentir em vez de faltar.

**Efémero, como os `Commands`.** É por-processo e nada é emitido ao arrancar, por
isso uma linha guardada só serviria para ressuscitar no ecrã tarefas que já não
existem. Esvazia-se no `started` e no fim da execução — esta última passou a ser
verdade com o #108, que faz a execução levar consigo o que levantou.

**Fora do fio, como as permissões.** Não é uma mensagem, é estado: dentro do
scroller ficava preso no sítio onde por acaso apareceu, a dizer uma coisa que
entretanto mudou.

**Uma tarefa sem id não vai ao ecrã, e um campo em falta chega vazio e não
ausente.** O primeiro porque sem id não há chave estável e duas delas seriam a
mesma linha a piscar. O segundo porque do outro lado isto desserializa para
`String`: um `undefined` no meio faria a carga inteira cair fora, e o ecrã
ficava com o conjunto anterior a dizer que ainda corria — a mentira que a
semântica de nível existe para evitar.

### 110. Um processo agarrado a uma sessão só é legítimo enquanto houver execução viva para ela
A 0.3.16 tornou o silêncio legível: uma conversa que não responde passou a dizer
porquê. Não impediu o silêncio. Na noite seguinte a mesma conversa passou dez
horas a recusar tudo — três mensagens, uma delas com dez horas entre a anterior
e ela — porque um processo continuava agarrado à sessão. Vivo, a gastar CPU, e
havia dez horas que não lia a fila dele.

**Vivo não é o mesmo que a servir, e é isso que dá o critério.** Não é uma
heurística sobre saúde: é estrutural. Se a Relay não tem execução viva para uma
sessão, ninguém está a ler o que aquele processo escreve — não serve nada nem
ninguém, esteja como estiver. Às 23:13 a execução acabou em condições, com 28
turnos e trinta e cinco dólares gastos; o processo ficou. A partir desse
segundo era um resto, e podia ter sido dito naquele momento.

**Por isso a limpeza é onde esse facto se sabe.** Logo a seguir à guarda das
duas execuções ter deixado passar este turno: nesse ponto está provado que a
conversa não tem turno vivo. Noutro sítio qualquer isto matava trabalho a sério.

**Descoberta, não um ficheiro de lock.** Um lock é uma afirmação sobre o passado
e mente de três maneiras — pid reaproveitado, processo que morreu sem limpar,
ficheiro escrito por uma Relay que já não existe. Pergunta-se ao sistema agora,
e o processo identifica-se sozinho: o id da sessão vem no `--resume` dele.

**Duas metades no reconhecimento, e as duas fazem falta.** O `--resume` sozinho
apanhava o Claude Code que a operadora tem aberto no terminal dela; o binário
sozinho apanhava os nossos que estão a servir outras conversas. E o `--resume`
compara-se até ao fim do argumento: com um `contains`, `--resume=abc` casava com
`--resume=abcdef` e o que estava do outro lado do engano era um `SIGKILL` numa
sessão alheia. Isso não se viu a ler o código — viu-se no teste que levanta
processos a sério.

**Ao arranque o critério é outro, e mais estreito.** A tentação era varrer tudo:
a Relay que acaba de nascer não tem execução viva nenhuma, logo nada seria dela.
Mas *nada dela* não é *de ninguém* — uma segunda Relay aberta ao mesmo tempo tem
turnos a correr. Ao arranque leva-se só o sidecar sem pai (`ppid` 1), que é a
única coisa que se pode provar de ninguém sem adivinhar de quem é.

**E o grupo só se leva quando o grupo é nosso.** Nos restos anteriores à 0.3.16
o grupo é o da própria Relay: levá-lo matava a aplicação. Confirma-se que o
líder do grupo é um sidecar nosso; não sendo, mata-se processo a processo, que é
mais lento e nunca é largo demais.

**Windows fica de fora, e diz-se.** A enumeração lá é WMI e não `ps`, e um
filtro errado aqui não devolve um resultado errado — mata um processo da
operadora. Escrevê-lo sem o poder exercitar valia menos do que o risco. Lá fica
a detecção do turno vazio, que é de plataforma nenhuma.

**O sidecar também se defende sozinho.** O stdin é o único fio que o prende à
Relay, que nunca o fecha enquanto viva; um EOF ali só quer dizer uma coisa, e o
sistema entrega-o mesmo quando ela morre de SIGKILL. Aborta o que estiver a
correr e leva o grupo. É a metade que não depende de ninguém o vir limpar.

### 111. O run deixa de morrer com quem o mandou fazer: socket em vez de cano
Um sidecar em cima de um cano dura o que durar a Relay. Ela reinicia — um force
quit, um crash, o instalador a relançá-la — e o turno em curso vai com ela. A
0.3.16 e a #110 trataram do que fica para trás; nenhuma das duas devolve o
trabalho. Isso pede outra coisa: uma ligação que seja uma visita e não um
cordão.

**O protocolo não muda; muda por onde passa.** As mesmas linhas JSON, o mesmo
despachante, o mesmo `drive`. Só o transporte é que passa a poder ser um socket,
e é isso que mantém a mudança pequena onde ela é arriscada.

**O que se diz é numerado e guardado.** Quem se liga diz por onde ia e recebe o
resto — sem buracos e sem repetições. Só os eventos entram no histórico: a um
pedido por responder não lhe falta ser visto outra vez, falta ser respondido, e
esses repetem-se a quem chegar a partir dos pendentes. Uma aprovação feita
enquanto ninguém estava ligado não se perde nem responde sozinha; espera.

**Sem número, manda-se só o que vier a seguir.** É a omissão segura. Uma Relay
que reiniciou não sabe por onde ia, e pedir tudo outra vez punha a conversa no
ecrã com todas as falas repetidas — uma duplicação é mais difícil de desfazer do
que uma falta, e o atraso não se perde: está na sessão em disco. Guardar o
número por conversa é o passo seguinte, e é o que fecha essa falta.

**Um socket é um sítio, não uma identidade.** Por isso quem atende diz quem é, e
quem se liga confere antes de adoptar. Sem essa conferência uma Relay ligava-se
a um caminho reaproveitado e adoptava o run de *outro* agente — um cartão a
compilar em vez da conversa do Director — e passava a escrever-lhe os eventos na
conversa errada, a responder-lhe às aprovações e a mandar-lhe mensagens que não
eram para ele. As chaves são prefixadas (`chat-`, `card-`) para nunca se
cruzarem, e um sidecar que não se identifica é recusado como qualquer outro.

**Os cartões também.** Dez minutos de build não se perdem porque a janela
fechou. A reatação não é um privilégio da conversa do Director.

**E a #110 aprende a diferença.** Um sidecar sem pai deixou de ser lixo por
definição: pode ser um turno que continuou sozinho, à espera de que alguém volte
a ligar-se. É o `--serve` que os separa. A limpeza passa a ser a segunda escolha
— havendo socket, quem decide é o porto, que se liga e confere a chave; não
havendo, é um resto do tempo dos canos e trata-se como sempre se tratou.
Confundi-los deitava fora exactamente o trabalho que isto existe para salvar.

**Windows fica pelos canos, e é de propósito.** Não há socket de domínio nesta
pilha, e o que aconteceria era pior do que não haver reatação: o sidecar
levantava-se com `--serve` e esperava-se por uma porta que nunca abria — o turno
falhava em vez de correr.

### 112. Codex passa a ser um segundo agente, e o plano passa a ser o que se
mede em vez do dinheiro

Um perfil escolhe agora **em que binário corre**: `claude` ou `codex`. É um
campo novo no `AgentProfile` (`backend`), não um endpoint, e a distinção é a
coisa toda desta secção.

**Porque não é um `Provider`.** O `providers.rs` diz o que um endpoint é: um
sítio que fala o protocolo Messages da Anthropic, alcançável com três variáveis
de ambiente. É por isso que apontar um agente ao Ollama ou ao OpenRouter não é
uma integração. O Codex não fala nada disso — tem protocolo próprio, sandbox
própria e login próprio —, portanto entrava como endpoint ou entrava como
agente, e como endpoint teria de mentir nas três variáveis. Entra como agente:
o `SwitchingAgent` lê o `spec.backend` **antes** de ler a definição
sidecar/CLI, para a preferência do operador sobre como falar com a Claude não
decidir calado um run de Codex.

**Porquê o app-server e não o `codex exec`.** Foram medidos os três caminhos. O
`codex exec --json` imprime quatro linhas e sai: sem deltas, sem canal para
responder a uma aprovação, sem ferramentas nossas. O terceiro caminho — conduzir
o TUI por um pseudo-terminal e ler o ecrã — é o que outros orquestradores fazem,
e é por isso que documentam `approval_policy = "never"`: uma caixa desenhada num
ecrã não é um evento a que se responda. O `codex app-server` é JSON-RPC
bidireccional por stdio, e as aprovações chegam como **pedidos**, que é o que
faz a folha de permissões do Relay funcionar igual nos dois agentes.

O preço está escrito: o app-server é experimental a montante e o esquema muda
entre versões do Codex. Aceita-se — a alternativa era um agente que não se pode
interromper nem perguntar.

**Dois erros que só a medição apanhou.** O `turn/start` responde
**imediatamente**, com `status: "inProgress"` e a lista de items vazia: é um
aviso de recepção, não o fim do turno. A primeira versão deste adaptador tomou-o
pelo fim e dava runs bem-sucedidos antes de o modelo dizer uma palavra. O fim é
a notificação `turn/completed`. E o `-c mcp_servers={}` **não** limpa os
conectores do operador — o override funde-se com a configuração carregada em vez
de a substituir, e mediu-se: quatro servidores do `~/.codex/config.toml`
anunciaram-se num run que devia estar isolado. O que fecha a #26 para o Codex é
um `CODEX_HOME` nosso, em appdata, onde não há nada com que fundir; o `auth.json`
entra por **link** e não por cópia, porque o token renova-se e uma cópia era um
login a apodrecer. O que o agente recebeu vai por cima disso, em `-c` com
caminhos pontuados — o valor de um `-c` lê-se como TOML, e um objecto JSON não é
uma tabela inline.

**O dinheiro deixa de existir onde não existe.** Um turno de subscrição não tem
preço: não há `cost_usd`, e o `max_budget_usd` de um perfil de Codex resolve-se
a `None` em vez de a zero. O ecrã de Agentes tira o botão do orçamento em vez de
o pôr a cinzento — um cap desactivado dizia que havia um cap — e põe no lugar a
percentagem das duas janelas do plano, lida ao Codex e não somada aqui: o plano
gasta-se com tudo o que corre na máquina, e um total nosso ficaria por baixo da
verdade.

### 113. O `image_gen` do Codex fica ao alcance da Claude

O modelo de imagem da OpenAI chega-se por dois caminhos: a Images API, que quer
uma `OPENAI_API_KEY`, e a ferramenta interna do Codex, que não quer nada — o
próprio ficheiro da skill o diz — e gasta o plano em que a máquina já está
ligada. Como o Relay já fala com aquele binário para runs inteiros, a ferramenta
`generate_image` é o mesmo adaptador pedido para um turno só. Um agente de
Claude ganha-a como qualquer outra ferramenta do Relay; um agente de Codex já a
tinha de origem e nunca passa por aqui.

Não é uma leitura, e está dito assim: gasta quota e escreve um PNG. Fica fora do
`allowed_tools`, portanto passa pela folha antes de acontecer, e passa livre pelo
teste de delegação pela razão do `record_decision` (#76) — não toca em quadro
nenhum.

**O ficheiro não é arrumado por nós.** O Codex guarda-o na sua casa e devolve o
caminho; decidir que uma imagem é um asset do repositório era uma decisão do
agente, que a toma com um `cp` que já pode correr.

**E vê-se.** O caminho entra na resposta como markdown e o `Streamdown` do chat
desenha-o — os bytes atravessam o IPC como data URL, a mesma estrada de um
anexo colado, para não se abrir o protocolo de assets nem mexer na CSP. Quais os
caminhos que podem fazer essa viagem é `preview::readable`, com teste: o caminho
vem dentro de uma transcrição escrita por um modelo, e sem cerca uma `<img>`
numa resposta era uma maneira de ler qualquer ficheiro da máquina para dentro da
janela. SVG fica deliberadamente de fora — um SVG embutido é um documento que
pode trazer script.

### 114. O socket que nunca podia abrir (bug)

Todos os runs de macOS falhavam com "sidecar never served", e a reatação da
#111 nunca funcionou nesta plataforma — desde o dia em que saiu.

O `sun_path` de um `sockaddr_un` é um array de **104 bytes** no macOS e nos BSD
(108 no Linux). Passar disso não trunca nem avisa: o `listen` devolve `EINVAL` e
o socket não chega a existir. O caminho que a Relay construía —
`<appdata>/run-sockets/<run_key>.sock` — dá **124 bytes** numa conta de macOS
vulgar, e o `run_key` de uma conversa ainda repete o prefixo (`chat-chat_…`).
Mediu-se: o mesmo sidecar, no mesmo binário, serve num instante em `/tmp/rl.sock`
e falha com `EINVAL` no caminho a sério.

O nome passa a ser um resumo de 16 hex da chave em vez da chave, o que devolve
uma conta normal a 98 bytes e mantém o socket em appdata. Se mesmo assim não
couber — uma home comprida, uma raiz funda — o socket muda-se para `/tmp/relay-
<utilizador>` com `0700` em vez de o run falhar. Um socket é um ponto de
encontro e não um registo: mudá-lo de sítio não custa nada, e quem o guarda
continua a ser a conferência da chave ao ligar (#111), que não depende do sítio.

**O que isto escondia.** O sintoma não era "um socket não abre", era "perdi a
minha sessão" — e por causa da #115 abaixo, era verdade.

### 115. Uma falha de transporte deixava de haver história (bug)

O `record_resume_failure` limpava o `session_id` da conversa a qualquer falha
durante um resume, com o comentário "drop it rather than retrying forever". Só
que "falhou durante um resume" e "a sessão já não existe" não são a mesma coisa:
com a #114 em cima, um socket que não abria — um problema de processo, com a
sessão inteira no disco — desligava uma conversa da sua própria história de
forma permanente. Uma conversa real ficou assim: 2480 eventos na transcrição,
14,6 MB de sessão do lado da Claude, e um `session_id: null` a apontar para
nada.

A regra passa a ser assimétrica de propósito (`conversations::session_was_lost`):
só se esquece a sessão quando a falha **nomeia a sessão**, e o desconhecido
guarda-se. Deitar fora um ponteiro bom não se desfaz; guardar um ponteiro velho
custa um turno e uma frase a dizê-lo.

### 116. Um turno morto trancava a conversa (bug)

O `Turns::register` recusa um segundo turno na mesma conversa, e com razão. Mas
o `finish` que desregista está no fim do corpo da tarefa, e um `panic` ou um
`abort` não passam por lá — o turno ficava registado para sempre e tudo o que o
operador escrevesse a seguir ia para a fila de um morto. Viu-se: a mesma
resposta ("esta sessão ainda está agarrada pela execução anterior") três vezes
ao longo de dez minutos.

Um guarda com `Drop` cancela o token do turno em qualquer saída, escrita ou não.
Não desregista — o `Drop` não pode esperar por um actor — e não precisa: o
`register` já trata um token cancelado como um turno acabado.

### 117. Um turno era cobrado uma vez por bloco de conteúdo (bug)

Uma mensagem do assistente é um turno do modelo, mas o SDK entrega-a uma vez por
**bloco de conteúdo**, e cada entrega traz o `usage` inteiro da mensagem — não
uma fatia dele. O sidecar contava-as à chegada, portanto um turno com três
chamadas de ferramenta era contado três vezes.

Está no log da execução `f6995015`: 317 eventos de `usage` para os 72 turnos que
o próprio SDK reportou no `done`, e valores repetidos em sequência exactamente
pelo número de blocos de cada turno. O `conversations::totals` soma esses
eventos, portanto tudo o que deles saía vinha inflacionado — **1,45x** na
execução, **2,01x** na conversa do Director (1292 eventos → 606 turnos, 566M →
281M tokens). É o número que o operador usa para escolher um modelo.

O que faz de um turno um turno é o id da mensagem, e é ele que se conta
(`turnFrom`, e o mesmo guarda no `model-claude`, que lê o mesmo formato).

Na mesma passagem, a segunda metade: os turnos de um subagente chegam ao mesmo
fluxo, intercalados. São gasto real e contam para o total; **não** são o contexto
desta sessão. Sem os distinguir, o indicador lia o do filho e saltava
34967 → 8544 → 34967 ao atravessar uma chamada `Task` — e o salto é para baixo,
que é o lado que esconde uma sessão prestes a ficar sem espaço. O `RunEvent::Usage`
leva agora um `subagent`, posto de onde se sabe (`parent_tool_use_id`).

### 118. Um resto trancava o cartão para sempre (bug)

O `self.runs` só era limpo pelo `finish_run`, portanto uma tarefa que morresse
sem entregar o `RunDone` — um pânico, a mensagem perdida, o sidecar morto por
baixo — deixava lá a entrada. A partir daí todo o arranque era recusado com
"card already has an active run", os controlos do quadro não a tiravam, e a
única saída encontrada na prática foi achar o processo pelo `lsof` do directório
de trabalho e mandar-lhe um sinal. Não é operação de utilizador.

O `JoinHandle` é a prova: uma tarefa terminada não volta. O `reap_dead_runs`
corre antes de recusar um arranque e fecha essas execuções como `Failed` — falhar
em silêncio continua a ser falhar, e um cartão que não reporta nada é pior do que
um que reporta uma falha. Há um tempo de graça de 5s porque o último acto da
tarefa é enviar o `RunDone`: entre acabar e ser processado há uma janela em que
uma execução viva parece morta.

Não é o mesmo que o `100c89c`, que limpa **processos** do sidecar; esta entrada é
do lado do engine e sobrevivia àquela limpeza.

### 119. Aprovar passou a integrar (feature)

Cada worktree é cortada do `base_branch` e o único `merge` em todo o código Rust
era o `--ff-only <remote>` do `refresh_from_remote`. Não havia passo de
integração nenhum: aprovar queria dizer que o quadro o dizia, e mais nada. Cada
cartão começava numa árvore sem o trabalho do anterior — o `c_3626`, mandado
crescer `src/art/`, não encontrou `src/` nenhum e reconstruiu o projecto inteiro
ao lado. Trabalho excelente, inutilizável.

O `GitPort` ganhou `merge_card`, e a ordem em `execute` é a correcção: o `decide`
valida, a integração corre, e só depois a aprovação é persistida. Uma fusão que
falha deixa o cartão em revisão, onde o operador ainda lhe pega, em vez de o
marcar feito por cima de trabalho que não aterrou. Recusa-se — alto — a fundir
para uma checkout que esteja noutro ramo ou com alterações por commitar: nos dois
casos o trabalho ia parar onde ninguém o foi pôr.

### 120. A memória deixou de ser uma passagem e passou a ser uma leitura

O `report_work` sempre recolheu notas e o log sempre as guardou. Nada as lia de
volta: o `curator` promovia-as para `memory/areas/` e regenerava um índice, mas
**ninguém o chamava** — estava escrito, testado, registado como comando, e o
`areas/` não existia em máquina nenhuma. Entretanto o `c_f50e` reportou nove
notas e os cartões seguintes pagaram para redescobrir o mesmo chão: 3
leituras/greps antes de o primeiro poder escrever, 71 antes do quinto.

O `curator` foi apagado. As notas derivam-se do log no momento em que são
precisas (`memory::notes_from`), como o `runstats` e o `insights` já faziam, e
entram no prompt do run ao lado da charter. As razões são melhores do que ser
menos código:

- uma passagem tem de ser **disparada**, e memória que precisa de ser disparada
  apodrece à primeira vez que ninguém a dispara — que foi o que aconteceu;
- um ficheiro sobrevive ao cartão que o escreveu, portanto um cartão rejeitado
  **depois** de reportar continuaria a afirmar o que aprendeu; derivado, deixa de
  ensinar no instante em que sai de Done;
- uma segunda cópia em disco é uma segunda coisa que pode discordar do log, e é
  também um sítio por onde dois cartões concorrentes se pisam — exactamente o que
  levou a charter para fora do repositório.

Esquecer é orçamento, não julgamento: mais recentes primeiro, com tecto, e o que
ficou de fora é dito em voz alta. Decidir que uma nota substitui outra precisa de
juízo, juízo precisa de um modelo, e um modelo neste caminho é a passagem que
isto veio substituir.

### 121. Uma aprovação que expira deixou de ser uma recusa (bug)

O `Approver` devolvia um `bool`, e um `timeout` de 30 minutos colapsava em
`false`, que o sidecar traduzia para as palavras **"denied by operator"**. O
operador não tinha recusado nada — estava a dormir. O agente seguia a contornar
uma recusa que nunca houve, com confiança, sobre uma premissa que lhe foi
entregue como facto. Para trabalho sem ninguém a ver, isso é pior do que parar.

Passou a `ApprovalOutcome` de três variantes, e a correcção tem de viver no
**tipo**: qualquer par de variantes volta a colapsar no momento em que alguém
escrever `if allowed`. Uma pergunta sem resposta chega ao agente a dizer-lhe que
pare e diga o que ia fazer — explicitamente **não** que arranje outra maneira. O
fecho da janela também passou a ser "ninguém respondeu": a Relay a fechar não é o
operador a recusar.

O `RunEvent::ApprovalAnswered` leva um `unanswered` ao lado do `allow`, porque a
transcrição é onde o operador mais tarde pergunta "fui eu que recusei aquilo?" —
e por duas vezes, no histórico desta app, a resposta era não.

### 122. Um check vermelho deixou de poder ser aprovado (feature)

Os checks já corriam: o `card_checks_after_run` dispara no fim de uma execução e
escreve o resultado contra o cartão. Ninguém o lia. O `CardChecks::failing()` era
calculado, guardado, e consultado por **nenhum** consumidor — portanto um cartão
podia ser aprovado com o build dele vermelho e nada em lado nenhum o dizia.

É a forma de todos os defeitos do relato de 2026-08-31: seis foram entregues com
a suite verde. A lição foi que um check que ninguém é obrigado a olhar não é um
check. Isto é a obrigação, nos dois caminhos que aprovam — o botão do operador e
a ferramenta do Director, pela mesma função. Não julga *quais* checks importam:
o operador configurou-os, e um deles em vermelho é a resposta dele. Um cartão sem
passagem registada passa — "não há checks" é um facto sobre o projecto, não uma
falha do cartão.

### 123. O Director pode planear em vez de ser escalonado (feature)

O `depends_on` está no domínio desde sempre, o `SetDependencies` também, e o
comando do ecrã igualmente. O Director é que não tinha por onde lhes chamar:
das trinta ferramentas dele, nenhuma punha um cartão à espera de outro. Planear
cinco cartões deixava o operador a arrancá-los à mão pela ordem certa — que é o
"ter de explicar outra vez a seguir a cada passo" de que ele se queixou.

`set_dependencies`, com a única recusa que o quadro não teria como dizer depois:
um cartão à espera de si próprio nunca arrancaria.

### 124. O `subagents: false` não travava nada, e a prosa de um subagente entrava na do Director (bugs)

Dois defeitos, um a esconder o outro. O operador viu-os pelo ecrã: `Agent` a
aparecer numa conversa do Director, e as frases dele cortadas a meio da palavra.

**O primeiro é um nome.** Os três guardas do `canUseTool` comparavam com
`"Task"`, e o SDK renomeou a ferramenta para `"Agent"` algures pelo caminho. Nos
logs desta máquina há 7 chamadas `Agent` e **zero** `Task`, portanto nenhum dos
três disparava:

- a recusa — a conversa do Director põe `subagents: false` (`chat.rs`) e abriu
  **nove** subagentes numa só conversa;
- o contador de profundidade — que nunca subiu, portanto **quatro** desses foram
  abertos *por* subagentes, e o tecto de um nível nunca existiu;
- o `PostToolUse` que o devolve — que nunca correu, o que só não se notou porque
  o contador também nunca subia.

Passou a haver uma pergunta (`isSubagentTool`) e não uma comparação, com as duas
grafias: o nome já mudou uma vez. O teste recusa qualquer um dos guardas voltar a
comparar com uma literal — incluindo com `"Agent"`, que tem o mesmo defeito da
próxima vez que o nome mudar.

**O segundo é uma atribuição em falta.** Só o `ToolUse` levava
`parent_tool_use_id`. O `Text`, o `Delta` e o `Thinking` não, portanto a prosa de
um subagente chegava ao fio indistinguível da do pai — 162 chamadas de
ferramenta atribuídas a filhos numa conversa, com o texto deles a aterrar no
meio do texto do Director.

E não era só o ecrã. O `chat.rs` juntava as fatias de raciocínio de **qualquer**
origem, e fechava o troço à chegada de **qualquer** outro evento: cada coisa que
um filho fazia enquanto o Director escrevia selava o pensamento dele e abria
outro. É isso que se lê no ecrã como `"high-R"` seguido de `"PM, narrow
audience"` — uma palavra partida em dois balões.

Os três eventos levam agora o autor, o `RunEvent::from_subagent()` responde à
pergunta num sítio só, e o chat ignora o que não é dele: nem acumula o raciocínio
de um filho, nem deixa um filho fechar o troço do pai. Do lado do ecrã, um
`delta` ou um `text` com pai não entra na resposta que o operador está a ver
escrever.

Não resolvido de propósito: **o que fazer com a prosa de um subagente**. Fica
registada e atribuída — deixou de se fazer passar por outra coisa — mas ainda não
tem forma própria no fio. Aninhá-la sob a chamada que a abriu é desenho, e este
passo era parar de mentir sobre quem falou.

### 125. Um leitor atrasado matava o fio para a janela (bug)

O quadro ficava parado depois de o Director criar um cartão, e não voltava
sozinho: só um refresh à mão ou um reinício o traziam de volta.

Os dois reencaminhadores do `spawn_runtime` liam assim:

```rust
while let Ok(envelope) = events_rx.recv().await { … }
while let Ok(update)   = runs_rx.recv().await   { … }
```

São `broadcast::Receiver`. O `recv` devolve `Err(Lagged(n))` quando **quem lê**
ficou para trás, e a chamada seguinte volta a funcionar — é um aviso, não um
fim. Lido como `while let Ok(..)`, terminava a tarefa, e a partir daí a janela
não recebia mais nada daquele projecto.

Criar um cartão é o que chegava ao buffer de 1024: o `create_card_inner` manda
`CreateCard`, `AssignAgent` e `MoveCard` seguidos, e com `start` vem atrás a
tempestade de uma execução. O fio das execuções é ainda mais fácil de encher —
leva os `delta`, um token de cada vez.

A parte amarga é que a defesa já existia e nunca foi alcançável: o `store.tsx`
vigia a sequência e força um refresh imediato assim que vê um buraco
(`env.seq > last + 1`). Nunca disparava, porque o evento que revelaria o buraco
era precisamente o que a tarefa morta já não entregava.

Passou a distinguir-se: `Lagged` continua e diz quantos se perderam, `Closed`
acaba. No fio do quadro a recuperação é completa — o evento seguinte carrega o
buraco e o ecrã actualiza-se. No das execuções perdem-se as linhas saltadas, que
não têm sequência por onde voltar; o que deixou de se perder é tudo o que vinha
depois.

Dois testes: um prende a semântica do canal — que é o que a correcção assume — e
o outro recusa qualquer dos dois fios voltar à forma antiga.

### 126. Um agente sem worktree podia escrever na checkout viva (bug + feature)

O Director encontrou isto pelo lado difícil, a tentar resolvê-lo: um cartão do
`scout` precisava de escrever um ficheiro, e ele recusou-se a conceder-lhe
`Write` com a razão certa — o `scout` tem `worktree: none`, portanto corre contra
o repositório vivo. Dar-lhe escrita seria editar a árvore do operador sem ramo,
sem diff, sem nada para aprovar ou rejeitar. Depois foi mudar o worktree e não
tinha por onde: o `edit_agent` expõe nome, título, brief, orçamento, revisor e
pausa. **Worktree não.**

Duas coisas, e a segunda é pior do que ele disse.

**O `edit_agent` passou a levar `worktree`.** O ecrã dos Agentes sempre o deixou
mudar (`Agents.tsx`); só o Director é que não podia — via o problema, via a
correcção, e a ferramenta não lha dava. Permissões continuam de fora de
propósito: essas vão pelo `grant_agent_tools`, que o operador responde uma a uma
e que nenhuma permissão permanente pode cobrir.

**E "lê a checkout principal, nunca escreve" não era verdade.** A frase está no
`vocabulary.rs`, no enum e no ecrã. Um run sem worktree recebe
`cwd = repo_root`, e o `inspect` do sidecar só recusa escritas *fora* do `cwd` —
portanto a frase descrevia uma intenção que nada guardava. Um cartão começado
naquela combinação teria escrito na árvore do operador e ninguém saberia até ao
`git status`.

O `writes_into_the_live_checkout()` diz a combinação e o arranque de um cartão
recusa-a, com as duas definições que discordam nomeadas. Recusar em vez de tirar
as ferramentas em silêncio: um agente que dá pela falta do `Write` a meio de um
cartão falha de uma maneira sobre a qual ninguém pode agir.

Uma conversa não está coberta, e é de propósito — é a cadeira do operador, com
ele a ver. É o cartão, que corre sozinho e é revisto por um diff que não
existiria, que não pode.

Nenhum perfil que a Relay instala nasce nesta combinação, e o único que a tem
(`director`, que escreve e não tem worktree) não pega em cartões. Um teste
percorre os templates para que um perfil novo parta aqui e não na cara do
operador.

### 127. A mesma resposta desenhada duas vezes (bug)

O operador viu a mesma resposta do Director duas vezes seguidas, igual palavra
por palavra e as duas às 23:07 — e descreveu o resto do defeito melhor do que
qualquer log: *"once i send a message the thing i asked above will show
answered, but now i need to answer to see it"*.

A transcrição no disco tem aquela linha **uma vez**. Isso é o que separa as duas
metades: o registo está certo e o defeito é do desenho.

Um `text` chega por duas vias — o evento ao vivo e a linha lida do disco — e as
duas são entregas do mesmo registo. O `openConversation` faz
`setChat(toChat(lines))`, que substitui, portanto ler não duplica por si. O que
duplica é a ordem:

```
transcrição lida        → balão B
evento ao vivo chega a seguir → balão A     ← o dobrado
```

E é exactamente a ordem que mandar uma mensagem provoca: a leitura da
transcrição é o que faz a resposta aparecer, e a entrega atrasada — ou repetida
por um reatamento, que pede o histórico a partir de uma marca de progresso que
pode estar atrás — acrescenta-a outra vez.

A causa por baixo é que **uma linha de conversa não tinha identidade**. Duas
fontes para um registo, juntas por acrescento cego. A identidade é o par que o
log já escreve — quando aconteceu e o que disse — e nada numa transcrição colide
com isso: duas respostas diferentes não partilham um milissegundo, e a mesma
frase dita outra vez segundos depois traz outro carimbo e continua a ser uma
linha nova. O `alreadySaid` vive no `bubbles.ts`, ao pé dos outros ajudantes
puros, e por isso tem testes.

Não corrigido aqui, e é a metade que resta: a **demora**. A resposta só aparecer
quando se manda a mensagem seguinte é o fio ao vivo a não entregar, e o
suspeito é o #125 — o reencaminhador que morria ao primeiro `Lagged` e a partir
daí não entregava mais nada. Essa correcção existe e ainda não chegou a nenhuma
instalação: a única versão que os instalados vêem é a 0.3.25.

### 128. Uma dívida que era uma linha num documento passou a ser um teste

A pergunta do operador foi: porque é que mexer numa coisa parte outra, e se isto
não é um problema de manutenção. A resposta honesta é que nada se partiu — os
defeitos desta noite eram todos anteriores, e um deles (`while let Ok` no
`broadcast`) está no repositório desde o **primeiro** commit do engine. O que
mudou foi o operador ter usado a app a sério durante um dia inteiro.

Mas o padrão é real e é um só: **uma coisa declarada num sítio, sem nada em lado
nenhum a obrigá-la a ser verdade.** Quatro dos nove desta noite são isso — o
`subagents: false` guardado por uma literal noutra linguagem, o "nunca escreve"
que era uma etiqueta, o `failing()` que ninguém lia, o `curator` que ninguém
chamava.

O repositório já sabe disto. Tem o ts-rs a gerar os tipos, o `check:store` a ler
o `useMemo` pelo AST, o `vocabulary.rs` a derivar o `LEGAL_MOVES` do domínio.
Falhou onde essa maquinaria não chega — e o `DEBT.md` é, em boa parte, a lista da
mesma classe, notada em vez de fechada.

O `check:commands` fecha a primeira. Lê o `generate_handler!` e o `ipc.ts`, e
falha em três casos: um comando registado sem embrulho, um embrulho para um
comando que não existe, e um embrulho que ninguém chama. As duas listas de
excepções vivem no script com a razão de cada uma, e o guarda também falha ao
contrário: uma excepção que deixou de o ser é apanhada em vez de ficar a mentir.

Encontrou quatro coisas que ninguém sabia:

- `api.checks`, `api.codexStatus` e `api.inbox` — embrulhos sem chamador, que se
  juntam aos cinco que o `DEBT.md` já nomeava;
- e o `prepare_shutdown`, que é pior do que estar sem porta: está registado, não
  é invocado por ninguém — nem pela janela nem pelo Rust — e corre exactamente
  os dois passos que o `closing.rs:117-129` já corre. Uma segunda cópia de
  lógica viva é a pior espécie de código morto, porque quem editar uma não edita
  a outra. Fica nomeado e por decidir; apagá-lo é uma escolha, não uma limpeza.

Provado a partir-se antes de ser acrescentado ao portão: um embrulho para um
comando inexistente e um comando registado sem embrulho, os dois apanhados.

Entra no `check`, no `CLAUDE.md` e no "Check before publishing" do CI. O que se
quer impedir não são as listas de excepções — é elas crescerem sem ninguém
decidir.

### 129. A fronteira que nenhum compilador atravessa passou a ser gerada

O #128 fechou a primeira classe de "declarado num sítio, sem nada a obrigá-lo".
Esta é a segunda, e é a que custou mais: tudo entre o `agent-sidecar` e o
`sidecar/index.mjs` viaja em JSON, e o JSON é onde os dois compiladores param de
olhar. Um nome escrito de um lado e desconhecido do outro não estoira — é
serializado, atravessa o cano, e cai num `match` sem braço para ele. Sem erro,
sem aviso: um ramo que nunca corre.

Foi assim que o `subagents: false` esteve desligado desde o dia em que foi
escrito (#124). Nada podia ter falhado, porque as duas metades nunca foram
apresentadas uma à outra.

**As grafias deixaram de ser literais.** O `SUBAGENT_TOOLS` vive no
`crates/ports`, e o `crates/app/src/protocol.rs` escreve o
`sidecar/protocol.generated.mjs` que o sidecar importa — a mesma mecânica que o
`vocabulary.rs` já usava para o frontend, e pela mesma razão. A regra que
importa é a que se herda daí: **os nomes não são escritos ali.** Um `kind` sai
de serializar um `RunEvent` de verdade, portanto o que o sidecar conhece é, por
construção, o que o adaptador lê. Um teste do sidecar recusa-lhe voltar a ter
cópia própria.

O `one_of_each` é escrito à mão, e uma lista à mão que ninguém obriga a estar
completa é a forma exacta do defeito que isto vem fechar — por isso o
`every_variant_is_listed` lê o enum na fonte e exige cobertura. Viu-se a falhar
com uma variante retirada antes de se acreditar nele.

**E o resto do vocabulário ganhou guarda.** Os `kind` continuam a ser literais
dos dois lados — são demasiados para valerem uma constante cada — portanto o
`check:protocol` compara-os: todo o `kind` que o sidecar emite tem de ser um que
o Rust serialize **ou** um braço do `match kind` do adaptador. Os dois conjuntos
e não só o primeiro, porque nem tudo o que atravessa é um evento: o
`message_read` é traduzido para `UserRead` e para uma marca na fila, portanto
existe no cano e não existe no enum. Derivar os dois conjuntos é o que distingue
uma tradução de uma gralha sem uma lista de excepções a envelhecer.

Ao contrário não é erro: há `kind` que só o Rust produz — o `thought`, selado
pelo `chat.rs` a partir das fatias — e que o sidecar nunca escreve. Folga, não
deriva.

Provado a partir-se com um `kind` trocado e com o módulo gerado desactualizado,
que é o outro modo de falhar: um `pnpm codegen` esquecido deixa o sidecar a
importar uma lista velha, e uma lista velha é a segunda cópia que isto veio
remover.

### 130. As nove portas sem ecrã, ligadas — e uma delas escondia uma funcionalidade inteira

O #128 pôs o guarda e nomeou nove embrulhos sem chamador. O operador leu a lista
e apanhou a entrada errada: *"inbox no caller? i can't see what the director is
proposing"*.

Tinha razão, e a minha razão escrita ao lado estava errada. Eu dizia
"redundante: o `bootstrap` traz as propostas e o evento traz as seguintes" —
verdade sobre o **carregamento**, e a coisa errada a concluir daí. As propostas
carregam, actualizam-se por evento e entram no contexto do store; **nenhum
componente as lia**. O único sítio que as mostrava era o rail do RightNow, que o
#89 tirou. Estavam catorze por ler no disco, enquanto o prompt do Director o
manda usar o `propose_improvement` a cada ferramenta recusada. Ele escrevia para
um sítio sem leitor, e depois era chamado de burocrático por isso.

Uma razão errada num ficheiro cujo propósito é as razões serem honestas é pior
do que não a ter. Ficou como o próprio guarda a apanhar: quando o `api.inbox`
passou a ser chamado, o `check:commands` recusou-se a deixar a excepção
sobreviver.

As nove:

- **`inbox`** — ecrã próprio, linha na nav com contador do que espera, aceitar e
  dispensar. Aceitar é permissão e não trabalho, e a linha di-lo.
- **`checks` + `projectUpdate`** — o `ProjectPage` voltou a ter destino. Existia
  inteiro e nada o importava desde o `6bc7309`; com ele voltaram o grafo, os
  ramos e as linguagens, e **configurar os checks passou a ter interface** — que
  desde o #122 é uma decisão com efeito, porque um check vermelho recusa uma
  aprovação. O `projectUpdate` ficou onde faltava mais: um projecto em pausa
  recusa todo o arranque e não havia por onde o retomar.
- **`cardRunChecks`** — na revisão, ao lado do que aprova. Correr outra vez
  depois de mexer no código era o que faltava.
- **`overrideCard`** — no `drop()` do quadro, que recusava um movimento ilegal
  **em silêncio**. Agora pergunta a razão e força com ela escrita: um estado
  forçado sem explicação é uma mentira no histórico.
- **`analystAsk`** — no Activity. Abre a conversa e salta para lá, porque é lá
  que a resposta chega.
- **`codexStatus`** — nas Settings, ao lado do sidecar. Um agente em Codex sem
  login falhava no arranque e o ecrã não tinha por onde dizer que era isso.
- **`openAgentTerminal`** — nas Worktrees, e só onde há cartão, porque é por ele
  que o comando resolve a sessão.
- **`approvalsPending`** — ao abrir a folha de permissões, para perguntar em vez
  de acreditar. A fila chega por evento, e um evento pode não chegar: foi
  exactamente isso o #125.

E uma frase que deixou de ser verdade, encontrada a caminho: a revisão dizia
"Relay does not merge — the branch and its worktree stay until you remove them".
Com o #119 aprovar integra. Uma frase que descreve o que a app já não faz é pior
do que nenhuma.

Fica **88 embrulhos, 88 chamados, zero sem ecrã**. A única excepção que resta é
do outro lado: o `prepare_shutdown`, registado e invocado por ninguém.

### 131. O Director conversava sobre o trabalho em vez de o fazer

O operador: *"i said i wanna do an experiment, he says blah blah blah and why
you shouldn't… i point him at something he should do it."*

O transcript dá-lhe razão, e num sítio exacto. À pergunta "isto precisa de uma
experiência, qual seria um bom começo — Remotion ou outra coisa?", a resposta
foi:

> Take your time. Nothing's running, nothing's costing you anything. (…) I'll
> write card 1 whenever you say. Or leave it and come back to it.

Uma pergunta que queria investigação e uma recomendação levou um adiamento. A
verificação da licença que a resolveu — e resolveu-a bem — só aconteceu depois
de ele escrever "hm".

**Duas causas, e a segunda é pior.**

**A frase.** O prompt dizia "Only put work on a board when they ask… **and say
what you are about to do before you do it**". A primeira metade foi escrita
contra transformar cada pergunta em maquinaria e funciona. A segunda lê-se como
anunciar e esperar. Saiu, e no lugar dela entraram três regras: agir no mesmo
turno (apontar é a instrução — "isto precisa de uma experiência" quer dizer
*corre a experiência*); as quatro coisas que param e mais nenhuma (dinheiro
acima do combinado, destrutivo ou irreversível, uma bifurcação no *que* se
constrói, e uma concessão); e discordar sem deixar de trabalhar — dizer que a
ideia é fraca em duas linhas e depois fazer a versão mais forte dela na mesma,
porque um mau resultado vê-se e uma coisa não construída não.

**E o `record_decision` era write-only.** O operador já tinha ditado esta regra
— *"verified work proceeds without asking: approve, merge, start the next
card"*, escrita depois de o Director lhe pedir duas vezes numa sessão uma
permissão que já tinha. O tool escreveu o ficheiro. **Nada o lia.** Nenhum
prompt, nem do chat nem de um cartão, alguma vez carregou
`memory/decisions/`. Seis ficheiros nos dois projectos, todos por ler.

É o mesmo defeito do `curator` (#120) e da caixa de entrada (#130) pela terceira
vez: um canal que só escreve. E é o pior dos três, porque escrever uma regra dá
a sensação de a ter resolvido — o operador ditou-a, viu-a guardada, e o
comportamento não mudou porque a regra nunca chegou a um turno.

O `memory::decisions_from` lê-as, mais recentes primeiro e com tecto, e entram
nos dois prompts: a conversa e o run de um cartão. Um agente passa a saber o que
já foi decidido no quadro em que trabalha.

Não mexido: as quatro coisas que param. São as do operador, escritas por ele
naquela decisão, e o prompt passa a dizer as mesmas em vez de uma lista minha.
