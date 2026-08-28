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

/** Tools that only ever read. Everything else with path-bearing input gets
 *  inspected — including tools we have never heard of, which is the point:
 *  omission guards, an explicit list exempts. */
export const READ_TOOLS = new Set([
  "Read",
  "Glob",
  "Grep",
  "NotebookRead",
  "LS",
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

/** Shell commands that only ever read. The same asymmetry as `READ_TOOLS`:
 *  the frozen zone is about writes, and an `ls -R` of somewhere else does not
 *  write. The list is explicit and short — omission guards. */
export const READ_COMMANDS = new Set([
  "ls",
  "cat",
  "head",
  "tail",
  "find",
  "grep",
  "wc",
  "stat",
]);

/** `find` is the one reader with writing built in. Any of these turns the
 *  segment back into a command that acts on what it walks. */
const FIND_WRITES = /(^|\s)-(exec|execdir|ok|okdir|delete|fprint|fprintf|fls)(\s|$)/;

/** Cut a command line into the pieces the shell would run separately.
 *  Separators never appear inside a path the scan below recognises (its
 *  character classes exclude `&|;<>()`), so a plain split is enough for a
 *  heuristic that is declared as one. */
function segments(cmd) {
  return cmd.split(/\|\||&&|;|\||&|\n/);
}

/** Is this segment a pure read? Conservative by construction: a redirection,
 *  a command substitution, a leading variable assignment or a `find` that
 *  executes all give the exemption back. */
function isReadSegment(segment) {
  const text = segment.trim();
  if (!text) return true;
  // `>`, `>>`, `2>`, `&>`, `>(…)` — any of them writes somewhere.
  if (text.includes(">")) return false;
  // `$(…)` and backticks hide a whole other command inside this one.
  if (text.includes("$(") || text.includes("`")) return false;
  const first = text.split(/\s+/)[0];
  // `FOO=/fora cat x` is not a read of `cat`'s arguments alone.
  if (/^[A-Za-z_][A-Za-z0-9_]*=/.test(first)) return false;
  const name = first.split(/[\\/]/).pop();
  if (!READ_COMMANDS.has(name)) return false;
  if (name === "find" && FIND_WRITES.test(text)) return false;
  return true;
}

/** Heuristic Bash scan — declared heuristic, not a border (#62).
 *
 *  Reads are exempt, exactly as `inspect` exempts `Read`/`Glob`/`Grep`: an
 *  `ls -R` outside the worktree writes nothing, and refusing it stopped the
 *  Director from looking at Relay's own data. Writes and redirection stay
 *  guarded — `cat x > /fora` is not a read, and neither is `ls | tee /fora`.
 *
 *  Windows-style absolutes (`C:\…`, `\\?\C:\…`): on a Windows host they are
 *  classified against the worktree; anywhere else they are outside *by
 *  definition* — `path.resolve` on Linux treats `C:\x` as a relative name
 *  with backslashes and would wave it through.
 *
 *  POSIX-style absolutes (`/Users/…`, `/c/…`): on a Windows host git-bash
 *  silently rewrites them to drive paths, so `/c/<worktree>/…` is *inside*
 *  and must be translated before judging, while `/usr`, `/etc` and friends
 *  live in the msys root — always outside. */
export function classifyBash(cwd, command, hostPlatform = process.platform) {
  const cmd = String(command ?? "");
  for (const segment of segments(cmd)) {
    if (isReadSegment(segment)) continue;
    const verdict = scanSegment(cwd, segment, hostPlatform === "win32");
    if (!verdict.ok) return verdict;
  }
  return { ok: true, path: null };
}

/** The path arithmetic of one segment, once it has lost the read exemption. */
function scanSegment(cwd, segment, winHost) {
  for (const m of segment.matchAll(/(?:\\\\\?\\)?[A-Za-z]:[\\/][^\s"'&|;<>()]*/g)) {
    if (!winHost) return { ok: false, path: m[0] };
    const v = classifyWrite(cwd, { file_path: m[0].replace(/[\\/]$/, "") });
    if (!v.ok) return { ok: false, path: m[0] };
  }
  for (const m of segment.matchAll(/(?:^|[\s=('"`])\/([A-Za-z0-9._-][^\s"'&|;<>()]*)/g)) {
    const posix = `/${m[1]}`;
    if (winHost) {
      // /c/<rest> is drive C:\ under git-bash. Translate, then judge like
      // any other path — a legitimate full worktree path must not be
      // refused, or the operator just turns the guard off.
      const drive = /^([A-Za-z])\/(.*)$/.exec(m[1]);
      if (drive) {
        const windowsForm = `${drive[1].toUpperCase()}:\\${drive[2].replace(/\//g, "\\")}`;
        const v = classifyWrite(cwd, { file_path: windowsForm });
        if (!v.ok) return { ok: false, path: posix };
        continue;
      }
      // /usr, /etc, /Users: msys root, never inside this worktree.
      return { ok: false, path: posix };
    }
    const v = classifyWrite(cwd, { file_path: posix });
    if (!v.ok) return { ok: false, path: posix };
  }
  return { ok: true, path: null };
}

/** The verdict for one tool call, whoever owns the tool.
 *
 *  - Reads never touch files destructively: skipped outright.
 *  - Harness's own MCP tools (`mcp__harness__*`) act on the app, not on
 *    repositories — they carry their own approval story (decisions #27–#29)
 *    and their arguments are board coordinates, not file paths.
 *  - Everything else — including third-party MCP tools nobody vetted — is
 *    inspected when its input carries candidate paths.
 */
export function inspect(toolName, cwd, input) {
  if (READ_TOOLS.has(toolName)) {
    return { skip: true, ok: true, path: null };
  }
  if (toolName.startsWith("mcp__harness__")) {
    return { skip: true, ok: true, path: null };
  }
  if (toolName === "Bash") {
    // Structured tools carry paths as fields; Bash carries them in text.
    // This is the declared heuristic — OS-level confinement stays open (#2).
    const verdict = classifyBash(cwd, input?.command);
    return { ...verdict, skip: false };
  }
  const verdict = classifyWrite(cwd, input);
  return { ...verdict, skip: false };
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
  const splitPattern = process.platform === "win32" ? /[\\/]+/ : /\/+/;
  const norm = (p) => {
    const joined = p.split(splitPattern).filter(Boolean).join("/");
    return process.platform === "win32" ? joined.toLowerCase() : joined;
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
  if (!root) {
    // The run's own root is unresolvable — naming it in the message would
    // read as if the worktree were the offending path, so say null and let
    // the caller explain.
    return { ok: false, path: null };
  }
  for (const raw of candidatePaths(input)) {
    const resolved = canonical(cwd, raw);
    if (!resolved || !contains(root, resolved)) {
      return { ok: false, path: raw };
    }
  }
  return { ok: true, path: null };
}
