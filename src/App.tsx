import { useCallback, useEffect, useMemo, useState } from "react";
import { check, type Update as Release } from "@tauri-apps/plugin-updater";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { relaunch } from "@tauri-apps/plugin-process";
import { TitleBar } from "./components/TitleBar";
import { NavRail } from "./components/NavRail";
import { RightNow, RightNowStrip } from "./components/RightNow";
import {
  ApprovalSheet,
  CommandPalette,
  Toasts,
  type PaletteAction,
} from "./components/Overlays";
import { Icon, Loading, Spinner, mono, truncate } from "./components/ui";
import { cx } from "./lib/cx";
import { sheetIn, veil } from "./lib/motion";
import { api, events, reason } from "./lib/ipc";
import { ago, money, plural } from "./lib/format";
import { STATUS_TONE, TONE, tone } from "./lib/types";
import type { ClosingBegan, ClosingPhase, PendingUpdate } from "./lib/types";
import { StoreProvider, useStore } from "./state/store";
import { Chat } from "./views/Chat";
import { Review } from "./views/Review";
import { FirstRun } from "./views/FirstRun";
import { Board } from "./views/Board";
import { Sessions } from "./views/Sessions";
import { Agents } from "./views/Agents";
import { ProjectPage, Projects } from "./views/Projects";
import { Activity, Settings, Worktrees } from "./views/Misc";
import { VIEW_TITLES, type View } from "./views/views";
import "./styles/app.css";

/** Uma pastilha de contorno no cabeçalho. */
const CHIP =
  "min-h-6 cursor-pointer rounded-sm border border-line3 bg-transparent text-text2 transition-[border-color,background,color] duration-150 hover:border-line4 hover:bg-surface2 hover:text-text dark:border-line3-d dark:text-text2-d dark:hover:border-line4-d dark:hover:bg-surface2-d dark:hover:text-text-d";

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
 *  — the rollback machinery decides whether the next start keeps it. */
