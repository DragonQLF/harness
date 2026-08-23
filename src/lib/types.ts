/** Mirrors of the Rust types crossing the IPC boundary.
 *  Field names are the serde names: snake_case in, camelCase out. */

export type Status = "backlog" | "ready" | "running" | "review" | "done";
export type Actor = "human" | "director";
export type WorktreeMode = "per_card" | "shared" | "none";
export type Reviewer = "director" | "human" | "nobody";

export interface Review {
  by: Actor;
  approved: boolean;
  reason: string;
}

export interface Card {
  id: string;
  title: string;
  status: Status;
  current_run: string | null;
  agent_id: string;
  cost_usd: number;
  turns: number;
  runs: number;
  last_review: Review | null;
}

export interface SessionView {
  card_id: string;
  run_id: string | null;
  worktree: string;
  branch: string | null;
  session_id: string | null;
  agent_id: string;
  started_ms: number;
  live: boolean;
}

export interface Snapshot {
  project_id: string;
  last_seq: number;
  cards: Card[];
  sessions: SessionView[];
}

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
  | "text"
  | "delta"
  | "thinking"
  | "tool_use"
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

export interface ActiveRun {
  card_id: string;
  run_id: string;
  agent_id: string;
  worktree: string;
  started_ms: number;
}

export interface AgentProfile {
  id: string;
  name: string;
  initial: string;
  title: string;
  role: string;
  brief: string;
  tone: string;
  model: string | null;
  permissions: string[];
  budget_usd: number | null;
  worktree: WorktreeMode;
  reviewer: Reviewer;
  paused: boolean;
  permission_mode: string | null;
}

export interface AgentStats {
  agent_id: string;
  runs: number;
  cards: number;
  cards_done: number;
  spend: number;
  avg_cost: number;
  turns: number;
  reviews: number;
  sent_back: number;
  week_runs: number[];
  lines_added: number;
  lines_removed: number;
  commits: number;
}

export interface Settings {
  theme: string;
  accent: string;
  sidecar: boolean;
  director_reviews_first: boolean;
  commit_wip_on_close: boolean;
  permission_mode: string;
  daily_budget_usd: number;
  always_allow: string[];
  last_project: string | null;
  user_name: string;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  glyph: string;
  tone: string;
  base_branch: string;
  added_ms: number;
  paused: boolean;
}

export interface ProjectStats {
  cards: number;
  backlog: number;
  ready: number;
  running: number;
  review: number;
  done: number;
  runs_total: number;
  runs_today: number;
  spend_total: number;
  spend_today: number;
  done_today: number;
  cost_per_card: number;
  week_runs: number[];
  last_event_ms: number;
}

/** What a folder looks like before adopting it. */
export interface FolderInfo {
  path: string;
  exists: boolean;
  is_repo: boolean;
  empty: boolean;
  name: string;
  already_added: boolean;
  next: "open" | "init" | "confirm_init" | "missing";
}

export interface ProjectView extends Project {
  exists: boolean;
  stats: ProjectStats;
}

export interface BranchRow {
  name: string;
  when: string;
  sha: string;
  state: "default" | "live" | "merged" | "open";
}

export interface CommitRow {
  sha: string;
  short: string;
  subject: string;
  author: string;
  when: string;
  at_secs: number;
  parents: string[];
  refs: string;
  card: string | null;
  agent: string | null;
  added: number;
  removed: number;
  files: number;
  on_default: boolean;
}

export interface LanguageRow {
  name: string;
  bytes: number;
  pct: number;
}

export interface WorktreeRow {
  path: string;
  head: string;
  branch: string | null;
  bare: boolean;
  dirty: boolean;
}

export interface CheckRow {
  name: string;
  command: string;
  status: "ok" | "warn" | "fail" | "idle";
  detail: string;
  ran_ms: number;
  duration_ms: number;
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

export interface ActivityRow {
  seq: number;
  ts_ms: number;
  kind: "card" | "run" | "review" | "approval";
  label: string;
  card_id: string;
  detail: string;
}

/** Where the Director asked the window to go. */
export interface Navigation {
  screen: string;
  card_id: string | null;
  why: string | null;
}

export interface PendingApproval {
  request_id: string;
  project_id: string;
  card_id: string | null;
  tool: string;
  summary: string;
  asked_ms: number;
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
}

export interface CreatedCard {
  card_id: string;
  run_id: string | null;
}

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
