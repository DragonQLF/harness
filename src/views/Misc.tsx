/** Worktrees, Activity and Settings. The Director now lives on the chat
 *  screen, so nothing here needs a page of its own. */

import { useEffect, useState, type ReactNode } from "react";
import { api, events, reason } from "../lib/ipc";
import { clock, money } from "../lib/format";
import { ruleIsRevoked, ruleLabel, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { Loading, Switch, tabular, truncate } from "../components/ui";

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

/** The accent choices, drawn from the palette rather than from four loose
 *  hexes. The first follows the theme's own accent, which is what an operator
 *  who never opened this row is already using. */
const ACCENTS: { name: string; value: string; swatch: string }[] = [
  { name: "Theme default", value: "", swatch: "var(--accent)" },
  { name: "Mint", value: "#4fd1a5", swatch: "#4fd1a5" },
  { name: "Periwinkle", value: "#9b8cff", swatch: "#9b8cff" },
  { name: "Amber", value: "#ffb35c", swatch: "#ffb35c" },
  { name: "Rose", value: "#ff6b81", swatch: "#ff6b81" },
];

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
            {ACCENTS.map((a) => {
              const picked = settings.accent === a.value;
              return (
                <button
                  key={a.value || "theme"}
                  type="button"
                  title={a.name}
                  aria-label={a.name}
                  aria-pressed={picked}
                  onClick={() => saveSettings({ accent: a.value })}
                  style={{
                    width: 20,
                    height: 20,
                    padding: 0,
                    borderRadius: "50%",
                    background: a.swatch,
                    border: picked
                      ? "2px solid var(--text)"
                      : "1px solid var(--line3)",
                    boxShadow: picked ? "0 0 0 3px var(--accentSoft)" : "none",
                    cursor: "pointer",
                    transition: "box-shadow .16s ease, border-color .16s ease",
                  }}
                />
              );
            })}
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
            note="Calls Relay stops asking about. Each one is scoped to the command it came from, so allowing git push does not allow every shell command. Click one to take it back."
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
            the repositories you point Relay at.
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
