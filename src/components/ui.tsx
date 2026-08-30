/** As primitivas do desenho.
 *
 *  Eram inline para se poderem ler linha a linha ao lado do ficheiro de
 *  desenho. Agora são classes do Tailwind e, o que interessa mais, **têm
 *  variantes**: `<Pill tone="bad">` em vez de a vista passar cores por cima.
 *  Uma vista que precise de mudar o aspecto de uma primitiva mudou-a aqui.
 *
 *  O que ficou como `style` são as medidas que o chamador escolhe — o lado de
 *  um glifo, a altura de uma barra —, porque essas são valores e não classes:
 *  o Tailwind precisa do nome escrito em código para o gerar. */

import type { ReactNode } from "react";
import {
  Activity as LuActivity,
  Archive as LuArchive,
  ArrowRight,
  ArrowUp,
  Bell,
  Check as LuCheck,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy as LuCopy,
  FileCode,
  Folder as LuFolder,
  GitBranch,
  House,
  Kanban,
  List as LuList,
  MessageCircle,
  Minus,
  PanelLeft,
  Paperclip,
  Pencil as LuPencil,
  CirclePlay,
  Plus as LuPlus,
  Search as LuSearch,
  Settings as LuSettings,
  Square as LuSquare,
  TriangleAlert,
  UserRound,
  Users,
  Waypoints,
  X,
} from "lucide-react";
import { cx } from "../lib/cx";
import { TONE, type Tone, type ToneName } from "../lib/types";

/** Um tom chega como nome (`"bad"`) ou já resolvido (o do perfil de um
 *  agente, que vem do backend como string). Ambos servem. */
function pickTone(t: ToneName | Tone | undefined, fallback: Tone = TONE.neutral): Tone {
  if (!t) return fallback;
  return typeof t === "string" ? (TONE[t] ?? fallback) : t;
}

// ---- classes que várias vistas repetem -------------------------------------

/** Uma linha que não pode crescer para lá do sítio onde está. */
export const truncate = "min-w-0 truncate";

/** O painel dos ecrãs de registo e de definições: linha de 1px, raio 20,
 *  superfície. Estava escrito uma vez para os três ecrãs que o usam, num
 *  ficheiro que eles partilhavam só por isto. */
export const PANEL =
  "overflow-hidden rounded-xl border border-line bg-surface dark:border-line-d dark:bg-surface-d";

/** Uma linha de lista que responde ao ponteiro. */
export const HOVER_ROW = "transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d";

/** Um botão de contorno discreto. */
export const QUIET =
  "min-h-6 cursor-pointer rounded-full border border-line bg-transparent font-semibold text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text focus-visible:bg-hovered focus-visible:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

/** O mesmo botão quando desfaz alguma coisa. */
export const DANGER =
  "min-h-6 cursor-pointer rounded-full border border-line bg-transparent font-semibold text-text3 transition-colors duration-150 hover:border-transparent hover:bg-badSoft hover:text-bad focus-visible:border-transparent focus-visible:bg-badSoft focus-visible:text-bad dark:border-line-d dark:text-text3-d dark:hover:bg-badSoft-d dark:hover:text-bad-d";

/** Uma pastilha numa fila de escolhas. */
export const CHOICE = "min-h-6 cursor-pointer rounded-full border-none transition-colors duration-150";
export const CHOICE_ON = "bg-accent font-bold text-onAccent dark:bg-accent-d dark:text-onAccent-d";
export const CHOICE_OFF =
  "bg-transparent font-medium text-text2 hover:bg-hovered hover:text-text dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d";

export const tabular = "tabular-nums";

/** A voz de metadados em monoespaçada: ids, custos, ramos, horas. */
export const mono = "font-mono tabular-nums";

// ---- painéis ---------------------------------------------------------------

const CARD_PAD: Record<"none" | "sm" | "md" | "lg", string> = {
  none: "",
  sm: "p-3",
  md: "p-4",
  lg: "p-5",
};

/** A forma de painel que o desenho usa por todo o lado: linha de 1px, raio 16,
 *  superfície. `pad` é a variante cheia; sem ela fica de bordo a bordo, que é
 *  o que uma lista precisa. */
