/** Primitives lifted straight from the design file, so a screen reads as the
 *  same composition the design has. Styles stay inline on purpose: it keeps
 *  each element comparable with `Relay v4.dc.html` line by line. */

import type { CSSProperties, ReactNode } from "react";

/** The panel shape the design uses everywhere: 1px line, radius 18, surface. */
export function Card({
  children,
  style,
  pad,
  className,
  animation,
}: {
  children: ReactNode;
  style?: CSSProperties;
  /** Padding for the plain variant; omit for edge-to-edge lists. */
  pad?: string;
  className?: string;
  animation?: string;
}) {
  return (
    <div
      className={className}
      style={{
        border: "1px solid var(--line)",
        borderRadius: "var(--r-lg)",
        background: "var(--surface)",
        overflow: "hidden",
        padding: pad,
        animation,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

/** Header row inside a card: title, optional count pill, right-hand link. */
export function CardHead({
  title,
  count,
  countColor,
  countSoft,
  right,
  note,
}: {
  title: string;
  count?: ReactNode;
  countColor?: string;
  countSoft?: string;
  right?: ReactNode;
  note?: string;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "17px 20px 14px" }}>
      <span style={{ fontSize: "var(--t-lg)", fontWeight: 700 }}>{title}</span>
      {count != null && (
        <span
          style={{
            padding: "2px 8px",
            borderRadius: 999,
            background: countSoft ?? "var(--surface2)",
            color: countColor ?? "var(--text3)",
            fontSize: "var(--t-xs)",
            fontWeight: 700,
          }}
        >
          {count}
        </span>
      )}
      <div style={{ flex: 1 }} />
      {note && <span style={{ fontSize: "var(--t-xs)", color: "var(--text3)" }}>{note}</span>}
      {right}
    </div>
  );
}

/** The quiet "Board →" style link the design puts in card headers. */
export function HeadLink({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className="hv-link"
      onClick={onClick}
      style={{
        background: "transparent",
        border: "none",
        color: "var(--text3)",
        fontSize: "var(--t-sm)",
        cursor: "pointer",
        padding: 0,
      }}
    >
      {label}
    </button>
  );
}

export function Avatar({
  children,
  color,
  soft,
  size = 36,
  radius,
  weight = 700,
  fontSize,
}: {
  children: ReactNode;
  color: string;
  soft: string;
  size?: number;
  radius?: number | string;
  weight?: number;
  fontSize?: number;
}) {
  return (
    <span
      style={{
        width: size,
        height: size,
        flex: "none",
        borderRadius: radius ?? "50%",
        background: soft,
        color,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: fontSize ?? (size >= 36 ? 12.5 : 11.5),
        fontWeight: weight,
      }}
    >
      {children}
    </span>
  );
}

export function Pill({
  children,
  color,
  soft,
  bold = true,
  size = 11,
  dot,
}: {
  children: ReactNode;
  color?: string;
  soft?: string;
  bold?: boolean;
  size?: number;
  dot?: boolean;
}) {
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        padding: dot ? "4px 10px" : "3px 9px",
        borderRadius: 999,
        background: soft ?? "var(--surface2)",
        color: color ?? "var(--text3)",
        fontSize: size,
        fontWeight: bold ? 700 : 500,
        whiteSpace: "nowrap",
      }}
    >
      {dot && (
        <span
          style={{
            width: 5,
            height: 5,
            borderRadius: "50%",
            background: color ?? "var(--text3)",
          }}
        />
      )}
      {children}
    </span>
  );
}

/** Solid dark action button ("Approve", "Review"). */
export function StrongButton({
  label,
  onClick,
  disabled,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="hv-brighter"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "9px 17px",
        border: "none",
        borderRadius: 999,
        background: "var(--text)",
        color: "var(--bg)",
        fontSize: "var(--t-sm)",
        fontWeight: 700,
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.5 : 1,
        transition: "filter .18s ease",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </button>
  );
}

