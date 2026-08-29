/** O estado de chat: as conversas com o Director e o que se diz nelas.
 *
 *  Vive à parte do resto do store porque tem um ciclo próprio — uma conversa
 *  sobrevive à troca de projecto, a resposta chega ao vivo pelo canal de
 *  execução com o id da conversa, e a transcrição é lida do disco.
 *
 *  Continua a não deter verdade nenhuma: o backend decide a que conversa
 *  pertence cada turno e devolve-a.
 */

import { useCallback, useRef, useState, type RefObject } from "react";
import { api } from "../lib/ipc";
import type { Conversation, RunLogLine, RunUpdate } from "../lib/types";

const DIRECTOR = "director";

export interface ChatMsg {
  /** `notice` is Relay itself talking: a failed resume, a cancelled turn.
   *  `tool` is what the agent tried (`summary`) — its result arrives as a
   *  second tool bubble matched by id, green or red, expandable. */
  role: "user" | "agent" | "notice" | "tool";
  text: string;
  /** When it was said, so the transcript can date itself. */
  ts: number;
  /** Tool bubble only: which tool, whether its result closed it, and the
   *  full output kept for expansion (#28: never dumped inline). */
  tool?: string;
  ok?: boolean | null;
  detail?: string | null;
  toolUseId?: string | null;
  parentToolUseId?: string | null;
}

/** One stored transcript line as a chat bubble. Deltas never reach here: the
 *  final `text` is the record (the backend does not log them). */
function toChatMsg(line: RunLogLine): ChatMsg | null {
  const ts = line.ts_ms;
  switch (line.kind) {
    case "user_message":
      return line.text?.trim() ? { role: "user", text: line.text, ts } : null;
    case "text":
      return line.text?.trim() ? { role: "agent", text: line.text, ts } : null;
    case "notice":
      return line.text?.trim() ? { role: "notice", text: line.text, ts } : null;
    case "failed":
      return { role: "notice", text: line.message ?? "the answer did not arrive", ts };
    case "tool_use": {
      const tool = toolName(line.tool);
      const ids = line as RunLogLine & {
        tool_use_id?: string | null;
        parent_tool_use_id?: string | null;
      };
      return {
        role: "tool",
        text: line.summary ?? "",
        ts,
        tool,
        toolUseId: ids.tool_use_id ?? null,
        parentToolUseId: ids.parent_tool_use_id ?? null,
        ok: null,
        detail: null,
      } as ChatMsg;
    }
    case "tool_result": {
      const res = line as RunLogLine & { tool_use_id?: string; ok?: boolean; detail?: string | null; summary?: string };
      return {
        role: "tool",
        text: res.summary ?? "",
        ts,
        // Without this the fold below has nothing to match on, and every
        // finished call rendered twice — once as a receipt still spinning,
        // once as its own orphaned result. The live path always carried it;
        // only the stored transcript dropped it.
        toolUseId: res.tool_use_id ?? null,
        ok: res.ok !== false,
        detail: res.detail ?? null,
      } as ChatMsg;
    }
    // Tool calls and session boundaries are in the log but would clutter the
    // conversation; they show live as progress instead.
    default:
      return null;
  }
}

/** The tool's own name, without the plumbing that routed it.
 *
 *  A call arrives as `mcp__harness__read_docs` over MCP or `harness:read_docs`
 *  from the CLI adapter, and the operator cares about neither prefix. The two
 *  paths into the transcript used to strip with two different patterns, and
 *  the one that matched `harness` without its colon left every stored receipt
 *  reading `:read_docs`. */
export function toolName(raw: string | undefined | null): string {
  return (raw ?? "tool")
    .replace(/^mcp__[a-z0-9_]+?__/i, "")
    .replace(/^harness[:_]+/i, "")
    .trim() || "tool";
}

/** Stored logs list call and result as separate lines; the transcript wants
 *  one bubble that opens and closes. Results with no open call stay alone. */
function foldToolResults(msgs: ChatMsg[]): ChatMsg[] {
  const out: ChatMsg[] = [];
  for (const m of msgs) {
    if (m.role === "tool" && m.ok != null && m.toolUseId) {
      let matched = false;
      for (let i = out.length - 1; i >= 0; i--) {
        const p = out[i];
        if (p.role === "tool" && p.ok == null && p.toolUseId === m.toolUseId) {
          out[i] = { ...p, ok: m.ok, detail: m.detail };
          matched = true;
          break;
        }
      }
      if (matched) continue;
    }
    out.push(m);
  }
  return out;
}

