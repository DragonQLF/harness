import { getCurrentWindow } from "@tauri-apps/api/window";
import { initials } from "../lib/format";
import { useStore } from "../state/store";
import { Icon } from "./ui";

const appWindow = getCurrentWindow();

/** The 58px window chrome from the design: brand, palette pill, theme, bell,
 *  who you are, and the three window buttons. */
export function TitleBar({
  onPalette,
  onApprovals,
}: {
  onPalette: () => void;
  onApprovals: () => void;
}) {
  const { settings, approvals, saveSettings } = useStore();
  const theme = settings?.theme ?? "dark";
  const name = settings?.user_name ?? "Operator";
  const waiting = approvals.length;

  return (
    <header
      style={{
        height: 58,
        flex: "none",
        display: "flex",
        alignItems: "center",
        background: "var(--surface)",
        borderBottom: "1px solid var(--line)",
        userSelect: "none",
        zIndex: 20,
      }}
    >
      <div
        data-tauri-drag-region
        style={{
          width: 224,
          flex: "none",
          display: "flex",
          alignItems: "center",
          gap: 10,
          paddingLeft: 20,
        }}
      >
        <span
          style={{
            width: 28,
            height: 28,
            borderRadius: 9,
            background: "var(--accent)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--onAccent)",
            fontSize: 13,
            fontWeight: 800,
          }}
        >
          H
        </span>
        <span style={{ fontSize: 16, fontWeight: 800, letterSpacing: "-.02em" }}>Harness</span>
      </div>

      <div
        data-tauri-drag-region
        style={{
          flex: 1,
          display: "flex",
          justifyContent: "center",
          minWidth: 0,
          padding: "0 20px",
        }}
      >
        <button
          type="button"
          className="hv-pill"
          onClick={onPalette}
          style={{
            width: "100%",
            maxWidth: 540,
            display: "flex",
            alignItems: "center",
            gap: 10,
            height: 38,
            padding: "0 16px",
            background: "var(--surface2)",
            border: "1px solid var(--line)",
            borderRadius: 999,
            color: "var(--text3)",
            fontSize: 13,
            cursor: "pointer",
            transition: "all .18s ease",
          }}
        >
          <Icon.search />
          <span style={{ flex: 1, textAlign: "left" }}>Search cards, sessions, agents…</span>
          <span style={{ fontSize: 11, opacity: 0.8 }}>Ctrl K</span>
        </button>
      </div>

      <div
        style={{
          flex: "none",
          display: "flex",
          alignItems: "center",
          gap: 10,
          paddingRight: 8,
        }}
      >
        <button
          type="button"
          className="hv-text"
          title="Theme"
          onClick={() => saveSettings({ theme: theme === "light" ? "dark" : "light" })}
          style={{
            width: 34,
            height: 34,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "var(--surface2)",
            border: "1px solid var(--line)",
            borderRadius: "50%",
            color: "var(--text2)",
            cursor: "pointer",
            fontSize: 12,
            transition: "all .18s ease",
          }}
        >
          {theme === "light" ? "☾" : "☀"}
        </button>

        <button
          type="button"
          className="hv-text"
          title="Waiting on you"
          onClick={onApprovals}
          style={{
            position: "relative",
            width: 34,
            height: 34,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "var(--surface2)",
            border: "1px solid var(--line)",
            borderRadius: "50%",
            color: "var(--text2)",
            cursor: "pointer",
            transition: "all .18s ease",
          }}
        >
          <Icon.bell />
          {waiting > 0 && (
            <span
              style={{
                position: "absolute",
                top: -2,
                right: -2,
                minWidth: 16,
                height: 16,
                padding: "0 4px",
                borderRadius: 999,
                background: "var(--bad)",
                color: "#fff",
                fontSize: 9.5,
                fontWeight: 700,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                border: "2px solid var(--surface)",
              }}
            >
              {waiting}
            </span>
          )}
        </button>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 9,
            paddingLeft: 11,
            borderLeft: "1px solid var(--line)",
          }}
        >
          <span
            style={{
              width: 34,
              height: 34,
              borderRadius: "50%",
              background: "var(--accentSoft)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 12.5,
              fontWeight: 700,
              color: "var(--accent)",
            }}
          >
            {initials(name)}
          </span>
          <span style={{ display: "flex", flexDirection: "column", lineHeight: 1.25 }}>
            <span style={{ fontSize: 12.5, fontWeight: 700 }}>{name}</span>
            <span style={{ fontSize: 11, color: "var(--text3)" }}>Owner</span>
          </span>
        </div>
      </div>

      <div style={{ display: "flex", height: "100%", alignSelf: "stretch" }}>
        {[
          { label: "minimize", icon: <Icon.minimize />, run: () => appWindow.minimize(), w: 44 },
          {
            label: "maximize",
            icon: <Icon.maximize />,
            run: () =>
              appWindow.isMaximized().then((m) => (m ? appWindow.unmaximize() : appWindow.maximize())),
            w: 44,
          },
          { label: "close", icon: <Icon.close />, run: () => appWindow.close(), w: 46 },
        ].map((b) => (
          <button
            key={b.label}
            type="button"
            aria-label={b.label}
            className={b.label === "close" ? "hv-close" : "hv-hover"}
            onClick={b.run}
            style={{
              width: b.w,
              height: "100%",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              border: "none",
              background: "transparent",
              color: "var(--text3)",
              cursor: "pointer",
              transition: "all .16s ease",
            }}
          >
            {b.icon}
          </button>
        ))}
      </div>
    </header>
  );
}
