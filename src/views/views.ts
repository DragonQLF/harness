/** The set of screens, shared by the nav, the router and the palette. */
export type View =
  | "chat"
  | "review"
  | "board"
  | "sessions"
  | "agents"
  | "code"
  | "activity"
  | "trees"
  | "projects"
  | "settings";

export const VIEW_TITLES: Record<View, string> = {
  chat: "Chat",
  review: "Review",
  board: "Board",
  sessions: "Sessions",
  agents: "Agents",
  code: "Code",
  activity: "Activity",
  trees: "Worktrees",
  projects: "Projects",
  settings: "Settings",
};
