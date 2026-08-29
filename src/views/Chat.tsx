/** The chat screen: one conversation, the receipts its tools left, the
 *  permission requests it produced, and the field you answer in.
 *
 *  The only screen whose pane does not scroll. The thread scrolls inside its
 *  own card so the composer stays where the hand expects it, and the rail
 *  beside it reports what the thread did rather than what it said. */

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, Plus, Square } from "lucide-react";
import { Streamdown } from "streamdown";
import { ago, bytes, money, num } from "../lib/format";
import { cx } from "../lib/cx";
import { paneIn, popover, rowIn } from "../lib/motion";
import {
  ruleLabel,
  STATUS_NAME,
  type AllowRule,
  type AttachmentPreview,
  type ConversationTotals,
  type PendingApproval,
} from "../lib/types";
import { toolName, useStore, type ChatMsg } from "../state/store";
import { api, reason } from "../lib/ipc";
import { mono } from "../components/ui";

/** A pill in the composer row and in the permission sheet. */
const PILL =
  "rounded-full border border-line bg-surface px-3 py-1 text-sm font-medium text-ink2 dark:border-line-d dark:bg-surface-d dark:text-ink2-d";

const POPOVER =
  "absolute z-40 min-w-[210px] rounded-md border border-line bg-surface p-1.5 shadow-soft dark:border-line-d dark:bg-surface-d dark:shadow-soft-d";

/** A quiet control in the header line: reads as part of the sentence, and says
 *  so by going flat when there is no conversation to act on. */
const HEAD_ACTION =
  "cursor-pointer border-none bg-transparent p-0 text-body text-faint underline-offset-2 hover:underline disabled:cursor-default disabled:no-underline disabled:opacity-70 dark:text-faint-d";

/** Why every one of these is off before the first message. A draft has no id,
 *  so there is nothing to rename, archive, delete or pin. */
const NO_THREAD_YET = "This chat does not exist yet — send a message to create it";

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

/** A chained command cannot be narrowed into a rule, so Relay will not record
 *  one — the sheet says so rather than pretending. */
function scopable(request: PendingApproval): boolean {
  const rule = scopeOf(request);
  if (!rule.command) return !["bash", "shell", "sh", "powershell"].includes(rule.tool.toLowerCase());
  const input = request.input as { command?: string } | null;
  return !/[;&|]{1,2}|\$\(/.test(input?.command ?? "");
}

/** The exact call the agent asked for. Never a paraphrase: a sheet that
 *  summarises the command is a sheet you cannot answer honestly. */
function commandOf(request: PendingApproval): string {
  const input = request.input as { command?: string } | null;
  return (input?.command ?? request.summary ?? request.tool).trim();
}

/** Backticks in an answer are code, not punctuation. */
/** Agent prose. Streamdown rather than a markdown renderer of the ordinary
 *  kind: a streamed answer is always mid-token — an unclosed fence, half a
 *  `**bold`, a table three rows in — and it completes those blocks for display
 *  so the text settles instead of flickering as the rest arrives. */
function Prose({ text }: { text: string }) {
  // Streamdown ships its own chip for inline code — dark and heavy, sized for
  // a white page rather than for this thread. The old hand-rolled Prose used
  // `bg-active`, which reads as emphasis instead of as a label, so the app's
  // own tokens win here. `:not(pre) > code` leaves fenced blocks alone: those
  // are meant to be a slab, and only the inline chips were shouting.
  return (
    <Streamdown className="[&_:not(pre)>code]:rounded-5px [&_:not(pre)>code]:bg-active [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-px [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-body [&_:not(pre)>code]:font-normal [&_:not(pre)>code]:text-ink2 [&_:not(pre)>code]:before:content-none [&_:not(pre)>code]:after:content-none dark:[&_:not(pre)>code]:bg-active-d dark:[&_:not(pre)>code]:text-ink2-d">
      {text}
    </Streamdown>
  );
}

const TICK = (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={3.2}
    strokeLinecap="round"
    strokeLinejoin="round"
    className="flex-none text-ok dark:text-ok-d"
    aria-hidden="true"
  >
    <path d="M20 6 9 17l-5-5" />
  </svg>
);

const CROSS = (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={3.2}
    strokeLinecap="round"
    strokeLinejoin="round"
    className="flex-none text-bad dark:text-bad-d"
    aria-hidden="true"
  >
    <path d="M18 6 6 18M6 6l12 12" />
  </svg>
);

const SPINNING = (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2.6}
    className="flex-none animate-spin-tool"
    aria-hidden="true"
  >
    <circle cx="12" cy="12" r="9" strokeDasharray="14 10" />
  </svg>
);

