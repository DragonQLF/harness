/** The chat screen: one conversation, the permission requests it produced, and
 *  the field you answer in. The Director is the default voice, but any profile
 *  that can hold a conversation gets the same screen. */

import { useEffect, useMemo, useRef, useState } from "react";
import { clock, money, plural } from "../lib/format";
import { ruleLabel, tone, type AllowRule, type PendingApproval } from "../lib/types";
import { useStore, type ChatMsg } from "../state/store";
import { api, reason } from "../lib/ipc";
import { Caret, Glyph, Icon, Spinner, mono, truncate } from "../components/ui";

/** What a standing rule for this request would cover. Mirrors the backend: a
 *  shell call is scoped to its leading words, anything else to the tool. */
function scopeOf(request: PendingApproval): AllowRule {
  const head = request.tool.toLowerCase().split("(")[0]!.trim();
  const shell = ["bash", "shell", "sh", "powershell"].includes(head);
  if (!shell) return { tool: request.tool };
  const input = request.input as { command?: string } | null;
  const command = (input?.command ?? request.summary ?? "").trim();
  // Two words is what reads as a scope: `git push`, `cargo test`.
  const words = command.split(/\s+/).filter(Boolean).slice(0, 2).join(" ");
  return { tool: request.tool, command: words || undefined };
}

/** A chained command cannot be narrowed into a rule, so Relay will not
 *  record one — the sheet says so rather than pretending. */
function scopable(request: PendingApproval): boolean {
  const rule = scopeOf(request);
  if (!rule.command) return !["bash", "shell", "sh", "powershell"].includes(rule.tool.toLowerCase());
  const input = request.input as { command?: string } | null;
  return !/[;&|]{1,2}|\$\(/.test(input?.command ?? "");
}

function CopyButton({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <span
      title={done ? "Copied" : "Copy"}
      onClick={() => {
        navigator.clipboard
          .writeText(text)
          .then(() => setDone(true))
          .catch(() => {});
        window.setTimeout(() => setDone(false), 1400);
      }}
      style={{
        display: "grid",
        placeItems: "center",
        width: 16,
        height: 16,
        color: done ? "var(--ok)" : "var(--text3)",
        cursor: "pointer",
      }}
    >
      {done ? <Icon.check /> : <Icon.copy />}
    </span>
  );
}

/** The permission sheet, in the conversation where the request was made. */
function ApprovalCard({
  request,
  more,
}: {
  request: PendingApproval;
  more: number;
}) {
  const { agents, snapshot, answerApproval } = useStore();
  const [always, setAlways] = useState(false);
  useEffect(() => setAlways(false), [request.request_id]);

  const card = snapshot?.cards.find((c) => c.id === request.card_id);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const t = tone(agent?.tone ?? "warn");
  const rule = scopeOf(request);
  const canScope = scopable(request);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 10,
        padding: "14px 16px",
        borderRadius: 12,
        background: "var(--surface)",
        border: "1px solid var(--warn)",
        animation: "sheetIn .42s cubic-bezier(.2,.8,.25,1) both",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <Glyph color={t.color} soft={t.soft} size={26} radius="50%" font={10}>
          {agent?.initial ?? "?"}
        </Glyph>
        <span style={{ flex: 1, minWidth: 0 }}>
          <span style={{ display: "block", font: "600 12.5px var(--sans)", color: "var(--text)" }}>
            {agent?.name ?? "An agent"} is asking permission
          </span>
          <span
            style={{ display: "block", marginTop: 2, ...mono, fontSize: 10.5, color: "var(--text3)" }}
          >
            {request.card_id ?? "no card"} · the run is paused until you answer
          </span>
        </span>
        <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
          {clock(request.asked_ms)}
        </span>
      </div>

      <span
        style={{
          alignSelf: "flex-start",
          padding: "6px 10px",
          borderRadius: 8,
          background: "var(--warnSoft)",
          ...mono,
          fontSize: 11.5,
          fontWeight: 500,
          color: "var(--warn)",
        }}
      >
        {request.tool}
      </span>

      <span
        style={{
          padding: "12px 14px",
          borderRadius: 12,
          background: "var(--bg)",
          font: "400 12.5px/1.65 var(--sans)",
          color: "var(--text2)",
          wordBreak: "break-word",
        }}
      >
        {card?.title ?? "It asked for a tool outside its permissions."}
        <span
          style={{
            display: "block",
            marginTop: 6,
            ...mono,
            fontSize: 11.5,
            fontWeight: 500,
            color: "var(--warn)",
          }}
        >
          {request.summary || "no details given"}
        </span>
      </span>

      <span
        onClick={() => canScope && setAlways((v) => !v)}
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: 10,
          cursor: canScope ? "pointer" : "default",
          opacity: canScope ? 1 : 0.6,
        }}
      >
        <span
          style={{
            width: 18,
            height: 18,
            flex: "none",
            marginTop: 1,
            borderRadius: 8,
            border: `1px solid ${always ? "var(--accent)" : "var(--line3)"}`,
            background: always ? "var(--accent)" : "transparent",
            display: "grid",
            placeItems: "center",
            transition: "background .16s ease,border-color .16s ease",
          }}
        >
          <span
            style={{
              width: 8,
              height: 8,
              borderRadius: 2,
              background: "var(--onAccent)",
              transform: `scale(${always ? 1 : 0})`,
              transition: "transform .18s cubic-bezier(.2,.8,.25,1)",
            }}
          />
        </span>
        <span style={{ flex: 1 }}>
          <span style={{ display: "block", font: "400 12.5px var(--sans)", color: "var(--text2)" }}>
            Always allow{" "}
            <span style={{ ...mono, fontSize: 11.5, color: "var(--text2)" }}>{ruleLabel(rule)}</span>
          </span>
          <span
            style={{
              display: "block",
              marginTop: 2,
              font: "400 10.5px/1.5 var(--sans)",
              color: "var(--text4)",
            }}
          >
            {canScope
              ? "Scoped to that command. A bare shell rule authorises nothing, so Relay will not record one."
              : "This command chains others, so it cannot be narrowed into a rule. It is allowed once."}
          </span>
        </span>
      </span>

      <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span
          className="primary"
          onClick={() => answerApproval(request.request_id, true, always && canScope)}
          style={{
            flex: 1,
            padding: 10,
            borderRadius: 999,
            background: "var(--accent)",
            color: "var(--onAccent)",
            font: "600 12.5px var(--sans)",
            textAlign: "center",
            cursor: "pointer",
          }}
        >
          Allow
        </span>
        <span
          className="quiet"
          onClick={() => answerApproval(request.request_id, false, false)}
          style={{
            flex: 1,
            padding: 10,
            borderRadius: 999,
            border: "1px solid var(--line3)",
            color: "var(--text2)",
            font: "500 12.5px var(--sans)",
            textAlign: "center",
            cursor: "pointer",
          }}
        >
          Deny
        </span>
      </span>
      {more > 0 && (
        <span style={{ font: "400 11.5px var(--sans)", color: "var(--text3)", textAlign: "center" }}>
          {more} more waiting after this one
        </span>
      )}
    </div>
  );
}

