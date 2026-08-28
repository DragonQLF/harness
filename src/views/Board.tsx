import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { money, plural } from "../lib/format";
import { cx } from "../lib/cx";
import { arrive, colIn, paneIn, rowIn, sheetIn } from "../lib/motion";
import {
  STATUS_NAME,
  STATUS_ORDER,
  tone,
  type Status,
} from "../lib/types";
import { useStore } from "../state/store";
import { Glyph, Loading, mono, truncate } from "../components/ui";

/** Moves the board offers by hand; anything else needs an override. */
const LEGAL: Record<Status, Status[]> = {
  backlog: ["ready"],
  ready: ["backlog", "running"],
  running: ["ready", "review"],
  review: ["ready", "done"],
  done: [],
};

/** What an empty column actually means. "Empty" five times over is the same
 *  word standing in for five different states of the work. */
const EMPTY_COLUMN: Record<Status, string> = {
  backlog: "Nothing parked for later",
  ready: "Nothing waiting to start",
  running: "No agent working",
  review: "Nothing to review",
  done: "Nothing finished yet",
};

/** A cor de cada coluna, no texto e no ponto. Não é o `STATUS_TONE`: o
 *  backlog aqui é `text4` e não `text3`, como o desenho o tem. */
const COLUMN_COLOR: Record<Status, { fg: string; dot: string }> = {
  backlog: { fg: "text-text4 dark:text-text4-d", dot: "bg-text4 dark:bg-text4-d" },
  ready: { fg: "text-info dark:text-info-d", dot: "bg-info dark:bg-info-d" },
  running: { fg: "text-accent dark:text-accent-d", dot: "bg-accent dark:bg-accent-d" },
  review: { fg: "text-warn dark:text-warn-d", dot: "bg-warn dark:bg-warn-d" },
  done: { fg: "text-ok dark:text-ok-d", dot: "bg-ok dark:bg-ok-d" },
};

/** Which cards changed column since the last render, and which way they went.
 *
 *  This is the board's whole reason for animating: an agent finishing a run
 *  moves a card with no action from the operator, and a silent re-render makes
 *  that indistinguishable from nothing having happened. The comparison is
 *  against the previous render rather than against a backend event, so a change
 *  is caught however it arrived — the operator's own drag included.
 *
 *  The mark clears itself, or a card would keep announcing a move it made
 *  minutes ago every time React re-rendered for an unrelated reason.
 */
function useArrivals(cards: { id: string; status: Status }[] | undefined) {
  const seen = useRef(new Map<string, Status>());
  const [moves, setMoves] = useState<Map<string, "forward" | "back">>(new Map());

  useEffect(() => {
    if (!cards) return;
    const previous = seen.current;
    const next = new Map<string, Status>();
    const fresh = new Map<string, "forward" | "back">();
    for (const card of cards) {
      next.set(card.id, card.status);
      const was = previous.get(card.id);
      // A card seen for the first time has not moved; it was created, or the
      // board just loaded. Announcing those would light up the whole screen.
      if (was && was !== card.status) {
        const from = STATUS_ORDER.indexOf(was);
        const to = STATUS_ORDER.indexOf(card.status);
        fresh.set(card.id, to >= from ? "forward" : "back");
      }
    }
    seen.current = next;
    if (fresh.size === 0) return;
    setMoves(fresh);
    const done = setTimeout(() => setMoves(new Map()), 900);
    return () => clearTimeout(done);
  }, [cards]);

  return moves;
}

/** Uma pastilha de contorno. */
const CHIP =
  "min-h-6 cursor-pointer rounded-full border border-line3 text-sm font-medium text-text2 transition-[border-color,background,color] duration-150 hover:border-line4 hover:bg-surface2 dark:border-line3-d dark:text-text2-d dark:hover:border-line4-d dark:hover:bg-surface2-d";

/** The one place a card is created by hand: a line, who takes it, and whether
 *  it starts now. */
