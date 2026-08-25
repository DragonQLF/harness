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
       enum: () => ({ describe: () => chain() }),
       record: () => chain(),
       boolean: () => chain(),
       number: () => chain(),
     };
     function chain() { const api = { describe: () => api, optional: () => api, default: () => api, int: () => api, min: () => api, max: () => api }; return api; }
     ${code}
     return { callFor, harnessTools, reportWorkTool };`,
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

  const request = builders ? undefined : undefined; // placeholder removed below
  assert.ok(true);
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

test("board tools route with their own run id", async () => {
  const { builders, recorded } = loadBuilders();
  const board = builders.harnessTools("run-director");
  const move = board.tools.find((t) => t.name === "move_card");
  await move.cb({ card_id: "c_1", to: "ready" });
  const request = recorded.find((m) => m.type === "tool_request");
  assert.equal(request.run_id, "run-director");
  assert.equal(request.name, "move_card");
});
