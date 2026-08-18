use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::error::{FlowError, Result};
use crate::model::{validate_run_id, WorkflowRunSnapshot, WorkflowSpec};

use super::validation::ensure_same_start;
use super::FlowEngine;

impl FlowEngine {
    /// Follow persisted continue-as-new links from `run_id` in execution order.
    ///
    /// Every returned snapshot owns an independent, append-only event stream.
    /// Missing successors and cycles fail closed instead of silently returning
    /// a partial lineage.
    pub async fn continuation_chain(&self, run_id: &str) -> Result<Vec<WorkflowRunSnapshot>> {
        validate_run_id(run_id)?;
        let mut current_run_id = run_id.to_string();
        let mut visited = BTreeSet::new();
        let mut chain = Vec::new();
        let mut expected_start: Option<(WorkflowSpec, serde_json::Value)> = None;

        for hop in 0..=self.max_continue_as_new_hops {
            if !visited.insert(current_run_id.clone()) {
                return Err(FlowError::ContinueAsNewCycle(current_run_id));
            }
            let snapshot = self.snapshot(&current_run_id).await?;
            if let Some((expected_spec, expected_input)) = expected_start.as_ref() {
                ensure_same_start(&current_run_id, &snapshot, expected_spec, expected_input)?;
            }
            let successor_run_id = snapshot
                .continuation
                .as_ref()
                .map(|continuation| continuation.successor_run_id.clone());
            expected_start = snapshot
                .continuation
                .as_ref()
                .map(|continuation| (snapshot.spec.clone(), continuation.input.clone()));
            chain.push(snapshot);
            let Some(successor_run_id) = successor_run_id else {
                return Ok(chain);
            };
            if hop == self.max_continue_as_new_hops {
                return Err(FlowError::ContinueAsNewLimitExceeded(
                    self.max_continue_as_new_hops,
                ));
            }
            current_run_id = successor_run_id;
        }

        Err(FlowError::ContinueAsNewLimitExceeded(
            self.max_continue_as_new_hops,
        ))
    }

    /// Replay and dispatch until the execution reaches a terminal state or an
    /// open wait/hook suspension.
    ///
    /// A continue-as-new terminal event is followed into its fresh successor
    /// segment. The returned snapshot therefore belongs to the active leaf of
    /// the execution chain, which can differ from `run_id`.
    pub async fn drive(&self, run_id: &str) -> Result<WorkflowRunSnapshot> {
        self.drive_at(run_id, Utc::now()).await
    }

    pub(super) async fn drive_at(
        &self,
        run_id: &str,
        now: DateTime<Utc>,
    ) -> Result<WorkflowRunSnapshot> {
        let mut current_run_id = run_id.to_string();
        let mut visited = BTreeSet::new();

        for hop in 0..=self.max_continue_as_new_hops {
            if !visited.insert(current_run_id.clone()) {
                return Err(FlowError::ContinueAsNewCycle(current_run_id));
            }

            let allow_continue_as_new = hop < self.max_continue_as_new_hops;
            let snapshot = self
                .drive_run_at(&current_run_id, now, allow_continue_as_new)
                .await?;
            let Some(successor_run_id) = self
                .ensure_continuation_successor(&snapshot, &visited, hop, true)
                .await?
            else {
                return Ok(snapshot);
            };
            current_run_id = successor_run_id;
        }

        Err(FlowError::ContinueAsNewLimitExceeded(
            self.max_continue_as_new_hops,
        ))
    }

    pub(super) async fn ensure_continuation_leaf(
        &self,
        run_id: &str,
        require_runtime_build: bool,
    ) -> Result<WorkflowRunSnapshot> {
        validate_run_id(run_id)?;
        let mut current_run_id = run_id.to_string();
        let mut visited = BTreeSet::new();

        for hop in 0..=self.max_continue_as_new_hops {
            if !visited.insert(current_run_id.clone()) {
                return Err(FlowError::ContinueAsNewCycle(current_run_id));
            }
            let snapshot = self.snapshot(&current_run_id).await?;
            let Some(successor_run_id) = self
                .ensure_continuation_successor(&snapshot, &visited, hop, require_runtime_build)
                .await?
            else {
                return Ok(snapshot);
            };
            current_run_id = successor_run_id;
        }

        Err(FlowError::ContinueAsNewLimitExceeded(
            self.max_continue_as_new_hops,
        ))
    }

    async fn ensure_continuation_successor(
        &self,
        snapshot: &WorkflowRunSnapshot,
        visited: &BTreeSet<String>,
        hop: usize,
        require_runtime_build: bool,
    ) -> Result<Option<String>> {
        let Some(continuation) = snapshot.continuation.as_ref() else {
            return Ok(None);
        };
        if hop == self.max_continue_as_new_hops {
            return Err(FlowError::ContinueAsNewLimitExceeded(
                self.max_continue_as_new_hops,
            ));
        }
        if visited.contains(&continuation.successor_run_id) {
            return Err(FlowError::ContinueAsNewCycle(
                continuation.successor_run_id.clone(),
            ));
        }
        self.ensure_run_started_with_admission(
            &continuation.successor_run_id,
            &snapshot.spec,
            &continuation.input,
            require_runtime_build,
        )
        .await?;
        Ok(Some(continuation.successor_run_id.clone()))
    }
}
