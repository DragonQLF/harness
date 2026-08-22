import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";

type Status = "backlog" | "ready" | "running" | "review" | "done";

interface Card {
  id: string;
  title: string;
  status: Status;
  current_run: string | null;
}

interface Snapshot {
  last_seq: number;
  cards: Card[];
  sessions: { card_id: string; worktree: string; session_id: string | null }[];
}

interface AgentStatus {
  cli_found: boolean;
  logged_in: boolean;
}

interface Envelope {
  seq: number;
  type: string;
  card_id?: string;
  title?: string;
  to?: Status;
  reason?: string;
}

interface RunUpdate {
  card_id: string;
  run_id: string;
  kind?: string;
  session_id?: string;
  text?: string;
  tool?: string;
  summary?: string;
  cost_usd?: number;
  message?: string;
}

const COLUMNS: Status[] = ["backlog", "ready", "running", "review", "done"];

function applyEnvelope(cards: Card[], env: Envelope): Card[] {
  switch (env.type) {
    case "card_created":
      return [
        ...cards,
        {
          id: env.card_id!,
          title: env.title ?? "",
          status: "backlog",
          current_run: null,
        },
      ];
    case "card_moved":
    case "card_overridden":
      return cards.map((c) =>
        c.id === env.card_id && env.to ? { ...c, status: env.to } : c,
      );
    case "run_started":
      return cards.map((c) =>
        c.id === env.card_id ? { ...c, status: "running", current_run: env.card_id } : c,
      );
    case "run_finished":
      return cards.map((c) => (c.id === env.card_id ? { ...c, current_run: null } : c));
    case "card_approved":
    case "card_rejected":
      return cards.map((c) =>
        c.id === env.card_id ? { ...c, current_run: null } : c,
      );
    default:
      return cards;
  }
}

function runLine(u: RunUpdate): string | null {
  switch (u.kind) {
    case "started":
      return `> session started (${u.session_id})`;
    case "text":
      return u.text ?? null;
    case "tool_use":
      return `[${u.tool}] ${u.summary ?? ""}`;
    case "done":
      return `done${u.cost_usd != null ? ` - cost $${u.cost_usd.toFixed(4)}` : ""}`;
    case "failed":
      return `FAILED: ${u.message ?? "unknown"}`;
    default:
      return null;
  }
}

