import { useEffect, useRef, useState } from "react";
import {
  ChevronsUpDown,
  FileText,
  Folder,
  Gauge,
  List,
  Plus,
  SlidersHorizontal,
  UserRound,
  Waypoints,
} from "lucide-react";
import { cx } from "../lib/cx";
import { initials, shortAgo } from "../lib/format";
import { useStore } from "../state/store";
import type { View } from "../views/views";

/** A section label: 10.5px, tracked out, over a count in its own chip. */
function Section({ label, count }: { label: string; count?: number }) {
  return (
    <div className="flex items-center gap-2 px-2 pb-2 pt-4.5">
      <span className="text-xs font-semibold tracking-[.08em] text-faint dark:text-faint-d">
        {label}
      </span>
      {count != null && count > 0 && (
        <span className="rounded-sm bg-active px-1.5 py-px text-xs font-semibold text-muted dark:bg-active-d dark:text-muted-d">
          {count}
        </span>
      )}
    </div>
  );
}

const ICON = { size: 16, strokeWidth: 2.4, "aria-hidden": true } as const;
const AGENT_ICON = { size: 15, strokeWidth: 2.4, "aria-hidden": true } as const;

/** One row of the sidebar. The whole row is the target. */
function Row({
  label,
  icon,
  on,
  onClick,
  right,
}: {
  label: string;
  icon: React.ReactNode;
  on?: boolean;
  onClick: () => void;
  right?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-current={on ? "page" : undefined}
      onClick={onClick}
      className={cx(
        "flex w-full cursor-pointer items-center gap-2.25 rounded-sm border-none px-2.5 py-1.75 text-left text-md transition-colors duration-150",
        on
          ? "bg-primarySoft font-semibold text-ink2 dark:bg-primarySoft-d dark:text-ink2-d"
          : "bg-transparent font-medium text-ink2 hover:bg-hovered dark:text-ink2-d dark:hover:bg-hovered-d",
      )}
    >
      <span
        className={cx(
          "flex flex-none",
          on ? "text-primary dark:text-primary-d" : "text-faint dark:text-faint-d",
        )}
      >
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {right}
    </button>
  );
}

/** The status pill on a crew row. Four states, and each is a fact:
 *  *working* is an active run, *blocked on you* is a pending approval,
 *  *watching* is the Director with the engine up, *idle Nm* is when it last
 *  finished something. Nothing here is decorative. */
function CrewState({ state }: { state: { label: string; kind: string } }) {
  const skin =
    state.kind === "working"
      ? "bg-primarySoft text-primary dark:bg-primarySoft-d dark:text-primary-d"
      : state.kind === "blocked"
        ? "bg-warnSoft text-warn dark:bg-warnSoft-d dark:text-warn-d"
        : state.kind === "watching"
          ? "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d"
          : "bg-active text-muted dark:bg-active-d dark:text-muted-d";
  return (
    <span className={cx("flex-none rounded-full px-2.25 py-0.5 text-xs font-bold", skin)}>
      {state.label}
    </span>
  );
}

/** The 258px sidebar: which repository, who is on it, what is recorded, and
 *  the operator at the bottom.
 *
 *  Order follows `docs/design/Relay.dc.html` — picker, RECORDS, CREW, SYSTEM —
 *  rather than the README's prose summary, which lists CREW first and leaves
 *  SYSTEM out. The file is the design; the prose is a description of it. */
