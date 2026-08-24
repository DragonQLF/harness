import { useCallback, useEffect, useMemo, useState } from "react";
import { TitleBar } from "./components/TitleBar";
import { NavRail } from "./components/NavRail";
import { RightNow, RightNowStrip } from "./components/RightNow";
import {
  ApprovalSheet,
  CommandPalette,
  Toasts,
  type PaletteAction,
} from "./components/Overlays";
import { Icon, Loading, mono, truncate } from "./components/ui";
import { api } from "./lib/ipc";
import { money, plural } from "./lib/format";
import { STATUS_TONE, tone } from "./lib/types";
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
import "./styles/theme.css";

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
      color: "var(--accent)",
      run: () => go(v),
    }));

    list.push({
      name: "New chat",
      hint: "action",
      color: "var(--info)",
      run: () => {
        newConversation();
        go("chat");
      },
    });
    list.push({ name: "Add a project", hint: "action", color: "var(--ok)", run: addProject });

    conversations.forEach((c) =>
      list.push({
        name: c.title,
        hint: "chat",
        color: "var(--accent2)",
        run: () => openChat(c.id),
      }),
    );
    projects.forEach((p) =>
      list.push({
        name: p.name,
        hint: "project",
        color: tone(p.tone).color,
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
        color: tone(a.tone).color,
        run: () => openAgent(a.id),
      }),
    );
    cards.forEach((c) =>
      list.push({
        name: c.title,
        hint: c.id,
        color: STATUS_TONE[c.status].color,
        run: () => (c.status === "review" ? openReview(c.id) : openRun(c.id)),
      }),
    );
    cards
      .filter((c) => c.status === "ready")
      .forEach((c) =>
        list.push({
          name: `Start: ${c.title}`,
          hint: "run",
          color: "var(--info)",
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
      <div style={{ padding: 40, maxWidth: 640 }}>
        <h1 style={{ fontSize: 20, fontWeight: 700, letterSpacing: "-.02em" }}>
          Harness could not start
        </h1>
        <pre
          style={{
            padding: "14px 16px",
            borderRadius: 14,
            background: "var(--surface)",
            border: "1px solid var(--line3)",
            fontFamily: "var(--mono)",
            fontSize: 12,
            whiteSpace: "pre-wrap",
          }}
        >
          {fatal}
        </pre>
        <p style={{ fontSize: 12.5, color: "var(--text3)" }}>
          The backend refused the first call. Check the terminal Harness was started from.
        </p>
      </div>
    );
  }

  if (!ready) {
    return (
      <div style={{ height: "100%", display: "grid", placeItems: "center" }}>
        <Loading what="Starting Harness" />
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
    settings: ["Settings", "what Harness may do without asking"],
  };
  const [headTitle, headMeta] = heads[view];

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: "var(--recess)",
        overflow: "hidden",
        position: "relative",
      }}
    >
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
          style={{
            flex: "none",
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "8px 18px",
            background: status.claude.logged_in ? "var(--warnSoft)" : "var(--badSoft)",
            color: status.claude.logged_in ? "var(--warn)" : "var(--bad2)",
            borderBottom: "1px solid var(--line)",
            font: "500 11.5px var(--sans)",
          }}
        >
          <span>{status.blocker}</span>
          <span style={{ flex: 1 }} />
          {!status.claude.logged_in && (
            <span
              className="quiet"
              onClick={() => api.openClaudeTerminal().catch(() => {})}
              style={{
                padding: "5px 12px",
                border: "1px solid currentColor",
                borderRadius: 999,
                font: "600 11px var(--sans)",
                cursor: "pointer",
              }}
            >
              Open a terminal
            </span>
          )}
          {status.claude.logged_in && !status.sidecar.ready && status.sidecar.node_found && (
            <span
              className="quiet"
              onClick={installSidecar}
              style={{
                padding: "5px 12px",
                border: "1px solid currentColor",
                borderRadius: 999,
                font: "600 11px var(--sans)",
                cursor: "pointer",
              }}
            >
              Install the sidecar
            </span>
          )}
        </div>
      )}

      <div style={{ flex: 1, minHeight: 0, display: "flex", overflow: "hidden" }}>
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
          style={{
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
            background: "var(--bg)",
            borderRight: railVisible ? "1px solid var(--line)" : undefined,
            overflow: "hidden",
          }}
        >
          <div
            style={{
              flex: "none",
              height: 46,
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "0 18px",
              borderBottom: "1px solid var(--line)",
            }}
          >
            {renaming && conversation ? (
              <input
                autoFocus
                value={draft}
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
                style={{
                  flex: 1,
                  maxWidth: 420,
                  padding: "5px 9px",
                  borderRadius: 8,
                  border: "1px solid var(--accentLine)",
                  background: "var(--surface)",
                  font: "600 13px var(--sans)",
                  color: "var(--text)",
                  outline: "none",
                }}
              />
            ) : (
              <>
                <span
                  style={{
                    font: "600 13.5px var(--sans)",
                    color: "var(--text)",
                    letterSpacing: "-.01em",
                    maxWidth: 460,
                    ...truncate,
                  }}
                >
                  {headTitle}
                </span>
                <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)", ...truncate }}>
                  {headMeta}
                </span>
              </>
            )}
            <div style={{ flex: 1 }} />

            {view === "chat" && conversation && (
              <>
                <span
                  className="chip"
                  title={
                    conversation.session_id
                      ? "The Claude session this chat continues"
                      : "No session yet — your next message starts one"
                  }
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 7,
                    padding: "4px 10px",
                    borderRadius: 999,
                    background: "var(--surface)",
                    border: "1px solid var(--line3)",
                    ...mono,
                    fontSize: 10.5,
                    color: conversation.resume_failed ? "var(--bad2)" : "var(--text2)",
                    cursor: "default",
                  }}
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
                  <span
                    key={b.label}
                    className="chip"
                    title={b.label}
                    onClick={b.run}
                    style={{
                      display: "grid",
                      placeItems: "center",
                      width: 24,
                      height: 24,
                      borderRadius: 7,
                      border: "1px solid var(--line3)",
                      color: "var(--text2)",
                      cursor: "pointer",
                    }}
                  >
                    {b.icon}
                  </span>
                ))}
              </>
            )}

            <span
              className="chip"
              title="New chat"
              onClick={() => {
                newConversation();
                go("chat");
              }}
              style={{
                display: "grid",
                placeItems: "center",
                width: 24,
                height: 24,
                borderRadius: 7,
                border: "1px solid var(--line3)",
                color: "var(--text2)",
                cursor: "pointer",
              }}
            >
              <Icon.plus />
            </span>
            {!railVisible && !RAIL_HIDDEN.includes(view) && (
              <span
                className="chip"
                title="Right now"
                onClick={() => setRail(true)}
                style={{
                  padding: "4px 10px",
                  borderRadius: 999,
                  border: "1px solid var(--line3)",
                  font: "500 10.5px var(--sans)",
                  color: "var(--text2)",
                  cursor: "pointer",
                }}
              >
                Right now
              </span>
            )}
          </div>

          {firstRun ? (
            <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
              <FirstRun openChat={() => go("chat")} />
            </div>
          ) : (
            <>
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
                <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
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

        {railVisible ? (
          <RightNow
            close={() => setRail(false)}
            openReview={openReview}
            openSession={openRun}
            openTrees={() => go("trees")}
          />
        ) : (
          !RAIL_HIDDEN.includes(view) && <RightNowStrip open={() => setRail(true)} />
        )}
      </div>

      <CommandPalette open={palette} close={() => setPalette(false)} actions={actions} />
      {approvalSheet && <ApprovalSheet close={() => setApprovalSheet(false)} />}
      <Toasts />
    </div>
  );
}

export default function App() {
  return (
    <StoreProvider>
      <Shell />
    </StoreProvider>
  );
}
