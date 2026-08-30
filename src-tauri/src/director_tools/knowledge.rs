//! O que o Relay sabe sobre si próprio, e o que uma conversa deixa escrito.
//!
//! Nenhuma destas toca num quadro: contam o nosso próprio histórico, lêem os
//! nossos próprios documentos, ou escrevem na nossa própria caixa. É por isso
//! que passam sem delegação (#76).

use std::sync::Arc;

use harness_ports::{ClockPort, ToolCall, ToolReply};

use super::text;
use crate::workspace::{SystemClock, Workspace};

/// UTC date as YYYY-MM-DD from a millisecond stamp (Howard Hinnant's
/// civil_from_days). No chrono dependency for one filename.
fn utc_date_string(now_ms: u64) -> String {
    let days = (now_ms / 1000 / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}


pub(super) fn self_report(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let days = call
            .input
            .get("days")
            .and_then(|v| v.as_u64())
            .map(|d| d.clamp(1, 30) as u32)
            .unwrap_or(7);
        let report = ws.collect_self_report(days);
        ToolReply::ok(harness_app::selfreport::render(&report))
}

pub(super) async fn read_docs(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(docs) = ws.harness_docs_dir().await else {
            return ToolReply::refused(
                "the harness repository is not registered as a project here, so DEBT.md and \
                 DECISIONS.md are out of reach — ask the operator to add it",
            );
        };
        let doc = match text(&call.input, "doc").as_deref().and_then(harness_app::devdocs::Doc::parse) {
            Some(d) => d,
            None => {
                return ToolReply::refused("read_docs needs doc as \"debt\" or \"decisions\"");
            }
        };
        match harness_app::devdocs::render(&docs, doc, text(&call.input, "find").as_deref())
        {
            Ok(rendered) => ToolReply::ok(rendered),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) fn propose_improvement(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let title = text(&call.input, "title").unwrap_or_default();
        let observation = text(&call.input, "observation").unwrap_or_default();
        let suggestion = text(&call.input, "proposal").unwrap_or_default();
        if title.is_empty() || observation.is_empty() || suggestion.is_empty() {
            return ToolReply::refused(
                "propose_improvement needs title, observation (what you saw — one occurrence is \
                 enough) and proposal (the correction)",
            );
        }
        match ws.propose_improvement(&title, &observation, &suggestion) {
            Ok(_) => ToolReply::ok(
                "filed in the operator's inbox — they decide whether it becomes work; announce \
                 that you proposed it",
            ),
            Err(e) => ToolReply::refused(e),
        }
}

/// Uma decisão tomada em conversa morre com a conversa a não ser que chegue ao
/// disco no momento em que acontece. Datada, append-only, na memória do próprio
/// projecto — fora de qualquer repositório (#59).
pub(super) fn record_decision(
    ws: &Arc<Workspace>,
    project_id: &str,
    call: &ToolCall,
) -> ToolReply {
        let title = text(&call.input, "title").unwrap_or_default();
        let content = text(&call.input, "content").unwrap_or_default();
        if title.trim().is_empty() || content.trim().is_empty() {
            return ToolReply::refused("record_decision needs a title and content");
        }
        let dir = ws
            .paths
            .project_dir(project_id)
            .join("memory")
            .join("decisions");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return ToolReply::refused(format!("could not create the memory folder: {e}"));
        }
        let now_ms = SystemClock.now_millis();
        let date = utc_date_string(now_ms);
        let slug: String = {
            let cleaned: String = title
                .trim()
                .to_lowercase()
                .chars()
                .take(40)
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            cleaned.trim_matches('-').to_string()
        };
        let mut n = 1;
        loop {
            let candidate = dir.join(format!("{date}-{slug}-{n:02}.md"));
            if !candidate.exists() {
                if let Err(e) =
                    std::fs::write(&candidate, format!("# {title}\n\n{content}\n"))
                {
                    return ToolReply::refused(format!(
                        "could not write the decision: {e}"
                    ));
                }
                return ToolReply::ok(format!(
                    "recorded as {} - announce that you wrote it",
                    candidate.display()
                ));
            }
            n += 1;
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_of_a_decision_is_utc_and_not_the_machine_clock() {
        assert_eq!(utc_date_string(0), "1970-01-01");
        assert_eq!(utc_date_string(1_756_512_000_000), "2025-08-30");
    }
}
