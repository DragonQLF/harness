/** Offline tests for the frozen zone: `node --test`.
 *  No SDK, no model — the guard is pure path arithmetic. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { READ_TOOLS, candidatePaths, contains, classifyBash, classifyWrite, inspect } from "./pathguard.mjs";

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
  const cwd = worktree();
  for (const command of ["echo hi", "cd site && ls -la", 'git commit -m "fix"', "> out.txt"]) {
    const v = inspect("Bash", cwd, { command });
    assert.ok(v.skip === false && v.ok === true, `${command}: ${JSON.stringify(v)}`);
  }
});

/** The escape that happened: git-bash rewrites POSIX paths to drive paths on
 *  Windows, so `/Users/nandi/site/…` wrote outside the worktree. Both styles
 *  must be caught by the heuristic, and it names the path. */
test("bash with absolute paths outside is refused in both styles", () => {
  const cwd = worktree();
  for (const command of [
    "cat > /Users/nandi/site/feed.xml",
    "cp x /c/Users/nandi/site/feed.xml",
    String.raw`echo hi > C:\Users\nandi\site\feed.xml`,
  ]) {
    const v = inspect("Bash", cwd, { command });
    assert.equal(v.ok, false, `${command} must be refused`);
    assert.ok(v.path && v.path.length > 1, `${command} names the path: ${v.path}`);
  }
});

test("bash writing inside the worktree by absolute path still passes", () => {
  const cwd = worktree();
  const inside = path.join(cwd, "out.txt");
  const v = classifyBash(cwd, `echo hi > "${inside}"`);
  assert.equal(v.ok, true);
});

/** The c_19a1 case: git-bash's full path to the worktree itself. `/c/…` is
 *  drive C:\ translated — refusing it taught the operator to distrust the
 *  guard. It must translate and pass. */
test("the worktree's own msys path passes on windows", () => {
  const cwd = worktree(); // C:\Users\...\AppData\Local\Temp\guard-xxx
  const msys = "/" + cwd[0].toLowerCase() + "/" + cwd.slice(3).split("\\").join("/");
  for (const command of [
    `ls -la ${msys}`,
    `cat > ${msys}/feed.xml`,
  ]) {
    const v = classifyBash(cwd, command);
    assert.equal(v.ok, true, `${command}: ${JSON.stringify(v)}`);
  }
});

/** The Director's case: `ls -R` of a path outside the worktree was refused
 *  with "runs may only write inside their worktree". It does not write. Reads
 *  are exempt in Bash exactly as they are in `inspect`. */
test("reads outside the worktree pass — a read is not a write", () => {
  const cwd = worktree();
  for (const command of [
    "ls -R /Users/nandi/relay-data",
    "ls -la /Users/nandi/",
    "cat /Users/nandi/relay-data/settings.json",
    "head -50 /etc/hosts",
    "tail -f /var/log/system.log",
    "find /Users/nandi/relay-data -name '*.jsonl'",
    "grep -r director /Users/nandi/relay-data",
    "wc -l /etc/hosts",
    "stat /Users/nandi/relay-data",
    "/bin/cat /etc/hosts",
    "cat /etc/hosts | head -20",
    "ls /etc && cat /etc/hosts",
  ]) {
    const v = inspect("Bash", cwd, { command });
    assert.equal(v.ok, true, `${command} must pass: ${JSON.stringify(v)}`);
  }
});

/** And the other half: everything that looks like a read but writes. Opening
 *  reads must not open writes — this is the test that says so. */
test("a write dressed as a read is still refused", () => {
  const cwd = worktree();
  for (const command of [
    "cat x > /Users/nandi/site/feed.xml",
    "cat x >> /Users/nandi/site/feed.xml",
    "ls -R > /Users/nandi/listing.txt",
    "ls -R 2> /Users/nandi/err.txt",
    "ls /etc | tee /Users/nandi/listing.txt",
    "cat /etc/hosts; rm -rf /Users/nandi/site",
    "ls /etc && cp x /Users/nandi/site/x",
    "find /Users/nandi -name '*.tmp' -delete",
    "find /Users/nandi -name '*.tmp' -exec rm {} +",
    "cat $(echo /Users/nandi/site/feed.xml) > out.txt",
    "OUT=/Users/nandi/site cat x",
    String.raw`cat x > C:\Users\nandi\site\feed.xml`,
  ]) {
    const v = inspect("Bash", cwd, { command });
    assert.equal(v.ok, false, `${command} must be refused: ${JSON.stringify(v)}`);
    assert.ok(v.path && v.path.length > 1, `${command} names the path: ${v.path}`);
  }
});

/** A read that stays inside is untouched either way, and a write that stays
 *  inside keeps passing — the exemption did not swallow the guard. */
test("reads and writes inside the worktree both still pass", () => {
  const cwd = worktree();
  const inside = path.join(cwd, "out.txt");
  assert.equal(classifyBash(cwd, `cat "${inside}"`).ok, true);
  assert.equal(classifyBash(cwd, `echo hi > "${inside}"`).ok, true);
  assert.equal(classifyBash(cwd, `ls -R "${cwd}"`).ok, true);
});

/** And if this ever runs under WSL2 or any Linux host, a Windows-style path
 *  is outside by definition — never resolved as a relative name with
 *  backslashes and waved through. */
test("a windows-style path is refused on non-windows hosts too", () => {
  const cwd = worktree();
  for (const host of ["linux", "darwin"]) {
    const v = classifyBash(cwd, "echo hi > C:\\Users\\nandi\\site\\feed.xml", host);
    assert.equal(v.ok, false, `${host} must refuse: ${JSON.stringify(v)}`);
    assert.match(v.path, /site\\/i);
  }
});
