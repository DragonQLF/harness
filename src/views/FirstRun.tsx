/** The three steps between installing Relay and having something to run.
 *
 *  Design: `docs/design/Relay Lifecycle.dc.html`, "First run" — 820px in the
 *  main window, one card per step. The handoff's note on step 1 is the whole
 *  point of the screen: *every check is a real probe; none of the ticks are
 *  decorative*. Nothing here congratulates the operator on a state it has not
 *  actually read. */

import { useCallback, useEffect, useState } from "react";
import { Check, FolderUp } from "lucide-react";
import { cx } from "../lib/cx";
import { api, reason } from "../lib/ipc";
import { useStore } from "../state/store";
import { mono, truncate } from "../components/ui";
import type { AgentProfile, FolderInfo } from "../lib/types";

const SHELL =
  "w-[820px] max-w-full overflow-hidden rounded-md border border-line bg-surface shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d";
const DARK_PILL =
  "cursor-pointer rounded-full border-none bg-ink px-5 py-2 text-md font-semibold text-white transition-[filter] duration-150 hover:brightness-125 disabled:cursor-default disabled:opacity-40 dark:bg-ink-d dark:text-canvas-d";
const QUIET_PILL =
  "cursor-pointer rounded-full border border-line bg-transparent px-4.5 py-2 text-md font-medium text-muted transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:text-muted-d dark:hover:bg-hovered-d";

/** One probe in step 1. `state` is what was actually read, never a default. */
function Probe({
  ok,
  label,
  detail,
  action,
}: {
  ok: boolean;
  label: string;
  detail: string;
  action?: { label: string; run: () => void; busy?: boolean };
}) {
  return (
    <div
      className={cx(
        "flex items-center gap-2.75 border-t border-line3 px-3.75 py-3.25 first:border-t-0 dark:border-line3-d",
        ok ? "" : "bg-warnSheet dark:bg-warnSheet-d",
      )}
    >
      {ok ? (
        <span className="grid h-5 w-5 flex-none place-items-center rounded-full bg-okSoft dark:bg-okSoft-d">
          <Check size={12} strokeWidth={3.4} className="text-ok dark:text-ok-d" aria-hidden />
        </span>
      ) : (
        <span className="grid h-5 w-5 flex-none place-items-center rounded-full border-[1.5px] border-warn text-11 font-bold text-warn dark:border-warn-d dark:text-warn-d">
          !
        </span>
      )}
      <span className="text-md font-medium text-ink dark:text-ink-d">{label}</span>
      <span
        className={cx(
          mono,
          truncate,
          ok
          ? "text-sm text-faint dark:text-faint-d"
          : "text-sm text-warnText2 dark:text-warnText2-d",
        )}
      >
        {detail}
      </span>
      {action && (
        <button
          type="button"
          disabled={action.busy}
          onClick={action.run}
          className={cx(DARK_PILL, "ml-auto flex-none px-3.5 py-1.5 text-sm")}
        >
          {action.busy ? "Installing…" : action.label}
        </button>
      )}
    </div>
  );
}