/** O que o chat precisa do resto do store: avisar o operador, e saber que
 *  projecto está em foco quando nasce uma conversa. */
export interface ChatDeps {
  toast: (tone: string, title: string, body?: string) => void;
  fail: (e: unknown, what: string) => void;
  projectRef: RefObject<string | null>;
}

export interface ChatState {
  /** Every conversation the backend knows about, newest first. */
  conversations: Conversation[];
  /** The one on screen, or `null` while the screen holds a draft — a chat that
   *  exists nowhere but here yet. A draft is the *absence* of a conversation,
   *  never a placeholder row with an invented id. */
  conversationId: string | null;
  /** Which profile a draft will speak to once it is created; `null` means the
   *  Director. Says nothing while `conversationId` is set. */
  draftProfile: string | null;
  chat: ChatMsg[];
  chatBusy: boolean;
  /** A stored transcript is being read off disk. Distinct from `chatBusy`,
   *  which is the model answering: an empty thread that is still loading and
   *  an empty thread that has nothing in it are different screens. */
  chatLoading: boolean;
  /** The Director's reasoning as it arrives; cleared when it answers. */
  chatThinking: string;
  /** A lista vem do backend, tanto pelo arranque como pelo evento. */
  setConversations: (list: Conversation[]) => void;
  /** O que o bootstrap deixa: a lista, e a conversa a reabrir. */
  hydrate: (conversations: Conversation[], lastConversation: string | null) => void;
  /** Absorve um update se ele for desta conversa. `true` quando o consumiu, e
   *  aí o feed de execução não o vê. */
  consume: (u: RunUpdate) => boolean;

  sendChat: (text: string, attachments?: string[]) => Promise<void>;
  /** Put the screen into a draft. Nothing is written until the first message:
   *  the row, and the Claude session behind it, are born on send. */
  newConversation: (profileId?: string) => Promise<void>;
  /** Open the standing conversation with a profile, creating one only if there
   *  is none: clicking "chat" twice continues the same thread. */
  chatWithProfile: (profileId: string) => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  archiveConversation: (id: string, archived: boolean) => Promise<void>;
  deleteConversation: (id: string) => Promise<void>;
  pinConversation: (id: string, projectId: string | null) => Promise<void>;
}

