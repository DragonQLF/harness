/** A reatação: o run deixa de morrer com quem o mandou fazer.
 *
 *  Em cima de um cano o sidecar durava o que durasse a Relay. Ela reiniciava —
 *  um force quit, o instalador — e o turno em curso ia com ela. Num socket a
 *  ligação é uma visita e não um cordão: quem se vai embora deixa o trabalho a
 *  andar, e quem chega diz por onde ia e recebe o que perdeu.
 *
 *  Duas metades, testadas às duas: a contabilidade do que fica dito enquanto
 *  ninguém ouve, e o processo a sobreviver mesmo à ida do cliente. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

/** Em Windows não há socket de domínio nesta pilha, e é por isso que o porto lá
 *  continua pelos canos. Estes dois levantam o sidecar a servir num caminho que
 *  lá não existe; o que eles guardam não se aplica a essa plataforma, e correr
 *  lá só dava vermelho por uma razão que não é um defeito. A contabilidade do
 *  `bus`, essa, é aritmética e corre em todo o lado. */
const noSockets = process.platform === "win32";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

function loadBus() {
  const start = source.indexOf("const bus = {");
  const end = source.indexOf("/** Modo cano");
  assert.ok(start > -1 && end > start, "o bus mudou de sítio; actualizar este teste");
  return new Function(`${source.slice(start, end)}; return bus;`)();
}

/** Um cliente de mentira: guarda o que lhe escrevem. */
function sink() {
  const got = [];
  return { got, write: (s) => got.push(JSON.parse(s)) };
}

test("o que se diz é numerado, e só os eventos", () => {
  const bus = loadBus();
  const client = sink();
  bus.attach(client, 0);

  bus.publish({ type: "event", event: { kind: "text", text: "a" } });
  bus.publish({ type: "event", event: { kind: "text", text: "b" } });
  assert.deepEqual(client.got.map((m) => m.seq), [1, 2]);

  // Um pedido não entra no histórico: o que lhe falta não é ser visto outra
  // vez, é ser respondido — e isso trata-se pelos pendentes.
  bus.publish({ type: "approval_request", request_id: "r1" });
  assert.equal(bus.history.length, 2);
  assert.equal(client.got.at(-1).type, "approval_request");
  assert.equal(client.got.at(-1).seq, undefined);
});

test("quem chega a meio recebe só o que lhe falta", () => {
  const bus = loadBus();
  bus.publish({ type: "event", event: { kind: "text", text: "um" } });
  bus.publish({ type: "event", event: { kind: "text", text: "dois" } });
  bus.publish({ type: "event", event: { kind: "text", text: "três" } });

  const late = sink();
  const at = bus.attach(late, 1);
  assert.equal(at, 3, "diz-lhe onde vai");
  assert.deepEqual(late.got.map((m) => m.event.text), ["dois", "três"], "sem buracos e sem repetições");
});

test("sem ninguém à escuta continua a contar", () => {
  const bus = loadBus();
  // É este o caso que interessa: a Relay morreu, o run continuou.
  bus.publish({ type: "event", event: { kind: "text", text: "no escuro" } });
  assert.equal(bus.seq, 1);

  const later = sink();
  bus.attach(later, 0);
  assert.deepEqual(later.got.map((m) => m.event.text), ["no escuro"]);
});

test("sair não desliga quem entrou depois", () => {
  const bus = loadBus();
  const velho = sink();
  const novo = sink();
  bus.attach(velho, 0);
  bus.attach(novo, 0);
  // O `close` do socket antigo chega depois do novo já estar ligado, e um
  // `detach` cego deixava a Relay nova a falar para o vazio.
  bus.detach(velho);
  bus.publish({ type: "event", event: { kind: "text", text: "para o novo" } });
  assert.equal(novo.got.at(-1).event.text, "para o novo");
});

/** E agora o processo a sério. */
function connect(sock) {
  return new Promise((resolve, reject) => {
    const s = net.createConnection(sock);
    s.on("connect", () => resolve(s));
    s.on("error", reject);
  });
}

