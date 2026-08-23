import { useEffect, useState } from "react";
import { money, num, plural } from "../lib/format";
import {
  ALL_PERMISSIONS,
  MODELS,
  REVIEWERS,
  STATUS_NAME,
  STATUS_TONE,
  WORKTREE_MODES,
  tone,
  type AgentProfile,
} from "../lib/types";
import { useStore } from "../state/store";
import { HeadLink, MiniBars, Switch, tabular, truncate } from "../components/ui";
import type { View } from "./views";

function stateOf(agent: AgentProfile, working: boolean) {
  if (agent.paused) return { label: "paused", fg: "var(--text3)", soft: "var(--surface2)" };
  if (working) return { label: "working", fg: "var(--accent)", soft: "var(--accentSoft)" };
  if (agent.id === "director") return { label: "watching", fg: "var(--info)", soft: "var(--infoSoft)" };
  return { label: "idle", fg: "var(--text3)", soft: "var(--surface2)" };
}

/** The templates on offer. Fetched only when this opens, and nothing is created
 *  until one is picked: a template is a menu entry, never something Harness
 *  turns on by itself. */
function TemplatePicker({ close }: { close: () => void }) {
  const { agentTemplates, createAgentFromTemplate, agents } = useStore();
  const [templates, setTemplates] = useState<AgentProfile[] | null>(null);

  useEffect(() => {
    let alive = true;
    agentTemplates().then((list) => alive && setTemplates(list));
    return () => {
      alive = false;
    };
  }, [agentTemplates]);

  const teams = [...new Set((templates ?? []).map((t) => t.team || "other"))];

  return (
    <div
      onClick={close}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 60,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 32,
        background: "rgba(10,10,20,.44)",
        animation: "fadeIn .18s ease both",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "min(720px,100%)",
          maxHeight: "100%",
          display: "flex",
          flexDirection: "column",
          border: "1px solid var(--line)",
          borderRadius: 22,
          background: "var(--surface)",
          overflow: "hidden",
          boxShadow: "0 24px 60px rgba(10,10,30,.28)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            padding: "16px 18px",
            borderBottom: "1px solid var(--line)",
          }}
        >
          <span style={{ flex: 1, fontSize: 15, fontWeight: 700 }}>Add a profile</span>
          <span style={{ fontSize: 11.5, color: "var(--text3)" }}>
            Only the Director is required. Everything here is optional.
          </span>
          <button
            type="button"
            className="hv-text"
            onClick={close}
            style={{
              width: 26,
              height: 26,
              border: "1px solid var(--line)",
              borderRadius: "50%",
              background: "transparent",
              color: "var(--text3)",
              cursor: "pointer",
              fontSize: 11,
            }}
          >
            &#10005;
          </button>
        </div>

        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 16 }}>
          {templates == null && (
            <div style={{ fontSize: 12.5, color: "var(--text3)" }}>Reading the templates\u2026</div>
          )}
          {teams.map((team) => (
            <div key={team} style={{ marginBottom: 16 }}>
              <div
                style={{
                  fontSize: 10.5,
                  fontWeight: 800,
                  letterSpacing: ".08em",
                  textTransform: "uppercase",
                  color: "var(--text3)",
                  marginBottom: 8,
                }}
              >
                {team}
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "repeat(2,minmax(0,1fr))", gap: 9 }}>
                {(templates ?? [])
                  .filter((t) => (t.team || "other") === team)
                  .map((t) => {
                    const tn = tone(t.tone);
                    const already = agents.some((a) => a.id === t.id);
                    return (
                      <button
                        key={t.id}
                        type="button"
                        className="hv-tile"
                        onClick={() => {
                          createAgentFromTemplate(t.id);
                          close();
                        }}
                        style={{
                          display: "flex",
                          gap: 10,
                          padding: 13,
                          border: "1px solid var(--line)",
                          borderRadius: 16,
                          background: "var(--surface)",
                          color: "var(--text)",
                          textAlign: "left",
                          cursor: "pointer",
                          transition: "all .2s ease",
                        }}
                      >
                        <span
                          style={{
                            width: 28,
                            height: 28,
                            flex: "none",
                            borderRadius: "50%",
                            background: tn.soft,
                            color: tn.color,
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                            fontSize: 11.5,
                            fontWeight: 800,
                          }}
                        >
                          {(t.initial || t.name[0] || "?").toUpperCase()}
                        </span>
                        <span style={{ minWidth: 0 }}>
                          <span
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: 6,
                              fontSize: 13,
                              fontWeight: 700,
                            }}
                          >
                            {t.name}
                            {already && (
                              <span
                                style={{
                                  padding: "1px 6px",
                                  borderRadius: 999,
                                  background: "var(--surface2)",
                                  color: "var(--text3)",
                                  fontSize: 9.5,
                                  fontWeight: 700,
                                }}
                              >
                                have one
                              </span>
                            )}
                          </span>
                          <span
                            style={{
                              display: "block",
                              marginTop: 3,
                              fontSize: 11.5,
                              color: "var(--text3)",
                              lineHeight: 1.5,
                            }}
                          >
                            {t.role}
                          </span>
                        </span>
                      </button>
                    );
                  })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/** The crew, as the design's tall cards with a tinted head. */
