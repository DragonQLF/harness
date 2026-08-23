import { useMemo, useState } from "react";
import {
  duration,
  greeting,
  money,
  num,
  plural,
  today,
  truncate as cut,
  weekLetters,
} from "../lib/format";
import { tone } from "../lib/types";
import type { View } from "./views";
import { useStore } from "../state/store";
import {
  Avatar,
  Card,
  CardHead,
  EmptyNote,
  HeadLink,
  Loading,
  PageHead,
  QuietButton,
  StrongButton,
  WeekBars,
  tabular,
  truncate,
} from "../components/ui";

/** The board is empty until something is asked for; say what to do, not that
 *  there is nothing. */
function NoCardsYet({ onPick }: { onPick: (text: string) => void }) {
  const examples = [
    "Add a health check endpoint",
    "Write tests for the parser",
    "Explain how auth works in this repo",
  ];
  return (
    <div style={{ padding: "18px 17px 20px", borderTop: "1px solid var(--line2)" }}>
      <div style={{ fontSize: 12.5, color: "var(--text2)", lineHeight: 1.6 }}>
        Nothing on the board yet. Describe the first thing you want done in the field above — or
        start from one of these:
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 6, marginTop: 11 }}>
        {examples.map((e) => (
          <button
            key={e}
            type="button"
            className="hv-soft"
            onClick={() => onPick(e)}
            style={{
              padding: "6px 12px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: 11.5,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            {e}
          </button>
        ))}
      </div>
    </div>
  );
}
type Mode = "plan" | "start" | "later";

const MODES: { id: Mode; name: string }[] = [
  { id: "plan", name: "Plan" },
  { id: "start", name: "Start" },
  { id: "later", name: "Later" },
];