export function Card({
  children,
  pad = "none",
  tone,
  raised,
  className,
}: {
  children: ReactNode;
  pad?: keyof typeof CARD_PAD;
  /** Tinge a linha, para um painel que está a dizer alguma coisa. */
  tone?: ToneName | Tone;
  /** Pousado sobre o pano de fundo em vez de assente nele. */
  raised?: boolean;
  className?: string;
}) {
  const t = tone ? pickTone(tone) : null;
  return (
    <div
      className={cx(
        "overflow-hidden rounded-lg border bg-surface dark:bg-surface-d",
        t ? t.line : "border-line dark:border-line-d",
        raised && "shadow-panel dark:shadow-panel-d",
        CARD_PAD[pad],
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Cabeçalho dentro de um painel: título, contagem, ligação à direita. */
export function CardHead({
  title,
  count,
  tone,
  right,
  note,
}: {
  title: string;
  count?: ReactNode;
  tone?: ToneName | Tone;
  right?: ReactNode;
  note?: string;
}) {
  const t = pickTone(tone);
  return (
    <div className="flex items-center gap-2.5 px-5 pb-3.5 pt-4.5">
      <span className="text-lg font-bold">{title}</span>
      {count != null && (
        <span className={cx("rounded-full px-2 py-0.5 text-xs font-bold", t.soft, t.fg)}>
          {count}
        </span>
      )}
      <div className="flex-1" />
      {note && <span className="text-xs text-text3 dark:text-text3-d">{note}</span>}
      {right}
    </div>
  );
}

/** A ligação discreta ("Board →") que o desenho põe nos cabeçalhos. */
export function HeadLink({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="cursor-pointer rounded-[4px] border-none bg-transparent p-0 text-sm text-text3 transition-colors duration-150 hover:text-text focus-visible:text-text dark:text-text3-d dark:hover:text-text-d dark:focus-visible:text-text-d"
    >
      {label}
    </button>
  );
}

// ---- marcas ----------------------------------------------------------------

export function Avatar({
  children,
  tone,
  size = 36,
  round = true,
  weight = 700,
  fontSize,
}: {
  children: ReactNode;
  tone?: ToneName | Tone;
  size?: number;
  round?: boolean;
  weight?: number;
  fontSize?: number;
}) {
  const t = pickTone(tone, TONE.accent);
  return (
    <span
      className={cx(
        "flex flex-none items-center justify-center",
        round ? "rounded-full" : "rounded-md",
        t.soft,
        t.fg,
      )}
      style={{
        width: size,
        height: size,
        fontSize: fontSize ?? (size >= 36 ? 12.5 : 11.5),
        fontWeight: weight,
      }}
    >
      {children}
    </span>
  );
}

/** O quadrado de 16px com a inicial que o desenho põe ao lado de tudo o que
 *  um agente é dono. As marcas dos agentes são identidade: o tom vem do perfil
 *  e não de uma variante escolhida aqui. */
export function Glyph({
  children,
  tone,
  size = 16,
  radius = 5,
  font,
  className,
}: {
  children: ReactNode;
  tone?: ToneName | Tone;
  size?: number;
  radius?: number | string;
  font?: number;
  /** Para o preenchimento que não é um tom — um degradê, por exemplo. Substitui
   *  as classes de cor em vez de se somar a elas. */
  className?: string;
}) {
  const t = pickTone(tone, TONE.accent);
  return (
    <span
      className={cx(
        "grid flex-none place-items-center font-mono font-semibold leading-none",
        className ?? cx(t.soft, t.fg),
      )}
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        fontSize: font ?? Math.max(8, Math.round(size * 0.5)),
      }}
    >
      {children}
    </span>
  );
}

const PILL_SIZE = {
  sm: "text-[10px]",
  md: "text-[11px]",
  lg: "text-md",
} as const;

export function Pill({
  children,
  tone,
  bold = true,
  size = "md",
  dot,
  className,
}: {
  children: ReactNode;
  tone?: ToneName | Tone;
  bold?: boolean;
  size?: keyof typeof PILL_SIZE;
  dot?: boolean;
  className?: string;
}) {
  const t = pickTone(tone);
  return (
    <span
      className={cx(
        "inline-flex items-center gap-1.5 whitespace-nowrap rounded-full",
        dot ? "px-2.5 py-1" : "px-2.25 py-[3px]",
        PILL_SIZE[size],
        bold ? "font-bold" : "font-medium",
        t.soft,
        t.fg,
        className,
      )}
    >
      {dot && <span className={cx("h-1.25 w-1.25 flex-none rounded-full", t.solid)} />}
      {children}
    </span>
  );
}

// ---- botões ----------------------------------------------------------------

/** Botão de acção cheio ("Approve", "Review"). */
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
      onClick={onClick}
      disabled={disabled}
      className="min-h-6 cursor-pointer whitespace-nowrap rounded-full border-none bg-text px-4.5 py-2.5 text-sm font-bold text-bg transition-[filter] duration-150 hover:brightness-125 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:brightness-100 dark:bg-text-d dark:text-bg-d"
    >
      {label}
    </button>
  );
}

