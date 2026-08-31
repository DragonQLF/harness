/** Mirrors of the Rust types crossing the IPC boundary.
 *  Field names are the serde names: snake_case in, camelCase out.
 *
 *  Everything under `generated/` is produced from the Rust structs by ts-rs —
 *  run `pnpm codegen` (cargo test --workspace --test export_types) after
 *  changing a Rust type, and never edit those files by hand.
 *
 *  What stays handwritten here is the surface ts-rs cannot mirror faithfully:
 *  - Envelope / RunUpdate / RunEventKind / RunLogLine: flattened event unions
 *    that ts-rs cannot mirror, because `#[serde(flatten)]` on an internally
 *    tagged enum has no shape it can write. Every field one of them can carry
 *    is declared, optional and named — "loose" is the *union*, not the fields.
 *    Six of them used to be read through inline casts at eight call sites,
 *    which is the same thing without the compiler;
 *  - the shell response wrappers that live in src-tauri itself (Bootstrap and
 *    friends), which are glue rather than domain.
 */

// ---- generated from Rust ---------------------------------------------------
import type { Actor } from "./generated/Actor";
import type { ActiveRun } from "./generated/ActiveRun";
import type { ActivityRow } from "./generated/ActivityRow";
import type { AgentProfile } from "./generated/AgentProfile";
import type { AgentStats } from "./generated/AgentStats";
import type { AllowRule } from "./generated/AllowRule";
import type { BranchRow } from "./generated/BranchRow";
import type { BranchState } from "./generated/BranchState";
import type { Card } from "./generated/Card";
import type { CardId } from "./generated/CardId";
import type { CardChecks } from "./generated/CardChecks";
import type { CheckRow } from "./generated/CheckRow";
import type { CommitRow } from "./generated/CommitRow";
import type { Conversation } from "./generated/Conversation";
import type { ConversationTotals } from "./generated/ConversationTotals";
import type { FileText } from "./generated/FileText";
import type { FolderInfo } from "./generated/FolderInfo";
import type { Hunk } from "./generated/Hunk";
import type { HunkLine } from "./generated/HunkLine";
import type { ToolCount } from "./generated/ToolCount";
import type { TranscriptExport } from "./generated/TranscriptExport";
import type { TreeEntry } from "./generated/TreeEntry";
import type { LanguageRow } from "./generated/LanguageRow";
import type { MirrorWarning } from "./generated/MirrorWarning";
import type { OutsideWork } from "./generated/OutsideWork";
import type { PendingApproval } from "./generated/PendingApproval";
import type { Proposal } from "./generated/Proposal";
import type { Project } from "./generated/Project";
import type { Provider } from "./generated/Provider";
import type { ProjectStats } from "./generated/ProjectStats";
import type { Review } from "./generated/Review";
import type { Backend } from "./generated/Backend";
import type { Reviewer } from "./generated/Reviewer";
import type { ActorFilter } from "./generated/ActorFilter";
import type { RunId } from "./generated/RunId";
import type { RunOutcome } from "./generated/RunOutcome";
import type { RunStats } from "./generated/RunStats";
import type { RunWindow } from "./generated/RunWindow";
import type { Spend } from "./generated/Spend";
import type { SessionView } from "./generated/SessionView";
import type { Settings } from "./generated/Settings";
import type { Snapshot } from "./generated/Snapshot";
import type { CatalogModel } from "./generated/CatalogModel";
import type { Status } from "./generated/Status";
import {
  ALL_PERMISSIONS,
  LEGAL_MOVES as GENERATED_LEGAL_MOVES,
  MODELS,
  REVIEWERS,
  SHELL_TOOLS,
  STATUSES,
  WORKTREE_MODES,
  type Choice,
} from "./generated/vocabulary";
import type { McpGrant } from "./generated/McpGrant";
import type { McpTransport } from "./generated/McpTransport";
import type { SkillGrant } from "./generated/SkillGrant";
import type { WorktreeMode } from "./generated/WorktreeMode";
import type { WorktreeRow } from "./generated/WorktreeRow";

