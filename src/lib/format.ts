/** Small formatting helpers. Kept together so wording stays consistent. */

export const plural = (n: number, one: string, many?: string) =>
  `${n} ${n === 1 ? one : (many ?? one + "s")}`;

/** Tolerates a missing number: a stat that has not arrived reads as zero. */
export const num = (n: number | null | undefined) => (n ?? 0).toLocaleString("en-US");

export const money = (n: number | null | undefined, digits = 2) => `$${(n ?? 0).toFixed(digits)}`;

/** Download sizes, in the "38.4 MB" shape the update sheets use. Megabytes all
 *  the way up: an update that reads 1.2 GB one release and 980 MB the next is
 *  harder to compare than one that always counts in the same unit. */
export const megabytes = (bytes: number) => `${(bytes / 1_048_576).toFixed(1)} MB`;

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

/** The sidebar's version of `ago`: "now", "1h", "yest", "2d". */
export function shortAgo(ms: number): string {
  if (!ms) return "—";
  const secs = Math.floor((Date.now() - ms) / 1000);
  if (secs < 90) return "now";
  if (secs < 3600) return `${Math.round(secs / 60)}m`;
  if (secs < 86400) return `${Math.round(secs / 3600)}h`;
  const days = Math.round(secs / 86400);
  return days === 1 ? "yest" : `${days}d`;
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

/** A file size a person reads at a glance: "312 KB", "1.4 MB". Whole numbers
 *  below a megabyte — nobody needs a tenth of a kilobyte. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function truncate(text: string, max: number): string {
  return text.length > max ? text.slice(0, max - 1) + "…" : text;
}

/** O fim de um texto, e não o princípio.
 *
 *  Um buffer que rola guarda os *últimos* N caracteres, portanto o que lá está
 *  de novo está no fim. `truncate` corta pela frente e mostraria a abertura de
 *  um pensamento que já passou — parado enquanto o buffer não enche, e a saltar
 *  letra a letra depois de encher, ao sabor do que cai à frente. Corta-se pelo
 *  fim, e nunca a meio de uma palavra: um espaço logo no início do corte é
 *  onde a palavra partida acaba.
 */
export function tail(text: string, max: number): string {
  const line = text.trimEnd().split("\n").filter((l) => l.trim()).pop()?.trim() ?? "";
  if (line.length <= max) return line;
  const cut = line.slice(line.length - max);
  const space = cut.indexOf(" ");
  return "…" + (space >= 0 && space < 12 ? cut.slice(space + 1) : cut);
}
