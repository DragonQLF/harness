/** The launch window's whole behaviour.
 *
 *  Design: `docs/design/Relay Lifecycle.dc.html`, "03 · LAUNCH". Two rules from
 *  the handoff shape everything here:
 *
 *  - *skipped entirely if ready lands under 400ms* — "a splash that outlives
 *    its purpose reads as slowness, not polish". So the window is created
 *    hidden and only shows itself once 400ms have passed with the shell still
 *    unpainted;
 *  - the status line reports **real** phases. It says what the shell has
 *    actually told it and nothing else: a phase with no event shows nothing
 *    rather than a fake step.
 *
 *  Nothing in here imports the app's store, its IPC module or its types. This
 *  window exists precisely for the window in time where none of that is up. */

import "./splash.css";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow, Window } from "@tauri-apps/api/window";

/** What the shell reports. `note` is null when it has nothing true to say. */
type Phase = { note: string | null; progress: number; done?: boolean };

/** Under this, the launch was quick enough that a splash would only be in the
 *  way. The window is never shown and never seen. */
const SHOW_AFTER_MS = 400;

/** The exit crossfade, matched to the CSS transition on `.card`. */
const FADE_MS = 160;

/** A shell that never reports anything is a shell that has gone wrong. Rather
 *  than sit in front of an app that may well be fine, hand over anyway — a
 *  half-drawn window can at least be read and closed. */
const HAND_OVER_AFTER_MS = 12_000;

const splash = getCurrentWindow();
const statusEl = document.getElementById("status");
const fillEl = document.getElementById("fill");

let shown = false;
let finished = false;

function show() {
  if (shown || finished) return;
  shown = true;
  document.body.classList.add("live");
  splash.show().catch(() => {});
}

function render(phase: Phase) {
  if (statusEl) statusEl.textContent = phase.note ?? "";
  if (fillEl) fillEl.style.width = `${Math.round(Math.min(1, Math.max(0, phase.progress)) * 100)}%`;
}

/** The handover. The main window comes up as the splash goes down, over the
 *  same 160ms, so there is never a frame with neither of them on screen. */
async function handOver() {
  if (finished) return;
  finished = true;
  try {
    const main = await Window.getByLabel("main");
    await main?.show();
    await main?.setFocus();
  } catch {
    /* If the main window cannot be raised there is nothing this one can do
       about it, and staying up would only hide the failure. */
  }
  document.body.classList.add("leaving");
  window.setTimeout(() => {
    splash.close().catch(() => {});
  }, FADE_MS);
}

listen<Phase>("splash://phase", (event) => {
  if (finished) return;
  // Rendered even while hidden: whatever the last phase was is already on the
  // face of the window by the time it is shown.
  render(event.payload);
  if (!event.payload.done) return;
  if (!shown) {
    // Ready inside 400ms. The window was never shown and never will be.
    finished = true;
    splash.close().catch(() => {});
    Window.getByLabel("main")
      .then((main) => main?.show().then(() => main?.setFocus()))
      .catch(() => {});
    return;
  }
  handOver();
})
  // The shell may well have started reporting before this window was
  // listening. One hello once the listener is real, and it repeats whatever
  // phase it is on.
  .then(() => emit("splash://listening"))
  .catch(() => {});

window.setTimeout(show, SHOW_AFTER_MS);
window.setTimeout(handOver, HAND_OVER_AFTER_MS);
