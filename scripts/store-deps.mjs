#!/usr/bin/env node
/**
 * Guarda: o valor do contexto do store não pode ter um campo fora da lista de
 * dependências do seu `useMemo`.
 *
 * O objecto era construído de raiz a cada render, o que o tornava trivialmente
 * correcto e caríssimo: qualquer render do provider — um quadro de tokens
 * incluído — re-renderizava todos os `useStore` da app (#99). Memoizá-lo troca
 * esse custo por um risco novo, e é um risco silencioso: um campo acrescentado
 * ao `Store` e esquecido na lista fica preso no valor com que entrou, e nem o
 * `tsc` nem os testes dizem nada. A pessoa que o descobre é o operador, meses
 * depois, com um painel que não actualiza.
 *
 * Regra: cada entrada do objecto tem de contribuir com algo para as
 * dependências — o nome, no caso das abreviadas; a origem, no caso de
 * `chat.x`/`feed.x`. Só as funções `() => …` escritas ali estão isentas, e só
 * porque o que fecham são setters do `useState`, cuja identidade o React
 * garante estável.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = fileURLToPath(new URL("..", import.meta.url));
const file = new URL("../src/state/store.tsx", import.meta.url);
const path = fileURLToPath(file);
const text = readFileSync(path, "utf8");
const source = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

/** O `useMemo(() => ({ … }), [ … ])` que devolve o valor do contexto. */
function findStoreMemo(node) {
  let found = null;
  const visit = (n) => {
    if (found) return;
    if (
      ts.isVariableDeclaration(n) &&
      n.name.getText(source) === "value" &&
      n.initializer &&
      ts.isCallExpression(n.initializer) &&
      n.initializer.expression.getText(source) === "useMemo"
    ) {
      found = n.initializer;
      return;
    }
    ts.forEachChild(n, visit);
  };
  visit(node);
  return found;
}

const memo = findStoreMemo(source);
if (!memo) {
  console.error("store-deps: não encontrei o `const value: Store = useMemo(...)` em src/state/store.tsx.");
  console.error("Se o store deixou de ser memoizado, esta verificação deixou de fazer sentido — apaga-a.");
  process.exit(1);
}

const [factory, depsArg] = memo.arguments;
if (!factory || !depsArg || !ts.isArrayLiteralExpression(depsArg)) {
  console.error("store-deps: o `useMemo` do store não tem a forma esperada (fábrica + array de dependências).");
  process.exit(1);
}

/** O objecto que a fábrica devolve, com ou sem parênteses. */
let body = factory.body;
while (ts.isParenthesizedExpression(body)) body = body.expression;
if (!ts.isObjectLiteralExpression(body)) {
  console.error("store-deps: a fábrica do `useMemo` não devolve um objecto literal directamente.");
  process.exit(1);
}

const deps = new Set(depsArg.elements.map((e) => e.getText(source)));
const missing = [];

for (const prop of body.properties) {
  const line = source.getLineAndCharacterOfPosition(prop.getStart(source)).line + 1;

  if (ts.isShorthandPropertyAssignment(prop)) {
    const name = prop.name.getText(source);
    if (!deps.has(name)) missing.push({ line, name, want: name });
    continue;
  }

  if (!ts.isPropertyAssignment(prop)) {
    missing.push({ line, name: prop.getText(source).slice(0, 40), want: "(forma não suportada)" });
    continue;
  }

  const name = prop.name.getText(source);
  const init = prop.initializer;

  // Uma função escrita aqui só pode fechar sobre setters do `useState`, cuja
  // identidade o React garante. Qualquer outra coisa lá dentro seria uma
  // dependência por direito próprio, e é por isso que só estas passam.
  if (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) continue;

  // Nomes que a própria expressão ata — o `c` de `.find((c) => c.id === …)`.
  // Não vêm de fora, portanto não são dependências de nada.
  const bound = new Set();
  const collectBindings = (n) => {
    if (ts.isArrowFunction(n) || ts.isFunctionExpression(n)) {
      for (const param of n.parameters) {
        if (ts.isIdentifier(param.name)) bound.add(param.name.getText(source));
      }
    }
    ts.forEachChild(n, collectBindings);
  };
  collectBindings(init);

  // Todos os identificadores de raiz que a expressão lê de fora:
  // `chat.conversations` conta como `chat.conversations`, e
  // `chat.conversations.find(...)` também.
  const sources = new Set();
  const scan = (n) => {
    if (ts.isPropertyAccessExpression(n) && ts.isIdentifier(n.expression)) {
      const root = n.expression.getText(source);
      if (!bound.has(root)) sources.add(`${root}.${n.name.getText(source)}`);
    }
    ts.forEachChild(n, scan);
  };
  scan(init);

  if (sources.size === 0) {
    missing.push({ line, name, want: "(expressão que não lê nada — verifica à mão)" });
    continue;
  }
  for (const s of sources) {
    if (!deps.has(s)) missing.push({ line, name, want: s });
  }
}

if (missing.length === 0) {
  console.log(
    `OK — os ${body.properties.length} campos do store estão todos cobertos pelas ${deps.size} dependências do useMemo.`,
  );
  process.exit(0);
}

console.error(
  `${missing.length} campo(s) do store fora da lista de dependências do useMemo — ficam presos no valor antigo:\n`,
);
for (const m of missing) {
  console.error(`  src/state/store.tsx:${m.line}  ${m.name}  →  falta \`${m.want}\` nas dependências`);
}
console.error(`\nAcrescenta-as ao array de dependências do \`const value: Store = useMemo(...)\`.`);
process.exit(1);