/** Acção secundária de contorno ("Send back", "Log"). */
export function QuietButton({
  label,
  onClick,
  disabled,
  tone,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  tone?: ToneName | Tone;
}) {
  const t = tone ? pickTone(tone) : null;
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cx(
        "min-h-6 cursor-pointer whitespace-nowrap rounded-full border px-4 py-2.5 text-sm transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-50",
        t
          ? cx("border-transparent font-bold", t.soft, t.fg)
          : "border-line bg-transparent font-medium text-text2 hover:bg-hovered hover:text-text dark:border-line-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
      )}
    >
      {label}
    </button>
  );
}

/** Controlo segmentado: a fila de escolhas em pastilha do desenho. */
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
    <div className="flex gap-1 rounded-full border border-line bg-surface2 p-1 dark:border-line-d dark:bg-surface2-d">
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onPick(o.id)}
            aria-pressed={on}
            className={cx(
              "min-h-6 flex-1 cursor-pointer whitespace-nowrap rounded-full border-none transition-colors duration-150",
              small ? "px-2.5 py-1.25 text-sm" : "px-[13px] py-1.75 text-md",
              on
                ? "bg-accent font-bold text-onAccent dark:bg-accent-d dark:text-onAccent-d"
                : "bg-transparent font-medium text-text2 hover:bg-hovered hover:text-text dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
            )}
          >
            {o.name}
          </button>
        );
      })}
    </div>
  );
}

/** O interruptor do desenho: carril 38x22, botão de 18px. */
export function Switch({
  on,
  onChange,
  label,
}: {
  on: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={on}
      onClick={() => onChange(!on)}
      className={cx(
        "relative h-5.5 w-[38px] flex-none cursor-pointer rounded-full border-none transition-colors duration-200",
        on ? "bg-accent dark:bg-accent-d" : "bg-surface2 dark:bg-surface2-d",
      )}
    >
      <span
        className={cx(
          "absolute top-0.5 h-4.5 w-4.5 rounded-full transition-all duration-200 ease-rise",
          on ? "left-4.5 bg-onAccent dark:bg-onAccent-d" : "left-0.5 bg-text3 dark:bg-text3-d",
        )}
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
      className={cx(
        "flex items-center gap-3.5 px-4.5 py-3.5",
        !first && "border-t border-line2 dark:border-line2-d",
      )}
    >
      <div className="min-w-0 flex-1">
        <div className="text-md font-semibold">{name}</div>
        <div className="mt-1 text-sm leading-normal text-text3 dark:text-text3-d">{note}</div>
      </div>
      <Switch on={on} onChange={onChange} label={name} />
    </div>
  );
}

// ---- números com forma -----------------------------------------------------

/** Barras de sete dias com as letras dos dias, como o desenho as desenha. */
export function WeekBars({
  values,
  labels,
  tone,
  height = 64,
}: {
  values: number[];
  labels: string[];
  tone?: ToneName | Tone;
  height?: number;
}) {
  const t = pickTone(tone, TONE.accent);
  const peak = Math.max(1, ...values);
  return (
    <div className="flex items-end gap-1.5" style={{ height }}>
      {values.map((v, i) => (
        <span key={i} className="flex flex-1 flex-col items-center gap-1.5">
          <span
            className={cx("w-full origin-bottom animate-riseBar rounded-[4px]", t.solid)}
            style={{
              height: `${Math.max(6, Math.round((v / peak) * (height - 18)))}px`,
              opacity: Number((0.3 + 0.7 * (v / peak)).toFixed(2)),
            }}
          />
          <span className="text-xs text-text3 dark:text-text3-d">{labels[i]}</span>
        </span>
      ))}
    </div>
  );
}

