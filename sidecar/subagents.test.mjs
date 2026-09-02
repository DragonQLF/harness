/** Regressão: `subagents: false` não travava nada.
 *
 *  Os três guardas do `canUseTool` estavam escritos contra `Task`. O SDK
 *  renomeou a ferramenta para `Agent` algures pelo caminho, e a partir daí
 *  nenhum deles disparava:
 *
 *    - a conversa do Director põe `subagents: false` (`chat.rs`) e abriu
 *      **nove** subagentes numa só conversa;
 *    - o tecto de profundidade nunca subiu, portanto **quatro** desses foram
 *      abertos *por* subagentes — subagentes a abrir subagentes, que é
 *      exactamente o que o guarda existia para impedir;
 *    - e o `PostToolUse` que devolve o contador também nunca correu.
 *
 *  Nos logs desta máquina há 7 chamadas `Agent` e zero `Task`. Aceitam-se as
 *  duas grafias: o nome já mudou uma vez, e falhar fechado é o lado certo. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

// Vem do módulo gerado, que é agora a única fonte: o `crates/app/src/protocol.rs`
// escreve-o a partir do que o Rust serializa, e o `pnpm codegen` regenera-o.
import { isSubagentTool, SUBAGENT_TOOLS } from "./protocol.generated.mjs";

function loadIsSubagentTool() {
  return isSubagentTool;
}

test("o nome que o SDK usa hoje conta como subagente", () => {
  const isSubagentTool = loadIsSubagentTool();
  // O que está nos logs: 7 `Agent`, 0 `Task`.
  assert.equal(isSubagentTool("Agent"), true, "é este o nome que o modelo chama");
});

test("o nome antigo continua a contar", () => {
  const isSubagentTool = loadIsSubagentTool();
  assert.equal(isSubagentTool("Task"), true, "um CLI mais velho ainda lhe chama assim");
});

test("e nada mais é um subagente", () => {
  const isSubagentTool = loadIsSubagentTool();
  for (const other of ["Read", "Bash", "WebSearch", "AskUserQuestion", "agent", "TaskRunner", ""]) {
    assert.equal(isSubagentTool(other), false, `${other} não abre subagente nenhum`);
  }
});

test("os três guardas perguntam pelo nome, não comparam com uma grafia", () => {
  // O defeito não foi um guarda em falta — foi três guardas a comparar com uma
  // literal que deixou de ser verdade. Se voltarem a comparar, isto falha.
  const guards = source.slice(source.indexOf("const canUseTool"));
  assert.ok(
    !/toolName === "Task"/.test(guards),
    "um guarda voltou a comparar com a literal `Task`",
  );
  assert.ok(
    !/toolName === "Agent"/.test(guards),
    "e comparar com `Agent` tem o mesmo defeito da próxima vez que o nome mudar",
  );
  assert.equal(
    (guards.match(/isSubagentTool\(toolName\)/g) ?? []).length,
    2,
    "a recusa e o contador de profundidade, os dois pela mesma pergunta",
  );
});

test("o sidecar não tem cópia própria do nome — usa a gerada", () => {
  // O defeito era uma literal deste lado do cano a discordar do outro. Uma
  // segunda cópia, ainda que certa hoje, é a mesma armadilha adiada.
  assert.ok(
    !/function\s+isSubagentTool/.test(source),
    "o sidecar voltou a escrever a sua própria versão em vez de importar a gerada",
  );
  assert.ok(
    source.includes('from "./protocol.generated.mjs"'),
    "o sidecar tem de importar o vocabulário gerado",
  );
});

test("as grafias vêm do Rust e não deste ficheiro", () => {
  assert.deepEqual([...SUBAGENT_TOOLS], ["Agent", "Task"]);
});

test("o PostToolUse cobre as duas grafias", () => {
  const hook = source.slice(source.indexOf("PostToolUse:"), source.indexOf("PostToolUse:") + 400);
  assert.ok(hook.includes('"Agent"'), "sem isto o contador sobe e nunca desce");
  assert.ok(hook.includes('"Task"'));
});
