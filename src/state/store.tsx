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
import { useChat, type ChatMsg } from "./chat";
import { useRunFeed, type LiveStream, type LogLine } from "./events";
import type {
  ActivityRow,
  AgentProfile,
  AgentStats,
  CardDiff,
  Conversation,
  Envelope,
  MirrorWarning,
  PendingApproval,
  Proposal,
  ProjectStats,
  Navigation,
  ProjectView,
  RunUpdate,
  Settings,
  Snapshot,
  WorktreeRow,
  Status,
  SystemStatus,
  SlashCommand,
} from "../lib/types";

// O redutor de eventos mudou-se para `./events` e o estado de chat para
// `./chat`; reexportam-se aqui para que quem importa do store continue a
// importar do mesmo sítio.
export { toLine } from "./events";
export { toolName } from "./chat";
export type { LiveStream, LogLine } from "./events";
export type { ChatMsg } from "./chat";

export interface Toast {
  id: number;
  tone: string;
  title: string;
  body?: string;
}

/** Um aviso de trabalho fora do quadro (#86), tal como chegou a esta janela.
 *
 *  O achado é do backend e vem inteiro (`MirrorWarning`); o que é desta janela
 *  é só o id e a hora — por isso o rail diz "seen" e não "detected". O backend
 *  guarda apenas o **último**, e isto é uma lista: um aviso novo não apaga o
 *  anterior. */
export interface OutsideWorkSeen {
  id: number;
  /** Os factos e a metade que fala ao Director, tal como o backend os escreveu.
   *  Nada aqui os recalcula nem os conta a partir de texto. */
  warning: MirrorWarning;
  /** Quando **este ecrã** o recebeu. */
  seen_ms: number;
}

/** A identidade de um aviso são os factos, não a frase.
 *
 *  O mesmo achado chega por dois caminhos — o evento e o `bootstrap` —, e o
 *  operador não pode ver o mesmo aviso duas vezes. A chave é o achado em si:
 *  quantos commits, desde quando, e que ficheiros. É o que o backend
 *  descobriu sobre o repositório, e é igual venha por onde vier; a prosa não
 *  serve, porque a idade que ela cita ("3 hours ago") é relativa ao instante
 *  em que foi escrita e a mesma descoberta descrita duas vezes daria duas
 *  frases diferentes. */