/** Outlined secondary action ("Send back", "Log"). */
export function QuietButton({
  label,
  onClick,
  disabled,
  tone,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  tone?: { color: string; soft: string };
}) {
  return (
    <button
      type="button"
      className="hv-soft"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "9px 15px",
        border: tone ? "1px solid transparent" : "1px solid var(--line)",
        borderRadius: 999,
        background: tone?.soft ?? "transparent",
        color: tone?.color ?? "var(--text2)",
        fontSize: "var(--t-sm)",
        fontWeight: tone ? 700 : 500,
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.5 : 1,
        transition: "all .18s ease",
        whiteSpace: "nowrap",
      }}
    >
      {label}
    </button>
  );
}

/** Segmented control: the design's pill row of choices. */
export function Segmented<T extends string>({
  value,
  options,
  onPick,
  small,
}: {
  value: T;
  options: { id: T; name: string }[];
  onPick: (id: T) => void;
  small?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        gap: 3,
        padding: 3,
        borderRadius: 999,
        background: "var(--surface2)",
        border: "1px solid var(--line)",
      }}
    >
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onPick(o.id)}
            style={{
              flex: 1,
              padding: small ? "5px 10px" : "7px 13px",
              border: "none",
              borderRadius: 999,
              background: on ? "var(--accent)" : "transparent",
              color: on ? "var(--onAccent)" : "var(--text2)",
              fontWeight: on ? 700 : 500,
              fontSize: small ? 11.5 : 12.5,
              cursor: "pointer",
              transition: "all .18s ease",
              whiteSpace: "nowrap",
            }}
          >
            {o.name}
          </button>
        );
      })}
    </div>
  );
}

/** The design's switch: 38x22 track, 18px knob. */
export function Switch({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={on}
      onClick={() => onChange(!on)}
      style={{
        position: "relative",
        width: 38,
        height: 22,
        flex: "none",
        border: "none",
        borderRadius: 999,
        background: on ? "var(--accent)" : "var(--surface2)",
        cursor: "pointer",
        transition: "background .2s ease",
      }}
    >
      <span
        style={{
          position: "absolute",
          top: 2,
          left: on ? 18 : 2,
          width: 18,
          height: 18,
          borderRadius: "50%",
          background: on ? "var(--onAccent)" : "var(--text3)",
          transition: "all .2s cubic-bezier(.2,.8,.2,1)",
        }}
      />
    </button>
  );
}

export function SwitchRow({
  name,
  note,
  on,
  onChange,
  first,
}: {
  name: string;
  note: string;
  on: boolean;
  onChange: (v: boolean) => void;
  first?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 13,
        padding: "13px 17px",
        borderTop: first ? "none" : "1px solid var(--line2)",
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 600 }}>{name}</div>
        <div style={{ marginTop: 3, fontSize: 11.5, color: "var(--text3)", lineHeight: 1.5 }}>
          {note}
        </div>
      </div>
      <Switch on={on} onChange={onChange} label={name} />
    </div>
  );
}

/** Seven-day bar chart with weekday letters, as the design draws it. */
export function WeekBars({
  values,
  labels,
  color = "var(--accent)",
  height = 64,
}: {
  values: number[];
  labels: string[];
  color?: string;
  height?: number;
}) {
  const peak = Math.max(1, ...values);
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 5, height }}>
      {values.map((v, i) => (
        <span
          key={i}
          style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 6 }}
        >
          <span
            style={{
              width: "100%",
              height: `${Math.max(6, Math.round((v / peak) * (height - 18)))}px`,
              borderRadius: 5,
              background: color,
              opacity: Number((0.3 + 0.7 * (v / peak)).toFixed(2)),
              transformOrigin: "bottom",
              animation: "riseBar .7s cubic-bezier(.2,.8,.2,1) both",
            }}
          />
          <span style={{ fontSize: 10, color: "var(--text3)" }}>{labels[i]}</span>
        </span>
      ))}
    </div>
  );
}

