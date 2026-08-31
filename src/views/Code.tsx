import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "motion/react";
import {
  ArrowUp,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Columns2,
  FileText,
  Folder,
  History,
  MessageCircle,
  MoreHorizontal,
  Plus,
  Search,
  SlidersHorizontal,
  Users,
  X,
} from "lucide-react";
import { api, events, reason as reasonOf, type UnlistenFn } from "../lib/ipc";
import { cx } from "../lib/cx";
import { initials, num, plural, shortAgo, truncate } from "../lib/format";
import { tokenize, type Token } from "../lib/highlight";
import { paneIn, rowIn } from "../lib/motion";
import { MODELS, type ActiveRun, type Hunk, type QueueRow, type TreeEntry } from "../lib/types";
import { useStore } from "../state/store";

/** One asynchronous read, with the three states the handoff names. The error
 *  carries the command's own string: a paraphrase would hide which git call
 *  failed, and that is the only useful part. */
type Read<T> =
  /** There is nothing to read yet — no project picked, no file open. */
  | { at: "idle" }
  | { at: "loading" }
  | { at: "ready"; data: T }
  | { at: "failed"; why: string };

const LOADING = { at: "loading" } as const;
const IDLE = { at: "idle" } as const;

function useRead<T>(load: (() => Promise<T>) | null, deps: unknown[]): [Read<T>, () => void] {
  const [state, setState] = useState<Read<T>>(IDLE);
  const [nonce, setNonce] = useState(0);
  const run = useRef(0);

  useEffect(() => {
    if (!load) {
      run.current += 1;
      setState(IDLE);
      return;
    }
    const mine = ++run.current;
    setState(LOADING);
    load().then(
      (data) => mine === run.current && setState({ at: "ready", data }),
      (e) => mine === run.current && setState({ at: "failed", why: reasonOf(e) }),
    );
    // Keyed on what the caller listed, not on the loader itself: a closure is
    // a new value every render and would re-read the file on each keystroke.
  }, [...deps, nonce]);

  return [state, useCallback(() => setNonce((n) => n + 1), [])];
}

/** A skeleton at the row's final height, so nothing shifts when it arrives. */
function Bars({ rows, className }: { rows: number; className?: string }) {
  return (
    <div className={cx("flex flex-col gap-1.5", className)} aria-hidden>
      {Array.from({ length: rows }, (_, i) => (
        <div
          key={i}
          className="h-[26px] animate-pulse rounded-6px bg-line2 dark:bg-line2-d"
          style={{ width: `${58 + ((i * 37) % 38)}%` }}
        />
      ))}
    </div>
  );
}

function Failed({ why, retry }: { why: string; retry: () => void }) {
  return (
    <div className="flex flex-col items-start gap-2 px-2 py-3">
      <p className="font-mono text-sm leading-relaxed text-bad dark:text-bad-d">{why}</p>
      <button
        type="button"
        onClick={retry}
        className="cursor-pointer rounded-full border border-line bg-transparent px-3 py-1.5 text-sm font-medium text-ink2 transition-colors duration-150 hover:bg-hovered dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d"
      >
        Try again
      </button>
    </div>
  );
}

function Note({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-2 py-3 text-sm leading-relaxed text-faint dark:text-faint-d">{children}</p>
  );
}

// ---- syntax colouring ------------------------------------------------------

/** The colours of one block of source, a line at a time.
 *
 *  `null` at every point where highlighting is not available — while the WASM
 *  and the grammar are still arriving, for a language this app does not carry,
 *  for a file past the highlighter's size cap, and after any failure. The
 *  panes below all render their plain text in that case, so the source is on
 *  screen from the first frame and gains colour when colour is ready. */
function useTokens(code: string | null, lang: string | null, mode: "light" | "dark") {
  const [lines, setLines] = useState<Token[][] | null>(null);
  useEffect(() => {
    setLines(null);
    if (code == null || !lang) return;
    let mine = true;
    void tokenize(code, lang, mode).then((out) => mine && setLines(out));
    return () => {
      mine = false;
    };
  }, [code, lang, mode]);
  return lines;
}

/** One line of source, coloured if there are colours for it.
 *
 *  Shiki's palette arrives per token and reaches the DOM as an inline `color`.
 *  That is the one place the frontend writes a colour outside the Tailwind
 *  tokens, and it is deliberate: a theme's values are data returned by a
 *  call, not a design decision taken in this file, so they cannot live in a
 *  `className`. Everything structural around them — the gutter, the diff fill,
 *  the `+` — stays on tokens. */
