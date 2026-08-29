/** The set of screens, shared by the nav, the router and the palette.
 *
 *  The five in the title bar are `home`, `chat`, `board`, `code`, `sessions`.
 *  The rest are reached from the sidebar — RECORDS and SYSTEM — or from the
 *  palette; they are screens, not nav slots. `agents` is the sixth nav slot
 *  the design reserves, and stays in the sidebar's CREW section until its
 *  design lands. */
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

/** The five that get a slot in the centred nav, in order. */
export const NAV_VIEWS = ["home", "chat", "board", "code", "sessions"] as const;

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
