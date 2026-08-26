import { useEffect, useRef, useState } from "react";
import { money, plural } from "../lib/format";
import {
  STATUS_NAME,
  STATUS_ORDER,
  tone,
  type Status,
} from "../lib/types";
import { useStore } from "../state/store";
import { Glyph, Loading, mono, truncate } from "../components/ui";

/** Moves the board offers by hand; anything else needs an override. */
const LEGAL: Record<Status, Status[]> = {
  backlog: ["ready"],
  ready: ["backlog", "running"],
  running: ["ready", "review"],
  review: ["ready", "done"],
  done: [],
};

/** What an empty column actually means. "Empty" five times over is the same
 *  word standing in for five different states of the work. */
const EMPTY_COLUMN: Record<Status, string> = {
  backlog: "Nothing parked for later",
  ready: "Nothing waiting to start",
  running: "No agent working",
  review: "Nothing to review",
  done: "Nothing finished yet",
};

const COLUMN_COLOR: Record<Status, string> = {
  backlog: "var(--text4)",
  ready: "var(--info)",
  running: "var(--accent)",
  review: "var(--warn)",
  done: "var(--ok)",
};

/** Which cards changed column since the last render, and which way they went.
 *
 *  This is the board's whole reason for animating: an agent finishing a run
 *  moves a card with no action from the operator, and a silent re-render makes
 *  that indistinguishable from nothing having happened. The comparison is
 *  against the previous render rather than against a backend event, so a change
 *  is caught however it arrived — the operator's own drag included.
 *
 *  The mark clears itself, or a card would keep announcing a move it made
 *  minutes ago every time React re-rendered for an unrelated reason.
 */
function useArrivals(cards: { id: string; status: Status }[] | undefined) {
  const seen = useRef(new Map<string, Status>());
  const [moves, setMoves] = useState<Map<string, "forward" | "back">>(new Map());

  useEffect(() => {
    if (!cards) return;
    const previous = seen.current;
    const next = new Map<string, Status>();
    const fresh = new Map<string, "forward" | "back">();
    for (const card of cards) {
      next.set(card.id, card.status);
      const was = previous.get(card.id);
      // A card seen for the first time has not moved; it was created, or the
      // board just loaded. Announcing those would light up the whole screen.
      if (was && was !== card.status) {
        const from = STATUS_ORDER.indexOf(was);
        const to = STATUS_ORDER.indexOf(card.status);
        fresh.set(card.id, to >= from ? "forward" : "back");
      }
    }
    seen.current = next;
    if (fresh.size === 0) return;
    setMoves(fresh);
    const done = setTimeout(() => setMoves(new Map()), 900);
    return () => clearTimeout(done);
  }, [cards]);

  return moves;
}

/** The one place a card is created by hand: a line, who takes it, and whether
 *  it starts now. */
function NewCard({ close }: { close: () => void }) {
  const { agents, createCard } = useStore();
  const takers = agents.filter((a) => a.tasks_enabled && !a.paused);
  const [title, setTitle] = useState("");
  const [who, setWho] = useState(takers.find((a) => a.id !== "director")?.id ?? takers[0]?.id ?? "");

  const add = (mode: "later" | "plan" | "start") => {
    if (!title.trim()) return;
    createCard(title, who, mode);
    close();
  };

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "8px 16px",
        borderBottom: "1px solid var(--line)",
        background: "var(--surface)",
        animation: "sheetIn .28s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") add(e.shiftKey ? "start" : "plan");
          if (e.key === "Escape") close();
        }}
        placeholder="What should happen? One card, one outcome."
        style={{
          flex: 1,
          minWidth: 0,
          padding: "8px 12px",
          borderRadius: 8,
          border: "1px solid var(--line3)",
          background: "var(--bg)",
          font: "400 12.5px var(--sans)",
          color: "var(--text)",
          outline: "none",
        }}
      />
      <select
        value={who}
        onChange={(e) => setWho(e.target.value)}
        style={{
          padding: "8px 10px",
          borderRadius: 8,
          border: "1px solid var(--line3)",
          background: "var(--bg)",
          font: "500 11.5px var(--sans)",
          color: "var(--text2)",
          cursor: "pointer",
        }}
      >
        {takers.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
      {[
        { label: "Later", mode: "later" as const, strong: false },
        { label: "Ready", mode: "plan" as const, strong: false },
        { label: "Start now", mode: "start" as const, strong: true },
      ].map((b) => (
        <span
          key={b.mode}
          className={b.strong ? "primary" : "chip"}
          onClick={() => add(b.mode)}
          style={{
            padding: "6px 12px",
            borderRadius: 999,
            border: b.strong ? "none" : "1px solid var(--line3)",
            background: b.strong ? "var(--accent)" : "transparent",
            color: b.strong ? "var(--onAccent)" : "var(--text2)",
            font: `${b.strong ? 600 : 500} 11.5px var(--sans)`,
            cursor: "pointer",
            opacity: title.trim() ? 1 : 0.55,
          }}
        >
          {b.label}
        </span>
      ))}
      <span
        onClick={close}
        style={{ font: "500 11.5px var(--sans)", color: "var(--text4)", cursor: "pointer" }}
      >
        Cancel
      </span>
    </div>
  );
}

