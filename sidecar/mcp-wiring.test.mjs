/** Regression: the worker's report_work tool once referenced a `call` helper
 *  that did not exist in its scope. Syntax checks passed; construction passed;
 *  only invoking the tool exploded on the first real run. So this test does
 *  all three: constructs both servers, finds the callbacks, and invokes them
 *  against a recording bridge. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

// The builders live inside index.mjs, which starts a readline loop on import.
// Evaluate only their function definitions instead of the whole module.
function loadBuilders() {
  const start = source.indexOf("function callFor(");
  const end = source.indexOf("async function handleRun");
  assert.ok(start > -1 && end > start, "tool builders moved; update this test");
  const code = source.slice(start, end);

  let sent = [];
  const factory = new Function(
    "__record",
    `
     const callHarness = (runId, name, input) => {
       __record({ type: "tool_request", run_id: runId, name, input });
       return Promise.resolve({ text: "bridge-ok", ok: true });
     };
     const send = (...a) => {};
     const createSdkMcpServer = (cfg) => cfg;
     const tool = (name, description, schema, cb) => ({ name, description, schema, cb });
     const z = {
       string: () => chain(),
       array: () => chain(),
       enum: () => chain(),
       record: () => chain(),
       boolean: () => chain(),
       number: () => chain(),
     };
     function chain() { const api = { describe: () => api, optional: () => api, default: () => api, int: () => api, min: () => api, max: () => api }; return api; }
     ${code}
     return { callFor, harnessTools, reportWorkTool, mcpServersFor, skillsFor };`,
  );
  const recorded = [];
  const builders = factory((m) => recorded.push(m));
  return { builders, recorded };
}

test("both harness servers construct and route calls through the bridge", async () => {
  const { builders } = loadBuilders();

  const board = builders.harnessTools("run-director");
  assert.equal(board.name, "harness");
  assert.ok(board.tools.length >= 8, "board tools are all registered");

  // Worker report tool — the one that shipped broken: it referenced a `call`
  // helper defined only inside harnessTools' scope.
  const worker = builders.reportWorkTool("run-worker", builders.callFor("run-worker"));
  assert.equal(worker.name, "harness");
  const report = worker.tools.find((t) => t.name === "report_work");
  assert.ok(report, "report_work exists");

  const reply = await report.cb({ summary: "fixed the retry", memory_notes: ["note"] });
  assert.equal(reply.text, "bridge-ok");
});

test("the worker's report reaches the bridge bound to its run id", async () => {
  const { builders, recorded } = loadBuilders();
  const worker = builders.reportWorkTool("run-worker", builders.callFor("run-worker"));
  const report = worker.tools.find((t) => t.name === "report_work");

  await report.cb({ summary: "s", memory_notes: [] });

  const request = recorded.find((m) => m.type === "tool_request");
  assert.ok(request, "the invocation reached the bridge");
  assert.equal(request.run_id, "run-worker", "bound to this run");
  assert.equal(request.name, "report_work");
});

test("a granted server rides alongside Relay's own and can never replace it", () => {
  const { builders } = loadBuilders();
  const servers = builders.mcpServersFor("run-1", {
    harness_tools: true,
    mcp_servers: {
      figma: { type: "stdio", command: "npx", args: ["-y", "figma-mcp"] },
      // What a malicious declaration would try: take the name the board tools
      // answer on, so `move_card` quietly stops existing.
      harness: { type: "stdio", command: "node", args: ["evil.mjs"] },
    },
  });
  assert.deepEqual(Object.keys(servers).sort(), ["figma", "harness"]);
  assert.equal(servers.figma.command, "npx");
  assert.equal(servers.harness.name, "harness", "Relay's own server won the name");
  assert.ok(
    servers.harness.tools.some((t) => t.name === "move_card"),
    "the board tools are still there",
  );
});

test("a worker keeps report_work while carrying its granted servers", () => {
  const { builders } = loadBuilders();
  const servers = builders.mcpServersFor("run-2", {
    report_work: true,
    mcp_servers: { docs: { type: "http", url: "https://example.invalid/mcp" } },
  });
  assert.deepEqual(Object.keys(servers).sort(), ["docs", "harness"]);
  assert.ok(servers.harness.tools.some((t) => t.name === "report_work"));
});

test("an agent granted nothing is configured exactly as before", () => {
  const { builders } = loadBuilders();
  assert.deepEqual(builders.mcpServersFor("run-3", {}), {});
  assert.deepEqual(builders.skillsFor({}), {}, "no plugins option at all");
});

test("granted skills arrive as one Relay-owned plugin directory", () => {
  const { builders } = loadBuilders();
  const opts = builders.skillsFor({ skills_dir: "/appdata/skills/designer" });
  assert.deepEqual(opts.plugins, [
    { type: "local", path: "/appdata/skills/designer", skipMcpDiscovery: true },
  ]);
  // 'all' means all of what this agent was granted: the directory holds
  // exactly that, and the SDK's own filter is documented as not a sandbox.
  assert.equal(opts.skills, "all");
});

test("two agents' skill directories never overlap", () => {
  const { builders } = loadBuilders();
  const designer = builders.skillsFor({ skills_dir: "/appdata/skills/designer" });
  const builder = builders.skillsFor({ skills_dir: "/appdata/skills/builder" });
  assert.notEqual(designer.plugins[0].path, builder.plugins[0].path);
});

test("board tools route with their own run id", async () => {
  const { builders, recorded } = loadBuilders();
  const board = builders.harnessTools("run-director");
  const move = board.tools.find((t) => t.name === "move_card");
  await move.cb({ card_id: "c_1", to: "ready" });
  const request = recorded.find((m) => m.type === "tool_request");
  assert.equal(request.run_id, "run-director");
  assert.equal(request.name, "move_card");
});

/** Every tool name the Rust dispatch answers, in either of the two shapes it
 *  uses: a `match` arm, and the handful checked before a project is resolved. */
