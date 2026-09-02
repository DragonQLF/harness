import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Check, GitBranch, ListFilter, Lock, Play, TriangleAlert } from "lucide-react";
import { OverrideSheet, RejectSheet } from "../components/Overlays";
import { cx } from "../lib/cx";
import { clock, duration, money, plural, tail, truncate } from "../lib/format";
import { api, events, reason, type UnlistenFn } from "../lib/ipc";
import { arrive, colIn, rowIn, sheetIn } from "../lib/motion";
import {
  LEGAL_MOVES,
  STATUS_NAME,
  STATUS_ORDER,
  tone,
  type AgentProfile,
  type CardChecks,
  type Status,
} from "../lib/types";
import { useStore } from "../state/store";

/** What an empty column actually means. "Empty" five times over is the same
 *  word standing in for five different states of the work. */
const EMPTY_COLUMN: Record<Status, string> = {
  backlog: "Nothing parked for later.",
  ready: "Nothing waiting to start.",
  running: "No agent is working right now.",
  review: "Nothing finished is waiting to be read.",
  done: "Nothing approved yet.",
};

/** The count chip's skin per column. Done reads `+N` and takes the success
 *  tone because it counts a day's output, not a queue's depth. */
const CHIP_SKIN: Record<Status, string> = {
  backlog: "bg-active text-muted dark:bg-active-d dark:text-muted-d",
  ready: "bg-active text-muted dark:bg-active-d dark:text-muted-d",
  running: "bg-primarySoft text-primary dark:bg-primarySoft-d dark:text-primary-d",
  review: "bg-warnSoft text-warn dark:bg-warnSoft-d dark:text-warn-d",
  done: "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d",
};

/** Done keeps every approved card, and the column would grow without end. The
 *  rest is stated as a count rather than dropped in silence. */
const DONE_SHOWN = 4;

const CARD = "rounded-sheet border bg-surface px-3.25 py-3 dark:bg-surface-d";
const ACTION = "flex-1 cursor-pointer rounded-full py-1.25 text-center text-11";
const ACTION_DARK = "bg-ink font-bold text-white dark:bg-ink-d dark:text-canvas-d";
const ACTION_QUIET = "border border-line font-medium text-ink2 dark:border-line-d dark:text-ink2-d";

/** The agent on a card: its initial in its own tone, then its name. Small
 *  enough to sit in a card's meta row without becoming the card's subject. */
function Who({ agent, id }: { agent: AgentProfile | undefined; id: string }) {
  const t = tone(agent?.tone ?? "accent");
  return (
    <span className="flex min-w-0 items-center gap-1.75">
      <span
        className={cx(
          "grid h-4 w-4 flex-none place-items-center rounded-4px font-mono text-2xs font-semibold",
          t.soft,
          t.fg,
        )}
      >
        {(agent?.initial || agent?.name?.[0] || "?").toUpperCase()}
      </span>
      <span className="truncate text-11 font-medium text-ink2 dark:text-ink2-d">
        {agent?.name ?? id}
      </span>
    </span>
  );
}

/** Which cards changed column since the last render, and which way they went.
 *
 *  This is the board's whole reason for animating: an agent finishing a run
 *  moves a card with no action from the operator, and a silent re-render makes
 *  that indistinguishable from nothing having happened. The comparison is
 *  against the previous render rather than against a backend event, so a change
 *  is caught however it arrived — the operator's own drag included.
 *
 *  The mark clears itself, or a card would keep announcing a move it made
 *  minutes ago every time React re-rendered for an unrelated reason.
 */
function useArrivals(cards: { id: string; status: Status }[] | undefined) {
  const seen = useRef(new Map<string, Status>());
  const [moves, setMoves] = useState<Map<string, "forward" | "back">>(new Map());

  useEffect(() => {
    if (!cards) return;
    const previous = seen.current;
    const next = new Map<string, Status>();
    const fresh = new Map<string, "forward" | "back">();
    for (const card of cards) {
      next.set(card.id, card.status);
      const was = previous.get(card.id);
      // A card seen for the first time has not moved; it was created, or the
      // board just loaded. Announcing those would light up the whole screen.
      if (was && was !== card.status) {
        const from = STATUS_ORDER.indexOf(was);
        const to = STATUS_ORDER.indexOf(card.status);
        fresh.set(card.id, to >= from ? "forward" : "back");
      }
    }
    seen.current = next;
    if (fresh.size === 0) return;
    setMoves(fresh);
    const done = setTimeout(() => setMoves(new Map()), 900);
    return () => clearTimeout(done);
  }, [cards]);

  return moves;
}

/** O tempo que uma execução leva até agora, com o seu próprio segundo.
 *
 *  Elapsed time that does not move is a stopped watch, not a fact — mas o
 *  relógio estava no quadro inteiro. Uma vez por segundo re-renderizava todas
 *  as colunas e todos os cartões, e cada cartão é um `motion.div` com `layout`,
 *  que se volta a medir a cada render: o quadro fazia uma medição completa por
 *  segundo, para mexer um número. Agora o relógio está onde o número está, e é
 *  só ele que mexe. */
