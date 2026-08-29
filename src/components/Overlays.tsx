/** Everything that floats above the shell: toasts, the permission sheet, the
 *  send-back sheet and the command palette. The conversation itself is a
 *  screen now, not an overlay. */

import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ago } from "../lib/format";
import { cx } from "../lib/cx";
import { sheetIn, toastIn, veil } from "../lib/motion";
import { TONE, tone, type Tone, type ToneName } from "../lib/types";
import { useStore } from "../state/store";
import { truncate } from "./ui";

/** O véu por trás de uma folha: escurece e desfoca o que está atrás. */
const SCRIM =
  "absolute inset-0 z-[80] flex items-center justify-center bg-[rgba(18,18,26,.42)] backdrop-blur-[4px]";

/** A folha em si. */
const SHEET =
  "rounded-xl border border-line bg-elev p-6 shadow-soft dark:border-line-d dark:bg-elev-d dark:shadow-soft-d";

export function Toasts() {
  const { toasts, dismissToast } = useStore();
  return (
    // Positioned by the shell's floating corner. The margin only exists when
    // there is a toast to hold off the sheet below.
    <div
      className={cx(
        "pointer-events-auto flex flex-col items-end gap-2.5",
        toasts.length > 0 && "mb-2.5",
      )}
    >
      <AnimatePresence>
        {toasts.map((t) => {
          const dot = TONE[t.tone as ToneName] ?? TONE.accent;
          return (
            <motion.button
              key={t.id}
              layout
              variants={toastIn}
              initial="hidden"
              animate="shown"
              exit="gone"
              type="button"
              onClick={() => dismissToast(t.id)}
              className="flex w-[250px] max-w-[330px] cursor-pointer gap-3 rounded-lg border border-line bg-elev px-4 py-3.5 text-left shadow-soft transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:bg-elev-d dark:shadow-soft-d dark:hover:bg-hovered-d"
            >
              <span
                className={cx("mt-1.5 h-1.75 w-1.75 flex-none rounded-full", dot.solid)}
              />
              <div className="min-w-0">
                <div className="mb-1 text-md font-bold">{t.title}</div>
                {t.body && (
                  <div className="text-md leading-normal text-text3 dark:text-text3-d">
                    {t.body}
                  </div>
                )}
              </div>
            </motion.button>
          );
        })}
      </AnimatePresence>
    </div>
  );
}

