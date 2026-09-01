/** Regressão: uma resposta partida ao meio pela pergunta seguinte.
 *
 *  A escrita ao vivo colava-se à última mensagem do fio. Bastava dizer alguma
 *  coisa enquanto o agente escrevia — que é o que a fila serve para permitir —
 *  para o balão da operadora passar a ser o último, e o resto da resposta em
 *  curso abria um balão novo *por baixo* dela. A mesma resposta ficava aos
 *  bocados, com a interrupção pelo meio, e lia-se como se o agente tivesse
 *  respondido duas vezes a coisas diferentes.
 *
 *  O que aqui se guarda é a ordem, que é a única coisa que uma transcrição
 *  promete. */

import test from "node:test";
import assert from "node:assert/strict";
import { appendStreamed, alreadySaid, type ChatMsg } from "./bubbles.ts";

const ids = (n = 0) => () => `id${++n}`;
const clock = () => 1000;

const user = (text: string): ChatMsg => ({ role: "user", text, ts: 1 });

test("o primeiro pedaço abre um balão no fim", () => {
  const { list, streamId } = appendStreamed([user("olá")], null, "Vou", ids(), clock);
  assert.equal(list.length, 2);
  assert.equal(list[1].role, "agent");
  assert.equal(list[1].text, "Vou");
  assert.equal(streamId, "id1");
});

test("os pedaços seguintes juntam-se ao mesmo balão", () => {
  const mint = ids();
  let { list, streamId } = appendStreamed([], null, "Vou ", mint, clock);
  ({ list, streamId } = appendStreamed(list, streamId, "já", mint, clock));
  assert.equal(list.length, 1, "um balão, não dois");
  assert.equal(list[0].text, "Vou já");
});

test("uma pergunta a meio não parte a resposta em curso", () => {
  const mint = ids();
  let { list, streamId } = appendStreamed([], null, "Estou a ", mint, clock);
  // A operadora escreve enquanto ele responde: o balão dela vai para o fim.
  list = [...list, user("espera")];
  ({ list, streamId } = appendStreamed(list, streamId, "meio disto", mint, clock));

  assert.equal(list.length, 2, "continua a haver um balão do agente, não dois");
  assert.equal(list[0].text, "Estou a meio disto", "a resposta continua inteira");
  assert.equal(list[1].role, "user", "e a pergunta fica por baixo dela");
});

test("depois de o modelo a ler, a resposta a ela começa por baixo", () => {
  const mint = ids();
  let { list, streamId } = appendStreamed([], null, "primeira", mint, clock);
  list = [...list, user("afinal faz outra coisa")];
  // `user_read`: o turno anterior fecha, e o que vier a seguir é a resposta a
  // esta — que pertence *depois* dela.
  ({ list, streamId } = appendStreamed(list, null, "segunda", mint, clock));

  assert.deepEqual(
    list.map((m) => `${m.role}:${m.text}`),
    ["agent:primeira", "user:afinal faz outra coisa", "agent:segunda"],
  );
  assert.notEqual(streamId, "id1", "balão novo, turno novo");
});

test("um balão que já não está no fio não é ressuscitado", () => {
  // Trocar de conversa esvazia o fio; o id que ficou na mão não pode fazer o
  // texto novo aparecer numa conversa onde aquele balão nunca existiu.
  const { list } = appendStreamed([], "de-outra-conversa", "texto", ids(), clock);
  assert.equal(list.length, 1);
  assert.equal(list[0].streamId, "id1", "abre um balão desta, não procura o de lá");
});

test("só se escreve em balões do agente", () => {
  // Um recibo de ferramenta com o mesmo id nunca deve receber texto: são
  // balões diferentes e desenham-se de maneira diferente.
  const tool: ChatMsg = { role: "tool", text: "Bash", ts: 1, streamId: "id1" };
  const { list } = appendStreamed([tool], "id1", "texto", ids(), clock);
  assert.equal(list.length, 2, "abre um balão do agente em vez de sujar o recibo");
  assert.equal(list[0].text, "Bash");
});

/** Regressão: a mesma resposta desenhada duas vezes.
 *
 *  Um `text` chega por duas vias — o evento ao vivo e a linha lida do disco — e
 *  as duas são entregas do mesmo registo. O ecrã juntava-as por acrescento
 *  cego, e a ordem decidia: lida a transcrição primeiro (que é o que mandar uma
 *  mensagem faz), a entrega atrasada acrescentava a resposta outra vez, igual
 *  palavra por palavra e com o mesmo carimbo.
 *
 *  Foi visto assim: a mesma resposta duas vezes, ambas às 23:07 — e no disco a
 *  linha existia **uma** vez só, que é o que prova que o defeito era do
 *  desenho e não da transcrição. */

const agent = (text: string, ts: number): ChatMsg => ({ role: "agent", text, ts });

test("a mesma linha, entregue outra vez, não se desenha outra vez", () => {
  const fio = [user("olá"), agent("Onde chegámos: repos do GitHub.", 1788294000000)];
  assert.equal(
    alreadySaid(fio, "Onde chegámos: repos do GitHub.", 1788294000000),
    true,
    "é a mesma linha: mesmo instante, mesmo texto",
  );
});

test("a mesma frase dita outra vez mais tarde é uma linha nova", () => {
  const fio = [agent("Vou tentar.", 1788294000000)];
  assert.equal(
    alreadySaid(fio, "Vou tentar.", 1788294005000),
    false,
    "cinco segundos depois é outra coisa que aconteceu, não um eco",
  );
});

test("um texto diferente no mesmo instante continua a entrar", () => {
  const fio = [agent("Vou tentar.", 1788294000000)];
  assert.equal(alreadySaid(fio, "Já está.", 1788294000000), false);
});

test("o que o operador escreveu não conta como resposta repetida", () => {
  // Uma pergunta e uma resposta podem partilhar o carimbo e o texto sem serem
  // a mesma coisa — é o papel que as separa.
  const fio: ChatMsg[] = [{ role: "user", text: "pronto?", ts: 1788294000000 }];
  assert.equal(alreadySaid(fio, "pronto?", 1788294000000), false);
});

test("um fio vazio não tem nada repetido", () => {
  assert.equal(alreadySaid([], "seja o que for", 1), false);
});
