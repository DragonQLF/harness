// Gerado por `pnpm codegen` a partir do `crates/app/src/protocol.rs`.
// Não editar à mão: os nomes aqui são os que o Rust serializa, e é isso
// que os torna verdadeiros dos dois lados do cano.

/** Os `kind` que um evento pode ter, tal como o adaptador os lê. */
export const EVENT_KINDS = Object.freeze([
  "approval_answered",
  "approval_requested",
  "background_tasks",
  "commands",
  "delta",
  "done",
  "failed",
  "local_output",
  "notice",
  "started",
  "text",
  "thinking",
  "thought",
  "tool_result",
  "tool_use",
  "turns",
  "usage",
  "user_message",
  "user_queued",
  "user_read",
]);

/** A ferramenta que abre um subagente, em todas as grafias que a CLI
*  já lhe deu. Era `Task`, é `Agent`; o guarda que comparava com a
*  primeira nunca disparou. */
export const SUBAGENT_TOOLS = Object.freeze([
  "Agent",
  "Task",
]);

/** É esta a chamada que abre um subagente? */
export function isSubagentTool(name) {
  return SUBAGENT_TOOLS.includes(name);
}
