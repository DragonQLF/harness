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
