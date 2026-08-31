/** Contar um grupo de chamadas de ferramenta numa linha.
 *
 *  Puro e à parte pela mesma razão que o `bubbles.ts`: é a única maneira de o
 *  exercitar sem levantar a aplicação, e uma contagem errada num cabeçalho é
 *  um número decorativo — a coisa que o `CLAUDE.md` proíbe.
 */

import type { ChatMsg } from "./bubbles";

/** Como se conta um grupo de chamadas numa linha só.
 *
 *  Por família e não por nome: "Edited 3 files, ran 7 commands" diz o que
 *  aconteceu, "Edit ×3, Bash ×7" diz que ferramentas foram chamadas — e quem lê
 *  quer a primeira. As famílias que não se reconhecem contam-se pelo nome,
 *  porque inventar-lhes um verbo era pior do que dizer o nome delas. */
const TOOL_VERBS: { match: (t: string) => boolean; one: string; many: string }[] = [
  { match: (t) => /^(edit|multiedit|write|notebookedit)$/i.test(t), one: "edited 1 file", many: "edited %n files" },
  { match: (t) => /^bash$/i.test(t), one: "ran 1 command", many: "ran %n commands" },
  { match: (t) => /^read$/i.test(t), one: "read 1 file", many: "read %n files" },
  { match: (t) => /^(grep|glob|search)$/i.test(t), one: "searched once", many: "searched %n times" },
  { match: (t) => /^(webfetch|websearch)$/i.test(t), one: "looked something up", many: "looked %n things up" },
];

export function summariseTools(tools: ChatMsg[]): string {
  const buckets = new Map<string, number>();
  for (const t of tools) {
    const name = t.tool ?? "tool";
    const verb = TOOL_VERBS.find((v) => v.match(name));
    const key = verb ? verb.many : `${name} ×%n`;
    buckets.set(key, (buckets.get(key) ?? 0) + 1);
  }
  const parts = [...buckets].map(([key, n]) => {
    const verb = TOOL_VERBS.find((v) => v.many === key);
    if (verb && n === 1) return verb.one;
    return key.replace("%n", String(n));
  });
  if (parts.length === 0) return "";
  const said = parts.join(", ");
  // Maiúscula só quando a frase começa por um verbo nosso. O nome de uma
  // ferramenta é um identificador — `generate_image` escreve-se assim, e
  // `Generate_image` é uma coisa que não existe.
  const startsWithVerb = TOOL_VERBS.some((v) => said.startsWith(v.many.split(" ")[0]) || said.startsWith(v.one.split(" ")[0]));
  return startsWithVerb ? said.charAt(0).toUpperCase() + said.slice(1) : said;
}

/** Quantas linhas um grupo mexeu ao todo, ou nada.
 *
 *  `null` quando nenhuma chamada do grupo soube dizer — um grupo só de leituras
 *  e comandos não mexeu em linha nenhuma que se possa contar, e `+0 −0` ali
 *  seria uma afirmação sobre trabalho que ninguém mediu. Uma chamada que sabe
 *  entre dez que não sabem conta na mesma: o que se mostra é o que se sabe, não
 *  uma média.
 */
export function countGroupLines(
  tools: ChatMsg[],
): { added: number; removed: number } | null {
  let added = 0;
  let removed = 0;
  let known = false;
  for (const t of tools) {
    if (t.added != null || t.removed != null) {
      known = true;
      added += t.added ?? 0;
      removed += t.removed ?? 0;
    }
  }
  return known ? { added, removed } : null;
}

/** O que se desenha para um conjunto de chamadas.
 *
 *  Três estados e não dois, porque uma chamada só **não tem** estado aberto ou
 *  fechado: é uma ficha, e a ficha já se abre sozinha quando tem saída para
 *  mostrar. Misturar as duas coisas foi o bug: uma chamada única nascia com
 *  cabeçalho *e* aberta, e fechá-la trocava o componente inteiro por uma ficha
 *  nua — o cartão ficava e o "Ran 1 command" desaparecia, que é o contrário do
 *  que um botão de fechar promete.
 *
 *  `chosen` é o que o operador carregou, ou `null` enquanto não carregou nada.
 *  Por omissão um grupo a meio está aberto — o que está a acontecer agora não
 *  se esconde atrás de um resumo — e um grupo acabado está fechado.
 */
export type GroupView = "chip" | "open" | "closed";

export function groupView(
  count: number,
  flying: boolean,
  chosen: boolean | null,
): GroupView {
  if (count <= 1) return "chip";
  return (chosen ?? flying) ? "open" : "closed";
}

/** O caminho que a marcação diz, como o disco o escreve.
 *
 *  Um `src` de markdown é uma **URL**, portanto o espaço de "Application
 *  Support" chega aqui como `%20` — e `%20` não é um ficheiro. Era o bug: a
 *  imagem existia, o caminho estava certo, e o que se pedia ao Rust era um
 *  caminho que nunca existiu.
 *
 *  Um caminho com um `%` literal faz o `decodeURIComponent` rebentar; nesse
 *  caso fica o que veio, que é a leitura certa — ninguém o codificou. */
export function decodePath(src: string): string {
  try {
    return decodeURIComponent(src);
  } catch {
    return src;
  }
}

/** O que dizer enquanto se espera.
 *
 *  "Thinking…" era a única resposta, e quase sempre a errada: o modelo passa a
 *  maior parte do tempo a correr comandos, não a pensar. Um indicador que diz
 *  sempre a mesma coisa deixa de ser informação e passa a ser um sinal de vida
 *  — e um sinal de vida que mente sobre o que está a acontecer é pior do que um
 *  ponto a girar.
 *
 *  A ordem é a da verdade disponível: o raciocínio a sério quando o há, o nome
 *  do que está a correr quando há uma chamada no ar, e "Working…" só quando não
 *  se sabe nada — que é o caso curto entre mandar a mensagem e o modelo abrir a
 *  boca. */
export function workingLabel(thinking: string, inFlight: ChatMsg | null): string {
  const thought = thinking.trim();
  if (thought) return thought;
  if (!inFlight) return "Working…";
  const what = (inFlight.text ?? "").trim();
  const verb = (() => {
    const tool = inFlight.tool ?? "";
    if (/^bash$/i.test(tool)) return "Running";
    if (/^(edit|multiedit|write|notebookedit)$/i.test(tool)) return "Editing";
    if (/^read$/i.test(tool)) return "Reading";
    if (/^(grep|glob|search)$/i.test(tool)) return "Searching";
    if (/^(webfetch|websearch)$/i.test(tool)) return "Looking up";
    return null;
  })();
  if (!verb) return `${inFlight.tool ?? "Working"}…`;
  return what ? `${verb} ${what}…` : `${verb}…`;
}
