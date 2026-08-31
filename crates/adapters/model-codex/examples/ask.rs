//! `cargo run -p harness-agent-codex --example ask` — the two one-shot questions.
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let home = harness_agent_codex::prepare_home(&std::env::temp_dir().join("relay-codex-probe"));
    match harness_agent_codex::models("codex", home.as_deref()).await {
        Ok(list) => {
            println!("MODELS ({}):", list.len());
            for m in &list {
                println!("  {} | {} | default effort {}", m.id, m.name, m.default_effort);
            }
        }
        Err(e) => println!("MODELS FAILED: {e}"),
    }
    match harness_agent_codex::plan_usage("codex", home.as_deref()).await {
        Ok(u) => println!("PLAN: {u:?}"),
        Err(e) => println!("PLAN FAILED: {e}"),
    }
}
