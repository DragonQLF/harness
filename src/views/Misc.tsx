/** Worktrees, Activity and Settings. The Director now lives on the chat
 *  screen, so nothing here needs a page of its own. */

import { Fragment, useEffect, useState, type ReactNode } from "react";
import { api, events, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { ago, clock, money } from "../lib/format";
import { ruleIsRevoked, ruleLabel, TONE, type Provider, type WorktreeRow } from "../lib/types";
import { useStore } from "../state/store";
import { checkForUpdate, useAppVersion, useUpdater } from "../components/Updater";
import { Loading, Switch, mono, tabular, truncate } from "../components/ui";

/** O painel que estes três ecrãs repetem: linha de 1px, raio 20, superfície. */
const PANEL = "overflow-hidden rounded-xl border border-line bg-surface dark:border-line-d dark:bg-surface-d";

/** Uma linha de lista que responde ao ponteiro. */
const HOVER_ROW = "transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d";

/** Um botão de contorno discreto. */
const QUIET =
  "min-h-6 cursor-pointer rounded-full border border-line bg-transparent font-semibold text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text focus-visible:bg-hovered focus-visible:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

/** O mesmo botão quando desfaz alguma coisa. */
const DANGER =
  "min-h-6 cursor-pointer rounded-full border border-line bg-transparent font-semibold text-text3 transition-colors duration-150 hover:border-transparent hover:bg-badSoft hover:text-bad focus-visible:border-transparent focus-visible:bg-badSoft focus-visible:text-bad dark:border-line-d dark:text-text3-d dark:hover:bg-badSoft-d dark:hover:text-bad-d";

/** Uma pastilha numa fila de escolhas. */
const CHOICE =
  "min-h-6 cursor-pointer rounded-full border-none transition-colors duration-150";
const CHOICE_ON = "bg-accent font-bold text-onAccent dark:bg-accent-d dark:text-onAccent-d";
const CHOICE_OFF =
  "bg-transparent font-medium text-text2 hover:bg-hovered hover:text-text dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

export function Worktrees() {
  const { projectId, project, snapshot, toast } = useStore();
  const [rows, setRows] = useState<WorktreeRow[] | null>(null);

  const load = () => {
    if (!projectId) return;
    api
      .worktrees(projectId)
      .then(setRows)
      .catch((e) => toast("bad", "Could not list worktrees", reason(e)));
  };

  useEffect(load, [projectId]);

  if (!project) {
    return (
      <div className="px-6.5 py-5.5 text-md text-text3 dark:text-text3-d">
        Add a git repository first.
      </div>
    );
  }
  if (!rows) return <Loading what="Listing worktrees" />;

  const grid = "grid grid-cols-[1.5fr_1fr_90px_1.4fr_150px] gap-3.5";
  const cardFor = (branch: string | null) => {
    const id = branch?.split("/").slice(-1)[0] ?? "";
    return snapshot?.cards.find((c) => c.id === id) ?? null;
  };

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <p className="mb-4 mt-0 text-md text-text2 dark:text-text2-d">
        One branch per card, created under app data. Finished runs commit themselves and leave a
        trailer pointing back at the card.
      </p>
      <div className={PANEL}>
        <div
          className={cx(
            grid,
            "border-b border-line px-4.5 py-3 text-sm font-bold uppercase tracking-[.08em] text-text3 dark:border-line-d dark:text-text3-d",
          )}
        >
          <span>Branch</span>
          <span>Card</span>
          <span>State</span>
          <span>Path</span>
          <span />
        </div>
        {rows.map((w) => {
          const card = cardFor(w.branch);
          const st = w.dirty
            ? { label: "dirty", tone: TONE.accent }
            : { label: "clean", tone: TONE.ok };
          return (
            <div
              key={w.path}
              className={cx(
                grid,
                HOVER_ROW,
                "items-center border-b border-line2 px-4.5 py-3 dark:border-line2-d",
              )}
            >
              <span className={cx(truncate, "font-mono text-md font-medium")}>
                {w.branch ?? "(detached)"}
              </span>
              <span
                title={card?.title}
                className={cx(truncate, "font-mono text-sm text-text3 dark:text-text3-d")}
              >
                {card?.id ?? "—"}
              </span>
              <span
                className={cx(
                  "justify-self-start rounded-full px-2.5 py-1 text-sm font-bold",
                  st.tone.soft,
                  st.tone.fg,
                )}
              >
                {st.label}
              </span>
              <span title={w.path} className={cx(truncate, "text-md text-text2 dark:text-text2-d")}>
                {w.path}
              </span>
              <span className="flex justify-self-end gap-1.5">
                <button
                  type="button"
                  onClick={() => api.reveal(w.path).catch(() => {})}
                  className={cx(QUIET, "px-3.5 py-1.5 text-sm")}
                >
                  Open
                </button>
                <button
                  type="button"
                  onClick={() => {
                    if (!projectId) return;
                    api
                      .removeWorktree(projectId, w.path)
                      .then(() => {
                        toast("ok", "Removed", w.branch ?? w.path);
                        load();
                      })
                      .catch((e) => toast("bad", "Could not remove it", reason(e)));
                  }}
                  className={cx(DANGER, "px-3.5 py-1.5 text-sm")}
                >
                  Drop
                </button>
              </span>
            </div>
          );
        })}
        {rows.length === 0 && (
          <div className="px-4.5 py-5.5 text-center text-md text-text3 dark:text-text3-d">
            No worktrees in this project. Agents open one the moment a card starts.
          </div>
        )}
      </div>
    </div>
  );
}