export function Sidebar({
  view,
  go,
  openAgent,
}: {
  view: View;
  go: (v: View) => void;
  openAgent: (id: string) => void;
}) {
  const {
    project,
    projects,
    projectId,
    selectProject,
    addProject,
    agents,
    agentStats,
    snapshot,
    approvals,
    activity,
    worktrees,
    settings,
    status,
  } = useStore();

  const [picking, setPicking] = useState(false);
  const box = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!picking) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setPicking(false);
    };
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [picking]);

  const cards = snapshot?.cards ?? [];
  const name = settings?.user_name ?? "Operator";

  /** Which agents are mid-run, and which are held up on an answer from here.
   *  Both come off the same snapshot the rest of the screen reads. */
  const runningBy = new Set(
    cards.filter((c) => c.status === "running").map((c) => c.agent_id),
  );
  const blockedBy = new Set(
    approvals
      .map((a) => cards.find((c) => c.id === a.card_id)?.agent_id)
      .filter((id): id is string => Boolean(id)),
  );

  const crewState = (agentId: string): { label: string; kind: string } => {
    if (blockedBy.has(agentId)) return { label: "blocked on you", kind: "blocked" };
    if (runningBy.has(agentId)) return { label: "working", kind: "working" };
    if (agentId === "director" && status?.ready) return { label: "watching", kind: "watching" };
    // Nothing running and nothing waiting: say when it last did something,
    // taken from the newest activity row that names one of its cards.
    const own = new Set(cards.filter((c) => c.agent_id === agentId).map((c) => c.id));
    const last = activity.find((r) => own.has(r.card_id));
    if (last) return { label: `idle ${shortAgo(last.ts_ms)}`, kind: "idle" };
    return { label: agentStats[agentId]?.runs ? "idle" : "no runs", kind: "idle" };
  };

  return (
    <nav className="flex w-[258px] flex-none flex-col overflow-hidden border-r border-line bg-surface px-3 py-3.5 dark:border-line-d dark:bg-surface-d">
      <div ref={box} className="relative flex flex-none items-center gap-2">
        <button
          type="button"
          onClick={() => setPicking((v) => !v)}
          aria-expanded={picking}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.25 rounded-9px border border-line bg-transparent px-2.5 py-1.75 text-left transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:hover:bg-hovered-d"
        >
          <span className="grid h-5 w-5 flex-none place-items-center rounded-6px bg-ink text-2xs font-bold text-white dark:bg-ink-d dark:text-canvas-d">
            {project ? initials(project.name) : "—"}
          </span>
          <span className="min-w-0 flex-1 truncate text-md font-semibold text-ink dark:text-ink-d">
            {project?.name ?? "No project"}
          </span>
          <ChevronsUpDown size={13} strokeWidth={2.4} className="flex-none text-faint" aria-hidden />
        </button>
        <button
          type="button"
          title="Add a repository"
          aria-label="Add a repository"
          onClick={addProject}
          className="grid h-7 w-7 flex-none cursor-pointer place-items-center rounded-full border-none bg-primary text-white transition-colors duration-150 hover:bg-primaryDeep dark:bg-primary-d"
        >
          <Plus size={14} strokeWidth={3} aria-hidden />
        </button>

        {picking && (
          <div className="absolute left-0 top-full z-[200] mt-1.5 w-full animate-popIn rounded-md border border-line bg-surface p-1.5 shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d">
            {projects.length === 0 && (
              <div className="px-2.5 py-2 text-sm text-faint dark:text-faint-d">
                No repositories yet. Add one with +.
              </div>
            )}
            {projects.map((p) => (
              <button
                key={p.id}
                type="button"
                onClick={() => {
                  selectProject(p.id);
                  setPicking(false);
                }}
                className={cx(
                  "flex w-full cursor-pointer items-center gap-2.25 rounded-sm border-none px-2.5 py-1.75 text-left text-md transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
                  p.id === projectId
                    ? "bg-primarySoft font-semibold text-ink dark:bg-primarySoft-d dark:text-ink-d"
                    : "bg-transparent font-medium text-ink2 dark:text-ink2-d",
                )}
              >
                <span className="grid h-5 w-5 flex-none place-items-center rounded-6px bg-ink text-2xs font-bold text-white dark:bg-ink-d dark:text-canvas-d">
                  {initials(p.name)}
                </span>
                <span className="min-w-0 flex-1 truncate">{p.name}</span>
                {!p.exists && (
                  <span className="flex-none font-mono text-xs text-bad dark:text-bad-d">
                    missing
                  </span>
                )}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <Section label="RECORDS" count={3} />
        <Row
          label="Usage"
          icon={<Gauge {...ICON} />}
          on={view === "home"}
          onClick={() => go("home")}
        />
        <Row
          label="Activity"
          icon={<List {...ICON} />}
          on={view === "activity"}
          onClick={() => go("activity")}
          right={
            activity.length > 0 ? (
              <span className="flex-none rounded-9px bg-primary px-1.75 py-px text-xs font-semibold text-white dark:bg-primary-d">
                {activity.length}
              </span>
            ) : undefined
          }
        />
        <Row
          label="Worktrees"
          icon={<Waypoints {...ICON} />}
          on={view === "trees"}
          onClick={() => go("trees")}
          right={
            worktrees.length > 0 ? (
              <span className="flex-none font-mono text-11 text-faint dark:text-faint-d">
                {worktrees.length}
              </span>
            ) : undefined
          }
        />

        <Section label="CREW" count={agents.length} />
        {agents.length === 0 && (
          <div className="px-2.5 pb-1 text-sm leading-relaxed text-faint dark:text-faint-d">
            No profiles yet. The crew is who can be given a card.
          </div>
        )}
        {/* Uma linha da tripulação abre o perfil dessa pessoa, não uma conversa.
            Abria: `openChat()` sem argumento abre a conversa que já estava no
            ecrã — clicar no Scout dava a última conversa do Director —, e para
            quem tinha o chat desligado caía no ecrã de Agentes sem escolher
            ninguém. A tripulação é a lista de *quem existe e o que pode fazer*;
            falar com alguém é o separador Chat, e o perfil tem lá o botão. */}
        {agents.map((a) => (
          <Row
            key={a.id}
            label={a.name}
            icon={<UserRound {...AGENT_ICON} />}
            on={false}
            onClick={() => openAgent(a.id)}
            right={<CrewState state={crewState(a.id)} />}
          />
        ))}
      </div>

      <div className="flex-none">
        <Section label="SYSTEM" />
        <Row
          label="Projects"
          icon={<Folder {...ICON} />}
          on={view === "projects"}
          onClick={() => go("projects")}
          right={
            <span className="flex-none font-mono text-11 text-faint dark:text-faint-d">
              {projects.length}
            </span>
          }
        />
        <Row
          label="Review"
          icon={<FileText {...ICON} />}
          on={view === "review"}
          onClick={() => go("review")}
          right={
            cards.filter((c) => c.status === "review").length > 0 ? (
              <span className="flex-none rounded-full bg-warnSoft px-2.25 py-0.5 text-xs font-bold text-warn dark:bg-warnSoft-d dark:text-warn-d">
                {cards.filter((c) => c.status === "review").length}
              </span>
            ) : undefined
          }
        />
        <Row
          label="Settings"
          icon={<SlidersHorizontal {...ICON} />}
          on={view === "settings"}
          onClick={() => go("settings")}
        />

        <button
          type="button"
          onClick={() => go("settings")}
          className="mt-2.5 flex w-full cursor-pointer items-center gap-2.5 rounded-10px border border-line bg-transparent px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:hover:bg-hovered-d"
        >
          <span className="grid h-7 w-7 flex-none place-items-center rounded-full bg-warnSoft text-xs font-semibold text-warn dark:bg-warnSoft-d dark:text-warn-d">
            {initials(name)}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-body font-semibold text-ink dark:text-ink-d">
              {name}
            </span>
            <span className="block truncate text-xs text-faint dark:text-faint-d">
              Owner · {status?.ready ? "local" : "engine down"}
            </span>
          </span>
          <ChevronsUpDown size={13} strokeWidth={2.4} className="flex-none text-faint" aria-hidden />
        </button>
      </div>
    </nav>
  );
}
