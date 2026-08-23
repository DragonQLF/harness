/** The one place frontend state lives.
 *
 *  The backend owns the truth: this store sends intents and re-reads snapshots
 *  when the engine says something changed. It never replays domain rules.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { api, events, reason, type UnlistenFn } from "../lib/ipc";
import type {
  ActivityRow,
  AgentProfile,
  AgentStats,
  Envelope,
  PendingApproval,
  ProjectStats,
  Navigation,
  ProjectView,
  RunLogLine,
  RunUpdate,
  Settings,
  Snapshot,
  Status,
  SystemStatus,
} from "../lib/types";

const DIRECTOR = "director";
const MAX_LINES = 400;

export interface LogLine {
  ts: number;
  kind: RunUpdate["kind"];
  text: string;
  /** CSS colour variable for this line. */
  color: string;
}

export interface ChatMsg {
  role: "user" | "director";
  text: string;
}

/** What is arriving right now for a card, before the final text lands. */
export interface LiveStream {
  text: string;
  thinking: string;
}

export interface Toast {
  id: number;
  tone: string;
  title: string;
  body?: string;
}

/** One shape for both live updates and stored log lines. */
export function toLine(u: RunUpdate | RunLogLine): LogLine | null {
  switch (u.kind) {
    // Handled as a live stream, never as a transcript line.
    case "delta":
    case "thinking":
      return null;
    case "text":
      return u.text?.trim()
        ? { ts: u.ts_ms, kind: u.kind, text: u.text, color: "var(--text)" }
        : null;
    case "tool_use":
      return {
        ts: u.ts_ms,
        kind: u.kind,
        text: `${u.tool ?? "tool"} — ${u.summary ?? ""}`.trim(),
        color: "var(--text2)",
      };
    case "started":
      return {
        ts: u.ts_ms,
        kind: u.kind,
        text: `session ${(u.session_id ?? "").slice(0, 8)} started`,
        color: "var(--text3)",
      };
    case "done": {
      const cost = u.cost_usd != null ? `$${u.cost_usd.toFixed(4)}` : "no cost recorded";
      const turns = u.turns != null ? ` · ${u.turns} turns` : "";
      return { ts: u.ts_ms, kind: u.kind, text: `done — ${cost}${turns}`, color: "var(--ok)" };
    }
    case "failed":
      return {
        ts: u.ts_ms,
        kind: u.kind,
        text: `failed — ${u.message ?? "unknown"}`,
        color: "var(--bad)",
      };
    case "approval_requested":
      return {
        ts: u.ts_ms,
        kind: u.kind,
        text: `waiting on you: ${u.tool ?? "tool"} — ${u.summary ?? ""}`,
        color: "var(--warn)",
      };
    case "approval_answered":
      return {
        ts: u.ts_ms,
        kind: u.kind,
        text: u.allow ? "you allowed it" : "you denied it",
        color: u.allow ? "var(--ok)" : "var(--bad)",
      };
    case "notice":
      return { ts: u.ts_ms, kind: u.kind, text: u.text ?? "", color: "var(--accent)" };
    default:
      return null;
  }
}

interface Store {
  ready: boolean;
  fatal: string | null;
  settings: Settings | null;
  agents: AgentProfile[];
  agentStats: Record<string, AgentStats>;
  status: SystemStatus | null;
  dataDir: string;
  projects: ProjectView[];
  projectId: string | null;
  project: ProjectView | null;
  snapshot: Snapshot | null;
  stats: ProjectStats | null;
  activity: ActivityRow[];
  outputs: Record<string, LogLine[]>;
  /** Token-level stream per card, cleared when the final text arrives. */
  streams: Record<string, LiveStream>;
  approvals: PendingApproval[];
  chat: ChatMsg[];
  chatBusy: boolean;
  /** The Director's reasoning as it arrives; cleared when it answers. */
  chatThinking: string;
  /** Set when the Director asks the window to go somewhere; clear it after. */
  navigation: (Navigation & { at: number }) | null;
  clearNavigation: () => void;
  toasts: Toast[];

  toast: (tone: string, title: string, body?: string) => void;
  dismissToast: (id: number) => void;
  selectProject: (id: string) => void;
  refresh: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  refreshStatus: () => Promise<void>;