function Line({ text, tokens }: { text: string; tokens?: Token[] }) {
  if (!tokens) return <>{text}</>;
  return (
    <>
      {tokens.map((token, i) => (
        <span key={i} style={{ color: token.color }}>
          {token.text}
        </span>
      ))}
    </>
  );
}

/** The section label over a count, both off the same response. */
function Eyebrow({ label, count }: { label: string; count: number }) {
  return (
    <div className="text-xs font-semibold tracking-[.08em] text-faint dark:text-faint-d">
      {label}
      <span className="ml-2 text-muted dark:text-muted-d">{count}</span>
    </div>
  );
}

// ---- the file tree ---------------------------------------------------------

const depthOf = (path: string) => path.split("/").length - 1;

/** Which rows are visible given the folders the operator has closed. */
function visible(entries: TreeEntry[], closed: Set<string>): TreeEntry[] {
  if (closed.size === 0) return entries;
  return entries.filter((e) => {
    for (const dir of closed) {
      if (e.path !== dir && e.path.startsWith(dir + "/")) return false;
    }
    return true;
  });
}

function TreeRow({
  entry,
  open,
  selected,
  onPick,
}: {
  entry: TreeEntry;
  open: boolean;
  selected: boolean;
  onPick: () => void;
}) {
  const dir = entry.kind === "dir";
  const depth = depthOf(entry.path);
  const name = entry.path.slice(entry.path.lastIndexOf("/") + 1);
  // The design indents in 16px steps. A file carries one extra step because it
  // has no chevron in front of it, which is what lines its icon up with the
  // folder icons above it; the 14px margin is where the guide rail sits.
  const pad = dir ? 8 + depth * 16 : depth * 16 + 10;

  return (
    <button
      type="button"
      onClick={onPick}
      aria-current={selected ? "true" : undefined}
      aria-expanded={dir ? open : undefined}
      style={{ paddingLeft: pad }}
      className={cx(
        "flex w-full cursor-pointer items-center gap-1.75 border-none py-1.25 pr-2 text-left text-md transition-colors duration-150",
        dir
          ? "bg-transparent hover:bg-hovered dark:hover:bg-hovered-d"
          : "ml-3.5 rounded-6px",
        !dir &&
          (selected
            ? "border-l-2 border-primary bg-primarySoft font-semibold text-ink dark:border-primary-d dark:bg-primarySoft-d dark:text-ink-d"
            : "border-l border-line3 bg-transparent text-ink2 hover:bg-hovered dark:border-line3-d dark:text-ink2-d dark:hover:bg-hovered-d"),
        dir && (depth === 0 ? "font-semibold text-ink dark:text-ink-d" : "font-medium text-ink2 dark:text-ink2-d"),
      )}
    >
      {dir &&
        (open ? (
          <ChevronDown size={12} strokeWidth={2.4} className="flex-none text-faint" aria-hidden />
        ) : (
          <ChevronRight size={12} strokeWidth={2.4} className="flex-none text-faint" aria-hidden />
        ))}
      {dir ? (
        <Folder size={14} strokeWidth={2.4} className="flex-none text-muted dark:text-muted-d" aria-hidden />
      ) : (
        <FileText
          size={14}
          strokeWidth={2.4}
          className={cx(
            "flex-none",
            selected ? "text-primary dark:text-primary-d" : "text-faint dark:text-faint-d",
          )}
          aria-hidden
        />
      )}
      <span className="min-w-0 flex-1 truncate">{name}</span>
      {entry.dirty && (
        <span
          title="changed in this worktree"
          className="h-1.5 w-1.5 flex-none rounded-full bg-warn dark:bg-warn-d"
        />
      )}
    </button>
  );
}

// ---- the review panel ------------------------------------------------------

/** The dashed circle the design spins beside work in flight. */
function Working() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      className="mt-0.5 flex-none animate-spin-slow text-warn dark:text-warn-d"
      aria-hidden
    >
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray="4 4" />
    </svg>
  );
}

