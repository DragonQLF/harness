//! Routing tool-permission requests to the operator and back.
//!
//! The request id is minted by the agent adapter and carried end to end: the
//! adapter sends it up with the request, the UI answers with the same id, and
//! this router matches them. Nothing here invents its own identifier.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use harness_ports::{ApprovalRequest, Approver};
use serde::Serialize;
use tokio::sync::oneshot;

use crate::settings::Settings;

/// How the router tells the operator something is waiting. The shell plugs the
/// window in here; tests plug in a recorder.
pub trait Notifier: Send + Sync + 'static {
    /// A new request arrived.
    fn asked(&self, request: &PendingApproval);
    /// The queue changed; `pending` is the whole queue, oldest first.
    fn queue(&self, pending: &[PendingApproval]);
}

/// How long a request waits for an answer before it is denied.
const WAIT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct PendingApproval {
    pub request_id: String,
    pub project_id: String,
    pub card_id: Option<String>,
    pub tool: String,
    pub summary: String,
    pub asked_ms: u64,
}

struct Waiting {
    view: PendingApproval,
    tx: oneshot::Sender<bool>,
}

pub struct ApprovalRouter {
    settings: Arc<Mutex<Settings>>,
    pending: Mutex<HashMap<String, Waiting>>,
    notifier: OnceLock<Box<dyn Notifier>>,
}

