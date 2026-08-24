/** The review screen: one finished run, what it changed, and the two things you
 *  can do about it. Harness never merges — approving moves the card, the branch
 *  and its worktree stay where they are. */

import { useEffect, useMemo, useState } from "react";
import { ago, money, plural } from "../lib/format";
import { MODELS, tone } from "../lib/types";
import { useStore } from "../state/store";
import { Eyebrow, Glyph, mono, truncate } from "../components/ui";

/** One line of a patch, coloured by what it does to the file. */
function classify(text: string): { bg: string; color: string } {
  if (text.startsWith("+++") || text.startsWith("---"))
    return { bg: "transparent", color: "var(--text4)" };
  if (text.startsWith("+")) return { bg: "rgba(79,209,165,.08)", color: "#5fdcb1" };
  if (text.startsWith("-")) return { bg: "rgba(255,107,129,.08)", color: "var(--bad2)" };
  if (text.startsWith("@@") || text.startsWith("diff --git") || text.startsWith("index "))
    return { bg: "transparent", color: "var(--text4)" };
  return { bg: "transparent", color: "var(--text3)" };
}

export function Review({
  selected,
  select,
}: {
  selected: string | null;
  select: (cardId: string) => void;
}) {
  const {
    snapshot,
    agents,
    project,
    activity,
    diffs,
    loadCardDiff,
    approve,
    reject,
    toast,
  } = useStore();

  const [why, setWhy] = useState("");
  const cards = useMemo(
    () => (snapshot?.cards ?? []).filter((c) => c.status === "review"),
    [snapshot],
  );
  const card = cards.find((c) => c.id === selected) ?? cards[0] ?? null;
  const diff = card ? diffs[card.id] : undefined;

  useEffect(() => {
    if (card) loadCardDiff(card.id);
  }, [card?.id, loadCardDiff]);

  useEffect(() => setWhy(""), [card?.id]);

  if (!card) {
    return (
      <div
        style={{
          flex: 1,
          display: "grid",
          placeItems: "center",
          padding: 40,
          animation: "paneIn .4s cubic-bezier(.2,.8,.25,1) both",
        }}
      >
        <div style={{ maxWidth: 420, textAlign: "center" }}>
          <div style={{ font: "600 15px var(--sans)", color: "var(--text)", marginBottom: 8 }}>
            Nothing is waiting for you
          </div>
          <div style={{ font: "400 12.5px/1.7 var(--sans)", color: "var(--text3)" }}>
            A run that finishes lands here once its reviewer has read it. Until then the board is the
            place to look.
          </div>
        </div>
      </div>
    );
  }

  const agent = agents.find((a) => a.id === card.agent_id);
  const t = tone(agent?.tone);
  const session = snapshot?.sessions.find((s) => s.card_id === card.id);
  const history = activity.filter((a) => a.card_id === card.id).slice(0, 8);

  const facts: { k: string; v: string }[] = [
    { k: "branch", v: diff?.branch ?? session?.branch ?? "no branch yet" },
    { k: "worktree", v: diff?.worktree ?? session?.worktree ?? "none" },
    { k: "session", v: (diff?.session_id ?? card.session_id ?? "none").slice(0, 16) },
    { k: "base", v: diff?.base ?? project?.base_branch ?? "—" },
    { k: "files", v: diff ? plural(diff.files.length, "file") : "reading…" },
    {
      k: "reviewer",
      v: agent?.reviewer === "director" ? "the Director, then you" : agent?.reviewer === "human" ? "you" : "nobody",
    },
  ];

  const lines = (diff?.patch ?? "").split("\n");

  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "grid",
        gridTemplateRows: "auto minmax(0,1fr)",
        overflow: "hidden",
        animation: "paneIn .4s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "14px 20px 12px",
          borderBottom: "1px solid var(--line)",
        }}
      >
        <Glyph color={t.color} soft={t.soft} size={24} radius={8} font={10}>
          {agent?.initial ?? "?"}
        </Glyph>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ font: "600 14px var(--sans)", color: "var(--text)", ...truncate }}>
            {card.title}
          </div>
          <div style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
            {card.id} · {agent?.name ?? card.agent_id} · {plural(card.turns, "turn")} ·{" "}
            {plural(card.runs, "run")} · {money(card.cost_usd, 4)}
          </div>
        </div>
        {cards.length > 1 && (
          <div style={{ display: "flex", gap: 5 }}>
            {cards.map((c) => (
              <span
                key={c.id}
                className="chip"
                onClick={() => select(c.id)}
                style={{
                  padding: "4px 9px",
                  borderRadius: 999,
                  border: `1px solid ${c.id === card.id ? "var(--accentLine)" : "var(--line3)"}`,
                  background: c.id === card.id ? "var(--accentSoft)" : "transparent",
                  ...mono,
                  fontSize: 10,
                  color: c.id === card.id ? "var(--accent2)" : "var(--text3)",
                  cursor: "pointer",
                }}
              >
                {c.id}
              </span>
            ))}
          </div>
        )}
        <span style={{ ...mono, fontSize: 11, fontWeight: 500, color: "var(--ok)" }}>
          +{diff?.added ?? 0}
        </span>
        <span style={{ ...mono, fontSize: 11, fontWeight: 500, color: "var(--bad)" }}>
          −{diff?.removed ?? 0}
        </span>
      </div>

      <div
        style={{
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: "264px minmax(0,1fr)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            minHeight: 0,
            borderRight: "1px solid var(--line)",
            display: "flex",
            flexDirection: "column",
            overflowY: "auto",
          }}
        >
          <div style={{ padding: "14px 15px 20px" }}>
            {card.last_review && (
              <div
                style={{
                  padding: "11px 12px",
                  borderRadius: 11,
                  background: "var(--surface)",
                  border: "1px solid var(--line2)",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 8, paddingBottom: 6 }}>
                  <Glyph
                    color="var(--onAccent)"
                    soft="linear-gradient(140deg,var(--warn),#e0854a)"
                    size={18}
                    radius={6}
                    font={8.5}
                  >
                    {card.last_review.by === "director" ? "D" : "Y"}
                  </Glyph>
                  <span style={{ font: "600 11px var(--sans)", color: "var(--text)" }}>
                    {card.last_review.by === "director" ? "The Director" : "You"}{" "}
                    {card.last_review.approved ? "approved it" : "sent it back"}
                  </span>
                </div>
                <div style={{ font: "400 11.5px/1.55 var(--sans)", color: "var(--text2)" }}>
                  {card.last_review.reason || "no reason recorded"}
                </div>
              </div>
            )}

            <Eyebrow style={{ display: "block", padding: "14px 2px 6px" }}>WHERE IT LIVES</Eyebrow>
            {facts.map((f) => (
              <div key={f.k} style={{ display: "flex", alignItems: "baseline", gap: 8, padding: "4px 2px" }}>
                <span
                  style={{
                    width: 74,
                    flex: "none",
                    font: "400 10.5px var(--sans)",
                    color: "var(--text4)",
                  }}
                >
                  {f.k}
                </span>
                <span
                  title={f.v}
                  style={{ flex: 1, ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text2)", ...truncate }}
                >
                  {f.v}
                </span>
              </div>
            ))}

            <Eyebrow style={{ display: "block", padding: "16px 2px 6px" }}>HISTORY</Eyebrow>
            {history.length === 0 && (
              <div style={{ font: "400 11px var(--sans)", color: "var(--text4)" }}>
                Nothing recorded for this card yet.
              </div>
            )}
            {history.map((row) => {
              const dot =
                row.kind === "review"
                  ? row.label.startsWith("Approved")
                    ? "var(--ok)"
                    : "var(--warn)"
                  : row.kind === "run"
                    ? "var(--accent)"
                    : "var(--line4)";
              return (
                <div key={row.seq} style={{ display: "flex", gap: 9, padding: "4px 2px" }}>
                  <span
                    style={{
                      width: 5,
                      height: 5,
                      flex: "none",
                      marginTop: 6,
                      borderRadius: "50%",
                      background: dot,
                    }}
                  />
                  <span style={{ flex: 1, font: "400 11px/1.5 var(--sans)", color: "var(--text2)" }}>
                    {row.label}
                    {row.detail ? ` — ${row.detail}` : ""}
                    <span style={{ color: "var(--text4)" }}> · {ago(row.ts_ms)}</span>
                  </span>
                </div>
              );
            })}

            <Eyebrow style={{ display: "block", padding: "16px 2px 6px" }}>MODEL</Eyebrow>
            <div style={{ font: "400 11.5px/1.6 var(--sans)", color: "var(--text2)" }}>
              {MODELS.find((m) => m.id === agent?.model)?.name ?? "auto"} ·{" "}
              {agent?.expected_output || "no expected output set"}
            </div>
          </div>
        </div>

        <div
          style={{
            minWidth: 0,
            minHeight: 0,
            display: "grid",
            gridTemplateRows: "minmax(0,1fr) auto",
            overflow: "hidden",
          }}
        >
          <div
            className="logscroll"
            style={{ minHeight: 0, overflowY: "auto", padding: "14px 0", animation: "fadeIn .5s ease .1s both" }}
          >
            <div style={{ padding: "0 18px 10px", ...mono, fontSize: 11, color: "var(--text4)" }}>
              git diff {diff?.base ?? project?.base_branch ?? "main"}…
              {diff?.branch ?? session?.branch ?? "the worktree"}
            </div>
            {!diff && (
              <div style={{ padding: "0 18px", ...mono, fontSize: 12, color: "var(--text4)" }}>
                reading the worktree…
              </div>
            )}
            {diff && diff.patch.trim() === "" && (
              <div style={{ padding: "0 18px", font: "400 12.5px/1.7 var(--sans)", color: "var(--text3)" }}>
                This card changed nothing against {diff.base}. Either the run wrote nothing, or its
                work was already on the base branch.
              </div>
            )}
            {lines.map((text, i) => {
              const c = classify(text);
              return (
                <div
                  key={i}
                  style={{
                    padding: "0 18px",
                    background: c.bg,
                    color: c.color,
                    ...mono,
                    fontSize: 12,
                    lineHeight: 1.85,
                    whiteSpace: "pre",
                  }}
                >
                  {text}
                </div>
              );
            })}
          </div>

          <div
            style={{
              borderTop: "1px solid var(--line)",
              padding: "12px 18px 14px",
              display: "flex",
              flexDirection: "column",
              gap: 9,
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "9px 12px",
                borderRadius: 10,
                background: "var(--surface)",
                border: "1px solid var(--line3)",
              }}
            >
              <input
                value={why}
                onChange={(e) => setWhy(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && why.trim()) reject(card.id, why);
                }}
                placeholder="Why it is going back — the agent gets this as its next instruction…"
                style={{
                  flex: 1,
                  minWidth: 0,
                  border: "none",
                  outline: "none",
                  background: "transparent",
                  font: "400 12px var(--sans)",
                  color: "var(--text)",
                }}
              />
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span
                style={{
                  flex: 1,
                  maxWidth: 430,
                  font: "400 11px/1.45 var(--sans)",
                  color: "var(--text4)",
                }}
              >
                Approving moves the card to Done. Harness does not merge:{" "}
                <span style={{ ...mono, fontSize: 10.5, color: "var(--text3)" }}>
                  {diff?.branch ?? session?.branch ?? "the branch"}
                </span>{" "}
                and its worktree stay until you remove them.
              </span>
              <span
                className="quiet"
                onClick={() => {
                  if (!why.trim()) {
                    toast(
                      "var(--warn)",
                      "Say why first",
                      "The reason is what the agent gets as its next instruction.",
                    );
                    return;
                  }
                  reject(card.id, why);
                }}
                style={{
                  padding: "8px 14px",
                  borderRadius: 9,
                  border: "1px solid rgba(255,179,92,.4)",
                  color: "var(--warn)",
                  font: "600 12px var(--sans)",
                  cursor: "pointer",
                }}
              >
                Send back
              </span>
              <span
                className="primary"
                onClick={() => approve(card.id)}
                style={{
                  padding: "8px 16px",
                  borderRadius: 9,
                  background: "var(--ok)",
                  color: "#08211a",
                  font: "600 12px var(--sans)",
                  cursor: "pointer",
                }}
              >
                Approve
              </span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
