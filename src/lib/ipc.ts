/** Every call into the backend, in one place and typed.
 *  Nothing else in the frontend imports `invoke`.
 *
 *  Fica um ficheiro, e isso é uma escolha e não inércia. Um `api` por
 *  funcionalidade poupava algumas linhas de importação e custava a única coisa
 *  que este ficheiro dá: **a lista completa do que a janela pode pedir**, numa
 *  página que se lê de cima a baixo ao lado do `invoke_handler` do `lib.rs`.
 *  É assim que se vê que um comando registado no Rust não tem porta cá — e é
 *  isso que aponta os que hoje não têm ecrã nenhum (`DEBT.md`). Repartido por
 *  ecrã, um comando sem porta deixa de ser uma ausência visível e passa a ser
 *  um ficheiro que ninguém abriu.
 *
 *  O que faltava eram os cabeçalhos: metade dos membros vivia numa corrida sem
 *  título nenhum, e os três que existiam tinham deixado de corresponder ao que
 *  estava por baixo. São a mesma divisão por dono que os módulos de comandos
 *  do Rust têm. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActiveRun,
  ActivityRow,
  AttachmentPreview,
  BrowserOffer,
  SkillOffer,
  AgentProfile,
  AgentStats,
  Bootstrap,
  CardChecks,
  CardChecksEvent,
  CardDiff,
  CatalogModel,
  ClosingBegan,
  ClosingPhase,
  CheckRow,
  Conversation,
  ConversationTotals,
  CreatedCard,
  Envelope,
  FileText,
  Hunk,
  MirrorWarning,
  Navigation,
  PendingApproval,
  PendingUpdate,
  Project,
  Proposal,
  FolderInfo,
  ProjectDetail,
  ProjectStats,
  ProjectView,
  Queued,
  QueueRow,
  ActorFilter,
  RunLogLine,
  RunStats,
  RunUpdate,
  Settings,
  Snapshot,
  Status,
  SystemStatus,
  TranscriptExport,
  TreeEntry,
  WorktreeRow,
} from "./types";

/** Minutes the operator's clock is behind UTC, for day bucketing. */
export const tzOffsetMinutes = () => new Date().getTimezoneOffset();

