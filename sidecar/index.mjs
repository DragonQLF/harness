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
/** Live runs that can still be spoken to, by run id. See `Inbox`. */
const inboxes = new Map();

function send(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

/** One user message, in exactly the shape the SDK writes for a string prompt.
 *
 *  Copied from the SDK's own one-shot path rather than invented: `streamInput`
 *  does no validation and no filling-in — it stringifies whatever the iterable
 *  yields straight onto the CLI's stdin — so a field guessed wrong here would
 *  be a field wrong on the wire. */
function userMessage(text) {
  return {
    type: "user",
    session_id: "",
    message: { role: "user", content: [{ type: "text", text }] },
    parent_tool_use_id: null,
  };
}

/** What the operator says while the turn is already running.
 *
 *  A string prompt is the SDK's one-shot form: one message in, and no way back
 *  in until the turn is over. An async iterable is *streaming input* — the SDK
 *  reads from it for as long as the run lives, so a message pushed here is
 *  written to the CLI's stdin immediately and the model picks it up at its
 *  next read, during the work rather than after it.
 *
 *  Nothing is acknowledged on `push`. The ack goes out when the `yield`
 *  returns, because that is the moment the SDK has actually written the
 *  message — until then the only honest thing to say is that Relay is still
 *  holding it.
 *
 *  `sent` is the count the run ends by. Every user message written to the CLI
 *  produces exactly one `result`, so the run is over when the results have
 *  caught up with the messages — and not on the first one, which is where the
 *  answer to a queued message would have been cut off. Measured, not assumed:
 *  a message handed over mid-turn came back as a second `init`, a second
 *  assistant message and a second `result` of its own, each with its own cost.
 *
 *  `close` discards rather than draining. A run that is over is over: a message
 *  yielded past that point would be written to a CLI nobody is reading any
 *  more, and reported as read while nothing ever answered it. Discarded, it
 *  goes back to the Rust side unacknowledged and becomes a turn of its own. */
class Inbox {
  constructor() {
    this.pending = [];
    this.wake = null;
    this.closed = false;
    this.sent = 1;
  }

  push(message) {
    if (this.closed) return false;
    this.pending.push(message);
    this.wake?.();
    return true;
  }

  /** Is the run still owed something — a message written and unanswered, or
   *  one about to be written? */
  owed(results) {
    return results < this.sent || this.pending.length > 0;
  }

  close() {
    this.closed = true;
    this.pending = [];
    this.wake?.();
  }

  async *stream(first, runId) {
    yield userMessage(first);
    while (!this.closed) {
      if (this.pending.length === 0) {
        await new Promise((resolve) => {
          this.wake = resolve;
        });
        this.wake = null;
        continue;
      }
      const next = this.pending.shift();
      this.sent++;
      yield userMessage(next.text);
      send({
        type: "event",
        run_id: runId,
        event: { kind: "message_read", message_id: next.id },
      });
    }
  }
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
        "Add a card to the board. Keep it small enough for one agent run. Say which column " +
          "it belongs in — asking for `later` and then moving it costs the operator two " +
          "approvals for one action.",
        {
          title: z.string().describe("What should happen, in one line"),
          agent_id: z.string().optional().describe("Which agent takes it, default builder"),
          column: z
            .enum(["later", "ready", "running"])
            .optional()
            .describe("Which column it is born in. Default ready; `running` starts it."),
          start: z.boolean().optional().describe("Hand it to the agent immediately"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
          proposal_id: z
            .string()
            .optional()
            .describe(
              "The accepted proposal this card carries out, for example prp_7b30. Pass it and " +
                "the acceptance stops being raised at you every turn.",
            ),
        },
        call("create_card"),
      ),
      tool(
        "message_agent",
        "Say something to an agent that is already working, without stopping it. It goes into " +
          "that run's inbox and the model reads it at its next natural read — so a correction " +
          "lands during the work instead of after it. Use it when you would otherwise let an " +
          "agent finish something you already know is wrong. Refused when nothing is running " +
          "on that card; then the card's own title is where the instruction belongs.",
        {
          card_id: z.string().describe("The card whose run you are talking to, for example c_7b30"),
          text: z
            .string()
            .describe("What it needs to know now, in one or two sentences"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("message_agent"),
      ),
      tool(
        "edit_card",
        "Correct what a card says, before anything has run on it. The title is the prompt the " +
          "agent receives, so a badly worded card is a badly worded instruction — fix it here " +
          "rather than deleting and recreating, which loses the card's id, its history and any " +
          "cards depending on it. Refused once the card has run: from then on the title is what " +
          "the transcript and the commit answered.",
        {
          card_id: z.string().describe("The card id, for example c_7b30"),
          title: z.string().describe("What the card should say instead, in one line"),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("edit_card"),
      ),
      tool(
        "add_endpoint",
        "Add a model endpoint the operator can then run agents on: ollama (local), ollama-cloud, openrouter, or any other host speaking the Anthropic Messages protocol. It is added without a key — never ask the operator to send you a key, this conversation is written to disk. Send them to the settings screen to paste it.",
        {
          name: z
            .string()
            .describe("ollama, ollama-cloud, openrouter, or a name for something else"),
          base_url: z
            .string()
            .optional()
            .describe("Only needed for an endpoint that is not one of the known three"),
        },
        call("add_endpoint"),
      ),
      tool(
        "work_on_relay",
        "Set Relay's own source up as a project so the operator can work on the app itself. Finds it if this machine already has it, clones it otherwise. Use when they say they want to work on Relay, or ask for a change to the app rather than to their code.",
        {},
        call("work_on_relay"),
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
        "grant_agent_tools",
        "Change what an agent may do — read, search, edit, write, git, web, shell — when the operator asks. Send the full list it should hold afterwards, not the ones to add. This one is never remembered: the operator is asked every single time.",
        {
          agent_id: z.string().describe("Which agent, by id"),
          tools: z
            .array(z.enum(["Read", "Search", "Edit", "Write", "Git", "Web", "Shell"]))
            .describe("The complete set it should hold afterwards"),
        },
        call("grant_agent_tools"),
      ),
      tool(
        "install_skill",
        "Install a skill on an agent: a short document it reads before every run, so it does " +
          "a recurring job the same way each time. Look up how the skill works first, then " +
          "declare it here — you never run an install command, and you never write a script; " +
          "Relay writes the file itself from what you declare. Say where the text came from in " +
          "`source`: the operator is shown it, and a skill from a page that asked to be " +
          "installed is exactly what they need to see.",
        {
          agent_id: z.string().describe("Which agent gets it, by id"),
          name: z
            .string()
            .describe("Lowercase letters, digits and hyphens, for example figma-export"),
          description: z
            .string()
            .describe("One line telling the agent when to reach for it"),
          instructions: z
            .string()
            .describe("The skill itself, in markdown. This enters the agent's prompt."),
          source: z
            .string()
            .describe("Where the text came from: a URL, a package name, or 'written here'"),
        },
        call("install_skill"),
      ),
      tool(
        "add_mcp_server",
        "Give an agent an MCP server. This is arbitrary code running with that agent's " +
          "permissions, so declare it rather than installing it: the name, how it is reached, " +
          "and every tool it brings. List the tools from its documentation — the operator " +
          "approves the list, so an incomplete one is a false approval. Never ask the operator " +
          "for a key here; this conversation is written to disk. Name the environment " +
          "variables it needs and send them to the Agents screen to fill them in.",
        {
          agent_id: z.string().describe("Which agent gets it, by id"),
          name: z
            .string()
            .describe("Server name; its tools arrive as mcp__<name>__<tool>. Not 'harness'."),
          transport: z.enum(["stdio", "http", "sse"]).describe("How it is reached"),
          command: z.string().optional().describe("For stdio: the program to run"),
          args: z.array(z.string()).optional().describe("For stdio: its arguments"),
          url: z.string().optional().describe("For http or sse: the endpoint"),
          tools: z
            .array(z.string())
            .describe("Every tool this server grants, from its documentation"),
          env_names: z
            .array(z.string())
            .optional()
            .describe("Environment variables it needs. Names only, never values."),
          source: z.string().describe("Where the declaration came from: a URL or a package"),
        },
        call("add_mcp_server"),
      ),
      tool(
        "revoke_grant",
        "Take a skill or an MCP server away from an agent again.",
        {
          agent_id: z.string().describe("Which agent, by id"),
          kind: z.enum(["skill", "mcp"]),
          name: z.string().describe("Which one, by name"),
        },
        call("revoke_grant"),
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
        "File an improvement proposal in the operator's inbox whenever you find a gap in what " +
          "Relay can do. A single occurrence is reason enough: one tool refused, one thing you " +
          "could not see, one step that took two approvals instead of one — each is a real hole, " +
          "and waiting for it to happen again only means it goes unrecorded. Counts from " +
          "self_report strengthen a proposal; they are not a requirement for opening one. " +
          "A proposal is not a card: they decide whether it becomes work, so NEVER create the " +
          "card yourself and never act on it while it is still open. Once the operator accepts " +
          "it you are told so, and carrying it out is then exactly what you should do.",
        {
          title: z.string().describe("One line naming the problem"),
          observation: z
            .string()
            .describe(
              "What you saw: the single occurrence — which tool, what you were trying to do, " +
                "what it said — or the counts from self_report when it repeats",
            ),
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
              // The five in the title bar first, in nav order, then everything
              // reachable from the sidebar. "director" and "work" are older
              // names for chat and board; the window still answers to them.
              "home",
              "chat",
              "board",
              "code",
              "sessions",
              "review",
              "agents",
              "activity",
              "worktrees",
              "projects",
              "settings",
              "director",
              "work",
              "runs",
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

/** The two tools a worker run carries: its own account of the work it did, and
 *  a line to the Director while it is still doing it.
 *
 *  The engine still owns the commit — `report_work` only feeds it, and records
 *  the durable notes for the memory layer. Absence of a call is normal and
 *  safe. `message_director` is the one that does not wait: it drops a line in
 *  his inbox and returns immediately, because an agent blocked on a person is
 *  an agent not working.
 *
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
      tool(
        "message_director",
        "Tell the Director something while you are still working — a blocker, a decision that " +
          "turned out to be wrong, a discovery that changes what this card should be. It lands " +
          "in his inbox and he reads it at his next turn. No answer comes back through this " +
          "tool, so do not wait for one: say it and carry on. If he wants to change course he " +
          "will say so on the card.",
        {
          text: z
            .string()
            .describe("What he needs to know, in one or two sentences. Say which card you are on."),
        },
        call("message_director"),
      ),
    ],
  });
}

/** Relay's own in-process server, plus whatever this agent was granted.
 *
 *  `harness` is written last and unconditionally: a granted server by that
 *  name would replace the board tools the Director answers with, and a config
 *  that silently loses `move_card` is worse than one that loses a connector.
 *  The Rust side refuses the name too — this is the last of three locks. */
function mcpServersFor(id, spec, build = { harnessTools, reportWorkTool, callFor }) {
  const servers = {};
  for (const [name, config] of Object.entries(spec.mcp_servers ?? {})) {
    if (!name || name === "harness") continue;
    servers[name] = config;
  }
  if (spec.harness_tools) servers.harness = build.harnessTools(id);
  else if (spec.report_work) servers.harness = build.reportWorkTool(id, build.callFor(id));
  return servers;
}

/** The slash commands this session knows, on their way to the composer.
 *
 *  Normalised here rather than in Rust because this is where the SDK's shape
 *  is known: `argumentHint` is always a string on the way in and an absent
 *  hint is the empty one, which is a hint the UI would draw. */
function sendCommands(id, commands) {
  const list = (commands ?? [])
    .filter((c) => c?.name)
    .map((c) => ({
      name: c.name,
      description: c.description ?? "",
      argument_hint: c.argumentHint?.trim() ? c.argumentHint : null,
      aliases: Array.isArray(c.aliases) ? c.aliases : [],
    }));
  send({ type: "event", run_id: id, event: { kind: "commands", commands: list } });
}

/** The skills this run may load, as a plugin directory Relay owns.
 *
 *  Why a plugin and not `settingSources: ['project']`: that would bring the
 *  operator's repository `.claude/settings.json`, its hooks and its `.mcp.json`
 *  along with the skills — configuration injected with no approval at all.
 *  Why not `CLAUDE_CONFIG_DIR` either: it moves where the CLI looks for its
 *  credentials, so pointing it at a Relay folder logs every run out on the
 *  platforms that keep the token in a file. A plugin path is neither: it is
 *  passed per run, it names one directory, and `skipMcpDiscovery` keeps that
 *  directory from declaring MCP servers of its own.
 *
 *  `skills: 'all'` reads as "all of what this agent was granted", because the
 *  directory holds exactly that. The SDK's own filter is documented as "a
 *  context filter, not a sandbox", so it is not what separates two agents —
 *  the directory is. */
function skillsFor(spec) {
  if (!spec.skills_dir) return {};
  return {
    plugins: [{ type: "local", path: spec.skills_dir, skipMcpDiscovery: true }],
    skills: "all",
  };
}

async function handleRun({ id, spec }) {
  const ac = new AbortController();
  controllers.set(id, ac);
  const inbox = new Inbox();
  inboxes.set(id, inbox);

  // tool_use_id → tool name: the result block only knows the id, and the
  // summary needs the name to be worth reading.
  const toolNames = new Map();
  let turnCount = 0;
  // A run answers one `result` per user message it was given, and a run that
  // was spoken to mid-turn was given more than one. These carry the totals
  // across all of them: reporting the last result's own numbers would lose
  // whatever the first turn spent.
  let resultCount = 0;
  let costTotal = 0;
  let costSeen = false;
  let turnsTotal = 0;
  let lastResult = null;
  let lastSession = null;

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
    // The sheet shows the declaration, not the intention: "install the MCP
    // server figma on Designer — grants get_file, export_frame", never "the
    // Director wants to install something". The Rust side falls back to a
    // generic key-value rendering when this is absent.
    send({
      type: "approval_request",
      request_id,
      run_id: id,
      tool: toolName,
      input,
      summary: summarizeUse(toolName, input),
    });
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
    // servers we pass. Without this the operator's account connectors load and
    // the model starts talking about authorising Linear or Notion.
    //
    // This stays exactly as it was when skills and MCP became grantable. The
    // isolation is not loosened; an explicit list is added on top of it, per
    // agent, and nothing is inherited. See `crates/app/src/grants.rs`.
    settingSources: [],
    mcpServers: mcpServersFor(id, spec),
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

  Object.assign(options, skillsFor(spec));

  if (spec.permission_mode) options.permissionMode = spec.permission_mode;
  if (Array.isArray(spec.allowed_tools) && spec.allowed_tools.length > 0) {
    options.allowedTools = spec.allowed_tools;
  }
  if (spec.model) options.model = spec.model;
  // `Concise`, `Explanatory`, … — built-ins, so nothing has to be shipped for
  // them. It rides in the system prompt, which is read once when the session
  // opens: a resumed session keeps the style it was born with, and no value
  // passed here changes that. The name is not validated on this side; the
  // engine answers `available_output_styles` and Relay picks from that.
  if (spec.output_style) options.outputStyle = spec.output_style;
  // Per turn, unlike the style: this one binds the request rather than the
  // system prompt, which is what lets the composer offer it per message. A
  // level the model does not have is downgraded by the engine, not here.
  if (spec.effort) options.effort = spec.effort;
  if (spec.max_budget_usd != null) options.maxBudgetUsd = spec.max_budget_usd;
  // No room to reason means no thinking to stream.
  if (spec.thinking_tokens != null) options.maxThinkingTokens = spec.thinking_tokens;
  if (spec.resume_session) options.resume = spec.resume_session;

  try {
    const q = query({ prompt: inbox.stream(spec.prompt, id), options });
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
            // What `/` can mean in this session: the engine's own commands
            // plus whatever the granted skills brought. Asked for rather than
            // read off the init message, because that one carries names only
            // and the composer has a description to show.
            q.supportedCommands()
              .then((commands) => sendCommands(id, commands))
              .catch(() => {});
          } else if (message.subtype === "commands_changed") {
            // A skill discovered mid-run. Documented as replace-the-list, not
            // merge, so it is passed on whole.
            sendCommands(id, message.commands);
          } else if (message.subtype === "local_command_output" && message.content?.trim()) {
            // A command the engine answered by itself — `/usage`, `/context`.
            // No model turn happened, so this is the only thing the operator
            // gets, and it belongs in the transcript rather than in a toast.
            send({
              type: "event",
              run_id: id,
              event: { kind: "local_output", text: message.content },
            });
          }
          break;
        case "assistant": {
          // One assistant message is one model turn. Emitted live so the
          // card shows progress toward the ceiling before the result lands.
          turnCount++;
          send({ type: "event", run_id: id, event: { kind: "turns", count: turnCount } });
          // Per-turn usage. The result message only totals the query, so it
          // cannot say how full the context is; the last assistant turn's
          // input side can, which is why this rides here and not on `done`.
          const usage = message.message?.usage;
          if (usage) {
            send({
              type: "event",
              run_id: id,
              event: {
                kind: "usage",
                input_tokens: usage.input_tokens ?? 0,
                output_tokens: usage.output_tokens ?? 0,
                cache_read_tokens: usage.cache_read_input_tokens ?? 0,
                cache_creation_tokens: usage.cache_creation_input_tokens ?? 0,
                model: message.message?.model ?? null,
              },
            });
          }
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
          resultCount++;
          // Every result is charged separately — the second turn of a run that
          // read a queued message came back with a cost of its own — so the
          // totals are summed rather than taken off the last one.
          if (typeof message.total_cost_usd === "number") {
            costTotal += message.total_cost_usd;
            costSeen = true;
          }
          if (typeof message.num_turns === "number") turnsTotal += message.num_turns;
          if (typeof message.result === "string") lastResult = message.result;
          if (message.session_id) lastSession = message.session_id;

          // An error result looks like a normal one from the outside: same
          // `result` message, cost 0, no text. A resume of a session that no
          // longer exists arrives exactly this way, and the SDK only throws
          // afterwards — by which time we have already returned. So the
          // failure has to be read off this message, or it passes for success.
          const failed = message.is_error === true || message.subtype !== "success";
          // The turn is answered, but the operator said something else while
          // it ran and the CLI has not answered *that* yet. Returning here is
          // what used to make queueing impossible: the answer to the queued
          // message is the next turn, and it has not happened.
          if (!failed && inbox.owed(resultCount)) break;

          controllers.delete(id);
          const errors = Array.isArray(message.errors)
            ? message.errors.map((e) => String(e).trim()).filter(Boolean).join("; ")
            : "";
          const detail =
            errors ||
            (typeof message.result === "string" && message.result.trim()) ||
            message.subtype ||
            "the run ended with an error";
          // Shut before declaring it over, so nothing can be handed to a CLI
          // that is about to be killed.
          inbox.close();
          send({
            type: "event",
            run_id: id,
            event: {
              kind: "done",
              session_id: lastSession ?? null,
              cost_usd: costSeen ? costTotal : null,
              turns: turnsTotal || null,
              result: lastResult,
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
  } finally {
    // However the run ended, nothing more can be said to it. A message that
    // arrives after this is never acknowledged, and the Rust side — which is
    // the one keeping count — starts a turn of its own for it.
    inboxes.delete(id);
    inbox.close();
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
      // The inbox first: a stop must not deliver one last correction on its
      // way out.
      inboxes.get(msg.id)?.close();
      controllers.get(msg.id)?.abort();
      break;
    }
    case "message": {
      // Typed while the turn was running. A run that has already ended has no
      // inbox, so this falls on the floor here on purpose — the Rust side sees
      // no `message_read` and gives it a turn of its own.
      inboxes.get(msg.id)?.push({ id: msg.message_id, text: msg.text ?? "" });
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
