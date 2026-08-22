import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const child = spawn(process.execPath, [join(import.meta.dirname, "index.mjs")], {
  stdio: ["pipe", "pipe", "inherit"],
});

let buffer = "";
child.stdout.on("data", (chunk) => {
  buffer += chunk.toString();
  let idx;
  while ((idx = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, idx);
    buffer = buffer.slice(idx + 1);
    if (!line.trim()) continue;
    const msg = JSON.parse(line);
    onMessage(msg);
  }
});

const approvalsSeen = new Set();

function onMessage(msg) {
  if (msg.type === "event") {
    console.log("[event]", JSON.stringify(msg.event).slice(0, 220));
    return;
  }
  if (msg.type === "approval_request") {
    console.log("[approval_request]", msg.tool, JSON.stringify(msg.input).slice(0, 160));
    approvalsSeen.add(msg.request_id);
    setTimeout(() => {
      child.stdin.write(
        JSON.stringify({ type: "approval_response", request_id: msg.request_id, allow: true }) + "\n",
      );
      console.log("[driver] approved", msg.request_id.slice(0, 8));
    }, 300);
  }
}

function send(obj) {
  child.stdin.write(JSON.stringify(obj) + "\n");
}

const dir = mkdtempSync(join(tmpdir(), "sidecar-smoke-"));
writeFileSync(join(dir, "seed.md"), "# seed\n");

console.log("=== scenario 1: plain prompt, subscription auth check ===");
send({
  type: "run",
  id: "r1",
  spec: {
    prompt: "Reply with exactly: ok",
    cwd: dir,
    permission_mode: "acceptEdits",
  },
});

await new Promise((resolve) => {
  const t = setInterval(() => {}, 1000);
  process.once("message", resolve);
  setTimeout(resolve, 60000);
});

console.log("=== scenario 2: write OUTSIDE cwd forces canUseTool ===");
const outside = tmpdir();
send({
  type: "run",
  id: "r2",
  spec: {
    prompt: `Create a file at ${join(outside, "harness-sidecar-proof.txt")} containing the word proof. Then reply done.`,
    cwd: dir,
    permission_mode: "acceptEdits",
  },
});

await new Promise((resolve) => {
  const check = setInterval(() => {
    if (existsSync(join(outside, "harness-sidecar-proof.txt"))) {
      clearInterval(check);
      setTimeout(() => resolve(), 4000);
    }
  }, 500);
  setTimeout(resolve, 90000);
});

console.log("=== file written outside cwd:", existsSync(join(outside, "harness-sidecar-proof.txt")));
child.kill();
process.exit(0);
