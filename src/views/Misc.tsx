/** Director, Worktrees, Activity and Settings, ported from the design. */

import { useEffect, useState, type ReactNode } from "react";
import { api, events, reason } from "../lib/ipc";
import { clock, money, num, plural } from "../lib/format";
import { MODELS, ruleIsRevoked, ruleLabel, tone, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { Loading, Switch, tabular, truncate } from "../components/ui";
import type { View } from "./views";

export function DirectorPage({
  go,
  openChat,
}: {
  go: (v: View) => void;
  openChat: () => void;
}) {
  const {
    agents,
    agentStats,
    projects,
    activity,
    settings,
    saveAgents,
    saveSettings,
    selectProject,
  } = useStore();
  const director = agents.find((a) => a.id === "director");
  const stats = agentStats.director;

  if (!director || !settings) return <Loading what="Reading the Director" />;

  const t = tone(director.tone ?? "info");
  const state = director.paused
    ? { label: "paused", fg: "var(--text3)", soft: "var(--surface2)" }
    : { label: "watching", fg: "var(--info)", soft: "var(--infoSoft)" };

  const patch = (next: Partial<typeof director>) =>
    saveAgents(agents.map((a) => (a.id === "director" ? { ...a, ...next } : a)));

  const tiles = [
    {
      label: "Diffs reviewed",
      value: num(stats?.reviews ?? 0),
      note: `across ${plural(projects.length, "project")}`,
    },
    {
      label: "Sent back",
      value: num(stats?.sent_back ?? 0),
      note: stats?.sent_back
        ? `about 1 diff in ${Math.max(2, Math.round((stats.reviews || 1) / stats.sent_back))}`
        : "nothing yet",
    },
    { label: "Runs of its own", value: num(stats?.runs ?? 0), note: "reviews and chats" },
    {
      label: "Spent reviewing",
      value: money(stats?.spend ?? 0),
      note: `${money(stats?.avg_cost ?? 0, 3)} a run`,
    },
  ];

  const decisions = activity
    .filter((a) => a.kind === "review" || a.label.startsWith("Card"))
    .slice(0, 7);

  const policy = [
    {
      key: "director_reviews_first" as const,
      name: "Read every diff first",
      note: "Nothing reaches you until the Director has read it",
      on: settings.director_reviews_first,
    },
    {
      key: "commit_wip_on_close" as const,
      name: "Commit on close",
      note: "Waits for running agents to commit before quitting",
      on: settings.commit_wip_on_close,
    },
  ];

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>Director</h1>
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            padding: "3px 10px",
            borderRadius: 999,
            background: state.soft,
            color: state.fg,
            fontSize: 11,
            fontWeight: 700,
          }}
        >
          <span style={{ width: 5, height: 5, borderRadius: "50%", background: state.fg }} />
          {state.label}
        </span>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>
          Your main assistant, across every project. It answers, plans, and puts work on a board
          when you ask for it.
        </span>
      </div>

      <section
        style={{
          position: "relative",
          display: "flex",
          alignItems: "flex-start",
          gap: 16,
          padding: 20,
          marginBottom: 13,
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
        <span
          style={{
            position: "relative",
            width: 54,
            height: 54,
            flex: "none",
            borderRadius: "50%",
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
          {director.initial}
        </span>
        <div style={{ position: "relative", flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
            <span style={{ fontSize: 17, fontWeight: 800, letterSpacing: "-.02em" }}>
              {director.name}
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
              {MODELS.find((m) => m.id === director.model)?.name ?? "auto"}
            </span>
            <span style={{ fontSize: 11, color: "var(--text3)", fontFamily: "var(--mono)" }}>
              {director.permissions.join(" · ")}
            </span>
          </div>
          <p
            style={{
              margin: "8px 0 0",
              maxWidth: 620,
              fontSize: 12.5,
              color: "var(--text2)",
              lineHeight: 1.55,
            }}
          >
            {director.role}
          </p>
        </div>
        <div style={{ position: "relative", flex: "none", display: "flex", gap: 8 }}>
          <button
            type="button"
            className="hv-bright"
            onClick={openChat}
            style={{
              padding: "9px 16px",
              border: "none",
              borderRadius: 999,
              background: "var(--info)",
              color: "#fff",
              fontSize: 12.5,
              fontWeight: 700,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            Ask the Director
          </button>
          <button
            type="button"
            className="hv-soft"
            onClick={() => patch({ paused: !director.paused })}
            style={{
              padding: "9px 15px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: 12.5,
              fontWeight: 600,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            {director.paused ? "Resume" : "Pause"}
          </button>
        </div>
      </section>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(4,minmax(0,1fr))",
          gap: 13,
          marginBottom: 13,
        }}
      >
        {tiles.map((st) => (
          <div
            key={st.label}
            style={{
              padding: "16px 18px",
              border: "1px solid var(--line)",
              borderRadius: 20,
              background: "var(--surface)",
            }}
          >
            <div style={{ fontSize: 11.5, color: "var(--text3)", marginBottom: 7 }}>{st.label}</div>
            <div
              style={{
                fontSize: 24,
                fontWeight: 800,
                letterSpacing: "-.03em",
                lineHeight: 1,
                ...tabular,
              }}
            >
              {st.value}
            </div>
            <div style={{ marginTop: 7, fontSize: 11, color: "var(--text3)" }}>{st.note}</div>
          </div>
        ))}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1fr) 340px",
          gap: 13,
          alignItems: "start",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 13, minWidth: 0 }}>
          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 22,
              background: "var(--surface)",
              overflow: "hidden",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "15px 18px 13px" }}>
              <h2 style={{ margin: 0, fontSize: 13.5, fontWeight: 800, letterSpacing: "-.01em" }}>
                Across your projects
              </h2>
              <span style={{ flex: 1 }} />
              <span style={{ fontSize: 11.5, color: "var(--text3)" }}>
                What it is holding right now
              </span>
            </div>
            {projects.map((p) => {
              const pt = tone(p.tone);
              const doing = !p.exists
                ? "folder is missing"
                : p.paused
                  ? "paused with the project"
                  : p.stats.review
                    ? `${plural(p.stats.review, "diff")} read, waiting on you`
                    : p.stats.running
                      ? `watching ${plural(p.stats.running, "run")}`
                      : "nothing to review";
              const doingFg = !p.exists
                ? "var(--bad)"
                : p.stats.review
                  ? "var(--warn)"
                  : p.stats.running
                    ? "var(--accent)"
                    : "var(--text3)";
              return (
                <button
                  key={p.id}
                  type="button"
                  className="hv-hover"
                  onClick={() => {
                    selectProject(p.id);
                    go("board");
                  }}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 13,
                    width: "100%",
                    padding: "13px 18px",
                    border: "none",
                    borderTop: "1px solid var(--line2)",
                    background: "transparent",
                    color: "var(--text)",
                    cursor: "pointer",
                    textAlign: "left",
                    transition: "background .16s ease",
                  }}
                >
                  <span
                    style={{
                      width: 28,
                      height: 28,
                      flex: "none",
                      borderRadius: 9,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      background: pt.soft,
                      color: pt.color,
                      fontSize: 11.5,
                      fontWeight: 800,
                    }}
                  >
                    {p.glyph}
                  </span>
                  <span
                    style={{
                      flex: "none",
                      minWidth: 104,
                      fontFamily: "var(--mono)",
                      fontSize: 12.5,
                      fontWeight: 600,
                      ...truncate,
                    }}
                  >
                    {p.name}
                  </span>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontSize: 12.5,
                      fontWeight: 600,
                      color: doingFg,
                      ...truncate,
                    }}
                  >
                    {doing}
                  </span>
                  <span style={{ flex: "none", fontSize: 11.5, color: "var(--text3)" }}>
                    {plural(p.stats.cards, "card")}
                  </span>
                  <span style={{ flex: "none", fontSize: 12, color: "var(--text3)" }}>→</span>
                </button>
              );
            })}
            {projects.length === 0 && (
              <div
                style={{
                  padding: 20,
                  borderTop: "1px solid var(--line2)",
                  fontSize: 12.5,
                  color: "var(--text3)",
                  textAlign: "center",
                }}
              >
                No projects yet.
              </div>
            )}
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 22,
              background: "var(--surface)",
              overflow: "hidden",
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "15px 18px 13px" }}>
              <h2 style={{ margin: 0, fontSize: 13.5, fontWeight: 800, letterSpacing: "-.01em" }}>
                Recent decisions
              </h2>
              <span style={{ flex: 1 }} />
              <button
                type="button"
                className="hv-link"
                onClick={() => go("log")}
                style={{
                  background: "transparent",
                  border: "none",
                  color: "var(--text3)",
                  fontSize: 11.5,
                  fontWeight: 700,
                  cursor: "pointer",
                }}
              >
                Activity →
              </button>
            </div>
            {decisions.map((d) => (
              <div
                key={d.seq}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                  padding: "12px 18px",
                  borderTop: "1px solid var(--line2)",
                }}
              >
                <span
                  style={{
                    width: 7,
                    height: 7,
                    flex: "none",
                    borderRadius: "50%",
                    background: d.label.includes("Sent back")
                      ? "var(--warn)"
                      : d.label.includes("Approved")
                        ? "var(--ok)"
                        : "var(--info)",
                  }}
                />
                <span style={{ flex: "none", minWidth: 168, fontSize: 12.5, fontWeight: 600 }}>
                  {d.label}
                </span>
                <span
                  style={{ flex: 1, minWidth: 0, fontSize: 12, color: "var(--text2)", ...truncate }}
                >
                  {d.detail}
                </span>
                <span
                  style={{
                    flex: "none",
                    fontFamily: "var(--mono)",
                    fontSize: 11,
                    color: "var(--text3)",
                  }}
                >
                  {d.card_id}
                </span>
                <span style={{ flex: "none", fontSize: 11, color: "var(--text3)", ...tabular }}>
                  {clock(d.ts_ms)}
                </span>
              </div>
            ))}
            {decisions.length === 0 && (
              <div
                style={{
                  padding: 20,
                  borderTop: "1px solid var(--line2)",
                  fontSize: 12.5,
                  color: "var(--text3)",
                  textAlign: "center",
                }}
              >
                No decisions logged yet.
              </div>
            )}
          </section>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 13 }}>
          <section
            style={{
              padding: "16px 18px 18px",
              border: "1px solid var(--line)",
              borderRadius: 22,
              background: "var(--surface)",
            }}
          >
            <div style={{ fontSize: 13, fontWeight: 800, letterSpacing: "-.01em", marginBottom: 4 }}>
              Standing instructions
            </div>
            <div style={{ fontSize: 11.5, color: "var(--text3)", marginBottom: 11 }}>
              Applies in every project, on every run.
            </div>
            <textarea
              rows={7}
              value={director.brief}
              onChange={(e) => patch({ brief: e.target.value })}
              style={{
                width: "100%",
                boxSizing: "border-box",
                padding: "12px 13px",
                border: "1px solid var(--line)",
                borderRadius: 14,
                background: "var(--recess)",
                color: "var(--text)",
                fontSize: 12.5,
                lineHeight: 1.6,
                fontFamily: "inherit",
                resize: "none",
                outline: "none",
              }}
            />
          </section>

          <section
            style={{
              border: "1px solid var(--line)",
              borderRadius: 22,
              background: "var(--surface)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                padding: "15px 18px 13px",
                fontSize: 13,
                fontWeight: 800,
                letterSpacing: "-.01em",
              }}
            >
              Policy
            </div>
            {policy.map((pl) => (
              <div
                key={pl.key}
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 14,
                  padding: "13px 18px",
                  borderTop: "1px solid var(--line2)",
                }}
              >
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "block", fontSize: 12.5, fontWeight: 600 }}>{pl.name}</span>
                  <span
                    style={{
                      display: "block",
                      marginTop: 3,
                      fontSize: 11,
                      color: "var(--text3)",
                      lineHeight: 1.5,
                    }}
                  >
                    {pl.note}
                  </span>
                </span>
                <span style={{ marginTop: 2 }}>
                  <Switch
                    on={pl.on}
                    onChange={(v) => saveSettings({ [pl.key]: v })}
                    label={pl.name}
                  />
                </span>
              </div>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}

