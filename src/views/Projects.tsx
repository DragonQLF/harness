import { useEffect, useState } from "react";
import { api, reason } from "../lib/ipc";
import { ago, money, num, plural } from "../lib/format";
import { tone, type CheckRow, type CommitRow, type ProjectDetail } from "../lib/types";
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

export function ProjectPage({ go }: { go: (v: View) => void }) {
  const { projectId, project, projects, snapshot, agents, toast, refreshProjects } = useStore();
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
      .catch((e) => toast("var(--bad)", "Could not read the repository", reason(e)));
    return () => {
      alive = false;
    };
  }, [projectId, toast]);

  if (!project) {
    return (
      <div style={{ padding: "22px 26px 28px", fontSize: 12.5, color: "var(--text3)" }}>
        Add a git repository from the switcher first.
      </div>
    );
  }
  if (!detail) return <Loading what="Reading the repository" />;

  const t = tone(project.tone);
  const langs = detail.languages;
  const langColors = ["var(--accent)", "var(--info)", "var(--ok)", "var(--warn)", "var(--bad)"];
  const langUsed = langs.reduce((a, l) => a + l.pct, 0);
  const worst = checks.some((c) => c.status === "fail")
    ? { label: "failing", fg: "var(--bad)", soft: "var(--badSoft)" }
    : checks.some((c) => c.status === "warn")
      ? { label: "warnings", fg: "var(--warn)", soft: "var(--warnSoft)" }
      : checks.some((c) => c.status === "ok")
        ? { label: "passing", fg: "var(--ok)", soft: "var(--okSoft)" }
        : { label: "not run", fg: "var(--text3)", soft: "var(--surface2)" };
  const agentNames = new Set(detail.commits.map((c) => c.agent).filter(Boolean));

  /** Mirror mode: the project Relay treats as its own home — where the
   *  Director's accepted proposals become cards (#72, #79) and where read_docs
   *  looks for DEBT.md and DECISIONS.md (#78). Exactly one project holds it;
   *  the backend takes it from whoever had it before. */
  const held = projects.find((p) => p.mirror);
  const toggleMirror = async () => {
    if (!project) return;
    setBusy(true);
    try {
      await api.projectUpdate({ ...project, mirror: !project.mirror });
      await refreshProjects();
      toast(
        "var(--ok)",
        project.mirror ? "Mirror mode off" : "Mirror mode on",
        project.mirror
          ? "The Director has nowhere to file accepted proposals until another project takes it."
          : `Accepted proposals become cards in ${project.name}, and read_docs reads its docs/.`,
      );
    } catch (e) {
      toast("var(--bad)", "Could not change mirror mode", reason(e));
    } finally {
      setBusy(false);
    }
  };

  const runChecks = async () => {
    if (!projectId) return;
    setBusy(true);
    try {
      setChecks(await api.runChecks(projectId));
    } catch (e) {
      toast("var(--bad)", "Checks failed to run", reason(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
        <button
          type="button"
          className="hv-link"
          onClick={() => go("projects")}
          style={{
            padding: 0,
            border: "none",
            background: "transparent",
            color: "var(--text3)",
            fontSize: 20,
            fontWeight: 800,
            letterSpacing: "-.02em",
            cursor: "pointer",
            transition: "color .16s ease",
          }}
        >
          Projects
        </button>
        <span style={{ color: "var(--text3)", fontSize: 14 }}>›</span>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>
          {project.name}
        </h1>
        <span
          style={{
            padding: "3px 9px",
            borderRadius: 999,
            background: "var(--surface)",
            border: "1px solid var(--line)",
            fontSize: 11.5,
            fontWeight: 700,
            color: "var(--text2)",
            fontFamily: "var(--mono)",
          }}
        >
          {detail.default_branch}
        </span>
        <span
          style={{
            padding: "3px 9px",
            borderRadius: 999,
            background: "var(--surface2)",
            border: "1px solid var(--line)",
            fontSize: 11,
            fontWeight: 700,
            color: "var(--text3)",
            fontFamily: "var(--mono)",
          }}
          title={
            detail.remote
              ? detail.remote
              : "Local only — Relay never needs a remote. Nothing leaves this machine unless an agent asks you to push."
          }
        >
          {detail.remote ? "origin" : "local only"}
        </span>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>
          Every commit with a card trailer was written by an agent in its own worktree
        </span>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1fr) 292px",
          gap: 13,
          alignItems: "start",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 13, minWidth: 0 }}>
          <section
            style={{
              position: "relative",
              display: "flex",
              flexDirection: "column",
              border: "1px solid var(--line)",
              borderRadius: 24,
              background: "var(--surface)",
              overflow: "hidden",
              boxShadow: "0 1px 2px rgba(20,20,40,.05)",
              animation: "fadeUp .45s ease both",
            }}
          >
            <span
              style={{
                position: "absolute",
                left: 0,
                right: 0,
                top: 0,
                height: 112,
                background: `linear-gradient(180deg,${t.soft} 0%,transparent 100%)`,
                pointerEvents: "none",
              }}
            />
            <div
              style={{
                position: "relative",
                display: "flex",
                alignItems: "flex-start",
                gap: 14,
                padding: "20px 20px 0",
              }}
            >
              <span
                style={{
                  width: 54,
                  height: 54,
                  flex: "none",
                  borderRadius: 17,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  background: "var(--surface)",
                  border: `1px solid ${t.color}`,
                  color: t.color,
                  fontSize: 19,
                  fontWeight: 800,
                }}
              >
                {project.glyph}
              </span>
              <div style={{ flex: 1, minWidth: 0, paddingTop: 2 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
                  <span
                    style={{
                      fontSize: 17,
                      fontWeight: 800,
                      letterSpacing: "-.02em",
                      fontFamily: "var(--mono)",
                    }}
                  >
                    {project.name}
                  </span>
                  <span
                    style={{
                      padding: "2px 8px",
                      borderRadius: 999,
                      background: "var(--surface2)",
                      border: "1px solid var(--line)",
                      fontSize: 10.5,
                      fontWeight: 700,
                      color: "var(--text3)",
                    }}
                  >
                    {langs.slice(0, 2).map((l) => l.name).join(" · ") || "no code yet"}
                  </span>
                </div>
                <p
                  style={{
                    margin: "7px 0 0",
                    maxWidth: 560,
                    fontSize: 12.5,
                    color: "var(--text2)",
                    lineHeight: 1.55,
                    fontFamily: "var(--mono)",
                    ...truncate,
                  }}
                >
                  {project.path}
                </p>
              </div>
              <div style={{ flex: "none", display: "flex", alignItems: "flex-end", gap: 14 }}>
                <span style={{ width: 112 }}>
                  <MiniBars values={detail.week_commits.map(Number)} color={t.color} height={46} />
                </span>
                <span
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "flex-end",
                    gap: 1,
                  }}
                >
                  <span
                    style={{ fontSize: 16, fontWeight: 800, letterSpacing: "-.02em", ...tabular }}
                  >
                    {num(detail.week_lines)}
                  </span>
                  <span style={{ fontSize: 10.5, color: "var(--text3)" }}>lines this week</span>
                </span>
              </div>
            </div>
            <div
              style={{
                position: "relative",
                display: "flex",
                alignItems: "center",
                gap: 9,
                marginTop: 18,
                padding: "13px 20px",
                borderTop: "1px solid var(--line2)",
                fontSize: 11.5,
                color: "var(--text3)",
              }}
            >
              <span style={{ fontWeight: 700, color: "var(--text2)" }}>
                {num(detail.commit_count)} commits
              </span>
              <span style={{ opacity: 0.5 }}>·</span>
              <span>{plural(detail.branches.length, "branch", "branches")}</span>
              <span style={{ opacity: 0.5 }}>·</span>
              <span>{plural(agentNames.size, "agent")}</span>
              <span style={{ opacity: 0.5 }}>·</span>
              <span>
                last commit{" "}
                {detail.commits[0]
                  ? ago(detail.commits[0].at_secs * 1000)
                  : "never"}
              </span>
              <span style={{ flex: 1 }} />
              <button
                type="button"
                className="hv-link"
                disabled={busy}
                onClick={toggleMirror}
                title={
                  project.mirror
                    ? "Relay's own home: accepted proposals are born here and read_docs reads this repository's docs/"
                    : held
                      ? `Mirror mode is on ${held.name}. Turning it on here takes it from there.`
                      : "Make this the project Relay improves itself in"
                }
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 7,
                  padding: "3px 10px 3px 7px",
                  borderRadius: 999,
                  background: project.mirror ? "var(--okSoft)" : "var(--surface2)",
                  border: `1px solid ${project.mirror ? "var(--ok)" : "var(--line)"}`,
                  color: project.mirror ? "var(--ok)" : "var(--text3)",
                  fontSize: 11,
                  fontWeight: 700,
                  cursor: busy ? "default" : "pointer",
                  opacity: busy ? 0.6 : 1,
                }}
              >
                <span
                  style={{
                    width: 22,
                    height: 12,
                    borderRadius: 999,
                    background: project.mirror ? "var(--ok)" : "var(--line3)",
                    position: "relative",
                    transition: "background .16s ease",
                  }}
                >
                  <span
                    style={{
                      position: "absolute",
                      top: 2,
                      left: project.mirror ? 12 : 2,
                      width: 8,
                      height: 8,
                      borderRadius: 999,
                      background: "var(--surface)",
                      transition: "left .16s ease",
                    }}
                  />
                </span>
                mirror mode
              </button>
              <span style={{ opacity: 0.5 }}>·</span>
              <button
                type="button"
                className="hv-link"
                onClick={() => go("trees")}
                style={{
                  background: "transparent",
                  border: "none",
                  color: "var(--text3)",
                  fontSize: 11.5,
                  cursor: "pointer",
                  fontWeight: 700,
                }}
              >
                Worktrees →
              </button>
            </div>
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 24,
              background: "var(--surface)",
              overflow: "hidden",
              boxShadow: "0 1px 2px rgba(20,20,40,.05)",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "15px 18px 14px" }}>
              <h2 style={{ margin: 0, fontSize: 13.5, fontWeight: 800, letterSpacing: "-.01em" }}>
                History
              </h2>
              <span
                style={{
                  padding: "2px 8px",
                  borderRadius: 999,
                  background: "var(--surface2)",
                  border: "1px solid var(--line)",
                  fontSize: 10.5,
                  fontWeight: 700,
                  color: "var(--text3)",
                  fontFamily: "var(--mono)",
                }}
              >
                {detail.branches.length > 1
                  ? `${detail.default_branch} + ${plural(detail.branches.length - 1, "branch", "branches")}`
                  : `${detail.default_branch} only`}
              </span>
              <span style={{ flex: 1 }} />
              <span style={{ fontSize: 11.5, color: "var(--text3)" }}>
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
                  className="hv-hover"
                  onClick={() => {
                    if (c.card) go("sessions");
                  }}
                  title={c.card ? `Open the session for ${c.card}` : undefined}
                  style={{
                    display: "flex",
                    alignItems: "stretch",
                    width: "100%",
                    height: 62,
                    padding: 0,
                    border: "none",
                    borderTop: "1px solid var(--line2)",
                    background: "transparent",
                    color: "var(--text)",
                    cursor: c.card ? "pointer" : "default",
                    textAlign: "left",
                    transition: "background .16s ease",
                  }}
                >
                  <span
                    style={{
                      flex: "none",
                      width: 64,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                    }}
                  >
                    <svg
                      width="64"
                      height="62"
                      viewBox="0 0 64 62"
                      fill="none"
                      style={{ display: "block", overflow: "visible" }}
                    >
                      {d1 && (
                        <path d={d1} stroke="var(--text3)" strokeWidth="2" strokeLinecap="round" />
                      )}
                      {d2 && (
                        <path
                          d={d2}
                          stroke="var(--accent)"
                          strokeWidth="2"
                          strokeLinecap="round"
                          strokeDasharray={lane.dash}
                        />
                      )}
                      <circle
                        cx={lane.lane ? 40 : 16}
                        cy="31"
                        r={lane.d2 && lane.dash === "0" && !lane.lane ? 6.5 : 5.5}
                        fill={
                          lane.dash !== "0"
                            ? "var(--bg)"
                            : lane.lane
                              ? "var(--accent)"
                              : "var(--text3)"
                        }
                        stroke={lane.lane ? "var(--accent)" : "var(--text3)"}
                        strokeWidth="2.4"
                      />
                    </svg>
                  </span>

                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      display: "flex",
                      alignItems: "center",
                      gap: 13,
                      paddingRight: 18,
                    }}
                  >
                    <span
                      style={{
                        flex: "none",
                        width: 26,
                        height: 26,
                        borderRadius: "50%",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        background: at.soft,
                        color: at.color,
                        fontSize: 11,
                        fontWeight: 800,
                      }}
                    >
                      {agent?.initial ?? (c.author[0]?.toUpperCase() ?? "?")}
                    </span>
                    <span style={{ flex: 1, minWidth: 0 }}>
                      <span
                        style={{
                          display: "block",
                          fontSize: 13.5,
                          fontWeight: 600,
                          letterSpacing: "-.01em",
                          ...truncate,
                        }}
                      >
                        {card?.title ?? c.subject ?? "(no message)"}
                      </span>
                      <span
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 7,
                          marginTop: 3,
                          fontSize: 11,
                          color: "var(--text3)",
                        }}
                      >
                        <span style={{ fontWeight: 700, color: "var(--text2)" }}>
                          {agent?.name ?? c.author}
                        </span>
                        <span style={{ opacity: 0.5 }}>·</span>
                        <span style={{ fontFamily: "var(--mono)" }}>
                          {c.on_default ? detail.default_branch : (c.card ? `harness/${c.card}` : "—")}
                        </span>
                        <span style={{ opacity: 0.5 }}>·</span>
                        <span>{c.when}</span>
                        {c.card && (
                          <span
                            style={{
                              padding: "1px 7px",
                              borderRadius: 999,
                              background: "var(--okSoft)",
                              color: "var(--ok)",
                              fontSize: 10,
                              fontWeight: 800,
                              fontFamily: "var(--mono)",
                            }}
                          >
                            {c.card}
                          </span>
                        )}
                      </span>
                    </span>
                    <span
                      style={{
                        flex: "none",
                        display: "flex",
                        alignItems: "center",
                        gap: 7,
                        fontFamily: "var(--mono)",
                        fontSize: 11,
                        ...tabular,
                      }}
                    >
                      <span style={{ color: "var(--ok)", fontWeight: 700 }}>+{num(c.added)}</span>
                      <span style={{ color: "var(--bad)", fontWeight: 700 }}>−{num(c.removed)}</span>
                    </span>
                    <DiffBlocks added={c.added} removed={c.removed} />
                    <span
                      style={{
                        flex: "none",
                        minWidth: 60,
                        textAlign: "right",
                        fontFamily: "var(--mono)",
                        fontSize: 11.5,
                        color: "var(--text3)",
                      }}
                    >
                      {c.short}
                    </span>
                  </span>
                </button>
              );
            })}
            {detail.commits.length === 0 && (
              <div
                style={{
                  padding: 22,
                  borderTop: "1px solid var(--line2)",
                  textAlign: "center",
                  fontSize: 12.5,
                  color: "var(--text3)",
                }}
              >
                No commits yet. The first agent run will make one.
              </div>
            )}
          </section>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 13 }}>
          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 20,
              background: "var(--surface)",
              overflow: "hidden",
            }}
          >
            <div style={{ padding: "14px 16px 12px", fontSize: 12.5, fontWeight: 800, letterSpacing: "-.01em" }}>
              Branches
            </div>
            {detail.branches.map((b) => {
              const dot =
                b.state === "live"
                  ? "var(--accent)"
                  : b.state === "merged"
                    ? "var(--ok)"
                    : b.state === "default"
                      ? "var(--text2)"
                      : "var(--text3)";
              return (
                <div
                  key={b.name}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "11px 16px",
                    borderTop: "1px solid var(--line2)",
                  }}
                >
                  <span
                    style={{
                      width: 7,
                      height: 7,
                      flex: "none",
                      borderRadius: "50%",
                      background: dot,
                      animation:
                        b.state === "live" ? "breathe 2.2s ease-in-out infinite" : undefined,
                    }}
                  />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontFamily: "var(--mono)",
                      fontSize: 11.5,
                      ...truncate,
                    }}
                  >
                    {b.name}
                  </span>
                  <span
                    style={{
                      flex: "none",
                      fontSize: 10.5,
                      color: b.state === "live" ? "var(--accent)" : "var(--text3)",
                      fontWeight: b.state === "live" ? 700 : 500,
                    }}
                  >
                    {b.state === "default" ? "default" : b.state === "live" ? "working" : b.when}
                  </span>
                </div>
              );
            })}
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 20,
              background: "var(--surface)",
              padding: "15px 16px 16px",
            }}
          >
            <div
              style={{ fontSize: 12.5, fontWeight: 800, letterSpacing: "-.01em", marginBottom: 12 }}
            >
              Languages
            </div>
            <div style={{ display: "flex", gap: 2, height: 8, borderRadius: 5, overflow: "hidden" }}>
              {langs.map((l, i) => (
                <span
                  key={l.name}
                  title={`${l.name} ${l.pct.toFixed(1)}%`}
                  style={{ width: `${l.pct}%`, background: langColors[i % langColors.length] }}
                />
              ))}
              {langUsed < 100 && (
                <span style={{ width: `${100 - langUsed}%`, background: "var(--line)" }} />
              )}
            </div>
            <div
              style={{
                display: "flex",
                flexWrap: "wrap",
                gap: "6px 14px",
                marginTop: 12,
                fontSize: 11,
                color: "var(--text3)",
              }}
            >
              {langs.map((l, i) => (
                <span key={l.name} style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  <span
                    style={{
                      width: 7,
                      height: 7,
                      borderRadius: "50%",
                      background: langColors[i % langColors.length],
                    }}
                  />
                  {l.name} {l.pct.toFixed(1)}%
                </span>
              ))}
              {langs.length === 0 && <span>Nothing recognisable yet.</span>}
            </div>
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 20,
              background: "var(--surface)",
              overflow: "hidden",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "14px 16px 12px" }}>
              <span style={{ fontSize: 12.5, fontWeight: 800, letterSpacing: "-.01em" }}>
                Checks on {detail.default_branch}
              </span>
              <span
                style={{
                  padding: "1px 7px",
                  borderRadius: 999,
                  background: worst.soft,
                  color: worst.fg,
                  fontSize: 10,
                  fontWeight: 800,
                }}
              >
                {worst.label}
              </span>
              <span style={{ flex: 1 }} />
              <button
                type="button"
                className="hv-link"
                disabled={busy}
                onClick={runChecks}
                style={{
                  background: "transparent",
                  border: "none",
                  color: "var(--text3)",
                  fontSize: 11.5,
                  fontWeight: 700,
                  cursor: busy ? "progress" : "pointer",
                }}
              >
                {busy ? "Running…" : "Run →"}
              </button>
            </div>
            {checks.map((ck) => {
              const dot =
                ck.status === "ok"
                  ? "var(--ok)"
                  : ck.status === "warn"
                    ? "var(--warn)"
                    : ck.status === "fail"
                      ? "var(--bad)"
                      : "var(--text3)";
              return (
                <div
                  key={ck.name}
                  title={ck.detail}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 16px",
                    borderTop: "1px solid var(--line2)",
                  }}
                >
                  <span
                    style={{ width: 7, height: 7, flex: "none", borderRadius: "50%", background: dot }}
                  />
                  <span style={{ flex: 1, fontFamily: "var(--mono)", fontSize: 11, ...truncate }}>
                    {ck.name}
                  </span>
                  <span
                    style={{
                      fontSize: 10.5,
                      color: ck.status === "warn" || ck.status === "fail" ? dot : "var(--text3)",
                      fontWeight: ck.status === "ok" ? 500 : 700,
                      ...tabular,
                    }}
                  >
                    {ck.ran_ms ? ck.detail.slice(0, 22) : "not run"}
                  </span>
                </div>
              );
            })}
            {checks.length === 0 && (
              <div
                style={{
                  padding: "14px 16px",
                  borderTop: "1px solid var(--line2)",
                  fontSize: 11.5,
                  color: "var(--text3)",
                  lineHeight: 1.5,
                }}
              >
                No checks recognised for this repository.
              </div>
            )}
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 20,
              background: "var(--surface)",
              padding: "15px 16px",
            }}
          >
            <div style={{ fontSize: 12.5, fontWeight: 800, letterSpacing: "-.01em" }}>Spend</div>
            <div
              style={{
                display: "flex",
                alignItems: "flex-end",
                justifyContent: "space-between",
                marginTop: 8,
              }}
            >
              <span style={{ fontSize: 22, fontWeight: 800, letterSpacing: "-.03em", ...tabular }}>
                {money(project.stats.spend_total)}
              </span>
              <span style={{ fontSize: 11, color: "var(--text3)" }}>
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
    <div style={{ padding: "22px 26px 28px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>Projects</h1>
        <span
          style={{
            padding: "3px 9px",
            borderRadius: 999,
            background: "var(--surface)",
            border: "1px solid var(--line)",
            fontSize: 11.5,
            fontWeight: 700,
            color: "var(--text2)",
            ...tabular,
          }}
        >
          {projects.length}
        </span>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>
          Every repository Relay is allowed to touch
        </span>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          className="hv-bright"
          onClick={addProject}
          style={{
            padding: "8px 15px",
            border: "none",
            borderRadius: 999,
            background: "var(--accent)",
            color: "var(--onAccent)",
            fontSize: 12.5,
            fontWeight: 700,
            cursor: "pointer",
            transition: "filter .18s ease",
          }}
        >
          Add a project
        </button>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(2,minmax(0,1fr))", gap: 13 }}>
        {projects.map((p) => {
          const t = tone(p.tone);
          const st = !p.exists
            ? { label: "folder missing", fg: "var(--bad)", soft: "var(--badSoft)" }
            : p.paused
              ? { label: "paused", fg: "var(--text3)", soft: "var(--surface2)" }
              : p.stats.running
                ? { label: "working", fg: "var(--accent)", soft: "var(--accentSoft)" }
                : p.stats.review
                  ? { label: "needs you", fg: "var(--warn)", soft: "var(--warnSoft)" }
                  : { label: "idle", fg: "var(--text3)", soft: "var(--surface2)" };
          return (
            <section
              key={p.id}
              style={{
                position: "relative",
                border: "1px solid var(--line)",
                borderRadius: 24,
                background: "var(--surface)",
                overflow: "hidden",
                boxShadow: "0 1px 2px rgba(20,20,40,.05)",
                animation: "fadeUp .45s ease both",
              }}
            >
              <span
                style={{
                  position: "absolute",
                  left: 0,
                  right: 0,
                  top: 0,
                  height: 112,
                  background: `linear-gradient(180deg,${t.soft} 0%,transparent 100%)`,
                  pointerEvents: "none",
                }}
              />
              <div
                style={{
                  position: "relative",
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 14,
                  padding: "20px 20px 0",
                }}
              >
                <span
                  style={{
                    width: 46,
                    height: 46,
                    flex: "none",
                    borderRadius: 15,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    background: "var(--surface)",
                    border: `1px solid ${t.color}`,
                    color: t.color,
                    fontSize: 16,
                    fontWeight: 800,
                  }}
                >
                  {p.glyph}
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
                    <span
                      style={{ fontSize: 16, fontWeight: 800, letterSpacing: "-.02em", ...truncate }}
                    >
                      {p.name}
                    </span>
                    <span
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 6,
                        padding: "3px 9px",
                        borderRadius: 999,
                        background: st.soft,
                        color: st.fg,
                        fontSize: 11,
                        fontWeight: 700,
                        flex: "none",
                      }}
                    >
                      <span
                        style={{ width: 5, height: 5, borderRadius: "50%", background: st.fg }}
                      />
                      {st.label}
                    </span>
                    {p.mirror && (
                      <span
                        title="Mirror mode: the project Relay improves itself in"
                        style={{
                          padding: "3px 9px",
                          borderRadius: 999,
                          background: "var(--okSoft)",
                          color: "var(--ok)",
                          fontSize: 11,
                          fontWeight: 700,
                          flex: "none",
                        }}
                      >
                        mirror
                      </span>
                    )}
                  </div>
                  <p
                    style={{
                      margin: "6px 0 0",
                      fontSize: 11.5,
                      color: "var(--text3)",
                      fontFamily: "var(--mono)",
                      ...truncate,
                    }}
                  >
                    {p.path}
                  </p>
                </div>
                <span style={{ width: 96, flex: "none" }}>
                  <MiniBars values={p.stats.week_runs} color={t.color} height={40} />
                </span>
              </div>

              <div
                style={{
                  position: "relative",
                  display: "flex",
                  alignItems: "center",
                  gap: 9,
                  marginTop: 16,
                  padding: "12px 20px",
                  borderTop: "1px solid var(--line2)",
                  fontSize: 11.5,
                  color: "var(--text3)",
                }}
              >
                <span style={{ fontWeight: 700, color: "var(--text2)" }}>
                  {plural(p.stats.cards, "card")}
                </span>
                <span style={{ opacity: 0.5 }}>·</span>
                <span>{money(p.stats.spend_total)} all time</span>
                <span style={{ flex: 1 }} />
                <button
                  type="button"
                  className="hv-link"
                  disabled={!p.exists}
                  onClick={() => {
                    selectProject(p.id);
                    go("board");
                  }}
                  style={{
                    background: "transparent",
                    border: "none",
                    color: p.exists ? t.color : "var(--text3)",
                    fontSize: 11.5,
                    fontWeight: 800,
                    cursor: p.exists ? "pointer" : "not-allowed",
                  }}
                >
                  Board →
                </button>
                <button
                  type="button"
                  className="hv-link"
                  disabled={!p.exists}
                  onClick={() => {
                    selectProject(p.id);
                    go("code");
                  }}
                  style={{
                    background: "transparent",
                    border: "none",
                    color: "var(--text3)",
                    fontSize: 11.5,
                    fontWeight: 700,
                    cursor: p.exists ? "pointer" : "not-allowed",
                  }}
                >
                  Code →
                </button>
                <button
                  type="button"
                  className="hv-link"
                  onClick={() => removeProject(p.id, false)}
                  style={{
                    background: "transparent",
                    border: "none",
                    color: "var(--text3)",
                    fontSize: 11.5,
                    cursor: "pointer",
                  }}
                >
                  Forget
                </button>
              </div>
            </section>
          );
        })}

        <button
          type="button"
          className="hv-border"
          onClick={addProject}
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 9,
            minHeight: 168,
            border: "1px dashed var(--line)",
            borderRadius: 24,
            background: "transparent",
            color: "var(--text3)",
            cursor: "pointer",
            transition: "all .22s cubic-bezier(.2,.8,.2,1)",
          }}
        >
          <span
            style={{
              width: 36,
              height: 36,
              borderRadius: "50%",
              border: "1px dashed currentColor",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 17,
            }}
          >
            +
          </span>
          <span style={{ fontSize: 12.5, fontWeight: 600 }}>Add a git repository</span>
        </button>
      </div>
    </div>
  );
}
