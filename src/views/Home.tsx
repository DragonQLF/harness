/** Home — what is running, what needs you, today's spend, and one field to add
 *  work. The design is `docs/design/Relay.dc.html`, lines 62–163.
 *
 *  Every number here is a projection of `snapshot`, `project_stats`,
 *  `activity`, `active_runs`, `review_queue`, `approvals_pending`, `worktrees`
 *  and `run_stats` — the last of which is the heatmap, the three window tiles,
 *  the per-actor spend split and the day's line counts, all decided in
 *  `crates/app/src/runstats.rs` over the project's event log.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import { ChevronsRight, ChevronsUpDown } from "lucide-react";
import { cx } from "../lib/cx";
import { api, reason } from "../lib/ipc";
import { ago, clock, duration, money, num, plural, greeting, shortAgo } from "../lib/format";
import { paneIn, rowIn } from "../lib/motion";
import type { RunOutcome } from "../lib/types";
import { Card, EmptyNote, Meter, mono, truncate } from "../components/ui";
import { useStore } from "../state/store";
import type { ActiveRun, ActorFilter, QueueRow, RunStats } from "../lib/types";
import type { View } from "./views";

// ---- shared shapes ---------------------------------------------------------

/** A block standing in for a value that has not arrived, at the size the value
 *  will take, so nothing moves when it does. */
function Skel({ w, h = 12, className }: { w: number | string; h?: number; className?: string }) {
  return (
    <span
      aria-hidden
      className={cx("inline-block animate-pulse rounded-4px bg-active dark:bg-active-d", className)}
      style={{ width: w, height: h }}
    />
  );
}

const CHIP =
  "cursor-pointer whitespace-nowrap rounded-full border border-line bg-transparent px-3 py-1 text-sm font-medium text-ink2 transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d";

const QUIET_BUTTON =
  "cursor-pointer rounded-sm border border-line bg-transparent px-3.25 py-1.25 text-sm font-semibold text-ink2 transition-colors duration-150 hover:bg-hovered disabled:cursor-not-allowed disabled:opacity-50 dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d";

