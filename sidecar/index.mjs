import { query, createSdkMcpServer, tool } from "@anthropic-ai/claude-agent-sdk";
import readline from "node:readline";
import net from "node:net";
import fs from "node:fs";
import { randomUUID } from "node:crypto";
import { z } from "zod";
import { inspect } from "./pathguard.mjs";
import { summarizeUse, summarizeResult, detailOf, countLines } from "./toolsum.mjs";
import { isSubagentTool } from "./protocol.generated.mjs";

const controllers = new Map();
const approvals = new Map();
/** Harness tool calls waiting on an answer from the Rust side. */
const toolCalls = new Map();
/** Live runs that can still be spoken to, by run id. See `Inbox`. */
const inboxes = new Map();
/** Pedidos feitos e ainda não respondidos, pelo id deles.
 *
 *  Uma aprovação e uma chamada às ferramentas da Relay não se resolvem sozinhas:
 *  são a Relay que as responde. Feitas enquanto ninguém está ligado, ficam aqui
 *  e repetem-se a quem se ligar a seguir — senão o run ficava à espera de uma
 *  resposta que ninguém sabia que estava a ser esperada. */
const pendingRequests = new Map();

/** Pergunta à Relay, e lembra-se de que perguntou. */
function ask(obj) {
  pendingRequests.set(obj.request_id, obj);
  send(obj);
}

function answered(requestId) {
  pendingRequests.delete(requestId);
}

/** Para onde vai o que dizemos, e o que fica dito enquanto ninguém ouve.
 *
 *  Em cima de um cano, o sidecar durava o que durasse a Relay: ela morria, o
 *  cano fechava, e o turno em curso ia com ela. Num socket não — o trabalho
 *  continua, e uma Relay nova volta a ligar-se e recebe o que perdeu. É essa a
 *  diferença entre um agente que sobrevive a um reinício e um que não.
 *
 *  Por isso tudo o que se diz é numerado e guardado. Quem se liga diz por onde
 *  ia (`from_seq`) e recebe o resto antes do que vier a seguir — sem buracos e
 *  sem repetições, que é o que permite à conversa no ecrã ser a mesma de antes.
 *
 *  A memória chega e o disco não faz falta: o histórico só serve enquanto este
 *  processo viver, e se ele morrer não há execução para onde voltar. */
const bus = {
  seq: 0,
  history: [],
  client: null,
  /** Só o que é evento entra no histórico. Os pedidos — aprovações, chamadas às
   *  ferramentas da Relay — voltam a fazer-se de outra maneira: continuam
   *  pendentes nos mapas acima e são repetidos a quem se ligar, porque o que
   *  falta neles não é serem vistos, é serem respondidos. */
  replayable(obj) {
    return obj?.type === "event";
  },
  write(obj) {
    if (!this.client) return;
    try {
      this.client.write(JSON.stringify(obj) + "\n");
    } catch {
      // Um cliente que se foi a meio de uma escrita não é uma falha do run.
    }
  },
  publish(obj) {
    if (this.replayable(obj)) {
      obj = { ...obj, seq: ++this.seq };
      this.history.push(obj);
    }
    this.write(obj);
  },
  attach(socket, fromSeq) {
    this.client = socket;
    for (const past of this.history) {
      if (past.seq > fromSeq) this.write(past);
    }
    return this.seq;
  },
  detach(socket) {
    if (this.client === socket) this.client = null;
  },
};

/** Modo cano, como sempre foi: sem `--serve`, fala-se por stdout. É o que os
 *  testes usam e o que serve uma Relay anterior à reatação. */
let stdioMode = true;

