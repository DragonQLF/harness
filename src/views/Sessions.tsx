import { useEffect, useMemo, useRef } from "react";
import { api, reason } from "../lib/ipc";
import { duration, money, plural } from "../lib/format";
import { MODELS, STATUS_NAME, STATUS_TONE } from "../lib/types";
import { useStore } from "../state/store";
import { Loading, tabular, truncate } from "../components/ui";

/** Two panes: the list of everything recorded, and the transcript. */
export function Sessions({
  selected,
  select,
  openReject,
}: {
  selected: string | null;
  select: (cardId: string) => void;
  openReject: (cardId: string) => void;
}) {
  const {
    snapshot,
    projectId,
    agents,
    outputs,
    streams,
    loadRunLog,
    startRun,
    cancelRun,
    approve,
    toast,
  } = useStore();
  const end = useRef<HTMLDivElement | null>(null);

  const recorded = useMemo(() => {
    const cards = snapshot?.cards ?? [];
    return cards.filter((c) => c.runs > 0 || c.status === "running" || outputs[c.id]?.length);
  }, [outputs, snapshot]);

  const card = recorded.find((c) => c.id === selected) ?? recorded[0] ?? null;
  const session = snapshot?.sessions.find((s) => s.card_id === card?.id);
  const lines = card ? (outputs[card.id] ?? []) : [];
  const stream = card ? streams[card.id] : undefined;
  const live = card?.status === "running";

  useEffect(() => {
    if (!card || !session?.run_id) return;
    if ((outputs[card.id] ?? []).length > 0) return;
    loadRunLog(session.run_id, card.id);
  }, [card, session?.run_id, outputs, loadRunLog]);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [lines.length, stream?.text, stream?.thinking]);

  if (!snapshot) return <Loading what="Reading sessions" />;

  const agent = agents.find((a) => a.id === card?.agent_id);
  const status = card ? STATUS_TONE[card.status] : STATUS_TONE.backlog;
  const running = recorded.filter((c) => c.status === "running").length;

  return (
    <div style={{ display: "flex", height: "100%", minHeight: 0 }}>
      <div
        style={{
          width: 276,
          flex: "none",
          borderRight: "1px solid var(--line)",
          display: "flex",
          flexDirection: "column",
          minHeight: 0,
          background: "var(--surface)",
        }}
      >
        <div style={{ padding: "18px 18px 12px" }}>
          <h1 style={{ margin: "0 0 4px", fontSize: 17, fontWeight: 800, letterSpacing: "-.02em" }}>
            Sessions
          </h1>
          <p style={{ margin: 0, fontSize: 12, color: "var(--text3)" }}>
            {recorded.length} recorded · {running} live
          </p>
        </div>
        <div
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            padding: "0 10px 12px",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          {recorded.map((c) => {
            const on = c.id === card?.id;
            const t = STATUS_TONE[c.status];
            return (
              <button
                key={c.id}
                type="button"
                className="hv-border"
                onClick={() => select(c.id)}
                style={{
                  textAlign: "left",
                  padding: "11px 12px",
                  border: `1px solid ${on ? "var(--accentLine)" : "transparent"}`,
                  borderRadius: 14,
                  background: on ? "var(--accentSoft)" : "transparent",
                  cursor: "pointer",
                  transition: "all .18s ease",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 5 }}>
                  <span
                    style={{
                      width: 6,
                      height: 6,
                      borderRadius: "50%",
                      flex: "none",
                      background: t.color,
                      animation: c.status === "running" ? "breathe 2.2s ease-in-out infinite" : undefined,
                    }}
                  />
                  <span style={{ fontSize: 13, fontWeight: 600, ...truncate }}>{c.title}</span>
                </div>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    fontSize: 11,
                    color: "var(--text3)",
                  }}
                >
                  <span style={{ fontFamily: "var(--mono)" }}>{c.id}</span>
                  <span style={tabular}>
                    {c.status === "running" && session?.card_id === c.id
                      ? duration(Date.now() - session.started_ms)
                      : money(c.cost_usd, 3)}
                  </span>
                </div>
              </button>
            );
          })}
          {recorded.length === 0 && (
            <div
              style={{
                margin: "6px 2px",
                padding: "16px 14px",
                border: "1px dashed var(--line)",
                borderRadius: 14,
                fontSize: 12,
                color: "var(--text3)",
                lineHeight: 1.5,
              }}
            >
              No sessions in this project yet. Add a card on Work to start one.
            </div>
          )}
        </div>
      </div>

      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
        {!card ? (
          <div style={{ padding: 26, fontSize: 12.5, color: "var(--text3)" }}>
            Nothing recorded yet.
          </div>
        ) : (
          <>
            <div
              style={{
                padding: "18px 22px 14px",
                borderBottom: "1px solid var(--line)",
                background: "var(--surface)",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 9 }}>
                <span
                  style={{
                    width: 32,
                    height: 32,
                    flex: "none",
                    borderRadius: "50%",
                    background: "var(--accentSoft)",
                    color: "var(--accent)",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 12,
                    fontWeight: 700,
                  }}
                >
                  {agent?.initial ?? "?"}
                </span>
                <h2
                  style={{
                    margin: 0,
                    fontSize: 17,
                    fontWeight: 800,
                    letterSpacing: "-.02em",
                    ...truncate,
                  }}
                >
                  {card.title}
                </h2>
                <span
                  style={{
                    fontSize: 11.5,
                    fontWeight: 700,
                    padding: "3px 10px",
                    borderRadius: 999,
                    background: status.soft,
                    color: status.color,
                    flex: "none",
                  }}
                >
                  {STATUS_NAME[card.status]}
                </span>
                <div style={{ flex: 1 }} />
                <button
                  type="button"
                  className="hv-text"
                  disabled={!session?.session_id || !projectId}
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .openAgentTerminal(projectId, card.id)
                      .then(() => toast("var(--info)", "Terminal opened", "Resumed the session"))
                      .catch((e) => toast("var(--bad)", "Could not open a terminal", reason(e)));
                  }}
                  style={{
                    padding: "8px 14px",
                    border: "1px solid var(--line)",
                    borderRadius: 999,
                    background: "transparent",
                    color: "var(--text2)",
                    fontSize: 12,
                    fontWeight: 500,
                    cursor: "pointer",
                    transition: "all .18s ease",
                    opacity: session?.session_id ? 1 : 0.5,
                  }}
                >
                  Attach terminal
                </button>
                {card.status === "review" && (
                  <button
                    type="button"
                    onClick={() => openReject(card.id)}
                    style={{
                      padding: "8px 15px",
                      border: "none",
                      borderRadius: 999,
                      background: "var(--warnSoft)",
                      color: "var(--warn)",
                      fontSize: 12,
                      fontWeight: 700,
                      cursor: "pointer",
                      transition: "filter .18s ease",
                    }}
                  >
                    Send back
                  </button>
                )}
                <button
                  type="button"
                  onClick={() =>
                    card.status === "running"
                      ? cancelRun(card.id)
                      : card.status === "review"
                        ? approve(card.id)
                        : startRun(card.id)
                  }
                  disabled={card.status === "done"}
                  style={{
                    padding: "8px 15px",
                    border: "none",
                    borderRadius: 999,
                    background:
                      card.status === "running"
                        ? "var(--badSoft)"
                        : card.status === "review"
                          ? "var(--okSoft)"
                          : "var(--infoSoft)",
                    color:
                      card.status === "running"
                        ? "var(--bad)"
                        : card.status === "review"
                          ? "var(--ok)"
                          : "var(--info)",
                    fontSize: 12,
                    fontWeight: 700,
                    cursor: card.status === "done" ? "not-allowed" : "pointer",
                    opacity: card.status === "done" ? 0.5 : 1,
                    transition: "filter .18s ease",
                  }}
                >
                  {card.status === "running"
                    ? "Stop"
                    : card.status === "review"
                      ? "Approve"
                      : card.runs > 0
                        ? "Run again"
                        : "Start"}
                </button>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  flexWrap: "wrap",
                  gap: 14,
                  fontSize: 11.5,
                  color: "var(--text3)",
                }}
              >
                <span style={{ fontFamily: "var(--mono)", color: "var(--text2)" }}>{card.id}</span>
                <span style={{ fontFamily: "var(--mono)" }}>{session?.branch ?? "no worktree"}</span>
                <span
                  style={{
                    width: 3,
                    height: 3,
                    borderRadius: "50%",
                    background: "var(--line)",
                    flex: "none",
                  }}
                />
                <span style={tabular}>
                  {live && session
                    ? duration(Date.now() - session.started_ms)
                    : plural(card.turns, "turn")}
                </span>
                <span style={{ fontFamily: "var(--mono)", ...tabular }}>
                  {money(card.cost_usd, 4)}
                </span>
                <span
                  style={{
                    width: 3,
                    height: 3,
                    borderRadius: "50%",
                    background: "var(--line)",
                    flex: "none",
                  }}
                />
                <span>
                  {agent?.name ?? card.agent_id} ·{" "}
                  {MODELS.find((m) => m.id === agent?.model)?.name ?? "auto"}
                </span>
                {session?.session_id && (
                  <span style={{ fontFamily: "var(--mono)" }}>
                    {session.session_id.slice(0, 12)}
                  </span>
                )}
              </div>
            </div>

            <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "16px 24px 24px" }}>
              {lines.length === 0 && (
                <div style={{ fontSize: 12, color: "var(--text3)", fontFamily: "var(--mono)" }}>
                  no transcript for this card yet
                </div>
              )}
              {lines.map((l, i) => (
                <div
                  key={i}
                  style={{
                    display: "flex",
                    gap: 14,
                    fontFamily: "var(--mono)",
                    fontSize: 12,
                    lineHeight: 1.95,
                    animation: "fadeIn .24s ease both",
                  }}
                >
                  <span
                    style={{
                      flex: "none",
                      width: 30,
                      paddingRight: 12,
                      borderRight: "1px solid var(--line2)",
                      textAlign: "right",
                      color: "var(--text3)",
                      opacity: 0.6,
                      ...tabular,
                    }}
                  >
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  <span
                    style={{
                      flex: 1,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      color: l.color,
                    }}
                  >
                    {l.text}
                  </span>
                </div>
              ))}
              {stream?.thinking && !stream.text && (
                <div
                  style={{
                    display: "flex",
                    gap: 14,
                    fontFamily: "var(--mono)",
                    fontSize: 12,
                    lineHeight: 1.95,
                  }}
                >
                  <span
                    style={{
                      flex: "none",
                      width: 30,
                      paddingRight: 12,
                      borderRight: "1px solid var(--line2)",
                      textAlign: "right",
                      color: "var(--text3)",
                      opacity: 0.6,
                    }}
                  >
                    ··
                  </span>
                  <span
                    style={{
                      flex: 1,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      color: "var(--text3)",
                      fontStyle: "italic",
                    }}
                  >
                    {stream.thinking}
                  </span>
                </div>
              )}
              {stream?.text && (
                <div
                  style={{
                    display: "flex",
                    gap: 14,
                    fontFamily: "var(--mono)",
                    fontSize: 12,
                    lineHeight: 1.95,
                  }}
                >
                  <span
                    style={{
                      flex: "none",
                      width: 30,
                      paddingRight: 12,
                      borderRight: "1px solid var(--line2)",
                      textAlign: "right",
                      color: "var(--text3)",
                      opacity: 0.6,
                      ...tabular,
                    }}
                  >
                    {String(lines.length + 1).padStart(2, "0")}
                  </span>
                  <span
                    style={{
                      flex: 1,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      color: "var(--text)",
                    }}
                  >
                    {stream.text}
                  </span>
                </div>
              )}
              {live && (
                <div style={{ display: "flex", gap: 14 }}>
                  <span
                    style={{
                      flex: "none",
                      width: 30,
                      paddingRight: 12,
                      borderRight: "1px solid var(--line2)",
                      height: 20,
                    }}
                  />
                  <span
                    style={{
                      display: "inline-block",
                      width: 7,
                      height: 14,
                      background: "var(--accent)",
                      animation: "blink 1.05s steps(1) infinite",
                    }}
                  />
                </div>
              )}
              <div ref={end} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}
