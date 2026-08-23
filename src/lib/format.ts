/** Small formatting helpers. Kept together so wording stays consistent. */

export const plural = (n: number, one: string, many?: string) =>
  `${n} ${n === 1 ? one : (many ?? one + "s")}`;

/** Tolerates a missing number: a stat that has not arrived reads as zero. */
export const num = (n: number | null | undefined) => (n ?? 0).toLocaleString("en-US");

export const money = (n: number | null | undefined, digits = 2) => `$${(n ?? 0).toFixed(digits)}`;

/** Elapsed time in the "4m 08s" shape the design uses. */
export function duration(ms: number): string {
  const secs = Math.max(0, Math.floor(ms / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ${String(secs % 60).padStart(2, "0")}s`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${String(mins % 60).padStart(2, "0")}m`;
}

export function clock(ms: number): string {
  if (!ms) return "—";
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes()].map((n) => String(n).padStart(2, "0")).join(":");
}

/** "just now", "14 min ago", "3 days ago". */
export function ago(ms: number): string {
  if (!ms) return "never";
  const secs = Math.floor((Date.now() - ms) / 1000);
  if (secs < 45) return "just now";
  if (secs < 3600) return `${Math.round(secs / 60)} min ago`;
  if (secs < 86400) return `${Math.round(secs / 3600)}h ago`;
  const days = Math.round(secs / 86400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

export function initials(name: string): string {
  return (
    name
      .split(/\s+/)
      .filter(Boolean)
      .map((w) => w[0]!.toUpperCase())
      .slice(0, 2)
      .join("") || "?"
  );
}

export function greeting(): string {
  const h = new Date().getHours();
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

export function today(): string {
  return new Date().toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}

export const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

export function truncate(text: string, max: number): string {
  return text.length > max ? text.slice(0, max - 1) + "…" : text;
}

/** Bar heights for a sparkline, as percentages with a visible floor. */
export function barHeights(values: number[]): { h: string; opacity: number }[] {
  const peak = Math.max(1, ...values);
  return values.map((v) => ({
    h: `${Math.max(6, Math.round((v / peak) * 100))}%`,
    opacity: Number((0.3 + 0.7 * (v / peak)).toFixed(2)),
  }));
}

export const DAY_LETTERS = ["S", "M", "T", "W", "T", "F", "S"];

/** Weekday letters for the last seven days, oldest first. */
export function weekLetters(): string[] {
  const out: string[] = [];
  const day = new Date().getDay();
  for (let i = 6; i >= 0; i--) out.push(DAY_LETTERS[(day - i + 7) % 7]!);
  return out;
}