/** One tool receipt. In flight while its result has not landed, then closed by
 *  the result — green when it worked, red when it did not. The full output is
 *  one click away rather than dumped into the thread (#28). */
function Receipt({ msg }: { msg: ChatMsg }) {
  const [open, setOpen] = useState(false);
  const flying = msg.ok == null;
  // The backend's summary often opens with the tool's own name — the raw one,
  // prefixes and all — so joining it to the label printed it twice:
  // `read_docs · mcp__harness__read_docs: decisions`. Say the name once.
  const said = (msg.text ?? "").replace(/^\s*(mcp__[a-z0-9_]+?__)?[a-z0-9_.-]+\s*[:·]\s*/i, (m) =>
    toolName(m.replace(/\s*[:·]\s*$/, "")) === msg.tool ? "" : m,
  );
  const label = [msg.tool, said.trim()].filter(Boolean).join(" · ") || "tool";
  const skin = cx(
    "flex max-w-full items-center gap-1.75 rounded-9px border px-2.5 py-1.5 font-mono text-11 font-medium",
    flying
      ? "border-primaryLine bg-primarySoft text-primary dark:border-primaryLine-d dark:bg-primarySoft-d dark:text-primary-d"
      : "border-line bg-surface text-ink2 dark:border-line-d dark:bg-surface-d dark:text-ink2-d",
  );

  const face = (
    <>
      {flying ? SPINNING : msg.ok ? TICK : CROSS}
      <span className="min-w-0 truncate">{label}</span>
    </>
  );

  if (!msg.detail) {
    return (
      <span className={skin} title={label}>
        {face}
      </span>
    );
  }
  return (
    <>
      <button
        type="button"
        aria-expanded={open}
        title={label}
        onClick={() => setOpen((o) => !o)}
        className={cx(skin, "cursor-pointer text-left")}
      >
        {face}
      </button>
      {open && (
        <pre className="max-h-[240px] w-full overflow-auto whitespace-pre-wrap break-words rounded-sm border border-line2 bg-active px-2.5 py-2 font-mono text-sm leading-[1.7] text-muted dark:border-line2-d dark:bg-active-d dark:text-muted-d">
          {msg.detail}
        </pre>
      )}
    </>
  );
}

/** The permission sheet, in the thread where the request was made. The command
 *  is quoted exactly and the three answers are `respond_approval`; "Always
 *  allow" is the `always` flag, which is what writes the standing rule. */
function PermissionSheet({ request }: { request: PendingApproval }) {
  const { agents, snapshot, answerApproval } = useStore();
  const card = snapshot?.cards.find((c) => c.id === request.card_id);
  const agent = agents.find((a) => a.id === card?.agent_id);
  const rule = scopeOf(request);
  const canScope = scopable(request);

  return (
    <div className="flex max-w-[82%] flex-col gap-2.5 rounded-lg border border-warnLine bg-warnSheet px-4.25 py-3.75 dark:border-warnLine-d dark:bg-warnSheet-d">
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="grid h-[17px] w-[17px] flex-none place-items-center rounded-full border-[1.5px] border-warn text-xs font-bold text-warn dark:border-warn-d dark:text-warn-d"
        >
          !
        </span>
        <span className="text-body font-bold text-warn dark:text-warn-d">
          {agent?.name ?? "An agent"} wants permission
        </span>
        <span className={cx(mono, "ml-auto text-xs text-warn/65 dark:text-warn-d/65")}>
          {ago(request.asked_ms)}
        </span>
      </div>

      <div className="break-all rounded-sm border border-warnLine bg-surface px-2.75 py-2.25 font-mono text-md font-medium text-ink dark:border-warnLine-d dark:bg-surface-d dark:text-ink-d">
        {commandOf(request)}
      </div>

      <div className="text-body leading-[1.55] text-muted dark:text-muted-d">
        {card ? card.title : "No card behind this call."} · the run is paused until you answer ·{" "}
        <span className={mono}>{request.card_id ?? request.tool}</span>
      </div>

      <div className="flex flex-wrap gap-1.75">
        <button
          type="button"
          onClick={() => answerApproval(request.request_id, true, false)}
          className="cursor-pointer rounded-full border-none bg-warn px-4 py-1.75 text-sm font-bold text-white dark:bg-warn-d dark:text-canvas-d"
        >
          Allow once
        </button>
        <button
          type="button"
          disabled={!canScope}
          title={
            canScope
              ? "Writes a standing allowance into settings"
              : "This command chains others, so it cannot be narrowed into a rule"
          }
          onClick={() => answerApproval(request.request_id, true, true)}
          className={cx(PILL, "cursor-pointer disabled:cursor-default disabled:opacity-60")}
        >
          Always allow {ruleLabel(rule)}
        </button>
        <button
          type="button"
          onClick={() => answerApproval(request.request_id, false, false)}
          className={cx(PILL, "cursor-pointer text-bad dark:text-bad-d")}
        >
          Deny
        </button>
      </div>
    </div>
  );
}

