import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/ipc";
import { money } from "../lib/format";
import { useStore } from "../state/store";
import { Icon, mono } from "./ui";
import type { View } from "../views/views";

const appWindow = getCurrentWindow();

/** One entry in a title-bar menu: a word and what it does. */
interface MenuItem {
  label: string;
  hint?: string;
  run?: () => void;
}

function Menu({ name, items }: { name: string; items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLSpanElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", away);
    return () => window.removeEventListener("mousedown", away);
  }, [open]);

  return (
    <span ref={box} style={{ position: "relative", display: "flex", alignItems: "stretch" }}>
      <span
        onClick={() => setOpen((v) => !v)}
        style={{
          display: "grid",
          placeItems: "center",
          padding: "0 10px",
          font: "400 11.5px var(--sans)",
          color: open ? "var(--text)" : "var(--text2)",
          background: open ? "var(--active)" : "transparent",
          cursor: "pointer",
        }}
      >
        {name}
      </span>
      {open && (
        <div
          style={{
            position: "absolute",
            top: "100%",
            left: 0,
            zIndex: 200,
            minWidth: 208,
            padding: 6,
            borderRadius: 12,
            background: "var(--elev)",
            border: "1px solid var(--line3)",
            boxShadow: "var(--shadow)",
            animation: "fadeIn .14s ease both",
          }}
        >
          {items.map((item) => (
            <div
              key={item.label}
              className={item.run ? "row" : undefined}
              onClick={() => {
                if (!item.run) return;
                setOpen(false);
                item.run();
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "6px 10px",
                borderRadius: 8,
                font: "400 12.5px var(--sans)",
                color: item.run ? "var(--text1)" : "var(--text4)",
                cursor: item.run ? "pointer" : "default",
              }}
            >
              <span style={{ flex: 1 }}>{item.label}</span>
              {item.hint && (
                <span style={{ ...mono, fontSize: 10.5, color: "var(--text4)" }}>{item.hint}</span>
              )}
            </div>
          ))}
        </div>
      )}
    </span>
  );
}

/** The 30px window chrome from the design: sidebar toggle, history arrows, the
 *  three menus, one line saying where you are and what is happening, and the
 *  window buttons. */
export function TitleBar({
  go,
  back,
  forward,
  canBack,
  canForward,
  toggleSidebar,
  toggleRail,
  onPalette,
  onNewChat,
}: {
  go: (v: View) => void;
  back: () => void;
  forward: () => void;
  canBack: boolean;
  canForward: boolean;
  toggleSidebar: () => void;
  toggleRail: () => void;
  onPalette: () => void;
  onNewChat: () => void;
}) {
  const { project, snapshot, status, settings, stats, saveSettings, addProject } = useStore();
  const running = (snapshot?.cards ?? []).filter((c) => c.status === "running").length;
  const sidecar = status?.sidecar.ready
    ? "sidecar ready"
    : status?.sidecar.node_found
      ? "sidecar not installed"
      : "no node";

  // The one line that says where you are: project, branch, what is running, and
  // whether anything can run at all.
  const line = [
    project?.name ?? "no project",
    project?.base_branch,
    running > 0 ? `${running} ${running === 1 ? "run" : "runs"} live` : "nothing running",
    sidecar,
  ]
    .filter(Boolean)
    .join(" · ");

  const chrome = (label: string, icon: React.ReactNode, run: () => void, width: number, dim = false) => (
    <span
      title={label}
      onClick={run}
      style={{
        display: "grid",
        placeItems: "center",
        width,
        height: 24,
        borderRadius: 8,
        color: dim ? "var(--line4)" : "var(--text4)",
        cursor: dim ? "default" : "pointer",
      }}
    >
      {icon}
    </span>
  );

  return (
    <div
      data-tauri-drag-region
      style={{
        flex: "none",
        height: 30,
        display: "flex",
        alignItems: "stretch",
        background: "var(--recess)",
        borderBottom: "1px solid var(--line)",
        userSelect: "none",
        zIndex: 100,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 1, paddingLeft: 8 }}>
        {chrome("Sidebar", <Icon.sidebar />, toggleSidebar, 24)}
        {chrome("Back", <Icon.back />, () => canBack && back(), 22, !canBack)}
        {chrome("Forward", <Icon.forward />, () => canForward && forward(), 22, !canForward)}
        <Menu
          name="File"
          items={[
            { label: "New chat", hint: "⌘N", run: onNewChat },
            { label: "Add a project…", run: addProject },
            { label: "Projects", run: () => go("projects") },
            { label: "Settings", hint: "⌘,", run: () => go("settings") },
          ]}
        />
        <Menu
          name="View"
          items={[
            { label: "Command palette", hint: "⌘K", run: onPalette },
            { label: "Toggle the sidebar", run: toggleSidebar },
            { label: "Toggle Right now", run: toggleRail },
            {
              label: settings?.theme === "light" ? "Dark theme" : "Light theme",
              run: () => saveSettings({ theme: settings?.theme === "light" ? "dark" : "light" }),
            },
            { label: "Worktrees", run: () => go("trees") },
            { label: "Activity", run: () => go("activity") },
          ]}
        />
        <Menu
          name="Help"
          items={[
            {
              label: status?.claude.logged_in ? "Claude is signed in" : "Sign in to Claude…",
              run: () => api.openClaudeTerminal().catch(() => {}),
            },
            {
              label: status?.claude.cli_version
                ? `Claude CLI ${status.claude.cli_version}`
                : "Claude CLI not found",
            },
            { label: settings ? `Daily budget ${money(settings.daily_budget_usd)}` : "No settings" },
          ]}
        />
      </div>

      <div
        data-tauri-drag-region
        style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", gap: 8 }}
      >
        <span style={{ ...mono, fontSize: 11.5, fontWeight: 500, color: "var(--text3)" }}>{line}</span>
        {stats != null && settings != null && stats.spend_today > settings.daily_budget_usd && (
          <span
            style={{
              ...mono,
              fontSize: 10.5,
              padding: "1px 6px",
              borderRadius: 8,
              background: "var(--badSoft)",
              color: "var(--bad2)",
            }}
          >
            over budget
          </span>
        )}
      </div>

      <div style={{ display: "flex", alignItems: "stretch" }}>
        {[
          { label: "minimize", icon: <Icon.minimize />, run: () => appWindow.minimize(), w: 42 },
          {
            label: "maximize",
            icon: <Icon.maximize />,
            run: () =>
              appWindow.isMaximized().then((m) => (m ? appWindow.unmaximize() : appWindow.maximize())),
            w: 42,
          },
          { label: "close", icon: <Icon.close />, run: () => appWindow.close(), w: 44 },
        ].map((b) => (
          <button
            key={b.label}
            type="button"
            aria-label={b.label}
            className={b.label === "close" ? "hv-close" : "hv-hover"}
            onClick={b.run}
            style={{
              width: b.w,
              border: "none",
              background: "transparent",
              color: "var(--text4)",
              display: "grid",
              placeItems: "center",
              cursor: "pointer",
              transition: "all .16s ease",
            }}
          >
            {b.icon}
          </button>
        ))}
      </div>
    </div>
  );
}