export default function App() {
  const [cards, setCards] = useState<Card[]>([]);
  const [sessions, setSessions] = useState<Snapshot["sessions"]>([]);
  const [outputs, setOutputs] = useState<Record<string, string[]>>({});
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const seqRef = useRef(-1);

  const refresh = useCallback(async () => {
    try {
      const snap = await invoke<Snapshot>("snapshot");
      setCards(snap.cards);
      setSessions(snap.sessions ?? []);
      seqRef.current = snap.last_seq;
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await invoke<AgentStatus>("agent_status"));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];

    refresh();

    listen<Envelope>("engine://event", (evt) => {
      if (cancelled) return;
      const env = evt.payload;
      const prev = seqRef.current;
      if (prev >= 0 && env.seq !== prev + 1) {
        refresh();
        return;
      }
      seqRef.current = env.seq;
      setCards((cs) => applyEnvelope(cs, env));
      const verdictLine =
        env.type === "card_approved"
          ? "[director] approved - moving to done"
          : env.type === "card_rejected"
            ? `[director] rejected: ${env.reason ?? "no reason given"}`
            : null;
      if (verdictLine && env.card_id) {
        const cid = env.card_id;
        setOutputs((prev) => ({
          ...prev,
          [cid]: [...(prev[cid] ?? []), verdictLine].slice(-40),
        }));
      }
    }).then((u) => {
      if (cancelled) u();
      else unlistens.push(u);
    });

    listen<RunUpdate>("engine://run", (evt) => {
      if (cancelled) return;
      const u = evt.payload;
      const line = runLine(u);
      if (!line) return;
      setOutputs((prev) => {
        const arr = [...(prev[u.card_id] ?? []), line];
        return { ...prev, [u.card_id]: arr.slice(-40) };
      });
    }).then((u) => {
      if (cancelled) u();
      else unlistens.push(u);
    });

    return () => {
      cancelled = true;
      unlistens.forEach((u) => u());
    };
  }, [refresh]);

  const create = async () => {
    try {
      await invoke("create_card", { title });
      setTitle("");
    } catch (e) {
      setError(String(e));
    }
  };

  const move = async (cardId: string, to: Status) => {
    setError(null);
    try {
      await invoke("move_card", { cardId, to });
    } catch (e) {
      setError(String(e));
    }
  };

  const override = async (cardId: string) => {
    const reason = prompt(`Reason for overriding ${cardId}:`);
    if (!reason) return;
    const to = prompt(`Target column (${COLUMNS.join(", ")}):`);
    if (!to || !COLUMNS.includes(to as Status)) return;
    setError(null);
    try {
      await invoke("override_card", { cardId, to, reason });
    } catch (e) {
      setError(String(e));
    }
  };

  const run = async (cardId: string) => {
    const task = prompt(`Task for the agent on ${cardId}:`);
    if (!task) return;
    setError(null);
    try {
      await invoke("start_run", { cardId, prompt: task });
    } catch (e) {
      setError(String(e));
    }
  };

  const stop = async (cardId: string) => {
    setError(null);
    try {
      await invoke("cancel_run", { cardId });
    } catch (e) {
      setError(String(e));
    }
  };

  const loginTerminal = async () => {
    try {
      await invoke("open_claude_terminal");
    } catch (e) {
      setError(String(e));
    }
  };

  const agentTerminal = async (cardId: string) => {
    try {
      await invoke("open_agent_terminal", { cardId });
    } catch (e) {
      setError(String(e));
    }
  };

  const approve = async (cardId: string) => {
    setError(null);
    try {
      await invoke("approve_card", { cardId });
    } catch (e) {
      setError(String(e));
    }
  };

  const reject = async (cardId: string) => {
    const reason = prompt("Why is this work rejected?");
    if (!reason) return;
    setError(null);
    try {
      await invoke("reject_card", { cardId, reason });
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    refreshStatus();
    const t = setInterval(refreshStatus, 15000);
    return () => clearInterval(t);
  }, [refreshStatus]);

  return (
    <div className="board">
      <header>
        <div className="statusbar">
          <span
            className={`dot ${
              status == null
                ? ""
                : status.cli_found && status.logged_in
                  ? "ok"
                  : "bad"
            }`}
          />
          <span className="statustext">
            {status == null
              ? "checking claude..."
              : !status.cli_found
                ? "claude CLI not found in PATH"
                : status.logged_in
                  ? "claude ready"
                  : "claude not logged in"}
          </span>
          {status != null && (!status.logged_in || !status.cli_found) && (
            <>
              <button onClick={loginTerminal}>open claude terminal (/login)</button>
              <button onClick={refreshStatus}>recheck</button>
            </>
          )}
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            create();
          }}
        >
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="New card title"
          />
          <button type="submit">Add card</button>
        </form>
        {error && <div className="error">{error}</div>}
      </header>
      <main>
        {COLUMNS.map((col) => (
          <section key={col} className="column">
            <h2>{col}</h2>
            {cards
              .filter((c) => c.status === col)
              .map((c) => (
                <article key={c.id} className="card">
                  <p>{c.title}</p>
                  {col === "ready" && (
                    <button onClick={() => run(c.id)}>run agent</button>
                  )}
                  {col === "running" && (
                    <>
                      <button onClick={() => stop(c.id)}>stop</button>
                      <button onClick={() => agentTerminal(c.id)}>terminal</button>
                      <pre className="output">
                        {(outputs[c.id] ?? []).join("\n")}
                      </pre>
                    </>
                  )}
                  {col === "review" && (
                    <div className="review-actions">
                      <button className="approve" onClick={() => approve(c.id)}>
                        approve
                      </button>
                      <button className="override" onClick={() => reject(c.id)}>
                        reject…
                      </button>
                    </div>
                  )}
                  {col !== "running" &&
                    sessions.some((s) => s.card_id === c.id) && (
                      <button onClick={() => agentTerminal(c.id)}>agent terminal</button>
                    )}
                  <div className="actions">
                    {COLUMNS.filter((t) => t !== col).map((t) => (
                      <button key={t} onClick={() => move(c.id, t)}>
                        → {t}
                      </button>
                    ))}
                    <button className="override" onClick={() => override(c.id)}>
                      override…
                    </button>
                  </div>
                </article>
              ))}
          </section>
        ))}
      </main>
    </div>
  );
}
