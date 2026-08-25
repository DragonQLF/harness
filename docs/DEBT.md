# Dívida técnica — reescrita a 2026-08-26

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
| Toggle `mirror` na UI | Hoje via `"mirror": true` no projects.json | #65 |
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
| Fecho do dia bloqueia o fecho da janela | Custo conhecido e deliberado: até ~120s no shutdown quando due ($0.30 de tecto). Se incomodar, desagendar ou encolher o relógio em `reflection.rs` | #79 |
| `self_report` relê transcrições inteiras por chamada | OK com semanas de dados; se crescer, filtrar por `ts_ms` ao ler em vez de depois | #78 |
| Caixa de entrada sem notificação fora do rail | Propostas só aparecem no RightNow; nenhum badge na nav nem toast quando chegam | #79 |
