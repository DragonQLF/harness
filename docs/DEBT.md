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
| UI de resultados: fechado — resultado funde com a chamada pelo id, aninhamento por pai, detalhe expansível (chat + sessions) | Feito | #70 |`n| Timers vivos | "há 2 min"/durações só actualizam ao re-render: sessão a correr (Sessions.tsx:114, RightNow.tsx:411), relógios de aprovação/chat/actividade (Overlays 146, Chat 106/577, Misc 311, NavRail 345, Projects 306). Fix: um `useTicker(ms)` partilhado consumido só onde o tempo é o dado | checklist do operador |
| Drag & drop, timer de inatividade, inspector do event log | Nunca começados | — |