export type {
  Actor,
  CatalogModel,
  ActiveRun,
  ActivityRow,
  AgentProfile,
  AgentStats,
  AllowRule,
  BranchRow,
  BranchState,
  Card,
  CardChecks,
  CardId,
  CheckRow,
  CommitRow,
  Conversation,
  ConversationTotals,
  FileText,
  FolderInfo,
  Hunk,
  HunkLine,
  LanguageRow,
  McpGrant,
  McpTransport,
  MirrorWarning,
  OutsideWork,
  PendingApproval,
  Proposal,
  Project,
  Provider,
  ProjectStats,
  Review,
  Backend,
  Reviewer,
  ActorFilter,
  RunId,
  RunOutcome,
  RunStats,
  RunWindow,
  SessionView,
  Spend,
  Settings,
  SkillGrant,
  Snapshot,
  Status,
  ToolCount,
  TranscriptExport,
  TreeEntry,
  WorktreeMode,
  WorktreeRow,
};

// ---- stream events: flattened unions, kept loose on purpose ----------------

/** An engine event. `type` is the domain event tag; the rest is flattened. */
export interface Envelope {
  seq: number;
  ts_ms: number;
  project_id: string;
  type: string;
  card_id?: string;
  title?: string;
  to?: Status;
  from?: Status;
  reason?: string;
  agent_id?: string;
  run_id?: string;
  outcome?: string;
  cost_usd?: number | null;
  turns?: number | null;
  by?: Actor;
}

export type RunEventKind =
  | "started"
  | "user_message"
  /** Said while a turn was already running, and not yet read by the model. */
  | "user_queued"
  /** The same message, once the run has it. The two fold into one bubble. */
  | "user_read"
  | "text"
  | "delta"
  | "thinking"
  | "thought"
  | "tool_use"
  | "tool_result"
  | "turns"
  | "usage"
  | "done"
  | "failed"
  | "approval_requested"
  | "approval_answered"
  | "notice"
  /** What `/` can mean in this session. Live only — asked for again on the
   *  next one, so it is never written to the transcript. */
  | "commands"
  /** A command the engine answered by itself, with no model turn behind it. */
  | "local_output"
  /** Everything running in the background right now. Live only, and a *level*:
   *  each one carries the whole live set and replaces the last. */
  | "background_tasks";

export interface RunUpdate {
  project_id: string;
  card_id: string;
  run_id: string;
  ts_ms: number;
  kind: RunEventKind;
  /** On `tool_use`: lines added and removed by this call. See `RunLogLine`. */
  added?: number | null;
  removed?: number | null;
  session_id?: string | null;
  text?: string;
  tool?: string;
  summary?: string;
  request_id?: string;
  message?: string;
  cost_usd?: number | null;
  turns?: number | null;
  result?: string | null;
  allow?: boolean;
  /** On `user_queued` and `user_read`: which queued message this is about. */
  queue_id?: string;
  /** On a `commands` update: everything `/` can reach in this session. */
  commands?: SlashCommand[];
  /** On a `usage` update: the model that spent those tokens — the only record
   *  of what the turn actually ran on. The profile says what it *would* run on
   *  today, which stops being the same question the moment it is edited. */
  model?: string | null;
  /** On `turns`: how many model turns so far. Interim; the total lands on
   *  `done`. */
  count?: number;
  /** On `tool_use` and `tool_result`: the id the model minted for the call,
   *  and the parent call when it runs inside a subagent. What lets a result
   *  fold into the call that asked for it. */
  tool_use_id?: string | null;
  parent_tool_use_id?: string | null;
  /** On `tool_result`: whether it worked, and the full (capped) output. A
   *  failed Bash that reads as a clean one is the bug this pair closes. */
  ok?: boolean;
  detail?: string | null;
  /** On `done`: set when the run ended in an error rather than an answer. It
   *  arrives on the same message as a success, so without it a failed run
   *  reads as a completed one. */
  error?: string | null;
  /** On `usage`: what one turn spent. `input_tokens` is the prompt the model
   *  was handed *this* turn, so the last one is also how full its context is. */
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_creation_tokens?: number;
  /** On `background_tasks`: everything still running underneath the answer.
   *  Replace what you had — never merge. */
  tasks?: BackgroundTask[];
}