/** The live panel for one running card: what it is doing while you read. */
function RunPanel({ cardId }: { cardId: string }) {
  const { snapshot, agents, outputs, streams } = useStore();
  const card = snapshot?.cards.find((c) => c.id === cardId);
  const session = snapshot?.sessions.find((s) => s.card_id === cardId);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const t = tone(agent?.tone);
  const log = (outputs[cardId] ?? []).slice(-3);
  const stream = streams[cardId];
  if (!card) return null;

  return (
    <div
      style={{
        flex: "none",
        borderRadius: 12,
        border: "1px solid var(--line2)",
        background: "var(--elev)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "8px 12px",
          background: "var(--elev)",
          borderBottom: "1px solid var(--line2)",
        }}
      >
        <Glyph color={t.color} soft={t.soft} size={16} font={8}>
          {agent?.initial ?? "?"}
        </Glyph>
        <span style={{ flex: 1, font: "500 11.5px var(--sans)", color: "var(--text2)", ...truncate }}>
          {agent?.name ?? card.agent_id} · {card.id} · live
        </span>
        <span style={{ ...mono, fontSize: 10.5, color: "var(--text3)" }}>
          {plural(card.turns, "turn")} · {money(card.cost_usd, 2)}
          {session ? ` · ${session.branch ?? "no branch"}` : ""}
        </span>
      </div>
      <div style={{ padding: "8px 12px", ...mono, fontSize: 11.5, lineHeight: 1.9, color: "var(--text3)" }}>
        {log.map((l, i) => (
          <div key={i}>
            <span style={{ color: l.labelColor }}>{l.label}</span>{" "}
            <span style={{ color: "var(--text3)" }}>{l.text}</span>
          </div>
        ))}
        {(stream?.text || stream?.thinking) && (
          <div style={{ color: "var(--text2)", fontStyle: stream.text ? "normal" : "italic" }}>
            {(stream.text || stream.thinking).slice(-160)}
            <Caret />
          </div>
        )}
      </div>
    </div>
  );
}

