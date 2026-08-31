//! Exercita os caminhos que os testes de unidade não alcançam: retomar uma
//! thread, cancelar um turno a meio, e uma aprovação a chegar como pedido.
//!
//! `cargo run -p harness-agent-codex --example verify`
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use harness_ports::{AgentPort, RunEvent, RunOutcome, RunSpec};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn agent() -> harness_agent_codex::CodexAgent {
    harness_agent_codex::CodexAgent::new("codex")
        .with_home(&std::env::temp_dir().join("relay-codex-probe"))
}

/// Corre um spec e devolve (outcome, texto junto, aprovações vistas).
async fn run(spec: RunSpec, cancel_after_ms: Option<u64>) -> (RunOutcome, String, usize) {
    let (tx, mut rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    if let Some(ms) = cancel_after_ms {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            token.cancel();
        });
    }
    let approvals = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&approvals);
    let pump = tokio::spawn(async move {
        let mut text = String::new();
        while let Some(ev) = rx.recv().await {
            match ev {
                RunEvent::Text { text: t } => text.push_str(&t),
                RunEvent::ApprovalRequested { tool, summary, .. } => {
                    seen.fetch_add(1, Ordering::Relaxed);
                    println!("    [approval] {tool} — {}", summary.chars().take(90).collect::<String>());
                }
                RunEvent::ToolUse { tool, .. } => println!("    [tool] {tool}"),
                _ => {}
            }
        }
        text
    });
    let outcome = agent().run(spec, tx, cancel).await.unwrap();
    let text = pump.await.unwrap();
    (outcome, text, approvals.load(Ordering::Relaxed))
}

fn spec(prompt: &str) -> RunSpec {
    let mut s = RunSpec::new(prompt, std::env::temp_dir());
    s.permission_mode = Some("acceptEdits".into());
    s
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // ---- 1. resume ----
    println!("1. RESUME");
    let (out, _, _) = run(spec("Remember this word: BANANA. Reply with just: stored"), None).await;
    let session = match &out {
        RunOutcome::Completed { session_id, .. } => session_id.clone(),
        other => panic!("first turn did not complete: {other:?}"),
    };
    println!("   thread: {session:?}");
    let mut second = spec("What word did I ask you to remember? Reply with only that word.");
    second.resume_session = session.clone();
    let (out2, text, _) = run(second, None).await;
    let remembered = text.to_uppercase().contains("BANANA");
    println!("   answer: {:?}", text.trim());
    println!("   => resume {}", if remembered { "WORKS" } else { "FAILED — no memory" });
    println!("   => same thread: {}", matches!(&out2, RunOutcome::Completed { session_id, .. } if *session_id == session));

    // ---- 2. cancel ----
    println!("\n2. CANCEL");
    let (out3, _, _) = run(
        spec("Run `sleep 60` in the shell, then say done."),
        Some(6_000),
    )
    .await;
    println!("   => {:?}", out3);
    println!("   => cancel {}", if matches!(out3, RunOutcome::Cancelled) { "WORKS" } else { "FAILED" });

    // ---- 3. approval ----
    // A rede está fechada na sandbox do Codex, portanto pedir um `curl` é a
    // maneira barata de o obrigar a escalar — e é o escalar que tem de chegar
    // aqui como pedido em vez de morrer no ecrã dele.
    println!("\n3. APPROVAL (denied on purpose)");
    let mut ask = spec("Run exactly: curl -sS https://example.com -o /dev/null && echo fetched");
    ask.approver = Some(Arc::new(|req| {
        Box::pin(async move {
            println!("    [answered: deny] {}", req.tool);
            false
        })
    }));
    let (out4, _, seen) = run(ask, None).await;
    println!("   => outcome {:?}", out4);
    println!("   => approvals reaching Relay: {seen}");
    println!("   => approvals {}", if seen > 0 { "WORK" } else { "NOT OBSERVED" });

    // ---- 4. approval, accepted ----
    // Negar é metade da prova: falta que um "sim" deixe mesmo passar o que
    // estava barrado. É a rede fechada da sandbox que se está a abrir aqui.
    println!("\n4. APPROVAL (accepted)");
    let mut yes = spec("Run exactly: curl -sS https://example.com -o /dev/null && echo fetched");
    yes.approver = Some(Arc::new(|req| {
        Box::pin(async move {
            println!("    [answered: allow] {}", req.tool);
            true
        })
    }));
    let (out5, text5, seen5) = run(yes, None).await;
    println!("   => outcome {:?}", out5);
    println!("   => approvals: {seen5}, answer: {:?}", text5.chars().take(160).collect::<String>());
    println!(
        "   => accept {}",
        if text5.to_lowercase().contains("fetched") { "LETS IT THROUGH" } else { "did not observably run" }
    );

    // ---- 5. steer: falar com um turno que já anda ----
    // O `turn/steer` só é aceite contra o turno vivo (`expectedTurnId`), por
    // isso o que isto prova não é só que a mensagem chega — é que chega ao
    // turno certo, a meio dele, e que o modelo lhe obedece.
    println!("\n5. STEER (a message typed mid-turn)");
    let inbox = harness_ports::queue::Queue::new("verify-steer");
    let mut steered = spec(
        "Count from 1 to 40, one number per line, sleeping 1 second between each using the \
         shell. Do not stop early unless you are told to.",
    );
    steered.inbox = Some(inbox.clone());
    let pushed = Arc::clone(&inbox);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(12)).await;
        let _ = pushed.push("Stop counting right now and reply with only the word PINEAPPLE.");
        println!("    [typed mid-turn] stop and say PINEAPPLE");
    });
    let (out6, text6, _) = run(steered, None).await;
    println!("   => outcome {:?}", out6);
    println!("   => answer: {:?}", text6.chars().take(200).collect::<String>());
    println!(
        "   => steer {}",
        if text6.to_uppercase().contains("PINEAPPLE") { "REACHED THE LIVE TURN" } else { "NOT OBSERVED" }
    );
}
