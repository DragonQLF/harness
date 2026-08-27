import { query, createSdkMcpServer, tool } from "@anthropic-ai/claude-agent-sdk";
import readline from "node:readline";
import { randomUUID } from "node:crypto";
import { z } from "zod";
import { inspect } from "./pathguard.mjs";
import { summarizeUse, summarizeResult, detailOf } from "./toolsum.mjs";

const controllers = new Map();
const approvals = new Map();
/** Harness tool calls waiting on an answer from the Rust side. */
const toolCalls = new Map();

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

/** Ask the app to do something, and wait for it to answer. */
function callHarness(runId, name, input) {
  const request_id = randomUUID();
  const answer = new Promise((resolve) => toolCalls.set(request_id, resolve));
  send({ type: "tool_request", request_id, run_id: runId, name, input });
  return answer.then((reply) => {
    toolCalls.delete(request_id);
    return {
      content: [{ type: "text", text: reply?.text ?? "no answer from Harness" }],
      isError: reply?.ok === false,
    };
  });
}

/** Every harness tool routes its calls through here. One bridge per process;
 *  replies come back as tool_response messages matched by request id. */
function callFor(runId) {
  return (name) => (args) => callHarness(runId, name, args);
}

/** The board actions and navigation the Director is allowed to perform. Every
 *  one of these is a tool the agent does not hold by default, so it goes through
 *  `canUseTool` — the operator sees it before it happens. */