/** Work the agent left running under its reply: a backgrounded command, a
 *  subagent. Handwritten beside `RunUpdate` for the same reason `SlashCommand`
 *  is — it only ever arrives on one.
 *
 *  It exists because a turn that answers is not a turn that finished, and
 *  nothing on screen used to say so. */
export interface BackgroundTask {
  task_id: string;
  /** `shell`, `subagent`, … — whatever the engine calls it. */
  task_type: string;
  description: string;
}

/** One thing `/` can mean. Handwritten beside `RunUpdate` because it only ever
 *  arrives on one, and the composer reads it straight off that. */
export interface SlashCommand {
  /** Without the leading slash. */
  name: string;
  description: string;
  /** What comes after the name, when it takes anything. */
  argument_hint: string | null;
  /** Other names that land on this same command. */
  aliases: string[];
}

/** What `chat_queue` did.
 *
 *  Handwritten for the same reason as `Bootstrap`: it is a shell response, not
 *  a domain type. `queue_id` set means the message went into a turn that was
 *  already running and the model has not read it yet — the screen marks it so,
 *  and settles it on the `user_read` that names the same id. `null` means
 *  there was no turn to join, so it started one and is an ordinary message. */
export interface Queued {
  queue_id: string | null;
  conversation: Conversation;
}

/** What an attachment looks like, for the chip that stands for it. Written by
 *  hand rather than generated: `AttachmentPreview` is a shell response, like
 *  `Bootstrap` and the rest above it, not a domain type. */
export interface AttachmentPreview {
  path: string;
  name: string;
  ext: string;
  size: number;
  /** A data URI when the file is an image small enough to inline. */
  image: string | null;
  /** The opening of a text file, so a pasted patch is recognisable. */
  head: string | null;
}

export interface RunLogLine {
  ts_ms: number;
  kind: RunEventKind;
  /** On a `tool_use` line: how many lines the call adds and removes, when the
   *  call itself said so. Absent — never zero — for a tool that does not touch
   *  lines, so a group header can show no number instead of claiming nothing
   *  changed. */
  added?: number | null;
  removed?: number | null;

  text?: string;
  tool?: string;
  summary?: string;
  message?: string;
  session_id?: string | null;
  cost_usd?: number | null;
  turns?: number | null;
  request_id?: string;
  allow?: boolean;
  /** On `user_queued` and `user_read`: which queued message this is about. */
  queue_id?: string;
  /** On a `usage` line: the model that spent those tokens. It is the only
   *  record of what a run actually ran on — the agent profile says what it
   *  would run on *today*, which is a different question once the profile has
   *  been edited. Absent on every other kind, and on runs recorded before
   *  usage was written down. */
  model?: string | null;
  /** On `turns`: how many model turns so far. Interim; the total lands on
   *  `done`. */
  count?: number;
  /** On `tool_use` and `tool_result`: the id the model minted for the call,
   *  and the parent call when it runs inside a subagent. What lets a result
   *  fold into the call that asked for it. */
  tool_use_id?: string | null;
  parent_tool_use_id?: string | null;
  /** On `tool_result`: whether it worked, and the full (capped) output. A
   *  failed Bash that reads as a clean one is the bug this pair closes. */
  ok?: boolean;
  detail?: string | null;
  /** On `done`: set when the run ended in an error rather than an answer. It
   *  arrives on the same message as a success, so without it a failed run
   *  reads as a completed one. */
  error?: string | null;
  /** On `usage`: what one turn spent. `input_tokens` is the prompt the model
   *  was handed *this* turn, so the last one is also how full its context is. */
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_creation_tokens?: number;
}

// ---- shell wrappers that still live in src-tauri ---------------------------

/** A card's checks were run again, on the project they belong to. */
export interface CardChecksEvent {
  project_id: string;
  checks: CardChecks;
}

/** What a folder looks like before adopting it — composed client-side. */
export interface ProjectView extends Project {
  exists: boolean;
  stats: ProjectStats;
}

/** Where the Director asked the window to go. */
export interface Navigation {
  screen: string;
  card_id: string | null;
  why: string | null;
}

export interface ClaudeStatus {
  cli_found: boolean;
  cli_version: string | null;
  logged_in: boolean;
  credentials_path: string | null;
}