function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const beat = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(beat);
  }, []);
  return <>{duration(now - since)}</>;
}

/** The one place a card is created by hand: a line, who takes it, and whether
 *  it starts now. */
function NewCard({ close }: { close: () => void }) {
  const { agents, createCard } = useStore();
  const takers = agents.filter((a) => a.tasks_enabled && !a.paused);
  const [title, setTitle] = useState("");
  const [who, setWho] = useState(takers.find((a) => a.id !== "director")?.id ?? takers[0]?.id ?? "");

  const add = (mode: "later" | "plan" | "start") => {
    if (!title.trim()) return;
    createCard(title, who, mode);
    close();
  };

  return (
    <motion.div
      variants={sheetIn}
      initial="hidden"
      animate="shown"
      exit="gone"
      className="flex items-center gap-2 rounded-sheet border border-line bg-surface px-3.25 py-2.75 dark:border-line-d dark:bg-surface-d"
    >
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") add(e.shiftKey ? "start" : "plan");
          if (e.key === "Escape") close();
        }}
        aria-label="What should happen?"
        placeholder="What should happen? One card, one outcome."
        className="min-w-0 flex-1 rounded-sm border border-line bg-canvas px-3 py-1.75 text-body text-ink outline-none focus-visible:border-primaryLine dark:border-line-d dark:bg-canvas-d dark:text-ink-d dark:focus-visible:border-primaryLine-d"
      />
      <select
        value={who}
        aria-label="Who takes this card"
        onChange={(e) => setWho(e.target.value)}
        className="cursor-pointer rounded-sm border border-line bg-canvas px-2.25 py-1.75 text-sm font-medium text-ink2 dark:border-line-d dark:bg-canvas-d dark:text-ink2-d"
      >
        {takers.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
      {[
        { label: "Later", mode: "later" as const, strong: false },
        { label: "Ready", mode: "plan" as const, strong: false },
        { label: "Start now", mode: "start" as const, strong: true },
      ].map((b) => (
        <button
          key={b.mode}
          type="button"
          onClick={() => add(b.mode)}
          className={cx(
            "cursor-pointer rounded-full px-3 py-1.25 text-sm",
            b.strong
              ? "bg-primary font-bold text-white dark:bg-primary-d"
              : "border border-line font-medium text-ink2 dark:border-line-d dark:text-ink2-d",
            title.trim() ? "opacity-100" : "opacity-55",
          )}
        >
          {b.label}
        </button>
      ))}
      <button
        type="button"
        onClick={close}
        className="cursor-pointer px-2 text-sm font-medium text-faint hover:text-ink dark:text-faint-d dark:hover:text-ink-d"
      >
        Cancel
      </button>
    </motion.div>
  );
}

/** The head of a column: its name and how much is under it. */
/** The column's own header. The action on the right belongs to the column, not
 *  to any card in it: adding to Later, starting all of Ready, the ceiling
 *  Working runs against, and whether Review is waiting on a person. */
function ColumnHead({
  status,
  label,
  action,
}: {
  status: Status;
  label: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-md font-bold text-ink dark:text-ink-d">{STATUS_NAME[status]}</span>
      <span className={cx("rounded-full px-2 py-[1.5px] text-10 font-bold", CHIP_SKIN[status])}>
        {label}
      </span>
      {action && <span className="ml-auto flex items-center">{action}</span>}
    </div>
  );
}

/** A short list to choose from, over the board. One shape for both the agent
 *  picker and the dependency editor: they differ only in whether a choice
 *  closes the sheet or toggles in place. */