/** A menu hanging off a composer chip. Closes on a click anywhere else. */
function ChipMenu<T extends string>({
  label,
  value,
  options,
  onPick,
}: {
  label: string;
  value: T;
  options: { id: T; name: string }[];
  onPick: (id: T) => void;
}) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [open]);
  const current = options.find((o) => o.id === value);
  return (
    <div ref={box} className="relative flex-none">
      <button
        type="button"
        aria-label={label}
        aria-expanded={open}
        disabled={options.length === 0}
        onClick={() => setOpen((v) => !v)}
        className={cx(CHIP, "disabled:cursor-not-allowed disabled:opacity-50")}
      >
        {current?.name ?? label}
      </button>
      {open && (
        <div className="absolute bottom-full right-0 z-[200] mb-1.5 min-w-[150px] animate-popIn rounded-md border border-line bg-surface p-1.5 shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d">
          {options.map((o) => (
            <button
              key={o.id}
              type="button"
              onClick={() => {
                onPick(o.id);
                setOpen(false);
              }}
              className={cx(
                "flex w-full cursor-pointer items-center rounded-sm border-none px-2.5 py-1.75 text-left text-md transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
                o.id === value
                  ? "bg-primarySoft font-semibold text-ink dark:bg-primarySoft-d dark:text-ink-d"
                  : "bg-transparent font-medium text-ink2 dark:text-ink2-d",
              )}
            >
              {o.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ---- run history -----------------------------------------------------------

type HeatWindow = "1h" | "24h" | "7d";

type RunStatsState =
  | { phase: "loading" }
  | { phase: "ready"; data: RunStats }
  | { phase: "empty" }
  | { phase: "error"; message: string };

/** `run_stats` for the open project and the tab the operator is on.
 *
 *  Re-read on `seq` like the rest of the screen, so a run finishing lands in
 *  the grid without a poll. "empty" is only ever the no-project case: once
 *  there is a project the command answers, and a project with no runs answers
 *  with zeroes, which the cards say in their own words. */
function useRunStats(
  projectId: string | null,
  actor: ActorFilter,
  seq: number | null,
  reload: number,
): RunStatsState {
  const [state, setState] = useState<RunStatsState>({ phase: "loading" });
  useEffect(() => {
    if (!projectId) {
      setState({ phase: "empty" });
      return;
    }
    let alive = true;
    // A re-read keeps what is on screen. Switching tab changes the run counts
    // and nothing else — the spend split and the line counts are the same
    // numbers whichever actor is asked about — so blanking the card would
    // flicker three values that did not move.
    setState((s) => (s.phase === "ready" ? s : { phase: "loading" }));
    api
      .runStats(projectId, actor)
      .then((data) => {
        if (alive) setState({ phase: "ready", data });
      })
      .catch((e) => {
        if (alive) setState({ phase: "error", message: reason(e) });
      });
    return () => {
      alive = false;
    };
    // `seq` is the board's cursor: a finished run moves it.
  }, [projectId, actor, seq, reload]);
  return state;
}

/** How many columns the month labels are laid out for. The grid itself is
 *  whatever `run_stats` sent — `runstats::WEEKS` in Rust — and the two are the
 *  same number on purpose: the labels stand over the columns. */
const WEEKS = 38;
/** Cell 13px, gap 4px: seven rows are 7·13 + 6·4 = 115. The empty, loading and
 *  error states are held to it so the card never changes height. */
const GRID_H = "h-[115px]";

/** Sunday of each of the last 38 weeks, oldest first. Calendar, not data: it
 *  is what the columns stand for whether or not any runs fall in them. */
function weekStarts(): Date[] {
  const base = new Date();
  base.setHours(0, 0, 0, 0);
  base.setDate(base.getDate() - base.getDay());
  const out: Date[] = [];
  for (let i = WEEKS - 1; i >= 0; i--) {
    const d = new Date(base);
    d.setDate(base.getDate() - i * 7);
    out.push(d);
  }
  return out;
}

const HEAT = [
  "bg-heat0 dark:bg-heat0-d",
  "bg-heat1 dark:bg-heat1-d",
  "bg-heat2 dark:bg-heat2-d",
  "bg-heat3 dark:bg-heat3-d",
  "bg-heat4 dark:bg-heat4-d",
];

/** Which step of the ramp a day sits on, against the busiest day in view. */
function step(value: number, peak: number): string {
  if (value <= 0 || peak <= 0) return HEAT[0]!;
  const i = Math.min(4, Math.max(1, Math.ceil((value / peak) * 4)));
  return HEAT[i]!;
}

/** The 88×30 path for a window's points. Only ever called with real ones. */
function sparkPath(points: number[]): string {
  if (points.length < 2) return "";
  const peak = Math.max(...points);
  const dx = 88 / (points.length - 1);
  return points
    .map((v, i) => {
      const y = peak > 0 ? 28 - (v / peak) * 26 : 28;
      return `${i === 0 ? "M" : "L"}${(i * dx).toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

function Heatmap({
  actor,
  onActor,
  state,
  onRetry,
}: {
  actor: ActorFilter;
  onActor: (a: ActorFilter) => void;
  state: RunStatsState;
  onRetry: () => void;
}) {
  const starts = useMemo(weekStarts, []);
  const data = state.phase === "ready" ? state.data : null;
  // A grid is only drawn when there is something in it: 38 columns of zero
  // would read as a busy record of nothing.
  const weeks: number[][] | null = data && data.total > 0 ? data.heatmap : null;
  const peak = weeks ? Math.max(0, ...weeks.flat()) : 0;

  return (
    <Card className="px-4.5 py-4">
      <div className="flex items-start justify-between">
        <div>
          <div className="text-md font-medium text-muted dark:text-muted-d">
            Agent runs · last {WEEKS} weeks
          </div>
          <div className="mt-0.5 text-26 font-bold text-ink dark:text-ink-d">
            {state.phase === "loading" ? <Skel w={76} h={24} /> : data ? num(data.total) : "—"}
          </div>
        </div>
        {/* Not `Segmented`: this one is the design's tab strip — a white
            segment on `active`, radius 9 outside and 7 in — and the shared
            primitive is the accent-filled pill the rest of the app uses. */}
        <div className="flex flex-none rounded-9px bg-active p-[3px] dark:bg-active-d">
          {(
            [
              { id: "all", name: "All" },
              { id: "director", name: "Director" },
              { id: "agents", name: "Agents" },
            ] as { id: ActorFilter; name: string }[]
          ).map((o) => (
            <button
              key={o.id}
              type="button"
              aria-pressed={o.id === actor}
              onClick={() => onActor(o.id)}
              className={cx(
                "cursor-pointer whitespace-nowrap rounded-7px border-none px-3.25 py-1 text-sm transition-colors duration-150",
                o.id === actor
                  ? "bg-surface font-semibold text-ink shadow-segment dark:bg-surface-d dark:text-ink-d"
                  : "bg-transparent font-medium text-muted hover:text-ink dark:text-muted-d dark:hover:text-ink-d",
              )}
            >
              {o.name}
            </button>
          ))}
        </div>
      </div>

      <div className="mt-3.5 flex gap-2.5">
        <div className="flex flex-none flex-col justify-between pb-0.5 pt-4 text-2xs text-faint dark:text-faint-d">
          <span>M</span>
          <span>W</span>
          <span>F</span>
        </div>
        <div className="min-w-0 flex-1">
          <div className="mb-1.5 flex gap-1 text-10 text-faint dark:text-faint-d">
            {starts.map((d, i) => {
              const fresh = i === 0 || d.getMonth() !== starts[i - 1]!.getMonth();
              return (
                <span key={i} className="min-w-0 max-w-[13px] flex-1 whitespace-nowrap">
                  {fresh ? d.toLocaleDateString(undefined, { month: "short" }) : ""}
                </span>
              );
            })}
          </div>

          {state.phase === "loading" && (
            <div className={cx("flex gap-1", GRID_H)} aria-hidden>
              {starts.map((_, w) => (
                <span
                  key={w}
                  className="min-w-0 max-w-[13px] flex-1 animate-pulse rounded-3.5px bg-active dark:bg-active-d"
                />
              ))}
            </div>
          )}

          {state.phase === "error" && (
            <div className={cx("flex flex-col items-start gap-2", GRID_H)}>
              <span className="text-body text-bad dark:text-bad-d">{state.message}</span>
              <button type="button" onClick={onRetry} className={QUIET_BUTTON}>
                Try again
              </button>
            </div>
          )}

          {(state.phase === "empty" || (data !== null && !weeks)) && (
            <div className={cx("flex items-start", GRID_H)}>
              <span className="text-body text-faint dark:text-faint-d">
                {actor === "all"
                  ? "No finished runs recorded yet."
                  : `No finished runs recorded for the ${actor === "director" ? "Director" : "agents"} yet.`}
              </span>
            </div>
          )}

          {weeks && (
            <div className="flex gap-1">
              {weeks.map((week, w) => (
                <div key={w} className="flex min-w-0 max-w-[13px] flex-1 flex-col gap-1">
                  {week.map((value, d) => (
                    <span
                      key={d}
                      title={plural(value, "run")}
                      className={cx("h-[13px] w-full rounded-3.5px", step(value, peak))}
                    />
                  ))}
                </div>
              ))}
            </div>
          )}

          <div className="mt-2.5 flex items-center gap-1.25 text-11 text-muted dark:text-muted-d">
            Fewer
            {HEAT.map((skin, i) => (
              <span key={i} className={cx("h-[13px] w-[13px] flex-none rounded-3.5px", skin)} />
            ))}
            More
          </div>
        </div>
      </div>
    </Card>
  );
}

const WINDOW_NAMES: { id: HeatWindow; name: string }[] = [
  { id: "1h", name: "Last 1h" },
  { id: "24h", name: "Last 24h" },
  { id: "7d", name: "Last 7d" },
];

function WindowTiles({ state }: { state: RunStatsState }) {
  return (
    <div className="flex gap-3">
      {WINDOW_NAMES.map((w) => {
        const slice =
          state.phase === "ready" ? state.data.windows.find((x) => x.id === w.id) : undefined;
        const path = slice ? sparkPath(slice.series) : "";
        return (
          <Card key={w.id} className="flex flex-1 items-end justify-between px-[15px] py-3.25">
            <div className="min-w-0">
              <div className="text-sm font-medium text-faint dark:text-faint-d">{w.name}</div>
              <div className="mt-[3px] text-2xl font-bold text-ink dark:text-ink-d">
                {state.phase === "loading" ? (
                  <Skel w={48} h={20} />
                ) : slice ? (
                  num(slice.succeeded)
                ) : (
                  "—"
                )}
              </div>
              <div className="text-11 text-muted dark:text-muted-d">
                {state.phase === "error"
                  ? state.message
                  : slice
                    ? "Successful"
                    : "Not recorded yet"}
              </div>
            </div>
            {/* The shape stays at its final size whether or not there is a
                line to draw in it — an empty tile must not resize the row. */}
            <svg width="88" height="30" viewBox="0 0 88 30" aria-hidden className="flex-none">
              {path && (
                <path
                  d={path}
                  fill="none"
                  strokeWidth="1.6"
                  className="stroke-primary dark:stroke-primary-d"
                />
              )}
            </svg>
          </Card>
        );
      })}
    </div>
  );
}

// ---- recent runs: real, from activity + active_runs ------------------------

type RunState = "running" | "succeeded" | "failed" | "stopped";

/** How the log says a run ended, in the log's own words rather than the
 *  operator-facing prose beside it. `label` is written for a person to read and
 *  will be reworded; `outcome` is the recorded fact. */
const RUN_ENDED: Record<RunOutcome, RunState> = {
  completed: "succeeded",
  cancelled: "stopped",
  failed: "failed",
};

interface RunRow {
  key: string;
  cardId: string;
  agentId: string;
  title: string;
  startedMs: number;
  /** Null while the engine is still driving it. */
  endedMs: number | null;
  state: RunState;
}

const STATUS_SKIN: Record<RunState, string> = {
  running: "bg-warnSoft text-warn dark:bg-warnSoft-d dark:text-warn-d",
  succeeded: "bg-primarySoft text-primary dark:bg-primarySoft-d dark:text-primary-d",
  failed: "bg-badSoft text-bad dark:bg-badSoft-d dark:text-bad-d",
  // Cancelled is neither of the two the design draws, and calling it FAILED
  // would be a claim about the run that is not true.
  stopped: "bg-active text-muted dark:bg-active-d dark:text-muted-d",
};

const RUN_COLS =
  "grid-cols-[minmax(0,2fr)_minmax(0,1.2fr)_minmax(0,1.1fr)_minmax(0,.9fr)_minmax(0,.7fr)] gap-x-3";

type SortKey = "agent" | "status" | "duration";

function SortHead({
  label,
  on,
  onClick,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={on}
      className={cx(
        "flex cursor-pointer items-center gap-1.25 border-none bg-transparent p-0 text-left text-sm font-semibold transition-colors duration-150",
        on ? "text-ink dark:text-ink-d" : "text-muted hover:text-ink dark:text-muted-d dark:hover:text-ink-d",
      )}
    >
      {label}
      <ChevronsUpDown size={11} strokeWidth={2.4} className="flex-none text-faint" aria-hidden />
    </button>
  );
}

// ---- the screen ------------------------------------------------------------

export function Home({ go, openRun }: { go: (v: View) => void; openRun: (cardId: string) => void }) {
  const {
    project,
    projectId,
    settings,
    stats,
    snapshot,
    agents,
    activity,
    worktrees,
    approvals,
    createCard,
    cancelRun,
  } = useStore();

  const cards = useMemo(() => snapshot?.cards ?? [], [snapshot]);
  const seq = snapshot?.last_seq ?? null;

  // ---- the two commands this screen reads for itself ----

  const [live, setLive] = useState<ActiveRun[] | null>(null);
  const [liveError, setLiveError] = useState<string | null>(null);
  const [queue, setQueue] = useState<QueueRow[] | null>(null);
  const [queueError, setQueueError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    if (!projectId) {
      setLive(null);
      setQueue(null);
      return;
    }
    let alive = true;
    api
      .activeRuns(projectId)
      .then((rows) => {
        if (!alive) return;
        setLive(rows);
        setLiveError(null);
      })
      .catch((e) => {
        if (!alive) return;
        setLive(null);
        setLiveError(reason(e));
      });
    api
      .reviewQueue(projectId)
      .then((rows) => {
        if (!alive) return;
        setQueue(rows);
        setQueueError(null);
      })
      .catch((e) => {
        if (!alive) return;
        setQueue(null);
        setQueueError(reason(e));
      });
    return () => {
      alive = false;
    };
  }, [projectId, seq, reload]);

  // A duration that does not move is ambiguous between working and wedged.
  const [now, setNow] = useState(() => Date.now());
  const anyLive = (live?.length ?? 0) > 0;
  useEffect(() => {
    if (!anyLive) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [anyLive]);

  // ---- derived counts, all from the reads above ----

  const running = useMemo(() => cards.filter((c) => c.status === "running"), [cards]);
  const agentName = useCallback(
    (id: string) => agents.find((a) => a.id === id)?.name ?? id,
    [agents],
  );
  const workingAgents = useMemo(
    () => new Set((live ?? []).map((r) => r.agent_id)).size,
    [live],
  );
  const reviews = queue?.length ?? null;
  const needsYou = reviews == null ? null : reviews + approvals.length;
  // A checkout the engine is writing in is not stale whatever its age says.
  const stale = useMemo(
    () => worktrees.filter((w) => w.stale && !running.some((c) => c.worktree === w.path)),
    [running, worktrees],
  );

  // ---- the run rows ----

  const runs = useMemo<RunRow[]>(() => {
    const byId = new Map(cards.map((c) => [c.id, c]));
    const rows: RunRow[] = [];

    for (const r of live ?? []) {
      rows.push({
        key: r.run_id,
        cardId: r.card_id,
        agentId: r.agent_id,
        title: byId.get(r.card_id)?.title ?? r.card_id,
        startedMs: r.started_ms,
        endedMs: null,
        state: "running",
      });
    }

    // `activity` is newest first, so a run's end is met before its start. They
    // are paired by run id: a card can be run more than once and the ids say
    // which ending belongs to which start, where a per-card stack only assumed
    // it. A row from a log written before run ids were carried has none, and
    // goes unpaired rather than being guessed at.
    const ends = new Map<string, { ts: number; state: RunState }>();
    for (const row of activity) {
      if (row.kind !== "run" || !row.run_id) continue;
      if (row.outcome) {
        ends.set(row.run_id, { ts: row.ts_ms, state: RUN_ENDED[row.outcome] });
        continue;
      }
      const end = ends.get(row.run_id);
      // No recorded end: either the run the engine is still driving — already
      // above, from `active_runs` — or one whose ending never reached the log.
      // Neither has a status and a duration to state here.
      if (!end) continue;
      rows.push({
        key: `${row.card_id}:${row.seq}`,
        cardId: row.card_id,
        agentId: byId.get(row.card_id)?.agent_id ?? "",
        title: byId.get(row.card_id)?.title || row.detail || row.card_id,
        startedMs: row.ts_ms,
        endedMs: end.ts,
        state: end.state,
      });
    }

    return rows.sort((a, b) => b.startedMs - a.startedMs);
  }, [activity, cards, live]);

  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 } | null>(null);
  const flip = (key: SortKey) =>
    setSort((s) => (s?.key === key && s.dir === 1 ? { key, dir: -1 } : { key, dir: 1 }));

  const sorted = useMemo(() => {
    if (!sort) return runs;
    const value = (r: RunRow) =>
      sort.key === "agent"
        ? agentName(r.agentId)
        : sort.key === "status"
          ? r.state
          : String((r.endedMs ?? now) - r.startedMs).padStart(14, "0");
    return [...runs].sort((a, b) => value(a).localeCompare(value(b)) * sort.dir);
  }, [agentName, now, runs, sort]);

  const shown = sorted.slice(0, 7);
  const runAgents = useMemo(() => new Set(runs.map((r) => r.agentId)).size, [runs]);

  // ---- the composer ----

  const field = useRef<HTMLInputElement | null>(null);
  const assignable = useMemo(() => agents.filter((a) => a.tasks_enabled), [agents]);
  const [title, setTitle] = useState("");
  const [agentId, setAgentId] = useState("");
  const [mode, setMode] = useState<"plan" | "start" | "later">("plan");
  const [adding, setAdding] = useState(false);
  const chosen = assignable.find((a) => a.id === agentId)?.id ?? assignable[0]?.id ?? "";
  useEffect(() => {
    if (!agentId && chosen) setAgentId(chosen);
  }, [agentId, chosen]);

  const submit = async () => {
    const clean = title.trim();
    if (!clean || !chosen || adding) return;
    setAdding(true);
    try {
      await createCard(clean, chosen, mode);
      setTitle("");
    } finally {
      setAdding(false);
    }
  };

  // `cancelRun` says so itself, once per card; nothing extra to announce here.
  const stopAll = () => Promise.all(running.map((c) => cancelRun(c.id)));

  // ---- run history ----

  const [actor, setActor] = useState<ActorFilter>("all");
  const runStats = useRunStats(projectId, actor, seq, reload);
  const history = runStats.phase === "ready" ? runStats.data : null;

  const budget = settings?.daily_budget_usd ?? 0;
  const spent = stats?.spend_today ?? 0;
  const spentPct = budget > 0 ? Math.min(100, (spent / budget) * 100) : 0;
  /** A share of the day's cap. Without a cap there is nothing to be a share
   *  of, and the rows say so rather than drawing an empty bar. */
  const share = (usd: number) => (budget > 0 ? Math.min(100, (usd / budget) * 100) : 0);

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="min-h-0 flex-1 overflow-auto px-5.5 py-5"
    >
      <div className="flex min-w-[880px] flex-col gap-3.25">
        {/* ---- who and where ---- */}
        <div className="flex items-start justify-between">
          <div className="min-w-0">
            <div className="text-body font-medium text-muted dark:text-muted-d">
              {project ? `${project.name} · ${project.base_branch}` : "No repository"}
            </div>
            <div className="mt-0.5 text-title font-bold text-ink dark:text-ink-d">
              {settings ? `${greeting()}, ${settings.user_name}` : <Skel w={220} h={20} />}
            </div>
            <div className="mt-[3px] flex items-center gap-1.5 text-body text-faint dark:text-faint-d">
              {live == null || needsYou == null || !stats ? (
                <Skel w={280} h={12} />
              ) : (
                <>
                  {plural(workingAgents, "agent")} working · {needsYou} things need you ·{" "}
                  {money(spent)} spent today
                </>
              )}
            </div>
          </div>
          <div className="flex flex-none items-center gap-2">
            <button
              type="button"
              onClick={() => void stopAll()}
              disabled={running.length === 0}
              className="cursor-pointer whitespace-nowrap rounded-full border border-line bg-surface px-[15px] py-1.75 text-body font-medium text-ink2 transition-colors duration-150 hover:bg-hovered disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-surface dark:border-line-d dark:bg-surface-d dark:text-ink2-d dark:hover:bg-hovered-d"
            >
              Stop all runs
            </button>
            <button
              type="button"
              onClick={() => field.current?.focus()}
              className="cursor-pointer whitespace-nowrap rounded-full border-none bg-primary px-[17px] py-1.75 text-body font-bold text-white transition-colors duration-150 hover:bg-primaryDeep dark:bg-primary-d"
            >
              New card
            </button>
          </div>
        </div>

        {/* ---- three counts ---- */}
        <div className="grid grid-cols-3 gap-3">
          <Card className="px-4 py-3.25">
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium text-muted dark:text-muted-d">Working now</span>
              {live != null && live.length > 0 && (
                <span className="rounded-full bg-okSoft px-2.25 py-0.5 text-xs font-bold text-ok dark:bg-okSoft-d dark:text-ok-d">
                  {live.length} live
                </span>
              )}
            </div>
            <div className="mt-1 flex items-baseline gap-2">
              <span className="text-stat font-bold text-ink dark:text-ink-d">
                {snapshot ? running.length : <Skel w={28} h={22} />}
              </span>
              <span className="text-sm text-faint dark:text-faint-d">
                runs · {plural(worktrees.length, "worktree")} open
              </span>
            </div>
            {liveError && (
              <div className="mt-1.5 flex items-center gap-2 text-11 text-bad dark:text-bad-d">
                <span className={truncate}>{liveError}</span>
                <button
                  type="button"
                  onClick={() => setReload((n) => n + 1)}
                  className="flex-none cursor-pointer border-none bg-transparent p-0 font-semibold text-primary underline dark:text-primary-d"
                >
                  Retry
                </button>
              </div>
            )}
          </Card>

          <Card className="px-4 py-3.25">
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium text-muted dark:text-muted-d">Needs you</span>
              {needsYou != null && needsYou > 0 && (
                <span className="rounded-full bg-warnSoft px-2.25 py-0.5 text-xs font-bold text-warn dark:bg-warnSoft-d dark:text-warn-d">
                  waiting
                </span>
              )}
            </div>
            <div className="mt-1 flex items-baseline gap-2">
              <span className="text-stat font-bold text-ink dark:text-ink-d">
                {needsYou == null ? <Skel w={28} h={22} /> : needsYou}
              </span>
              <span className="text-sm text-faint dark:text-faint-d">
                {reviews == null
                  ? "reading the queue"
                  : `${plural(reviews, "review")} · ${plural(approvals.length, "permission")}`}
              </span>
            </div>
            {queueError && (
              <div className="mt-1.5 flex items-center gap-2 text-11 text-bad dark:text-bad-d">
                <span className={truncate}>{queueError}</span>
                <button
                  type="button"
                  onClick={() => setReload((n) => n + 1)}
                  className="flex-none cursor-pointer border-none bg-transparent p-0 font-semibold text-primary underline dark:text-primary-d"
                >
                  Retry
                </button>
              </div>
            )}
          </Card>

          <Card className="px-4 py-3.25">
            <div className="flex items-center gap-2">
              <span className="text-sm font-medium text-muted dark:text-muted-d">
                Cards done today
              </span>
            </div>
            <div className="mt-1 flex items-baseline gap-2">
              <span className="text-stat font-bold text-ink dark:text-ink-d">
                {stats ? num(stats.done_today) : <Skel w={28} h={22} />}
              </span>
              {/* Lines are git's answer, not the board's: `run_stats` asks
                  `git log --all --numstat` from the operator's own midnight,
                  so this is what today's commits actually moved. Until it
                  arrives the card says what the log knows. */}
              <span className="text-sm text-faint dark:text-faint-d">
                {history
                  ? `+${num(history.lines_added_today)} −${num(history.lines_removed_today)} lines`
                  : stats
                    ? `${plural(stats.runs_today, "run")} · ${money(stats.spend_today)}`
                    : ""}
              </span>
            </div>
          </Card>
        </div>

        {/* ---- one field to add work ---- */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
          className="flex items-center gap-2.5 rounded-full border border-line bg-surface px-4 py-2.25 transition-shadow duration-150 focus-within:border-primaryLine focus-within:shadow-composer dark:border-line-d dark:bg-surface-d dark:focus-within:border-primaryLine-d"
        >
          <input
            ref={field}
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            aria-label="Describe what should happen next"
            placeholder="Describe what should happen next…"
            className="min-w-0 flex-1 border-none bg-transparent text-md text-ink outline-none placeholder:text-faint dark:text-ink-d dark:placeholder:text-faint-d"
          />
          <ChipMenu
            label="Agent"
            value={chosen}
            options={assignable.map((a) => ({ id: a.id, name: a.name }))}
            onPick={setAgentId}
          />
          <ChipMenu
            label="Mode"
            value={mode}
            options={[
              { id: "plan", name: "Plan" },
              { id: "start", name: "Start" },
              { id: "later", name: "Later" },
            ]}
            onPick={setMode}
          />
          <button
            type="submit"
            disabled={!title.trim() || !chosen || adding}
            className="flex-none cursor-pointer whitespace-nowrap rounded-full border-none bg-primary px-3.5 py-1 text-sm font-bold text-white transition-colors duration-150 hover:bg-primaryDeep disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-primary dark:bg-primary-d"
          >
            Add
          </button>
        </form>

        {/* ---- history on the left, money and checkouts on the right ---- */}
        <div className="flex items-start gap-4">
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            <Heatmap
              actor={actor}
              onActor={setActor}
              state={runStats}
              onRetry={() => setReload((n) => n + 1)}
            />
            <WindowTiles state={runStats} />

            <div className="overflow-hidden rounded-md border border-line bg-surface dark:border-line-d dark:bg-surface-d">
              <div
                className={cx(
                  "grid items-center border-b border-line3 px-4 py-2.5 text-sm font-semibold text-muted dark:border-line3-d dark:text-muted-d",
                  RUN_COLS,
                )}
              >
                <SortHead label="Agent" on={sort?.key === "agent"} onClick={() => flip("agent")} />
                <span>Card</span>
                <span>Started</span>
                <SortHead
                  label="Status"
                  on={sort?.key === "status"}
                  onClick={() => flip("status")}
                />
                <SortHead
                  label="Duration"
                  on={sort?.key === "duration"}
                  onClick={() => flip("duration")}
                />
              </div>

              {!snapshot &&
                [0, 1, 2, 3].map((i) => (
                  <div
                    key={i}
                    className={cx(
                      "grid items-center border-b border-line2 px-4 py-2.75 dark:border-line2-d",
                      RUN_COLS,
                    )}
                  >
                    <Skel w="60%" />
                    <Skel w="80%" />
                    <Skel w={46} />
                    <Skel w={62} />
                    <Skel w={38} />
                  </div>
                ))}

              {snapshot &&
                shown.map((r, i) => (
                  <motion.button
                    key={r.key}
                    type="button"
                    custom={i}
                    variants={rowIn}
                    initial="hidden"
                    animate="shown"
                    onClick={() => openRun(r.cardId)}
                    className={cx(
                      "grid w-full cursor-pointer items-center border-b border-line2 bg-transparent px-4 py-2.75 text-left text-body transition-colors duration-150 hover:bg-hovered dark:border-line2-d dark:hover:bg-hovered-d",
                      RUN_COLS,
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-1.75 text-ink2 dark:text-ink2-d">
                      <ChevronsRight
                        size={13}
                        strokeWidth={2.4}
                        className="flex-none text-faint"
                        aria-hidden
                      />
                      <span className={truncate}>{agentName(r.agentId)}</span>
                    </span>
                    <span className={cx(truncate, "text-muted dark:text-muted-d")}>{r.title}</span>
                    <span className="text-muted dark:text-muted-d">{clock(r.startedMs)}</span>
                    <span>
                      <span
                        className={cx(
                          mono,
                          "rounded-full px-2.25 py-0.5 text-10 font-bold uppercase",
                          STATUS_SKIN[r.state],
                        )}
                      >
                        {r.state}
                      </span>
                    </span>
                    <span className={cx(mono, "text-muted dark:text-muted-d")}>
                      {duration((r.endedMs ?? now) - r.startedMs)}
                    </span>
                  </motion.button>
                ))}

              {snapshot && runs.length === 0 && (
                <EmptyNote>
                  No runs recorded for this project yet. Start a card and it appears here.
                </EmptyNote>
              )}

              <div className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium text-muted dark:text-muted-d">
                {/* Both counts come out of the rows above, which are the
                    `activity` window the store reads plus `active_runs`. */}
                Showing {shown.length} of {plural(runs.length, "run")} across{" "}
                {plural(runAgents, "agent")}
                <button
                  type="button"
                  onClick={() => go("sessions")}
                  className="ml-auto cursor-pointer border-none bg-transparent p-0 font-semibold text-primary transition-colors duration-150 hover:text-primaryDeep dark:text-primary-d"
                >
                  Open in Sessions →
                </button>
              </div>
            </div>
          </div>

          {/* ---- the rail ---- */}
          <div className="flex w-[286px] flex-none flex-col gap-3">
            <Card className="px-4 py-[15px]">
              <div className="text-base font-semibold text-ink dark:text-ink-d">Usage</div>
              {/* The split is `run_stats.spend_today`: every `run.finished` in
                  the log carries its cost, and the owning card names the actor
                  that spent it. The heatmap's tab does not narrow it — all
                  three rows are the point of the card. */}
              {(
                [
                  {
                    id: "total",
                    name: "Total",
                    usd: history?.spend_today.total ?? spent,
                    note:
                      budget > 0
                        ? `Of today's ${money(budget)} budget — includes your Claude Code login usage.`
                        : "No daily cap set, so there is nothing to measure today's spend against.",
                  },
                  {
                    id: "director",
                    name: "Director",
                    usd: history?.spend_today.director ?? 0,
                    note: "Review passes and board actions.",
                  },
                  {
                    id: "agents",
                    name: "Agents",
                    usd: history?.spend_today.agents ?? 0,
                    note: "Runs the crew did on cards.",
                  },
                ] as { id: string; name: string; usd: number; note: string }[]
              ).map((row, i) => (
                <div
                  key={row.id}
                  className={cx(
                    "mt-3",
                    i > 0 && "border-t border-dashed border-line3 pt-3 dark:border-line3-d",
                  )}
                >
                  <div className="flex justify-between text-body font-semibold text-ink dark:text-ink-d">
                    <span>{row.name}</span>
                    <span className="text-muted dark:text-muted-d">
                      {runStats.phase === "loading" && i > 0 ? (
                        <Skel w={30} h={12} />
                      ) : budget > 0 ? (
                        `${Math.round(share(row.usd))}%`
                      ) : (
                        money(row.usd)
                      )}
                    </span>
                  </div>
                  <div className="mt-1.5">
                    <Meter pct={share(row.usd)} height={4} />
                  </div>
                  <div className="mt-1.25 text-xs text-faint dark:text-faint-d">{row.note}</div>
                </div>
              ))}
              {runStats.phase === "error" && (
                <div className="mt-3 flex items-center gap-2 text-11 text-bad dark:text-bad-d">
                  <span className={truncate}>{runStats.message}</span>
                  <button
                    type="button"
                    onClick={() => setReload((n) => n + 1)}
                    className="flex-none cursor-pointer border-none bg-transparent p-0 font-semibold text-primary underline dark:text-primary-d"
                  >
                    Retry
                  </button>
                </div>
              )}
            </Card>

            <Card className="px-4 py-[15px]">
              <div className="text-base font-semibold text-ink dark:text-ink-d">Daily budget</div>
              <div className="mt-2.5 flex items-baseline justify-between">
                <span className="text-15 font-bold text-ink dark:text-ink-d">
                  {stats ? money(spent) : <Skel w={52} h={14} />}
                </span>
                <span className={cx(mono, "text-body font-semibold text-muted dark:text-muted-d")}>
                  of {money(budget)}
                </span>
              </div>
              <div className="mt-2">
                <Meter pct={spentPct} height={4} />
              </div>
              <div className="mt-1.5 text-11 text-faint dark:text-faint-d">
                Resets at midnight · runs pause at the cap
              </div>
              <button type="button" onClick={() => go("settings")} className={cx(QUIET_BUTTON, "mt-2.75")}>
                Adjust cap
              </button>
            </Card>

            <Card className="px-4 py-[15px]">
              <div className="text-base font-semibold text-ink dark:text-ink-d">Worktrees</div>
              {worktrees.length === 0 && (
                <div className="mt-2.75 text-body text-faint dark:text-faint-d">
                  No checkouts yet. One is made the first time an agent runs on a card.
                </div>
              )}
              {worktrees.map((w, i) => {
                const busy = running.some((c) => c.worktree === w.path);
                return (
                  <div
                    key={w.path}
                    className={cx(
                      "flex justify-between gap-2 text-body text-ink2 dark:text-ink2-d",
                      i === 0 ? "mt-2.75" : "mt-2",
                    )}
                  >
                    <span className={cx(truncate, mono, "text-11")} title={w.path}>
                      {w.branch ?? w.path.split("/").slice(-2).join("/")}
                    </span>
                    {/* The age is git's: the later of this checkout's HEAD
                        committer date and the directory's own mtime, so
                        "stale" means nothing has happened here rather than
                        nothing has been committed. A run in flight outranks
                        it — that checkout is being written in right now. */}
                    <span
                      className={cx(
                        "flex-none font-semibold",
                        busy
                          ? "text-ok dark:text-ok-d"
                          : w.dirty
                            ? "text-warn dark:text-warn-d"
                            : "font-normal text-faint dark:text-faint-d",
                      )}
                      title={w.last_activity_ms ? `last activity ${ago(w.last_activity_ms)}` : ""}
                    >
                      {busy
                        ? "running"
                        : w.stale
                          ? `stale ${shortAgo(w.last_activity_ms)}`
                          : w.dirty
                            ? "uncommitted"
                            : "idle"}
                    </span>
                  </div>
                );
              })}
              {/* Removing a checkout is `remove_worktree`, one at a time and
                  each with its own confirmation — so the button opens the
                  screen that does it rather than dropping several from here. */}
              <button
                type="button"
                onClick={() => go("trees")}
                disabled={stale.length === 0}
                className={cx(QUIET_BUTTON, "mt-3")}
              >
                {stale.length > 0 ? `Clean up ${plural(stale.length, "stale tree")}` : "Clean up stale"}
              </button>
            </Card>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
