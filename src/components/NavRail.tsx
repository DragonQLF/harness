import type { ReactNode } from "react";
import { initials, money, shortAgo } from "../lib/format";
import { cx } from "../lib/cx";
import { tone } from "../lib/types";
import { useStore } from "../state/store";
import type { View } from "../views/views";
import { Eyebrow, Glyph, Icon, Spinner, mono, truncate } from "./ui";

/** Uma linha da barra lateral que acende debaixo do ponteiro. */
const ROW = "transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d";

/** Um botão só de ícone no cabeçalho da barra. */
const ICON_BUTTON =
  "grid h-6 w-6 cursor-pointer place-items-center rounded-sm border-none bg-transparent text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

/** Um "ver tudo" em letra pequena ao lado de um rótulo de secção. */
const SECTION_LINK =
  "min-h-6 cursor-pointer rounded-sm border-none bg-transparent text-xs font-medium text-text4 transition-colors duration-150 hover:text-text dark:text-text4-d dark:hover:text-text-d";

/** The 246px sidebar: where you are, what you were talking about, and which
 *  repository the answers are about. */
export function NavRail({
  view,
  go,
  openChat,
  onPalette,
  onApprovals,
}: {
  view: View;
  go: (v: View) => void;
  /** Open one stored conversation, and show the chat screen. */
  openChat: (conversationId?: string) => void;
  onPalette: () => void;
  onApprovals: () => void;
}) {
  const {
    snapshot,
    agents,
    settings,
    stats,
    project,
    projects,
    projectId,
    selectProject,
    conversations,
    conversationId,
    approvals,
    newConversation,
  } = useStore();

  const cards = snapshot?.cards ?? [];
  const running = cards.filter((c) => c.status === "running").length;
  const inReview = cards.filter((c) => c.status === "review").length;
  const open = cards.filter((c) => c.status !== "done").length;
  const spendToday = stats?.spend_today ?? 0;
  const budget = settings?.daily_budget_usd ?? 10;
  const name = settings?.user_name ?? "Operator";

  const item = (
    v: View,
    label: string,
    icon: ReactNode,
    iconColor: string,
    right?: ReactNode,
  ) => {
    const on = view === v;
    return (
      <button
        key={v}
        type="button"
        aria-current={on ? "page" : undefined}
        onClick={() => (v === "chat" ? openChat() : go(v))}
        className={cx(
          ROW,
          "relative flex h-8 w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 text-left",
        )}
      >
        {on && (
          <div className="absolute inset-0 animate-[fadeIn_.22s_ease_both] rounded-sm bg-active shadow-[inset_0_0_0_1px_theme(colors.line3.DEFAULT)] dark:bg-active-d dark:shadow-[inset_0_0_0_1px_theme(colors.line3.d)]" />
        )}
        <span className={cx("relative grid h-4 w-4 place-items-center", iconColor)}>{icon}</span>
        <span
          className={cx(
            "relative flex-1 text-md font-medium",
            on ? "text-text dark:text-text-d" : "text-text1 dark:text-text1-d",
          )}
        >
          {label}
        </span>
        {right}
      </button>
    );
  };

  const countToken = (n: number) =>
    n > 0 ? (
      <span
        className={cx(mono, "relative text-xs font-medium text-text4 dark:text-text4-d")}
      >
        {n}
      </span>
    ) : undefined;

  return (
    <nav className="flex w-[246px] flex-none flex-col overflow-hidden border-r border-line bg-recess dark:border-line-d dark:bg-recess-d">
      <div className="flex flex-none items-center gap-2.5 px-3 pb-3 pt-3.5">
        {/* A marca do Relay: o acento para o âmbar, e segue o tema nos dois
            extremos como sempre seguiu. */}
        <span className="grid h-5.5 w-5.5 place-items-center rounded-sm bg-[linear-gradient(140deg,var(--accent,#0d74b8),#b5751a)] text-sm font-bold text-onAccent dark:bg-[linear-gradient(140deg,var(--accent,#38adee),#ffb35c)] dark:text-onAccent-d">
          H
        </span>
        <span className="text-xl font-semibold tracking-[-.02em] text-text dark:text-text-d">
          Relay
        </span>
        <div className="flex-1" />
        <button
          type="button"
          title="Command palette ⌘K"
          aria-label="Command palette"
          onClick={onPalette}
          className={ICON_BUTTON}
        >
          <Icon.search />
        </button>
        <button
          type="button"
          title="Waiting on you"
          aria-label="Waiting on you"
          onClick={onApprovals}
          className={cx(ICON_BUTTON, "relative")}
        >
          <Icon.bell />
          {approvals.length > 0 && (
            <span className="absolute right-px top-px h-1.5 w-1.5 rounded-full border-[1.5px] border-recess bg-warn dark:border-recess-d dark:bg-warn-d" />
          )}
        </button>
      </div>

      <div className="flex flex-none flex-col gap-px px-2 pb-1.5">
        {item("chat", "Chat", <Icon.chat />, "text-accent dark:text-accent-d", (
          <span
            className={cx(mono, "relative text-xs font-medium text-text4 dark:text-text4-d")}
          >
            ⌘J
          </span>
        ))}
        {item(
          "review",
          "Review",
          <Icon.check />,
          inReview > 0
            ? "text-warn dark:text-warn-d"
            : "text-text2 dark:text-text2-d",
          inReview > 0 ? (
            <span
              className={cx(
                mono,
                "relative rounded-sm bg-warnSoft px-1.5 py-px text-xs font-semibold text-warn dark:bg-warnSoft-d dark:text-warn-d",
              )}
            >
              {inReview}
            </span>
          ) : undefined,
        )}
        {item("board", "Board", <Icon.board />, "text-text2 dark:text-text2-d", countToken(open))}
        {item(
          "sessions",
          "Sessions",
          <Icon.runs />,
          "text-text2 dark:text-text2-d",
          running > 0 ? (
            <span className="relative h-1.5 w-1.5 animate-pulse rounded-full bg-ok dark:bg-ok-d" />
          ) : undefined,
        )}
        {item(
          "agents",
          "Agents",
          <Icon.crew />,
          "text-text2 dark:text-text2-d",
          countToken(agents.length),
        )}
        {item(
          "code",
          "Code",
          <Icon.code />,
          "text-text2 dark:text-text2-d",
          project ? (
            <span
              className={cx(
                mono,
                truncate,
                "relative max-w-[74px] text-xs text-text4 dark:text-text4-d",
              )}
            >
              {project.base_branch}
            </span>
          ) : undefined,
        )}
        {item("activity", "Activity", <Icon.pulse />, "text-text2 dark:text-text2-d")}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2">
        <div className="flex items-baseline gap-2 px-2.5 pb-1.5 pt-3">
          <Eyebrow>CHATS</Eyebrow>
          <div className="flex-1" />
          <button
            type="button"
            onClick={() => {
              newConversation();
              go("chat");
            }}
            className={cx(mono, SECTION_LINK)}
          >
            new
          </button>
        </div>

        {conversations.length === 0 && (
          <div className="px-2.5 pb-2 pt-1 text-sm font-normal leading-relaxed text-text4 dark:text-text4-d">
            Nothing yet. Anything you ask starts a chat, and it is kept.
          </div>
        )}

        {conversations.slice(0, 12).map((c) => {
          const on = c.id === conversationId;
          const speaker = agents.find((a) => a.id === c.profile_id);
          const pinned = projects.find((p) => p.id === c.project_id);
          const t = tone(speaker?.tone ?? "accent");
          return (
            <button
              key={c.id}
              type="button"
              aria-current={on ? "true" : undefined}
              onClick={() => openChat(c.id)}
              className={cx(
                ROW,
                "relative flex w-full cursor-pointer flex-col gap-0.5 rounded-sm border-none px-2.5 py-1.5 text-left",
                on ? "bg-active dark:bg-active-d" : "bg-transparent",
              )}
            >
              <span className="flex items-center gap-2">
                <span
                  className={cx(
                    "h-1.25 w-1.25 flex-none rounded-full",
                    on ? t.solid : "bg-line4 dark:bg-line4-d",
                  )}
                />
                <span
                  className={cx(
                    truncate,
                    "flex-1 text-md font-normal",
                    on ? "text-text dark:text-text-d" : "text-text2 dark:text-text2-d",
                  )}
                >
                  {c.title}
                </span>
                <span className={cx(mono, "text-xs text-text3 dark:text-text3-d")}>
                  {shortAgo(c.updated_ms)}
                </span>
              </span>
              <span
                className={cx(
                  mono,
                  "flex items-center gap-1.5 pl-3 text-xs text-text4 dark:text-text4-d",
                )}
              >
                {speaker?.name ?? c.profile_id}
                <span className="text-line4 dark:text-line4-d">·</span>
                {pinned?.name ?? "no project"}
              </span>
              {c.resume_failed && (
                <span
                  title="The Claude session behind this chat could not be resumed."
                  className={cx(
                    mono,
                    "mb-px ml-3 mt-0.5 self-start rounded-sm bg-badSoft px-1.5 py-px text-xs font-medium text-bad2 dark:bg-badSoft-d dark:text-bad2-d",
                  )}
                >
                  resume refused · transcript only
                </span>
              )}
            </button>
          );
        })}

        <div className="flex items-baseline gap-2 px-2.5 pb-1.5 pt-3.5">
          <Eyebrow>PROJECTS</Eyebrow>
          <div className="flex-1" />
          <button
            type="button"
            onClick={() => go("projects")}
            className={cx(mono, SECTION_LINK)}
          >
            all
          </button>
        </div>
        {projects.map((p) => {
          const t = tone(p.tone);
          const on = p.id === projectId;
          const state = !p.exists
            ? "missing"
            : p.stats.running
              ? `${p.stats.running} live`
              : p.stats.review
                ? `${p.stats.review} waiting`
                : p.paused
                  ? "paused"
                  : "idle";
          const stateColor = !p.exists
            ? "text-bad2 dark:text-bad2-d"
            : p.stats.running
              ? "text-accent2 dark:text-accent2-d"
              : p.stats.review
                ? "text-warn dark:text-warn-d"
                : "text-text4 dark:text-text4-d";
          return (
            <button
              key={p.id}
              type="button"
              aria-current={on ? "true" : undefined}
              onClick={() => selectProject(p.id)}
              className={cx(
                ROW,
                "flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none px-2.5 py-1.5 text-left",
                on ? "bg-active dark:bg-active-d" : "bg-transparent",
              )}
            >
              <Glyph tone={t} size={17} font={8.5}>
                {p.glyph}
              </Glyph>
              <span
                className={cx(
                  mono,
                  truncate,
                  "flex-1 text-md font-medium text-text1 dark:text-text1-d",
                )}
              >
                {p.name}
              </span>
              <span className={cx("text-xs font-medium", stateColor)}>{state}</span>
            </button>
          );
        })}
      </div>

      <div className="flex-none border-t border-line px-3 pb-3 pt-2.5 dark:border-line-d">
        <div className="flex items-center gap-2.5 pb-2.5">
          {running > 0 ? <Spinner /> : <span className="h-4 w-4 flex-none" />}
          <span className="flex-1 text-sm font-medium text-text2 dark:text-text2-d">
            {running > 0 ? `${running} ${running === 1 ? "run" : "runs"} live` : "nothing running"}
          </span>
          <span
            className={cx(
              mono,
              "text-xs font-medium",
              spendToday > budget
                ? "text-bad2 dark:text-bad2-d"
                : "text-text4 dark:text-text4-d",
            )}
          >
            {money(spendToday)} / {money(budget, 0)}
          </span>
        </div>
        <div className="flex items-center gap-2.5">
          <span
            className={cx(
              mono,
              "grid h-5.5 w-5.5 place-items-center rounded-full bg-accentDeep text-xs font-semibold text-accent2 dark:bg-accentDeep-d dark:text-accent2-d",
            )}
          >
            {initials(name)}
          </span>
          <span
            className={cx(
              truncate,
              "flex-1 text-md font-medium text-text1 dark:text-text1-d",
            )}
          >
            {name}
          </span>
          <button
            type="button"
            title="Settings"
            aria-label="Settings"
            onClick={() => go("settings")}
            className={cx(
              "grid h-6 w-6 cursor-pointer place-items-center rounded-sm border-none bg-transparent transition-colors duration-150 hover:bg-hovered hover:text-text dark:hover:bg-hovered-d dark:hover:text-text-d",
              view === "settings"
                ? "text-text dark:text-text-d"
                : "text-text4 dark:text-text4-d",
            )}
          >
            <Icon.gear />
          </button>
        </div>
      </div>
    </nav>
  );
}