export function ApprovalSheet({ close }: { close: () => void }) {
  const { approvals, answerApproval, agents, snapshot, projects } = useStore();
  const [always, setAlways] = useState(false);
  const request = approvals[0];

  useEffect(() => {
    setAlways(false);
  }, [request?.request_id]);

  if (!request) return null;

  const card = snapshot?.cards.find((c) => c.id === request.card_id);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const project = projects.find((p) => p.id === request.project_id);
  const t: Tone = tone(agent?.tone ?? "warn");

  const answer = (allow: boolean) => {
    answerApproval(request.request_id, allow, allow && always);
    if (approvals.length <= 1) close();
  };

  return (
    <motion.div
      variants={veil}
      initial="hidden"
      animate="shown"
      exit="gone"
      className={SCRIM}
      onClick={close}
    >
      <motion.div
        variants={sheetIn}
        initial="hidden"
        animate="shown"
        exit="gone"
        onClick={(e) => e.stopPropagation()}
        className={cx(SHEET, "w-[490px]")}
      >
        <div className="mb-4 flex items-center gap-3">
          <span
            className={cx(
              "flex h-[38px] w-[38px] items-center justify-center rounded-full text-md font-bold",
              t.soft,
              t.fg,
            )}
          >
            {agent?.initial ?? "?"}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-lg font-bold">
              {agent?.name ?? "An agent"} is asking
            </span>
            <span className="mt-1 block font-mono text-sm text-text3 dark:text-text3-d">
              {request.card_id ?? "—"} · paused · {project?.name ?? request.project_id}
            </span>
          </span>
          <span className="text-sm text-text3 dark:text-text3-d">{ago(request.asked_ms)}</span>
        </div>

        <div className="mb-1.5 text-[20px] font-extrabold tracking-[-.02em]">
          {card?.title ?? `Permission for ${request.tool}`}
        </div>
        <div className="mb-3.5 inline-block rounded-sm bg-warnSoft px-3 py-1.5 font-mono text-md text-warn dark:bg-warnSoft-d dark:text-warn-d">
          {request.tool}
        </div>
        <pre className="mx-0 mb-3.5 mt-0 max-h-[160px] overflow-auto whitespace-pre-wrap rounded-lg bg-surface2 px-4 py-3.5 font-sans text-md leading-[1.65] text-text2 dark:bg-surface2-d dark:text-text2-d">
          {request.summary ||
            "The agent asked to use a tool outside its permissions. No details were given."}
        </pre>

        <div className="mb-4 flex items-center gap-2.5">
          <button
            type="button"
            role="checkbox"
            aria-checked={always}
            aria-label={`Stop asking me about ${request.tool}`}
            onClick={() => setAlways((v) => !v)}
            className={cx(
              "flex h-4.5 w-4.5 flex-none cursor-pointer items-center justify-center rounded-sm border border-line text-xs leading-none text-onAccent transition-colors duration-150 dark:border-line-d dark:text-onAccent-d",
              always ? "bg-accent dark:bg-accent-d" : "bg-transparent",
            )}
          >
            {always ? "✓" : ""}
          </button>
          <span className="text-md text-text2 dark:text-text2-d">
            Stop asking me about {request.tool}
          </span>
        </div>

        <div className="flex gap-2.5">
          <button
            type="button"
            onClick={() => answer(true)}
            className="min-h-6 flex-1 cursor-pointer rounded-full border-none bg-accent p-3 text-lg font-bold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-accent-d dark:text-onAccent-d"
          >
            {always ? "Allow from now on" : "Allow once"}
          </button>
          <button
            type="button"
            onClick={() => answer(false)}
            className="min-h-6 flex-1 cursor-pointer rounded-full border border-line bg-transparent p-3 text-lg font-semibold transition-colors duration-150 hover:border-transparent hover:bg-badSoft hover:text-bad dark:border-line-d dark:hover:bg-badSoft-d dark:hover:text-bad-d"
          >
            Deny
          </button>
        </div>
        {approvals.length > 1 && (
          <div className="mt-3 text-center text-sm text-text3 dark:text-text3-d">
            {approvals.length - 1} more waiting after this one
          </div>
        )}
      </motion.div>
    </motion.div>
  );
}