function NewCard({ close }: { close: () => void }) {
  const { agents, createCard } = useStore();
  const takers = agents.filter((a) => a.tasks_enabled && !a.paused);
  const [title, setTitle] = useState("");
  const [who, setWho] = useState(takers.find((a) => a.id !== "director")?.id ?? takers[0]?.id ?? "");

  const add = (mode: "later" | "plan" | "start") => {
    if (!title.trim()) return;
    createCard(title, who, mode);
    close();
  };

  return (
    <motion.div
      variants={sheetIn}
      initial="hidden"
      animate="shown"
      exit="gone"
      className="flex items-center gap-2 border-b border-line bg-surface px-4 py-2 dark:border-line-d dark:bg-surface-d"
    >
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") add(e.shiftKey ? "start" : "plan");
          if (e.key === "Escape") close();
        }}
        aria-label="What should happen?"
        placeholder="What should happen? One card, one outcome."
        className="min-w-0 flex-1 rounded-sm border border-line3 bg-bg px-3 py-2 text-md font-normal text-text outline-none focus-visible:border-accentLine dark:border-line3-d dark:bg-bg-d dark:text-text-d dark:focus-visible:border-accentLine-d"
      />
      <select
        value={who}
        aria-label="Who takes this card"
        onChange={(e) => setWho(e.target.value)}
        className="cursor-pointer rounded-sm border border-line3 bg-bg px-2.5 py-2 text-sm font-medium text-text2 dark:border-line3-d dark:bg-bg-d dark:text-text2-d"
      >
        {takers.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </select>
      {[
        { label: "Later", mode: "later" as const, strong: false },
        { label: "Ready", mode: "plan" as const, strong: false },
        { label: "Start now", mode: "start" as const, strong: true },
      ].map((b) => (
        <button
          key={b.mode}
          type="button"
          onClick={() => add(b.mode)}
          className={cx(
            "min-h-6 cursor-pointer rounded-full px-3 py-1.5 text-sm",
            b.strong
              ? "border-none bg-accent font-semibold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-accent-d dark:text-onAccent-d"
              : cx(CHIP, "bg-transparent"),
            title.trim() ? "opacity-100" : "opacity-55",
          )}
        >
          {b.label}
        </button>
      ))}
      <button
        type="button"
        onClick={close}
        className="min-h-6 cursor-pointer rounded-sm border-none bg-transparent px-2 text-sm font-medium text-text4 transition-colors duration-150 hover:text-text dark:text-text4-d dark:hover:text-text-d"
      >
        Cancel
      </button>
    </motion.div>
  );
}

