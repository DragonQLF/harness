//! Regenerates the TypeScript mirrors into the frontend tree. See the same
//! test in `crates/domain` — `pnpm codegen` runs them all.

use std::path::PathBuf;
use ts_rs::TS;

#[test]
fn export_types() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/generated");
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("TS_RS_EXPORT_DIR", &dir);

    harness_app::agents::AgentProfile::export().unwrap();
    harness_app::allow::AllowRule::export().unwrap();
    harness_app::settings::Settings::export().unwrap();
    harness_app::projects::Project::export().unwrap();
    harness_app::projects::FolderInfo::export().unwrap();
    harness_app::conversations::Conversation::export().unwrap();
    harness_app::approvals::PendingApproval::export().unwrap();
    harness_app::insights::ActivityRow::export().unwrap();
    harness_app::insights::ProjectStats::export().unwrap();
    harness_app::insights::AgentStats::export().unwrap();
    harness_app::checks::CheckRow::export().unwrap();
}
