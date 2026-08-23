import { useState } from "react";
import { money, plural } from "../lib/format";
import { STATUS_NAME, STATUS_ORDER, STATUS_TONE, type Status } from "../lib/types";
import { useStore } from "../state/store";
import { Icon, Loading } from "../components/ui";

/** Moves the board offers by hand; anything else needs an override. */
const LEGAL: Record<Status, Status[]> = {
  backlog: ["ready"],
  ready: ["backlog", "running"],
  running: ["ready", "review"],
  review: ["ready", "done"],
  done: [],
};

export function Board({
  openRun,
  openReject,
}: {
  openRun: (cardId: string) => void;
  openReject: (cardId: string) => void;
}) {
  const { snapshot, outputs, moveCard, startRun, cancelRun, approve, discard } = useStore();
  const [drag, setDrag] = useState<string | null>(null);
  const [over, setOver] = useState<Status | null>(null);

  if (!snapshot) return <Loading what="Reading the board" />;
  const cards = snapshot.cards;
  const dragged = cards.find((c) => c.id === drag);

  const drop = (to: Status) => {
    setOver(null);
    const card = dragged;
    setDrag(null);
    if (!card || card.status === to || !LEGAL[card.status].includes(to)) return;
    if (to === "running") startRun(card.id);
    else if (to === "done") approve(card.id);
    else moveCard(card.id, to);
  };

  return (
    <div
      style={{
        padding: "22px 26px 24px",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 14 }}>
        <h1
          style={{
            margin: 0,
            fontSize: "var(--t-xl)",
            fontWeight: 800,
            letterSpacing: "-.03em",
            lineHeight: 1.2,
          }}
        >
          Work
        </h1>
        <span style={{ fontSize: "var(--t-sm)", color: "var(--text3)" }}>
          Drag a card to move it
        </span>
      </div>

      <div
        style={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "repeat(5,minmax(0,1fr))",
          gap: 11,
        }}
      >
        {STATUS_ORDER.map((status) => {
          const list = cards.filter((c) => c.status === status);
          const t = STATUS_TONE[status];
          const canDrop = dragged ? LEGAL[dragged.status].includes(status) : false;
          const hot = over === status && canDrop;
          return (
            <section
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
                display: "flex",
                flexDirection: "column",
                minHeight: 0,
                borderRadius: 18,
                border: `1px solid ${hot ? "var(--accentLine)" : "var(--line)"}`,
                background: hot ? "var(--accentSoft)" : "var(--recess)",
                transition: "all .2s ease",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "13px 14px 11px" }}>
                <span
                  style={{ width: 7, height: 7, borderRadius: "50%", background: t.color }}
                />
                <span
                  style={{
                    fontSize: "var(--t-xs)",
                    fontWeight: 700,
                    letterSpacing: ".06em",
                    textTransform: "uppercase",
                    color: "var(--text2)",
                  }}
                >
                  {STATUS_NAME[status]}
                </span>
                <span style={{ fontSize: "var(--t-xs)", color: "var(--text3)" }}>{list.length}</span>
              </div>

              <div
                style={{
                  flex: 1,
                  minHeight: 0,
                  overflowY: "auto",
                  padding: "0 9px 10px",
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                }}
              >
                {list.map((card) => {
                  const isRun = status === "running";
                  const isReview = status === "review";
                  const isReady = status === "ready";
                  const log = outputs[card.id] ?? [];
                  const badge = isRun
                    ? "live"
                    : isReview
                      ? "diff ready"
                      : status === "done" && card.cost_usd
                        ? money(card.cost_usd)
                        : null;
                  const badgeColor = isRun
                    ? "var(--accent)"
                    : isReview
                      ? "var(--warn)"
                      : "var(--text3)";
                  const badgeSoft = isRun
                    ? "var(--accentSoft)"
                    : isReview
                      ? "var(--warnSoft)"
                      : "var(--surface2)";
                  const actionLabel = isRun ? "Stop" : isReview ? "Approve" : "Start";
                  const actionColor = isRun
                    ? "var(--bad)"
                    : isReview
                      ? "var(--ok)"
                      : "var(--info)";
                  const actionSoft = isRun
                    ? "var(--badSoft)"
                    : isReview
                      ? "var(--okSoft)"
                      : "var(--infoSoft)";
                  return (
                    <article
                      key={card.id}
                      draggable
                      onDragStart={() => setDrag(card.id)}
                      onDragEnd={() => {
                        setDrag(null);
                        setOver(null);
                      }}
                      onClick={() => openRun(card.id)}
                      className="hv-tile"
                      style={{
                        padding: "12px 13px",
                        border: "1px solid var(--line)",
                        borderRadius: 15,
                        background: "var(--surface)",
                        cursor: "grab",
                        opacity: drag === card.id ? 0.45 : 1,
                        transition: "all .2s cubic-bezier(.2,.8,.2,1)",
                        animation: "fadeUp .3s ease both",
                      }}
                    >
                      <div
                        style={{ display: "flex", alignItems: "flex-start", gap: 6, margin: "0 0 9px" }}
                      >
                        <p
                          style={{
                            margin: 0,
                            flex: 1,
                            minWidth: 0,
                            fontSize: "var(--t-md)",
                            fontWeight: 500,
                            lineHeight: 1.45,
                          }}
                        >
                          {card.title}
                        </p>
                        {!isRun && (
                          <button
                            type="button"
                            className="hv-danger"
                            title="Delete this card"
                            onClick={(e) => {
                              e.stopPropagation();
                              discard(card.id);
                            }}
                            style={{
                              flex: "none",
                              width: 20,
                              height: 20,
                              border: "none",
                              borderRadius: 6,
                              background: "transparent",
                              color: "var(--text3)",
                              display: "flex",
                              alignItems: "center",
                              justifyContent: "center",
                              lineHeight: 1,
                              cursor: "pointer",
                              transition: "all .16s ease",
                            }}
                          >
                            <Icon.close />
                          </button>
                        )}
                      </div>
                      <div
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 7,
                          fontSize: 11,
                          color: "var(--text3)",
                        }}
                      >
                        <span
                          className="card-id"
                          style={{ fontFamily: "var(--mono)", letterSpacing: "-.01em" }}
                        >
                          {card.id}
                        </span>
                        {badge && (
                          <span
                            style={{
                              padding: "2px 8px",
                              borderRadius: 999,
                              background: badgeSoft,
                              color: badgeColor,
                              fontWeight: 700,
                            }}
                          >
                            {badge}
                          </span>
                        )}
                        {status === "done" && card.turns > 0 && (
                          <span>{plural(card.turns, "turn")}</span>
                        )}
                      </div>

                      {card.last_review && status !== "done" && (
                        <p
                          style={{
                            margin: "9px 0 0",
                            fontSize: 11,
                            lineHeight: 1.5,
                            color: card.last_review.approved ? "var(--ok)" : "var(--warn)",
                          }}
                        >
                          {card.last_review.by === "director" ? "Director" : "You"}:{" "}
                          {card.last_review.reason}
                        </p>
                      )}

                      {isRun && log.length > 0 && (
                        <p
                          style={{
                            margin: "9px 0 0",
                            fontFamily: "var(--mono)",
                            fontSize: 10.5,
                            color: "var(--text3)",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {log[log.length - 1]!.text}
                        </p>
                      )}

                      {(isRun || isReview || isReady) && (
                        <div style={{ display: "flex", gap: 6, marginTop: 10 }}>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              if (isRun) cancelRun(card.id);
                              else if (isReview) approve(card.id);
                              else startRun(card.id);
                            }}
                            style={{
                              flex: 1,
                              padding: "7px 9px",
                              border: "none",
                              borderRadius: 9,
                              background: actionSoft,
                              color: actionColor,
                              fontSize: "var(--t-sm)",
                              fontWeight: 700,
                              cursor: "pointer",
                              transition: "filter .18s ease",
                            }}
                          >
                            {actionLabel}
                          </button>
                          <button
                            type="button"
                            className="hv-soft"
                            onClick={(e) => {
                              e.stopPropagation();
                              if (isReview) openReject(card.id);
                              else openRun(card.id);
                            }}
                            style={{
                              padding: "7px 10px",
                              border: "none",
                              borderRadius: 9,
                              background: "transparent",
                              color: "var(--text3)",
                              fontSize: "var(--t-sm)",
                              cursor: "pointer",
                              transition: "all .18s ease",
                            }}
                          >
                            {isReview ? "Send back" : "Log"}
                          </button>
                        </div>
                      )}
                    </article>
                  );
                })}

                {list.length === 0 && (
                  <div
                    style={{
                      padding: "14px 8px",
                      textAlign: "center",
                      fontSize: "var(--t-sm)",
                      color: "var(--text3)",
                      border: "1px dashed var(--line)",
                      borderRadius: 13,
                    }}
                  >
                    {hot ? "Drop here" : "Empty"}
                  </div>
                )}
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}
