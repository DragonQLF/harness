/** O redutor de eventos: o que o motor emite durante uma execução.
 *
 *  Uma parte é pura — `toLine` traduz um `RunUpdate` ao vivo ou uma linha de
 *  log guardada para a mesma forma de transcrição. A outra é o estado que
 *  esses eventos alimentam: as linhas por cartão e o que está a chegar agora.
 *
 *  Não decide nada de domínio: o backend continua a deter a verdade, isto só
 *  desenha o que ele mandou.
 */

import { useCallback, useState } from "react";
import type { RunLogLine, RunUpdate } from "../lib/types";

const MAX_LINES = 400;

export interface LogLine {
  ts: number;
  kind: RunUpdate["kind"];
  /** The word printed in the transcript's left gutter: a tool name, or what
   *  kind of line this is. */
  label: string;
  text: string;
  /** As classes de cor desta linha, já com o par `dark:`. */
  color: string;
  /** As classes de cor da palavra da goteira. */
  labelColor: string;
  /** Tool-call linkage: this call's id and its parent's, so results nest
   *  under the call that produced them and subagent calls indent further. */
  toolUseId?: string | null;
  parentToolUseId?: string | null;
  /** For a tool_result: did it succeed? */
  ok?: boolean | null;
  /** Full output for expandable results (#28: never dumped inline). */
  detail?: string | null;
  italic?: boolean;
}

/** What is arriving right now for a card, before the final text lands. */
export interface LiveStream {
  text: string;
  thinking: string;
  /** Model turns so far, while the run is alive. The total lands on Done. */
  turns?: number;
}

/** One shape for both live updates and stored log lines. The gutter word and
 *  the text are kept apart, so the transcript can align one and wrap the
 *  other. */
export function toLine(u: RunUpdate | RunLogLine): LogLine | null {
  const line = (
    label: string,
    text: string,
    labelColor: string,
    color: string,
    italic?: boolean,
  ): LogLine => ({ ts: u.ts_ms, kind: u.kind, label, text, color, labelColor, italic });

  switch (u.kind) {
    // Handled as a live stream, never as a transcript line.
    case "delta":
    case "thinking":
      return null;
    case "text":
      return u.text?.trim() ? line("text", u.text, "text-text4 dark:text-text4-d", "text-text2 dark:text-text2-d") : null;
    case "user_message":
      return u.text?.trim() ? line("you", u.text, "text-text4 dark:text-text4-d", "text-text2 dark:text-text2-d") : null;
    case "tool_use": {
      // The tool's own name is the gutter word: Read, Edit, Bash. Its colour
      // says what kind of call it was without a legend.
      const tool = (u.tool ?? "tool").replace(/^(harness|mcp__harness__)/, "").replace(/^__/, "");
      const colours: Record<string, string> = {
        Read: "text-ok dark:text-ok-d",
        Glob: "text-ok dark:text-ok-d",
        Grep: "text-ok dark:text-ok-d",
        Edit: "text-accent dark:text-accent-d",
        Write: "text-accent dark:text-accent-d",
        Bash: "text-info dark:text-info-d",
      };
      const l = line(tool, u.summary ?? "", colours[tool] ?? "text-text3 dark:text-text3-d", "text-text2 dark:text-text2-d");
      return {
        ...l,
        toolUseId: (u as RunUpdate & { tool_use_id?: string }).tool_use_id ?? null,
        parentToolUseId:
          (u as RunUpdate & { parent_tool_use_id?: string }).parent_tool_use_id ?? null,
      };
    }
    case "tool_result": {
      const ok = (u as RunUpdate & { ok?: boolean }).ok !== false;
      const detail = (u as RunUpdate & { detail?: string | null }).detail ?? null;
      return {
        ...line(
          ok ? "↳ ok" : "↳ failed",
          (u as RunUpdate & { summary?: string }).summary ?? "",
          ok ? "text-ok dark:text-ok-d" : "text-bad dark:text-bad-d",
          ok ? "text-text2 dark:text-text2-d" : "text-bad2 dark:text-bad2-d",
          !ok,
        ),
        toolUseId: (u as RunUpdate & { tool_use_id?: string }).tool_use_id ?? null,
        ok,
        detail,
      };
    }
    case "started":
      return line(
        "started",
        u.session_id ? `resumed ${u.session_id.slice(0, 12)}` : "new session",
        "text-text4 dark:text-text4-d",
        "text-text3 dark:text-text3-d",
      );
    case "done": {
      // One line that tells the truth: a done with an error is a failure
      // that happens to know its own cost — never two contradicting lines.
      const err = (u as RunUpdate & { error?: string | null }).error;
      if (err) {
        return line("failed", err, "text-bad dark:text-bad-d", "text-bad2 dark:text-bad2-d");
      }
      const cost = u.cost_usd != null ? `$${u.cost_usd.toFixed(4)}` : "no cost recorded";
      const turns = u.turns != null ? `${u.turns} turns · ` : "";
      return line("done", `${turns}${cost}`, "text-text4 dark:text-text4-d", "text-ok dark:text-ok-d");
    }
    case "failed":
      return line("failed", u.message ?? "unknown", "text-bad dark:text-bad-d", "text-bad2 dark:text-bad2-d");
    case "approval_requested":
      return line(
        "approval",
        `${u.tool ?? "tool"} — ${u.summary ?? ""}`.trim(),
        "text-warn dark:text-warn-d",
        "text-warn dark:text-warn-d",
      );
    case "approval_answered":
      return line(
        "approval",
        u.allow ? "you allowed it" : "you denied it",
        "text-warn dark:text-warn-d",
        u.allow ? "text-ok dark:text-ok-d" : "text-bad2 dark:text-bad2-d",
      );
    case "notice":
      return line("notice", u.text ?? "", "text-warn dark:text-warn-d", "text-warn dark:text-warn-d");
    default:
      return null;
  }
}

