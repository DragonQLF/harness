import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TitleBar } from "./components/TitleBar";
import { Sidebar } from "./components/Sidebar";
import {
  ApprovalSheet,
  CommandPalette,
  Toasts,
  type PaletteAction,
} from "./components/Overlays";
import { UpdateSheets } from "./components/Updater";
import { Loading, Spinner, mono, truncate } from "./components/ui";
import { cx } from "./lib/cx";
import { sheetIn, veil } from "./lib/motion";
import { api, events, reason } from "./lib/ipc";
import { ago, money, plural } from "./lib/format";
import { STATUS_TONE, TONE, tone } from "./lib/types";
import type { ClosingBegan, ClosingPhase, PendingUpdate } from "./lib/types";
import { StoreProvider, useStore } from "./state/store";
import { Home } from "./views/Home";
import { Chat } from "./views/Chat";
import { Code } from "./views/Code";
import { Review } from "./views/Review";
import { FirstRun } from "./views/FirstRun";
import { Board } from "./views/Board";
import { Sessions } from "./views/Sessions";
import { Agents } from "./views/Agents";
import { Projects } from "./views/Projects";
import { Activity, Settings, Worktrees } from "./views/Misc";
import { NAV_VIEWS, VIEW_TITLES, type View } from "./views/views";
import "./styles/app.css";

/** The window is held on close for two deliberate reasons — agents leaving a
 *  wip commit, and once a day the Director's end-of-day look. Held silently,
 *  that is indistinguishable from a hung app: the close button stops working
 *  and nothing says why.
 *
 *  So the wait gets a face. It names what is being waited on, counts the time
 *  out loud against the ceiling the backend promised, and always offers a way
 *  out — nothing in the wait is lost by leaving, because proposals are written
 *  when the tool runs and an unfinished look is due again rather than done. */