function HunkCard({
  hunk,
  busy,
  decided,
  mode,
  onApprove,
  onReject,
}: {
  hunk: Hunk;
  busy: boolean;
  /** The verdict already recorded against this block, if there is one. */
  decided: boolean | null;
  mode: "light" | "dark";
  onApprove: () => void;
  onReject: () => void;
}) {
  const [open, setOpen] = useState(true);
  const changed = hunk.added + hunk.removed;
  const file = hunk.file.slice(hunk.file.lastIndexOf("/") + 1);
  // The block's own text, without the signs — those are drawn separately, and
  // feeding them to the grammar would colour a diff rather than the code.
  const body = useMemo(() => hunk.lines.map((l) => l.text).join("\n"), [hunk.lines]);
  const tokens = useTokens(body, hunk.lang, mode);

  return (
    <div className="overflow-hidden rounded-10px border border-line dark:border-line-d">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full cursor-pointer items-center gap-2.5 border-none border-b border-line3 bg-transparent px-3 py-2.25 text-left font-mono text-body font-medium text-ink transition-colors duration-150 hover:bg-hovered dark:border-line3-d dark:text-ink-d dark:hover:bg-hovered-d"
      >
        <span className="min-w-0 truncate">{file}</span>
        {hunk.symbol && (
          <span className="min-w-0 truncate text-muted dark:text-muted-d">
            {truncate(hunk.symbol, 28)}
          </span>
        )}
        <ChevronDown
          size={13}
          strokeWidth={2.4}
          className={cx("ml-auto flex-none text-faint", !open && "-rotate-90")}
          aria-hidden
        />
      </button>

      {open && (
        <div className="bg-hovered py-2.5 font-mono text-sm leading-[1.7] dark:bg-hovered-d">
          {hunk.lines.map((line, i) => (
            <div
              key={i}
              className={cx(
                "flex",
                line.sign === "+" && "bg-primarySoft dark:bg-primarySoft-d",
              )}
            >
              <span
                className={cx(
                  "w-9 flex-none border-l-[3px] border-primary pr-3 text-right dark:border-primary-d",
                  line.sign === "+"
                    ? "text-primary dark:text-primary-d"
                    : "text-faint dark:text-faint-d",
                )}
              >
                {i + 1}
              </span>
              <span className="min-w-0 whitespace-pre-wrap break-all text-ink dark:text-ink-d">
                {line.sign === "+" && <span className="text-primary dark:text-primary-d">+</span>}
                {line.sign === "-" && <span className="text-bad dark:text-bad-d">-</span>}
                <Line text={line.text} tokens={tokens?.[i]} />
              </span>
            </div>
          ))}
        </div>
      )}

      <div className="flex items-center gap-2 border-t border-line3 px-3 py-2.5 dark:border-line3-d">
        {/* The verdict already taken on this block, so the operator can see
            what is left to read. Either button still works: deciding a block
            twice replaces the verdict rather than adding one. */}
        <span className="font-mono text-sm font-medium text-muted dark:text-muted-d">
          {plural(changed, "line")} change
          {decided !== null && (decided ? " · approved" : " · sent back")}
        </span>
        <button
          type="button"
          onClick={onReject}
          disabled={busy}
          className="ml-auto cursor-pointer rounded-full border border-line bg-transparent px-4 py-1.75 text-body font-medium text-ink2 transition-colors duration-150 hover:bg-hovered disabled:cursor-not-allowed disabled:opacity-50 dark:border-line-d dark:text-ink2-d dark:hover:bg-hovered-d"
        >
          Reject
        </button>
        <button
          type="button"
          onClick={onApprove}
          disabled={busy}
          className="cursor-pointer rounded-full border-none bg-primary px-4.5 py-1.75 text-body font-bold text-white transition-colors duration-150 hover:bg-primaryDeep disabled:cursor-not-allowed disabled:opacity-50 dark:bg-primary-d"
        >
          Approve
        </button>
      </div>
    </div>
  );
}

// ---- the screen ------------------------------------------------------------

/** The Code screen: a card's worktree on the left, one file of it read-only in
 *  the middle, and what the Director is waiting on you for on the right.
 *
 *  Read-only is the product decision, not a gap: writing through this pane
 *  means write-locking the worktree of an agent that may be mid-run. The
 *  Director applies changes; this approves them. */
