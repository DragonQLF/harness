/** The crew: who exists, and what each one is allowed to do. The left pane is
 *  the roster grouped by team; the right pane is one profile, editable in
 *  place. Every change is saved through the backend, never held here. */

import { useEffect, useMemo, useState } from "react";
import { money, num, plural } from "../lib/format";
import {
  ALL_PERMISSIONS,
  MODELS,
  REVIEWERS,
  WORKTREE_MODES,
  tone,
  type AgentProfile,
} from "../lib/types";
import { useStore } from "../state/store";
import { Eyebrow, Glyph, mono, truncate } from "../components/ui";

function stateOf(agent: AgentProfile, running: number) {
  if (agent.paused) return { label: "paused", color: "var(--text4)" };
  if (running > 0) return { label: `${running} running`, color: "var(--ok)" };
  if (agent.id === "director") return { label: "chat", color: "var(--warn)" };
  return { label: "idle", color: "var(--text4)" };
}

/** The template footer. Nothing is fetched until this mounts, and nothing is
 *  created until a name is picked: a template is a menu entry. */
function Templates() {
  const { agentTemplates, createAgentFromTemplate, agents, saveAgents } = useStore();
  const [templates, setTemplates] = useState<AgentProfile[] | null>(null);

  useEffect(() => {
    let alive = true;
    agentTemplates().then((list) => alive && setTemplates(list));
    return () => {
      alive = false;
    };
  }, [agentTemplates]);

  /** A profile from nothing. The id is settled here so two customs in a row
   *  cannot collide on the same name. */
  const custom = () => {
    const taken = new Set(agents.map((a) => a.id));
    let id = "new-agent";
    for (let n = 2; taken.has(id); n += 1) id = `new-agent-${n}`;
    saveAgents([
      ...agents,
      {
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
      },
    ]);
  };

  return (
    <div style={{ flex: "none", borderTop: "1px solid var(--line)", padding: "11px 12px 13px" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 7, paddingBottom: 8 }}>
        <span style={{ font: "600 11px var(--sans)", color: "var(--text2)" }}>New from template</span>
        <div style={{ flex: 1 }} />
        <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>
          {templates == null ? "…" : templates.length}
        </span>
      </div>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
        {(templates ?? []).map((t) => {
          const already = agents.some((a) => a.id === t.id);
          return (
            <span
              key={t.id}
              className="chip"
              title={already ? `${t.name} — you already have one` : t.role}
              onClick={() => createAgentFromTemplate(t.id)}
              style={{
                padding: "4px 9px",
                borderRadius: 999,
                border: "1px solid var(--line3)",
                font: "400 10.5px var(--sans)",
                color: already ? "var(--text4)" : "var(--text2)",
                cursor: "pointer",
              }}
            >
              {t.name}
            </span>
          );
        })}
        <span
          className="chip"
          onClick={custom}
          style={{
            padding: "4px 9px",
            borderRadius: 999,
            border: "1px dashed var(--line3)",
            font: "400 10.5px var(--sans)",
            color: "var(--text4)",
            cursor: "pointer",
          }}
        >
          custom
        </span>
      </div>
      <div style={{ paddingTop: 9, font: "400 10px/1.5 var(--sans)", color: "var(--text4)" }}>
        A template is a menu entry. Nothing is installed until you pick one.
      </div>
    </div>
  );
}

/** One of the five knobs across the top of a profile. Clicking cycles it. */
function Knob({
  label,
  value,
  hint,
  onCycle,
}: {
  label: string;
  value: string;
  hint: string;
  onCycle: () => void;
}) {
  return (
    <div
      className="row"
      onClick={onCycle}
      style={{ padding: "11px 13px", background: "var(--surface)", cursor: "pointer" }}
    >
      <div style={{ font: "400 10px var(--sans)", color: "var(--text4)", letterSpacing: ".06em" }}>
        {label}
      </div>
      <div style={{ marginTop: 4, font: "600 12.5px var(--sans)", color: "var(--text1)" }}>{value}</div>
      <div style={{ marginTop: 3, font: "400 10px/1.4 var(--sans)", color: "var(--text4)" }}>{hint}</div>
    </div>
  );
}