export const api = {
  // ---- the shell: what the window opens with ----
  bootstrap: () => invoke<Bootstrap>("bootstrap"),
  status: () => invoke<SystemStatus>("status"),

  // ---- settings ----
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsUpdate: (settings: Settings) => invoke<Settings>("settings_update", { settings }),

  // ---- the crew ----
  agentsGet: () => invoke<AgentProfile[]>("agents_get"),
  skillOffers: () => invoke<SkillOffer[]>("skill_offers"),
  skillGrant: (agentId: string, skillId: string) =>
    invoke<AgentProfile[]>("skill_grant", { agentId, skillId }),
  browserOffers: () => invoke<BrowserOffer[]>("browser_offers"),
  browserGrant: (agentId: string, browserId: string) =>
    invoke<AgentProfile[]>("browser_grant", { agentId, browserId }),
  agentsSave: (agents: AgentProfile[]) => invoke<AgentProfile[]>("agents_save", { agents }),
  agentsStats: () =>
    invoke<AgentStats[]>("agents_stats", { tzOffsetMinutes: tzOffsetMinutes() }),

  // ---- projects ----
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
  // ---- checks ----
  checks: (projectId: string) => invoke<CheckRow[]>("project_checks", { projectId }),
  setChecks: (projectId: string, checks: CheckRow[]) =>
    invoke<CheckRow[]>("project_set_checks", { projectId, checks }),
  runChecks: (projectId: string) => invoke<CheckRow[]>("project_run_checks", { projectId }),
  /** The last check pass made in this card's own worktree. `null` means none
   *  ever was, which is not the same fact as nothing having failed. */
  cardChecks: (projectId: string, cardId: string) =>
    invoke<CardChecks | null>("card_checks", { projectId, cardId }),
  /** Run the project's checks in this card's worktree and record the result
   *  against the card. */
  cardRunChecks: (projectId: string, cardId: string) =>
    invoke<CardChecks>("card_run_checks", { projectId, cardId }),

  // ---- worktrees ----
  worktrees: (projectId: string) => invoke<WorktreeRow[]>("worktrees", { projectId }),
  removeWorktree: (projectId: string, path: string) =>
    invoke<void>("remove_worktree", { projectId, path }),
  reveal: (path: string) => invoke<void>("reveal_path", { path }),

  // ---- the board, and the runs it starts ----
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
  /** Copy run transcripts into a folder the operator picks. `runIds` omitted
   *  takes every transcript the project has; null comes back when the picker
   *  was dismissed. */
  // ---- sessions ----
  exportTranscripts: (projectId: string, runIds?: string[]) =>
    invoke<TranscriptExport | null>("export_transcripts", {
      projectId,
      runIds: runIds ?? null,
    }),
  /** What a card changed against the base branch, read from its worktree. */
  // ---- review ----
  cardDiff: (projectId: string, cardId: string) =>
    invoke<CardDiff>("card_diff", { projectId, cardId }),
  reviewQueue: (projectId: string) => invoke<QueueRow[]>("review_queue", { projectId }),
  /** Which cards must be Done before this one may start. The command has been
   *  registered since the board was written; it had no wrapper, so the one
   *  affordance that reaches it — Deps on a blocked card — had nowhere to go. */
  setDependencies: (projectId: string, cardId: string, dependsOn: string[]) =>
    invoke<number>("set_dependencies", { projectId, cardId, dependsOn }),
  /** Correct a card's wording in place. The engine refuses once the card has
   *  run — by then a transcript and a commit subject carry the old title — so
   *  this is only ever offered while `runs == 0`. */
  editCard: (projectId: string, cardId: string, title: string) =>
    invoke<number>("edit_card", { projectId, cardId, title }),
  /** Finished runs across the last 38 weeks: the heatmap, the three window
   *  tiles, the per-actor spend and the day's line counts. `actor` is the
   *  heatmap's own tab and narrows the run counts, never the money. */
  // ---- numbers ----
  runStats: (projectId: string, actor: ActorFilter = "all") =>
    invoke<RunStats>("run_stats", {
      projectId,
      actor,
      tzOffsetMinutes: tzOffsetMinutes(),
    }),

  // ---- code ----
  /** Every file in a card's worktree, with the ones it changed marked. With
   *  no card it is the project's own checkout. */
  listTree: (projectId: string, cardId?: string | null) =>
    invoke<TreeEntry[]>("list_tree", { projectId, cardId: cardId ?? null }),
  /** One file, read-only. `rev` reads it out of a commit instead of off disk. */
  readWorktreeFile: (
    projectId: string,
    cardId: string | null,
    path: string,
    rev?: string | null,
  ) =>
    invoke<FileText>("read_worktree_file", {
      projectId,
      cardId: cardId ?? null,
      path,
      rev: rev ?? null,
    }),
  /** What a card changed, as `@@` blocks. `path` narrows it to one file. */
  diffHunks: (projectId: string, cardId: string, path?: string | null) =>
    invoke<Hunk[]>("diff_hunks", { projectId, cardId, path: path ?? null }),
  /** Decide one block of a card's diff. The block is named by its file and its
   *  `@@` header — git's own identity for it — and what the verdict means for
   *  the card is the engine's to decide: nothing until every block has one,
   *  then approve, send back, or approve with the rejected blocks carried onto
   *  a follow-up card. Returns the sequence number of the decision. */
  reviewHunk: (
    projectId: string,
    cardId: string,
    file: string,
    header: string,
    approved: boolean,
    reason?: string,
  ) =>
    invoke<number>("review_hunk", {
      projectId,
      cardId,
      file,
      header,
      approved,
      reason: reason ?? null,
    }),

  // ---- the analyst, on demand ----
  analystAsk: (projectId: string | null) => invoke<string>("analyst_ask", { projectId }),
  /** Stop waiting for the close sequence; the window goes as soon as it can. */
  // ---- closing ----
  closeNow: () => invoke<void>("close_now"),
  /** What models an endpoint offers. Cached a day behind the scenes. */
  // ---- model endpoints ----
  modelCatalog: (providerId: string, baseUrl: string, refresh = false) =>
    invoke<CatalogModel[]>("model_catalog", { providerId, baseUrl, refresh }),
  // ---- updates ----
  updatesList: () => invoke<PendingUpdate[]>("updates_list"),
  updateInstall: (cardId: string) => invoke<void>("update_install", { cardId }),
  // ---- conversations: the live turn ----
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
  /** The thread's accounting, counted over the whole transcript rather than
   *  over whatever the screen has loaded. */
  conversationTotals: (conversationId: string) =>
    invoke<ConversationTotals>("conversation_totals", { conversationId }),
  /** Say something to a conversation — the one way in. It does not interrupt
   *  and never starts a second turn: with a turn in flight the message goes
   *  into that run's inbox and the model reads it at its next read; with none
   *  it becomes an ordinary turn, and says so by answering with a null
   *  `queue_id`. The screen never has to know which. */
  chatQueue: (
    text: string,
    conversationId: string,
    attachments: string[] = [],
    effort: string | null = null,
  ) => invoke<Queued>("chat_queue", { text, conversationId, attachments, effort }),
  /** Native picker for files to attach to the next message. */
  pickFiles: () => invoke<string[]>("chat_pick_files"),
  /** Write a pasted or dropped attachment to disk and answer with its path.
   *
   *  Everything downstream of an attachment speaks in paths — the agent is
   *  told to read the file with its own tools — and the clipboard speaks in
   *  bytes. This is where the two meet. `data` is base64: a 20 MB image as a
   *  JSON array of numbers is ~100 MB of wire. */
  saveAttachment: (name: string | null, mime: string, data: string) =>
    invoke<string>("chat_save_attachment", { name, mime, data }),
  /** Enough about an attachment to draw it rather than name it. */
  attachmentPreview: (path: string) =>
    invoke<AttachmentPreview>("chat_attachment_preview", { path }),

  /** Profiles you can create from. Fetched on request: a template is a menu
   *  entry, never something Relay installs by itself. */
  // ---- crew templates ----
  agentTemplates: () => invoke<AgentProfile[]>("agent_templates"),
  agentCreateFromTemplate: (templateId: string) =>
    invoke<AgentProfile>("agent_create_from_template", { templateId }),
  agentDuplicate: (agentId: string) => invoke<AgentProfile>("agent_duplicate", { agentId }),
  agentRemove: (agentId: string) => invoke<AgentProfile[]>("agent_remove", { agentId }),
  // ---- activity ----
  activity: (projectId: string, limit = 200) =>
    invoke<ActivityRow[]>("activity", { projectId, limit }),

  // ---- approvals ----
  approvalsPending: () => invoke<PendingApproval[]>("approvals_pending"),
  /** Answer a permission request. With `always`, the returned label is the
   *  scoped rule that was recorded — null when the call could not be scoped
   *  safely and will be asked about again. */
  respondApproval: (requestId: string, allow: boolean, always: boolean) =>
    invoke<string | null>("respond_approval", { requestId, allow, always }),

  // ---- inbox ----
  /** The Director's improvement proposals, newest first. */
  inbox: () => invoke<Proposal[]>("inbox_list"),
  /** Accept one: permission for the Director to act on it, not a card. He is
   *  told in his next turn and creates it himself, or does not. */
  inboxAccept: (proposalId: string) =>
    invoke<Proposal>("inbox_accept", { proposalId }),
  inboxDismiss: (proposalId: string) =>
    invoke<Proposal>("inbox_dismiss", { proposalId }),

  // ---- the machine: sidecar, menu, terminals ----
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
  /** A card's checks were run again — a finished run triggers its own pass, so
   *  this arrives without anybody having asked for it. */
  onCardChecks: (fn: (e: CardChecksEvent) => void) =>
    listen<CardChecksEvent>("checks://card", (evt) => fn(evt.payload)),
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
   *  The payload carries the `OutsideWork` the backend folded the commits into
   *  — the count, the paths, the oldest timestamp — beside the half of the
   *  sentence that addresses the Director. So the numbers on screen are the
   *  backend's numbers, not counts read back out of a paragraph, and nothing
   *  here cuts prose.
   *
   *  It arrives at most twice a session (startup and the end-of-day close) and
   *  is never repeated: the backend re-anchors the sha it compares against on
   *  every look, so the same commits are reported once and then are past. The
   *  same value comes back on `bootstrap`, because the startup emit may have
   *  had no window to reach. */
  onOutsideWork: (fn: (w: MirrorWarning) => void) =>
    listen<MirrorWarning>("mirror://outside-work", (evt) => fn(evt.payload)),
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