/** Mirrors `commands::codex::CodexStatus`. Hand-written like `ClaudeStatus`
 *  beside it — neither crosses the ts-rs boundary, because both are answers the
 *  shell composes rather than state the engine holds. */
export interface CodexStatus {
  cli_found: boolean;
  cli_version: string | null;
  logged_in: boolean;
  /** `chatgpt` for a subscription, `apikey` for a key. Only the first has a
   *  plan window to show. The plan's *name* comes from `codexPlanUsage`, which
   *  asks the provider rather than decoding a token here. */
  auth_mode: string | null;
}

/** Mirrors `harness_agent_codex::PlanUsage`. Percentages of two rolling
 *  windows, which is what a subscription has instead of a bill. */
export interface PlanUsage {
  plan: string;
  primary_percent: number;
  primary_resets_at: number | null;
  primary_window_mins: number | null;
  secondary_percent: number;
  secondary_resets_at: number | null;
  secondary_window_mins: number | null;
  reached: string | null;
}

export interface SidecarStatus {
  dir: string;
  script: string;
  script_found: boolean;
  ready: boolean;
  node_found: boolean;
  node_version: string | null;
  development: boolean;
}

export interface SystemStatus {
  claude: ClaudeStatus;
  sidecar: SidecarStatus;
  ready: boolean;
  blocker: string | null;
}

export interface Bootstrap {
  settings: Settings;
  agents: AgentProfile[];
  projects: Project[];
  status: SystemStatus;
  approvals: PendingApproval[];
  /** What `/` can reach, as the last session described it. The event that
   *  publishes it is ephemeral, so without this the menu is empty after every
   *  restart — which is exactly when nothing has yet happened to refill it. */
  commands: SlashCommand[];
  inbox: Proposal[];
  data_dir: string;
  conversations: Conversation[];
  /** The chat to reopen, so the window comes back where it was left. */
  last_conversation: string | null;
  /** Unscoped shell allowances from an older build. They authorise nothing. */
  revoked_allowances: string[];
  /** The last warning that Relay's own source moved without a card behind it,
   *  or null. Here because the `mirror://outside-work` event is emitted from
   *  `setup()`, before this window exists: a warning that arrived while nobody
   *  was listening — or before a reload — is only recoverable through this. */
  outside_work: MirrorWarning | null;
}

/** One of the two browsers an agent can be given. Handwritten beside
 *  `Bootstrap`: a shell response, not a domain type. */
export interface BrowserOffer {
  id: string;
  name: string;
  /** What granting it costs, in the backend's own words. */
  note: string;
}

/** One of the skills Relay ships. A skill is prose that enters an agent's
 *  prompt: it grants no reach on its own, which is what `note` says out loud. */
export interface SkillOffer {
  id: string;
  name: string;
  description: string;
  note: string;
}

export interface CreatedCard {
  card_id: string;
  run_id: string | null;
}

/** What a card actually changed against the project's base branch, read from
 *  its worktree: the facts the review screen states, and the patch it shows. */
export interface CardDiff {
  card_id: string;
  base: string;
  branch: string | null;
  worktree: string | null;
  session_id: string | null;
  files: string[];
  added: number;
  removed: number;
  patch: string;
}

export interface ProjectDetail {
  project: Project;
  head: string | null;
  default_branch: string;
  /** `origin`, when there is one. A local-only project has none. */
  remote: string | null;
  commit_count: number;
  line_count: number;
  branches: BranchRow[];
  languages: LanguageRow[];
  commits: CommitRow[];
  week_commits: number[];
  week_lines: number;
  worktrees: WorktreeRow[];
  checks: CheckRow[];
}

/** One card in the Review queue, ordered by the Triador. */
export interface QueueRow {
  card_id: string;
  title: string;
  risk: number;
  reasons: string[];
}

/** A build waiting for the operator's decision. */
export interface PendingUpdate {
  card_id: string;
  commit_sha: string;
  built_at_ms: number;
  binary: string;
  /** `card` when an agent's run produced it, `build` when it is a newer binary
   *  the operator compiled themselves. */
  kind: "card" | "build";
}

