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
import type { PendingApproval } from "./generated/PendingApproval";
import type { Project } from "./generated/Project";
import type { ProjectStats } from "./generated/ProjectStats";
import type { Review } from "./generated/Review";
import type { Reviewer } from "./generated/Reviewer";
import type { RunId } from "./generated/RunId";
import type { RunOutcome } from "./generated/RunOutcome";
import type { SessionView } from "./generated/SessionView";
import type { Settings } from "./generated/Settings";
import type { Snapshot } from "./generated/Snapshot";
import type { Status } from "./generated/Status";
import type { WorktreeMode } from "./generated/WorktreeMode";
import type { WorktreeRow } from "./generated/WorktreeRow";

export type {
  Actor,
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
  PendingApproval,
  Project,
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
  data_dir: string;
  conversations: Conversation[];
  /** The chat to reopen, so the window comes back where it was left. */
  last_conversation: string | null;
  /** Unscoped shell allowances from an older build. They authorise nothing. */
  revoked_allowances: string[];
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

/** A mirror build waiting for the operator's decision. */
export interface PendingUpdate {
  card_id: string;
  commit_sha: string;
  built_at_ms: number;
  binary: string;
}

// ---- constants and helpers -------------------------------------------------

/** Column order and the words the UI uses for each status. */
export const STATUS_ORDER: Status[] = ["backlog", "ready", "running", "review", "done"];

export const STATUS_NAME: Record<Status, string> = {
  backlog: "Later",
  ready: "Ready",
  running: "Working",
  review: "Review",
  done: "Done",
};

/** CSS variable pairs per status, so colour lives in one place. */
export const STATUS_TONE: Record<Status, { color: string; soft: string }> = {
  backlog: { color: "var(--text3)", soft: "var(--surface2)" },
  ready: { color: "var(--info)", soft: "var(--infoSoft)" },
  running: { color: "var(--accent)", soft: "var(--accentSoft)" },
  review: { color: "var(--warn)", soft: "var(--warnSoft)" },
  done: { color: "var(--ok)", soft: "var(--okSoft)" },
};

export const TONE: Record<string, { color: string; soft: string }> = {
  accent: { color: "var(--accent)", soft: "var(--accentSoft)" },
  info: { color: "var(--info)", soft: "var(--infoSoft)" },
  ok: { color: "var(--ok)", soft: "var(--okSoft)" },
  warn: { color: "var(--warn)", soft: "var(--warnSoft)" },
  bad: { color: "var(--bad)", soft: "var(--badSoft)" },
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

export function tone(name: string | undefined) {
  return TONE[name ?? "accent"] ?? TONE.accent;
}

export const MODELS = [
  { id: "opus", name: "Opus", hint: "Deepest reasoning, highest cost" },
  { id: "sonnet", name: "Sonnet", hint: "The everyday worker" },
  { id: "haiku", name: "Haiku", hint: "Fast and cheap, for lookups" },
];

export const ALL_PERMISSIONS = ["Read", "Search", "Edit", "Write", "Git", "Web", "Shell"];

export const WORKTREE_MODES: { id: WorktreeMode; name: string; hint: string }[] = [
  { id: "per_card", name: "Per card", hint: "A fresh branch and checkout for every card" },
  { id: "shared", name: "Shared", hint: "One long-lived branch for the project" },
  { id: "none", name: "None", hint: "Reads the main checkout, never writes" },
];

export const REVIEWERS: { id: Reviewer; name: string; hint: string }[] = [
  {
    id: "director",
    name: "Director",
    hint: "The Director reads the diff first and only sends you what passes.",
  },
  { id: "human", name: "You", hint: "Every finished run lands in your review queue." },
  { id: "nobody", name: "Nobody", hint: "Finished runs go straight to Done." },
];
