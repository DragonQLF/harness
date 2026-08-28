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
  Proposal,
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
  /** As classes de cor desta linha, já com o par `dark:`. */
  color: string;
  /** As classes de cor da palavra da goteira. */
  labelColor: string;
  /** Tool-call linkage: this call's id and its parent's, so results nest
   *  under the call that produced them and subagent calls indent further. */
  toolUseId?: string | null;
  parentToolUseId?: string | null;
  /** For a tool_result: did it succeed? */
  ok?: boolean | null;
  /** Full output for expandable results (#28: never dumped inline). */
  detail?: string | null;
  italic?: boolean;
}

export interface ChatMsg {
  /** `notice` is Relay itself talking: a failed resume, a cancelled turn.
   *  `tool` is what the agent tried (`summary`) — its result arrives as a
   *  second tool bubble matched by id, green or red, expandable. */
  role: "user" | "agent" | "notice" | "tool";
  text: string;
  /** When it was said, so the transcript can date itself. */
  ts: number;
  /** Tool bubble only: which tool, whether its result closed it, and the
   *  full output kept for expansion (#28: never dumped inline). */
  tool?: string;
  ok?: boolean | null;
  detail?: string | null;
  toolUseId?: string | null;
  parentToolUseId?: string | null;
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
    case "tool_use": {
      const tool = (line.tool ?? "tool").replace(/^(harness|mcp__harness__)/, "").replace(/^__/, "");
      const ids = line as RunLogLine & {
        tool_use_id?: string | null;
        parent_tool_use_id?: string | null;
      };
      return {
        role: "tool",
        text: line.summary ?? "",
        ts,
        tool,
        toolUseId: ids.tool_use_id ?? null,
        parentToolUseId: ids.parent_tool_use_id ?? null,
        ok: null,
        detail: null,
      } as ChatMsg;
    }
    case "tool_result": {
      const res = line as RunLogLine & { tool_use_id?: string; ok?: boolean; detail?: string | null; summary?: string };
      return {
        role: "tool",
        text: res.summary ?? "",
        ts,
        ok: res.ok !== false,
        detail: res.detail ?? null,
      } as ChatMsg;
    }
    // Tool calls and session boundaries are in the log but would clutter the
    // conversation; they show live as progress instead.
    default:
      return null;
  }
}

/** Stored logs list call and result as separate lines; the transcript wants
 *  one bubble that opens and closes. Results with no open call stay alone. */
function foldToolResults(msgs: ChatMsg[]): ChatMsg[] {
  const out: ChatMsg[] = [];
  for (const m of msgs) {
    if (m.role === "tool" && m.ok != null && m.toolUseId) {
      let matched = false;
      for (let i = out.length - 1; i >= 0; i--) {
        const p = out[i];
        if (p.role === "tool" && p.ok == null && p.toolUseId === m.toolUseId) {
          out[i] = { ...p, ok: m.ok, detail: m.detail };
          matched = true;
          break;
        }
      }
      if (matched) continue;
    }
    out.push(m);
  }
  return out;
}

