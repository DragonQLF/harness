/** Every call into the backend, in one place and typed.
 *  Nothing else in the frontend imports `invoke`. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActiveRun,
  ActivityRow,
  AgentProfile,
  AgentStats,
  Bootstrap,
  CardDiff,
  CatalogModel,
  ClosingBegan,
  ClosingPhase,
  CheckRow,
  Conversation,
  CreatedCard,
  Envelope,
  Navigation,
  PendingApproval,
  PendingUpdate,
  Project,
  Proposal,
  FolderInfo,
  ProjectDetail,
  ProjectStats,
  ProjectView,
  QueueRow,
  RunLogLine,
  RunUpdate,
  Settings,
  Snapshot,
  Status,
  SystemStatus,
  WorktreeRow,
} from "./types";

/** Minutes the operator's clock is behind UTC, for day bucketing. */
export const tzOffsetMinutes = () => new Date().getTimezoneOffset();

export const api = {
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  status: () => invoke<SystemStatus>("status"),

  settingsGet: () => invoke<Settings>("settings_get"),
  settingsUpdate: (settings: Settings) => invoke<Settings>("settings_update", { settings }),

  agentsGet: () => invoke<AgentProfile[]>("agents_get"),
  agentsSave: (agents: AgentProfile[]) => invoke<AgentProfile[]>("agents_save", { agents }),
  agentsStats: () =>
    invoke<AgentStats[]>("agents_stats", { tzOffsetMinutes: tzOffsetMinutes() }),

  projects: () =>
    invoke<ProjectView[]>("projects_list", { tzOffsetMinutes: tzOffsetMinutes() }),
  pickFolder: () => invoke<string | null>("project_pick_folder"),
  inspectFolder: (path: string) => invoke<FolderInfo>("project_inspect", { path }),
  projectAdd: (path: string, name?: string, init = false) =>
    invoke<Project>("project_add", { path, name: name ?? null, init }),
  projectCreate: (parent: string, name: string) =>
    invoke<Project>("project_create", { parent, name }),
  projectUpdate: (project: Project) => invoke<Project>("project_update", { project }),
  /** Point mirror mode at Relay's own source, cloning it if it is not here. */
  mirrorSetup: () => invoke<Project>("mirror_setup"),
  projectRemove: (projectId: string, deleteData: boolean) =>
    invoke<void>("project_remove", { projectId, deleteData }),
  projectDetail: (projectId: string, commitLimit = 14) =>
    invoke<ProjectDetail>("project_detail", { projectId, commitLimit }),
  projectStats: (projectId: string) =>
    invoke<ProjectStats>("project_stats", { projectId, tzOffsetMinutes: tzOffsetMinutes() }),
  checks: (projectId: string) => invoke<CheckRow[]>("project_checks", { projectId }),
  setChecks: (projectId: string, checks: CheckRow[]) =>
    invoke<CheckRow[]>("project_set_checks", { projectId, checks }),
  runChecks: (projectId: string) => invoke<CheckRow[]>("project_run_checks", { projectId }),

  worktrees: (projectId: string) => invoke<WorktreeRow[]>("worktrees", { projectId }),
  removeWorktree: (projectId: string, path: string) =>
    invoke<void>("remove_worktree", { projectId, path }),
  reveal: (path: string) => invoke<void>("reveal_path", { path }),

  snapshot: (projectId: string) => invoke<Snapshot>("snapshot", { projectId }),
  createCard: (projectId: string, title: string, agentId: string, start: boolean, ready: boolean) =>
    invoke<CreatedCard>("create_card", { projectId, title, agentId, start, ready }),
  moveCard: (projectId: string, cardId: string, to: Status) =>
    invoke<number>("move_card", { projectId, cardId, to }),
  overrideCard: (projectId: string, cardId: string, to: Status, reason: string) =>
    invoke<number>("override_card", { projectId, cardId, to, reason }),
  assignAgent: (projectId: string, cardId: string, agentId: string) =>
    invoke<number>("assign_agent", { projectId, cardId, agentId }),
  approveCard: (projectId: string, cardId: string, reason?: string) =>
    invoke<number>("approve_card", { projectId, cardId, reason: reason ?? null }),
  rejectCard: (projectId: string, cardId: string, reason: string) =>
    invoke<number>("reject_card", { projectId, cardId, reason }),
  discardCard: (projectId: string, cardId: string, reason?: string) =>
    invoke<number>("discard_card", { projectId, cardId, reason: reason ?? null }),
  startRun: (projectId: string, cardId: string, prompt?: string) =>
    invoke<string>("start_run", { projectId, cardId, prompt: prompt ?? null }),
  cancelRun: (projectId: string, cardId: string) =>
    invoke<void>("cancel_run", { projectId, cardId }),
  activeRuns: (projectId: string) => invoke<ActiveRun[]>("active_runs", { projectId }),
  runLog: (projectId: string, runId: string) =>
    invoke<RunLogLine[]>("run_log", { projectId, runId }),
  /** What a card changed against the base branch, read from its worktree. */
  cardDiff: (projectId: string, cardId: string) =>
    invoke<CardDiff>("card_diff", { projectId, cardId }),
  reviewQueue: (projectId: string) => invoke<QueueRow[]>("review_queue", { projectId }),
  analystAsk: (projectId: string | null) => invoke<string>("analyst_ask", { projectId }),
  /** Stop waiting for the close sequence; the window goes as soon as it can. */
  closeNow: () => invoke<void>("close_now"),
  /** What models an endpoint offers. Cached a day behind the scenes. */
  modelCatalog: (providerId: string, baseUrl: string, refresh = false) =>
    invoke<CatalogModel[]>("model_catalog", { providerId, baseUrl, refresh }),
  updatesList: () => invoke<PendingUpdate[]>("updates_list"),
  updateInstall: (cardId: string) => invoke<void>("update_install", { cardId }),
  chatStop: (conversationId: string) => invoke<void>("chat_stop", { conversationId }),
  // ---- conversations ----
  /** Every chat, newest first. Archived ones only when asked for. */
  conversations: (includeArchived = false) =>
    invoke<Conversation[]>("conversations_list", { includeArchived }),
  /** A new chat, which means a new Claude session: nothing is resumed. */
  conversationNew: (profileId?: string | null, projectId?: string | null) =>
    invoke<Conversation>("conversation_new", {
      profileId: profileId ?? null,
      projectId: projectId ?? null,
    }),
  /** The chat to talk in: the last one for this profile, or a new one. */
  conversationOpen: (profileId?: string | null, projectId?: string | null) =>
    invoke<Conversation>("conversation_open", {
      profileId: profileId ?? null,
      projectId: projectId ?? null,
    }),
  conversationSelect: (conversationId: string) =>
    invoke<Conversation>("conversation_select", { conversationId }),
  conversationRename: (conversationId: string, title: string) =>
    invoke<Conversation>("conversation_rename", { conversationId, title }),
  conversationArchive: (conversationId: string, archived: boolean) =>
    invoke<Conversation>("conversation_archive", { conversationId, archived }),
  conversationDelete: (conversationId: string) =>
    invoke<void>("conversation_delete", { conversationId }),
  /** Pin a chat to a project: that is the code it can read while answering. */
  conversationPin: (conversationId: string, projectId: string | null) =>
    invoke<Conversation>("conversation_pin", { conversationId, projectId }),
  /** The stored transcript, readable whether or not the session resumes. */
  conversationTranscript: (conversationId: string) =>
    invoke<RunLogLine[]>("conversation_transcript", { conversationId }),
  /** Send a message. The answer streams back on the run channel, keyed by the
   *  conversation id. */
  chatSend: (text: string, conversationId?: string | null, attachments: string[] = []) =>
    invoke<Conversation>("chat_send", {
      text,
      conversationId: conversationId ?? null,
      attachments,
    }),
  /** Native picker for files to attach to the next message. */
  pickFiles: () => invoke<string[]>("chat_pick_files"),

  /** Profiles you can create from. Fetched on request: a template is a menu
   *  entry, never something Relay installs by itself. */
  agentTemplates: () => invoke<AgentProfile[]>("agent_templates"),
  agentCreateFromTemplate: (templateId: string) =>
    invoke<AgentProfile>("agent_create_from_template", { templateId }),
  agentDuplicate: (agentId: string) => invoke<AgentProfile>("agent_duplicate", { agentId }),
  agentRemove: (agentId: string) => invoke<AgentProfile[]>("agent_remove", { agentId }),
  activity: (projectId: string, limit = 200) =>
    invoke<ActivityRow[]>("activity", { projectId, limit }),

  approvalsPending: () => invoke<PendingApproval[]>("approvals_pending"),
  /** Answer a permission request. With `always`, the returned label is the
   *  scoped rule that was recorded — null when the call could not be scoped
   *  safely and will be asked about again. */
  respondApproval: (requestId: string, allow: boolean, always: boolean) =>
    invoke<string | null>("respond_approval", { requestId, allow, always }),

  // ---- inbox ----
  /** The Director's improvement proposals, newest first. */
  inbox: () => invoke<Proposal[]>("inbox_list"),
  /** Accept one: its card is born in the harness's own project. */
  inboxAccept: (proposalId: string) =>
    invoke<Proposal>("inbox_accept", { proposalId }),
  inboxDismiss: (proposalId: string) =>
    invoke<Proposal>("inbox_dismiss", { proposalId }),

  sidecarInstall: () => invoke<string>("sidecar_install"),
  /** Keep the three describing lines in the macOS menu bar true. The wording
   *  is settled here, where the window already formats the same facts. */
  syncMenu: (claude: string, cli: string, budget: string) =>
    invoke<void>("sync_menu", { claude, cli, budget }),
  openClaudeTerminal: (projectId?: string) =>
    invoke<void>("open_claude_terminal", { projectId: projectId ?? null }),
  openAgentTerminal: (projectId: string, cardId: string) =>
    invoke<void>("open_agent_terminal", { projectId, cardId }),
};

