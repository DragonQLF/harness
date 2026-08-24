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
| UI: ToolResult aninhável/expansível (ids já no stream e no log; falta render com indent por `parent_tool_use_id`, pendente/verde/vermelho) | Por fazer; a cadeia sidecar→RunEvent→log está completa | #67 |`n| Drag & drop, timer de inatividade, inspector do event log | Nunca começados | — |
