import { useEffect, useRef, useState, type ReactNode } from "react";
import { money, plural } from "../lib/format";
import { tone } from "../lib/types";
import { useStore } from "../state/store";
import type { View } from "../views/views";
import { Icon, Meter, truncate } from "./ui";

/** Project picker at the top of the rail, with the popover list. */
function Switcher() {
  const { projects, project, selectProject, addProject } = useStore();
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [open]);

  const t = tone(project?.tone);
  const meta = !project
    ? "no project yet"
    : !project.exists
      ? "folder is missing"
      : project.stats.running
        ? `${plural(project.stats.running, "run")} live`
        : project.stats.review
          ? `${plural(project.stats.review, "diff")} waiting`
          : project.paused
            ? "paused"
            : "idle";

  return (
    <div style={{ position: "relative", padding: "0 12px 14px", zIndex: 30 }} ref={box}>
      <button
        type="button"
        className="hv-pill"
        onClick={() => setOpen((v) => !v)}
        style={{
          width: "100%",
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "8px 10px",
          border: "1px solid var(--line)",
          borderRadius: 14,
          background: "var(--surface2)",
          color: "var(--text)",
          cursor: "pointer",
          textAlign: "left",
          transition: "all .18s ease",
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
            background: t.soft,
            color: t.color,
            fontSize: 12,
            fontWeight: 800,
          }}
        >
          {project?.glyph ?? "+"}
        </span>
        <span style={{ flex: 1, minWidth: 0 }}>
          <span
            style={{
              display: "block",
              fontSize: 12.5,
              fontWeight: 700,
              letterSpacing: "-.01em",
              ...truncate,
            }}
          >
            {project?.name ?? "Add a project"}
          </span>
          <span
            style={{ display: "block", marginTop: 1, fontSize: 10.5, color: "var(--text3)", ...truncate }}
          >
            {meta}
          </span>
        </span>
        <Icon.chevron />
      </button>

      {open && (
        <div
          style={{
            position: "absolute",
            top: "calc(100% - 6px)",
            left: 12,
            right: 12,
            background: "var(--elev)",
            border: "1px solid var(--line)",
            borderRadius: 16,
            boxShadow: "var(--shadow)",
            padding: 6,
            animation: "popIn .16s ease both",
            maxHeight: 360,
            overflowY: "auto",
          }}
        >
          {projects.length === 0 && (
            <div style={{ padding: "10px 9px", fontSize: 11.5, color: "var(--text3)", lineHeight: 1.5 }}>
              Point Harness at a git repository to start.
            </div>
          )}
          {projects.map((p) => {
            const pt = tone(p.tone);
            const on = p.id === project?.id;
            return (
              <button
                key={p.id}
                type="button"
                className="hv-hover"
                onClick={() => {
                  selectProject(p.id);
                  setOpen(false);
                }}
                style={{
                  width: "100%",
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "8px 9px",
                  border: "none",
                  borderRadius: 11,
                  background: on ? "var(--accentSoft)" : "transparent",
                  color: "var(--text)",
                  cursor: "pointer",
                  textAlign: "left",
                }}
              >
                <span
                  style={{
                    width: 24,
                    height: 24,
                    flex: "none",
                    borderRadius: 8,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    background: pt.soft,
                    color: pt.color,
                    fontSize: 10.5,
                    fontWeight: 800,
                  }}
                >
                  {p.glyph}
                </span>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "block", fontSize: 12.5, fontWeight: 700, ...truncate }}>
                    {p.name}
                  </span>
                  <span
                    style={{
                      display: "block",
                      fontSize: 10.5,
                      color: !p.exists
                        ? "var(--bad)"
                        : p.stats.running
                          ? "var(--accent)"
                          : p.stats.review
                            ? "var(--warn)"
                            : "var(--text3)",
                    }}
                  >
                    {!p.exists
                      ? "folder is missing"
                      : p.stats.running
                        ? `${p.stats.running} live`
                        : p.stats.review
                          ? `${p.stats.review} waiting`
                          : p.paused
                            ? "paused"
                            : "idle"}
                  </span>
                </span>
                <span style={{ opacity: on ? 1 : 0, color: "var(--accent)", fontSize: 12 }}>✓</span>
              </button>
            );
          })}
          <div style={{ height: 1, background: "var(--line)", margin: "6px 4px" }} />
          <button
            type="button"
            className="hv-hover"
            onClick={() => {
              setOpen(false);
              addProject();
            }}
            style={{
              width: "100%",
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "8px 9px",
              border: "none",
              borderRadius: 11,
              background: "transparent",
              color: "var(--text2)",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <span
              style={{
                width: 24,
                height: 24,
                flex: "none",
                borderRadius: 8,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                background: "var(--surface2)",
                color: "var(--text2)",
              }}
            >
              <Icon.plus />
            </span>
            <span style={{ fontSize: 12.5, fontWeight: 600 }}>Add a project…</span>
          </button>
        </div>
      )}
    </div>
  );
}

function SectionLabel({ children, top }: { children: ReactNode; top?: number }) {
  return (
    <div
      style={{
        marginTop: top,
        padding: "0 20px 8px",
        fontSize: 10.5,
        fontWeight: 700,
        letterSpacing: ".12em",
        textTransform: "uppercase",
        color: "var(--text3)",
      }}
    >
      {children}
    </div>
  );
}