/** What is arriving right now for a card, before the final text lands. */
export interface LiveStream {
  text: string;
  thinking: string;
  /** Model turns so far, while the run is alive. The total lands on Done. */
  turns?: number;
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
      return u.text?.trim() ? line("text", u.text, "text-text4 dark:text-text4-d", "text-text2 dark:text-text2-d") : null;
    case "user_message":
      return u.text?.trim() ? line("you", u.text, "text-text4 dark:text-text4-d", "text-text2 dark:text-text2-d") : null;
    case "tool_use": {
      // The tool's own name is the gutter word: Read, Edit, Bash. Its colour
      // says what kind of call it was without a legend.
      const tool = (u.tool ?? "tool").replace(/^(harness|mcp__harness__)/, "").replace(/^__/, "");
      const colours: Record<string, string> = {
        Read: "text-ok dark:text-ok-d",
        Glob: "text-ok dark:text-ok-d",
        Grep: "text-ok dark:text-ok-d",
        Edit: "text-accent dark:text-accent-d",
        Write: "text-accent dark:text-accent-d",
        Bash: "text-info dark:text-info-d",
      };
      const l = line(tool, u.summary ?? "", colours[tool] ?? "text-text3 dark:text-text3-d", "text-text2 dark:text-text2-d");
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
          ok ? "text-ok dark:text-ok-d" : "text-bad dark:text-bad-d",
          ok ? "text-text2 dark:text-text2-d" : "text-bad2 dark:text-bad2-d",
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
        "text-text4 dark:text-text4-d",
        "text-text3 dark:text-text3-d",
      );
    case "done": {
      // One line that tells the truth: a done with an error is a failure
      // that happens to know its own cost — never two contradicting lines.
      const err = (u as RunUpdate & { error?: string | null }).error;
      if (err) {
        return line("failed", err, "text-bad dark:text-bad-d", "text-bad2 dark:text-bad2-d");
      }
      const cost = u.cost_usd != null ? `$${u.cost_usd.toFixed(4)}` : "no cost recorded";
      const turns = u.turns != null ? `${u.turns} turns · ` : "";
      return line("done", `${turns}${cost}`, "text-text4 dark:text-text4-d", "text-ok dark:text-ok-d");
    }
    case "failed":
      return line("failed", u.message ?? "unknown", "text-bad dark:text-bad-d", "text-bad2 dark:text-bad2-d");
    case "approval_requested":
      return line(
        "approval",
        `${u.tool ?? "tool"} — ${u.summary ?? ""}`.trim(),
        "text-warn dark:text-warn-d",
        "text-warn dark:text-warn-d",
      );
    case "approval_answered":
      return line(
        "approval",
        u.allow ? "you allowed it" : "you denied it",
        "text-warn dark:text-warn-d",
        u.allow ? "text-ok dark:text-ok-d" : "text-bad2 dark:text-bad2-d",
      );
    case "notice":
      return line("notice", u.text ?? "", "text-warn dark:text-warn-d", "text-warn dark:text-warn-d");
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
  /** The Director's improvement proposals waiting on you, newest first. */
  proposals: Proposal[];
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