/** Barras compactas sem rótulos (cartões de agente e de projecto). */
export function MiniBars({
  values,
  tone,
  height = 34,
}: {
  values: number[];
  tone?: ToneName | Tone;
  height?: number;
}) {
  const t = pickTone(tone, TONE.accent);
  const peak = Math.max(1, ...values);
  return (
    <div className="flex items-end gap-1" style={{ height }}>
      {values.map((v, i) => (
        <span
          key={i}
          className={cx("flex-1 origin-bottom animate-riseBar-fast rounded-[4px]", t.solid)}
          style={{
            height: `${Math.max(5, Math.round((v / peak) * 100))}%`,
            opacity: Number((0.28 + 0.72 * (v / peak)).toFixed(2)),
          }}
        />
      ))}
    </div>
  );
}

const BLOCK_SIZE = { sm: "h-1.25 w-1.25", md: "h-1.75 w-1.75" } as const;

/** Cinco quadrados com o equilíbrio entre acrescentado e removido. */
export function DiffBlocks({
  added,
  removed,
  size = "md",
}: {
  added: number;
  removed: number;
  size?: keyof typeof BLOCK_SIZE;
}) {
  const span = added + removed;
  const green = span ? Math.max(1, Math.min(5, Math.round((added / span) * 5))) : 0;
  return (
    <span className="flex items-center gap-0.5">
      {[0, 1, 2, 3, 4].map((i) => (
        <span
          key={i}
          className={cx(
            "rounded-px",
            BLOCK_SIZE[size],
            span === 0
              ? "bg-line dark:bg-line-d"
              : i < green
                ? "bg-ok dark:bg-ok-d"
                : "bg-bad dark:bg-bad-d",
          )}
        />
      ))}
    </span>
  );
}

export function Meter({
  pct,
  tone,
  height = 5,
}: {
  pct: number;
  tone?: ToneName | Tone;
  height?: number;
}) {
  const t = pickTone(tone, TONE.accent);
  return (
    <div
      className="overflow-hidden bg-line dark:bg-line-d"
      style={{ height, borderRadius: height }}
    >
      <div
        className={cx(
          "h-full origin-left animate-barGrow transition-[width] duration-500 ease-[ease]",
          t.solid,
        )}
        style={{ width: `${Math.max(0, Math.min(100, pct))}%` }}
      />
    </div>
  );
}

// ---- estados ---------------------------------------------------------------

export function EmptyNote({
  children,
  bordered = true,
}: {
  children: ReactNode;
  bordered?: boolean;
}) {
  return (
    <div
      className={cx(
        "p-6 text-center text-sm text-text3 dark:text-text3-d",
        bordered && "border-t border-line2 dark:border-line2-d",
      )}
    >
      {children}
    </div>
  );
}

export function Loading({ what }: { what: string }) {
  return (
    // Centrado no espaço que lhe deram, não preso ao topo dele. Um spinner
    // debaixo do cabeçalho com um ecrã de nada por baixo lê-se como uma página
    // que falhou e não como uma que está a trabalhar.
    <div
      className="flex min-h-[220px] flex-1 items-center justify-center gap-2.5 p-11 text-sm text-text3 dark:text-text3-d"
      role="status"
    >
      <span className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-line border-t-accent dark:border-line-d dark:border-t-accent-d" />
      {what}
    </div>
  );
}

