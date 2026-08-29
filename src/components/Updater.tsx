/** The application updater, all four of its sheets, and the state behind them.
 *
 *  Design: `docs/design/Relay Lifecycle.dc.html`, "05 · UPDATER". The plugin
 *  emits events and ships no interface, so every pixel here is ours. Two
 *  decisions from the handoff are not negotiable and are the reason the code
 *  is shaped the way it is:
 *
 *  - **the default action is "Install on quit", not "Restart now"**, because
 *    agents may be mid-run. Choosing it downloads and verifies; nothing is
 *    swapped and no run is cut short. The sheet then names the real number of
 *    agents working, read from `active_runs`;
 *  - **the failure sheet shows the raw updater log**, not a paraphrase. Every
 *    line in `log` is what actually happened, and the error string is the
 *    plugin's own.
 *
 *  This is *not* `updates_list` / `update_install`. Those are about builds a
 *  card produced in the operator's own checkout — a different offer, from a
 *  different place, and `App.tsx` still shows them on their own strip. */

import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { AnimatePresence, motion } from "motion/react";
import { Check, X } from "lucide-react";
import { cx } from "../lib/cx";
import { sheetIn } from "../lib/motion";
import { ago, megabytes, plural } from "../lib/format";
import { api, reason } from "../lib/ipc";
import { useStore } from "../state/store";
import { Spinner, mono, truncate } from "./ui";

/** Where a release is between "there is one" and "it is on disk". */
type Stage = "none" | "available" | "downloading" | "ready" | "failed";

type State = {
  stage: Stage;
  release: Update | null;
  /** Verbatim, in order. The failure sheet prints this and nothing else. */
  log: string[];
  got: number;
  total: number;
  /** When the download began, so the rate on screen is measured and not guessed. */
  since: number;
  /** The plugin's own error string. */
  error: string | null;
  /** The last time the feed actually answered, either way. */
  checkedMs: number;
  checking: boolean;
  /** A version the operator said "Later" to. Not offered again this session. */
  dismissed: string | null;
  /** The signature is checked by `download`, so this can only be true once the
   *  download has resolved. Showing it any earlier would be a decoration. */
  verified: boolean;
};

const initial: State = {
  stage: "none",
  release: null,
  log: [],
  got: 0,
  total: 0,
  since: 0,
  error: null,
  checkedMs: 0,
  checking: false,
  dismissed: null,
  verified: false,
};

// One updater per process, not per mounted component: the Settings row and the
// sheet are two views of the same download, and a second `check()` behind the
// operator's back is how you end up downloading a release twice.
let state = initial;
const watchers = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  watchers.forEach((w) => w());
}

function say(line: string) {
  set({ log: [...state.log, line].slice(-40) });
}

/** The state as it is *now*. `set` replaces it, so a read taken before an
 *  await is stale — and the compiler, seeing a module-level binding, is happy
 *  to narrow it and keep the stale answer. */
function now(): State {
  return state;
}

const subscribe = (fn: () => void) => {
  watchers.add(fn);
  return () => {
    watchers.delete(fn);
  };
};

/** Ask the release feed. Quiet on the way in — this runs on a timer and on
 *  every window focus, and an operator who is up to date should never know. */
export async function checkForUpdate(): Promise<void> {
  if (state.checking || state.stage === "downloading" || state.stage === "ready") return;
  set({ checking: true });
  try {
    const found = await check();
    set({ checking: false, checkedMs: Date.now(), error: null });
    if (!found) {
      say("updater: no update available");
      set({ release: null, stage: "none" });
      return;
    }
    say(`updater: ${found.version} available, you have ${found.currentVersion}`);
    set({ release: found, stage: found.version === state.dismissed ? "none" : "available" });
  } catch (e) {
    // Silence here cost an evening once: the feed was answering 404 because
    // the repository is private and the app had no way to say so. It is still
    // not worth a toast on every transient hiccup, so it lands in Settings
    // beside the rest of the system's state — and in the log, verbatim.
    const raw = reason(e);
    say(`updater: check failed — ${raw}`);
    set({ checking: false, checkedMs: Date.now(), error: raw });
  }
}

/** Download and verify. Nothing is installed and nothing restarts: this is the
 *  half of "Install on quit" that can happen while agents are working. */
