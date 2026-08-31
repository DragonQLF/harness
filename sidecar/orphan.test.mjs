/** O sidecar tem de morrer com a Relay.
 *
 *  A limpeza do lado da Rust corre no fim do `drive`. Uma Relay que morre a
 *  meio de um turno — um force quit, um crash, o instalador a reiniciá-la —
 *  nunca lá chega, e o sidecar fica órfão com o CLI ao colo. Órfão, continua a
 *  segurar a sessão: as mensagens seguintes vão para a fila dele e as respostas
 *  saem por um stream que já ninguém lê (#108). Aconteceu mesmo, e não em
 *  teoria: um deles ficou doze horas assim, e nesse tempo todas as mensagens
 *  daquela conversa foram recusadas.
 *
 *  O stdin é o fio. A Relay nunca o fecha enquanto está viva — mantém-no aberto
 *  para o comando seguinte —, por isso um EOF só quer dizer que ela se foi, e o
 *  sistema entrega esse EOF mesmo quando ela morre de SIGKILL.
 *
 *  O que aqui se guarda é a estrutura do tratamento, e vale a pena dizer
 *  porquê. Parado, o sidecar já se ia abaixo sozinho ao fim do stdin: sem nada
 *  a segurar o event loop, o node sai por si. Um teste que só fechasse o stdin
 *  passava com o tratamento *e sem ele* — e foi o que fez, escrito assim à
 *  primeira. O caso que interessa é o outro, o do turno vivo, e esse não se
 *  monta sem levantar uma sessão a sério contra a API, que é coisa que o CI não
 *  pode correr. Fica a estrutura, e fica dito que a outra metade não está
 *  coberta aqui. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

function closeHandler() {
  const start = source.indexOf('rl.on("close"');
  assert.ok(start > -1, "o sidecar deixou de reagir ao fim do stdin: volta a poder ficar órfão");
  return source.slice(start);
}

test("o fim do stdin desmonta o que estiver a correr", () => {
  const handler = closeHandler();
  // Abortar primeiro dá ao SDK a hipótese de derrubar o CLI em condições. Sem
  // isto o `kill` de baixo é a única saída, e é a bruta.
  assert.match(
    handler,
    /for \(const ac of controllers\.values\(\)\) ac\.abort\(\)/,
    "um turno vivo tem de ser abortado, ou o CLI fica de pé",
  );
});

test("e leva o grupo, que é o que apanha o CLI", () => {
  const handler = closeHandler();
  // O sinal negativo é o ponto todo: `process.kill(pid)` matava só o node e
  // deixava o neto vivo — a avaria de origem, outra vez.
  assert.match(
    handler,
    /process\.kill\(-process\.pid, "SIGKILL"\)/,
    "sem o grupo, o CLI sobrevive ao sidecar",
  );
  // E não fica pendurado se o grupo já não existir.
  assert.match(handler, /catch \{[\s\S]*?process\.exit\(0\)/);
});