export function NavRail({ view, go }: { view: View; go: (v: View) => void }) {
  const { snapshot, agents, settings, stats, project, projects } = useStore();
  const cards = snapshot?.cards ?? [];
  const running = cards.filter((c) => c.status === "running").length;
  const director = agents.find((a) => a.id === "director");
  const dt = tone(director?.tone ?? "info");

  const on = (v: View) =>
    view === v ||
    (v === "agents" && view === "agent") ||
    (v === "projects" && view === "project");

  const item = (v: View, label: string, icon: ReactNode, right?: ReactNode) => {
    const active = on(v);
    return (
      <button
        key={v}
        type="button"
        className={active ? undefined : "hv-hover"}
        onClick={() => go(v)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 11,
          height: 38,
          padding: "0 13px",
          border: "none",
          borderRadius: 11,
          cursor: "pointer",
          textAlign: "left",
          fontSize: 13.5,
          transition: "all .18s ease",
          background: active ? "var(--accentSoft)" : "transparent",
          color: active ? "var(--accent)" : "var(--text2)",
          fontWeight: active ? 700 : 500,
          boxShadow: active ? "inset 0 0 0 1px var(--accentLine)" : "none",
        }}
      >
        {icon}
        <span style={{ flex: 1 }}>{label}</span>
        {right}
      </button>
    );
  };

  const count = (n: number) =>
    n > 0 ? <span style={{ fontSize: 11.5, opacity: 0.75 }}>{n}</span> : undefined;

  const spendToday = stats?.spend_today ?? 0;
  const budget = settings?.daily_budget_usd ?? 5;

  return (
    <nav
      style={{
        width: 224,
        flex: "none",
        display: "flex",
        flexDirection: "column",
        background: "var(--surface)",
        borderRight: "1px solid var(--line)",
        padding: "16px 0 14px",
        minHeight: 0,
      }}
    >
      <Switcher />

      <div style={{ padding: "0 12px 14px" }}>
        <button
          type="button"
          className={on("director") ? undefined : "hv-hover"}
          onClick={() => go("director")}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 11,
            width: "100%",
            height: 42,
            padding: "0 12px",
            border: "none",
            borderRadius: 12,
            cursor: "pointer",
            textAlign: "left",
            fontSize: 13.5,
            transition: "all .18s ease",
            background: on("director") ? "var(--accentSoft)" : "transparent",
            color: on("director") ? "var(--accent)" : "var(--text2)",
            fontWeight: on("director") ? 700 : 500,
            boxShadow: on("director") ? "inset 0 0 0 1px var(--accentLine)" : "none",
          }}
        >
          <span
            style={{
              width: 26,
              height: 26,
              flex: "none",
              borderRadius: "50%",
              background: dt.soft,
              color: dt.color,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 11.5,
              fontWeight: 800,
            }}
          >
            {director?.initial ?? "D"}
          </span>
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 13, fontWeight: 700, letterSpacing: "-.01em" }}>
              Director
            </span>
            <span style={{ display: "block", fontSize: 10.5, color: "var(--text3)" }}>
              {director?.paused ? "paused" : "watching"} · all projects
            </span>
          </span>
        </button>
      </div>

      <SectionLabel>This project</SectionLabel>
      <div style={{ display: "flex", flexDirection: "column", gap: 2, padding: "0 12px" }}>
        {item("home", "Home", <Icon.home />)}
        {item(
          "project",
          "Code",
          <Icon.code />,
          project && (
            <span style={{ fontSize: 11, opacity: 0.7, fontFamily: "var(--mono)" }}>
              {project.base_branch}
            </span>
          ),
        )}
        {item("agents", "Agents", <Icon.agents />, count(agents.length))}
        {item("board", "Work", <Icon.board />, count(cards.filter((c) => c.status !== "done").length))}
        {item(
          "runs",
          "Sessions",
          <Icon.runs />,
          running > 0 ? (
            <span
              style={{
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: "var(--accent)",
                animation: "breathe 2.2s ease-in-out infinite",
              }}
            />
          ) : undefined,
        )}
      </div>

      <SectionLabel top={16}>Records</SectionLabel>
      <div style={{ display: "flex", flexDirection: "column", gap: 2, padding: "0 12px" }}>
        {item("trees", "Worktrees", <Icon.trees />)}
        {item("log", "Activity", <Icon.log />)}
        {item("projects", "Projects", <Icon.folder />, count(projects.length))}
      </div>

      <div style={{ flex: 1 }} />

      <div
        style={{
          margin: "0 12px 10px",
          padding: 14,
          borderRadius: 16,
          background: "var(--surface2)",
          border: "1px solid var(--line)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: 9,
          }}
        >
          <span style={{ fontSize: 11.5, color: "var(--text2)", fontWeight: 500 }}>Today</span>
          <span style={{ fontSize: 13, fontWeight: 700, fontVariantNumeric: "tabular-nums" }}>
            {money(spendToday)}
          </span>
        </div>
        <Meter
          pct={(spendToday / Math.max(0.01, budget)) * 100}
          color={spendToday > budget ? "var(--bad)" : "var(--accent)"}
        />
        <div style={{ marginTop: 7, fontSize: 11, color: "var(--text3)" }}>
          of {money(budget)} daily budget
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 2, padding: "0 12px" }}>
        {item("settings", "Settings", <Icon.gear />)}
      </div>
    </nav>
  );
}
