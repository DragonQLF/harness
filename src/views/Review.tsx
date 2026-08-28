/** The review screen: one finished run, what it changed, and the two things you
 *  can do about it. Relay never merges — approving moves the card, the branch
 *  and its worktree stay where they are. */

import { useEffect, useMemo, useState } from "react";
import { motion } from "motion/react";
import { ago, money, plural } from "../lib/format";
import { cx } from "../lib/cx";
import { paneIn } from "../lib/motion";
import { MODELS, tone } from "../lib/types";
import type { QueueRow } from "../lib/types";
import { api } from "../lib/ipc";
import { useStore } from "../state/store";
import { Eyebrow, Glyph, mono, truncate } from "../components/ui";

/** One line of a patch, coloured by what it does to the file. */
function classify(text: string): string {
  if (text.startsWith("+++") || text.startsWith("---"))
    return "text-text4 dark:text-text4-d";
  if (text.startsWith("+")) return "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d";
  if (text.startsWith("-")) return "bg-badSoft text-bad2 dark:bg-badSoft-d dark:text-bad2-d";
  if (text.startsWith("@@") || text.startsWith("diff --git") || text.startsWith("index "))
    return "text-text4 dark:text-text4-d";
  return "text-text3 dark:text-text3-d";
}

/** A patch split into files, so a large change can be walked one file at a time. */
type FilePatch = { path: string; added: number; removed: number; lines: string[] };

function splitFiles(patch: string): FilePatch[] {
  const files: FilePatch[] = [];
  let current: FilePatch | null = null;
  for (const text of patch.split("\n")) {
    if (text.startsWith("diff --git ")) {
      const match = /^diff --git a\/(.*) b\/(.*)$/.exec(text);
      current = {
        path: match ? match[2] : text.slice("diff --git ".length),
        added: 0,
        removed: 0,
        lines: [],
      };
      files.push(current);
      continue;
    }
    // Lines before the first header are mail-format noise; drop them.
    if (!current) continue;
    current.lines.push(text);
    if (text.startsWith("+") && !text.startsWith("+++")) current.added++;
    else if (text.startsWith("-") && !text.startsWith("---")) current.removed++;
  }
  return files;
}

/** One changed file: a header you can collapse, and the hunks under it. */
function FileSection({ file }: { file: FilePatch }) {
  const [open, setOpen] = useState(true);
  return (
    <div className="mb-2.5">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="sticky top-0 flex w-full cursor-pointer items-center gap-2 border-y border-line bg-bg px-4.5 py-1.5 text-left transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:bg-bg-d dark:hover:bg-hovered-d"
      >
        <span className={cx(mono, "w-2.5 text-xs text-text4 dark:text-text4-d")}>
          {open ? "▾" : "▸"}
        </span>
        <span
          title={file.path}
          className={cx(mono, truncate, "flex-1 text-sm font-semibold text-text2 dark:text-text2-d")}
        >
          {file.path}
        </span>
        <span className={cx(mono, "text-xs font-medium text-ok dark:text-ok-d")}>
          +{file.added}
        </span>
        <span className={cx(mono, "text-xs font-medium text-bad dark:text-bad-d")}>
          −{file.removed}
        </span>
      </button>
      {open &&
        file.lines.map((text, i) => (
          <div
            key={i}
            className={cx(
              mono,
              classify(text),
              "whitespace-pre px-4.5 text-md leading-[1.85]",
            )}
          >
            {text}
          </div>
        ))}
    </div>
  );
}

