import { useEffect, useMemo, useRef } from "react";
import { api, reason } from "../lib/ipc";
import { duration, money, plural } from "../lib/format";
import { MODELS, STATUS_NAME, STATUS_TONE, tone } from "../lib/types";
import { useStore } from "../state/store";
import { Caret, Glyph, Loading, mono, truncate } from "../components/ui";

const STATUS_DOT: Record<string, string> = {
  backlog: "var(--text4)",
  ready: "var(--info)",
  running: "var(--accent)",
  review: "var(--warn)",
  done: "var(--ok)",
};

/** Two panes: everything recorded, and the transcript of the one you picked. */
export function Sessions({
  selected,
  select,
  openReview,
}: {
  selected: string | null;
  select: (cardId: string) => void;
  openReview: (cardId: string) => void;
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
  const at = tone(agent?.tone);
  const status = card ? STATUS_TONE[card.status] : STATUS_TONE.backlog;

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "grid",
        gridTemplateColumns: "280px minmax(0,1fr)",
        overflow: "hidden",
        animation: "paneIn .4s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div
        style={{
          minWidth: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderRight: "1px solid var(--line)",
          overflow: "hidden",
        }}
      >
        <div
          className="stagger"
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            padding: "12px 10px",
            display: "flex",
            flexDirection: "column",
            gap: 4,
          }}
        >
          {recorded.map((c) => {
            const on = c.id === card?.id;
            const who = agents.find((a) => a.id === c.agent_id);
            const wt = tone(who?.tone);
            const s = snapshot.sessions.find((x) => x.card_id === c.id);
            const right =
              c.status === "running" && s
                ? duration(Date.now() - s.started_ms)
                : c.cost_usd > 0
                  ? money(c.cost_usd, 3)
                  : plural(c.turns, "turn");
            return (
              <div
                key={c.id}
                className="row"
                onClick={() => select(c.id)}
                style={{
                  padding: "10px 11px",
                  borderRadius: 12,
                  border: `1px solid ${on ? "var(--accentLine)" : "transparent"}`,
                  background: on ? "var(--accentSoft)" : "transparent",
                  cursor: "pointer",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 5 }}>
                  <span
                    style={{
                      width: 6,
                      height: 6,
                      flex: "none",
                      borderRadius: "50%",
                      background: STATUS_DOT[c.status],
                      animation: c.status === "running" ? "pulse 2.4s ease-in-out infinite" : undefined,
                    }}
                  />
                  <span style={{ flex: 1, font: "500 12.5px var(--sans)", color: "var(--text)", ...truncate }}>
                    {c.title}
                  </span>
                  <Glyph color={wt.color} soft={wt.soft} size={16} font={8}>
                    {who?.initial ?? "?"}
                  </Glyph>
                </div>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    ...mono,
                    fontSize: 10.5,
                    color: "var(--text3)",
                  }}
                >
                  <span>
                    {c.id}
                    {c.status === "running" ? " · live" : ""}
                  </span>
                  <span>{right}</span>
                </div>
              </div>
            );
          })}
          {recorded.length === 0 && (
            <div
              style={{
                margin: "6px 2px",
                padding: "16px 14px",
                border: "1px dashed var(--line2)",
                borderRadius: 12,
                font: "400 11.5px/1.6 var(--sans)",
                color: "var(--text4)",
              }}
            >
              No session in this project yet. Start a card on the board and its transcript arrives
              here.
            </div>
          )}
        </div>
      </div>

      <div
        style={{
          minWidth: 0,
          minHeight: 0,
          display: "grid",
          gridTemplateRows: "auto minmax(0,1fr)",
          overflow: "hidden",
        }}
      >
        {!card ? (
          <div style={{ padding: 26, font: "400 12.5px var(--sans)", color: "var(--text4)" }}>
            Nothing recorded yet.
          </div>
        ) : (
          <>
            <div style={{ padding: "14px 22px 12px", borderBottom: "1px solid var(--line)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 9 }}>
                <Glyph color={at.color} soft={at.soft} size={26} radius="50%" font={10}>
                  {agent?.initial ?? "?"}
                </Glyph>
                <span
                  style={{
                    font: "600 14.5px var(--sans)",
                    color: "var(--text)",
                    letterSpacing: "-.01em",
                    ...truncate,
                  }}
                >
                  {card.title}
                </span>
                <span
                  style={{
                    flex: "none",
                    padding: "3px 10px",
                    borderRadius: 999,
                    background: status.soft,
                    color: status.color,
                    font: "600 11px var(--sans)",
                  }}
                >
                  {STATUS_NAME[card.status]}
                </span>
                <div style={{ flex: 1 }} />
                <span
                  className="chip"
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .openAgentTerminal(projectId, card.id)
                      .then(() => toast("var(--info)", "Terminal opened", "Resumed the session"))
                      .catch((e) => toast("var(--bad)", "Could not open a terminal", reason(e)));
                  }}
                  style={{
                    padding: "7px 13px",
                    borderRadius: 999,
                    border: "1px solid var(--line3)",
                    font: "400 11.5px var(--sans)",
                    color: "var(--text2)",
                    cursor: "pointer",
                    opacity: card.session_id ? 1 : 0.5,
                  }}
                >
                  Attach terminal
                </span>
                <span
                  className="chip"
                  onClick={() => {
                    if (!session?.worktree) {
                      toast("var(--warn)", "No worktree", "This card has not written anything yet.");
                      return;
                    }
                    api.reveal(session.worktree).catch((e) =>
                      toast("var(--bad)", "Could not open that folder", reason(e)),
                    );
                  }}
                  style={{
                    padding: "7px 13px",
                    borderRadius: 999,
                    border: "1px solid var(--line3)",
                    font: "400 11.5px var(--sans)",
                    color: "var(--text2)",
                    cursor: "pointer",
                  }}
                >
                  Reveal worktree
                </span>
                <span
                  className="primary"
                  onClick={() =>
                    card.status === "running"
                      ? cancelRun(card.id)
                      : card.status === "review"
                        ? openReview(card.id)
                        : startRun(card.id)
                  }
                  style={{
                    padding: "7px 14px",
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
                    font: "600 11.5px var(--sans)",
                    cursor: "pointer",
                  }}
                >
                  {card.status === "running"
                    ? "Stop"
                    : card.status === "review"
                      ? "Read the diff"
                      : card.runs > 0
                        ? "Run again"
                        : "Start"}
                </span>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  flexWrap: "wrap",
                  gap: 13,
                  ...mono,
                  fontSize: 11,
                  color: "var(--text3)",
                }}
              >
                <span style={{ color: "var(--text2)" }}>{card.id}</span>
                {session?.run_id && (
                  <span>
                    {session.run_id.slice(0, 8)} · {plural(card.runs, "run")}
                  </span>
                )}
                <span style={{ width: 3, height: 3, borderRadius: "50%", background: "var(--line3)" }} />
                <span>{session?.branch ?? card.branch ?? "no worktree"}</span>
                <span>{plural(card.turns, "turn")}</span>
                <span>{money(card.cost_usd, 4)}</span>
                <span style={{ width: 3, height: 3, borderRadius: "50%", background: "var(--line3)" }} />
                <span>
                  {agent?.name ?? card.agent_id} ·{" "}
                  {MODELS.find((m) => m.id === agent?.model)?.name ?? "auto"}
                </span>
                {card.session_id && <span>{card.session_id.slice(0, 12)}</span>}
                {card.runs > 1 && card.session_id && (
                  <span style={{ color: "var(--ok)" }}>resumed from the last run</span>
                )}
              </div>
            </div>

            <div
              className="stagger logscroll"
              style={{ minHeight: 0, overflowY: "auto", padding: "14px 22px 20px" }}
            >
              {lines.length === 0 && (
                <div style={{ ...mono, fontSize: 12, color: "var(--text4)" }}>
                  no transcript for this card yet
                </div>
              )}
              {lines.map((l, i) => (
                <div key={i} style={{ display: "flex", gap: 12, ...mono, fontSize: 12, lineHeight: 1.9 }}>
                  <span
                    style={{
                      flex: "none",
                      width: 74,
                      textAlign: "right",
                      color: l.labelColor,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {l.label}
                  </span>
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      paddingLeft: 12,
                      borderLeft: "1px solid var(--line)",
                      color: l.color,
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      fontStyle: l.italic ? "italic" : "normal",
                    }}
                  >
                    {l.text}
                  </span>
                </div>
              ))}

              {stream?.thinking && !stream.text && (
                <div style={{ display: "flex", gap: 12, ...mono, fontSize: 12, lineHeight: 1.9 }}>
                  <span style={{ flex: "none", width: 74, textAlign: "right", color: "var(--text4)" }}>
                    thinking
                  </span>
                  <span
                    style={{
                      flex: 1,
                      paddingLeft: 12,
                      borderLeft: "1px solid var(--line)",
                      color: "var(--text3)",
                      fontStyle: "italic",
                      whiteSpace: "pre-wrap",
                    }}
                  >
                    {stream.thinking}
                    <Caret />
                  </span>
                </div>
              )}
              {stream?.text && (
                <div style={{ display: "flex", gap: 12, ...mono, fontSize: 12, lineHeight: 1.9 }}>
                  <span style={{ flex: "none", width: 74, textAlign: "right", color: "var(--text4)" }}>
                    text
                  </span>
                  <span
                    style={{
                      flex: 1,
                      paddingLeft: 12,
                      borderLeft: "1px solid var(--line)",
                      color: "var(--text2)",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                    }}
                  >
                    {stream.text}
                    <Caret />
                  </span>
                </div>
              )}
              {live && !stream?.text && !stream?.thinking && (
                <div style={{ display: "flex", gap: 12, ...mono, fontSize: 12, lineHeight: 1.9 }}>
                  <span style={{ flex: "none", width: 74, textAlign: "right", color: "var(--text4)" }}>
                    live
                  </span>
                  <span style={{ flex: 1, paddingLeft: 12, borderLeft: "1px solid var(--line)" }}>
                    <Caret />
                  </span>
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