const outsideWorkKey = (w: MirrorWarning) =>
  [w.work.commits, w.work.since_ms, w.work.files_total, w.work.files.join(">")].join("|");

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
  /** One checkout per card, read with the board. The sidebar counts them and
   *  Home lists them, so they travel with the snapshot rather than being
   *  fetched twice. */
  worktrees: WorktreeRow[];
  outputs: Record<string, LogLine[]>;
  /** What a loaded run actually ran on, per card. See `useRunFeed`. */
  runModels: Record<string, string>;
  /** Token-level stream per card, cleared when the final text arrives. */
  streams: Record<string, LiveStream>;
  approvals: PendingApproval[];
  /** The Director's improvement proposals waiting on you, newest first. */
  proposals: Proposal[];
  /** Warnings that Relay's own source moved without a card behind it, newest
   *  first. They never expire on their own and nothing here clears them: only
   *  `dismissOutsideWork` takes one off the rail. */
  outsideWork: OutsideWorkSeen[];
  dismissOutsideWork: (id: number) => void;
  /** What each card changed, once something has asked for it. */
  diffs: Record<string, CardDiff>;
  /** Read a card's diff from its worktree. Cheap to call again: the answer
   *  replaces the cached one, so a re-run shows the new patch. */
  loadCardDiff: (cardId: string) => Promise<void>;
  /** Every conversation the backend knows about, newest first. */
  conversations: Conversation[];
  /** The one on screen, or `null` while this is a draft: a chat that has not
   *  been created yet, because nothing has been sent in it. */
  conversationId: string | null;
  /** Which profile a draft will speak to; `null` means the Director. */
  draftProfile: string | null;
  conversation: Conversation | null;
  chat: ChatMsg[];
  /** O modelo em que a conversa aberta correu de facto. Ver `state/chat`. */
  chatModel: string | null;
  chatBusy: boolean;
  /** A stored transcript is being read off disk — not the model answering. */
  chatLoading: boolean;
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

  /** What `/` can reach in this session — the engine's own commands and what
   *  the granted skills brought. Published by the engine; never assembled
   *  here, so a granted skill shows up without Relay knowing its name. */
  commands: SlashCommand[];
  /** Say something. Never refused while the agent is working: the message is
   *  queued into the turn in flight, shown as not yet read, and settles into
   *  an ordinary one when the backend says the model has it.
   *
   *  `effort` is the level currently chosen. It binds the request, so a
   *  change takes effect on the next message without a new session. */
  sendChat: (text: string, attachments?: string[], effort?: string | null) => Promise<void>;
  /** Put the screen into a draft. The row and its Claude session are created
   *  by the first message, so a draft nobody types into costs nothing. */
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
  const [worktrees, setWorktrees] = useState<WorktreeRow[]>([]);
  const feed = useRunFeed();
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [outsideWork, setOutsideWork] = useState<OutsideWorkSeen[]>([]);
  const [diffs, setDiffs] = useState<Record<string, CardDiff>>({});
  const [navigation, setNavigation] = useState<(Navigation & { at: number }) | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const projectRef = useRef<string | null>(null);
  projectRef.current = projectId;
  const toastSeq = useRef(0);
  const outsideSeq = useRef(0);
  /** Which findings this window has already been told about, by
   *  `outsideWorkKey`. See `noteOutsideWork`. */
  const outsideKeys = useRef<Set<string>>(new Set());

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

  /** Put a warning about work that skipped the board on the rail.
   *
   *  Two callers, on purpose: the `mirror://outside-work` event, and the
   *  `bootstrap` field that carries whatever the backend already had. The
   *  startup look is spawned before this window exists, so the event may have
   *  been emitted to nobody, and a reload loses it the same way — the two
   *  paths together are what makes the warning survive both.
   *
   *  Whichever arrives first wins, and the second is dropped by
   *  `outsideWorkKey`. The toast rides on the append rather than on the event,
   *  so it fires exactly once per warning per window — which is the same claim
   *  the rail's "seen HH:MM" makes: this window has just learned about this. */
  const noteOutsideWork = useCallback(
    (warning: MirrorWarning) => {
      // The ledger is a ref and not the list itself because the decision has
      // to be made now, in this call: a state updater runs during the next
      // render, too late to say whether the toast is owed. Dismissing takes
      // the entry off the rail without forgetting the key, so the copy that
      // arrives by the other path cannot put it back.
      const key = outsideWorkKey(warning);
      if (outsideKeys.current.has(key)) return;
      outsideKeys.current.add(key);
      setOutsideWork((prev) => [
        { id: ++outsideSeq.current, warning, seen_ms: Date.now() },
        ...prev,
      ]);
      // The toast is the only signal on the two screens the rail is hidden
      // from. It is the *extra*, never the surface: the rail entry is what
      // stays until the operator says otherwise.
      toast(
        "warn",
        "Work that skipped the board",
        "Relay's own repository moved without a card behind it — see Right now.",
      );
    },
    [toast],
  );

  const chat = useChat({ toast, fail, projectRef });

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
      const [snap, st, acts, trees] = await Promise.all([
        api.snapshot(id),
        api.projectStats(id),
        api.activity(id, 200),
        // A repository that has gone missing still has a board; it just has
        // no checkouts to report. That is an empty list, not a failed read.
        api.worktrees(id).catch(() => [] as WorktreeRow[]),
      ]);
      if (projectRef.current !== id) return;
      setSnapshot(snap);
      lastSeqRef.current = snap.last_seq;
      setStats(st);
      setActivity(acts);
      setWorktrees(trees);
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
        // Whatever the look already found, whether or not this window was
        // there to hear it announced. On a reload this is the only path.
        if (boot.outside_work) noteOutsideWork(boot.outside_work);
        chat.hydrate(boot.conversations, boot.last_conversation, boot.commands);
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
    feed.clear();
    setDiffs({});
    // The conversation is not per project: a Director chat outlives switching
    // boards, and is pinned to a project only if you pin it.
    refresh();
    refreshAgentStats();
  }, [projectId, feed.clear, refresh, refreshAgentStats]);

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
        // A conversa consome o que é dela — o resto é do quadro.
        if (chat.consume(u)) return;

        if (u.project_id !== projectRef.current) return;

        feed.consume(u);
      }),
    );

    keep(events.onApprovalQueue((list) => !closed && setApprovals(list)));
    keep(events.onConversations((list) => !closed && chat.setConversations(list)));
    keep(events.onInbox((list) => !closed && setProposals(list)));
    keep(
      events.onOutsideWork((warning) => {
        if (closed) return;
        noteOutsideWork(warning);
      }),
    );
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
  }, [chat.consume, chat.setConversations, feed.consume, scheduleRefresh, refreshProjects, toast]);

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
        feed.reset(cardId);
        await api.startRun(id, cardId, prompt);
        await refresh();
      }, "Could not start the run")();
    },
    [feed.reset, refresh, withProject],
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
        feed.setRunLog(cardId, lines);
      }, "Could not read the run log")();
    },
    [feed.setRunLog, withProject],
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
        // Accepting is permission, not work: nothing is created here.
        toast(
          "ok",
          "Accepted",
          `${accepted.title} — the Director will pick it up on his next turn`,
        );
      } catch (e) {
        fail(e, "Could not accept the proposal");
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

  /** Take a warning off the rail. Nothing is told to the backend, because the
   *  backend has nowhere to be told: the look already re-anchored its sha, so
   *  these commits are past whether the operator dismisses them or not. */
  const dismissOutsideWork = useCallback((id: number) => {
    setOutsideWork((prev) => prev.filter((w) => w.id !== id));
  }, []);

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

  /** A memória aqui não é micro-optimização: sem ela este objecto é novo a
   *  cada render do provider, e um render do provider é *qualquer* coisa que
   *  mude — incluindo o chat a assentar um quadro de texto. Todo o `useStore`
   *  da app re-renderizava por isso, a barra lateral e o título incluídos, que
   *  não leram um token na vida. */
  const value: Store = useMemo(
    () => ({
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
      worktrees,
      outputs: feed.outputs,
      runModels: feed.runModels,
      streams: feed.streams,
      approvals,
      diffs,
      loadCardDiff,
      conversations: chat.conversations,
      conversationId: chat.conversationId,
      draftProfile: chat.draftProfile,
      conversation: chat.conversations.find((c) => c.id === chat.conversationId) ?? null,
      chat: chat.chat,
      chatModel: chat.chatModel,
      chatBusy: chat.chatBusy,
      chatLoading: chat.chatLoading,
      chatThinking: chat.chatThinking,
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
      commands: chat.commands,
      sendChat: chat.sendChat,
      newConversation: chat.newConversation,
      chatWithProfile: chat.chatWithProfile,
      openConversation: chat.openConversation,
      renameConversation: chat.renameConversation,
      archiveConversation: chat.archiveConversation,
      deleteConversation: chat.deleteConversation,
      pinConversation: chat.pinConversation,
      agentTemplates,
      createAgentFromTemplate,
      duplicateAgent,
      removeAgent,
      answerApproval,
      proposals,
      acceptProposal,
      dismissProposal,
      outsideWork,
      dismissOutsideWork,
      saveSettings,
      saveAgents,
      addProject,
      createProject,
      removeProject,
      installSidecar,
    }),
    [
      acceptProposal,
      activity,
      addProject,
      agentStats,
      agentTemplates,
      agents,
      answerApproval,
      approvals,
      approve,
      assignAgent,
      cancelRun,
      chat.archiveConversation,
      chat.chat,
      chat.chatBusy,
      chat.chatModel,
      chat.chatLoading,
      chat.chatThinking,
      chat.chatWithProfile,
      chat.commands,
      chat.conversationId,
      chat.conversations,
      chat.deleteConversation,
      chat.draftProfile,
      chat.newConversation,
      chat.openConversation,
      chat.pinConversation,
      chat.renameConversation,
      chat.sendChat,
      createAgentFromTemplate,
      createCard,
      createProject,
      dataDir,
      diffs,
      discard,
      dismissOutsideWork,
      dismissProposal,
      dismissToast,
      duplicateAgent,
      fatal,
      feed.outputs,
      feed.runModels,
      feed.streams,
      installSidecar,
      loadCardDiff,
      loadRunLog,
      moveCard,
      navigation,
      outsideWork,
      project,
      projectId,
      projects,
      proposals,
      ready,
      refresh,
      refreshProjects,
      refreshStatus,
      reject,
      removeAgent,
      removeProject,
      saveAgents,
      saveSettings,
      selectProject,
      settings,
      snapshot,
      startRun,
      stats,
      status,
      toast,
      toasts,
      worktrees,
    ],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
