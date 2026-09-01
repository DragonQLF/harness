//! Drives the Codex adapter against the real binary and prints the stream.
//!
//! `cargo run -p harness-agent-codex --example probe -- "your prompt"`
use std::sync::Arc;

use harness_ports::{AgentPort, RunSpec};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Say PROBE_OK and nothing else.".to_string());
    let cwd = std::env::current_dir().unwrap();
    let mut spec = RunSpec::new(prompt, cwd);
    spec.permission_mode = Some("acceptEdits".into());
    spec.approver = Some(Arc::new(|req| {
        Box::pin(async move {
            println!("[approval] {} — {} => allow", req.tool, req.summary);
            harness_ports::ApprovalOutcome::Allowed
        })
    }));

    let (tx, mut rx) = mpsc::channel(64);
    let home = std::env::temp_dir().join("relay-codex-probe");
    let agent = harness_agent_codex::CodexAgent::new("codex").with_home(&home);
    let cancel = CancellationToken::new();
    let run = tokio::spawn(async move { agent.run(spec, tx, cancel).await });

    while let Some(ev) = rx.recv().await {
        match ev {
            harness_ports::RunEvent::Delta { text } => print!("{text}"),
            harness_ports::RunEvent::Thinking { .. } => print!("."),
            other => println!("\n[{other:?}]"),
        }
    }
    println!("\noutcome: {:?}", run.await.unwrap());
}