export function Code() {
  const { projectId, project, snapshot, agents, settings, sendChat, chatBusy, refresh } =
    useStore();
  // The highlighter needs the theme as a value, not as a class: shiki resolves
  // one palette per call. `applyTheme` treats anything but "light" as dark, so
  // this reads it the same way.
  const mode = settings?.theme === "light" ? "light" : "dark";

  // Which card's worktree is being read. `undefined` means nobody chose, so
  // the screen follows the queue; `null` is the operator stepping back out to
  // the project's own checkout.
  const [picked, setPicked] = useState<string | null | undefined>(undefined);
  const [openFiles, setOpenFiles] = useState<string[]>([]);
  const [path, setPath] = useState<string | null>(null);
  const [closed, setClosed] = useState<Set<string>>(new Set());
  const [surface, setSurface] = useState<"code" | "design">("code");
  const [draft, setDraft] = useState("");
  const [deciding, setDeciding] = useState(false);
  // Bumped by the engine's own broadcast: this screen re-reads, it never polls.
  const [beat, setBeat] = useState(0);
  const pending = useRef<number | null>(null);

  /** Uma batida junta as que vierem logo a seguir.
   *
   *  O `beat` é dependência de cinco leituras — a fila, os runs, a árvore, os
   *  hunks e o ficheiro aberto —, portanto cada batida relê o repositório
   *  inteiro e volta a passar o shiki por cima. Uma rajada de resultados de
   *  ferramentas chega em dezenas de eventos seguidos; sem juntar, são dezenas
   *  de releituras para chegar ao mesmo sítio. */
  const beatSoon = useCallback(() => {
    if (pending.current != null) return;
    pending.current = window.setTimeout(() => {
      pending.current = null;
      setBeat((n) => n + 1);
    }, 250);
  }, []);

  useEffect(() => {
    const subs: Promise<UnlistenFn>[] = [
      events.onEngineEvent((e) => {
        if (e.project_id === projectId) beatSoon();
      }),
      events.onRunUpdate((u) => {
        // Só o que pode ter mexido em ficheiros. Isto reagia a **todos** os
        // eventos de um run, e um `delta` é um token: com um modelo a escrever,
        // o ecrã relia a árvore e o diff dezenas de vezes por segundo e
        // re-realçava tudo de cada vez. Um token não muda um ficheiro; um
        // resultado de ferramenta pode, e o fim do run também.
        if (u.project_id !== projectId) return;
        if (u.kind === "tool_result" || u.kind === "done" || u.kind === "failed") beatSoon();
      }),
    ];
    return () => {
      subs.forEach((s) => void s.then((off) => off()));
      if (pending.current != null) window.clearTimeout(pending.current);
    };
  }, [projectId, beatSoon]);

  const [queue, reloadQueue] = useRead<QueueRow[]>(
    projectId ? () => api.reviewQueue(projectId) : null,
    [projectId, beat],
  );
  const [runs, reloadRuns] = useRead<ActiveRun[]>(
    projectId ? () => api.activeRuns(projectId) : null,
    [projectId, beat],
  );

  const queueRows = queue.at === "ready" ? queue.data : [];
  const runRows = runs.at === "ready" ? runs.data : [];

  // The card the screen reads. Review first — that is what the panel exists
  // for — then whatever is running. Neither means the project's own checkout.
  const cardId =
    picked !== undefined ? picked : (queueRows[0]?.card_id ?? runRows[0]?.card_id ?? null);

  const [tree, reloadTree] = useRead<TreeEntry[]>(
    projectId ? () => api.listTree(projectId, cardId) : null,
    [projectId, cardId, beat],
  );
  const [hunks, reloadHunks] = useRead<Hunk[]>(
    projectId && cardId ? () => api.diffHunks(projectId, cardId) : null,
    [projectId, cardId, beat],
  );
  const [file, reloadFile] = useRead(
    projectId && path ? () => api.readWorktreeFile(projectId, cardId, path) : null,
    [projectId, cardId, path, beat],
  );

  const hunkRows = useMemo(() => (hunks.at === "ready" ? hunks.data : []), [hunks]);
  const treeRows = useMemo(
    () => (tree.at === "ready" ? visible(tree.data, closed) : []),
    [tree, closed],
  );

  // A card's own file is the one worth opening first; without a card there is
  // nothing changed to point at, so the pane waits for a click.
  useEffect(() => {
    setOpenFiles([]);
    setPath(null);
  }, [cardId]);
  useEffect(() => {
    if (path || hunkRows.length === 0) return;
    const first = hunkRows[0]!.file;
    setOpenFiles([first]);
    setPath(first);
  }, [hunkRows, path]);

  const card = snapshot?.cards.find((c) => c.id === cardId) ?? null;
  const session = snapshot?.sessions.find((s) => s.card_id === cardId) ?? null;

  /** The lines of the open file that this card added, so the source pane can
   *  mark them. Numbers come from the diff, never from comparing text here. */
  const addedLines = useMemo(() => {
    const out = new Set<number>();
    for (const hunk of hunkRows) {
      if (hunk.file !== path) continue;
      for (const line of hunk.lines) {
        if (line.sign === "+" && line.new_line != null) out.add(line.new_line);
      }
    }
    return out;
  }, [hunkRows, path]);

  // The open file's text, and its colours. Split once here so the tokens and
  // the lines they belong to are indexed the same way.
  const source = file.at === "ready" && !file.data.binary ? file.data : null;
  const sourceLines = useMemo(() => (source ? source.text.split("\n") : []), [source]);
  const sourceTokens = useTokens(source?.text ?? null, source?.lang ?? null, mode);

  /** The paths this card touched, folded to a chip per top folder. */
  const touched = useMemo(() => {
    const byDir = new Map<string, number>();
    for (const f of new Set(hunkRows.map((h) => h.file))) {
      const cut = f.lastIndexOf("/");
      const dir = cut === -1 ? f : f.slice(0, cut);
      byDir.set(dir, (byDir.get(dir) ?? 0) + 1);
    }
    return [...byDir.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 3)
      .map(([dir, n]) => `${dir} · ${plural(n, "file")}`);
  }, [hunkRows]);

  /** What the engine has already recorded about each block of this diff.
   *  Read off the card, never accumulated here: the verdicts are the engine's
   *  state and this screen only draws them. */
  const verdicts = useMemo(() => {
    const out = new Map<string, boolean>();
    for (const v of card?.hunk_verdicts ?? []) out.set(`${v.hunk.file}${v.hunk.header}`, v.approved);
    return out;
  }, [card]);

  /** One block decided. The card moves — or does not — according to the rule
   *  in `harness_domain`: nothing happens until every block has a verdict,
   *  then the card is approved, sent back, or approved with what was rejected
   *  carried onto a follow-up card. None of that is replayed here. */
  const decide = async (hunk: Hunk, allow: boolean) => {
    if (!projectId || !cardId) return;
    setDeciding(true);
    try {
      await api.reviewHunk(projectId, cardId, hunk.file, hunk.header, allow);
      await refresh();
      reloadHunks();
      reloadQueue();
    } finally {
      setDeciding(false);
    }
  };

  const director = agents.find((a) => a.id === "director") ?? agents[0] ?? null;
  const modelName = director?.model
    ? (MODELS.find((m) => m.id === director.model)?.name ?? director.model)
    : null;

  const send = async () => {
    const text = draft.trim();
    if (!text || chatBusy) return;
    setDraft("");
    await sendChat(text);
  };

  if (!projectId || !project) {
    return (
      <motion.div
        variants={paneIn}
        initial="hidden"
        animate="shown"
        className="flex min-w-0 flex-1 items-center justify-center bg-surface p-11 dark:bg-surface-d"
      >
        <p className="text-sm text-faint dark:text-faint-d">
          Pick a repository in the sidebar and its files, diffs and review queue appear here.
        </p>
      </motion.div>
    );
  }

  // Horizontally the pane scrolls against the 940px canvas floor, as every
  // screen does. Vertically it must not: the three columns are three
  // independent readers — a file tree, a source file and a review queue — and
  // scrolling the page moved all three at once, so reading down a long file
  // dragged the tree out of sight with it.
  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="flex min-w-0 flex-1 overflow-x-auto overflow-y-hidden"
    >
      <div className="flex h-full min-w-[940px] flex-1 bg-surface dark:bg-surface-d">
        {/* ---- the tree ---- */}
        <div className="flex w-[256px] flex-none flex-col border-r border-line dark:border-line-d">
          <div className="flex h-[46px] flex-none items-center gap-2.5 border-b border-line3 px-3.5 dark:border-line3-d">
            <button
              type="button"
              onClick={() => setPicked(null)}
              disabled={cardId === null}
              title="Leave the card's worktree and browse the project itself"
              className="flex cursor-pointer items-center gap-2.5 border-none bg-transparent p-0 text-md font-medium text-ink disabled:cursor-not-allowed disabled:opacity-40 dark:text-ink-d"
            >
              <ChevronLeft size={13} strokeWidth={2.4} className="flex-none text-muted" aria-hidden />
              Back
            </button>
            <div className="ml-auto flex items-center gap-3 text-faint dark:text-faint-d">
              <Search size={14} strokeWidth={2.4} aria-hidden />
              <Columns2 size={14} strokeWidth={2.4} aria-hidden />
              <MoreHorizontal size={15} strokeWidth={3} aria-hidden />
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2.5 text-md text-ink2 dark:text-ink2-d">
            {tree.at === "loading" && <Bars rows={9} className="px-2 py-1" />}
            {tree.at === "failed" && <Failed why={tree.why} retry={reloadTree} />}
            {tree.at === "ready" && tree.data.length === 0 && (
              <Note>
                Nothing is checked out here yet. Start a run on a card and its worktree appears.
              </Note>
            )}
            {tree.at === "ready" &&
              treeRows.map((entry, i) => (
                <motion.div key={entry.path} variants={rowIn} initial="hidden" animate="shown" custom={i}>
                  <TreeRow
                    entry={entry}
                    open={!closed.has(entry.path)}
                    selected={entry.path === path}
                    onPick={() => {
                      if (entry.kind === "dir") {
                        setClosed((was) => {
                          const next = new Set(was);
                          if (!next.delete(entry.path)) next.add(entry.path);
                          return next;
                        });
                        return;
                      }
                      setOpenFiles((was) =>
                        was.includes(entry.path) ? was : [...was, entry.path],
                      );
                      setPath(entry.path);
                      setSurface("code");
                    }}
                  />
                </motion.div>
              ))}
          </div>
        </div>

        {/* ---- the source ---- */}
        <div className="flex min-w-0 flex-1 flex-col border-r border-line dark:border-line-d">
          <div className="flex h-[46px] flex-none items-center gap-1 border-b border-line3 px-3 dark:border-line3-d">
            <button
              type="button"
              onClick={() => setSurface("code")}
              className={cx(
                "cursor-pointer rounded-sm border-none px-3 py-1.5 text-md transition-colors duration-150",
                surface === "code"
                  ? "bg-active font-semibold text-ink dark:bg-active-d dark:text-ink-d"
                  : "bg-transparent font-medium text-muted dark:text-muted-d",
              )}
            >
              Code
            </button>
            <button
              type="button"
              onClick={() => setSurface("design")}
              className={cx(
                "cursor-pointer rounded-sm border-none px-3 py-1.5 text-md transition-colors duration-150",
                surface === "design"
                  ? "bg-active font-semibold text-ink dark:bg-active-d dark:text-ink-d"
                  : "bg-transparent font-medium text-muted dark:text-muted-d",
              )}
            >
              Design
            </button>
            <span className="px-2 text-faint dark:text-faint-d">+</span>
            <div className="ml-auto flex items-center gap-3.5 text-faint dark:text-faint-d">
              <MessageCircle size={15} strokeWidth={2.4} aria-hidden />
              <History size={15} strokeWidth={2.4} aria-hidden />
            </div>
          </div>

          <div className="flex h-[42px] flex-none items-center gap-0.5 border-b border-line3 px-3 text-md font-medium text-muted dark:border-line3-d dark:text-muted-d">
            {openFiles.length === 0 && (
              <span className="px-2.5 text-sm text-faint dark:text-faint-d">No file open</span>
            )}
            {openFiles.map((open) => {
              const name = open.slice(open.lastIndexOf("/") + 1);
              const on = open === path;
              return (
                <span
                  key={open}
                  className={cx(
                    "flex items-center gap-2 rounded-sm px-2.5 py-1.5",
                    on && "bg-active font-semibold text-ink dark:bg-active-d dark:text-ink-d",
                  )}
                >
                  <button
                    type="button"
                    onClick={() => {
                      setPath(open);
                      setSurface("code");
                    }}
                    className="cursor-pointer border-none bg-transparent p-0 text-inherit"
                  >
                    {name}
                  </button>
                  {on && (
                    <button
                      type="button"
                      aria-label={`Close ${name}`}
                      onClick={() => {
                        const rest = openFiles.filter((f) => f !== open);
                        setOpenFiles(rest);
                        setPath(rest[rest.length - 1] ?? null);
                      }}
                      className="flex cursor-pointer border-none bg-transparent p-0 text-faint dark:text-faint-d"
                    >
                      <X size={12} strokeWidth={3} aria-hidden />
                    </button>
                  )}
                </span>
              );
            })}
            <div className="ml-auto flex items-center gap-3 text-faint dark:text-faint-d">
              <Columns2 size={14} strokeWidth={2.4} aria-hidden />
              <MoreHorizontal size={15} strokeWidth={3} aria-hidden />
            </div>
          </div>

          {/* The source itself. Colour comes from `shiki`, off `FileText.lang`
              and lazily — see `src/lib/highlight.ts`. It is layered under the
              diff marking rather than replacing it: the fill, the gutter and
              the `+` are still drawn from `diff_hunks`, and a line that has no
              tokens (or a file the highlighter cannot read) prints as it
              always did. */}
          <div className="min-h-0 flex-1 overflow-auto py-3.5 font-mono text-md leading-[1.75] text-ink dark:text-ink-d">
            {surface === "design" ? (
              <Note>
                No design surface is recorded for this project. Nothing in the engine produces one
                yet.
              </Note>
            ) : !path ? (
              <Note>Pick a file on the left to read it. The pane is read-only.</Note>
            ) : file.at === "failed" ? (
              <Failed why={file.why} retry={reloadFile} />
            ) : file.at !== "ready" ? (
              <Bars rows={14} className="px-4" />
            ) : file.data.binary ? (
              <Note>
                {file.data.path} is binary — {num(file.data.size)} bytes. Nothing to read here.
              </Note>
            ) : (
              <>
                {sourceLines.map((line, i) => {
                  const at = i + 1;
                  const added = addedLines.has(at);
                  return (
                    <div
                      key={at}
                      className={cx("flex", added && "bg-primarySoft dark:bg-primarySoft-d")}
                    >
                      <span
                        className={cx(
                          "w-[52px] flex-none pr-4 text-right",
                          added
                            ? "text-primary dark:text-primary-d"
                            : "text-faint dark:text-faint-d",
                        )}
                      >
                        {at}
                      </span>
                      <span className="min-w-0 whitespace-pre-wrap break-all pr-4">
                        {added && <span className="text-primary dark:text-primary-d">+ </span>}
                        <Line text={line} tokens={sourceTokens?.[i]} />
                      </span>
                    </div>
                  );
                })}
                {file.data.truncated && (
                  <Note>
                    Cut at 1 MB. The rest of {file.data.path} is on disk but not in this pane.
                  </Note>
                )}
              </>
            )}
          </div>
        </div>

        {/* ---- the Director's panel ---- */}
        <div className="flex w-[380px] flex-none flex-col bg-surface dark:bg-surface-d">
          <div className="flex-none border-b border-line3 px-4 pb-3 pt-4 dark:border-line3-d">
            <div className="flex items-start gap-2.5">
              <span className="grid h-[38px] w-[38px] flex-none place-items-center rounded-9px bg-ink text-body font-bold text-white dark:bg-ink-d dark:text-canvas-d">
                {initials(project.name)}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="truncate text-sheet font-bold text-ink dark:text-ink-d">
                    {project.name}
                  </span>
                  <span
                    className={cx(
                      "rounded-full px-2.25 py-0.5 text-11 font-bold",
                      runRows.length > 0
                        ? "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d"
                        : "bg-active text-muted dark:bg-active-d dark:text-muted-d",
                    )}
                  >
                    {runRows.length > 0 ? "Active" : "Idle"}
                  </span>
                  {queueRows.length > 0 && (
                    <span className="rounded-full bg-warnSoft px-2.25 py-0.5 text-11 font-bold text-warn dark:bg-warnSoft-d dark:text-warn-d">
                      {plural(queueRows.length, "card")} {queueRows.length === 1 ? "needs" : "need"}{" "}
                      approval
                    </span>
                  )}
                </div>
                <div className="mt-1.5 flex gap-3.5 text-sm font-medium text-muted dark:text-muted-d">
                  <span className="flex items-center gap-1.25">
                    <SlidersHorizontal size={13} strokeWidth={2.4} aria-hidden />
                    Settings
                  </span>
                  <span className="flex items-center gap-1.25">
                    <Users size={13} strokeWidth={2.4} aria-hidden />
                    {plural(agents.length, "member")}
                  </span>
                </div>
              </div>
              <div className="flex flex-none items-center gap-2.5 text-faint dark:text-faint-d">
                <History size={15} strokeWidth={2.4} aria-hidden />
                <MoreHorizontal size={15} strokeWidth={3} aria-hidden />
              </div>
            </div>

            <div className="mt-2.5 flex flex-wrap gap-1.5">
              {touched.map((chip) => (
                <span
                  key={chip}
                  className="rounded-6px bg-active px-2.25 py-[3px] text-11 font-medium text-ink2 dark:bg-active-d dark:text-ink2-d"
                >
                  {chip}
                </span>
              ))}
              {session && (
                <span className="rounded-6px bg-active px-2.25 py-[3px] font-mono text-11 font-medium text-ink2 dark:bg-active-d dark:text-ink2-d">
                  worktree {session.worktree.slice(session.worktree.lastIndexOf("/") + 1)}
                </span>
              )}
              {!session && cardId === null && (
                <span className="rounded-6px bg-active px-2.25 py-[3px] font-mono text-11 font-medium text-ink2 dark:bg-active-d dark:text-ink2-d">
                  {project.base_branch}
                </span>
              )}
            </div>
          </div>

          <div className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto px-4 py-3.5">
            <div>
              <Eyebrow label="IN PROGRESS" count={runRows.length} />
              {runs.at === "loading" && <Bars rows={1} className="mt-2.25" />}
              {runs.at === "failed" && <Failed why={runs.why} retry={reloadRuns} />}
              {runs.at === "ready" && runRows.length === 0 && (
                <p className="mt-2.25 text-sm text-faint dark:text-faint-d">
                  No agent is running. Start a card on the Board and it appears here.
                </p>
              )}
              {runRows.map((run) => {
                const on = snapshot?.cards.find((c) => c.id === run.card_id);
                const who = agents.find((a) => a.id === run.agent_id);
                return (
                  <button
                    key={run.run_id}
                    type="button"
                    onClick={() => setPicked(run.card_id)}
                    className="mt-2.25 flex w-full cursor-pointer gap-2.5 border-none bg-transparent p-0 text-left"
                  >
                    <Working />
                    <span className="min-w-0">
                      <span className="block truncate text-md font-semibold text-ink dark:text-ink-d">
                        {on?.title ?? run.card_id}
                      </span>
                      <span className="mt-0.5 block text-sm text-muted dark:text-muted-d">
                        {who?.name ?? run.agent_id} · started {shortAgo(run.started_ms)} ago
                      </span>
                    </span>
                  </button>
                );
              })}
            </div>

            <div>
              <Eyebrow label="NEEDS YOUR REVIEW" count={queueRows.length} />
              {queue.at === "loading" && <Bars rows={1} className="mt-2.25" />}
              {queue.at === "failed" && <Failed why={queue.why} retry={reloadQueue} />}
              {queue.at === "ready" && queueRows.length === 0 && (
                <p className="mt-2.25 text-sm text-faint dark:text-faint-d">
                  Nothing is waiting on you. Finished runs land here.
                </p>
              )}
              {queueRows.map((row) => (
                <button
                  key={row.card_id}
                  type="button"
                  onClick={() => setPicked(row.card_id)}
                  className="mt-2.25 flex w-full cursor-pointer gap-2.5 border-none bg-transparent p-0 text-left"
                >
                  <span className="mt-0.5 grid h-4 w-4 flex-none place-items-center rounded-full border-[1.5px] border-warn text-10 font-bold text-warn dark:border-warn-d dark:text-warn-d">
                    !
                  </span>
                  <span className="min-w-0">
                    <span className="block truncate text-md font-semibold text-ink dark:text-ink-d">
                      {row.title}
                    </span>
                    <span className="mt-0.5 block text-sm text-muted dark:text-muted-d">
                      Waiting on your review
                    </span>
                  </span>
                </button>
              ))}
            </div>

            {hunks.at === "loading" && (
              <div className="rounded-10px border border-line p-3 dark:border-line-d">
                <Bars rows={5} />
              </div>
            )}
            {hunks.at === "failed" && <Failed why={hunks.why} retry={reloadHunks} />}
            {hunks.at === "ready" && hunkRows.length === 0 && cardId && (
              <p className="text-sm text-faint dark:text-faint-d">
                {card?.title ?? cardId} has changed nothing against {project.base_branch} yet.
              </p>
            )}
            {hunkRows.map((hunk) => (
              <HunkCard
                key={`${hunk.file}${hunk.header}`}
                hunk={hunk}
                busy={deciding}
                decided={verdicts.get(`${hunk.file}${hunk.header}`) ?? null}
                mode={mode}
                onApprove={() => void decide(hunk, true)}
                onReject={() => void decide(hunk, false)}
              />
            ))}
          </div>

          <div className="flex-none px-4 pb-3.5 pt-3">
            <div className="rounded-sheet border-[1.5px] border-primaryLine px-3.5 py-3 shadow-hunk dark:border-primaryLine-d">
              <textarea
                rows={1}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send();
                  }
                }}
                placeholder="How can I help you?"
                className="w-full resize-none border-none bg-transparent text-base text-ink outline-none placeholder:text-faint dark:text-ink-d dark:placeholder:text-faint-d"
              />
              <div className="mt-3 flex items-center gap-2">
                <span className="grid h-6.5 w-6.5 place-items-center rounded-full border border-line text-muted dark:border-line-d dark:text-muted-d">
                  <Plus size={13} strokeWidth={3} aria-hidden />
                </span>
                {modelName && (
                  <span className="flex items-center gap-1.25 rounded-sheet border border-line px-3 py-1 text-body font-medium text-ink2 dark:border-line-d dark:text-ink2-d">
                    {modelName}
                    <ChevronDown size={11} strokeWidth={2.4} aria-hidden />
                  </span>
                )}
                {director && (
                  <span className="flex items-center gap-1.25 rounded-sheet border border-line px-3 py-1 text-body font-medium text-ink2 dark:border-line-d dark:text-ink2-d">
                    {director.name}
                    <ChevronDown size={11} strokeWidth={2.4} aria-hidden />
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => void send()}
                  disabled={!draft.trim() || chatBusy}
                  aria-label="Send"
                  className="ml-auto grid h-6.5 w-6.5 cursor-pointer place-items-center rounded-full border-none bg-active text-muted transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-active-d dark:text-muted-d"
                >
                  <ArrowUp size={13} strokeWidth={2.7} aria-hidden />
                </button>
              </div>
            </div>
            <p className="mt-2.25 text-center text-11 text-faint dark:text-faint-d">
              Agents can make mistakes — nothing is applied until you approve it
            </p>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