/** The cards this thread put on the board, found by the ids its tool calls
 *  named and read back from the snapshot — the board is the record, not the
 *  transcript. */
const CARD_ID = /\bc_[0-9a-z]+/gi;

function Touched() {
  const { chat, snapshot, agents } = useStore();
  const cards = useMemo(() => {
    const ids = new Set<string>();
    for (const m of chat) {
      if (m.role !== "tool") continue;
      const hay = `${m.tool ?? ""} ${m.text} ${m.detail ?? ""}`;
      for (const hit of hay.matchAll(CARD_ID)) ids.add(hit[0]);
    }
    return (snapshot?.cards ?? []).filter((c) => ids.has(c.id));
  }, [chat, snapshot]);

  return (
    <div className="flex-none rounded-lg border border-line bg-surface px-4 py-3.75 dark:border-line-d dark:bg-surface-d">
      <div className="text-md font-bold text-ink dark:text-ink-d">What it touched</div>
      {cards.length === 0 ? (
        <div className="mt-3 text-body leading-[1.55] text-muted dark:text-muted-d">
          Nothing on the board came out of this thread yet. Ask for work and the cards it opens
          appear here.
        </div>
      ) : (
        cards.map((card, i) => (
          <div
            key={card.id}
            className={cx(
              i === 0
                ? "mt-3"
                : "mt-2.75 border-t border-line2 pt-2.75 dark:border-line2-d",
            )}
          >
            <div className="text-body font-semibold text-ink dark:text-ink-d">{card.title}</div>
            <div className={cx(mono, "mt-0.5 text-xs text-faint dark:text-faint-d")}>
              {card.id} · {agents.find((a) => a.id === card.agent_id)?.name ?? card.agent_id} ·{" "}
              {STATUS_NAME[card.status]}
            </div>
          </div>
        ))
      )}
    </div>
  );
}

function Total({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex justify-between gap-3 text-body text-muted dark:text-muted-d">
      {label}
      <span className={cx(mono, "text-ink dark:text-ink-d")}>{value}</span>
    </div>
  );
}

/** What this thread cost, counted by the backend over the whole transcript.
 *  An em-dash where the answer is genuinely missing — a thread recorded before
 *  usage was logged has no token count, and inventing one is worse than the
 *  dash. Re-read when the turn ends, which is when the numbers moved. */