function Toggle({
  label,
  hint,
  on,
  onChange,
}: {
  label: string;
  hint: string;
  on: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      className="row"
      onClick={() => onChange(!on)}
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 11,
        padding: "12px 13px",
        borderBottom: "1px solid var(--line)",
        cursor: "pointer",
      }}
    >
      <span
        style={{
          width: 30,
          height: 17,
          flex: "none",
          marginTop: 1,
          borderRadius: 999,
          background: on ? "var(--accentLine)" : "var(--line3)",
          display: "flex",
          alignItems: "center",
          padding: 2,
          justifyContent: on ? "flex-end" : "flex-start",
          transition: "background .18s ease",
        }}
      >
        <span
          style={{
            width: 13,
            height: 13,
            borderRadius: "50%",
            background: on ? "var(--accent2)" : "#5a564e",
            transition: "background .18s ease",
          }}
        />
      </span>
      <span style={{ flex: 1, minWidth: 0 }}>
        <span style={{ display: "block", font: "500 12px var(--sans)", color: "var(--text1)" }}>
          {label}
        </span>
        <span
          style={{ display: "block", marginTop: 2, font: "400 10.5px/1.5 var(--sans)", color: "var(--text4)" }}
        >
          {hint}
        </span>
      </span>
    </div>
  );
}