function send(obj) {
  if (stdioMode) {
    process.stdout.write(JSON.stringify(obj) + "\n");
    return;
  }
  bus.publish(obj);
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

/** Um `result` que não correu turno nenhum não é uma resposta, por mais alegre
 *  que venha o `subtype`.
 *
 *  É o que volta quando a sessão que se está a retomar ainda está agarrada por
 *  outro processo vivo: esse CLI mete o pedido na *sua* fila, responde-lhe num
 *  stream que ninguém aqui está a ler, e este sai `success` sem ter falado com
 *  modelo nenhum — sem turnos, sem custo, sem texto. Lido como sucesso, dá
 *  silêncio no ecrã, que é a única coisa sobre a qual a operadora não pode
 *  agir. O texto vazio sozinho não chega para o dizer: um turno que só chamou
 *  ferramentas também acaba sem texto, e esse correu. O turno é que separa os
 *  dois. */
function answeredNothing(message) {
  return (
    message.subtype === "success" &&
    !(message.num_turns > 0) &&
    !(typeof message.result === "string" && message.result.trim())
  );
}

/** Um turno, a partir de uma mensagem do assistente — ou `null` quando esta
 *  mensagem já foi contada.
 *
 *  Uma mensagem do assistente é um turno do modelo, mas o SDK entrega-a uma vez
 *  **por bloco de conteúdo**, e cada entrega traz o `usage` inteiro da mensagem.
 *  Contá-las à chegada cobrava três vezes um turno que fez três chamadas: o log
 *  de 2026-09-01 tem 317 eventos de `usage` para 72 turnos, e os totais tirados
 *  deles ficaram entre 1,45x e 2,01x acima da verdade. O que faz de um turno um
 *  turno é o id da mensagem, portanto é ele que se conta.
 *
 *  O `subagent` acompanha o `usage` porque um subagente escreve neste mesmo
 *  fluxo: o gasto é real e conta, mas o contexto é outro, e o indicador que
 *  pergunta "quão cheia está esta sessão" não pode ler o dele. Sem isto a
 *  leitura salta 34967 → 8544 → 34967 ao atravessar uma chamada `Task`. */
function turnFrom(message, counted) {
  const id = message?.message?.id ?? null;
  if (id !== null && counted.has(id)) return null;
  if (id !== null) counted.add(id);
  const usage = message?.message?.usage;
  if (!usage) return { usage: null };
  return {
    usage: {
      kind: "usage",
      input_tokens: usage.input_tokens ?? 0,
      output_tokens: usage.output_tokens ?? 0,
      cache_read_tokens: usage.cache_read_input_tokens ?? 0,
      cache_creation_tokens: usage.cache_creation_input_tokens ?? 0,
      model: message.message?.model ?? null,
      subagent: (message.parent_tool_use_id ?? null) !== null,
    },
  };
}

/** O conjunto de trabalho de fundo vivo, na forma que a Relay guarda.
 *
 *  Nível e não aresta: cada carga traz tudo o que está vivo e substitui a
 *  anterior. Emparelhar `task_started` com `task_notification` deixaria um
 *  indicador preso a girar por causa de uma aresta perdida — e o conjunto é
 *  por-processo, portanto começa vazio em cada sessão.
 *
 *  Uma tarefa sem `task_id` cai fora: sem id não há chave estável no ecrã, e
 *  duas dessas seriam a mesma linha a piscar. */
function liveTasks(message) {
  return (message.tasks ?? [])
    .filter((t) => t && typeof t.task_id === "string" && t.task_id)
    .map((t) => ({
      task_id: t.task_id,
      task_type: typeof t.task_type === "string" ? t.task_type : "",
      description: typeof t.description === "string" ? t.description : "",
    }));
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
  ask({ type: "tool_request", request_id, run_id: runId, name, input });
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
        "set_dependencies",
        "Say which cards must reach Done before this one may start. Order, not file conflict. " +
          "Use it when you have planned several cards at once: the board then starts each one " +
          "as its dependencies land, instead of waiting for the operator to start them in the " +
          "right sequence by hand. Pass an empty list to clear.",
        {
          card_id: z.string().describe("The card that has to wait, for example c_7b30"),
          depends_on: z
            .array(z.string())
            .describe("Card ids that must be Done first. Empty clears the dependencies."),
          project_id: z
            .string()
            .optional()
            .describe("Which project board. Defaults to the one this conversation is pinned to."),
        },
        call("set_dependencies"),
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
        "Change an existing agent's profile when the operator asks: what it is called, what it is for, its brief, its budget, who reviews it, whether it is paused, and where its work happens. Tools and permissions are not editable here — those go through grant_agent_tools, which the operator answers each time.",
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
          worktree: z
            .enum(["per_card", "shared", "none"])
            .optional()
            .describe(
              "Where its work happens. `per_card` gives every card its own branch and checkout, " +
                "which is what makes a diff to review; `shared` is one long-lived branch; `none` " +
                "runs against the live repository and can only read. An agent that writes needs " +
                "one of the first two — the board refuses to start it otherwise.",
            ),
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
        "Point an existing agent at a different model, a different backend, or a different " +
          "endpoint, when the operator asks. `backend` is which agent binary runs it: `claude`, " +
          "or `codex` on the ChatGPT plan this machine is logged into. Changing backend clears " +
          "the model unless this same call names a new one, because the two backends do not " +
          "share model names — `sonnet` means nothing to Codex and `gpt-5.6-terra` means " +
          "nothing to Claude. A Codex agent has no endpoint and no dollar budget: it runs on " +
          "the plan, which is measured in a share of a window rather than in money.",
        {
          agent_id: z.string().describe("Which agent, by id"),
          backend: z
            .enum(["claude", "codex"])
            .optional()
            .describe("Which agent binary runs it"),
          model: z.string().optional().describe("Model name the chosen backend knows"),
          provider: z
            .string()
            .optional()
            .describe(
              "Endpoint id — or 'anthropic' (also 'none', 'default') to CLEAR it and send the " +
                "agent back to the Claude login this machine already has. Clearing is how you " +
                "undo a custom endpoint: an agent left pointing at one it cannot serve the " +
                "chosen model with fails at its first API call. Refused on a Codex agent, " +
                "which has no endpoint to point anywhere.",
            ),
        },
        call("set_agent_model"),
      ),
      tool(
        "generate_image",
        "Make an image from a description. It runs on OpenAI's image model through the ChatGPT " +
          "plan this machine is logged into — no API key, and nothing is billed per call. Takes " +
          "up to a few minutes. You get back a file path; put it in your answer as " +
          "`![description](path)` and it renders inline in the conversation. The file is saved " +
          "outside the repository: copy it in yourself if it belongs there. Use it for a real " +
          "asset or a mockup, not for a diagram or an icon that would be better as SVG.",
        {
          prompt: z
            .string()
            .describe(
              "What the image shows, in a sentence or two. Say the subject, the style and the " +
                "background — a vague prompt gets a generic picture.",
            ),
        },
        call("generate_image"),
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
            .default([])
            .describe(
              "Durable facts, decisions or conventions worth remembering after this card is done. Omit it, or send an empty array, when there are none."
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
  // Assistant message ids already counted. The SDK repeats a message once per
  // content block and the repeat carries the same usage, so without this the
  // turn count and every token total scale with how many tool calls a turn
  // happened to make.
  const countedMessages = new Set();
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

    if (isSubagentTool(toolName)) {
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
    ask({
      type: "approval_request",
      request_id,
      run_id: id,
      tool: toolName,
      input,
      summary: summarizeUse(toolName, input),
    });
    let answer;
    try {
      answer = await Promise.race([
        decision,
        // The run was cancelled with the question still on screen. Nobody
        // refused it; the run is going away.
        new Promise((resolve) =>
          ac.signal.addEventListener("abort", () =>
            resolve({ allow: false, unanswered: true }),
          ),
        ),
      ]);
    } finally {
      approvals.delete(request_id);
    }
    if (!answer?.allow) {
      // The distinction this sentence carries is the whole of #3 in the
      // 2026-08-31 write-up. Every refusal used to read "denied by operator",
      // including a 30-minute timeout — so an agent working overnight was told
      // the operator had refused, when the operator was asleep, and it went on
      // to design around a decision that was never made. A refusal is a fact
      // to work with; silence is a reason to stop and say what you were about
      // to do.
      return answer?.unanswered
        ? {
            behavior: "deny",
            message:
              "nobody answered this request — the operator did not see it, and did not refuse it. " +
              "Do not work around it and do not try a different way to do the same thing. " +
              "Stop here and say plainly what you were about to do and what you need permission for.",
          }
        : { behavior: "deny", message: "denied by operator" };
    }
    if (isSubagentTool(toolName)) childDepth++;
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
      // Uma entrada por grafia em vez de uma expressão: o que o `matcher`
      // aceita é do SDK, e este contador é o que impede um run de se abrir em
      // árvore. Não se apoia em sintaxe que não controlamos.
      PostToolUse: ["Agent", "Task"].map((matcher) => ({
        matcher,
        hooks: [
          async () => {
            childDepth = Math.max(0, childDepth - 1);
            return {};
          },
        ],
      })),
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
          // A stream event belongs to whoever is streaming it. Without this a
          // subagent's tokens were appended to the answer the operator was
          // watching the parent write, mid-word.
          const from = message.parent_tool_use_id ?? null;
          if (ev?.type === "content_block_delta") {
            const d = ev.delta;
            if (d?.type === "text_delta" && d.text) {
              send({
                type: "event",
                run_id: id,
                event: { kind: "delta", text: d.text, parent_tool_use_id: from },
              });
            } else if (d?.type === "thinking_delta" && d.thinking) {
              send({
                type: "event",
                run_id: id,
                event: { kind: "thinking", text: d.thinking, parent_tool_use_id: from },
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
          } else if (message.subtype === "background_tasks_changed") {
            // O trabalho que continua depois de o turno responder — um `sleep`
            // posto em fundo, um subagente. Não passava por aqui, e por isso
            // não havia nada no ecrã a dizer que existia: a única pista era uma
            // linha de resultado a dizer "running in the background".
            //
            // Documentado como *nível* e não como par de arestas: cada carga
            // traz o conjunto vivo inteiro, e substitui-se o que se tinha. É o
            // que impede um sinal perdido de deixar um indicador preso — e é
            // por-processo, portanto começa vazio em cada sessão e não se
            // guarda em disco.
            send({
              type: "event",
              run_id: id,
              event: { kind: "background_tasks", tasks: liveTasks(message) },
            });
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
          const parent = message.parent_tool_use_id ?? null;
          // A turn is counted once, however many blocks it arrives in. The
          // usage rides here and not on `done` because the result message only
          // totals the query: it cannot say how full the context is, and the
          // last turn's input side can.
          const turn = turnFrom(message, countedMessages);
          if (turn) {
            turnCount++;
            send({ type: "event", run_id: id, event: { kind: "turns", count: turnCount } });
            if (turn.usage) {
              send({ type: "event", run_id: id, event: turn.usage });
            }
          }
          for (const block of message.message?.content ?? []) {
            if (block.type === "text" && block.text?.trim()) {
              // Whose words these are. Only `tool_use` carried this, so a
              // subagent's prose arrived indistinguishable from its parent's
              // and the screen wove the two into one voice — cutting the
              // parent's own sentences at every point a child spoke.
              send({
                type: "event",
                run_id: id,
                event: { kind: "text", text: block.text, parent_tool_use_id: parent },
              });
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
                  // Quantas linhas isto mexe, quando a chamada o diz. Ausente
                  // — e não zero — quando a ferramenta não mexe em linhas.
                  ...(countLines(block.name, block.input) ?? {}),
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
          const nothing = answeredNothing(message);
          const failed =
            message.is_error === true || message.subtype !== "success" || nothing;
          // The turn is answered, but the operator said something else while
          // it ran and the CLI has not answered *that* yet. Returning here is
          // what used to make queueing impossible: the answer to the queued
          // message is the next turn, and it has not happened.
          if (!failed && inbox.owed(resultCount)) break;

          controllers.delete(id);
          const errors = Array.isArray(message.errors)
            ? message.errors.map((e) => String(e).trim()).filter(Boolean).join("; ")
            : "";
          // O `subtype` como último recurso serve para os erros, que o trazem
          // legível. Para o turno vazio dizia "success", que é o contrário do
          // que se passou — por isso este tem texto próprio, e diz onde é que
          // a mensagem foi parar.
          const detail = nothing
            ? "nada correu: esta sessão ainda está agarrada pela execução anterior, " +
              "que levou a mensagem para a fila dela — a resposta não chega aqui"
            : errors ||
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
    // E nada lhe sobrevive. O stream acabar não é o processo acabar: fechar a
    // entrada mata-lhe as tarefas de fundo mas deixa-o de pé, e de pé continua
    // a segurar a sessão. O turno que a Rust abre a seguir retoma então uma
    // sessão que é de outro, o CLI entrega-lhe a mensagem pela fila e sai sem
    // correr nada — a mensagem chega, a resposta sai por um stream que já
    // ninguém lê, e o ecrã fica em branco sem erro nenhum.
    ac.abort();
  }
}

function handleLine(line) {
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
      answered(msg.request_id);
      approvals.get(msg.request_id)?.({
        allow: !!msg.allow,
        // Three outcomes reach here, not two. See `canUseTool`.
        unanswered: !!msg.unanswered,
      });
      break;
    }
    case "tool_response": {
      answered(msg.request_id);
      toolCalls.get(msg.request_id)?.({ ok: msg.ok !== false, text: msg.text ?? "" });
      break;
    }
  }
}

/** A Relay foi-se.
 *
 *  O stdin é o único fio que nos prende a ela: enquanto está viva nunca o
 *  fecha — mantém-no aberto para o comando seguinte — por isso um EOF aqui só
 *  quer dizer uma coisa, e o sistema garante-o mesmo quando ela morre de
 *  SIGKILL, que é o caso em que nada mais nos avisaria.
 *
 *  Sem isto ficávamos órfãos com o CLI ao colo, e um órfão continua a segurar a
 *  sessão: as mensagens seguintes iam para a fila dele e as respostas saíam por
 *  um stream que já ninguém lia (#108). A limpeza do lado da Rust não chega
 *  para este caso — ela corre no fim do `drive`, e uma Relay morta a meio de um
 *  turno nunca lá chega. Um deles apanhou doze horas assim.
 *
 *  Aborta primeiro, para o SDK ter a hipótese de desmontar em condições, e
 *  leva o grupo a seguir — o grupo é o nosso, criado à nascença, e nós somos o
 *  líder, portanto isto não alcança nada que a Relay não tenha levantado aqui.
 *  Levar o grupo mata-nos também, que é o objectivo. */
function tearDown() {
  for (const ac of controllers.values()) ac.abort();
  setTimeout(() => {
    try {
      process.kill(-process.pid, "SIGKILL");
    } catch {
      process.exit(0);
    }
  }, 1500);
}

/** Como se fala com a Relay: por cano ou por socket.
 *
 *  **Cano** (sem `--serve`) é o que sempre foi, e continua a ser o que os
 *  testes usam. O sidecar vive o que a Relay viver: um EOF no stdin só pode
 *  querer dizer que ela morreu — viva, nunca o fecha —, e nesse caso desmonta-se
 *  e leva o grupo, para não ficar órfão a segurar a sessão (#108).
 *
 *  **Socket** (`--serve <caminho>`) é o contrário, de propósito: o run
 *  sobrevive a quem o mandou fazer. Um cliente que se desliga não é um fim, é
 *  uma ausência — o trabalho continua, o que se diz fica numerado no `bus`, e a
 *  Relay seguinte liga-se, diz por onde ia e recebe o resto. Um turno deixa de
 *  se perder num reinício.
 *
 *  O que *não* muda entre os dois é o protocolo: as mesmas linhas JSON, o mesmo
 *  despachante. Só muda por onde passam, e é isso que mantém o `drive` do lado
 *  da Rust quase intacto. */
function serveOnSocket(path) {
  stdioMode = false;
  // Um socket deixado por um processo que já morreu não é dono de nada. Se
  // ainda estivesse alguém à escuta neste caminho o `listen` falhava, e falhar
  // é o que queremos — dois servidores no mesmo sítio era pior do que não
  // arrancar.
  try {
    fs.unlinkSync(path);
  } catch {
    /* não existia, que é o caso normal */
  }

  const server = net.createServer((socket) => {
    socket.setNoDelay(true);
    let greeted = false;
    const lines = readline.createInterface({ input: socket, terminal: false });
    lines.on("line", (line) => {
      if (!greeted) {
        greeted = true;
        let hello = {};
        try {
          hello = JSON.parse(line);
        } catch {
          /* fica no zero, que é o mesmo que "manda-me tudo" */
        }
        // Sem número, manda-se só o que vier a seguir. É a omissão segura: o
        // contrário — mandar tudo de novo — punha a conversa no ecrã com todas
        // as falas repetidas, e uma duplicação é mais difícil de desfazer do
        // que uma falta. Quem souber por onde ia diz o número e recebe o
        // atraso; enquanto a Relay não guardar esse número, fica pelo vivo.
        const from = Number.isFinite(hello.from_seq) ? hello.from_seq : bus.seq;
        // Primeiro dizer quem somos e em que pé estamos, depois o atraso, e só
        // então o que estiver por responder: por esta ordem, quem se liga tem o
        // contexto antes de lhe pedirem uma decisão.
        socket.write(
          JSON.stringify({
            type: "attached",
            run_key: runKey,
            seq: bus.seq,
            running: controllers.size > 0,
            // **Qual** run, e não só que há um.
            //
            // Quem se liga a um turno vivo não lhe sabia o nome, e cunhava um
            // id novo — depois mandava as mensagens do operador endereçadas a
            // esse, que deste lado não existe, e caíam no chão. Uma conversa
            // reatada aceitava tudo o que se lhe escrevesse e não entregava
            // nada.
            run_id: [...controllers.keys()][0] ?? null,
            // Com que autenticação é que este processo foi levantado.
            //
            // As variáveis do endpoint são do **processo**, não do run: quem
            // sobrevive leva-as consigo. Um sidecar levantado para um run no
            // OpenRouter e reatado por um run do login da Claude dava
            // `401 Missing Authentication header` — o `ANTHROPIC_API_KEY` vem
            // vazio de propósito, e o `BASE_URL` continuava a apontar para
            // outro sítio. Quem se liga tem de poder recusar.
            //
            // O que se diz é o que a **Relay** mandou, e não o
            // `ANTHROPIC_BASE_URL` que estiver no ambiente: numa máquina que o
            // tenha exportado, o segundo não distingue um endpoint escolhido
            // de um que já lá estava, e a comparação dava sempre diferente —
            // matava e levantava um sidecar novo a cada run. Vazio quer dizer
            // "o login da Claude", que é como a ausência se escreve.
            auth: process.env.RELAY_PROVIDER ?? "",
            pid: process.pid,
          }) + "\n",
        );
        bus.attach(socket, from);
        for (const request of pendingRequests.values()) bus.write(request);
        // Um `attach` sozinho é só isso; o `run` vem a seguir, na mesma ligação.
        if (hello.type !== "attach") handleLine(line);
        return;
      }
      handleLine(line);
    });
    socket.on("close", () => {
      lines.close();
      bus.detach(socket);
    });
    socket.on("error", () => bus.detach(socket));
  });

  server.listen(path);
  server.on("error", (e) => {
    process.stderr.write(`sidecar: cannot serve on ${path}: ${e.message}\n`);
    process.exit(1);
  });

  // Qual ficheiro é o nosso, e não só qual caminho.
  //
  // O caminho vem da chave do run, portanto é um nome fixo: todos os sidecares
  // daquela conversa querem aquele sítio. Dois chegaram a estar vivos no mesmo
  // — o primeiro perdeu o ficheiro, o segundo criou-o de novo — e quando o
  // primeiro morreu apagou, à saída, o socket **do segundo**, que ficou vivo e
  // inalcançável para sempre. Visto a acontecer, não deduzido.
  //
  // O inode é o que distingue os dois: um caminho igual não quer dizer o mesmo
  // ficheiro. Quem morre só varre o que ainda é seu.
  let ours = null;
  server.on("listening", () => {
    try {
      ours = fs.statSync(path).ino;
    } catch {
      /* sem inode não se varre nada, que é o lado seguro */
    }
  });

  // O socket é o nosso nome: sem ele ninguém nos volta a encontrar, e um
  // ficheiro deixado para trás faz a Relay bater a uma porta que já não abre.
  const sweep = () => {
    try {
      if (ours !== null && fs.statSync(path).ino !== ours) return;
      fs.unlinkSync(path);
    } catch {
      /* já lá não estava */
    }
  };
  process.on("exit", sweep);
  for (const signal of ["SIGTERM", "SIGINT"]) {
    process.on(signal, () => {
      sweep();
      process.exit(0);
    });
  }

  // Sobreviver ao cliente é o ponto (#111); sobreviver a *todos* eles, para
  // sempre, sem trabalho nenhum, é lixo — e lixo que segura uma sessão da
  // Claude, portanto a conversa seguinte apanha "esta sessão ainda está
  // agarrada pela execução anterior". Um destes ficou hora e três quartos
  // assim.
  //
  // As três condições são todas necessárias. Sair com trabalho a andar desfazia
  // exactamente aquilo que isto existe para fazer, e sair com alguém ligado
  // fechava-lhe a porta na cara.
  const ALONE_LIMIT_MS = 15 * 60 * 1000;
  let aloneSince = Date.now();
  const loitering = setInterval(() => {
    if (controllers.size > 0 || bus.client) {
      aloneSince = Date.now();
      return;
    }
    if (Date.now() - aloneSince < ALONE_LIMIT_MS) return;
    process.stderr.write("sidecar: nobody came back and there is no work; leaving\n");
    sweep();
    process.exit(0);
  }, 60_000);
  // Sem isto o próprio temporizador segurava o processo de pé.
  loitering.unref();
}

const argAfter = (flag) => {
  const at = process.argv.indexOf(flag);
  return at > -1 ? process.argv[at + 1] : null;
};
const serveAt = argAfter("--serve");
/** Quem este sidecar é. Dito na saudação para quem se liga poder conferir que
 *  encontrou o trabalho que procurava, e não o de outro agente que por acaso
 *  estivesse no mesmo caminho. Um socket é um sítio; a chave é a identidade. */
const runKey = argAfter("--key");

if (serveAt) {
  serveOnSocket(serveAt);
} else {
  const rl = readline.createInterface({ input: process.stdin, terminal: false });
  rl.on("line", handleLine);
  rl.on("close", tearDown);
}
