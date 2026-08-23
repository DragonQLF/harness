import { useCallback, useEffect, useMemo, useState } from "react";
import { TitleBar } from "./components/TitleBar";
import { NavRail } from "./components/NavRail";
import {
  ApprovalSheet,
  CommandPalette,
  DirectorDock,
  DirectorRail,
  RejectSheet,
  Toasts,
  type PaletteAction,
} from "./components/Overlays";
import { Loading } from "./components/ui";
import { api } from "./lib/ipc";
import { STATUS_TONE, tone } from "./lib/types";
import { StoreProvider, useStore } from "./state/store";
import { Overview } from "./views/Overview";
import { FirstRun } from "./views/FirstRun";
import { Board } from "./views/Board";
import { Sessions } from "./views/Sessions";
import { AgentDrawer, AgentList } from "./views/Agents";
import { ProjectPage, Projects } from "./views/Projects";
import { Activity, DirectorPage, Settings, Worktrees } from "./views/Misc";
import { VIEW_TITLES, type View } from "./views/views";
import "./styles/theme.css";

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
    addProject,
    selectProject,
    installSidecar,
    startRun,
    navigation,
    clearNavigation,
  } = useStore();

  const [view, setView] = useState<View>("home");
  const [agentId, setAgentId] = useState<string | null>(null);
  const [selectedCard, setSelectedCard] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [approvalSheet, setApprovalSheet] = useState(false);
  const [dock, setDock] = useState(false);

  const go = useCallback((v: View) => setView(v), []);

  const openRun = useCallback((cardId: string) => {
    setSelectedCard(cardId);
    setView("runs");
  }, []);

  const openAgent = useCallback((id: string) => {
    setAgentId(id);
    setView("agent");
  }, []);

  // The Director can take the operator somewhere: it calls open_screen, the
  // shell follows. Screen names are its vocabulary, mapped to real views here.
  useEffect(() => {
    if (!navigation) return;
    const map: Record<string, View> = {
      home: "home",
      board: "board",
      work: "board",
      runs: "runs",
      sessions: "runs",
      code: "project",
      project: "project",
      trees: "trees",
      worktrees: "trees",
      sessions_list: "runs",
      log: "log",
      activity: "log",
      agents: "agents",
      projects: "projects",
      settings: "settings",
      director: "director",
    };
    const view = map[navigation.screen.toLowerCase()];
    if (view) {
      if (navigation.card_id) setSelectedCard(navigation.card_id);
      setView(view);
    }
    clearNavigation();
  }, [navigation, clearNavigation]);

  // A permission request takes the front when nothing else is open.
  useEffect(() => {
    if (approvals.length > 0 && !palette && !rejecting) setApprovalSheet(true);
    if (approvals.length === 0) setApprovalSheet(false);
  }, [approvals.length, palette, rejecting]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
        return;
      }
      if (e.key !== "Escape") return;
      if (palette) setPalette(false);
      else if (rejecting) setRejecting(null);
      else if (approvalSheet) setApprovalSheet(false);
      else if (view === "agent") setView("agents");
      else if (dock) setDock(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [approvalSheet, dock, palette, rejecting, view]);

  const actions = useMemo<PaletteAction[]>(() => {
    const screens: View[] = [
      "home",
      "director",
      "project",
      "projects",
      "agents",
      "board",
      "runs",
      "trees",
      "log",
      "settings",
    ];
    const list: PaletteAction[] = screens.map((v) => ({
      name: VIEW_TITLES[v],
      hint: "screen",
      color: "var(--accent)",
      run: () => setView(v),
    }));

    list.push({
      name: "Ask the Director",
      hint: "panel",
      color: "var(--info)",
      run: () => setDock(true),
    });
    list.push({ name: "Add a project", hint: "action", color: "var(--ok)", run: addProject });

    projects.forEach((p) =>
      list.push({
        name: p.name,
        hint: "project",
        color: tone(p.tone).color,
        run: () => {
          selectProject(p.id);
          setView("board");
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
    (snapshot?.cards ?? []).forEach((c) =>
      list.push({
        name: c.title,
        hint: c.id,
        color: STATUS_TONE[c.status].color,
        run: () => openRun(c.id),
      }),
    );
    (snapshot?.cards ?? [])
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
  }, [addProject, agents, openAgent, openRun, projects, selectProject, snapshot, startRun]);

  if (fatal) {
    return (
      <div style={{ padding: 40, maxWidth: 640 }}>
        <h1 style={{ fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>
          Harness could not start
        </h1>
        <pre
          style={{
            padding: "14px 16px",
            borderRadius: 14,
            background: "var(--surface2)",
            border: "1px solid var(--line)",
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
      <div style={{ height: "100%", display: "flex", alignItems: "center", justifyContent: "center" }}>
        <Loading what="Starting Harness" />
      </div>
    );
  }

  // With no project there is no board, no history and no sessions: every one of
  // those screens becomes the on-ramp instead of an empty panel.
  const needsProject =
    view === "home" ||
    view === "board" ||
    view === "runs" ||
    view === "log" ||
    view === "trees" ||
    view === "project";
  const firstRun = !project && needsProject;

  return (
    <div
      style={{
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: "var(--bg)",
        overflow: "hidden",
        position: "relative",
      }}
    >
      <TitleBar onPalette={() => setPalette(true)} onApprovals={() => setApprovalSheet(true)} />

      {status && !status.ready && status.blocker && (
        <div
          style={{
            flex: "none",
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "9px 26px",
            background: status.claude.logged_in ? "var(--warnSoft)" : "var(--badSoft)",
            color: status.claude.logged_in ? "var(--warn)" : "var(--bad)",
            borderBottom: "1px solid var(--line)",
            fontSize: 12.5,
            fontWeight: 600,
          }}
        >
          <span>{status.blocker}</span>
          <span style={{ flex: 1 }} />
          {!status.claude.logged_in && (
            <button
              type="button"
              className="hv-bright"
              onClick={() => api.openClaudeTerminal().catch(() => {})}
              style={{
                padding: "6px 13px",
                border: "1px solid currentColor",
                borderRadius: 999,
                background: "transparent",
                color: "inherit",
                fontSize: 12,
                fontWeight: 700,
                cursor: "pointer",
              }}
            >
              Open a terminal
            </button>
          )}
          {status.claude.logged_in && !status.sidecar.ready && status.sidecar.node_found && (
            <button
              type="button"
              className="hv-bright"
              onClick={installSidecar}
              style={{
                padding: "6px 13px",
                border: "1px solid currentColor",
                borderRadius: 999,
                background: "transparent",
                color: "inherit",
                fontSize: 12,
                fontWeight: 700,
                cursor: "pointer",
              }}
            >
              Install the sidecar
            </button>
          )}
        </div>
      )}

      <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
        <NavRail view={view} go={go} />

        <main
          style={{
            position: "relative",
            flex: 1,
            minWidth: 0,
            display: "flex",
            flexDirection: "column",
            background: "var(--bg)",
            overflow: "hidden",
          }}
        >
          <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
            {firstRun ? (
              <FirstRun openChat={() => setDock(true)} />
            ) : (
              <>
                {view === "home" && (
                  <Overview
                    go={go}
                    openRun={openRun}
                    openAgent={openAgent}
                    openReject={setRejecting}
                    openApprovals={() => setApprovalSheet(true)}
                  />
                )}
                {view === "director" && <DirectorPage go={go} openChat={() => setDock(true)} />}
                {view === "projects" && <Projects go={go} />}
                {view === "project" && <ProjectPage go={go} />}
                {(view === "agents" || view === "agent") && (
                  <AgentList open={openAgent} go={go} openChat={() => setDock(true)} />
                )}
                {view === "board" && <Board openRun={openRun} openReject={setRejecting} />}
                {view === "runs" && (
                  <Sessions
                    selected={selectedCard}
                    select={setSelectedCard}
                    openReject={setRejecting}
                  />
                )}
                {view === "trees" && <Worktrees />}
                {view === "log" && <Activity openRun={openRun} />}
                {view === "settings" && <Settings />}
              </>
            )}
          </div>

          {view === "agent" && agentId && (
            <AgentDrawer
              agentId={agentId}
              close={() => setView("agents")}
              openRun={openRun}
              go={go}
              openChat={() => setDock(true)}
            />
          )}
        </main>

        {dock ? <DirectorDock close={() => setDock(false)} /> : <DirectorRail open={() => setDock(true)} />}
      </div>

      <CommandPalette open={palette} close={() => setPalette(false)} actions={actions} />
      {approvalSheet && <ApprovalSheet close={() => setApprovalSheet(false)} />}
      <RejectSheet cardId={rejecting} close={() => setRejecting(null)} />
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