function Picker({
  title,
  options,
  chosen,
  selected,
  multi,
  pick,
  close,
}: {
  title: string;
  options: { id: string; label: string; hint?: string }[];
  chosen?: string;
  selected?: string[];
  multi?: boolean;
  pick: (id: string) => void;
  close: () => void;
}) {
  return (
    <motion.div
      variants={sheetIn}
      initial="hidden"
      animate="shown"
      exit="gone"
      className="fixed inset-0 z-[300] grid place-items-center bg-[rgba(8,8,14,.5)] p-6"
      onClick={close}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="max-h-[70vh] w-[380px] overflow-auto rounded-sheet border border-line bg-surface p-2 shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d"
      >
        <div className="px-2.5 py-2 text-md font-bold text-ink dark:text-ink-d">{title}</div>
        {options.length === 0 && (
          <div className="px-2.5 pb-2 text-sm text-faint dark:text-faint-d">
            Nothing to choose from.
          </div>
        )}
        {options.map((o) => {
          const on = multi ? selected?.includes(o.id) : chosen === o.id;
          return (
            <button
              key={o.id}
              type="button"
              onClick={() => pick(o.id)}
              className={cx(
                "flex w-full cursor-pointer items-center gap-2 rounded-sm border-none px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
                on ? "bg-primarySoft dark:bg-primarySoft-d" : "bg-transparent",
              )}
            >
              {multi && (
                <span
                  className={cx(
                    "grid h-3.5 w-3.5 flex-none place-items-center rounded-4px border",
                    on
                      ? "border-primary bg-primary text-white dark:border-primary-d dark:bg-primary-d"
                      : "border-line dark:border-line-d",
                  )}
                >
                  {on && <Check size={9} strokeWidth={3.4} aria-hidden />}
                </span>
              )}
              <span
                className={cx(
                  "min-w-0 flex-1 truncate text-md",
                  on
                    ? "font-semibold text-ink dark:text-ink-d"
                    : "font-medium text-ink2 dark:text-ink2-d",
                )}
              >
                {o.label}
              </span>
              {o.hint && (
                <span className="flex-none font-mono text-xs text-faint dark:text-faint-d">
                  {o.hint}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </motion.div>
  );
}

export function Board({
  openRun,
  openReview,
}: {
  openRun: (cardId: string) => void;
  openReview: (cardId: string) => void;
}) {
  const {
    snapshot,
    stats,
    agents,
    activity,
    approvals,
    outputs,
    streams,
    diffs,
    loadCardDiff,
    projectId,
    moveCard,
    startRun,
    cancelRun,
    assignAgent,
    approve,
    discard,
    toast,
  } = useStore();

  const [drag, setDrag] = useState<string | null>(null);
  const [over, setOver] = useState<Status | null>(null);
  const [adding, setAdding] = useState(false);
  const [rejecting, setRejecting] = useState<string | null>(null);
  const [forcing, setForcing] = useState<{ cardId: string; to: Status } | null>(null);
  const [only, setOnly] = useState<string | null>(null);
  const [filtering, setFiltering] = useState(false);
  const [assigning, setAssigning] = useState<string | null>(null);
  const [editingDeps, setEditingDeps] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [checks, setChecks] = useState<Record<string, CardChecks | null>>({});
  const [failed, setFailed] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const arrived = useArrivals(snapshot?.cards);
  // Com movimento reduzido o cartão continua a dizer que mudou de coluna — só
  // não viaja para o dizer. Calar-se aqui esconderia uma mudança de estado,
  // que é o contrário do que a preferência pede.
  const still = useReducedMotion();

  const cards = snapshot?.cards ?? [];
  const sessions = snapshot?.sessions ?? [];

  // A review card states what the run changed, and that lives in the worktree
  // rather than in the snapshot. Asked for once per card that needs it.
  const inReview = cards.filter((c) => c.status === "review").map((c) => c.id);
  const reviewKey = inReview.join(",");
  useEffect(() => {
    for (const id of reviewKey ? reviewKey.split(",") : []) loadCardDiff(id);
  }, [reviewKey, loadCardDiff]);

  // Checks belong to the card that ran them, in that card's own worktree. This
  // only reads what the last pass recorded — running them is minutes of the
  // operator's build, and the engine already starts a pass of its own when a
  // run finishes.
  useEffect(() => {
    if (!projectId) return;
    let alive = true;
    for (const id of reviewKey ? reviewKey.split(",") : []) {
      api
        .cardChecks(projectId, id)
        .then((pass) => alive && setChecks((seen) => ({ ...seen, [id]: pass })))
        .catch((e) => alive && setFailed(reason(e)));
    }
    return () => {
      alive = false;
    };
  }, [projectId, reviewKey, attempt]);

  // The pass a finished run triggers lands after the card is already sitting
  // in Review, so it arrives rather than being waited for.
  useEffect(() => {
    const subs: Promise<UnlistenFn>[] = [
      events.onCardChecks((e) => {
        if (e.project_id !== projectId) return;
        setChecks((seen) => ({ ...seen, [e.checks.card_id]: e.checks }));
      }),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()));
  }, [projectId]);

  /** When each card was last heard from. Only a fallback now: `finished_ms`
   *  on the card is the real finish time and is preferred everywhere below.
   *  This still covers cards whose log was written before finish times were
   *  recorded, and it reaches no further back than the activity window does —
   *  which is exactly why it is not the answer. */
  const lastSeen = useMemo(() => {
    const at = new Map<string, number>();
    for (const row of activity) {
      const known = at.get(row.card_id);
      if (known == null || row.ts_ms > known) at.set(row.card_id, row.ts_ms);
    }
    return at;
  }, [activity]);

  /** When a Done card finished, from the card itself where that exists. */
  const finishedAt = (card: { id: string; finished_ms: number | null }) =>
    card.finished_ms ?? lastSeen.get(card.id);

  /** A card is blocked while anything it depends on is not Done. The engine
   *  owns that rule; this only reads the same fact so the card can say why its
   *  Start is dark. Starting anyway is left to the engine to refuse. */
  const blockedBy = (card: { depends_on: string[] }) =>
    card.depends_on.filter((id) => cards.find((c) => c.id === id)?.status !== "done");

  /** Every card the filter lets through. "All agents" is no filter at all. */
  const visible = only ? cards.filter((c) => c.agent_id === only) : cards;

  /** How many runs may be in flight at once, summed over the profiles that can
   *  be handed a card. It is the engine's ceiling, not a display choice. */
  const cap = agents
    .filter((a) => a.tasks_enabled && !a.paused)
    .reduce((n, a) => n + a.max_concurrent, 0);

  const dragged = cards.find((c) => c.id === drag);

  /** A drop is an intent, not a move: nothing shifts on screen until the
   *  engine's next snapshot says it did, so a refused move leaves the card
   *  where it was. */
  const drop = (to: Status) => {
    setOver(null);
    const card = dragged;
    setDrag(null);
    if (!card || card.status === to) return;
    // Um movimento que o quadro não permite deixava de acontecer em silêncio:
    // o cartão voltava ao sítio e nada dizia porquê nem dava saída. O
    // `override_card` existia no motor e não tinha botão. Perguntar a razão é
    // a forma de o oferecer sem o tornar um atalho — um estado forçado sem
    // explicação é uma mentira no histórico.
    if (!LEGAL_MOVES[card.status].includes(to)) {
      setForcing({ cardId: card.id, to });
      return;
    }
    if (to === "running") startRun(card.id);
    else if (to === "done") openReview(card.id);
    else moveCard(card.id, to);
  };

  return (
    <div className="min-h-0 flex-1 overflow-auto px-5.5 py-5">
      <div className="flex min-w-[960px] flex-col gap-3.5">
        <div className="flex items-end justify-between">
          <div>
            <div className="text-title font-bold text-ink dark:text-ink-d">Board</div>
            <div className="mt-0.75 text-body text-faint dark:text-faint-d">
              {projectId && !snapshot
                ? ""
                : `${plural(visible.length, "card")} · ${
                    visible.filter((c) => c.status === "running").length
                  } running · `}
              drag to move · a run gets its own worktree
            </div>
          </div>
          <div className="flex items-center gap-2">
            {/* One agent's board. It narrows what is shown and nothing else —
                the counts follow the filter because they count what is on
                screen, and saying otherwise would be a different number beside
                the same cards. */}
            <div className="relative">
              <button
                type="button"
                aria-expanded={filtering}
                onClick={() => setFiltering((v) => !v)}
                className="flex cursor-pointer items-center gap-1.75 rounded-full border border-line bg-transparent px-3.25 py-1.5 text-body font-medium text-ink2 transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d"
              >
                <ListFilter size={13} strokeWidth={2.4} aria-hidden />
                {only ? (agents.find((a) => a.id === only)?.name ?? only) : "All agents"}
              </button>
              {filtering && (
                <div className="absolute right-0 top-full z-[60] mt-1.5 w-[190px] animate-popIn rounded-md border border-line bg-surface p-1.5 shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d">
                  {[{ id: null, name: "All agents" }, ...agents].map((a) => (
                    <button
                      key={a.id ?? "all"}
                      type="button"
                      onClick={() => {
                        setOnly(a.id);
                        setFiltering(false);
                      }}
                      className={cx(
                        "flex w-full cursor-pointer items-center gap-2 rounded-sm border-none bg-transparent px-2.5 py-1.5 text-left text-md transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
                        (a.id ?? null) === only
                          ? "font-semibold text-ink dark:text-ink-d"
                          : "font-medium text-ink2 dark:text-ink2-d",
                      )}
                    >
                      {a.name}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <button
              type="button"
              aria-expanded={adding}
              onClick={() => setAdding((v) => !v)}
              className="cursor-pointer rounded-full bg-primary px-4.25 py-1.75 text-body font-bold text-white dark:bg-primary-d"
            >
              New card
            </button>
          </div>
        </div>

        {failed && (
          <div className="flex items-center gap-3 rounded-sheet border border-warnLine bg-warnSheet px-3.25 py-2.75 dark:border-warnLine-d dark:bg-warnSheet-d">
            <span className="min-w-0 flex-1 font-mono text-11 text-warn dark:text-warn-d">
              {failed}
            </span>
            <button
              type="button"
              onClick={() => {
                setFailed(null);
                setAttempt((n) => n + 1);
              }}
              className="cursor-pointer rounded-full border border-line px-3 py-1 text-11 font-medium text-ink2 dark:border-line-d dark:text-ink2-d"
            >
              Retry
            </button>
          </div>
        )}

        <AnimatePresence>{adding && <NewCard close={() => setAdding(false)} />}</AnimatePresence>

        {/* Loading is the board at its own dimensions: the five column heads
            are the same five whatever the snapshot turns out to hold. With no
            project there is nothing on the way, so that reads as empty. */}
        {projectId && !snapshot ? (
          <div className="grid grid-cols-5 items-start gap-3">
            {STATUS_ORDER.map((status) => (
              <div key={status} className="flex min-h-0 flex-col gap-2.5">
                <div className="flex items-center gap-2">
                  <span className="text-md font-bold text-ink dark:text-ink-d">
                    {STATUS_NAME[status]}
                  </span>
                  <span className="h-3.5 w-6 animate-pulse rounded-full bg-active dark:bg-active-d" />
                </div>
              </div>
            ))}
          </div>
        ) : (
          <motion.div
            initial="hidden"
            animate="shown"
            className="grid grid-cols-5 items-start gap-3"
          >
            {STATUS_ORDER.map((status, ci) => {
              // Ready cards the engine would actually take: a card waiting on
              // a dependency is not one of them, and "Start all" must not fire
              // an intent it knows will be refused.
              const startable = visible.filter(
                (c) => c.status === "ready" && blockedBy(c).length === 0,
              );
              const all = visible.filter((c) => c.status === status);
              const list =
                status === "done"
                  ? [...all]
                      .sort((a, b) => (finishedAt(b) ?? 0) - (finishedAt(a) ?? 0))
                      .slice(0, DONE_SHOWN)
                  : all;
              const hidden = all.length - list.length;
              const canDrop = dragged ? LEGAL_MOVES[dragged.status].includes(status) : false;
              const hot = over === status && canDrop;

              return (
                <motion.div
                  key={status}
                  custom={ci}
                  variants={colIn}
                  onDragOver={(e) => {
                    if (!canDrop) return;
                    e.preventDefault();
                    if (over !== status) setOver(status);
                  }}
                  onDragLeave={() => setOver((s) => (s === status ? null : s))}
                  onDrop={(e) => {
                    e.preventDefault();
                    drop(status);
                  }}
                  className={cx(
                    "flex min-h-0 flex-col gap-2.5 rounded-sheet transition-colors duration-200",
                    hot && "bg-primarySoft dark:bg-primarySoft-d",
                  )}
                >
                  <ColumnHead
                    status={status}
                    label={
                      status === "done" ? `+${stats?.done_today ?? 0}` : String(all.length)
                    }
                    action={
                      status === "backlog" ? (
                        <button
                          type="button"
                          aria-label="Add a card to Later"
                          title="Add a card"
                          onClick={() => setAdding(true)}
                          className="cursor-pointer px-1 text-base text-faint transition-colors duration-150 hover:text-ink dark:text-faint-d dark:hover:text-ink-d"
                        >
                          +
                        </button>
                      ) : status === "ready" && startable.length > 0 ? (
                        <button
                          type="button"
                          onClick={() => startable.forEach((c) => startRun(c.id))}
                          className="flex cursor-pointer items-center gap-1 text-11 font-semibold text-primary dark:text-primary-d"
                        >
                          <Play size={10} strokeWidth={3} fill="currentColor" aria-hidden />
                          Start all
                        </button>
                      ) : status === "running" ? (
                        // The ceiling the board runs against, summed from the
                        // profiles that can actually be given a card. Without
                        // agents there is no ceiling to state, so none is.
                        cap > 0 ? (
                          <span className="font-mono text-11 text-faint dark:text-faint-d">
                            cap {cap}
                          </span>
                        ) : null
                      ) : status === "review" && all.length > 0 ? (
                        <span className="text-11 font-semibold text-warn dark:text-warn-d">
                          needs you
                        </span>
                      ) : status === "done" ? (
                        <span className="text-11 text-faint dark:text-faint-d">today</span>
                      ) : null
                    }
                  />

                  {list.map((card, ri) => {
                    const agent = agents.find((a) => a.id === card.agent_id);
                    const moved = arrived.get(card.id);
                    const ask = approvals.find((a) => a.card_id === card.id);
                    const session = sessions.find((s) => s.card_id === card.id && s.live);
                    const diff = diffs[card.id];
                    // A pass answers one run. The card's last run moving on
                    // makes it a fact about work that is no longer there, so
                    // it stops being shown rather than being shown as current.
                    const pass = checks[card.id];
                    const ranFor = sessions.find((s) => s.card_id === card.id)?.run_id ?? null;
                    const stale =
                      pass != null && pass.run_id !== "" && ranFor != null && pass.run_id !== ranFor;
                    const red =
                      pass && !stale ? pass.rows.filter((r) => r.status === "fail").length : 0;
                    const green =
                      pass && !stale ? pass.rows.filter((r) => r.status === "ok").length : 0;
                    const log = outputs[card.id] ?? [];
                    const last = log[log.length - 1];
                    // O que a execução está a fazer *agora*. O pensamento chega
                    // num buffer que guarda os últimos 2000 caracteres
                    // (`state/events.ts`), por isso lê-se pelo fim; a linha de
                    // transcrição já vem inteira e lê-se pelo princípio, que é
                    // onde está o nome da ferramenta.
                    const think = streams[card.id]?.thinking?.trim();
                    const doing = think
                      ? tail(think, 40)
                      : last
                        ? truncate(`${last.label} ${last.text}`.trim(), 40)
                        : "";

                    const waiting = blockedBy(card);
                    const at = finishedAt(card);
                    // What sits on the right of the card's meta row. The left
                    // is always who owns it; only this changes per column.
                    // O tempo decorrido conta o seu próprio segundo, por
                    // isso esta linha é um nó e já não uma string.
                    const elapsed = session ? <Elapsed since={session.started_ms} /> : null;
                    const cost = card.cost_usd > 0 ? money(card.cost_usd) : null;
                    const aside =
                      status === "running"
                        ? elapsed || cost
                          ? (
                              <>
                                {elapsed}
                                {elapsed && cost ? " · " : null}
                                {cost}
                              </>
                            )
                          : null
                        : status === "review"
                          ? null
                          : card.id;

                    return (
                      // `layout` é a razão de o `motion` estar aqui: o CSS não
                      // anima um elemento que muda de sítio no DOM, e um cartão
                      // que passa de Working para Review faz exactamente isso.
                      //
                      // O arrastar fica no filho de propósito: o `motion` come
                      // os `onDragStart`/`onDragEnd` para o seu próprio gesto e
                      // não os passa ao DOM, o que mataria o arrastar nativo.
                      <motion.div
                        key={card.id}
                        layout
                        custom={moved ? undefined : ri}
                        variants={moved && !still ? arrive(moved === "back") : rowIn}
                      >
                        <div
                          draggable
                          onDragStart={() => setDrag(card.id)}
                          onDragEnd={() => {
                            setDrag(null);
                            setOver(null);
                          }}
                          onClick={() =>
                            status === "review" ? openReview(card.id) : openRun(card.id)
                          }
                          className={cx(
                            "group relative cursor-grab",
                            CARD,
                            ask
                              ? "border-warnLine dark:border-warnLine-d"
                              : status === "running"
                                ? "border-primaryLine dark:border-primaryLine-d"
                                : "border-line dark:border-line-d",
                            drag === card.id ? "opacity-45" : "opacity-100",
                            // Com movimento reduzido a mudança diz-se com uma
                            // lavagem em vez de uma viagem.
                            moved && still && "bg-primarySoft dark:bg-primarySoft-d",
                          )}
                        >
                          {renaming === card.id ? (
                            // Correcting the wording is not the same act as
                            // discarding the card and writing it again: the id,
                            // the assignment and the dependencies all survive.
                            <input
                              autoFocus
                              value={draft}
                              aria-label={`Rename ${card.title}`}
                              onClick={(e) => e.stopPropagation()}
                              onChange={(e) => setDraft(e.target.value)}
                              onBlur={() => setRenaming(null)}
                              onKeyDown={(e) => {
                                e.stopPropagation();
                                if (e.key === "Escape") setRenaming(null);
                                if (e.key !== "Enter") return;
                                const next = draft.trim();
                                setRenaming(null);
                                if (!next || next === card.title || !projectId) return;
                                api
                                  .editCard(projectId, card.id, next)
                                  .catch((err) =>
                                    toast("bad", "Could not rename that card", reason(err)),
                                  );
                              }}
                              className="w-full rounded-sm border border-primaryLine bg-surface px-1.5 py-0.5 text-md font-semibold leading-[1.4] text-ink outline-none dark:border-primaryLine-d dark:bg-surface-d dark:text-ink-d"
                            />
                          ) : (
                            <div
                              onDoubleClick={(e) => {
                                // The engine refuses a rename once the card has
                                // run, so the affordance is not offered then —
                                // better than offering it and being refused.
                                if (card.runs > 0) return;
                                e.stopPropagation();
                                setDraft(card.title);
                                setRenaming(card.id);
                              }}
                              title={card.runs === 0 ? "Double-click to rename" : undefined}
                              className="pr-4 text-md font-semibold leading-[1.4] text-ink dark:text-ink-d"
                            >
                              {card.title}
                            </div>
                          )}

                          {/* Who owns the card, and the one fact that column
                              cares about beside it. */}
                          <div className="mt-2 flex items-center gap-2">
                            {status === "backlog" ? (
                              <span className="font-mono text-xs text-faint dark:text-faint-d">
                                {card.id}
                              </span>
                            ) : (
                              <Who agent={agent} id={card.agent_id} />
                            )}
                            {status === "review" && diff && (
                              <span className="ml-auto font-mono text-11">
                                <span className="text-ok dark:text-ok-d">+{diff.added}</span>{" "}
                                <span className="text-bad dark:text-bad-d">−{diff.removed}</span>
                              </span>
                            )}
                            {status === "backlog" && (
                              <button
                                type="button"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  moveCard(card.id, "ready");
                                }}
                                className="ml-auto flex cursor-pointer items-center gap-0.5 text-11 font-medium text-muted transition-colors duration-150 hover:text-ink dark:text-muted-d dark:hover:text-ink-d"
                              >
                                Ready ›
                              </button>
                            )}
                            {aside && status !== "backlog" && (
                              <span className="ml-auto shrink-0 font-mono text-xs text-faint dark:text-faint-d">
                                {aside}
                              </span>
                            )}
                          </div>

                          {status === "ready" && (
                            <>
                              {waiting.length > 0 && (
                                <div className="mt-2 flex items-center gap-1.75 rounded-sm bg-active px-2.25 py-1.5 dark:bg-active-d">
                                  <Lock
                                    size={11}
                                    strokeWidth={2.4}
                                    className="flex-none text-faint dark:text-faint-d"
                                    aria-hidden
                                  />
                                  <span className="truncate font-mono text-11 text-muted dark:text-muted-d">
                                    blocked by {waiting.join(", ")}
                                  </span>
                                </div>
                              )}
                              <div className="mt-2.5 flex gap-1.5">
                                <button
                                  type="button"
                                  disabled={waiting.length > 0}
                                  title={
                                    waiting.length > 0
                                      ? `Waiting on ${waiting.join(", ")}`
                                      : "Start a run for this card"
                                  }
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    startRun(card.id);
                                  }}
                                  className={cx(
                                    ACTION,
                                    "flex items-center justify-center gap-1.25",
                                    waiting.length > 0
                                      ? "cursor-default border border-line text-faint dark:border-line-d dark:text-faint-d"
                                      : ACTION_DARK,
                                  )}
                                >
                                  <Play size={9} strokeWidth={3} fill="currentColor" aria-hidden />
                                  Start
                                </button>
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    if (waiting.length > 0) setEditingDeps(card.id);
                                    else setAssigning(card.id);
                                  }}
                                  className={cx(ACTION, ACTION_QUIET)}
                                >
                                  {waiting.length > 0 ? "Deps" : "Assign"}
                                </button>
                              </div>
                            </>
                          )}

                          {status === "running" && (
                            <>
                              {doing && (
                                <div className="mt-1.5 truncate font-mono text-11 text-primary dark:text-primary-d">
                                  {doing}
                                </div>
                              )}
                              {/* Still empty, and deliberately.
                                  A ratio needs a numerator and a ceiling, and
                                  the engine has no pair of them. Turns are
                                  counted live (`RunEvent::Turns`) but nothing
                                  — not the agent profile, not Settings, not
                                  RunSpec — sets a turn ceiling to count them
                                  against. Spend has a real ceiling in
                                  `AgentProfile.budget_usd`, but no live
                                  numerator: cost only arrives on the run's
                                  final message, so `card.cost_usd` is what
                                  *earlier* runs cost and does not move while
                                  this one works. Elapsed against a median of
                                  past runs is a forecast, not progress.
                                  A bar filled from any of those would be a
                                  number invented to fill a bar. */}
                              <div className="mt-2 h-[3px] overflow-hidden rounded-3px bg-heat0 dark:bg-heat0-d">
                                {/* Alive, not a percentage. The mock draws a
                                    part-filled bar and there is nothing
                                    truthful to fill it with: turns are counted
                                    live but no ceiling exists anywhere to
                                    count them against, and spend has a real
                                    ceiling with no live numerator, because
                                    cost only lands on a run's final message.
                                    So this says "still going" and declines to
                                    say how far. */}
                                <div className="h-full w-1/4 animate-crawl rounded-3px bg-primary dark:bg-primary-d" />
                              </div>
                              <div className="mt-2.5 flex gap-1.5">
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    openReview(card.id);
                                  }}
                                  className={cx(ACTION, ACTION_QUIET)}
                                >
                                  Open diff
                                </button>
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    cancelRun(card.id);
                                  }}
                                  className={cx(
                                    ACTION,
                                    "border border-warnLine font-medium text-warn dark:border-warnLine-d dark:text-warn-d",
                                  )}
                                >
                                  Stop
                                </button>
                              </div>
                            </>
                          )}

                          {status === "review" && (
                            <>
                              {/* One verdict, not a pile of badges: either
                                  something is holding this card up, or the
                                  review passed and says so. */}
                              {ask || red > 0 ? (
                                <div className="mt-2 flex items-start gap-2 rounded-sm border border-warnLine bg-warnSoft px-2.25 py-1.75 dark:border-warnLine-d dark:bg-warnSoft-d">
                                  <TriangleAlert
                                    size={12}
                                    strokeWidth={2.4}
                                    className="mt-px flex-none text-warn dark:text-warn-d"
                                    aria-hidden
                                  />
                                  <span className="min-w-0 flex-1 text-11 font-semibold leading-[1.35] text-warn dark:text-warn-d">
                                    {[
                                      ask ? `wants ${truncate(ask.summary || ask.tool, 24)}` : null,
                                      red > 0 ? `${plural(red, "check")} red` : null,
                                    ]
                                      .filter(Boolean)
                                      .join(" · ")}
                                  </span>
                                </div>
                              ) : (
                                (card.last_review || green > 0) && (
                                  <div className="mt-2 flex items-start gap-1.75">
                                    <Check
                                      size={12}
                                      strokeWidth={3}
                                      className="mt-px flex-none text-ok dark:text-ok-d"
                                      aria-hidden
                                    />
                                    <span className="min-w-0 flex-1 text-11 leading-[1.35] text-muted dark:text-muted-d">
                                      {[
                                        card.last_review
                                          ? `${card.last_review.by === "director" ? "Director" : "You"}: ${truncate(card.last_review.reason, 22)}`
                                          : null,
                                        green > 0 ? `${plural(green, "check")} green` : null,
                                      ]
                                        .filter(Boolean)
                                        .join(" · ")}
                                    </span>
                                  </div>
                                )
                              )}
                              <div className="mt-2.5 flex gap-1.5">
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    approve(card.id);
                                  }}
                                  className={cx(
                                    ACTION,
                                    "bg-ink font-bold text-white dark:bg-ink-d dark:text-canvas-d",
                                  )}
                                >
                                  Approve
                                </button>
                                {/* Send back needs a reason the agent can act
                                    on, so the sheet asks for one rather than
                                    the board making one up. */}
                                <button
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    setRejecting(card.id);
                                  }}
                                  className={cx(
                                    ACTION,
                                    "border border-line font-medium text-ink2 dark:border-line-d dark:text-ink2-d",
                                  )}
                                >
                                  Send back
                                </button>
                              </div>
                            </>
                          )}

                          {status === "done" && (
                            <div className="mt-2 flex items-center gap-1.75 font-mono text-xs text-faint dark:text-faint-d">
                              <GitBranch size={11} strokeWidth={2.4} className="flex-none" aria-hidden />
                              {/* The branch a card's work landed on, and when.
                                  `finished_ms` is the card's own record; the
                                  activity fallback only covers logs written
                                  before finish times were kept. */}
                              <span className="truncate">
                                {card.branch ? card.branch.split("/").pop() : "merged"}
                              </span>
                              {at && <span>· {clock(at)}</span>}
                            </div>
                          )}

                          {status !== "running" && (
                            <button
                              type="button"
                              title="Delete this card"
                              aria-label={`Delete ${card.title}`}
                              onClick={(e) => {
                                e.stopPropagation();
                                discard(card.id);
                              }}
                              className="absolute right-2 top-2 cursor-pointer px-1 font-mono text-sm text-faint opacity-0 transition-opacity duration-150 hover:text-ink group-focus-within:opacity-100 group-hover:opacity-100 dark:text-faint-d dark:hover:text-ink-d"
                            >
                              ✕
                            </button>
                          )}
                        </div>
                      </motion.div>
                    );
                  })}

                  {hidden > 0 && (
                    <div className="py-1.5 text-center text-sm font-medium text-muted dark:text-muted-d">
                      {hidden} more
                    </div>
                  )}

                  {all.length === 0 && (
                    <div className="text-sm text-faint dark:text-faint-d">
                      {hot ? "Drop here" : EMPTY_COLUMN[status]}
                      {status === "backlog" && !hot && (
                        <button
                          type="button"
                          onClick={() => setAdding(true)}
                          className="ml-1 cursor-pointer font-medium text-primary underline-offset-2 hover:underline dark:text-primary-d"
                        >
                          Add one.
                        </button>
                      )}
                    </div>
                  )}
                </motion.div>
              );
            })}
          </motion.div>
        )}
      </div>

      <AnimatePresence>
        {rejecting && <RejectSheet cardId={rejecting} close={() => setRejecting(null)} />}
        {forcing && <OverrideSheet ask={forcing} close={() => setForcing(null)} />}
      </AnimatePresence>

      {/* Hand the card to someone else. Only profiles that may be given work
          are offered — a chat-only profile in this list would be an intent the
          engine refuses. */}
      <AnimatePresence>
        {assigning && (
          <Picker
            title="Hand this card to"
            close={() => setAssigning(null)}
            options={agents
              .filter((a) => a.tasks_enabled)
              .map((a) => ({
                id: a.id,
                label: a.name,
                hint: a.paused ? "paused" : (a.model ?? "claude chooses"),
              }))}
            chosen={cards.find((c) => c.id === assigning)?.agent_id}
            pick={(id) => {
              assignAgent(assigning, id);
              setAssigning(null);
            }}
          />
        )}
      </AnimatePresence>

      {/* What this card is waiting on. Toggling a row rewrites the whole list,
          because `set_dependencies` replaces rather than appends. */}
      <AnimatePresence>
        {editingDeps && (
          <Picker
            title="Waiting on"
            close={() => setEditingDeps(null)}
            multi
            options={cards
              .filter((c) => c.id !== editingDeps && c.status !== "done")
              .map((c) => ({ id: c.id, label: c.title, hint: STATUS_NAME[c.status] }))}
            selected={cards.find((c) => c.id === editingDeps)?.depends_on ?? []}
            pick={(id) => {
              const card = cards.find((c) => c.id === editingDeps);
              if (!card || !projectId) return;
              const next = card.depends_on.includes(id)
                ? card.depends_on.filter((d) => d !== id)
                : [...card.depends_on, id];
              api
                .setDependencies(projectId, card.id, next)
                .catch((e) => toast("bad", "Could not change what this card waits on", reason(e)));
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