  sendChat: (text: string, attachments?: string[]) => Promise<void>;
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
  /** Accept a proposal: its card is born in the harness's own project. */
  acceptProposal: (proposalId: string) => Promise<void>;
  dismissProposal: (proposalId: string) => Promise<void>;
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

/** Relative luminance, so the text on a chosen accent is decided rather than
 *  assumed. A picker that accepts any hex cannot also hardcode white on it. */
function luminance(hex: string): number {
  const channel = (i: number) => {
    const v = parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16) / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(0) + 0.7152 * channel(1) + 0.0722 * channel(2);
}

/** Lighten toward white by `amount`, for the hover tone of a chosen accent. */
function lift(hex: string, amount: number): string {
  const mix = (i: number) => {
    const v = parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16);
    return Math.round(v + (255 - v) * amount)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${mix(0)}${mix(1)}${mix(2)}`;
}

export function applyTheme(settings: Pick<Settings, "theme" | "accent">) {
  const root = document.documentElement;
  root.setAttribute("data-theme", settings.theme === "light" ? "light" : "dark");
  // An empty accent is the normal state, not a missing one: the theme's own
  // token wins, so a palette change in CSS actually reaches the screen. Clear
  // the overrides rather than leaving the last choice stuck on the element.
  const a = settings.accent;
  const vars = ["--accent", "--accent2", "--accentSoft", "--accentLine", "--onAccent", "--select"];
  if (!/^#[0-9a-fA-F]{6}$/.test(a)) {
    vars.forEach((v) => root.style.removeProperty(v));
    return;
  }
  root.style.setProperty("--accent", a);
  // Not the same colour twice: accent2 is the hover, and a hover that does not
  // move is not a hover.
  root.style.setProperty("--accent2", lift(a, 0.28));
  root.style.setProperty("--accentSoft", `${a}29`);
  root.style.setProperty("--accentLine", `${a}55`);
  root.style.setProperty("--select", `${a}52`);
  // What sits on the fill follows the fill. White on a light accent is the
  // contrast failure this picker would otherwise ship at four of five choices.
  root.style.setProperty("--onAccent", luminance(a) > 0.45 ? "#0b1116" : "#ffffff");
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
  const [proposals, setProposals] = useState<Proposal[]>([]);
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
      toast("bad", what, reason(e));
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
      lastSeqRef.current = snap.last_seq;
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
        setProposals(boot.inbox);
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
            "warn",
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
  const lastSeqRef = useRef<number | null>(null);
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
        // Gap detection: the log is a strict sequence. A hole means this
        // panel missed events, so any view derived from them could be stale
        // (RightNow was). Refresh immediately instead of trusting the debounced one.
        const last = lastSeqRef.current;
        if (last != null && env.seq > last + 1) {
          pending.current = null;
          refresh();
          refreshProjects();
        }
        if (last == null || env.seq > last) lastSeqRef.current = env.seq;
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
              // A tool call is a transcript line, not a transient badge: five
              // calls in a row used to leave the trace of one, and a failed
              // one read like a clean one (#41 in the visual layer).
              const ev = u as RunUpdate & {
                tool_use_id?: string;
                parent_tool_use_id?: string | null;
              };
              const tool = (u.tool ?? "tool").replace(/^(harness:|mcp__harness__)/, "");
              setChat((cs) => [
                ...cs,
                {
                  role: "tool",
                  text: u.summary ?? "",
                  ts: u.ts_ms,
                  tool,
                  toolUseId: ev.tool_use_id ?? null,
                  parentToolUseId: ev.parent_tool_use_id ?? null,
                  ok: null,
                  detail: null,
                },
              ]);
              break;
            }
            case "tool_result": {
              // Closes the matching call by id — replaces the pending bubble
              // in place instead of appending a second line. A result with no
              // open call (replay started mid-run) lands closed on its own.
              const res = u as RunUpdate & {
                tool_use_id?: string;
                ok?: boolean | null;
                detail?: string | null;
                summary?: string;
              };
              setChat((cs) => {
                let closed = false;
                const next = cs.map((m) => {
                  if (
                    !closed &&
                    m.role === "tool" &&
                    m.ok == null &&
                    m.toolUseId != null &&
                    m.toolUseId === res.tool_use_id
                  ) {
                    closed = true;
                    return { ...m, ok: res.ok !== false, detail: res.detail ?? null };
                  }
                  return m;
                });
                if (closed) return next;
                return [
                  ...next,
                  {
                    role: "tool",
                    text: res.summary ?? "",
                    ts: u.ts_ms,
                    toolUseId: res.tool_use_id ?? null,
                    ok: res.ok !== false,
                    detail: res.detail ?? null,
                  } as ChatMsg,
                ];
              });
              break;
            }
            case "text":
              // Already shown token by token; the full text would double it.
              if (u.text && !streamedRef.current) appendToDirector(u.text);
              streamedRef.current = false;
              break;
            case "notice":
              // Relay itself talking — a resume that could not be honoured.
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
        if (u.kind === "turns") {
          // Live progress toward the ceiling: turns counted per assistant
          // message. Cleared when the run ends (text/done/failed below).
          const count = (u as RunUpdate & { count?: number }).count ?? 0;
          setStreams((prev) => ({
            ...prev,
            [u.card_id]: { ...(prev[u.card_id] ?? { text: "", thinking: "" }), turns: count },
          }));
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
    keep(events.onInbox((list) => !closed && setProposals(list)));
    keep(
      events.onNavigate((n) => {
        if (closed) return;
        setNavigation({ ...n, at: Date.now() });
        toast("info", "The Director opened " + n.screen, n.why ?? undefined);
      }),
    );
    keep(
      events.onApprovalAsked((a) => {
        if (closed) return;
        toast("warn", "Permission needed", `${a.tool} — ${a.summary}`);
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
          toast("bad", "No project", "Add a git repository first.");
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
        toast("bad", "Nothing to add", "Say what should happen first.");
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
          toast("accent", "Started", `${clean}`);
        } else {
          toast("ok", "Added", mode === "later" ? "Parked in Later" : "Ready to start");
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
        toast("bad", "Stopping", "Work in progress will be committed.");
      }, "Could not stop the run")();
    },
    [toast, withProject],
  );

  const approve = useCallback(
    async (cardId: string) => {
      await withProject(async (id) => {
        await api.approveCard(id, cardId, "approved by you");
        toast("ok", "Approved", "The card is done.");
        await refresh();
      }, "Could not approve the card")();
    },
    [refresh, toast, withProject],
  );

  const reject = useCallback(
    async (cardId: string, why: string) => {
      await withProject(async (id) => {
        await api.rejectCard(id, cardId, why.trim() || "no reason given");
        toast("warn", "Sent back", "The agent gets your reason on the next run.");
        await refresh();
      }, "Could not send the card back")();
    },
    [refresh, toast, withProject],
  );

  const discard = useCallback(
    async (cardId: string) => {
      const card = snapshot?.cards.find((c) => c.id === cardId);
      if (card?.status === "running") {
        toast("bad", "It is running", "Stop the run before deleting the card.");
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
        toast("ok", "Deleted", card?.title ?? cardId);
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
    async (text: string, attachments: string[] = []) => {
      const clean = text.trim();
      if ((!clean && attachments.length === 0) || chatBusy) return;
      // What goes on screen is what the backend will fold into the turn: the
      // message, then the files by name. No hidden context.
      const shown = attachments.length
        ? [clean, attachments.map((f) => `- ${f}`).join("\n")].filter(Boolean).join("\n\n")
        : clean;
      setChat((cs) => [...cs, { role: "user", text: shown, ts: Date.now() }]);
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
        const conversation = await api.chatSend(clean, chatRef.current, attachments);
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
        setChat(foldToolResults(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null)));
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
        toast("ok", archived ? "Archived" : "Restored");
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
        toast("ok", "Deleted", which?.title);
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
        toast("ok", "Added", `${created.name} joined the crew`);
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
        toast("ok", "Duplicated", copy.name);
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
        toast("ok", "Removed", which?.name);
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
            "warn",
            "Allowed once",
            "That command could not be narrowed into a rule, so you will be asked again.",
          );
        } else {
          toast(
            allow ? "ok" : "bad",
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

  const acceptProposal = useCallback(
    async (proposalId: string) => {
      try {
        const accepted = await api.inboxAccept(proposalId);
        toast(
          "ok",
          "Card created",
          `${accepted.title} — born in the harness's own project as ${accepted.card_id}`,
        );
      } catch (e) {
        fail(e, "Could not create the card");
      }
    },
    [fail, toast],
  );

  const dismissProposal = useCallback(
    async (proposalId: string) => {
      try {
        await api.inboxDismiss(proposalId);
      } catch (e) {
        fail(e, "Could not dismiss the proposal");
      }
    },
    [fail],
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
      toast("ok", "Project added", project.name);
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
        toast("info", "Already added", info.name);
        return;
      }
      if (info.next === "missing") {
        toast("bad", "Gone", `${info.path} is not a directory any more.`);
        return;
      }
      if (info.next === "confirm_init") {
        // Files but no repository: never git init behind the operator's back.
        const ok = window.confirm(
          `${info.path} has files but no git repository.

` +
            "Run git init there so Relay can work on it?",
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
        toast("bad", "Name it first", "A project needs a name.");
        return;
      }
      try {
        const parent = await api.pickFolder();
        if (!parent) return;
        const project = await api.projectCreate(parent, clean);
        await refreshProjects();
        selectProject(project.id);
        toast("ok", "Project created", `${project.name} — a fresh repository`);
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
        toast("ok", "Removed", deleteData ? "Project and its history" : "Project forgotten");
      } catch (e) {
        fail(e, "Could not remove the project");
      }
    },
    [fail, toast],
  );

  const installSidecar = useCallback(async () => {
    toast("info", "Installing", "Fetching the agent SDK…");
    try {
      await api.sidecarInstall();
      await refreshStatus();
      toast("ok", "Sidecar ready", "Agents can run now.");
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
    proposals,
    acceptProposal,
    dismissProposal,
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
