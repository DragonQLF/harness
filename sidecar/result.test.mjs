/** Regressão: uma conversa deixou de responder sem dar erro nenhum.
 *
 *  A execução acabou às 19:26 e o `handleRun` voltou, mas o processo do CLI
 *  ficou de pé a segurar a sessão. As duas mensagens seguintes retomaram uma
 *  sessão que já era de outro: o CLI entregou-as à fila do processo vivo e saiu
 *  `success` sem correr turno nenhum — custo 0, turnos 0, texto vazio. O
 *  sidecar leu aquilo como uma resposta boa e mandou um `done` sem erro. As
 *  respostas existem no transcript e nunca chegaram ao ecrã.
 *
 *  Duas metades, e as duas fazem falta: reconhecer o turno vazio, e não deixar
 *  o processo sobreviver à execução a que pertence. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

// O index.mjs abre um readline ao ser importado, por isso avalia-se só a função
// — como no mcp-wiring.test.mjs.
function loadAnsweredNothing() {
  const start = source.indexOf("function answeredNothing(");
  const end = source.indexOf("function summarize(");
  assert.ok(start > -1 && end > start, "answeredNothing mudou de sítio; actualizar este teste");
  return new Function(`${source.slice(start, end)}; return answeredNothing;`)();
}

test("o turno que não correu não passa por resposta", () => {
  const answeredNothing = loadAnsweredNothing();

  // A assinatura exacta do 19:29:09: init, result, e nada pelo meio.
  assert.equal(
    answeredNothing({ subtype: "success", num_turns: 0, total_cost_usd: 0, result: "" }),
    true,
    "success com zero turnos e sem texto é o pedido entregue à fila de outro processo",
  );
  // Os campos podem nem vir.
  assert.equal(answeredNothing({ subtype: "success" }), true);
});

test("um turno a sério não é confundido com esse", () => {
  const answeredNothing = loadAnsweredNothing();

  assert.equal(
    answeredNothing({ subtype: "success", num_turns: 3, result: "feito." }),
    false,
  );
  // O caso que obriga a olhar para os turnos e não só para o texto: um turno
  // que só chamou ferramentas acaba sem texto, e correu.
  assert.equal(
    answeredNothing({ subtype: "success", num_turns: 2, result: "" }),
    false,
    "um turno só de ferramentas é um turno",
  );
});

test("os erros continuam a ser erros, e com o texto deles", () => {
  const answeredNothing = loadAnsweredNothing();

  // Estes já eram apanhados pelo `subtype !== success`. O que interessa é que
  // não passem por aqui: senão perdiam a razão legível e ficavam com a do
  // turno vazio, que não é a deles.
  for (const subtype of ["error_during_execution", "error_max_turns"]) {
    assert.equal(answeredNothing({ subtype, num_turns: 0, result: "" }), false, subtype);
  }
});

test("o veredicto está ligado ao que decide, e não só calculado", () => {
  // Um predicado certo que ninguém usa dá exactamente a avaria de origem. As
  // duas ligações que contam: entra no `failed`, e traz razão própria em vez
  // da genérica — que aqui diria "success".
  const block = source.slice(
    source.indexOf('case "result": {'),
    source.indexOf('          return;\n        }\n      }'),
  );
  assert.match(block, /const nothing = answeredNothing\(message\)/);
  assert.match(block, /const failed =[\s\S]*?\|\| nothing;/, "o turno vazio tem de reprovar");
  assert.match(block, /const detail = nothing\s*\?/, "e tem de dizer porquê");
});

test("nada sobrevive à execução a que pertence", () => {
  // A outra metade não se consegue exercitar sem levantar o SDK inteiro, e o
  // que ela vale é ser incondicional — está no `finally`, não num dos ramos de
  // saída. Isso lê-se na fonte, e é o que aqui se guarda.
  const finallyBlock = source.slice(source.indexOf("  } finally {", source.indexOf("async function handleRun")));
  assert.match(
    finallyBlock.slice(0, finallyBlock.indexOf("\n}")),
    /ac\.abort\(\)/,
    "o handleRun tem de abortar o controlador ao sair, ou o CLI fica a segurar a sessão",
  );
});
