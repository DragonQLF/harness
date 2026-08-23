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

## Conversas que sobrevivem ao restart (2026-08-23)

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

## Dívida técnica conhecida (atualizada)

- Compaction do event log.
- Sem diff viewer dentro da UI.
- `max_concurrent` é guardado e mostrado, mas ainda não limita runs em paralelo.
- Continua não verificado com o modelo a correr: um Builder a levar um cartão de
  ready a review dentro da app (herdado da sessão anterior).