export function Review({
  selected,
  select,
}: {
  selected: string | null;
  select: (cardId: string) => void;
}) {
  const {
    snapshot,
    agents,
    project,
    activity,
    diffs,
    loadCardDiff,
    approve,
    reject,
    toast,
  } = useStore();

  const [why, setWhy] = useState("");
  const [queue, setQueue] = useState<QueueRow[]>([]);
  const cards = useMemo(
    () => (snapshot?.cards ?? []).filter((c) => c.status === "review"),
    [snapshot],
  );
  // The Triador's ordering: widest surface and longest wait first.
  useEffect(() => {
    if (!project?.id) return;
    let alive = true;
    api
      .reviewQueue(project.id)
      .then((rows) => alive && setQueue(rows))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [project?.id, cards.length]);
  const riskOf = (id: string) => queue.find((q) => q.card_id === id)?.risk ?? -1;
  const sorted = useMemo(
    () => [...cards].sort((a, b) => riskOf(b.id) - riskOf(a.id)),
    [cards, queue],
  );
  const card = sorted.find((c) => c.id === selected) ?? sorted[0] ?? null;
  const diff = card ? diffs[card.id] : undefined;

  useEffect(() => {
    if (card) loadCardDiff(card.id);
  }, [card?.id, loadCardDiff]);

  useEffect(() => setWhy(""), [card?.id]);

  const files = useMemo(() => (diff ? splitFiles(diff.patch) : []), [diff]);

  if (!card) {
    return (
      <motion.div
        variants={paneIn}
        initial="hidden"
        animate="shown"
        className="grid flex-1 place-items-center p-10"
      >
        <div className="max-w-[420px] text-center">
          <div className="mb-2 text-xl font-semibold text-text dark:text-text-d">
            Nothing is waiting for you
          </div>
          <div className="text-md font-normal leading-[1.7] text-text3 dark:text-text3-d">
            A run that finishes lands here once its reviewer has read it. Until then the board is the
            place to look.
          </div>
        </div>
      </motion.div>
    );
  }

  const agent = agents.find((a) => a.id === card.agent_id);
  const t = tone(agent?.tone);
  const session = snapshot?.sessions.find((s) => s.card_id === card.id);
  const history = activity.filter((a) => a.card_id === card.id).slice(0, 8);

  const facts: { k: string; v: string }[] = [
    { k: "branch", v: diff?.branch ?? session?.branch ?? "no branch yet" },
    { k: "worktree", v: diff?.worktree ?? session?.worktree ?? "none" },
    { k: "session", v: (diff?.session_id ?? card.session_id ?? "none").slice(0, 16) },
    { k: "base", v: diff?.base ?? project?.base_branch ?? "—" },
    { k: "files", v: diff ? plural(diff.files.length, "file") : "reading…" },
    {
      k: "reviewer",
      v: agent?.reviewer === "director" ? "the Director, then you" : agent?.reviewer === "human" ? "you" : "nobody",
    },
  ];

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] overflow-hidden"
    >
      <div className="flex items-center gap-3 border-b border-line px-5 pb-3 pt-3.5 dark:border-line-d">
        <Glyph tone={t} size={24} radius={8} font={10}>
          {agent?.initial ?? "?"}
        </Glyph>
        <div className="min-w-0 flex-1">
          <div className={cx(truncate, "text-lg font-semibold text-text dark:text-text-d")}>
            {card.title}
          </div>
          <div className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
            {card.id} · {agent?.name ?? card.agent_id} · {plural(card.turns, "turn")} ·{" "}
            {plural(card.runs, "run")} · {money(card.cost_usd, 4)}
          </div>
        </div>
        {sorted.length > 1 && (
          <div className="flex gap-1.5">
            {sorted.map((c) => {
              const r = riskOf(c.id);
              const on = c.id === card.id;
              return (
                <button
                  key={c.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() => select(c.id)}
                  title={r >= 0 ? `triage risk ${r}` : undefined}
                  className={cx(
                    mono,
                    "inline-flex min-h-6 cursor-pointer items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-colors duration-150",
                    on
                      ? "border-accentLine bg-accentSoft text-accent2 dark:border-accentLine-d dark:bg-accentSoft-d dark:text-accent2-d"
                      : "border-line3 bg-transparent text-text3 hover:bg-hovered hover:text-text dark:border-line3-d dark:text-text3-d dark:hover:bg-hovered-d dark:hover:text-text-d",
                  )}
                >
                  {c.id}
                  {queue.length > 0 && r >= 0 && (
                    <b
                      className={cx(
                        "font-semibold",
                        r > 40 ? "text-warn dark:text-warn-d" : "text-text4 dark:text-text4-d",
                      )}
                    >
                      {r}
                    </b>
                  )}
                </button>
              );
            })}
          </div>
        )}
        <span className={cx(mono, "text-sm font-medium text-ok dark:text-ok-d")}>
          +{diff?.added ?? 0}
        </span>
        <span className={cx(mono, "text-sm font-medium text-bad dark:text-bad-d")}>
          −{diff?.removed ?? 0}
        </span>
      </div>

      <div className="grid min-h-0 grid-cols-[264px_minmax(0,1fr)] overflow-hidden">
        <div className="flex min-h-0 flex-col overflow-y-auto border-r border-line dark:border-line-d">
          <div className="px-4 pb-5 pt-3.5">
            {card.last_review && (
              <div className="rounded-md border border-line2 bg-surface p-3 dark:border-line2-d dark:bg-surface-d">
                <div className="flex items-center gap-2 pb-1.5">
                  {/* A marca do revisor é identidade, e o degradê não é um tom:
                      fica escrito à mão, como estava. */}
                  <Glyph
                    className="bg-[linear-gradient(140deg,#b5751a,#e0854a)] text-onAccent dark:bg-[linear-gradient(140deg,#ffb35c,#e0854a)] dark:text-onAccent-d"
                    size={18}
                    radius={6}
                    font={8.5}
                  >
                    {card.last_review.by === "director" ? "D" : "Y"}
                  </Glyph>
                  <span className="text-sm font-semibold text-text dark:text-text-d">
                    {card.last_review.by === "director" ? "The Director" : "You"}{" "}
                    {card.last_review.approved ? "approved it" : "sent it back"}
                  </span>
                </div>
                <div className="text-sm font-normal leading-[1.55] text-text2 dark:text-text2-d">
                  {card.last_review.reason || "no reason recorded"}
                </div>
              </div>
            )}

            <Eyebrow className="block px-0.5 pb-1.5 pt-3.5">WHERE IT LIVES</Eyebrow>
            {facts.map((f) => (
              <div key={f.k} className="flex items-baseline gap-2 px-0.5 py-1">
                <span className="w-[74px] flex-none text-xs font-normal text-text4 dark:text-text4-d">
                  {f.k}
                </span>
                <span
                  title={f.v}
                  className={cx(
                    mono,
                    truncate,
                    "flex-1 text-xs font-medium text-text2 dark:text-text2-d",
                  )}
                >
                  {f.v}
                </span>
              </div>
            ))}

            <Eyebrow className="block px-0.5 pb-1.5 pt-4">HISTORY</Eyebrow>
            {history.length === 0 && (
              <div className="text-sm font-normal text-text4 dark:text-text4-d">
                Nothing recorded for this card yet.
              </div>
            )}
            {history.map((row) => {
              const dot =
                row.kind === "review"
                  ? row.label.startsWith("Approved")
                    ? "bg-ok dark:bg-ok-d"
                    : "bg-warn dark:bg-warn-d"
                  : row.kind === "run"
                    ? "bg-accent dark:bg-accent-d"
                    : "bg-line4 dark:bg-line4-d";
              return (
                <div key={row.seq} className="flex gap-2.5 px-0.5 py-1">
                  <span className={cx("mt-1.5 h-1.25 w-1.25 flex-none rounded-full", dot)} />
                  <span className="flex-1 text-sm font-normal leading-normal text-text2 dark:text-text2-d">
                    {row.label}
                    {row.detail ? ` — ${row.detail}` : ""}
                    <span className="text-text4 dark:text-text4-d"> · {ago(row.ts_ms)}</span>
                  </span>
                </div>
              );
            })}

            <Eyebrow className="block px-0.5 pb-1.5 pt-4">MODEL</Eyebrow>
            <div className="text-sm font-normal leading-relaxed text-text2 dark:text-text2-d">
              {MODELS.find((m) => m.id === agent?.model)?.name ?? "auto"} ·{" "}
              {agent?.expected_output || "no expected output set"}
            </div>
          </div>
        </div>

        <div className="grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto] overflow-hidden">
          <div className="min-h-0 animate-[fadeIn_.5s_ease_.1s_both] overflow-y-auto py-3.5 [scrollbar-gutter:stable]">
            <div className={cx(mono, "px-4.5 pb-2.5 text-sm text-text4 dark:text-text4-d")}>
              git diff {diff?.base ?? project?.base_branch ?? "main"}…
              {diff?.branch ?? session?.branch ?? "the worktree"}
            </div>
            {!diff && (
              <div className={cx(mono, "px-4.5 text-md text-text4 dark:text-text4-d")}>
                reading the worktree…
              </div>
            )}
            {diff && diff.patch.trim() === "" && (
              <div className="px-4.5 text-md font-normal leading-[1.7] text-text3 dark:text-text3-d">
                This card changed nothing against {diff.base}. Either the run wrote nothing, or its
                work was already on the base branch.
              </div>
            )}
            {files.map((file, i) => (
              <FileSection key={`${i}:${file.path}`} file={file} />
            ))}
          </div>

          <div className="flex flex-col gap-2.5 border-t border-line px-4.5 pb-3.5 pt-3 dark:border-line-d">
            <div className="flex items-center gap-2.5 rounded-md border border-line3 bg-surface px-3 py-2.5 focus-within:border-accentLine dark:border-line3-d dark:bg-surface-d dark:focus-within:border-accentLine-d">
              <input
                value={why}
                onChange={(e) => setWhy(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && why.trim()) reject(card.id, why);
                }}
                aria-label="Why it is going back"
                placeholder="Why it is going back — the agent gets this as its next instruction…"
                className="min-w-0 flex-1 border-none bg-transparent text-md font-normal text-text outline-none dark:text-text-d"
              />
            </div>
            <div className="flex items-center gap-2.5">
              <span className="max-w-[430px] flex-1 text-sm font-normal leading-[1.45] text-text4 dark:text-text4-d">
                Approving moves the card to Done. Relay does not merge:{" "}
                <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
                  {diff?.branch ?? session?.branch ?? "the branch"}
                </span>{" "}
                and its worktree stay until you remove them.
              </span>
              <button
                type="button"
                onClick={() => {
                  if (!why.trim()) {
                    toast(
                      "warn",
                      "Say why first",
                      "The reason is what the agent gets as its next instruction.",
                    );
                    return;
                  }
                  reject(card.id, why);
                }}
                className="min-h-6 cursor-pointer rounded-sm border border-warn bg-transparent px-3.5 py-2 text-md font-semibold text-warn transition-colors duration-150 hover:bg-warnSoft dark:border-warn-d dark:text-warn-d dark:hover:bg-warnSoft-d"
              >
                Send back
              </button>
              <button
                type="button"
                onClick={() => approve(card.id)}
                className="min-h-6 cursor-pointer rounded-sm border-none bg-ok px-4 py-2 text-md font-semibold text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px dark:bg-ok-d dark:text-onAccent-d"
              >
                Approve
              </button>
            </div>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