/** O que os eventos de execução deixam no ecrã, por cartão. */
export interface RunFeed {
  outputs: Record<string, LogLine[]>;
  /** The model a card's loaded run actually ran on, off its own `usage` lines.
   *  Not derivable from the agent profile: that says what the card would run on
   *  *today*, and a profile edited since the run would rewrite history. */
  runModels: Record<string, string>;
  /** Token-level stream per card, cleared when the final text arrives. */
  streams: Record<string, LiveStream>;
  /** Absorve um update já filtrado para o projecto em foco. */
  consume: (u: RunUpdate) => void;
  /** Uma execução nova começa com a transcrição do cartão vazia. */
  reset: (cardId: string) => void;
  /** Trocar de projecto deita fora o que era do anterior. */
  clear: () => void;
  /** Substitui a transcrição de um cartão pelo log guardado de uma execução. */
  setRunLog: (cardId: string, lines: RunLogLine[]) => void;
}

export function useRunFeed(): RunFeed {
  const [outputs, setOutputs] = useState<Record<string, LogLine[]>>({});
  const [runModels, setRunModels] = useState<Record<string, string>>({});
  const [streams, setStreams] = useState<Record<string, LiveStream>>({});

  const consume = useCallback((u: RunUpdate) => {
    // Deltas live outside the transcript: they are replaced by the final
    // text, and would otherwise be one log line per token.
    if (u.kind === "delta" || u.kind === "thinking") {
      if (!u.text) return;
      const key = u.kind === "delta" ? "text" : "thinking";
      setStreams((prev) => {
        const cur = prev[u.card_id] ?? { text: "", thinking: "" };
        return {
          ...prev,
          [u.card_id]: { ...cur, [key]: (cur[key] + u.text).slice(-2000) },
        };
      });
      return;
    }
    if (u.kind === "turns") {
      // Live progress toward the ceiling: turns counted per assistant
      // message. Cleared when the run ends (text/done/failed below).
      const count = (u as RunUpdate & { count?: number }).count ?? 0;
      setStreams((prev) => ({
        ...prev,
        [u.card_id]: { ...(prev[u.card_id] ?? { text: "", thinking: "" }), turns: count },
      }));
      return;
    }
    if (u.kind === "text" || u.kind === "done" || u.kind === "failed") {
      setStreams((prev) => {
        if (!prev[u.card_id]) return prev;
        const next = { ...prev };
        delete next[u.card_id];
        return next;
      });
    }

    const line = toLine(u);
    if (!line) return;
    setOutputs((prev) => ({
      ...prev,
      [u.card_id]: [...(prev[u.card_id] ?? []), line].slice(-MAX_LINES),
    }));
  }, []);

  const reset = useCallback((cardId: string) => {
    setOutputs((prev) => ({ ...prev, [cardId]: [] }));
    setStreams((prev) => ({ ...prev, [cardId]: { text: "", thinking: "" } }));
  }, []);

  const clear = useCallback(() => {
    setOutputs({});
    setStreams({});
  }, []);

  const setRunLog = useCallback((cardId: string, lines: RunLogLine[]) => {
    const mapped = lines.map(toLine).filter((l): l is LogLine => l != null);
    setOutputs((prev) => ({ ...prev, [cardId]: mapped.slice(-MAX_LINES) }));
    // `toLine` drops usage lines — they are accounting, not transcript — so the
    // model is read here, before that happens. The last one wins: a run that
    // fell back to another model ran on the one it finished with.
    const model = lines.reduce<string | null>(
      (found, l) => (l.kind === "usage" && l.model ? l.model : found),
      null,
    );
    if (model) setRunModels((prev) => ({ ...prev, [cardId]: model }));
  }, []);

  return { outputs, runModels, streams, consume, reset, clear, setRunLog };
}