export function AgentList({
  open,
  go,
  openChat,
}: {
  open: (id: string) => void;
  go: (v: View) => void;
  /** Opens the conversation dock, for a direct chat with a profile. */
  openChat?: () => void;
}) {
  const { agents, agentStats, snapshot, projects, chatWithProfile, saveAgents, duplicateAgent, removeAgent } =
    useStore();
  const running = snapshot?.cards.filter((c) => c.status === "running") ?? [];
  const [picking, setPicking] = useState(false);

  /** A direct, persistent conversation with one profile: the same thread each
   *  time, not a new session per click. */
  const chatWith = (id: string) => {
    chatWithProfile(id);
    openChat?.();
  };

  /** A profile from nothing. The id is settled here so two customs in a row
   *  cannot collide on the same sanitised name. */
  const custom = () => {
    const taken = new Set(agents.map((a) => a.id));
    let id = "new-agent";
    for (let n = 2; taken.has(id); n += 1) id = `new-agent-${n}`;
    const blank: AgentProfile = {
      id,
      name: "New agent",
      initial: "N",
      title: "Specialist",
      role: "Say what this one is for.",
      brief: "",
      tone: "accent",
      model: "sonnet",
      permissions: ["Read", "Search"],
      budget_usd: 0.5,
      worktree: "none",
      reviewer: "human",
      paused: false,
      permission_mode: null,
      team: "",
      chat_enabled: true,
      tasks_enabled: true,
      max_concurrent: 1,
      skills: [],
      reports_to: null,
      can_delegate: false,
      expected_output: "",
      escalate_to: null,
    };
    saveAgents([...agents, blank]);
    open(id);
  };

  return (
    <div style={{ padding: "22px 26px 28px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>Agents</h1>
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
          {agents.length}
        </span>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>
          Shared across every project. Open one to see what it has been doing.
        </span>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          className="hv-border"
          onClick={custom}
          style={{
            padding: "6px 13px",
            border: "1px solid var(--line)",
            borderRadius: 999,
            background: "transparent",
            color: "var(--text2)",
            fontSize: 12,
            fontWeight: 600,
            cursor: "pointer",
            marginRight: 7,
            transition: "all .18s ease",
          }}
        >
          Custom profile
        </button>
        <button
          type="button"
          className="hv-bright"
          onClick={() => setPicking(true)}
          style={{
            padding: "6px 13px",
            border: "none",
            borderRadius: 999,
            background: "var(--accent)",
            color: "var(--onAccent)",
            fontSize: 12,
            fontWeight: 700,
            cursor: "pointer",
            marginRight: 7,
          }}
        >
          + From a template
        </button>
        <button
          type="button"
          className="hv-border"
          onClick={() => go("director")}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "6px 13px",
            border: "1px solid var(--line)",
            borderRadius: 999,
            background: "transparent",
            color: "var(--text2)",
            fontSize: 12,
            fontWeight: 600,
            cursor: "pointer",
            transition: "all .18s ease",
          }}
        >
          <span
            style={{
              width: 18,
              height: 18,
              borderRadius: "50%",
              background: "var(--infoSoft)",
              color: "var(--info)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 9.5,
              fontWeight: 800,
            }}
          >
            D
          </span>
          The Director →
        </button>
      </div>

      {picking && <TemplatePicker close={() => setPicking(false)} />}

      <div style={{ display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 13 }}>
        {agents.map((a) => {
          const t = tone(a.tone);
          const s = agentStats[a.id];
          const st = stateOf(a, running.some((c) => c.agent_id === a.id));
          const wrote = (s?.lines_added ?? 0) + (s?.lines_removed ?? 0);
          const metric = wrote ? num(wrote) : s?.reviews ? num(s.reviews) : num(s?.runs ?? 0);
          const canChatNow = a.chat_enabled && !a.paused;
          const metricLabel = wrote
            ? "lines / week"
            : s?.reviews
              ? "diffs reviewed"
              : "runs recorded";
          return (
            <div key={a.id} style={{ position: "relative", display: "flex" }}>
            <button
              type="button"
              className="hv-tile"
              onClick={() => open(a.id)}
              style={{
                position: "relative",
                display: "flex",
                flexDirection: "column",
                padding: 0,
                border: "1px solid var(--line)",
                borderRadius: 24,
                background: "var(--surface)",
                color: "var(--text)",
                cursor: "pointer",
                textAlign: "left",
                overflow: "hidden",
                boxShadow: "0 1px 2px rgba(20,20,40,.05)",
                transition: "all .24s cubic-bezier(.2,.8,.2,1)",
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
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 14,
                  padding: "20px 20px 0",
                }}
              >
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
                  {a.initial}
                  <span
                    style={{
                      position: "absolute",
                      right: -2,
                      bottom: -2,
                      width: 13,
                      height: 13,
                      borderRadius: "50%",
                      background: st.fg,
                      border: "3px solid var(--surface)",
                    }}
                  />
                </span>
                <span style={{ flex: 1, minWidth: 0, paddingTop: 4 }}>
                  <span
                    style={{ display: "block", fontSize: 17, fontWeight: 800, letterSpacing: "-.02em" }}
                  >
                    {a.name}
                  </span>
                  <span
                    style={{
                      display: "block",
                      marginTop: 5,
                      fontSize: 10.5,
                      fontWeight: 800,
                      letterSpacing: ".11em",
                      textTransform: "uppercase",
                      color: t.color,
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
                    padding: "4px 10px",
                    borderRadius: 999,
                    background: st.soft,
                    color: st.fg,
                    fontSize: 11,
                    fontWeight: 700,
                  }}
                >
                  <span style={{ width: 5, height: 5, borderRadius: "50%", background: st.fg }} />
                  {st.label}
                </span>
              </span>

              <span
                style={{
                  position: "relative",
                  display: "block",
                  padding: "15px 20px 0",
                  fontSize: 12.5,
                  color: "var(--text2)",
                  lineHeight: 1.55,
                  minHeight: 64,
                }}
              >
                {a.role}
              </span>

              <span
                style={{
                  position: "relative",
                  display: "flex",
                  alignItems: "flex-end",
                  gap: 16,
                  padding: "18px 20px 0",
                }}
              >
                <span style={{ flex: 1 }}>
                  <MiniBars values={s?.week_runs ?? [0, 0, 0, 0, 0, 0, 0]} color={t.color} height={46} />
                </span>
                <span
                  style={{
                    flex: "none",
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "flex-end",
                    gap: 1,
                  }}
                >
                  <span
                    style={{ fontSize: 16, fontWeight: 800, letterSpacing: "-.02em", ...tabular }}
                  >
                    {metric}
                  </span>
                  <span style={{ fontSize: 10.5, color: "var(--text3)" }}>{metricLabel}</span>
                </span>
              </span>

              <span
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
                  {MODELS.find((m) => m.id === a.model)?.name ?? "auto"}
                </span>
                <span style={{ opacity: 0.5 }}>·</span>
                <span>{plural(projects.length, "project")}</span>
                <span style={{ opacity: 0.5 }}>·</span>
                <span>{money(s?.spend ?? 0)}</span>
                <span style={{ flex: 1 }} />
                <span
                  style={{ display: "flex", alignItems: "center", gap: 6, fontWeight: 800, color: t.color }}
                >
                  Open<span>→</span>
                </span>
              </span>
            </button>

            {/* Real buttons, so they sit outside the tile rather than nested
                inside it. Chat is the direct conversation with this profile;
                the tile itself opens its page. */}
            <div
              style={{
                position: "absolute",
                top: 12,
                right: 12,
                display: "flex",
                gap: 4,
              }}
            >
              {[
                {
                  label: a.chat_enabled ? `Chat with ${a.name}` : `${a.name} has chat turned off`,
                  glyph: "\u25cf\u25cf",
                  disabled: !canChatNow,
                  run: () => chatWith(a.id),
                },
                { label: "Duplicate", glyph: "\u29c9", disabled: false, run: () => duplicateAgent(a.id) },
                {
                  label:
                    a.id === "director"
                      ? "The Director cannot be removed"
                      : `Remove ${a.name}`,
                  glyph: "\u2715",
                  disabled: a.id === "director",
                  run: () => removeAgent(a.id),
                },
              ].map((action) => (
                <button
                  key={action.label}
                  type="button"
                  className="hv-text"
                  title={action.label}
                  disabled={action.disabled}
                  onClick={(e) => {
                    e.stopPropagation();
                    action.run();
                  }}
                  style={{
                    width: 24,
                    height: 24,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    border: "1px solid var(--line)",
                    borderRadius: 8,
                    background: "var(--surface)",
                    color: "var(--text3)",
                    fontSize: 9.5,
                    cursor: action.disabled ? "not-allowed" : "pointer",
                    opacity: action.disabled ? 0.35 : 1,
                  }}
                >
                  {action.glyph}
                </button>
              ))}
            </div>
            </div>
          );
        })}

      </div>
    </div>
  );
}

