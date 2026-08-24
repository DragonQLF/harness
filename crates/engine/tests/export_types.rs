//! Regenerates the TypeScript mirrors into the frontend tree. See the same
//! test in `crates/domain` — `pnpm codegen` runs them all.

use std::path::{Path, PathBuf};
use ts_rs::TS;

/// See `crates/domain/tests/export_types.rs` for why this pass exists.
fn numbers_not_bigints(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ts") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let fixed = text.replace("bigint", "number");
        if fixed != text {
            std::fs::write(&path, fixed).unwrap();
        }
    }
}

#[test]
fn export_types() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/generated");
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("TS_RS_EXPORT_DIR", &dir);

    harness_engine::Snapshot::export().unwrap();
    harness_engine::SessionView::export().unwrap();
    harness_engine::ActiveRun::export().unwrap();

    numbers_not_bigints(&dir);
}