const FILTERS = ["All", "Cards", "Runs", "Reviews"] as const;

export function Activity({ openRun }: { openRun: (cardId: string) => void }) {
  const { activity, snapshot, project } = useStore();
  const [filter, setFilter] = useState<(typeof FILTERS)[number]>("All");

  if (!project) {
    return (
      <div className="px-6.5 py-5.5 text-md text-text3 dark:text-text3-d">
        Add a git repository first.
      </div>
    );
  }

  const rows = activity.filter((r) =>
    filter === "All"
      ? true
      : filter === "Cards"
        ? r.kind === "card"
        : filter === "Runs"
          ? r.kind === "run"
          : r.kind === "review" || r.kind === "approval",
  );

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <div className="mb-3.5 flex items-center gap-3.5">
        {/* The chrome above already names the screen and what it lists. Every
            other view leaves the heading to it; this one said it twice. */}
        <div className="flex-1" />
        <div className="flex gap-0.5 rounded-full border border-line bg-surface p-1 dark:border-line-d dark:bg-surface-d">
          {FILTERS.map((f) => {
            const on = filter === f;
            return (
              <button
                key={f}
                type="button"
                aria-pressed={on}
                onClick={() => setFilter(f)}
                className={cx(CHOICE, "px-4 py-2 text-md", on ? CHOICE_ON : CHOICE_OFF)}
              >
                {f}
              </button>
            );
          })}
        </div>
      </div>

      <div className={PANEL}>
        {rows.map((e, i) => {
          // Events written before the envelope carried a timestamp deserialize
          // to zero. Dating them 1 January 1970 is a confident wrong answer;
          // saying they predate the record is the true one.
          const undated = !e.ts_ms;
          const day = undated ? "undated" : new Date(e.ts_ms).toDateString();
          const prev = rows[i - 1];
          const prevDay = !prev ? null : !prev.ts_ms ? "undated" : new Date(prev.ts_ms).toDateString();
          const fresh = day !== prevDay;
          const today = new Date().toDateString();
          const dot =
            e.kind === "run"
              ? TONE.accent
              : e.kind === "approval"
                ? TONE.warn
                : e.kind === "review"
                  ? TONE.ok
                  : TONE.info;
          return (
            <Fragment key={e.seq}>
              {fresh && (
                <div className="border-b border-line2 bg-recess px-4.5 pb-2 pt-2.5 text-xs font-semibold tracking-[.08em] text-text4 dark:border-line2-d dark:bg-recess-d dark:text-text4-d">
                  {undated
                    ? "BEFORE TIMES WERE RECORDED"
                    : day === today
                      ? "TODAY"
                      : new Date(e.ts_ms)
                          .toLocaleDateString(undefined, { day: "numeric", month: "long" })
                          .toUpperCase()}
                </div>
              )}
              <button
                key={e.seq}
                type="button"
                onClick={() => openRun(e.card_id)}
                className={cx(
                  HOVER_ROW,
                  "grid w-full animate-[fadeIn_.25s_ease_both] cursor-pointer grid-cols-[14px_190px_74px_1fr_60px] items-center gap-3.5 border-b border-line2 bg-transparent px-4.5 py-3 text-left text-md text-text dark:border-line2-d dark:text-text-d",
                )}
              >
                <span className={cx("h-1.75 w-1.75 rounded-full", dot.solid)} />
                <span className={cx(truncate, "font-semibold")}>{e.label}</span>
                <span
                  title={e.card_id}
                  className={cx(truncate, "font-mono text-sm text-text3 dark:text-text3-d")}
                >
                  {e.card_id}
                </span>
                <span className={cx(truncate, "text-text2 dark:text-text2-d")}>
                  {e.detail || snapshot?.cards.find((c) => c.id === e.card_id)?.title || ""}
                </span>
                <span
                  className={cx(tabular, "text-right text-sm text-text3 dark:text-text3-d")}
                >
                  {undated ? "—" : clock(e.ts_ms)}
                </span>
              </button>
            </Fragment>
          );
        })}
        {rows.length === 0 && (
          <div className="px-4.5 py-5.5 text-center text-md text-text3 dark:text-text3-d">
            Nothing logged yet. Every card created, moved, run or reviewed in this
            project lands here, newest first.
          </div>
        )}
      </div>
    </div>
  );
}