const fieldStyle = {
  width: "100%",
  padding: "8px 11px",
  border: "1px solid var(--line)",
  borderRadius: 10,
  background: "var(--surface2)",
  color: "var(--text)",
  fontSize: 12.5,
  outline: "none",
} as const;

const labelStyle = {
  fontSize: 11,
  fontWeight: 700,
  color: "var(--text3)",
  marginBottom: 5,
} as const;

function Divider() {
  return <div style={{ height: 1, background: "var(--line2)", margin: "16px 0" }} />;
}

function PillChoice<T extends string>({
  value,
  options,
  onPick,
}: {
  value: T;
  options: { id: T; name: string }[];
  onPick: (id: T) => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        gap: 2,
        padding: 3,
        borderRadius: 12,
        background: "var(--surface2)",
        border: "1px solid var(--line)",
      }}
    >
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onPick(o.id)}
            style={{
              flex: 1,
              padding: "8px 6px",
              border: "none",
              borderRadius: 9,
              fontSize: 12.5,
              cursor: "pointer",
              transition: "all .18s ease",
              background: on ? "var(--accent)" : "transparent",
              color: on ? "var(--onAccent)" : "var(--text2)",
              fontWeight: on ? 700 : 500,
            }}
          >
            {o.name}
          </button>
        );
      })}
    </div>
  );
}

