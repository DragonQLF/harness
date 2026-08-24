/** Everything that floats above the shell: toasts, the permission sheet, the
 *  send-back sheet and the command palette. The conversation itself is a
 *  screen now, not an overlay. */

import { useEffect, useMemo, useRef, useState } from "react";
import { ago } from "../lib/format";
import { tone } from "../lib/types";
import { useStore } from "../state/store";
import { truncate } from "./ui";

export function Toasts() {
  const { toasts, dismissToast } = useStore();
  return (
    <div
      style={{
        position: "absolute",
        right: 22,
        bottom: 22,
        display: "flex",
        flexDirection: "column",
        gap: 9,
        alignItems: "flex-end",
        zIndex: 60,
      }}
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => dismissToast(t.id)}
          style={{
            minWidth: 250,
            maxWidth: 330,
            display: "flex",
            gap: 11,
            padding: "13px 15px",
            borderRadius: 15,
            background: "var(--elev)",
            border: "1px solid var(--line)",
            boxShadow: "var(--shadow)",
            animation: "toastIn .3s cubic-bezier(.2,.8,.2,1) both",
            cursor: "pointer",
          }}
        >
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: "50%",
              marginTop: 6,
              flex: "none",
              background: t.tone,
            }}
          />
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 13, fontWeight: 700, marginBottom: 3 }}>{t.title}</div>
            {t.body && (
              <div style={{ fontSize: 12, color: "var(--text3)", lineHeight: 1.5 }}>{t.body}</div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

const scrim = {
  position: "absolute" as const,
  inset: 0,
  zIndex: 80,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  background: "rgba(18,18,26,.42)",
  backdropFilter: "blur(4px)",
  animation: "fadeIn .18s ease both",
};

export function ApprovalSheet({ close }: { close: () => void }) {
  const { approvals, answerApproval, agents, snapshot, projects } = useStore();
  const [always, setAlways] = useState(false);
  const request = approvals[0];

  useEffect(() => {
    setAlways(false);
  }, [request?.request_id]);

  if (!request) return null;

  const card = snapshot?.cards.find((c) => c.id === request.card_id);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const project = projects.find((p) => p.id === request.project_id);
  const t = tone(agent?.tone ?? "warn");

  const answer = (allow: boolean) => {
    answerApproval(request.request_id, allow, allow && always);
    if (approvals.length <= 1) close();
  };

  return (
    <div style={scrim} onClick={close}>
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 490,
          padding: 24,
          borderRadius: 22,
          background: "var(--elev)",
          border: "1px solid var(--line)",
          boxShadow: "var(--shadow)",
          animation: "popIn .3s cubic-bezier(.2,.8,.2,1) both",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 11, marginBottom: 16 }}>
          <span
            style={{
              width: 38,
              height: 38,
              borderRadius: "50%",
              background: t.soft,
              color: t.color,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 13,
              fontWeight: 700,
            }}
          >
            {agent?.initial ?? "?"}
          </span>
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 13.5, fontWeight: 700 }}>
              {agent?.name ?? "An agent"} is asking
            </span>
            <span
              style={{
                display: "block",
                fontFamily: "var(--mono)",
                fontSize: 11,
                color: "var(--text3)",
                marginTop: 3,
              }}
            >
              {request.card_id ?? "—"} · paused · {project?.name ?? request.project_id}
            </span>
          </span>
          <span style={{ fontSize: 11, color: "var(--text3)" }}>{ago(request.asked_ms)}</span>
        </div>

        <div style={{ fontSize: 18, fontWeight: 800, marginBottom: 5, letterSpacing: "-.02em" }}>
          {card?.title ?? `Permission for ${request.tool}`}
        </div>
        <div
          style={{
            display: "inline-block",
            fontFamily: "var(--mono)",
            fontSize: 12,
            color: "var(--warn)",
            background: "var(--warnSoft)",
            padding: "6px 11px",
            borderRadius: 9,
            marginBottom: 14,
          }}
        >
          {request.tool}
        </div>
        <pre
          style={{
            margin: "0 0 14px",
            padding: "14px 16px",
            borderRadius: 15,
            background: "var(--surface2)",
            fontFamily: "inherit",
            fontSize: 12.5,
            lineHeight: 1.65,
            color: "var(--text2)",
            maxHeight: 160,
            overflow: "auto",
            whiteSpace: "pre-wrap",
          }}
        >
          {request.summary ||
            "The agent asked to use a tool outside its permissions. No details were given."}
        </pre>

        <div style={{ display: "flex", alignItems: "center", gap: 9, marginBottom: 16 }}>
          <button
            type="button"
            onClick={() => setAlways((v) => !v)}
            aria-pressed={always}
            style={{
              width: 18,
              height: 18,
              flex: "none",
              borderRadius: 6,
              border: "1px solid var(--line)",
              background: always ? "var(--accent)" : "transparent",
              cursor: "pointer",
              color: "var(--onAccent)",
              fontSize: 10,
              lineHeight: 1,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            {always ? "✓" : ""}
          </button>
          <span style={{ fontSize: 12.5, color: "var(--text2)" }}>
            Stop asking me about {request.tool}
          </span>
        </div>

        <div style={{ display: "flex", gap: 9 }}>
          <button
            type="button"
            className="hv-bright"
            onClick={() => answer(true)}
            style={{
              flex: 1,
              padding: 12,
              border: "none",
              borderRadius: 999,
              background: "var(--accent)",
              color: "var(--onAccent)",
              fontSize: 13.5,
              fontWeight: 700,
              cursor: "pointer",
              transition: "filter .18s ease",
            }}
          >
            {always ? "Allow from now on" : "Allow once"}
          </button>
          <button
            type="button"
            className="hv-danger"
            onClick={() => answer(false)}
            style={{
              flex: 1,
              padding: 12,
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              fontSize: 13.5,
              fontWeight: 600,
              cursor: "pointer",
              transition: "all .18s ease",
            }}
          >
            Deny
          </button>
        </div>
        {approvals.length > 1 && (
          <div style={{ marginTop: 12, fontSize: 11.5, color: "var(--text3)", textAlign: "center" }}>
            {approvals.length - 1} more waiting after this one
          </div>
        )}
      </div>
    </div>
  );
}