function harnessTools(runId) {
  const call = callFor(runId);
  return createSdkMcpServer({
    name: "harness",
    version: "1.0.0",
    tools: [
      tool(
        "move_card",
        "Move a card to another column on the board: later, ready, running, review or done.",
        {
          card_id: z.string().describe("The card id, for example c_7b30"),
          to: z.enum(["later", "ready", "running", "review", "done"]),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("move_card"),
      ),
      tool(
        "create_card",
        "Add a card to the board. Keep it small enough for one agent run.",
        {
          title: z.string().describe("What should happen, in one line"),
          agent_id: z.string().optional().describe("Which agent takes it, default builder"),
          start: z.boolean().optional().describe("Hand it to the agent immediately"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("create_card"),
      ),
      tool(
        "create_agent",
        "Add an agent to the crew, when the operator asks for one. It starts able to read and search only; widening that is theirs to do on the Agents screen.",
        {
          name: z.string().describe("What to call it, for example Scout"),
          title: z.string().optional().describe("One line on what it is for"),
          brief: z.string().optional().describe("What it is told before every run"),
          model: z
            .string()
            .optional()
            .describe("Model name the endpoint knows, for example qwen3.5 or anthropic/claude-opus-5"),
          provider: z
            .string()
            .optional()
            .describe(
              "Which configured model endpoint it runs on, by id. Omit for the Claude login this machine already has.",
            ),
        },
        call("create_agent"),
      ),
      tool(
        "edit_agent",
        "Change an existing agent's profile when the operator asks: what it is called, what it is for, its brief, its budget, who reviews it, or whether it is paused. Tools and permissions are not editable here.",
        {
          agent_id: z.string().describe("Which agent, by id"),
          name: z.string().optional(),
          title: z.string().optional().describe("One line on what it is for"),
          brief: z.string().optional().describe("What it is told before every run"),
          budget_usd: z.number().optional().describe("Dollar ceiling for one run"),
          paused: z.boolean().optional().describe("A paused agent starts no runs"),
          reviewer: z
            .enum(["director", "human", "nobody"])
            .optional()
            .describe("Who reads the diff when a run finishes"),
        },
        call("edit_agent"),
      ),
      tool(
        "set_agent_model",
        "Point an existing agent at a different model, or a different endpoint, when the operator asks.",
        {
          agent_id: z.string().describe("Which agent, by id"),
          model: z.string().optional().describe("Model name the endpoint knows"),
          provider: z
            .string()
            .optional()
            .describe("Endpoint id, or 'anthropic' for the Claude login this machine already has"),
        },
        call("set_agent_model"),
      ),
      tool(
        "approve_card",
        "Approve a card that is waiting for review, sending it to done.",
        {
          card_id: z.string(),
          reason: z.string().describe("Why it holds up"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("approve_card"),
      ),
      tool(
        "reject_card",
        "Send a card in review back to ready, with a reason the agent will read.",
        {
          card_id: z.string(),
          reason: z.string().describe("What has to change"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("reject_card"),
      ),
      tool(
        "delete_card",
        "Delete a card and its worktree for good. Refuses while a card is running.",
        {
          card_id: z.string(),
          reason: z.string().optional().describe("Why it is going"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("delete_card"),
      ),
      tool(
        "read_diff",
        "Read what a card's run actually changed. Use this instead of guessing.",
        {
          card_id: z.string(),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("read_diff"),
      ),
      tool(
        "list_projects",
        "List every project Harness knows about, with the id to use for the other tools. " +
          "Use this rather than guessing an id, and before saying a project does or does not exist.",
        {},
        call("list_projects"),
      ),
      tool(
        "create_project",
        "Create a new project: a git repository with its own board, initialised at " +
          "<parent_path>/<name>. Ask where it should live rather than choosing for them.",
        {
          name: z.string().describe("What the project is called"),
          parent_path: z
            .string()
            .describe("The existing folder to create it inside, as an absolute path"),
        },
        call("create_project"),
      ),
      tool(
        "record_decision",
        "Record a decision the moment it is made, into this project's memory. " +
          "Use it the instant you and the operator agree on something durable — " +
          "then say you recorded it.",
        {
          title: z.string().describe("One line naming the decision"),
          content: z
            .string()
            .describe("The decision, its context, and what it rules out"),
        },
        call("record_decision"),
      ),
      tool(
        "self_report",
        "What happened to you and to every agent lately, counted: tool refusals by reason, " +
          "approvals that expired unanswered, failed runs (budget cuts named apart), commit " +
          "errors, unreported work, cards sent back from review. Call it when asked what has " +
          "been going wrong or repeating — never guess these numbers. Counts and one example " +
          "per pattern; there is no raw log behind it.",
        {
          days: z
            .number()
            .int()
            .min(1)
            .max(30)
            .optional()
            .describe("How far back to count, in days. Default 7."),
        },
        call("self_report"),
      ),
      tool(
        "read_docs",
        "Read Harness's own records: doc \"debt\" is DEBT.md — what is known-broken or " +
          "deliberately deferred — and doc \"decisions\" is DECISIONS.md, the numbered history " +
          "of how the app was built and why. Use \"find\" to pull specific sections (a number " +
          "like 75, or a few words) instead of reading the whole log. Check debt before " +
          "proposing anything, so you do not propose what is already tracked.",
        {
          doc: z.enum(["debt", "decisions"]),
          find: z
            .string()
            .optional()
            .describe("A section to find: a decision number or words from its title"),
        },
        call("read_docs"),
      ),
      tool(
        "propose_improvement",
        "File an improvement proposal in the operator's inbox when you can see a pattern that " +
          "repeats and has an obvious correction. A proposal is not a card: they decide whether " +
          "it becomes work, so NEVER create the card yourself and never act on your own " +
          "suggestion. Cite counts from self_report as the observation.",
        {
          title: z.string().describe("One line naming the problem"),
          observation: z
            .string()
            .describe("The evidence: which counts repeat, from self_report"),
          proposal: z.string().describe("The correction you suggest"),
        },
        call("propose_improvement"),
      ),
      tool(
        "open_screen",
        "Take the operator to a place in the app — this navigates their window immediately. " +
          "Whenever they ask to see, show, find or open anything, call this FIRST and then say " +
          "what they are looking at. Never describe which buttons to click instead: pointing at " +
          "the screen IS the answer. Changes nothing, so use it freely.",
        {
          screen: z
            .enum([
              "home",
              "work",
              "board",
              "sessions",
              "runs",
              "code",
              "worktrees",
              "activity",
              "agents",
              "projects",
              "settings",
              "director",
            ])
            .describe("Which screen to open"),
          card_id: z.string().optional().describe("A card to select, for board or runs"),
          why: z.string().optional().describe("One line shown to the operator"),
        },
        call("open_screen"),
      ),
    ],
  });
}

/** The one tool a worker run carries: its own account of the work it did.
 *  The engine still owns the commit — this only feeds it, and records the
 *  durable notes for the memory layer. Absence of a call is normal and safe.
 *  `call` is injected: a free reference here passed node --check and only
 *  exploded on the first real worker run. */
function reportWorkTool(runId, call) {
  return createSdkMcpServer({
    name: "harness",
    version: "1.0.0",
    tools: [
      tool(
        "report_work",
        "Report what you did, once, when your work for the card is done. " +
          "The summary becomes the body of Harness's commit; the memory notes " +
          "are durable facts that outlive the card. Distinction: if it stops " +
          "being true when the code changes, it belongs in the summary; if it " +
          "is a decision or convention that lasts, it belongs in memory_notes.",
        {
          summary: z
            .string()
            .describe(
              "What changed and why, in two or three sentences. Becomes the commit body."
            ),
          memory_notes: z
            .array(z.string())
            .describe(
              "Durable facts, decisions or conventions worth remembering after this card is done. Empty array if none."
            ),
        },
        call("report_work"),
      ),
    ],
  });
}

async function handleRun({ id, spec }) {
  const ac = new AbortController();
  controllers.set(id, ac);

  // tool_use_id → tool name: the result block only knows the id, and the
  // summary needs the name to be worth reading.
  const toolNames = new Map();
  let turnCount = 0;

  // Fan-out cap: a run may spawn subagents only when its spec allows it, and
  // a subagent may never spawn one — depth is capped at one level. The
  // counter rises when a Task is approved and falls on the SDK's PostToolUse
  // hook; if that hook never fires (a crash mid-task), the counter stays up,
  // which denies further spawns. Failing closed is the safe direction.
  let childDepth = 0;
  const canUseTool = async (toolName, input) => {
    // The frozen zone is a path comparison, not a list of modules: a run
    // writes inside its own worktree and nowhere else. Every tool with
    // path-bearing input is inspected — known or not; only reads and
    // Harness's own MCP tools are exempt. Checked before the approval flow,
    // because a refusal here is not a question for the operator — and the
    // transcript names the path that was refused.
    // Native terminal-era tools assume a human in front of a terminal. Here
    // the human may be anywhere, so a silent disappearance is #41's shape:
    // name it on the transcript, refuse with a readable reason, and let the
    // agent ask in text instead of deciding by omission.
    if (toolName === "AskUserQuestion") {
      send({
        type: "event",
        run_id: id,
        event: {
          kind: "notice",
          text: "the agent tried to ask you a question through a terminal-only tool; refused — it should ask in text instead",
        },
      });
      return {
        behavior: "deny",
        message:
          "there is no way to show this question to the operator right now. Say what you need to know in plain text and wait for their reply.",
      };
    }

    const verdict = inspect(toolName, spec.cwd, input);
    if (!verdict.skip && !verdict.ok) {
      const detail = verdict.path
        ? `refused: ${verdict.path}`
        : "this run's worktree could not be resolved";
      send({
        type: "event",
        run_id: id,
        event: {
          kind: "notice",
          text: `write refused outside this run's worktree — ${detail}`,
        },
      });
      return {
        behavior: "deny",
        message: `runs may only write inside their worktree (${spec.cwd}); ${detail}`,
      };
    }

    if (toolName === "Task") {
      if (!spec.subagents) {
        return { behavior: "deny", message: "subagents are off for this run" };
      }
      if (childDepth > 0) {
        return {
          behavior: "deny",
          message: "subagents cannot spawn subagents; do the work directly",
        };
      }
    }

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
    if (toolName === "Task") childDepth++;
    return { behavior: "allow", updatedInput: input };
  };

  const options = {
    cwd: spec.cwd,
    abortController: ac,
    // Token-level events, so the app can show the answer as it is written
    // instead of in one lump when the turn ends.
    includePartialMessages: true,
    // Harness runs are isolated: no filesystem settings, and only the MCP
    // servers we pass (none). Without this the operator's account connectors
    // load and the model starts talking about authorising Linear or Notion.
    settingSources: [],
    mcpServers: spec.harness_tools
      ? { harness: harnessTools(id) }
      : spec.report_work
        ? { harness: reportWorkTool(id, callFor(id)) }
        : {},
    strictMcpConfig: true,
    canUseTool,
    hooks: {
      PostToolUse: [
        {
          matcher: "Task",
          hooks: [
            async () => {
              childDepth = Math.max(0, childDepth - 1);
              return {};
            },
          ],
        },
      ],
    },
  };

  if (spec.permission_mode) options.permissionMode = spec.permission_mode;
  if (Array.isArray(spec.allowed_tools) && spec.allowed_tools.length > 0) {
    options.allowedTools = spec.allowed_tools;
  }
  if (spec.model) options.model = spec.model;
  if (spec.max_budget_usd != null) options.maxBudgetUsd = spec.max_budget_usd;
  // No room to reason means no thinking to stream.
  if (spec.thinking_tokens != null) options.maxThinkingTokens = spec.thinking_tokens;
  if (spec.resume_session) options.resume = spec.resume_session;

  try {
    const q = query({ prompt: spec.prompt, options });
    for await (const message of q) {
      if (ac.signal.aborted) break;
      switch (message.type) {
        case "stream_event": {
          // Raw Anthropic deltas: text as it is written, and the thinking that
          // precedes it. Ephemeral — the final assistant message is what gets
          // logged.
          const ev = message.event;
          if (ev?.type === "content_block_delta") {
            const d = ev.delta;
            if (d?.type === "text_delta" && d.text) {
              send({ type: "event", run_id: id, event: { kind: "delta", text: d.text } });
            } else if (d?.type === "thinking_delta" && d.thinking) {
              send({
                type: "event",
                run_id: id,
                event: { kind: "thinking", text: d.thinking },
              });
            }
          }
          break;
        }
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
          // One assistant message is one model turn. Emitted live so the
          // card shows progress toward the ceiling before the result lands.
          turnCount++;
          send({ type: "event", run_id: id, event: { kind: "turns", count: turnCount } });
          const parent = message.parent_tool_use_id ?? null;
          for (const block of message.message?.content ?? []) {
            if (block.type === "text" && block.text?.trim()) {
              send({ type: "event", run_id: id, event: { kind: "text", text: block.text } });
            } else if (block.type === "tool_use") {
              if (block.id) toolNames.set(block.id, block.name);
              send({
                type: "event",
                run_id: id,
                event: {
                  kind: "tool_use",
                  tool: block.name,
                  summary: summarizeUse(block.name, block.input),
                  tool_use_id: block.id ?? null,
                  parent_tool_use_id: parent,
                },
              });
            }
          }
          break;
        }
        case "user": {
          // Tool results arrive as user messages whose content blocks carry
          // the same ids the calls minted. Without these the transcript shows
          // what the agent tried, never what happened (#41's shape again).
          for (const block of message.message?.content ?? []) {
            if (block.type !== "tool_result") continue;
            const name = toolNames.get(block.tool_use_id) ?? "";
            const text = typeof block.content === "string"
              ? block.content
              : (block.content ?? [])
                  .filter((c) => c.type === "text")
                  .map((c) => c.text)
                  .join("\n");
            send({
              type: "event",
              run_id: id,
              event: {
                kind: "tool_result",
                tool_use_id: block.tool_use_id,
                ok: !block.is_error,
                summary: summarizeResult(name, text, !!block.is_error),
                detail: detailOf(text),
              },
            });
          }
          break;
        }
        case "result": {
          controllers.delete(id);
          // An error result looks like a normal one from the outside: same
          // `result` message, cost 0, no text. A resume of a session that no
          // longer exists arrives exactly this way, and the SDK only throws
          // afterwards — by which time we have already returned. So the
          // failure has to be read off this message, or it passes for success.
          const failed = message.is_error === true || message.subtype !== "success";
          const errors = Array.isArray(message.errors)
            ? message.errors.map((e) => String(e).trim()).filter(Boolean).join("; ")
            : "";
          const detail =
            errors ||
            (typeof message.result === "string" && message.result.trim()) ||
            message.subtype ||
            "the run ended with an error";
          send({
            type: "event",
            run_id: id,
            event: {
              kind: "done",
              session_id: message.session_id ?? null,
              cost_usd: message.total_cost_usd ?? null,
              turns: typeof message.num_turns === "number" ? message.num_turns : null,
              result: typeof message.result === "string" ? message.result : null,
              error: failed ? detail : null,
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
    case "tool_response": {
      toolCalls.get(msg.request_id)?.({ ok: msg.ok !== false, text: msg.text ?? "" });
      break;
    }
  }
});
