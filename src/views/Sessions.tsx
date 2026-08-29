import { useEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import { cx } from "../lib/cx";
import { clock, duration, money, num } from "../lib/format";
import { api, reason } from "../lib/ipc";
import { paneIn, rowIn } from "../lib/motion";
import { MODELS, tone } from "../lib/types";
import type { RunOutcome, ToolCount } from "../lib/types";
import type { LogLine } from "../state/events";
import { useStore } from "../state/store";
import { Caret, mono, truncate } from "../components/ui";

/** The six tracks of the runs table. Written once so the header, the rows and
 *  the skeleton cannot drift apart. */
const COLS =
  "grid grid-cols-[minmax(0,1.5fr)_minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,.9fr)_minmax(0,.8fr)_minmax(0,.6fr)] gap-x-3.5";

/** How a run ended, as the log recorded it. `unknown` is a run whose start is
 *  in the feed and whose ending is not, on a card the engine is not running:
 *  naming that succeeded or failed would invent the one fact it is missing. */
type Outcome = "running" | "succeeded" | "failed" | "stopped" | "unknown";

const OUTCOME: Record<Outcome, { label: string; skin: string }> = {
  running: { label: "RUNNING", skin: "bg-warnSoft text-warn dark:bg-warnSoft-d dark:text-warn-d" },
  succeeded: {
    label: "SUCCEEDED",
    skin: "bg-primarySoft text-primary dark:bg-primarySoft-d dark:text-primary-d",
  },
  failed: { label: "FAILED", skin: "bg-badSoft text-bad dark:bg-badSoft-d dark:text-bad-d" },
  // Cancelled is neither of the other two: the operator stopped it, and
  // calling that a failure blames the agent for an order it obeyed.
  stopped: { label: "STOPPED", skin: "bg-badSoft text-bad dark:bg-badSoft-d dark:text-bad-d" },
  unknown: { label: "UNFINISHED", skin: "bg-active text-muted dark:bg-active-d dark:text-muted-d" },
};

/** How the engine names an ending, in the five words this table shows. A run
 *  the operator stopped is not a run that failed. */
const ENDED: Record<RunOutcome, Outcome> = {
  completed: "succeeded",
  cancelled: "stopped",
  failed: "failed",
};

interface RunRow {
  key: string;
  runId: string;
  cardId: string;
  title: string;
  agentId: string;
  startedMs: number;
  endedMs: number | null;
  outcome: Outcome;
  /** What this run called, counted by the projection from the transcript on
   *  disk. Empty for a run whose log is gone. */
  tools: ToolCount[];
}

/** `Read·Edit×6` from the calls the run actually made. Null when the
 *  projection found no transcript — the cell then says so with an em-dash
 *  rather than guessing at what the run touched. */
function toolSummary(tools: ToolCount[]): string | null {
  if (tools.length === 0) return null;
  const parts = tools.slice(0, 3).map(({ tool, count }) => (count > 1 ? `${tool}×${count}` : tool));
  if (tools.length > 3) parts.push("…");
  return parts.join("·");
}

/** The transcript palette, keyed by what a line is rather than by its words. */
function verbSkin(l: LogLine): string {
  switch (l.kind) {
    case "tool_use":
    case "started":
      return "text-primary dark:text-primary-d";
    case "tool_result":
      return l.ok === false ? "text-bad dark:text-bad-d" : "text-ok dark:text-ok-d";
    case "done":
      return "text-ok dark:text-ok-d";
    case "failed":
      return "text-bad dark:text-bad-d";
    case "approval_requested":
    case "approval_answered":
    case "notice":
      return "text-warn dark:text-warn-d";
    default:
      return "text-faint dark:text-faint-d";
  }
}

/** A result, a failure and a permission carry their colour into the detail;
 *  everything else is body text. */
function detailSkin(l: LogLine): string {
  switch (l.kind) {
    case "tool_result":
    case "failed":
    case "done":
    case "approval_requested":
    case "approval_answered":
    case "notice":
      return verbSkin(l);
    default:
      return "text-muted dark:text-muted-d";
  }
}

/** One line of plain copy where rows would otherwise be. */
function Note({ children }: { children: React.ReactNode }) {
  return (
    <div className="border-t border-line2 px-4 py-4 text-body leading-relaxed text-muted dark:border-line2-d dark:text-muted-d">
      {children}
    </div>
  );
}

/** The table's own skeleton, at the dimensions the rows will have, so nothing
 *  moves when they arrive. */
function Skeleton() {
  const bar = "h-2.5 rounded-full bg-line3 dark:bg-line3-d";
  return (
    <>
      {[0, 1, 2, 3, 4, 5].map((i) => (
        <div
          key={i}
          aria-hidden
          className={cx(COLS, "items-center border-t border-line2 px-4 py-2.75 dark:border-line2-d")}
        >
          <span className="flex items-center gap-2">
            <span className="h-[22px] w-[22px] flex-none rounded-7px bg-line3 dark:bg-line3-d" />
            <span className={cx(bar, "w-14")} />
          </span>
          <span className={cx(bar, "w-full")} />
          <span className={cx(bar, "w-12")} />
          <span className={cx(bar, "w-16")} />
          <span className={cx(bar, "w-14")} />
          <span className={cx(bar, "w-9")} />
        </div>
      ))}
    </>
  );
}

/** Every recorded run on the left, the transcript of the one you picked on the
 *  right. Both are read back from the engine; nothing here is remembered. */
export function Sessions({
  selected,
  select,
  openReview,
}: {
  selected: string | null;
  select: (id: string | null) => void;
  openReview: (cardId: string) => void;
}) {
  const { snapshot, projectId, fatal, agents, activity, outputs, runModels, streams, loadRunLog, refresh } =
    useStore();

  // Elapsed times breathe once a second while something runs; a frozen number
  // is ambiguous between "thinking" and "stuck".
  const anyLive = !!snapshot?.sessions.some((s) => s.live);
  const [, tick] = useState(0);
  useEffect(() => {
    if (!anyLive) return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [anyLive]);

  // Rows and the total below them come out of one read: `refresh` fetches the
  // snapshot and the activity feed together, so the footer describes the same
  // moment as the rows above it.
  const rows = useMemo<RunRow[]>(() => {
    if (!snapshot) return [];
    const cards = new Map(snapshot.cards.map((c) => [c.id, c]));
    const live = new Set(
      snapshot.sessions.filter((s) => s.live && s.run_id).map((s) => s.run_id as string),
    );
    const open = new Map<string, RunRow>();
    const out: RunRow[] = [];
    // The feed arrives newest first; runs are paired by walking it forward in
    // time. The id does the pairing, so two runs of one card cannot be crossed
    // and no line of this depends on how a label happens to be worded.
    for (let i = activity.length - 1; i >= 0; i--) {
      const a = activity[i]!;
      // A run row without a run is bookkeeping about one — "Work reported" —
      // not a session that can be listed or replayed.
      if (a.kind !== "run" || !a.run_id) continue;
      const card = cards.get(a.card_id);
      // A discarded card takes its runs with it: the feed still has the
      // events, but there is no card left for the row to be about.
      if (!card) continue;
      if (!a.outcome) {
        const row: RunRow = {
          key: String(a.seq),
          runId: a.run_id,
          cardId: a.card_id,
          title: card.title,
          agentId: card.agent_id,
          startedMs: a.ts_ms,
          endedMs: null,
          outcome: live.has(a.run_id) ? "running" : "unknown",
          tools: a.tools,
        };
        open.set(a.run_id, row);
        out.push(row);
        continue;
      }
      // An ending whose start has scrolled out of the feed's window: there is
      // no start time to subtract from, so it is not a row this table can
      // state. It still counts in the total below, which comes off the cards.
      const started = open.get(a.run_id);
      if (!started) continue;
      started.outcome = ENDED[a.outcome];
      started.endedMs = a.ts_ms;
      open.delete(a.run_id);
    }
    out.reverse();
    return out;
  }, [activity, snapshot]);

  /** Every run the board has had, from the snapshot read alongside the feed.
   *  The feed is a window, so this is larger than the rows whenever the older
   *  runs have scrolled out of it. */
  const total = useMemo(
    () => (snapshot?.cards ?? []).reduce((n, c) => n + c.runs, 0),
    [snapshot],
  );

  // Which run the operator clicked, now that a row names one. Without it the
  // panel could only ever show a card's newest run, whichever row was picked.
  // The card is kept beside it: selection also moves from elsewhere in the
  // app, and a run id held over from the last card belongs to no row here.
  const [picked, setPicked] = useState<{ card: string; run: string } | null>(null);
  const pickedRun = picked && picked.card === selected ? picked.run : null;
  const card = snapshot?.cards.find((c) => c.id === selected) ?? null;
  const session = snapshot?.sessions.find((s) => s.card_id === card?.id);
  // A live run is writing into this card's lines as we watch, so that is the
  // one on screen; anything finished is read back by the id on its row.
  const runId = session?.live
    ? (session.run_id ?? null)
    : (pickedRun ?? session?.run_id ?? card?.current_run ?? null);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const lines = card ? (outputs[card.id] ?? []) : [];
  const stream = card ? streams[card.id] : undefined;

  const shown = useRef<string | null>(null);
  useEffect(() => {
    if (!card || !runId) return;
    const want = `${card.id}:${runId}`;
    if (shown.current === want) return;
    // Re-reading the file behind a live run would replace what is arriving
    // with a snapshot of it, mid-sentence.
    if (session?.live && (outputs[card.id] ?? []).length > 0) {
      shown.current = want;
      return;
    }
    shown.current = want;
    void loadRunLog(runId, card.id);
  }, [card, runId, session?.live, outputs, loadRunLog]);

  // Exporting is the operator picking a folder in their own file manager, so
  // the screen reports where the copy landed and nothing else.
  const [exporting, setExporting] = useState(false);
  const [exportFailed, setExportFailed] = useState<string | null>(null);
  const exportAll = async () => {
    if (!projectId) return;
    setExporting(true);
    setExportFailed(null);
    try {
      const done = await api.exportTranscripts(projectId);
      // A dismissed picker is an answer, not a failure.
      if (done) await api.reveal(done.dir);
    } catch (e) {
      setExportFailed(reason(e));
    } finally {
      setExporting(false);
    }
  };

  const end = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!session?.live) return;
    end.current?.scrollIntoView({ block: "nearest" });
  }, [session?.live, lines.length, stream?.text, stream?.thinking]);

  const pill = "rounded-full px-2.25 py-0.5 text-10 font-bold";
  const foot =
    "cursor-pointer rounded-full border border-line px-3.5 py-1.5 text-sm font-medium text-ink2 transition-colors duration-150 hover:bg-hovered disabled:cursor-not-allowed disabled:opacity-50 dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d";

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="min-h-0 flex-1 overflow-auto px-5.5 py-5"
    >
      <div className="flex min-w-[900px] flex-row items-start gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-3.5">
          <div>
            <div className="text-title font-bold text-ink dark:text-ink-d">Sessions</div>
            <div className="mt-0.75 text-body text-faint dark:text-faint-d">
              Every recorded run, replayed from disk
            </div>
          </div>

          <motion.div
            initial="hidden"
            animate="shown"
            className="overflow-hidden rounded-lg border border-line bg-surface dark:border-line-d dark:bg-surface-d"
          >
            <div className={cx(COLS, "px-4 py-2.5 text-sm font-semibold text-muted dark:text-muted-d")}>
              <span>Agent</span>
              <span>Card</span>
              <span>Started</span>
              <span>Tools</span>
              <span>Status</span>
              <span>Time</span>
            </div>

            {fatal ? (
              <div className="border-t border-line2 px-4 py-4 dark:border-line2-d">
                <div className={cx(mono, "text-11 leading-relaxed text-bad dark:text-bad-d")}>
                  {fatal}
                </div>
                <button type="button" onClick={() => void refresh()} className={cx(foot, "mt-3")}>
                  Retry
                </button>
              </div>
            ) : !projectId ? (
              <Note>
                No project selected. Pick one in the sidebar and its runs appear here.
              </Note>
            ) : !snapshot ? (
              <Skeleton />
            ) : rows.length === 0 ? (
              <Note>
                No run recorded in this project yet. Start a card on the board and the run it
                opens is listed here, transcript and all.
              </Note>
            ) : (
              rows.map((r, i) => {
                const who = agents.find((a) => a.id === r.agentId);
                const t = tone(who?.tone);
                const o = OUTCOME[r.outcome];
                const tools = toolSummary(r.tools);
                const elapsed =
                  r.endedMs != null
                    ? duration(r.endedMs - r.startedMs)
                    : r.outcome === "running"
                      ? duration(Date.now() - r.startedMs)
                      : "—";
                return (
                  <motion.button
                    key={r.key}
                    type="button"
                    custom={i}
                    variants={rowIn}
                    aria-pressed={r.cardId === selected && r.runId === runId}
                    onClick={() => {
                      select(r.cardId);
                      setPicked({ card: r.cardId, run: r.runId });
                    }}
                    className={cx(
                      COLS,
                      "w-full cursor-pointer items-center border-t border-line2 px-4 py-2.75 text-left text-body text-ink transition-colors duration-150 hover:bg-hovered dark:border-line2-d dark:text-ink-d dark:hover:bg-hovered-d",
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <span
                        className={cx(
                          "grid h-[22px] w-[22px] flex-none place-items-center rounded-7px font-mono text-[9px] font-semibold leading-none",
                          t.soft,
                          t.fg,
                        )}
                      >
                        {who?.initial ?? "?"}
                      </span>
                      <span className={truncate}>{who?.name ?? r.agentId}</span>
                    </span>
                    <span className={cx(truncate, "text-ink2 dark:text-ink2-d")}>{r.title}</span>
                    <span className="text-muted dark:text-muted-d">{clock(r.startedMs)}</span>
                    <span className={cx(mono, truncate, "text-11 text-muted dark:text-muted-d")}>
                      {tools ?? "—"}
                    </span>
                    <span>
                      <span className={cx(mono, pill, o.skin)}>{o.label}</span>
                    </span>
                    <span className={cx(mono, "text-muted dark:text-muted-d")}>{elapsed}</span>
                  </motion.button>
                );
              })
            )}

            {snapshot && !fatal && rows.length > 0 && (
              <div className="flex items-center gap-2 border-t border-line2 px-4 py-2.5 text-sm font-medium text-muted dark:border-line2-d dark:text-muted-d">
                <span className="flex-none">
                  Showing {num(rows.length)} of {num(total)} runs
                </span>
                {exportFailed && (
                  <span className={cx(mono, truncate, "text-11 text-bad dark:text-bad-d")}>
                    {exportFailed}
                  </span>
                )}
                <button
                  type="button"
                  disabled={exporting}
                  onClick={() => void exportAll()}
                  className="ml-auto flex-none cursor-pointer font-semibold text-primary transition-opacity duration-150 hover:opacity-80 disabled:cursor-not-allowed disabled:opacity-50 dark:text-primary-d"
                >
                  {exporting ? "Exporting…" : "Export transcripts →"}
                </button>
              </div>
            )}
          </motion.div>
        </div>

        <div className="w-[340px] flex-none overflow-hidden rounded-lg border border-line bg-surface dark:border-line-d dark:bg-surface-d">
          <div className="border-b border-line2 px-4 py-3.5 dark:border-line2-d">
            <div className="text-md font-bold text-ink dark:text-ink-d">Transcript</div>
            <div className={cx(mono, truncate, "mt-0.75 text-xs text-faint dark:text-faint-d")}>
              {card
                ? [
                    agent?.name ?? card.agent_id,
                    card.id,
                    // What the run reported spending tokens on, not what the
                    // profile is set to now — those differ the moment someone
                    // edits the profile, and the transcript is the record.
                    runModels[card.id] ??
                      MODELS.find((m) => m.id === agent?.model)?.name ??
                      "auto",
                    money(card.cost_usd, 4),
                  ].join(" · ")
                : "nothing picked"}
            </div>
          </div>

          <div className={cx(mono, "px-4 py-3 text-11 leading-[1.9] text-muted dark:text-muted-d")}>
            {!card ? (
              <span className="font-sans text-body text-muted dark:text-muted-d">
                Pick a run on the left to read its transcript here.
              </span>
            ) : lines.length === 0 && !stream ? (
              <span className="text-faint dark:text-faint-d">
                {runId ? "reading the log…" : "this card has no recorded run yet"}
              </span>
            ) : (
              lines.map((l, i) => (
                <div key={i} className="flex gap-1.5">
                  <span className="flex-none text-faint dark:text-faint-d">{clock(l.ts)}</span>
                  <span className={cx("flex-none", verbSkin(l))}>{l.label}</span>
                  <span
                    className={cx(
                      "min-w-0 flex-1 whitespace-pre-wrap break-words",
                      detailSkin(l),
                      l.italic && "italic",
                    )}
                  >
                    {l.text}
                  </span>
                </div>
              ))
            )}

            {stream?.thinking && !stream.text && (
              <div className="flex gap-1.5">
                <span className="flex-none text-faint dark:text-faint-d">thinking</span>
                <span className="min-w-0 flex-1 whitespace-pre-wrap italic text-muted dark:text-muted-d">
                  {stream.thinking}
                  <Caret />
                </span>
              </div>
            )}
            {stream?.text && (
              <div className="flex gap-1.5">
                <span className="flex-none text-primary dark:text-primary-d">text</span>
                <span className="min-w-0 flex-1 whitespace-pre-wrap break-words text-ink2 dark:text-ink2-d">
                  {stream.text}
                  <Caret />
                </span>
              </div>
            )}
            <div ref={end} />
          </div>

          <div className="flex gap-1.75 border-t border-line2 px-4 py-3 dark:border-line2-d">
            <button
              type="button"
              disabled={!card}
              onClick={() => card && openReview(card.id)}
              className={foot}
            >
              Open diff
            </button>
            <button
              type="button"
              disabled={!card || !runId}
              onClick={() => card && runId && void loadRunLog(runId, card.id)}
              className={foot}
            >
              Replay
            </button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