export function Board({
  openRun,
  openReview,
}: {
  openRun: (cardId: string) => void;
  openReview: (cardId: string) => void;
}) {
  const { snapshot, agents, outputs, streams, moveCard, startRun, cancelRun, discard } = useStore();
  const [drag, setDrag] = useState<string | null>(null);
  const [over, setOver] = useState<Status | null>(null);
  const [adding, setAdding] = useState(false);
  const arrived = useArrivals(snapshot?.cards);
  // Com movimento reduzido o cartão continua a dizer que mudou de coluna — só
  // não viaja para o dizer. Calar-se aqui esconderia uma mudança de estado,
  // que é o contrário do que a preferência pede.
  const still = useReducedMotion();

  if (!snapshot) return <Loading what="Reading the board" />;
  const cards = snapshot.cards;
  const dragged = cards.find((c) => c.id === drag);

  const drop = (to: Status) => {
    setOver(null);
    const card = dragged;
    setDrag(null);
    if (!card || card.status === to || !LEGAL[card.status].includes(to)) return;
    if (to === "running") startRun(card.id);
    else if (to === "done") openReview(card.id);
    else moveCard(card.id, to);
  };

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="grid min-h-0 flex-1 grid-rows-[auto_auto_minmax(0,1fr)] overflow-hidden"
    >
      <div className="flex items-center gap-2.5 border-b border-line px-4 py-2.5 dark:border-line-d">
        <span className="text-sm font-normal text-text4 dark:text-text4-d">
          A card moves one column at a time. Anything else is an override, and an override needs a
          reason.
        </span>
        <div className="flex-1" />
        <button
          type="button"
          aria-expanded={adding}
          onClick={() => setAdding((v) => !v)}
          className={cx(CHIP, "flex items-center gap-2 px-3 py-1.5")}
        >
          New card
        </button>
      </div>

      {/* A faixa mantém a sua linha da grelha esteja lá ou não, para o quadro
          não saltar. O `AnimatePresence` é o que dá a saída ao painel — que é
          exactamente o que o CSS não consegue animar. */}
      <div>
        <AnimatePresence>
          {adding && <NewCard close={() => setAdding(false)} />}
        </AnimatePresence>
      </div>

      {/* As colunas chegam uma a seguir à outra — o `.cols` do desenho. */}
      <motion.div
        initial="hidden"
        animate="shown"
        className="grid min-h-0 grid-cols-[repeat(5,minmax(0,1fr))] gap-px overflow-hidden bg-line dark:bg-line-d"
      >
        {STATUS_ORDER.map((status, ci) => {
          const list = cards.filter((c) => c.status === status);
          const color = COLUMN_COLOR[status];
          const canDrop = dragged ? LEGAL[dragged.status].includes(status) : false;
          const hot = over === status && canDrop;
          const took = list.some(
            (c) => arrived.get(c.id) === "forward" || arrived.get(c.id) === "back",
          );
          return (
            <motion.div
              key={status}
              custom={ci}
              variants={colIn}
              onDragOver={(e) => {
                if (!canDrop) return;
                e.preventDefault();
                if (over !== status) setOver(status);
              }}
              onDragLeave={() => setOver((s) => (s === status ? null : s))}
              onDrop={(e) => {
                e.preventDefault();
                drop(status);
              }}
              className={cx(
                "grid min-h-0 grid-rows-[auto_minmax(0,1fr)] transition-colors duration-200",
                hot ? "bg-hovered dark:bg-hovered-d" : "bg-bg dark:bg-bg-d",
                // A coluna que recebeu, para a mudança ser apanhável pelo canto
                // do olho sem estar a olhar para o cartão.
                took && "animate-tookOne dark:animate-tookOneDark",
              )}
            >
              <div className="flex items-center gap-2 px-3.5 pb-2.5 pt-3">
                <span className={cx("h-1.5 w-1.5 rounded-full", color.dot)} />
                <span
                  className={cx("flex-1 text-xs font-semibold tracking-[.08em]", color.fg)}
                >
                  {STATUS_NAME[status].toUpperCase()}
                </span>
                <span className={cx(mono, "text-xs font-medium text-text3 dark:text-text3-d")}>
                  {list.length}
                </span>
              </div>

              {/* Os cartões chegam linha a linha — o `.stagger` do desenho. */}
              <motion.div
                initial="hidden"
                animate="shown"
                className="flex min-h-0 flex-col gap-2 overflow-y-auto px-2.5 pb-2.5"
              >
                {list.map((card, ri) => {
                  const agent = agents.find((a) => a.id === card.agent_id);
                  const t = tone(agent?.tone);
                  const isRun = status === "running";
                  const isReview = status === "review";
                  const isReady = status === "ready";
                  const log = outputs[card.id] ?? [];
                  const stream = streams[card.id];
                  const meta = isRun
                    ? `${plural(card.turns, "turn")} · ${money(card.cost_usd, 2)}`
                    : card.runs > 0
                      ? `${plural(card.turns, "turn")} · ${money(card.cost_usd, 2)}`
                      : `${card.runs} runs`;

                  // One line under the title: the review that sent it back, what
                  // it is doing right now, or nothing.
                  let note = "";
                  let noteColor = "text-text3 dark:text-text3-d";
                  if (isRun) {
                    note =
                      stream?.thinking?.slice(-70) ||
                      (log.length > 0 ? `${log[log.length - 1]!.label} ${log[log.length - 1]!.text}` : "starting…");
                  } else if (card.last_review && status !== "done") {
                    note = `${card.last_review.by === "director" ? "Director" : "You"}: ${card.last_review.reason}`;
                    noteColor = card.last_review.approved
                      ? "text-ok dark:text-ok-d"
                      : "text-warn dark:text-warn-d";
                  } else if (status === "done" && card.last_review) {
                    // The verdict stays legible after Done: who approved and
                    // why — a silent approval is indistinguishable from one
                    // that never ran.
                    note = `${card.last_review.by === "director" ? "Director" : "You"} approved: ${card.last_review.reason}`;
                    noteColor = "text-ok dark:text-ok-d";
                  } else if (status === "done" && card.branch) {
                    note = `${card.branch} still unmerged`;
                  }

                  const moved = arrived.get(card.id);
                  return (
                    // `layout` é a razão de o `motion` estar aqui: o CSS não
                    // anima um elemento que muda de sítio no DOM, e um cartão
                    // que passa de Working para Review faz exactamente isso.
                    //
                    // O arrastar fica no filho de propósito: o `motion` come os
                    // `onDragStart`/`onDragEnd` para o seu próprio gesto e não
                    // os passa ao DOM, o que mataria o arrastar nativo.
                    <motion.div
                      key={card.id}
                      layout
                      custom={moved ? undefined : ri}
                      variants={moved && !still ? arrive(moved === "back") : rowIn}
                    >
                    <div
                      draggable
                      onDragStart={() => setDrag(card.id)}
                      onDragEnd={() => {
                        setDrag(null);
                        setOver(null);
                      }}
                      onClick={() => (isReview ? openReview(card.id) : openRun(card.id))}
                      className={cx(
                        "group flex cursor-grab flex-col gap-2 rounded-md border border-line2 bg-surface p-3 transition-[transform,border-color,background,box-shadow] duration-150 ease-rise dark:border-line2-d dark:bg-surface-d",
                        // Os valores crus que o desenho dá ao `.tile:hover`, os
                        // mesmos nos dois temas — como sempre estiveram.
                        "hover:-translate-y-0.5 hover:border-tileHoverLine hover:bg-tileHover hover:shadow-lift dark:hover:shadow-lift-d",
                        drag === card.id ? "opacity-45" : "opacity-100",
                        // Com movimento reduzido a mudança diz-se com uma
                        // lavagem em vez de uma viagem.
                        moved && still && "bg-accentSoft dark:bg-accentSoft-d",
                      )}
                    >
                      <span
                        title={card.title}
                        className="line-clamp-3 text-md font-medium leading-[1.42] text-text dark:text-text-d"
                      >
                        {card.title}
                      </span>
                      <span className="flex items-center gap-2">
                        <Glyph tone={t} size={16} font={8}>
                          {agent?.initial ?? "?"}
                        </Glyph>
                        {/* O id de um cartão é como se lhe chama ao Director, e
                            ruído o resto do tempo: guarda o lugar e mostra-se
                            quando se olha. */}
                        <span
                          className={cx(
                            mono,
                            "text-xs text-text3 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100 dark:text-text3-d",
                          )}
                        >
                          {card.id}
                        </span>
                        <span className="flex-1" />
                        <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
                          {meta}
                        </span>
                        {!isRun && (
                          <button
                            type="button"
                            title="Delete this card"
                            aria-label={`Delete ${card.title}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              discard(card.id);
                            }}
                            className={cx(
                              mono,
                              "min-h-6 cursor-pointer rounded-sm border-none bg-transparent px-1 text-sm text-text4 transition-colors duration-150 hover:text-text dark:text-text4-d dark:hover:text-text-d",
                            )}
                          >
                            ✕
                          </button>
                        )}
                      </span>

                      {note && (
                        <span
                          className={cx(
                            truncate,
                            "text-xs font-normal leading-normal",
                            noteColor,
                          )}
                        >
                          {note}
                        </span>
                      )}

                      {card.session_id && (
                        <span
                          className={cx(
                            mono,
                            truncate,
                            "flex max-w-full items-center gap-1.5 self-start rounded-sm bg-surface2 px-2 py-px text-xs font-medium text-text3 dark:bg-surface2-d dark:text-text3-d",
                          )}
                        >
                          Start continues session {card.session_id.slice(0, 8)}
                        </span>
                      )}

                      {(isRun || isReview || isReady) && (
                        <span className="flex items-center gap-1.5">
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              if (isRun) cancelRun(card.id);
                              else if (isReview) openReview(card.id);
                              else startRun(card.id);
                            }}
                            className={cx(
                              "min-h-6 flex-1 cursor-pointer rounded-sm border-none p-1.5 text-center text-sm font-semibold transition-[filter] duration-150 hover:brightness-[1.06]",
                              isRun
                                ? "bg-badSoft text-bad dark:bg-badSoft-d dark:text-bad-d"
                                : isReview
                                  ? "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d"
                                  : "bg-infoSoft text-info dark:bg-infoSoft-d dark:text-info-d",
                            )}
                          >
                            {isRun ? "Stop" : isReview ? "Read the diff" : "Start"}
                          </button>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              openRun(card.id);
                            }}
                            className="min-h-6 cursor-pointer rounded-sm border-none bg-transparent px-2.5 py-1.5 text-sm font-medium text-text3 transition-colors duration-150 hover:bg-hovered hover:text-text dark:text-text3-d dark:hover:bg-hovered-d dark:hover:text-text-d"
                          >
                            {isRun ? "Transcript" : isReview ? "Session" : "Log"}
                          </button>
                        </span>
                      )}
                    </div>
                    </motion.div>
                  );
                })}

                {list.length === 0 && (
                  <div className="rounded-md border border-dashed border-line2 px-2 py-3.5 text-center text-sm font-normal text-text4 dark:border-line2-d dark:text-text4-d">
                    {hot ? "Drop here" : EMPTY_COLUMN[status]}
                  </div>
                )}
              </motion.div>
            </motion.div>
          );
        })}
      </motion.div>
    </motion.div>
  );
}
