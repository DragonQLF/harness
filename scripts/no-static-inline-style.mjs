#!/usr/bin/env node
/**
 * Guarda: nenhum `style={{ … }}` pode ser estático.
 *
 * Depois da migração para Tailwind, o inline só se justifica para valores que
 * o CSS não consegue saber de antemão — `style={{ width: `${pct}%` }}`. Um
 * objecto de estilo feito só de literais é uma classe que ninguém escreveu, e
 * sem esta verificação voltam a ser duzentas dentro de um mês.
 *
 * Regra: se o objecto de estilo não usa nenhuma variável, falha.
 * Correr com `--stats` só conta (estáticos vs dinâmicos) e nunca falha.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = fileURLToPath(new URL("..", import.meta.url));
const srcDir = join(root, "src");
const statsOnly = process.argv.includes("--stats");

/** Ficheiros .tsx debaixo de src/, sem os gerados. */
function files(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "generated" || entry === "node_modules") continue;
      out.push(...files(full));
    } else if (entry.endsWith(".tsx")) {
      out.push(full);
    }
  }
  return out;
}

/** Um valor é literal se nada nele depende do runtime. */
function isLiteral(node) {
  if (ts.isStringLiteral(node) || ts.isNumericLiteral(node)) return true;
  if (ts.isNoSubstitutionTemplateLiteral(node)) return true;
  if (node.kind === ts.SyntaxKind.TrueKeyword) return true;
  if (node.kind === ts.SyntaxKind.FalseKeyword) return true;
  if (node.kind === ts.SyntaxKind.NullKeyword) return true;
  if (ts.isPrefixUnaryExpression(node)) return isLiteral(node.operand);
  if (ts.isParenthesizedExpression(node)) return isLiteral(node.expression);
  if (ts.isAsExpression(node)) return isLiteral(node.expression);
  if (ts.isObjectLiteralExpression(node)) return isStaticObject(node);
  return false;
}

/** Um objecto de estilo é estático quando todas as suas partes são literais. */
function isStaticObject(obj) {
  if (obj.properties.length === 0) return false;
  for (const prop of obj.properties) {
    if (ts.isSpreadAssignment(prop)) return false;
    if (ts.isShorthandPropertyAssignment(prop)) return false;
    if (!ts.isPropertyAssignment(prop)) return false;
    if (ts.isComputedPropertyName(prop.name)) return false;
    if (!isLiteral(prop.initializer)) return false;
  }
  return true;
}

const offenders = [];
let staticCount = 0;
let dynamicCount = 0;

for (const file of files(srcDir)) {
  const text = readFileSync(file, "utf8");
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);

  const visit = (node) => {
    if (
      ts.isJsxAttribute(node) &&
      node.name.getText(source) === "style" &&
      node.initializer &&
      ts.isJsxExpression(node.initializer) &&
      node.initializer.expression &&
      ts.isObjectLiteralExpression(node.initializer.expression)
    ) {
      const obj = node.initializer.expression;
      if (isStaticObject(obj)) {
        staticCount += 1;
        const { line } = source.getLineAndCharacterOfPosition(obj.getStart(source));
        offenders.push({
          file: relative(root, file),
          line: line + 1,
          text: obj.getText(source).replace(/\s+/g, " ").slice(0, 96),
        });
      } else {
        dynamicCount += 1;
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

if (statsOnly) {
  console.log(`estáticos: ${staticCount}`);
  console.log(`dinâmicos: ${dynamicCount}`);
  console.log(`total:     ${staticCount + dynamicCount}`);
  process.exit(0);
}

if (offenders.length === 0) {
  console.log(`OK — nenhum style={{}} estático (${dynamicCount} dinâmicos, todos com variáveis).`);
  process.exit(0);
}

console.error(`${offenders.length} style={{}} estático(s) — isto é uma classe, não um estilo calculado:\n`);
for (const o of offenders) console.error(`  ${o.file}:${o.line}  ${o.text}`);
console.error(`\nPõe-no numa className do Tailwind, ou numa variante do ui.tsx.`);
process.exit(1);