export async function downloadUpdate(): Promise<void> {
  const release = state.release;
  if (!release || state.stage === "downloading") return;
  set({ stage: "downloading", got: 0, total: 0, since: Date.now(), verified: false, error: null });
  say(`updater: downloading ${release.version}`);
  try {
    await release.download((event) => {
      if (event.event === "Started") set({ total: event.data.contentLength ?? 0 });
      else if (event.event === "Progress") set({ got: state.got + event.data.chunkLength });
    });
    // Cancelled while the bytes were still arriving. They are dropped rather
    // than installed: "Cancel" has to mean nothing happens.
    if (now().stage !== "downloading") {
      say("updater: cancelled — the download was discarded");
      return;
    }
    // `download` verifies the signature before it resolves. This is the first
    // moment the tick is true.
    say(`updater: signature verified (${megabytes(state.got)})`);
    set({ stage: "ready", verified: true });
  } catch (e) {
    const raw = reason(e);
    say(`updater: download failed — ${raw}`);
    say("updater: nothing was replaced");
    if (now().stage === "downloading") set({ stage: "failed", error: raw });
  }
}

/** Swap the binary now and come back on the new one. The only path that ends a
 *  run early, which is why it is never the default. */
export async function installAndRestart(): Promise<void> {
  const release = state.release;
  if (!release) return;
  try {
    say(`updater: installing ${release.version}`);
    await release.install();
    await relaunch();
  } catch (e) {
    const raw = reason(e);
    say(`updater: install failed — ${raw}`);
    set({ stage: "failed", error: raw });
  }
}

/** The other half of "Install on quit": the swap is attempted as the window
 *  goes, so a running agent is never interrupted by it. Best effort by
 *  construction — if the process is gone before the swap lands, nothing was
 *  replaced and the same release is offered again next launch. */
let quitHook: (() => void) | null = null;

export function installOnQuit(): void {
  if (quitHook) return;
  getCurrentWindow()
    .onCloseRequested(() => {
      state.release?.install().catch(() => {});
    })
    .then((un) => {
      quitHook = un;
    })
    .catch(() => {});
}

export function dismissUpdate(): void {
  set({ stage: "none", dismissed: state.release?.version ?? null });
}

/** Everything the sheets and the Settings row read. */
export function useUpdater() {
  return useSyncExternalStore(subscribe, () => state);
}

// ---- the sheets -------------------------------------------------------------

const SHEET =
  "w-[420px] rounded-sheet border border-line bg-elev px-5.5 py-5 shadow-soft dark:border-line-d dark:bg-elev-d dark:shadow-soft-d";
const TITLE = "text-sheet font-bold text-ink dark:text-ink-d";
const META = "mt-0.75 text-sm text-faint dark:text-faint-d";
const QUIET_PILL =
  "cursor-pointer rounded-full border border-line bg-transparent px-4 py-2 text-body font-medium text-muted transition-colors duration-150 hover:bg-hovered disabled:cursor-default disabled:opacity-50 dark:border-line-d dark:text-muted-d dark:hover:bg-hovered-d";
const DARK_PILL =
  "cursor-pointer rounded-full border-none bg-ink px-4.5 py-2 text-body font-semibold text-white transition-[filter] duration-150 hover:brightness-125 disabled:cursor-default disabled:opacity-40 dark:bg-ink-d dark:text-canvas-d";
const FOOT = "mt-3.75 flex items-center gap-2.25";
const NOTE = "text-sm text-faint dark:text-faint-d";

/** A finished step in the download's own checklist. Ticked only for something
 *  that actually happened. */
function Step({ done, children }: { done: boolean; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2.25 text-body text-muted dark:text-muted-d">
      {done ? (
        <span className="grid h-4 w-4 flex-none place-items-center rounded-full bg-okSoft dark:bg-okSoft-d">
          <Check size={10} strokeWidth={4} className="text-ok dark:text-ok-d" aria-hidden />
        </span>
      ) : (
        <span className="h-4 w-4 flex-none rounded-full bg-active dark:bg-active-d" />
      )}
      {children}
    </div>
  );
}

/** How many agents are working right now, across every registered project —
 *  the number the operator is actually being asked to weigh. */
function useRunningAgents(enabled: boolean): number | null {
  const { projects } = useStore();
  const [count, setCount] = useState<number | null>(null);
  const ids = useMemo(() => projects.filter((p) => p.exists).map((p) => p.id), [projects]);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const read = () =>
      Promise.all(ids.map((id) => api.activeRuns(id).catch(() => [])))
        .then((all) => alive && setCount(all.reduce((n, rows) => n + rows.length, 0)))
        .catch(() => {});
    read();
    const every = setInterval(read, 5_000);
    return () => {
      alive = false;
      clearInterval(every);
    };
  }, [enabled, ids]);

  return count;
}

/** The four states, in one place. Mounted always; it draws nothing until the
 *  feed has something to say. */
