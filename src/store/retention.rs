use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use uuid::Uuid;

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope, WorkflowContinuation, WorkflowSpec};

struct LinkedWorkflowStart {
    run_id: String,
    spec: WorkflowSpec,
    input: serde_json::Value,
}

/// Bounded policy for deleting complete terminal histories.
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
    #[cfg(any(feature = "postgres", feature = "sqlite"))]
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
#[cfg(any(feature = "postgres", feature = "sqlite"))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowHistoryHold {
    pub run_id: String,
    pub hold_id: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Minimal audit record retained after a complete event history is deleted.
#[cfg(any(feature = "postgres", feature = "sqlite"))]
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
/// Storage adapters own locking, transaction, and deletion details, while this
/// function is the single source of truth for terminal eligibility and
/// linked-component protection.
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
    let mut continuations = BTreeMap::new();
    let mut child_workflows = BTreeMap::<String, Vec<LinkedWorkflowStart>>::new();
    for (parent_run_id, history) in histories {
        for envelope in history {
            let Some(child_run_id) = linked_flow_run_id(&envelope.event) else {
                continue;
            };
            if let FlowEvent::RunContinuedAsNew {
                successor_run_id,
                input,
            } = &envelope.event
            {
                continuations.insert(
                    parent_run_id.clone(),
                    WorkflowContinuation {
                        successor_run_id: successor_run_id.clone(),
                        input: input.clone(),
                    },
                );
            }
            if let FlowEvent::ChildWorkflowRequested {
                child_run_id,
                spec,
                input,
                ..
            } = &envelope.event
            {
                child_workflows
                    .entry(parent_run_id.clone())
                    .or_default()
                    .push(LinkedWorkflowStart {
                        run_id: child_run_id.clone(),
                        spec: spec.clone(),
                        input: input.clone(),
                    });
            }
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
        validate_linked_workflow_component(
            &component,
            histories,
            &continuations,
            &child_workflows,
            storage_name,
        )?;
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

fn validate_linked_workflow_component(
    component: &BTreeSet<String>,
    histories: &BTreeMap<String, Vec<FlowEventEnvelope>>,
    continuations: &BTreeMap<String, WorkflowContinuation>,
    child_workflows: &BTreeMap<String, Vec<LinkedWorkflowStart>>,
    storage_name: &str,
) -> Result<()> {
    for start in component {
        let mut path = BTreeSet::new();
        let mut current = start.as_str();
        while component.contains(current) {
            let Some(continuation) = continuations.get(current) else {
                break;
            };
            if !path.insert(current.to_string()) {
                return Err(FlowError::ContinueAsNewCycle(current.to_string()));
            }
            current = &continuation.successor_run_id;
        }
    }

    validate_child_workflow_cycles(component, continuations, child_workflows)?;

    for (predecessor_run_id, continuation) in continuations {
        if !component.contains(predecessor_run_id) {
            continue;
        }
        let Some(successor_history) = histories.get(&continuation.successor_run_id) else {
            continue;
        };
        let predecessor_history = histories.get(predecessor_run_id).ok_or_else(|| {
            FlowError::Store(format!(
                "{storage_name} continuation predecessor {predecessor_run_id} disappeared during retention"
            ))
        })?;
        let predecessor = project_run(predecessor_run_id, predecessor_history)?;
        let successor = project_run(&continuation.successor_run_id, successor_history)?;
        if successor.spec != predecessor.spec {
            return Err(FlowError::RunConflict {
                run_id: continuation.successor_run_id.clone(),
                reason: "continue-as-new successor workflow spec differs".to_string(),
            });
        }
        if successor.input != continuation.input {
            return Err(FlowError::RunConflict {
                run_id: continuation.successor_run_id.clone(),
                reason: "continue-as-new successor input differs".to_string(),
            });
        }
    }
    for (parent_run_id, children) in child_workflows {
        if !component.contains(parent_run_id) {
            continue;
        }
        for child in children {
            let Some(child_history) = histories.get(&child.run_id) else {
                continue;
            };
            let child_snapshot = project_run(&child.run_id, child_history)?;
            if child_snapshot.spec != child.spec {
                return Err(FlowError::RunConflict {
                    run_id: child.run_id.clone(),
                    reason: "child workflow spec differs from parent request".to_string(),
                });
            }
            if child_snapshot.input != child.input {
                return Err(FlowError::RunConflict {
                    run_id: child.run_id.clone(),
                    reason: "child workflow input differs from parent request".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_child_workflow_cycles(
    component: &BTreeSet<String>,
    continuations: &BTreeMap<String, WorkflowContinuation>,
    child_workflows: &BTreeMap<String, Vec<LinkedWorkflowStart>>,
) -> Result<()> {
    let mut outgoing = component
        .iter()
        .map(|run_id| (run_id.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut indegree = component
        .iter()
        .map(|run_id| (run_id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    for (source, continuation) in continuations {
        add_owned_edge(
            component,
            &mut outgoing,
            &mut indegree,
            source,
            &continuation.successor_run_id,
        );
    }
    for (source, children) in child_workflows {
        for child in children {
            add_owned_edge(
                component,
                &mut outgoing,
                &mut indegree,
                source,
                &child.run_id,
            );
        }
    }

    let mut ready = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(run_id, _)| run_id.clone())
        .collect::<Vec<_>>();
    let mut removed = 0_usize;
    while let Some(run_id) = ready.pop() {
        removed += 1;
        if let Some(targets) = outgoing.get(&run_id) {
            for target in targets {
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push(target.clone());
                    }
                }
            }
        }
    }
    if removed != component.len() {
        let run_id = indegree
            .into_iter()
            .find(|(_, degree)| *degree > 0)
            .map(|(run_id, _)| run_id)
            .unwrap_or_else(|| "unknown".to_string());
        return Err(FlowError::ChildWorkflowCycle(run_id));
    }
    Ok(())
}

fn add_owned_edge(
    component: &BTreeSet<String>,
    outgoing: &mut BTreeMap<String, BTreeSet<String>>,
    indegree: &mut BTreeMap<String, usize>,
    source: &str,
    target: &str,
) {
    if !component.contains(source) || !component.contains(target) {
        return;
    }
    if outgoing
        .entry(source.to_string())
        .or_default()
        .insert(target.to_string())
    {
        *indegree.entry(target.to_string()).or_default() += 1;
    }
}

pub(crate) fn linked_flow_run_id(event: &FlowEvent) -> Option<&str> {
    match event {
        FlowEvent::ChildOperationLinked { child } => child.flow_run_id.as_deref(),
        FlowEvent::RunContinuedAsNew {
            successor_run_id, ..
        } => Some(successor_run_id),
        FlowEvent::ChildWorkflowRequested { child_run_id, .. } => Some(child_run_id),
        _ => None,
    }
}

/// Return a reference that must already exist when its event is appended.
///
/// Continue-as-new intentionally commits the predecessor link first so a
/// replacement worker can recover a missing successor. Child-operation links
/// retain their stronger same-store existence invariant.
pub(crate) fn required_linked_flow_run_id(event: &FlowEvent) -> Option<&str> {
    match event {
        FlowEvent::ChildOperationLinked { child } => child.flow_run_id.as_deref(),
        _ => None,
    }
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub(crate) fn history_checksum(history: &[FlowEventEnvelope]) -> Result<String> {
    let digest = Sha256::digest(serde_json::to_vec(history)?);
    Ok(format!("{digest:x}"))
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
pub(crate) fn validate_history_hold(run_id: &str, hold_id: &str, reason: &str) -> Result<()> {
    if run_id.trim().is_empty() || hold_id.trim().is_empty() || reason.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "history hold run id, hold id, and reason must not be empty".to_string(),
        ));
    }
    Ok(())
}
