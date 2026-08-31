//! Proves the image tool end to end: `cargo run -p harness-agent-codex --example image -- "..."`
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "a flat vector icon of a green leaf on white".to_string());
    let home = std::env::temp_dir().join("relay-codex-probe");
    let home = harness_agent_codex::prepare_home(&home);
    let cwd = std::env::temp_dir();
    match harness_agent_codex::generate_image("codex", home.as_deref(), &prompt, &cwd).await {
        Ok(path) => println!("SAVED: {path}"),
        Err(e) => println!("FAILED: {e}"),
    }
}