/** Backend push channels. Every one is unsubscribed on teardown. */
export const events = {
  onEngineEvent: (fn: (e: Envelope) => void) =>
    listen<Envelope>("engine://event", (evt) => fn(evt.payload)),
  onRunUpdate: (fn: (u: RunUpdate) => void) =>
    listen<RunUpdate>("engine://run", (evt) => fn(evt.payload)),
  onApprovalAsked: (fn: (a: PendingApproval) => void) =>
    listen<PendingApproval>("approvals://asked", (evt) => fn(evt.payload)),
  onApprovalQueue: (fn: (a: PendingApproval[]) => void) =>
    listen<PendingApproval[]>("approvals://pending", (evt) => fn(evt.payload)),
  /** The Director asked to take the operator somewhere. */
  onNavigate: (fn: (n: Navigation) => void) =>
    listen<Navigation>("ui://navigate", (evt) => fn(evt.payload)),
  /** The conversation list changed on the backend. */
  onConversations: (fn: (list: Conversation[]) => void) =>
    listen<Conversation[]>("chat://conversations", (evt) => fn(evt.payload)),
  /** A proposal was filed, accepted or dismissed on the backend. */
  onInbox: (fn: (list: Proposal[]) => void) =>
    listen<Proposal[]>("inbox://proposals", (evt) => fn(evt.payload)),
  /** Relay's own repository moved without a card behind it (#86).
   *
   *  The payload is **one string**, not a structure: `workspace.rs` emits the
   *  prose that `harness_app::mirror::describe` writes, and the `OutsideWork`
   *  it was folded from — the count, the paths, the oldest timestamp — never
   *  crosses. So the counts on screen are the ones inside that sentence, and
   *  nothing here recomputes them.
   *
   *  It arrives at most twice a session (startup and the end-of-day close) and
   *  is never repeated: the backend re-anchors the sha it compares against on
   *  every look, so the same commits are reported once and then are past. */
  onOutsideWork: (fn: (said: string) => void) =>
    listen<string>("mirror://outside-work", (evt) => fn(evt.payload)),
  /** An item was picked in the macOS menu bar; the payload is its id. */
  onMenuPick: (fn: (id: string) => void) =>
    listen<string>("menu://picked", (evt) => fn(evt.payload)),
  onSidecarLog: (fn: (line: string) => void) =>
    listen<string>("sidecar://log", (evt) => fn(evt.payload)),
  /** The window is being held: what for, and for how long at most. */
  onClosingBegan: (fn: (c: ClosingBegan) => void) =>
    listen<ClosingBegan>("closing://began", (evt) => fn(evt.payload)),
  /** Where the close sequence got to. */
  onClosingPhase: (fn: (p: ClosingPhase) => void) =>
    listen<ClosingPhase>("closing://phase", (evt) => fn(evt.payload)),
};

export type { UnlistenFn };

/** Turn any thrown value into something worth showing an operator. */
export function reason(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
