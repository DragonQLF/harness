import { useEffect, useState } from "react";
import { api, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { ago, money, num, plural } from "../lib/format";
import { TONE, tone, type CheckRow, type CommitRow, type ProjectDetail } from "../lib/types";
import { useStore } from "../state/store";
import { DiffBlocks, Loading, MiniBars, tabular, truncate } from "../components/ui";
import type { View } from "./views";

/** Lane geometry, copied from the design's LANES table. Each row is 64x62. */
const LANES = {
  wip: { lane: 1, d1: "", d2: "M40 31 C40 46 16 46 16 62", dash: "5 5" },
  merge: { lane: 0, d1: "M16 0 V62", d2: "M16 31 C16 46 40 46 40 62", dash: "0" },
  branch: { lane: 1, d1: "M16 0 V62", d2: "M40 0 V62", dash: "0" },
  root: { lane: 1, d1: "M16 0 V62", d2: "M40 0 V31 C40 46 16 46 16 62", dash: "0" },
  main: { lane: 0, d1: "M16 0 V62", d2: "", dash: "0" },
  tail: { lane: 0, d1: "M16 0 V31", d2: "", dash: "0" },
} as const;

type LaneName = keyof typeof LANES;

/** Which lane shape a commit gets, read off the real history. */
function laneFor(commits: CommitRow[], i: number): LaneName {
  const c = commits[i]!;
  const older = commits[i + 1];
  const newer = commits[i - 1];
  if (!older) return c.on_default ? "tail" : "wip";
  if (!c.on_default) return older.on_default ? "root" : "branch";
  if (newer && !newer.on_default) return "merge";
  if (c.parents.length > 1) return "merge";
  return "main";
}

/** The first row is clipped so no line hangs above the top commit. */
function clipTop(d: string): string {
  return d
    .replace("M16 0 V", "M16 31 V")
    .replace("M40 0 V62", "M40 31 V62")
    .replace("M40 0 V31 C", "M40 31 C");
}

/** O painel destes ecrãs: raio 20, superfície, linha de 1px. */
const PANEL =
  "overflow-hidden rounded-xl border border-line bg-surface dark:border-line-d dark:bg-surface-d";

/** Uma ligação de texto discreta que acende ao passar por cima. */
const LINK =
  "cursor-pointer border-none bg-transparent transition-colors duration-150 hover:text-text focus-visible:text-text disabled:cursor-not-allowed dark:hover:text-text-d dark:focus-visible:text-text-d";

/** As cinco cores por que as linguagens passam, na ordem do desenho. */
const LANG = [TONE.accent, TONE.info, TONE.ok, TONE.warn, TONE.bad];

export function ProjectPage({ go }: { go: (v: View) => void }) {
  const { projectId, project, snapshot, agents, toast } = useStore();
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [checks, setChecks] = useState<CheckRow[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!projectId) return;
    let alive = true;
    setDetail(null);
    api
      .projectDetail(projectId, 14)
      .then((d) => {
        if (!alive) return;
        setDetail(d);
        setChecks(d.checks);
      })
      .catch((e) => toast("bad", "Could not read the repository", reason(e)));
    return () => {
      alive = false;
    };
  }, [projectId, toast]);

  if (!project) {
    return (
      <div className="px-6.5 pb-7 pt-5.5 text-md text-text3 dark:text-text3-d">
        Add a git repository from the switcher first.
      </div>
    );
  }
  if (!detail) return <Loading what="Reading the repository" />;

  const t = tone(project.tone);
  const langs = detail.languages;
  const langUsed = langs.reduce((a, l) => a + l.pct, 0);
  const worst = checks.some((c) => c.status === "fail")
    ? { label: "failing", tone: TONE.bad }
    : checks.some((c) => c.status === "warn")
      ? { label: "warnings", tone: TONE.warn }
      : checks.some((c) => c.status === "ok")
        ? { label: "passing", tone: TONE.ok }
        : { label: "not run", tone: TONE.neutral };
  const agentNames = new Set(detail.commits.map((c) => c.agent).filter(Boolean));

  const runChecks = async () => {
    if (!projectId) return;
    setBusy(true);
    try {
      setChecks(await api.runChecks(projectId));
    } catch (e) {
      toast("bad", "Checks failed to run", reason(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <div className="mb-4 flex items-center gap-2.5">
        <button
          type="button"
          onClick={() => go("projects")}
          className={cx(
            LINK,
            "p-0 text-[20px] font-extrabold tracking-[-.02em] text-text3 dark:text-text3-d",
          )}
        >
          Projects
        </button>
        <span className="text-lg text-text3 dark:text-text3-d">›</span>
        <h1 className="m-0 text-[20px] font-extrabold tracking-[-.02em]">{project.name}</h1>
        <span className="rounded-full border border-line bg-surface px-2.5 py-1 font-mono text-sm font-bold text-text2 dark:border-line-d dark:bg-surface-d dark:text-text2-d">
          {detail.default_branch}
        </span>
        <span
          className="rounded-full border border-line bg-surface2 px-2.5 py-1 font-mono text-sm font-bold text-text3 dark:border-line-d dark:bg-surface2-d dark:text-text3-d"
          title={
            detail.remote
              ? detail.remote
              : "Local only — Relay never needs a remote. Nothing leaves this machine unless an agent asks you to push."
          }
        >
          {detail.remote ? "origin" : "local only"}
        </span>
        <span className="text-md text-text3 dark:text-text3-d">
          Every commit with a card trailer was written by an agent in its own worktree
        </span>
      </div>

      <div className="grid grid-cols-[minmax(0,1fr)_292px] items-start gap-3.5">
        <div className="flex min-w-0 flex-col gap-3.5">
          <section
            className={cx(
              PANEL,
              "relative flex animate-[fadeUp_.45s_ease_both] flex-col shadow-panel dark:shadow-panel-d",
            )}
          >
            <span
              className={cx(
                "pointer-events-none absolute inset-x-0 top-0 h-[112px] bg-gradient-to-b to-transparent",
                t.wash,
              )}
            />
            <div className="relative flex items-start gap-3.5 px-5 pt-5">
              <span
                className={cx(
                  "flex h-[54px] w-[54px] flex-none items-center justify-center rounded-lg border bg-surface text-[20px] font-extrabold dark:bg-surface-d",
                  t.edge,
                  t.fg,
                )}
              >
                {project.glyph}
              </span>
              <div className="min-w-0 flex-1 pt-0.5">
                <div className="flex items-center gap-2.5">
                  <span className="font-mono text-xl font-extrabold tracking-[-.02em]">
                    {project.name}
                  </span>
                  <span className="rounded-full border border-line bg-surface2 px-2 py-0.5 text-xs font-bold text-text3 dark:border-line-d dark:bg-surface2-d dark:text-text3-d">
                    {langs.slice(0, 2).map((l) => l.name).join(" · ") || "no code yet"}
                  </span>
                </div>
                <p
                  className={cx(
                    truncate,
                    "mb-0 mt-1.75 max-w-[560px] font-mono text-md leading-[1.55] text-text2 dark:text-text2-d",
                  )}
                >
                  {project.path}
                </p>
              </div>
              <div className="flex flex-none items-end gap-3.5">
                <span className="w-[112px]">
                  <MiniBars values={detail.week_commits.map(Number)} tone={t} height={46} />
                </span>
                <span className="flex flex-col items-end gap-px">
                  <span className={cx(tabular, "text-xl font-extrabold tracking-[-.02em]")}>
                    {num(detail.week_lines)}
                  </span>
                  <span className="text-xs text-text3 dark:text-text3-d">lines this week</span>
                </span>
              </div>
            </div>
            <div className="relative mt-4.5 flex items-center gap-2.5 border-t border-line2 px-5 py-3.5 text-sm text-text3 dark:border-line2-d dark:text-text3-d">
              <span className="font-bold text-text2 dark:text-text2-d">
                {num(detail.commit_count)} commits
              </span>
              <span className="opacity-50">·</span>
              <span>{plural(detail.branches.length, "branch", "branches")}</span>
              <span className="opacity-50">·</span>
              <span>{plural(agentNames.size, "agent")}</span>
              <span className="opacity-50">·</span>
              <span>
                last commit{" "}
                {detail.commits[0]
                  ? ago(detail.commits[0].at_secs * 1000)
                  : "never"}
              </span>
              <span className="flex-1" />
              {project.mirror ? (
                <span
                  title="Relay's own source: accepted proposals are born here and read_docs reads this repository's docs/"
                  className="rounded-full border border-ok bg-okSoft px-2.5 py-1 text-sm font-bold text-ok dark:border-ok-d dark:bg-okSoft-d dark:text-ok-d"
                >
                  mirror mode
                </span>
              ) : (
                <span />
              )}
              <span className="opacity-50">·</span>
              <button
                type="button"
                onClick={() => go("trees")}
                className={cx(LINK, "text-sm font-bold text-text3 dark:text-text3-d")}
              >
                Worktrees →
              </button>
            </div>
          </section>

          <section className={cx(PANEL, "shadow-panel dark:shadow-panel-d")}>
            <div className="flex items-center gap-2.5 px-4.5 pb-3.5 pt-4">
              <h2 className="m-0 text-lg font-extrabold tracking-[-.01em]">History</h2>
              <span className="rounded-full border border-line bg-surface2 px-2 py-0.5 font-mono text-xs font-bold text-text3 dark:border-line-d dark:bg-surface2-d dark:text-text3-d">
                {detail.branches.length > 1
                  ? `${detail.default_branch} + ${plural(detail.branches.length - 1, "branch", "branches")}`
                  : `${detail.default_branch} only`}
              </span>
              <span className="flex-1" />
              <span className="text-sm text-text3 dark:text-text3-d">
                Click a commit to open its session
              </span>
            </div>

            {detail.commits.map((c, i) => {
              const lane = LANES[laneFor(detail.commits, i)];
              const d1 = i === 0 ? clipTop(lane.d1) : lane.d1;
              const d2 = i === 0 && lane.dash === "0" ? clipTop(lane.d2) : lane.d2;
              const agent = agents.find((a) => a.id === c.agent);
              const at = tone(agent?.tone ?? (c.agent ? "accent" : "warn"));
              const card = snapshot?.cards.find((x) => x.id === c.card);
              return (
                <button
                  key={c.sha}
                  type="button"
                  onClick={() => {
                    if (c.card) go("sessions");
                  }}
                  title={c.card ? `Open the session for ${c.card}` : undefined}
                  className={cx(
                    "flex h-[62px] w-full items-stretch border-t border-line2 bg-transparent p-0 text-left text-text transition-colors duration-150 hover:bg-hovered dark:border-line2-d dark:text-text-d dark:hover:bg-hovered-d",
                    c.card ? "cursor-pointer" : "cursor-default",
                  )}
                >
                  <span className="flex w-16 flex-none items-center justify-center">
                    {/* O grafo de commits é geometria, não um ícone: as linhas
                        vêm da história real e nenhuma biblioteca as desenha. */}
                    <svg
                      width="64"
                      height="62"
                      viewBox="0 0 64 62"
                      fill="none"
                      className="block overflow-visible"
                      aria-hidden="true"
                    >
                      {d1 && (
                        <path
                          d={d1}
                          className="stroke-text3 dark:stroke-text3-d"
                          strokeWidth="2"
                          strokeLinecap="round"
                        />
                      )}
                      {d2 && (
                        <path
                          d={d2}
                          className="stroke-accent dark:stroke-accent-d"
                          strokeWidth="2"
                          strokeLinecap="round"
                          strokeDasharray={lane.dash}
                        />
                      )}
                      <circle
                        cx={lane.lane ? 40 : 16}
                        cy="31"
                        r={lane.d2 && lane.dash === "0" && !lane.lane ? 6.5 : 5.5}
                        className={cx(
                          lane.dash !== "0"
                            ? "fill-bg dark:fill-bg-d"
                            : lane.lane
                              ? "fill-accent dark:fill-accent-d"
                              : "fill-text3 dark:fill-text3-d",
                          lane.lane
                            ? "stroke-accent dark:stroke-accent-d"
                            : "stroke-text3 dark:stroke-text3-d",
                        )}
                        strokeWidth="2.4"
                      />
                    </svg>
                  </span>

                  <span className="flex min-w-0 flex-1 items-center gap-3.5 pr-4.5">
                    <span
                      className={cx(
                        "flex h-6.5 w-6.5 flex-none items-center justify-center rounded-full text-sm font-extrabold",
                        at.soft,
                        at.fg,
                      )}
                    >
                      {agent?.initial ?? (c.author[0]?.toUpperCase() ?? "?")}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span
                        className={cx(truncate, "block text-lg font-semibold tracking-[-.01em]")}
                      >
                        {card?.title ?? c.subject ?? "(no message)"}
                      </span>
                      <span className="mt-1 flex items-center gap-2 text-sm text-text3 dark:text-text3-d">
                        <span className="font-bold text-text2 dark:text-text2-d">
                          {agent?.name ?? c.author}
                        </span>
                        <span className="opacity-50">·</span>
                        <span className="font-mono">
                          {c.on_default ? detail.default_branch : (c.card ? `harness/${c.card}` : "—")}
                        </span>
                        <span className="opacity-50">·</span>
                        <span>{c.when}</span>
                        {c.card && (
                          <span className="rounded-full bg-okSoft px-2 py-px font-mono text-xs font-extrabold text-ok dark:bg-okSoft-d dark:text-ok-d">
                            {c.card}
                          </span>
                        )}
                      </span>
                    </span>
                    <span
                      className={cx(tabular, "flex flex-none items-center gap-2 font-mono text-sm")}
                    >
                      <span className="font-bold text-ok dark:text-ok-d">+{num(c.added)}</span>
                      <span className="font-bold text-bad dark:text-bad-d">−{num(c.removed)}</span>
                    </span>
                    <DiffBlocks added={c.added} removed={c.removed} />
                    <span className="min-w-[60px] flex-none text-right font-mono text-sm text-text3 dark:text-text3-d">
                      {c.short}
                    </span>
                  </span>
                </button>
              );
            })}
            {detail.commits.length === 0 && (
              <div className="border-t border-line2 p-5.5 text-center text-md text-text3 dark:border-line2-d dark:text-text3-d">
                No commits yet. The first agent run will make one.
              </div>
            )}
          </section>
        </div>

        <div className="flex flex-col gap-3.5">
          <section className={PANEL}>
            <div className="px-4 pb-3 pt-3.5 text-md font-extrabold tracking-[-.01em]">
              Branches
            </div>
            {detail.branches.map((b) => {
              const dot =
                b.state === "live"
                  ? "bg-accent dark:bg-accent-d"
                  : b.state === "merged"
                    ? "bg-ok dark:bg-ok-d"
                    : b.state === "default"
                      ? "bg-text2 dark:bg-text2-d"
                      : "bg-text3 dark:bg-text3-d";
              return (
                <div
                  key={b.name}
                  className="flex items-center gap-2.5 border-t border-line2 px-4 py-3 dark:border-line2-d"
                >
                  <span
                    className={cx(
                      "h-1.75 w-1.75 flex-none rounded-full",
                      dot,
                      b.state === "live" && "animate-[breathe_2.2s_ease-in-out_infinite]",
                    )}
                  />
                  <span className={cx(truncate, "flex-1 font-mono text-sm")}>{b.name}</span>
                  <span
                    className={cx(
                      "flex-none text-xs",
                      b.state === "live"
                        ? "font-bold text-accent dark:text-accent-d"
                        : "font-medium text-text3 dark:text-text3-d",
                    )}
                  >
                    {b.state === "default" ? "default" : b.state === "live" ? "working" : b.when}
                  </span>
                </div>
              );
            })}
          </section>

          <section className={cx(PANEL, "p-4")}>
            <div className="mb-3 text-md font-extrabold tracking-[-.01em]">Languages</div>
            <div className="flex h-2 gap-0.5 overflow-hidden rounded-[4px]">
              {langs.map((l, i) => (
                <span
                  key={l.name}
                  title={`${l.name} ${l.pct.toFixed(1)}%`}
                  className={LANG[i % LANG.length].solid}
                  style={{ width: `${l.pct}%` }}
                />
              ))}
              {langUsed < 100 && (
                <span
                  className="bg-line dark:bg-line-d"
                  style={{ width: `${100 - langUsed}%` }}
                />
              )}
            </div>
            <div className="mt-3 flex flex-wrap gap-x-3.5 gap-y-1.5 text-sm text-text3 dark:text-text3-d">
              {langs.map((l, i) => (
                <span key={l.name} className="flex items-center gap-1.5">
                  <span
                    className={cx("h-1.75 w-1.75 rounded-full", LANG[i % LANG.length].solid)}
                  />
                  {l.name} {l.pct.toFixed(1)}%
                </span>
              ))}
              {langs.length === 0 && <span>Nothing recognisable yet.</span>}
            </div>
          </section>

          <section className={PANEL}>
            <div className="flex items-center gap-2.5 px-4 pb-3 pt-3.5">
              <span className="text-md font-extrabold tracking-[-.01em]">
                Checks on {detail.default_branch}
              </span>
              <span
                className={cx(
                  "rounded-full px-2 py-px text-xs font-extrabold",
                  worst.tone.soft,
                  worst.tone.fg,
                )}
              >
                {worst.label}
              </span>
              <span className="flex-1" />
              <button
                type="button"
                disabled={busy}
                onClick={runChecks}
                className={cx(
                  LINK,
                  "text-sm font-bold text-text3 dark:text-text3-d",
                  busy && "cursor-progress",
                )}
              >
                {busy ? "Running…" : "Run →"}
              </button>
            </div>
            {checks.map((ck) => {
              const st =
                ck.status === "ok"
                  ? TONE.ok
                  : ck.status === "warn"
                    ? TONE.warn
                    : ck.status === "fail"
                      ? TONE.bad
                      : TONE.neutral;
              return (
                <div
                  key={ck.name}
                  title={ck.detail}
                  className="flex items-center gap-2.5 border-t border-line2 px-4 py-2.5 dark:border-line2-d"
                >
                  <span className={cx("h-1.75 w-1.75 flex-none rounded-full", st.solid)} />
                  <span className={cx(truncate, "flex-1 font-mono text-sm")}>{ck.name}</span>
                  <span
                    className={cx(
                      tabular,
                      "text-xs",
                      ck.status === "warn" || ck.status === "fail"
                        ? st.fg
                        : "text-text3 dark:text-text3-d",
                      ck.status === "ok" ? "font-medium" : "font-bold",
                    )}
                  >
                    {ck.ran_ms ? ck.detail.slice(0, 22) : "not run"}
                  </span>
                </div>
              );
            })}
            {checks.length === 0 && (
              <div className="border-t border-line2 px-4 py-3.5 text-sm leading-normal text-text3 dark:border-line2-d dark:text-text3-d">
                No checks recognised for this repository.
              </div>
            )}
          </section>

          <section className={cx(PANEL, "p-4")}>
            <div className="text-md font-extrabold tracking-[-.01em]">Spend</div>
            <div className="mt-2 flex items-end justify-between">
              <span className={cx(tabular, "text-2xl font-extrabold tracking-[-.02em]")}>
                {money(project.stats.spend_total)}
              </span>
              <span className="text-sm text-text3 dark:text-text3-d">
                {plural(project.stats.runs_total, "run")}
              </span>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

/** The list of registered repositories. */
export function Projects({ go }: { go: (v: View) => void }) {
  const { projects, selectProject, addProject, removeProject } = useStore();

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <div className="mb-4 flex items-center gap-2.5">
        {/* The chrome above already carries "Projects" and the repository
            count. What it cannot say is what registering one means. */}
        <span className="text-md text-text3 dark:text-text3-d">
          Every repository Relay is allowed to touch
        </span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={addProject}
          className="min-h-6 cursor-pointer rounded-full border-none bg-accent px-4 py-2 text-md font-bold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-accent-d dark:text-onAccent-d"
        >
          Add a project
        </button>
      </div>

      <div className="grid grid-cols-[repeat(2,minmax(0,1fr))] gap-3.5">
        {projects.map((p) => {
          const t = tone(p.tone);
          const st = !p.exists
            ? { label: "folder missing", tone: TONE.bad }
            : p.paused
              ? { label: "paused", tone: TONE.neutral }
              : p.stats.running
                ? { label: "working", tone: TONE.accent }
                : p.stats.review
                  ? { label: "needs you", tone: TONE.warn }
                  : { label: "idle", tone: TONE.neutral };
          return (
            <section
              key={p.id}
              className={cx(
                PANEL,
                "relative animate-[fadeUp_.45s_ease_both] shadow-panel dark:shadow-panel-d",
              )}
            >
              <span
                className={cx(
                  "pointer-events-none absolute inset-x-0 top-0 h-[112px] bg-gradient-to-b to-transparent",
                  t.wash,
                )}
              />
              <div className="relative flex items-start gap-3.5 px-5 pt-5">
                <span
                  className={cx(
                    "flex h-[46px] w-[46px] flex-none items-center justify-center rounded-lg border bg-surface text-xl font-extrabold dark:bg-surface-d",
                    t.edge,
                    t.fg,
                  )}
                >
                  {p.glyph}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2.5">
                    <span
                      className={cx(truncate, "text-xl font-extrabold tracking-[-.02em]")}
                    >
                      {p.name}
                    </span>
                    <span
                      className={cx(
                        "flex flex-none items-center gap-1.5 rounded-full px-2.5 py-1 text-sm font-bold",
                        st.tone.soft,
                        st.tone.fg,
                      )}
                    >
                      <span className={cx("h-1.25 w-1.25 rounded-full", st.tone.solid)} />
                      {st.label}
                    </span>
                    {p.mirror && (
                      <span
                        title="Mirror mode: the project Relay improves itself in"
                        className="flex-none rounded-full bg-okSoft px-2.5 py-1 text-sm font-bold text-ok dark:bg-okSoft-d dark:text-ok-d"
                      >
                        mirror
                      </span>
                    )}
                  </div>
                  <p
                    className={cx(
                      truncate,
                      "mb-0 mt-1.5 font-mono text-sm text-text3 dark:text-text3-d",
                    )}
                  >
                    {p.path}
                  </p>
                </div>
                <span className="w-24 flex-none">
                  <MiniBars values={p.stats.week_runs} tone={t} height={40} />
                </span>
              </div>

              <div className="relative mt-4 flex items-center gap-2.5 border-t border-line2 px-5 py-3 text-sm text-text3 dark:border-line2-d dark:text-text3-d">
                <span className="font-bold text-text2 dark:text-text2-d">
                  {plural(p.stats.cards, "card")}
                </span>
                <span className="opacity-50">·</span>
                <span>{money(p.stats.spend_total)} all time</span>
                <span className="flex-1" />
                <button
                  type="button"
                  disabled={!p.exists}
                  onClick={() => {
                    selectProject(p.id);
                    go("board");
                  }}
                  className={cx(
                    LINK,
                    "text-sm font-extrabold",
                    p.exists ? t.fg : "text-text3 dark:text-text3-d",
                  )}
                >
                  Board →
                </button>
                <button
                  type="button"
                  disabled={!p.exists}
                  onClick={() => {
                    selectProject(p.id);
                    go("code");
                  }}
                  className={cx(LINK, "text-sm font-bold text-text3 dark:text-text3-d")}
                >
                  Code →
                </button>
                <button
                  type="button"
                  onClick={() => removeProject(p.id, false)}
                  className={cx(LINK, "text-sm text-text3 dark:text-text3-d")}
                >
                  Forget
                </button>
              </div>
            </section>
          );
        })}

        <button
          type="button"
          onClick={addProject}
          className="flex min-h-[168px] cursor-pointer flex-col items-center justify-center gap-2.5 rounded-xl border border-dashed border-line bg-transparent text-text3 transition-[border-color,color] duration-200 hover:border-accentLine hover:text-text2 focus-visible:border-accentLine dark:border-line-d dark:text-text3-d dark:hover:border-accentLine-d dark:hover:text-text2-d dark:focus-visible:border-accentLine-d"
        >
          <span className="flex h-9 w-9 items-center justify-center rounded-full border border-dashed border-current text-xl">
            +
          </span>
          <span className="text-md font-semibold">Add a git repository</span>
        </button>
      </div>
    </div>
  );
}
