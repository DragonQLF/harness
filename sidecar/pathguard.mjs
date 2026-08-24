/** The frozen zone, as a path comparison.
 *
 *  A compiled binary does not edit itself: a run writes inside its own
 *  worktree and nowhere else. This module decides that — pure functions, no
 *  SDK, so it can be tested without a model (see pathguard.test.mjs).
 *
 *  Canonicalization follows decision #39: resolve, then compare — never a
 *  component-wise `starts_with`, which `<expected>/../../other` walks straight
 *  through. A path that cannot be resolved is refused, not guessed.
 */

import fs from "node:fs";
import path from "node:path";

/** Tools whose input names files the agent is changing. Reads stay free:
 *  the frozen zone is about writes, and Read/Glob/Grep name paths too. */
export const WRITE_TOOLS = new Set([
  "Write",
  "Edit",
  "MultiEdit",
  "NotebookEdit",
]);

/** Pull every candidate file path out of a tool input. Known fields first,
 *  then any string under a key that ends in `path` — new tools should trip
 *  the guard by default rather than need a rule each. */
export function candidatePaths(input) {
  const found = [];
  const walk = (value, key) => {
    if (typeof value === "string") {
      if (key === "file_path" || key === "notebook_path" || /path$/i.test(key)) {
        if (value.trim()) found.push(value.trim());
      }
    } else if (Array.isArray(value)) {
      for (const item of value) walk(item, key);
    } else if (value && typeof value === "object") {
      for (const [k, v] of Object.entries(value)) walk(v, k);
    }
  };
  walk(input ?? {}, "");
  return found;
}

/** Resolve a possibly-relative path against the run's cwd, following symlinks
 *  on the deepest ancestor that exists. Returns null when it cannot be
 *  resolved — which the caller treats as a refusal, per #39. */
function canonical(cwd, raw) {
  try {
    const joined = path.isAbsolute(raw) ? raw : path.join(cwd, raw);
    let resolved = path.resolve(joined);
    // Follow symlinks/junctions on whatever part of the chain exists; a new
    // file's parent always does.
    let probe = resolved;
    const tail = [];
    while (!fs.existsSync(probe)) {
      const parent = path.dirname(probe);
      if (parent === probe) return null;
      tail.push(path.basename(probe));
      probe = parent;
    }
    const real = fs.realpathSync(probe);
    resolved = tail.length ? path.join(real, ...tail.reverse()) : real;
    return path.normalize(resolved);
  } catch {
    return null;
  }
}

/** Is `candidate` inside `root`? Boundary-aware: `/worktrees/c1` must not
 *  contain `/worktrees/c11`. Windows paths differ only by case. */
export function contains(root, candidate) {
  const norm = (p) => {
    let s = p.split(/[\\/]+/).filter(Boolean).join("/");
    return process.platform === "win32" ? s.toLowerCase() : s;
  };
  const r = norm(root);
  const c = norm(candidate);
  if (c === r) return true;
  return c.startsWith(`${r}/`);
}

/** The verdict for one tool call. `null` means nothing to check (the tool
 *  carries no paths); otherwise ok:false comes with the offending path,
 *  because the transcript names what was refused. */
export function classifyWrite(cwd, input) {
  const root = canonical(cwd, ".");
  if (!root) return { ok: false, path: cwd };
  for (const raw of candidatePaths(input)) {
    const resolved = canonical(cwd, raw);
    if (!resolved || !contains(root, resolved)) {
      return { ok: false, path: raw };
    }
  }
  return { ok: true, path: null };
}