function Row({
  name,
  note,
  children,
  last,
}: {
  name: string;
  note: string;
  children: ReactNode;
  last?: boolean;
}) {
  return (
    <div
      className={cx(
        "flex items-center justify-between gap-5.5 p-4.5",
        !last && "border-b border-line2 dark:border-line2-d",
      )}
    >
      <div className="min-w-0">
        <div className="mb-1 text-lg font-bold">{name}</div>
        <div className="text-md leading-normal text-text3 dark:text-text3-d">{note}</div>
      </div>
      <div className="flex-none">{children}</div>
    </div>
  );
}

/** The accent choices, drawn from the palette rather than from four loose
 *  hexes. The first follows the theme's own accent, which is what an operator
 *  who never opened this row is already using. */
const ACCENTS: { name: string; value: string; swatch: string | null }[] = [
  // Sem amostra própria: segue o acento do tema, seja ele qual for.
  { name: "Theme default", value: "", swatch: null },
  { name: "Mint", value: "#4fd1a5", swatch: "#4fd1a5" },
  { name: "Periwinkle", value: "#9b8cff", swatch: "#9b8cff" },
  { name: "Amber", value: "#ffb35c", swatch: "#ffb35c" },
  { name: "Rose", value: "#ff6b81", swatch: "#ff6b81" },
];

/** Starting points for a model endpoint. Both speak the Anthropic Messages
 *  protocol, which is the only reason this works without a translation proxy:
 *  Ollama serves one on localhost, OpenRouter serves one over the wire and
 *  forwards to whoever actually holds the model. Choosing one fills the form;
 *  it installs nothing. */
const PROVIDER_TEMPLATES: { id: string; name: string; base_url: string; token: string; hint: string }[] = [
  {
    id: "ollama",
    name: "Ollama (local)",
    base_url: "http://localhost:11434",
    token: "ollama",
    hint: "Runs on this machine. No cost, nothing leaves the box. Give it a model with 64k+ context or it cannot hold a repository.",
  },
  {
    id: "ollama-cloud",
    name: "Ollama Cloud",
    base_url: "https://ollama.com",
    token: "",
    hint: "Ollama's hosted models, no local daemon. Needs a key from ollama.com/settings/keys. Every model it offers can call tools and holds at least 64k.",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    base_url: "https://openrouter.ai/api",
    token: "",
    hint: "One key, most models. Tool calls and thinking pass through, which an agent needs to work a card.",
  },
];