function UpdateBanner() {
  const { toast } = useStore();
  const [pending, setPending] = useState<PendingUpdate[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [release, setRelease] = useState<Release | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [feedError, setFeedError] = useState<string | null>(null);

  // A published version, from the release feed rather than from anything on
  // this machine. This is what makes an update a version instead of a file: it
  // is the same answer on every device, and the signature is checked before a
  // byte of it runs.
  useEffect(() => {
    let alive = true;
    const look = async () => {
      try {
        const found = await check();
        if (alive) {
          setRelease(found);
          setFeedError(null);
        }
      } catch (e) {
        // Silence here cost an evening: the feed was answering 404 because the
        // repository is private, and the app had no way to say so. A failure
        // the operator cannot see is a failure they cannot fix. It is still not
        // worth a toast on every transient hiccup, so it lands in Settings
        // beside the rest of the system's state.
        if (alive) setFeedError(reason(e));
      }
    };
    look();
    const every = setInterval(look, 3 * 60 * 60 * 1000);
    window.addEventListener("focus", look);
    return () => {
      alive = false;
      window.removeEventListener("focus", look);
      clearInterval(every);
    };
  }, []);

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

  if (release && release.version !== dismissed) {
    const install = async () => {
      setBusy(true);
      setProgress(0);
      try {
        let got = 0;
        let total = 0;
        await release.downloadAndInstall((event) => {
          if (event.event === "Started") total = event.data.contentLength ?? 0;
          else if (event.event === "Progress") {
            got += event.data.chunkLength;
            if (total) setProgress(Math.round((got / total) * 100));
          }
        });
        await relaunch();
      } catch (e) {
        setBusy(false);
        setProgress(null);
        toast("bad", "Could not install the update", reason(e));
      }
    };
    return (
      <Banner
        label={`Relay ${release.version} is available`}
        detail={
          busy
            ? progress === null
              ? "installing…"
              : `downloading ${progress}%`
            : "installing restarts the app"
        }
        busy={busy}
        action={busy ? "installing…" : "Install & restart"}
        onInstall={install}
        onLater={() => setDismissed(release.version)}
      />
    );
  }

  // A feed that cannot be reached is worth one quiet line, not a banner: it is
  // the difference between "you are up to date" and "nobody knows".
  if (feedError && !pending?.length) {
    return (
      <div
        className={cx(
          mono,
          "flex items-center gap-2 border-b border-line bg-surface px-4 py-1.5 text-xs text-text4 dark:border-line-d dark:bg-surface-d dark:text-text4-d",
        )}
        title={feedError}
      >
        <span>could not check for updates — {feedError}</span>
      </div>
    );
  }

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

/** The one strip, whichever source the update came from. A version from the
 *  release feed and a build a card produced are the same offer to the operator:
 *  something newer exists, here is what it is, install or not. */
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

/** The rail is about work in flight. The two screens that already are a wall of
 *  work do not need it beside them. */
const RAIL_HIDDEN: View[] = ["board", "agents"];

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
    conversation,
    conversations,
    addProject,
    selectProject,
    installSidecar,
    startRun,
    newConversation,
    openConversation,
    chatWithProfile,
    renameConversation,
    archiveConversation,
    deleteConversation,
    navigation,
    clearNavigation,
    settings,
    saveSettings,
  } = useStore();

  // One history of screens, so the title bar's arrows mean something.
  const [history, setHistory] = useState<{ list: View[]; at: number }>({ list: ["chat"], at: 0 });
  const view = history.list[history.at] ?? "chat";

  const [agentId, setAgentId] = useState<string | null>(null);
  const [selectedCard, setSelectedCard] = useState<string | null>(null);
  const [reviewCard, setReviewCard] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [approvalSheet, setApprovalSheet] = useState(false);
  const [sidebar, setSidebar] = useState(true);
  const [rail, setRail] = useState(true);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");

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
        case "toggle-sidebar":
          setSidebar((v) => !v);
          break;
        case "toggle-rail":
          setRail((v) => !v);
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
      home: "chat",
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

  // A permission request has to be answerable from wherever you are. The rail
  // shows it when the rail is there; when it is not, the sheet takes the front.
  const railVisible = rail && !RAIL_HIDDEN.includes(view);
  useEffect(() => {
    if (approvals.length > 0 && !railVisible && !palette) setApprovalSheet(true);
    if (approvals.length === 0) setApprovalSheet(false);
  }, [approvals.length, railVisible, palette]);

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
      if (meta && e.key === ",") {
        e.preventDefault();
        go("settings");
        return;
      }
      if (e.key !== "Escape") return;
      if (palette) setPalette(false);
      else if (approvalSheet) setApprovalSheet(false);
      else if (renaming) setRenaming(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [approvalSheet, go, palette, renaming]);

  const cards = snapshot?.cards ?? [];
  const running = cards.filter((c) => c.status === "running").length;
  const reviewing = cards.filter((c) => c.status === "review").length;
  const recorded = cards.filter((c) => c.runs > 0 || c.status === "running").length;

  const actions = useMemo<PaletteAction[]>(() => {
    const screens: View[] = [
      "chat",
      "review",
      "board",
      "sessions",
      "agents",
      "code",
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

  const speaker = agents.find((a) => a.id === (conversation?.profile_id ?? "director"));
  const pinned = projects.find((p) => p.id === conversation?.project_id);

  const heads: Record<View, [string, string]> = {
    chat: [
      conversation?.title && conversation.title !== "New conversation"
        ? conversation.title
        : `Ask ${speaker?.name ?? "the Director"}`,
      [
        speaker?.name ?? "Director",
        pinned ? `pinned to ${pinned.name}` : "no project pinned",
        conversation ? plural(conversation.messages, "message") : "nothing said yet",
        conversation ? money(conversation.cost_usd, 2) : money(0, 2),
      ].join(" · "),
    ],
    review: [
      "Review",
      reviewing === 0
        ? "nothing waiting on you"
        : `${plural(reviewing, "finished run")} · the reviewer read them first`,
    ],
    board: [
      "Board",
      `${project?.name ?? "no project"} · ${plural(cards.length, "card")} · ${running} running`,
    ],
    sessions: [
      "Sessions",
      `${recorded} recorded · ${running} live · replayed from disk`,
    ],
    agents: ["Agents", `${plural(agents.length, "profile")} · the crew that can be given cards`],
    code: ["Code", project ? `${project.name} · ${project.base_branch}` : "no project"],
    activity: ["Activity", `every board change, newest first`],
    trees: ["Worktrees", "one checkout per card, until you remove it"],
    projects: ["Projects", plural(projects.length, "repository")],
    settings: ["Settings", "what Relay may do without asking"],
  };
  const [headTitle, headMeta] = heads[view];

  return (
    <div className="relative flex h-full flex-col overflow-hidden bg-recess dark:bg-recess-d">
      <TitleBar
        go={go}
        back={back}
        forward={forward}
        canBack={history.at > 0}
        canForward={history.at < history.list.length - 1}
        toggleSidebar={() => setSidebar((v) => !v)}
        toggleRail={() => setRail((v) => !v)}
        onPalette={() => setPalette(true)}
        onNewChat={() => {
          newConversation();
          go("chat");
        }}
      />

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
        {sidebar && (
          <NavRail
            view={view}
            go={go}
            openChat={openChat}
            onPalette={() => setPalette(true)}
            onApprovals={() => setApprovalSheet(true)}
          />
        )}

        <main
          className={cx(
            "flex min-w-0 flex-1 flex-col overflow-hidden bg-bg dark:bg-bg-d",
            railVisible && "border-r border-line dark:border-line-d",
          )}
        >
          <div className="flex h-[46px] flex-none items-center gap-2.5 border-b border-line px-4.5 dark:border-line-d">
            {renaming && conversation ? (
              <input
                autoFocus
                value={draft}
                aria-label="Rename this conversation"
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => {
                  if (draft.trim()) renameConversation(conversation.id, draft.trim());
                  setRenaming(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    if (draft.trim()) renameConversation(conversation.id, draft.trim());
                    setRenaming(false);
                  }
                }}
                className="max-w-[420px] flex-1 rounded-sm border border-accentLine bg-surface px-2.5 py-1.5 text-md font-semibold text-text outline-none dark:border-accentLine-d dark:bg-surface-d dark:text-text-d"
              />
            ) : (
              <>
                <span
                  className={cx(
                    truncate,
                    "max-w-[460px] text-lg font-semibold tracking-[-.01em] text-text dark:text-text-d",
                  )}
                >
                  {headTitle}
                </span>
                <span className={cx(mono, truncate, "text-xs text-text4 dark:text-text4-d")}>
                  {headMeta}
                </span>
              </>
            )}
            <div className="flex-1" />

            {view === "chat" && conversation && (
              <>
                <span
                  title={
                    conversation.session_id
                      ? "The Claude session this chat continues"
                      : "No session yet — your next message starts one"
                  }
                  className={cx(
                    mono,
                    "flex cursor-default items-center gap-2 rounded-full border border-line3 bg-surface px-2.5 py-1 text-xs dark:border-line3-d dark:bg-surface-d",
                    conversation.resume_failed
                      ? "text-bad2 dark:text-bad2-d"
                      : "text-text2 dark:text-text2-d",
                  )}
                >
                  {conversation.session_id
                    ? `${conversation.session_id.slice(0, 12)} · ${
                        conversation.resume_failed ? "resume refused" : "resumed"
                      }`
                    : "no session yet"}
                </span>
                {[
                  {
                    label: "Rename",
                    icon: <Icon.pencil />,
                    run: () => {
                      setDraft(conversation.title);
                      setRenaming(true);
                    },
                  },
                  {
                    label: conversation.archived ? "Restore" : "Archive",
                    icon: <Icon.archive />,
                    run: () => archiveConversation(conversation.id, !conversation.archived),
                  },
                  {
                    label: "Delete",
                    icon: <Icon.close />,
                    run: () => deleteConversation(conversation.id),
                  },
                ].map((b) => (
                  <button
                    key={b.label}
                    type="button"
                    title={b.label}
                    aria-label={b.label}
                    onClick={b.run}
                    className={cx(CHIP, "grid h-6 w-6 place-items-center")}
                  >
                    {b.icon}
                  </button>
                ))}
              </>
            )}

            <button
              type="button"
              title="New chat"
              aria-label="New chat"
              onClick={() => {
                newConversation();
                go("chat");
              }}
              className={cx(CHIP, "grid h-6 w-6 place-items-center")}
            >
              <Icon.plus />
            </button>
            {!railVisible && !RAIL_HIDDEN.includes(view) && (
              <button
                type="button"
                title="Right now"
                onClick={() => setRail(true)}
                className={cx(CHIP, "rounded-full px-2.5 py-1 text-xs font-medium")}
              >
                Right now
              </button>
            )}
          </div>

          {firstRun ? (
            <div className="min-h-0 flex-1 overflow-y-auto">
              <FirstRun openChat={() => go("chat")} />
            </div>
          ) : (
            <>
              <UpdateBanner />
              {view === "chat" && <Chat />}
              {view === "review" && <Review selected={reviewCard} select={setReviewCard} />}
              {view === "board" && <Board openRun={openRun} openReview={openReview} />}
              {view === "sessions" && (
                <Sessions selected={selectedCard} select={setSelectedCard} openReview={openReview} />
              )}
              {view === "agents" && (
                <Agents
                  selected={agentId}
                  select={setAgentId}
                  openChat={openChat}
                  openSession={openRun}
                />
              )}
              {(view === "code" || view === "activity" || view === "trees" || view === "projects" ||
                view === "settings") && (
                <div className="min-h-0 flex-1 overflow-y-auto">
                  {view === "code" && <ProjectPage go={go} />}
                  {view === "activity" && <Activity openRun={openRun} />}
                  {view === "trees" && <Worktrees />}
                  {view === "projects" && <Projects go={go} />}
                  {view === "settings" && <Settings />}
                </div>
              )}
            </>
          )}
        </main>

        {/* O rail sai por onde entrou: é o `AnimatePresence` que lhe dá a
            saída, e é isso que o CSS nunca conseguiu animar. */}
        <AnimatePresence mode="wait">
          {railVisible ? (
            <RightNow
              key="rail"
              close={() => setRail(false)}
              openReview={openReview}
              openSession={openRun}
              openTrees={() => go("trees")}
            />
          ) : (
            !RAIL_HIDDEN.includes(view) && <RightNowStrip key="strip" open={() => setRail(true)} />
          )}
        </AnimatePresence>
      </div>

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
        <Shell />
      </StoreProvider>
    </MotionConfig>
  );
}
