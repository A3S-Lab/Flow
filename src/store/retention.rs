use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope};

/// Bounded policy for deleting complete terminal SQL histories.
///
/// Flow never rewrites or partially compacts an event stream. A retention scan
/// removes an entire terminal history only when it is older than
/// `terminal_before`, has no durable audit hold, and belongs to a linked-run
/// component whose other histories are eligible in the same scan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryRetentionPolicy {
    pub terminal_before: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ids: Option<BTreeSet<String>>,
}

impl FlowHistoryRetentionPolicy {
    pub fn new(terminal_before: DateTime<Utc>) -> Self {
        Self {
            terminal_before,
            run_ids: None,
        }
    }

    /// Restrict deletion candidates to an explicit set of run IDs.
    ///
    /// Linked histories outside this set remain protected and therefore also
    /// protect candidate histories connected to them.
    pub fn with_run_ids<I, S>(mut self, run_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.run_ids = Some(run_ids.into_iter().map(Into::into).collect());
        self
    }

    pub(crate) fn includes(&self, run_id: &str) -> bool {
        self.run_ids
            .as_ref()
            .is_none_or(|run_ids| run_ids.contains(run_id))
    }
}

/// Persistent reason that prevents a run history from being pruned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryHold {
    pub run_id: String,
    pub hold_id: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Minimal audit record retained after a complete event history is deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryTombstone {
    pub run_id: String,
    pub deleted_at: DateTime<Utc>,
    pub terminal_sequence: u64,
    pub terminal_event_id: Uuid,
    pub terminal_event_key: String,
    pub history_sha256: String,
}

/// Detailed result of a terminal-history retention scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryRetentionReport {
    pub deleted_run_ids: Vec<String>,
    pub held_run_ids: Vec<String>,
    pub referenced_run_ids: Vec<String>,
    pub non_terminal_run_ids: Vec<String>,
    pub recent_terminal_run_ids: Vec<String>,
}

pub(crate) struct FlowHistoryRetentionPlan {
    pub(crate) deletable_run_ids: BTreeSet<String>,
    pub(crate) report: FlowHistoryRetentionReport,
}

/// Apply the backend-independent retention rules to one consistent history view.
///
/// SQL adapters own transaction and deletion details, while this function is
/// the single source of truth for terminal eligibility and linked-component
/// protection.
pub(crate) fn plan_history_retention(
    histories: &BTreeMap<String, Vec<FlowEventEnvelope>>,
    hold_run_ids: &BTreeSet<String>,
    policy: &FlowHistoryRetentionPolicy,
    storage_name: &str,
) -> Result<FlowHistoryRetentionPlan> {
    let mut report = FlowHistoryRetentionReport::default();
    let mut eligible = BTreeSet::new();
    for (run_id, history) in histories {
        if !policy.includes(run_id) {
            continue;
        }
        let snapshot = project_run(run_id, history)?;
        if !snapshot.status.is_terminal() {
            report.non_terminal_run_ids.push(run_id.clone());
            continue;
        }
        let terminal = history.last().ok_or_else(|| {
            FlowError::Store(format!(
                "{storage_name} history for {run_id} is unexpectedly empty"
            ))
        })?;
        if terminal.timestamp >= policy.terminal_before {
            report.recent_terminal_run_ids.push(run_id.clone());
            continue;
        }
        if hold_run_ids.contains(run_id) {
            report.held_run_ids.push(run_id.clone());
            continue;
        }
        eligible.insert(run_id.clone());
    }

    let mut adjacency = histories
        .keys()
        .map(|run_id| (run_id.clone(), BTreeSet::<String>::new()))
        .collect::<BTreeMap<_, _>>();
    let mut dangling_reference_runs = BTreeSet::new();
    for (parent_run_id, history) in histories {
        for envelope in history {
            let Some(child_run_id) = linked_flow_run_id(&envelope.event) else {
                continue;
            };
            if !histories.contains_key(child_run_id) {
                dangling_reference_runs.insert(parent_run_id.clone());
                continue;
            }
            adjacency
                .entry(parent_run_id.clone())
                .or_default()
                .insert(child_run_id.to_string());
            adjacency
                .entry(child_run_id.to_string())
                .or_default()
                .insert(parent_run_id.clone());
        }
    }

    let mut visited = BTreeSet::new();
    let mut deletable = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for start in &eligible {
        if visited.contains(start) {
            continue;
        }
        let mut component = BTreeSet::new();
        let mut pending = vec![start.clone()];
        while let Some(run_id) = pending.pop() {
            if !component.insert(run_id.clone()) {
                continue;
            }
            if let Some(neighbors) = adjacency.get(&run_id) {
                pending.extend(neighbors.iter().cloned());
            }
        }
        visited.extend(component.iter().cloned());
        let component_is_deletable = component.iter().all(|run_id| eligible.contains(run_id))
            && component
                .iter()
                .all(|run_id| !dangling_reference_runs.contains(run_id));
        if component_is_deletable {
            deletable.extend(component);
        } else {
            referenced.extend(
                component
                    .into_iter()
                    .filter(|run_id| eligible.contains(run_id)),
            );
        }
    }

    report.referenced_run_ids = referenced.into_iter().collect();
    report.held_run_ids.sort();
    report.non_terminal_run_ids.sort();
    report.recent_terminal_run_ids.sort();
    Ok(FlowHistoryRetentionPlan {
        deletable_run_ids: deletable,
        report,
    })
}

pub(crate) fn linked_flow_run_id(event: &FlowEvent) -> Option<&str> {
    match event {
        FlowEvent::ChildOperationLinked { child } => child.flow_run_id.as_deref(),
        _ => None,
    }
}

pub(crate) fn history_checksum(history: &[FlowEventEnvelope]) -> Result<String> {
    let digest = Sha256::digest(serde_json::to_vec(history)?);
    Ok(format!("{digest:x}"))
}

pub(crate) fn validate_history_hold(run_id: &str, hold_id: &str, reason: &str) -> Result<()> {
    if run_id.trim().is_empty() || hold_id.trim().is_empty() || reason.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "history hold run id, hold id, and reason must not be empty".to_string(),
        ));
    }
    Ok(())
}
