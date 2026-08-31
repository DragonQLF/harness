#!/usr/bin/env node
/**
 * O `latest.json` diz a verdade sobre os ficheiros que estão ao lado dele?
 *
 * Toda a segurança da actualização assenta numa assinatura: a app descarrega o
 * que este ficheiro aponta e recusa-o se a assinatura não bater certo. Se as
 * duas coisas vierem de sítios diferentes, o resultado não é uma instalação
 * má — é nenhuma instalação, com um "signature verification failed" que não
 * diz de onde vem.
 *
 * Aconteceu na 0.3.17: a entrada de darwin trazia uma assinatura que não
 * correspondia a nenhum `.sig` publicado, enquanto as de Windows batiam certo.
 * A release parecia completa — sete ficheiros, todos assinados — e mesmo assim
 * nenhum Mac a conseguia instalar. Nada no CI notava, porque cada metade estava
 * bem por si; o que estava errado era a relação entre elas.
 *
 * Correr antes de publicar o rascunho:
 *   node scripts/check-release.mjs v0.3.18
 */
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const tag = process.argv[2];
if (!tag) {
  console.error("uso: node scripts/check-release.mjs <tag>");
  process.exit(2);
}

const dir = mkdtempSync(join(tmpdir(), "relay-release-"));
execFileSync("gh", ["release", "download", tag, "-D", dir, "--clobber"], {
  stdio: ["ignore", "ignore", "inherit"],
});

const files = readdirSync(dir);
const manifest = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
const sigs = new Map(
  files
    .filter((f) => f.endsWith(".sig"))
    .map((f) => [f, readFileSync(join(dir, f), "utf8").trim()]),
);

const problems = [];

const version = tag.replace(/^v/, "");
if (manifest.version !== version) {
  problems.push(`latest.json diz ${manifest.version}, a tag diz ${version}`);
}

for (const [platform, entry] of Object.entries(manifest.platforms ?? {})) {
  const asset = decodeURIComponent(entry.url.split("/").pop() ?? "");
  if (!files.includes(asset)) {
    problems.push(`${platform}: aponta para ${asset}, que não está na release`);
    continue;
  }
  const signature = (entry.signature ?? "").trim();
  if (!signature) {
    problems.push(`${platform}: sem assinatura`);
    continue;
  }
  const beside = sigs.get(`${asset}.sig`);
  if (beside === undefined) {
    problems.push(`${platform}: falta ${asset}.sig`);
  } else if (beside !== signature) {
    // A avaria da 0.3.17, exactamente nesta forma.
    problems.push(
      `${platform}: a assinatura em latest.json não é a de ${asset}.sig — ` +
        `o ficheiro e a assinatura vêm de builds diferentes`,
    );
  }
}

// Uma plataforma em falta é uma plataforma que nunca recebe a actualização, e
// isso não dá erro nenhum a ninguém: fica simplesmente para trás, em silêncio.
for (const expected of ["darwin-aarch64", "darwin-x86_64", "windows-x86_64"]) {
  if (!manifest.platforms?.[expected]) problems.push(`falta a plataforma ${expected}`);
}

if (problems.length) {
  console.error(`${tag}: NÃO publicar\n`);
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}

console.log(`${tag}: ${Object.keys(manifest.platforms).length} plataformas, todas`);
console.log("assinadas pelo ficheiro que está ao lado delas. Pode publicar-se.");
