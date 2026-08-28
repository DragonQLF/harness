/** Mirrors of the Rust types crossing the IPC boundary.
 *  Field names are the serde names: snake_case in, camelCase out.
 *
 *  Everything under `generated/` is produced from the Rust structs by ts-rs —
 *  run `pnpm codegen` (cargo test --workspace --test export_types) after
 *  changing a Rust type, and never edit those files by hand.
 *
 *  What stays handwritten here is the surface ts-rs cannot mirror faithfully:
 *  - Envelope / RunUpdate / RunEventKind / RunLogLine: flattened event unions
 *    where the UI reads loose fields;
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
import type { CheckRow } from "./generated/CheckRow";
import type { CommitRow } from "./generated/CommitRow";
import type { Conversation } from "./generated/Conversation";
import type { FolderInfo } from "./generated/FolderInfo";
import type { LanguageRow } from "./generated/LanguageRow";
import type { MirrorWarning } from "./generated/MirrorWarning";
import type { OutsideWork } from "./generated/OutsideWork";
import type { PendingApproval } from "./generated/PendingApproval";
import type { Proposal } from "./generated/Proposal";
import type { Project } from "./generated/Project";
import type { Provider } from "./generated/Provider";
import type { ProjectStats } from "./generated/ProjectStats";
import type { Review } from "./generated/Review";
import type { Reviewer } from "./generated/Reviewer";
import type { RunId } from "./generated/RunId";
import type { RunOutcome } from "./generated/RunOutcome";
import type { SessionView } from "./generated/SessionView";
import type { Settings } from "./generated/Settings";
import type { Snapshot } from "./generated/Snapshot";
import type { CatalogModel } from "./generated/CatalogModel";
import type { Status } from "./generated/Status";
import {
  ALL_PERMISSIONS,
  MODELS,
  REVIEWERS,
  STATUSES,
  WORKTREE_MODES,
  type Choice,
} from "./generated/vocabulary";
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
  CardId,
  CheckRow,
  CommitRow,
  Conversation,
  FolderInfo,
  LanguageRow,
  MirrorWarning,
  OutsideWork,
  PendingApproval,
  Proposal,
  Project,
  Provider,
  ProjectStats,
  Review,
  Reviewer,
  RunId,
  RunOutcome,
  SessionView,
  Settings,
  Snapshot,
  Status,
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
  | "text"
  | "delta"
  | "thinking"
  | "tool_use"
  | "tool_result"
  | "turns"
  | "done"
  | "failed"
  | "approval_requested"
  | "approval_answered"
  | "notice";

export interface RunUpdate {
  project_id: string;
  card_id: string;
  run_id: string;
  ts_ms: number;
  kind: RunEventKind;
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
}

export interface RunLogLine {
  ts_ms: number;
  kind: RunEventKind;
  text?: string;
  tool?: string;
  summary?: string;
  message?: string;
  session_id?: string | null;
  cost_usd?: number | null;
  turns?: number | null;
  request_id?: string;
  allow?: boolean;
}

// ---- shell wrappers that still live in src-tauri ---------------------------

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
  return !rule.command && ["bash", "shell", "sh", "powershell"].includes(head);
}

export function tone(name: string | undefined): Tone {
  return TONE[(name ?? "accent") as ToneName] ?? TONE.accent;
}

/** O nome do tom, quando é o nome que viaja (num toast, numa acção da paleta)
 *  em vez do conjunto de classes. */
export function toneName(name: string | undefined): ToneName {
  return name && name in TONE ? (name as ToneName) : "accent";
}






// The backend's own vocabulary, passed through so a screen imports every
// list from one place whether it is generated or drawn here.
export { ALL_PERMISSIONS, MODELS, REVIEWERS, STATUSES, WORKTREE_MODES };
export type { Choice };
