/** Everything that floats above the shell: the Director dock and its collapsed
 *  rail, toasts, the permission sheet, the send-back sheet and the palette. */

import { useEffect, useMemo, useRef, useState } from "react";
import { ago } from "../lib/format";
import { tone } from "../lib/types";
import { useStore } from "../state/store";
import { tabular, truncate } from "./ui";

/** The list of conversations: what exists, and what to do with each one. It is
 *  a view of backend state — every action is a round trip, nothing is decided
 *  here. */
function ConversationList({ close }: { close: () => void }) {
  const {
    conversations,
    conversationId,
    agents,
    projects,
    openConversation,
    renameConversation,
    archiveConversation,
    deleteConversation,
  } = useStore();
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const commit = (id: string) => {
    const clean = draft.trim();
    setRenaming(null);
    if (clean) renameConversation(id, clean);
  };

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 3,
        display: "flex",
        flexDirection: "column",
        background: "var(--surface)",
        animation: "fadeIn .18s ease both",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "14px 16px",
          borderBottom: "1px solid var(--line)",
        }}
      >
        <span style={{ flex: 1, fontSize: 13.5, fontWeight: 700 }}>Conversations</span>
        <span style={{ fontSize: 11.5, color: "var(--text3)", ...tabular }}>
          {conversations.length}
        </span>
        <button
          type="button"
          className="hv-text"
          title="Back to the conversation"
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

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "8px 8px 12px" }}>
        {conversations.length === 0 && (
          <p style={{ margin: 0, padding: 14, fontSize: 12.5, color: "var(--text3)", lineHeight: 1.7 }}>
            No conversations yet. Anything you ask starts one, and it is kept — you can come back
            to it after restarting Harness.
          </p>
        )}
        {conversations.map((c) => {
          const profile = agents.find((a) => a.id === c.profile_id);
          const project = projects.find((p) => p.id === c.project_id);
          const current = c.id === conversationId;
          const t = tone(profile?.tone ?? "info");
          return (
            <div
              key={c.id}
              style={{
                padding: "9px 10px",
                marginBottom: 4,
                borderRadius: 12,
                background: current ? "var(--accentSoft)" : "transparent",
                boxShadow: current ? "inset 0 0 0 1px var(--accentLine)" : "none",
              }}
            >
              {renaming === c.id ? (
                <input
                  autoFocus
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onBlur={() => commit(c.id)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commit(c.id);
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  style={{
                    width: "100%",
                    padding: "5px 8px",
                    border: "1px solid var(--accentLine)",
                    borderRadius: 8,
                    background: "var(--surface2)",
                    color: "var(--text)",
                    fontSize: 12.5,
                    outline: "none",
                  }}
                />
              ) : (
                <button
                  type="button"
                  className="hv-text"
                  onClick={() => {
                    openConversation(c.id);
                    close();
                  }}
                  style={{
                    display: "block",
                    width: "100%",
                    padding: 0,
                    border: "none",
                    background: "transparent",
                    color: "var(--text)",
                    fontSize: 12.5,
                    fontWeight: current ? 700 : 600,
                    textAlign: "left",
                    cursor: "pointer",
                    ...truncate,
                  }}
                >
                  {c.title}
                </button>
              )}

              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  marginTop: 5,
                  flexWrap: "wrap",
                }}
              >
                <span
                  style={{
                    padding: "1px 7px",
                    borderRadius: 999,
                    background: t.soft,
                    color: t.color,
                    fontSize: 10,
                    fontWeight: 700,
                  }}
                >
                  {profile?.name ?? c.profile_id}
                </span>
                {project && (
                  <span
                    style={{
                      padding: "1px 7px",
                      borderRadius: 999,
                      background: "var(--surface2)",
                      color: "var(--text3)",
                      fontSize: 10,
                      fontWeight: 700,
                    }}
                  >
                    {project.name}
                  </span>
                )}
                {c.resume_failed && (
                  <span
                    title="The Claude session behind this chat could not be resumed. The transcript is still here."
                    style={{
                      padding: "1px 7px",
                      borderRadius: 999,
                      background: "var(--warnSoft)",
                      color: "var(--warn)",
                      fontSize: 10,
                      fontWeight: 700,
                    }}
                  >
                    session lost
                  </span>
                )}
                <span style={{ fontSize: 10.5, color: "var(--text3)", ...tabular }}>
                  {ago(c.updated_ms)}
                </span>
                <span style={{ flex: 1 }} />
                {[
                  {
                    label: "Rename",
                    glyph: "\u270e",
                    run: () => {
                      setDraft(c.title);
                      setRenaming(c.id);
                    },
                  },
                  {
                    label: c.archived ? "Restore" : "Archive",
                    glyph: "\u25f4",
                    run: () => archiveConversation(c.id, !c.archived),
                  },
                  { label: "Delete", glyph: "\u2715", run: () => deleteConversation(c.id) },
                ].map((action) => (
                  <button
                    key={action.label}
                    type="button"
                    className="hv-text"
                    title={action.label}
                    onClick={action.run}
                    style={{
                      width: 20,
                      height: 20,
                      border: "none",
                      borderRadius: 6,
                      background: "transparent",
                      color: "var(--text3)",
                      fontSize: 10.5,
                      cursor: "pointer",
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

export function DirectorDock({ close }: { close: () => void }) {
  const {
    chat,
    chatBusy,
    chatThinking,
    sendChat,
    agents,
    projects,
    conversation,
    conversations,
    newConversation,
    pinConversation,
  } = useStore();
  const [text, setText] = useState("");
  const [history, setHistory] = useState(false);
  const end = useRef<HTMLDivElement | null>(null);
  // Whoever this conversation is with — the Director unless it was started
  // with a specialist.
  const speaker = agents.find((a) => a.id === (conversation?.profile_id ?? "director"));
  const t = tone(speaker?.tone ?? "info");
  const project = projects.find((p) => p.id === conversation?.project_id);

  useEffect(() => {
    end.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat, chatBusy, chatThinking]);

  const send = () => {
    if (!text.trim() || chatBusy) return;
    sendChat(text);
    setText("");
  };

  return (
    <aside
      style={{
        position: "relative",
        width: 344,
        flex: "none",
        display: "flex",
        flexDirection: "column",
        background: "var(--surface)",
        borderLeft: "1px solid var(--line)",
        minHeight: 0,
        animation: "fadeIn .25s ease both",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 11,
          padding: "14px 16px",
          borderBottom: "1px solid var(--line)",
        }}
      >
        <span
          style={{
            width: 34,
            height: 34,
            borderRadius: "50%",
            background: t.soft,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            fontSize: 13,
            fontWeight: 700,
            color: t.color,
            flex: "none",
          }}
        >
          {speaker?.initial ?? "D"}
        </span>
        <span style={{ flex: 1, minWidth: 0 }}>
          <span
            style={{ display: "block", fontSize: 13.5, fontWeight: 700, ...truncate }}
            title={conversation?.title}
          >
            {conversation?.title && conversation.title !== "New conversation"
              ? conversation.title
              : (speaker?.name ?? "Director")}
          </span>
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 5,
              fontSize: 11.5,
              color: "var(--text3)",
              marginTop: 2,
            }}
          >
            <span style={{ ...truncate }}>
              {chatBusy ? "thinking" : (speaker?.name ?? "Director")}
            </span>
            {project && (
              <>
                <span>&middot;</span>
                <span style={{ ...truncate }}>{project.name}</span>
              </>
            )}
          </span>
        </span>
        <button
          type="button"
          className="hv-text"
          title={`Conversations (${conversations.length})`}
          onClick={() => setHistory((h) => !h)}
          style={{
            width: 26,
            height: 26,
            flex: "none",
            border: "1px solid var(--line)",
            borderRadius: "50%",
            background: "transparent",
            color: "var(--text3)",
            cursor: "pointer",
            fontSize: 11,
            transition: "all .18s ease",
          }}
        >
          &#9776;
        </button>
        <button
          type="button"
          className="hv-text"
          title="New conversation"
          onClick={() => newConversation()}
          style={{
            width: 26,
            height: 26,
            flex: "none",
            border: "1px solid var(--line)",
            borderRadius: "50%",
            background: "transparent",
            color: "var(--text3)",
            cursor: "pointer",
            fontSize: 13,
            transition: "all .18s ease",
          }}
        >
          +
        </button>
        <button
          type="button"
          className="hv-text"
          title="Hide"
          onClick={close}
          style={{
            width: 26,
            height: 26,
            flex: "none",
            border: "1px solid var(--line)",
            borderRadius: "50%",
            background: "transparent",
            color: "var(--text3)",
            cursor: "pointer",
            fontSize: 11,
            transition: "all .18s ease",
          }}
        >
          &#8250;
        </button>
      </div>

      {conversation?.resume_failed && (
        <div
          style={{
            padding: "9px 16px",
            background: "var(--warnSoft)",
            color: "var(--warn)",
            borderBottom: "1px solid var(--line)",
            fontSize: 11.5,
            lineHeight: 1.55,
          }}
        >
          The Claude session behind this conversation could not be resumed. Everything above is
          still readable; your next message starts a new session.
        </div>
      )}

      <div
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "14px 16px",
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        {chat.length === 0 && !chatBusy && (
          <p style={{ margin: 0, fontSize: 12.5, color: "var(--text3)", lineHeight: 1.7 }}>
            Ask about anything — a plan, a question, a piece of work you want done. This
            conversation is kept, so you can pick it up again after a restart.
          </p>
        )}
        {chat.map((m, i) =>
          m.role === "notice" ? (
            <div
              key={i}
              style={{
                alignSelf: "stretch",
                padding: "8px 11px",
                borderRadius: 10,
                background: "var(--surface2)",
                color: "var(--text3)",
                fontSize: 11.5,
                lineHeight: 1.55,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {m.text}
            </div>
          ) : (
            <div
              key={i}
              style={{
                maxWidth: "90%",
                alignSelf: m.role === "user" ? "flex-end" : "flex-start",
                padding: "11px 14px",
                borderRadius: m.role === "user" ? "16px 16px 5px 16px" : "16px 16px 16px 5px",
                fontSize: 13,
                lineHeight: 1.6,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                background: m.role === "user" ? "var(--accent)" : "var(--surface2)",
                color: m.role === "user" ? "var(--onAccent)" : "var(--text)",
                animation: "fadeUp .3s ease both",
              }}
            >
              {m.text}
            </div>
          ),
        )}
        {chatBusy && (
          <div
            style={{
              alignSelf: "flex-start",
              maxWidth: "90%",
              display: "flex",
              alignItems: "flex-start",
              gap: 8,
              padding: "11px 14px",
              borderRadius: "16px 16px 16px 5px",
              background: "var(--surface2)",
            }}
          >
            <span
              style={{
                width: 12,
                height: 12,
                flex: "none",
                marginTop: 3,
                borderRadius: "50%",
                border: "1.5px solid var(--accentLine)",
                borderTopColor: "var(--accent)",
                animation: "spin .9s linear infinite",
              }}
            />
            <span
              style={{
                fontSize: 12,
                color: "var(--text3)",
                fontStyle: chatThinking ? "italic" : "normal",
                lineHeight: 1.55,
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
                maxHeight: 96,
                overflow: "hidden",
              }}
            >
              {chatThinking || "thinking\u2026"}
            </span>
          </div>
        )}
        <div ref={end} />
      </div>

      <div style={{ padding: "12px 14px 14px", borderTop: "1px solid var(--line)" }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginBottom: 9,
            flexWrap: "wrap",
          }}
        >
          {/* Which repository it can read while answering. Not a scope on the
              conversation: it still sees every board. */}
          <select
            value={conversation?.project_id ?? ""}
            onChange={(e) =>
              conversation && pinConversation(conversation.id, e.target.value || null)
            }
            disabled={!conversation}
            title="The project whose code this conversation can read"
            style={{
              maxWidth: 150,
              padding: "5px 8px",
              border: "1px solid var(--line)",
              borderRadius: 999,
              background: "transparent",
              color: "var(--text2)",
              fontSize: 11,
              cursor: "pointer",
            }}
          >
            <option value="">No project</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          {["What needs me?", "Help me plan something"].map((p) => (
            <button
              key={p}
              type="button"
              className="hv-soft"
              onClick={() => setText(p)}
              style={{
                padding: "6px 12px",
                border: "1px solid var(--line)",
                borderRadius: 999,
                background: "transparent",
                color: "var(--text2)",
                fontSize: 11.5,
                cursor: "pointer",
                transition: "all .18s ease",
              }}
            >
              {p}
            </button>
          ))}
        </div>
        <div
          style={{
            display: "flex",
            gap: 8,
            alignItems: "center",
            padding: "5px 6px 5px 14px",
            border: "1px solid var(--line)",
            borderRadius: 999,
            background: "var(--surface2)",
          }}
        >
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder={`Ask ${speaker?.name ?? "the Director"}\u2026`}
            style={{
              flex: 1,
              minWidth: 0,
              border: "none",
              background: "transparent",
              fontSize: 13,
              outline: "none",
              padding: "8px 0",
            }}
          />
          <button
            type="button"
            className="hv-bright"
            onClick={send}
            style={{
              width: 32,
              height: 32,
              flex: "none",
              border: "none",
              borderRadius: "50%",
              background: "var(--accent)",
              color: "var(--onAccent)",
              fontSize: 14,
              cursor: "pointer",
              transition: "filter .18s ease",
              opacity: text.trim() && !chatBusy ? 1 : 0.5,
            }}
          >
            &#8593;
          </button>
        </div>
      </div>

      {history && <ConversationList close={() => setHistory(false)} />}
    </aside>
  );
}

/** The 46px rail that stands in for the dock when it is hidden. */
export function DirectorRail({ open }: { open: () => void }) {
  const { agents, chatBusy } = useStore();
  const director = agents.find((a) => a.id === "director");
  const t = tone(director?.tone ?? "info");
  return (
    <button
      type="button"
      className="hv-soft"
      onClick={open}
      title="Ask the Director"
      style={{
        width: 46,
        flex: "none",
        border: "none",
        borderLeft: "1px solid var(--line)",
        background: "var(--surface)",
        color: "var(--text3)",
        cursor: "pointer",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 12,
        transition: "all .18s ease",
      }}
    >
      <span
        style={{
          width: 30,
          height: 30,
          borderRadius: "50%",
          background: t.soft,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 12,
          fontWeight: 700,
          color: t.color,
          animation: chatBusy ? "breathe 1.6s ease-in-out infinite" : undefined,
        }}
      >
        {director?.initial ?? "D"}
      </span>
      <span
        style={{
          writingMode: "vertical-rl",
          fontSize: 11.5,
          fontWeight: 700,
          letterSpacing: ".09em",
        }}
      >
        Director
      </span>
    </button>
  );
}

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