export function Agents({
  selected,
  select,
  openChat,
  openSession,
}: {
  selected: string | null;
  select: (id: string) => void;
  openChat: (conversationId?: string, profileId?: string) => void;
  openSession: (cardId: string) => void;
}) {
  const {
    agents,
    agentStats,
    snapshot,
    settings,
    saveAgents,
    duplicateAgent,
    removeAgent,
    conversations,
  } = useStore();

  const cards = snapshot?.cards ?? [];
  const agent = agents.find((a) => a.id === selected) ?? agents[0] ?? null;
  const [skill, setSkill] = useState("");

  const teams = useMemo(() => {
    const groups = new Map<string, AgentProfile[]>();
    agents.forEach((a) => {
      const key = (a.team || "other").toUpperCase();
      groups.set(key, [...(groups.get(key) ?? []), a]);
    });
    return [...groups.entries()];
  }, [agents]);

  if (!agent) {
    return (
      <div style={{ flex: 1, display: "grid", placeItems: "center", color: "var(--text3)" }}>
        No profiles yet.
      </div>
    );
  }

  const patch = (next: Partial<AgentProfile>) =>
    saveAgents(agents.map((a) => (a.id === agent.id ? { ...a, ...next } : a)));

  const stats = agentStats[agent.id];
  const t = tone(agent.tone);
  const mine = cards.filter((c) => c.agent_id === agent.id);
  const running = mine.filter((c) => c.status === "running").length;
  const st = stateOf(agent, running);
  const chats = conversations.filter((c) => c.profile_id === agent.id);

  // Each knob steps through its own list, so the whole strip is editable
  // without a form.
  const cycle = <T,>(list: T[], current: T): T => {
    const at = list.findIndex((x) => x === current);
    return list[(at + 1) % list.length]!;
  };
  const budgets = [0.25, 0.5, 1, 2, 5, null];

  const knobs = [
    {
      label: "MODEL",
      value: MODELS.find((m) => m.id === agent.model)?.name ?? "auto",
      hint: MODELS.find((m) => m.id === agent.model)?.hint ?? "Claude picks one",
      onCycle: () => patch({ model: cycle(MODELS.map((m) => m.id), agent.model ?? "sonnet") }),
    },
    {
      label: "REVIEWER",
      value: REVIEWERS.find((r) => r.id === agent.reviewer)!.name,
      hint:
        agent.reviewer === "director"
          ? "Reads the diff before you do"
          : agent.reviewer === "human"
            ? "Every run lands in your queue"
            : "Finished runs go straight to Done",
      onCycle: () => patch({ reviewer: cycle(REVIEWERS.map((r) => r.id), agent.reviewer) }),
    },
    {
      label: "WORKTREE",
      value: WORKTREE_MODES.find((w) => w.id === agent.worktree)!.name,
      hint: WORKTREE_MODES.find((w) => w.id === agent.worktree)!.hint,
      onCycle: () => patch({ worktree: cycle(WORKTREE_MODES.map((w) => w.id), agent.worktree) }),
    },
    {
      label: "AT ONCE",
      value: plural(agent.max_concurrent, "card"),
      hint: `A ${agent.max_concurrent === 1 ? "second" : "further"} card waits`,
      onCycle: () => patch({ max_concurrent: (agent.max_concurrent % 4) + 1 }),
    },
    {
      label: "BUDGET",
      value: agent.budget_usd == null ? "no cap" : money(agent.budget_usd),
      hint: settings ? `Counts against ${money(settings.daily_budget_usd, 0)} a day` : "per run",
      onCycle: () => patch({ budget_usd: cycle(budgets, agent.budget_usd) }),
    },
  ];

  const week = stats?.week_runs ?? [0, 0, 0, 0, 0, 0, 0];
  const peak = Math.max(1, ...week);
  const numbers = [
    { k: "runs", v: num(stats?.runs ?? 0), color: "var(--text1)" },
    { k: "cards done", v: num(stats?.cards_done ?? 0), color: "var(--ok)" },
    { k: "sent back", v: num(stats?.sent_back ?? 0), color: "var(--warn)" },
    { k: "spend", v: money(stats?.spend ?? 0), color: "var(--text1)" },
    { k: "avg / card", v: money(stats?.avg_cost ?? 0), color: "var(--text1)" },
    { k: "commits", v: num(stats?.commits ?? 0), color: "var(--text1)" },
  ];

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "grid",
        gridTemplateColumns: "266px minmax(0,1fr)",
        overflow: "hidden",
        animation: "paneIn .4s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div
        style={{
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderRight: "1px solid var(--line)",
          overflow: "hidden",
        }}
      >
        <div className="stagger" style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "10px 9px 12px" }}>
          {teams.map(([team, members]) => (
            <div key={team}>
              <Eyebrow style={{ display: "block", padding: "9px 8px 5px" }}>{team}</Eyebrow>
              {members.map((a) => {
                const at = tone(a.tone);
                const on = a.id === agent.id;
                const busy = cards.filter((c) => c.status === "running" && c.agent_id === a.id).length;
                const state = stateOf(a, busy);
                return (
                  <div
                    key={a.id}
                    className="row"
                    onClick={() => select(a.id)}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 9,
                      padding: "8px 9px",
                      borderRadius: 10,
                      cursor: "pointer",
                      background: on ? "var(--active)" : "transparent",
                      boxShadow: on ? "inset 0 0 0 1px var(--line3)" : "none",
                    }}
                  >
                    <Glyph color={at.color} soft={at.soft} size={24} radius={8} font={9.5}>
                      {a.initial}
                    </Glyph>
                    <span style={{ flex: 1, minWidth: 0 }}>
                      <span
                        style={{
                          display: "block",
                          font: "600 12px var(--sans)",
                          color: on ? "var(--text)" : "var(--text1)",
                          ...truncate,
                        }}
                      >
                        {a.name}
                      </span>
                      <span
                        style={{ display: "block", ...mono, fontSize: 10, color: "var(--text4)", ...truncate }}
                      >
                        {a.title} · {a.model ?? "auto"}
                      </span>
                    </span>
                    <span style={{ font: "500 9.5px var(--sans)", color: state.color }}>
                      {state.label}
                    </span>
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        <Templates />
      </div>

      <div style={{ minWidth: 0, minHeight: 0, overflowY: "auto" }}>
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 13,
            padding: "18px 22px 14px",
            borderBottom: "1px solid var(--line)",
            animation: "rowIn .4s cubic-bezier(.2,.8,.25,1) both",
          }}
        >
          <Glyph color={t.color} soft={t.soft} size={38} radius={12} font={14}>
            {agent.initial}
          </Glyph>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
              <input
                value={agent.name}
                onChange={(e) => patch({ name: e.target.value })}
                style={{
                  border: "none",
                  background: "transparent",
                  outline: "none",
                  padding: 0,
                  font: "600 16px var(--sans)",
                  color: "var(--text)",
                  letterSpacing: "-.01em",
                  width: `${Math.max(6, agent.name.length + 1)}ch`,
                }}
              />
              <input
                value={agent.team}
                onChange={(e) => patch({ team: e.target.value })}
                placeholder="team"
                style={{
                  padding: "2px 8px",
                  borderRadius: 6,
                  border: "none",
                  background: "var(--surface2)",
                  outline: "none",
                  ...mono,
                  fontSize: 10,
                  color: "var(--text2)",
                  width: 108,
                }}
              />
              <span
                style={{
                  padding: "2px 8px",
                  borderRadius: 6,
                  background: running > 0 ? "var(--okSoft)" : "var(--surface2)",
                  font: "600 10px var(--sans)",
                  color: running > 0 ? "var(--ok)" : "var(--text3)",
                }}
              >
                {st.label}
              </span>
            </div>
            <input
              value={agent.role}
              onChange={(e) => patch({ role: e.target.value })}
              placeholder="What is this one for?"
              style={{
                display: "block",
                width: "100%",
                marginTop: 3,
                border: "none",
                background: "transparent",
                outline: "none",
                padding: 0,
                font: "400 12px var(--sans)",
                color: "var(--text3)",
              }}
            />
          </div>
          {[
            {
              label: "Chat",
              run: () => openChat(chats[0]?.id, agent.id),
              off: !agent.chat_enabled || agent.paused,
            },
            { label: "Duplicate", run: () => duplicateAgent(agent.id), off: false },
            {
              label: agent.paused ? "Resume" : "Pause",
              run: () => patch({ paused: !agent.paused }),
              off: false,
            },
          ].map((b) => (
            <span
              key={b.label}
              className="chip"
              onClick={() => !b.off && b.run()}
              style={{
                padding: "7px 13px",
                borderRadius: 9,
                border: "1px solid var(--line3)",
                font: "500 11.5px var(--sans)",
                color: "var(--text2)",
                cursor: b.off ? "not-allowed" : "pointer",
                opacity: b.off ? 0.45 : 1,
              }}
            >
              {b.label}
            </span>
          ))}
        </div>

        <div className="stagger" style={{ padding: "16px 22px 20px", display: "flex", flexDirection: "column", gap: 18 }}>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(5,minmax(0,1fr))",
              gap: 1,
              background: "var(--line)",
              border: "1px solid var(--line)",
              borderRadius: 12,
              overflow: "hidden",
            }}
          >
            {knobs.map((k) => (
              <Knob key={k.label} {...k} />
            ))}
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1.35fr) minmax(0,1fr)", gap: 16 }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              <div>
                <Eyebrow style={{ display: "block", paddingBottom: 6 }}>BRIEF</Eyebrow>
                <textarea
                  rows={4}
                  value={agent.brief}
                  onChange={(e) => patch({ brief: e.target.value })}
                  placeholder="What is it told before every run?"
                  style={{
                    width: "100%",
                    resize: "vertical",
                    padding: "12px 14px",
                    borderRadius: 11,
                    background: "var(--surface)",
                    border: "1px solid var(--line2)",
                    font: "400 12.5px/1.7 var(--sans)",
                    color: "var(--text2)",
                    outline: "none",
                  }}
                />
              </div>
              <div>
                <Eyebrow style={{ display: "block", paddingBottom: 6 }}>EXPECTED OUTPUT</Eyebrow>
                <textarea
                  rows={2}
                  value={agent.expected_output}
                  onChange={(e) => patch({ expected_output: e.target.value })}
                  placeholder="What finished work looks like."
                  style={{
                    width: "100%",
                    resize: "vertical",
                    padding: "12px 14px",
                    borderRadius: 11,
                    background: "var(--surface)",
                    border: "1px solid var(--line2)",
                    font: "400 12.5px/1.7 var(--sans)",
                    color: "var(--text2)",
                    outline: "none",
                  }}
                />
              </div>
              <div>
                <Eyebrow style={{ display: "block", paddingBottom: 7 }}>TOOLS IT MAY USE</Eyebrow>
                <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                  {ALL_PERMISSIONS.map((p) => {
                    const on = agent.permissions.includes(p);
                    return (
                      <span
                        key={p}
                        onClick={() =>
                          patch({
                            permissions: on
                              ? agent.permissions.filter((x) => x !== p)
                              : [...agent.permissions, p],
                          })
                        }
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 6,
                          padding: "5px 10px",
                          borderRadius: 8,
                          background: on ? "var(--accentSoft)" : "var(--surface)",
                          border: `1px solid ${on ? "var(--accentLine)" : "var(--line2)"}`,
                          font: "500 11.5px var(--sans)",
                          color: on ? "var(--text1)" : "var(--text4)",
                          cursor: "pointer",
                        }}
                      >
                        <span
                          style={{
                            width: 11,
                            height: 11,
                            borderRadius: 4,
                            border: `1px solid ${on ? "var(--accent2)" : "#3a3833"}`,
                            background: on ? "var(--accent2)" : "transparent",
                          }}
                        />
                        {p}
                      </span>
                    );
                  })}
                </div>
                <div style={{ paddingTop: 7, font: "400 10.5px/1.5 var(--sans)", color: "var(--text4)" }}>
                  Anything outside this list is refused before it runs. Anything inside it still asks
                  you, unless a scoped standing rule covers it.
                </div>
              </div>
              <div>
                <Eyebrow style={{ display: "block", paddingBottom: 7 }}>SKILLS</Eyebrow>
                <div style={{ display: "flex", flexWrap: "wrap", gap: 6, alignItems: "center" }}>
                  {agent.skills.map((s) => (
                    <span
                      key={s}
                      title="Remove"
                      onClick={() => patch({ skills: agent.skills.filter((x) => x !== s) })}
                      style={{
                        padding: "4px 10px",
                        borderRadius: 999,
                        background: "var(--surface)",
                        border: "1px solid var(--line2)",
                        ...mono,
                        fontSize: 11,
                        color: "var(--text2)",
                        cursor: "pointer",
                      }}
                    >
                      {s}
                    </span>
                  ))}
                  <input
                    value={skill}
                    onChange={(e) => setSkill(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key !== "Enter" || !skill.trim()) return;
                      patch({ skills: [...agent.skills, skill.trim()] });
                      setSkill("");
                    }}
                    placeholder="add"
                    style={{
                      width: 96,
                      padding: "4px 10px",
                      borderRadius: 999,
                      border: "1px dashed var(--line3)",
                      background: "transparent",
                      outline: "none",
                      font: "400 11px var(--sans)",
                      color: "var(--text2)",
                    }}
                  />
                </div>
              </div>
              <div>
                <Eyebrow style={{ display: "block", paddingBottom: 7 }}>WHERE IT SITS</Eyebrow>
                <div style={{ display: "flex", gap: 10 }}>
                  {[
                    {
                      label: "reports to",
                      value: agent.reports_to ?? "",
                      set: (v: string) => patch({ reports_to: v || null }),
                      none: "Nobody",
                    },
                    {
                      label: "escalates to",
                      value: agent.escalate_to ?? "",
                      set: (v: string) => patch({ escalate_to: v || null }),
                      none: "You",
                    },
                  ].map((f) => (
                    <div key={f.label} style={{ flex: 1 }}>
                      <div style={{ ...mono, fontSize: 10, color: "var(--text4)", paddingBottom: 4 }}>
                        {f.label}
                      </div>
                      <select
                        value={f.value}
                        onChange={(e) => f.set(e.target.value)}
                        style={{
                          width: "100%",
                          padding: "7px 9px",
                          borderRadius: 9,
                          border: "1px solid var(--line2)",
                          background: "var(--surface)",
                          font: "400 12px var(--sans)",
                          color: "var(--text2)",
                          cursor: "pointer",
                        }}
                      >
                        <option value="">{f.none}</option>
                        {agents
                          .filter((o) => o.id !== agent.id)
                          .map((o) => (
                            <option key={o.id} value={o.id}>
                              {o.name}
                            </option>
                          ))}
                      </select>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              <div
                style={{
                  borderRadius: 12,
                  background: "var(--surface)",
                  border: "1px solid var(--line2)",
                  overflow: "hidden",
                }}
              >
                <Toggle
                  label="Can hold a conversation"
                  hint="Gets its own chat and its own resumable session."
                  on={agent.chat_enabled}
                  onChange={(v) => patch({ chat_enabled: v })}
                />
                <Toggle
                  label="Can be given cards"
                  hint="Turn this off and the board will not offer it."
                  on={agent.tasks_enabled}
                  onChange={(v) => patch({ tasks_enabled: v })}
                />
                <Toggle
                  label="Can put work on a board for others"
                  hint="Off: it can describe work, not create or move cards."
                  on={agent.can_delegate}
                  onChange={(v) => patch({ can_delegate: v })}
                />
                <div style={{ padding: "11px 13px", font: "400 10.5px/1.5 var(--sans)", color: "var(--text4)" }}>
                  Board changes an agent makes still come to you as a permission request — the same
                  sheet a shell command uses.
                </div>
              </div>

              <div
                style={{
                  borderRadius: 12,
                  background: "var(--surface)",
                  border: "1px solid var(--line2)",
                  padding: 13,
                }}
              >
                <div style={{ display: "flex", alignItems: "baseline", gap: 8, paddingBottom: 10 }}>
                  <span style={{ font: "600 11.5px var(--sans)", color: "var(--text1)" }}>
                    What it has done
                  </span>
                  <div style={{ flex: 1 }} />
                  <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>all time</span>
                </div>
                <div
                  style={{
                    display: "grid",
                    gridTemplateColumns: "repeat(3,minmax(0,1fr))",
                    gap: "11px 8px",
                  }}
                >
                  {numbers.map((n) => (
                    <div key={n.k}>
                      <div style={{ ...mono, fontSize: 15, fontWeight: 600, color: n.color }}>{n.v}</div>
                      <div style={{ marginTop: 1, font: "400 10px var(--sans)", color: "var(--text4)" }}>
                        {n.k}
                      </div>
                    </div>
                  ))}
                </div>
                <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height: 34, paddingTop: 12 }}>
                  {week.map((v, i) => (
                    <span
                      key={i}
                      style={{
                        flex: 1,
                        height: `${Math.max(6, Math.round((v / peak) * 100))}%`,
                        borderRadius: 2,
                        background: v === peak && v > 0 ? "var(--accent)" : "var(--line3)",
                        transformOrigin: "bottom",
                        animation: `grow .55s cubic-bezier(.2,.8,.25,1) ${0.06 + i * 0.05}s both`,
                      }}
                    />
                  ))}
                </div>
                <div style={{ paddingTop: 5, ...mono, fontSize: 10, color: "var(--text4)" }}>
                  runs, last 7 days
                </div>
              </div>

              {mine.length > 0 && (
                <div
                  style={{
                    borderRadius: 12,
                    background: "var(--surface)",
                    border: "1px solid var(--line2)",
                    overflow: "hidden",
                  }}
                >
                  <div style={{ padding: "11px 13px 8px", font: "600 11.5px var(--sans)", color: "var(--text1)" }}>
                    Its cards here
                  </div>
                  {mine.slice(0, 6).map((c) => (
                    <div
                      key={c.id}
                      className="row"
                      onClick={() => openSession(c.id)}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 9,
                        padding: "8px 13px",
                        borderTop: "1px solid var(--line)",
                        cursor: "pointer",
                      }}
                    >
                      <span style={{ flex: 1, font: "400 11.5px var(--sans)", color: "var(--text2)", ...truncate }}>
                        {c.title}
                      </span>
                      <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>
                        {money(c.cost_usd, 2)}
                      </span>
                    </div>
                  ))}
                </div>
              )}

              <div style={{ padding: "11px 13px", borderRadius: 12, border: "1px solid rgba(255,107,129,.22)" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
                  <span style={{ flex: 1, font: "500 11.5px var(--sans)", color: "var(--text2)" }}>
                    Remove this profile
                  </span>
                  <span
                    onClick={() => agent.id !== "director" && removeAgent(agent.id)}
                    style={{
                      padding: "5px 11px",
                      borderRadius: 8,
                      border: "1px solid rgba(255,107,129,.35)",
                      color: "var(--bad2)",
                      font: "600 11px var(--sans)",
                      cursor: agent.id === "director" ? "not-allowed" : "pointer",
                      opacity: agent.id === "director" ? 0.45 : 1,
                    }}
                  >
                    Remove
                  </span>
                </div>
                <div style={{ paddingTop: 6, font: "400 10.5px/1.5 var(--sans)", color: "var(--text4)" }}>
                  Finished cards keep their history. The Director cannot be removed — the review loop
                  needs it.
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
