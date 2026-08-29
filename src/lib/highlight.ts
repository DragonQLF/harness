/** Syntax colouring for the Code screen, entirely on the client.
 *
 *  Three rules shape this module:
 *
 *  - **Nothing here is in the main bundle.** Every `import()` below is dynamic,
 *    so the oniguruma WASM and each grammar become chunks of their own that
 *    Vite only fetches when a file that needs them is opened. Opening a `.rs`
 *    file costs the engine plus `rust`; it never costs `typescript`.
 *  - **Nothing here touches the network.** The app runs from `tauri://localhost`
 *    with no origin to fetch from, so the grammars are bundled and the themes
 *    are written below rather than downloaded. `shiki/wasm` is the inlined
 *    build of the regex engine for the same reason.
 *  - **Failure is plain text, never an error.** Every entry point returns
 *    `null` when a grammar is missing, the file is too big, or anything at all
 *    throws. The caller renders the source it already has.
 *
 *  The hex values in the two themes are the one place in the frontend that
 *  writes colour outside the Tailwind tokens, and they are data: a TextMate
 *  theme *is* a table of colours, shiki hands them back per token, and they
 *  reach the DOM as an inline `color` — never as a class. The light values are
 *  the design's own (`docs/design/Relay.dc.html`, the source pane); the dark
 *  ones are the Asphalt dark counterparts of the same four roles.
 */

// Through `shiki`'s own entry point, not `@shikijs/types` directly. That
// package is a transitive dependency: npm hoists it into a flat node_modules
// so it resolves locally, and pnpm — which CI installs with — does not. An
// import that only works under one package manager is a phantom dependency,
// and it broke the v0.3.5 build on both platforms.
import type { ThemedToken } from "shiki/types";
import type { HighlighterCore } from "shiki/core";

/** One run of characters that shares a colour. */
export type Token = { text: string; color?: string };

/** The grammars this app carries, keyed by the `lang` the backend reports.
 *
 *  Deliberately the set this repository actually contains — Rust, the
 *  TypeScript family, JSON, TOML, Markdown, CSS and HTML — rather than shiki's
 *  full bundle, which is some three hundred grammars and forty megabytes. A
 *  language outside this list renders plain, which is what `language_for` in
 *  `crates/app/src/code.rs` already promises for anything it cannot name. */
const GRAMMARS: Record<string, () => Promise<unknown>> = {
  rust: () => import("@shikijs/langs/rust"),
  typescript: () => import("@shikijs/langs/typescript"),
  tsx: () => import("@shikijs/langs/tsx"),
  javascript: () => import("@shikijs/langs/javascript"),
  // `.jsx` is the same grammar as `.tsx` for our purposes and shipping a
  // fourth copy of the JavaScript tables to say so would not be.
  jsx: () => import("@shikijs/langs/tsx"),
  json: () => import("@shikijs/langs/json"),
  toml: () => import("@shikijs/langs/toml"),
  markdown: () => import("@shikijs/langs/markdown"),
  css: () => import("@shikijs/langs/css"),
  html: () => import("@shikijs/langs/html"),
};

/** The shiki id a `lang` maps to, or null when nothing here covers it. */
function grammarFor(lang: string): string | null {
  return lang in GRAMMARS ? lang : null;
}

/** Whether this file is worth highlighting at all.
 *
 *  A megabyte of source is the read cap, and tokenising one blocks the main
 *  thread for seconds — long enough that the pane would sit empty while it
 *  ran. Past this the source renders plain, immediately, which is the better
 *  of the two honest answers. */
const MAX_CHARS = 200_000;

/** The design's own four roles, in the light theme's values. */
const LIGHT = {
  fg: "#16191E",
  bg: "#FFFFFF",
  keyword: "#7C3AED",
  fn: "#2563EB",
  comment: "#DB2777",
  literal: "#C2410C",
  type: "#3A4353",
  muted: "#5A6472",
};

