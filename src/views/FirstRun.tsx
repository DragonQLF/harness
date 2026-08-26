/** What Relay shows before it has a project: the two ways in, and the
 *  Director, who can be asked what to do first. Never a dead end. */

import { useState } from "react";
import { greeting } from "../lib/format";
import { useStore } from "../state/store";
import { tabular, truncate } from "../components/ui";

function Step({
  n,
  title,
  body,
}: {
  n: string;
  title: string;
  body: string;
}) {
  return (
    <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>
      <span
        style={{
          width: 22,
          height: 22,
          flex: "none",
          borderRadius: "50%",
          background: "var(--surface2)",
          border: "1px solid var(--line)",
          color: "var(--text3)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 11,
          fontWeight: 800,
          ...tabular,
        }}
      >
        {n}
      </span>
      <span style={{ minWidth: 0 }}>
        <span style={{ display: "block", fontSize: 12.5, fontWeight: 700 }}>{title}</span>
        <span
          style={{
            display: "block",
            marginTop: 3,
            fontSize: 11.5,
            color: "var(--text3)",
            lineHeight: 1.55,
          }}
        >
          {body}
        </span>
      </span>
    </div>
  );
}

export function FirstRun({ openChat }: { openChat: () => void }) {
  const { settings, addProject, createProject, status, dataDir, installSidecar } = useStore();
  const [name, setName] = useState("");
  const firstName = (settings?.user_name ?? "Operator").split(/\s+/)[0];

  return (
    <div style={{ padding: "22px 26px 28px", maxWidth: 1000 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16 }}>
        <h1 style={{ margin: 0, fontSize: 20, fontWeight: 800, letterSpacing: "-.02em" }}>
          Overview
        </h1>
        <span style={{ color: "var(--text3)", fontSize: 12 }}>›</span>
        <span style={{ fontSize: 12.5, color: "var(--text3)" }}>no project yet</span>
      </div>

      <div
        style={{
          position: "relative",
          borderRadius: 20,
          overflow: "hidden",
          background: "var(--ink)",
          boxShadow: "var(--lift)",
          animation: "fadeUp .5s ease both",
        }}
      >
        <div
          style={{
            position: "absolute",
            right: -60,
            top: -80,
            width: 240,
            height: 240,
            borderRadius: "50%",
            background: "radial-gradient(circle,rgba(139,125,255,.3),transparent 68%)",
            pointerEvents: "none",
          }}
        />
        <div style={{ position: "relative", padding: "26px 26px 24px" }}>
          <div style={{ fontSize: 21, fontWeight: 800, color: "#fff", letterSpacing: "-.02em" }}>
            {greeting()}, {firstName}. Nothing is set up yet.
          </div>
          <div
            style={{
              marginTop: 6,
              fontSize: 13,
              lineHeight: 1.55,
              color: "rgba(255,255,255,.62)",
              maxWidth: "62ch",
            }}
          >
            Relay works on git repositories — local ones. Point it at a repo you already have, or
            start a new one from scratch: no remote, no account, nothing leaves this machine unless
            an agent asks you to push.
          </div>

          <div style={{ display: "flex", gap: 8, marginTop: 18, flexWrap: "wrap" }}>
            <button
              type="button"
              className="hv-rise"
              onClick={addProject}
              style={{
                padding: "10px 18px",
                border: "none",
                borderRadius: 999,
                background: "#fff",
                color: "#17171f",
                fontSize: 13,
                fontWeight: 700,
                cursor: "pointer",
                transition: "transform .18s ease",
              }}
            >
              Open a repository…
            </button>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "4px 4px 4px 14px",
                border: "1px solid rgba(255,255,255,.2)",
                borderRadius: 999,
                background: "rgba(255,255,255,.07)",
              }}
            >
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && name.trim()) createProject(name);
                }}
                placeholder="or name a new local repo…"
                style={{
                  width: 168,
                  border: "none",
                  background: "transparent",
                  color: "#fff",
                  fontSize: 12.5,
                  outline: "none",
                  padding: "6px 0",
                }}
              />
              <button
                type="button"
                className="hv-white"
                onClick={() => createProject(name)}
                disabled={!name.trim()}
                style={{
                  padding: "7px 14px",
                  border: "none",
                  borderRadius: 999,
                  background: name.trim() ? "var(--accent2)" : "rgba(255,255,255,.12)",
                  color: name.trim() ? "#17171f" : "rgba(255,255,255,.5)",
                  fontSize: 12.5,
                  fontWeight: 700,
                  cursor: name.trim() ? "pointer" : "not-allowed",
                }}
              >
                Create
              </button>
            </div>
            <button
              type="button"
              className="hv-white"
              onClick={openChat}
              style={{
                padding: "10px 16px",
                border: "1px solid rgba(255,255,255,.2)",
                borderRadius: 999,
                background: "rgba(255,255,255,.07)",
                color: "#fff",
                fontSize: 12.5,
                fontWeight: 600,
                cursor: "pointer",
                transition: "background .18s ease",
              }}
            >
              Ask the Director what to start
            </button>
          </div>
        </div>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)",
          gap: 12,
          marginTop: 12,
          alignItems: "start",
        }}
      >
        <div
          style={{
            border: "1px solid var(--line)",
            borderRadius: 18,
            background: "var(--surface)",
            padding: "17px 18px",
            display: "flex",
            flexDirection: "column",
            gap: 14,
          }}
        >
          <div style={{ fontSize: 14, fontWeight: 700 }}>How it goes from here</div>
          <Step
            n="1"
            title="Pick a repository"
            body="A local repository is enough — no remote required. Relay never writes inside it directly: each card gets its own worktree under app data, on a harness/<card> branch."
          />
          <Step
            n="2"
            title="Say what should happen"
            body="One line on Home becomes a card. Plan leaves it ready; Start hands it to the Builder straight away."
          />
          <Step
            n="3"
            title="The Director reads the diff"
            body="When a run finishes it reviews the work and either approves it or sends it back with a reason. Only what passes reaches you."
          />
          <Step
            n="4"
            title="You stay in charge"
            body="Anything outside an agent's permissions stops the run and asks. Every decision is in the event log."
          />
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div
            style={{
              border: "1px solid var(--line)",
              borderRadius: 18,
              background: "var(--surface)",
              padding: "17px 18px",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 12 }}>Before you start</div>
            {[
              {
                label: "Claude",
                ok: status?.claude.logged_in ?? false,
                good: status?.claude.cli_version
                  ? `logged in · claude ${status.claude.cli_version}`
                  : "logged in",
                bad: "not logged in — open a terminal and run /login",
              },
              {
                label: "Agent sidecar",
                ok: status?.sidecar.ready ?? false,
                good: `ready · node ${status?.sidecar.node_version ?? ""}`.trim(),
                bad: status?.sidecar.node_found
                  ? "dependencies not installed yet"
                  : "node was not found on PATH",
              },
              {
                label: "git",
                ok: true,
                good: "used through the command line · local only, no remote",
                bad: "",
              },
            ].map((row, i) => (
              <div
                key={row.label}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  padding: "10px 0",
                  borderTop: i === 0 ? "none" : "1px solid var(--line2)",
                }}
              >
                <span
                  style={{
                    width: 7,
                    height: 7,
                    flex: "none",
                    borderRadius: "50%",
                    background: row.ok ? "var(--ok)" : "var(--warn)",
                  }}
                />
                <span style={{ flex: "none", minWidth: 96, fontSize: 12.5, fontWeight: 600 }}>
                  {row.label}
                </span>
                <span style={{ flex: 1, minWidth: 0, fontSize: 11.5, color: "var(--text3)", ...truncate }}>
                  {row.ok ? row.good : row.bad}
                </span>
                {!row.ok && row.label === "Agent sidecar" && status?.sidecar.node_found && (
                  <button
                    type="button"
                    className="hv-bright"
                    onClick={installSidecar}
                    style={{
                      padding: "5px 12px",
                      border: "none",
                      borderRadius: 999,
                      background: "var(--accent)",
                      color: "var(--onAccent)",
                      fontSize: 11.5,
                      fontWeight: 700,
                      cursor: "pointer",
                    }}
                  >
                    Install
                  </button>
                )}
              </div>
            ))}
          </div>

          <div
            style={{
              border: "1px solid var(--line)",
              borderRadius: 18,
              background: "var(--surface)",
              padding: "17px 18px",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 700, marginBottom: 6 }}>
              Where Relay keeps things
            </div>
            <div
              title={dataDir}
              style={{
                fontSize: 11.5,
                color: "var(--text3)",
                fontFamily: "var(--mono)",
                ...truncate,
              }}
            >
              {dataDir || "—"}
            </div>
            <div style={{ marginTop: 8, fontSize: 11.5, color: "var(--text3)", lineHeight: 1.6 }}>
              Event logs, run transcripts, agent profiles and worktrees live there. Your repository
              only ever receives commits.
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
