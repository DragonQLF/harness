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
  CardDiff,
  Conversation,
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
  /** The word printed in the transcript's left gutter: a tool name, or what
   *  kind of line this is. */
  label: string;
  text: string;
  /** CSS colour variable for this line. */
  color: string;
  /** CSS colour variable for the gutter word. */
  labelColor: string;
  /** Tool-call linkage: this call's id and its parent's, so results nest
   *  under the call that produced them and subagent calls indent further. */
  toolUseId?: string | null;
  parentToolUseId?: string | null;
  /** For a tool_result: did it succeed? */
  ok?: boolean;
  /** Full output for expandable results (#28: never dumped inline). */
  detail?: string | null;
  italic?: boolean;
}

export interface ChatMsg {
  /** `notice` is Harness itself talking: a failed resume, a cancelled turn. */
  role: "user" | "agent" | "notice";
  text: string;
  /** When it was said, so the transcript can date itself. */
  ts: number;
}

/** One stored transcript line as a chat bubble. Deltas never reach here: the
 *  final `text` is the record (the backend does not log them). */
function toChatMsg(line: RunLogLine): ChatMsg | null {
  const ts = line.ts_ms;
  switch (line.kind) {
    case "user_message":
      return line.text?.trim() ? { role: "user", text: line.text, ts } : null;
    case "text":
      return line.text?.trim() ? { role: "agent", text: line.text, ts } : null;
    case "notice":
      return line.text?.trim() ? { role: "notice", text: line.text, ts } : null;
    case "failed":
      return { role: "notice", text: line.message ?? "the answer did not arrive", ts };
    // Tool calls and session boundaries are in the log but would clutter the
    // conversation; they show live as progress instead.
    default:
      return null;
  }
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

/** One shape for both live updates and stored log lines. The gutter word and
 *  the text are kept apart, so the transcript can align one and wrap the
 *  other. */
export function toLine(u: RunUpdate | RunLogLine): LogLine | null {
  const line = (
    label: string,
    text: string,
    labelColor: string,
    color: string,
    italic?: boolean,
  ): LogLine => ({ ts: u.ts_ms, kind: u.kind, label, text, color, labelColor, italic });

  switch (u.kind) {
    // Handled as a live stream, never as a transcript line.
    case "delta":
    case "thinking":
      return null;
    case "text":
      return u.text?.trim() ? line("text", u.text, "var(--text4)", "var(--text2)") : null;
    case "user_message":
      return u.text?.trim() ? line("you", u.text, "var(--text4)", "var(--text2)") : null;
    case "tool_use": {
      // The tool's own name is the gutter word: Read, Edit, Bash. Its colour
      // says what kind of call it was without a legend.
      const tool = (u.tool ?? "tool").replace(/^(harness|mcp__harness__)/, "").replace(/^__/, "");
      const colours: Record<string, string> = {
        Read: "var(--ok)",
        Glob: "var(--ok)",
        Grep: "var(--ok)",
        Edit: "var(--accent)",
        Write: "var(--accent)",
        Bash: "var(--info)",
      };
      const l = line(tool, u.summary ?? "", colours[tool] ?? "var(--text3)", "var(--text2)");
      return {
        ...l,
        toolUseId: (u as RunUpdate & { tool_use_id?: string }).tool_use_id ?? null,
        parentToolUseId:
          (u as RunUpdate & { parent_tool_use_id?: string }).parent_tool_use_id ?? null,
      };
    }
    case "tool_result": {
      const ok = (u as RunUpdate & { ok?: boolean }).ok !== false;
      const detail = (u as RunUpdate & { detail?: string | null }).detail ?? null;
      return {
        ...line(
          ok ? "↳ ok" : "↳ failed",
          (u as RunUpdate & { summary?: string }).summary ?? "",
          ok ? "var(--ok)" : "var(--bad)",
          ok ? "var(--text2)" : "var(--bad2)",
          !ok,
        ),
        toolUseId: (u as RunUpdate & { tool_use_id?: string }).tool_use_id ?? null,
        ok,
        detail,
      };
    }
    case "started":
      return line(
        "started",
        u.session_id ? `resumed ${u.session_id.slice(0, 12)}` : "new session",
        "var(--text4)",
        "var(--text3)",
      );
    case "done": {
      const cost = u.cost_usd != null ? `$${u.cost_usd.toFixed(4)}` : "no cost recorded";
      const turns = u.turns != null ? `${u.turns} turns · ` : "";
      return line("done", `${turns}${cost}`, "var(--text4)", "var(--ok)");
    }
    case "failed":
      return line("failed", u.message ?? "unknown", "var(--bad)", "var(--bad2)");
    case "approval_requested":
      return line(
        "approval",
        `${u.tool ?? "tool"} — ${u.summary ?? ""}`.trim(),
        "var(--warn)",
        "var(--warn)",
      );
    case "approval_answered":
      return line(
        "approval",
        u.allow ? "you allowed it" : "you denied it",
        "var(--warn)",
        u.allow ? "var(--ok)" : "var(--bad2)",
      );
    case "notice":
      return line("notice", u.text ?? "", "var(--warn)", "var(--warn)");
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
  /** What each card changed, once something has asked for it. */
  diffs: Record<string, CardDiff>;
  /** Read a card's diff from its worktree. Cheap to call again: the answer
   *  replaces the cached one, so a re-run shows the new patch. */
  loadCardDiff: (cardId: string) => Promise<void>;
  /** Every conversation the backend knows about, newest first. */
  conversations: Conversation[];
  /** The one on screen. */
  conversationId: string | null;
  conversation: Conversation | null;
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
  /** Start a fresh conversation, which means a fresh Claude session. */
  newConversation: (profileId?: string) => Promise<void>;
  /** Open the standing conversation with a profile, creating one only if there
   *  is none: clicking "chat" twice continues the same thread. */
  chatWithProfile: (profileId: string) => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  archiveConversation: (id: string, archived: boolean) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  pinConversation: (id: string, projectId: string | null) => Promise<void>;
  /** Templates are fetched on demand: nothing is installed until you say so. */
  agentTemplates: () => Promise<AgentProfile[]>;
  createAgentFromTemplate: (templateId: string) => Promise<void>;
  duplicateAgent: (agentId: string) => Promise<void>;
  removeAgent: (agentId: string) => Promise<void>;
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
  const [diffs, setDiffs] = useState<Record<string, CardDiff>>({});
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [conversationId, setConversationId] = useState<string | null>(null);
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
  // The run channel is keyed by conversation id, so the listener needs the
  // current one without re-subscribing on every switch.
  const chatRef = useRef<string | null>(null);
  chatRef.current = conversationId;
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
        setConversations(boot.conversations);
        // The backend decides which conversation reopens; the frontend just
        // renders it. Its transcript is read from disk, so it is there whether
        // or not the native Claude session can still be resumed.
        if (boot.last_conversation) {
          setConversationId(boot.last_conversation);
          chatRef.current = boot.last_conversation;
          api
            .conversationTranscript(boot.last_conversation)
            .then((lines) =>
              setChat(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null)),
            )
            .catch(() => {});
        }
        if (boot.revoked_allowances.length > 0) {
          // Said once, because it changes what the app will do without asking.
          toast(
            "var(--warn)",
            "Standing permissions were narrowed",
            `${boot.revoked_allowances.join(", ")} allowed every command, so it no longer allows any. Approve once more to record a scoped rule.`,
          );
        }

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
    setDiffs({});
    // The conversation is not per project: a Director chat outlives switching
    // boards, and is pinned to a project only if you pin it.
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
            if (last && last.role === "agent") {
              return [...cs.slice(0, -1), { ...last, text: last.text + text }];
            }
            return [...cs, { role: "agent", text, ts: Date.now() }];
          });

        // A conversation streams under its own id. `DIRECTOR` is the id older
        // builds published chat on; kept so nothing from before is orphaned.
        if (u.card_id === chatRef.current || u.card_id === DIRECTOR) {
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
            case "notice":
              // Harness itself talking — a resume that could not be honoured.
              if (u.text) setChat((cs) => [...cs, { role: "notice", text: u.text!, ts: u.ts_ms }]);
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
                { role: "notice", text: `No answer: ${u.message}`, ts: u.ts_ms },
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
    keep(events.onConversations((list) => !closed && setConversations(list)));
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

  const loadCardDiff = useCallback(
    async (cardId: string) => {
      const id = projectRef.current;
      if (!id) return;
      try {
        const diff = await api.cardDiff(id, cardId);
        if (projectRef.current !== id) return;
        setDiffs((prev) => ({ ...prev, [cardId]: diff }));
      } catch {
        /* a card with no worktree has nothing to show; the screen says so */
      }
    },
    [],
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

  const refreshConversations = useCallback(async () => {
    try {
      setConversations(await api.conversations());
    } catch {
      /* the list is a view of backend state; a failed read is not fatal */
    }
  }, []);

  const sendChat = useCallback(
    async (text: string) => {
      const clean = text.trim();
      if (!clean || chatBusy) return;
      setChat((cs) => [...cs, { role: "user", text: clean, ts: Date.now() }]);
      setChatBusy(true);
      setChatThinking("");
      streamedRef.current = false;
      try {
        // The first message of a session with no conversation open gets one
        // pinned to the project on screen, so it can read that code. After
        // that the backend owns which conversation this belongs to.
        if (!chatRef.current) {
          const started = await api.conversationNew(DIRECTOR, projectRef.current);
          setConversationId(started.id);
          chatRef.current = started.id;
        }
        // The reply streams back on the run channel, keyed by the conversation
        // id; `chatBusy` clears when the done event arrives, not when this call
        // returns. The backend decides which conversation this belongs to and
        // hands it back, so the first message of a new chat lands in the right
        // thread.
        const conversation = await api.chatSend(clean, chatRef.current);
        setConversationId(conversation.id);
        chatRef.current = conversation.id;
        await refreshConversations();
      } catch (e) {
        setChatBusy(false);
        fail(e, "The message could not be sent");
      }
    },
    [chatBusy, fail, refreshConversations],
  );

  /** Load a conversation and its stored transcript. */
  const openConversation = useCallback(
    async (id: string) => {
      try {
        const conversation = await api.conversationSelect(id);
        const lines = await api.conversationTranscript(id);
        setConversationId(conversation.id);
        chatRef.current = conversation.id;
        setChat(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null));
        setChatThinking("");
        setChatBusy(false);
        streamedRef.current = false;
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not open that conversation");
      }
    },
    [fail, refreshConversations],
  );

  /** A direct, persistent conversation with one profile. Resumes the last one
   *  rather than piling up a new session per click. */
  const chatWithProfile = useCallback(
    async (profileId: string) => {
      try {
        const conversation = await api.conversationOpen(profileId, projectRef.current);
        await openConversation(conversation.id);
      } catch (e) {
        fail(e, "Could not open that conversation");
      }
    },
    [fail, openConversation],
  );

  const newConversation = useCallback(
    async (profileId?: string) => {
      try {
        // A new row is a new native session: nothing from the last chat is
        // resumed, which is the whole point of New Chat.
        const conversation = await api.conversationNew(
          profileId ?? DIRECTOR,
          projectRef.current,
        );
        setConversationId(conversation.id);
        chatRef.current = conversation.id;
        setChat([]);
        setChatThinking("");
        setChatBusy(false);
        streamedRef.current = false;
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not start a new conversation");
      }
    },
    [fail, refreshConversations],
  );

  const renameConversation = useCallback(
    async (id: string, title: string) => {
      try {
        await api.conversationRename(id, title);
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not rename the conversation");
      }
    },
    [fail, refreshConversations],
  );

  const archiveConversation = useCallback(
    async (id: string, archived: boolean) => {
      try {
        await api.conversationArchive(id, archived);
        await refreshConversations();
        if (archived && chatRef.current === id) {
          setConversationId(null);
          chatRef.current = null;
          setChat([]);
        }
        toast("var(--ok)", archived ? "Archived" : "Restored");
      } catch (e) {
        fail(e, "Could not archive the conversation");
      }
    },
    [fail, refreshConversations, toast],
  );

  const deleteConversation = useCallback(
    async (id: string) => {
      const which = conversations.find((c) => c.id === id);
      const ok = window.confirm(
        `Delete "${which?.title ?? id}"?\n\n` +
          "The transcript is deleted with it, and the Claude session it continues " +
          "can no longer be reopened. This cannot be undone.",
      );
      if (!ok) return;
      try {
        await api.conversationDelete(id);
        if (chatRef.current === id) {
          setConversationId(null);
          chatRef.current = null;
          setChat([]);
        }
        await refreshConversations();
        toast("var(--ok)", "Deleted", which?.title);
      } catch (e) {
        fail(e, "Could not delete the conversation");
      }
    },
    [conversations, fail, refreshConversations, toast],
  );

  const pinConversation = useCallback(
    async (id: string, project: string | null) => {
      try {
        await api.conversationPin(id, project);
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not change the project");
      }
    },
    [fail, refreshConversations],
  );

  // ---- the crew ----

  const agentTemplates = useCallback(async () => {
    try {
      return await api.agentTemplates();
    } catch (e) {
      fail(e, "Could not read the templates");
      return [];
    }
  }, [fail]);

  const createAgentFromTemplate = useCallback(
    async (templateId: string) => {
      try {
        const created = await api.agentCreateFromTemplate(templateId);
        setAgents(await api.agentsGet());
        toast("var(--ok)", "Added", `${created.name} joined the crew`);
      } catch (e) {
        fail(e, "Could not create that profile");
      }
    },
    [fail, toast],
  );

  const duplicateAgent = useCallback(
    async (agentId: string) => {
      try {
        const copy = await api.agentDuplicate(agentId);
        setAgents(await api.agentsGet());
        toast("var(--ok)", "Duplicated", copy.name);
      } catch (e) {
        fail(e, "Could not duplicate that profile");
      }
    },
    [fail, toast],
  );

  const removeAgent = useCallback(
    async (agentId: string) => {
      const which = agents.find((a) => a.id === agentId);
      const ok = window.confirm(
        `Remove ${which?.name ?? agentId}?\n\n` +
          "Cards already assigned to it keep the name, but nothing new can be given to it.",
      );
      if (!ok) return;
      try {
        setAgents(await api.agentRemove(agentId));
        toast("var(--ok)", "Removed", which?.name);
      } catch (e) {
        fail(e, "Could not remove that profile");
      }
    },
    [agents, fail, toast],
  );

  const answerApproval = useCallback(
    async (requestId: string, allow: boolean, always: boolean) => {
      try {
        const recorded = await api.respondApproval(requestId, allow, always);
        if (always && allow) {
          setSettings(await api.settingsGet());
        }
        if (allow && always && !recorded) {
          // Nothing safe to remember: a chained shell command cannot be scoped,
          // so it is allowed once and asked about again.
          toast(
            "var(--warn)",
            "Allowed once",
            "That command could not be narrowed into a rule, so you will be asked again.",
          );
        } else {
          toast(
            allow ? "var(--ok)" : "var(--bad)",
            allow ? "Allowed" : "Denied",
            recorded
              ? `Not asking again about ${recorded}`
              : allow
                ? "The agent carried on."
                : "The agent was told no.",
          );
        }
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
    diffs,
    loadCardDiff,
    conversations,
    conversationId,
    conversation: conversations.find((c) => c.id === conversationId) ?? null,
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
    newConversation,
    chatWithProfile,
    openConversation,
    renameConversation,
    archiveConversation,
    deleteConversation,
    pinConversation,
    agentTemplates,
    createAgentFromTemplate,
    duplicateAgent,
    removeAgent,
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
