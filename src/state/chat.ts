/** O estado de chat: as conversas com o Director e o que se diz nelas.
 *
 *  Vive à parte do resto do store porque tem um ciclo próprio — uma conversa
 *  sobrevive à troca de projecto, a resposta chega ao vivo pelo canal de
 *  execução com o id da conversa, e a transcrição é lida do disco.
 *
 *  Continua a não deter verdade nenhuma: o backend decide a que conversa
 *  pertence cada turno e devolve-a.
 */

import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { api } from "../lib/ipc";
import { appendStreamed, alreadySaid, type ChatMsg } from "./bubbles";
import type {
  BackgroundTask,
  Conversation,
  RunLogLine,
  RunUpdate,
  SlashCommand,
} from "../lib/types";

const DIRECTOR = "director";

export type { ChatMsg } from "./bubbles";
/** One stored transcript line as a chat bubble. Deltas never reach here: the
 *  final `text` is the record (the backend does not log them). */
function toChatMsg(line: RunLogLine): ChatMsg | null {
  const ts = line.ts_ms;
  switch (line.kind) {
    case "user_message":
      return line.text?.trim() ? { role: "user", text: line.text, ts } : null;
    case "user_queued":
      // Written down when it was accepted and nothing more. A transcript that
      // stops here is one where the message never reached the model, and the
      // bubble goes on saying so.
      return line.text?.trim()
        ? {
            role: "user",
            text: line.text,
            ts,
            queueId: line.queue_id ?? null,
            pending: true,
          }
        : null;
    case "user_read":
      // Carries no words: it only settles the queued line above it, which the
      // fold does by id.
      return { role: "user", text: "", ts, queueId: line.queue_id ?? null, pending: false };
    case "text":
      // Quem falou vem com o que foi dito. Um subagente escreve neste mesmo
      // fio, e sem isto a prosa dele era um balão do Director.
      return line.text?.trim()
        ? {
            role: "agent",
            text: line.text,
            ts,
            parentToolUseId: line.parent_tool_use_id ?? null,
          }
        : null;
    case "notice":
      return line.text?.trim() ? { role: "notice", text: line.text, ts } : null;
    case "thought":
      // Guardado, ao contrário das fatias que o formaram. Um troço vazio não é
      // um pensamento — é um turno que não pensou.
      return line.text?.trim() ? { role: "thinking", text: line.text, ts } : null;
    case "failed":
      return { role: "notice", text: line.message ?? "the answer did not arrive", ts };
    case "tool_use": {
      const tool = toolName(line.tool);
      return {
        role: "tool",
        text: line.summary ?? "",
        ts,
        tool,
        toolUseId: line.tool_use_id ?? null,
        parentToolUseId: line.parent_tool_use_id ?? null,
        added: line.added ?? null,
        removed: line.removed ?? null,
        ok: null,
        detail: null,
      } as ChatMsg;
    }
    case "tool_result": {
      return {
        role: "tool",
        text: line.summary ?? "",
        ts,
        // Without this the fold below has nothing to match on, and every
        // finished call rendered twice — once as a receipt still spinning,
        // once as its own orphaned result. The live path always carried it;
        // only the stored transcript dropped it.
        toolUseId: line.tool_use_id ?? null,
        ok: line.ok !== false,
        detail: line.detail ?? null,
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
 *  reading `:read_docs`.
 *
 *  Live no chat e é lido daqui pelo `events.ts`, que é a outra metade: ele
 *  tinha o seu, e só tirava o prefixo do nosso próprio servidor — uma
 *  ferramenta de um servidor concedido ao agente lia-se `mcp__figma__export`
 *  num ecrã e `export` no outro. Um nome, uma função. */
export function toolName(raw: string | undefined | null): string {
  return (raw ?? "tool")
    .replace(/^mcp__[a-z0-9_]+?__/i, "")
    .replace(/^harness[:_]+/i, "")
    .trim() || "tool";
}

/** Stored logs record a thing and then what became of it on two lines; the
 *  transcript wants one bubble that opens and closes. Two pairs fold that way:
 *  a tool call and its result, and a queued message and the moment the run
 *  read it. A closing line with nothing open stays alone rather than being
 *  dropped — a transcript joined mid-run is missing the opening, not lying. */
function fold(msgs: ChatMsg[]): ChatMsg[] {
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
    if (m.role === "user" && m.pending === false && m.queueId) {
      for (let i = out.length - 1; i >= 0; i--) {
        const p = out[i];
        if (p.role === "user" && p.pending && p.queueId === m.queueId) {
          out[i] = { ...p, pending: false };
          break;
        }
      }
      // A `user_read` whose queued line is not on screen has no words of its
      // own, so there is nothing honest to draw for it.
      continue;
    }
    out.push(m);
  }
  return out;
}

/** One stored transcript, as bubbles. */
function toChat(lines: RunLogLine[]): ChatMsg[] {
  return fold(lines.map(toChatMsg).filter((m): m is ChatMsg => m != null));
}

/** O modelo em que esta conversa correu de facto, da última vez que correu.
 *
 *  Medido, não deduzido. O perfil pode dizer `opus`, que é um apelido e não um
 *  modelo: qual dos Opus é decidido pelo login da Claude no momento do run, e
 *  o único sítio onde isso aparece é a linha de `usage` que o próprio turno
 *  escreveu. Mostrar o apelido era mostrar a pergunta em vez da resposta.
 *
 *  `toChat` deita as linhas de `usage` fora — são contabilidade, não conversa —
 *  por isso lê-se aqui, antes disso. A última ganha: uma conversa que caiu para
 *  outro modelo a meio correu no que acabou. */
function modelOfTranscript(lines: RunLogLine[]): string | null {
  return lines.reduce<string | null>(
    (found, l) => (l.kind === "usage" && l.model ? l.model : found),
    null,
  );
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
  /** What `/` can reach in this session: the engine's own commands plus what
   *  the granted skills brought. Published by the engine, never assembled
   *  here — a hardcoded list would be wrong the day a skill is granted. */
  commands: SlashCommand[];
  /** O trabalho que continua por baixo da resposta, agora mesmo. */
  backgroundTasks: BackgroundTask[];
  chat: ChatMsg[];
  /** O modelo em que esta conversa correu de facto, ou `null` antes do
   *  primeiro turno. Medido nas linhas de `usage`, nunca deduzido do perfil. */
  chatModel: string | null;
  chatBusy: boolean;
  /** A stored transcript is being read off disk. Distinct from `chatBusy`,
   *  which is the model answering: an empty thread that is still loading and
   *  an empty thread that has nothing in it are different screens. */
  chatLoading: boolean;
  /** The Director's reasoning as it arrives; cleared when it answers. */
  chatThinking: string;
  /** Há um balão de resposta aberto neste momento?
   *
   *  É o que a `streaming.current` sabe e mais ninguém sabia. O ecrã deduzia-o
   *  de "o último bloco é do agente e tem texto", o que passou a ser falso
   *  assim que uma chamada de ferramenta fechou o balão sem tirar o bloco do
   *  fim — e nessa altura o cursor piscava num balão acabado e o indicador de
   *  actividade desligava-se justamente enquanto havia trabalho a acontecer. */
  chatWriting: boolean;
  /** A lista vem do backend, tanto pelo arranque como pelo evento. */
  setConversations: (list: Conversation[]) => void;
  /** O que o bootstrap deixa: a lista, e a conversa a reabrir. */
  /** `known` is the slash menu the last session published, off the bootstrap.
   *  Empty on a first ever run, and replaced whole by the next session. */
  hydrate: (
    conversations: Conversation[],
    lastConversation: string | null,
    known: SlashCommand[],
  ) => void;
  /** Absorve um update se ele for desta conversa. `true` quando o consumiu, e
   *  aí o feed de execução não o vê. */
  consume: (u: RunUpdate) => boolean;

  /** Say something. While a turn is running this queues rather than refusing:
   *  the message joins the run in flight and the model reads it at its next
   *  read, so a correction lands during the work instead of after it.
   *
   *  `effort` is whatever level is currently chosen — `low` through `max`, or
   *  `null` for the model's own default. It binds the request, not the
   *  session, so changing it takes effect on the very next message. The engine
   *  downgrades a level the model does not have. */
  sendChat: (text: string, attachments?: string[], effort?: string | null) => Promise<void>;
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
  const [chatWriting, setChatWriting] = useState(false);
  /** Em que modelo esta conversa correu, das linhas de `usage` dela. */
  const [chatModel, setChatModel] = useState<string | null>(null);
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
  /** What `/` can reach. Published by the engine when a session opens, so it
   *  is empty until the first turn of the app's life — and kept across
   *  conversations afterwards, because the built-ins do not vary by thread and
   *  an empty menu is worse than a slightly stale one. */
  const [commands, setCommands] = useState<SlashCommand[]>([]);
  /** O que continua a correr por baixo da resposta. Nível, não acumulado: cada
   *  evento traz o conjunto vivo inteiro e substitui este. */
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTask[]>([]);
  /** Numbers the pending bubbles until the backend hands each one its real id. */
  const pendingSeq = useRef(0);
  /** One send at a time, and a ref rather than state because state is a render
   *  behind: two submits inside one render both read the same `false`. This
   *  guards the call, not the turn — the turn is the backend's to guard, and
   *  it does. What it stops here is the one thing the backend cannot see as a
   *  repeat: a draft sent twice, which is two `conversation_new` rows. */
  const sending = useRef(false);
  /** Queued messages the run said it had read before the call that queued them
   *  came back. Without this the late reply would re-mark a message pending
   *  that the model already has. */
  const readAlready = useRef<Set<string>>(new Set());

  /** Land on a real conversation: whatever was drafted is not pending any more. */
  const settle = useCallback((id: string) => {
    setConversationId(id);
    chatRef.current = id;
    setDraftProfile(null);
    draftRef.current = null;
  }, []);

  const hydrate = useCallback(
    (list: Conversation[], lastConversation: string | null, known: SlashCommand[]) => {
      setConversations(list);
      // O menu do `/` chega por um evento efémero, que só uma sessão nova
      // publica. Sem esta linha, o compositor abria sem menu a cada reinício e
      // só o recuperava depois de o operador já ter escrito alguma coisa — que
      // é exactamente quando já não precisava dele.
      if (known.length > 0) setCommands(known);
      // The backend decides which conversation reopens; the frontend just
      // renders it. Its transcript is read from disk, so it is there whether
      // or not the native Claude session can still be resumed.
      if (lastConversation) {
        setConversationId(lastConversation);
        chatRef.current = lastConversation;
        api
          .conversationTranscript(lastConversation)
          .then((lines) => {
            setChat(toChat(lines));
            setChatModel(modelOfTranscript(lines));
          })
          .catch(() => {});
      }
    },
    [],
  );

  /** O balão onde o turno em curso está a escrever. Nulo entre turnos. */
  const streaming = useRef<string | null>(null);

  const appendToDirector = useCallback(
    (text: string) =>
      setChat((cs) => {
        const placed = appendStreamed(cs, streaming.current, text, () =>
          `s${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        );
        streaming.current = placed.streamId;
        setChatWriting(true);
        return placed.list;
      }),
    [],
  );

  /** O mesmo, mas só se a transcrição ainda não o tiver.
   *
   *  Um `text` chega por duas vias — o evento ao vivo e a linha lida do disco —
   *  e as duas são entregas do mesmo registo. Juntavam-se por acrescento cego:
   *  bastava a transcrição ser lida primeiro (que é o que mandar uma mensagem
   *  faz) para a resposta aparecer duas vezes, igual e com o mesmo carimbo. */
  const appendUnlessPresent = useCallback(
    (text: string, tsMs: number) =>
      setChat((cs) => {
        if (alreadySaid(cs, text, tsMs)) return cs;
        const placed = appendStreamed(cs, streaming.current, text, () =>
          `s${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        );
        streaming.current = placed.streamId;
        setChatWriting(true);
        return placed.list;
      }),
    [],
  );

  /** O turno acabou: o balão dele fecha, e o que vier a seguir abre outro. */
  const endStream = useCallback(() => {
    streaming.current = null;
    setChatWriting(false);
  }, []);

  /** O que chegou desde o último quadro e ainda não foi para o estado.
   *
   *  Um `delta` é um token, e um token era um `setChat`: a janela inteira
   *  re-renderizava dezenas de vezes por segundo, com o markdown de cada balão
   *  a ser reanalisado de cada vez. O texto acumula-se aqui e assenta uma vez
   *  por quadro — nenhum modelo escreve mais depressa do que 60 fps, portanto
   *  não se perde nada da escrita ao vivo, e o resto da app deixa de pagar por
   *  ela.
   *
   *  Qualquer outro evento esvazia isto primeiro: um recibo de ferramenta que
   *  chegasse à frente do texto que o anunciou trocaria a ordem da transcrição,
   *  que é a única coisa que ela promete. */
  const held = useRef({ text: "", thinking: "", clearThinking: false });
  const frame = useRef<number | null>(null);

  const flush = useCallback(() => {
    if (frame.current != null) {
      cancelAnimationFrame(frame.current);
      frame.current = null;
    }
    const { text, thinking, clearThinking } = held.current;
    held.current = { text: "", thinking: "", clearThinking: false };
    if (clearThinking) setChatThinking("");
    else if (thinking) setChatThinking((t) => (t + thinking).slice(-600));
    if (text) appendToDirector(text);
  }, [appendToDirector]);

  /** Deitar fora o que ainda não assentou.
   *
   *  Trocar de conversa é a fronteira que isto protege: os tokens que estavam
   *  à espera do próximo quadro pertencem à conversa que saiu do ecrã, e
   *  assentariam por cima da que entrou. */
  const drop = useCallback(() => {
    if (frame.current != null) {
      cancelAnimationFrame(frame.current);
      frame.current = null;
    }
    held.current = { text: "", thinking: "", clearThinking: false };
    streaming.current = null;
    setChatWriting(false);
  }, []);

  const schedule = useCallback(() => {
    if (frame.current != null) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      flush();
    });
  }, [flush]);

  // Uma janela escondida não corre `requestAnimationFrame`. O que ficar por
  // assentar assenta no primeiro evento que não seja um token — e `done` é
  // sempre um deles —, por isso nada se perde por não estar a olhar.
  useEffect(
    () => () => {
      if (frame.current != null) cancelAnimationFrame(frame.current);
    },
    [],
  );

  const consume = useCallback((u: RunUpdate): boolean => {
    // A conversation streams under its own id. `DIRECTOR` is the id older
    // builds published chat on; kept so nothing from before is orphaned.
    if (u.card_id !== chatRef.current && u.card_id !== DIRECTOR) return false;

    if (u.kind !== "thinking" && u.kind !== "delta") flush();

    switch (u.kind) {
      case "started":
        endStream();
        // Por-processo: nada é emitido ao arrancar, por isso quem consome tem
        // de voltar ao conjunto vazio a cada sessão. Sem isto, tarefas de uma
        // execução anterior ficavam no ecrã como se ainda corressem.
        setBackgroundTasks([]);
        break;
      case "thinking":
        if (u.text) {
          held.current.thinking += u.text;
          schedule();
        }
        break;
      case "delta":
        // Um subagente escreve no mesmo fluxo. Sem esta pergunta os tokens
        // dele eram acrescentados, a meio de uma palavra, à resposta que o
        // operador estava a ver o Director escrever — que é a frase cortada
        // no ecrã: "high-R" / "PM, narrow audience".
        if (u.parent_tool_use_id) break;
        if (u.text) {
          streamedRef.current = true;
          held.current.thinking = "";
          held.current.clearThinking = true;
          held.current.text += u.text;
          schedule();
        }
        break;
      case "thought":
        // O troço fechou. Entra no fio como bloco fechado, e o balão de texto
        // que estivesse aberto fecha com ele: o que o modelo disser a seguir é
        // resposta ao que acabou de pensar, e vai por baixo.
        if (u.text?.trim()) {
          endStream();
          setChat((cs) => [...cs, { role: "thinking", text: u.text!, ts: u.ts_ms }]);
        }
        break;
      case "tool_use": {
        // A chamada fecha o balão que estava a ser escrito, e é isso que põe a
        // resposta pela ordem em que aconteceu.
        //
        // Sem isto, o `streaming.current` continuava a apontar para o balão de
        // cima e **todo** o texto do turno caía lá dentro, por muito que o
        // modelo tivesse falado depois de chamar a ferramenta. O resultado era
        // a prosa toda em cima e as chamadas todas empilhadas por baixo — a
        // ordem trocada, que é a única coisa que uma transcrição promete. O
        // texto que vier a seguir abre balão novo, por baixo da chamada.
        //
        // O `flush` no topo do `consume` já assentou o que estava por assentar,
        // portanto o que estava escrito fica onde estava.
        endStream();
        // A tool call is a transcript line, not a transient badge: five
        // calls in a row used to leave the trace of one, and a failed
        // one read like a clean one (#41 in the visual layer).
        const tool = toolName(u.tool);
        setChat((cs) => [
          ...cs,
          {
            role: "tool",
            text: u.summary ?? "",
            ts: u.ts_ms,
            tool,
            toolUseId: u.tool_use_id ?? null,
            parentToolUseId: u.parent_tool_use_id ?? null,
            added: u.added ?? null,
            removed: u.removed ?? null,
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
        setChat((cs) => {
          let closed = false;
          const next = cs.map((m) => {
            if (
              !closed &&
              m.role === "tool" &&
              m.ok == null &&
              m.toolUseId != null &&
              m.toolUseId === u.tool_use_id
            ) {
              closed = true;
              return { ...m, ok: u.ok !== false, detail: u.detail ?? null };
            }
            return m;
          });
          if (closed) return next;
          return [
            ...next,
            {
              role: "tool",
              text: u.summary ?? "",
              ts: u.ts_ms,
              toolUseId: u.tool_use_id ?? null,
              ok: u.ok !== false,
              detail: u.detail ?? null,
            } as ChatMsg,
          ];
        });
        break;
      }
      case "text":
        // Already shown token by token; the full text would double it.
        if (u.parent_tool_use_id) break;
        // And the transcript may already hold it. A stored line and a live
        // event are two deliveries of one record, and until now they were
        // merged by blind append: read the transcript first — which is what
        // sending a message does — and a late or replayed event added the same
        // answer a second time, word for word, at the same timestamp.
        //
        // The line's identity is the pair the log writes: when it happened and
        // what it said. Nothing else in a transcript can collide with that —
        // two different answers do not share a millisecond.
        if (u.text && !streamedRef.current) appendUnlessPresent(u.text, u.ts_ms);
        streamedRef.current = false;
        break;
      case "user_read": {
        // The run has it. This is the only thing that retires the "not read
        // yet" mark — the screen never decides that for itself.
        const id = u.queue_id;
        if (id) {
          // A resposta em curso acabou de ser interrompida por esta: fecha-se o
          // balão dela, e o que vier a seguir abre um novo — que fica por baixo
          // da mensagem, porque é a resposta a ela.
          endStream();
          readAlready.current.add(id);
          setChat((cs) =>
            cs.map((m) => (m.queueId === id && m.pending ? { ...m, pending: false } : m)),
          );
        }
        break;
      }
      case "notice":
        // Relay itself talking — a resume that could not be honoured.
        if (u.text) setChat((cs) => [...cs, { role: "notice", text: u.text!, ts: u.ts_ms }]);
        break;
      case "usage":
        // O turno diz em que modelo correu. É a única fonte honesta: o perfil
        // guarda o que foi pedido, isto é o que respondeu.
        if (u.model) setChatModel(u.model);
        break;
      case "commands":
        // Documented as replace, not merge: a skill that went away should stop
        // being offered.
        if (u.commands) setCommands(u.commands);
        break;
      case "background_tasks":
        // Também substituição, e pela razão mais forte: emparelhar arestas
        // deixaria um indicador presa a girar se uma delas se perdesse.
        setBackgroundTasks(u.tasks ?? []);
        break;
      case "local_output":
        // The engine answered by itself — `/usage`, `/context`. No model turn
        // is coming, so this is the whole reply and it lands as one.
        if (u.text) setChat((cs) => [...cs, { role: "agent", text: u.text!, ts: u.ts_ms }]);
        break;
      case "done":
        streamedRef.current = false;
        endStream();
        setChatThinking("");
        setChatBusy(false);
        // A execução acabou e leva consigo o que levantou (#108): o que ficasse
        // aqui seria trabalho que já não existe.
        setBackgroundTasks([]);
        break;
      case "failed":
        streamedRef.current = false;
        endStream();
        setChatThinking("");
        setChatBusy(false);
        setBackgroundTasks([]);
        setChat((cs) => [
          ...cs,
          { role: "notice", text: `No answer: ${u.message}`, ts: u.ts_ms },
        ]);
        break;
    }
    return true;
  }, [appendToDirector, appendUnlessPresent, endStream, flush, schedule]);

  const refreshConversations = useCallback(async () => {
    try {
      setConversations(await api.conversations());
    } catch {
      /* the list is a view of backend state; a failed read is not fatal */
    }
  }, []);

  const sendChat = useCallback(
    async (text: string, attachments: string[] = [], effort: string | null = null) => {
      const clean = text.trim();
      if (!clean && attachments.length === 0) return;
      if (sending.current) return;
      sending.current = true;
      // What goes on screen is what the backend will fold into the turn: the
      // message, then the files by name. No hidden context.
      const shown = attachments.length
        ? [clean, attachments.map((f) => `- ${f}`).join("\n")].filter(Boolean).join("\n\n")
        : clean;

      // A local handle only, so the reply can find the bubble it belongs to.
      // The real id is the backend's, and replaces this the moment it lands.
      const mark = `pending-${++pendingSeq.current}`;
      // The bubble appears now — the message has left the composer, and hiding
      // it until the model gets round to it is the worse lie. Marked as unread
      // only where that is plausibly true; the answer below decides for real.
      setChat((cs) => [
        ...cs,
        { role: "user", text: shown, ts: Date.now(), queueId: mark, pending: chatBusy },
      ]);

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
        let conversationId = chatRef.current;
        if (!conversationId) {
          const started = await api.conversationNew(
            draftRef.current ?? DIRECTOR,
            projectRef.current,
          );
          settle(started.id);
          conversationId = started.id;
        }

        // One command for both cases, because only the backend knows which
        // this is. It joins the turn in flight if there is one and starts an
        // ordinary turn if there is not — and it is the only thing that can
        // tell them apart without racing, since a turn can end between the
        // screen believing it runs and this call arriving.
        const queued = await api.chatQueue(clean, conversationId, attachments, effort);
        settle(queued.conversation.id);
        setChat((cs) =>
          cs.map((m) =>
            m.queueId === mark
              ? {
                  ...m,
                  queueId: queued.queue_id,
                  // The run may already have said it read this, before the
                  // call answering with its id came back.
                  pending:
                    queued.queue_id != null && !readAlready.current.has(queued.queue_id),
                }
              : m,
          ),
        );
        // No turn to join: the backend started one, and the screen has to
        // catch up with it.
        if (!queued.queue_id) {
          setChatBusy(true);
          setChatThinking("");
          streamedRef.current = false;
        }
        await refreshConversations();
      } catch (e) {
        // Refused by the backend, so it is nowhere: not on disk, not in a
        // queue. Nothing to leave on screen.
        setChat((cs) => cs.filter((m) => m.queueId !== mark));
        setChatBusy(false);
        fail(e, "The message could not be sent");
      } finally {
        sending.current = false;
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
        setChat(toChat(lines));
        setChatModel(modelOfTranscript(lines));
        setChatThinking("");
        setChatBusy(false);
        streamedRef.current = false;
        drop();
        await refreshConversations();
      } catch (e) {
        fail(e, "Could not open that conversation");
      } finally {
        setChatLoading(false);
      }
    },
    [drop, fail, refreshConversations, settle],
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
    setChatModel(null);
    setChatThinking("");
    setChatBusy(false);
    setChatLoading(false);
    streamedRef.current = false;
    drop();
  }, [drop]);

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
    commands,
    backgroundTasks,
    chat,
    chatModel,
    chatBusy,
    chatLoading,
    chatThinking,
    chatWriting,
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
