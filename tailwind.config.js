/** Os tokens do desenho, em valores literais.
 *
 *  Antes viviam como custom properties em `src/styles/theme.css`, redefinidas
 *  num segundo bloco para o tema claro. Agora vivem aqui: o valor base é o do
 *  tema **claro** e a variante `dark:` é a do tema **escuro**, porque é assim
 *  que o Tailwind lê um tema. O atributo continua a ser o que o `store.tsx`
 *  escreve — `data-theme` — e é isso que o `darkMode` abaixo aponta.
 *
 *  Os tokens do acento são a única excepção e trazem `var(--accent, …)`: o
 *  operador pode escolher um acento no ecrã de definições, e o `applyTheme`
 *  escreve essas seis propriedades no elemento raiz em runtime. O literal é o
 *  fallback — quando ninguém escolheu nada, e é o caso normal, resolve para a
 *  cor do tema. Nenhuma folha de estilo as declara; só existem quando o
 *  operador escolhe.
 *
 *  @type {import('tailwindcss').Config}
 */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],

  // O tema é um atributo, não uma classe: `store.tsx:380` põe
  // `data-theme="dark"|"light"` na raiz e o index.html arranca já em dark.
  darkMode: ["selector", '[data-theme="dark"]'],

  theme: {
    // A escala do desenho substitui a do Tailwind por inteiro — trabalha em
    // meios-pontos, e `text-sm` tem de significar 11.5px e não 14px.
    fontSize: {
      xs: "10.5px",
      sm: "11.5px",
      md: "12.5px",
      base: "13px",
      lg: "14px",
      xl: "16px",
      "2xl": "21px",
      "3xl": "30px",
    },

    borderRadius: {
      none: "0",
      px: "1px",
      "2px": "2px",
      "4px": "4px",
      "5px": "5px",
      "6px": "6px",
      sm: "8px",
      DEFAULT: "8px",
      md: "12px",
      lg: "16px",
      xl: "20px",
      full: "999px",
    },

    extend: {
      fontFamily: {
        sans: ['"Space Grotesk"', "system-ui", "-apple-system", '"Segoe UI"', "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },

      // Os passos que o desenho usa e a escala de 4px do Tailwind não tem.
      spacing: {
        1.25: "5px",
        1.75: "7px",
        2.25: "9px",
        2.75: "11px",
        4.5: "18px",
        5.5: "22px",
        6.5: "26px",
      },

      colors: {
        // Três planos de quase-preto quente no escuro, papel quente no claro:
        // secretária, pano de fundo, painel.
        desk: { DEFAULT: "#e9e5de", d: "#0b0b09" },
        recess: { DEFAULT: "#f2efe9", d: "#0f0f0d" },
        bg: { DEFAULT: "#f7f5f1", d: "#131311" },
        surface: { DEFAULT: "#fffefc", d: "#1b1a17" },
        surface2: { DEFAULT: "#f2efe9", d: "#232120" },
        elev: { DEFAULT: "#ffffff", d: "#1a1916" },
        hovered: { DEFAULT: "#ece8e1", d: "#1c1b18" },
        active: { DEFAULT: "#ece8e1", d: "#1f1e1b" },

        line: { DEFAULT: "#e7e2d9", d: "#1f1e1b" },
        line2: { DEFAULT: "#f0ece5", d: "#232120" },
        line3: { DEFAULT: "#ddd7cb", d: "#2a2825" },
        line4: { DEFAULT: "#c9c2b3", d: "#3f3b34" },

        text: { DEFAULT: "#191712", d: "#f2efe8" },
        text1: { DEFAULT: "#2c2820", d: "#e6e2d9" },
        text2: { DEFAULT: "#55503f", d: "#c4bfb2" },
        text3: { DEFAULT: "#8b8577", d: "#8d8880" },
        text4: { DEFAULT: "#9a9487", d: "#6f6a60" },

        // O acento que o operador pode trocar em runtime — ver o cabeçalho.
        accent: {
          DEFAULT: "var(--accent, #0d74b8)",
          d: "var(--accent, #38adee)",
        },
        accent2: {
          DEFAULT: "var(--accent2, #1f92da)",
          d: "var(--accent2, #74c8f6)",
        },
        accentSoft: {
          DEFAULT: "var(--accentSoft, #e7f3fb)",
          d: "var(--accentSoft, rgba(56, 173, 238, 0.16))",
        },
        accentLine: {
          DEFAULT: "var(--accentLine, #c5e2f5)",
          d: "var(--accentLine, rgba(56, 173, 238, 0.34))",
        },
        accentDeep: { DEFAULT: "#e7f3fb", d: "#0e2634" },
        onAccent: {
          DEFAULT: "var(--onAccent, #ffffff)",
          d: "var(--onAccent, #041520)",
        },

        ok: { DEFAULT: "#12866a", d: "#4fd1a5" },
        okSoft: { DEFAULT: "#e4f4ee", d: "rgba(79, 209, 165, 0.14)" },
        warn: { DEFAULT: "#b5751a", d: "#ffb35c" },
        warnSoft: { DEFAULT: "#fbf0dd", d: "rgba(255, 179, 92, 0.14)" },
        bad: { DEFAULT: "#cf4257", d: "#ff6b81" },
        bad2: { DEFAULT: "#cf4257", d: "#ff8b9d" },
        badSoft: { DEFAULT: "#fbe9ec", d: "rgba(255, 107, 129, 0.12)" },
        info: { DEFAULT: "#5a52d0", d: "#9b8cff" },
        infoSoft: { DEFAULT: "#ecebfb", d: "rgba(155, 140, 255, 0.16)" },

        // O que assenta sobre o banner escuro não muda com o tema: o banner é
        // escuro nos dois.
        onBanner: {
          DEFAULT: "#f2efe8",
          2: "rgba(242, 239, 232, 0.66)",
          3: "rgba(242, 239, 232, 0.44)",
        },

        // O levantamento de um cartão sob o ponteiro, que o desenho declarava
        // com valores crus em vez de tokens.
        tileHover: "#1e1d19",
        tileHoverLine: "#33302b",
      },

      backgroundImage: {
        ink: "linear-gradient(140deg, #14252f 0%, #16191b 60%, #131311 100%)",
        "ink-light": "linear-gradient(122deg, #26302a 0%, #1a201c 58%, #141715 100%)",
        banner: "radial-gradient(120% 150% at 12% 8%, #16323f 0%, #172227 45%, #131311 100%)",
        "banner-light":
          "radial-gradient(120% 150% at 12% 8%, #2f3b33 0%, #1d2420 45%, #141715 100%)",
        bannerGlow: "radial-gradient(circle, rgba(56, 173, 238, 0.3), transparent 68%)",
        // A trama de pontos por cima do banner.
        bannerDots: "radial-gradient(rgba(242, 239, 232, 0.055) 1px, transparent 1px)",
      },

      boxShadow: {
        soft: "0 40px 90px -32px rgba(45, 38, 24, 0.26), 0 0 0 1px rgba(30, 26, 16, 0.05)",
        "soft-d": "0 30px 70px -28px rgba(0, 0, 0, 0.72), 0 0 0 1px rgba(255, 255, 255, 0.04)",
        lift: "0 12px 26px -18px rgba(45, 38, 24, 0.3)",
        "lift-d": "0 6px 18px rgba(0, 0, 0, 0.35)",
        // O painel pousado no pano de fundo. A linha interior é o rebordo
        // iluminado: numa superfície quase preta é isso que lê como material,
        // onde uma sombra projectada não lê nada.
        panel:
          "inset 0 1px 0 rgba(255, 255, 255, 0.9), 0 1px 2px rgba(45, 38, 24, 0.07), 0 14px 30px -24px rgba(45, 38, 24, 0.24)",
        "panel-d":
          "inset 0 1px 0 rgba(255, 255, 255, 0.035), 0 1px 2px rgba(0, 0, 0, 0.45), 0 14px 32px -22px rgba(0, 0, 0, 0.8)",
        banner: "0 26px 60px -30px rgba(0, 0, 0, 0.7)",
      },

      // Movimento que fica em CSS. O que anima posição no DOM ou saídas está
      // no `motion` — ver `src/lib/motion.ts`.
      keyframes: {
        spin: { to: { transform: "rotate(360deg)" } },
        pulse: { "0%,100%": { opacity: "1" }, "50%": { opacity: "0.3" } },
        breathe: { "0%,100%": { opacity: "1" }, "50%": { opacity: "0.4" } },
        blink: { "0%,45%": { opacity: "1" }, "50%,100%": { opacity: "0" } },
        caret: { "0%,45%": { opacity: "1" }, "50%,100%": { opacity: "0" } },
        fadeIn: { from: { opacity: "0" }, to: { opacity: "1" } },
        fadeUp: {
          from: { opacity: "0", transform: "translateY(10px)" },
          to: { opacity: "1", transform: "none" },
        },
        popIn: {
          from: { opacity: "0", transform: "scale(0.97) translateY(8px)" },
          to: { opacity: "1", transform: "none" },
        },
        // Um agente parou e não continua sem resposta do operador. É a única
        // coisa na app que está mesmo bloqueada por uma pessoa, por isso pode
        // chegar em vez de aparecer — uma vez, à entrada.
        askedIn: {
          from: { opacity: "0", transform: "translateY(-8px)" },
          to: { opacity: "1", transform: "none" },
        },
        grow: { from: { transform: "scaleY(0)" }, to: { transform: "scaleY(1)" } },
        riseBar: { from: { transform: "scaleY(0)" }, to: { transform: "scaleY(1)" } },
        barGrow: { from: { transform: "scaleX(0)" }, to: { transform: "scaleX(1)" } },
        barIn: {
          from: { opacity: "0", transform: "translateY(4px)" },
          to: { opacity: "1", transform: "none" },
        },
        // A coluna que recebeu o cartão, para a mudança ser apanhável pelo
        // canto do olho sem estar a olhar para o cartão.
        tookOne: {
          "0%": { boxShadow: "inset 0 2px 0 -1px transparent" },
          "30%": { boxShadow: "inset 0 2px 0 -1px var(--accent, #0d74b8)" },
          "100%": { boxShadow: "inset 0 2px 0 -1px transparent" },
        },
        // O mesmo no escuro: um keyframe não tem variante `dark:`, por isso
        // são dois.
        tookOneDark: {
          "0%": { boxShadow: "inset 0 2px 0 -1px transparent" },
          "30%": { boxShadow: "inset 0 2px 0 -1px var(--accent, #38adee)" },
          "100%": { boxShadow: "inset 0 2px 0 -1px transparent" },
        },
      },

      animation: {
        spin: "spin .7s linear infinite",
        "spin-slow": "spin 1.1s linear infinite",
        pulse: "pulse 2.4s ease-in-out infinite",
        breathe: "breathe 2.6s ease-in-out infinite",
        blink: "blink 1.05s steps(1) infinite",
        caret: "caret 1.05s steps(1) infinite",
        fadeIn: "fadeIn .3s ease both",
        "fadeIn-slow": "fadeIn .6s ease both",
        fadeUp: "fadeUp .4s cubic-bezier(.2,.8,.25,1) both",
        popIn: "popIn .28s cubic-bezier(.2,.8,.25,1) both",
        askedIn: "askedIn .34s cubic-bezier(.2,.8,.25,1) both",
        grow: "grow .5s cubic-bezier(.2,.8,.25,1) both",
        riseBar: "riseBar .7s cubic-bezier(.2,.8,.2,1) both",
        "riseBar-fast": "riseBar .6s cubic-bezier(.2,.8,.2,1) both",
        barGrow: "barGrow .8s cubic-bezier(.2,.8,.2,1) both",
        barIn: "barIn .4s cubic-bezier(.2,.8,.25,1) both",
        tookOne: "tookOne .9s ease both",
        tookOneDark: "tookOneDark .9s ease both",
      },

      transitionTimingFunction: {
        rise: "cubic-bezier(.2,.8,.25,1)",
        out: "cubic-bezier(.16,1,.3,1)",
      },
    },
  },

  plugins: [],
};
