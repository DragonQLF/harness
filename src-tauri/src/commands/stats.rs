//! Run history for the Home screen: the heatmap, the three window tiles, the
//! per-actor spend and the day's line counts.
//!
//! Everything but the line counts is decided in `harness_app::runstats` over
//! the project's event log, which is where the tests are. This file reads the
//! log, asks git for the one number the log does not carry, and hands both
//! back in a single round trip — the screen opens once, not four times.

use std::sync::Arc;

use harness_app::runstats::{self, ActorFilter, RunStats};
use tauri::State;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Finished runs across the last 38 weeks, filtered to whoever the operator
/// asked about.
///
/// The log is read whole rather than through an index. At the size a project
/// reaches it is a few milliseconds, `project_stats` and `activity` already do
/// the same read for the same screen, and an index would be a second copy of
/// the truth that can drift from the first.
#[tauri::command]
pub async fn run_stats(
    project_id: String,
    actor: Option<ActorFilter>,
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<RunStats, String> {
    let runtime = ws.runtime(&project_id).await?;
    let cards = runtime.engine.snapshot().await?.cards;
    let store = Arc::clone(&runtime.store);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    let tz = tz_offset_minutes.unwrap_or(0);
    let now = now_ms();
    let mut stats = runstats::run_stats(&history, &cards, now, tz, actor.unwrap_or_default());

    // The log counts runs and money; only git knows what the day wrote. Asked
    // from the operator's own midnight, so the lines and the runs beside them
    // are for the same day.
    let git = Arc::clone(&runtime.git);
    // Never a bare zero: git reads `@0` as a date it could not parse and falls
    // back to now, which would silently report the day as empty.
    let midnight = (runstats::local_midnight_ms(now, tz) / 1000).max(1);
    let since = format!("@{midnight}");
    let (added, removed) = tauri::async_runtime::spawn_blocking(move || git.lines_since(&since))
        .await
        .map_err(|e| e.to_string())?;
    stats.lines_added_today = added;
    stats.lines_removed_today = removed;
    Ok(stats)
}