/** The same four roles against the dark surface. */
const DARK = {
  fg: "#E8EAEE",
  bg: "#14171D",
  keyword: "#A78BE8",
  fn: "#6E92F0",
  comment: "#DE7FA8",
  literal: "#E0965C",
  type: "#C2C7D0",
  muted: "#949AA6",
};

/** A TextMate theme over the four roles the design names, and nothing else.
 *
 *  Deliberately small: a theme with two hundred scopes is a second palette
 *  competing with the app's, and the design colours keywords, function names,
 *  comments and literals. Everything else is the pane's own ink, which is what
 *  keeps the source reading as part of the screen rather than a widget on it. */
function theme(name: string, type: "light" | "dark", c: typeof LIGHT) {
  return {
    name,
    type,
    colors: { "editor.background": c.bg, "editor.foreground": c.fg },
    settings: [
      { settings: { foreground: c.fg, background: c.bg } },
      { scope: ["comment", "punctuation.definition.comment"], settings: { foreground: c.comment } },
      {
        scope: [
          "keyword",
          "storage",
          "storage.type",
          "storage.modifier",
          "keyword.control",
          "variable.language",
          "constant.language",
          "entity.name.tag",
          "meta.tag",
        ],
        settings: { foreground: c.keyword },
      },
      {
        scope: [
          "entity.name.function",
          "support.function",
          "meta.function-call",
          "entity.name.function.macro",
          "entity.other.attribute-name",
        ],
        settings: { foreground: c.fn },
      },
      {
        scope: [
          "string",
          "constant.numeric",
          "constant.character",
          "constant.other",
          "support.constant",
        ],
        settings: { foreground: c.literal },
      },
      {
        scope: ["entity.name.type", "support.type", "entity.name.namespace", "support.class"],
        settings: { foreground: c.type },
      },
      // Operators and separators come back to the pane's ink. Left to the
      // keyword scope they turn every `:` and `->` purple, which the design
      // does not do.
      {
        scope: ["keyword.operator", "punctuation", "meta.brace"],
        settings: { foreground: c.muted },
      },
    ],
  };
}

const THEMES = {
  light: theme("relay-light", "light", LIGHT),
  dark: theme("relay-dark", "dark", DARK),
} as const;

/** The one highlighter, built at most once and only when something asks. */
let starting: Promise<HighlighterCore | null> | null = null;
const loaded = new Set<string>();

async function highlighter(): Promise<HighlighterCore | null> {
  starting ??= (async () => {
    try {
      const [{ createHighlighterCore }, { createOnigurumaEngine }] = await Promise.all([
        import("shiki/core"),
        import("shiki/engine/oniguruma"),
      ]);
      return await createHighlighterCore({
        themes: [THEMES.light, THEMES.dark],
        // Grammars arrive one at a time, as files are opened.
        langs: [],
        engine: createOnigurumaEngine(import("shiki/wasm")),
      });
    } catch {
      // A highlighter that will not start is a highlighter this screen does
      // without. Remembered as null so the next file does not try again.
      return null;
    }
  })();
  return starting;
}

/** Colour `code` under `lang`, one array of tokens per line.
 *
 *  Returns `null` — never throws, never a partial result — when the language
 *  is not carried, the file is past the cap, or the highlighter failed. Line
 *  count always matches `code.split("\n")`, so the caller can pair tokens with
 *  the gutter and the diff marks it drew from the hunks. */
export async function tokenize(
  code: string,
  lang: string,
  mode: "light" | "dark",
): Promise<Token[][] | null> {
  const id = grammarFor(lang);
  if (!id || code.length > MAX_CHARS) return null;

  const shiki = await highlighter();
  if (!shiki) return null;

  try {
    if (!loaded.has(id)) {
      const mod = (await GRAMMARS[id]!()) as { default: unknown };
      await shiki.loadLanguage(mod.default as never);
      loaded.add(id);
    }
    const { tokens } = shiki.codeToTokens(code, {
      lang: id,
      theme: THEMES[mode].name,
    });
    return tokens.map((line: ThemedToken[]) =>
      line.map((t) => ({ text: t.content, color: t.color })),
    );
  } catch {
    return null;
  }
}