export function Worktrees() {
  const { projectId, project, snapshot, toast } = useStore();
  const [rows, setRows] = useState<WorktreeRow[] | null>(null);

  const load = () => {
    if (!projectId) return;
    api
      .worktrees(projectId)
      .then(setRows)
      .catch((e) => toast("var(--bad)", "Could not list worktrees", reason(e)));
  };

  useEffect(load, [projectId]);

  if (!project) {
    return (
      <div style={{ padding: "22px 26px", fontSize: 12.5, color: "var(--text3)" }}>
        Add a git repository first.
      </div>
    );
  }
  if (!rows) return <Loading what="Listing worktrees" />;

  const grid = "1.5fr 1fr 90px 1.4fr 150px";
  const cardFor = (branch: string | null) => {
    const id = branch?.split("/").slice(-1)[0] ?? "";
    return snapshot?.cards.find((c) => c.id === id) ?? null;
  };

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <h1 style={{ margin: "0 0 5px", fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>
        Worktrees
      </h1>
      <p style={{ margin: "0 0 16px", fontSize: 13, color: "var(--text2)" }}>
        One branch per card, created under app data. Finished runs commit themselves and leave a
        trailer pointing back at the card.
      </p>
      <div
        style={{
          border: "1px solid var(--line)",
          borderRadius: 18,
          overflow: "hidden",
          background: "var(--surface)",
        }}
      >
        <div
          style={{
            display: "grid",
            gridTemplateColumns: grid,
            gap: 14,
            padding: "12px 18px",
            borderBottom: "1px solid var(--line)",
            fontSize: 11,
            fontWeight: 700,
            letterSpacing: ".09em",
            textTransform: "uppercase",
            color: "var(--text3)",
          }}
        >
          <span>Branch</span>
          <span>Card</span>
          <span>State</span>
          <span>Path</span>
          <span />
        </div>
        {rows.map((w) => {
          const card = cardFor(w.branch);
          const st = w.dirty
            ? { label: "dirty", fg: "var(--accent)", soft: "var(--accentSoft)" }
            : { label: "clean", fg: "var(--ok)", soft: "var(--okSoft)" };
          return (
            <div
              key={w.path}
              className="hv-row"
              style={{
                display: "grid",
                gridTemplateColumns: grid,
                gap: 14,
                alignItems: "center",
                padding: "12px 18px",
                borderBottom: "1px solid var(--line2)",
                transition: "background .18s ease",
              }}
            >
              <span style={{ fontFamily: "var(--mono)", fontSize: 12, fontWeight: 500, ...truncate }}>
                {w.branch ?? "(detached)"}
              </span>
              <span
                title={card?.title}
                style={{
                  fontFamily: "var(--mono)",
                  fontSize: 11.5,
                  color: "var(--text3)",
                  ...truncate,
                }}
              >
                {card?.id ?? "—"}
              </span>
              <span
                style={{
                  fontSize: 11.5,
                  fontWeight: 700,
                  padding: "3px 10px",
                  borderRadius: 999,
                  justifySelf: "start",
                  background: st.soft,
                  color: st.fg,
                }}
              >
                {st.label}
              </span>
              <span title={w.path} style={{ fontSize: 12.5, color: "var(--text2)", ...truncate }}>
                {w.path}
              </span>
              <span style={{ justifySelf: "end", display: "flex", gap: 6 }}>
                <button
                  type="button"
                  className="hv-soft"
                  onClick={() => api.reveal(w.path).catch(() => {})}
                  style={{
                    padding: "6px 13px",
                    border: "1px solid var(--line)",
                    borderRadius: 999,
                    background: "transparent",
                    color: "var(--text2)",
                    fontSize: 11.5,
                    fontWeight: 600,
                    cursor: "pointer",
                    transition: "all .18s ease",
                  }}
                >
                  Open
                </button>
                <button
                  type="button"
                  className="hv-danger"
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .removeWorktree(projectId, w.path)
                      .then(() => {
                        toast("var(--ok)", "Removed", w.branch ?? w.path);
                        load();
                      })
                      .catch((e) => toast("var(--bad)", "Could not remove it", reason(e)));
                  }}
                  style={{
                    padding: "6px 13px",
                    border: "1px solid var(--line)",
                    borderRadius: 999,
                    background: "transparent",
                    color: "var(--text3)",
                    fontSize: 11.5,
                    fontWeight: 600,
                    cursor: "pointer",
                    transition: "all .18s ease",
                  }}
                >
                  Drop
                </button>
              </span>
            </div>
          );
        })}
        {rows.length === 0 && (
          <div
            style={{ padding: "22px 18px", textAlign: "center", fontSize: 12.5, color: "var(--text3)" }}
          >
            No worktrees in this project. Agents open one the moment a card starts.
          </div>
        )}
      </div>
    </div>
  );
}

