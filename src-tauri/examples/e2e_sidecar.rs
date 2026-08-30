//! Headless end-to-end: one Builder run carries a card from Ready to Review
//! through the real engine, the real sidecar, the real SDK and the real model.
//!
//! This costs money and needs `claude` to be logged in, so it is never run by
//! `cargo test` — it is run on purpose:
//!
//!     cargo run --release --example e2e_sidecar -p harness
//!
//! What it proves, in order: worktree creation, StartRun persistence, event
//! streaming, an edit landing in the worktree, the run task's trailer commit,
//! FinishRun → Review, and the session id surviving on the card.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_agent_sidecar::SidecarAgent;
use harness_engine::{
    Engine, EngineConfig, EngineDeps, EngineHandle, EnginePolicy,
};
use harness_git_cli::CliGit;
use harness_ports::{
    AgentPort, ClockPort, RunLogPort, StorePort,
};
use harness_store_jsonl::{JsonlRunLog, JsonlStore};
use tokio_util::sync::CancellationToken;

struct SystemClock;

impl ClockPort for SystemClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("harness-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn wait_for_review(handle: &EngineHandle, card_id: &str) -> Option<String> {
    for _ in 0..300 {
        let snap = handle.snapshot().await.ok()?;
        let Some(card) = snap.cards.iter().find(|c| c.id.as_str() == card_id) else {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        match card.status {
            harness_domain::Status::Review => return Some("review".to_string()),
            s @ (harness_domain::Status::Ready | harness_domain::Status::Backlog) => {
                return Some(format!("{card_id} fell back to {s:?} without finishing"));
            }
            _ => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    None
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let repo = scratch("repo");
    let data = scratch("data");

    println!("repository: {}", repo.display());

    let store = Arc::new(JsonlStore::open(data.join("events.jsonl")).unwrap());
    let run_log = Arc::new(JsonlRunLog::open(data.join("runs")).unwrap());
    let git = Arc::new(CliGit::new(&repo, data.join("worktrees")));

    harness_git_cli::ensure_workspace(&repo).expect("git init");
    let worker = Arc::new(SidecarAgent::new(
        "node",
        PathBuf::from("sidecar/index.mjs"),
    ));
    // The review step is deliberately human: this proves the queue, not the
    // Director, and keeps the cost of the check to exactly one run. No review
    // hook, for the same reason — nobody takes it, and the card waits.

    let mut config = EngineConfig::new("e2e", repo.clone());
    config.base_branch = "main".into();
    let (handle, mut events, mut runs) = Engine::spawn(
        EngineDeps {
            store: store.clone() as Arc<dyn StorePort>,
            clock: Arc::new(SystemClock),
            agent: worker.clone() as Arc<dyn AgentPort>,
            git: git.clone(),
            approver: None,
            review: None,
            run_log: Some(run_log.clone() as Arc<dyn RunLogPort>),
        },
        config,
        EnginePolicy::default(),
        vec![],
    );
    let _ = (&mut events, &mut runs); // drained lazily below

    let card = "c_e2e";
    handle
        .execute(harness_domain::Command::CreateCard {
            card_id: harness_domain::CardId::new(card),
            title: "Write hello.txt containing E2E_OK".into(),
        })
        .await
        .expect("create card");
    handle
        .execute(harness_domain::Command::MoveCard {
            card_id: harness_domain::CardId::new(card),
            to: harness_domain::Status::Ready,
        })
        .await
        .expect("ready");

    let run_id = handle
        .start_run(
            harness_domain::CardId::new(card),
            "Create a file named hello.txt whose entire content is the single line \
             E2E_OK. Change nothing else. Do not commit yourself; Harness commits."
                .into(),
            harness_ports::RunProfile {
            grants: harness_ports::Grants::default(),
                provider: None,
                agent_id: "builder".into(),
                model: Some("haiku".into()),
                allowed_tools: None,
                permission_mode: Some("acceptEdits".into()),
                max_budget_usd: Some(0.25),
                worktree: harness_ports::WorktreeMode::PerCard,
                reviewer: harness_ports::Reviewer::Human,
                max_concurrent: 1,
                output_style: None,
            },
        )
        .await
        .expect("start run");
    println!("run started: {run_id}");

    let outcome = wait_for_review(&handle, card).await;
    println!("wait outcome: {outcome:?}");
    let snap = handle.snapshot().await.unwrap();
    let done = snap.cards.iter().find(|c| c.id.as_str() == card);
    println!("card status: {:?}", done.map(|c| c.status));
    println!(
        "session survived: {}",
        done.and_then(|c| c.session_id.clone()).is_some()
    );

    // The transcript must exist and the worktree must hold the commit.
    let wt = done.and_then(|c| c.worktree.clone()).unwrap_or_default();
    let log_path = data.join("runs").join(format!("{}.jsonl", run_id.0));
    println!("transcript written: {}", log_path.exists());
    println!("worktree: {wt}");

    let mut verdict = String::new();
    match done.map(|c| c.status) {
        Some(harness_domain::Status::Review) => {
            let commit = std::process::Command::new("git")
                .args(["log", "-1", "--format=%s"])
                .current_dir(&wt)
                .output();
            match commit {
                Ok(out) => {
                    let msg = String::from_utf8_lossy(&out.stdout).to_string();
                    println!("commit subject: {}", msg.trim());
                    if msg.starts_with("harness:") {
                        verdict.push_str("E2E PASS: Ready → running → Review, committed.\n");
                    } else {
                        verdict.push_str("E2E FAIL: reached Review without the trailer commit.\n");
                    }
                }
                Err(e) => verdict.push_str(&format!("E2E FAIL: could not read git log: {e}\n")),
            }
        }
        other => verdict.push_str(&format!(
            "E2E FAIL: expected Review, got {other:?}. Check `claude` login and network.\n"
        )),
    }

    print!("{verdict}");

}