const PROVIDER_INPUT =
  "rounded-sm border border-line3 bg-surface2 px-2.5 py-2 font-mono text-sm text-text outline-none focus-visible:border-accentLine dark:border-line3-d dark:bg-surface2-d dark:text-text-d dark:focus-visible:border-accentLine-d";

/** O painel de definições traz margem por baixo; o último não. */
const SECTION = cx(PANEL, "mb-3");

/** Where the updater lives when there is nothing to say — the design's own
 *  words for it. The version, whatever the feed last answered and the one
 *  toggle; the four sheets take over the moment there is a release. */
function UpdateRow() {
  const { settings, saveSettings } = useStore();
  const version = useAppVersion();
  const { release, stage, checking, checkedMs, error } = useUpdater();

  const state = error
    ? `could not check · ${error}`
    : checking
      ? "checking…"
      : release && stage !== "none"
        ? `${release.version} ready to install`
        : checkedMs
          ? `up to date · checked ${ago(checkedMs)}`
          : "not checked yet";

  return (
    <div className={cx(PANEL, "mb-3")}>
      <div className="flex items-center gap-3.5 p-4.5">
        <img src="/relay.svg" alt="" width={30} height={30} className="flex-none" />
        <div className="min-w-0">
          <div className="text-base font-semibold">Relay {version ?? "—"}</div>
          <div
            title={error ?? undefined}
            className={cx(mono, truncate, "mt-0.5 text-11 text-text3 dark:text-text3-d")}
          >
            {state}
          </div>
        </div>
        <div className="ml-auto flex items-center gap-4">
          <span className="flex items-center gap-2.25 text-body text-text2 dark:text-text2-d">
            Install updates automatically
            <Switch
              on={settings?.auto_install_updates ?? false}
              onChange={(v) => saveSettings({ auto_install_updates: v })}
              label="Install updates automatically"
            />
          </span>
          <button
            type="button"
            disabled={checking}
            onClick={() => checkForUpdate()}
            className={cx(QUIET, "px-4 py-2 text-body disabled:cursor-default disabled:opacity-60")}
          >
            {checking ? "Checking…" : "Check now"}
          </button>
        </div>
      </div>
    </div>
  );
}

