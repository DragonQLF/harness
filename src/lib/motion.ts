/** O movimento que o CSS não consegue fazer.
 *
 *  As Web Interface Guidelines preferem CSS a JavaScript, e a maior parte do
 *  movimento desta app ficou lá: girar, pulsar, piscar, aparecer, crescer —
 *  tudo isso é `animate-*` do Tailwind. Aqui está só o que o CSS não faz:
 *
 *  1. **Um elemento que muda de sítio no DOM.** Um cartão que passa de Working
 *     para Review é removido de uma coluna e montado noutra; o CSS anima a
 *     montagem, não a viagem. É o `layout` do `motion` que a anima.
 *  2. **Uma saída.** Um painel que vai desaparecer já não está lá para o CSS o
 *     animar. É o `AnimatePresence`.
 *  3. **Sequências orquestradas.** O `.stagger` do desenho era uma dúzia de
 *     regras `nth-child` com atrasos à mão; aqui é uma variante com índice.
 *
 *  A preferência de movimento reduzido é respeitada dos dois lados: no CSS
 *  pelo bloco global em `styles/app.css` e pelo `motion-reduce:`, aqui pelo
 *  `<MotionConfig reducedMotion="user">` que o `App` monta à volta de tudo, e
 *  pelo `useReducedMotion()` onde a resposta certa não é "não mexas" mas
 *  "diz-o de outra maneira". */

import type { Transition, Variants } from "motion/react";

/** A curva do desenho, a mesma em todo o lado. */
export const RISE: [number, number, number, number] = [0.2, 0.8, 0.25, 1];

export const rise = (duration: number): Transition => ({ duration, ease: RISE });

/** Os atrasos que o `.stagger` tinha escritos à mão, um por `nth-child`, com o
 *  último a valer para tudo o que venha a seguir: um quadro comprido não pode
 *  entrar a rastejar. */
const ROW_DELAYS = [0.01, 0.05, 0.09, 0.13, 0.17, 0.21, 0.24, 0.27, 0.3, 0.33, 0.35, 0.37];
const ROW_CAP = 0.39;

/** Uma linha de lista a chegar. Passa-lhe o índice em `custom`. */
export const rowIn: Variants = {
  hidden: { opacity: 0, y: 7 },
  shown: (i = 0) => ({
    opacity: 1,
    y: 0,
    transition: { ...rise(0.38), delay: ROW_DELAYS[i as number] ?? ROW_CAP },
  }),
};

const COL_DELAYS = [0.02, 0.07, 0.12, 0.17, 0.22];

/** Uma coluna do quadro a chegar. Passa-lhe o índice em `custom`. */
export const colIn: Variants = {
  hidden: { opacity: 0, y: 14 },
  shown: (i = 0) => ({
    opacity: 1,
    y: 0,
    transition: { ...rise(0.42), delay: COL_DELAYS[i as number] ?? 0.22 },
  }),
};

/** Um painel a subir para o sítio — e, ao contrário do CSS, a sair dele. */
export const paneIn: Variants = {
  hidden: { opacity: 0, y: 10 },
  shown: { opacity: 1, y: 0, transition: rise(0.32) },
  gone: { opacity: 0, y: 6, transition: { duration: 0.16, ease: RISE } },
};

/** Uma folha modal: sobe e encolhe um nada, como o `sheetIn` do desenho. */
export const sheetIn: Variants = {
  hidden: { opacity: 0, y: 12, scale: 0.985 },
  shown: { opacity: 1, y: 0, scale: 1, transition: rise(0.34) },
  gone: { opacity: 0, y: 8, scale: 0.99, transition: { duration: 0.16, ease: RISE } },
};

/** A gaveta que sobe de baixo (`drawerUp`). */
export const drawerUp: Variants = {
  hidden: { y: "101%" },
  shown: { y: 0, transition: rise(0.34) },
  gone: { y: "101%", transition: { duration: 0.2, ease: RISE } },
};

/** Um aviso a entrar e a sair da pilha (`toastIn`). */
export const toastIn: Variants = {
  hidden: { opacity: 0, y: 10, scale: 0.97 },
  shown: { opacity: 1, y: 0, scale: 1, transition: rise(0.28) },
  gone: { opacity: 0, y: 6, scale: 0.97, transition: { duration: 0.18, ease: RISE } },
};

/** O rail a entrar pela direita (`railIn`). */
export const railIn: Variants = {
  hidden: { opacity: 0, x: 18 },
  shown: { opacity: 1, x: 0, transition: rise(0.36) },
  gone: { opacity: 0, x: 12, transition: { duration: 0.18, ease: RISE } },
};

/** Um véu por trás de uma folha. */
export const veil: Variants = {
  hidden: { opacity: 0 },
  shown: { opacity: 1, transition: { duration: 0.18 } },
  gone: { opacity: 0, transition: { duration: 0.14 } },
};

/** Um cartão que mudou de coluna enquanto o operador olhava para outro lado.
 *
 *  A direcção é informação a sério — o quadro corre da esquerda para a direita,
 *  portanto um cartão que entrou em Review veio da esquerda, e um que foi
 *  devolvido veio da direita. Só se move o cartão que mudou: se tudo se mexe,
 *  não se mexeu nada. */
export function arrive(back: boolean): Variants {
  return {
    hidden: { opacity: 0, x: back ? 14 : -14, scale: 0.99 },
    shown: { opacity: 1, x: 0, scale: 1, transition: rise(0.42) },
  };
}