/** A tool call and its result. The call opens the bubble pending-grey; the
 *  result closes it green or red, matched by id, with the full output one
 *  click away instead of dumped inline (#28). */
function ToolBubble({ msg, depth = 0 }: { msg: ChatMsg; depth?: number }) {
  const [open, setOpen] = useState(false);
  const isResult = msg.ok != null;
  const accent =
    !isResult
      ? "var(--info)"
      : msg.ok
        ? "var(--ok)"
        : "var(--bad)";
  return (
    <div style={{ display: "flex", gap: 12, alignItems: "flex-start", paddingLeft: depth * 16 }}>
      <span style={{ width: 28, flex: "none" }} />
      <div
        style={{
          flex: 1,
          minWidth: 0,
          borderRadius: 8,
          background: "var(--surface)",
          border: `1px solid ${isResult && msg.ok === false ? "var(--bad)" : "var(--line2)"}`,
          overflow: "hidden",
        }}
      >
        <div
          onClick={msg.detail ? () => setOpen((o) => !o) : undefined}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "6px 10px",
            cursor: msg.detail ? "pointer" : "default",
            ...mono,
            fontSize: 11.5,
          }}
        >
          <b style={{ color: accent, fontWeight: 600 }}>
            {isResult ? (msg.ok ? "↳ ok" : "↳ failed") : "tool"}
          </b>
          <span
            title={msg.tool}
            style={{
              flex: 1,
              minWidth: 0,
              color: "var(--text3)",
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {msg.tool ? `${msg.tool} · ` : ""}
            {msg.text}
          </span>
          {msg.detail && (
            <span style={{ color: "var(--text4)", flex: "none" }}>{open ? "▾" : "▸"}</span>
          )}
        </div>
        {open && msg.detail && (
          <div
            style={{
              borderTop: "1px solid var(--line)",
              padding: "8px 10px",
              maxHeight: 260,
              overflowY: "auto",
              color: "var(--text3)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              fontSize: 11.5,
              lineHeight: 1.7,
            }}
          >
            {msg.detail}
          </div>
        )}
      </div>
    </div>
  );
}

/** The last segment of a path, either separator — the chip shows the name and
 *  keeps the full path in its tooltip. */
function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut >= 0 ? path.slice(cut + 1) : path;
}

