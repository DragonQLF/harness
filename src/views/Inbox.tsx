/** Inbox — o que o Director propõe, e o que o operador decide sobre isso.
 *
 *  Este ecrã faltava, e a falta era silenciosa. As propostas eram carregadas
 *  no arranque, actualizadas por evento, e postas no contexto do store — e
 *  nenhum componente as lia. O único sítio que as mostrava era o rail do
 *  RightNow, que o #89 tirou do código.
 *
 *  O resultado: o `propose_improvement` — que o *prompt do Director manda usar*
 *  sempre que uma ferramenta é recusada — escrevia para um sítio sem leitor.
 *  Quando isto foi escrito havia catorze por ler no disco, incluindo capacidades
 *  que ele tinha percebido faltarem-lhe e não tinha por onde dizer. O operador
 *  via os chips a passar na conversa e nunca mais nada.
 *
 *  Aceitar é permissão, não trabalho: nada é criado aqui. O Director pega nela
 *  no turno seguinte — e é por isso que a linha diz o que vai acontecer em vez
 *  de fingir que já aconteceu.
 */

import { useEffect, useState } from "react";
import { api, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { TONE } from "../lib/types";
import type { Proposal } from "../lib/generated/Proposal";
import { useStore } from "../state/store";
import { HOVER_ROW, Loading, PANEL, QUIET, StrongButton, truncate } from "../components/ui";

/** Há quanto tempo, na forma curta que o resto da app usa. */
function when(ms: number): string {
  const mins = Math.max(0, Math.round((Date.now() - ms) / 60000));
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

export function Inbox() {
  const { proposals, acceptProposal, dismissProposal, toast } = useStore();
  // O store já traz as propostas do arranque e do evento. Isto é a leitura
  // explícita, para quem chega ao ecrã depois de uma janela aberta há horas:
  // o evento pode ter passado enquanto ninguém olhava.
  const [loaded, setLoaded] = useState<Proposal[] | null>(null);

  useEffect(() => {
    api
      .inbox()
      .then(setLoaded)
      .catch((e) => {
        toast("bad", "Could not read the inbox", reason(e));
        setLoaded([]);
      });
  }, [toast]);

  // O que o store tem ganha assim que tem alguma coisa: chega por evento e é
  // mais novo do que qualquer leitura feita ao entrar.
  const rows = proposals.length > 0 ? proposals : loaded;
  if (!rows) return <Loading what="Reading the inbox" />;

  const waiting = rows.filter((p) => p.status === "open");
  const settled = rows.filter((p) => p.status !== "open");

  return (
    <div className="px-6.5 pb-7 pt-5.5">
      <p className="mb-4 mt-0 text-md text-text2 dark:text-text2-d">
        What the Director noticed about Relay itself and wants your permission on. Accepting is
        permission, not work — nothing is created here, and he picks it up on his next turn.
      </p>

      {waiting.length === 0 ? (
        <div
          className={cx(
            PANEL,
            "px-4.5 py-6 text-center text-md text-text3 dark:text-text3-d",
          )}
        >
          Nothing waiting.
          {settled.length > 0 ? ` ${settled.length} already decided.` : ""}
        </div>
      ) : (
        <div className={PANEL}>
          {waiting.map((p) => (
            <article
              key={p.id}
              className={cx(
                HOVER_ROW,
                "border-b border-line2 px-4.5 py-4 last:border-b-0 dark:border-line2-d",
              )}
            >
              <header className="mb-2 flex items-baseline gap-3">
                <h3 className={cx(truncate, "m-0 text-md font-bold")}>{p.title}</h3>
                <span className="shrink-0 text-sm text-text3 dark:text-text3-d">
                  {when(p.created_ms)}
                </span>
              </header>
              {/* O que se repete, e o que ele sugere sobre isso. Os dois, porque
                  uma sugestão sem a prova não é uma coisa sobre a qual se possa
                  decidir. */}
              <p className="mb-2 mt-0 text-md text-text2 dark:text-text2-d">{p.observation}</p>
              <p className="mb-3 mt-0 text-md">{p.proposal}</p>
              <div className="flex gap-2.5">
                <StrongButton label="Accept" onClick={() => acceptProposal(p.id)} />
                <button className={QUIET} onClick={() => dismissProposal(p.id)}>
                  Dismiss
                </button>
              </div>
            </article>
          ))}
        </div>
      )}

      {settled.length > 0 && (
        <>
          <h2 className="mb-2.5 mt-6 text-sm font-bold uppercase tracking-[.08em] text-text3 dark:text-text3-d">
            Decided
          </h2>
          <div className={PANEL}>
            {settled.map((p) => {
              const tone = p.status === "accepted" ? TONE.ok : TONE.neutral;
              return (
                <div
                  key={p.id}
                  className={cx(
                    HOVER_ROW,
                    "flex items-center gap-3 border-b border-line2 px-4.5 py-3 last:border-b-0 dark:border-line2-d",
                  )}
                >
                  <span className={cx(truncate, "flex-1 text-md")}>{p.title}</span>
                  {/* O cartão que saiu disto, quando já saiu algum: é a única
                      prova de que aceitar deu em trabalho. */}
                  {p.card_id && (
                    <span className="shrink-0 font-mono text-sm text-text3 dark:text-text3-d">
                      {p.card_id}
                    </span>
                  )}
                  <span
                    className={cx(
                      "shrink-0 rounded-full px-2.5 py-1 text-sm font-bold",
                      tone.soft,
                      tone.fg,
                    )}
                  >
                    {p.status}
                  </span>
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