/** Compact bars without labels (agent and project cards). */
export function MiniBars({
  values,
  color = "var(--accent)",
  height = 34,
}: {
  values: number[];
  color?: string;
  height?: number;
}) {
  const peak = Math.max(1, ...values);
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 4, height }}>
      {values.map((v, i) => (
        <span
          key={i}
          style={{
            flex: 1,
            height: `${Math.max(5, Math.round((v / peak) * 100))}%`,
            borderRadius: 4,
            background: color,
            opacity: Number((0.28 + 0.72 * (v / peak)).toFixed(2)),
            transformOrigin: "bottom",
            animation: "riseBar .6s cubic-bezier(.2,.8,.2,1) both",
          }}
        />
      ))}
    </div>
  );
}

/** Five squares showing the added/removed balance of a commit. */
export function DiffBlocks({ added, removed }: { added: number; removed: number }) {
  const span = added + removed;
  const green = span ? Math.max(1, Math.min(5, Math.round((added / span) * 5))) : 0;
  return (
    <span style={{ display: "flex", gap: 2, alignItems: "center" }}>
      {[0, 1, 2, 3, 4].map((i) => (
        <span
          key={i}
          style={{
            width: 7,
            height: 7,
            borderRadius: 1,
            background: span === 0 ? "var(--line)" : i < green ? "var(--ok)" : "var(--bad)",
          }}
        />
      ))}
    </span>
  );
}

export function Meter({
  pct,
  color = "var(--accent)",
  track = "var(--line)",
  height = 5,
}: {
  pct: number;
  color?: string;
  track?: string;
  height?: number;
}) {
  return (
    <div style={{ height, borderRadius: height, background: track, overflow: "hidden" }}>
      <div
        style={{
          height: "100%",
          width: `${Math.max(0, Math.min(100, pct))}%`,
          background: color,
          transformOrigin: "left",
          animation: "barGrow .8s cubic-bezier(.2,.8,.2,1) both",
          transition: "width .5s ease",
        }}
      />
    </div>
  );
}

export function EmptyNote({ children, bordered = true }: { children: ReactNode; bordered?: boolean }) {
  return (
    <div
      style={{
        padding: 24,
        textAlign: "center",
        fontSize: "var(--t-sm)",
        color: "var(--text3)",
        borderTop: bordered ? "1px solid var(--line2)" : undefined,
      }}
    >
      {children}
    </div>
  );
}

export function Loading({ what }: { what: string }) {
  return (
    <div
      style={{
        // Centred in the space it is given, not pinned to the top of it. A
        // spinner sitting under the header with a screenful of nothing below
        // reads as a page that failed rather than one that is working.
        flex: 1,
        minHeight: 220,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 10,
        padding: 44,
        color: "var(--text3)",
        fontSize: "var(--t-sm)",
      }}
    >
      <span
        style={{
          width: 14,
          height: 14,
          border: "2px solid var(--line)",
          borderTopColor: "var(--accent)",
          borderRadius: "50%",
          animation: "spin .7s linear infinite",
        }}
      />
      {what}
    </div>
  );
}

/** Page heading: "Overview › workspace" with the date on the right. */
export function PageHead({
  title,
  crumb,
  right,
  children,
}: {
  title: string;
  crumb?: string;
  right?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 20 }}>
      <h1
        style={{
          margin: 0,
          fontSize: "var(--t-xl)",
          fontWeight: 800,
          letterSpacing: "-.03em",
          lineHeight: 1.2,
        }}
      >
        {title}
      </h1>
      {crumb && (
        <span
          style={{
            padding: "4px 11px",
            borderRadius: 999,
            background: "var(--surface2)",
            border: "1px solid var(--line)",
            fontSize: "var(--t-xs)",
            fontWeight: 600,
            color: "var(--text2)",
          }}
        >
          {crumb}
        </span>
      )}
      {children}
      <div style={{ flex: 1 }} />
      {right}
    </div>
  );
}

export const truncate: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  minWidth: 0,
};

export const tabular: CSSProperties = { fontVariantNumeric: "tabular-nums" };

/** The mono metadata voice: ids, costs, branches, timestamps. */
export const mono: CSSProperties = {
  fontFamily: "var(--mono)",
  fontVariantNumeric: "tabular-nums",
};

