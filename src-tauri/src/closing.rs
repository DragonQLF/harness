//! Closing the window, out loud.
//!
//! Relay holds the window on close for two reasons: running agents get to
//! leave a wip commit behind, and once a day the Director takes his end-of-day
//! look (#79). Both are deliberate. Neither used to say anything — the operator
//! pressed the close button and the window simply refused, which from the
//! outside is indistinguishable from a hung app.
//!
//! So the hold is now narrated and always escapable:
//!
//! - `closing://began` says what is being waited on before any of it starts;
//! - `closing://phase` names each step as it runs;
//! - the operator can end the wait at any moment (`close_now`, or pressing
//!   close a second time), and a hard ceiling ends it even if nobody does.
//!
//! The window is destroyed on every path out of here. A close that cannot be
//! escaped is a bug no matter how good the reason for waiting was — and
//! nothing in the wait is lost by skipping it: proposals are written when the
//! tool runs, not at the end, and a look that did not finish is due again
//! rather than marked done.

use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt;
use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::reflection;
use crate::workspace::Workspace;

/// The wait ends here whatever happens. Longer than the end-of-day look's own
/// budget (120s) plus room for the engines to stop, and short enough that a
/// wedged shutdown still lets go of the window while the operator is watching.
const HARD_LIMIT: Duration = Duration::from_secs(180);

/// After the operator stops waiting, the step in flight gets this long to
/// finish the write it was in the middle of — a transcript line, the mark of
/// the look. Skipping should cost the wait, not the record of it.
const GRACE: Duration = Duration::from_secs(5);

/// What the window is being held for, sent before the waiting starts so the
/// overlay can name it instead of showing a bare spinner.
#[derive(Debug, Clone, Serialize)]
pub struct ClosingBegan {
    /// The Director's end-of-day look is due.
    pub look: bool,
    /// Running agents are being asked to commit their work in progress.
    pub wip: bool,
    /// Seconds after which the window closes regardless.
    pub limit_secs: u64,
}

/// Where the close got to. `phase` is for the UI to switch on; `detail` is the
/// sentence shown to the operator.
#[derive(Debug, Clone, Serialize)]
pub struct ClosingPhase {
    pub phase: &'static str,
    pub detail: String,
}

fn say(app: &tauri::AppHandle, phase: &'static str, detail: impl Into<String>) {
    let _ = app.emit(
        crate::events::CLOSING_PHASE,
        ClosingPhase {
            phase,
            detail: detail.into(),
        },
    );
}

/// Run the close sequence and destroy the window. Safe to call once per close;
/// `Workspace::begin_closing` decides who gets to be that one.
///
/// The destroy is the caller's job, not the sequence's, so a panic anywhere
/// inside still lets go of the window. A close button that stops working
/// because a shutdown step failed is the exact bug this module exists to fix.
pub async fn run(window: tauri::Window, ws: Arc<Workspace>) {
    let guard = window.clone();
    if std::panic::AssertUnwindSafe(sequence(window, ws))
        .catch_unwind()
        .await
        .is_err()
    {
        eprintln!("the close sequence panicked; closing anyway");
    }
    let _ = guard.destroy();
}

async fn sequence(window: tauri::Window, ws: Arc<Workspace>) {
    let app = window.app_handle().clone();
    let skip = ws.closing_token();
    let look = ws.daily_look_due();
    let wip = ws.settings().commit_wip_on_close;

    let _ = app.emit(
        crate::events::CLOSING_BEGAN,
        ClosingBegan {
            look,
            wip,
            limit_secs: HARD_LIMIT.as_secs(),
        },
    );

    let work = {
        let ws = Arc::clone(&ws);
        let app = app.clone();
        let skip = skip.clone();
        async move {
            if look {
                say(
                    &app,
                    "look",
                    "The Director is taking his end-of-day look at the week — what he \
                     notices becomes proposals in your inbox, never cards.",
                );
                reflection::maybe_run_daily_look(&ws, skip).await;
            }
            say(
                &app,
                "wip",
                if wip {
                    "Asking the agents still working to commit what they have, so no \
                     work is stranded in a worktree."
                } else {
                    "Stopping the project engines."
                },
            );
            ws.shutdown().await;
        }
    };

    tokio::pin!(work);
    tokio::select! {
        _ = &mut work => {}
        _ = skip.cancelled() => {
            say(&app, "skipped", "Closing now. Anything already filed is saved.");
            // The step in flight is already unwinding. Give its last writes a
            // moment to reach disk rather than killing the process mid-append —
            // bounded, because a skip that waits is not a skip.
            let _ = tokio::time::timeout(GRACE, &mut work).await;
        }
        _ = tokio::time::sleep(HARD_LIMIT) => {
            say(
                &app,
                "timeout",
                "Shutdown took longer than expected; closing anyway.",
            );
        }
    }

    say(&app, "done", "Goodbye.");
}
