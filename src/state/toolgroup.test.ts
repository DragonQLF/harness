import { test } from "node:test";
import assert from "node:assert/strict";
import { countGroupLines, summariseTools } from "./toolgroup.ts";
import type { ChatMsg } from "./bubbles.ts";

const call = (tool: string): ChatMsg => ({ role: "tool", text: "", ts: 0, tool, ok: true });

/** O cabeçalho do grupo diz o que aconteceu, não que ferramentas foram
 *  chamadas. É a diferença entre "Edited 3 files, ran 7 commands" e
 *  "Edit ×3, Bash ×7". */
test("um grupo conta-se por família e no singular quando é uma só", () => {
  assert.equal(
    summariseTools([
      call("Edit"),
      call("Edit"),
      call("Write"),
      ...Array.from({ length: 7 }, () => call("Bash")),
    ]),
    "Edited 3 files, ran 7 commands",
  );
  assert.equal(summariseTools([call("Bash")]), "Ran 1 command");
  assert.equal(summariseTools([call("Grep")]), "Searched once");
  assert.equal(summariseTools([call("Grep"), call("Glob")]), "Searched 2 times");
});

/** Uma ferramenta que não se reconhece diz o nome dela. Inventar-lhe um verbo
 *  era descrever trabalho que ninguém sabe qual foi. */
test("o que não se reconhece conta-se pelo nome", () => {
  assert.equal(summariseTools([call("generate_image")]), "generate_image ×1");
  assert.equal(
    summariseTools([call("Edit"), call("generate_image"), call("generate_image")]),
    "Edited 1 file, generate_image ×2",
  );
});

test("um grupo vazio não diz nada", () => {
  assert.equal(summariseTools([]), "");
});

const edit = (added: number, removed: number): ChatMsg => ({
  role: "tool",
  text: "",
  ts: 0,
  tool: "Edit",
  ok: true,
  added,
  removed,
});

/** `+0 −0` é uma afirmação sobre trabalho que ninguém mediu. Um grupo que não
 *  mexeu em linhas não mostra número nenhum. */
test("um grupo sem nada para contar não conta nada", () => {
  assert.equal(countGroupLines([call("Bash"), call("Read")]), null);
  assert.equal(countGroupLines([]), null);
});

test("o que se sabe soma-se, mesmo entre chamadas que não sabem", () => {
  assert.deepEqual(countGroupLines([edit(7, 0), edit(56, 7), call("Bash")]), {
    added: 63,
    removed: 7,
  });
  // Uma só chamada que saiba chega para haver número.
  assert.deepEqual(countGroupLines([call("Bash"), edit(2, 1)]), { added: 2, removed: 1 });
});
