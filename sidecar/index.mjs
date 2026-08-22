import { query } from "@anthropic-ai/claude-agent-sdk";
import readline from "node:readline";
import { randomUUID } from "node:crypto";

const controllers = new Map();
const approvals = new Map();

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function summarize(input) {
  if (!input || typeof input !== "object") return String(input ?? "");
  return Object.entries(input)
    .slice(0, 3)
    .map(([k, v]) => {
      const rendered =
        typeof v === "string" ? v.slice(0, 80) : JSON.stringify(v)?.slice(0, 80);
      return `${k}: ${rendered}`;
    })
    .join(" | ");
}

async function handleRun({ id, spec }) {
  const ac = new AbortController();
  controllers.set(id, ac);

  const options = {
    cwd: spec.cwd,
    abortController: ac,
    canUseTool: async (toolName, input) => {
      const request_id = randomUUID();
      const decision = new Promise((resolve) => approvals.set(request_id, resolve));
      send({ type: "approval_request", request_id, run_id: id, tool: toolName, input });
      let allow;
      try {
        allow = await Promise.race([
          decision,
          new Promise((resolve) =>
            ac.signal.addEventListener("abort", () => resolve(false)),
          ),
        ]);
      } finally {
        approvals.delete(request_id);
      }
      if (!allow) return { behavior: "deny", message: "denied by operator" };
      return { behavior: "allow", updatedInput: input };
    },
  };

  if (spec.permission_mode) options.permissionMode = spec.permission_mode;
  if (Array.isArray(spec.allowed_tools) && spec.allowed_tools.length > 0) {
    options.allowedTools = spec.allowed_tools;
  }
  if (spec.model) options.model = spec.model;
  if (spec.max_budget_usd != null) options.maxBudgetUsd = spec.max_budget_usd;
  if (spec.resume_session) options.resume = spec.resume_session;

  try {
    const q = query({ prompt: spec.prompt, options });
    for await (const message of q) {
      if (ac.signal.aborted) break;
      switch (message.type) {
        case "system":
          if (message.subtype === "init" && message.session_id) {
            send({
              type: "event",
              run_id: id,
              event: { kind: "started", session_id: message.session_id },
            });
          }
          break;
        case "assistant": {
          for (const block of message.message?.content ?? []) {
            if (block.type === "text" && block.text?.trim()) {
              send({ type: "event", run_id: id, event: { kind: "text", text: block.text } });
            } else if (block.type === "tool_use") {
              send({
                type: "event",
                run_id: id,
                event: {
                  kind: "tool_use",
                  tool: block.name,
                  summary: summarize(block.input),
                },
              });
            }
          }
          break;
        }
        case "result": {
          controllers.delete(id);
          send({
            type: "event",
            run_id: id,
            event: {
              kind: "done",
              session_id: message.session_id ?? null,
              cost_usd: message.total_cost_usd ?? null,
              result: typeof message.result === "string" ? message.result : null,
            },
          });
          return;
        }
      }
    }
    controllers.delete(id);
    send({
      type: "event",
      run_id: id,
      event: { kind: "failed", message: "stream ended without result" },
    });
  } catch (err) {
    controllers.delete(id);
    const aborted = ac.signal.aborted || err?.name === "AbortError";
    send({
      type: "event",
      run_id: id,
      event: aborted
        ? { kind: "done", session_id: null, cost_usd: null, result: null }
        : { kind: "failed", message: String(err?.message ?? err) },
    });
  }
}

const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on("line", (line) => {
  let msg;
  try {
    msg = JSON.parse(line);
  } catch {
    return;
  }
  switch (msg.type) {
    case "run":
      handleRun(msg).catch((e) =>
        send({ type: "event", run_id: msg.id, event: { kind: "failed", message: String(e) } }),
      );
      break;
    case "cancel": {
      controllers.get(msg.id)?.abort();
      break;
    }
    case "approval_response": {
      approvals.get(msg.request_id)?.(!!msg.allow);
      break;
    }
  }
});
