/** Worktrees — os checkouts que existem agora, e a maneira de os largar.
 *
 *  Vinha do `Misc.tsx`, que era "Worktrees, Activity e Settings": três ecrãs
 *  da barra lateral, três destinos, zero estado partilhado. O que os juntava
 *  eram quatro `const` de classes, e essas são agora do `ui.tsx`.
 */

import { useEffect, useState } from "react";
import { api, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { TONE, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { DANGER, HOVER_ROW, Loading, PANEL, QUIET, truncate } from "../components/ui";

export function Worktrees() {
  const { projectId, project, snapshot, toast } = useStore();
  const [rows, setRows] = useState<WorktreeRow[] | null>(null);

  const load = () => {
    if (!projectId) return;
    api
      .worktrees(projectId)
      .then(setRows)
      .catch((e) => toast("bad", "Could not list worktrees", reason(e)));
  };

  useEffect(load, [projectId]);

  if (!project) {
    return (
      <div className="px-6.5 py-5.5 text-md text-text3 dark:text-text3-d">
        Add a git repository first.
      </div>
    );
  }
  if (!rows) return <Loading what="Listing worktrees" />;

  const grid = "grid grid-cols-[1.5fr_1fr_90px_1.4fr_150px] gap-3.5";
  const cardFor = (branch: string | null) => {
    const id = branch?.split("/").slice(-1)[0] ?? "";
    return snapshot?.cards.find((c) => c.id === id) ?? null;
  };

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <p className="mb-4 mt-0 text-md text-text2 dark:text-text2-d">
        One branch per card, created under app data. Finished runs commit themselves and leave a
        trailer pointing back at the card.
      </p>
      <div className={PANEL}>
        <div
          className={cx(
            grid,
            "border-b border-line px-4.5 py-3 text-sm font-bold uppercase tracking-[.08em] text-text3 dark:border-line-d dark:text-text3-d",
          )}
        >
          <span>Branch</span>
          <span>Card</span>
          <span>State</span>
          <span>Path</span>
          <span />
        </div>
        {rows.map((w) => {
          const card = cardFor(w.branch);
          const st = w.dirty
            ? { label: "dirty", tone: TONE.accent }
            : { label: "clean", tone: TONE.ok };
          return (
            <div
              key={w.path}
              className={cx(
                grid,
                HOVER_ROW,
                "items-center border-b border-line2 px-4.5 py-3 dark:border-line2-d",
              )}
            >
              <span className={cx(truncate, "font-mono text-md font-medium")}>
                {w.branch ?? "(detached)"}
              </span>
              <span
                title={card?.title}
                className={cx(truncate, "font-mono text-sm text-text3 dark:text-text3-d")}
              >
                {card?.id ?? "—"}
              </span>
              <span
                className={cx(
                  "justify-self-start rounded-full px-2.5 py-1 text-sm font-bold",
                  st.tone.soft,
                  st.tone.fg,
                )}
              >
                {st.label}
              </span>
              <span title={w.path} className={cx(truncate, "text-md text-text2 dark:text-text2-d")}>
                {w.path}
              </span>
              <span className="flex justify-self-end gap-1.5">
                <button
                  type="button"
                  onClick={() => api.reveal(w.path).catch(() => {})}
                  className={cx(QUIET, "px-3.5 py-1.5 text-sm")}
                >
                  Open
                </button>
                <button
                  type="button"
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .removeWorktree(projectId, w.path)
                      .then(() => {
                        toast("ok", "Removed", w.branch ?? w.path);
                        load();
                      })
                      .catch((e) => toast("bad", "Could not remove it", reason(e)));
                  }}
                  className={cx(DANGER, "px-3.5 py-1.5 text-sm")}
                >
                  Drop
                </button>
              </span>
            </div>
          );
        })}
        {rows.length === 0 && (
          <div className="px-4.5 py-5.5 text-center text-md text-text3 dark:text-text3-d">
            No worktrees in this project. Agents open one the moment a card starts.
          </div>
        )}
      </div>
    </div>
  );
}
