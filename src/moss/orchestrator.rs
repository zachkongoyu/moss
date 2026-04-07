use std::sync::{Arc, Mutex};

use minijinja::{Environment, context};
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinSet;
use tracing::{info, warn};
use uuid::Uuid;

use crate::error::MossError;
use crate::providers::{Message, Role, Provider};

use super::artifact_guard::{ArtifactGuard, ScanVerdict};
use super::blackboard::{Blackboard, Evidence, EvidenceStatus, Gap, GapState};
use super::compiler::Compiler;
use super::decomposition::Decomposition;
use super::executor::Executor;
use super::signal::{self, Event};

/// After this many failed attempts a Gap is force-closed to prevent infinite loops.
const MAX_RETRIES: u32 = 3;

pub(crate) struct Orchestrator {
    provider: Arc<dyn Provider>,
    compiler: Arc<Compiler>,
    guard: Arc<ArtifactGuard>,
    blackboard: Mutex<Arc<Blackboard>>,
    tx: broadcast::Sender<signal::Payload>,
}

impl Orchestrator {
    pub(crate) fn new(provider: Arc<dyn Provider>, tx: broadcast::Sender<signal::Payload>) -> Self {
        Self {
            compiler: Arc::new(Compiler::new(Arc::clone(&provider))),
            guard: Arc::new(ArtifactGuard::new()),
            provider,
            blackboard: Mutex::new(Arc::new(Blackboard::new(tx.clone()))),
            tx,
        }
    }

    pub(crate) fn approve(&self, gap_id: Uuid, approved: bool) {
        self.blackboard.lock().unwrap().approve(gap_id, approved);
    }

    /// Run a single user query end-to-end.
    pub(crate) async fn run(&self, query: &str) -> Result<String, MossError> {
        let board = self.blackboard.lock().unwrap().clone();

        let decomposition = self.decompose(query, &board).await?;

        let board = if decomposition.is_follow_up {
            board
        } else {
            // TODO: Sealing the old blackboard

            let fresh = Arc::new(Blackboard::new(self.tx.clone()));
            *self.blackboard.lock().unwrap() = Arc::clone(&fresh);
            fresh
        };

        if let Some(ref intent) = decomposition.intent {
            board.set_intent(intent.as_str());
        }

        for spec in decomposition.gaps.unwrap_or_default() {
            let gap = Gap::new(
                spec.name,
                spec.description,
                spec.gap_type,
                spec.dependencies.into_iter().map(|s| s.into_boxed_str()).collect(),
                spec.constraints,
                spec.expected_output.map(|s| s.into_boxed_str()),
            );
            board.insert_gap(gap)?;
        }

        self.drive_gaps(Arc::clone(&board)).await?;

        self.synthesize(&board).await
    }

