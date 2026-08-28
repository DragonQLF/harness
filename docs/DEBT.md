# Dívida técnica — reescrita a 2026-08-28

Este ficheiro é **reescrito a cada passagem** a partir do que as decisões dizem,
não acumulado. A versão acumulada dentro do DECISIONS chegou a contradizer-se
(dizia que compaction/ts-rs/e2e estavam por fazer depois de #50/#51/#56 os
registarem como feitos).

| Item | Estado real | Aponta para |
|---|---|---|
| Compaction sob pedido na UI | Automática no arranque feita; falta botão | #50 |
| ts-rs | Feito; exceções manuais documentadas em `types.ts` | #51 |
| Verificação end-to-end | Feita com modelo real (`e2e_sidecar`) | #56 |
| Curador + árvore `areas/` | Promocão mecânica feita (`curator_run`); falta o passe de julgamento com modelo | #58–#60, #77 |
| Triador: ficheiros protegidos no risco | Por fazer (operador ainda não os nomeia) | #55 |
| Analista semanal | Hoje é sob pedido; agendador é infraestrutura nova | #55 |
| Modo Espelho aponta-se sozinho | Não é um interruptor: "Work on Relay itself" procura o repositório do Relay entre os projectos (pelo remote), depois o checkout de onde este binário foi compilado, e só então clona. Um projecto qualquer já não pode reclamar o papel | #65 |
| Hooks de telemetria estruturada | Parciais: expirações de aprovação registadas (#78); o resto não | #24–#31, #78 |
| Grafo de commits com curvas | Hoje é lista classificada, não as pistas do design | #18b |
| Sandbox / confinamento de shell | Adiado conscientemente; pathguard cobre caminhos estruturados, Bash fica na allowlist | #2, #62 |
| UI de resultados no chat | Feito — resultado funde com a chamada pelo id, aninhamento por pai (`parent_tool_use_id`), detalhe expansível (chat + sessions) | #70 |
| Responder aprovações pela conversa | Por fazer: ferramenta `answer_pending(approve\|reject, reason)` para o Director transportar a resposta do operador. Guardas: só o pedido mais recente e só no turno imediatamente seguinte; nunca para destrutivas (`delete_card`, `reject_card` exigem botão). **Pré-requisito da voz** (#69) — sem ecrã a fila é invisível, e encaixa no `AskUserQuestion` já intercetado | #70, #69 |
| #73/1 Pausa por orçamento | **Feito** — `Card.budget_paused` + `SetBudgetPause`; start recusa até o tecto cobrir o gasto | #73, #74 |
| #73/3 Painéis desactualizados | Defesa feita: sequência por evento; buraco → refresh imediato de snapshot/activity/projects para toda a UI. Se ainda houver divergência após isto, aí sim auditar a derivação interna do RightNow | #73 |
| #73/4 Custo e turnos ao vivo | Por fazer: stream só traz totais no `done`; emitir intercalar por turno para ver aproximação de tectos antes de bater neles | #73 |
| Timers vivos (restantes) | "há 2 min" em rótulos (Overlays 146, Chat 106/577, Misc 311, NavRail 345, Projects 306) ainda só actualiza ao re-render — usar o mesmo padrão de tick quando tocares | checklist do operador |
| Drag & drop, timer de inatividade, inspector do event log | Nunca começados | — |
| Fecho do dia bloqueia o fecho da janela | Narrado e escapável: overlay diz o que se espera, conta o tempo, e "Close now" corta (`closing.rs`). Tecto duro de 180s solta a janela aconteça o que acontecer | #79 |
| `self_report` relê transcrições inteiras por chamada | OK com semanas de dados; se crescer, filtrar por `ts_ms` ao ler em vez de depois | #78 |
| Caixa de entrada sem notificação fora do rail | Propostas só aparecem no RightNow; nenhum badge na nav nem toast quando chegam | #79 |
| Publicar uma versão exige um tag à mão | `git tag vX.Y.Z` dispara o workflow; o rascunho é publicado à mão de propósito. O número da versão vive em três ficheiros que têm de concordar (`tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`) e nada verifica que concordam | #79 |
| Cores continuam só no frontend | `TONE` e `STATUS_TONE` ficam em `types.ts` de propósito — o Rust não tem que saber o que um tom resolve. Desde #80 um tom é um conjunto de classes do Tailwind e já não uma variável CSS. É a única parte do vocabulário que não vem do backend | #51, #80 |
| Codegen de um crate só corrompe os tipos | `cargo test -p <crate> --test export_types` regenera tudo com `bigint` em vez de `number`: a feature do ts-rs só unifica ao construir o workspace inteiro. Usar sempre `pnpm codegen`; nada impede o contrário | #51 |
| Endpoints alternativos por testar com um modelo real | Ollama e OpenRouter estão ligados (três variáveis de ambiente por run) e o caminho compila, mas nenhum agente correu ainda contra um modelo que não seja da Anthropic. Falta saber como se portam as chamadas de ferramentas em modelos pequenos | #79 |
| Três testes do engine já falharam sob carga | `a_shared_worktree_is_adopted_after_a_restart_not_rebuilt` e `the_loser_of_the_agent_limit_never_builds`, numa corrida do workspace inteiro com outra compilação em paralelo; passam 3/3 isolados e 3/3 em workspace desde então. Mensagem perdida. **Voltou a 2026-08-28**, num terceiro teste — `a_failed_run_leaves_work_and_the_next_run_finds_it` — com duas corridas de `cargo test --workspace` a competir pela mesma máquina: falhou aos 30.02s, passou isolado em 0.03s. Três ocorrências, três testes diferentes, sempre sob carga paralela: é a família que é sensível ao tempo, não um teste. O `wait_for` tem 30s e o falhado bateu exactamente nos 30.02s, portanto **é** timeout — a hipótese anterior de que não seria está errada. Se for para fechar, é o `wait_for` que precisa de relógio virtual ou de folga maior, não cada teste | 2026-08-28 |
| Versão nunca sobe sozinha | Em 0.2.0 desde 2026-08-26; sobe à mão. O updater compara versões, logo um release novo com a mesma versão não é oferecido a ninguém | #79 |
| Updates só existem nesta máquina (histórico) | O Modo Espelho compila e estaciona o binário em `appdata/updates`: não há versão, não há canal, não há outro SO. Um Mac nunca vê o que este Windows construiu, e não há forma de dizer "esta é mais nova que a que tens". **Resolvido**: `tauri-plugin-updater` lê o `latest.json` do release mais recente, escolhe o asset do SO e verifica a assinatura antes de executar o que descarregou. A chave privada vive fora do repositório (`~/.relay`) e no secret `TAURI_SIGNING_PRIVATE_KEY` | #79 |
| Banner de update: o caminho do espelho ainda só lê ao montar | O feed de releases é consultado ao arrancar e de 3 em 3 horas, mas `updates_list` (builds parqueados por um cartão) continua a ler uma vez. O erro do install já é um toast | #79 |
| Migração para Tailwind por validar a olho | Feita e verificada por construção: `tsc --noEmit` limpo, `pnpm build` a construir, zero `style={{}}` estáticos, e a troca de tema confirmada num probe com as classes reais da app (escuro↔claro e o acento escolhido pelo operador a sobrepor-se). O que **não** foi feito é abrir cada um dos onze ecrãs no Tauri e compará-los com o antes: a app não arranca num browser (`getCurrentWindow()` precisa do runtime) e não há dev server configurado para ela | #80 |
| `.tile:hover` não tem par para o tema claro | Preservado tal e qual: os três valores crus do desenho (`#1e1d19`, `#33302b`) aplicam-se nos dois temas, portanto um cartão claro escurece ao passar por cima. Anotado, não corrigido — é do brief da v2 | #80 |
| `PRODUCT.md` diz que o acento é `#8b7cff`/`#5b53d8` | O `theme.css` tinha `#38adee`/`#0d74b8` e é isso que a migração levou para o `tailwind.config.js`. O documento está desactualizado desde alguma passagem anterior; a reconciliação do acento com o ícone novo é do brief da v2 e não foi tocada | #80 |
