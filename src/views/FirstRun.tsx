/** What Relay shows before it has a project: the two ways in, and the
 *  Director, who can be asked what to do first. Never a dead end. */

import { useState } from "react";
import { greeting } from "../lib/format";
import { cx } from "../lib/cx";
import { useStore } from "../state/store";
import { tabular, truncate } from "../components/ui";

/** Um dos três painéis brancos deste ecrã. */
const PANEL =
  "rounded-xl border border-line bg-surface p-4.5 dark:border-line-d dark:bg-surface-d";

function Step({
  n,
  title,
  body,
}: {
  n: string;
  title: string;
  body: string;
}) {
  return (
    <div className="flex items-start gap-3">
      <span
        className={cx(
          tabular,
          "flex h-5.5 w-5.5 flex-none items-center justify-center rounded-full border border-line bg-surface2 text-sm font-extrabold text-text3 dark:border-line-d dark:bg-surface2-d dark:text-text3-d",
        )}
      >
        {n}
      </span>
      <span className="min-w-0">
        <span className="block text-md font-bold">{title}</span>
        <span className="mt-1 block text-sm leading-[1.55] text-text3 dark:text-text3-d">
          {body}
        </span>
      </span>
    </div>
  );
}

export function FirstRun({ openChat }: { openChat: () => void }) {
  const { settings, addProject, createProject, status, dataDir, installSidecar } = useStore();
  const [name, setName] = useState("");
  const firstName = (settings?.user_name ?? "Operator").split(/\s+/)[0];

  return (
    <div className="max-w-[1000px] px-6.5 pb-7 pt-5.5">
      {/* O banner é escuro nos dois temas, por isso o que assenta nele não
          tem par claro: é o mesmo tinteiro em ambos. */}
      <div className="relative animate-[fadeUp_.5s_ease_both] overflow-hidden rounded-xl bg-ink-light shadow-lift dark:bg-ink dark:shadow-lift-d">
        <div className="pointer-events-none absolute -right-[60px] -top-[80px] h-[240px] w-[240px] rounded-full bg-bannerGlow" />
        <div className="relative px-6.5 pb-6 pt-6.5">
          <div className="text-2xl font-extrabold tracking-[-.02em] text-onBanner">
            {greeting()}, {firstName}. Nothing is set up yet.
          </div>
          <div className="mt-1.5 max-w-[62ch] text-md leading-[1.55] text-[rgba(255,255,255,.62)]">
            Relay works on git repositories — local ones. Point it at a repo you already have, or
            start a new one from scratch: no remote, no account, nothing leaves this machine unless
            an agent asks you to push.
          </div>

          <div className="mt-4.5 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={addProject}
              className="min-h-6 cursor-pointer rounded-full border-none bg-white px-4.5 py-2.5 text-md font-bold text-[#17171f] transition-transform duration-150 ease-out hover:-translate-y-px"
            >
              Open a repository…
            </button>
            <div className="flex items-center gap-1.5 rounded-full border border-[rgba(255,255,255,.2)] bg-[rgba(255,255,255,.07)] py-1 pl-3.5 pr-1 transition-colors duration-200 focus-within:border-[rgba(242,239,232,.42)] focus-within:bg-[rgba(242,239,232,.11)]">
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && name.trim()) createProject(name);
                }}
                aria-label="Name a new local repository"
                placeholder="or name a new local repo…"
                className="w-[168px] border-none bg-transparent py-1.5 text-md text-onBanner outline-none placeholder:text-onBanner-3"
              />
              <button
                type="button"
                onClick={() => createProject(name)}
                disabled={!name.trim()}
                className={cx(
                  "min-h-6 rounded-full border-none px-3.5 py-2 text-md font-bold transition-colors duration-150",
                  name.trim()
                    ? "cursor-pointer bg-accent2 text-onAccent hover:bg-[rgba(255,255,255,.15)] dark:bg-accent2-d dark:text-onAccent-d"
                    : "cursor-not-allowed bg-[rgba(255,255,255,.12)] text-onBanner-3",
                )}
              >
                Create
              </button>
            </div>
            <button
              type="button"
              onClick={openChat}
              className="min-h-6 cursor-pointer rounded-full border border-[rgba(255,255,255,.2)] bg-[rgba(255,255,255,.07)] px-4 py-2.5 text-md font-semibold text-onBanner transition-colors duration-150 hover:bg-[rgba(242,239,232,.16)]"
            >
              Ask the Director what to start
            </button>
          </div>
        </div>
      </div>

      <div className="mt-3 grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-start gap-3">
        <div className={cx(PANEL, "flex flex-col gap-3.5")}>
          <div className="text-lg font-bold">How it goes from here</div>
          <Step
            n="1"
            title="Pick a repository"
            body="A local repository is enough — no remote required. Relay never writes inside it directly: each card gets its own worktree under app data, on a harness/<card> branch."
          />
          <Step
            n="2"
            title="Say what should happen"
            body="One line on Home becomes a card. Plan leaves it ready; Start hands it to the Builder straight away."
          />
          <Step
            n="3"
            title="The Director reads the diff"
            body="When a run finishes it reviews the work and either approves it or sends it back with a reason. Only what passes reaches you."
          />
          <Step
            n="4"
            title="You stay in charge"
            body="Anything outside an agent's permissions stops the run and asks. Every decision is in the event log."
          />
        </div>

        <div className="flex flex-col gap-3">
          <div className={PANEL}>
            <div className="mb-3 text-lg font-bold">Before you start</div>
            {[
              {
                label: "Claude",
                ok: status?.claude.logged_in ?? false,
                good: status?.claude.cli_version
                  ? `logged in · claude ${status.claude.cli_version}`
                  : "logged in",
                bad: "not logged in — open a terminal and run /login",
              },
              {
                label: "Agent sidecar",
                ok: status?.sidecar.ready ?? false,
                good: `ready · node ${status?.sidecar.node_version ?? ""}`.trim(),
                bad: status?.sidecar.node_found
                  ? "dependencies not installed yet"
                  : "node was not found on PATH",
              },
              {
                label: "git",
                ok: true,
                good: "used through the command line · local only, no remote",
                bad: "",
              },
            ].map((row, i) => (
              <div
                key={row.label}
                className={cx(
                  "flex items-center gap-2.5 py-2.5",
                  i > 0 && "border-t border-line2 dark:border-line2-d",
                )}
              >
                <span
                  className={cx(
                    "h-1.75 w-1.75 flex-none rounded-full",
                    row.ok ? "bg-ok dark:bg-ok-d" : "bg-warn dark:bg-warn-d",
                  )}
                />
                <span className="min-w-24 flex-none text-md font-semibold">{row.label}</span>
                <span
                  className={cx(
                    truncate,
                    "flex-1 text-sm text-text3 dark:text-text3-d",
                  )}
                >
                  {row.ok ? row.good : row.bad}
                </span>
                {!row.ok && row.label === "Agent sidecar" && status?.sidecar.node_found && (
                  <button
                    type="button"
                    onClick={installSidecar}
                    className="min-h-6 cursor-pointer rounded-full border-none bg-accent px-3 py-1.5 text-sm font-bold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-accent-d dark:text-onAccent-d"
                  >
                    Install
                  </button>
                )}
              </div>
            ))}
          </div>

          <div className={PANEL}>
            <div className="mb-1.5 text-lg font-bold">Where Relay keeps things</div>
            <div
              title={dataDir}
              className={cx(truncate, "font-mono text-sm text-text3 dark:text-text3-d")}
            >
              {dataDir || "—"}
            </div>
            <div className="mt-2 text-sm leading-relaxed text-text3 dark:text-text3-d">
              Event logs, run transcripts, agent profiles and worktrees live there. Your repository
              only ever receives commits.
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
