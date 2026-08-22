import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";

type Status = "backlog" | "ready" | "running" | "review" | "done";

interface Card {
  id: string;
  title: string;
  status: Status;
}

interface Snapshot {
  last_seq: number;
  cards: Card[];
}

interface Envelope {
  seq: number;
  type: string;
  card_id?: string;
  title?: string;
  to?: Status;
}

const COLUMNS: Status[] = ["backlog", "ready", "running", "review", "done"];

function applyEnvelope(cards: Card[], env: Envelope): Card[] {
  switch (env.type) {
    case "card_created":
      return [
        ...cards,
        { id: env.card_id!, title: env.title ?? "", status: "backlog" },
      ];
    case "card_moved":
    case "card_overridden":
      return cards.map((c) =>
        c.id === env.card_id && env.to ? { ...c, status: env.to } : c,
      );
    default:
      return cards;
  }
}

export default function App() {
  const [cards, setCards] = useState<Card[]>([]);
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const seqRef = useRef(-1);

  const refresh = useCallback(async () => {
    try {
      const snap = await invoke<Snapshot>("snapshot");
      setCards(snap.cards);
      seqRef.current = snap.last_seq;
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;

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
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });

    return () => {
      cancelled = true;
      unlisten?.();
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

  return (
    <div className="board">
      <header>
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