/** A section label in the sidebar and the rails: small, spaced, quiet. */
export function Eyebrow({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <span
      style={{
        fontSize: "var(--t-xs)",
        fontWeight: 500,
        letterSpacing: ".1em",
        color: "var(--text3)",
        ...style,
      }}
    >
      {children}
    </span>
  );
}

/** The 16px square initial the design puts beside anything an agent owns. */
export function Glyph({
  children,
  color,
  soft,
  size = 16,
  radius = 5,
  font,
}: {
  children: ReactNode;
  color: string;
  soft: string;
  size?: number;
  radius?: number | string;
  font?: number;
}) {
  return (
    <span
      style={{
        width: size,
        height: size,
        flex: "none",
        borderRadius: radius,
        background: soft,
        color,
        display: "grid",
        placeItems: "center",
        fontFamily: "var(--mono)",
        fontSize: font ?? Math.max(8, Math.round(size * 0.5)),
        fontWeight: 600,
        lineHeight: 1,
      }}
    >
      {children}
    </span>
  );
}

/** The spinner that says a run is alive. */
export function Spinner({ size = 16 }: { size?: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        flex: "none",
        borderRadius: "50%",
        border: "1.6px solid var(--line3)",
        borderTopColor: "var(--accent)",
        animation: "spin 1.1s linear infinite",
      }}
    />
  );
}

/** A live dot: green and breathing while something is actually running. */
export function LiveDot({ color = "var(--ok)", size = 6 }: { color?: string; size?: number }) {
  return (
    <span
      style={{
        width: size,
        height: size,
        flex: "none",
        borderRadius: "50%",
        background: color,
        animation: "pulse 2.4s ease-in-out infinite",
      }}
    />
  );
}

/** The caret that says an answer is still arriving. */
export function Caret() {
  return (
    <span
      style={{
        display: "inline-block",
        width: 7,
        height: 12,
        marginLeft: 3,
        background: "var(--accent)",
        animation: "caret 1.05s steps(1) infinite",
        verticalAlign: "-1px",
      }}
    />
  );
}

