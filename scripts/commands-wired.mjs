#!/usr/bin/env node
/**
 * Guarda: um comando registado tem de ter porta, e uma porta tem de dar para
 * algum lado.
 *
 * Um `#[tauri::command]` só existe para ser chamado da janela. Registá-lo e não
 * lhe dar embrulho no `ipc.ts` deixa-o inalcançável; dar-lhe embrulho e nunca o
 * chamar deixa a funcionalidade escrita, testada e sem botão. Nem o `tsc` nem o
 * `cargo` dizem nada sobre nenhuma das duas coisas — são dois lados de uma
 * fronteira que nenhum compilador atravessa, e o que as liga é uma string.
 *
 * Não é hipótese. O `curator` esteve assim: escrito, testado, registado, e
 * chamado por ninguém — nenhuma máquina chegou a ter a pasta `memory/areas/`
 * que ele produzia, enquanto cada cartão pagava para redescobrir o que o
 * anterior já sabia. O `DEBT.md` descrevia-o com precisão meses antes de
 * custar alguma coisa. Uma linha num documento não falha; isto falha.
 *
 * Três regras:
 *
 *   1. Todo o comando registado no `generate_handler!` tem embrulho no `ipc.ts`.
 *   2. Todo o embrulho nomeia um comando que existe — um nome errado só se
 *      descobre em produção, quando o Tauri responde que não conhece aquilo.
 *   3. Todo o embrulho é chamado em `src/`. As excepções vivem no `SEM_PORTA`
 *      abaixo, com a razão à vista: o que se quer impedir não é a lista, é ela
 *      crescer sem ninguém decidir.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, extname } from "node:path";

const root = fileURLToPath(new URL("..", import.meta.url));
const read = (p) => readFileSync(join(root, p), "utf8");

/**
 * Comandos registados que a janela não chama, e porquê.
 *
 * Registar e não ligar não é neutro: o comando compila, os testes passam, e a
 * ausência só se nota quando alguém precisa dele.
 */
const SEM_JANELA = new Map([
  [
    "prepare_shutdown",
    "duplica o fecho vivo: o `closing.rs` já corre o `maybe_run_daily_look` e o " +
      "`ws.shutdown()` nos mesmos dois passos. Nada o invoca — nem a janela, nem o Rust — " +
      "portanto é uma segunda cópia de lógica viva, que é a pior espécie de código morto: " +
      "quem editar uma não edita a outra",
  ],
]);

/**
 * Embrulhos que existem sem chamador, e porquê.
 *
 * Não se apagam: o embrulho é a porta, e apagá-la esconde a ausência em vez de
 * a fechar (`DEBT.md`). Fica aqui para que a ausência tenha um sítio onde se
 * lê, e para que a próxima seja uma decisão e não um esquecimento.
 */
const SEM_PORTA = new Map([
]);

/** Os comandos que o Rust regista, do `generate_handler!`. */
function registados() {
  const lib = read("src-tauri/src/lib.rs");
  const abre = lib.indexOf("generate_handler![");
  if (abre < 0) throw new Error("não encontrei o `generate_handler!` no lib.rs");
  // Fecha no `]` da macro. As entradas são caminhos `commands::x::y`, uma por
  // linha, com comentários pelo meio.
  const fecha = lib.indexOf("])", abre);
  const bloco = lib.slice(abre, fecha);
  const nomes = new Set();
  for (const linha of bloco.split("\n").slice(1)) {
    const limpa = linha.split("//")[0].trim().replace(/,$/, "");
    if (!limpa) continue;
    const ultimo = limpa.split("::").pop();
    if (/^[a-z_][a-z0-9_]*$/.test(ultimo)) nomes.add(ultimo);
  }
  return nomes;
}

