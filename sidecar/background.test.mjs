/** O trabalho de fundo, a caminho do ecrã.
 *
 *  Um turno que responde não é um turno que acabou: pode deixar um comando a
 *  correr por baixo. Isso não passava pela Relay — a única pista era uma linha
 *  de resultado a dizer "running in the background" — e foi essa invisibilidade
 *  que fez o #108 levar uma tarde a encontrar.
 *
 *  O que aqui se guarda é a semântica, que é onde isto se parte em silêncio:
 *  nível e não aresta. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

function loadLiveTasks() {
  const start = source.indexOf("function liveTasks(");
  const end = source.indexOf("function summarize(");
  assert.ok(start > -1 && end > start, "liveTasks mudou de sítio; actualizar este teste");
  return new Function(`${source.slice(start, end)}; return liveTasks;`)();
}

test("o conjunto chega inteiro e na forma que a Relay guarda", () => {
  const liveTasks = loadLiveTasks();

  assert.deepEqual(
    liveTasks({
      tasks: [
        { task_id: "t1", task_type: "shell", description: "sleep 400" },
        { task_id: "t2", task_type: "subagent", description: "reviewer" },
      ],
    }),
    [
      { task_id: "t1", task_type: "shell", description: "sleep 400" },
      { task_id: "t2", task_type: "subagent", description: "reviewer" },
    ],
  );
});

test("o conjunto vazio é uma resposta, não uma ausência", () => {
  const liveTasks = loadLiveTasks();

  // É isto que desliga o indicador. Se o vazio fosse tratado como "não sei",
  // a última tarefa a acabar deixava-o a girar para sempre — que é exactamente
  // a avaria que a semântica de nível existe para evitar.
  assert.deepEqual(liveTasks({ tasks: [] }), []);
  assert.deepEqual(liveTasks({}), []);
});

test("uma tarefa sem id não vai ao ecrã", () => {
  const liveTasks = loadLiveTasks();

  // Sem id não há chave estável: duas dessas seriam a mesma linha a piscar.
  assert.deepEqual(liveTasks({ tasks: [{ task_type: "shell", description: "x" }] }), []);
  assert.deepEqual(liveTasks({ tasks: [{ task_id: "", description: "x" }] }), []);
  assert.equal(liveTasks({ tasks: [{ task_id: "t1" }, null] }).length, 1);
});

test("os campos que faltam ficam vazios, e não `undefined`", () => {
  const liveTasks = loadLiveTasks();

  // O Rust desserializa isto para `String`, não `Option<String>`: um
  // `undefined` no meio fazia a carga inteira cair fora, e o ecrã ficava com o
  // conjunto anterior a dizer que ainda corria.
  assert.deepEqual(liveTasks({ tasks: [{ task_id: "t1" }] }), [
    { task_id: "t1", task_type: "", description: "" },
  ]);
});

test("está ligado ao evento que a Relay lê", () => {
  const branch = source.slice(
    source.indexOf('message.subtype === "background_tasks_changed"'),
    source.indexOf('message.subtype === "local_command_output"'),
  );
  assert.ok(branch.length > 0, "o ramo do system mudou; actualizar este teste");
  assert.match(branch, /kind: "background_tasks"/, "o kind é o que o Rust casa");
  assert.match(branch, /tasks: liveTasks\(message\)/, "um mapeamento à parte volta a divergir");
});
