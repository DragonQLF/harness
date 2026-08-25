# Dívida técnica — reescrita a 2026-08-24

Este ficheiro é **reescrito a cada passagem** a partir do que as decisões dizem,
não acumulado. A versão acumulada dentro do DECISIONS chegou a contradizer-se
(dizia que compaction/ts-rs/e2e estavam por fazer depois de #50/#51/#56 os
registarem como feitos).

| Item | Estado real | Aponta para |
|---|---|---|
| Compaction sob pedido na UI | Automática no arranque feita; falta botão | #50 |
| ts-rs | Feito; exceções manuais documentadas em `types.ts` | #51 |
| Verificação end-to-end | Feita com modelo real (`e2e_sidecar`) | #56 |
| Curador + árvore `areas/` | Por fazer; `WorkReported` já acumula no log | #58–#60 |
| Triador: ficheiros protegidos no risco | Por fazer (operador ainda não os nomeia) | #55 |
| Analista semanal | Hoje é sob pedido; agendador é infraestrutura nova | #55 |
| Toggle `mirror` na UI | Hoje via `"mirror": true` no projects.json | #65 |
| Hooks de telemetria estruturada | Não registados | #24–#31 |
| Grafo de commits com curvas | Hoje é lista classificada, não as pistas do design | #18b |
| Sandbox / confinamento de shell | Adiado conscientemente; pathguard cobre caminhos estruturados, Bash fica na allowlist | #2, #62 |
| UI de resultados no chat | Feito — resultado funde com a chamada pelo id, aninhamento por pai (`parent_tool_use_id`), detalhe expansível (chat + sessions) | #70 |
| Responder aprovações pela conversa | Por fazer: ferramenta `answer_pending(approve\|reject, reason)` para o Director transportar a resposta do operador. Guardas: só o pedido mais recente e só no turno imediatamente seguinte; nunca para destrutivas (`delete_card`, `reject_card` exigem botão). **Pré-requisito da voz** (#69) — sem ecrã a fila é invisível, e encaixa no `AskUserQuestion` já intercetado | #70, #69 |
| #73/1 Pausa por orçamento | **Por fazer — o único que ainda perde trabalho/quota.** O corte mata o processo Claude Code: "pausar" constrói-se do lado do harness = commitar (já corre em Failed) + estado próprio no cartão (pausado, não falhado) + continuar exige tecto novo antes de arrancar. Distinguir do erro genérico pelo texto do corte | #71, #73 |
| #73/2 Revisão do Director visível | Por fazer: sessão da revisão registada como qualquer run; veredicto já vai para `last_review`, falta superfície em Sessions/cartão. Verificar também quando o notice é emitido vs arranque do run | #73 |
| #73/3 RightNow desactualizado | Investigado: deriva do MESMO estado (refresh traz snapshot+stats+activity a cada evento). Buraco está na derivação/memo interna — reproduzir e auditar; a seguir, sequência por evento + refetch de snapshot em buracos | #73 |
| #73/4 Custo e turnos ao vivo | Por fazer: stream só traz totais no `done`; emitir intercalar por turno para ver aproximação de tectos antes de bater neles | #73 |
| #73/5 Relógio 1s enquanto live | **Feito** — tick local de 1s em Sessions e RightNowStrip, parado sem runs; valor continua de `started_ms` | #73 |
| Timers vivos (restantes) | "há 2 min" em rótulos (Overlays 146, Chat 106/577, Misc 311, NavRail 345, Projects 306) ainda só actualiza ao re-render — usar o mesmo padrão de tick quando tocares | checklist do operador |
| Drag & drop, timer de inatividade, inspector do event log | Nunca começados | — |