export const Icon = {
  search: () => (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.4 10.4L14 14" />
    </svg>
  ),
  bell: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M8 2.6a3.4 3.4 0 00-3.4 3.4c0 3-1.2 4-1.2 4h9.2s-1.2-1-1.2-4A3.4 3.4 0 008 2.6z" />
      <path d="M6.8 12.4a1.3 1.3 0 002.4 0" />
    </svg>
  ),
  chevron: () => (
    <svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      style={{ flex: "none", color: "var(--text3)" }}
    >
      <path d="M3.4 4.6L6 7.2l2.6-2.6" />
    </svg>
  ),
  minimize: () => (
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
      <path d="M.6 5h8.8" />
    </svg>
  ),
  maximize: () => (
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2">
      <rect x=".7" y=".7" width="8.6" height="8.6" rx="1.6" />
    </svg>
  ),
  close: () => (
    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.3">
      <path d="M1.2 1.2l7.6 7.6M8.8 1.2L1.2 8.8" />
    </svg>
  ),
  home: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M2.6 6.7L8 2.6l5.4 4.1V13a.5.5 0 01-.5.5H3.1a.5.5 0 01-.5-.5z" />
    </svg>
  ),
  code: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M3.5 2.6h6.3l2.7 2.7V13a.5.5 0 01-.5.5H3.5a.5.5 0 01-.5-.5V3.1a.5.5 0 01.5-.5z" />
      <path d="M5.7 8.1h4.6M5.7 10.6h3" />
    </svg>
  ),
  agents: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="8" cy="5.8" r="2.7" />
      <path d="M3.2 13.4c.6-2.5 2.5-3.8 4.8-3.8s4.2 1.3 4.8 3.8" />
    </svg>
  ),
  board: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <rect x="2" y="2.8" width="3.4" height="10.4" rx="1.2" />
      <rect x="6.3" y="2.8" width="3.4" height="6.8" rx="1.2" />
      <rect x="10.6" y="2.8" width="3.4" height="8.6" rx="1.2" />
    </svg>
  ),
  runs: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="8" cy="8" r="5.8" />
      <path d="M6.9 5.9l3.4 2.1-3.4 2.1z" fill="currentColor" stroke="none" />
    </svg>
  ),
  trees: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="4.3" cy="4" r="1.8" />
      <circle cx="4.3" cy="12" r="1.8" />
      <circle cx="11.7" cy="8" r="1.8" />
      <path d="M4.3 5.8v4.4M6.1 4.5h2c1 0 1.7.6 1.7 1.6v.4M6.1 11.5h2c1 0 1.7-.6 1.7-1.6V9.6" />
    </svg>
  ),
  log: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M3 4.2h10M3 8h10M3 11.8h6" />
    </svg>
  ),
  gear: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="8" cy="8" r="2.2" />
      <circle cx="8" cy="8" r="5.8" />
    </svg>
  ),
  folder: () => (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M2.2 4.4c0-.7.6-1.2 1.2-1.2h2.3l1.3 1.6h5.6c.7 0 1.2.5 1.2 1.2v5.6c0 .7-.5 1.2-1.2 1.2H3.4c-.6 0-1.2-.5-1.2-1.2V4.4z" />
    </svg>
  ),
  plus: () => (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.7">
      <path d="M6 2.4v7.2M2.4 6h7.2" />
    </svg>
  ),
  clip: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <path d="M11.6 7.2l-4.3 4.3a2.3 2.3 0 01-3.3-3.3l4.9-4.9a1.5 1.5 0 012.1 2.1l-4.9 4.9a.7.7 0 01-1-1l4.4-4.4" />
    </svg>
  ),
  chat: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M13.4 8.2c0 2.6-2.4 4.7-5.4 4.7-.7 0-1.4-.1-2-.3L3 13.6l.6-2.3a4.4 4.4 0 01-1-2.9c0-2.6 2.4-4.7 5.4-4.7s5.4 2.1 5.4 4.5z" />
    </svg>
  ),
  check: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M2.6 8.4l3 3 7.8-7.8" />
    </svg>
  ),
  crew: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <circle cx="6.2" cy="5.8" r="2.5" />
      <path d="M2 13.2c.5-2.3 2.2-3.5 4.2-3.5s3.7 1.2 4.2 3.5" />
      <path d="M10.6 4.1a2.3 2.3 0 010 4.3M11.8 13.2c-.2-1.2-.6-2.1-1.2-2.8" />
    </svg>
  ),
  pulse: () => (
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M2.4 8h2.4l1.6-4 2.4 8 1.6-4h3.2" />
    </svg>
  ),
  arrow: () => (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M3.4 8h9.2M8.8 4.2L12.6 8l-3.8 3.8" />
    </svg>
  ),
  send: () => (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M8 13V3.5" />
      <path d="M4.2 7.3L8 3.4l3.8 3.9" />
    </svg>
  ),
  copy: () => (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <rect x="5" y="5" width="8.5" height="8.5" rx="1.6" />
      <path d="M11 5V3.2a.7.7 0 00-.7-.7H3.2a.7.7 0 00-.7.7v7.1c0 .4.3.7.7.7H5" />
    </svg>
  ),
  pencil: () => (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M10.6 3.2l2.2 2.2-7 7-2.9.7.7-2.9z" />
    </svg>
  ),
  archive: () => (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <rect x="2.4" y="3" width="11.2" height="3" rx="1" />
      <path d="M3.6 6v6.4a.6.6 0 00.6.6h7.6a.6.6 0 00.6-.6V6M6.4 8.8h3.2" />
    </svg>
  ),
  branch: () => (
    <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6">
      <path d="M8 9.6v3.8M5.6 2.6h4.8l-.7 4.2 1.5 1.4H4.8l1.5-1.4z" />
    </svg>
  ),
  sidebar: () => (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4">
      <rect x="2" y="2.6" width="12" height="10.8" rx="2" />
      <path d="M6.2 2.6v10.8" />
    </svg>
  ),
  back: () => (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M9.5 3.5L5 8l4.5 4.5" />
    </svg>
  ),
  forward: () => (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
      <path d="M6.5 3.5L11 8l-4.5 4.5" />
    </svg>
  ),
};