  createCard: (title: string, agentId: string, mode: "plan" | "start" | "later") => Promise<void>;
  moveCard: (cardId: string, to: Status) => Promise<void>;
  assignAgent: (cardId: string, agentId: string) => Promise<void>;
  startRun: (cardId: string, prompt?: string) => Promise<void>;
  cancelRun: (cardId: string) => Promise<void>;
  approve: (cardId: string) => Promise<void>;
  reject: (cardId: string, reason: string) => Promise<void>;
  /** Take a card off the board for good, worktree and all. */
  discard: (cardId: string) => Promise<void>;
  loadRunLog: (runId: string, cardId: string) => Promise<void>;

  sendChat: (text: string) => Promise<void>;
  answerApproval: (requestId: string, allow: boolean, always: boolean) => Promise<void>;
  saveSettings: (patch: Partial<Settings>) => Promise<void>;
  saveAgents: (agents: AgentProfile[]) => Promise<void>;
  /** Pick a folder and adopt it, asking first when it is not a repository. */
  addProject: () => Promise<void>;
  /** Create a repository from nothing under a folder you pick. */
  createProject: (name: string) => Promise<void>;
  removeProject: (id: string, deleteData: boolean) => Promise<void>;
  installSidecar: () => Promise<void>;
}

const Ctx = createContext<Store | null>(null);

export function useStore(): Store {
  const store = useContext(Ctx);
  if (!store) throw new Error("useStore used outside the provider");
  return store;
}

export function applyTheme(settings: Pick<Settings, "theme" | "accent">) {
  const root = document.documentElement;
  root.setAttribute("data-theme", settings.theme === "light" ? "light" : "dark");
  const a = settings.accent;
  if (/^#[0-9a-fA-F]{6}$/.test(a)) {
    root.style.setProperty("--accent", a);
    root.style.setProperty("--accent2", a);
    root.style.setProperty("--accentSoft", `${a}1f`);
    root.style.setProperty("--accentLine", `${a}4d`);
  }
}