export function RejectSheet({ cardId, close }: { cardId: string | null; close: () => void }) {
  const { snapshot, reject } = useStore();
  const [why, setWhy] = useState("");
  const card = snapshot?.cards.find((c) => c.id === cardId);

  useEffect(() => {
    setWhy("");
  }, [cardId]);

  if (!cardId) return null;

  const send = () => {
    reject(cardId, why);
    close();
  };

  return (
    <div style={scrim} onClick={close}>
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 440,
          padding: 24,
          borderRadius: 22,
          background: "var(--elev)",
          border: "1px solid var(--line)",
          boxShadow: "var(--shadow)",
          animation: "popIn .3s cubic-bezier(.2,.8,.2,1) both",
        }}
      >
        <div style={{ fontSize: 18, fontWeight: 800, marginBottom: 6, letterSpacing: "-.02em" }}>
          Send it back
        </div>
        <p style={{ margin: "0 0 14px", fontSize: 13, color: "var(--text2)", lineHeight: 1.55 }}>
          {card?.title ?? cardId}
        </p>
        <textarea
          rows={3}
          autoFocus
          value={why}
          onChange={(e) => setWhy(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) send();
          }}
          placeholder="What has to change? The agent gets this verbatim."
          className="hv-border"
          style={{
            width: "100%",
            resize: "none",
            padding: "13px 15px",
            borderRadius: 15,
            border: "1px solid var(--line)",
            background: "var(--surface2)",
            fontSize: 13,
            lineHeight: 1.6,
            outline: "none",
          }}
        />
        <div style={{ display: "flex", gap: 9, marginTop: 14 }}>
          <button
            type="button"
            className="hv-bright"
            onClick={send}
            style={{
              flex: 1,
              padding: 12,
              border: "none",
              borderRadius: 999,
              background: "var(--bad)",
              color: "#fff",
              fontSize: 13.5,
              fontWeight: 700,
              cursor: "pointer",
              transition: "filter .18s ease",
              opacity: why.trim() ? 1 : 0.65,
            }}
          >
            Send back with reason
          </button>
          <button
            type="button"
            className="hv-soft"
            onClick={close}
            style={{
              padding: "12px 18px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: 13.5,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export interface PaletteAction {
  name: string;
  hint: string;
  color: string;
  run: () => void;
}

export function CommandPalette({
  open,
  close,
  actions,
}: {
  open: boolean;
  close: () => void;
  actions: PaletteAction[];
}) {
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const input = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    setQ("");
    setCursor(0);
    const t = window.setTimeout(() => input.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, [open]);

  const hits = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return actions
      .filter(
        (a) =>
          !needle ||
          a.name.toLowerCase().includes(needle) ||
          a.hint.toLowerCase().includes(needle),
      )
      .slice(0, 9);
  }, [actions, q]);

  if (!open) return null;

  const pick = (i: number) => {
    const action = hits[i];
    if (!action) return;
    close();
    action.run();
  };

  return (
    <div
      onClick={close}
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 90,
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "center",
        paddingTop: 100,
        background: "rgba(18,18,26,.38)",
        backdropFilter: "blur(4px)",
        animation: "fadeIn .16s ease both",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 560,
          borderRadius: 20,
          background: "var(--elev)",
          border: "1px solid var(--line)",
          boxShadow: "var(--shadow)",
          overflow: "hidden",
          animation: "popIn .26s cubic-bezier(.2,.8,.2,1) both",
        }}
      >
        <input
          ref={input}
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setCursor(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setCursor((c) => Math.min(hits.length - 1, c + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setCursor((c) => Math.max(0, c - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              pick(cursor);
            } else if (e.key === "Escape") {
              close();
            }
          }}
          placeholder="Search cards, sessions, agents…"
          style={{
            width: "100%",
            padding: "17px 20px",
            border: "none",
            borderBottom: "1px solid var(--line)",
            background: "transparent",
            fontSize: 14,
            outline: "none",
          }}
        />
        <div style={{ maxHeight: 320, overflowY: "auto", padding: 8 }}>
          {hits.map((a, i) => (
            <button
              key={`${a.name}-${i}`}
              type="button"
              onMouseEnter={() => setCursor(i)}
              onClick={() => pick(i)}
              style={{
                width: "100%",
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "10px 12px",
                border: "none",
                borderRadius: 12,
                background: i === cursor ? "var(--hover)" : "transparent",
                color: "var(--text)",
                fontSize: 13.5,
                cursor: "pointer",
                textAlign: "left",
                transition: "background .14s ease",
              }}
            >
              <span
                style={{ width: 7, height: 7, borderRadius: "50%", flex: "none", background: a.color }}
              />
              <span style={{ flex: 1, fontWeight: 500, ...truncate }}>{a.name}</span>
              <span style={{ fontSize: 11.5, color: "var(--text3)" }}>{a.hint}</span>
            </button>
          ))}
          {hits.length === 0 && (
            <div style={{ padding: 24, textAlign: "center", fontSize: 12.5, color: "var(--text3)" }}>
              No matches
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