const FILTERS = ["All", "Cards", "Runs", "Reviews"] as const;

export function Activity({ openRun }: { openRun: (cardId: string) => void }) {
  const { activity, snapshot, project } = useStore();
  const [filter, setFilter] = useState<(typeof FILTERS)[number]>("All");

  if (!project) {
    return (
      <div style={{ padding: "22px 26px", fontSize: 12.5, color: "var(--text3)" }}>
        Add a git repository first.
      </div>
    );
  }

  const rows = activity.filter((r) =>
    filter === "All"
      ? true
      : filter === "Cards"
        ? r.kind === "card"
        : filter === "Runs"
          ? r.kind === "run"
          : r.kind === "review" || r.kind === "approval",
  );

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 14 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>Activity</h1>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>
          Every event in this project, newest first
        </span>
        <div style={{ flex: 1 }} />
        <div
          style={{
            display: "flex",
            gap: 2,
            padding: 3,
            borderRadius: 999,
            background: "var(--surface)",
            border: "1px solid var(--line)",
          }}
        >
          {FILTERS.map((f) => {
            const on = filter === f;
            return (
              <button
                key={f}
                type="button"
                onClick={() => setFilter(f)}
                style={{
                  padding: "7px 15px",
                  border: "none",
                  borderRadius: 999,
                  fontSize: 12,
                  cursor: "pointer",
                  transition: "all .18s ease",
                  background: on ? "var(--accent)" : "transparent",
                  color: on ? "var(--onAccent)" : "var(--text2)",
                  fontWeight: on ? 700 : 500,
                }}
              >
                {f}
              </button>
            );
          })}
        </div>
      </div>

      <div
        style={{
          border: "1px solid var(--line)",
          borderRadius: 18,
          overflow: "hidden",
          background: "var(--surface)",
        }}
      >
        {rows.map((e) => (
          <button
            key={e.seq}
            type="button"
            className="hv-row"
            onClick={() => openRun(e.card_id)}
            style={{
              display: "grid",
              gridTemplateColumns: "14px 190px 74px 1fr 60px",
              gap: 14,
              alignItems: "center",
              width: "100%",
              padding: "11px 18px",
              border: "none",
              borderBottom: "1px solid var(--line2)",
              background: "transparent",
              color: "var(--text)",
              fontSize: 12.5,
              textAlign: "left",
              cursor: "pointer",
              animation: "fadeIn .25s ease both",
              transition: "background .18s ease",
            }}
          >
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                background:
                  e.kind === "run"
                    ? "var(--accent)"
                    : e.kind === "approval"
                      ? "var(--warn)"
                      : e.kind === "review"
                        ? "var(--ok)"
                        : "var(--info)",
              }}
            />
            <span style={{ fontWeight: 600, ...truncate }}>{e.label}</span>
            <span style={{ fontFamily: "var(--mono)", fontSize: 11.5, color: "var(--text3)" }}>
              {e.card_id}
            </span>
            <span style={{ color: "var(--text2)", ...truncate }}>
              {e.detail || snapshot?.cards.find((c) => c.id === e.card_id)?.title || ""}
            </span>
            <span style={{ fontSize: 11.5, color: "var(--text3)", textAlign: "right", ...tabular }}>
              {clock(e.ts_ms)}
            </span>
          </button>
        ))}
        {rows.length === 0 && (
          <div
            style={{ padding: "22px 18px", textAlign: "center", fontSize: 12.5, color: "var(--text3)" }}
          >
            Nothing logged yet.
          </div>
        )}
      </div>
    </div>
  );
}

