import test from "node:test";
import assert from "node:assert/strict";
import { summarizeUse, summarizeResult, detailOf } from "./toolsum.mjs";

test("use summaries name the attempt per tool", () => {
  assert.equal(
    summarizeUse("Edit", { file_path: "C:/wt/src/lib.rs" }),
    "lib.rs",
  );
  assert.equal(
    summarizeUse("Bash", { command: "cargo test -p harness-engine" }),
    "cargo test -p harness-engine",
  );
  assert.equal(
    summarizeUse("Task", { subagent_type: "scout", description: "find the race" }),
    "scout: find the race",
  );
});

test("a grant is summarised as the declaration, not as the intention", () => {
  // This line is what the operator reads on the approval sheet. It has to say
  // what the grant gives and to whom, because every grant otherwise looks the
  // same: one MCP call from the Director.
  const mcp = summarizeUse("mcp__harness__add_mcp_server", {
    agent_id: "designer",
    name: "figma",
    command: "npx",
    args: ["-y", "figma-mcp"],
    tools: ["get_file", "export_frame"],
    source: "https://example.invalid/figma",
  });
  assert.match(mcp, /add the MCP server "figma" to designer/);
  assert.match(mcp, /npx -y figma-mcp/);
  assert.match(mcp, /grants get_file, export_frame/);

  // A declaration that names no tools must not read like a harmless one.
  assert.match(
    summarizeUse("mcp__harness__add_mcp_server", { agent_id: "designer", name: "x", url: "https://e.invalid" }),
    /no tools declared/,
  );

  const skill = summarizeUse("mcp__harness__install_skill", {
    agent_id: "designer",
    name: "colour-audit",
    source: "https://example.invalid/skills/colour",
    instructions: "abc",
  });
  assert.match(skill, /install the skill "colour-audit" on designer/);
  assert.match(skill, /example\.invalid/);
  assert.match(skill, /3 characters enter its prompt/);

  assert.equal(
    summarizeUse("mcp__harness__grant_agent_tools", { agent_id: "builder", tools: ["Read", "Shell"] }),
    "builder would hold Read, Shell afterwards",
  );
});

test("results distinguish failure from success and keep an excerpt", () => {
  const failed = summarizeResult("Bash", "error: link failed\nat main.rs", true);
  assert.match(failed, /^failed/);
  assert.match(failed, /link failed/);

  const ok = summarizeResult("Bash", "line1\nline2\nline3", false);
  assert.match(ok, /ok · 3 lines out/);

  const read = summarizeResult("Read", "a\nb\nc\nd\n e ", false);
  assert.equal(read, "read · 5 lines");
});

test("detail is capped so one runaway cannot bloat the log", () => {
  const big = "x".repeat(20_000);
  const d = detailOf(big, 8000);
  assert.ok(d.length < 8100);
  assert.ok(d.endsWith("[truncated]"));
});

/** Uma leitura de imagem guarda o caminho; as outras continuam a dizer só o
 *  nome. É o caminho que deixa a janela desenhá-la — o nome não abre nada. */
test("um Read de imagem guarda o caminho, e o resto continua pelo nome", () => {
  assert.equal(
    summarizeUse("Read", { file_path: "/tmp/shots/image-1788207889430.png" }),
    "/tmp/shots/image-1788207889430.png",
  );
  assert.equal(summarizeUse("Read", { file_path: "/a/b/notes.md" }), "notes.md");
  // Um SVG não conta: embutido é um documento que pode trazer script, e por
  // isso o `preview` também o recusa.
  assert.equal(summarizeUse("Read", { file_path: "/a/b/logo.svg" }), "logo.svg");
  // Escrever num ficheiro não é olhar para ele.
  assert.equal(summarizeUse("Write", { file_path: "/a/b/hero.png" }), "hero.png");
});
