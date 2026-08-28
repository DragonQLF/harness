/** The right rail: everything that is waiting, running, or finished today, in
 *  that order. It is the same information the screens hold, ranked by whether
 *  it needs the operator. */

import { useEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { api } from "../lib/ipc";
import { cx } from "../lib/cx";
import { railIn, rowIn } from "../lib/motion";
import { ago, clock, duration, money, plural } from "../lib/format";
import { tone, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { Glyph, Icon, mono, truncate } from "./ui";

/** Um cartão do rail. */
const RAIL_CARD = "mb-2 rounded-md border border-line2 px-3 py-2.5 dark:border-line2-d";

/** Uma linha do rail que acende debaixo do ponteiro. */
const ROW = "transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d";

/** Um botão de texto discreto dentro de um cartão do rail. */
const QUIET_LINK =
  "min-h-6 cursor-pointer rounded-sm border-none bg-transparent text-xs font-medium text-text2 transition-colors duration-150 hover:text-text dark:text-text2-d dark:hover:text-text-d";

function Section({
  title,
  count,
  right,
  top,
}: {
  title: string;
  count?: string;
  right?: React.ReactNode;
  /** O espaço por cima, em px, quando esta secção segue outra. */
  top?: number;
}) {
  return (
    <div
      className="flex items-baseline gap-2 px-[3px] pb-2.25"
      style={top ? { paddingTop: top } : undefined}
    >
      <span className="text-sm font-semibold text-text2 dark:text-text2-d">{title}</span>
      {count && (
        <span className={cx(mono, "text-xs font-medium text-text3 dark:text-text3-d")}>
          · {count}
        </span>
      )}
      <div className="flex-1" />
      {right}
    </div>
  );
}

/** The 44px strip the rail collapses to: the count, who is working, and a way
 *  back. */
export function RightNowStrip({ open }: { open: () => void }) {
  const { approvals, snapshot, agents, proposals, outsideWork } = useStore();
  const cards = snapshot?.cards ?? [];
  const openProposals = proposals.filter((p) => p.status === "open");
  // The same arithmetic as the open rail's "Waiting on you", and it has to
  // stay the same: this badge is the only thing a collapsed rail says, so a
  // warning the rail counts and the strip does not is a warning nobody sees.
  const waiting =
    approvals.length +
    openProposals.length +
    outsideWork.length +
    cards.filter((c) => c.status === "review").length;
  const workers = [...new Set(cards.filter((c) => c.status === "running").map((c) => c.agent_id))];

  return (
    <button
      type="button"
      onClick={open}
      title="Right now"
      aria-label="Open the Right now rail"
      className={cx(
        ROW,
        "flex w-11 flex-none cursor-pointer flex-col items-center gap-2.5 border-none bg-recess py-3.5 dark:bg-recess-d",
      )}
    >
      {waiting > 0 && (
        <span
          className={cx(
            mono,
            "rounded-sm bg-warnSoft px-1.5 py-0.5 text-xs font-semibold text-warn dark:bg-warnSoft-d dark:text-warn-d",
          )}
        >
          {waiting}
        </span>
      )}
      {workers.slice(0, 4).map((id) => {
        const agent = agents.find((a) => a.id === id);
        const t = tone(agent?.tone);
        return (
          <Glyph key={id} tone={t} size={26} radius={9} font={10}>
            {agent?.initial ?? "?"}
          </Glyph>
        );
      })}
      <span className="text-xs font-medium tracking-[.08em] text-text4 [writing-mode:vertical-rl] dark:text-text4-d">
        RIGHT NOW
      </span>
    </button>
  );
}

export function RightNow({
  close,
  openReview,
  openSession,
  openTrees,
}: {
  close: () => void;
  /** Take the operator to the review screen for one card. */
  openReview: (cardId: string) => void;
  openSession: (cardId: string) => void;
  openTrees: () => void;
}) {
  const {
    approvals,
    answerApproval,
    snapshot,
    agents,
    projectId,
    activity,
    outputs,
    streams,
    stats,
    diffs,
    loadCardDiff,
    cancelRun,
    proposals,
    acceptProposal,
    dismissProposal,
    outsideWork,
    dismissOutsideWork,
  } = useStore();

  const cards = snapshot?.cards ?? [];
  const reviewing = cards.filter((c) => c.status === "review");
  const runningCards = cards.filter((c) => c.status === "running");
  const openProposals = proposals.filter((p) => p.status === "open");
  const [trees, setTrees] = useState<WorktreeRow[]>([]);
  const [shownWarning, setShownWarning] = useState<number | null>(null);

  // Elapsed timers must breathe: a frozen number reads as "frozen app". The
  // tick lives here because the elapsed number does — it used to sit in the
  // collapsed strip, which shows no time at all, so the one duration on screen
  // only moved when a token happened to arrive and stood still through every
  // long tool call.
  const anyLive = runningCards.length > 0;
  const [, tick] = useState(0);
  useEffect(() => {
    if (!anyLive) return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [anyLive]);

  // The numbers beside a review row are the real ones, read from the worktree.
  // Keyed by the card's run count, not by its id: a card sent back and run
  // again returns to Review with a different patch, and the cached answer from
  // the run before is the wrong diff shown with confidence.
  const readDiffs = useRef<Record<string, number>>({});
  useEffect(() => {
    reviewing.forEach((c) => {
      if (readDiffs.current[c.id] === c.runs) return;
      readDiffs.current[c.id] = c.runs;
      loadCardDiff(c.id);
    });
    // The key below is the whole dependency: `diffs` is deliberately out, since
    // re-running on every cached answer would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reviewing.map((c) => `${c.id}:${c.runs}`).join(","), loadCardDiff]);

  useEffect(() => {
    if (!projectId) return;
    let alive = true;
    api
      .worktrees(projectId)
      .then((rows) => alive && setTrees(rows))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [projectId, snapshot?.last_seq]);

  const startOfDay = new Date();
  startOfDay.setHours(0, 0, 0, 0);
  // A linha diz que foi uma aprovação; até aqui isto lia o rótulo por prefixo,
  // que é prosa escrita no backend e muda sem ninguém dar por isso.
  const doneToday = activity.filter(
    (a) => a.approved && a.ts_ms >= startOfDay.getTime(),
  );

  const liveSpend = runningCards.reduce((sum, c) => sum + c.cost_usd, 0);
  const waiting =
    approvals.length + openProposals.length + outsideWork.length + reviewing.length;
  // Four sections each announcing their own emptiness is the same news told
  // four times, and it makes a calm machine look like a broken one. When
  // nothing at all is happening, say that once and let the rail go quiet.
  const allQuiet =
    waiting === 0 &&
    openProposals.length === 0 &&
    outsideWork.length === 0 &&
    runningCards.length === 0 &&
    doneToday.length === 0;

  return (
    <motion.div
      variants={railIn}
      initial="hidden"
      animate="shown"
      exit="gone"
      className="flex w-[318px] flex-none flex-col overflow-hidden bg-recess dark:bg-recess-d"
    >
      <div className="flex flex-none items-center gap-2.5 border-b border-line px-4 pb-3 pt-3.5 dark:border-line-d">
        <span className="flex-1 text-md font-semibold text-text dark:text-text-d">Right now</span>
        <button
          type="button"
          onClick={close}
          aria-label="Close the Right now rail"
          className="grid h-6 w-6 cursor-pointer place-items-center rounded-sm border-none bg-transparent text-text4 transition-colors duration-150 hover:bg-hovered hover:text-text dark:text-text4-d dark:hover:bg-hovered-d dark:hover:text-text-d"
        >
          <Icon.close />
        </button>
      </div>

      {/* O rail chega secção a secção — o `.stagger` do desenho. */}
      <motion.div
        initial="hidden"
        animate="shown"
        className="min-h-0 flex-1 overflow-y-auto px-3 pb-4 pt-3"
      >
        <motion.div custom={0} variants={rowIn}>
          <Section
            title="Waiting on you"
            right={
              waiting > 0 ? (
                <span className={cx(mono, "text-xs font-medium text-warn dark:text-warn-d")}>
                  · {waiting}
                </span>
              ) : undefined
            }
          />

          {waiting === 0 && (
            <div
              className={cx(
                RAIL_CARD,
                "text-sm font-normal leading-relaxed text-text4 dark:text-text4-d",
                allQuiet ? "bg-surface dark:bg-surface-d" : "bg-transparent",
              )}
            >
              {allQuiet ? (
                <>
                  <span className="block font-semibold text-text2 dark:text-text2-d">
                    All quiet
                  </span>
                  Nothing running, nothing waiting on you, nothing approved yet today. A
                  run that wants a permission, or a diff that is finished, arrives here.
                </>
              ) : (
                <>
                  Nothing needs you. A run that wants a permission, or a diff that is
                  finished, arrives here.
                </>
              )}
            </div>
          )}

          {approvals.map((request) => {
            const card = cards.find((c) => c.id === request.card_id);
            const agent = agents.find((a) => a.id === card?.agent_id);
            const t = tone(agent?.tone ?? "warn");
            return (
              <div
                key={request.request_id}
                // Um agente parou e não continua sem resposta: é a única coisa
                // na app que está mesmo bloqueada por uma pessoa, e por isso
                // pode chegar em vez de aparecer. Uma vez, à entrada.
                className="mb-2 flex animate-[askedIn_.34s_cubic-bezier(.16,1,.3,1)_both] flex-col gap-2 rounded-md border border-warn bg-warnSoft px-3 py-2.5 dark:border-warn-d dark:bg-warnSoft-d"
              >
                <span className="flex items-center gap-2">
                  <Glyph tone={t} size={18} radius={6} font={8}>
                    {agent?.initial ?? "?"}
                  </Glyph>
                  <span
                    className={cx(
                      truncate,
                      "flex-1 text-sm font-semibold text-text dark:text-text-d",
                    )}
                  >
                    {agent?.name ?? "An agent"} · permission
                  </span>
                  <span
                    className={cx(
                      mono,
                      "rounded-sm bg-warnSoft px-2 py-px text-xs font-medium text-warn dark:bg-warnSoft-d dark:text-warn-d",
                    )}
                  >
                    {request.tool}
                  </span>
                </span>
                <span
                  className={cx(
                    mono,
                    "break-words text-xs font-medium leading-normal text-warn dark:text-warn-d",
                  )}
                >
                  {request.summary || "no details given"}
                </span>
                <span className="flex items-center gap-2.5">
                  <button
                    type="button"
                    onClick={() => answerApproval(request.request_id, true, false)}
                    className="min-h-6 cursor-pointer rounded-full border-none bg-accent px-3 py-1.5 text-xs font-semibold text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px dark:bg-accent-d dark:text-onAccent-d"
                  >
                    Allow
                  </button>
                  <button
                    type="button"
                    onClick={() => answerApproval(request.request_id, false, false)}
                    className={QUIET_LINK}
                  >
                    Deny
                  </button>
                </span>
              </div>
            );
          })}

          {reviewing.map((card) => {
            const diff = diffs[card.id];
            return (
              <button
                key={card.id}
                type="button"
                onClick={() => openReview(card.id)}
                className={cx(
                  RAIL_CARD,
                  ROW,
                  "flex w-full cursor-pointer flex-col gap-1.5 bg-transparent text-left",
                )}
              >
                <span
                  className={cx(truncate, "text-sm font-semibold text-text dark:text-text-d")}
                >
                  {card.title}
                </span>
                <span
                  className={cx(
                    mono,
                    "flex items-center gap-2 text-xs text-text3 dark:text-text3-d",
                  )}
                >
                  {diff ? (
                    <>
                      <span className="text-ok dark:text-ok-d">+{diff.added}</span>
                      <span className="text-bad dark:text-bad-d">−{diff.removed}</span>
                    </>
                  ) : (
                    <span>reading…</span>
                  )}
                  {card.id}
                  <span className="flex-1" />
                  <span className="font-sans text-xs font-semibold text-ok dark:text-ok-d">
                    Review
                  </span>
                </span>
              </button>
            );
          })}
        </motion.div>

        {/* Trabalho que não passou pelo quadro (#86). Fica onde estão as
            propostas e comporta-se como elas: o backend descobriu, disse, e a
            decisão é do operador. Não desaparece com o tempo nem com um
            evento — só o botão o tira daqui. */}
        <motion.div custom={1} variants={rowIn}>
          <Section
            title="Outside the board"
            count={String(outsideWork.length)}
            top={14}
            right={
              outsideWork.length > 0 ? (
                <span
                  title="Commits reached Relay's own repository without a Harness-Card trailer"
                  className={cx(mono, "text-xs text-text4 dark:text-text4-d")}
                >
                  his flag, your call
                </span>
              ) : undefined
            }
          />
          {outsideWork.length === 0 && !allQuiet && (
            <div className="px-1 pb-1 text-sm font-normal leading-relaxed text-text4 dark:text-text4-d">
              No warning since this window opened. The look runs at startup and
              at the day's close, over the mirror project only.
            </div>
          )}
          {outsideWork.map((seen) => {
            // The facts are the backend's `OutsideWork`, as numbers. Nothing
            // here counts commits, names files, or reads them back out of a
            // sentence — and `for_director` arrives already separated, so the
            // half written in the second person is never mistaken for an
            // instruction to the operator (#86).
            const { work, for_director: forDirector } = seen.warning;
            const shown = shownWarning === seen.id;
            const capped = work.files_total - work.files.length;
            return (
              <div
                key={seen.id}
                className={cx(RAIL_CARD, "flex flex-col gap-1.5 bg-surface dark:bg-surface-d")}
              >
                <span className="flex items-center gap-2">
                  <span className="flex text-warn dark:text-warn-d">
                    <Icon.alert />
                  </span>
                  <span
                    className={cx(
                      truncate,
                      "flex-1 text-sm font-semibold text-text dark:text-text-d",
                    )}
                  >
                    Commits without a card
                  </span>
                </span>
                {/* Os três factos que o #86 diz que um aviso tem de carregar:
                    quantos, que ficheiros, desde quando. A idade sai do
                    `since_ms` do commit mais antigo, que é um facto do
                    repositório; quando o git não datou nada, diz-se. */}
                <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
                  {plural(work.commits, "commit")} ·{" "}
                  {plural(work.files_total, "file")} ·{" "}
                  {work.since_ms ? `oldest ${ago(work.since_ms)}` : "git did not say when"}
                </span>
                {/* A lista já vem cortada pelo backend (`FILES_NAMED`), e o
                    resto conta-se, não se esconde. */}
                <span
                  className={cx(mono, "text-xs leading-[1.6] text-text2 dark:text-text2-d")}
                >
                  {work.files.length === 0 ? (
                    <span className="text-text4 dark:text-text4-d">none that git named</span>
                  ) : (
                    work.files.map((file) => (
                      <span key={file} className="block">
                        {file}
                      </span>
                    ))
                  )}
                  {capped > 0 && (
                    <span className="block text-text4 dark:text-text4-d">and {capped} more</span>
                  )}
                </span>
                {forDirector && (
                  <>
                    <button
                      type="button"
                      aria-expanded={shown}
                      onClick={() => setShownWarning(shown ? null : seen.id)}
                      className={cx(QUIET_LINK, "self-start text-left")}
                    >
                      {shown ? "hide what the Director was told" : "what the Director was told"}
                    </button>
                    {shown && (
                      <span className="text-sm font-normal leading-[1.55] text-text3 dark:text-text3-d">
                        {forDirector}
                      </span>
                    )}
                  </>
                )}
                <span className="flex items-center gap-2.5">
                  <button
                    type="button"
                    onClick={() => dismissOutsideWork(seen.id)}
                    title="Takes it off the rail. The same commits are never reported twice: the look already moved the sha it compares against."
                    className={QUIET_LINK}
                  >
                    Dismiss
                  </button>
                  <span className="flex-1" />
                  <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                    seen {clock(seen.seen_ms)}
                  </span>
                </span>
              </div>
            );
          })}
        </motion.div>

        <motion.div custom={2} variants={rowIn}>
          <Section
            title="Proposals"
            count={String(openProposals.length)}
            top={14}
            right={
              openProposals.length > 0 ? (
                <span
                  title="The Director noticed a pattern; you decide whether it becomes work"
                  className={cx(mono, "text-xs text-text4 dark:text-text4-d")}
                >
                  his call, your decision
                </span>
              ) : undefined
            }
          />
          {openProposals.length === 0 && !allQuiet && (
            <div className="px-1 pb-1 text-sm font-normal text-text4 dark:text-text4-d">
              No proposals waiting.
            </div>
          )}
          {openProposals.map((proposal) => (
            <div
              key={proposal.id}
              className={cx(RAIL_CARD, "flex flex-col gap-1.5 bg-surface dark:bg-surface-d")}
            >
              <span
                className={cx(truncate, "text-sm font-semibold text-text dark:text-text-d")}
              >
                {proposal.title}
              </span>
              <span className="text-sm font-normal leading-[1.55] text-text3 dark:text-text3-d">
                {proposal.observation}
              </span>
              <span className="text-sm font-normal leading-[1.55] text-text2 dark:text-text2-d">
                {proposal.proposal}
              </span>
              <span className="flex items-center gap-2.5">
                <button
                  type="button"
                  onClick={() => acceptProposal(proposal.id)}
                  title="Creates the card in the harness's own project — never the one you have open"
                  className="min-h-6 cursor-pointer rounded-full border-none bg-accent px-3 py-1.5 text-xs font-semibold text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px dark:bg-accent-d dark:text-onAccent-d"
                >
                  Make card in _harness
                </button>
                <button
                  type="button"
                  onClick={() => dismissProposal(proposal.id)}
                  className={QUIET_LINK}
                >
                  Dismiss
                </button>
                <span className="flex-1" />
                <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                  {clock(proposal.created_ms)}
                </span>
              </span>
            </div>
          ))}
        </motion.div>

        <motion.div custom={3} variants={rowIn}>
          <Section
            title="Running"
            count={String(runningCards.length)}
            top={14}
            right={
              <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                {money(liveSpend, 2)}
              </span>
            }
          />
          {runningCards.length === 0 && !allQuiet && (
            <div
              className={cx(RAIL_CARD, "text-sm font-normal text-text4 dark:text-text4-d")}
            >
              No run in flight.
            </div>
          )}
          {runningCards.map((card) => {
            const agent = agents.find((a) => a.id === card.agent_id);
            const t = tone(agent?.tone);
            const session = snapshot?.sessions.find((s) => s.card_id === card.id);
            const log = outputs[card.id] ?? [];
            const stream = streams[card.id];
            const doing =
              stream?.text?.slice(-90) ||
              stream?.thinking?.slice(-90) ||
              (log.length > 0 ? `${log[log.length - 1]!.label} ${log[log.length - 1]!.text}` : "starting…");
            return (
              <div
                key={card.id}
                className={cx(RAIL_CARD, "flex gap-2.5 bg-surface dark:bg-surface-d")}
              >
                <Glyph tone={t} size={26} radius={9} font={10}>
                  {agent?.initial ?? "?"}
                </Glyph>
                <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                  <div className="flex items-baseline gap-2">
                    <span className="text-md font-semibold text-text dark:text-text-d">
                      {agent?.name ?? card.agent_id}
                    </span>
                    <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                      {card.id}
                    </span>
                    <div className="flex-1" />
                    <span
                      className={cx(mono, "text-xs font-medium text-text3 dark:text-text3-d")}
                    >
                      {session ? duration(Date.now() - session.started_ms) : "—"}
                    </span>
                  </div>
                  <button
                    type="button"
                    onClick={() => openSession(card.id)}
                    className="cursor-pointer rounded-sm border-none bg-transparent text-left text-sm font-normal leading-snug text-text2 transition-colors duration-150 hover:text-text dark:text-text2-d dark:hover:text-text-d"
                  >
                    {card.title}
                  </button>
                  <span className={cx(mono, truncate, "text-xs", t.fg)}>{doing}</span>
                  <span
                    className={cx(
                      mono,
                      "flex items-center gap-2 text-xs text-text3 dark:text-text3-d",
                    )}
                  >
                    {plural(card.turns, "turn")} · {money(card.cost_usd, 2)}
                    <span className="flex-1" />
                    <button
                      type="button"
                      onClick={() => cancelRun(card.id)}
                      className={cx(mono, QUIET_LINK, "text-text3 dark:text-text3-d")}
                    >
                      stop
                    </button>
                  </span>
                </div>
              </div>
            );
          })}
        </motion.div>

        <motion.div custom={4} variants={rowIn}>
          <Section
            title="Done today"
            count={String(stats?.done_today ?? doneToday.length)}
            top={14}
            right={
              <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                {money(stats?.spend_today ?? 0)}
              </span>
            }
          />
          {doneToday.length === 0 && !allQuiet && (
            <div className="px-1 pb-1 text-sm font-normal text-text4 dark:text-text4-d">
              Nothing approved yet today.
            </div>
          )}
          {doneToday.slice(0, 8).map((row) => {
            const card = cards.find((c) => c.id === row.card_id);
            const agent = agents.find((a) => a.id === card?.agent_id);
            const t = tone(agent?.tone);
            return (
              <button
                key={row.seq}
                type="button"
                onClick={() => openSession(row.card_id)}
                className={cx(
                  ROW,
                  "flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent p-2 text-left",
                )}
              >
                <Glyph tone={t} size={20} radius={7} font={8.5}>
                  {agent?.initial ?? "·"}
                </Glyph>
                <span
                  className={cx(
                    truncate,
                    "flex-1 text-sm font-normal text-text2 dark:text-text2-d",
                  )}
                >
                  {card?.title ?? row.card_id}
                </span>
                <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                  {clock(row.ts_ms)}
                </span>
              </button>
            );
          })}
        </motion.div>

        <motion.div custom={5} variants={rowIn}>
          <Section
            title="Worktrees"
            count={String(trees.length)}
            top={16}
            right={
              <button
                type="button"
                onClick={openTrees}
                className="min-h-6 cursor-pointer rounded-sm border-none bg-transparent text-xs font-normal text-text4 transition-colors duration-150 hover:text-text dark:text-text4-d dark:hover:text-text-d"
              >
                manage
              </button>
            }
          />
          <div className="flex flex-col gap-px">
            {trees.length === 0 && (
              <span className="px-1 text-sm font-normal text-text4 dark:text-text4-d">
                No worktree has been created yet.
              </span>
            )}
            {trees.map((t) => (
              <button
                key={t.path}
                type="button"
                onClick={() => api.reveal(t.path).catch(() => {})}
                className={cx(
                  ROW,
                  "flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent p-2 text-left",
                )}
              >
                <span
                  className={cx(
                    mono,
                    truncate,
                    "flex-1 text-sm font-medium text-text2 dark:text-text2-d",
                  )}
                >
                  {t.branch ?? t.path.split(/[\\/]/).pop()}
                </span>
                <span
                  className={cx(
                    "text-xs font-normal",
                    t.dirty ? "text-warn dark:text-warn-d" : "text-text3 dark:text-text3-d",
                  )}
                >
                  {t.bare ? "main" : t.dirty ? "dirty" : "clean"}
                </span>
              </button>
            ))}
          </div>
        </motion.div>
      </motion.div>
    </motion.div>
  );
}