export function Settings() {
  const { settings, status, dataDir, saveSettings, installSidecar, toast, projects, refreshProjects } =
    useStore();
  const [log, setLog] = useState<string[]>([]);
  const [fetchingRelay, setFetchingRelay] = useState(false);
  const mirror = projects.find((p) => p.mirror);

  const updateProvider = (id: string, patch: Partial<Provider>) =>
    saveSettings({
      providers: (settings?.providers ?? []).map((p) => (p.id === id ? { ...p, ...patch } : p)),
    });

  useEffect(() => {
    let un: (() => void) | null = null;
    events.onSidecarLog((line) => setLog((l) => [...l, line].slice(-6))).then((u) => {
      un = u;
    });
    return () => un?.();
  }, []);

  if (!settings) return <Loading what="Reading settings" />;

  const pillRow = (options: string[], value: string, pick: (v: string) => void, wide?: boolean) => (
    <div className="flex gap-0.5 rounded-full border border-line bg-surface2 p-1 dark:border-line-d dark:bg-surface2-d">
      {options.map((o) => {
        const on = value === o;
        return (
          <button
            key={o}
            type="button"
            aria-pressed={on}
            onClick={() => pick(o)}
            className={cx(
              CHOICE,
              "py-2 text-md",
              wide ? "px-4.5" : "px-3.25",
              on ? CHOICE_ON : CHOICE_OFF,
            )}
          >
            {o}
          </button>
        );
      })}
    </div>
  );

  return (
    <div className="max-w-[880px] px-6.5 pb-7 pt-5.5">
      <p className="mb-4 mt-0 text-md text-text2 dark:text-text2-d">
        Applies to new runs. Anything already running keeps the profile it started with.
      </p>

      <div className={SECTION}>
        <Row name="Appearance" note="Light by day, dark for late sessions" last>
          <div className="flex items-center gap-2.5">
            {ACCENTS.map((a) => {
              const picked = settings.accent === a.value;
              return (
                <button
                  key={a.value || "theme"}
                  type="button"
                  title={a.name}
                  aria-label={a.name}
                  aria-pressed={picked}
                  onClick={() => saveSettings({ accent: a.value })}
                  className={cx(
                    "h-6 w-6 cursor-pointer rounded-full p-0 transition-[box-shadow,border-color] duration-150",
                    a.swatch ? "" : "bg-accent dark:bg-accent-d",
                    picked
                      ? "border-2 border-text ring-[3px] ring-accentSoft dark:border-text-d dark:ring-accentSoft-d"
                      : "border border-line3 dark:border-line3-d",
                  )}
                  style={a.swatch ? { background: a.swatch } : undefined}
                />
              );
            })}
            {pillRow(["light", "dark"], settings.theme, (v) => saveSettings({ theme: v }), true)}
          </div>
        </Row>
      </div>

      <div className={SECTION}>
        <Row
          name="Work on Relay itself"
          note="Relay can be given cards like anything else it works on. This finds its source on this machine, or fetches it if this machine has not got it."
          last
        >
          {mirror ? (
            <span className={cx(mono, "text-sm text-ok dark:text-ok-d")}>on · {mirror.path}</span>
          ) : (
            <button
              type="button"
              disabled={fetchingRelay}
              onClick={async () => {
                setFetchingRelay(true);
                try {
                  await api.mirrorSetup();
                  await refreshProjects();
                } catch (e) {
                  toast("bad", "Could not set Relay up", reason(e));
                } finally {
                  setFetchingRelay(false);
                }
              }}
              className="min-h-6 cursor-pointer rounded-sm border border-line3 bg-surface2 px-3.5 py-2 text-md font-medium text-text2 transition-colors duration-150 hover:border-line4 hover:text-text disabled:cursor-default disabled:opacity-60 dark:border-line3-d dark:bg-surface2-d dark:text-text2-d dark:hover:border-line4-d dark:hover:text-text-d"
            >
              {fetchingRelay ? "fetching…" : "Set it up"}
            </button>
          )}
        </Row>
      </div>

      <div className={SECTION}>
        <Row
          name="Model endpoints"
          note="Where agents run. Anything speaking the Anthropic Messages protocol works — an agent profile picks one, so a local model can do the work while a hosted one reviews it."
          last={(settings.providers ?? []).length === 0}
        >
          <div className="flex gap-1.5">
            {PROVIDER_TEMPLATES.filter(
              (t) => !(settings.providers ?? []).some((p) => p.id === t.id),
            ).map((t) => (
              <button
                key={t.id}
                type="button"
                title={t.hint}
                onClick={() =>
                  saveSettings({
                    providers: [
                      ...(settings.providers ?? []),
                      { id: t.id, name: t.name, base_url: t.base_url, token: t.token },
                    ],
                  })
                }
                className="min-h-6 cursor-pointer rounded-sm border border-line3 bg-surface2 px-3 py-1.5 text-sm font-medium text-text2 transition-colors duration-150 hover:border-line4 hover:text-text dark:border-line3-d dark:bg-surface2-d dark:text-text2-d dark:hover:border-line4-d dark:hover:text-text-d"
              >
                + {t.name}
              </button>
            ))}
          </div>
        </Row>

        {(settings.providers ?? []).map((provider, i, all) => (
          <Row
            key={provider.id}
            name={provider.name}
            note={provider.id === "ollama" ? "Nothing leaves this machine." : "Sent over the wire."}
            last={i === all.length - 1}
          >
            <div className="flex items-center gap-1.5">
              <input
                defaultValue={provider.base_url}
                placeholder="http://localhost:11434"
                spellCheck={false}
                aria-label={`${provider.name} base URL`}
                onBlur={(e) => updateProvider(provider.id, { base_url: e.target.value.trim() })}
                className={cx(PROVIDER_INPUT, "w-[180px]")}
              />
              <input
                type="password"
                defaultValue={provider.token}
                placeholder="key"
                spellCheck={false}
                aria-label={`${provider.name} key`}
                onBlur={(e) => updateProvider(provider.id, { token: e.target.value.trim() })}
                className={cx(PROVIDER_INPUT, "w-[120px]")}
              />
              <button
                type="button"
                title={`Remove ${provider.name}`}
                onClick={() =>
                  saveSettings({
                    providers: (settings.providers ?? []).filter((p) => p.id !== provider.id),
                  })
                }
                className="min-h-6 cursor-pointer rounded-sm border border-line3 bg-transparent px-2.5 py-1.5 text-sm font-medium text-text4 transition-colors duration-150 hover:border-transparent hover:bg-badSoft hover:text-bad dark:border-line3-d dark:text-text4-d dark:hover:bg-badSoft-d dark:hover:text-bad-d"
              >
                Remove
              </button>
            </div>
          </Row>
        ))}
      </div>

      <div className={SECTION}>
        <Row
          name="Node sidecar"
          note="Runs agents through the Claude Agent SDK. Off falls back to the claude command line."
        >
          <Switch
            on={settings.sidecar}
            onChange={(v) => saveSettings({ sidecar: v })}
            label="Node sidecar"
          />
        </Row>
        <Row name="Director reviews first" note="Reads every finished diff before it reaches you">
          <Switch
            on={settings.director_reviews_first}
            onChange={(v) => saveSettings({ director_reviews_first: v })}
            label="Director reviews first"
          />
        </Row>
        <Row
          name="Commit on close"
          note="Waits for running agents to commit work in progress before quitting"
        >
          <Switch
            on={settings.commit_wip_on_close}
            onChange={(v) => saveSettings({ commit_wip_on_close: v })}
            label="Commit on close"
          />
        </Row>
        <Row name="Permission mode" note="The default for new runs; an agent profile can override it">
          {pillRow(["acceptEdits", "manual", "dontAsk", "plan"], settings.permission_mode, (v) =>
            saveSettings({ permission_mode: v }),
          )}
        </Row>
        <Row name="Daily budget" note="Across every agent in this workspace" last>
          <div className="flex items-center gap-2">
            <button
              type="button"
              aria-label="Lower the daily budget"
              onClick={() =>
                saveSettings({ daily_budget_usd: Math.max(0, settings.daily_budget_usd - 1) })
              }
              className="h-[30px] w-[30px] cursor-pointer rounded-md border border-line bg-transparent text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d"
            >
              −
            </button>
            <span className={cx(tabular, "min-w-[66px] text-center text-[20px] font-extrabold")}>
              {money(settings.daily_budget_usd)}
            </span>
            <button
              type="button"
              aria-label="Raise the daily budget"
              onClick={() => saveSettings({ daily_budget_usd: settings.daily_budget_usd + 1 })}
              className="h-[30px] w-[30px] cursor-pointer rounded-md border border-line bg-transparent text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d"
            >
              +
            </button>
          </div>
        </Row>
      </div>

      <div className={SECTION}>
        <Row
          name="Claude"
          note={
            status?.claude.logged_in
              ? `logged in${status.claude.cli_version ? ` · claude ${status.claude.cli_version}` : ""}`
              : "not logged in — open a terminal and run /login"
          }
        >
          <div className="flex items-center gap-2.5">
            <span
              className={cx(
                "h-1.75 w-1.75 rounded-full",
                status?.claude.logged_in ? "bg-ok dark:bg-ok-d" : "bg-bad dark:bg-bad-d",
              )}
            />
            <button
              type="button"
              onClick={() => api.openClaudeTerminal().catch(() => {})}
              className={cx(QUIET, "px-4 py-2 text-md")}
            >
              Open a terminal
            </button>
          </div>
        </Row>
        <Row
          name="Sidecar"
          note={`${status?.sidecar.ready ? "ready" : "dependencies missing"} · node ${
            status?.sidecar.node_version ?? "not found"
          }${status?.sidecar.development ? " · running from the checkout" : ""}`}
          last
        >
          <div className="flex items-center gap-2.5">
            <span
              className={cx(
                "h-1.75 w-1.75 rounded-full",
                status?.sidecar.ready ? "bg-ok dark:bg-ok-d" : "bg-warn dark:bg-warn-d",
              )}
            />
            {!status?.sidecar.ready && (
              <button
                type="button"
                onClick={installSidecar}
                className="min-h-6 cursor-pointer rounded-full border-none bg-accent px-4 py-2 text-md font-bold text-onAccent transition-[filter] duration-150 hover:brightness-[1.06] dark:bg-accent-d dark:text-onAccent-d"
              >
                Install
              </button>
            )}
          </div>
        </Row>
      </div>

      {settings.always_allow.length > 0 && (
        <div className={SECTION}>
          <Row
            name="Standing allowances"
            note="Calls Relay stops asking about. Each one is scoped to the command it came from, so allowing git push does not allow every shell command. Click one to take it back."
            last
          >
            <div className="flex flex-wrap justify-end gap-1.5">
              {settings.always_allow.map((rule) => {
                const label = ruleLabel(rule);
                // An unscoped shell rule from an older build. It authorises
                // nothing now; it is shown so it can be seen and removed.
                const revoked = ruleIsRevoked(rule);
                return (
                  <button
                    key={label}
                    type="button"
                    title={
                      revoked
                        ? "This allowed every command, so it no longer allows any. Approve once more to record a scoped rule."
                        : "Stop allowing this"
                    }
                    onClick={() =>
                      saveSettings({
                        always_allow: settings.always_allow.filter(
                          (x) => ruleLabel(x) !== label,
                        ),
                      })
                    }
                    className={cx(
                      DANGER,
                      "flex items-center gap-1.5 px-3 py-1.5 font-mono text-sm",
                      revoked
                        ? "text-text3 line-through dark:text-text3-d"
                        : "text-text2 no-underline dark:text-text2-d",
                    )}
                  >
                    {label}
                    {revoked && (
                      <span className="font-sans text-xs font-bold text-warn no-underline dark:text-warn-d">
                        revoked
                      </span>
                    )}
                    <span>&#10005;</span>
                  </button>
                );
              })}
            </div>
          </Row>
        </div>
      )}

      <UpdateRow />

      <div className={PANEL}>
        <div className="flex flex-col gap-2.5 p-4.5">
          <div className="flex items-center justify-between gap-3.5">
            <span className="text-md">Where everything is written</span>
            <span
              title={dataDir}
              className={cx(truncate, "max-w-[460px] font-mono text-md text-text3 dark:text-text3-d")}
            >
              {dataDir}
            </span>
          </div>
          <div className="text-sm leading-relaxed text-text3 dark:text-text3-d">
            Event logs, run transcripts, agent profiles and worktrees all live there — never inside
            the repositories you point Relay at.
          </div>
          <button
            type="button"
            onClick={() =>
              api.reveal(dataDir).catch((e) => toast("bad", "Could not open it", reason(e)))
            }
            className={cx(QUIET, "mt-1.5 self-start px-4 py-2 text-md")}
          >
            Show files
          </button>
          {log.length > 0 && (
            <pre className="m-0 whitespace-pre-wrap rounded-md border border-line bg-surface2 px-3 py-2.5 font-mono text-sm text-text2 dark:border-line-d dark:bg-surface2-d dark:text-text2-d">
              {log.join("\n")}
            </pre>
          )}
        </div>
      </div>
    </div>
  );
}
