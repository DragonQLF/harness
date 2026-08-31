/** The chat screen: one conversation, the receipts its tools left, the
 *  permission requests it produced, and the field you answer in.
 *
 *  The only screen whose pane does not scroll. The thread scrolls inside its
 *  own card so the composer stays where the hand expects it, and the rail
 *  beside it reports what the thread did rather than what it said. */

import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { ArrowUp, Plus, Square } from "lucide-react";
import { Streamdown } from "streamdown";
import { ago, bytes, clock, money, num, plural, shortAgo } from "../lib/format";
import { cx } from "../lib/cx";
import { paneIn, popover, rowIn } from "../lib/motion";
import {
  modelLabel,
  ruleLabel,
  SHELL_TOOLS,
  STATUS_NAME,
  type AllowRule,
  type AttachmentPreview,
  type BackgroundTask,
  type ConversationTotals,
  type PendingApproval,
} from "../lib/types";
import { toolName, useStore, type ChatMsg } from "../state/store";
import { countGroupLines, decodePath, groupView, summariseTools } from "../state/toolgroup";
import { api, reason } from "../lib/ipc";
import { mono } from "../components/ui";

/** A pill in the composer row and in the permission sheet. */
const PILL =
  "rounded-full border border-line bg-surface px-3 py-1 text-sm font-medium text-ink2 dark:border-line-d dark:bg-surface-d dark:text-ink2-d";

/** The levels the engine takes, and `null` for whatever the model does on its
 *  own. Written out rather than read from the model's own list because that
 *  list only exists once a session is open, and the choice is made before the
 *  first message of one. A level the model does not have is downgraded by the
 *  engine, so the worst case here is asking for more than exists. */
const EFFORTS: { id: string | null; name: string }[] = [
  { id: null, name: "Model decides" },
  { id: "low", name: "Low" },
  { id: "medium", name: "Medium" },
  { id: "high", name: "High" },
  { id: "xhigh", name: "Extra high" },
  { id: "max", name: "Max" },
];

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
  // A lista vem do `allow.rs` pelo `vocabulary.ts`, como no `ruleIsRevoked`.
  // Estava escrita à mão aqui e outra vez no `scopable` — a mesma regra de
  // segurança em três cópias, duas delas sem nada que as mantivesse a par.
  if (!SHELL_TOOLS.includes(head)) return { tool: request.tool };
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
  if (!rule.command) return !SHELL_TOOLS.includes(rule.tool.toLowerCase());
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
/** An image an agent produced, fetched over the IPC boundary.
 *
 *  The path in the markdown is a real file on this machine, and the webview
 *  cannot open one: the page is served from a custom protocol, so `/Users/…`
 *  in a `src` resolves against the page rather than against the disk. The
 *  bytes come across as a data URL — the road a pasted attachment already
 *  travels — and the backend decides which paths are allowed to make that
 *  trip, because the path was written by a model.
 *
 *  A remote `src` is left exactly as it is. The CSP already refuses it, and a
 *  broken image is the honest outcome of an answer that pointed at the web. */
function InlineImage({ src, alt }: { src?: string; alt?: string }) {
  const [data, setData] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const local = !!src && !/^[a-z]+:/i.test(src);

  useEffect(() => {
    if (!local || !src) return;
    let alive = true;
    api
      .previewImage(decodePath(src))
      .then((url) => alive && setData(url))
      .catch((e) => alive && setFailed(reason(e)));
    return () => {
      alive = false;
    };
  }, [src, local]);

  if (!local) return <img src={src} alt={alt ?? ""} className="max-w-full rounded-8px" />;

  // Não se pinta um rectângulo cinzento à espera: uma imagem que falhou e uma
  // que ainda vem seriam a mesma caixa. Enquanto vem não há nada; se falhar,
  // diz-se porquê e mostra-se o caminho, que é o que o operador precisa para
  // ir vê-la ele próprio.
  if (failed) {
    return (
      <span className="block text-sm leading-relaxed text-text4 dark:text-text4-d">
        {alt || "image"} — {failed}
        <br />
        <span className={mono}>{src}</span>
      </span>
    );
  }
  if (!data) return null;
  return (
    <img
      src={data}
      alt={alt ?? ""}
      className="max-w-full rounded-8px border border-line dark:border-line-d"
    />
  );
}

const PROSE_COMPONENTS = { img: InlineImage };