    /// Drive the Gap DAG to completion: dispatch ready gaps, await one result per tick.
    async fn drive_gaps(&self, blackboard: Arc<Blackboard>) -> Result<(), MossError> {
        let mut tasks: JoinSet<Result<(), MossError>> = JoinSet::new();

        loop {
            blackboard.promote_unblocked();

            for gap in blackboard.drain_ready() {
                let compiler = Arc::clone(&self.compiler);
                let guard = Arc::clone(&self.guard);
                let bb = Arc::clone(&blackboard);

                info!(gap = %gap.name(), "dispatched");

                tasks.spawn(async move {
                    let evs = bb.get_evidence(&gap.gap_id());
                    let attempt_count = evs.len() as u32;

                    let prior: Vec<Box<str>> = evs
                        .iter()
                        .filter_map(|ev| match ev.status() {
                            EvidenceStatus::Failure { reason } => Some(reason.as_str().into()),
                            _ => None,
                        })
                        .collect();

                    if attempt_count >= MAX_RETRIES {
                        warn!(gap = %gap.name(), "max retries reached — force closing");
                        return bb.set_gap_state(&gap.gap_id(), GapState::Closed);
                    }

                    let artifact = compiler.compile(&gap, &prior).await?;

                    match guard.scan(&artifact, gap.constraints()) {
                        ScanVerdict::Approved => {}
                        ScanVerdict::Rejected { reason } => {
                            warn!(gap = %gap.name(), %reason, "rejected by guard");
                            let attempt = bb.get_evidence(&gap.gap_id()).len() as u32 + 1;
                            bb.append_evidence(Evidence::new(
                                gap.gap_id(),
                                attempt,
                                serde_json::json!({ "guard": "rejected", "reason": reason.as_ref() }),
                                EvidenceStatus::Failure { reason: reason.into() },
                            ));
                            return bb.set_gap_state(&gap.gap_id(), GapState::Closed);
                        }
                        ScanVerdict::Gated { reason } => {
                            bb.set_gap_state(&gap.gap_id(), GapState::Gated)?;

                            let (request, approval) = oneshot::channel();
                            bb.register_approval(gap.gap_id(), request);
                            let _ = bb.signal_tx().send(Event::ApprovalRequested {
                                gap_id: gap.gap_id(),
                                gap_name: gap.name().into(),
                                reason: reason.clone(),
                            });

                            let approved = approval.await.unwrap_or(false);
                            if !approved {
                                warn!(gap = %gap.name(), "guard denied by user");
                                let attempt = bb.get_evidence(&gap.gap_id()).len() as u32 + 1;
                                bb.append_evidence(Evidence::new(
                                    gap.gap_id(),
                                    attempt,
                                    serde_json::json!({ "guard": "denied", "reason": reason.as_ref() }),
                                    EvidenceStatus::Failure { reason: reason.into() },
                                ));
                                return bb.set_gap_state(&gap.gap_id(), GapState::Closed);
                            }

                            bb.set_gap_state(&gap.gap_id(), GapState::Assigned)?;
                        }
                    }

                    Executor::new().run(&gap, &artifact, &bb).await?;

                    let last_success = bb
                        .get_evidence(&gap.gap_id())
                        .last()
                        .map(|e| matches!(e.status(), EvidenceStatus::Success))
                        .unwrap_or(false);

                    if last_success {
                        info!(gap = %gap.name(), "closed (success)");
                        bb.set_gap_state(&gap.gap_id(), GapState::Closed)
                    } else {
                        warn!(gap = %gap.name(), "execution failed — will retry");
                        bb.set_gap_state(&gap.gap_id(), GapState::Ready)
                    }
                });
            }

            if tasks.is_empty() {
                return if blackboard.all_closed() {
                    Ok(())
                } else {
                    Err(MossError::Deadlock)
                };
            }

            if let Some(result) = tasks.join_next().await {
                result.map_err(|e| MossError::Blackboard(format!("task panicked: {e}")))??;
            }
        }
    }

    /// Ask the LLM to decompose the query into a Gap DAG and insert gaps into the Blackboard.
    /// Returns the intent string and the names of the gaps created.
    pub(crate) async fn decompose(&self, query: &str, blackboard: &Blackboard) -> Result<Decomposition, MossError> {
        let template_src = include_str!("prompts/decompose.md");

        let blackboard_state = blackboard.snapshot();

        let mut env = Environment::new();
        env.add_template("decompose", template_src)
            .map_err(|e| MossError::Blackboard(format!("template error: {e}")))?;

        let tmpl = env.get_template("decompose")
            .map_err(|e| MossError::Blackboard(format!("template load error: {e}")))?;

        let rendered = tmpl
            .render(context! { user_query => query, blackboard_state => blackboard_state })
            .map_err(|e| MossError::Blackboard(format!("template render error: {e}")))?;

        let messages = vec![Message { role: Role::User, content: rendered.into_boxed_str() }];

        let raw = self.provider.complete_chat(messages).await?;

        // Strip markdown fences if the model wrapped the JSON anyway
        let json_str = raw
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let decomposition: Decomposition = serde_json::from_str(json_str)?;

        if let Ok(pretty) = serde_json::to_string_pretty(&decomposition) {
            info!("Decomposed DAG:\n{}", pretty);
        }

        Ok(decomposition)
    }