export function Overview({
  go,
  openRun,
  openAgent,
  openReject,
  openApprovals,
}: {
  go: (v: View) => void;
  openRun: (cardId: string) => void;
  openAgent: (id: string) => void;
  openReject: (cardId: string) => void;
  openApprovals: () => void;
}) {
  const {
    snapshot,
    stats,
    project,
    settings,
    agents,
    agentStats,
    approvals,
    outputs,
    createCard,
    approve,
    answerApproval,
    cancelRun,
  } = useStore();

  const [intent, setIntent] = useState("");
  const [mode, setMode] = useState<Mode>("plan");
  const workers = agents.filter((a) => a.id !== "director");
  const [agentId, setAgentId] = useState(workers[0]?.id ?? "builder");

  const code = useMemo(() => {
    const totals = [0, 0, 0, 0, 0, 0, 0];
    let added = 0;
    let removed = 0;
    Object.values(agentStats).forEach((s) => {
      s.week_runs.forEach((v, i) => {
        totals[i] = (totals[i] ?? 0) + v;
      });
      added += s.lines_added;
      removed += s.lines_removed;
    });
    return { bars: totals, added, removed };
  }, [agentStats]);

  // The shell shows the first-run screen when there is no project, so by the
  // time this renders both are present.
  if (!snapshot || !stats || !project) return <Loading what="Reading the board" />;

  const cards = snapshot.cards;
  const running = cards.filter((c) => c.status === "running");
  const inReview = cards.filter((c) => c.status === "review");
  const budget = settings?.daily_budget_usd ?? 5;
  const needsCount = approvals.length + inReview.length;
  const firstName = (settings?.user_name ?? "Operator").split(/\s+/)[0];

  const homeSub = running.length
    ? `The agents are working on ${running.length === 1 ? "one card" : `${running.length} cards`} in ${project.name}.` +
      (needsCount ? ` ${plural(needsCount, "thing")} need${needsCount === 1 ? "s" : ""} you when you have a minute.` : "")
    : needsCount
      ? `${plural(needsCount, "thing")} ${needsCount === 1 ? "is" : "are"} waiting on you. Everything else is quiet.`
      : "Nothing running, nothing waiting.";

  const tiles = [
    {
      key: "w",
      label: "Working now",
      value: stats.running,
      delta: stats.running ? "live" : "idle",
      deltaColor: stats.running ? "var(--accent)" : "var(--text3)",
      deltaSoft: stats.running ? "var(--accentSoft)" : "var(--surface2)",
      note: plural(snapshot.sessions.filter((s) => s.live).length, "worktree open"),
      go: () => go("runs"),
    },
    {
      key: "r",
      label: "For your review",
      value: stats.review,
      delta: stats.review ? "waiting" : "clear",
      deltaColor: stats.review ? "var(--warn)" : "var(--ok)",
      deltaSoft: stats.review ? "var(--warnSoft)" : "var(--okSoft)",
      note: `in ${project.name}`,
      go: () => go("board"),
    },
    {
      key: "d",
      label: "Cards done",
      value: stats.done,
      delta: stats.done_today ? `+${stats.done_today}` : "—",
      deltaColor: "var(--ok)",
      deltaSoft: "var(--okSoft)",
      note: `${stats.done_today} today`,
      go: () => go("board"),
    },
    {
      key: "c",
      label: "Cost per card",
      value: money(stats.cost_per_card),
      delta: `${plural(stats.runs_today, "run")}`,
      deltaColor: "var(--text3)",
      deltaSoft: "var(--surface2)",
      note: `${money(stats.spend_total)} all time`,
      go: () => go("log"),
    },
  ];

  const attention = [
    ...approvals.map((a) => ({
      key: a.request_id,
      mark: "!",
      accent: "var(--warn)",
      soft: "var(--warnSoft)",
      title: `An agent wants to use ${a.tool}`,
      note: a.summary || "Outside its permissions, so it stopped and asked",
      primaryLabel: "Review",
      primary: openApprovals,
      secondaryLabel: "Allow",
      secondary: () => answerApproval(a.request_id, true, false),
    })),
    ...inReview.map((c) => {
      const agent = agents.find((x) => x.id === c.agent_id);
      return {
        key: c.id,
        mark: agent?.initial ?? "?",
        accent: "var(--ok)",
        soft: "var(--okSoft)",
        title: c.title,
        note: c.last_review
          ? `${c.last_review.by === "director" ? "Director" : "You"}: ${c.last_review.reason}`
          : `${money(c.cost_usd, 4)} · ${plural(c.turns, "turn")}`,
        primaryLabel: "Approve",
        primary: () => approve(c.id),
        secondaryLabel: "Send back",
        secondary: () => openReject(c.id),
      };
    }),
  ];

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <PageHead
        title="Overview"
        crumb={project.name}
        right={<span style={{ fontSize: 12.5, color: "var(--text3)" }}>{today()}</span>}
      />

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(4,minmax(0,1fr))",
          gap: 12,
          animation: "fadeUp .45s ease both",
        }}
      >
        {tiles.map((k) => (
          <button
            key={k.key}
            type="button"
            className="hv-tile"
            onClick={k.go}
            style={{
              display: "flex",
              flexDirection: "column",
              padding: "16px 17px",
              border: "1px solid var(--line)",
              borderRadius: 18,
              background: "var(--surface)",
              cursor: "pointer",
              textAlign: "left",
              transition: "all .2s cubic-bezier(.2,.8,.2,1)",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                width: "100%",
                gap: 10,
              }}
            >
              <span style={{ fontSize: 13.5, fontWeight: 600 }}>{k.label}</span>
              <span
                style={{
                  padding: "3px 9px",
                  borderRadius: 999,
                  background: k.deltaSoft,
                  color: k.deltaColor,
                  fontSize: 11.5,
                  fontWeight: 700,
                }}
              >
                {k.delta}
              </span>
            </div>
            <div style={{ display: "flex", alignItems: "flex-end", gap: 9, marginTop: 14 }}>
              <span
                style={{
                  fontSize: 28,
                  fontWeight: 800,
                  letterSpacing: "-.03em",
                  lineHeight: 1,
                  ...tabular,
                }}
              >
                {k.value}
              </span>
              <span style={{ fontSize: 11.5, color: "var(--text3)", paddingBottom: 2 }}>{k.note}</span>
            </div>
          </button>
        ))}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1.55fr 1fr",
          gap: 12,
          marginTop: 12,
          alignItems: "start",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {/* greeting hero */}
          <div
            style={{
              position: "relative",
              borderRadius: 20,
              overflow: "hidden",
              background: "var(--ink)",
              boxShadow: "var(--lift)",
              animation: "fadeUp .5s .04s ease both",
            }}
          >
            <div
              style={{
                position: "absolute",
                right: -60,
                top: -80,
                width: 240,
                height: 240,
                borderRadius: "50%",
                background: "radial-gradient(circle,rgba(139,125,255,.3),transparent 68%)",
                pointerEvents: "none",
              }}
            />
            <div
              style={{
                position: "relative",
                display: "flex",
                alignItems: "center",
                gap: 24,
                padding: "22px 24px",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 21, fontWeight: 800, color: "#fff", letterSpacing: "-.02em" }}>
                  {greeting()}, {firstName}
                </div>
                <div
                  style={{
                    marginTop: 6,
                    fontSize: 13,
                    lineHeight: 1.55,
                    color: "rgba(255,255,255,.62)",
                    maxWidth: "44ch",
                  }}
                >
                  {homeSub}
                </div>
                <div style={{ display: "flex", gap: 8, marginTop: 15 }}>
                  <button
                    type="button"
                    className="hv-rise"
                    onClick={() => go("board")}
                    style={{
                      padding: "9px 16px",
                      border: "none",
                      borderRadius: 999,
                      background: "#fff",
                      color: "#17171f",
                      fontSize: 12.5,
                      fontWeight: 700,
                      cursor: "pointer",
                      transition: "transform .18s ease",
                    }}
                  >
                    Open board
                  </button>
                  <button
                    type="button"
                    className="hv-white"
                    onClick={() => go("agents")}
                    style={{
                      padding: "9px 16px",
                      border: "1px solid rgba(255,255,255,.2)",
                      borderRadius: 999,
                      background: "rgba(255,255,255,.07)",
                      color: "#fff",
                      fontSize: 12.5,
                      fontWeight: 600,
                      cursor: "pointer",
                      transition: "background .18s ease",
                    }}
                  >
                    Agents
                  </button>
                </div>
              </div>
              <div
                style={{
                  flex: "none",
                  width: 190,
                  paddingLeft: 22,
                  borderLeft: "1px solid rgba(255,255,255,.14)",
                }}
              >
                <div style={{ fontSize: 11.5, color: "rgba(255,255,255,.6)" }}>Spend today</div>
                <div
                  style={{
                    marginTop: 4,
                    fontSize: 30,
                    fontWeight: 800,
                    color: "#fff",
                    letterSpacing: "-.03em",
                    ...tabular,
                  }}
                >
                  {money(stats.spend_today)}
                </div>
                <div
                  style={{
                    marginTop: 9,
                    height: 5,
                    borderRadius: 5,
                    background: "rgba(255,255,255,.16)",
                    overflow: "hidden",
                  }}
                >
                  <div
                    style={{
                      height: "100%",
                      width: `${Math.min(100, (stats.spend_today / Math.max(0.01, budget)) * 100)}%`,
                      background: "var(--accent2)",
                      transformOrigin: "left",
                      animation: "barGrow .9s cubic-bezier(.2,.8,.2,1) both",
                      transition: "width .5s ease",
                    }}
                  />
                </div>
                <div style={{ marginTop: 7, fontSize: 11.5, color: "rgba(255,255,255,.55)" }}>
                  {plural(stats.runs_today, "run")} today
                </div>
              </div>
            </div>
          </div>

          {/* intent bar */}
          <div
            className="hv-border"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "7px 8px 7px 16px",
              border: "1px solid var(--line)",
              borderRadius: 16,
              background: "var(--surface)",
              transition: "border-color .2s ease",
            }}
          >
            <input
              value={intent}
              onChange={(e) => setIntent(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  createCard(intent, agentId, mode);
                  setIntent("");
                }
              }}
              placeholder="Describe what should happen next…"
              style={{
                flex: 1,
                minWidth: 0,
                border: "none",
                background: "transparent",
                fontSize: 13.5,
                outline: "none",
                padding: "9px 0",
              }}
            />
            <select
              value={agentId}
              onChange={(e) => setAgentId(e.target.value)}
              title="Which agent takes it"
              style={{
                flex: "none",
                border: "none",
                background: "transparent",
                color: "var(--text2)",
                fontSize: 12.5,
                fontWeight: 600,
                cursor: "pointer",
                outline: "none",
              }}
            >
              {workers.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <div style={{ display: "flex", gap: 2, flex: "none" }}>
              {MODES.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => setMode(m.id)}
                  style={{
                    padding: "6px 12px",
                    border: "none",
                    borderRadius: 999,
                    fontSize: 12.5,
                    cursor: "pointer",
                    transition: "all .18s ease",
                    background: mode === m.id ? "var(--accentSoft)" : "transparent",
                    color: mode === m.id ? "var(--accent)" : "var(--text3)",
                    fontWeight: mode === m.id ? 700 : 500,
                  }}
                >
                  {m.name}
                </button>
              ))}
            </div>
            <button
              type="button"
              className="hv-bright"
              onClick={() => {
                createCard(intent, agentId, mode);
                setIntent("");
              }}
              style={{
                flex: "none",
                padding: "9px 18px",
                border: "none",
                borderRadius: 999,
                background: intent.trim() ? "var(--accent)" : "var(--surface2)",
                color: intent.trim() ? "var(--onAccent)" : "var(--text3)",
                fontSize: 13,
                fontWeight: 700,
                cursor: intent.trim() ? "pointer" : "not-allowed",
                transition: "filter .18s ease",
              }}
            >
              {mode === "start" ? "Start" : "Add"}
            </button>
          </div>

          {/* waiting on you */}
          <Card animation="fadeUp .55s .08s ease both">
            <CardHead
              title="Waiting on you"
              count={needsCount}
              countColor="var(--bad)"
              countSoft="var(--badSoft)"
              right={<HeadLink label="Board →" onClick={() => go("board")} />}
            />
            {attention.map((a) => (
              <div
                key={a.key}
                className="hv-row"
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 13,
                  padding: "13px 17px",
                  borderTop: "1px solid var(--line2)",
                  transition: "background .18s ease",
                }}
              >
                <Avatar color={a.accent} soft={a.soft}>
                  {a.mark}
                </Avatar>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13.5, fontWeight: 600, ...truncate }}>{a.title}</div>
                  <div style={{ marginTop: 3, fontSize: 11.5, color: "var(--text3)", ...truncate }}>
                    {a.note}
                  </div>
                </div>
                <div style={{ display: "flex", gap: 6, flex: "none" }}>
                  <StrongButton label={a.primaryLabel} onClick={a.primary} />
                  <QuietButton label={a.secondaryLabel} onClick={a.secondary} />
                </div>
              </div>
            ))}
            {needsCount === 0 &&
              (cards.length === 0 ? (
                <NoCardsYet onPick={setIntent} />
              ) : (
                <EmptyNote>Nothing waiting. The Director has the board.</EmptyNote>
              ))}
          </Card>

          {/* in progress */}
          <Card animation="fadeUp .6s .12s ease both">
            <CardHead
              title="In progress"
              right={<HeadLink label="All sessions →" onClick={() => go("runs")} />}
            />
            {running.map((c) => {
              const log = outputs[c.id] ?? [];
              const last = log[log.length - 1];
              const session = snapshot.sessions.find((s) => s.card_id === c.id);
              const agent = agents.find((a) => a.id === c.agent_id);
              return (
                <div
                  key={c.id}
                  className="hv-row"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 13,
                    padding: "13px 17px",
                    borderTop: "1px solid var(--line2)",
                    transition: "background .18s ease",
                  }}
                >
                  <Avatar color="var(--accent)" soft="var(--accentSoft)">
                    {agent?.initial ?? "?"}
                  </Avatar>
                  <button
                    type="button"
                    onClick={() => openRun(c.id)}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      border: "none",
                      background: "transparent",
                      padding: 0,
                      textAlign: "left",
                      cursor: "pointer",
                    }}
                  >
                    <span style={{ display: "block", fontSize: 13.5, fontWeight: 600, ...truncate }}>
                      {c.title}
                    </span>
                    <span
                      style={{
                        display: "block",
                        marginTop: 4,
                        fontFamily: "var(--mono)",
                        fontSize: 11,
                        color: "var(--text3)",
                        ...truncate,
                      }}
                    >
                      {last ? cut(last.text, 62) : "starting up"}
                    </span>
                  </button>
                  <span
                    style={{
                      flex: "none",
                      display: "flex",
                      alignItems: "center",
                      gap: 12,
                      fontSize: 12,
                      color: "var(--text3)",
                      ...tabular,
                    }}
                  >
                    <span>{session ? duration(Date.now() - session.started_ms) : "—"}</span>
                    <span style={{ fontWeight: 600, color: "var(--text2)" }}>
                      {money(c.cost_usd)}
                    </span>
                    <span
                      style={{
                        width: 6,
                        height: 6,
                        borderRadius: "50%",
                        background: "var(--accent)",
                        animation: "breathe 2s ease-in-out infinite",
                      }}
                    />
                  </span>
                  <QuietButton label="Stop" onClick={() => cancelRun(c.id)} />
                </div>
              );
            })}
            {running.length === 0 && <EmptyNote>Nothing running right now.</EmptyNote>}
          </Card>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {/* code written */}
          <Card pad="16px 17px" animation="fadeUp .5s .06s ease both" style={{ overflow: "visible" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 9, marginBottom: 4 }}>
              <span style={{ fontSize: 14, fontWeight: 700 }}>Code written</span>
              <div style={{ flex: 1 }} />
              <span style={{ fontSize: 11.5, color: "var(--text3)" }}>Last 7 days</span>
            </div>
            <div style={{ display: "flex", alignItems: "flex-end", gap: 9 }}>
              <span style={{ fontSize: 26, fontWeight: 800, letterSpacing: "-.03em", ...tabular }}>
                {num(code.added + code.removed)}
              </span>
              <span style={{ fontSize: 11.5, color: "var(--text3)", paddingBottom: 3 }}>
                lines changed
              </span>
            </div>
            <div style={{ marginTop: 14 }}>
              <WeekBars values={code.bars} labels={weekLetters()} />
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 14 }}>
              {[
                { label: "Added", value: `+${num(code.added)}`, color: "var(--ok)" },
                { label: "Removed", value: `−${num(code.removed)}`, color: "var(--bad)" },
              ].map((c) => (
                <span
                  key={c.label}
                  style={{
                    flex: 1,
                    padding: "10px 12px",
                    borderRadius: 13,
                    background: "var(--surface2)",
                    display: "flex",
                    flexDirection: "column",
                    gap: 3,
                  }}
                >
                  <span style={{ fontSize: 11, color: "var(--text3)" }}>{c.label}</span>
                  <span style={{ fontSize: 15, fontWeight: 700, color: c.color, ...tabular }}>
                    {c.value}
                  </span>
                </span>
              ))}
            </div>
          </Card>

          {/* your agents */}
          <Card animation="fadeUp .55s .1s ease both">
            <CardHead
              title="Your agents"
              right={<HeadLink label="Manage →" onClick={() => go("agents")} />}
            />
            {agents.map((a) => {
              const t = tone(a.tone);
              const isWorking = running.some((c) => c.agent_id === a.id);
              const state = a.paused
                ? "paused"
                : isWorking
                  ? "working"
                  : a.id === "director"
                    ? "watching"
                    : "idle";
              const stateFg = a.paused
                ? "var(--text3)"
                : isWorking
                  ? "var(--accent)"
                  : a.id === "director"
                    ? "var(--info)"
                    : "var(--text3)";
              const stateSoft = a.paused
                ? "var(--surface2)"
                : isWorking
                  ? "var(--accentSoft)"
                  : a.id === "director"
                    ? "var(--infoSoft)"
                    : "var(--surface2)";
              return (
                <button
                  key={a.id}
                  type="button"
                  className="hv-row"
                  onClick={() => openAgent(a.id)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 11,
                    width: "100%",
                    textAlign: "left",
                    padding: "12px 17px",
                    border: "none",
                    borderTop: "1px solid var(--line2)",
                    background: "transparent",
                    cursor: "pointer",
                    transition: "background .18s ease",
                  }}
                >
                  <Avatar color={t.color} soft={t.soft} fontSize={13}>
                    {a.initial}
                  </Avatar>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span style={{ display: "block", fontSize: 13.5, fontWeight: 700 }}>{a.name}</span>
                    <span
                      style={{ display: "block", marginTop: 2, fontSize: 11.5, color: "var(--text3)" }}
                    >
                      {a.title}
                    </span>
                  </span>
                  <span
                    style={{
                      flex: "none",
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                      padding: "4px 10px",
                      borderRadius: 999,
                      background: stateSoft,
                      color: stateFg,
                      fontSize: 11,
                      fontWeight: 700,
                    }}
                  >
                    <span
                      style={{ width: 5, height: 5, borderRadius: "50%", background: stateFg }}
                    />
                    {state}
                  </span>
                </button>
              );
            })}
          </Card>
        </div>
      </div>
    </div>
  );
}
