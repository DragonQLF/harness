import { test } from "node:test";
import assert from "node:assert/strict";
import { countGroupLines, decodePath, groupView, summariseTools } from "./toolgroup.ts";
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

/** O bug que se viu no ecrã: uma chamada única nascia com cabeçalho e aberta, e
 *  fechá-la deixava a ficha e levava o cabeçalho — o botão de fechar entregava
 *  um estado pior do que aquele de onde veio. Uma chamada não tem estado. */
test("uma chamada é uma ficha, e não tem aberto nem fechado", () => {
  assert.equal(groupView(1, false, null), "chip");
  assert.equal(groupView(1, true, null), "chip");
  // Nem carregando: não há botão para carregar.
  assert.equal(groupView(1, false, true), "chip");
  assert.equal(groupView(1, false, false), "chip");
  assert.equal(groupView(0, false, null), "chip");
});

/** O que está a acontecer agora não se esconde atrás de um resumo; o que já
 *  acabou esconde-se, e abre-se a pedido. */
test("um grupo a meio abre-se sozinho, um acabado fecha-se", () => {
  assert.equal(groupView(3, true, null), "open");
  assert.equal(groupView(3, false, null), "closed");
  // E o operador ganha sempre à omissão, nos dois sentidos.
  assert.equal(groupView(3, true, false), "closed");
  assert.equal(groupView(3, false, true), "open");
});

/** O bug que se viu: a imagem existia, o caminho estava certo, e o que se
 *  pedia ao disco era "Application%20Support" — que nunca foi um sítio. */
test("um src de markdown volta a ser um caminho", () => {
  assert.equal(
    decodePath("/Users/f/Library/Application%20Support/com.harness.app/x.png"),
    "/Users/f/Library/Application Support/com.harness.app/x.png",
  );
  // Um caminho que ninguém codificou passa intacto — é o caso do `Read`, que
  // traz o caminho do disco e não uma URL.
  assert.equal(decodePath("/tmp/shots/a b.png"), "/tmp/shots/a b.png");
  // E um `%` literal não faz rebentar nada: fica o que veio, que é a leitura
  // certa quando ninguém codificou.
  assert.equal(decodePath("/tmp/100%/x.png"), "/tmp/100%/x.png");
});