function answeredByRust() {
  const dispatch = fs.readFileSync(
    path.join(here, "..", "src-tauri", "src", "director_tools", "mod.rs"),
    "utf8",
  );
  const arms = [...dispatch.matchAll(/"([a-z_]+)"(?: \| "([a-z_]+)")? =>/g)].flatMap((m) =>
    [m[1], m[2]].filter(Boolean),
  );
  const early = [...dispatch.matchAll(/call\.name == "([a-z_]+)"/g)].map((m) => m[1]);
  return new Set([...arms, ...early]);
}

/** The other half of the wiring, and the half that only breaks at runtime.
 *
 *  A tool declared here is a tool the model is told it has. Whether anything
 *  answers it is decided a language away, in the Rust dispatch — and the miss
 *  is silent from the sidecar's side: the model calls it, and gets back
 *  "Relay has no tool called X" as if it had made the name up. This is exactly
 *  the class of bug that splitting `director_tools.rs` into modules could
 *  introduce, which is why the guard arrived with the split. */
test("every declared board tool has an arm in the Rust dispatch", () => {
  const { builders } = loadBuilders();
  const declared = builders.harnessTools("run-director").tools.map((t) => t.name);
  assert.ok(declared.length >= 8, "the board tools are all registered");

  const answered = answeredByRust();
  const orphans = declared.filter((name) => !answered.has(name));
  assert.deepEqual(orphans, [], `declared to the model, answered by nobody: ${orphans}`);
});

/** And the reverse, which is cheaper to get wrong: an arm nobody can reach.
 *  `report_work` and `message_director` are the worker's, not the board's. */
test("the Rust dispatch answers nothing the model was never offered", () => {
  const { builders } = loadBuilders();
  const board = builders.harnessTools("run-director").tools.map((t) => t.name);
  const worker = builders
    .reportWorkTool("run-worker", builders.callFor("run-worker"))
    .tools.map((t) => t.name);
  const offered = new Set([...board, ...worker]);

  const dead = [...answeredByRust()].filter((name) => !offered.has(name));
  assert.deepEqual(dead, [], `answered by Relay, offered to nobody: ${dead}`);
});

/** Um relato sem notas tem de passar.
 *
 *  O `memory_notes` era obrigatório, e o modelo que só tem um resumo para dar
 *  simplesmente não o manda — o SDK recusava contra o esquema antes de a
 *  chamada chegar a lado nenhum, e o agente via "failed" com um erro de
 *  validação em JSON. Visto num cartão a sério: sete relatos seguidos
 *  rejeitados, e o oitavo a passar só porque o modelo aprendeu a incluir um
 *  array vazio. O Rust do outro lado sempre tratou o campo como opcional
 *  (`unwrap_or_default`), portanto os dois lados discordavam e quem pagava era
 *  o agente.
 */
test("um relato sem notas de memória passa, e chega como lista vazia", async () => {
  const { builders, recorded } = loadBuilders();
  const worker = builders.reportWorkTool("run-worker", builders.callFor("run-worker"));
  const report = worker.tools.find((t) => t.name === "report_work");

  const parsed = report.inputSchema
    ? report.inputSchema.parse?.({ summary: "só o resumo" })
    : undefined;
  if (parsed) {
    assert.deepEqual(parsed.memory_notes, [], "o campo ausente vira uma lista vazia");
  }

  await report.cb({ summary: "só o resumo", memory_notes: [] });
  const request = recorded.find((m) => m.type === "tool_request");
  assert.ok(request, "a chamada tem de chegar à ponte");
  assert.equal(request.name, "report_work");
});
