/** The set of screens, shared by the nav, the router and the palette.
 *
 *  The six in the title bar are `home`, `chat`, `board`, `code`, `sessions`
 *  and `agents`. The rest are reached from the sidebar — RECORDS and SYSTEM —
 *  or from the palette; they are screens, not nav slots.
 *
 *  `agents` was the sixth slot the design reserved and it stayed out of the
 *  bar "until its design lands". It landed: the screen edits a whole profile —
 *  model, endpoint, tools, skills, MCP servers, who it reports to — and it was
 *  reachable only through the palette, or by clicking a crew member that
 *  happened to have chat turned off. A screen that is the only place a setting
 *  exists is not a screen you find by accident. */
export type View =
  | "home"
  | "chat"
  | "board"
  | "code"
  | "sessions"
  | "review"
  | "agents"
  | "activity"
  | "trees"
  | "projects"
  | "settings";

/** The six that get a slot in the centred nav, in order. */
export const NAV_VIEWS = ["home", "chat", "board", "code", "sessions", "agents"] as const;

export const VIEW_TITLES: Record<View, string> = {
  home: "Home",
  chat: "Chat",
  board: "Board",
  code: "Code",
  sessions: "Sessions",
  review: "Review",
  agents: "Agents",
  activity: "Activity",
  trees: "Worktrees",
  projects: "Projects",
  settings: "Settings",
};
