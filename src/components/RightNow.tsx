/** The right rail: everything that is waiting, running, or finished today, in
 *  that order. It is the same information the screens hold, ranked by whether
 *  it needs the operator. */

import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { clock, duration, money, plural } from "../lib/format";
import { tone, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { Glyph, Icon, mono, truncate } from "./ui";

function Section({
  title,
  count,
  right,
  top,
}: {
  title: string;
  count?: string;
  right?: React.ReactNode;
  top?: number;
}) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 8, padding: `${top ?? 0}px 3px 9px` }}>
      <span style={{ font: "600 11px var(--sans)", color: "var(--text2)" }}>{title}</span>
      {count && (
        <span style={{ ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text3)" }}>
          · {count}
        </span>
      )}
      <div style={{ flex: 1 }} />
      {right}
    </div>
  );
}

/** The 44px strip the rail collapses to: the count, who is working, and a way
 *  back. */
export function RightNowStrip({ open }: { open: () => void }) {
  const { approvals, snapshot, agents, proposals } = useStore();
  // Elapsed timers must breathe: a frozen number reads as "frozen app".
  const anyLive = (snapshot?.cards ?? []).some((c) => c.status === "running");
  const [, tick] = useState(0);
  useEffect(() => {
    if (!anyLive) return;
    const t = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(t);
  }, [anyLive]);
  const cards = snapshot?.cards ?? [];
  const openProposals = proposals.filter((p) => p.status === "open");
  const waiting =
    approvals.length + openProposals.length + cards.filter((c) => c.status === "review").length;
  const workers = [...new Set(cards.filter((c) => c.status === "running").map((c) => c.agent_id))];

  return (
    <div
      onClick={open}
      title="Right now"
      style={{
        width: 44,
        flex: "none",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 10,
        padding: "14px 0",
        background: "var(--recess)",
        cursor: "pointer",
      }}
    >
      {waiting > 0 && (
        <span
          style={{
            padding: "2px 6px",
            borderRadius: 7,
            background: "var(--warnSoft)",
            color: "var(--warn)",
            ...mono,
            fontSize: 9.5,
            fontWeight: 600,
          }}
        >
          {waiting}
        </span>
      )}
      {workers.slice(0, 4).map((id) => {
        const agent = agents.find((a) => a.id === id);
        const t = tone(agent?.tone);
        return (
          <Glyph key={id} color={t.color} soft={t.soft} size={26} radius={9} font={10}>
            {agent?.initial ?? "?"}
          </Glyph>
        );
      })}
      <span
        style={{
          writingMode: "vertical-rl",
          font: "500 10.5px var(--sans)",
          color: "var(--text4)",
          letterSpacing: ".12em",
        }}
      >
        RIGHT NOW
      </span>
    </div>
  );
}