export function Chat() {
  const {
    chat,
    chatBusy,
    chatThinking,
    sendChat,
    agents,
    projects,
    project,
    snapshot,
    conversation,
    approvals,
    newConversation,
    pinConversation,
    toast,
  } = useStore();
  const conversationId = conversation?.id ?? null;

  const stopTurn = () => {
    if (conversationId) api.chatStop(conversationId).catch(() => {});
  };

  const [text, setText] = useState("");
  const [attached, setAttached] = useState<string[]>([]);
  const [pickProfile, setPickProfile] = useState(false);
  const [pickProject, setPickProject] = useState(false);
  const end = useRef<HTMLDivElement | null>(null);
  const field = useRef<HTMLTextAreaElement | null>(null);

  const speaker = agents.find((a) => a.id === (conversation?.profile_id ?? "director"));
  const t = tone(speaker?.tone ?? "warn");
  const pinned = projects.find((p) => p.id === conversation?.project_id);
  const running = (snapshot?.cards ?? []).filter((c) => c.status === "running");

  useEffect(() => {
    end.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [chat.length, chatBusy, chatThinking, approvals.length]);

  const send = () => {
    if ((!text.trim() && attached.length === 0) || chatBusy) return;
    sendChat(text, attached);
    setText("");
    setAttached([]);
  };

  /** Files travel with the next message and no further: the picker adds, the
   *  chip removes, sending clears. Nothing is copied anywhere — the paths are
   *  named in the turn and the agent reads them from disk. */
  const attach = async () => {
    try {
      const picked = await api.pickFiles();
      if (picked.length) setAttached((was) => [...new Set([...was, ...picked])]);
    } catch (e) {
      toast("var(--bad)", "Could not open the file picker", reason(e));
    }
  };

  // One divider per day, the way the design dates the conversation.
  const depthBy = useMemo(() => {
    const map = new Map<string, number>();
    return chat.map((m) => {
      let depth = 0;
      if (m.role === "tool" && m.toolUseId) {
        if (!map.has(m.toolUseId)) {
          const parent = m.parentToolUseId;
          map.set(m.toolUseId, parent ? (map.get(parent) ?? 0) + 1 : 0);
        }
        depth = map.get(m.toolUseId) ?? 0;
      }
      return depth;
    });
  }, [chat]);
  const dated = useMemo(() => {
    let day = "";
    return chat.map((m, mi) => {
      const stamp = new Date(m.ts || Date.now());
      const key = stamp.toDateString();
      const fresh = key !== day;
      day = key;
      const today = new Date().toDateString();
      return {
        msg: m,
        depth: depthBy[mi] ?? 0,
        divider: fresh
          ? `${key === today ? "TODAY" : stamp.toLocaleDateString(undefined, { day: "numeric", month: "short" }).toUpperCase()} · ${clock(m.ts)}`
          : null,
      };
    });
  }, [chat]);

  return (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {conversation?.resume_failed && (
        <div
          style={{
            flex: "none",
            padding: "10px 20px",
            background: "var(--badSoft)",
            borderBottom: "1px solid var(--line)",
            font: "400 11.5px/1.55 var(--sans)",
            color: "var(--bad2)",
          }}
        >
          The Claude session behind this conversation could not be resumed. Everything below is still
          readable; your next message starts a new session.
        </div>
      )}

      <div
        className="stagger"
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "18px 20px 10px",
          display: "flex",
          flexDirection: "column",
          // A short conversation belongs against the composer, not stranded at
          // the top of the pane with a screen of nothing under it. Once the
          // transcript is long enough to scroll, this stops having any effect.
          justifyContent: "flex-end",
          gap: 16,
        }}
      >
        {chat.length === 0 && !chatBusy && (
          <div style={{ maxWidth: 620, font: "400 14px/1.75 var(--sans)", color: "var(--text3)" }}>
            Ask {speaker?.name ?? "the Director"} about anything — a plan, a question, work you want
            done. It can put cards on the board and read the diffs, and every one of those calls comes
            back to you as a permission request. This conversation is kept, so it survives a restart.
          </div>
        )}

        {dated.map(({ msg, depth, divider }, i) => (
          <div key={i} style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            {divider && (
              <div style={{ flex: "none", display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text3)" }}>
                  {divider}
                </span>
                <span style={{ flex: 1, height: 1, background: "var(--line)" }} />
              </div>
            )}

            {msg.role === "user" && (
              <div
                style={{
                  flex: "none",
                  alignSelf: "flex-end",
                  maxWidth: 600,
                  borderRadius: "14px 14px 5px 14px",
                  background: "var(--surface)",
                  border: "1px solid var(--line3)",
                  overflow: "hidden",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "8px 14px",
                    borderBottom: "1px solid var(--line2)",
                  }}
                >
                  <span style={{ flex: 1, ...mono, fontSize: 10.5, fontWeight: 500, color: "var(--text4)" }}>
                    you
                  </span>
                  <CopyButton text={msg.text} />
                </div>
                <div
                  style={{
                    padding: "12px 14px",
                    ...mono,
                    fontSize: 12.5,
                    lineHeight: 1.8,
                    color: "var(--text2)",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                  }}
                >
                  {msg.text}
                </div>
              </div>
            )}

            {msg.role === "agent" && (
              <div style={{ flex: "none", display: "flex", gap: 12 }}>
                <span
                  style={{
                    width: 28,
                    height: 28,
                    flex: "none",
                    borderRadius: 8,
                    background: `linear-gradient(140deg,${t.color},${t.color})`,
                    color: "var(--onAccent)",
                    display: "grid",
                    placeItems: "center",
                    font: "700 11.5px var(--sans)",
                  }}
                >
                  {speaker?.initial ?? "D"}
                </span>
                <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 10 }}>
                  <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
                    <span style={{ font: "600 12.5px var(--sans)", color: "var(--text)" }}>
                      {speaker?.name ?? "Director"}
                    </span>
                    <span style={{ ...mono, fontSize: 10.5, color: "var(--text3)" }}>
                      {clock(msg.ts)}
                      {speaker?.model ? ` · ${speaker.model}` : ""}
                    </span>
                  </div>
                  <div
                    style={{
                      maxWidth: 660,
                      font: "400 14px/1.72 var(--sans)",
                      color: "var(--text1)",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      textWrap: "pretty",
                    }}
                  >
                    {msg.text}
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 14, color: "var(--text4)" }}>
                    <CopyButton text={msg.text} />
                    {conversation && (
                      <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
                        this chat {money(conversation.cost_usd, 2)}
                      </span>
                    )}
                  </div>
                </div>
              </div>
            )}

            {msg.role === "tool" && (
              <ToolBubble msg={msg} depth={depth} />
            )}

            {msg.role === "notice" && (
              <div
                style={{
                  flex: "none",
                  alignSelf: "stretch",
                  padding: "10px 12px",
                  borderRadius: 12,
                  background: "var(--surface)",
                  border: "1px solid var(--line2)",
                  font: "400 11.5px/1.6 var(--sans)",
                  color: "var(--text3)",
                  whiteSpace: "pre-wrap",
                }}
              >
                {msg.text}
              </div>
            )}
          </div>
        ))}

        {chatBusy && (
          <div style={{ flex: "none", display: "flex", gap: 12, alignItems: "center" }}>
            <span
              onClick={stopTurn}
              title="stop this turn"
              style={{
                marginLeft: "auto",
                padding: "4px 10px",
                borderRadius: 8,
                border: "1px solid var(--line3)",
                ...mono,
                fontSize: 10.5,
                color: "var(--text3)",
                cursor: "pointer",
              }}
            >
              ■ stop
            </span>
            <span
              style={{
                width: 28,
                height: 28,
                flex: "none",
                borderRadius: 8,
                background: t.color,
                color: "var(--onAccent)",
                display: "grid",
                placeItems: "center",
                font: "700 11.5px var(--sans)",
              }}
            >
              {speaker?.initial ?? "D"}
            </span>
            <div style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 10 }}>
              <Spinner size={14} />
              <span
                style={{
                  font: "400 12.5px/1.6 var(--sans)",
                  color: "var(--text3)",
                  fontStyle: chatThinking ? "italic" : "normal",
                  maxHeight: 60,
                  overflow: "hidden",
                }}
              >
                {chatThinking || "thinking…"}
              </span>
              <Caret />
            </div>
          </div>
        )}

        {running.map((c) => (
          <RunPanel key={c.id} cardId={c.id} />
        ))}

        {approvals.length > 0 && (
          <ApprovalCard request={approvals[0]!} more={approvals.length - 1} />
        )}

        <div ref={end} />
      </div>

      <div
        style={{
          flex: "none",
          padding: "10px 20px 16px",
          animation: "paneIn .42s cubic-bezier(.2,.8,.25,1) .06s both",
        }}
      >
        <div
          style={{
            position: "relative",
            borderRadius: 16,
            background: "var(--surface)",
            border: "1px solid var(--line3)",
          }}
        >
          {attached.length > 0 && (
            <div
              style={{
                display: "flex",
                flexWrap: "wrap",
                gap: 6,
                padding: "10px 12px 0",
              }}
            >
              {attached.map((file) => (
                <span
                  key={file}
                  title={file}
                  onClick={() => setAttached((was) => was.filter((f) => f !== file))}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 6,
                    maxWidth: 260,
                    padding: "4px 10px",
                    borderRadius: 8,
                    background: "var(--surface2)",
                    border: "1px solid var(--line3)",
                    ...mono,
                    fontSize: 10.5,
                    color: "var(--text2)",
                    cursor: "pointer",
                  }}
                >
                  <Icon.clip />
                  <span style={{ ...truncate }}>{baseName(file)}</span>
                  <span style={{ color: "var(--text4)" }}>×</span>
                </span>
              ))}
              <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)", alignSelf: "center" }}>
                read from disk, not uploaded
              </span>
            </div>
          )}

          <textarea
            ref={field}
            rows={2}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder={`Ask ${speaker?.name ?? "the Director"}…`}
            style={{
              width: "100%",
              resize: "none",
              border: "none",
              outline: "none",
              background: "transparent",
              padding: "14px 16px 6px",
              font: "400 12.5px/1.6 var(--sans)",
              color: "var(--text)",
            }}
          />
          <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 10px 12px" }}>
            <span
              title="Attach files to this message"
              onClick={attach}
              style={{
                display: "grid",
                placeItems: "center",
                width: 26,
                height: 26,
                borderRadius: 8,
                background: attached.length ? tone("accent").soft : "var(--surface2)",
                color: attached.length ? "var(--accent)" : "var(--text2)",
                cursor: "pointer",
              }}
            >
              <Icon.clip />
            </span>

            <span style={{ position: "relative" }}>
              <span
                className="chip"
                onClick={() => setPickProfile((v) => !v)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "6px 12px",
                  borderRadius: 8,
                  background: "var(--surface2)",
                  font: "500 11.5px var(--sans)",
                  color: "var(--text2)",
                  cursor: "pointer",
                }}
              >
                <span style={{ width: 14, height: 14, borderRadius: 4, background: t.color }} />
                {speaker?.name ?? "Director"} ▾
              </span>
              {pickProfile && (
                <div
                  style={{
                    position: "absolute",
                    bottom: "calc(100% + 6px)",
                    left: 0,
                    zIndex: 40,
                    minWidth: 210,
                    padding: 6,
                    borderRadius: 12,
                    background: "var(--elev)",
                    border: "1px solid var(--line3)",
                    boxShadow: "var(--shadow)",
                    animation: "fadeIn .14s ease both",
                  }}
                >
                  {agents
                    .filter((a) => a.chat_enabled && !a.paused)
                    .map((a) => {
                      const at = tone(a.tone);
                      return (
                        <div
                          key={a.id}
                          className="row"
                          onClick={() => {
                            setPickProfile(false);
                            newConversation(a.id);
                          }}
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 10,
                            padding: "8px 10px",
                            borderRadius: 8,
                            cursor: "pointer",
                          }}
                        >
                          <Glyph color={at.color} soft={at.soft} size={18} radius={6} font={8.5}>
                            {a.initial}
                          </Glyph>
                          <span style={{ flex: 1, font: "500 12.5px var(--sans)", color: "var(--text1)" }}>
                            {a.name}
                          </span>
                          <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
                            {a.model ?? "auto"}
                          </span>
                        </div>
                      );
                    })}
                  <div
                    style={{
                      padding: "6px 10px 4px",
                      font: "400 10.5px/1.5 var(--sans)",
                      color: "var(--text4)",
                    }}
                  >
                    Picking one starts a new chat: a different profile means a
                    different session.
                  </div>
                </div>
              )}
            </span>

            <span style={{ position: "relative" }}>
              <span
                className="chip"
                title="The code this chat may read"
                onClick={() => setPickProject((v) => !v)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "6px 12px",
                  borderRadius: 8,
                  background: "var(--surface2)",
                  ...mono,
                  fontSize: 11.5,
                  fontWeight: 500,
                  color: "var(--text2)",
                  cursor: conversation ? "pointer" : "default",
                  opacity: conversation ? 1 : 0.6,
                }}
              >
                <Icon.branch />
                {pinned?.name ?? "no project"} ▾
              </span>
              {pickProject && conversation && (
                <div
                  style={{
                    position: "absolute",
                    bottom: "calc(100% + 6px)",
                    left: 0,
                    zIndex: 40,
                    minWidth: 200,
                    padding: 6,
                    borderRadius: 12,
                    background: "var(--elev)",
                    border: "1px solid var(--line3)",
                    boxShadow: "var(--shadow)",
                    animation: "fadeIn .14s ease both",
                  }}
                >
                  {[{ id: "", name: "No project", glyph: "·", tone: "accent" }, ...projects].map((p) => (
                    <div
                      key={p.id || "none"}
                      className="row"
                      onClick={() => {
                        setPickProject(false);
                        pinConversation(conversation.id, p.id || null);
                      }}
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 10,
                        padding: "8px 10px",
                        borderRadius: 8,
                        cursor: "pointer",
                      }}
                    >
                      <Glyph
                        color={tone(p.tone).color}
                        soft={tone(p.tone).soft}
                        size={18}
                        radius={6}
                        font={8.5}
                      >
                        {p.glyph}
                      </Glyph>
                      <span style={{ flex: 1, font: "500 12.5px var(--sans)", color: "var(--text1)" }}>
                        {p.name}
                      </span>
                      {conversation.project_id === (p.id || null) && (
                        <span style={{ color: "var(--accent)", fontSize: 11.5 }}>✓</span>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </span>

            {!conversation && project && (
              <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
                a new chat is pinned to {project.name}
              </span>
            )}

            <div style={{ flex: 1 }} />
            <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>
              ⏎ send · ⇧⏎ newline
            </span>
            <span
              className="primary"
              onClick={send}
              style={{
                display: "grid",
                placeItems: "center",
                width: 28,
                height: 28,
                borderRadius: 8,
                background: "var(--accent)",
                color: "var(--onAccent)",
                cursor: "pointer",
                opacity: text.trim() && !chatBusy ? 1 : 0.5,
              }}
            >
              <Icon.send />
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