export function StoreProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [fatal, setFatal] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [agents, setAgents] = useState<AgentProfile[]>([]);
  const [agentStats, setAgentStats] = useState<Record<string, AgentStats>>({});
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [dataDir, setDataDir] = useState("");
  const [projects, setProjects] = useState<ProjectView[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [stats, setStats] = useState<ProjectStats | null>(null);
  const [activity, setActivity] = useState<ActivityRow[]>([]);
  const [outputs, setOutputs] = useState<Record<string, LogLine[]>>({});
  const [streams, setStreams] = useState<Record<string, LiveStream>>({});
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [chat, setChat] = useState<ChatMsg[]>([]);
  const [chatBusy, setChatBusy] = useState(false);
  const [chatThinking, setChatThinking] = useState("");
  const [navigation, setNavigation] = useState<(Navigation & { at: number }) | null>(null);
  // True while deltas are arriving for the current answer, so the final `text`
  // event is not appended on top of what was already streamed.
  const streamedRef = useRef(false);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const projectRef = useRef<string | null>(null);
  projectRef.current = projectId;
  const toastSeq = useRef(0);

  const toast = useCallback((tone: string, title: string, body?: string) => {
    const id = ++toastSeq.current;
    setToasts((t) => [...t, { id, tone, title, body }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4600);
  }, []);

  const dismissToast = useCallback((id: number) => {
    setToasts((t) => t.filter((x) => x.id !== id));
  }, []);

  const fail = useCallback(
    (e: unknown, what: string) => {
      toast("var(--bad)", what, reason(e));
    },
    [toast],
  );

  // ---- reads ----

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.status());
    } catch {
      /* a failed status check is not worth interrupting anyone */
    }
  }, []);

  const refreshProjects = useCallback(async () => {
    try {
      setProjects(await api.projects());
    } catch (e) {
      fail(e, "Could not read the project list");
    }
  }, [fail]);

  const refresh = useCallback(async () => {
    const id = projectRef.current;
    if (!id) return;
    try {
      const [snap, st, acts] = await Promise.all([
        api.snapshot(id),
        api.projectStats(id),
        api.activity(id, 200),
      ]);
      if (projectRef.current !== id) return;
      setSnapshot(snap);
      setStats(st);
      setActivity(acts);
    } catch (e) {
      fail(e, "Could not read the board");
    }
  }, [fail]);

  const refreshAgentStats = useCallback(async () => {
    try {
      const rows = await api.agentsStats();
      setAgentStats(Object.fromEntries(rows.map((r) => [r.agent_id, r])));
    } catch {
      /* stats are decoration; never block on them */
    }
  }, []);

  // ---- startup ----

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const boot = await api.bootstrap();
        if (!alive) return;
        applyTheme(boot.settings);
        setSettings(boot.settings);
        setAgents(boot.agents);
        setStatus(boot.status);
        setApprovals(boot.approvals);
        setDataDir(boot.data_dir);

        const list = await api.projects();
        if (!alive) return;
        setProjects(list);
        const preferred =
          list.find((p) => p.id === boot.settings.last_project) ?? list.find((p) => p.exists);
        setProjectId(preferred?.id ?? null);
        projectRef.current = preferred?.id ?? null;
        setReady(true);
      } catch (e) {
        if (alive) setFatal(reason(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  // Whenever the project changes, load its board from scratch.
  useEffect(() => {
    if (!projectId) return;
    setSnapshot(null);
    setOutputs({});
    setStreams({});
    setChat([]);
    setChatThinking("");
    refresh();
    refreshAgentStats();
  }, [projectId, refresh, refreshAgentStats]);

  // ---- live wiring ----

  const pending = useRef<number | null>(null);
  const scheduleRefresh = useCallback(() => {
    if (pending.current != null) return;
    pending.current = window.setTimeout(() => {
      pending.current = null;
      refresh();
      refreshProjects();
    }, 90);
  }, [refresh, refreshProjects]);

  useEffect(() => {
    const subs: Promise<UnlistenFn>[] = [];
    let closed = false;
    const keep = (p: Promise<UnlistenFn>) => {
      subs.push(p);
      return p;
    };

    keep(
      events.onEngineEvent((env: Envelope) => {
        if (closed) return;
        if (env.project_id !== projectRef.current) {
          refreshProjects();
          return;
        }
        scheduleRefresh();
      }),
    );

    keep(
      events.onRunUpdate((u: RunUpdate) => {
        if (closed) return;
        const appendToDirector = (text: string) =>
          setChat((cs) => {
            const last = cs[cs.length - 1];
            if (last && last.role === "director") {
              return [...cs.slice(0, -1), { role: "director", text: last.text + text }];
            }
            return [...cs, { role: "director", text }];
          });

        if (u.card_id === DIRECTOR) {
          switch (u.kind) {
            case "thinking":
              if (u.text) setChatThinking((t) => (t + u.text).slice(-600));
              break;
            case "delta":
              if (u.text) {
                streamedRef.current = true;
                setChatThinking("");
                appendToDirector(u.text);
              }
              break;
            case "tool_use": {
              // Not every model streams its reasoning, but every model's tool
              // calls are visible progress — show those instead of a bare
              // spinner while it works.
              const tool = (u.tool ?? "").replace(/^harness:/, "");
              const said: Record<string, string> = {
                read_diff: "reading the diff",
                open_screen: "opening the screen",
                move_card: "moving the card",
                create_card: "adding the card",
                approve_card: "approving",
                reject_card: "sending it back",
                delete_card: "deleting the card",
              };
              setChatThinking(said[tool] ? `${said[tool]}…` : `using ${tool}…`);
              break;
            }
            case "text":
              // Already shown token by token; the full text would double it.
              if (u.text && !streamedRef.current) appendToDirector(u.text);
              streamedRef.current = false;
              break;
            case "done":
              streamedRef.current = false;
              setChatThinking("");
              setChatBusy(false);
              break;
            case "failed":
              streamedRef.current = false;
              setChatThinking("");
              setChatBusy(false);
              setChat((cs) => [
                ...cs,
                { role: "director", text: `(the Director could not answer: ${u.message})` },
              ]);
              break;
          }
          return;
        }

        if (u.project_id !== projectRef.current) return;

        // Deltas live outside the transcript: they are replaced by the final
        // text, and would otherwise be one log line per token.
        if (u.kind === "delta" || u.kind === "thinking") {
          if (!u.text) return;
          const key = u.kind === "delta" ? "text" : "thinking";
          setStreams((prev) => {
            const cur = prev[u.card_id] ?? { text: "", thinking: "" };
            return {
              ...prev,
              [u.card_id]: { ...cur, [key]: (cur[key] + u.text).slice(-2000) },
            };
          });
          return;
        }
        if (u.kind === "text" || u.kind === "done" || u.kind === "failed") {
          setStreams((prev) => {
            if (!prev[u.card_id]) return prev;
            const next = { ...prev };
            delete next[u.card_id];
            return next;
          });
        }

        const line = toLine(u);
        if (!line) return;
        setOutputs((prev) => ({
          ...prev,
          [u.card_id]: [...(prev[u.card_id] ?? []), line].slice(-MAX_LINES),
        }));
      }),
    );

    keep(events.onApprovalQueue((list) => !closed && setApprovals(list)));
    keep(
      events.onNavigate((n) => {
        if (closed) return;
        setNavigation({ ...n, at: Date.now() });
        toast("var(--info)", "The Director opened " + n.screen, n.why ?? undefined);
      }),
    );
    keep(
      events.onApprovalAsked((a) => {
        if (closed) return;
        toast("var(--warn)", "Permission needed", `${a.tool} — ${a.summary}`);
      }),
    );

    return () => {
      closed = true;
      subs.forEach((p) => p.then((un) => un()).catch(() => {}));
      if (pending.current != null) window.clearTimeout(pending.current);
    };
  }, [scheduleRefresh, refreshProjects, toast]);

  // Keep the "is anything actually able to run" banner honest.
  useEffect(() => {
    const t = window.setInterval(refreshStatus, 20000);
    return () => window.clearInterval(t);
  }, [refreshStatus]);

  // ---- intents ----

  const withProject = useCallback(
    <T,>(fn: (id: string) => Promise<T>, what: string) =>
      async (): Promise<T | undefined> => {
        const id = projectRef.current;
        if (!id) {
          toast("var(--bad)", "No project", "Add a git repository first.");
          return;
        }
        try {
          return await fn(id);
        } catch (e) {
          fail(e, what);
          return;
        }
      },
    [fail, toast],
  );

  const createCard = useCallback(
    async (title: string, agentId: string, mode: "plan" | "start" | "later") => {
      const clean = title.trim();
      if (!clean) {
        toast("var(--bad)", "Nothing to add", "Say what should happen first.");
        return;
      }
      await withProject(async (id) => {
        const created = await api.createCard(
          id,
          clean,
          agentId,
          mode === "start",
          mode === "plan",
        );
        if (created.run_id) {
          toast("var(--accent)", "Started", `${clean}`);
        } else {
          toast("var(--ok)", "Added", mode === "later" ? "Parked in Later" : "Ready to start");
        }
        await refresh();
      }, "Could not add the card")();
    },
    [refresh, toast, withProject],
  );

  const moveCard = useCallback(
    async (cardId: string, to: Status) => {
      await withProject(async (id) => {
        await api.moveCard(id, cardId, to);
        await refresh();
      }, "Could not move the card")();
    },
    [refresh, withProject],
  );

  const assignAgent = useCallback(
    async (cardId: string, agentId: string) => {
      await withProject(async (id) => {
        await api.assignAgent(id, cardId, agentId);
        await refresh();
      }, "Could not reassign the card")();
    },
    [refresh, withProject],
  );

  const startRun = useCallback(
    async (cardId: string, prompt?: string) => {
      await withProject(async (id) => {
        setOutputs((prev) => ({ ...prev, [cardId]: [] }));
        setStreams((prev) => ({ ...prev, [cardId]: { text: "", thinking: "" } }));
        await api.startRun(id, cardId, prompt);
        await refresh();
      }, "Could not start the run")();
    },
    [refresh, withProject],
  );

  const cancelRun = useCallback(
    async (cardId: string) => {
      await withProject(async (id) => {
        await api.cancelRun(id, cardId);
        toast("var(--bad)", "Stopping", "Work in progress will be committed.");
      }, "Could not stop the run")();
    },
    [toast, withProject],
  );

  const approve = useCallback(
    async (cardId: string) => {
      await withProject(async (id) => {
        await api.approveCard(id, cardId, "approved by you");
        toast("var(--ok)", "Approved", "The card is done.");
        await refresh();
      }, "Could not approve the card")();
    },
    [refresh, toast, withProject],
  );

  const reject = useCallback(
    async (cardId: string, why: string) => {
      await withProject(async (id) => {
        await api.rejectCard(id, cardId, why.trim() || "no reason given");
        toast("var(--warn)", "Sent back", "The agent gets your reason on the next run.");
        await refresh();
      }, "Could not send the card back")();
    },
    [refresh, toast, withProject],
  );

  const discard = useCallback(
    async (cardId: string) => {
      const card = snapshot?.cards.find((c) => c.id === cardId);
      if (card?.status === "running") {
        toast("var(--bad)", "It is running", "Stop the run before deleting the card.");
        return;
      }
      // Unreviewed work is the one case worth interrupting for: deleting it
      // throws away a diff nobody has seen.
      if (card?.status === "review") {
        const ok = window.confirm(
          `"${card.title}" is waiting for review.

` +
            "Deleting it discards that run and its worktree. Continue?",
        );
        if (!ok) return;
      }
      await withProject(async (id) => {
        await api.discardCard(id, cardId);
        toast("var(--ok)", "Deleted", card?.title ?? cardId);
        await refresh();
      }, "Could not delete the card")();
    },
    [refresh, snapshot, toast, withProject],
  );

  const loadRunLog = useCallback(
    async (runId: string, cardId: string) => {
      await withProject(async (id) => {
        const lines = await api.runLog(id, runId);
        const mapped = lines.map(toLine).filter((l): l is LogLine => l != null);
        setOutputs((prev) => ({ ...prev, [cardId]: mapped.slice(-MAX_LINES) }));
      }, "Could not read the run log")();
    },
    [withProject],
  );

  const sendChat = useCallback(
    async (text: string) => {
      const clean = text.trim();
      if (!clean || chatBusy) return;
      setChat((cs) => [...cs, { role: "user", text: clean }]);
      setChatBusy(true);
      setChatThinking("");
      streamedRef.current = false;
      try {
        // The reply streams back on the run channel; `chatBusy` clears when the
        // done event arrives, not when this call returns. There is one Director:
        // it sees every board, and reads code from the project that is open.
        await api.directorAsk(clean, projectRef.current);
      } catch (e) {
        setChatBusy(false);
        fail(e, "The Director could not be reached");
      }
    },
    [chatBusy, fail],
  );

  const answerApproval = useCallback(
    async (requestId: string, allow: boolean, always: boolean) => {
      try {
        await api.respondApproval(requestId, allow, always);
        if (always && allow) {
          setSettings(await api.settingsGet());
        }
        toast(
          allow ? "var(--ok)" : "var(--bad)",
          allow ? "Allowed" : "Denied",
          allow ? "The agent carried on." : "The agent was told no.",
        );
      } catch (e) {
        fail(e, "Could not answer the request");
      }
    },
    [fail, toast],
  );

  const saveSettings = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      applyTheme(next);
      setSettings(next);
      try {
        setSettings(await api.settingsUpdate(next));
        await refreshStatus();
      } catch (e) {
        fail(e, "Could not save settings");
        setSettings(settings);
        applyTheme(settings);
      }
    },
    [fail, refreshStatus, settings],
  );

  const saveAgents = useCallback(
    async (next: AgentProfile[]) => {
      setAgents(next);
      try {
        setAgents(await api.agentsSave(next));
      } catch (e) {
        fail(e, "Could not save the crew");
      }
    },
    [fail],
  );

  const selectProject = useCallback(
    (id: string) => {
      setProjectId(id);
      projectRef.current = id;
      if (settings && settings.last_project !== id) {
        api.settingsUpdate({ ...settings, last_project: id }).then(setSettings).catch(() => {});
      }
    },
    [settings],
  );

  const adopt = useCallback(
    async (path: string, init: boolean, name?: string) => {
      const project = await api.projectAdd(path, name, init);
      await refreshProjects();
      selectProject(project.id);
      toast("var(--ok)", "Project added", project.name);
    },
    [refreshProjects, selectProject, toast],
  );

  const addProject = useCallback(async () => {
    try {
      const path = await api.pickFolder();
      if (!path) return;
      const info = await api.inspectFolder(path);
      if (info.already_added) {
        const existing = (await api.projects()).find((p) =>
          p.path.toLowerCase() === info.path.toLowerCase(),
        );
        if (existing) selectProject(existing.id);
        toast("var(--info)", "Already added", info.name);
        return;
      }
      if (info.next === "missing") {
        toast("var(--bad)", "Gone", `${info.path} is not a directory any more.`);
        return;
      }
      if (info.next === "confirm_init") {
        // Files but no repository: never git init behind the operator's back.
        const ok = window.confirm(
          `${info.path} has files but no git repository.

` +
            "Run git init there so Harness can work on it?",
        );
        if (!ok) return;
        await adopt(info.path, true, info.name);
        return;
      }
      await adopt(info.path, info.next === "init", info.name);
    } catch (e) {
      fail(e, "Could not add that folder");
    }
  }, [adopt, fail, selectProject, toast]);

  const createProject = useCallback(
    async (name: string) => {
      const clean = name.trim();
      if (!clean) {
        toast("var(--bad)", "Name it first", "A project needs a name.");
        return;
      }
      try {
        const parent = await api.pickFolder();
        if (!parent) return;
        const project = await api.projectCreate(parent, clean);
        await refreshProjects();
        selectProject(project.id);
        toast("var(--ok)", "Project created", `${project.name} — a fresh repository`);
      } catch (e) {
        fail(e, "Could not create the project");
      }
    },
    [fail, refreshProjects, selectProject, toast],
  );

  const removeProject = useCallback(
    async (id: string, deleteData: boolean) => {
      try {
        await api.projectRemove(id, deleteData);
        const list = await api.projects();
        setProjects(list);
        if (projectRef.current === id) {
          const next = list.find((p) => p.exists) ?? list[0] ?? null;
          setProjectId(next?.id ?? null);
          projectRef.current = next?.id ?? null;
        }
        toast("var(--ok)", "Removed", deleteData ? "Project and its history" : "Project forgotten");
      } catch (e) {
        fail(e, "Could not remove the project");
      }
    },
    [fail, toast],
  );

  const installSidecar = useCallback(async () => {
    toast("var(--info)", "Installing", "Fetching the agent SDK…");
    try {
      await api.sidecarInstall();
      await refreshStatus();
      toast("var(--ok)", "Sidecar ready", "Agents can run now.");
    } catch (e) {
      fail(e, "The sidecar install failed");
    }
  }, [fail, refreshStatus, toast]);

  const project = useMemo(
    () => projects.find((p) => p.id === projectId) ?? null,
    [projectId, projects],
  );

  const value: Store = {
    ready,
    fatal,
    settings,
    agents,
    agentStats,
    status,
    dataDir,
    projects,
    projectId,
    project,
    snapshot,
    stats,
    activity,
    outputs,
    streams,
    approvals,
    chat,
    chatBusy,
    chatThinking,
    navigation,
    clearNavigation: () => setNavigation(null),
    toasts,
    toast,
    dismissToast,
    selectProject,
    refresh,
    refreshProjects,
    refreshStatus,
    createCard,
    moveCard,
    assignAgent,
    startRun,
    cancelRun,
    approve,
    reject,
    discard,
    loadRunLog,
    sendChat,
    answerApproval,
    saveSettings,
    saveAgents,
    addProject,
    createProject,
    removeProject,
    installSidecar,
  };

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** Cards of the active project, or an empty board. */
export function useCards() {
  const { snapshot } = useStore();
  return snapshot?.cards ?? [];
}

export function useAgent(id: string | null | undefined) {
  const { agents } = useStore();
  return agents.find((a) => a.id === id) ?? agents.find((a) => a.id === "builder") ?? agents[0];
}
