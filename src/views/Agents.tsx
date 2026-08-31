/** The crew: who exists, and what each one is allowed to do. The left pane is
 *  the roster grouped by team; the right pane is one profile, editable in
 *  place. Every change is saved through the backend, never held here. */

import { useEffect, useMemo, useState } from "react";
import { motion } from "motion/react";
import { money, num, plural } from "../lib/format";
import { api, reason } from "../lib/ipc";
import { cx } from "../lib/cx";
import { paneIn, rowIn } from "../lib/motion";
import {
  ALL_PERMISSIONS,
  modelLabel,
  MODELS,
  REVIEWERS,
  WORKTREE_MODES,
  tone,
  type AgentProfile,
  type Backend,
  type BrowserOffer,
  type SkillOffer,
  type CatalogModel,
  type McpTransport,
  type PlanUsage,
  type Reviewer,
  type WorktreeMode,
} from "../lib/types";
import { useStore } from "../state/store";
import { Eyebrow, Glyph, mono, truncate } from "../components/ui";

/** Uma pastilha de contorno que responde ao ponteiro. */
const CHIP =
  "min-h-6 cursor-pointer rounded-full border border-line3 transition-[border-color,background,color] duration-150 hover:border-line4 hover:bg-surface2 dark:border-line3-d dark:hover:border-line4-d dark:hover:bg-surface2-d";

/** Uma linha de lista que acende debaixo do ponteiro. */
const ROW = "transition-colors duration-150 hover:bg-hovered dark:hover:bg-hovered-d";

/** Um campo de texto assente na superfície. */
const FIELD =
  "w-full rounded-sm border border-line2 bg-surface text-text outline-none focus-visible:border-accentLine dark:border-line2-d dark:bg-surface-d dark:text-text-d dark:focus-visible:border-accentLine-d";

/** Where a granted MCP server is reached, in one line. */
function reachOf(t: McpTransport): string {
  return t.kind === "stdio" ? [t.command, ...t.args].join(" ").trim() : t.url;
}

/** What has been installed on this agent: skills it reads before every run,
 *  and MCP servers it may call.
 *
 *  Both are shown as what the operator approved, not as a count: the source of
 *  a skill and the tools a server was declared to grant are the whole of the
 *  decision they made, and a screen that hides them makes the approval sheet
 *  the only place they ever existed. Nothing here is inherited from the
 *  machine — an agent holds exactly this list and no more. */