/** Os embrulhos do `ipc.ts`: nome no `api` → comando que invocam. */
function embrulhos() {
  const ipc = read("src/lib/ipc.ts");
  const abre = ipc.indexOf("export const api = {");
  const fecha = ipc.indexOf("\n};", abre);
  const bloco = ipc.slice(abre, fecha);
  const mapa = new Map();
  // `nome: (…) => invoke<T>("comando"…` — o embrulho pode ocupar várias linhas,
  // por isso procura-se o par e não a linha.
  const re = /(\w+)\s*:\s*(?:\([^)]*\)|async\s*\([^)]*\))?\s*=>[\s\S]{0,400}?invoke<[^>]*>\(\s*"([a-z_][a-z0-9_]*)"/g;
  let m;
  while ((m = re.exec(bloco)) !== null) mapa.set(m[1], m[2]);
  return mapa;
}

/** Todo o `src/`, menos o próprio `ipc.ts`. */
function fontes() {
  const out = [];
  const anda = (dir) => {
    for (const entrada of readdirSync(join(root, dir))) {
      const rel = `${dir}/${entrada}`;
      if (statSync(join(root, rel)).isDirectory()) {
        anda(rel);
      } else if ([".ts", ".tsx"].includes(extname(entrada)) && rel !== "src/lib/ipc.ts") {
        out.push(read(rel));
      }
    }
  };
  anda("src");
  return out.join("\n");
}

const registo = registados();
const portas = embrulhos();
const codigo = fontes();
const erros = [];

// 1. Um comando sem porta é inalcançável a partir da janela.
const invocados = new Set(portas.values());
for (const comando of registo) {
  if (!invocados.has(comando)) {
    if (SEM_JANELA.has(comando)) continue;
    erros.push(
      `o comando \`${comando}\` está registado e não tem embrulho no ipc.ts — ` +
        `a janela não tem por onde lhe chamar`,
    );
  }
}

// 2. Uma porta para um comando que não existe só falha em produção.
for (const [nome, comando] of portas) {
  if (!registo.has(comando)) {
    erros.push(
      `\`api.${nome}\` invoca "${comando}", que não está registado no lib.rs — ` +
        `isto só se descobre a correr`,
    );
  }
}

// 3. Uma porta que ninguém abre é funcionalidade sem botão.
const orfas = [];
for (const nome of portas.keys()) {
  if (new RegExp(`\\b${nome}\\s*\\(`).test(codigo)) continue;
  if (SEM_PORTA.has(nome)) continue;
  orfas.push(nome);
}
for (const nome of orfas) {
  erros.push(
    `\`api.${nome}\` não é chamado em lado nenhum — ou se liga a um ecrã, ` +
      `ou se acrescenta ao SEM_PORTA neste ficheiro com a razão`,
  );
}

// E o contrário: uma excepção que deixou de o ser fica a mentir.
for (const [comando, razao] of SEM_JANELA) {
  if (!registo.has(comando)) {
    erros.push(`o SEM_JANELA nomeia \`${comando}\`, que já não está registado — apagar a entrada`);
  } else if (invocados.has(comando)) {
    erros.push(
      `\`${comando}\` já tem embrulho, portanto deixou de estar sem janela — ` +
        `tirar do SEM_JANELA (a razão que lá está: ${razao})`,
    );
  }
}

for (const [nome, razao] of SEM_PORTA) {
  if (!portas.has(nome)) {
    erros.push(`o SEM_PORTA nomeia \`${nome}\`, que já não existe no ipc.ts — apagar a entrada`);
  } else if (new RegExp(`\\b${nome}\\s*\\(`).test(codigo)) {
    erros.push(
      `\`api.${nome}\` já é chamado, portanto deixou de estar sem porta — ` +
        `tirar do SEM_PORTA (a razão que lá está: ${razao})`,
    );
  }
}

if (erros.length > 0) {
  console.error("Comandos mal ligados:\n");
  for (const e of erros) console.error(`  - ${e}`);
  console.error("");
  process.exit(1);
}

console.log(
  `OK — ${registo.size} comandos registados (${SEM_JANELA.size} sem embrulho, com razão); ` +
    `${portas.size} embrulhos, ${portas.size - SEM_PORTA.size} chamados e ` +
    `${SEM_PORTA.size} sem ecrã, todos com razão escrita.`,
);
