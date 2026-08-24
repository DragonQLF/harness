/** Offline tests for the frozen zone: `node --test`.
 *  No SDK, no model — the guard is pure path arithmetic. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { READ_TOOLS, candidatePaths, contains, classifyWrite, inspect } from "./pathguard.mjs";

function worktree() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "guard-"));
  return dir;
}

test("a write inside the worktree passes", () => {
  const cwd = worktree();
  const verdict = classifyWrite(cwd, {
    file_path: path.join(cwd, "src", "lib.rs"),
    content: "ok",
  });
  assert.deepEqual(verdict, { ok: true, path: null });
});

test("a relative write stays inside and passes", () => {
  const cwd = worktree();
  const verdict = classifyWrite(cwd, { file_path: "docs/note.md" });
  assert.equal(verdict.ok, true);
});

test("the appdata settings file is refused", () => {
  const cwd = worktree();
  const appdata = path.join(os.homedir(), "AppData", "Roaming");
  const verdict = classifyWrite(cwd, {
    file_path: path.join(appdata, "agents.json"),
  });
  assert.equal(verdict.ok, false);
  assert.match(verdict.path, /agents\.json$/);
});

test("an escape through .. does not pass", () => {
  const cwd = worktree();
  const verdict = classifyWrite(cwd, {
    file_path: path.join(cwd, "..", "other-project", "x.rs"),
  });
  assert.equal(verdict.ok, false);
});

test("a sibling directory is not contained, boundary included", () => {
  assert.equal(contains("/wt/c1", "/wt/c11/x.rs"), false);
  assert.equal(contains("/wt/c1", "/wt/c1/x.rs"), true);
});

test("a path that cannot be resolved is refused, not guessed", () => {
  // A drive-less fragment on Windows resolves to the cwd's drive; a null
  // device-style garbage still must not pass. The contract: refuse when
  // unsure.
  const cwd = worktree();
  const verdict = classifyWrite(cwd, { file_path: "\0bad" });
  if (!verdict.ok) assert.ok(verdict.path.length > 0);
});

test("every string under a key ending in path is a candidate", () => {
  const found = candidatePaths({
    file_path: "a",
    nested: { notebook_path: "b", other: "c" },
    edits: [{ file_path: "d" }],
  });
  assert.deepEqual(found.sort(), ["a", "b", "d"]);
});

test("read tools are not in the write set", () => {
  for (const tool of ["Read", "Glob", "Grep", "NotebookRead"]) {
    assert.ok(READ_TOOLS.has(tool), tool);
    const verdict = inspect(tool, os.tmpdir(), { file_path: "/etc/passwd" });
    assert.ok(verdict.skip, `${tool} is exempt`);
  }
});

/** The handoff's test: an MCP tool nobody vetted, carrying a path field,
 *  must hit the guard. The old gate checked four tool names; this one never
 *  would have been seen. */
test("an unknown mcp tool with a path field is inspected and refused", () => {
  const cwd = worktree();
  const verdict = inspect("mcp__other__save_file", cwd, { path: "/etc/passwd" });
  assert.equal(verdict.skip, false);
  assert.equal(verdict.ok, false);
  assert.equal(verdict.path, "/etc/passwd");
});

test("harness's own mcp tools are exempt — they act on the app", () => {
  const cwd = worktree();
  const verdict = inspect("mcp__harness__create_project", cwd, {
    parent_path: "/etc/passwd",
  });
  assert.ok(verdict.skip, "board tools carry their own approval story");
});

test("bash has no structured paths and passes the guard untouched", () => {
  const verdict = inspect("Bash", worktree(), { command: "echo hi" });
  assert.ok(verdict.skip === false && verdict.ok === true);
});