/** Why the window is being held on close, sent before the waiting starts. */
export interface ClosingBegan {
  /** The Director's end-of-day look is due. */
  look: boolean;
  /** Running agents are being asked to commit their work in progress. */
  wip: boolean;
  /** Seconds after which the window closes regardless. */
  limit_secs: number;
}

/** Where the close sequence got to. */
export interface ClosingPhase {
  phase: "look" | "wip" | "skipped" | "timeout" | "done";
  detail: string;
}

// ---- constants and helpers -------------------------------------------------

/** Column order and the words the UI uses for each status. */
// STATUS_ORDER and STATUS_NAME are the backend's vocabulary now: see
// generated/vocabulary.ts, written by crates/app/src/vocabulary.rs.
export const STATUS_ORDER: Status[] = STATUSES.map((s) => s.id as Status);

/** Which column a card may move to, from where. The backend's own
 *  `Status::LEGAL_MOVES`, generated rather than typed out here: the board
 *  offers exactly the moves the engine accepts, and stops holding a second
 *  copy of the state machine that can quietly drift from it. */
export const LEGAL_MOVES = GENERATED_LEGAL_MOVES as Record<Status, Status[]>;

export const STATUS_NAME: Record<Status, string> = Object.fromEntries(
  STATUSES.map((s) => [s.id, s.name]),
) as Record<Status, string>;

/** Um tom é um conjunto de classes, não uma cor.
 *
 *  Enquanto os tokens eram custom properties, um tom podia ser a string
 *  `var(--ok)` e ir parar a um `style`. Com o Tailwind o nome da classe tem de
 *  estar escrito em código para ser gerado, e cada cor traz o seu par
 *  `dark:` — daí quatro campos por tom em vez de dois. */
export interface Tone {
  /** Texto e glifos. */
  fg: string;
  /** O fundo lavado por baixo desse texto. */
  soft: string;
  /** O preenchimento cheio: pontos, barras, medidores. */
  solid: string;
  /** A linha ténue. */
  line: string;
  /** A linha à força toda, na própria cor do tom. */
  edge: string;
  /** O lavado como princípio de um degradê que se apaga. */
  wash: string;
}

export type ToneName = "neutral" | "accent" | "info" | "ok" | "warn" | "bad";

export const TONE: Record<ToneName, Tone> = {
  neutral: {
    fg: "text-text3 dark:text-text3-d",
    soft: "bg-surface2 dark:bg-surface2-d",
    solid: "bg-text3 dark:bg-text3-d",
    line: "border-line3 dark:border-line3-d",
    edge: "border-text3 dark:border-text3-d",
    wash: "from-surface2 dark:from-surface2-d",
  },
  accent: {
    fg: "text-accent dark:text-accent-d",
    soft: "bg-accentSoft dark:bg-accentSoft-d",
    solid: "bg-accent dark:bg-accent-d",
    line: "border-accentLine dark:border-accentLine-d",
    edge: "border-accent dark:border-accent-d",
    wash: "from-accentSoft dark:from-accentSoft-d",
  },
  info: {
    fg: "text-info dark:text-info-d",
    soft: "bg-infoSoft dark:bg-infoSoft-d",
    solid: "bg-info dark:bg-info-d",
    line: "border-info dark:border-info-d",
    edge: "border-info dark:border-info-d",
    wash: "from-infoSoft dark:from-infoSoft-d",
  },
  ok: {
    fg: "text-ok dark:text-ok-d",
    soft: "bg-okSoft dark:bg-okSoft-d",
    solid: "bg-ok dark:bg-ok-d",
    line: "border-ok dark:border-ok-d",
    edge: "border-ok dark:border-ok-d",
    wash: "from-okSoft dark:from-okSoft-d",
  },
  warn: {
    fg: "text-warn dark:text-warn-d",
    soft: "bg-warnSoft dark:bg-warnSoft-d",
    solid: "bg-warn dark:bg-warn-d",
    line: "border-warn dark:border-warn-d",
    edge: "border-warn dark:border-warn-d",
    wash: "from-warnSoft dark:from-warnSoft-d",
  },
  bad: {
    fg: "text-bad dark:text-bad-d",
    soft: "bg-badSoft dark:bg-badSoft-d",
    solid: "bg-bad dark:bg-bad-d",
    line: "border-bad dark:border-bad-d",
    edge: "border-bad dark:border-bad-d",
    wash: "from-badSoft dark:from-badSoft-d",
  },
};