export function FirstRun({ openChat }: { openChat: () => void }) {
  const {
    status,
    settings,
    agents,
    projects,
    installSidecar,
    refreshStatus,
    saveSettings,
    agentTemplates,
    toast,
  } = useStore();

  const [step, setStep] = useState(1);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<FolderInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [crew, setCrew] = useState<AgentProfile[] | null>(null);

  // Step 3 shows what the crew would be. When profiles already exist those are
  // the answer; when none do, the templates are — and the templates are only
  // fetched here, because a menu Relay never opens is a menu it never installs.
  useEffect(() => {
    if (step !== 3) return;
    if (agents.length > 0) {
      setCrew(agents);
      return;
    }
    let alive = true;
    agentTemplates()
      .then((rows) => alive && setCrew(rows))
      .catch((e) => alive && setError(reason(e)));
    return () => {
      alive = false;
    };
  }, [step, agents, agentTemplates]);

  const browse = useCallback(async () => {
    setError(null);
    try {
      const path = await api.pickFolder();
      if (!path) return;
      setPicked(await api.inspectFolder(path));
    } catch (e) {
      setError(reason(e));
    }
  }, []);

  const add = useCallback(async () => {
    if (!picked) return;
    setBusy(true);
    setError(null);
    try {
      await api.projectAdd(picked.path, picked.name, !picked.is_repo && picked.empty);
      setStep(3);
    } catch (e) {
      setError(reason(e));
    } finally {
      setBusy(false);
    }
  }, [picked]);

  const claude = status?.claude;
  const sidecar = status?.sidecar;
  const canContinue = Boolean(claude?.cli_found && claude?.logged_in);

  const head = (
    <div className="flex h-11 flex-none items-center gap-3 border-b border-line px-4 dark:border-line-d">
      <span className="font-display text-lg font-bold tracking-[.075em] text-ink [text-shadow:1.5px_1.5px_0_theme(colors.wordmarkShadow.DEFAULT)] dark:text-ink-d dark:[text-shadow:1.5px_1.5px_0_theme(colors.wordmarkShadow.d)]">
        RELAY
      </span>
      <span className={cx(mono, "ml-auto text-11 text-faint dark:text-faint-d")}>
        step {step} of 3
      </span>
    </div>
  );

  const foot = (note: string, actions: React.ReactNode) => (
    <div className="flex flex-none items-center gap-2.5 border-t border-line px-4 py-3.25 dark:border-line-d">
      <span className="text-body text-faint dark:text-faint-d">{note}</span>
      <span className="ml-auto flex gap-2">{actions}</span>
    </div>
  );

  return (
    <div className="grid min-h-full place-items-center px-6 py-8">
      <div className="flex flex-col gap-2.25">
        <div className={SHELL}>
          {head}

          {step === 1 && (
            <>
              <div className="flex gap-10 px-10 pb-8 pt-8.5">
                <div className="min-w-0 flex-1">
                  <h1 className="m-0 font-display text-stat font-bold tracking-[-.01em] text-ink dark:text-ink-d">
                    Claude Code is the engine
                  </h1>
                  <p className="mx-0 mb-0 mt-2.75 text-base leading-[1.7] text-muted dark:text-muted-d">
                    Relay runs agents through your own Claude Code login. Nothing leaves this
                    machine except the model calls it already makes.
                  </p>

                  <div className="mt-5 overflow-hidden rounded-md border border-line dark:border-line-d">
                    {!status ? (
                      // The probes have not answered yet. Three rows at final
                      // height, so nothing shifts when they do.
                      [0, 1, 2].map((i) => (
                        <div
                          key={i}
                          className="flex items-center gap-2.75 border-t border-line3 px-3.75 py-3.25 first:border-t-0 dark:border-line3-d"
                        >
                          <span className="h-5 w-5 flex-none animate-pulse rounded-full bg-active dark:bg-active-d" />
                          <span className="h-3 w-32 animate-pulse rounded-4px bg-active dark:bg-active-d" />
                        </div>
                      ))
                    ) : (
                      <>
                        <Probe
                          ok={Boolean(claude?.cli_found)}
                          label="claude found"
                          detail={
                            claude?.cli_found
                              ? [claude.cli_version, "PATH"].filter(Boolean).join(" · ")
                              : "not on PATH"
                          }
                        />
                        <Probe
                          ok={Boolean(claude?.logged_in)}
                          label="logged in"
                          detail={
                            claude?.logged_in
                              ? (claude.credentials_path ?? "credentials found")
                              : "no credentials"
                          }
                          action={
                            claude?.logged_in
                              ? undefined
                              : {
                                  label: "Open a terminal",
                                  run: () => {
                                    api.openClaudeTerminal().catch((e) => setError(reason(e)));
                                  },
                                }
                          }
                        />
                        <Probe
                          ok={Boolean(sidecar?.ready)}
                          label="sidecar dependencies"
                          detail={
                            sidecar?.ready
                              ? (sidecar.node_version ?? "installed")
                              : sidecar?.node_found
                                ? "not installed"
                                : "no node"
                          }
                          action={
                            sidecar?.ready || !sidecar?.node_found
                              ? undefined
                              : {
                                  label: "Install",
                                  busy,
                                  run: async () => {
                                    setBusy(true);
                                    try {
                                      await installSidecar();
                                      await refreshStatus();
                                    } finally {
                                      setBusy(false);
                                    }
                                  },
                                }
                          }
                        />
                      </>
                    )}
                  </div>
                </div>

                <div className="flex w-[186px] flex-none flex-col items-center justify-center gap-3.5 rounded-sheet border border-line3 bg-hovered p-5 dark:border-line3-d dark:bg-hovered-d">
                  <img src="/relay.svg" alt="" width={58} height={58} />
                  <div
                    className={cx(mono, "text-center text-11 leading-[1.6] text-faint dark:text-faint-d")}
                  >
                    no account
                    <br />
                    no remote
                    <br />
                    local git only
                  </div>
                </div>
              </div>

              {foot(
                "You can change all of this in Settings later.",
                <>
                  <button type="button" onClick={openChat} className={QUIET_PILL}>
                    Skip
                  </button>
                  <button
                    type="button"
                    disabled={!canContinue}
                    title={canContinue ? undefined : "Relay needs a signed-in Claude Code first"}
                    onClick={() => setStep(2)}
                    className={DARK_PILL}
                  >
                    Continue
                  </button>
                </>,
              )}
            </>
          )}

          {step === 2 && (
            <>
              <div className="px-10 pb-8 pt-8.5">
                <h1 className="m-0 font-display text-stat font-bold tracking-[-.01em] text-ink dark:text-ink-d">
                  Point it at a repository
                </h1>
                <p className="mx-0 mb-0 mt-2.75 max-w-[520px] text-base leading-[1.7] text-muted dark:text-muted-d">
                  One engine per project. Worktrees are created under app data, never inside the
                  repository.
                </p>

                <button
                  type="button"
                  onClick={browse}
                  className="mt-5 flex w-full cursor-pointer items-center gap-4.5 rounded-sheet border-[1.5px] border-dashed border-line4 bg-hovered p-6.5 text-left transition-colors duration-150 hover:border-primary dark:border-line4-d dark:bg-hovered-d"
                >
                  <FolderUp
                    size={34}
                    strokeWidth={1.7}
                    className="flex-none text-faint dark:text-faint-d"
                    aria-hidden
                  />
                  <span>
                    <span className="block text-base font-semibold text-ink dark:text-ink-d">
                      Choose a folder
                    </span>
                    <span className="mt-0.75 block text-body text-faint dark:text-faint-d">
                      must contain a .git directory
                    </span>
                  </span>
                  <span className={cx(QUIET_PILL, "ml-auto flex-none bg-surface dark:bg-surface-d")}>
                    Browse…
                  </span>
                </button>

                <div className="mt-4 grid grid-cols-3 gap-3">
                  {picked ? (
                    <div className="rounded-md border border-line px-3.75 py-3.5 dark:border-line-d">
                      <div className="text-md font-semibold text-ink dark:text-ink-d">
                        {picked.name}
                      </div>
                      <div
                        className={cx(mono, truncate, "mt-0.75 text-xs text-faint dark:text-faint-d")}
                      >
                        {picked.path}
                      </div>
                      <div className="mt-2.25 text-11 text-muted dark:text-muted-d">
                        {picked.already_added
                          ? "already added"
                          : picked.is_repo
                            ? "git repository"
                            : picked.empty
                              ? "empty · Relay will run git init"
                              : "no .git here"}
                      </div>
                    </div>
                  ) : (
                    <div className="col-span-3 rounded-md border border-line bg-hovered px-3.75 py-3.5 dark:border-line-d dark:bg-hovered-d">
                      <div className="text-md font-semibold text-faint dark:text-faint-d">
                        Nothing chosen yet
                      </div>
                      <div className="mt-2.25 text-11 text-faint dark:text-faint-d">
                        Relay reads nothing until you add it.
                      </div>
                    </div>
                  )}
                </div>

                {error && (
                  <div
                    className={cx(
                      mono,
                      "mt-3 rounded-sm border border-warnLine bg-warnSheet px-3 py-2 text-sm text-bad dark:border-warnLine-d dark:bg-warnSheet-d dark:text-bad-d",
                    )}
                  >
                    {error}
                  </div>
                )}
              </div>

              {foot(
                "Add more later from the project picker.",
                <>
                  <button type="button" onClick={() => setStep(1)} className={QUIET_PILL}>
                    Back
                  </button>
                  <button
                    type="button"
                    disabled={!picked || busy || (!picked.is_repo && !picked.empty)}
                    onClick={add}
                    className={DARK_PILL}
                  >
                    {picked ? `Add ${picked.name}` : "Add"}
                  </button>
                </>,
              )}
            </>
          )}

          {step === 3 && (
            <>
              <div className="px-10 pb-8 pt-8.5">
                <h1 className="m-0 font-display text-stat font-bold tracking-[-.01em] text-ink dark:text-ink-d">
                  Your crew, as shipped
                </h1>
                <p className="mx-0 mb-0 mt-2.75 max-w-[560px] text-base leading-[1.7] text-muted dark:text-muted-d">
                  Model, budget, where each one works and who reviews it are all editable — this is
                  the only screen that decides what an agent is allowed to do.
                </p>

                <div className="mt-5 grid grid-cols-4 gap-3">
                  {crew === null
                    ? [0, 1, 2, 3].map((i) => (
                        <div
                          key={i}
                          className="h-[104px] animate-pulse rounded-md border border-line bg-hovered dark:border-line-d dark:bg-hovered-d"
                        />
                      ))
                    : crew.slice(0, 4).map((a) => (
                        <div
                          key={a.id}
                          className="rounded-md border border-line px-4 py-3.75 dark:border-line-d"
                        >
                          <div className="text-base font-bold text-ink dark:text-ink-d">
                            {a.name}
                          </div>
                          <div className={cx(mono, "mt-1 text-xs text-faint dark:text-faint-d")}>
                            {a.model ?? "claude chooses"}
                          </div>
                          <div className="mt-2.5 text-sm leading-[1.6] text-muted dark:text-muted-d">
                            {a.title || a.role}
                          </div>
                        </div>
                      ))}
                </div>
                {crew?.length === 0 && (
                  <p className="mt-3 text-body text-faint dark:text-faint-d">
                    No profiles and no templates. You can create a crew from the Agents screen.
                  </p>
                )}

                <div className="mt-4 flex items-center gap-3.5 rounded-md border border-line bg-hovered px-4.25 py-3.75 dark:border-line-d dark:bg-hovered-d">
                  <span className="flex-none text-md font-semibold text-ink dark:text-ink-d">
                    Ask before anything leaves this machine
                  </span>
                  <span className="text-body text-muted dark:text-muted-d">
                    git push, network calls, and writes outside the worktree reach your permission
                    sheet.
                  </span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={settings?.permission_mode !== "acceptEdits"}
                    aria-label="Ask before anything leaves this machine"
                    onClick={() =>
                      saveSettings({
                        permission_mode:
                          settings?.permission_mode === "acceptEdits" ? "default" : "acceptEdits",
                      })
                    }
                    className={cx(
                      "ml-auto flex h-5.5 w-[38px] flex-none cursor-pointer items-center rounded-full border-none p-0.5 transition-colors duration-150",
                      settings?.permission_mode === "acceptEdits"
                        ? "bg-line4 dark:bg-line4-d"
                        : "bg-ink dark:bg-ink-d",
                    )}
                  >
                    <span
                      className={cx(
                        "h-[18px] w-[18px] rounded-full bg-white transition-transform duration-150",
                        settings?.permission_mode === "acceptEdits"
                          ? "translate-x-0"
                          : "translate-x-4",
                      )}
                    />
                  </button>
                </div>
              </div>

              {foot(
                "⌘K opens every screen, project, agent and card.",
                <>
                  <button type="button" onClick={() => setStep(2)} className={QUIET_PILL}>
                    Back
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      if (projects.length === 0) toast("warn", "No repository added yet");
                      openChat();
                    }}
                    className={DARK_PILL}
                  >
                    Open Relay
                  </button>
                </>,
              )}
            </>
          )}
        </div>

        <div className={cx(mono, "text-11 text-faint dark:text-faint-d")}>
          {step === 1
            ? "step 1 · checks, not a marketing page"
            : step === 2
              ? "step 2 · a drop target plus what it already knows"
              : "step 3 · the crew and the one switch that matters"}
        </div>
      </div>
    </div>
  );
}
