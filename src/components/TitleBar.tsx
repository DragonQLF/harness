import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/ipc";
import { cx } from "../lib/cx";
import { popover } from "../lib/motion";
import { money } from "../lib/format";
import { useStore } from "../state/store";
import { Icon, mono } from "./ui";
import type { View } from "../views/views";

const appWindow = getCurrentWindow();

/** macOS draws its own close/minimise/zoom buttons over the top-left of the
 *  window, and carries Relay's menus in the bar at the top of the screen, so
 *  the window draws neither. */
const IS_MAC = navigator.userAgent.includes("Macintosh");

/** How much room the system's three buttons take, with the margin it leaves
 *  around them, measured from the window edge. */
const TRAFFIC_LIGHTS = 78;

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
    <span ref={box} className="relative flex items-stretch">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={cx(
          "grid cursor-pointer place-items-center border-none px-2.5 text-sm font-normal transition-colors duration-150",
          open
            ? "bg-active text-text dark:bg-active-d dark:text-text-d"
            : "bg-transparent text-text2 hover:bg-hovered hover:text-text dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
        )}
      >
        {name}
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            variants={popover}
            initial="hidden"
            animate="shown"
            exit="gone"
            className="absolute left-0 top-full z-[200] min-w-[208px] rounded-md border border-line3 bg-elev p-1.5 shadow-soft dark:border-line3-d dark:bg-elev-d dark:shadow-soft-d"
          >
            {items.map((item) =>
              item.run ? (
                <button
                  key={item.label}
                  type="button"
                  onClick={() => {
                    setOpen(false);
                    item.run?.();
                  }}
                  className="flex w-full cursor-pointer items-center gap-2.5 rounded-sm border-none bg-transparent px-2.5 py-1.5 text-left text-md font-normal text-text1 transition-colors duration-150 hover:bg-hovered dark:text-text1-d dark:hover:bg-hovered-d"
                >
                  <span className="flex-1">{item.label}</span>
                  {item.hint && (
                    <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                      {item.hint}
                    </span>
                  )}
                </button>
              ) : (
                <div
                  key={item.label}
                  className="flex cursor-default items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-md font-normal text-text4 dark:text-text4-d"
                >
                  <span className="flex-1">{item.label}</span>
                  {item.hint && (
                    <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                      {item.hint}
                    </span>
                  )}
                </div>
              ),
            )}
          </motion.div>
        )}
      </AnimatePresence>
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

  // Fullscreen takes the traffic lights away, so the gap we hold open for them
  // has to go with it, or the row starts with 78px of nothing.
  const [fullscreen, setFullscreen] = useState(false);
  useEffect(() => {
    if (!IS_MAC) return;
    const read = () => {
      appWindow.isFullscreen().then(setFullscreen).catch(() => {});
    };
    read();
    const stop = appWindow.onResized(read);
    return () => {
      stop.then((off) => off()).catch(() => {});
    };
  }, []);

  const chrome = (
    label: string,
    icon: React.ReactNode,
    run: () => void,
    width: string,
    dim = false,
  ) => (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={dim}
      onClick={run}
      className={cx(
        "grid h-6 place-items-center rounded-sm border-none bg-transparent transition-colors duration-150",
        width,
        dim
          ? "cursor-default text-line4 dark:text-line4-d"
          : "cursor-pointer text-text4 hover:bg-hovered hover:text-text dark:text-text4-d dark:hover:bg-hovered-d dark:hover:text-text-d",
      )}
    >
      {icon}
    </button>
  );

  return (
    <div
      data-tauri-drag-region
      className="z-[100] flex h-[30px] flex-none select-none items-stretch border-b border-line bg-recess dark:border-line-d dark:bg-recess-d"
    >
      <div
        className="flex items-center gap-px"
        style={{ paddingLeft: IS_MAC && !fullscreen ? TRAFFIC_LIGHTS : 8 }}
      >
        {chrome("Sidebar", <Icon.sidebar />, toggleSidebar, "w-6")}
        {chrome("Back", <Icon.back />, back, "w-[22px]", !canBack)}
        {chrome("Forward", <Icon.forward />, forward, "w-[22px]", !canForward)}
        {!IS_MAC && (
          <>
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
          </>
        )}
      </div>

      <div
        data-tauri-drag-region
        className="flex flex-1 items-center justify-center gap-2"
      >
        <span className={cx(mono, "text-sm font-medium text-text3 dark:text-text3-d")}>{line}</span>
        {stats != null && settings != null && stats.spend_today > settings.daily_budget_usd && (
          <span
            className={cx(
              mono,
              "rounded-sm bg-badSoft px-1.5 py-px text-xs text-bad2 dark:bg-badSoft-d dark:text-bad2-d",
            )}
          >
            over budget
          </span>
        )}
      </div>

      {!IS_MAC && (
        <div className="flex items-stretch">
          {[
            {
              label: "minimize",
              icon: <Icon.minimize />,
              run: () => appWindow.minimize(),
              w: "w-[42px]",
              close: false,
            },
            {
              label: "maximize",
              icon: <Icon.maximize />,
              run: () =>
                appWindow.isMaximized().then((m) => (m ? appWindow.unmaximize() : appWindow.maximize())),
              w: "w-[42px]",
              close: false,
            },
            {
              label: "close",
              icon: <Icon.close />,
              run: () => appWindow.close(),
              w: "w-11",
              close: true,
            },
          ].map((b) => (
            <button
              key={b.label}
              type="button"
              aria-label={b.label}
              onClick={b.run}
              className={cx(
                "grid cursor-pointer place-items-center border-none bg-transparent text-text4 transition-colors duration-150 dark:text-text4-d",
                b.w,
                b.close
                  ? "hover:bg-bad hover:text-onAccent dark:hover:bg-bad-d dark:hover:text-onAccent-d"
                  : // Os controlos da janela assentam no `recess`, onde o `hover`
                    // é um degrau tão pequeno que num alvo de 42px não se lê. A
                    // convenção da plataforma é um fundo simples e o glifo a
                    // subir à força toda.
                    "hover:bg-surface2 hover:text-text active:bg-active dark:hover:bg-surface2-d dark:hover:text-text-d dark:active:bg-active-d",
              )}
            >
              {b.icon}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
