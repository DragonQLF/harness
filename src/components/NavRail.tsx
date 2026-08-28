import type { ReactNode } from "react";
import { initials, money, shortAgo } from "../lib/format";
import { tone } from "../lib/types";
import { useStore } from "../state/store";
import type { View } from "../views/views";
import { Eyebrow, Glyph, Icon, Spinner, monoStyle, truncateStyle } from "./ui";

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
      <div
        key={v}
        onClick={() => (v === "chat" ? openChat() : go(v))}
        className="row"
        style={{
          position: "relative",
          display: "flex",
          alignItems: "center",
          gap: 10,
          height: 32,
          padding: "0 10px",
          borderRadius: 8,
          cursor: "pointer",
        }}
      >
        {on && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              borderRadius: 8,
              background: "var(--active)",
              boxShadow: "inset 0 0 0 1px var(--line3)",
              animation: "fadeIn .22s ease both",
            }}
          />
        )}
        <span
          style={{
            position: "relative",
            display: "grid",
            placeItems: "center",
            width: 16,
            height: 16,
            color: iconColor,
          }}
        >
          {icon}
        </span>
        <span
          style={{
            position: "relative",
            flex: 1,
            font: "500 12.5px var(--sans)",
            color: on ? "var(--text)" : "var(--text1)",
          }}
        >
          {label}
        </span>
        {right}
      </div>
    );
  };

  const countToken = (n: number) =>
    n > 0 ? (
      <span style={{ position: "relative", ...monoStyle, fontSize: 10.5, fontWeight: 500, color: "var(--text4)" }}>
        {n}
      </span>
    ) : undefined;

  return (
    <nav
      style={{
        width: 246,
        flex: "none",
        display: "flex",
        flexDirection: "column",
        background: "var(--recess)",
        borderRight: "1px solid var(--line)",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          flex: "none",
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "14px 12px 12px",
        }}
      >
        <span
          style={{
            width: 22,
            height: 22,
            borderRadius: 8,
            background: "linear-gradient(140deg,var(--accent),var(--warn))",
            display: "grid",
            placeItems: "center",
            font: "700 11.5px var(--sans)",
            color: "var(--onAccent)",
          }}
        >
          H
        </span>
        <span style={{ font: "600 16px var(--sans)", color: "var(--text)", letterSpacing: "-.02em" }}>
          Relay
        </span>
        <div style={{ flex: 1 }} />
        <span
          className="hv-soft"
          title="Command palette ⌘K"
          onClick={onPalette}
          style={{
            display: "grid",
            placeItems: "center",
            width: 24,
            height: 24,
            borderRadius: 8,
            color: "var(--text2)",
            cursor: "pointer",
          }}
        >
          <Icon.search />
        </span>
        <span
          className="hv-soft"
          title="Waiting on you"
          onClick={onApprovals}
          style={{
            position: "relative",
            display: "grid",
            placeItems: "center",
            width: 24,
            height: 24,
            borderRadius: 8,
            color: "var(--text2)",
            cursor: "pointer",
          }}
        >
          <Icon.bell />
          {approvals.length > 0 && (
            <span
              style={{
                position: "absolute",
                top: 1,
                right: 1,
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: "var(--warn)",
                border: "1.5px solid var(--recess)",
              }}
            />
          )}
        </span>
      </div>

      <div
        style={{
          flex: "none",
          padding: "0 8px 6px",
          display: "flex",
          flexDirection: "column",
          gap: 1,
        }}
      >
        {item("chat", "Chat", <Icon.chat />, "var(--accent)", (
          <span style={{ position: "relative", ...monoStyle, fontSize: 10.5, fontWeight: 500, color: "var(--text4)" }}>
            ⌘J
          </span>
        ))}
        {item(
          "review",
          "Review",
          <Icon.check />,
          inReview > 0 ? "var(--warn)" : "var(--text2)",
          inReview > 0 ? (
            <span
              style={{
                position: "relative",
                padding: "1px 6px",
                borderRadius: 8,
                background: "var(--warnSoft)",
                color: "var(--warn)",
                ...monoStyle,
                fontSize: 10.5,
                fontWeight: 600,
              }}
            >
              {inReview}
            </span>
          ) : undefined,
        )}
        {item("board", "Board", <Icon.board />, "var(--text2)", countToken(open))}
        {item(
          "sessions",
          "Sessions",
          <Icon.runs />,
          "var(--text2)",
          running > 0 ? (
            <span
              style={{
                position: "relative",
                width: 6,
                height: 6,
                borderRadius: "50%",
                background: "var(--ok)",
                animation: "pulse 2.4s ease-in-out infinite",
              }}
            />
          ) : undefined,
        )}
        {item("agents", "Agents", <Icon.crew />, "var(--text2)", countToken(agents.length))}
        {item(
          "code",
          "Code",
          <Icon.code />,
          "var(--text2)",
          project ? (
            <span
              style={{ position: "relative", ...monoStyle, fontSize: 10.5, color: "var(--text4)", maxWidth: 74, ...truncateStyle }}
            >
              {project.base_branch}
            </span>
          ) : undefined,
        )}
        {item("activity", "Activity", <Icon.pulse />, "var(--text2)")}
      </div>

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: "0 8px" }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 8, padding: "12px 10px 6px" }}>
          <Eyebrow>CHATS</Eyebrow>
          <div style={{ flex: 1 }} />
          <span
            className="hv-soft"
            onClick={() => {
              newConversation();
              go("chat");
            }}
            style={{ ...monoStyle, fontSize: 10.5, fontWeight: 500, color: "var(--text4)", cursor: "pointer" }}
          >
            new
          </span>
        </div>

        {conversations.length === 0 && (
          <div
            style={{
              padding: "4px 10px 8px",
              font: "400 11.5px var(--sans)",
              lineHeight: 1.6,
              color: "var(--text4)",
            }}
          >
            Nothing yet. Anything you ask starts a chat, and it is kept.
          </div>
        )}

        {conversations.slice(0, 12).map((c) => {
          const on = c.id === conversationId;
          const speaker = agents.find((a) => a.id === c.profile_id);
          const pinned = projects.find((p) => p.id === c.project_id);
          const t = tone(speaker?.tone ?? "accent");
          return (
            <div
              key={c.id}
              onClick={() => openChat(c.id)}
              className="row"
              style={{
                position: "relative",
                display: "flex",
                flexDirection: "column",
                gap: 2,
                padding: "6px 10px",
                borderRadius: 8,
                cursor: "pointer",
                background: on ? "var(--active)" : "transparent",
              }}
            >
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span
                  style={{
                    width: 5,
                    height: 5,
                    flex: "none",
                    borderRadius: "50%",
                    background: on ? t.cssColor : "var(--line4)",
                  }}
                />
                <span
                  style={{
                    flex: 1,
                    font: "400 12.5px var(--sans)",
                    color: on ? "var(--text)" : "var(--text2)",
                    ...truncateStyle,
                  }}
                >
                  {c.title}
                </span>
                <span style={{ ...monoStyle, fontSize: 10.5, color: "var(--text3)" }}>
                  {shortAgo(c.updated_ms)}
                </span>
              </span>
              <span
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  paddingLeft: 12,
                  ...monoStyle,
                  fontSize: 10.5,
                  color: "var(--text4)",
                }}
              >
                {speaker?.name ?? c.profile_id}
                <span style={{ color: "var(--line4)" }}>·</span>
                {pinned?.name ?? "no project"}
              </span>
              {c.resume_failed && (
                <span
                  title="The Claude session behind this chat could not be resumed."
                  style={{
                    margin: "2px 0 1px 12px",
                    alignSelf: "flex-start",
                    padding: "1px 6px",
                    borderRadius: 8,
                    background: "var(--badSoft)",
                    color: "var(--bad2)",
                    ...monoStyle,
                    fontSize: 10.5,
                    fontWeight: 500,
                  }}
                >
                  resume refused · transcript only
                </span>
              )}
            </div>
          );
        })}

        <div style={{ display: "flex", alignItems: "baseline", gap: 8, padding: "14px 10px 6px" }}>
          <Eyebrow>PROJECTS</Eyebrow>
          <div style={{ flex: 1 }} />
          <span
            className="hv-soft"
            onClick={() => go("projects")}
            style={{ ...monoStyle, fontSize: 10.5, fontWeight: 500, color: "var(--text4)", cursor: "pointer" }}
          >
            all
          </span>
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
            ? "var(--bad2)"
            : p.stats.running
              ? "var(--accent2)"
              : p.stats.review
                ? "var(--warn)"
                : "var(--text4)";
          return (
            <div
              key={p.id}
              onClick={() => selectProject(p.id)}
              className="row"
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "6px 10px",
                borderRadius: 8,
                cursor: "pointer",
                background: on ? "var(--active)" : "transparent",
              }}
            >
              <Glyph tone={t} size={17} font={8.5}>
                {p.glyph}
              </Glyph>
              <span style={{ flex: 1, ...monoStyle, fontSize: 12.5, fontWeight: 500, color: "var(--text1)", ...truncateStyle }}>
                {p.name}
              </span>
              <span style={{ font: "500 10.5px var(--sans)", color: stateColor }}>{state}</span>
            </div>
          );
        })}
      </div>

      <div style={{ flex: "none", borderTop: "1px solid var(--line)", padding: "10px 12px 12px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, paddingBottom: 10 }}>
          {running > 0 ? <Spinner /> : <span style={{ width: 16, height: 16, flex: "none" }} />}
          <span style={{ flex: 1, font: "500 11.5px var(--sans)", color: "var(--text2)" }}>
            {running > 0 ? `${running} ${running === 1 ? "run" : "runs"} live` : "nothing running"}
          </span>
          <span
            style={{
              ...monoStyle,
              fontSize: 10.5,
              fontWeight: 500,
              color: spendToday > budget ? "var(--bad2)" : "var(--text4)",
            }}
          >
            {money(spendToday)} / {money(budget, 0)}
          </span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span
            style={{
              width: 22,
              height: 22,
              borderRadius: "50%",
              background: "var(--accentDeep)",
              color: "var(--accent2)",
              display: "grid",
              placeItems: "center",
              ...monoStyle,
              fontSize: 10.5,
              fontWeight: 600,
            }}
          >
            {initials(name)}
          </span>
          <span style={{ flex: 1, font: "500 12.5px var(--sans)", color: "var(--text1)", ...truncateStyle }}>
            {name}
          </span>
          <span
            className="hv-soft"
            title="Settings"
            onClick={() => go("settings")}
            style={{
              display: "grid",
              placeItems: "center",
              width: 20,
              height: 20,
              color: view === "settings" ? "var(--text) " : "var(--text4)",
              cursor: "pointer",
            }}
          >
            <Icon.gear />
          </span>
        </div>
      </div>
    </nav>
  );
}