function Row({
  name,
  note,
  children,
  last,
}: {
  name: string;
  note: string;
  children: ReactNode;
  last?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 22,
        padding: "17px 18px",
        borderBottom: last ? "none" : "1px solid var(--line2)",
      }}
    >
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 13.5, fontWeight: 700, marginBottom: 3 }}>{name}</div>
        <div style={{ fontSize: 12, color: "var(--text3)", lineHeight: 1.5 }}>{note}</div>
      </div>
      <div style={{ flex: "none" }}>{children}</div>
    </div>
  );
}

export function Settings() {
  const { settings, status, dataDir, saveSettings, installSidecar, toast } = useStore();
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    let un: (() => void) | null = null;
    events.onSidecarLog((line) => setLog((l) => [...l, line].slice(-6))).then((u) => {
      un = u;
    });
    return () => un?.();
  }, []);

  if (!settings) return <Loading what="Reading settings" />;

  const card = {
    border: "1px solid var(--line)",
    borderRadius: 18,
    background: "var(--surface)",
    overflow: "hidden" as const,
    marginBottom: 12,
  };

  const pillRow = (options: string[], value: string, pick: (v: string) => void, wide?: boolean) => (
    <div
      style={{
        display: "flex",
        gap: 2,
        padding: 3,
        borderRadius: 999,
        background: "var(--surface2)",
        border: "1px solid var(--line)",
      }}
    >
      {options.map((o) => {
        const on = value === o;
        return (
          <button
            key={o}
            type="button"
            onClick={() => pick(o)}
            style={{
              padding: wide ? "8px 18px" : "8px 13px",
              border: "none",
              borderRadius: 999,
              fontSize: 12.5,
              cursor: "pointer",
              transition: "all .18s ease",
              background: on ? "var(--accent)" : "transparent",
              color: on ? "var(--onAccent)" : "var(--text2)",
              fontWeight: on ? 700 : 500,
            }}
          >
            {o}
          </button>
        );
      })}
    </div>
  );

  return (
    <div style={{ padding: "22px 26px 28px", maxWidth: 880 }}>
      <h1 style={{ margin: "0 0 5px", fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>
        Settings
      </h1>
      <p style={{ margin: "0 0 16px", fontSize: 13, color: "var(--text2)" }}>
        Applies to new runs. Anything already running keeps the profile it started with.
      </p>

      <div style={card}>
        <Row name="Appearance" note="Light by day, dark for late sessions" last>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            {["#6b5cf6", "#0fa47f", "#3b7ff0", "#e0455f"].map((c) => (
              <button
                key={c}
                type="button"
                aria-label={`accent ${c}`}
                onClick={() => saveSettings({ accent: c })}
                style={{
                  width: 20,
                  height: 20,
                  borderRadius: "50%",
                  background: c,
                  border: settings.accent === c ? "2px solid var(--text)" : "1px solid var(--line)",
                  cursor: "pointer",
                }}
              />
            ))}
            {pillRow(["light", "dark"], settings.theme, (v) => saveSettings({ theme: v }), true)}
          </div>
        </Row>
      </div>

      <div style={card}>
        <Row
          name="Node sidecar"
          note="Runs agents through the Claude Agent SDK. Off falls back to the claude command line."
        >
          <Switch
            on={settings.sidecar}
            onChange={(v) => saveSettings({ sidecar: v })}
            label="Node sidecar"
          />
        </Row>
        <Row name="Director reviews first" note="Reads every finished diff before it reaches you">
          <Switch
            on={settings.director_reviews_first}
            onChange={(v) => saveSettings({ director_reviews_first: v })}
            label="Director reviews first"
          />
        </Row>
        <Row
          name="Commit on close"
          note="Waits for running agents to commit work in progress before quitting"
        >
          <Switch
            on={settings.commit_wip_on_close}
            onChange={(v) => saveSettings({ commit_wip_on_close: v })}
            label="Commit on close"
          />
        </Row>
        <Row name="Permission mode" note="The default for new runs; an agent profile can override it">
          {pillRow(["acceptEdits", "manual", "dontAsk", "plan"], settings.permission_mode, (v) =>
            saveSettings({ permission_mode: v }),
          )}
        </Row>
        <Row name="Daily budget" note="Across every agent in this workspace" last>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <button
              type="button"
              className="hv-soft"
              onClick={() =>
                saveSettings({ daily_budget_usd: Math.max(0, settings.daily_budget_usd - 1) })
              }
              style={{
                width: 30,
                height: 30,
                border: "1px solid var(--line)",
                borderRadius: 10,
                background: "transparent",
                color: "var(--text2)",
                cursor: "pointer",
              }}
            >
              −
            </button>
            <span
              style={{
                fontSize: 19,
                fontWeight: 800,
                minWidth: 66,
                textAlign: "center",
                ...tabular,
              }}
            >
              {money(settings.daily_budget_usd)}
            </span>
            <button
              type="button"
              className="hv-soft"
              onClick={() => saveSettings({ daily_budget_usd: settings.daily_budget_usd + 1 })}
              style={{
                width: 30,
                height: 30,
                border: "1px solid var(--line)",
                borderRadius: 10,
                background: "transparent",
                color: "var(--text2)",
                cursor: "pointer",
              }}
            >
              +
            </button>
          </div>
        </Row>
      </div>

      <div style={card}>
        <Row
          name="Claude"
          note={
            status?.claude.logged_in
              ? `logged in${status.claude.cli_version ? ` · claude ${status.claude.cli_version}` : ""}`
              : "not logged in — open a terminal and run /login"
          }
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                background: status?.claude.logged_in ? "var(--ok)" : "var(--bad)",
              }}
            />
            <button
              type="button"
              className="hv-text"
              onClick={() => api.openClaudeTerminal().catch(() => {})}
              style={{
                padding: "8px 15px",
                border: "1px solid var(--line)",
                borderRadius: 999,
                background: "transparent",
                color: "var(--text2)",
                fontSize: 12.5,
                fontWeight: 600,
                cursor: "pointer",
                transition: "all .18s ease",
              }}
            >
              Open a terminal
            </button>
          </div>
        </Row>
        <Row
          name="Sidecar"
          note={`${status?.sidecar.ready ? "ready" : "dependencies missing"} · node ${
            status?.sidecar.node_version ?? "not found"
          }${status?.sidecar.development ? " · running from the checkout" : ""}`}
          last
        >
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: "50%",
                background: status?.sidecar.ready ? "var(--ok)" : "var(--warn)",
              }}
            />
            {!status?.sidecar.ready && (
              <button
                type="button"
                className="hv-bright"
                onClick={installSidecar}
                style={{
                  padding: "8px 15px",
                  border: "none",
                  borderRadius: 999,
                  background: "var(--accent)",
                  color: "var(--onAccent)",
                  fontSize: 12.5,
                  fontWeight: 700,
                  cursor: "pointer",
                }}
              >
                Install
              </button>
            )}
          </div>
        </Row>
      </div>

      {settings.always_allow.length > 0 && (
        <div style={card}>
          <Row
            name="Standing allowances"
            note="Calls Harness stops asking about. Each one is scoped to the command it came from, so allowing git push does not allow every shell command. Click one to take it back."
            last
          >
            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, justifyContent: "flex-end" }}>
              {settings.always_allow.map((rule) => {
                const label = ruleLabel(rule);
                // An unscoped shell rule from an older build. It authorises
                // nothing now; it is shown so it can be seen and removed.
                const revoked = ruleIsRevoked(rule);
                return (
                  <button
                    key={label}
                    type="button"
                    className="hv-danger"
                    title={
                      revoked
                        ? "This allowed every command, so it no longer allows any. Approve once more to record a scoped rule."
                        : "Stop allowing this"
                    }
                    onClick={() =>
                      saveSettings({
                        always_allow: settings.always_allow.filter(
                          (x) => ruleLabel(x) !== label,
                        ),
                      })
                    }
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                      padding: "5px 11px",
                      border: "1px solid var(--line)",
                      borderRadius: 999,
                      background: "transparent",
                      color: revoked ? "var(--text3)" : "var(--text2)",
                      fontSize: 11.5,
                      fontWeight: 600,
                      cursor: "pointer",
                      fontFamily: "var(--mono)",
                      textDecoration: revoked ? "line-through" : "none",
                    }}
                  >
                    {label}
                    {revoked && (
                      <span
                        style={{
                          fontFamily: "var(--sans)",
                          fontSize: 10,
                          fontWeight: 700,
                          color: "var(--warn)",
                          textDecoration: "none",
                        }}
                      >
                        revoked
                      </span>
                    )}
                    <span>&#10005;</span>
                  </button>
                );
              })}
            </div>
          </Row>
        </div>
      )}

      <div style={{ ...card, marginBottom: 0 }}>
        <div style={{ padding: "17px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
          <div
            style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 14 }}
          >
            <span style={{ fontSize: 13 }}>Where everything is written</span>
            <span
              title={dataDir}
              style={{
                fontSize: 12,
                color: "var(--text3)",
                fontFamily: "var(--mono)",
                maxWidth: 460,
                ...truncate,
              }}
            >
              {dataDir}
            </span>
          </div>
          <div style={{ fontSize: 11.5, color: "var(--text3)", lineHeight: 1.6 }}>
            Event logs, run transcripts, agent profiles and worktrees all live there — never inside
            the repositories you point Harness at.
          </div>
          <button
            type="button"
            className="hv-text"
            onClick={() =>
              api.reveal(dataDir).catch((e) => toast("var(--bad)", "Could not open it", reason(e)))
            }
            style={{
              alignSelf: "flex-start",
              marginTop: 5,
              padding: "8px 15px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: 12.5,
              fontWeight: 600,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            Show files
          </button>
          {log.length > 0 && (
            <pre
              style={{
                margin: 0,
                padding: "10px 12px",
                borderRadius: 12,
                background: "var(--surface2)",
                border: "1px solid var(--line)",
                fontFamily: "var(--mono)",
                fontSize: 11,
                whiteSpace: "pre-wrap",
                color: "var(--text2)",
              }}
            >
              {log.join("\n")}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}
