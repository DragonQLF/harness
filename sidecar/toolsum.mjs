/** Per-tool summaries for the transcript: what the agent tried (from the
 *  call's input) and what actually happened (from the tool_result block).
 *  Pure functions, no SDK — testable offline. Never dumps content (#28). */

const basename = (p) => String(p ?? "").split(/[\\/]/).filter(Boolean).pop() ?? "";

/** The line that names the attempt, per tool. */
export function summarizeUse(name, input = {}) {
  const str = (k) => (typeof input[k] === "string" ? input[k].trim() : "");
  switch (name) {
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "Read":
      return basename(str("file_path")) || name;
    case "Bash":
      return str("command").slice(0, 160) || "shell";
    case "Task":
      return `${str("subagent_type") || "subagent"}: ${str("description").slice(0, 80)}`;
    case "Grep":
      return `grep ${str("pattern").slice(0, 80)}${input.path ? ` in ${basename(input.path)}` : ""}`;
    case "Glob":
      return `glob ${str("pattern").slice(0, 80)}`;
    case "WebFetch":
    case "WebSearch":
      return str("url") || str("query").slice(0, 120) || name;
    default: {
      // Unknown tools: first string value wins, else the name alone.
      for (const v of Object.values(input)) {
        if (typeof v === "string" && v.trim()) return `${name}: ${v.trim().slice(0, 80)}`;
      }
      return name;
    }
  }
}

const excerpt = (text, maxLines = 3, maxWidth = 200) => {
  const lines = String(text ?? "")
    .replace(/\r/g, "")
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => l.trim().slice(0, maxWidth));
  const tail = lines.slice(-maxLines);
  const hidden = lines.length - tail.length;
  return `${hidden > 0 ? `…${hidden} lines… ` : ""}${tail.join(" ⏎ ")}`;
};

/** The line that says what happened. `content` is the raw text of the
 *  result; counts where counting makes sense, output tail where it does not. */
export function summarizeResult(name, content, isError) {
  const text = String(content ?? "");
  const lines = text.split("\n").filter((l) => l.trim());
  if (isError) {
    return `failed — ${excerpt(text, 2, 160) || "no output"}`;
  }
  switch (name) {
    case "Bash":
      return `ok · ${lines.length} line${lines.length === 1 ? "" : "s"} out · ${excerpt(text, 2)}`;
    case "Read":
      return `read · ${lines.length} line${lines.length === 1 ? "" : "s"}`;
    case "Grep":
    case "Glob": {
      const hits = (text.match(/^[\s\S]*?$/m) && lines.length) || 0;
      return `${hits} match${hits === 1 ? "" : "es"}`;
    }
    case "Edit":
    case "Write":
    case "MultiEdit":
      return `written · ${excerpt(text, 2)}`;
    default:
      return lines.length ? `ok · ${excerpt(text, 2)}` : "ok";
  }
}

/** Full body for the expandable detail, hard-capped so one runaway command
 *  cannot bloat the transcript file. */
export function detailOf(content, cap = 8000) {
  const text = String(content ?? "");
  return text.length > cap ? text.slice(0, cap) + "\n…[truncated]" : text;
}
