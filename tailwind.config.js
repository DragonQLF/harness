/** Os tokens do desenho, em valores literais.
 *
 *  O valor base é o do tema **claro** e a variante `dark:` é a do tema
 *  **escuro**, porque é assim que o Tailwind lê um tema. O atributo continua a
 *  ser o que o `store.tsx` escreve — `data-theme` — e é isso que o `darkMode`
 *  abaixo aponta.
 *
 *  O tema claro é a paleta **Asphalt** do `docs/design/README.md`: pano de
 *  fundo `#F6F7F9`, superfície branca, tinta `#16191E`, acento `#3559E9`. O
 *  escuro é o seu par, e é um desenho por direito próprio, não uma inversão:
 *  `#0C0E12` de fundo, `#14171D` de superfície, tinta `#E8EAEE`, acento
 *  `#6E92F0` — um azul que clareia em vez de escurecer quando ganha ênfase,
 *  porque num fundo escuro é para lá que a ênfase vai. Os
 *  nomes dos tokens não mudaram — só os valores — para que um ecrã ainda não
 *  migrado apanhe a paleta nova sem lhe tocar. Os nomes que o desenho usa
 *  (`canvas`, `ink`, `muted`, `faint`, `primary`) existem também, e são o que
 *  os ecrãs novos escrevem.
 *
 *  Os tokens do acento trazem `var(--accent, …)`: o operador pode escolher um
 *  acento no ecrã de definições, e o `applyTheme` escreve essas propriedades
 *  no elemento raiz em runtime. O literal é o fallback.
 *
 *  @type {import('tailwindcss').Config}
 */