function ClosingOverlay() {
  const [began, setBegan] = useState<ClosingBegan | null>(null);
  const [phase, setPhase] = useState<ClosingPhase | null>(null);
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    const subs = [events.onClosingBegan(setBegan), events.onClosingPhase(setPhase)];
    return () => {
      subs.forEach((s) => s.then((un) => un()).catch(() => {}));
    };
  }, []);

  // The count starts when the hold does, not when a phase lands: the first
  // seconds of a slow shutdown are exactly the ones that feel like a freeze.
  useEffect(() => {
    if (!began) return;
    setElapsed(0);
    const id = setInterval(() => setElapsed((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [began]);

  const leaving =
    phase?.phase === "skipped" || phase?.phase === "timeout" || phase?.phase === "done";
  const waitingFor = began?.look
    ? "The Director is taking his end-of-day look"
    : began?.wip
      ? "Letting the agents commit what they have"
      : "Closing down";
  const left = Math.max(0, (began?.limit_secs ?? 0) - elapsed);

  return (
    <AnimatePresence>
      {began && (
        <motion.div
          variants={veil}
          initial="hidden"
          animate="shown"
          exit="gone"
          className="fixed inset-0 z-[900] grid place-items-center bg-[rgba(8,8,14,.62)] backdrop-blur-[3px]"
        >
          <motion.div
            variants={sheetIn}
            initial="hidden"
            animate="shown"
            exit="gone"
            className="w-[420px] max-w-[88vw] rounded-xl border border-line3 bg-elev px-6 pb-4.5 pt-5.5 shadow-soft dark:border-line3-d dark:bg-elev-d dark:shadow-soft-d"
          >
            <div className="flex items-center gap-2.5">
              <Spinner />
              <span className="text-lg font-bold tracking-[-.01em]">
                {leaving ? "Closing Relay" : waitingFor}
              </span>
            </div>

            <p className="mx-0 mb-0 mt-3 text-md font-normal leading-relaxed text-text2 dark:text-text2-d">
              {phase?.detail ??
                "Relay is finishing what it started before it lets go of the window."}
            </p>

            <div className="mt-4 flex items-center gap-2 border-t border-line2 pt-3.5 dark:border-line2-d">
              <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                {elapsed}s · closes on its own in {left}s
              </span>
              <div className="flex-1" />
              {!leaving && (
                <button
                  type="button"
                  onClick={() => api.closeNow().catch(() => {})}
                  className="min-h-6 cursor-pointer rounded-sm border border-line3 bg-surface2 px-3.5 py-1.5 text-sm font-semibold text-text transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px dark:border-line3-d dark:bg-surface2-d dark:text-text-d"
                >
                  Close now
                </button>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

/** Mirror mode's thread end: a slim line, on every screen, for as long as an
 *  approved build sits uninstalled. Installing swaps the binary and relaunches
 *  — the rollback machinery decides whether the next start keeps it.
 *
 *  This is `updates_list` / `update_install`, and it is deliberately *not* the
 *  application updater. A build an agent produced in the operator's own
 *  checkout and a signed release off the feed are two different offers with
 *  two different consequences; `components/Updater.tsx` draws the other one.
 *  Conflating them once meant "Install" could mean either. */
function BuildBanner() {
  const { toast } = useStore();
  const [pending, setPending] = useState<PendingUpdate[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);

  // Checked at mount, again whenever the window regains focus, and on a slow
  // timer. The original read once and never again, so a build finishing while
  // Relay was open stayed invisible until the next launch — which is exactly
  // when it matters, because the operator has just gone and compiled it.
  useEffect(() => {
    let alive = true;
    const look = () =>
      api
        .updatesList()
        .then((rows) => alive && setPending(rows))
        .catch(() => {});
    look();
    window.addEventListener("focus", look);
    const every = setInterval(look, 60_000);
    return () => {
      alive = false;
      window.removeEventListener("focus", look);
      clearInterval(every);
    };
  }, []);

  if (!pending?.length) return null;
  const update = pending[0];
  if (update.card_id === dismissed) return null;

  const install = () => {
    setBusy(true);
    api
      .updateInstall(update.card_id)
      .catch((e) => {
        setBusy(false);
        // It refuses while any agent is working. Sending that to the console
        // left the button flipping back with nothing said.
        toast("bad", "Could not install the update", reason(e));
      });
    // If the swap works, this process is already on its way out.
  };

  return (
    <Banner
      label={
        update.kind === "build"
          ? "A newer Relay is built in your checkout"
          : `Built from ${update.card_id}`
      }
      detail={[
        update.commit_sha ? update.commit_sha.slice(0, 7) : "",
        update.kind === "build" ? ago(update.built_at_ms) : "",
        "installing restarts the app (the previous version is kept)",
      ]
        .filter(Boolean)
        .join(" · ")}
      busy={busy}
      action={busy ? "installing…" : "Install & restart"}
      onInstall={install}
      onLater={() => setDismissed(update.card_id)}
    />
  );
}

/** The strip a mirror build gets. */
function Banner({
  label,
  detail,
  busy,
  action,
  onInstall,
  onLater,
}: {
  label: string;
  detail: string;
  busy: boolean;
  action: string;
  onInstall: () => void;
  onLater: () => void;
}) {
  return (
    <div className="flex items-center gap-2.5 border-b border-line bg-accentSoft px-4 py-2 dark:border-line-d dark:bg-accentSoft-d">
      <span className={cx(mono, "text-xs text-accent2 dark:text-accent2-d")}>UPDATE</span>
      <span className={cx(truncate, "flex-1 text-md font-normal text-text dark:text-text-d")}>
        <b className="font-semibold">{label}</b> · {detail}
      </span>
      {!busy && (
        <button
          type="button"
          onClick={onLater}
          className="min-h-6 cursor-pointer rounded-sm border-none bg-transparent px-2 py-1 text-sm font-medium transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
        >
          Later
        </button>
      )}
      <button
        type="button"
        disabled={busy}
        onClick={onInstall}
        className={cx(
          "min-h-6 rounded-sm border-none bg-accent px-3 py-1 text-sm font-semibold text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px disabled:opacity-60 disabled:hover:translate-y-0 disabled:hover:brightness-100 dark:bg-accent-d dark:text-onAccent-d",
          busy ? "cursor-default" : "cursor-pointer",
        )}
      >
        {action}
      </button>
    </div>
  );
}

/** What the shell tells the launch window while it is coming up.
 *
 *  The splash draws itself; the only thing it cannot know is how far along the
 *  engine is. So this reports the phases that have *actually happened* and
 *  nothing else — DATA-MAP is explicit that a phase with no event shows
 *  nothing rather than a fake step. There are exactly two before the window is
 *  usable, and each one is a fact:
 *
 *  1. `bootstrap` is in flight — the engine is starting;
 *  2. it answered, so the number of boards is known and the current one is
 *     being read. With no project registered there is no board to read and the
 *     line stays empty rather than inventing a step.
 *
 *  When the shell has something on screen it says so, the splash fades over
 *  160ms and this window comes up underneath it. It also shows itself: if the
 *  splash never loaded at all, the app must still appear. */
function SplashHandoff() {
  const { ready, fatal, projects, projectId, snapshot } = useStore();

  const painted = ready && (projectId === null || snapshot !== null);
  const phase = useMemo(() => {
    if (fatal) return { note: "the engine refused to start", progress: 1, done: true };
    if (painted) return { note: null, progress: 1, done: true };
    if (!ready) return { note: "starting engine", progress: 0.34 };
    return {
      note: projects.length > 0 ? `reading ${plural(projects.length, "board")}` : null,
      progress: 0.72,
    };
  }, [fatal, painted, ready, projects.length]);

  const latest = useRef(phase);
  latest.current = phase;

  useEffect(() => {
    emit("splash://phase", phase).catch(() => {});
  }, [phase]);

  // The splash may not have been listening yet when the first phase went out.
  // It says hello once its listener is real; this answers with wherever the
  // shell has got to.
  useEffect(() => {
    const un = listen("splash://listening", () => {
      emit("splash://phase", latest.current).catch(() => {});
    });
    return () => {
      un.then((off) => off()).catch(() => {});
    };
  }, []);

  // The window is created hidden so the splash can own the first frame. Shown
  // here as well as by the splash, because a splash that failed to load must
  // not be able to keep the app invisible — and, on a timer, because a
  // bootstrap that never answers must not either. An app with no window at all
  // is worse than one showing a screen that is still loading.
  useEffect(() => {
    const reveal = () => {
      const window = getCurrentWindow();
      window
        .show()
        .then(() => window.setFocus())
        .catch(() => {});
    };
    if (phase.done) {
      reveal();
      return;
    }
    const failsafe = setTimeout(reveal, 15_000);
    return () => clearTimeout(failsafe);
  }, [phase.done]);

  return null;
}

/** The rail is about work in flight. The two screens that already are a wall of
 *  work do not need it beside them. */


function Shell() {
  const {
    ready,
    fatal,
    project,
    projects,
    snapshot,
    status,
    agents,
    approvals,
    conversations,
    addProject,
    selectProject,
    installSidecar,
    startRun,
    newConversation,
    openConversation,
    chatWithProfile,
    navigation,
    clearNavigation,
    settings,
    saveSettings,
  } = useStore();

  // One history of screens, so the title bar's arrows mean something.
  const [history, setHistory] = useState<{ list: View[]; at: number }>({ list: ["home"], at: 0 });
  const view = history.list[history.at] ?? "home";

  const [agentId, setAgentId] = useState<string | null>(null);
  const [selectedCard, setSelectedCard] = useState<string | null>(null);
  const [reviewCard, setReviewCard] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [approvalSheet, setApprovalSheet] = useState(false);

  const go = useCallback((v: View) => {
    setHistory((h) => {
      if (h.list[h.at] === v) return h;
      const list = [...h.list.slice(0, h.at + 1), v].slice(-40);
      return { list, at: list.length - 1 };
    });
  }, []);

  // macOS puts Relay's commands in the menu bar at the top of the screen. The
  // items carry no behaviour of their own — each one names something this
  // screen already does, so the menu and the window can never disagree.
  useEffect(() => {
    const un = events.onMenuPick((id) => {
      switch (id) {
        case "new-chat":
          newConversation();
          go("chat");
          break;
        case "add-project":
          addProject();
          break;
        case "projects":
          go("projects");
          break;
        case "settings":
          go("settings");
          break;
        case "palette":
          setPalette(true);
          break;
        case "home":
          go("home");
          break;
        case "toggle-theme":
          saveSettings({ theme: settings?.theme === "light" ? "dark" : "light" });
          break;
        case "trees":
          go("trees");
          break;
        case "activity":
          go("activity");
          break;
        case "claude-terminal":
          api.openClaudeTerminal().catch(() => {});
          break;
      }
    });
    return () => {
      un.then((off) => off()).catch(() => {});
    };
  }, [go, newConversation, addProject, saveSettings, settings?.theme]);

  // The Help lines describe the world, so they have to be told when it moves.
  useEffect(() => {
    if (!status && !settings) return;
    api
      .syncMenu(
        status?.claude.logged_in ? "Claude is signed in" : "Sign in to Claude…",
        status?.claude.cli_version
          ? `Claude CLI ${status.claude.cli_version}`
          : "Claude CLI not found",
        settings ? `Daily budget ${money(settings.daily_budget_usd)}` : "No settings",
      )
      .catch(() => {});
  }, [
    status?.claude.logged_in,
    status?.claude.cli_version,
    settings?.daily_budget_usd,
    status,
    settings,
  ]);

  const back = useCallback(
    () => setHistory((h) => ({ ...h, at: Math.max(0, h.at - 1) })),
    [],
  );
  const forward = useCallback(
    () => setHistory((h) => ({ ...h, at: Math.min(h.list.length - 1, h.at + 1) })),
    [],
  );

  const openRun = useCallback(
    (cardId: string) => {
      setSelectedCard(cardId);
      go("sessions");
    },
    [go],
  );

  const openReview = useCallback(
    (cardId: string) => {
      setReviewCard(cardId);
      go("review");
    },
    [go],
  );

  const openAgent = useCallback(
    (id: string) => {
      setAgentId(id);
      go("agents");
    },
    [go],
  );

  /** Open the chat screen: a stored conversation, a profile's standing chat, or
   *  whatever was already on screen. */
  const openChat = useCallback(
    (conversationId?: string, profileId?: string) => {
      if (conversationId) openConversation(conversationId);
      else if (profileId) chatWithProfile(profileId);
      go("chat");
    },
    [chatWithProfile, go, openConversation],
  );

  // The Director can take the operator somewhere: it calls open_screen, the
  // shell follows. Screen names are its vocabulary, mapped to real views here.
  useEffect(() => {
    if (!navigation) return;
    const map: Record<string, View> = {
      home: "home",
      chat: "chat",
      director: "chat",
      board: "board",
      work: "board",
      review: "review",
      runs: "sessions",
      sessions: "sessions",
      sessions_list: "sessions",
      code: "code",
      project: "code",
      trees: "trees",
      worktrees: "trees",
      log: "activity",
      activity: "activity",
      agents: "agents",
      projects: "projects",
      settings: "settings",
    };
    const next = map[navigation.screen.toLowerCase()];
    if (next) {
      if (navigation.card_id) {
        setSelectedCard(navigation.card_id);
        setReviewCard(navigation.card_id);
      }
      go(next);
    }
    clearNavigation();
  }, [navigation, clearNavigation, go]);

  // A permission request has to be answerable from wherever you are. Chat
  // draws the amber card inline, so it does not need the sheet on top of it;
  // everywhere else the sheet takes the front.
  useEffect(() => {
    if (approvals.length > 0 && view !== "chat" && !palette) setApprovalSheet(true);
    if (approvals.length === 0) setApprovalSheet(false);
  }, [approvals.length, view, palette]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
        return;
      }
      if (meta && e.key.toLowerCase() === "j") {
        e.preventDefault();
        go("chat");
        return;
      }
      if (meta && (e.key === "[" || (e.altKey && e.key === "ArrowLeft"))) {
        e.preventDefault();
        back();
        return;
      }
      if (meta && (e.key === "]" || (e.altKey && e.key === "ArrowRight"))) {
        e.preventDefault();
        forward();
        return;
      }
      if (meta && e.key === ",") {
        e.preventDefault();
        go("settings");
        return;
      }
      if (e.key !== "Escape") return;
      if (palette) setPalette(false);
      else if (approvalSheet) setApprovalSheet(false);

    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [approvalSheet, back, forward, go, palette]);

  const cards = snapshot?.cards ?? [];

  const actions = useMemo<PaletteAction[]>(() => {
    const screens: View[] = [
      ...NAV_VIEWS,
      "review",
      "agents",
      "activity",
      "trees",
      "projects",
      "settings",
    ];
    const list: PaletteAction[] = screens.map((v) => ({
      name: VIEW_TITLES[v],
      hint: "screen",
      tone: TONE.accent,
      run: () => go(v),
    }));

    list.push({
      name: "New chat",
      hint: "action",
      tone: TONE.info,
      run: () => {
        newConversation();
        go("chat");
      },
    });
    list.push({ name: "Add a project", hint: "action", tone: TONE.ok, run: addProject });

    conversations.forEach((c) =>
      list.push({
        name: c.title,
        hint: "chat",
        tone: TONE.accent,
        run: () => openChat(c.id),
      }),
    );
    projects.forEach((p) =>
      list.push({
        name: p.name,
        hint: "project",
        tone: tone(p.tone),
        run: () => {
          selectProject(p.id);
          go("board");
        },
      }),
    );
    agents.forEach((a) =>
      list.push({
        name: a.name,
        hint: "agent",
        tone: tone(a.tone),
        run: () => openAgent(a.id),
      }),
    );
    cards.forEach((c) =>
      list.push({
        name: c.title,
        hint: c.id,
        tone: STATUS_TONE[c.status],
        run: () => (c.status === "review" ? openReview(c.id) : openRun(c.id)),
      }),
    );
    cards
      .filter((c) => c.status === "ready")
      .forEach((c) =>
        list.push({
          name: `Start: ${c.title}`,
          hint: "run",
          tone: TONE.info,
          run: () => startRun(c.id),
        }),
      );
    return list;
  }, [
    addProject,
    agents,
    cards,
    conversations,
    go,
    newConversation,
    openAgent,
    openChat,
    openReview,
    openRun,
    projects,
    selectProject,
    startRun,
  ]);

  if (fatal) {
    return (
      <div className="max-w-[640px] p-10">
        <h1 className="text-[20px] font-bold tracking-[-.02em]">Relay could not start</h1>
        <pre className="whitespace-pre-wrap rounded-lg border border-line3 bg-surface px-4 py-3.5 font-mono text-md dark:border-line3-d dark:bg-surface-d">
          {fatal}
        </pre>
        <p className="text-md text-text3 dark:text-text3-d">
          The backend refused the first call. Check the terminal Relay was started from.
        </p>
      </div>
    );
  }

  if (!ready) {
    return (
      <div className="grid h-full place-items-center">
        <Loading what="Starting Relay" />
      </div>
    );
  }

  // With no project there is no board, no history and no sessions: every one of
  // those screens becomes the on-ramp instead of an empty panel.
  const needsProject =
    view === "board" || view === "sessions" || view === "review" || view === "activity" ||
    view === "trees" || view === "code";
  const firstRun = !project && needsProject;

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-canvas dark:bg-canvas-d">
      <TitleBar view={view} go={go} onPalette={() => setPalette(true)} />

      {status && !status.ready && status.blocker && (
        <div
          className={cx(
            "flex flex-none items-center gap-3 border-b border-line px-4.5 py-2 text-sm font-medium dark:border-line-d",
            status.claude.logged_in
              ? "bg-warnSoft text-warn dark:bg-warnSoft-d dark:text-warn-d"
              : "bg-badSoft text-bad2 dark:bg-badSoft-d dark:text-bad2-d",
          )}
        >
          <span>{status.blocker}</span>
          <span className="flex-1" />
          {!status.claude.logged_in && (
            <button
              type="button"
              onClick={() => api.openClaudeTerminal().catch(() => {})}
              className="min-h-6 cursor-pointer rounded-full border border-current bg-transparent px-3 py-1.5 text-sm font-semibold transition-colors duration-150 hover:bg-[rgba(127,127,127,.12)]"
            >
              Open a terminal
            </button>
          )}
          {status.claude.logged_in && !status.sidecar.ready && status.sidecar.node_found && (
            <button
              type="button"
              onClick={installSidecar}
              className="min-h-6 cursor-pointer rounded-full border border-current bg-transparent px-3 py-1.5 text-sm font-semibold transition-colors duration-150 hover:bg-[rgba(127,127,127,.12)]"
            >
              Install the sidecar
            </button>
          )}
        </div>
      )}

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <Sidebar view={view} go={go} openChat={openChat} />

        {/* The pane. Every screen scrolls inside it against an inner canvas
            floor of 880–960px, so nothing collapses and nothing gets a private
            scrollbar. Chat is the exception the design names: its pane does
            not scroll — the thread does, inside its own card. */}
        <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
          {firstRun ? (
            <div className="min-h-0 flex-1 overflow-y-auto">
              <FirstRun openChat={() => go("chat")} />
            </div>
          ) : (
            <>
              <BuildBanner />
              {view === "home" && <Home go={go} openRun={openRun} />}
              {view === "chat" && <Chat />}
              {view === "board" && <Board openRun={openRun} openReview={openReview} />}
              {view === "code" && <Code />}
              {view === "sessions" && (
                <Sessions selected={selectedCard} select={setSelectedCard} openReview={openReview} />
              )}
              {view === "review" && <Review selected={reviewCard} select={setReviewCard} />}
              {view === "agents" && (
                <Agents
                  selected={agentId}
                  select={setAgentId}
                  openChat={openChat}
                  openSession={openRun}
                />
              )}
              {(view === "activity" ||
                view === "trees" ||
                view === "projects" ||
                view === "settings") && (
                <div className="min-h-0 flex-1 overflow-y-auto">
                  {view === "activity" && <Activity openRun={openRun} />}
                  {view === "trees" && <Worktrees />}
                  {view === "projects" && <Projects go={go} />}
                  {view === "settings" && <Settings />}
                </div>
              )}
            </>
          )}
        </main>
      </div>

      {/* Never a scrim and never modal: the whole point of the sheets is that
          an update cannot stand between the operator and a running agent. */}
      <UpdateSheets />
      <CommandPalette open={palette} close={() => setPalette(false)} actions={actions} />
      <AnimatePresence>
        {approvalSheet && <ApprovalSheet key="approval" close={() => setApprovalSheet(false)} />}
      </AnimatePresence>
      <Toasts />
      {/* Above everything, including the palette and the approval sheet: once
          the window is going, nothing else is actionable. */}
      <ClosingOverlay />
    </div>
  );
}

export default function App() {
  return (
    // `reducedMotion="user"` é o lado do `motion` da mesma preferência que o
    // bloco `@media (prefers-reduced-motion)` do `styles/app.css` trata pelo
    // lado do CSS. Movimento a menos onde foi pedido, nos dois caminhos.
    <MotionConfig reducedMotion="user">
      <StoreProvider>
        {/* Outside `Shell` on purpose: it reports through the loading and the
            fatal states too, and those are exactly the ones where the splash
            has to be told something. */}
        <SplashHandoff />
        <Shell />
      </StoreProvider>
    </MotionConfig>
  );
}