export function RejectSheet({ cardId, close }: { cardId: string | null; close: () => void }) {
  const { snapshot, reject } = useStore();
  const [why, setWhy] = useState("");
  const card = snapshot?.cards.find((c) => c.id === cardId);

  useEffect(() => {
    setWhy("");
  }, [cardId]);

  if (!cardId) return null;

  const send = () => {
    reject(cardId, why);
    close();
  };

  return (
    <motion.div
      variants={veil}
      initial="hidden"
      animate="shown"
      exit="gone"
      className={SCRIM}
      onClick={close}
    >
      <motion.div
        variants={sheetIn}
        initial="hidden"
        animate="shown"
        exit="gone"
        onClick={(e) => e.stopPropagation()}
        className={cx(SHEET, "w-[440px]")}
      >
        <div className="mb-1.5 text-[20px] font-extrabold tracking-[-.02em]">Send it back</div>
        <p className="mx-0 mb-3.5 mt-0 text-md leading-[1.55] text-text2 dark:text-text2-d">
          {card?.title ?? cardId}
        </p>
        <textarea
          rows={3}
          autoFocus
          value={why}
          onChange={(e) => setWhy(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) send();
          }}
          aria-label="What has to change?"
          placeholder="What has to change? The agent gets this verbatim."
          className="w-full resize-none rounded-lg border border-line bg-surface2 px-4 py-3.5 text-md leading-relaxed outline-none transition-colors duration-150 hover:border-accentLine focus-visible:border-accentLine dark:border-line-d dark:bg-surface2-d dark:hover:border-accentLine-d dark:focus-visible:border-accentLine-d"
        />
        <div className="mt-3.5 flex gap-2.5">
          <button
            type="button"
            onClick={send}
            className={cx(
              "min-h-6 flex-1 cursor-pointer rounded-full border-none bg-bad p-3 text-lg font-bold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-bad-d dark:text-onAccent-d",
              why.trim() ? "opacity-100" : "opacity-65",
            )}
          >
            Send back with reason
          </button>
          <button
            type="button"
            onClick={close}
            className="min-h-6 cursor-pointer rounded-full border border-line bg-transparent px-4.5 py-3 text-lg font-semibold text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d"
          >
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

export interface PaletteAction {
  name: string;
  hint: string;
  /** O tom do ponto à esquerda — já resolvido em classes. */
  tone: Tone;
  run: () => void;
}

export function CommandPalette({
  open,
  close,
  actions,
}: {
  open: boolean;
  close: () => void;
  actions: PaletteAction[];
}) {
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const input = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setQ("");
    setCursor(0);
    const t = window.setTimeout(() => input.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, [open]);

  const hits = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return actions
      .filter(
        (a) =>
          !needle ||
          a.name.toLowerCase().includes(needle) ||
          a.hint.toLowerCase().includes(needle),
      )
      .slice(0, 9);
  }, [actions, q]);

  const pick = (i: number) => {
    const action = hits[i];
    if (!action) return;
    close();
    action.run();
  };

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          variants={veil}
          initial="hidden"
          animate="shown"
          exit="gone"
          onClick={close}
          className="absolute inset-0 z-[90] flex items-start justify-center bg-[rgba(18,18,26,.38)] pt-[100px] backdrop-blur-[4px]"
        >
          <motion.div
            variants={sheetIn}
            initial="hidden"
            animate="shown"
            exit="gone"
            onClick={(e) => e.stopPropagation()}
            className="w-[560px] overflow-hidden rounded-xl border border-line bg-elev shadow-soft dark:border-line-d dark:bg-elev-d dark:shadow-soft-d"
          >
            <input
              ref={input}
              value={q}
              onChange={(e) => {
                setQ(e.target.value);
                setCursor(0);
              }}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setCursor((c) => Math.min(hits.length - 1, c + 1));
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setCursor((c) => Math.max(0, c - 1));
                } else if (e.key === "Enter") {
                  e.preventDefault();
                  pick(cursor);
                } else if (e.key === "Escape") {
                  close();
                }
              }}
              aria-label="Search cards, sessions, agents"
              placeholder="Search cards, sessions, agents…"
              className="w-full border-none border-b border-line bg-transparent px-5 py-4.5 text-lg outline-none dark:border-line-d"
            />
            <div className="max-h-[320px] overflow-y-auto p-2">
              {hits.map((a, i) => (
                <button
                  key={`${a.name}-${i}`}
                  type="button"
                  onMouseEnter={() => setCursor(i)}
                  onClick={() => pick(i)}
                  className={cx(
                    "flex w-full cursor-pointer items-center gap-3 rounded-md border-none px-3 py-2.5 text-left text-lg text-text transition-colors duration-150 dark:text-text-d",
                    i === cursor
                      ? "bg-hovered dark:bg-hovered-d"
                      : "bg-transparent",
                  )}
                >
                  <span className={cx("h-1.75 w-1.75 flex-none rounded-full", a.tone.solid)} />
                  <span className={cx(truncate, "flex-1 font-medium")}>{a.name}</span>
                  <span className="text-sm text-text3 dark:text-text3-d">{a.hint}</span>
                </button>
              ))}
              {hits.length === 0 && (
                <div className="p-6 text-center text-md text-text3 dark:text-text3-d">
                  No matches
                </div>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