export function UpdateSheets() {
  const { settings } = useStore();
  const { stage, release, log, got, total, since, error, verified } = useUpdater();
  const auto = settings?.auto_install_updates ?? false;
  const running = useRunningAgents(stage !== "none");
  const [copied, setCopied] = useState(false);

  // Checked at mount, again whenever the window regains focus, and on a slow
  // timer. Reading once and never again meant a release cut while Relay was
  // open stayed invisible until the next launch.
  useEffect(() => {
    checkForUpdate();
    const every = setInterval(checkForUpdate, 3 * 60 * 60 * 1000);
    window.addEventListener("focus", checkForUpdate);
    return () => {
      window.removeEventListener("focus", checkForUpdate);
      clearInterval(every);
    };
  }, []);

  // The toggle in Settings, honoured. It skips the offer, never the download's
  // own rules: the swap still waits for the window to close.
  useEffect(() => {
    if (auto && stage === "available") {
      downloadUpdate().then(installOnQuit);
    }
  }, [auto, stage]);

  const onQuit = useCallback(() => {
    installOnQuit();
    downloadUpdate();
  }, []);

  useEffect(() => setCopied(false), [stage]);

  const copy = useCallback(() => {
    navigator.clipboard
      .writeText(log.join("\n"))
      .then(() => setCopied(true))
      .catch(() => {});
  }, [log]);

  const version = release?.version ?? "";
  const secs = Math.max(0.001, (Date.now() - since) / 1000);
  const pct = total > 0 ? Math.min(100, Math.round((got / total) * 100)) : 0;
  const agents = running === null ? null : plural(running, "agent");

  return (
    // The shell owns the corner: this stacks with the toasts there.
    <div className="pointer-events-none flex flex-col items-end">
      <AnimatePresence>
        {stage !== "none" && (
          <motion.div
            key={stage}
            variants={sheetIn}
            initial="hidden"
            animate="shown"
            exit="gone"
            className={cx(SHEET, "pointer-events-auto")}
          >
            {stage === "available" && (
              <>
                <div className="flex items-start gap-3.25">
                  <img src="/relay.svg" alt="" width={36} height={36} className="flex-none" />
                  <div className="min-w-0 flex-1">
                    <div className={TITLE}>Relay {version} is ready to install</div>
                    <div className={cx(mono, truncate, META)}>
                      you have {release?.currentVersion}
                      {released(release?.date) ? ` · released ${released(release?.date)}` : ""}
                    </div>
                  </div>
                  <button
                    type="button"
                    aria-label="Not now"
                    onClick={dismissUpdate}
                    className="cursor-pointer border-none bg-transparent p-0 text-faint transition-colors duration-150 hover:text-ink dark:text-faint-d dark:hover:text-ink-d"
                  >
                    <X size={13} strokeWidth={2.4} aria-hidden />
                  </button>
                </div>

                {release?.body && (
                  <div className="mt-3.75 max-h-[104px] overflow-hidden rounded-10px border border-line3 px-3.75 py-3.25 dark:border-line3-d">
                    <div
                      className={cx(mono, "text-11 font-semibold tracking-[.06em] text-faint dark:text-faint-d")}
                    >
                      WHAT CHANGED
                    </div>
                    <div className="mt-1.5 whitespace-pre-wrap text-body leading-[1.75] text-ink2 dark:text-ink2-d">
                      {release.body.trim()}
                    </div>
                  </div>
                )}

                <div className={FOOT}>
                  {/* The whole reason the default action is what it is. */}
                  <span className={NOTE}>{agents === null ? "" : `${agents} running`}</span>
                  <span className="ml-auto flex gap-2">
                    <button type="button" onClick={dismissUpdate} className={QUIET_PILL}>
                      Later
                    </button>
                    <button type="button" onClick={onQuit} className={DARK_PILL}>
                      Install on quit
                    </button>
                  </span>
                </div>
              </>
            )}

            {stage === "downloading" && (
              <>
                <div className="flex items-start gap-3.25">
                  <img src="/relay.svg" alt="" width={36} height={36} className="flex-none" />
                  <div className="min-w-0 flex-1">
                    <div className={TITLE}>Downloading {version}</div>
                    <div className={cx(mono, truncate, META)}>
                      {total > 0
                        ? `${megabytes(got)} of ${megabytes(total)} · ${megabytes(got / secs)}/s`
                        : `${megabytes(got)} · ${megabytes(got / secs)}/s`}
                    </div>
                  </div>
                  <Spinner size={15} />
                </div>

                <div className="mt-4.25 h-1.5 overflow-hidden rounded-full bg-active dark:bg-active-d">
                  {/* The width is the only thing here the stylesheet cannot
                      know: it is the bytes that have actually arrived. */}
                  <div
                    className="h-full rounded-full bg-ink transition-[width] duration-200 dark:bg-ink-d"
                    style={{ width: `${pct}%` }}
                  />
                </div>

                <div className="mt-3.75 flex flex-col gap-2">
                  <Step done={verified}>Signature verified</Step>
                  <Step done={false}>
                    {settings?.commit_wip_on_close
                      ? "Agents will finish and commit before restart"
                      : "Running agents are cancelled on restart"}
                  </Step>
                </div>

                <div className={FOOT}>
                  <span className="ml-auto flex gap-2">
                    <button type="button" onClick={dismissUpdate} className={QUIET_PILL}>
                      Cancel
                    </button>
                  </span>
                </div>
              </>
            )}

            {stage === "ready" && (
              <>
                <div className="flex items-start gap-3.25">
                  <span className="grid h-9 w-9 flex-none place-items-center rounded-[11px] bg-okSoft dark:bg-okSoft-d">
                    <Check size={19} strokeWidth={3} className="text-ok dark:text-ok-d" aria-hidden />
                  </span>
                  <div className="min-w-0 flex-1">
                    {/* Not "installed": the swap has deliberately not
                        happened yet, and saying it had would be the friendly
                        lie the rest of this screen refuses. */}
                    <div className={TITLE}>Relay {version} installs when you quit</div>
                    <div className={cx(mono, truncate, META)}>
                      downloaded and verified · {megabytes(got)}
                    </div>
                  </div>
                </div>

                <div className="mt-3.75 rounded-10px border border-line3 px-3.5 py-3 text-sm leading-[1.65] text-muted dark:border-line3-d dark:text-muted-d">
                  {settings?.commit_wip_on_close
                    ? "Restarting now cancels running agents and commits their work in progress with a "
                    : "Restarting now cancels running agents and leaves their worktrees as they are, "}
                  {settings?.commit_wip_on_close && (
                    <span className={cx(mono, "text-11")}>wip:</span>
                  )}
                  {settings?.commit_wip_on_close
                    ? " message, exactly like closing the window."
                    : "exactly like closing the window."}
                </div>

                <div className={FOOT}>
                  <span className={NOTE}>{agents === null ? "" : `${agents} running`}</span>
                  <span className="ml-auto flex gap-2">
                    <button type="button" onClick={dismissUpdate} className={QUIET_PILL}>
                      On quit
                    </button>
                    <button type="button" onClick={installAndRestart} className={DARK_PILL}>
                      Restart now
                    </button>
                  </span>
                </div>
              </>
            )}

            {stage === "failed" && (
              <>
                <div className="flex items-start gap-3.25">
                  <span className="grid h-9 w-9 flex-none place-items-center rounded-[11px] bg-badSoft text-[17px] font-bold text-bad dark:bg-badSoft-d dark:text-bad-d">
                    !
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className={TITLE}>Update failed</div>
                    <div className={cx(mono, truncate, META)}>nothing was replaced</div>
                  </div>
                </div>

                {/* The log, not a friendlier version of it. The line that
                    matters is almost always the one nobody would have thought
                    to paraphrase. */}
                <pre
                  className={cx(
                    mono,
                    "mt-3.75 max-h-[128px] overflow-auto whitespace-pre-wrap rounded-10px border border-line3 bg-hovered px-3.5 py-3 text-11 leading-[1.7] text-muted dark:border-line3-d dark:bg-hovered-d dark:text-muted-d",
                  )}
                >
                  {log.join("\n") || error}
                </pre>

                <div className={FOOT}>
                  <span className={NOTE}>Your install is untouched.</span>
                  <span className="ml-auto flex gap-2">
                    <button type="button" onClick={copy} className={QUIET_PILL}>
                      {copied ? "Copied" : "Copy log"}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        set({ stage: "available", error: null });
                        downloadUpdate();
                      }}
                      className={DARK_PILL}
                    >
                      Try again
                    </button>
                  </span>
                </div>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

/** When the release was cut, or nothing. Tauri writes `pub_date` as RFC 3339
 *  with a nanosecond fraction and a space before the offset, neither of which
 *  `Date.parse` will read — and a date it cannot read is a line the sheet
 *  leaves out rather than guesses at. */
function released(raw: string | undefined): string | null {
  if (!raw) return null;
  const ms = Date.parse(raw.replace(/\.(\d{3})\d+/, ".$1").replace(/ (?=[+-]\d{2}:?\d{2}$)/, ""));
  return Number.isFinite(ms) ? ago(ms) : null;
}

// ---- the Settings row -------------------------------------------------------

/** "Relay 1.4.2 · up to date · checked 12 minutes ago". The version, the last
 *  answer from the feed and the toggle, in the one place the operator goes to
 *  look at the rest of the system's state. */
export function useAppVersion(): string | null {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);
  return version;
}
