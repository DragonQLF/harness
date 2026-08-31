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
