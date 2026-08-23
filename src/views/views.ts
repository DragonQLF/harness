/** The set of screens, shared by the nav, the router and the palette. */
export type View =
  | "home"
  | "director"
  | "projects"
  | "project"
  | "agents"
  | "agent"
  | "board"
  | "runs"
  | "trees"
  | "log"
  | "settings";

export const VIEW_TITLES: Record<View, string> = {
  home: "Overview",
  director: "Director",
  projects: "Projects",
  project: "Project",
  agents: "Agents",
  agent: "Agent",
  board: "Work",
  runs: "Sessions",
  trees: "Worktrees",
  log: "Activity",
  settings: "Settings",
};
