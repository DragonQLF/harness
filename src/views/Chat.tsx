/** The chat screen: one conversation, the permission requests it produced, and
 *  the field you answer in. The Director is the default voice, but any profile
 *  that can hold a conversation gets the same screen. */

import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { clock, money, plural } from "../lib/format";
import { cx } from "../lib/cx";
import { paneInDelayed, popover, rowIn, sheetIn } from "../lib/motion";
import { ruleLabel, tone, type AllowRule, type PendingApproval } from "../lib/types";
import { useStore, type ChatMsg } from "../state/store";
import { api, reason } from "../lib/ipc";
import { Caret, Glyph, Icon, Spinner, mono, truncate } from "../components/ui";

/** Uma pastilha assente na superfície de trás. */
const CHIP =
  "min-h-6 cursor-pointer rounded-sm border-none bg-surface2 text-text2 transition-[border-color,background,color] duration-150 hover:bg-hovered hover:text-text dark:bg-surface2-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

/** O painel que sai de uma pastilha para cima. */
const POPOVER =
  "absolute bottom-[calc(100%+6px)] left-0 z-40 rounded-md border border-line3 bg-elev p-1.5 shadow-soft dark:border-line3-d dark:bg-elev-d dark:shadow-soft-d";

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
    <button
      type="button"
      aria-label={done ? "Copied" : "Copy this message"}
      title={done ? "Copied" : "Copy"}
      onClick={() => {
        navigator.clipboard
          .writeText(text)
          .then(() => setDone(true))
          .catch(() => {});
        window.setTimeout(() => setDone(false), 1400);
      }}
      className={cx(
        "grid h-6 w-6 cursor-pointer place-items-center rounded-sm border-none bg-transparent transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
        done ? "text-ok dark:text-ok-d" : "text-text3 hover:text-text dark:text-text3-d dark:hover:text-text-d",
      )}
    >
      {done ? <Icon.check /> : <Icon.copy />}
    </button>
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
    <motion.div
      variants={sheetIn}
      initial="hidden"
      animate="shown"
      exit="gone"
      className="flex flex-col gap-2.5 rounded-md border border-warn bg-surface px-4 py-3.5 dark:border-warn-d dark:bg-surface-d"
    >
      <div className="flex items-center gap-2.5">
        <Glyph tone={t} size={26} radius="50%" font={10}>
          {agent?.initial ?? "?"}
        </Glyph>
        <span className="min-w-0 flex-1">
          <span className="block text-md font-semibold text-text dark:text-text-d">
            {agent?.name ?? "An agent"} is asking permission
          </span>
          <span className={cx(mono, "mt-0.5 block text-xs text-text3 dark:text-text3-d")}>
            {request.card_id ?? "no card"} · the run is paused until you answer
          </span>
        </span>
        <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
          {clock(request.asked_ms)}
        </span>
      </div>

      <span
        className={cx(
          mono,
          "self-start rounded-sm bg-warnSoft px-2.5 py-1.5 text-sm font-medium text-warn dark:bg-warnSoft-d dark:text-warn-d",
        )}
      >
        {request.tool}
      </span>

      <span className="break-words rounded-md bg-bg px-3.5 py-3 text-md font-normal leading-[1.65] text-text2 dark:bg-bg-d dark:text-text2-d">
        {card?.title ?? "It asked for a tool outside its permissions."}
        <span
          className={cx(
            mono,
            "mt-1.5 block text-sm font-medium text-warn dark:text-warn-d",
          )}
        >
          {request.summary || "no details given"}
        </span>
      </span>

      <button
        type="button"
        role="checkbox"
        aria-checked={always}
        disabled={!canScope}
        onClick={() => setAlways((v) => !v)}
        className={cx(
          "flex cursor-pointer items-start gap-2.5 rounded-sm border-none bg-transparent text-left transition-colors duration-150 disabled:cursor-default disabled:opacity-60",
        )}
      >
        <span
          className={cx(
            "mt-px grid h-4.5 w-4.5 flex-none place-items-center rounded-sm border transition-[background,border-color] duration-150",
            always
              ? "border-accent bg-accent dark:border-accent-d dark:bg-accent-d"
              : "border-line3 bg-transparent dark:border-line3-d",
          )}
        >
          <span
            className={cx(
              "h-2 w-2 rounded-2px bg-onAccent transition-transform duration-200 ease-rise dark:bg-onAccent-d",
              always ? "scale-100" : "scale-0",
            )}
          />
        </span>
        <span className="flex-1">
          <span className="block text-md font-normal text-text2 dark:text-text2-d">
            Always allow{" "}
            <span className={cx(mono, "text-sm text-text2 dark:text-text2-d")}>
              {ruleLabel(rule)}
            </span>
          </span>
          <span className="mt-0.5 block text-xs font-normal leading-normal text-text4 dark:text-text4-d">
            {canScope
              ? "Scoped to that command. A bare shell rule authorises nothing, so Relay will not record one."
              : "This command chains others, so it cannot be narrowed into a rule. It is allowed once."}
          </span>
        </span>
      </button>

      <span className="flex items-center gap-2.5">
        <button
          type="button"
          onClick={() => answerApproval(request.request_id, true, always && canScope)}
          className="min-h-6 flex-1 cursor-pointer rounded-full border-none bg-accent p-2.5 text-center text-md font-semibold text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px dark:bg-accent-d dark:text-onAccent-d"
        >
          Allow
        </button>
        <button
          type="button"
          onClick={() => answerApproval(request.request_id, false, false)}
          className="min-h-6 flex-1 cursor-pointer rounded-full border border-line3 bg-transparent p-2.5 text-center text-md font-medium text-text2 transition-colors duration-150 hover:border-line4 hover:text-text dark:border-line3-d dark:text-text2-d dark:hover:border-line4-d dark:hover:text-text-d"
        >
          Deny
        </button>
      </span>
      {more > 0 && (
        <span className="text-center text-sm font-normal text-text3 dark:text-text3-d">
          {more} more waiting after this one
        </span>
      )}
    </motion.div>
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
    <div className="flex-none overflow-hidden rounded-md border border-line2 bg-elev dark:border-line2-d dark:bg-elev-d">
      <div className="flex items-center gap-2.5 border-b border-line2 bg-elev px-3 py-2 dark:border-line2-d dark:bg-elev-d">
        <Glyph tone={t} size={16} font={8}>
          {agent?.initial ?? "?"}
        </Glyph>
        <span className={cx(truncate, "flex-1 text-sm font-medium text-text2 dark:text-text2-d")}>
          {agent?.name ?? card.agent_id} · {card.id} · live
        </span>
        <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
          {plural(card.turns, "turn")} · {money(card.cost_usd, 2)}
          {session ? ` · ${session.branch ?? "no branch"}` : ""}
        </span>
      </div>
      <div className={cx(mono, "px-3 py-2 text-sm leading-[1.9] text-text3 dark:text-text3-d")}>
        {log.map((l, i) => (
          <div key={i}>
            <span className={l.labelColor}>{l.label}</span>{" "}
            <span className="text-text3 dark:text-text3-d">{l.text}</span>
          </div>
        ))}
        {(stream?.text || stream?.thinking) && (
          <div
            className={cx(
              "text-text2 dark:text-text2-d",
              stream.text ? "not-italic" : "italic",
            )}
          >
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
  const accent = !isResult
    ? "text-info dark:text-info-d"
    : msg.ok
      ? "text-ok dark:text-ok-d"
      : "text-bad dark:text-bad-d";
  const head = (
    <>
      <b className={cx(accent, "font-semibold")}>
        {isResult ? (msg.ok ? "↳ ok" : "↳ failed") : "tool"}
      </b>
      <span
        title={msg.tool}
        className={cx(truncate, "flex-1 text-text3 dark:text-text3-d")}
      >
        {msg.tool ? `${msg.tool} · ` : ""}
        {msg.text}
      </span>
      {msg.detail && (
        <span className="flex-none text-text4 dark:text-text4-d">{open ? "▾" : "▸"}</span>
      )}
    </>
  );
  const headSkin = cx(mono, "flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-sm");
  return (
    <div className="flex items-start gap-3" style={{ paddingLeft: depth * 16 }}>
      <span className="w-7 flex-none" />
      <div
        className={cx(
          "min-w-0 flex-1 overflow-hidden rounded-sm border bg-surface dark:bg-surface-d",
          isResult && msg.ok === false
            ? "border-bad dark:border-bad-d"
            : "border-line2 dark:border-line2-d",
        )}
      >
        {msg.detail ? (
          <button
            type="button"
            aria-expanded={open}
            onClick={() => setOpen((o) => !o)}
            className={cx(
              headSkin,
              "cursor-pointer bg-transparent transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d",
            )}
          >
            {head}
          </button>
        ) : (
          <div className={headSkin}>{head}</div>
        )}
        {open && msg.detail && (
          <div className="max-h-[260px] overflow-y-auto whitespace-pre-wrap break-words border-t border-line px-2.5 py-2 text-sm leading-[1.7] text-text3 dark:border-line-d dark:text-text3-d">
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
    refreshProjects,
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
      toast("bad", "Could not open the file picker", reason(e));
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
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {conversation?.resume_failed && (
        <div className="flex-none border-b border-line bg-badSoft px-5 py-2.5 text-sm font-normal leading-[1.55] text-bad2 dark:border-line-d dark:bg-badSoft-d dark:text-bad2-d">
          The Claude session behind this conversation could not be resumed. Everything below is still
          readable; your next message starts a new session.
        </div>
      )}

      {/* A conversa chega mensagem a mensagem — o `.stagger` do desenho.
          Uma conversa curta encosta ao compositor em vez de ficar perdida no
          topo do painel com um ecrã de nada por baixo; assim que a transcrição
          é comprida ao ponto de rolar, isto deixa de ter efeito. */}
      <motion.div
        initial="hidden"
        animate="shown"
        className="flex min-h-0 flex-1 flex-col justify-end gap-4 overflow-y-auto px-5 pb-2.5 pt-4.5"
      >
        {chat.length === 0 && !chatBusy && (
          <div className="max-w-[620px] text-lg font-normal leading-[1.75] text-text3 dark:text-text3-d">
            Ask {speaker?.name ?? "the Director"} about anything — a plan, a question, work you want
            done. It can put cards on the board and read the diffs, and every one of those calls comes
            back to you as a permission request. This conversation is kept, so it survives a restart.
          </div>
        )}

        {dated.map(({ msg, depth, divider }, i) => (
          <motion.div key={i} custom={i} variants={rowIn} className="flex flex-col gap-4">
            {divider && (
              <div className="flex flex-none items-center gap-2.5">
                <span className={cx(mono, "text-xs font-medium text-text3 dark:text-text3-d")}>
                  {divider}
                </span>
                <span className="h-px flex-1 bg-line dark:bg-line-d" />
              </div>
            )}

            {msg.role === "user" && (
              <div className="max-w-[600px] flex-none self-end overflow-hidden rounded-[14px_14px_5px_14px] border border-line3 bg-surface dark:border-line3-d dark:bg-surface-d">
                <div className="flex items-center gap-2.5 border-b border-line2 px-3.5 py-2 dark:border-line2-d">
                  <span
                    className={cx(mono, "flex-1 text-xs font-medium text-text4 dark:text-text4-d")}
                  >
                    you
                  </span>
                  <CopyButton text={msg.text} />
                </div>
                <div
                  className={cx(
                    mono,
                    "whitespace-pre-wrap break-words px-3.5 py-3 text-md leading-[1.8] text-text2 dark:text-text2-d",
                  )}
                >
                  {msg.text}
                </div>
              </div>
            )}

            {msg.role === "agent" && (
              <div className="flex flex-none gap-3">
                <span
                  className={cx(
                    "grid h-7 w-7 flex-none place-items-center rounded-sm text-sm font-bold text-onAccent dark:text-onAccent-d",
                    // O desenho pedia um degradê de uma cor para si própria, o
                    // que é a cor lisa; fica a cor lisa.
                    t.solid,
                  )}
                >
                  {speaker?.initial ?? "D"}
                </span>
                <div className="flex min-w-0 flex-1 flex-col gap-2.5">
                  <div className="flex items-baseline gap-2.5">
                    <span className="text-md font-semibold text-text dark:text-text-d">
                      {speaker?.name ?? "Director"}
                    </span>
                    <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
                      {clock(msg.ts)}
                      {speaker?.model ? ` · ${speaker.model}` : ""}
                    </span>
                  </div>
                  <div className="max-w-[660px] whitespace-pre-wrap break-words text-lg font-normal leading-[1.72] text-text1 [text-wrap:pretty] dark:text-text1-d">
                    {msg.text}
                  </div>
                  <div className="flex items-center gap-3.5 text-text4 dark:text-text4-d">
                    <CopyButton text={msg.text} />
                    {conversation && (
                      <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                        this chat {money(conversation.cost_usd, 2)}
                      </span>
                    )}
                  </div>
                </div>
              </div>
            )}

            {msg.role === "tool" && <ToolBubble msg={msg} depth={depth} />}

            {msg.role === "notice" && (
              <div className="flex-none self-stretch whitespace-pre-wrap rounded-md border border-line2 bg-surface px-3 py-2.5 text-sm font-normal leading-relaxed text-text3 dark:border-line2-d dark:bg-surface-d dark:text-text3-d">
                {msg.text}
              </div>
            )}
          </motion.div>
        ))}

        {chatBusy && (
          <div className="flex flex-none items-center gap-3">
            <button
              type="button"
              onClick={stopTurn}
              title="stop this turn"
              className={cx(
                mono,
                "ml-auto min-h-6 cursor-pointer rounded-sm border border-line3 bg-transparent px-2.5 py-1 text-xs text-text3 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line3-d dark:text-text3-d dark:hover:bg-hovered-d dark:hover:text-text-d",
              )}
            >
              ■ stop
            </button>
            <span
              className={cx(
                "grid h-7 w-7 flex-none place-items-center rounded-sm text-sm font-bold text-onAccent dark:text-onAccent-d",
                t.solid,
              )}
            >
              {speaker?.initial ?? "D"}
            </span>
            <div className="flex min-w-0 flex-1 items-center gap-2.5">
              <Spinner size={14} />
              <span
                className={cx(
                  "max-h-[60px] overflow-hidden text-md font-normal leading-relaxed text-text3 dark:text-text3-d",
                  chatThinking ? "italic" : "not-italic",
                )}
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

        <AnimatePresence>
          {approvals.length > 0 && (
            <ApprovalCard
              key={approvals[0]!.request_id}
              request={approvals[0]!}
              more={approvals.length - 1}
            />
          )}
        </AnimatePresence>

        <div ref={end} />
      </motion.div>

      <motion.div
        variants={paneInDelayed}
        initial="hidden"
        animate="shown"
        className="flex-none px-5 pb-4 pt-2.5"
      >
        <div className="relative rounded-lg border border-line3 bg-surface focus-within:border-accentLine dark:border-line3-d dark:bg-surface-d dark:focus-within:border-accentLine-d">
          {attached.length > 0 && (
            <div className="flex flex-wrap gap-1.5 px-3 pt-2.5">
              {attached.map((file) => (
                <button
                  key={file}
                  type="button"
                  title={file}
                  aria-label={`Remove ${baseName(file)}`}
                  onClick={() => setAttached((was) => was.filter((f) => f !== file))}
                  className={cx(
                    mono,
                    "flex min-h-6 max-w-[260px] cursor-pointer items-center gap-1.5 rounded-sm border border-line3 bg-surface2 px-2.5 py-1 text-xs text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line3-d dark:bg-surface2-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
                  )}
                >
                  <Icon.clip />
                  <span className={truncate}>{baseName(file)}</span>
                  <span className="text-text4 dark:text-text4-d">×</span>
                </button>
              ))}
              <span
                className={cx(mono, "self-center text-xs text-text4 dark:text-text4-d")}
              >
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
            aria-label={`Ask ${speaker?.name ?? "the Director"}`}
            placeholder={`Ask ${speaker?.name ?? "the Director"}…`}
            className="w-full resize-none border-none bg-transparent px-4 pb-1.5 pt-3.5 text-md font-normal leading-relaxed text-text outline-none dark:text-text-d"
          />
          <div className="flex items-center gap-2 pb-2.5 pl-3 pr-2.5 pt-2">
            <button
              type="button"
              title="Attach files to this message"
              aria-label="Attach files to this message"
              onClick={attach}
              className={cx(
                "grid h-6.5 w-6.5 cursor-pointer place-items-center rounded-sm border-none transition-colors duration-150",
                attached.length
                  ? "bg-accentSoft text-accent dark:bg-accentSoft-d dark:text-accent-d"
                  : "bg-surface2 text-text2 hover:bg-hovered hover:text-text dark:bg-surface2-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
              )}
            >
              <Icon.clip />
            </button>

            <span className="relative">
              <button
                type="button"
                aria-expanded={pickProfile}
                onClick={() => setPickProfile((v) => !v)}
                className={cx(CHIP, "flex items-center gap-2 px-3 py-1.5 text-sm font-medium")}
              >
                <span className={cx("h-3.5 w-3.5 rounded-[4px]", t.solid)} />
                {speaker?.name ?? "Director"} ▾
              </button>
              <AnimatePresence>
                {pickProfile && (
                  <motion.div
                    variants={popover}
                    initial="hidden"
                    animate="shown"
                    exit="gone"
                    className={cx(POPOVER, "min-w-[210px]")}
                  >
                    {agents
                      .filter((a) => a.chat_enabled && !a.paused)
                      .map((a) => {
                        const at = tone(a.tone);
                        return (
                          <button
                            key={a.id}
                            type="button"
                            onClick={() => {
                              setPickProfile(false);
                              newConversation(a.id);
                            }}
                            className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
                          >
                            <Glyph tone={at} size={18} radius={6} font={8.5}>
                              {a.initial}
                            </Glyph>
                            <span className="flex-1 text-md font-medium text-text1 dark:text-text1-d">
                              {a.name}
                            </span>
                            <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                              {a.model ?? "auto"}
                            </span>
                          </button>
                        );
                      })}
                    <div className="px-2.5 pb-1 pt-1.5 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                      Picking one starts a new chat: a different profile means a
                      different session.
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </span>

            <span className="relative">
              <button
                type="button"
                title="The code this chat may read"
                aria-expanded={pickProject}
                disabled={!conversation}
                onClick={() => setPickProject((v) => !v)}
                className={cx(
                  CHIP,
                  mono,
                  "flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium disabled:cursor-default disabled:opacity-60",
                )}
              >
                <Icon.branch />
                {pinned?.name ?? "no project"} ▾
              </button>
              <AnimatePresence>
                {pickProject && conversation && (
                  <motion.div
                    variants={popover}
                    initial="hidden"
                    animate="shown"
                    exit="gone"
                    className={cx(POPOVER, "min-w-[200px]")}
                  >
                    {[{ id: "", name: "No project", glyph: "·", tone: "accent" }, ...projects].map(
                      (p) => (
                        <button
                          key={p.id || "none"}
                          type="button"
                          onClick={() => {
                            setPickProject(false);
                            pinConversation(conversation.id, p.id || null);
                          }}
                          className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
                        >
                          <Glyph tone={tone(p.tone)} size={18} radius={6} font={8.5}>
                            {p.glyph}
                          </Glyph>
                          <span className="flex-1 text-md font-medium text-text1 dark:text-text1-d">
                            {p.name}
                          </span>
                          {conversation.project_id === (p.id || null) && (
                            <span className="text-sm text-accent dark:text-accent-d">✓</span>
                          )}
                        </button>
                      ),
                    )}
                  </motion.div>
                )}
              </AnimatePresence>
            </span>

            {/* Working on the app itself is a mode, not a repository the
                operator has to know to register first. Offered once, here,
                and gone the moment it is on. */}
            {!projects.some((p) => p.mirror) && (
              <button
                type="button"
                title="Sets Relay's own source up so the app can be given cards. Finds it on this machine, or fetches it."
                onClick={async () => {
                  try {
                    await api.mirrorSetup();
                    await refreshProjects();
                    toast("ok", "Relay is set up", "The app can be given cards now.");
                  } catch (e) {
                    toast("bad", "Could not set Relay up", reason(e));
                  }
                }}
                className={cx(CHIP, mono, "px-3 py-1.5 text-sm font-medium")}
              >
                work on Relay
              </button>
            )}

            {!conversation && project && (
              <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                a new chat is pinned to {project.name}
              </span>
            )}

            <div className="flex-1" />
            <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
              ⏎ send · ⇧⏎ newline
            </span>
            <button
              type="button"
              aria-label="Send"
              onClick={send}
              disabled={(!text.trim() && attached.length === 0) || chatBusy}
              className={cx(
                "grid h-7 w-7 cursor-pointer place-items-center rounded-sm border-none bg-accent text-onAccent transition-[filter,transform] duration-150 hover:-translate-y-px hover:brightness-[1.08] active:translate-y-px disabled:cursor-default disabled:hover:translate-y-0 disabled:hover:brightness-100 dark:bg-accent-d dark:text-onAccent-d",
                text.trim() && !chatBusy ? "opacity-100" : "opacity-50",
              )}
            >
              <Icon.send />
            </button>
          </div>
        </div>
      </motion.div>
    </div>
  );
}
