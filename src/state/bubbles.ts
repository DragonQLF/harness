/** Onde é que a escrita ao vivo assenta no fio.
 *
 *  Parece detalhe e não é: era aqui que uma resposta se partia ao meio. O texto
 *  colava-se sempre à *última* mensagem, portanto bastava a operadora dizer
 *  alguma coisa a meio do turno para o balão dela passar a ser o último — e o
 *  resto da resposta em curso abria um balão novo por baixo da interrupção. A
 *  mesma resposta ficava aos bocados, com a pergunta dela pelo meio.
 *
 *  Marcado o balão com o turno a que pertence, o turno escreve sempre no seu,
 *  esteja onde estiver no fio. A resposta interrompida continua *por cima* da
 *  interrupção, e só quando o modelo lê o que ela disse é que a resposta a
 *  *isso* começa por baixo — que é a ordem em que as coisas aconteceram.
 *
 *  Puro de propósito: é lógica de ordenação, e é a única maneira de a exercitar
 *  sem levantar a aplicação. */

export interface ChatMsg {
  /** `notice` is Relay itself talking: a failed resume, a cancelled turn.
   *  `tool` is what the agent tried (`summary`) — its result arrives as a
   *  second tool bubble matched by id, green or red, expandable.
   *  `thinking` is a sealed stretch of reasoning: collapsed to one line, opened
   *  when somebody wants to read it. */
  role: "user" | "agent" | "notice" | "tool" | "thinking";
  text: string;
  /** When it was said, so the transcript can date itself. */
  ts: number;
  /** Tool bubble only: which tool, whether its result closed it, and the
   *  full output kept for expansion (#28: never dumped inline). */
  tool?: string;
  ok?: boolean | null;
  detail?: string | null;
  toolUseId?: string | null;
  parentToolUseId?: string | null;
  /** Tool bubble only: lines this call adds and removes, when the call itself
   *  said so. Absent — never zero — for a tool that does not touch lines. */
  added?: number | null;
  removed?: number | null;
  /** User bubble only: this was said while a turn was already running, and the
   *  backend has not yet said the model read it. Never a guess — it is set from
   *  the id `chat_queue` answered with, and cleared by the `user_read` that
   *  names the same id. */
  queueId?: string | null;
  pending?: boolean;
  /** Balão do agente: a que turno pertence.
   *
   *  Sem isto, a escrita ao vivo colava-se sempre à *última* mensagem do fio —
   *  e assim que a operadora dizia alguma coisa a meio, o balão dela passava a
   *  ser o último e o resto da resposta em curso abria um balão novo por baixo.
   *  A mesma resposta ficava partida ao meio, com a interrupção pelo meio dela.
   *  Marcado, o turno escreve sempre no balão dele, esteja onde estiver. */
  streamId?: string | null;
}


export interface Placed {
  list: ChatMsg[];
  streamId: string;
}

/** Junta `text` ao balão deste turno, ou abre um se ainda não há.
 *
 *  `streamId` nulo quer dizer "turno novo". `mint` dá o nome ao balão novo —
 *  passado de fora para o teste não ter de adivinhar um id nem depender do
 *  relógio. */
export function appendStreamed(
  list: ChatMsg[],
  streamId: string | null,
  text: string,
  mint: () => string,
  now: () => number = Date.now,
): Placed {
  if (streamId) {
    const at = list.findIndex((m) => m.role === "agent" && m.streamId === streamId);
    if (at > -1) {
      const next = [...list];
      next[at] = { ...next[at], text: next[at].text + text };
      return { list: next, streamId };
    }
  }
  const id = mint();
  return {
    list: [...list, { role: "agent", text, ts: now(), streamId: id }],
    streamId: id,
  };
}