export function useChat({ toast, fail, projectRef }: ChatDeps): ChatState {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [conversationId, setConversationId] = useState<string | null>(null);
  const [draftProfile, setDraftProfile] = useState<string | null>(null);
  const [chat, setChat] = useState<ChatMsg[]>([]);
  const [chatBusy, setChatBusy] = useState(false);
  const [chatLoading, setChatLoading] = useState(false);
  const [chatThinking, setChatThinking] = useState("");
  // True while deltas are arriving for the current answer, so the final `text`
  // event is not appended on top of what was already streamed.
  const streamedRef = useRef(false);

  // The run channel is keyed by conversation id, so the listener needs the
  // current one without re-subscribing on every switch.
  const chatRef = useRef<string | null>(null);
  chatRef.current = conversationId;
  // The profile a draft is waiting on, read inside `sendChat` at the moment it
  // creates the row — state would be a render behind.
  const draftRef = useRef<string | null>(null);
  draftRef.current = draftProfile;

  /** Land on a real conversation: whatever was drafted is not pending any more. */
  const settle = useCallback((id: string) => {
    setConversationId(id);
    chatRef.current = id;
    setDraftProfile(null);
    draftRef.current = null;
  }, []);

  const hydrate = useCallback((list: Conversation[], lastConversation: string | null) => {
    setConversations(list);
    // The backend decides which conversation reopens; the frontend just
    // renders it. Its transcript is read from disk, so it is there whether
    // or not the native Claude session can still be resumed.
    if (lastConversation) {
      setConversationId(lastConversation);
      chatRef.current = lastConversation;
      api
        .conversationTranscript(lastConversation)
        .then((lines) => setChat(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null)))
        .catch(() => {});
    }
  }, []);

  const consume = useCallback((u: RunUpdate): boolean => {
    const appendToDirector = (text: string) =>
      setChat((cs) => {
        const last = cs[cs.length - 1];
        if (last && last.role === "agent") {
          return [...cs.slice(0, -1), { ...last, text: last.text + text }];
        }
        return [...cs, { role: "agent", text, ts: Date.now() }];
      });

    // A conversation streams under its own id. `DIRECTOR` is the id older
    // builds published chat on; kept so nothing from before is orphaned.
    if (u.card_id !== chatRef.current && u.card_id !== DIRECTOR) return false;

    switch (u.kind) {
      case "thinking":
        if (u.text) setChatThinking((t) => (t + u.text).slice(-600));
        break;
      case "delta":
        if (u.text) {
          streamedRef.current = true;
          setChatThinking("");
          appendToDirector(u.text);
        }
        break;
      case "tool_use": {
        // A tool call is a transcript line, not a transient badge: five
        // calls in a row used to leave the trace of one, and a failed
        // one read like a clean one (#41 in the visual layer).
        const ev = u as RunUpdate & {
          tool_use_id?: string;
          parent_tool_use_id?: string | null;
        };
        const tool = toolName(u.tool);
        setChat((cs) => [
          ...cs,
          {
            role: "tool",
            text: u.summary ?? "",
            ts: u.ts_ms,
            tool,
            toolUseId: ev.tool_use_id ?? null,
            parentToolUseId: ev.parent_tool_use_id ?? null,
            ok: null,
            detail: null,
          },
        ]);
        break;
      }
      case "tool_result": {
        // Closes the matching call by id — replaces the pending bubble
        // in place instead of appending a second line. A result with no
        // open call (replay started mid-run) lands closed on its own.
        const res = u as RunUpdate & {
          tool_use_id?: string;
          ok?: boolean | null;
          detail?: string | null;
          summary?: string;
        };
        setChat((cs) => {
          let closed = false;
          const next = cs.map((m) => {
            if (
              !closed &&
              m.role === "tool" &&
              m.ok == null &&
              m.toolUseId != null &&
              m.toolUseId === res.tool_use_id
            ) {
              closed = true;
              return { ...m, ok: res.ok !== false, detail: res.detail ?? null };
            }
            return m;
          });
          if (closed) return next;
          return [
            ...next,
            {
              role: "tool",
              text: res.summary ?? "",
              ts: u.ts_ms,
              toolUseId: res.tool_use_id ?? null,
              ok: res.ok !== false,
              detail: res.detail ?? null,
            } as ChatMsg,
          ];
        });
        break;
      }
      case "text":
        // Already shown token by token; the full text would double it.
        if (u.text && !streamedRef.current) appendToDirector(u.text);
        streamedRef.current = false;
        break;
      case "notice":
        // Relay itself talking — a resume that could not be honoured.
        if (u.text) setChat((cs) => [...cs, { role: "notice", text: u.text!, ts: u.ts_ms }]);
        break;
      case "done":
        streamedRef.current = false;
        setChatThinking("");
        setChatBusy(false);
        break;
      case "failed":
        streamedRef.current = false;
        setChatThinking("");
        setChatBusy(false);
        setChat((cs) => [
          ...cs,
          { role: "notice", text: `No answer: ${u.message}`, ts: u.ts_ms },
        ]);
        break;
    }
    return true;
  }, []);

  const refreshConversations = useCallback(async () => {
    try {
      setConversations(await api.conversations());
    } catch {
      /* the list is a view of backend state; a failed read is not fatal */
    }
  }, []);

  const sendChat = useCallback(
    async (text: string, attachments: string[] = []) => {
      const clean = text.trim();
      if ((!clean && attachments.length === 0) || chatBusy) return;
      // What goes on screen is what the backend will fold into the turn: the
      // message, then the files by name. No hidden context.
      const shown = attachments.length
        ? [clean, attachments.map((f) => `- ${f}`).join("\n")].filter(Boolean).join("\n\n")
        : clean;
      setChat((cs) => [...cs, { role: "user", text: shown, ts: Date.now() }]);
      setChatBusy(true);
      setChatThinking("");
      streamedRef.current = false;
      try {
        // This is where a draft becomes a conversation: the row and the native
        // session are minted by the first message, not by the click that
        // opened the screen. Pinned to the project in focus so it can read
        // that code, and to the profile the draft was opened for.
        //
        // `chat_send` does accept a null id, but what it does with one is
        // `open_conversation`, which *resumes the last thread of that profile*
        // — so a draft sent that way would land in the previous chat instead
        // of a new one, and would lose both the project pin and the chosen
        // profile. Hence the explicit create here.
        if (!chatRef.current) {
          const started = await api.conversationNew(
            draftRef.current ?? DIRECTOR,
            projectRef.current,
          );
          settle(started.id);
        }
        // The reply streams back on the run channel, keyed by the conversation
        // id; `chatBusy` clears when the done event arrives, not when this call
        // returns. The backend decides which conversation this belongs to and
        // hands it back, so the first message of a new chat lands in the right
        // thread.
        const conversation = await api.chatSend(clean, chatRef.current, attachments);
        settle(conversation.id);
        await refreshConversations();
      } catch (e) {
        setChatBusy(false);
        fail(e, "The message could not be sent");
      }
    },
    [chatBusy, fail, projectRef, refreshConversations, settle],
  );

  /** Load a conversation and its stored transcript. */
  const openConversation = useCallback(
    async (id: string) => {
      setChatLoading(true);
      try {
        const conversation = await api.conversationSelect(id);
        const lines = await api.conversationTranscript(id);
        settle(conversation.id);
        setChat(foldToolResults(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null)));
        setChatThinking("");
        setChatBusy(false);
        streamedRef.current = false;
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not open that conversation");
      } finally {
        setChatLoading(false);
      }
    },
    [fail, refreshConversations, settle],
  );

  /** A direct, persistent conversation with one profile. Resumes the last one
   *  rather than piling up a new session per click. */
  const chatWithProfile = useCallback(
    async (profileId: string) => {
      try {
        const conversation = await api.conversationOpen(profileId, projectRef.current);
        await openConversation(conversation.id);
      } catch (e) {
        fail(e, "Could not open that conversation");
      }
    },
    [fail, openConversation, projectRef],
  );

  /** New Chat, which is a draft and nothing else.
   *
   *  Minting the row here is what put two empty threads on disk for an
   *  operator who clicked `+` twice while thinking. Nothing is created — no
   *  row, no native session — until there is something to put in it; the
   *  create happens in `sendChat`, and a draft nobody types into leaves no
   *  trace at all. A new row is still a new session, so New Chat still does
   *  not continue the last one. */
  const newConversation = useCallback(async (profileId?: string) => {
    setConversationId(null);
    chatRef.current = null;
    setDraftProfile(profileId ?? null);
    draftRef.current = profileId ?? null;
    setChat([]);
    setChatThinking("");
    setChatBusy(false);
    setChatLoading(false);
    streamedRef.current = false;
  }, []);

  const renameConversation = useCallback(
    async (id: string, title: string) => {
      try {
        await api.conversationRename(id, title);
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not rename the conversation");
      }
    },
    [fail, refreshConversations],
  );

  const archiveConversation = useCallback(
    async (id: string, archived: boolean) => {
      try {
        await api.conversationArchive(id, archived);
        await refreshConversations();
        // Archiving the open one leaves the screen on a draft: no id, no
        // thread, and nothing written until the next message.
        if (archived && chatRef.current === id) {
          setConversationId(null);
          chatRef.current = null;
          setChat([]);
        }
        toast("ok", archived ? "Archived" : "Restored");
      } catch (e) {
        fail(e, "Could not archive the conversation");
      }
    },
    [fail, refreshConversations, toast],
  );

  const deleteConversation = useCallback(
    async (id: string) => {
      const which = conversations.find((c) => c.id === id);
      const ok = window.confirm(
        `Delete "${which?.title ?? id}"?\n\n` +
          "The transcript is deleted with it, and the Claude session it continues " +
          "can no longer be reopened. This cannot be undone.",
      );
      if (!ok) return;
      try {
        await api.conversationDelete(id);
        // Same as archiving: what is left on screen is a draft, not a hole.
        if (chatRef.current === id) {
          setConversationId(null);
          chatRef.current = null;
          setChat([]);
        }
        await refreshConversations();
        toast("ok", "Deleted", which?.title);
      } catch (e) {
        fail(e, "Could not delete the conversation");
      }
    },
    [conversations, fail, refreshConversations, toast],
  );

  const pinConversation = useCallback(
    async (id: string, project: string | null) => {
      try {
        await api.conversationPin(id, project);
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not change the project");
      }
    },
    [fail, refreshConversations],
  );

  return {
    conversations,
    conversationId,
    draftProfile,
    chat,
    chatBusy,
    chatLoading,
    chatThinking,
    setConversations,
    hydrate,
    consume,
    sendChat,
    newConversation,
    chatWithProfile,
    openConversation,
    renameConversation,
    archiveConversation,
    deleteConversation,
    pinConversation,
  };
}