function Granted({
  agent,
  patch,
}: {
  agent: AgentProfile;
  patch: (next: Partial<AgentProfile>) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  if (agent.granted_skills.length === 0 && agent.mcp_servers.length === 0) {
    return (
      <div>
        <Eyebrow className="block pb-2">INSTALLED</Eyebrow>
        <div className="text-xs font-normal leading-normal text-text4 dark:text-text4-d">
          Nothing installed. Ask the Director for a skill or an MCP server and it will put the
          declaration in front of you before anything is written.
        </div>
      </div>
    );
  }
  return (
    <div>
      <Eyebrow className="block pb-2">INSTALLED</Eyebrow>
      <div className="flex flex-col gap-1.5">
        {agent.granted_skills.map((s) => (
          <div
            key={`skill-${s.name}`}
            className="rounded-md border border-line2 bg-surface dark:border-line2-d dark:bg-surface-d"
          >
            <div className="flex items-center gap-2 px-3 py-2">
              <span className={cx(mono, "text-sm text-text dark:text-text-d")}>{s.name}</span>
              <span className="text-xs text-text4 dark:text-text4-d">skill</span>
              <span className="flex-1 truncate text-xs text-text3 dark:text-text3-d">
                {s.description}
              </span>
              <button
                type="button"
                onClick={() => setOpen(open === `skill-${s.name}` ? null : `skill-${s.name}`)}
                className="cursor-pointer text-xs text-text3 underline-offset-2 hover:underline dark:text-text3-d"
              >
                {open === `skill-${s.name}` ? "hide" : "what it says"}
              </button>
              <button
                type="button"
                aria-label={`Remove the ${s.name} skill`}
                onClick={() =>
                  patch({ granted_skills: agent.granted_skills.filter((x) => x.name !== s.name) })
                }
                className="cursor-pointer text-xs text-bad hover:underline dark:text-bad-d"
              >
                remove
              </button>
            </div>
            {open === `skill-${s.name}` && (
              <div className="border-t border-line2 px-3 py-2 dark:border-line2-d">
                <div className="pb-1.5 text-xs text-text4 dark:text-text4-d">
                  from {s.source || "an unnamed source"}
                </div>
                <pre
                  className={cx(
                    mono,
                    "max-h-64 overflow-auto whitespace-pre-wrap text-xs leading-relaxed text-text2 dark:text-text2-d",
                  )}
                >
                  {s.body}
                </pre>
              </div>
            )}
          </div>
        ))}
        {agent.mcp_servers.map((m) => (
          <div
            key={`mcp-${m.name}`}
            className="rounded-md border border-line2 bg-surface dark:border-line2-d dark:bg-surface-d"
          >
            <div className="flex items-center gap-2 px-3 py-2">
              <span className={cx(mono, "text-sm text-text dark:text-text-d")}>{m.name}</span>
              <span className="text-xs text-text4 dark:text-text4-d">{m.transport.kind}</span>
              <span className="flex-1 truncate text-xs text-text3 dark:text-text3-d">
                {m.tools.length ? m.tools.join(", ") : "no tools declared"}
              </span>
              <button
                type="button"
                onClick={() => setOpen(open === `mcp-${m.name}` ? null : `mcp-${m.name}`)}
                className="cursor-pointer text-xs text-text3 underline-offset-2 hover:underline dark:text-text3-d"
              >
                {open === `mcp-${m.name}` ? "hide" : "how it is reached"}
              </button>
              <button
                type="button"
                aria-label={`Remove the ${m.name} server`}
                onClick={() =>
                  patch({ mcp_servers: agent.mcp_servers.filter((x) => x.name !== m.name) })
                }
                className="cursor-pointer text-xs text-bad hover:underline dark:text-bad-d"
              >
                remove
              </button>
            </div>
            {open === `mcp-${m.name}` && (
              <div className="flex flex-col gap-1.5 border-t border-line2 px-3 py-2 dark:border-line2-d">
                <div className={cx(mono, "text-xs text-text2 dark:text-text2-d")}>
                  {reachOf(m.transport)}
                </div>
                <div className="text-xs text-text4 dark:text-text4-d">
                  declared from {m.source || "an unnamed source"}
                </div>
                {Object.keys(m.env ?? {}).map((key) => (
                  <label key={key} className="flex items-center gap-2">
                    <span className={cx(mono, "w-48 shrink-0 text-xs text-text3 dark:text-text3-d")}>
                      {key}
                    </span>
                    <input
                      type="password"
                      value={m.env?.[key] ?? ""}
                      placeholder="not set — the server will not connect"
                      aria-label={`Value for ${key}`}
                      onChange={(e) =>
                        patch({
                          mcp_servers: agent.mcp_servers.map((x) =>
                            x.name === m.name
                              ? { ...x, env: { ...x.env, [key]: e.target.value } }
                              : x,
                          ),
                        })
                      }
                      className={cx(FIELD, "px-2 py-1 text-xs")}
                    />
                  </label>
                ))}
                <div className="text-xs text-text4 dark:text-text4-d">
                  Its tools arrive as <span className={mono}>mcp__{m.name}__&lt;tool&gt;</span> and
                  every call still asks you. The list above is what was declared when you approved
                  it, not what the server reports.
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

/** As skills que o Relay traz, um clique cada.
 *
 *  Uma skill é prosa que entra no prompt: não concede alcance nenhum por si.
 *  A frase por baixo diz isso em voz alta, porque o contrário — pensar que
 *  conceder a skill do terminal dá terminal a alguém — é o engano caro. Quem
 *  dá alcance é a lista de ferramentas acima. */
function Skills({ agent }: { agent: AgentProfile }) {
  const { saveAgents, toast } = useStore();
  const [offers, setOffers] = useState<SkillOffer[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .skillOffers()
      .then((list) => alive && setOffers(list))
      .catch(() => alive && setOffers([]));
    return () => {
      alive = false;
    };
  }, []);

  const give = async (id: string) => {
    setBusy(id);
    try {
      await saveAgents(await api.skillGrant(agent.id, id));
    } catch (e) {
      toast("bad", "Could not grant that skill", reason(e));
    } finally {
      setBusy(null);
    }
  };

  if (offers != null && offers.length === 0) return null;
  return (
    <div>
      <Eyebrow className="block pb-2">SKILLS RELAY SHIPS</Eyebrow>
      <div className="flex flex-col gap-1.5">
        {(offers ?? []).map((k) => {
          const has = agent.granted_skills.some((g) => g.name === k.id);
          const canShell = agent.permissions.includes("Shell");
          return (
            <div
              key={k.id}
              className="rounded-md border border-line2 bg-surface px-3 py-2 dark:border-line2-d dark:bg-surface-d"
            >
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-text1 dark:text-text1-d">{k.name}</span>
                <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>{k.id}</span>
                <div className="flex-1" />
                {has ? (
                  <span className={cx(mono, "text-xs text-ok dark:text-ok-d")}>granted</span>
                ) : (
                  <button
                    type="button"
                    disabled={busy != null}
                    onClick={() => give(k.id)}
                    className={cx(CHIP, "px-2.5 py-1 text-xs font-normal text-text2 disabled:opacity-50 dark:text-text2-d")}
                  >
                    {busy === k.id ? "granting…" : "Grant"}
                  </button>
                )}
              </div>
              <div className="pt-1 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                {k.note}
              </div>
              {/* O caso que faz a concessão não valer nada, dito onde se vê. */}
              {has && k.id === "cli" && !canShell && (
                <div className="pt-1 text-xs font-normal leading-normal text-warn dark:text-warn-d">
                  {agent.name} does not have Shell, so this only describes a tool it cannot reach.
                  Tick Shell above.
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** Os dois browsers, um clique cada.
 *
 *  Um browser é alcance, e o alcance concede-se por agente — por isso isto vive
 *  no perfil e não nas Definições. Não há formulário: o comando, as opções, a
 *  pasta do perfil e a lista de ferramentas estão todos decididos, e a única
 *  escolha a sério é qual dos dois. A frase por baixo de cada um é o que se
 *  está a conceder, e é a que o backend escreve — não uma paráfrase que possa
 *  vir a discordar dela. */
function Browsers({ agent }: { agent: AgentProfile }) {
  const { saveAgents, toast } = useStore();
  const [offers, setOffers] = useState<BrowserOffer[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .browserOffers()
      .then((list) => alive && setOffers(list))
      .catch(() => alive && setOffers([]));
    return () => {
      alive = false;
    };
  }, []);

  const give = async (id: string) => {
    setBusy(id);
    try {
      // O backend monta a concessão (comando, opções, pasta, ferramentas) e
      // devolve a tripulação já com ela. Guardá-la de volta é o que põe o
      // ecrã em dia, pelo mesmo caminho de qualquer outra edição de perfil.
      await saveAgents(await api.browserGrant(agent.id, id));
    } catch (e) {
      toast("bad", "Could not grant that browser", reason(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div>
      <Eyebrow className="block pb-2">BROWSERS</Eyebrow>
      <div className="flex flex-col gap-1.5">
        {(offers ?? []).map((b) => {
          const has = agent.mcp_servers.some((m) => m.name === b.id);
          return (
            <div
              key={b.id}
              className="rounded-md border border-line2 bg-surface px-3 py-2 dark:border-line2-d dark:bg-surface-d"
            >
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-text1 dark:text-text1-d">{b.name}</span>
                <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>{b.id}</span>
                <div className="flex-1" />
                {has ? (
                  <span className={cx(mono, "text-xs text-ok dark:text-ok-d")}>granted</span>
                ) : (
                  <button
                    type="button"
                    disabled={busy != null}
                    onClick={() => give(b.id)}
                    className={cx(CHIP, "px-2.5 py-1 text-xs font-normal text-text2 disabled:opacity-50 dark:text-text2-d")}
                  >
                    {busy === b.id ? "granting…" : "Grant"}
                  </button>
                )}
              </div>
              <div className="pt-1 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                {b.note}
              </div>
            </div>
          );
        })}
      </div>
      <div className="pt-2 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
        Chrome is started by the run and every call still asks you. Remove one under INSTALLED.
      </div>
    </div>
  );
}

function stateOf(agent: AgentProfile, running: number) {
  if (agent.paused) return { label: "paused", fg: "text-text4 dark:text-text4-d" };
  if (running > 0) return { label: `${running} running`, fg: "text-ok dark:text-ok-d" };
  if (agent.id === "director") return { label: "chat", fg: "text-warn dark:text-warn-d" };
  return { label: "idle", fg: "text-text4 dark:text-text4-d" };
}

/** The template footer. Nothing is fetched until this mounts, and nothing is
 *  created until a name is picked: a template is a menu entry. */
function Templates() {
  const { agentTemplates, createAgentFromTemplate, agents, saveAgents } = useStore();
  const [templates, setTemplates] = useState<AgentProfile[] | null>(null);

  useEffect(() => {
    let alive = true;
    agentTemplates().then((list) => alive && setTemplates(list));
    return () => {
      alive = false;
    };
  }, [agentTemplates]);

  /** A profile from nothing. The id is settled here so two customs in a row
   *  cannot collide on the same name.
   *
   *  **Cinco valores discordam do `AgentProfile::default()` do Rust, e é de
   *  propósito.** Um perfil nasce em dois sítios: aqui, com o operador a
   *  clicar, e no `agents::drafted`, quando o Director pede um. O que o
   *  Director cria vai trabalhar — worktree própria, revisão do Director. O
   *  que nasce deste botão é um rascunho no ecrã: sem worktree, revisto por
   *  si, com um tecto pequeno, para não haver forma de clicar "novo agente" e
   *  ficar com uma coisa a escrever no disco. O que os mantém honestos é o
   *  `tsc` — este literal tem de satisfazer `AgentProfile`, portanto um campo
   *  novo no Rust parte aqui. São os *valores* que divergem, não a forma. */
  const custom = () => {
    const taken = new Set(agents.map((a) => a.id));
    let id = "new-agent";
    for (let n = 2; taken.has(id); n += 1) id = `new-agent-${n}`;
    saveAgents([
      ...agents,
      {
        id,
        name: "New agent",
        initial: "N",
        title: "Specialist",
        role: "Say what this one is for.",
        brief: "",
        tone: "accent",
        backend: "claude",
        model: "sonnet",
        permissions: ["Read", "Search"],
        budget_usd: 0.5,
        worktree: "none",
        provider: "",
        reviewer: "human",
        paused: false,
        permission_mode: null,
        // Sem cerco: um perfil novo age no projecto da conversa, como o
        // Director. Pôr-lhe um quadro é uma decisão a tomar, não um começo.
        project: null,
        team: "",
        chat_enabled: true,
        tasks_enabled: true,
        max_concurrent: 1,
        skills: [],
        // A new profile is granted nothing. Installing is an approval, not a
        // default, and this is the one place a profile is born in the UI.
        granted_skills: [],
        mcp_servers: [],
        reports_to: null,
        can_delegate: false,
        expected_output: "",
        escalate_to: null,
        // The engine's default voice. Only the Director is opinionated about
        // how it writes, because only the Director is read line by line.
        output_style: null,
      },
    ]);
  };

  return (
    <div className="flex-none border-t border-line px-3 pb-3.5 pt-3 dark:border-line-d">
      <div className="flex items-baseline gap-2 pb-2">
        <span className="text-sm font-semibold text-text2 dark:text-text2-d">New from template</span>
        <div className="flex-1" />
        <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
          {templates == null ? "…" : templates.length}
        </span>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {(templates ?? []).map((t) => {
          const already = agents.some((a) => a.id === t.id);
          return (
            <button
              key={t.id}
              type="button"
              title={already ? `${t.name} — you already have one` : t.role}
              onClick={() => createAgentFromTemplate(t.id)}
              className={cx(
                CHIP,
                "px-2.5 py-1 text-xs font-normal",
                already ? "text-text4 dark:text-text4-d" : "text-text2 dark:text-text2-d",
              )}
            >
              {t.name}
            </button>
          );
        })}
        <button
          type="button"
          onClick={custom}
          className={cx(CHIP, "border-dashed px-2.5 py-1 text-xs font-normal text-text4 dark:text-text4-d")}
        >
          custom
        </button>
      </div>
      <div className="pt-2.5 text-xs leading-normal text-text4 dark:text-text4-d">
        A template is a menu entry. Nothing is installed until you pick one.
      </div>
    </div>
  );
}

/** One of the five knobs across the top of a profile. Clicking cycles it. */
/** Why a model is listed but not offered first. The same two failures the
 *  catalogue itself names: an agent that cannot call tools cannot act, and one
 *  without room cannot hold the repository. */
function caveatOf(m: CatalogModel): string | null {
  if (!m.tool_call) return "cannot call tools";
  if (m.context > 0 && m.context < 64000) return `${Math.round(m.context / 1000)}k context`;
  return null;
}

/** The models an endpoint actually offers, from models.dev for the hosted ones
 *  and from the machine itself for a local Ollama.
 *
 *  A name typed from memory fails twenty minutes into a run, and the two ways
 *  it fails are invisible until then: a model that cannot call tools produces
 *  prose about the work instead of doing it, and one with a small context
 *  cannot hold the repository. Both look like Relay being broken. So the ones
 *  that can do the job are listed first and the rest say why not. */
function ModelPicker({
  providerId,
  baseUrl,
  aliases,
  chosen,
  onPick,
}: {
  providerId: string;
  baseUrl: string;
  /** Os apelidos que este endpoint aceita, fixados no topo. Só o login da
   *  Claude tem — e são uma escolha a sério, não um atalho: um perfil em
   *  `opus` sobe de versão sozinho, um perfil em `claude-opus-4-8` não. */
  aliases?: { id: string; name: string; hint: string }[];
  chosen: string;
  onPick: (id: string) => void;
}) {
  const [models, setModels] = useState<CatalogModel[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [find, setFind] = useState("");

  useEffect(() => {
    let alive = true;
    setModels(null);
    setError(null);
    api
      .modelCatalog(providerId, baseUrl)
      .then((rows) => alive && setModels(rows))
      .catch((e) => alive && setError(reason(e)));
    return () => {
      alive = false;
    };
  }, [providerId, baseUrl]);

  const needle = find.trim().toLowerCase();
  const shown = (models ?? []).filter(
    (m) => !needle || m.id.toLowerCase().includes(needle) || m.name.toLowerCase().includes(needle),
  );

  return (
    <div>
      <Eyebrow className="block pb-2">MODEL</Eyebrow>

      <input
        value={find}
        onChange={(e) => setFind(e.target.value)}
        aria-label="Search the models this endpoint offers"
        placeholder={
          models === null ? "reading what this endpoint offers…" : `search ${models.length} models`
        }
        spellCheck={false}
        className={cx(FIELD, "px-3 py-2.5 font-mono text-[12px]")}
      />

      {error && (
        <p className="mb-0 mt-2 text-sm leading-relaxed text-warn dark:text-warn-d">
          {error} — the name can still be typed, it just is not being checked.
        </p>
      )}

      {models !== null && models.length === 0 && !error && (
        <p className="mb-0 mt-2 text-sm leading-relaxed text-text4 dark:text-text4-d">
          Nothing to list for this endpoint. A local Ollama reports only what has been
          pulled onto this machine — `ollama pull qwen3.5` and it appears here.
        </p>
      )}

      {/* Os apelidos primeiro, e separados. `opus` e `claude-opus-4-8` não são
          a mesma escolha: o primeiro nomeia uma família e deixa a versão ao
          login da Claude no momento do run, o segundo fixa uma. A lista não
          deve fazer parecer que são duas maneiras de dizer o mesmo. */}
      {aliases && aliases.length > 0 && !needle && (
        <div className="mt-2 overflow-hidden rounded-md border border-line2 dark:border-line2-d">
          {aliases.map((a) => {
            const picked = a.id === chosen;
            return (
              <button
                key={a.id}
                type="button"
                onClick={() => onPick(a.id)}
                className={cx(
                  ROW,
                  "flex w-full items-baseline gap-2.5 border-b border-line2 px-3 py-2.5 text-left dark:border-line2-d",
                  picked ? "bg-accentSoft dark:bg-accentSoft-d" : "bg-transparent",
                )}
              >
                <span
                  className={cx(
                    mono,
                    "flex-none text-sm",
                    picked ? "text-accent dark:text-accent-d" : "text-text1 dark:text-text1-d",
                  )}
                >
                  {a.id}
                </span>
                <span className="min-w-0 flex-1 truncate text-xs text-text3 dark:text-text3-d">
                  {a.hint}
                </span>
                <span className="flex-none text-xs text-text4 dark:text-text4-d">
                  version picked at run time
                </span>
              </button>
            );
          })}
        </div>
      )}

      <div
        className={cx(
          "mt-2 max-h-[260px] overflow-y-auto rounded-md",
          !!models?.length && "border border-line2 dark:border-line2-d",
        )}
      >
        {shown.map((m) => {
          const picked = m.id === chosen;
          return (
            <button
              key={m.id}
              type="button"
              onClick={() => onPick(m.id)}
              className={cx(
                ROW,
                "flex w-full items-baseline gap-2.5 border-b border-line2 px-3 py-2.5 text-left dark:border-line2-d",
                picked ? "bg-accentSoft dark:bg-accentSoft-d" : "bg-transparent",
                m.usable ? "opacity-100" : "opacity-55",
              )}
            >
              <span
                className={cx(
                  mono,
                  truncate,
                  "flex-1 text-sm",
                  picked ? "text-accent dark:text-accent-d" : "text-text1 dark:text-text1-d",
                )}
              >
                {m.id}
              </span>
              {caveatOf(m) ? (
                <span className="flex-none text-xs font-normal text-warn dark:text-warn-d">
                  {caveatOf(m)}
                </span>
              ) : (
                <span className={cx(mono, "flex-none text-xs text-text4 dark:text-text4-d")}>
                  {Math.round(m.context / 1000)}k
                  {m.input_cost ? ` · $${m.input_cost}/M` : " · free"}
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/* The list is a convenience, never the only way in. A catalogue can be
          empty, stale, or unreachable, and an endpoint can serve a model nobody
          has published — none of which should leave the operator unable to name
          the model they meant. */}
      <div className="mt-2 flex items-center gap-2">
        <span className={cx(mono, "flex-none text-xs text-text4 dark:text-text4-d")}>set to</span>
        <input
          key={chosen}
          defaultValue={chosen}
          aria-label="Model name"
          placeholder="type a name the endpoint knows"
          spellCheck={false}
          onBlur={(e) => {
            const next = e.target.value.trim();
            if (next !== chosen) onPick(next);
          }}
          className={cx(FIELD, "min-w-0 flex-1 px-2.5 py-2 font-mono text-sm")}
        />
      </div>
    </div>
  );
}

/** Um mosaico que é uma escolha de uma lista.
 *
 *  Era um `Knob` que ciclava: para chegar ao terceiro endpoint de quatro
 *  clicava-se três vezes, sem nunca ver quais eram os outros, e passar do fim
 *  ao princípio dava a volta sem avisar. Uma lista de escolhas quer-se vista
 *  toda de uma vez — e o `select` nativo já é o que o resto deste ecrã usa para
 *  "reports to" e "escalates to", portanto é também o que aqui não introduz um
 *  padrão novo. */
function KnobPick({
  label,
  hint,
  value,
  options,
  onPick,
}: {
  label: string;
  hint: string;
  value: string;
  options: { id: string; name: string }[];
  onPick: (id: string) => void;
}) {
  return (
    <div className="bg-surface px-3.5 py-3 text-left dark:bg-surface-d">
      <div className="text-xs font-normal tracking-[.08em] text-text4 dark:text-text4-d">
        {label}
      </div>
      <select
        value={value}
        aria-label={label}
        onChange={(e) => onPick(e.target.value)}
        className="mt-1 w-full cursor-pointer truncate border-none bg-transparent p-0 text-md font-semibold text-text1 outline-none dark:text-text1-d"
      >
        {options.map((o) => (
          <option key={o.id} value={o.id}>
            {o.name}
          </option>
        ))}
      </select>
      <div className="mt-1 text-xs font-normal leading-snug text-text4 dark:text-text4-d">
        {hint}
      </div>
    </div>
  );
}

/** What the ChatGPT plan has left, for an agent that runs on it.
 *
 *  This sits where the budget knob sits for a Claude agent, and it is not the
 *  same kind of thing: a budget is a ceiling the operator sets, this is a fact
 *  the provider reports. It is read from Codex rather than added up here,
 *  because the plan is spent by everything on the machine — a `codex` in a
 *  terminal counts too — so a total Relay kept itself would be lower than the
 *  truth and look like a bug the first time somebody compared it. */
function PlanMeter() {
  const [usage, setUsage] = useState<PlanUsage | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .codexPlanUsage()
      .then((u) => alive && setUsage(u))
      .catch((e) => alive && setError(reason(e)));
    return () => {
      alive = false;
    };
  }, []);

  if (error) {
    return (
      <p className="mb-0 text-sm leading-relaxed text-warn dark:text-warn-d">
        Could not read the plan: {error}
      </p>
    );
  }
  // Nada enquanto vem. Uma barra vazia e uma barra a zero por cento são
  // desenhos diferentes da mesma coisa, e só uma delas é verdade.
  if (!usage) return null;

  const windows = [
    {
      k: usage.primary_window_mins ? windowName(usage.primary_window_mins) : "this window",
      pct: usage.primary_percent,
      resets: usage.primary_resets_at,
    },
    {
      k: usage.secondary_window_mins ? windowName(usage.secondary_window_mins) : "the long window",
      pct: usage.secondary_percent,
      resets: usage.secondary_resets_at,
    },
  ];

  return (
    <div className="flex flex-col gap-2.5">
      <Eyebrow className="block">
        {usage.plan ? `${usage.plan.toUpperCase()} PLAN` : "PLAN"} · SPENT
      </Eyebrow>
      {windows.map((w) => (
        <div key={w.k} className="flex flex-col gap-1">
          <div className="flex items-baseline justify-between">
            <span className="text-sm text-text3 dark:text-text3-d">{w.k}</span>
            <span className={cx(mono, "text-sm text-text2 dark:text-text2-d")}>
              {w.pct}%{w.resets ? ` · resets ${shortWhen(w.resets)}` : ""}
            </span>
          </div>
          <div className="h-1 w-full overflow-hidden rounded-full bg-active dark:bg-active-d">
            <div
              className={cx(
                "h-full rounded-full",
                w.pct >= 90 ? "bg-warn dark:bg-warn-d" : "bg-accent dark:bg-accent-d",
              )}
              style={{ width: `${Math.min(100, Math.max(0, w.pct))}%` }}
            />
          </div>
        </div>
      ))}
      {usage.reached && (
        <p className="mb-0 text-sm leading-relaxed text-warn dark:text-warn-d">
          The plan says this limit is reached ({usage.reached}). Runs on this agent will fail
          until it resets.
        </p>
      )}
    </div>
  );
}

/** Codex reports a window as a length in minutes, not as a name. */
function windowName(mins: number): string {
  if (mins % (60 * 24 * 7) === 0) return plural(mins / (60 * 24 * 7), "week");
  if (mins % (60 * 24) === 0) return plural(mins / (60 * 24), "day");
  if (mins % 60 === 0) return plural(mins / 60, "hour");
  return plural(mins, "minute");
}

/** A unix second, as a short local time. */
function shortWhen(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

function Knob({
  label,
  value,
  hint,
  onCycle,
}: {
  label: string;
  value: string;
  hint: string;
  /** Absent when the value is not a cycle — a model name typed by hand, say.
   *  A tile that looks clickable and does nothing is worse than a still one. */
  onCycle?: () => void;
}) {
  const body = (
    <>
      <div className="text-xs font-normal tracking-[.08em] text-text4 dark:text-text4-d">
        {label}
      </div>
      <div className="mt-1 text-md font-semibold text-text1 dark:text-text1-d">{value}</div>
      <div className="mt-1 text-xs font-normal leading-snug text-text4 dark:text-text4-d">
        {hint}
      </div>
    </>
  );
  const skin = "bg-surface px-3.5 py-3 text-left dark:bg-surface-d";
  // Um botão quando cicla, uma caixa quando não: um mosaico que parece
  // clicável e não faz nada é pior do que um que está quieto.
  return onCycle ? (
    <button type="button" onClick={onCycle} className={cx(skin, ROW, "cursor-pointer")}>
      {body}
    </button>
  ) : (
    <div className={cx(skin, "cursor-default")}>{body}</div>
  );
}

function Toggle({
  label,
  hint,
  on,
  onChange,
}: {
  label: string;
  hint: string;
  on: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={cx(
        ROW,
        "flex w-full cursor-pointer items-start gap-3 border-b border-line px-3.5 py-3 text-left dark:border-line-d",
      )}
    >
      <span
        className={cx(
          "mt-px flex h-4.5 w-[30px] flex-none items-center rounded-full p-0.5 transition-colors duration-150",
          on
            ? "justify-end bg-accentLine dark:bg-accentLine-d"
            : "justify-start bg-line3 dark:bg-line3-d",
        )}
      >
        <span
          className={cx(
            "h-3.5 w-3.5 rounded-full transition-colors duration-150",
            on ? "bg-accent2 dark:bg-accent2-d" : "bg-line4 dark:bg-line4-d",
          )}
        />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-md font-medium text-text1 dark:text-text1-d">{label}</span>
        <span className="mt-0.5 block text-xs font-normal leading-normal text-text4 dark:text-text4-d">
          {hint}
        </span>
      </span>
    </button>
  );
}

export function Agents({
  selected,
  select,
  openChat,
  openSession,
}: {
  selected: string | null;
  select: (id: string) => void;
  openChat: (conversationId?: string, profileId?: string) => void;
  openSession: (cardId: string) => void;
}) {
  const {
    agents,
    agentStats,
    snapshot,
    settings,
    saveAgents,
    duplicateAgent,
    removeAgent,
    conversations,
    projects,
  } = useStore();

  const cards = snapshot?.cards ?? [];
  const agent = agents.find((a) => a.id === selected) ?? agents[0] ?? null;
  const [skill, setSkill] = useState("");

  const teams = useMemo(() => {
    const groups = new Map<string, AgentProfile[]>();
    agents.forEach((a) => {
      const key = (a.team || "other").toUpperCase();
      groups.set(key, [...(groups.get(key) ?? []), a]);
    });
    return [...groups.entries()];
  }, [agents]);

  if (!agent) {
    return (
      <div className="grid flex-1 place-items-center text-text3 dark:text-text3-d">
        No profiles yet.
      </div>
    );
  }

  const patch = (next: Partial<AgentProfile>) =>
    saveAgents(agents.map((a) => (a.id === agent.id ? { ...a, ...next } : a)));

  const stats = agentStats[agent.id];
  const t = tone(agent.tone);
  const mine = cards.filter((c) => c.agent_id === agent.id);
  const running = mine.filter((c) => c.status === "running").length;
  const st = stateOf(agent, running);
  const chats = conversations.filter((c) => c.profile_id === agent.id);

  // Each knob steps through its own list, so the whole strip is editable
  // without a form.
  const cycle = <T,>(list: T[], current: T): T => {
    const at = list.findIndex((x) => x === current);
    return list[(at + 1) % list.length]!;
  };
  const budgets = [0.25, 0.5, 1, 2, 5, null];

  // O login incorporado da Claude é um endpoint como os outros para efeitos de
  // escolha; o que o distingue é aceitar apelidos além dos nomes exactos.
  const providers = settings?.providers ?? [];
  const endpoint = providers.find((p) => p.id === agent.provider);
  const ALIAS_HINTS: Record<string, string> = {
    opus: "Deepest reasoning, highest cost",
    sonnet: "The everyday worker",
    haiku: "Fast and cheap, for lookups",
  };

  // Escolhas de uma lista: vêem-se todas de uma vez em vez de se ciclar até
  // acertar. As duas que restam em `Knob` são um número de 1 a 4 e seis
  // orçamentos, onde ciclar é mais rápido do que abrir uma lista.
  const onCodex = agent.backend === "codex";
  const picks = [
    {
      label: "AGENT",
      value: agent.backend,
      hint: onCodex
        ? "Codex, on this machine's ChatGPT plan — no key, no per-run cost"
        : "Claude, on the login or endpoint below",
      options: [
        { id: "claude", name: "Claude" },
        { id: "codex", name: "Codex" },
      ],
      // Mudar de agente invalida o modelo pela mesma razão que mudar de
      // endpoint o invalida, e com mais força: os dois nem sequer partilham o
      // vocabulário. `sonnet` não quer dizer nada ao Codex.
      onPick: (id: string) =>
        patch(
          id === agent.backend
            ? {}
            : { backend: id as Backend, model: null, ...(id === "codex" ? { budget_usd: null } : {}) },
        ),
    },
    // Um endpoint só quer dizer alguma coisa a quem lê ANTHROPIC_BASE_URL. Num
    // perfil de Codex não é uma escolha desactivada — é uma escolha que não
    // existe, e um selector a cinzento dizia que existia.
    ...(onCodex
      ? []
      : [
          {
            label: "RUNS ON",
            value: agent.provider ?? "",
            hint: endpoint ? endpoint.base_url : "The Claude login this machine already has",
            options: [
              { id: "", name: "Claude login" },
              ...providers.map((p) => ({ id: p.id, name: p.name })),
            ],
            onPick: (id: string) =>
              // Trocar de endpoint invalida o modelo: `opus` não quer dizer nada num
              // Ollama, e `qwen3.5` não quer dizer nada na Anthropic. Limpar é mais
              // honesto do que deixar lá um nome que vai falhar a meio de um run.
              patch({ provider: id, model: id === (agent.provider ?? "") ? agent.model : null }),
          },
        ]),
    {
      label: "REVIEWER",
      value: agent.reviewer,
      hint: REVIEWERS.find((r) => r.id === agent.reviewer)!.hint,
      options: REVIEWERS.map((r) => ({ id: r.id, name: r.name })),
      // The generated vocabulary types ids as strings; the profile wants the
      // narrowed enum. The cast is safe because the list is written from the
      // Rust enum itself.
      onPick: (id: string) => patch({ reviewer: id as Reviewer }),
    },
    {
      label: "WORKTREE",
      value: agent.worktree,
      hint: WORKTREE_MODES.find((w) => w.id === agent.worktree)!.hint,
      options: WORKTREE_MODES.map((w) => ({ id: w.id, name: w.name })),
      onPick: (id: string) => patch({ worktree: id as WorktreeMode }),
    },
  ];

  const knobs = [
    {
      label: "AT ONCE",
      value: plural(agent.max_concurrent, "card"),
      hint: `A ${agent.max_concurrent === 1 ? "second" : "further"} card waits`,
      onCycle: () => patch({ max_concurrent: (agent.max_concurrent % 4) + 1 }),
    },
    // Um tecto em dólares não se aplica a um plano que não cobra em dólares.
    // O botão sai em vez de ficar a cinzento: um cap desactivado dizia que
    // existia um cap. O que o substitui é o `PlanMeter`, com a percentagem da
    // janela — que é um número real e não uma conversão inventada.
    ...(onCodex
      ? []
      : [
          {
            label: "BUDGET",
            value: agent.budget_usd == null ? "no cap" : money(agent.budget_usd),
            hint: settings
              ? `Counts against ${money(settings.daily_budget_usd, 0)} a day`
              : "per run",
            onCycle: () => patch({ budget_usd: cycle(budgets, agent.budget_usd) }),
          },
        ]),
  ];

  const week = stats?.week_runs ?? [0, 0, 0, 0, 0, 0, 0];
  const peak = Math.max(1, ...week);
  // Six zeros and a flat sparkline are a chart of nothing wearing the clothes
  // of a chart of something. A profile that has never run says so instead.
  const neverRan = (stats?.runs ?? 0) === 0;
  const numbers = [
    { k: "runs", v: num(stats?.runs ?? 0), fg: "text-text1 dark:text-text1-d" },
    { k: "cards done", v: num(stats?.cards_done ?? 0), fg: "text-ok dark:text-ok-d" },
    { k: "sent back", v: num(stats?.sent_back ?? 0), fg: "text-warn dark:text-warn-d" },
    { k: "spend", v: money(stats?.spend ?? 0), fg: "text-text1 dark:text-text1-d" },
    { k: "avg / card", v: money(stats?.avg_cost ?? 0), fg: "text-text1 dark:text-text1-d" },
    { k: "commits", v: num(stats?.commits ?? 0), fg: "text-text1 dark:text-text1-d" },
  ];

  return (
    <motion.div
      variants={paneIn}
      initial="hidden"
      animate="shown"
      className="grid min-h-0 flex-1 grid-cols-[266px_minmax(0,1fr)] overflow-hidden"
    >
      <div className="flex min-h-0 flex-col overflow-hidden border-r border-line dark:border-line-d">
        {/* A lista chega linha a linha. É o `.stagger` do desenho, agora com os
            mesmos atrasos escritos num sítio só. */}
        <motion.div
          initial="hidden"
          animate="shown"
          className="min-h-0 flex-1 overflow-y-auto px-2.5 pb-3 pt-2.5"
        >
          {teams.map(([team, members], ti) => (
            <motion.div key={team} custom={ti} variants={rowIn}>
              <Eyebrow className="block px-2 pb-1.5 pt-2.5">{team}</Eyebrow>
              {members.map((a) => {
                const at = tone(a.tone);
                const on = a.id === agent.id;
                const busy = cards.filter((c) => c.status === "running" && c.agent_id === a.id).length;
                const state = stateOf(a, busy);
                return (
                  <button
                    key={a.id}
                    type="button"
                    aria-pressed={on}
                    onClick={() => select(a.id)}
                    className={cx(
                      ROW,
                      "flex w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 py-2 text-left",
                      on
                        ? "bg-active shadow-[inset_0_0_0_1px_theme(colors.line3.DEFAULT)] dark:bg-active-d dark:shadow-[inset_0_0_0_1px_theme(colors.line3.d)]"
                        : "bg-transparent",
                    )}
                  >
                    <Glyph tone={at} size={24} radius={8} font={9.5}>
                      {a.initial}
                    </Glyph>
                    <span className="min-w-0 flex-1">
                      <span
                        className={cx(
                          truncate,
                          "block text-md font-semibold",
                          on ? "text-text dark:text-text-d" : "text-text1 dark:text-text1-d",
                        )}
                      >
                        {a.name}
                      </span>
                      <span
                        className={cx(mono, truncate, "block text-xs text-text4 dark:text-text4-d")}
                      >
                        {a.title} · {modelLabel(a.model).label}
                      </span>
                    </span>
                    <span className={cx("text-xs font-medium", state.fg)}>{state.label}</span>
                  </button>
                );
              })}
            </motion.div>
          ))}
        </motion.div>
        <Templates />
      </div>

      <div className="min-h-0 min-w-0 overflow-y-auto">
        <motion.div
          custom={0}
          variants={rowIn}
          initial="hidden"
          animate="shown"
          className="flex items-start gap-3.5 border-b border-line px-5.5 pb-3.5 pt-4.5 dark:border-line-d"
        >
          <Glyph tone={t} size={38} radius={12} font={14}>
            {agent.initial}
          </Glyph>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2.5">
              <input
                value={agent.name}
                aria-label="Agent name"
                onChange={(e) => patch({ name: e.target.value })}
                className="border-none bg-transparent p-0 text-xl font-semibold tracking-[-.01em] text-text outline-none dark:text-text-d"
                style={{ width: `${Math.max(6, agent.name.length + 1)}ch` }}
              />
              <input
                value={agent.team}
                onChange={(e) => patch({ team: e.target.value })}
                placeholder="team"
                aria-label="Team"
                className={cx(
                  mono,
                  "w-[108px] rounded-sm border-none bg-surface2 px-2 py-0.5 text-xs text-text2 outline-none dark:bg-surface2-d dark:text-text2-d",
                )}
              />
              <span
                className={cx(
                  "rounded-sm px-2 py-0.5 text-xs font-semibold",
                  running > 0
                    ? "bg-okSoft text-ok dark:bg-okSoft-d dark:text-ok-d"
                    : "bg-surface2 text-text3 dark:bg-surface2-d dark:text-text3-d",
                )}
              >
                {st.label}
              </span>
            </div>
            <input
              value={agent.role}
              onChange={(e) => patch({ role: e.target.value })}
              placeholder="What is this one for?"
              aria-label="What this agent is for"
              className="mt-1 block w-full border-none bg-transparent p-0 text-md font-normal text-text3 outline-none dark:text-text3-d"
            />
          </div>
          {[
            {
              label: "Chat",
              run: () => openChat(chats[0]?.id, agent.id),
              off: !agent.chat_enabled || agent.paused,
            },
            { label: "Duplicate", run: () => duplicateAgent(agent.id), off: false },
            {
              label: agent.paused ? "Resume" : "Pause",
              run: () => patch({ paused: !agent.paused }),
              off: false,
            },
          ].map((b) => (
            <button
              key={b.label}
              type="button"
              disabled={b.off}
              onClick={b.run}
              className={cx(
                CHIP,
                "rounded-sm px-3.5 py-2 text-sm font-medium text-text2 disabled:cursor-not-allowed disabled:opacity-45 dark:text-text2-d",
              )}
            >
              {b.label}
            </button>
          ))}
        </motion.div>

        <motion.div
          initial="hidden"
          animate="shown"
          className="flex flex-col gap-4.5 px-5.5 pb-5 pt-4"
        >
          <motion.div
            custom={0}
            variants={rowIn}
            className="grid grid-cols-[repeat(5,minmax(0,1fr))] gap-px overflow-hidden rounded-md border border-line bg-line dark:border-line-d dark:bg-line-d"
          >
            {picks.map((k) => (
              <KnobPick key={k.label} {...k} />
            ))}
            {knobs.map((k) => (
              <Knob key={k.label} {...k} />
            ))}
          </motion.div>

          {onCodex && <PlanMeter />}

          <motion.div
            custom={1}
            variants={rowIn}
            className="grid grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)] gap-4"
          >
            <div className="flex flex-col gap-3.5">
              <div>
                <Eyebrow className="block pb-1.5">BRIEF</Eyebrow>
                <textarea
                  rows={4}
                  value={agent.brief}
                  onChange={(e) => patch({ brief: e.target.value })}
                  placeholder="What is it told before every run?"
                  aria-label="Brief"
                  className={cx(
                    FIELD,
                    "resize-y rounded-md px-3.5 py-3 text-md font-normal leading-[1.7] text-text2 dark:text-text2-d",
                  )}
                />
              </div>
              <div>
                <Eyebrow className="block pb-1.5">EXPECTED OUTPUT</Eyebrow>
                <textarea
                  rows={2}
                  value={agent.expected_output}
                  onChange={(e) => patch({ expected_output: e.target.value })}
                  placeholder="What finished work looks like."
                  aria-label="Expected output"
                  className={cx(
                    FIELD,
                    "resize-y rounded-md px-3.5 py-3 text-md font-normal leading-[1.7] text-text2 dark:text-text2-d",
                  )}
                />
              </div>
              {/* Sempre um selector, e para o login da Claude também. Era um
                  mosaico que ciclava entre "Opus", "Sonnet" e "Haiku", que não
                  são modelos — são apelidos, e nenhum deles diz em que Opus o
                  agente vai correr. O catálogo dá os nomes a sério (models.dev
                  para os hospedados, a própria máquina para um Ollama local) e
                  os apelidos ficam no topo, ditos como o que são. */}
              <ModelPicker
                providerId={onCodex ? "codex" : endpoint ? endpoint.id : "anthropic"}
                baseUrl={onCodex ? "" : endpoint ? endpoint.base_url : ""}
                aliases={
                  endpoint || onCodex
                    ? undefined
                    : MODELS.map((m) => ({
                        id: m.id,
                        name: m.name,
                        hint: ALIAS_HINTS[m.id] ?? m.hint,
                      }))
                }
                chosen={agent.model ?? ""}
                onPick={(id) => patch({ model: id || null })}
              />

              <div>
                <Eyebrow className="block pb-2">TOOLS IT MAY USE</Eyebrow>
                <div className="flex flex-wrap gap-1.5">
                  {ALL_PERMISSIONS.map((p) => {
                    const on = agent.permissions.includes(p);
                    return (
                      <button
                        key={p}
                        type="button"
                        role="checkbox"
                        aria-checked={on}
                        onClick={() =>
                          patch({
                            permissions: on
                              ? agent.permissions.filter((x) => x !== p)
                              : [...agent.permissions, p],
                          })
                        }
                        className={cx(
                          "flex min-h-6 cursor-pointer items-center gap-1.5 rounded-sm border px-2.5 py-1.5 text-sm font-medium transition-colors duration-150",
                          on
                            ? "border-accentLine bg-accentSoft text-text1 dark:border-accentLine-d dark:bg-accentSoft-d dark:text-text1-d"
                            : "border-line2 bg-surface text-text4 hover:bg-hovered hover:text-text dark:border-line2-d dark:bg-surface-d dark:text-text4-d dark:hover:bg-hovered-d dark:hover:text-text-d",
                        )}
                      >
                        <span
                          className={cx(
                            "h-3 w-3 rounded-[4px] border",
                            on
                              ? "border-accent2 bg-accent2 dark:border-accent2-d dark:bg-accent2-d"
                              : "border-line3 bg-transparent dark:border-line3-d",
                          )}
                        />
                        {p}
                      </button>
                    );
                  })}
                </div>
                <div className="pt-2 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                  Anything outside this list is refused before it runs. Anything inside it still asks
                  you, unless a scoped standing rule covers it.
                </div>
              </div>
              <div>
                <Eyebrow className="block pb-2">SKILLS</Eyebrow>
                <div className="flex flex-wrap items-center gap-1.5">
                  {agent.skills.map((s) => (
                    <button
                      key={s}
                      type="button"
                      title="Remove"
                      aria-label={`Remove the ${s} skill`}
                      onClick={() => patch({ skills: agent.skills.filter((x) => x !== s) })}
                      className={cx(
                        mono,
                        "min-h-6 cursor-pointer rounded-full border border-line2 bg-surface px-2.5 py-1 text-sm text-text2 transition-colors duration-150 hover:bg-hovered hover:text-text dark:border-line2-d dark:bg-surface-d dark:text-text2-d dark:hover:bg-hovered-d dark:hover:text-text-d",
                      )}
                    >
                      {s}
                    </button>
                  ))}
                  <input
                    value={skill}
                    onChange={(e) => setSkill(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key !== "Enter" || !skill.trim()) return;
                      patch({ skills: [...agent.skills, skill.trim()] });
                      setSkill("");
                    }}
                    placeholder="add"
                    aria-label="Add a skill"
                    className="w-24 rounded-full border border-dashed border-line3 bg-transparent px-2.5 py-1 text-sm font-normal text-text2 outline-none dark:border-line3-d dark:text-text2-d"
                  />
                </div>
                <div className="pt-2 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                  Words for the brief, not packages: these go into the prompt as what it is relied
                  on for. Installed skills are below.
                </div>
              </div>
              <Granted agent={agent} patch={patch} />
              <Skills agent={agent} />
              <Browsers agent={agent} />
              <div>
                <Eyebrow className="block pb-2">MCP AND SKILLS ARE PER AGENT</Eyebrow>
                <div className="text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                  Nothing is inherited from this machine. A run loads this agent's own folder and
                  the servers listed here — never your <span className={mono}>~/.claude</span>, and
                  never a <span className={mono}>.claude</span> or{" "}
                  <span className={mono}>.mcp.json</span> inside the repository being worked on.
                </div>
              </div>
              <div>
                <Eyebrow className="block pb-2">WHICH BOARD IT OWNS</Eyebrow>
                {agent.id === "director" ? (
                  <div className="text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                    The Director is handed every board's finished work, so he is never fenced to
                    one. Fencing him would leave him unable to act on what he is given.
                  </div>
                ) : (
                  <>
                    <select
                      value={agent.project ?? ""}
                      aria-label="Which board it owns"
                      onChange={(e) => patch({ project: e.target.value || null })}
                      className={cx(FIELD, "cursor-pointer px-2.5 py-2 text-md font-normal text-text2 dark:text-text2-d")}
                    >
                      <option value="">Every project</option>
                      {projects.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                    <div className="pt-2 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                      {agent.project
                        ? agent.can_delegate
                          ? "A fence, not a preference: board tools default here and refuse any other project. Finished work on this board is reviewed by this agent instead of the Director."
                          : "This profile cannot change a board, so the fence does nothing and the Director still reviews. Turn on \u201ccan put work on a board\u201d below."
                        : "Acts on whichever project the conversation is on, like the Director."}
                    </div>
                  </>
                )}
              </div>

              <div>
                <Eyebrow className="block pb-2">WHERE IT SITS</Eyebrow>
                <div className="flex gap-2.5">
                  {[
                    {
                      label: "reports to",
                      value: agent.reports_to ?? "",
                      set: (v: string) => patch({ reports_to: v || null }),
                      none: "Nobody",
                    },
                    {
                      label: "escalates to",
                      value: agent.escalate_to ?? "",
                      set: (v: string) => patch({ escalate_to: v || null }),
                      none: "You",
                    },
                  ].map((f) => (
                    <div key={f.label} className="flex-1">
                      <div className={cx(mono, "pb-1 text-xs text-text4 dark:text-text4-d")}>
                        {f.label}
                      </div>
                      <select
                        value={f.value}
                        aria-label={f.label}
                        onChange={(e) => f.set(e.target.value)}
                        className={cx(FIELD, "cursor-pointer px-2.5 py-2 text-md font-normal text-text2 dark:text-text2-d")}
                      >
                        <option value="">{f.none}</option>
                        {agents
                          .filter((o) => o.id !== agent.id)
                          .map((o) => (
                            <option key={o.id} value={o.id}>
                              {o.name}
                            </option>
                          ))}
                      </select>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            <div className="flex flex-col gap-3.5">
              <div className="overflow-hidden rounded-md border border-line2 bg-surface dark:border-line2-d dark:bg-surface-d">
                <Toggle
                  label="Can hold a conversation"
                  hint="Gets its own chat and its own resumable session."
                  on={agent.chat_enabled}
                  onChange={(v) => patch({ chat_enabled: v })}
                />
                <Toggle
                  label="Can be given cards"
                  hint="Turn this off and the board will not offer it."
                  on={agent.tasks_enabled}
                  onChange={(v) => patch({ tasks_enabled: v })}
                />
                <Toggle
                  label="Can put work on a board for others"
                  hint="Off: it can describe work, not create or move cards."
                  on={agent.can_delegate}
                  onChange={(v) => patch({ can_delegate: v })}
                />
                <div className="px-3.5 py-3 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                  Board changes an agent makes still come to you as a permission request — the same
                  sheet a shell command uses.
                </div>
              </div>

              <div className="rounded-md border border-line2 bg-surface p-3.5 dark:border-line2-d dark:bg-surface-d">
                <div className="flex items-baseline gap-2 pb-2.5">
                  <span className="text-sm font-semibold text-text1 dark:text-text1-d">
                    What it has done
                  </span>
                  <div className="flex-1" />
                  <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>all time</span>
                </div>
                {neverRan ? (
                  <p className="m-0 text-md font-normal leading-relaxed text-text3 dark:text-text3-d">
                    {agent.name} has not run yet. Give it a card and its runs, spend and
                    commits are counted here from the event log.
                  </p>
                ) : (
                  <>
                    <div className="grid grid-cols-[repeat(3,minmax(0,1fr))] gap-x-2 gap-y-3">
                      {numbers.map((n) => (
                        <div key={n.k}>
                          <div className={cx(mono, "text-xl font-semibold", n.fg)}>{n.v}</div>
                          <div className="mt-px text-xs font-normal text-text4 dark:text-text4-d">
                            {n.k}
                          </div>
                        </div>
                      ))}
                    </div>
                    <div className="flex h-[34px] items-end gap-1 pt-3">
                      {week.map((v, i) => (
                        <span
                          key={i}
                          className={cx(
                            "flex-1 origin-bottom animate-[grow_.55s_cubic-bezier(.2,.8,.25,1)_both] rounded-2px",
                            v === peak && v > 0
                              ? "bg-accent dark:bg-accent-d"
                              : "bg-line3 dark:bg-line3-d",
                          )}
                          style={{
                            height: `${Math.max(6, Math.round((v / peak) * 100))}%`,
                            animationDelay: `${0.06 + i * 0.05}s`,
                          }}
                        />
                      ))}
                    </div>
                    <div className={cx(mono, "pt-1.5 text-xs text-text4 dark:text-text4-d")}>
                      runs, last 7 days
                    </div>
                  </>
                )}
              </div>

              {mine.length > 0 && (
                <div className="overflow-hidden rounded-md border border-line2 bg-surface dark:border-line2-d dark:bg-surface-d">
                  <div className="px-3.5 pb-2 pt-3 text-sm font-semibold text-text1 dark:text-text1-d">
                    Its cards here
                  </div>
                  {mine.slice(0, 6).map((c) => (
                    <button
                      key={c.id}
                      type="button"
                      onClick={() => openSession(c.id)}
                      className={cx(
                        ROW,
                        "flex w-full cursor-pointer items-center gap-2.5 border-t border-line px-3.5 py-2 text-left dark:border-line-d",
                      )}
                    >
                      <span
                        className={cx(
                          truncate,
                          "flex-1 text-sm font-normal text-text2 dark:text-text2-d",
                        )}
                      >
                        {c.title}
                      </span>
                      <span className={cx(mono, "text-xs text-text4 dark:text-text4-d")}>
                        {money(c.cost_usd, 2)}
                      </span>
                    </button>
                  ))}
                </div>
              )}

              <div className="rounded-md border border-bad px-3.5 py-3 dark:border-bad-d">
                <div className="flex items-center gap-2.5">
                  <span className="flex-1 text-sm font-medium text-text2 dark:text-text2-d">
                    Remove this profile
                  </span>
                  <button
                    type="button"
                    disabled={agent.id === "director"}
                    onClick={() => removeAgent(agent.id)}
                    className="min-h-6 cursor-pointer rounded-sm border border-bad px-3 py-1.5 text-sm font-semibold text-bad2 transition-colors duration-150 hover:bg-badSoft disabled:cursor-not-allowed disabled:opacity-45 disabled:hover:bg-transparent dark:border-bad-d dark:text-bad2-d dark:hover:bg-badSoft-d"
                  >
                    Remove
                  </button>
                </div>
                <div className="pt-1.5 text-xs font-normal leading-normal text-text4 dark:text-text4-d">
                  Finished cards keep their history. The Director cannot be removed — the review loop
                  needs it.
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      </div>
    </motion.div>
  );
}
