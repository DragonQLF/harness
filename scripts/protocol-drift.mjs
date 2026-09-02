#!/usr/bin/env node
/**
 * Guarda: o sidecar e o adaptador têm de dizer as mesmas palavras.
 *
 * Tudo entre o `crates/adapters/agent-sidecar` e o `sidecar/index.mjs` viaja
 * em JSON, e o JSON é onde os dois compiladores param de olhar. Um `kind`
 * escrito de um lado e não conhecido do outro não estoira: o evento é
 * serializado, atravessa o cano, e cai num `match` que não tem braço para ele.
 * Não há erro, não há aviso — há um ramo que nunca corre.
 *
 * Foi assim que o `subagents: false` esteve desligado desde o dia em que foi
 * escrito: o guarda comparava com `Task` e o modelo chamava `Agent`. Essa
 * metade fechou-se por construção — as grafias vêm agora do
 * `protocol.generated.mjs`, que o Rust escreve. Esta é a outra metade: os
 * `kind` continuam a ser literais nos dois lados, e são demasiados para
 * valerem uma constante cada.
 *
 * Duas regras, e a segunda é a que importa mais:
 *
 *   1. Todo o `kind` que o sidecar emite é um `kind` que o Rust serializa —
 *      senão é um evento que ninguém do outro lado sabe ler.
 *   2. O módulo gerado está em dia com o Rust. Um `pnpm codegen` esquecido
 *      deixa o sidecar a importar uma lista velha, e uma lista velha é
 *      exactamente a segunda cópia que isto veio remover.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(join(root, p), "utf8");

/** Os `kind` no módulo gerado — a verdade, escrita pelo Rust. */
function conhecidos() {
  const js = read("sidecar/protocol.generated.mjs");
  const bloco = js.slice(js.indexOf("EVENT_KINDS"), js.indexOf("SUBAGENT_TOOLS"));
  return new Set([...bloco.matchAll(/"([a-z_]+)"/g)].map((m) => m[1]));
}

/**
 * Os `kind` que o adaptador trata explicitamente.
 *
 * Nem todo o `kind` do cano é um `RunEvent`: alguns são traduzidos. O
 * `message_read` é o caso — o adaptador marca a mensagem como lida na fila e
 * emite um `UserRead`, portanto a palavra existe no cano e não existe no enum.
 * Aceitar os dois conjuntos é o que distingue uma tradução de uma gralha, sem
 * precisar de uma lista de excepções a envelhecer no fundo deste ficheiro.
 */
function tratados() {
  const rs = read("crates/adapters/agent-sidecar/src/lib.rs");
  const abre = rs.indexOf("match kind {");
  if (abre < 0) return new Set();
  const bloco = rs.slice(abre, rs.indexOf("\n                    }", abre));
  const out = new Set();
  for (const m of bloco.matchAll(/^\s*((?:"[a-z_]+"\s*\|\s*)*"[a-z_]+")\s*=>/gm)) {
    for (const nome of m[1].matchAll(/"([a-z_]+)"/g)) out.add(nome[1]);
  }
  return out;
}

/** Os `kind` que o sidecar escreve nos eventos que manda. */
function emitidos() {
  const js = read("sidecar/index.mjs");
  return new Set([...js.matchAll(/\bkind:\s*"([a-z_]+)"/g)].map((m) => m[1]));
}

const sabidos = conhecidos();
const escritos = emitidos();
const lidos = tratados();
const erros = [];

if (sabidos.size === 0) {
  erros.push(
    "o `sidecar/protocol.generated.mjs` não tem `kind` nenhum — correr `pnpm codegen`",
  );
}

for (const kind of escritos) {
  if (!sabidos.has(kind) && !lidos.has(kind)) {
    erros.push(
      `o sidecar emite \`kind: "${kind}"\` e o Rust não o conhece: ` +
        `nem é um \`RunEvent\` que ele serialize, nem um braço do \`match kind\` ` +
        `do adaptador. O evento atravessa o cano e não é lido por ninguém`,
    );
  }
}

// O contrário não é erro: há `kind` que só o Rust produz (o `thought`, que o
// `chat.rs` sela a partir das fatias) e que o sidecar nunca escreve. Uma lista
// maior do que o que se emite é folga, não deriva.

if (erros.length > 0) {
  console.error("O sidecar e o Rust deixaram de dizer o mesmo:\n");
  for (const e of erros) console.error(`  - ${e}`);
  console.error("");
  process.exit(1);
}

console.log(
  `OK — ${escritos.size} kinds emitidos pelo sidecar, todos conhecidos pelo Rust: ` +
    `${sabidos.size} eventos gerados do \`RunEvent\` e ${lidos.size} braços do adaptador.`,
);