/** O tom de cada coluna, para a cor viver num sítio só. */
export const STATUS_TONE: Record<Status, Tone> = {
  backlog: TONE.neutral,
  ready: TONE.info,
  running: TONE.accent,
  review: TONE.warn,
  done: TONE.ok,
};

/** How a standing allowance reads, matching the backend label. */
export function ruleLabel(rule: AllowRule): string {
  return rule.command ? `${rule.tool}(${rule.command} \u2026)` : rule.tool;
}

/** A shell rule with no command scope authorises nothing: it is left in the
 *  list only so it can be seen and removed. */
export function ruleIsRevoked(rule: AllowRule): boolean {
  const head = rule.tool.toLowerCase().split("(")[0].trim();
  // A lista vem do `allow.rs`, não é escrita outra vez aqui: quem revoga a
  // regra é que sabe quais são as shells.
  return !rule.command && SHELL_TOOLS.includes(head);
}

export function tone(name: string | undefined): Tone {
  return TONE[(name ?? "accent") as ToneName] ?? TONE.accent;
}

/** O nome do tom, quando é o nome que viaja (num toast, numa acção da paleta)
 *  em vez do conjunto de classes. */
export function toneName(name: string | undefined): ToneName {
  return name && name in TONE ? (name as ToneName) : "accent";
}






/** Os apelidos que o login da Claude aceita. Não são modelos: são "seja qual
 *  for o Opus de hoje", e é por isso que continuam a valer a pena — um perfil
 *  posto em `opus` sobe de versão sozinho. Mas também é por isso que dizer só
 *  "opus" num ecrã não diz nada: há muitos Opus, e este não escolhe nenhum. */
export const MODEL_ALIASES: Record<string, string> = {
  opus: "Opus",
  sonnet: "Sonnet",
  haiku: "Haiku",
};

/** Como se escreve, num ecrã, o modelo que está configurado.
 *
 *  A regra é uma só: **o rótulo é literalmente o que lá está**. Um perfil posto
 *  em `opus` mostra `opus`; um posto em `claude-opus-4-8` mostra
 *  `claude-opus-4-8`; um que não tem nada mostra um travessão, como qualquer
 *  outro valor que o backend não deu.
 *
 *  Nada de palavras inventadas para o estado — nem "default", nem "latest".
 *  Não dizem qual é o modelo, que é a única coisa que a pergunta faz; obrigam a
 *  saber o que significam; e "latest" era duplamente falso, porque quem escolhe
 *  qual dos Opus é o login da Claude no momento do run, e não este ecrã.
 *
 *  O que aquilo *quer dizer* vai no `title`, que é onde uma explicação cabe sem
 *  tomar o lugar do facto.
 */
export function modelLabel(model: string | null | undefined): {
  /** O que a pastilha mostra: o valor tal e qual, ou um travessão. */
  label: string;
  /** O que o `title` explica. Uma frase. */
  hint: string;
  /** Um apelido — não escolhe versão nenhuma; o run é que a decide. */
  isAlias: boolean;
} {
  const id = (model ?? "").trim();
  if (!id) {
    return {
      label: "—",
      hint: "No model set on this profile: the Claude login picks one when a run starts.",
      isAlias: false,
    };
  }
  const alias = MODEL_ALIASES[id.toLowerCase()];
  if (alias) {
    return {
      label: id,
      hint: `A family, not a version: which ${alias} runs is decided by the Claude login when the run starts.`,
      isAlias: true,
    };
  }
  return {
    label: id,
    hint: `Every run on this profile uses ${id}, until you change it.`,
    isAlias: false,
  };
}

// The backend's own vocabulary, passed through so a screen imports every
// list from one place whether it is generated or drawn here.
export { ALL_PERMISSIONS, MODELS, REVIEWERS, SHELL_TOOLS, STATUSES, WORKTREE_MODES };
export type { Choice };