/** The agent profile, as the design's bottom drawer over the app. */
export function AgentDrawer({
  agentId,
  close,
  openRun,
  go,
  openChat,
}: {
  agentId: string;
  close: () => void;
  openRun: (cardId: string) => void;
  go: (v: View) => void;
  openChat?: () => void;
}) {
  const {
    agents,
    agentStats,
    snapshot,
    saveAgents,
    settings,
    toast,
    chatWithProfile,
    conversations,
    openConversation,
  } = useStore();
  const agent = agents.find((a) => a.id === agentId);
  const stats = agentStats[agentId];
  const cards = (snapshot?.cards ?? []).filter((c) => c.agent_id === agentId);

  if (!agent) return null;

  const patch = (next: Partial<AgentProfile>) =>
    saveAgents(agents.map((a) => (a.id === agent.id ? { ...a, ...next } : a)));

  // Its own conversations, alongside its runs and costs.
  const mine = conversations.filter((c) => c.profile_id === agent.id);
  const working = (snapshot?.cards ?? []).some(
    (c) => c.status === "running" && c.agent_id === agent.id,
  );
  const st = stateOf(agent, working);
  const isCoder = (stats?.lines_added ?? 0) + (stats?.lines_removed ?? 0) > 0;

  const identity = [
    { label: "Model", value: MODELS.find((m) => m.id === agent.model)?.name ?? "auto" },
    { label: "Runs recorded", value: num(stats?.runs ?? 0) },
    { label: "Avg cost per run", value: money(stats?.avg_cost ?? 0, 3) },
    { label: "Where it works", value: WORKTREE_MODES.find((w) => w.id === agent.worktree)!.name },
  ];

  const output = isCoder
    ? [
        {
          label: "Lines added",
          value: `+${num(stats?.lines_added ?? 0)}`,
          delta: "commits",
          deltaColor: "var(--ok)",
          deltaSoft: "var(--okSoft)",
          note: "from its trailers",
        },
        {
          label: "Lines removed",
          value: `−${num(stats?.lines_removed ?? 0)}`,
          delta: "commits",
          deltaColor: "var(--bad)",
          deltaSoft: "var(--badSoft)",
          note: "from its trailers",
        },
        {
          label: "Commits",
          value: num(stats?.commits ?? 0),
          delta: plural(stats?.cards_done ?? 0, "card"),
          deltaColor: "var(--ok)",
          deltaSoft: "var(--okSoft)",
          note: "done so far",
        },
        {
          label: "Spent",
          value: money(stats?.spend ?? 0),
          delta: `${plural(stats?.turns ?? 0, "turn")}`,
          deltaColor: "var(--text3)",
          deltaSoft: "var(--surface2)",
          note: "across every run",
        },
      ]
    : [
        {
          label: "Runs",
          value: num(stats?.runs ?? 0),
          delta: plural(stats?.cards ?? 0, "card"),
          deltaColor: "var(--text3)",
          deltaSoft: "var(--surface2)",
          note: "in this project",
        },
        {
          label: "Diffs reviewed",
          value: num(stats?.reviews ?? 0),
          delta: "as reviewer",
          deltaColor: "var(--info)",
          deltaSoft: "var(--infoSoft)",
          note: "all projects",
        },
        {
          label: "Sent back",
          value: num(stats?.sent_back ?? 0),
          delta: stats?.reviews
            ? `${Math.round(((stats.sent_back ?? 0) / stats.reviews) * 100)}%`
            : "—",
          deltaColor: "var(--warn)",
          deltaSoft: "var(--warnSoft)",
          note: "of what it read",
        },
        {
          label: "Spent",
          value: money(stats?.spend ?? 0),
          delta: money(stats?.avg_cost ?? 0, 3),
          deltaColor: "var(--text3)",
          deltaSoft: "var(--surface2)",
          note: "a run on average",
        },
      ];

  return (
    <div
      onClick={close}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 70,
        overflow: "hidden",
        display: "flex",
        justifyContent: "center",
        alignItems: "flex-end",
        padding: "34px 30px 0",
        background: "rgba(16,16,26,.42)",
        backdropFilter: "blur(5px)",
        animation: "fadeIn .2s ease both",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "100%",
          maxWidth: 1140,
          maxHeight: "100%",
          display: "flex",
          flexDirection: "column",
          background: "var(--bg)",
          border: "1px solid var(--line)",
          borderBottom: "none",
          borderRadius: "28px 28px 0 0",
          boxShadow: "var(--shadow)",
          overflow: "hidden",
          animation: "drawerUp .44s cubic-bezier(.16,.86,.28,1) both",
        }}
      >
        <div
          style={{
            position: "relative",
            flex: "none",
            display: "flex",
            alignItems: "center",
            padding: "13px 14px 12px 22px",
            background: "var(--surface)",
            borderBottom: "1px solid var(--line)",
          }}
        >
          <span
            style={{
              position: "absolute",
              left: "50%",
              top: 6,
              transform: "translateX(-50%)",
              width: 46,
              height: 4,
              borderRadius: 4,
              background: "var(--line)",
            }}
          />
          <span
            style={{
              fontSize: 10.5,
              fontWeight: 800,
              letterSpacing: ".12em",
              textTransform: "uppercase",
              color: "var(--text3)",
            }}
          >
            Agent profile
          </span>
          <span style={{ flex: 1 }} />
          <span style={{ fontSize: 11, color: "var(--text3)", marginRight: 10 }}>Esc to close</span>
          <button
            type="button"
            className="hv-soft"
            title="Close"
            onClick={close}
            style={{
              width: 28,
              height: 28,
              border: "1px solid var(--line)",
              borderRadius: "50%",
              background: "transparent",
              color: "var(--text2)",
              fontSize: 11,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            ✕
          </button>
        </div>

        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            overflowX: "hidden",
            padding: "20px 22px 26px",
          }}
        >
          <div
            style={{
              position: "relative",
              borderRadius: 22,
              overflow: "hidden",
              background: "var(--ink)",
              boxShadow: "var(--lift)",
              animation: "fadeUp .45s ease both",
            }}
          >
            <div
              style={{
                position: "absolute",
                right: -70,
                top: -90,
                width: 260,
                height: 260,
                borderRadius: "50%",
                background: "radial-gradient(circle,rgba(139,125,255,.32),transparent 68%)",
                pointerEvents: "none",
              }}
            />
            <div style={{ position: "relative", padding: "24px 26px" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
                <span
                  style={{
                    width: 66,
                    height: 66,
                    flex: "none",
                    borderRadius: "50%",
                    background: "rgba(255,255,255,.1)",
                    border: "1px solid rgba(255,255,255,.18)",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 24,
                    fontWeight: 800,
                    color: "#fff",
                  }}
                >
                  {agent.initial}
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span
                      style={{
                        fontSize: 24,
                        fontWeight: 800,
                        color: "#fff",
                        letterSpacing: "-.02em",
                      }}
                    >
                      {agent.name}
                    </span>
                    <span
                      style={{
                        padding: "4px 11px",
                        borderRadius: 999,
                        background: "rgba(255,255,255,.12)",
                        color: "#fff",
                        fontSize: 11.5,
                        fontWeight: 700,
                      }}
                    >
                      {agent.title}
                    </span>
                    <span
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 6,
                        padding: "4px 11px",
                        borderRadius: 999,
                        background: "rgba(139,125,255,.22)",
                        color: "#cfc7ff",
                        fontSize: 11.5,
                        fontWeight: 700,
                      }}
                    >
                      <span
                        style={{ width: 5, height: 5, borderRadius: "50%", background: "#cfc7ff" }}
                      />
                      {st.label}
                    </span>
                  </div>
                  <div
                    style={{
                      marginTop: 7,
                      fontSize: 13,
                      color: "rgba(255,255,255,.6)",
                      lineHeight: 1.55,
                      maxWidth: "60ch",
                    }}
                  >
                    {agent.role}
                  </div>
                </div>
                <div style={{ flex: "none", display: "flex", gap: 8 }}>
                  <button
                    type="button"
                    className="hv-white"
                    onClick={() => patch({ paused: !agent.paused })}
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
                    {agent.paused ? "Resume" : "Pause"}
                  </button>
                  <button
                    type="button"
                    className="hv-rise"
                    onClick={() => {
                      close();
                      go("home");
                      toast(
                        "var(--accent)",
                        agent.name,
                        "Describe the task in the field on Home and pick it there.",
                      );
                    }}
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
                    Give it work
                  </button>
                </div>
              </div>

              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "repeat(4,1fr)",
                  gap: 1,
                  marginTop: 22,
                  background: "rgba(255,255,255,.12)",
                  borderRadius: 14,
                  overflow: "hidden",
                }}
              >
                {identity.map((i) => (
                  <span
                    key={i.label}
                    style={{
                      padding: "13px 15px",
                      background: "rgba(20,20,26,.55)",
                      display: "flex",
                      flexDirection: "column",
                      gap: 4,
                    }}
                  >
                    <span style={{ fontSize: 11, color: "rgba(255,255,255,.55)" }}>{i.label}</span>
                    <span style={{ fontSize: 14.5, fontWeight: 700, color: "#fff" }}>{i.value}</span>
                  </span>
                ))}
              </div>
            </div>
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4,minmax(0,1fr))",
              gap: 12,
              marginTop: 12,
            }}
          >
            {output.map((o) => (
              <div
                key={o.label}
                style={{
                  padding: "16px 17px",
                  border: "1px solid var(--line)",
                  borderRadius: 18,
                  background: "var(--surface)",
                  animation: "fadeUp .45s .04s ease both",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                  }}
                >
                  <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text2)" }}>
                    {o.label}
                  </span>
                  <span
                    style={{
                      padding: "3px 9px",
                      borderRadius: 999,
                      background: o.deltaSoft,
                      color: o.deltaColor,
                      fontSize: 11.5,
                      fontWeight: 700,
                    }}
                  >
                    {o.delta}
                  </span>
                </div>
                <div style={{ display: "flex", alignItems: "flex-end", gap: 8, marginTop: 13 }}>
                  <span
                    style={{
                      fontSize: 26,
                      fontWeight: 800,
                      letterSpacing: "-.03em",
                      lineHeight: 1,
                      ...tabular,
                    }}
                  >
                    {o.value}
                  </span>
                  <span style={{ fontSize: 11.5, color: "var(--text3)", paddingBottom: 2 }}>
                    {o.note}
                  </span>
                </div>
              </div>
            ))}
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1.5fr 1fr",
              gap: 12,
              marginTop: 12,
              alignItems: "start",
            }}
          >
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div
                style={{
                  padding: "17px 18px",
                  border: "1px solid var(--line)",
                  borderRadius: 18,
                  background: "var(--surface)",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 9, marginBottom: 11 }}>
                  <span style={{ fontSize: 14, fontWeight: 700 }}>Brief</span>
                  <span style={{ fontSize: 11.5, color: "var(--text3)" }}>
                    what {agent.name} is told before every run
                  </span>
                </div>
                <textarea
                  rows={4}
                  value={agent.brief}
                  onChange={(e) => patch({ brief: e.target.value })}
                  placeholder="How should this agent work?"
                  className="hv-border"
                  style={{
                    width: "100%",
                    resize: "none",
                    padding: "13px 15px",
                    borderRadius: 14,
                    border: "1px solid var(--line)",
                    background: "var(--surface2)",
                    fontSize: 13,
                    lineHeight: 1.65,
                    outline: "none",
                    transition: "all .2s ease",
                  }}
                />
              </div>

              <div
                style={{
                  border: "1px solid var(--line)",
                  borderRadius: 18,
                  background: "var(--surface)",
                  overflow: "hidden",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "15px 18px 13px" }}>
                  <span style={{ fontSize: 14, fontWeight: 700 }}>Recent sessions</span>
                  <div style={{ flex: 1 }} />
                  <HeadLink
                    label="All →"
                    onClick={() => {
                      close();
                      go("runs");
                    }}
                  />
                </div>
                {cards.slice(0, 6).map((c) => {
                  const t = STATUS_TONE[c.status];
                  return (
                    <button
                      key={c.id}
                      type="button"
                      className="hv-row"
                      onClick={() => {
                        close();
                        openRun(c.id);
                      }}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 12,
                        width: "100%",
                        textAlign: "left",
                        padding: "12px 18px",
                        border: "none",
                        borderTop: "1px solid var(--line2)",
                        background: "transparent",
                        cursor: "pointer",
                        transition: "background .18s ease",
                      }}
                    >
                      <span
                        style={{
                          width: 7,
                          height: 7,
                          borderRadius: "50%",
                          flex: "none",
                          background: t.color,
                        }}
                      />
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <span
                          style={{ display: "block", fontSize: 13, fontWeight: 600, ...truncate }}
                        >
                          {c.title}
                        </span>
                        <span
                          style={{
                            display: "block",
                            marginTop: 3,
                            fontSize: 11.5,
                            color: "var(--text3)",
                          }}
                        >
                          {c.id} · {plural(c.turns, "turn")}
                        </span>
                      </span>
                      <span
                        style={{
                          flex: "none",
                          padding: "3px 10px",
                          borderRadius: 999,
                          background: t.soft,
                          color: t.color,
                          fontSize: 11,
                          fontWeight: 700,
                        }}
                      >
                        {STATUS_NAME[c.status]}
                      </span>
                      <span
                        style={{
                          flex: "none",
                          width: 56,
                          textAlign: "right",
                          fontSize: 12.5,
                          fontWeight: 600,
                          ...tabular,
                        }}
                      >
                        {money(c.cost_usd)}
                      </span>
                    </button>
                  );
                })}
                {cards.length === 0 && (
                  <div
                    style={{
                      padding: 20,
                      borderTop: "1px solid var(--line2)",
                      textAlign: "center",
                      fontSize: 12.5,
                      color: "var(--text3)",
                    }}
                  >
                    This agent has not run in this project yet.
                  </div>
                )}
              </div>
            </div>

            <div
              style={{
                padding: "17px 18px",
                border: "1px solid var(--line)",
                borderRadius: 18,
                background: "var(--surface)",
              }}
            >
              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 12 }}>Model</div>
              <PillChoice
                value={agent.model ?? "sonnet"}
                options={MODELS.map((m) => ({ id: m.id, name: m.name }))}
                onPick={(id) => patch({ model: id })}
              />
              <div style={{ marginTop: 10, fontSize: 11.5, color: "var(--text3)", lineHeight: 1.5 }}>
                {MODELS.find((m) => m.id === agent.model)?.hint ?? "Claude picks a model."}
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>Permissions</div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                {ALL_PERMISSIONS.map((p) => {
                  const on = agent.permissions.includes(p);
                  return (
                    <button
                      key={p}
                      type="button"
                      className="hv-border"
                      onClick={() =>
                        patch({
                          permissions: on
                            ? agent.permissions.filter((x) => x !== p)
                            : [...agent.permissions, p],
                        })
                      }
                      style={{
                        padding: "6px 12px",
                        border: `1px solid ${on ? "var(--accentLine)" : "var(--line)"}`,
                        borderRadius: 999,
                        background: on ? "var(--accentSoft)" : "transparent",
                        color: on ? "var(--accent)" : "var(--text3)",
                        fontSize: 11.5,
                        fontWeight: on ? 700 : 500,
                        cursor: "pointer",
                        transition: "all .18s ease",
                      }}
                    >
                      {p}
                    </button>
                  );
                })}
              </div>
              <div style={{ marginTop: 11, fontSize: 11.5, color: "var(--text3)", lineHeight: 1.5 }}>
                Anything else pauses the run and asks you.
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>Budget per run</div>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <button
                  type="button"
                  className="hv-soft"
                  onClick={() => {
                    const next = Number(((agent.budget_usd ?? 0.25) - 0.25).toFixed(2));
                    patch({ budget_usd: next > 0 ? next : null });
                  }}
                  style={{
                    width: 34,
                    height: 34,
                    border: "1px solid var(--line)",
                    borderRadius: 11,
                    background: "transparent",
                    color: "var(--text2)",
                    fontSize: 16,
                    cursor: "pointer",
                    transition: "all .18s ease",
                  }}
                >
                  −
                </button>
                <span
                  style={{ flex: 1, textAlign: "center", fontSize: 22, fontWeight: 800, ...tabular }}
                >
                  {agent.budget_usd == null ? "no cap" : money(agent.budget_usd)}
                </span>
                <button
                  type="button"
                  className="hv-soft"
                  onClick={() =>
                    patch({ budget_usd: Number(((agent.budget_usd ?? 0) + 0.25).toFixed(2)) })
                  }
                  style={{
                    width: 34,
                    height: 34,
                    border: "1px solid var(--line)",
                    borderRadius: 11,
                    background: "transparent",
                    color: "var(--text2)",
                    fontSize: 16,
                    cursor: "pointer",
                    transition: "all .18s ease",
                  }}
                >
                  +
                </button>
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>How it is used</div>
              <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                <Switch
                  on={agent.chat_enabled}
                  onChange={(v) => patch({ chat_enabled: v })}
                  label="Can hold a conversation"
                />
                <Switch
                  on={agent.tasks_enabled}
                  onChange={(v) => patch({ tasks_enabled: v })}
                  label="Can be given cards"
                />
                <Switch
                  on={agent.can_delegate}
                  onChange={(v) => patch({ can_delegate: v })}
                  label="Can put work on a board for others"
                />
                {agent.chat_enabled && mine.length > 0 && (
                  <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                    <div style={labelStyle}>
                      {plural(mine.length, "conversation")} with {agent.name}
                    </div>
                    {mine.slice(0, 5).map((c) => (
                      <button
                        key={c.id}
                        type="button"
                        className="hv-soft"
                        onClick={() => {
                          openConversation(c.id);
                          openChat?.();
                        }}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 8,
                          padding: "7px 10px",
                          border: "1px solid var(--line)",
                          borderRadius: 10,
                          background: "transparent",
                          color: "var(--text2)",
                          fontSize: 12,
                          textAlign: "left",
                          cursor: "pointer",
                        }}
                      >
                        <span style={{ flex: 1, minWidth: 0, ...truncate }}>{c.title}</span>
                        <span style={{ fontSize: 10.5, color: "var(--text3)", ...tabular }}>
                          {money(c.cost_usd, 3)}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
                {agent.chat_enabled && (
                  <button
                    type="button"
                    className="hv-border"
                    onClick={() => {
                      chatWithProfile(agent.id);
                      openChat?.();
                    }}
                    style={{
                      alignSelf: "flex-start",
                      padding: "7px 14px",
                      border: "1px solid var(--line)",
                      borderRadius: 999,
                      background: "transparent",
                      color: "var(--text2)",
                      fontSize: 12,
                      fontWeight: 700,
                      cursor: "pointer",
                    }}
                  >
                    Start a conversation with {agent.name}
                  </button>
                )}
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>Team</div>
              <input
                value={agent.team}
                onChange={(e) => patch({ team: e.target.value })}
                placeholder="engineering, growth, operations\u2026"
                style={fieldStyle}
              />

              <div style={{ ...labelStyle, marginTop: 13 }}>What finished work looks like</div>
              <input
                value={agent.expected_output}
                onChange={(e) => patch({ expected_output: e.target.value })}
                placeholder="A scoped diff with tests"
                style={fieldStyle}
              />

              <div style={{ ...labelStyle, marginTop: 13 }}>Skills, comma separated</div>
              <input
                value={agent.skills.join(", ")}
                onChange={(e) =>
                  patch({
                    skills: e.target.value
                      .split(",")
                      .map((x) => x.trim())
                      .filter(Boolean),
                  })
                }
                placeholder="schema markup, internal links"
                style={fieldStyle}
              />

              <div style={{ display: "flex", gap: 10, marginTop: 13 }}>
                <div style={{ flex: 1 }}>
                  <div style={labelStyle}>Reports to</div>
                  <select
                    value={agent.reports_to ?? ""}
                    onChange={(e) => patch({ reports_to: e.target.value || null })}
                    style={fieldStyle}
                  >
                    <option value="">Nobody</option>
                    {agents
                      .filter((other) => other.id !== agent.id)
                      .map((other) => (
                        <option key={other.id} value={other.id}>
                          {other.name}
                        </option>
                      ))}
                  </select>
                </div>
                <div style={{ flex: 1 }}>
                  <div style={labelStyle}>Escalates to</div>
                  <select
                    value={agent.escalate_to ?? ""}
                    onChange={(e) => patch({ escalate_to: e.target.value || null })}
                    style={fieldStyle}
                  >
                    <option value="">You</option>
                    {agents
                      .filter((other) => other.id !== agent.id)
                      .map((other) => (
                        <option key={other.id} value={other.id}>
                          {other.name}
                        </option>
                      ))}
                  </select>
                </div>
                <div style={{ flex: "none", width: 84 }}>
                  <div style={labelStyle}>At once</div>
                  <input
                    type="number"
                    min={1}
                    max={8}
                    value={agent.max_concurrent}
                    onChange={(e) =>
                      patch({ max_concurrent: Math.max(1, Number(e.target.value) || 1) })
                    }
                    style={fieldStyle}
                  />
                </div>
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>Worktree</div>
              <PillChoice
                value={agent.worktree}
                options={WORKTREE_MODES.map((w) => ({ id: w.id, name: w.name }))}
                onPick={(id) => patch({ worktree: id })}
              />
              <div style={{ marginTop: 9, fontSize: 11.5, color: "var(--text3)" }}>
                {WORKTREE_MODES.find((w) => w.id === agent.worktree)?.hint}
              </div>

              <Divider />

              <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 11 }}>Reviewed by</div>
              <PillChoice
                value={agent.reviewer}
                options={REVIEWERS.map((r) => ({ id: r.id, name: r.name }))}
                onPick={(id) => patch({ reviewer: id })}
              />
              <div style={{ marginTop: 9, fontSize: 11.5, color: "var(--text3)", lineHeight: 1.5 }}>
                {REVIEWERS.find((r) => r.id === agent.reviewer)?.hint}
                {settings && !settings.director_reviews_first && agent.reviewer === "director"
                  ? " Automatic review is off in Settings, so these land in your queue instead."
                  : ""}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
