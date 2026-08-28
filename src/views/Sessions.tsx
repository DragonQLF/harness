import { useEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import { api, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { paneIn, rowIn } from "../lib/motion";
import { duration, money, plural } from "../lib/format";
import { MODELS, STATUS_NAME, STATUS_TONE, tone } from "../lib/types";
import { useStore } from "../state/store";
import { Caret, Glyph, Loading, mono, truncate } from "../components/ui";

/** O ponto de cada estado. Não é o `STATUS_TONE`: aqui o backlog é `text4`,
 *  como o desenho o tem. */
const STATUS_DOT: Record<string, string> = {
  backlog: "bg-text4 dark:bg-text4-d",
  ready: "bg-info dark:bg-info-d",
  running: "bg-accent dark:bg-accent-d",
  review: "bg-warn dark:bg-warn-d",
  done: "bg-ok dark:bg-ok-d",
};

/** Uma pastilha de contorno. */
const CHIP =
  "min-h-6 cursor-pointer rounded-full border border-line3 text-sm font-normal text-text2 transition-[border-color,background,color] duration-150 hover:border-line4 hover:bg-surface2 dark:border-line3-d dark:text-text2-d dark:hover:border-line4-d dark:hover:bg-surface2-d";

/** A goteira da transcrição: a palavra à esquerda, o texto encostado a uma
 *  linha vertical. */
const GUTTER = "w-[74px] flex-none truncate text-right";
const BODY = "flex-1 min-w-0 pl-3 border-l border-line dark:border-line-d";

/** Two panes: everything recorded, and the transcript of the one you picked. */
export function Sessions({
  selected,
  select,
  openReview,
}: {
  selected: string | null;
  select: (cardId: string) => void;
  openReview: (cardId: string) => void;
}) {
  const {
    snapshot,
    projectId,
    agents,
    outputs,
    streams,
    loadRunLog,
    startRun,
    cancelRun,
    toast,
  } = useStore();
  const end = useRef<HTMLDivElement | null>(null);
  const [openDetails, setOpenDetails] = useState<Set<number>>(new Set());
  const toggleDetail = (i: number) =>
    setOpenDetails((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });

  const recorded = useMemo(() => {
    const cards = snapshot?.cards ?? [];
    return cards.filter((c) => c.runs > 0 || c.status === "running" || outputs[c.id]?.length);
  }, [outputs, snapshot]);

  const card = recorded.find((c) => c.id === selected) ?? recorded[0] ?? null;
  const session = snapshot?.sessions.find((s) => s.card_id === card?.id);
  const lines = card ? (outputs[card.id] ?? []) : [];
  const stream = card ? streams[card.id] : undefined;
  const live = card?.status === "running";
  // Elapsed timers breathe once a second while something runs; a frozen
  // number is ambiguous between "thinking" and "frozen".
  const [, tick] = useState(0);
  useEffect(() => {
    if (!live) return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [live]);

  useEffect(() => {
    if (!card || !session?.run_id) return;
    if ((outputs[card.id] ?? []).length > 0) return;
    loadRunLog(session.run_id, card.id);
  }, [card, session?.run_id, outputs, loadRunLog]);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [lines.length, stream?.text, stream?.thinking]);

  if (!snapshot) return <Loading what="Reading sessions" />;

  const agent = agents.find((a) => a.id === card?.agent_id);
  const at = tone(agent?.tone);
  const status = card ? STATUS_TONE[card.status] : STATUS_TONE.backlog;

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="grid min-h-0 flex-1 grid-cols-[280px_minmax(0,1fr)] overflow-hidden"
    >
      <div className="flex min-h-0 min-w-0 flex-col overflow-hidden border-r border-line dark:border-line-d">
        {/* A lista chega linha a linha — o `.stagger` do desenho. */}
        <motion.div
          initial="hidden"
          animate="shown"
          className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-2.5 py-3"
        >
          {recorded.map((c, i) => {
            const on = c.id === card?.id;
            const who = agents.find((a) => a.id === c.agent_id);
            const wt = tone(who?.tone);
            const s = snapshot.sessions.find((x) => x.card_id === c.id);
            const right =
              c.status === "running" && s
                ? duration(Date.now() - s.started_ms)
                : c.cost_usd > 0
                  ? money(c.cost_usd, 3)
                  : plural(c.turns, "turn");
            return (
              <motion.button
                key={c.id}
                custom={i}
                variants={rowIn}
                type="button"
                aria-pressed={on}
                onClick={() => select(c.id)}
                className={cx(
                  "w-full cursor-pointer rounded-md border px-3 py-2.5 text-left transition-colors duration-150",
                  on
                    ? "border-accentLine bg-accentSoft dark:border-accentLine-d dark:bg-accentSoft-d"
                    : "border-transparent bg-transparent hover:bg-hovered dark:hover:bg-hovered-d",
                )}
              >
                <div className="mb-1.5 flex items-center gap-2">
                  <span
                    className={cx(
                      "h-1.5 w-1.5 flex-none rounded-full",
                      STATUS_DOT[c.status],
                      c.status === "running" && "animate-pulse",
                    )}
                  />
                  <span
                    className={cx(truncate, "flex-1 text-md font-medium text-text dark:text-text-d")}
                  >
                    {c.title}
                  </span>
                  <Glyph tone={wt} size={16} font={8}>
                    {who?.initial ?? "?"}
                  </Glyph>
                </div>
                <div
                  className={cx(
                    mono,
                    "flex items-center justify-between gap-2.5 text-xs text-text3 dark:text-text3-d",
                  )}
                >
                  <span>
                    {c.id}
                    {c.status === "running" ? " · live" : ""}
                  </span>
                  <span>{right}</span>
                </div>
              </motion.button>
            );
          })}
          {recorded.length === 0 && (
            <div className="mx-0.5 my-1.5 rounded-md border border-dashed border-line2 px-3.5 py-4 text-sm font-normal leading-relaxed text-text4 dark:border-line2-d dark:text-text4-d">
              No session in this project yet. Start a card on the board and its transcript arrives
              here.
            </div>
          )}
        </motion.div>
      </div>

      <div className="grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
        {!card ? (
          <div className="max-w-[460px] p-6.5 text-md font-normal leading-[1.7] text-text4 dark:text-text4-d">
            Nothing recorded yet. Every run writes its transcript to disk as it goes, so
            once an agent has worked a card you can open it here and read it back turn by
            turn — including the runs that failed.
          </div>
        ) : (
          <>
            <div className="border-b border-line px-5.5 pb-3 pt-3.5 dark:border-line-d">
              <div className="mb-2.5 flex items-center gap-2.5">
                <Glyph tone={at} size={26} radius="50%" font={10}>
                  {agent?.initial ?? "?"}
                </Glyph>
                <span
                  className={cx(
                    truncate,
                    "text-lg font-semibold tracking-[-.01em] text-text dark:text-text-d",
                  )}
                >
                  {card.title}
                </span>
                <span
                  className={cx(
                    "flex-none rounded-full px-2.5 py-1 text-sm font-semibold",
                    status.soft,
                    status.fg,
                  )}
                >
                  {STATUS_NAME[card.status]}
                </span>
                <div className="flex-1" />
                <button
                  type="button"
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .openAgentTerminal(projectId, card.id)
                      .then(() => toast("info", "Terminal opened", "Resumed the session"))
                      .catch((e) => toast("bad", "Could not open a terminal", reason(e)));
                  }}
                  className={cx(
                    CHIP,
                    "px-3.5 py-2",
                    card.session_id ? "opacity-100" : "opacity-50",
                  )}
                >
                  Attach terminal
                </button>
                <button
                  type="button"
                  onClick={() => {
                    if (!session?.worktree) {
                      toast("warn", "No worktree", "This card has not written anything yet.");
                      return;
                    }
                    api.reveal(session.worktree).catch((e) =>
                      toast("bad", "Could not open that folder", reason(e)),
                    );
                  }}
                  className={cx(CHIP, "px-3.5 py-2")}
                >
                  Reveal worktree
                </button>
                <button
                  type="button"
                  onClick={() =>
                    card.status === "running"
                      ? cancelRun(card.id)
                      : card.status === "review"
                        ? openReview(card.id)
                        : startRun(card.id)
                  }
                  className={cx(
                    "min-h-6 cursor-pointer rounded-full border-none px-3.5 py-2 text-sm font-semibold transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px",
                    card.status === "running"
                      ? "bg-badSoft text-bad dark:bg-badSoft-d dark:text-bad-d"
                      : card.status === "review"
                        ? "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d"
                        : "bg-infoSoft text-info dark:bg-infoSoft-d dark:text-info-d",
                  )}
                >
                  {card.status === "running"
                    ? "Stop"
                    : card.status === "review"
                      ? "Read the diff"
                      : card.runs > 0
                        ? "Run again"
                        : "Start"}
                </button>
              </div>
              <div
                className={cx(
                  mono,
                  "flex flex-wrap items-center gap-3.5 text-sm text-text3 dark:text-text3-d",
                )}
              >
                <span className="text-text2 dark:text-text2-d">{card.id}</span>
                {session?.run_id && (
                  <span>
                    {session.run_id.slice(0, 8)} · {plural(card.runs, "run")}
                  </span>
                )}
                <span className="h-[3px] w-[3px] rounded-full bg-line3 dark:bg-line3-d" />
                <span>{session?.branch ?? card.branch ?? "no worktree"}</span>
                <span>{plural(card.turns, "turn")}</span>
                <span>{money(card.cost_usd, 4)}</span>
                <span className="h-[3px] w-[3px] rounded-full bg-line3 dark:bg-line3-d" />
                <span>
                  {agent?.name ?? card.agent_id} ·{" "}
                  {MODELS.find((m) => m.id === agent?.model)?.name ?? "auto"}
                </span>
                {card.session_id && <span>{card.session_id.slice(0, 12)}</span>}
                {card.runs > 1 && card.session_id && (
                  <span className="text-ok dark:text-ok-d">resumed from the last run</span>
                )}
              </div>
            </div>

            <div className="min-h-0 overflow-y-auto px-5.5 pb-5 pt-3.5 [scrollbar-gutter:stable]">
              {lines.length === 0 && (
                <div className={cx(mono, "text-md text-text4 dark:text-text4-d")}>
                  no transcript for this card yet
                </div>
              )}
              {(() => {
                // Nesting: each call's depth is one past its parent's, so a
                // subagent's calls sit inside the Task that spawned them.
                const depthsBy = new Map<string, number>();
                const depthOf = (l: (typeof lines)[number]) => {
                  if (!l.toolUseId) return 0;
                  if (!depthsBy.has(l.toolUseId)) {
                    const parent = l.parentToolUseId;
                    depthsBy.set(l.toolUseId, parent ? (depthsBy.get(parent) ?? 0) + 1 : 0);
                  }
                  return depthsBy.get(l.toolUseId) ?? 0;
                };
                return lines.map((l, i) => {
                  const depth = depthOf(l);
                  const expandable = l.kind === "tool_result" && !!l.detail;
                  const open = expandable && openDetails.has(i);
                  const row = (
                    <>
                      <span className={cx(GUTTER, l.labelColor)}>{l.label}</span>
                      <span
                        className={cx(
                          BODY,
                          l.color,
                          "whitespace-pre-wrap break-words",
                          l.italic && "italic",
                        )}
                      >
                        {l.text}
                      </span>
                    </>
                  );
                  const skin = cx(mono, "flex gap-3 text-md leading-[1.9]");
                  return (
                    <div key={i}>
                      {expandable ? (
                        <button
                          type="button"
                          aria-expanded={open}
                          onClick={() => toggleDetail(i)}
                          className={cx(
                            skin,
                            "w-full cursor-pointer text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
                          )}
                          style={{ paddingLeft: depth * 16 }}
                        >
                          {row}
                        </button>
                      ) : (
                        <div className={skin} style={{ paddingLeft: depth * 16 }}>
                          {row}
                        </div>
                      )}
                      {open && l.detail && (
                        <div
                          className={cx(
                            mono,
                            "mb-1.5 mt-1 overflow-x-auto whitespace-pre-wrap rounded-sm border border-line2 bg-surface px-2.5 py-2 text-sm leading-[1.7] text-text3 dark:border-line2-d dark:bg-surface-d dark:text-text3-d",
                          )}
                          style={{ marginLeft: depth * 16 + 86 }}
                        >
                          {l.detail}
                        </div>
                      )}
                    </div>
                  );
                });
              })()}

              {live && !!stream?.turns && (
                <div className={cx(mono, "flex gap-3 text-md leading-[1.9]")}>
                  <span className={cx(GUTTER, "text-text4 dark:text-text4-d")}>turns</span>
                  <span className={cx(BODY, "text-text3 dark:text-text3-d")}>
                    {stream.turns} so far
                  </span>
                </div>
              )}

              {stream?.thinking && !stream.text && (
                <div className={cx(mono, "flex gap-3 text-md leading-[1.9]")}>
                  <span className={cx(GUTTER, "text-text4 dark:text-text4-d")}>thinking</span>
                  <span
                    className={cx(BODY, "whitespace-pre-wrap italic text-text3 dark:text-text3-d")}
                  >
                    {stream.thinking}
                    <Caret />
                  </span>
                </div>
              )}
              {stream?.text && (
                <div className={cx(mono, "flex gap-3 text-md leading-[1.9]")}>
                  <span className={cx(GUTTER, "text-text4 dark:text-text4-d")}>text</span>
                  <span
                    className={cx(
                      BODY,
                      "whitespace-pre-wrap break-words text-text2 dark:text-text2-d",
                    )}
                  >
                    {stream.text}
                    <Caret />
                  </span>
                </div>
              )}
              {live && !stream?.text && !stream?.thinking && (
                <div className={cx(mono, "flex gap-3 text-md leading-[1.9]")}>
                  <span className={cx(GUTTER, "text-text4 dark:text-text4-d")}>live</span>
                  <span className={BODY}>
                    <Caret />
                  </span>
                </div>
              )}
              <div ref={end} />
            </div>
          </>
        )}
      </div>
    </motion.div>
  );
}
