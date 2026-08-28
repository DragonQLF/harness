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
  PendingApproval,
  Proposal,
  ProjectStats,
  Navigation,
  ProjectView,
  RunUpdate,
  Settings,
  Snapshot,
  Status,
  SystemStatus,
} from "../lib/types";

// O redutor de eventos mudou-se para `./events` e o estado de chat para
// `./chat`; reexportam-se aqui para que quem importa do store continue a
// importar do mesmo sítio.
export { toLine } from "./events";
export type { LiveStream, LogLine } from "./events";
export type { ChatMsg } from "./chat";

export interface Toast {
  id: number;
  tone: string;
  title: string;
  body?: string;
}

/** Um aviso de trabalho fora do quadro (#86), tal como chegou.
 *
 *  Não é um espelho de nada do backend: o backend guarda só o **último**
 *  achado, numa string, e não o expõe por comando nenhum — nem no `bootstrap`.
 *  Isto é o registo do que esta janela recebeu, como os `Toast` ou as linhas
 *  do run feed, e por isso o id e a hora são desta janela e dizem-se assim
 *  ("seen", não "detected"). */
export interface OutsideWork {
  id: number;
  /** O texto tal e qual o backend o escreveu. Nada aqui o recalcula. */
  said: string;
  /** Quando **este ecrã** o recebeu. */
  seen_ms: number;
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
  /** Warnings that Relay's own source moved without a card behind it, newest
   *  first. They never expire on their own and nothing here clears them: only
   *  `dismissOutsideWork` takes one off the rail. */
  outsideWork: OutsideWork[];
  dismissOutsideWork: (id: number) => void;
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
  const feed = useRunFeed();
  const [approvals, setApprovals] = useState<PendingApproval[]>([]);
  const [proposals, setProposals] = useState<Proposal[]>([]);
  const [outsideWork, setOutsideWork] = useState<OutsideWork[]>([]);
  const [diffs, setDiffs] = useState<Record<string, CardDiff>>({});
  const [navigation, setNavigation] = useState<(Navigation & { at: number }) | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const projectRef = useRef<string | null>(null);
  projectRef.current = projectId;
  const toastSeq = useRef(0);
  const outsideSeq = useRef(0);

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
        chat.hydrate(boot.conversations, boot.last_conversation);
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
      events.onOutsideWork((said) => {
        if (closed) return;
        const text = said.trim();
        if (!text) return;
        setOutsideWork((prev) => {
          // The look runs twice a session and re-anchors its sha each time, so
          // the same sentence twice means the same commits twice: keep the
          // first, which carries the hour it actually arrived.
          if (prev.some((w) => w.said === text)) return prev;
          return [{ id: ++outsideSeq.current, said: text, seen_ms: Date.now() }, ...prev];
        });
        // The toast is the only signal on the two screens the rail is hidden
        // from. It is the *extra*, never the surface: the rail entry above is
        // what stays until the operator says otherwise.
        toast(
          "warn",
          "Work that skipped the board",
          "Relay's own repository moved without a card behind it — see Right now.",
        );
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
    outputs: feed.outputs,
    streams: feed.streams,
    approvals,
    diffs,
    loadCardDiff,
    conversations: chat.conversations,
    conversationId: chat.conversationId,
    conversation: chat.conversations.find((c) => c.id === chat.conversationId) ?? null,
    chat: chat.chat,
    chatBusy: chat.chatBusy,
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
