/** Junta classes, deitando fora o que é falso.
 *
 *  Com o Tailwind uma classe passa a ser condicional onde antes era um ternário
 *  dentro de um objecto de estilo. Isto é o mínimo que torna isso legível sem
 *  trazer uma dependência para o fazer. */
export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}