/** O spinner que diz que um run está vivo. */
export function Spinner({ size = 16 }: { size?: number }) {
  return (
    <span
      className="flex-none animate-spin-slow rounded-full border-[1.6px] border-line3 border-t-accent dark:border-line3-d dark:border-t-accent-d"
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

/** Um ponto vivo: verde e a respirar enquanto alguma coisa corre mesmo. */
export function LiveDot({ tone, size = 6 }: { tone?: ToneName | Tone; size?: number }) {
  const t = pickTone(tone, TONE.ok);
  return (
    <span
      className={cx("flex-none animate-pulse rounded-full", t.solid)}
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}

/** O cursor que diz que uma resposta ainda está a chegar. */
export function Caret() {
  return (
    <span
      className="ml-1 inline-block h-3 w-1.75 animate-caret bg-accent align-[-1px] dark:bg-accent-d"
      aria-hidden="true"
    />
  );
}

// ---- cabeçalhos ------------------------------------------------------------

/** Cabeçalho de página: "Overview › workspace" com a data à direita. */
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
    <div className="mb-5 flex items-center gap-3">
      <h1 className="m-0 text-xl font-extrabold leading-tight tracking-[-.02em]">{title}</h1>
      {crumb && (
        <span className="rounded-full border border-line bg-surface2 px-3 py-1 text-xs font-semibold text-text2 dark:border-line-d dark:bg-surface2-d dark:text-text2-d">
          {crumb}
        </span>
      )}
      {children}
      <div className="flex-1" />
      {right}
    </div>
  );
}

/** Um rótulo de secção na barra lateral e nos rails: pequeno, espaçado, quieto. */
export function Eyebrow({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span
      className={cx("text-xs font-medium tracking-[.08em] text-text3 dark:text-text3-d", className)}
    >
      {children}
    </span>
  );
}

// ---- ícones ----------------------------------------------------------------

/** Os trinta e um SVG à mão passaram a `lucide-react`.
 *
 *  A fachada fica: cada entrada guarda o tamanho e o peso de traço que o
 *  desenho lhe deu, convertidos para a grelha de 24 do lucide, para o ecrã não
 *  mudar de espessura ao mudar de biblioteca. São todos decorativos —
 *  `aria-hidden` — e quem tem um botão só de ícone põe-lhe o `aria-label`. */
const hidden = { "aria-hidden": true } as const;

export const Icon = {
  search: () => <LuSearch size={14} strokeWidth={2.4} {...hidden} />,
  bell: () => <Bell size={15} strokeWidth={2.25} {...hidden} />,
  alert: () => <TriangleAlert size={13} strokeWidth={2.4} {...hidden} />,
  chevron: () => (
    <ChevronDown
      size={12}
      strokeWidth={3}
      className="flex-none text-text3 dark:text-text3-d"
      {...hidden}
    />
  ),
  minimize: () => <Minus size={10} strokeWidth={2.88} {...hidden} />,
  maximize: () => <LuSquare size={10} strokeWidth={2.88} {...hidden} />,
  close: () => <X size={10} strokeWidth={3.12} {...hidden} />,
  home: () => <House size={16} strokeWidth={2.4} {...hidden} />,
  code: () => <FileCode size={16} strokeWidth={2.4} {...hidden} />,
  agents: () => <UserRound size={16} strokeWidth={2.4} {...hidden} />,
  board: () => <Kanban size={16} strokeWidth={2.4} {...hidden} />,
  runs: () => <CirclePlay size={16} strokeWidth={2.4} {...hidden} />,
  trees: () => <Waypoints size={16} strokeWidth={2.4} {...hidden} />,
  log: () => <LuList size={16} strokeWidth={2.4} {...hidden} />,
  gear: () => <LuSettings size={16} strokeWidth={2.4} {...hidden} />,
  folder: () => <LuFolder size={16} strokeWidth={2.4} {...hidden} />,
  plus: () => <LuPlus size={12} strokeWidth={3.4} {...hidden} />,
  clip: () => <Paperclip size={15} strokeWidth={2.4} {...hidden} />,
  chat: () => <MessageCircle size={15} strokeWidth={2.4} {...hidden} />,
  check: () => <LuCheck size={15} strokeWidth={2.4} {...hidden} />,
  crew: () => <Users size={15} strokeWidth={2.4} {...hidden} />,
  pulse: () => <LuActivity size={15} strokeWidth={2.4} {...hidden} />,
  arrow: () => <ArrowRight size={13} strokeWidth={2.4} {...hidden} />,
  send: () => <ArrowUp size={14} strokeWidth={2.7} {...hidden} />,
  copy: () => <LuCopy size={12} strokeWidth={2.4} {...hidden} />,
  pencil: () => <LuPencil size={13} strokeWidth={2.4} {...hidden} />,
  archive: () => <LuArchive size={13} strokeWidth={2.4} {...hidden} />,
  branch: () => <GitBranch size={11} strokeWidth={2.4} {...hidden} />,
  sidebar: () => <PanelLeft size={13} strokeWidth={2.1} {...hidden} />,
  back: () => <ChevronLeft size={12} strokeWidth={2.25} {...hidden} />,
  forward: () => <ChevronRight size={12} strokeWidth={2.25} {...hidden} />,
};
