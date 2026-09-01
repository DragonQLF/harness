//! Prova o caminho inteiro da ferramenta de imagem: um agente **Claude** vê o
//! `generate_image`, chama-o, o pedido chega ao runner do Relay em Rust, e o
//! caminho volta para dentro da resposta.
//!
//! O que corre a geração é o mesmo `harness_agent_codex::generate_image` que o
//! `director_tools::images` corre — ou seja, isto prova a canalização *e* o
//! trabalho, sem precisar de levantar uma janela.
//!
//! `cargo run -p relay --example image_tool_e2e`
use std::path::PathBuf;
use std::sync::Arc;

use harness_agent_sidecar::SidecarAgent;
use harness_ports::{AgentPort, RunEvent, RunSpec, ToolReply};
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar/index.mjs");
    let cwd = std::env::temp_dir();
    let home = std::env::temp_dir().join("relay-codex-probe");

    let mut spec = RunSpec::new(
        "Use the generate_image tool to make a flat vector icon of a single orange traffic \
         cone on a white background. Then reply with only the file path it gave you.",
        cwd.clone(),
    );
    spec.model = Some("haiku".to_string());
    spec.permission_mode = Some("acceptEdits".to_string());
    // A ferramenta não está nos `allowed_tools`, portanto passa pelo
    // `canUseTool` — que é o caminho a sério. Sem aprovador seria negada.
    spec.approver = Some(Arc::new(|req| {
        Box::pin(async move {
            println!("  [approval] {} — {}", req.tool, req.summary);
            harness_ports::ApprovalOutcome::Allowed
        })
    }));

    // O runner do Relay, com a mesma implementação que o `director_tools` usa.
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let calls = Arc::clone(&seen);
    spec.tools = Some(Arc::new(move |call: harness_ports::ToolCall| {
        let calls = Arc::clone(&calls);
        let home = home.clone();
        let cwd = cwd.clone();
        Box::pin(async move {
            calls.lock().unwrap().push(call.name.clone());
            println!("  [tool_request reached Rust] {}", call.name);
            if call.name != "generate_image" {
                return ToolReply::refused(format!("not this test's tool: {}", call.name));
            }
            let prompt = call
                .input
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or_default()
                .to_string();
            match harness_agent_codex::generate_image("codex", Some(&home), &prompt, &cwd).await {
                Ok(path) => ToolReply::ok(format!("Saved to {path}.")),
                Err(e) => ToolReply::refused(e),
            }
        })
    }));

    let agent = SidecarAgent::new("node", script);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let pump = tokio::spawn(async move {
        let mut said = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                RunEvent::Text { text } => said.push_str(&text),
                RunEvent::ToolUse { tool, .. } => println!("  [tool] {tool}"),
                _ => {}
            }
        }
        said
    });
    let outcome = agent.run(spec, tx, CancellationToken::new()).await;
    let said = pump.await.unwrap();

    println!("\noutcome: {outcome:?}");
    println!("answer : {}", said.trim());
    let called = seen.lock().unwrap().iter().any(|n| n == "generate_image");
    let path = said.split_whitespace().find(|w| w.ends_with(".png"));
    let real = path.map(|p| std::path::Path::new(p.trim_matches(|c| c == '`' || c == '.')).is_file());
    println!("=> tool reached Rust: {called}");
    println!("=> path in the answer: {path:?}");
    println!("=> file exists on disk: {real:?}");
}