export function Board({
  openRun,
  openReview,
}: {
  openRun: (cardId: string) => void;
  openReview: (cardId: string) => void;
}) {
  const { snapshot, agents, outputs, streams, moveCard, startRun, cancelRun, discard } = useStore();
  const [drag, setDrag] = useState<string | null>(null);
  const [over, setOver] = useState<Status | null>(null);
  const [adding, setAdding] = useState(false);
  const arrived = useArrivals(snapshot?.cards);

  if (!snapshot) return <Loading what="Reading the board" />;
  const cards = snapshot.cards;
  const dragged = cards.find((c) => c.id === drag);

  const drop = (to: Status) => {
    setOver(null);
    const card = dragged;
    setDrag(null);
    if (!card || card.status === to || !LEGAL[card.status].includes(to)) return;
    if (to === "running") startRun(card.id);
    else if (to === "done") openReview(card.id);
    else moveCard(card.id, to);
  };

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "grid",
        gridTemplateRows: "auto auto minmax(0,1fr)",
        overflow: "hidden",
        animation: "paneIn .4s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "10px 16px",
          borderBottom: "1px solid var(--line)",
        }}
      >
        <span style={{ font: "400 11.5px var(--sans)", color: "var(--text4)" }}>
          A card moves one column at a time. Anything else is an override, and an override needs a
          reason.
        </span>
        <div style={{ flex: 1 }} />
        <span
          className="chip"
          onClick={() => setAdding((v) => !v)}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "6px 12px",
            borderRadius: 999,
            border: "1px solid var(--line3)",
            font: "500 11.5px var(--sans)",
            color: "var(--text2)",
            cursor: "pointer",
          }}
        >
          New card
        </span>
      </div>

      {adding ? <NewCard close={() => setAdding(false)} /> : <span />}

      <div
        className="cols"
        style={{
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "repeat(5,minmax(0,1fr))",
          gap: 1,
          background: "var(--line)",
          overflow: "hidden",
        }}
      >
        {STATUS_ORDER.map((status) => {
          const list = cards.filter((c) => c.status === status);
          const color = COLUMN_COLOR[status];
          const canDrop = dragged ? LEGAL[dragged.status].includes(status) : false;
          const hot = over === status && canDrop;
          return (
            <div
              key={status}
              onDragOver={(e) => {
                if (!canDrop) return;
                e.preventDefault();
                if (over !== status) setOver(status);
              }}
              onDragLeave={() => setOver((s) => (s === status ? null : s))}
              onDrop={(e) => {
                e.preventDefault();
                drop(status);
              }}
              style={{
                minHeight: 0,
                display: "grid",
                gridTemplateRows: "auto minmax(0,1fr)",
                background: hot ? "var(--hover)" : "var(--bg)",
                transition: "background .2s ease",
                animation: list.some((c) => arrived.get(c.id) === "forward" || arrived.get(c.id) === "back")
                  ? "tookOne .9s ease both"
                  : undefined,
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 14px 10px" }}>
                <span style={{ width: 6, height: 6, borderRadius: "50%", background: color }} />
                <span
                  style={{
                    flex: 1,
                    font: "600 10.5px var(--sans)",
                    color,
                    letterSpacing: ".08em",
                  }}
                >
                  {STATUS_NAME[status].toUpperCase()}
                </span>
                <span style={{ ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text3)" }}>
                  {list.length}
                </span>
              </div>

              <div
                className="stagger"
                style={{
                  minHeight: 0,
                  overflowY: "auto",
                  padding: "0 10px 10px",
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                }}
              >
                {list.map((card) => {
                  const agent = agents.find((a) => a.id === card.agent_id);
                  const t = tone(agent?.tone);
                  const isRun = status === "running";
                  const isReview = status === "review";
                  const isReady = status === "ready";
                  const log = outputs[card.id] ?? [];
                  const stream = streams[card.id];
                  const meta = isRun
                    ? `${plural(card.turns, "turn")} · ${money(card.cost_usd, 2)}`
                    : card.runs > 0
                      ? `${plural(card.turns, "turn")} · ${money(card.cost_usd, 2)}`
                      : `${card.runs} runs`;

                  // One line under the title: the review that sent it back, what
                  // it is doing right now, or nothing.
                  let note = "";
                  let noteColor = "var(--text3)";
                  if (isRun) {
                    note =
                      stream?.thinking?.slice(-70) ||
                      (log.length > 0 ? `${log[log.length - 1]!.label} ${log[log.length - 1]!.text}` : "starting…");
                  } else if (card.last_review && status !== "done") {
                    note = `${card.last_review.by === "director" ? "Director" : "You"}: ${card.last_review.reason}`;
                    noteColor = card.last_review.approved ? "var(--ok)" : "var(--warn)";
                  } else if (status === "done" && card.last_review) {
                    // The verdict stays legible after Done: who approved and
                    // why — a silent approval is indistinguishable from one
                    // that never ran.
                    note = `${card.last_review.by === "director" ? "Director" : "You"} approved: ${card.last_review.reason}`;
                    noteColor = "var(--ok)";
                  } else if (status === "done" && card.branch) {
                    note = `${card.branch} still unmerged`;
                  }

                  return (
                    <div
                      key={card.id}
                      draggable
                      onDragStart={() => setDrag(card.id)}
                      onDragEnd={() => {
                        setDrag(null);
                        setOver(null);
                      }}
                      onClick={() => (isReview ? openReview(card.id) : openRun(card.id))}
                      className="tile"
                      style={{
                        padding: "12px 12px",
                        borderRadius: 12,
                        background: "var(--surface)",
                        border: "1px solid var(--line2)",
                        display: "flex",
                        flexDirection: "column",
                        gap: 8,
                        cursor: "grab",
                        opacity: drag === card.id ? 0.45 : 1,
                        animation: arrived.has(card.id)
                          ? `${arrived.get(card.id) === "back" ? "arriveBack" : "arriveForward"} .42s cubic-bezier(.16,1,.3,1) both`
                          : undefined,
                      }}
                      data-arrived={arrived.has(card.id) ? "" : undefined}
                    >
                      <span
                        title={card.title}
                        style={{
                          font: "500 12.5px/1.42 var(--sans)",
                          color: "var(--text)",
                          display: "-webkit-box",
                          WebkitLineClamp: 3,
                          WebkitBoxOrient: "vertical",
                          overflow: "hidden",
                        }}
                      >
                        {card.title}
                      </span>
                      <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <Glyph color={t.color} soft={t.soft} size={16} font={8}>
                          {agent?.initial ?? "?"}
                        </Glyph>
                        <span className="card-id" style={{ ...mono, fontSize: 10.5, color: "var(--text3)" }}>
                          {card.id}
                        </span>
                        <span style={{ flex: 1 }} />
                        <span style={{ ...mono, fontSize: 10.5, color: "var(--text3)" }}>{meta}</span>
                        {!isRun && (
                          <span
                            title="Delete this card"
                            onClick={(e) => {
                              e.stopPropagation();
                              discard(card.id);
                            }}
                            style={{ ...mono, fontSize: 11.5, color: "var(--text4)", cursor: "pointer" }}
                          >
                            ✕
                          </span>
                        )}
                      </span>

                      {note && (
                        <span
                          style={{
                            font: "400 10.5px/1.5 var(--sans)",
                            color: noteColor,
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {note}
                        </span>
                      )}

                      {card.session_id && (
                        <span
                          style={{
                            alignSelf: "flex-start",
                            display: "flex",
                            alignItems: "center",
                            gap: 6,
                            padding: "1px 8px",
                            borderRadius: 8,
                            background: "var(--surface2)",
                            ...mono,
                            fontSize: 10.5,
                            fontWeight: 500,
                            color: "var(--text3)",
                            maxWidth: "100%",
                            ...truncate,
                          }}
                        >
                          Start continues session {card.session_id.slice(0, 8)}
                        </span>
                      )}

                      {(isRun || isReview || isReady) && (
                        <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <span
                            onClick={(e) => {
                              e.stopPropagation();
                              if (isRun) cancelRun(card.id);
                              else if (isReview) openReview(card.id);
                              else startRun(card.id);
                            }}
                            style={{
                              flex: 1,
                              padding: 6,
                              borderRadius: 8,
                              background: isRun
                                ? "var(--badSoft)"
                                : isReview
                                  ? "var(--okSoft)"
                                  : "var(--infoSoft)",
                              color: isRun ? "var(--bad)" : isReview ? "var(--ok)" : "var(--info)",
                              font: "600 11.5px var(--sans)",
                              textAlign: "center",
                              cursor: "pointer",
                            }}
                          >
                            {isRun ? "Stop" : isReview ? "Read the diff" : "Start"}
                          </span>
                          <span
                            onClick={(e) => {
                              e.stopPropagation();
                              openRun(card.id);
                            }}
                            style={{
                              padding: "6px 10px",
                              borderRadius: 8,
                              font: "500 11.5px var(--sans)",
                              color: "var(--text3)",
                              cursor: "pointer",
                            }}
                          >
                            {isRun ? "Transcript" : isReview ? "Session" : "Log"}
                          </span>
                        </span>
                      )}
                    </div>
                  );
                })}

                {list.length === 0 && (
                  <div
                    style={{
                      padding: "14px 8px",
                      textAlign: "center",
                      font: "400 11.5px var(--sans)",
                      color: "var(--text4)",
                      border: "1px dashed var(--line2)",
                      borderRadius: 12,
                    }}
                  >
                    {hot ? "Drop here" : EMPTY_COLUMN[status]}
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
