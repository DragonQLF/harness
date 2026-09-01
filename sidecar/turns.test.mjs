/** Regressão: um turno era cobrado uma vez por bloco de conteúdo.
 *
 *  Uma mensagem do assistente é um turno do modelo, mas o SDK entrega-a uma vez
 *  por bloco — texto, `tool_use`, `tool_use` — e cada entrega traz o `usage`
 *  inteiro da mensagem, não uma fatia dele. O sidecar contava-as à chegada.
 *
 *  A prova está no log de 2026-09-01, na execução `f6995015`: 317 eventos de
 *  `usage` para os 72 turnos que o próprio SDK reportou no `done`, e valores
 *  repetidos em sequência exactamente pelo número de blocos de cada turno:
 *
 *      usage cr=17291      ← uma mensagem,
 *        tool_use Bash        dois blocos `tool_use`,
 *      usage cr=17291      ← o mesmo `usage` outra vez
 *        tool_use Bash
 *
 *  O `conversations.rs` soma esses eventos, portanto tudo o que deles saía
 *  vinha inflacionado: 1,45x na execução, 2,01x na conversa do Director
 *  (1292 eventos → 606 turnos, 566M → 281M tokens). O número que o operador usa
 *  para escolher um modelo era o dobro do que se gastou.
 *
 *  A segunda metade é o subagente: os turnos dele chegam a este mesmo fluxo. O
 *  gasto é real e conta, mas o contexto é outro — sem os distinguir, o
 *  indicador de contexto lê o do subagente e salta 34967 → 8544 → 34967 ao
 *  atravessar uma chamada `Task`. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

// O index.mjs abre um readline ao ser importado, por isso avalia-se só a função
// — como no result.test.mjs e no mcp-wiring.test.mjs.
function loadTurnFrom() {
  const start = source.indexOf("function turnFrom(");
  const end = source.indexOf("/** O conjunto de trabalho de fundo vivo");
  assert.ok(start > -1 && end > start, "turnFrom mudou de sítio; actualizar este teste");
  return new Function(`${source.slice(start, end)}; return turnFrom;`)();
}

/** Uma mensagem do assistente como o SDK a entrega, uma vez por bloco. */
function assistant(id, usage, { parent = null } = {}) {
  return {
    type: "assistant",
    parent_tool_use_id: parent,
    message: { id, model: "claude-opus-5", usage },
  };
}

const USAGE = {
  input_tokens: 2,
  output_tokens: 3,
  cache_read_input_tokens: 17291,
  cache_creation_input_tokens: 1517,
};

test("um turno de três blocos conta uma vez, e o usage vai uma vez", () => {
  const turnFrom = loadTurnFrom();
  const counted = new Set();

  // A assinatura exacta do log: a mesma mensagem, três entregas.
  const first = turnFrom(assistant("msg_01", USAGE), counted);
  const second = turnFrom(assistant("msg_01", USAGE), counted);
  const third = turnFrom(assistant("msg_01", USAGE), counted);

  assert.ok(first, "a primeira entrega é o turno");
  assert.equal(first.usage.cache_read_tokens, 17291);
  assert.equal(second, null, "a segunda entrega é o mesmo turno");
  assert.equal(third, null, "e a terceira também");
});

test("mensagens diferentes são turnos diferentes", () => {
  const turnFrom = loadTurnFrom();
  const counted = new Set();

  assert.ok(turnFrom(assistant("msg_01", USAGE), counted));
  assert.ok(turnFrom(assistant("msg_02", USAGE), counted), "outro id, outro turno");
});

test("o turno do subagente conta, e vai marcado", () => {
  const turnFrom = loadTurnFrom();
  const counted = new Set();

  const mine = turnFrom(assistant("msg_01", USAGE), counted);
  const child = turnFrom(assistant("msg_02", USAGE, { parent: "toolu_01" }), counted);

  assert.equal(mine.usage.subagent, false);
  assert.ok(child, "o gasto do subagente é gasto e conta");
  assert.equal(child.usage.subagent, true, "mas o contexto dele não é o desta sessão");
});

test("uma mensagem sem id é contada, não engolida", () => {
  const turnFrom = loadTurnFrom();
  const counted = new Set();

  // Um adaptador que não numere as mensagens perderia todos os turnos se a
  // ausência de id contasse como "já visto". Falhar para o lado de contar.
  assert.ok(turnFrom(assistant(undefined, USAGE), counted));
  assert.ok(turnFrom(assistant(undefined, USAGE), counted));
});

test("um turno sem usage continua a ser um turno", () => {
  const turnFrom = loadTurnFrom();
  const counted = new Set();

  const turn = turnFrom(assistant("msg_01", undefined), counted);
  assert.ok(turn, "conta para os turnos");
  assert.equal(turn.usage, null, "e não inventa números que não vieram");
});
