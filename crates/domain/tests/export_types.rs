//! Regenerates the TypeScript mirrors of these types into the frontend tree.
//! Run via `pnpm codegen`, which is `cargo test --workspace --test export_types`.

use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Every integer we put on the wire is a millisecond stamp or a counter, far
/// under `Number.MAX_SAFE_INTEGER`, and serde writes it as a plain JSON
/// number — so the honest mirror is `number`, not ts-rs's default `bigint`.
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
    // Every export in this workspace writes to the same directory, so the
    // frontend imports them from one place.
    std::env::set_var("TS_RS_EXPORT_DIR", &dir);

    harness_domain::CardId::export().unwrap();
    harness_domain::RunId::export().unwrap();
    harness_domain::Status::export().unwrap();
    harness_domain::Actor::export().unwrap();
    harness_domain::RunOutcome::export().unwrap();
    harness_domain::HunkRef::export().unwrap();
    harness_domain::HunkVerdict::export().unwrap();
    harness_domain::Review::export().unwrap();
    harness_domain::Card::export().unwrap();

    numbers_not_bigints(&dir);
}