function Prose({ text }: { text: string }) {
  // Streamdown ships its own chip for inline code — dark and heavy, sized for
  // a white page rather than for this thread. The old hand-rolled Prose used
  // `bg-active`, which reads as emphasis instead of as a label, so the app's
  // own tokens win here. `:not(pre) > code` leaves fenced blocks alone: those
  // are meant to be a slab, and only the inline chips were shouting.
  return (
    <Streamdown components={PROSE_COMPONENTS} className="[&_:not(pre)>code]:rounded-5px [&_:not(pre)>code]:bg-active [&_:not(pre)>code]:px-1.5 [&_:not(pre)>code]:py-px [&_:not(pre)>code]:font-mono [&_:not(pre)>code]:text-body [&_:not(pre)>code]:font-normal [&_:not(pre)>code]:text-ink2 [&_:not(pre)>code]:before:content-none [&_:not(pre)>code]:after:content-none dark:[&_:not(pre)>code]:bg-active-d dark:[&_:not(pre)>code]:text-ink2-d">
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
/** Uma leitura que se pode ver.
 *
 *  Um `Read` de uma imagem é a única leitura cujo resultado é para *olhar*, e
 *  chegava como uma linha a dizer o nome do ficheiro. O caminho vem inteiro do
 *  `toolsum` só neste caso; a etiqueta corta-o para o nome, e o que abre é a
 *  imagem em vez de um bloco de texto que não existe. */
const IMAGE_PATH = /^[^\s]*[\\/][^\s\\/]+\.(png|jpe?g|webp|gif)$/i;

function Receipt({ msg }: { msg: ChatMsg }) {
  const [open, setOpen] = useState(false);
  const flying = msg.ok == null;
  // The backend's summary often opens with the tool's own name — the raw one,
  // prefixes and all — so joining it to the label printed it twice:
  // `read_docs · mcp__harness__read_docs: decisions`. Say the name once.
  const said = (msg.text ?? "").replace(/^\s*(mcp__[a-z0-9_]+?__)?[a-z0-9_.-]+\s*[:·]\s*/i, (m) =>
    toolName(m.replace(/\s*[:·]\s*$/, "")) === msg.tool ? "" : m,
  );
  const picture = msg.tool === "Read" && IMAGE_PATH.test(said.trim()) ? said.trim() : null;
  const shownSaid = picture ? picture.split(/[\\/]/).pop()! : said.trim();
  const label = [msg.tool, shownSaid].filter(Boolean).join(" · ") || "tool";
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

  if (!msg.detail && !picture) {
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
      {open && picture && (
        <div className="w-full">
          <InlineImage src={picture} alt={shownSaid} />
        </div>
      )}
      {open && msg.detail && (
        <pre className="max-h-[240px] w-full overflow-auto whitespace-pre-wrap break-words rounded-sm border border-line2 bg-active px-2.5 py-2 font-mono text-sm leading-[1.7] text-muted dark:border-line2-d dark:bg-active-d dark:text-muted-d">
          {msg.detail}
        </pre>
      )}
    </>
  );
}

/** Um troço de raciocínio, fechado.
 *
 *  Fechado por omissão e sempre: ao contrário de um grupo de ferramentas, isto
 *  não é trabalho a acontecer — já aconteceu, e quem quer lê-lo diz que quer.
 *  Aberto por omissão punha o raciocínio inteiro entre a pergunta e a resposta,
 *  que é onde ele menos serve.
 *
 *  Ao vivo continua a haver o indicador de sempre por cima do compositor; isto
 *  é o que fica depois, e o que sobrevive a recarregar a conversa — as fatias
 *  que o formaram são efémeras e nunca chegam ao disco (#25). */
function Thought({ text, ts }: { text: string; ts: number }) {
  const [open, setOpen] = useState(false);
  const words = text.trim().split(/\s+/).length;
  return (
    <div className="flex w-full min-w-0 max-w-[82%] flex-col gap-1.5">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex cursor-pointer items-center gap-2 text-left text-sm text-muted hover:text-ink2 dark:text-muted-d dark:hover:text-ink2-d"
      >
        <span className={cx(mono, "text-11")} aria-hidden="true">
          {open ? "⌄" : "›"}
        </span>
        <span>Thought · {plural(words, "word")}</span>
      </button>
      {open && (
        <div className="max-h-[340px] overflow-y-auto whitespace-pre-wrap break-words rounded-9px border border-line px-3 py-2.5 text-sm leading-[1.7] text-muted dark:border-line-d dark:text-muted-d">
          {text}
        </div>
      )}
      {!open && (
        <span className={cx(mono, "text-2xs text-faint dark:text-faint-d")}>{clock(ts)}</span>
      )}
    </div>
  );
}

/** Um grupo de chamadas consecutivas, embrulhado numa linha.
 *
 *  Era um `flex-wrap` de fichas sem tecto: vinte chamadas davam vinte fichas
 *  empilhadas, a empurrar a resposta para fora do ecrã, e nada as fazia parar
 *  de crescer. Agora o grupo diz-se numa linha — quantos ficheiros, quantos
 *  comandos — e abre-se para uma lista com altura máxima e barra própria.
 *
 *  Fechado por omissão só quando **já acabou**. Enquanto alguma chamada está no
 *  ar fica aberto: o que está a acontecer agora é a única coisa que não se pode
 *  esconder atrás de um resumo. */
function ToolGroup({ tools }: { tools: ChatMsg[] }) {
  const flying = tools.some((t) => t.ok == null);
  const failed = tools.filter((t) => t.ok === false).length;
  const [open, setOpen] = useState<boolean | null>(null);
  const view = groupView(tools.length, flying, open);
  const shown = view === "open";

  const summary = summariseTools(tools);
  const lines = countGroupLines(tools);

  // Uma chamada é uma ficha e nada mais: sem cabeçalho, sem seta, sem estado
  // que se possa fechar para um sítio pior do que aquele de onde veio.
  if (view === "chip") {
    return <Receipt msg={tools[0]} />;
  }

  return (
    <div className="flex w-full min-w-0 flex-col gap-1.5">
      <button
        type="button"
        aria-expanded={shown}
        onClick={() => setOpen(!shown)}
        className="flex w-full cursor-pointer items-center gap-2 text-left text-sm text-muted hover:text-ink2 dark:text-muted-d dark:hover:text-ink2-d"
      >
        <span className={cx(mono, "text-11")} aria-hidden="true">
          {shown ? "⌄" : "›"}
        </span>
        <span className="min-w-0 truncate">{summary}</span>
        {lines && (
          <span className={cx(mono, "shrink-0 text-11")}>
            <span className="text-ok dark:text-ok-d">+{lines.added}</span>{" "}
            <span className="text-bad dark:text-bad-d">−{lines.removed}</span>
          </span>
        )}
        {failed > 0 && (
          <span className="shrink-0 text-bad dark:text-bad-d">
            {failed} failed
          </span>
        )}
        {flying && <span className="shrink-0">{SPINNING}</span>}
      </button>

      {shown && (
        <div className="flex max-h-[340px] flex-col gap-1.5 overflow-y-auto rounded-9px border border-line px-2 py-2 dark:border-line-d">
          {tools.map((t, i) => (
            <Receipt key={t.toolUseId ?? i} msg={t} />
          ))}
        </div>
      )}
    </div>
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
/** As conversas que existem, e o botão que faz mais uma.
 *
 *  Não havia nenhum dos dois em lado nenhum do ecrã de Chat. Criar uma conversa
 *  era o ⌘K ou o menu do sistema; trocar de conversa era o ⌘K e saber o título
 *  de cor. Uma app de conversas em que a lista de conversas não está visível
 *  esconde a coisa de que é feita — e um `+` que só existe numa paleta é um `+`
 *  que ninguém encontra.
 *
 *  A que está aberta marca-se; um rascunho é o estado sem id, e diz-se como
 *  tal em vez de se desenhar uma linha que ainda não existe. */
function Threads() {
  const { conversations, conversationId, agents, newConversation, openConversation, draftProfile } =
    useStore();
  const [showArchived, setShowArchived] = useState(false);

  const shown = conversations.filter((c) => (showArchived ? true : !c.archived));
  const archived = conversations.filter((c) => c.archived).length;
  const draft = conversationId == null;

  return (
    <div className="flex-none rounded-lg border border-line bg-surface px-4 py-3.75 dark:border-line-d dark:bg-surface-d">
      <div className="flex items-center gap-2">
        <div className="min-w-0 flex-1 text-md font-bold text-ink dark:text-ink-d">Chats</div>
        <button
          type="button"
          title="Start a new chat — nothing is written until you send the first message"
          aria-label="New chat"
          onClick={() => newConversation()}
          className="flex flex-none cursor-pointer items-center gap-1 rounded-full border-none bg-primary px-2.5 py-1 text-xs font-bold text-white transition-colors duration-150 hover:bg-primaryDeep dark:bg-primary-d"
        >
          <Plus size={11} strokeWidth={3} aria-hidden="true" />
          New
        </button>
      </div>

      {/* Um rascunho não é uma linha da lista: não tem id, não está no disco, e
          desenhá-lo lá em cima fingia uma conversa que ainda não existe. */}
      {draft && (
        <div className="mt-3 rounded-9px border border-dashed border-line2 px-2.5 py-2 dark:border-line2-d">
          <div className="truncate text-body font-semibold text-ink dark:text-ink-d">
            New chat with {agents.find((a) => a.id === (draftProfile ?? "director"))?.name ?? "the Director"}
          </div>
          <div className={cx(mono, "mt-0.5 text-xs text-faint dark:text-faint-d")}>
            draft · saved when you send
          </div>
        </div>
      )}

      {shown.length === 0 && !draft && (
        <div className="mt-3 text-body leading-[1.55] text-muted dark:text-muted-d">
          No chats yet. Anything you ask starts one, and it is kept.
        </div>
      )}

      <div className="mt-2 flex flex-col">
        {shown.map((c) => {
          const on = c.id === conversationId;
          const who = agents.find((a) => a.id === c.profile_id)?.name ?? c.profile_id;
          return (
            <button
              key={c.id}
              type="button"
              aria-current={on ? "true" : undefined}
              onClick={() => openConversation(c.id)}
              className={cx(
                "cursor-pointer rounded-9px border-none px-2.5 py-1.75 text-left transition-colors duration-150",
                on
                  ? "bg-primarySoft dark:bg-primarySoft-d"
                  : "bg-transparent hover:bg-hovered dark:hover:bg-hovered-d",
              )}
            >
              <div
                className={cx(
                  "truncate text-body",
                  on
                    ? "font-semibold text-ink dark:text-ink-d"
                    : "font-medium text-ink2 dark:text-ink2-d",
                )}
              >
                {c.title}
              </div>
              <div className={cx(mono, "mt-0.5 truncate text-xs text-faint dark:text-faint-d")}>
                {who} · {shortAgo(c.updated_ms)}
                {c.archived && " · archived"}
              </div>
            </button>
          );
        })}
      </div>

      {archived > 0 && (
        <button
          type="button"
          onClick={() => setShowArchived((v) => !v)}
          className="mt-2 cursor-pointer border-none bg-transparent p-0 text-xs text-faint underline-offset-2 hover:underline dark:text-faint-d"
        >
          {showArchived ? "hide archived" : `${archived} archived`}
        </button>
      )}
    </div>
  );
}

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
        {/* Um travessão e não `$0.00`. `spend_usd` é nulo quando o fio correu,
            no todo ou em parte, num sítio que não factura em dólares — e um
            zero ali dizia que o trabalho foi de graça. O SDK factura sempre
            contra as tabelas da Anthropic, portanto o número que ele dava para
            um endpoint qualquer estava 27× acima do que a coisa custou. */}
        <Total
          label="Spend"
          value={totals?.spend_usd != null ? money(totals.spend_usd) : "—"}
        />
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
  kind: "user" | "agent" | "notice" | "thinking";
  msg: ChatMsg | null;
  tools: ChatMsg[];
}

/** Uma vez da conversa: o que foi dito, e os recibos das ferramentas que isso
 *  puxou.
 *
 *  Está aqui fora, e memoizada, por causa do streaming. `blocks` é reconstruído
 *  a cada quadro de texto — os invólucros são novos, as mensagens lá dentro é
 *  que não —, e sem esta comparação cada quadro reanalisava o markdown de
 *  *todos* os balões da conversa para redesenhar o último. Numa conversa longa
 *  isso é uma dúzia de documentos por quadro, e é isso que se sentia como
 *  gaguez: não o texto a chegar, mas tudo o que já lá estava a ser refeito
 *  atrás dele.
 *
 *  O `motion.div` vem para dentro de propósito: no pai, o `memo` só pouparia os
 *  filhos e a árvore do `motion` era montada na mesma. */
const Turn = memo(
  function Turn({
    kind,
    msg,
    tools,
    index,
    streaming,
    askAgain,
  }: {
    kind: Block["kind"];
    msg: ChatMsg | null;
    tools: ChatMsg[];
    index: number;
    /** O cursor a piscar. Só a última vez o tem, e só enquanto o turno corre. */
    streaming: boolean;
    /** Presente só na última vez, e só quando falhou: repetir a pergunta.
     *  `null` em tudo o resto, que é o que deixa a comparação abaixo passar. */
    askAgain: (() => void) | null;
  }) {
    return (
      <motion.div custom={index} variants={rowIn} className="flex flex-none flex-col gap-3">
        {/* A queued message is drawn as what it is: said, written down, and
            not yet read. The filled bubble is reserved for messages the run
            actually has — drawing this one the same way would claim the model
            had seen it, which is the one thing the screen must never claim on
            its own. It settles into an ordinary bubble on the backend's
            `user_read`. */}
        {kind === "user" && (
          <div className="flex max-w-[62%] flex-col items-end gap-1 self-end">
            <div
              className={cx(
                "w-full whitespace-pre-wrap break-words rounded-[16px_16px_4px_16px] px-3.75 py-2.75 text-base leading-[1.55]",
                msg!.pending
                  ? "border border-dashed border-line2 bg-surface text-ink2 dark:border-line2-d dark:bg-surface-d dark:text-ink2-d"
                  : "bg-ink text-white dark:bg-ink-d dark:text-canvas-d",
              )}
            >
              {msg!.text}
            </div>
            {/* Quando foi dito, e em que pé está.
                Só três estados são honestos aqui. *Queued* e *read* são os
                únicos que o backend confirma, e confirma-os por id: o
                `chat_queue` responde com um, o `user_read` fecha-o. Uma
                mensagem que não passou por fila começou o turno, portanto foi
                lida por construção e não precisa de o dizer — dizer-lhe "read"
                seria inventar um facto que ninguém mediu. */}
            <span className={cx(mono, "text-2xs text-faint dark:text-faint-d")}>
              {clock(msg!.ts)}
              {msg!.pending
                ? " · queued · not read yet"
                : msg!.queueId
                  ? " · read"
                  : ""}
            </span>
          </div>
        )}

        {kind === "thinking" && <Thought text={msg!.text} ts={msg!.ts} />}

        {kind === "notice" && (
          <div className="flex max-w-[82%] flex-col items-start gap-2">
            <div className="whitespace-pre-wrap break-words text-base leading-[1.65] text-bad dark:text-bad-d">
              {msg!.text}
            </div>
            {askAgain && (
              <button type="button" onClick={askAgain} className={cx(PILL, "cursor-pointer")}>
                Ask again
              </button>
            )}
          </div>
        )}

        {kind === "agent" && (
          <div className="flex max-w-[82%] flex-col gap-3">
            {msg?.text && (
              <div className="break-words text-base leading-[1.65] text-ink2 dark:text-ink2-d">
                <Prose text={msg.text} />
                {streaming && (
                  <span
                    aria-hidden="true"
                    className="ml-0.75 inline-block h-[13px] w-1.5 animate-blink bg-primary align-[-2px] dark:bg-primary-d"
                  />
                )}
              </div>
            )}
            {tools.length > 0 && <ToolGroup tools={tools} />}
            {/* A hora em que a resposta começou a chegar. Enquanto está a
                escrever não se diz: o carimbo mudaria por baixo do texto e
                pareceria a resposta a saltar no tempo. */}
            {msg?.text && !streaming && (
              <span className={cx(mono, "text-2xs text-faint dark:text-faint-d")}>
                {clock(msg.ts)}
              </span>
            )}
          </div>
        )}
      </motion.div>
    );
  },
  // As mensagens antigas mantêm a identidade — só a última é substituída a
  // cada quadro —, portanto comparar por referência chega, desde que o array
  // de recibos se compare elemento a elemento: esse é novo em cada quadro.
  (a, b) =>
    a.kind === b.kind &&
    a.msg === b.msg &&
    a.index === b.index &&
    a.streaming === b.streaming &&
    a.askAgain === b.askAgain &&
    a.tools.length === b.tools.length &&
    a.tools.every((t, i) => t === b.tools[i]),
);

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

/** O trabalho que continua por baixo da resposta.
 *
 *  Fora do fio e pela mesma razão que as permissões: isto não é uma mensagem, é
 *  estado — o que está a correr *agora*. Dentro do scroller ficava preso no
 *  sítio onde por acaso apareceu, a dizer uma coisa que entretanto mudou.
 *
 *  Existe porque um turno que responde não é um turno que acabou. Um watchdog
 *  posto em fundo não deixava marca nenhuma no ecrã, e foi essa invisibilidade
 *  que fez o #108 levar uma tarde a encontrar. O conjunto chega inteiro e
 *  substitui-se; esvazia-se quando a execução acaba, porque agora ela leva
 *  consigo o que levantou. */
function BackgroundWork({ tasks }: { tasks: BackgroundTask[] }) {
  if (tasks.length === 0) return null;
  return (
    <div className="flex flex-none flex-col gap-1.5 rounded-lg border border-line bg-surface2 px-3 py-2.5 dark:border-line-d dark:bg-surface2-d">
      <div className="flex items-center gap-2 text-xs font-medium text-muted dark:text-muted-d">
        <span className="text-primary dark:text-primary-d">{SPINNING}</span>
        <span>
          {tasks.length === 1
            ? "1 task still running in the background"
            : `${tasks.length} tasks still running in the background`}
        </span>
      </div>
      <ul className="flex flex-col gap-1">
        {tasks.map((task) => (
          <li key={task.task_id} className="flex min-w-0 items-baseline gap-2 text-xs">
            {/* O tipo vem do motor; quando ele não o sabe cai no discriminante
                cru, e é isso que se mostra — nunca uma etiqueta inventada. */}
            <span className="flex-none rounded bg-primarySoft px-1.5 py-0.5 font-mono text-[11px] text-primary dark:bg-primarySoft-d dark:text-primary-d">
              {task.task_type || "task"}
            </span>
            <span className="min-w-0 truncate text-faint dark:text-faint-d">
              {task.description || "no description"}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function Chat() {
  const {
    chat,
    chatModel,
    chatBusy,
    chatLoading,
    chatThinking,
    backgroundTasks,
    sendChat,
    agents,
    project,
    projects,
    conversation,
    conversationId,
    draftProfile,
    approvals,
    newConversation,
    chatWithProfile,
    pinConversation,
    renameConversation,
    archiveConversation,
    deleteConversation,
    commands,
    toast,
  } = useStore();

  const [text, setText] = useState("");
  const [attached, setAttached] = useState<string[]>([]);
  const [taking, setTaking] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [pickProfile, setPickProfile] = useState(false);
  const [pickProject, setPickProject] = useState(false);
  const [pickAgent, setPickAgent] = useState(false);
  /** How hard to think, from here on. Null is the model's own default.
   *
   *  Sticky rather than per-message: chosen when a problem needs it and kept
   *  until it is changed, which is what makes it worth reaching for. It rides
   *  on each request, so it can be changed mid-conversation and the next
   *  message goes out at the new level — no new session, nothing to restart.
   *  Held here rather than on the conversation because it is how the operator
   *  is working, not what the thread is. */
  const [effort, setEffort] = useState<string | null>(null);
  const [pickEffort, setPickEffort] = useState(false);
  /** Whether the slash menu is welcome. Typing `/` opens it; Escape and a
   *  choice close it, and it stays closed until the next `/`. */
  const [slashing, setSlashing] = useState(false);
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

  /** Repetir a última pergunta. Tem de ser estável: é uma prop do `Turn`, e uma
   *  função nova a cada render fazia falhar a comparação de todas as vezes —
   *  que é exactamente o que o `memo` lá está a evitar. O `ref` mantém-na
   *  actual sem lhe mudar a identidade. */
  const retry = useRef<() => void>(() => {});
  retry.current = () => send(lastAsked);
  const askAgain = useCallback(() => retry.current(), []);

  useEffect(() => {
    const el = thread.current;
    if (el && stuck.current) el.scrollTop = el.scrollHeight;
  }, [chat, chatBusy, chatThinking]);

  /** The composer stays live for the whole turn. `sendChat` decides what that
   *  means — a message typed while the agent is working joins the run instead
   *  of starting a second one — so there is nothing to refuse here. */
  const send = (body: string = text) => {
    if (!body.trim() && attached.length === 0) return;
    sendChat(body, attached, effort);
    setText("");
    setAttached([]);
    // The choice stays until it is changed again. It is picked when a problem
    // needs it, and a problem rarely needs it for exactly one message — having
    // to re-pick it every turn would be the real cost.
    setSlashing(false);
  };

  /** What the operator is reaching for when the line starts with a slash.
   *
   *  Only ever the *first* line and only while it is still one word: `/model
   *  opus` has already chosen, and a menu over it would be in the way. */
  const slashQuery = (() => {
    const line = text.trimStart();
    if (!line.startsWith("/") || line.startsWith("//")) return null;
    const rest = line.slice(1);
    if (/[\s\n]/.test(rest)) return null;
    return rest.toLowerCase();
  })();

  const matches = useMemo(() => {
    if (slashQuery === null) return [];
    return commands
      .filter((c) => {
        const names = [c.name, ...(c.aliases ?? [])];
        return names.some((n) => n.toLowerCase().startsWith(slashQuery));
      })
      .slice(0, 8);
  }, [commands, slashQuery]);

  // Aberto também quando não há lista nenhuma, para dizer porquê. O menu vinha
  // de um evento efémero publicado por cada sessão nova: numa instalação
  // acabada de abrir não há nada para mostrar, e mostrar nada lia-se como o `/`
  // estar avariado. Agora a lista sobrevive ao reinício (vem no `bootstrap`) e
  // este ramo é só para o primeiro arranque de todos.
  const open = slashing && (matches.length > 0 || commands.length === 0);

  /** Put the command in the box rather than sending it: most take arguments,
   *  and the ones that do not are one keystroke from going anyway. */
  const pick = (name: string) => {
    setText(`/${name} `);
    setSlashing(false);
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

  const modelOf = modelLabel(speaker?.model);
  const facts = [chatModel ?? (speaker?.model ? modelOf.label : null), speaker?.title]
    .filter(Boolean)
    .join(" · ");

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
            {/* Trocar de agente é trocar de fio, não trocar o dono deste.
                Uma sessão pertence ao perfil que a abriu — o modelo dela, as
                ferramentas dela —, portanto reatribuir a conversa a outro
                agente era pô-lo a continuar uma conversa que não teve. O
                `chatWithProfile` abre a que esse agente já tem, e só cria uma
                quando não há nenhuma. */}
            <div className="relative">
              <button
                type="button"
                aria-expanded={pickAgent}
                title="Talk to another agent"
                onClick={() => setPickAgent((v) => !v)}
                className="flex cursor-pointer items-center gap-1.5 border-none bg-transparent p-0 text-title font-bold text-ink dark:text-ink-d"
              >
                {speaker?.name ?? "Director"}
                <span className={cx(mono, "text-body font-normal text-faint dark:text-faint-d")} aria-hidden="true">
                  ⌄
                </span>
              </button>
              <AnimatePresence>
                {pickAgent && (
                  <motion.div
                    variants={popover}
                    initial="hidden"
                    animate="shown"
                    exit="gone"
                    className={cx(POPOVER, "left-0 top-[calc(100%+6px)] z-20")}
                  >
                    {agents.filter((a) => a.chat_enabled && !a.paused).map((a) => (
                      <button
                        key={a.id}
                        type="button"
                        onClick={() => {
                          setPickAgent(false);
                          if (a.id !== speaker?.id) void chatWithProfile(a.id);
                        }}
                        className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left text-md font-medium text-ink2 transition-colors duration-150 hover:bg-hovered dark:text-ink2-d dark:hover:bg-hovered-d"
                      >
                        <span className="min-w-0 flex-1 truncate">{a.name}</span>
                        <span className={cx(mono, "shrink-0 text-2xs text-faint dark:text-faint-d")}>
                          {a.backend === "codex" ? "codex" : (a.model ?? "claude")}
                        </span>
                      </button>
                    ))}
                  </motion.div>
                )}
              </AnimatePresence>
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
              <Turn
                key={i}
                index={i}
                kind={block.kind}
                msg={block.msg}
                tools={block.tools}
                streaming={streaming && i === blocks.length - 1}
                askAgain={
                  block.kind === "notice" && i === blocks.length - 1 && lastAsked && !chatBusy
                    ? askAgain
                    : null
                }
              />
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
          <BackgroundWork tasks={backgroundTasks} />

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

            {/* The slash menu sits above the box, not over the thread: what
                is being completed is the line being typed, and covering the
                conversation to show it would hide the thing it is about. */}
            <AnimatePresence>
              {open && (
                <motion.div
                  variants={popover}
                  initial="hidden"
                  animate="shown"
                  exit="gone"
                  className="mb-2 max-h-[228px] overflow-y-auto rounded-10px border border-line bg-elev p-1 shadow-soft dark:border-line-d dark:bg-elev-d dark:shadow-soft-d"
                >
                  {commands.length === 0 && (
                    <div className="px-2.5 py-2 text-sm leading-normal text-muted dark:text-muted-d">
                      No commands yet. The list comes from the session itself, so it arrives with
                      the first answer on this machine — and is remembered after that.
                    </div>
                  )}
                  {matches.map((c) => (
                    <button
                      key={c.name}
                      type="button"
                      onClick={() => pick(c.name)}
                      className="flex w-full cursor-pointer items-baseline gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-1.75 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
                    >
                      <span className={cx(mono, "flex-none text-md text-ink2 dark:text-ink2-d")}>
                        /{c.name}
                      </span>
                      {c.argument_hint && (
                        <span className={cx(mono, "flex-none text-xs text-faint dark:text-faint-d")}>
                          {c.argument_hint}
                        </span>
                      )}
                      <span className="min-w-0 flex-1 truncate text-sm text-muted dark:text-muted-d">
                        {c.description}
                      </span>
                    </button>
                  ))}
                </motion.div>
              )}
            </AnimatePresence>

            <textarea
              rows={2}
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                // Opened by typing the slash, never by the menu deciding it
                // knows better: an operator who dismissed it is not asked again
                // until they start a new command.
                if (e.target.value.trimStart() === "/") setSlashing(true);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape" && open) {
                  e.preventDefault();
                  setSlashing(false);
                  return;
                }
                // Tab completes, Enter sends. A menu that swallowed Enter
                // would make `/usage` — a whole command on its own — take two
                // keystrokes to say.
                if (e.key === "Tab" && open) {
                  e.preventDefault();
                  pick(matches[0].name);
                  return;
                }
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

              {/* O modelo em que esta conversa correu, medido na linha de
                  `usage` do último turno — `claude-opus-5`, e não `opus`.
                  O perfil guarda a pergunta ("um Opus qualquer"); isto é a
                  resposta, e é a resposta que interessa a quem está a olhar.
                  Antes do primeiro turno não há resposta nenhuma, e então
                  mostra-se o que está configurado; se nem isso, um travessão,
                  como qualquer outro valor que ninguém deu. */}
              <span
                className={cx(PILL, "flex-none")}
                title={
                  chatModel
                    ? `The last turn here ran on ${chatModel}.`
                    : `${modelOf.hint} Nothing has run in this chat yet.`
                }
              >
                {chatModel ?? modelOf.label}
              </span>

              {/* Per message, and it says so. Effort binds the request rather
                  than the session, which is the whole reason it can be chosen
                  here at all — the output style cannot, and lives in the
                  agent's settings instead. */}
              <span className="relative flex-none">
                <button
                  type="button"
                  aria-expanded={pickEffort}
                  title={
                    effort
                      ? `Thinking at ${effort}, from this message on. A model without that level is downgraded by the engine.`
                      : "Relay is not asking for a level, so the model thinks as much as it thinks. Pick one to push it harder."
                  }
                  onClick={() => setPickEffort((v) => !v)}
                  className={cx(
                    PILL,
                    "cursor-pointer",
                    effort &&
                      "border-primaryLine bg-primarySoft text-primary dark:border-primaryLine-d dark:bg-primarySoft-d dark:text-primary-d",
                  )}
                >
                  {`thinking · ${effort ?? "model decides"}`}
                </button>
                <AnimatePresence>
                  {pickEffort && (
                    <motion.div
                      variants={popover}
                      initial="hidden"
                      animate="shown"
                      exit="gone"
                      className={cx(POPOVER, "bottom-[calc(100%+6px)] left-0")}
                    >
                      {EFFORTS.map((level) => (
                        <button
                          key={level.id ?? "default"}
                          type="button"
                          onClick={() => {
                            setEffort(level.id);
                            setPickEffort(false);
                          }}
                          className="flex w-full cursor-pointer items-baseline gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-2 text-left transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d"
                        >
                          <span className="min-w-0 flex-1 truncate text-md font-medium text-ink2 dark:text-ink2-d">
                            {level.name}
                          </span>
                          {level.id === effort && (
                            <span className={cx(mono, "text-xs text-primary dark:text-primary-d")}>
                              ✓
                            </span>
                          )}
                        </button>
                      ))}
                      <div className="px-2.5 pb-1 pt-1.5 text-xs leading-normal text-faint dark:text-faint-d">
                        Stays until you change it, this conversation and the next.
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
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
          <Threads />
          <Touched />
          <ThreadTotals />
        </div>
      </div>
    </motion.div>
  );
}