export function RightNow({
  close,
  openReview,
  openSession,
  openTrees,
}: {
  close: () => void;
  /** Take the operator to the review screen for one card. */
  openReview: (cardId: string) => void;
  openSession: (cardId: string) => void;
  openTrees: () => void;
}) {
  const {
    approvals,
    answerApproval,
    snapshot,
    agents,
    projectId,
    activity,
    outputs,
    streams,
    stats,
    diffs,
    loadCardDiff,
    cancelRun,
    proposals,
    acceptProposal,
    dismissProposal,
  } = useStore();

  const cards = snapshot?.cards ?? [];
  const reviewing = cards.filter((c) => c.status === "review");
  const runningCards = cards.filter((c) => c.status === "running");
  const openProposals = proposals.filter((p) => p.status === "open");
  const [trees, setTrees] = useState<WorktreeRow[]>([]);

  // The numbers beside a review row are the real ones, read from the worktree.
  useEffect(() => {
    reviewing.forEach((c) => {
      if (!diffs[c.id]) loadCardDiff(c.id);
    });
    // Card list identity is what matters here, not the diff cache: re-reading on
    // every cached answer would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reviewing.map((c) => c.id).join(","), loadCardDiff]);

  useEffect(() => {
    if (!projectId) return;
    let alive = true;
    api
      .worktrees(projectId)
      .then((rows) => alive && setTrees(rows))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [projectId, snapshot?.last_seq]);

  const startOfDay = new Date();
  startOfDay.setHours(0, 0, 0, 0);
  const doneToday = activity.filter(
    (a) => a.kind === "review" && a.label.startsWith("Approved") && a.ts_ms >= startOfDay.getTime(),
  );

  const liveSpend = runningCards.reduce((sum, c) => sum + c.cost_usd, 0);
  const waiting = approvals.length + openProposals.length + reviewing.length;

  return (
    <div
      style={{
        width: 318,
        flex: "none",
        display: "flex",
        flexDirection: "column",
        background: "var(--recess)",
        overflow: "hidden",
        animation: "railIn .38s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div
        style={{
          flex: "none",
          display: "flex",
          alignItems: "center",
          gap: 9,
          padding: "13px 15px 11px",
          borderBottom: "1px solid var(--line)",
        }}
      >
        <span style={{ flex: 1, font: "600 12.5px var(--sans)", color: "var(--text)" }}>
          Right now
        </span>
        <span
          onClick={close}
          style={{
            display: "grid",
            placeItems: "center",
            width: 20,
            height: 20,
            color: "var(--text4)",
            cursor: "pointer",
          }}
        >
          <Icon.close />
        </span>
      </div>

      <div className="stagger" style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "12px 12px 16px" }}>
        <Section
          title="Waiting on you"
          right={
            waiting > 0 ? (
              <span style={{ ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--warn)" }}>
                · {waiting}
              </span>
            ) : undefined
          }
        />

        {waiting === 0 && (
          <div
            style={{
              marginBottom: 7,
              padding: "10px 11px",
              borderRadius: 12,
              border: "1px solid var(--line2)",
              font: "400 11px var(--sans)",
              lineHeight: 1.6,
              color: "var(--text4)",
            }}
          >
            Nothing needs you. A run that wants a permission, or a diff that is
            finished, arrives here.
          </div>
        )}

        {approvals.map((request) => {
          const card = cards.find((c) => c.id === request.card_id);
          const agent = agents.find((a) => a.id === card?.agent_id);
          const t = tone(agent?.tone ?? "warn");
          return (
            <div
              key={request.request_id}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 7,
                padding: "10px 11px",
                borderRadius: 12,
                background: "rgba(255,179,92,.08)",
                border: "1px solid rgba(255,179,92,.24)",
                marginBottom: 7,
              }}
            >
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <Glyph color={t.color} soft={t.soft} size={18} radius={6} font={8}>
                  {agent?.initial ?? "?"}
                </Glyph>
                <span style={{ flex: 1, font: "600 11.5px var(--sans)", color: "var(--text)", ...truncate }}>
                  {agent?.name ?? "An agent"} · permission
                </span>
                <span
                  style={{
                    padding: "1px 7px",
                    borderRadius: 6,
                    background: "var(--warnSoft)",
                    color: "var(--warn)",
                    ...mono,
                    fontSize: 10,
                    fontWeight: 500,
                  }}
                >
                  {request.tool}
                </span>
              </span>
              <span
                style={{
                  ...mono,
                  fontSize: 10.5,
                  fontWeight: 500,
                  color: "var(--warn)",
                  lineHeight: 1.5,
                  wordBreak: "break-word",
                }}
              >
                {request.summary || "no details given"}
              </span>
              <span style={{ display: "flex", alignItems: "center", gap: 9 }}>
                <span
                  className="primary"
                  onClick={() => answerApproval(request.request_id, true, false)}
                  style={{
                    padding: "5px 12px",
                    borderRadius: 999,
                    background: "var(--accent)",
                    color: "var(--onAccent)",
                    font: "600 10.5px var(--sans)",
                    cursor: "pointer",
                  }}
                >
                  Allow
                </span>
                <span
                  onClick={() => answerApproval(request.request_id, false, false)}
                  style={{ font: "500 10.5px var(--sans)", color: "var(--text2)", cursor: "pointer" }}
                >
                  Deny
                </span>
              </span>
            </div>
          );
        })}

        {reviewing.map((card) => {
          const diff = diffs[card.id];
          return (
            <div
              key={card.id}
              className="row"
              onClick={() => openReview(card.id)}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                padding: "10px 11px",
                borderRadius: 12,
                border: "1px solid var(--line2)",
                marginBottom: 7,
                cursor: "pointer",
              }}
            >
              <span style={{ font: "600 11.5px var(--sans)", color: "var(--text)", ...truncate }}>
                {card.title}
              </span>
              <span
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  ...mono,
                  fontSize: 10.5,
                  color: "var(--text3)",
                }}
              >
                {diff ? (
                  <>
                    <span style={{ color: "var(--ok)" }}>+{diff.added}</span>
                    <span style={{ color: "var(--bad)" }}>−{diff.removed}</span>
                  </>
                ) : (
                  <span>reading…</span>
                )}
                {card.id}
                <span style={{ flex: 1 }} />
                <span style={{ font: "600 10.5px var(--sans)", color: "var(--ok)" }}>Review</span>
              </span>
            </div>
          );
        })}

        <Section
          title="Proposals"
          count={String(openProposals.length)}
          top={14}
          right={
            openProposals.length > 0 ? (
              <span
                title="The Director noticed a pattern; you decide whether it becomes work"
                style={{ ...mono, fontSize: 10, color: "var(--text4)" }}
              >
                his call, your decision
              </span>
            ) : undefined
          }
        />
        {openProposals.length === 0 && (
          <div style={{ padding: "0 3px 4px", font: "400 11px var(--sans)", color: "var(--text4)" }}>
            No proposals waiting.
          </div>
        )}
        {openProposals.map((proposal) => (
          <div
            key={proposal.id}
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 6,
              padding: "10px 11px",
              borderRadius: 12,
              background: "var(--surface)",
              border: "1px solid var(--line2)",
              marginBottom: 7,
            }}
          >
            <span style={{ font: "600 11.5px var(--sans)", color: "var(--text)", ...truncate }}>
              {proposal.title}
            </span>
            <span style={{ font: "400 11px var(--sans)", lineHeight: 1.55, color: "var(--text3)" }}>
              {proposal.observation}
            </span>
            <span style={{ font: "400 11px var(--sans)", lineHeight: 1.55, color: "var(--text2)" }}>
              {proposal.proposal}
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 9 }}>
              <span
                className="primary"
                onClick={() => acceptProposal(proposal.id)}
                title="Creates the card in the harness's own project — never the one you have open"
                style={{
                  padding: "5px 12px",
                  borderRadius: 999,
                  background: "var(--accent)",
                  color: "var(--onAccent)",
                  font: "600 10.5px var(--sans)",
                  cursor: "pointer",
                }}
              >
                Make card in _harness
              </span>
              <span
                onClick={() => dismissProposal(proposal.id)}
                style={{ font: "500 10.5px var(--sans)", color: "var(--text2)", cursor: "pointer" }}
              >
                Dismiss
              </span>
              <span style={{ flex: 1 }} />
              <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>
                {clock(proposal.created_ms)}
              </span>
            </span>
          </div>
        ))}

        <Section
          title="Running"
          count={String(runningCards.length)}
          top={14}
          right={
            <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>{money(liveSpend, 2)}</span>
          }
        />
        {runningCards.length === 0 && (
          <div
            style={{
              marginBottom: 7,
              padding: "10px 11px",
              borderRadius: 12,
              border: "1px solid var(--line2)",
              font: "400 11px var(--sans)",
              color: "var(--text4)",
            }}
          >
            No run in flight.
          </div>
        )}
        {runningCards.map((card) => {
          const agent = agents.find((a) => a.id === card.agent_id);
          const t = tone(agent?.tone);
          const session = snapshot?.sessions.find((s) => s.card_id === card.id);
          const log = outputs[card.id] ?? [];
          const stream = streams[card.id];
          const doing =
            stream?.text?.slice(-90) ||
            stream?.thinking?.slice(-90) ||
            (log.length > 0 ? `${log[log.length - 1]!.label} ${log[log.length - 1]!.text}` : "starting…");
          return (
            <div
              key={card.id}
              style={{
                display: "flex",
                gap: 10,
                padding: "10px 11px",
                borderRadius: 12,
                background: "var(--surface)",
                border: "1px solid var(--line2)",
                marginBottom: 7,
              }}
            >
              <Glyph color={t.color} soft={t.soft} size={26} radius={9} font={10}>
                {agent?.initial ?? "?"}
              </Glyph>
              <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 5 }}>
                <div style={{ display: "flex", alignItems: "baseline", gap: 7 }}>
                  <span style={{ font: "600 12px var(--sans)", color: "var(--text)" }}>
                    {agent?.name ?? card.agent_id}
                  </span>
                  <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>{card.id}</span>
                  <div style={{ flex: 1 }} />
                  <span style={{ ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text3)" }}>
                    {session ? duration(Date.now() - session.started_ms) : "—"}
                  </span>
                </div>
                <span
                  onClick={() => openSession(card.id)}
                  style={{
                    font: "400 11.5px var(--sans)",
                    lineHeight: 1.4,
                    color: "var(--text2)",
                    cursor: "pointer",
                  }}
                >
                  {card.title}
                </span>
                <span
                  style={{
                    ...mono,
                    fontSize: 10.5,
                    color: t.color,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {doing}
                </span>
                <span
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    ...mono,
                    fontSize: 10,
                    color: "var(--text3)",
                  }}
                >
                  {plural(card.turns, "turn")} · {money(card.cost_usd, 2)}
                  <span style={{ flex: 1 }} />
                  <span onClick={() => cancelRun(card.id)} style={{ cursor: "pointer" }}>
                    stop
                  </span>
                </span>
              </div>
            </div>
          );
        })}

        <Section
          title="Done today"
          count={String(stats?.done_today ?? doneToday.length)}
          top={14}
          right={
            <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>
              {money(stats?.spend_today ?? 0)}
            </span>
          }
        />
        {doneToday.length === 0 && (
          <div style={{ padding: "0 3px 4px", font: "400 11px var(--sans)", color: "var(--text4)" }}>
            Nothing approved yet today.
          </div>
        )}
        {doneToday.slice(0, 8).map((row) => {
          const card = cards.find((c) => c.id === row.card_id);
          const agent = agents.find((a) => a.id === card?.agent_id);
          const t = tone(agent?.tone);
          return (
            <div
              key={row.seq}
              className="row"
              onClick={() => openSession(row.card_id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 9,
                padding: "7px 8px",
                borderRadius: 9,
                cursor: "pointer",
              }}
            >
              <Glyph color={t.color} soft={t.soft} size={20} radius={7} font={8.5}>
                {agent?.initial ?? "·"}
              </Glyph>
              <span style={{ flex: 1, font: "400 11.5px var(--sans)", color: "var(--text2)", ...truncate }}>
                {card?.title ?? row.card_id}
              </span>
              <span style={{ ...mono, fontSize: 10, color: "var(--text4)" }}>{clock(row.ts_ms)}</span>
            </div>
          );
        })}

        <Section
          title="Worktrees"
          count={String(trees.length)}
          top={16}
          right={
            <span
              onClick={openTrees}
              style={{ font: "400 10px var(--sans)", color: "var(--text4)", cursor: "pointer" }}
            >
              manage
            </span>
          }
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 1 }}>
          {trees.length === 0 && (
            <span style={{ padding: "0 3px", font: "400 11px var(--sans)", color: "var(--text4)" }}>
              No worktree has been created yet.
            </span>
          )}
          {trees.map((t) => (
            <div
              key={t.path}
              className="row"
              onClick={() => api.reveal(t.path).catch(() => {})}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 9,
                padding: "7px 8px",
                borderRadius: 9,
                cursor: "pointer",
              }}
            >
              <span style={{ flex: 1, ...mono, fontSize: 11, fontWeight: 500, color: "var(--text2)", ...truncate }}>
                {t.branch ?? t.path.split(/[\\/]/).pop()}
              </span>
              <span
                style={{
                  font: "400 10px var(--sans)",
                  color: t.dirty ? "var(--warn)" : "var(--text3)",
                }}
              >
                {t.bare ? "main" : t.dirty ? "dirty" : "clean"}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