function ThreadTotals() {
  const { conversation, chatBusy } = useStore();
  const conversationId = conversation?.id ?? null;
  const [totals, setTotals] = useState<ConversationTotals | null>(null);

  useEffect(() => {
    if (!conversationId) {
      setTotals(null);
      return;
    }
    let current = true;
    api
      .conversationTotals(conversationId)
      .then((t) => {
        if (current) setTotals(t);
      })
      .catch(() => {
        if (current) setTotals(null);
      });
    return () => {
      current = false;
    };
  }, [conversationId, chatBusy]);

  return (
    <div className="flex-none rounded-lg border border-line bg-surface px-4 py-3.75 dark:border-line-d dark:bg-surface-d">
      <div className="text-md font-bold text-ink dark:text-ink-d">This thread</div>
      <div className="mt-3 flex flex-col gap-2.25">
        <Total label="Tokens" value={totals?.tokens != null ? num(totals.tokens) : "—"} />
        <Total label="Spend" value={totals ? money(totals.spend_usd) : "—"} />
        <Total label="Tool calls" value={totals ? num(totals.tool_calls) : "—"} />
        <Total
          label="Context"
          value={totals?.context_pct != null ? `${totals.context_pct.toFixed(1)}%` : "—"}
        />
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

/** A turn, once the flat transcript is folded: what was said, and the receipts
 *  the same turn left behind. */
interface Block {
  kind: "user" | "agent" | "notice";
  msg: ChatMsg | null;
  tools: ChatMsg[];
}

/** The chip standing for one attachment.
 *
 *  A path is what the agent needs and the worst thing to show a person: the
 *  operator pasted a picture, and `pasted-1724930400000.png` is a worse answer
 *  than the picture. So the chip asks the backend what the file looks like and
 *  draws that — a thumbnail for an image, the opening line for text, and the
 *  name with its size for everything else, which is all there honestly is. */
function Attached({ path, remove }: { path: string; remove: () => void }) {
  const [look, setLook] = useState<AttachmentPreview | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .attachmentPreview(path)
      .then((p) => alive && setLook(p))
      // A preview that cannot be read is not worth a toast: the chip falls
      // back to the name, which is what it would have shown anyway.
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [path]);

  const name = look?.name ?? baseName(path);

  return (
    <button
      type="button"
      title={path}
      aria-label={`Remove ${name}`}
      onClick={remove}
      className="group flex max-w-[260px] cursor-pointer items-center gap-2 rounded-9px border border-line bg-surface py-1 pl-1 pr-2.5 text-left dark:border-line-d dark:bg-surface-d"
    >
      {look?.image ? (
        <img
          src={look.image}
          alt=""
          className="h-7 w-7 flex-none rounded-6px border border-line object-cover dark:border-line-d"
        />
      ) : (
        <span className="grid h-7 w-7 flex-none place-items-center rounded-6px bg-active font-mono text-2xs font-semibold uppercase text-muted dark:bg-active-d dark:text-muted-d">
          {look?.ext || "file"}
        </span>
      )}
      <span className="min-w-0">
        <span className={cx(mono, "block truncate text-11 text-ink2 dark:text-ink2-d")}>
          {name}
        </span>
        {look && (
          <span className="block truncate text-2xs text-faint dark:text-faint-d">
            {look.head ? look.head.replace(/\s+/g, " ") : bytes(look.size)}
          </span>
        )}
      </span>
      <span className="flex-none text-faint transition-colors duration-150 group-hover:text-bad dark:text-faint-d dark:group-hover:text-bad-d">
        ×
      </span>
    </button>
  );
}

export function Chat() {
  const {
    chat,
    chatBusy,
    chatLoading,
    chatThinking,
    sendChat,
    agents,
    project,
    projects,
    conversation,
    conversationId,
    draftProfile,
    approvals,
    newConversation,
    pinConversation,
    renameConversation,
    archiveConversation,
    deleteConversation,
    toast,
  } = useStore();

  const [text, setText] = useState("");
  const [attached, setAttached] = useState<string[]>([]);
  const [taking, setTaking] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [pickProfile, setPickProfile] = useState(false);
  const [pickProject, setPickProject] = useState(false);
  /** The title being typed, or `null` when nobody is renaming. */
  const [renaming, setRenaming] = useState<string | null>(null);
  const thread = useRef<HTMLDivElement | null>(null);
  /** Whether the reader is at the newest message. Only then does a new one
   *  pull the view down — reading back is not interrupted. */
  const stuck = useRef(true);

  /** No id means a draft: nothing has been written for this thread, so every
   *  control that needs one is off. The id, not the row, is what says so —
   *  the row lands a moment later, when the list comes back from the backend. */
  const draft = conversationId == null;
  const speaker = agents.find(
    (a) => a.id === (conversation?.profile_id ?? draftProfile ?? "director"),
  );
  const pinned = projects.find((p) => p.id === conversation?.project_id);

  // A draft leaves the rename field open onto a thread that is not the same
  // one any more; close it when the thread under it changes.
  useEffect(() => setRenaming(null), [conversationId]);

  const blocks = useMemo(() => {
    const out: Block[] = [];
    for (const m of chat) {
      if (m.role === "tool") {
        const last = out[out.length - 1];
        if (last?.kind === "agent") last.tools.push(m);
        else out.push({ kind: "agent", msg: null, tools: [m] });
        continue;
      }
      out.push({ kind: m.role, msg: m, tools: [] });
    }
    return out;
  }, [chat]);

  const last = blocks[blocks.length - 1];
  const streaming = chatBusy && last?.kind === "agent" && !!last.msg?.text;
  const lastAsked = [...chat].reverse().find((m) => m.role === "user")?.text ?? "";

  useEffect(() => {
    const el = thread.current;
    if (el && stuck.current) el.scrollTop = el.scrollHeight;
  }, [chat, chatBusy, chatThinking]);

  /** The composer stays live for the whole turn. `sendChat` decides what that
   *  means — a message typed while the agent is working joins the run instead
   *  of starting a second one — so there is nothing to refuse here. */
  const send = (body: string = text) => {
    if (!body.trim() && attached.length === 0) return;
    sendChat(body, attached);
    setText("");
    setAttached([]);
  };

  const stop = () => {
    if (!conversationId) return;
    api.chatStop(conversationId).catch((e) => toast("bad", "Could not stop the turn", reason(e)));
  };

  const rename = () => {
    const title = (renaming ?? "").trim();
    setRenaming(null);
    // An empty title is a cancel, not a rename to nothing.
    if (conversationId && title && title !== conversation?.title) {
      renameConversation(conversationId, title);
    }
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

  /** A screenshot in the clipboard has bytes and a type, and no path at all —
   *  and a path is the only thing an attachment can be, because the agent is
   *  told to read the file with its own tools. So the bytes are written to
   *  disk first and the path is what gets attached. Same for a file dragged
   *  onto the composer: the browser hands over a `File`, never a location. */
  const take = useCallback(
    async (files: File[]) => {
      if (files.length === 0) return;
      setTaking(true);
      try {
        const saved = await Promise.all(
          files.map(async (file) => {
            const buf = await file.arrayBuffer();
            // Chunked so a large image does not blow the argument limit of
            // String.fromCharCode with one spread of a few million bytes.
            const bytes = new Uint8Array(buf);
            let binary = "";
            for (let i = 0; i < bytes.length; i += 0x8000) {
              binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
            }
            return api.saveAttachment(file.name || null, file.type, btoa(binary));
          }),
        );
        setAttached((was) => [...new Set([...was, ...saved])]);
      } catch (e) {
        // The backend refuses by name — too big, empty, a type it has no name
        // for — and that sentence is the useful one, not a paraphrase.
        toast("bad", "Could not attach that", reason(e));
      } finally {
        setTaking(false);
      }
    },
    [toast],
  );

  const onPaste = useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = Array.from(e.clipboardData.files);
      // Text pasted alongside an image is still text: only swallow the event
      // when the clipboard carried files and nothing typed would be lost.
      if (files.length === 0) return;
      e.preventDefault();
      void take(files);
    },
    [take],
  );

  const facts = [speaker?.model, speaker?.title].filter(Boolean).join(" · ");

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="flex min-h-0 flex-1 overflow-hidden px-5.5 py-5"
    >
      <div className="flex h-full min-w-[860px] flex-1 flex-row items-stretch gap-4">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-3.5">
          <div className="flex-none">
            <div className="text-title font-bold text-ink dark:text-ink-d">
              {speaker?.name ?? "Director"}
            </div>
            <div className="relative mt-0.75 flex flex-wrap items-center gap-x-1.5 text-body text-faint dark:text-faint-d">
              {facts && <span>{facts} ·</span>}
              <button
                type="button"
                aria-expanded={pickProject}
                disabled={draft}
                title={
                  draft
                    ? "A draft is pinned to the project on screen when you send the first message"
                    : "The code this chat may read"
                }
                onClick={() => setPickProject((v) => !v)}
                className={HEAD_ACTION}
              >
                {draft ? "will run inside " : "runs inside "}
                <span className={mono}>
                  {(draft ? project?.name : pinned?.name) ?? "no project"}
                </span>
              </button>
              <span aria-hidden="true">·</span>
              {/* The native session, which is the one fact a draft genuinely
                  does not have — so it says that instead of showing a dash. */}
              <span
                className={cx(mono, "text-xs text-faint dark:text-faint-d")}
                title={
                  draft
                    ? "Nothing is on disk yet: the chat and its Claude session are created by your first message"
                    : (conversation?.session_id ??
                      "The Claude session starts with the first answer")
                }
              >
                {draft
                  ? "draft · nothing saved yet"
                  : conversation?.session_id
                    ? `session ${conversation.session_id.slice(0, 12)}`
                    : "no session yet"}
              </span>
              <span aria-hidden="true">·</span>
              <button
                type="button"
                disabled={draft}
                title={draft ? NO_THREAD_YET : "Rename this chat"}
                onClick={() => setRenaming(conversation?.title ?? "")}
                className={HEAD_ACTION}
              >
                Rename
              </button>
              <button
                type="button"
                disabled={draft}
                title={draft ? NO_THREAD_YET : "Take it off the list; the transcript is kept"}
                onClick={() =>
                  conversationId && archiveConversation(conversationId, !conversation?.archived)
                }
                className={HEAD_ACTION}
              >
                {conversation?.archived ? "Restore" : "Archive"}
              </button>
              <button
                type="button"
                disabled={draft}
                title={draft ? NO_THREAD_YET : "Delete this chat and its transcript"}
                onClick={() => conversationId && deleteConversation(conversationId)}
                className={cx(HEAD_ACTION, "text-bad dark:text-bad-d")}
              >
                Delete
              </button>
              <AnimatePresence>
                {pickProject && conversationId && (
                  <motion.div
                    variants={popover}
                    initial="hidden"
                    animate="shown"
                    exit="gone"
                    className={cx(POPOVER, "left-0 top-[calc(100%+6px)]")}
                  >
                    {[{ id: "", name: "No project" }, ...projects].map((p) => (
                      <button
                        key={p.id || "none"}
                        type="button"
                        onClick={() => {
                          setPickProject(false);
                          pinConversation(conversationId, p.id || null);
                        }}
                        className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left text-md font-medium text-ink2 transition-colors duration-150 hover:bg-hovered dark:text-ink2-d dark:hover:bg-hovered-d"
                      >
                        <span className="min-w-0 flex-1 truncate">{p.name}</span>
                        {conversation?.project_id === (p.id || null) && (
                          <span className="text-sm text-primary dark:text-primary-d">✓</span>
                        )}
                      </button>
                    ))}
                  </motion.div>
                )}
              </AnimatePresence>
            </div>

            {renaming != null && !draft && (
              <div className="mt-2 flex items-center gap-1.75">
                <input
                  autoFocus
                  value={renaming}
                  aria-label="Rename this chat"
                  placeholder="What is this chat about?"
                  onChange={(e) => setRenaming(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") rename();
                    if (e.key === "Escape") setRenaming(null);
                  }}
                  className="min-w-0 flex-1 rounded-9px border border-line bg-surface px-2.5 py-1.5 text-md text-ink outline-none placeholder:text-faint dark:border-line-d dark:bg-surface-d dark:text-ink-d dark:placeholder:text-faint-d"
                />
                <button type="button" onClick={rename} className={cx(PILL, "cursor-pointer")}>
                  Save
                </button>
                <button
                  type="button"
                  onClick={() => setRenaming(null)}
                  className={cx(PILL, "cursor-pointer")}
                >
                  Cancel
                </button>
              </div>
            )}
          </div>

          <motion.div
            ref={thread}
            initial="hidden"
            animate="shown"
            onScroll={() => {
              const el = thread.current;
              if (el) stuck.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
            }}
            className="flex min-h-0 flex-1 flex-col gap-5 overflow-auto rounded-lg border border-line bg-surface px-5 py-4.5 dark:border-line-d dark:bg-surface-d"
          >
            {conversation?.resume_failed && (
              <div className="flex-none rounded-md border border-warnLine bg-warnSheet px-3 py-2.25 text-body leading-[1.55] text-warn dark:border-warnLine-d dark:bg-warnSheet-d dark:text-warn-d">
                The Claude session behind this conversation could not be resumed. Everything below is
                still readable; your next message starts a new session.
              </div>
            )}

            {/* A thread still being read off disk and a thread with nothing in
                it are different screens: the skeleton holds three bubbles at
                the shape they will take, so nothing jumps when they land. */}
            {blocks.length === 0 && chatLoading && (
              <>
                <div className="h-11 w-[46%] flex-none animate-pulse self-end rounded-[16px_16px_4px_16px] bg-active dark:bg-active-d" />
                <div className="h-16 w-[72%] flex-none animate-pulse rounded-md bg-active dark:bg-active-d" />
                <div className="h-11 w-[58%] flex-none animate-pulse rounded-md bg-active dark:bg-active-d" />
              </>
            )}

            {blocks.length === 0 && !chatBusy && !chatLoading && (
              <div className="max-w-[82%] text-base leading-[1.65] text-muted dark:text-muted-d">
                {draft
                  ? "Nothing here yet, and nothing written down: this chat is created when you send the first message."
                  : "Nothing said here yet. Anything you ask starts a chat, and it is kept."}
              </div>
            )}

            {blocks.map((block, i) => (
              <motion.div
                key={i}
                custom={i}
                variants={rowIn}
                className="flex flex-none flex-col gap-3"
              >
                {/* A queued message is drawn as what it is: said, written
                    down, and not yet read. The filled bubble is reserved for
                    messages the run actually has — drawing this one the same
                    way would claim the model had seen it, which is the one
                    thing the screen must never claim on its own. It settles
                    into an ordinary bubble on the backend's `user_read`. */}
                {block.kind === "user" &&
                  (block.msg!.pending ? (
                    <div className="flex max-w-[62%] flex-col items-end gap-1 self-end">
                      <div className="w-full whitespace-pre-wrap break-words rounded-[16px_16px_4px_16px] border border-dashed border-line2 bg-surface px-3.75 py-2.75 text-base leading-[1.55] text-ink2 dark:border-line2-d dark:bg-surface-d dark:text-ink2-d">
                        {block.msg!.text}
                      </div>
                      <span className={cx(mono, "text-2xs text-faint dark:text-faint-d")}>
                        queued · not read yet
                      </span>
                    </div>
                  ) : (
                    <div className="max-w-[62%] self-end whitespace-pre-wrap break-words rounded-[16px_16px_4px_16px] bg-ink px-3.75 py-2.75 text-base leading-[1.55] text-white dark:bg-ink-d dark:text-canvas-d">
                      {block.msg!.text}
                    </div>
                  ))}

                {block.kind === "notice" && (
                  <div className="flex max-w-[82%] flex-col items-start gap-2">
                    <div className="whitespace-pre-wrap break-words text-base leading-[1.65] text-bad dark:text-bad-d">
                      {block.msg!.text}
                    </div>
                    {i === blocks.length - 1 && lastAsked && !chatBusy && (
                      <button
                        type="button"
                        onClick={() => send(lastAsked)}
                        className={cx(PILL, "cursor-pointer")}
                      >
                        Ask again
                      </button>
                    )}
                  </div>
                )}

                {block.kind === "agent" && (
                  <div className="flex max-w-[82%] flex-col gap-3">
                    {block.msg?.text && (
                      <div className="break-words text-base leading-[1.65] text-ink2 dark:text-ink2-d">
                        <Prose text={block.msg.text} />
                        {streaming && i === blocks.length - 1 && (
                          <span
                            aria-hidden="true"
                            className="ml-0.75 inline-block h-[13px] w-1.5 animate-blink bg-primary align-[-2px] dark:bg-primary-d"
                          />
                        )}
                      </div>
                    )}
                    {block.tools.length > 0 && (
                      <div className="flex flex-wrap gap-1.75">
                        {block.tools.map((t, ti) => (
                          <Receipt key={ti} msg={t} />
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </motion.div>
            ))}

            {chatBusy && !streaming && (
              <div className="flex max-w-[82%] flex-none items-start gap-2 text-base leading-[1.65] text-faint dark:text-faint-d">
                <span className="mt-1 text-primary dark:text-primary-d">{SPINNING}</span>
                <span className="min-w-0">{chatThinking || "Thinking…"}</span>
              </div>
            )}

          </motion.div>

          {/* Outside the thread on purpose.
              A permission is not a message: an agent is stopped until it is
              answered. Inside the scroller it was appended below whatever the
              operator happened to be reading, and the thread only scrolls
              itself when they are already at the bottom — so a request that
              arrived while they were looking further up simply never came into
              view, and the first they knew of it was the sheet appearing when
              they navigated to another screen. Pinned here it cannot be
              missed and cannot be scrolled away from. */}
          {approvals.length > 0 && (
            <div className="flex flex-none flex-col gap-2.5">
              {approvals.map((request) => (
                <PermissionSheet key={request.request_id} request={request} />
              ))}
            </div>
          )}

          <div
            onDragOver={(e) => {
              // Only claim the drop when the drag actually carries files;
              // dragging selected text around the thread is not an attachment.
              if (!e.dataTransfer.types.includes("Files")) return;
              e.preventDefault();
              setDragging(true);
            }}
            onDragLeave={(e) => {
              if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
              setDragging(false);
            }}
            onDrop={(e) => {
              if (!e.dataTransfer.types.includes("Files")) return;
              e.preventDefault();
              setDragging(false);
              void take(Array.from(e.dataTransfer.files));
            }}
            className={cx(
              "flex-none rounded-lg border-[1.5px] bg-surface px-3.5 py-3 shadow-composer transition-colors duration-150 dark:bg-surface-d",
              dragging
                ? "border-primary bg-primarySoft dark:border-primary-d dark:bg-primarySoft-d"
                : "border-primaryLine dark:border-primaryLine-d",
            )}
          >
            {attached.length > 0 && (
              <div className="mb-2 flex flex-wrap gap-1.5">
                {attached.map((file) => (
                  <Attached
                    key={file}
                    path={file}
                    remove={() => setAttached((was) => was.filter((f) => f !== file))}
                  />
                ))}
                <span className={cx(mono, "self-center text-xs text-faint dark:text-faint-d")}>
                  read from disk, not uploaded
                </span>
              </div>
            )}

            <textarea
              rows={2}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  send();
                }
              }}
              onPaste={onPaste}
              aria-label={`Tell ${speaker?.name ?? "the Director"} what happens next`}
              placeholder={
                taking
                  ? "Saving that attachment…"
                  : chatBusy
                    ? "Say something while it works — it reads this at its next turn…"
                    : "Tell the crew what happens next…"
              }
              className="w-full resize-none border-none bg-transparent text-md leading-[1.6] text-ink outline-none placeholder:text-faint dark:text-ink-d dark:placeholder:text-faint-d"
            />

            <div className="mt-3 flex items-center gap-1.75">
              <button
                type="button"
                aria-label="Attach files to this message"
                title="Attach files to this message"
                onClick={attach}
                className={cx(
                  "grid h-6.5 w-6.5 flex-none cursor-pointer place-items-center rounded-full border transition-colors duration-150",
                  attached.length
                    ? "border-primaryLine bg-primarySoft text-primary dark:border-primaryLine-d dark:bg-primarySoft-d dark:text-primary-d"
                    : "border-line bg-surface text-muted hover:bg-hovered dark:border-line-d dark:bg-surface-d dark:text-muted-d dark:hover:bg-hovered-d",
                )}
              >
                <Plus size={12} strokeWidth={3} aria-hidden="true" />
              </button>

              <span className={cx(PILL, "flex-none")} title="The model this profile runs on">
                {speaker?.model ?? "model chosen by Claude"}
              </span>

              <span className="relative flex-none">
                <button
                  type="button"
                  aria-expanded={pickProfile}
                  onClick={() => setPickProfile((v) => !v)}
                  className={cx(PILL, "cursor-pointer")}
                >
                  {speaker?.name ?? "Director"}
                </button>
                <AnimatePresence>
                  {pickProfile && (
                    <motion.div
                      variants={popover}
                      initial="hidden"
                      animate="shown"
                      exit="gone"
                      className={cx(POPOVER, "bottom-[calc(100%+6px)] left-0")}
                    >
                      {agents
                        .filter((a) => a.chat_enabled && !a.paused)
                        .map((a) => (
                          <button
                            key={a.id}
                            type="button"
                            onClick={() => {
                              setPickProfile(false);
                              newConversation(a.id);
                            }}
                            className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
                          >
                            <span className="min-w-0 flex-1 truncate text-md font-medium text-ink2 dark:text-ink2-d">
                              {a.name}
                            </span>
                            <span className={cx(mono, "text-xs text-faint dark:text-faint-d")}>
                              {a.model ?? "auto"}
                            </span>
                          </button>
                        ))}
                      <div className="px-2.5 pb-1 pt-1.5 text-xs leading-normal text-faint dark:text-faint-d">
                        Picking one starts a new chat: a different profile means a different
                        session, created when you send.
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </span>

              <span
                aria-disabled="true"
                title="Not built yet"
                className="flex-none rounded-full border border-line bg-surface px-3 py-1 text-sm font-medium text-faint dark:border-line-d dark:bg-surface-d dark:text-faint-d"
              >
                Voice · soon
              </span>

              {/* Both, while a turn runs. Stop used to replace Send, which is
                  what made the composer dead for the length of a turn: there
                  was no way to say anything short of ending the work. */}
              <span className="ml-auto flex flex-none items-center gap-1.75">
                {chatBusy && (
                  <button
                    type="button"
                    aria-label="Stop this turn"
                    title="Stop this turn — anything you have queued is dropped, and stays above unsent"
                    onClick={stop}
                    className="grid h-6.5 w-6.5 flex-none cursor-pointer place-items-center rounded-full border border-line bg-surface text-muted hover:bg-hovered dark:border-line-d dark:bg-surface-d dark:text-muted-d dark:hover:bg-hovered-d"
                  >
                    <Square size={9} strokeWidth={3} fill="currentColor" aria-hidden="true" />
                  </button>
                )}
                <button
                  type="button"
                  aria-label={chatBusy ? "Queue this for the running turn" : "Send"}
                  title={
                    chatBusy
                      ? "It joins the turn already running — the agent reads it while it works"
                      : undefined
                  }
                  onClick={() => send()}
                  disabled={!text.trim() && attached.length === 0}
                  className="grid h-6.5 w-6.5 flex-none cursor-pointer place-items-center rounded-full border-none bg-primary text-white disabled:cursor-default disabled:opacity-50 dark:bg-primary-d dark:text-canvas-d"
                >
                  <ArrowUp size={13} strokeWidth={2.7} aria-hidden="true" />
                </button>
              </span>
            </div>
          </div>
        </div>

        <div className="flex min-h-0 w-[286px] flex-none flex-col gap-3.25 overflow-auto">
          <Touched />
          <ThreadTotals />
        </div>
      </div>
    </motion.div>
  );
}
