import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { GitBranch, Minus, Search, SlidersHorizontal, Square, X } from "lucide-react";
import { cx } from "../lib/cx";
import { NAV_VIEWS, VIEW_TITLES, type View } from "../views/views";

const appWindow = getCurrentWindow();

/** macOS draws its own close/minimise/zoom buttons over the top-left of the
 *  window, and carries Relay's menus in the bar at the top of the screen, so
 *  the window draws neither. */
const IS_MAC = navigator.userAgent.includes("Macintosh");

/** How much room the system's three buttons take, with the margin it leaves
 *  around them, measured from the window edge.
 *
 *  In CSS pixels, not screen ones. macOS draws those buttons itself, at the
 *  window's own scale, but everything in here is inside the `zoom: .86` root —
 *  so a literal 78 arrives as 67 and the wordmark sits on top of the green
 *  one. The reserved gap has to be divided back out by the same factor that
 *  will shrink it. */
const TRAFFIC_LIGHTS = Math.round(78 / 0.86);

/** The three icons on the right, at the design's 15px and stroke 2.4. */
const ACTION = { size: 15, strokeWidth: 2.4, "aria-hidden": true } as const;

/** The 52px title bar: the wordmark, the five screens, and three ways out.
 *
 *  The wordmark is the whole identity here — `docs/design/README.md` settled
 *  that the tile mark belongs to the installer and the dock, and that the
 *  letter beside the word was the redundancy that went. The shadow is a flat
 *  offset, not a blur: it is the same extrusion the tile mark has, written in
 *  type. */
export function TitleBar({
  view,
  go,
  onPalette,
}: {
  view: View;
  go: (v: View) => void;
  onPalette: () => void;
}) {
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

  return (
    <div
      data-tauri-drag-region
      className="z-[100] flex h-[52px] flex-none select-none items-center gap-3 border-b border-line bg-surface pr-4.5 dark:border-line-d dark:bg-surface-d"
      style={{ paddingLeft: IS_MAC && !fullscreen ? TRAFFIC_LIGHTS : 18 }}
    >
      <button
        type="button"
        onClick={() => go("home")}
        title="Home"
        className="flex-none cursor-pointer border-none bg-transparent font-display text-15 font-bold tracking-[.075em] text-ink [text-shadow:1.6px_1.6px_0_theme(colors.wordmarkShadow.DEFAULT)] dark:text-ink-d dark:[text-shadow:1.6px_1.6px_0_theme(colors.wordmarkShadow.d)]"
      >
        RELAY
      </button>

      <nav className="mx-auto flex gap-0.5" aria-label="Screens">
        {NAV_VIEWS.map((v) => {
          const on = view === v;
          return (
            <button
              key={v}
              type="button"
              aria-current={on ? "page" : undefined}
              onClick={() => go(v)}
              className={cx(
                "cursor-pointer rounded-sm border-none px-3.25 py-1.75 text-md transition-colors duration-150",
                on
                  ? "bg-active font-semibold text-ink dark:bg-active-d dark:text-ink-d"
                  : "bg-transparent font-medium text-muted hover:text-ink dark:text-muted-d dark:hover:text-ink-d",
              )}
            >
              {VIEW_TITLES[v]}
            </button>
          );
        })}
      </nav>

      <div className="flex flex-none items-center gap-4 text-muted dark:text-muted-d">
        {[
          { label: "Search everything  ⌘K", icon: <Search {...ACTION} />, run: onPalette },
          { label: "Worktrees", icon: <GitBranch {...ACTION} />, run: () => go("trees") },
          { label: "Settings", icon: <SlidersHorizontal {...ACTION} />, run: () => go("settings") },
        ].map((b) => (
          <button
            key={b.label}
            type="button"
            title={b.label}
            aria-label={b.label}
            onClick={b.run}
            className="grid cursor-pointer place-items-center border-none bg-transparent text-current transition-colors duration-150 hover:text-ink dark:hover:text-ink-d"
          >
            {b.icon}
          </button>
        ))}
      </div>

      {/* The window's own buttons, where the platform does not draw them. The
          design does not show these — it does not have to; a window that
          cannot be closed is not a design decision. */}
      {!IS_MAC && (
        <div className="-mr-4.5 flex items-stretch self-stretch">
          {[
            {
              label: "minimize",
              icon: <Minus size={10} strokeWidth={2.88} aria-hidden />,
              run: () => appWindow.minimize(),
              close: false,
            },
            {
              label: "maximize",
              icon: <Square size={10} strokeWidth={2.88} aria-hidden />,
              run: () =>
                appWindow
                  .isMaximized()
                  .then((m) => (m ? appWindow.unmaximize() : appWindow.maximize())),
              close: false,
            },
            {
              label: "close",
              icon: <X size={10} strokeWidth={3.12} aria-hidden />,
              run: () => appWindow.close(),
              close: true,
            },
          ].map((b) => (
            <button
              key={b.label}
              type="button"
              aria-label={b.label}
              onClick={b.run}
              className={cx(
                "grid w-[42px] cursor-pointer place-items-center border-none bg-transparent text-faint transition-colors duration-150 dark:text-faint-d",
                b.close
                  ? "hover:bg-bad hover:text-white"
                  : "hover:bg-active hover:text-ink dark:hover:bg-active-d dark:hover:text-ink-d",
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