impl ApprovalRouter {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            settings,
            pending: Mutex::new(HashMap::new()),
            notifier: OnceLock::new(),
        }
    }

    /// Attach the operator-facing notifier. Called once, at startup.
    pub fn attach(&self, notifier: Box<dyn Notifier>) {
        let _ = self.notifier.set(notifier);
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn broadcast(&self) {
        if let Some(notifier) = self.notifier.get() {
            notifier.queue(&self.pending_list());
        }
    }

    pub fn pending_list(&self) -> Vec<PendingApproval> {
        let mut list: Vec<PendingApproval> = self
            .pending
            .lock()
            .unwrap()
            .values()
            .map(|w| w.view.clone())
            .collect();
        list.sort_by_key(|p| p.asked_ms);
        list
    }

    /// Fill in the card once the run stream tells us which one asked.
    pub fn attach_card(&self, request_id: &str, card_id: &str) {
        let mut changed = false;
        if let Some(entry) = self.pending.lock().unwrap().get_mut(request_id) {
            if entry.view.card_id.as_deref() != Some(card_id) {
                entry.view.card_id = Some(card_id.to_string());
                changed = true;
            }
        }
        if changed {
            self.broadcast();
        }
    }

    pub fn resolve(&self, request_id: &str, allow: bool) -> Result<(), String> {
        let waiting = self.pending.lock().unwrap().remove(request_id);
        match waiting {
            Some(entry) => {
                let _ = entry.tx.send(allow);
                self.broadcast();
                Ok(())
            }
            None => Err(format!(
                "nothing is waiting on request {request_id} any more"
            )),
        }
    }

    /// Deny everything still waiting; used when shutting down.
    pub fn deny_all(&self) {
        let waiting: Vec<Waiting> = self.pending.lock().unwrap().drain().map(|(_, w)| w).collect();
        for entry in waiting {
            let _ = entry.tx.send(false);
        }
        self.broadcast();
    }

    /// Build the approver handed to one project's engine.
    pub fn approver_for(self: &Arc<Self>, project_id: &str) -> Approver {
        let me = Arc::clone(self);
        let project_id = project_id.to_string();
        Arc::new(move |request: ApprovalRequest| {
            let me = Arc::clone(&me);
            let project_id = project_id.clone();
            Box::pin(async move {
                // A standing allowance answers without bothering the operator.
                if me
                    .settings
                    .lock()
                    .unwrap()
                    .allows(&request.tool, &request.summary)
                {
                    return true;
                }

                let (tx, rx) = oneshot::channel();
                let view = PendingApproval {
                    request_id: request.request_id.clone(),
                    project_id,
                    card_id: None,
                    tool: request.tool.clone(),
                    summary: request.summary.clone(),
                    asked_ms: Self::now_ms(),
                };
                me.pending.lock().unwrap().insert(
                    request.request_id.clone(),
                    Waiting {
                        view: view.clone(),
                        tx,
                    },
                );
                if let Some(notifier) = me.notifier.get() {
                    notifier.asked(&view);
                }
                me.broadcast();

                let decision = tokio::time::timeout(WAIT, rx).await;
                me.pending.lock().unwrap().remove(&request.request_id);
                me.broadcast();
                // Timed out, dropped or denied all mean the same thing: no.
                matches!(decision, Ok(Ok(true)))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        asked: Mutex<Vec<String>>,
        queues: Mutex<Vec<usize>>,
    }

    impl Notifier for Arc<Recorder> {
        fn asked(&self, request: &PendingApproval) {
            self.asked.lock().unwrap().push(request.request_id.clone());
        }

        fn queue(&self, pending: &[PendingApproval]) {
            self.queues.lock().unwrap().push(pending.len());
        }
    }

    fn router() -> (Arc<ApprovalRouter>, Arc<Mutex<Settings>>, Arc<Recorder>) {
        let settings = Arc::new(Mutex::new(Settings::default()));
        let router = Arc::new(ApprovalRouter::new(Arc::clone(&settings)));
        let recorder = Arc::new(Recorder::default());
        router.attach(Box::new(Arc::clone(&recorder)));
        (router, settings, recorder)
    }

    fn request(id: &str) -> ApprovalRequest {
        ApprovalRequest {
            request_id: id.to_string(),
            tool: "Bash".to_string(),
            summary: "command: git push origin main".to_string(),
            input: serde_json::json!({ "command": "git push origin main" }),
        }
    }

    #[tokio::test]
    async fn the_answer_is_matched_by_the_adapters_request_id() {
        let (router, _s, recorder) = router();
        let approve = router.approver_for("proj");
        let waiting = tokio::spawn(async move { approve(request("req-7")).await });

        // Wait for the request to register, then answer it by its own id.
        for _ in 0..100 {
            if !router.pending_list().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let pending = router.pending_list();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request_id, "req-7");
        assert_eq!(pending[0].project_id, "proj");
        assert_eq!(pending[0].tool, "Bash");

        router.attach_card("req-7", "c_1");
        assert_eq!(router.pending_list()[0].card_id.as_deref(), Some("c_1"));

        assert_eq!(recorder.asked.lock().unwrap().clone(), vec!["req-7".to_string()]);
        assert!(!recorder.queues.lock().unwrap().is_empty());

        router.resolve("req-7", true).unwrap();
        assert!(waiting.await.unwrap());
        assert!(router.pending_list().is_empty());
    }

    #[tokio::test]
    async fn a_standing_allowance_answers_without_asking() {
        let (router, settings, recorder) = router();
        settings.lock().unwrap().allow_always("Bash");
        let approve = router.approver_for("proj");
        assert!(approve(request("req-1")).await);
        assert!(router.pending_list().is_empty());
        assert!(
            recorder.asked.lock().unwrap().is_empty(),
            "a standing allowance must not bother the operator"
        );
    }

    #[tokio::test]
    async fn denying_and_answering_twice_behave() {
        let (router, _s, _recorder) = router();
        let approve = router.approver_for("proj");
        let waiting = tokio::spawn(async move { approve(request("req-2")).await });
        for _ in 0..100 {
            if !router.pending_list().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        router.resolve("req-2", false).unwrap();
        assert!(!waiting.await.unwrap());
        assert!(router.resolve("req-2", true).is_err());
        assert!(router.resolve("never-existed", true).is_err());
    }

    #[tokio::test]
    async fn shutting_down_denies_everything_still_waiting() {
        let (router, _s, _recorder) = router();
        let approve = router.approver_for("proj");
        let waiting = tokio::spawn(async move { approve(request("req-3")).await });
        for _ in 0..100 {
            if !router.pending_list().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        router.deny_all();
        assert!(!waiting.await.unwrap());
    }
}