function firstLine(socket) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({ input: socket, terminal: false });
    rl.once("line", (l) => {
      rl.close();
      resolve(JSON.parse(l));
    });
  });
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

test("o sidecar sobrevive ao cliente e volta a atender", { skip: noSockets }, async (t) => {
  const sock = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "relay-sock-")), "run.sock");
  const child = spawn(
    process.execPath,
    [path.join(here, "index.mjs"), "--serve", sock, "--key", "chat-testes"],
    { stdio: ["ignore", "ignore", "pipe"], detached: true },
  );
  t.after(() => {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      child.kill("SIGKILL");
    }
  });

  for (let i = 0; i < 60 && !fs.existsSync(sock); i++) await wait(50);
  assert.ok(fs.existsSync(sock), "o socket devia estar de pé");

  const first = await connect(sock);
  const hello = firstLine(first);
  first.write(JSON.stringify({ type: "attach", from_seq: 0 }) + "\n");
  const greeting = await hello;
  assert.equal(greeting.type, "attached");
  assert.equal(greeting.seq, 0);
  assert.equal(greeting.running, false, "ainda ninguém lhe mandou trabalho");
  // A identidade, e não só o sítio: é o que deixa o outro lado recusar um
  // socket que esteja a servir o run de outro agente.
  assert.equal(greeting.run_key, "chat-testes");
  // **Qual** run, e não só que há um. Quem se liga a um turno vivo tem de o
  // poder tratar pelo nome: sem isto cunhava um id novo e mandava-lhe as
  // mensagens do operador endereçadas a um run que deste lado não existe — a
  // conversa aceitava tudo e não entregava nada. Sem trabalho a andar é nulo,
  // que é a resposta certa a "qual" quando não há nenhum.
  assert.ok("run_id" in greeting, "o cumprimento tem de dizer qual é o run");
  assert.equal(greeting.run_id, null, "e sem trabalho a andar não há nenhum");

  // A Relay vai-se embora — é isto que antes matava o turno.
  first.destroy();
  await wait(400);
  assert.equal(child.exitCode, null, "o run não pode morrer com o cliente");

  // E a Relay seguinte volta a encontrá-lo.
  const second = await connect(sock);
  const again = firstLine(second);
  second.write(JSON.stringify({ type: "attach", from_seq: 0 }) + "\n");
  assert.equal((await again).type, "attached", "tem de voltar a atender");
  second.destroy();
});

test("o socket é apagado à saída, para ninguém bater a uma porta que não abre", { skip: noSockets }, async (t) => {
  const sock = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "relay-sock-")), "run.sock");
  const child = spawn(process.execPath, [path.join(here, "index.mjs"), "--serve", sock], {
    stdio: ["ignore", "ignore", "ignore"],
    detached: true,
  });
  const exited = new Promise((r) => child.on("exit", () => r(true)));
  t.after(() => {
    try {
      process.kill(child.pid, "SIGKILL");
    } catch {
      /* já morreu */
    }
  });
  for (let i = 0; i < 100 && !fs.existsSync(sock); i++) await wait(50);
  assert.ok(fs.existsSync(sock));

  process.kill(child.pid, "SIGTERM");

  // Esperar pela saída antes de olhar para o ficheiro. Escrito ao contrário, o
  // teste media a lentidão da máquina em vez do que quer guardar: num runner
  // carregado o processo ainda não tinha saído quando se lhe perguntava pelo
  // socket, e falhava por uma razão que não é a dele.
  const left = await Promise.race([exited, wait(15000).then(() => false)]);
  assert.ok(left, "o sidecar não saiu com o SIGTERM");

  for (let i = 0; i < 100 && fs.existsSync(sock); i++) await wait(50);
  // A rede de segurança verdadeira é do outro lado — o porto tenta ligar-se e,
  // não atendendo ninguém, apaga o ficheiro antes de levantar um run novo. Isto
  // é a boa educação de quem sai: sem ela, cada Relay começava por bater a uma
  // porta que já não abre.
  assert.ok(!fs.existsSync(sock), "saiu mas deixou o socket para trás");
});