    /// Collect evidence from the Blackboard and synthesize a final answer.
    pub(crate) async fn synthesize(&self, blackboard: &Blackboard) -> Result<String, MossError> {
        let template_src = include_str!("prompts/synthesize.md");

        let intent = blackboard
            .get_intent()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown intent".to_string());

        let evidence = serde_json::to_string_pretty(&blackboard.all_evidence())?;

        let mut env = Environment::new();
        env.add_template("synthesize", template_src)
            .map_err(|e| MossError::Blackboard(format!("template error: {e}")))?;

        let tmpl = env.get_template("synthesize")
            .map_err(|e| MossError::Blackboard(format!("template load error: {e}")))?;

        let rendered = tmpl
            .render(context! { intent => intent, evidence => evidence })
            .map_err(|e| MossError::Blackboard(format!("template render error: {e}")))?;

        let messages = vec![Message { role: Role::User, content: rendered.into_boxed_str() }];

        info!("synthesizing final answer");
        let response = self.provider.complete_chat(messages).await?;

        Ok(response)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    use async_trait::async_trait;

    use crate::error::ProviderError;
    use crate::moss::blackboard::{Blackboard, Gap, GapState, GapType};
    use crate::moss::signal;
    use crate::providers::{Message, Provider};

    use super::Orchestrator;

    fn bb() -> Arc<Blackboard> { Arc::new(Blackboard::new(signal::channel(1).0)) }

    fn orchestrator(provider: impl Provider + 'static) -> Orchestrator {
        let (tx, _rx) = signal::channel(16);
        Orchestrator::new(Arc::new(provider), tx)
    }

    struct AlwaysSucceedProvider;

    #[async_trait]
    impl Provider for AlwaysSucceedProvider {
        async fn complete_chat(&self, _: Vec<Message>) -> Result<String, ProviderError> {
            Ok(r#"{"type":"SCRIPT","language":"shell","code":"echo '{\"result\":\"ok\"}'","timeout_secs":10}"#.into())
        }
    }

    /// Fails on the first call, succeeds on all subsequent calls.
    struct FailOnceThenSucceedProvider {
        calls: Arc<AtomicUsize>,
    }

    impl FailOnceThenSucceedProvider {
        fn new() -> Self { Self { calls: Arc::new(AtomicUsize::new(0)) } }
    }

    #[async_trait]
    impl Provider for FailOnceThenSucceedProvider {
        async fn complete_chat(&self, _: Vec<Message>) -> Result<String, ProviderError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Ok(r#"{"type":"SCRIPT","language":"shell","code":"exit 1","timeout_secs":10}"#.into())
            } else {
                Ok(r#"{"type":"SCRIPT","language":"shell","code":"echo '{\"result\":\"ok\"}'","timeout_secs":10}"#.into())
            }
        }
    }

    fn gap(name: &str, deps: Vec<&str>) -> Gap {
        Gap::new(
            name,
            "test gap",
            GapType::Proactive,
            deps.into_iter().map(|s| s.into()).collect(),
            None,
            None,
        )
    }

    #[tokio::test]
    async fn single_gap_closes_on_success() {
        let o = orchestrator(AlwaysSucceedProvider);
        let bb = bb();
        bb.insert_gap(gap("g1", vec![])).unwrap();
        o.drive_gaps(Arc::clone(&bb)).await.unwrap();
        let id = bb.get_gap_id_by_name("g1").unwrap();
        assert_eq!(bb.get_gap(&id).unwrap().state(), &GapState::Closed);
    }

    #[tokio::test]
    async fn linear_chain_closes_in_order() {
        let o = orchestrator(AlwaysSucceedProvider);
        let bb = bb();
        bb.insert_gap(gap("A", vec![])).unwrap();
        bb.insert_gap(gap("B", vec!["A"])).unwrap();
        o.drive_gaps(Arc::clone(&bb)).await.unwrap();
        let a_id = bb.get_gap_id_by_name("A").unwrap();
        let b_id = bb.get_gap_id_by_name("B").unwrap();
        assert_eq!(bb.get_gap(&a_id).unwrap().state(), &GapState::Closed);
        assert_eq!(bb.get_gap(&b_id).unwrap().state(), &GapState::Closed);
    }

    #[tokio::test]
    async fn gap_retries_after_failure() {
        let o = orchestrator(FailOnceThenSucceedProvider::new());
        let bb = bb();
        bb.insert_gap(gap("retry_gap", vec![])).unwrap();
        o.drive_gaps(Arc::clone(&bb)).await.unwrap();
        let id = bb.get_gap_id_by_name("retry_gap").unwrap();
        assert_eq!(bb.get_gap(&id).unwrap().state(), &GapState::Closed);
        assert_eq!(bb.get_evidence(&id).len(), 2);
    }

    #[tokio::test]
    async fn deadlock_if_deps_never_close() {
        let o = orchestrator(AlwaysSucceedProvider);
        let bb = bb();
        bb.insert_gap(gap("B", vec!["A"])).unwrap();
        let result = o.drive_gaps(Arc::clone(&bb)).await;
        assert!(matches!(result, Err(crate::error::MossError::Deadlock)));
    }
}
