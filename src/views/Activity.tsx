/** Activity — o log de eventos do projecto, filtrável, o mais recente primeiro.
 *
 *  Saiu do `Misc.tsx` pela mesma razão que os outros dois.
 */

import { Fragment, useState } from "react";
import { cx } from "../lib/cx";
import { clock } from "../lib/format";
import { TONE } from "../lib/types";
import { useStore } from "../state/store";
import { CHOICE, CHOICE_OFF, CHOICE_ON, HOVER_ROW, PANEL, tabular, truncate } from "../components/ui";

const FILTERS = ["All", "Cards", "Runs", "Reviews"] as const;

export function Activity({ openRun }: { openRun: (cardId: string) => void }) {
  const { activity, snapshot, project } = useStore();
  const [filter, setFilter] = useState<(typeof FILTERS)[number]>("All");

  if (!project) {
    return (
      <div className="px-6.5 py-5.5 text-md text-text3 dark:text-text3-d">
        Add a git repository first.
      </div>
    );
  }

  const rows = activity.filter((r) =>
    filter === "All"
      ? true
      : filter === "Cards"
        ? r.kind === "card"
        : filter === "Runs"
          ? r.kind === "run"
          : r.kind === "review" || r.kind === "approval",
  );

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <div className="mb-3.5 flex items-center gap-3.5">
        {/* The chrome above already names the screen and what it lists. Every
            other view leaves the heading to it; this one said it twice. */}
        <div className="flex-1" />
        <div className="flex gap-0.5 rounded-full border border-line bg-surface p-1 dark:border-line-d dark:bg-surface-d">
          {FILTERS.map((f) => {
            const on = filter === f;
            return (
              <button
                key={f}
                type="button"
                aria-pressed={on}
                onClick={() => setFilter(f)}
                className={cx(CHOICE, "px-4 py-2 text-md", on ? CHOICE_ON : CHOICE_OFF)}
              >
                {f}
              </button>
            );
          })}
        </div>
      </div>

      <div className={PANEL}>
        {rows.map((e, i) => {
          // Events written before the envelope carried a timestamp deserialize
          // to zero. Dating them 1 January 1970 is a confident wrong answer;
          // saying they predate the record is the true one.
          const undated = !e.ts_ms;
          const day = undated ? "undated" : new Date(e.ts_ms).toDateString();
          const prev = rows[i - 1];
          const prevDay = !prev ? null : !prev.ts_ms ? "undated" : new Date(prev.ts_ms).toDateString();
          const fresh = day !== prevDay;
          const today = new Date().toDateString();
          const dot =
            e.kind === "run"
              ? TONE.accent
              : e.kind === "approval"
                ? TONE.warn
                : e.kind === "review"
                  ? TONE.ok
                  : TONE.info;
          return (
            <Fragment key={e.seq}>
              {fresh && (
                <div className="border-b border-line2 bg-recess px-4.5 pb-2 pt-2.5 text-xs font-semibold tracking-[.08em] text-text4 dark:border-line2-d dark:bg-recess-d dark:text-text4-d">
                  {undated
                    ? "BEFORE TIMES WERE RECORDED"
                    : day === today
                      ? "TODAY"
                      : new Date(e.ts_ms)
                          .toLocaleDateString(undefined, { day: "numeric", month: "long" })
                          .toUpperCase()}
                </div>
              )}
              <button
                key={e.seq}
                type="button"
                onClick={() => openRun(e.card_id)}
                className={cx(
                  HOVER_ROW,
                  "grid w-full animate-[fadeIn_.25s_ease_both] cursor-pointer grid-cols-[14px_190px_74px_1fr_60px] items-center gap-3.5 border-b border-line2 bg-transparent px-4.5 py-3 text-left text-md text-text dark:border-line2-d dark:text-text-d",
                )}
              >
                <span className={cx("h-1.75 w-1.75 rounded-full", dot.solid)} />
                <span className={cx(truncate, "font-semibold")}>{e.label}</span>
                <span
                  title={e.card_id}
                  className={cx(truncate, "font-mono text-sm text-text3 dark:text-text3-d")}
                >
                  {e.card_id}
                </span>
                <span className={cx(truncate, "text-text2 dark:text-text2-d")}>
                  {e.detail || snapshot?.cards.find((c) => c.id === e.card_id)?.title || ""}
                </span>
                <span
                  className={cx(tabular, "text-right text-sm text-text3 dark:text-text3-d")}
                >
                  {undated ? "—" : clock(e.ts_ms)}
                </span>
              </button>
            </Fragment>
          );
        })}
        {rows.length === 0 && (
          <div className="px-4.5 py-5.5 text-center text-md text-text3 dark:text-text3-d">
            Nothing logged yet. Every card created, moved, run or reviewed in this
            project lands here, newest first.
          </div>
        )}
      </div>
    </div>
  );
}