export default {
  // O último caminho é o do `streamdown`: as classes dele vivem no JS já
  // compilado do pacote, não no nosso código-fonte, por isso o Tailwind não
  // as gera a menos que o conteúdo aponte para lá. Sem esta linha o pacote
  // renderiza sem estilo nenhum e parece partido.
  content: ["./index.html", "./src/**/*.{ts,tsx}", "./node_modules/streamdown/dist/*.js"],

  darkMode: ["selector", '[data-theme="dark"]'],

  theme: {
    // A escala do desenho substitui a do Tailwind por inteiro — trabalha em
    // meios-pontos, e `text-sm` tem de significar 11.5px e não 14px.
    fontSize: {
      "2xs": "9.5px",
      "10": "10px",
      xs: "10.5px",
      "11": "11px",
      sm: "11.5px",
      body: "12px",
      md: "12.5px",
      base: "13px",
      lg: "14px",
      sheet: "14.5px",
      "15": "15px",
      xl: "16px",
      title: "20px",
      "2xl": "21px",
      stat: "24px",
      "26": "26px",
      "3xl": "30px",
    },

    borderRadius: {
      none: "0",
      px: "1px",
      "2px": "2px",
      "3.5px": "3.5px",
      "4px": "4px",
      "5px": "5px",
      "6px": "6px",
      "7px": "7px",
      sm: "8px",
      DEFAULT: "8px",
      "9px": "9px",
      "10px": "10px",
      md: "12px",
      sheet: "14px",
      lg: "16px",
      xl: "20px",
      full: "999px",
    },

    extend: {
      fontFamily: {
        // Inter faz a interface; a Space Grotesk só existe para o wordmark.
        sans: ["Inter", "system-ui", "-apple-system", '"Segoe UI"', "sans-serif"],
        display: ['"Space Grotesk"', "system-ui", "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "SFMono-Regular", "Menlo", "monospace"],
      },

      spacing: {
        0.75: "3px",
        1.25: "5px",
        1.75: "7px",
        2.25: "9px",
        2.75: "11px",
        3.25: "13px",
        3.75: "15px",
        4.25: "17px",
        4.5: "18px",
        4.75: "19px",
        5.25: "21px",
        5.5: "22px",
        6.5: "26px",
        7.5: "30px",
        8.5: "34px",
        9.5: "38px",
        10.5: "42px",
      },

      colors: {
        // ---- os nomes do desenho ----
        canvas: { DEFAULT: "#F6F7F9", d: "#0C0E12" },
        ink: { DEFAULT: "#16191E", d: "#E8EAEE" },
        ink2: { DEFAULT: "#3A4353", d: "#C2C7D0" },
        // Um título pousado num painel embutido. No claro é tinta como
        // qualquer outro; no escuro abranda um passo, porque tinta cheia sobre
        // `surface2` fica a vibrar.
        inkHead: { DEFAULT: "#16191E", d: "#D3D8E1" },
        muted: { DEFAULT: "#5A6472", d: "#949AA6" },
        faint: { DEFAULT: "#98A1B2", d: "#6A707C" },
        primary: { DEFAULT: "#3559E9", d: "#6E92F0" },
        primaryDeep: { DEFAULT: "#1D2E8C", d: "#B3C6FA" },
        // O acento sob o ponteiro e o acento desligado. No claro escurece, no
        // escuro clareia: a ênfase afasta-se do fundo, nos dois sentidos.
        primaryHover: { DEFAULT: "#1D2E8C", d: "#8AA9F5" },
        primaryDim: { DEFAULT: "#AABBF5", d: "#3D4870" },
        primarySoft: { DEFAULT: "#EEF1FE", d: "#171C2B" },
        primaryLine: { DEFAULT: "#DCE4FC", d: "#2C3550" },
        // Os cinco degraus do mapa de calor, do mais frio ao mais quente. No
        // escuro a rampa é a própria escala do acento — soft, border, dim,
        // primary, active — porque o desenho escuro não traz uma sua e uma
        // rampa inventada ao lado de um acento definido lê-se como erro.
        heat0: { DEFAULT: "#EEF0F4", d: "#191D24" },
        heat1: { DEFAULT: "#DCE4FC", d: "#2C3550" },
        heat2: { DEFAULT: "#AABBF5", d: "#3D4870" },
        heat3: { DEFAULT: "#3559E9", d: "#6E92F0" },
        heat4: { DEFAULT: "#1D2E8C", d: "#B3C6FA" },

        // ---- os nomes antigos, agora a apontar para a paleta nova ----
        desk: { DEFAULT: "#F6F7F9", d: "#0C0E12" },
        recess: { DEFAULT: "#FFFFFF", d: "#14171D" },
        bg: { DEFAULT: "#F6F7F9", d: "#0C0E12" },
        surface: { DEFAULT: "#FFFFFF", d: "#14171D" },
        surface2: { DEFAULT: "#F1F3F7", d: "#191D24" },
        elev: { DEFAULT: "#FFFFFF", d: "#14171D" },
        hovered: { DEFAULT: "#FAFBFC", d: "#191D24" },
        active: { DEFAULT: "#F1F3F7", d: "#212731" },

        line: { DEFAULT: "#E4E7EC", d: "#252A33" },
        line2: { DEFAULT: "#F4F5F8", d: "#1B2027" },
        line3: { DEFAULT: "#EEF0F4", d: "#1F242C" },
        line4: { DEFAULT: "#D3D8E0", d: "#2C323E" },

        text: { DEFAULT: "#16191E", d: "#E8EAEE" },
        text1: { DEFAULT: "#3A4353", d: "#C2C7D0" },
        text2: { DEFAULT: "#5A6472", d: "#949AA6" },
        text3: { DEFAULT: "#98A1B2", d: "#6A707C" },
        text4: { DEFAULT: "#98A1B2", d: "#6A707C" },

        accent: { DEFAULT: "var(--accent, #3559E9)", d: "var(--accent, #6E92F0)" },
        accent2: { DEFAULT: "var(--accent2, #1D2E8C)", d: "var(--accent2, #B3C6FA)" },
        accentSoft: {
          DEFAULT: "var(--accentSoft, #EEF1FE)",
          d: "var(--accentSoft, #171C2B)",
        },
        accentLine: {
          DEFAULT: "var(--accentLine, #DCE4FC)",
          d: "var(--accentLine, #2C3550)",
        },
        accentDeep: { DEFAULT: "#EEF1FE", d: "#171C2B" },
        onAccent: { DEFAULT: "var(--onAccent, #ffffff)", d: "var(--onAccent, #0C0E12)" },

        ok: { DEFAULT: "#1B7F4D", d: "#5BC48D" },
        okSoft: { DEFAULT: "#E8F5EE", d: "#12211A" },
        warn: { DEFAULT: "#C2410C", d: "#E0965C" },
        warnSoft: { DEFAULT: "#FEF0E6", d: "#241A11" },
        // O cartão de permissão: fundo mais pálido que a pastilha, e uma linha
        // própria — é a única superfície da app com contorno âmbar.
        warnSheet: { DEFAULT: "#FEF9F4", d: "#2A1E13" },
        warnLine: { DEFAULT: "#F0C9A8", d: "#47301B" },
        // O cartão de permissão tem três camadas, não uma: a folha, a linha
        // que aninha o comando lá dentro, e o texto secundário que os
        // acompanha. Estavam os três escritos à mão nas vistas.
        warnSheet2: { DEFAULT: "#FEF3EA", d: "#332413" },
        warnLine2: { DEFAULT: "#F0DFCE", d: "#3A2A1A" },
        warnText2: { DEFAULT: "#B08A62", d: "#A98358" },
        bad: { DEFAULT: "#B3243B", d: "#E0687F" },
        bad2: { DEFAULT: "#B3243B", d: "#E0687F" },
        badSoft: { DEFAULT: "#FBE9EC", d: "#251419" },
        info: { DEFAULT: "#6D3FD4", d: "#A78BE8" },
        infoSoft: { DEFAULT: "#F3EEFE", d: "#1E1A2E" },

        // As cores do código. Existem porque o painel de fonte as escreve, e
        // um realce que não segue o tema é a única coisa no ecrã que fica com
        // o contraste do outro.
        syntaxPurple: { DEFAULT: "#7C3AED", d: "#A78BE8" },
        syntaxPurpleSoft: { DEFAULT: "#F3EEFE", d: "#1E1A2E" },
        syntaxPink: { DEFAULT: "#DB2777", d: "#DE7FA8" },
        syntaxBlue: { DEFAULT: "#2563EB", d: "#6E92F0" },

        // A extrusão do wordmark: um deslocamento chapado, não um esbatimento.
        wordmarkShadow: { DEFAULT: "#C3C9D3", d: "#2C323E" },

      },

      boxShadow: {
        soft: "0 24px 60px -28px rgba(22, 25, 30, .22), 0 0 0 1px rgba(22, 25, 30, .05)",
        "soft-d": "0 30px 70px -28px rgba(0, 0, 0, 0.72), 0 0 0 1px rgba(255, 255, 255, 0.04)",
        lift: "0 12px 26px -18px rgba(22, 25, 30, 0.26)",
        "lift-d": "0 6px 18px rgba(0, 0, 0, 0.35)",
        panel: "0 1px 2px rgba(22, 25, 30, 0.04)",
        "panel-d": "0 1px 2px rgba(0, 0, 0, 0.45)",
        banner: "0 26px 60px -30px rgba(0, 0, 0, 0.7)",
        // A única sombra do desenho: o compositor com o foco.
        composer: "0 5px 16px -12px rgba(53, 89, 233, .4)",
        "composer-d": "0 5px 16px -12px rgba(110, 146, 240, .4)",
        hunk: "0 4px 16px -8px rgba(53, 89, 233, .18)",
        // O separador escolhido dentro de um `Segmented`.
        segment: "0 1px 2px rgba(0, 0, 0, .06)",
      },

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
        // A barrinha de progresso a assentar no seu valor.
        fill: { from: { width: "0%" } },
        // Uma corrida que está a acontecer e não sabe dizer quanto falta. Não
        // é uma percentagem disfarçada: atravessa e volta, e não pára — é
        // exactamente o que se sabe.
        crawl: {
          "0%": { transform: "translateX(-110%)" },
          "100%": { transform: "translateX(410%)" },
        },
        tookOne: {
          "0%": { boxShadow: "inset 0 2px 0 -1px transparent" },
          "30%": { boxShadow: "inset 0 2px 0 -1px var(--accent, #3559E9)" },
          "100%": { boxShadow: "inset 0 2px 0 -1px transparent" },
        },
        tookOneDark: {
          "0%": { boxShadow: "inset 0 2px 0 -1px transparent" },
          "30%": { boxShadow: "inset 0 2px 0 -1px var(--accent, #6E92F0)" },
          "100%": { boxShadow: "inset 0 2px 0 -1px transparent" },
        },
      },

      animation: {
        spin: "spin .7s linear infinite",
        // O círculo tracejado de um recibo de ferramenta a meio.
        "spin-tool": "spin 1.2s linear infinite",
        "spin-slow": "spin 1.4s linear infinite",
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
        fill: "fill .6s cubic-bezier(.2,.8,.25,1) both",
        crawl: "crawl 1.6s cubic-bezier(.4,0,.6,1) infinite",
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
