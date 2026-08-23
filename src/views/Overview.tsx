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
    <div style={{ padding: "18px 20px 22px", borderTop: "1px solid var(--line2)" }}>
      <div style={{ fontSize: "var(--t-sm)", color: "var(--text2)", lineHeight: 1.6 }}>
        Nothing on the board yet. Describe the first thing you want done in the field above — or
        start from one of these:
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 7, marginTop: 12 }}>
        {examples.map((e) => (
          <button
            key={e}
            type="button"
            className="hv-soft"
            onClick={() => onPick(e)}
            style={{
              padding: "7px 13px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: "var(--t-sm)",
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

  // One sentence, in the operator's language rather than the engine's. No
  // worktrees, no sidecars, no card ids — those live on the screens that are
  // actually about them.
  const homeSub = running.length
    ? `The crew is working on ${running.length === 1 ? "one card" : `${running.length} cards`}.` +
      (needsCount
        ? ` ${plural(needsCount, "thing")} ${needsCount === 1 ? "needs" : "need"} a minute of yours.`
        : " Nothing needs you yet.")
    : needsCount
      ? `${plural(needsCount, "thing")} ${needsCount === 1 ? "is" : "are"} waiting on you. Everything else is quiet.`
      : "All quiet. Nothing running, nothing waiting.";

  // The four numbers that used to be four separate tiles above the greeting.
  // Inside the banner they read as one glance instead of four.
  const stripe = [
    {
      key: "w",
      label: "Working now",
      value: String(stats.running),
      note: stats.running ? "live" : "idle",
      live: stats.running > 0,
      go: () => go("runs"),
    },
    {
      key: "n",
      label: "Needs you",
      value: String(needsCount),
      note: needsCount ? "waiting" : "clear",
      live: false,
      go: () => (approvals.length ? openApprovals() : go("board")),
    },
    {
      key: "d",
      label: "Done today",
      value: String(stats.done_today),
      note: `${stats.done} all together`,
      live: false,
      go: () => go("board"),
    },
    {
      key: "s",
      label: "Spent today",
      value: money(stats.spend_today),
      note: `of ${money(budget)}`,
      live: false,
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

  const submit = () => {
    if (!intent.trim()) return;
    createCard(intent, agentId, mode);
    setIntent("");
  };

  return (
    <div style={{ padding: "20px 26px 30px", display: "flex", flexDirection: "column", gap: 14 }}>
      {/* ---------------- the banner ---------------- */}
      <section className="banner" style={{ animation: "fadeUp .5s ease both" }}>
        <div
          aria-hidden
          style={{
            position: "absolute",
            right: -80,
            top: -120,
            width: 420,
            height: 420,
            borderRadius: "50%",
            background: "var(--bannerGlow)",
            pointerEvents: "none",
            zIndex: 0,
          }}
        />

        <div style={{ padding: "26px 30px 22px" }}>
          {/* eyebrow: where you are and when, so no page title is needed */}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              fontSize: "var(--t-xs)",
              color: "var(--onBanner3)",
            }}
          >
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 7,
                padding: "5px 12px",
                borderRadius: 999,
                background: "rgba(246,244,239,.1)",
                color: "var(--onBanner2)",
                fontWeight: 700,
                letterSpacing: ".01em",
              }}
            >
              <span
                style={{
                  width: 6,
                  height: 6,
                  borderRadius: "50%",
                  background: running.length ? "var(--accent2)" : "var(--onBanner3)",
                  animation: running.length ? "breathe 2s ease-in-out infinite" : undefined,
                }}
              />
              {project.name}
            </span>
            <span style={{ flex: 1 }} />
            <span>{today()}</span>
          </div>

          <h1
            style={{
              margin: "18px 0 0",
              fontSize: "var(--t-3xl)",
              lineHeight: 1.05,
              fontWeight: 800,
              letterSpacing: "-.035em",
              color: "var(--onBanner)",
            }}
          >
            {greeting()}, {firstName}
          </h1>
          <p
            style={{
              margin: "10px 0 0",
              maxWidth: "52ch",
              fontSize: "var(--t-lg)",
              lineHeight: 1.5,
              color: "var(--onBanner2)",
            }}
          >
            {homeSub}
          </p>

          {/* the one field the screen is built around */}
          <div className="banner-field" style={{ marginTop: 22, maxWidth: 760 }}>
            <input
              value={intent}
              onChange={(e) => setIntent(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && submit()}
              placeholder="Describe what should happen next…"
              aria-label="Describe what should happen next"
            />
            <select
              value={agentId}
              onChange={(e) => setAgentId(e.target.value)}
              aria-label="Which agent takes it"
              style={{
                flex: "none",
                border: "none",
                background: "transparent",
                color: "var(--onBanner2)",
                fontSize: "var(--t-sm)",
                fontWeight: 600,
                cursor: "pointer",
                outline: "none",
              }}
            >
              {workers.map((a) => (
                <option key={a.id} value={a.id} style={{ color: "var(--text)" }}>
                  {a.name}
                </option>
              ))}
            </select>
            <div
              style={{
                display: "flex",
                gap: 2,
                flex: "none",
                padding: 3,
                borderRadius: 999,
                background: "rgba(246,244,239,.08)",
              }}
            >
              {MODES.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => setMode(m.id)}
                  aria-pressed={mode === m.id}
                  style={{
                    padding: "6px 13px",
                    border: "none",
                    borderRadius: 999,
                    fontSize: "var(--t-sm)",
                    cursor: "pointer",
                    transition: "all .18s ease",
                    background: mode === m.id ? "var(--onBanner)" : "transparent",
                    color: mode === m.id ? "#191712" : "var(--onBanner3)",
                    fontWeight: mode === m.id ? 700 : 500,
                  }}
                >
                  {m.name}
                </button>
              ))}
            </div>
            <button
              type="button"
              className="hv-rise"
              onClick={submit}
              disabled={!intent.trim()}
              style={{
                flex: "none",
                padding: "10px 20px",
                border: "none",
                borderRadius: 999,
                background: intent.trim() ? "var(--accent2)" : "rgba(246,244,239,.1)",
                color: intent.trim() ? "#16141f" : "var(--onBanner3)",
                fontSize: "var(--t-md)",
                fontWeight: 700,
                cursor: intent.trim() ? "pointer" : "not-allowed",
                transition: "transform .18s ease, background .18s ease",
              }}
            >
              {mode === "start" ? "Start" : "Add"}
            </button>
          </div>
        </div>

        {/* the stat strip along the foot */}
        <div
          style={{
            display: "flex",
            borderTop: "1px solid rgba(246,244,239,.13)",
            background: "rgba(20,23,21,.28)",
          }}
        >
          {stripe.map((s) => (
            <button key={s.key} type="button" className="banner-stat" onClick={s.go}>
              <span
                style={{
                  fontSize: "var(--t-xs)",
                  color: "var(--onBanner3)",
                  fontWeight: 600,
                  letterSpacing: ".04em",
                  textTransform: "uppercase",
                }}
              >
                {s.label}
              </span>
              <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span
                  style={{
                    fontSize: "var(--t-2xl)",
                    fontWeight: 800,
                    letterSpacing: "-.03em",
                    lineHeight: 1.1,
                    color: "var(--onBanner)",
                    ...tabular,
                  }}
                >
                  {s.value}
                </span>
                <span style={{ fontSize: "var(--t-xs)", color: "var(--onBanner3)" }}>{s.note}</span>
              </span>
            </button>
          ))}
        </div>
      </section>

      {/* ---------------- below the fold: two columns, four panels ---------------- */}
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1.6fr) minmax(300px,1fr)",
          gap: 14,
          alignItems: "start",
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <Card animation="fadeUp .55s .06s ease both" style={{ borderRadius: "var(--r-lg)" }}>
            <CardHead
              title="Needs you"
              count={needsCount || undefined}
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
                  gap: 14,
                  padding: "14px 20px",
                  borderTop: "1px solid var(--line2)",
                  transition: "background .18s ease",
                }}
              >
                <Avatar color={a.accent} soft={a.soft}>
                  {a.mark}
                </Avatar>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: "var(--t-md)", fontWeight: 600, ...truncate }}>
                    {a.title}
                  </div>
                  <div
                    style={{
                      marginTop: 3,
                      fontSize: "var(--t-sm)",
                      color: "var(--text3)",
                      ...truncate,
                    }}
                  >
                    {a.note}
                  </div>
                </div>
                <div style={{ display: "flex", gap: 7, flex: "none" }}>
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

          <Card animation="fadeUp .6s .1s ease both" style={{ borderRadius: "var(--r-lg)" }}>
            <CardHead
              title="Running now"
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
                    gap: 14,
                    padding: "14px 20px",
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
                    <span
                      style={{
                        display: "block",
                        fontSize: "var(--t-md)",
                        fontWeight: 600,
                        ...truncate,
                      }}
                    >
                      {c.title}
                    </span>
                    <span
                      style={{
                        display: "block",
                        marginTop: 4,
                        fontSize: "var(--t-xs)",
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
                      fontSize: "var(--t-sm)",
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

        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <Card animation="fadeUp .55s .08s ease both" style={{ borderRadius: "var(--r-lg)" }}>
            <CardHead
              title="The crew"
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
                    gap: 12,
                    width: "100%",
                    textAlign: "left",
                    padding: "13px 20px",
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
                    <span style={{ display: "block", fontSize: "var(--t-md)", fontWeight: 700 }}>
                      {a.name}
                    </span>
                    <span
                      style={{
                        display: "block",
                        marginTop: 2,
                        fontSize: "var(--t-sm)",
                        color: "var(--text3)",
                      }}
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
                      padding: "5px 11px",
                      borderRadius: 999,
                      background: stateSoft,
                      color: stateFg,
                      fontSize: "var(--t-xs)",
                      fontWeight: 700,
                    }}
                  >
                    <span style={{ width: 5, height: 5, borderRadius: "50%", background: stateFg }} />
                    {state}
                  </span>
                </button>
              );
            })}
          </Card>

          <Card
            pad="18px 20px 20px"
            animation="fadeUp .6s .12s ease both"
            style={{ overflow: "visible", borderRadius: "var(--r-lg)" }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
              <span style={{ fontSize: "var(--t-lg)", fontWeight: 700 }}>Code written</span>
              <div style={{ flex: 1 }} />
              <span style={{ fontSize: "var(--t-xs)", color: "var(--text3)" }}>Last 7 days</span>
            </div>
            <div style={{ display: "flex", alignItems: "baseline", gap: 9, marginTop: 8 }}>
              <span
                style={{
                  fontSize: "var(--t-2xl)",
                  fontWeight: 800,
                  letterSpacing: "-.03em",
                  ...tabular,
                }}
              >
                {num(code.added + code.removed)}
              </span>
              <span style={{ fontSize: "var(--t-sm)", color: "var(--text3)" }}>lines changed</span>
            </div>
            <div style={{ marginTop: 16 }}>
              <WeekBars values={code.bars} labels={weekLetters()} />
            </div>
            <div style={{ display: "flex", gap: 9, marginTop: 16 }}>
              {[
                { label: "Added", value: `+${num(code.added)}`, color: "var(--ok)" },
                { label: "Removed", value: `−${num(code.removed)}`, color: "var(--bad)" },
              ].map((c) => (
                <span
                  key={c.label}
                  style={{
                    flex: 1,
                    padding: "11px 13px",
                    borderRadius: "var(--r-md)",
                    background: "var(--surface2)",
                    display: "flex",
                    flexDirection: "column",
                    gap: 3,
                  }}
                >
                  <span style={{ fontSize: "var(--t-xs)", color: "var(--text3)" }}>{c.label}</span>
                  <span
                    style={{
                      fontSize: "var(--t-lg)",
                      fontWeight: 700,
                      color: c.color,
                      ...tabular,
                    }}
                  >
                    {c.value}
                  </span>
                </span>
              ))}
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}
