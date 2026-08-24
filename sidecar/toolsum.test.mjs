import test from "node:test";
import assert from "node:assert/strict";
import { summarizeUse, summarizeResult, detailOf } from "./toolsum.mjs";

test("use summaries name the attempt per tool", () => {
  assert.equal(
    summarizeUse("Edit", { file_path: "C:/wt/src/lib.rs" }),
    "lib.rs",
  );
  assert.equal(
    summarizeUse("Bash", { command: "cargo test -p harness-engine" }),
    "cargo test -p harness-engine",
  );
  assert.equal(
    summarizeUse("Task", { subagent_type: "scout", description: "find the race" }),
    "scout: find the race",
  );
});

test("results distinguish failure from success and keep an excerpt", () => {
  const failed = summarizeResult("Bash", "error: link failed\nat main.rs", true);
  assert.match(failed, /^failed/);
  assert.match(failed, /link failed/);

  const ok = summarizeResult("Bash", "line1\nline2\nline3", false);
  assert.match(ok, /ok · 3 lines out/);

  const read = summarizeResult("Read", "a\nb\nc\nd\n e ", false);
  assert.equal(read, "read · 5 lines");
});

test("detail is capped so one runaway cannot bloat the log", () => {
  const big = "x".repeat(20_000);
  const d = detailOf(big, 8000);
  assert.ok(d.length < 8100);
  assert.ok(d.endsWith("[truncated]"));
});
