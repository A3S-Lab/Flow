use std::collections::{BTreeMap, BTreeSet};

use a3s_orm::{sql_query, PostgresTransaction};
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::error::{FlowError, Result};
use crate::model::{project_run, FlowEvent, FlowEventEnvelope};

use super::{
    execute_postgres, fetch_all_postgres, fetch_optional_postgres, latest_postgres_sequence,
    lock_postgres_retention_guard_exclusive, lock_postgres_run, map_postgres_transaction,
    row_to_envelope, PostgresEventStore,
};
use crate::store::{
    FlowHistoryHold, FlowHistoryRetentionPolicy, FlowHistoryRetentionReport, FlowHistoryTombstone,
};

const RETENTION_GUARD_LOCK_ID: &str = "a3s-flow-history-retention-guard";

impl PostgresEventStore {
    /// Persist an audit hold that prevents a run history from being pruned.
    ///
    /// Repeating the same `(run_id, hold_id, reason)` is idempotent. Reusing a
    /// hold ID with another reason returns a conflict so audit intent cannot be
    /// silently replaced.
    pub async fn hold_history(&self, run_id: &str, hold_id: &str, reason: &str) -> Result<()> {
        validate_history_hold(run_id, hold_id, reason)?;
        let run_id = run_id.to_string();
        let hold_id = hold_id.to_string();
        let reason = reason.to_string();
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move {
                    lock_postgres_run(transaction, &run_id).await?;
                    ensure_postgres_history_not_tombstoned(transaction, &run_id).await?;
                    if latest_postgres_sequence(transaction, &run_id).await? == 0 {
                        return Err(FlowError::RunNotFound(run_id));
                    }
                    let existing = fetch_optional_postgres(
                        transaction,
                        sql_query::<String>(
                            "SELECT reason FROM flow_history_holds WHERE run_id = ",
                        )
                        .bind(run_id.clone())
                        .append(" AND hold_id = ")
                        .bind(hold_id.clone()),
                    )
                    .await?;
                    match existing {
                        Some(existing) if existing == reason => return Ok(()),
                        Some(_) => {
                            return Err(FlowError::RunConflict {
                                run_id,
                                reason: format!(
                                    "history hold {hold_id:?} differs from the durable hold"
                                ),
                            })
                        }
                        None => {}
                    }
                    execute_postgres(
                        transaction,
                        sql_query::<()>(
                            "INSERT INTO flow_history_holds (run_id, hold_id, reason, created_at) VALUES (",
                        )
                        .bind(run_id)
                        .append(", ")
                        .bind(hold_id)
                        .append(", ")
                        .bind(reason)
                        .append(", ")
                        .bind(Utc::now().to_rfc3339())
                        .append(")"),
                    )
                    .await?;
                    Ok(())
                })
            })
            .await;
        map_postgres_transaction(result)
    }

    /// Release one audit hold. Returns false when the hold did not exist.
    pub async fn release_history_hold(&self, run_id: &str, hold_id: &str) -> Result<bool> {
        if run_id.trim().is_empty() || hold_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "history hold run id and hold id must not be empty".to_string(),
            ));
        }
        let run_id = run_id.to_string();
        let hold_id = hold_id.to_string();
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move {
                    lock_postgres_run(transaction, &run_id).await?;
                    let rows = execute_postgres(
                        transaction,
                        sql_query::<()>("DELETE FROM flow_history_holds WHERE run_id = ")
                            .bind(run_id)
                            .append(" AND hold_id = ")
                            .bind(hold_id),
                    )
                    .await?;
                    Ok(rows > 0)
                })
            })
            .await;
        map_postgres_transaction(result)
    }

    /// List durable audit holds for one run in stable hold-ID order.
    pub async fn history_holds(&self, run_id: &str) -> Result<Vec<FlowHistoryHold>> {
        let rows = fetch_all_postgres(
            &self.executor,
            sql_query::<(String, String, String, String)>(
                "SELECT run_id, hold_id, reason, created_at FROM flow_history_holds WHERE run_id = ",
            )
            .bind(run_id)
            .append(" ORDER BY hold_id ASC"),
        )
        .await?;
        rows.into_iter().map(history_hold_row).collect()
    }

    /// Read the minimal audit tombstone retained after history deletion.
    pub async fn history_tombstone(&self, run_id: &str) -> Result<Option<FlowHistoryTombstone>> {
        fetch_optional_postgres(
            &self.executor,
            sql_query::<(String, String, i64, String, String, String)>(
                "SELECT run_id, deleted_at, terminal_sequence, terminal_event_id, terminal_event_key, history_sha256 FROM flow_history_tombstones WHERE run_id = ",
            )
            .bind(run_id),
        )
        .await?
        .map(history_tombstone_row)
        .transpose()
    }

    /// Delete complete eligible terminal histories in one consistent scan.
    ///
    /// The scan takes an exclusive retention guard, locks existing run streams
    /// in stable order, preserves durable holds and linked components, writes a
    /// checksum tombstone, and only then deletes event rows. It never performs
    /// partial stream compaction.
    pub async fn prune_terminal_history(
        &self,
        policy: FlowHistoryRetentionPolicy,
    ) -> Result<FlowHistoryRetentionReport> {
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move { prune_postgres_history(transaction, &policy).await })
            })
            .await;
        map_postgres_transaction(result)
    }
}

async fn prune_postgres_history(
    transaction: &PostgresTransaction,
    policy: &FlowHistoryRetentionPolicy,
) -> Result<FlowHistoryRetentionReport> {
    lock_postgres_retention_guard_exclusive(transaction, RETENTION_GUARD_LOCK_ID).await?;
    let mut run_ids = fetch_all_postgres(
        transaction,
        sql_query::<String>("SELECT DISTINCT run_id FROM flow_events ORDER BY run_id ASC"),
    )
    .await?;
    run_ids.sort();
    run_ids.dedup();
    for run_id in &run_ids {
        lock_postgres_run(transaction, run_id).await?;
    }

    let rows = fetch_all_postgres(
        transaction,
        sql_query::<(String, i64, String, String, String)>(
            "SELECT run_id, sequence, event_id, timestamp, event_json FROM flow_events ORDER BY run_id ASC, sequence ASC",
        ),
    )
    .await?;
    let mut histories = BTreeMap::<String, Vec<FlowEventEnvelope>>::new();
    for row in rows {
        let envelope = row_to_envelope(row)?;
        histories
            .entry(envelope.run_id.clone())
            .or_default()
            .push(envelope);
    }

    let hold_run_ids = fetch_all_postgres(
        transaction,
        sql_query::<String>("SELECT DISTINCT run_id FROM flow_history_holds"),
    )
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();

    let mut report = FlowHistoryRetentionReport::default();
    let mut eligible = BTreeSet::new();
    for (run_id, history) in &histories {
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
                "PostgreSQL history for {run_id} is unexpectedly empty"
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
    for (parent_run_id, history) in &histories {
        for envelope in history {
            let FlowEvent::ChildOperationLinked { child } = &envelope.event else {
                continue;
            };
            let Some(child_run_id) = child.flow_run_id.as_ref() else {
                continue;
            };
            if !histories.contains_key(child_run_id) {
                dangling_reference_runs.insert(parent_run_id.clone());
                continue;
            }
            adjacency
                .entry(parent_run_id.clone())
                .or_default()
                .insert(child_run_id.clone());
            adjacency
                .entry(child_run_id.clone())
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

    for run_id in &deletable {
        let history = histories.get(run_id).ok_or_else(|| {
            FlowError::Store(format!("retention lost PostgreSQL history for {run_id}"))
        })?;
        let terminal = history.last().ok_or_else(|| {
            FlowError::Store(format!(
                "retention found empty PostgreSQL history for {run_id}"
            ))
        })?;
        let terminal_sequence = i64::try_from(terminal.sequence).map_err(|error| {
            FlowError::Store(format!(
                "terminal sequence {} for {run_id} exceeds PostgreSQL bigint: {error}",
                terminal.sequence
            ))
        })?;
        let history_sha256 = history_checksum(history)?;
        execute_postgres(
            transaction,
            sql_query::<()>(
                "INSERT INTO flow_history_tombstones (run_id, deleted_at, terminal_sequence, terminal_event_id, terminal_event_key, history_sha256) VALUES (",
            )
            .bind(run_id.clone())
            .append(", ")
            .bind(Utc::now().to_rfc3339())
            .append(", ")
            .bind(terminal_sequence)
            .append(", ")
            .bind(terminal.event_id.to_string())
            .append(", ")
            .bind(terminal.event.event_key())
            .append(", ")
            .bind(history_sha256)
            .append(")"),
        )
        .await?;
        execute_postgres(
            transaction,
            sql_query::<()>("DELETE FROM flow_events WHERE run_id = ").bind(run_id.clone()),
        )
        .await?;
    }

    report.deleted_run_ids = deletable.into_iter().collect();
    report.referenced_run_ids = referenced.into_iter().collect();
    report.held_run_ids.sort();
    report.non_terminal_run_ids.sort();
    report.recent_terminal_run_ids.sort();
    Ok(report)
}

pub(super) fn linked_flow_run_id(event: &FlowEvent) -> Option<&str> {
    match event {
        FlowEvent::ChildOperationLinked { child } => child.flow_run_id.as_deref(),
        _ => None,
    }
}

fn history_checksum(history: &[FlowEventEnvelope]) -> Result<String> {
    let digest = Sha256::digest(serde_json::to_vec(history)?);
    Ok(format!("{digest:x}"))
}

fn validate_history_hold(run_id: &str, hold_id: &str, reason: &str) -> Result<()> {
    if run_id.trim().is_empty() || hold_id.trim().is_empty() || reason.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "history hold run id, hold id, and reason must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn history_hold_row(
    (run_id, hold_id, reason, created_at): (String, String, String, String),
) -> Result<FlowHistoryHold> {
    Ok(FlowHistoryHold {
        run_id,
        hold_id,
        reason,
        created_at: created_at.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL history hold timestamp {created_at}: {error}"
            ))
        })?,
    })
}

fn history_tombstone_row(
    (
        run_id,
        deleted_at,
        terminal_sequence,
        terminal_event_id,
        terminal_event_key,
        history_sha256,
    ): (String, String, i64, String, String, String),
) -> Result<FlowHistoryTombstone> {
    Ok(FlowHistoryTombstone {
        run_id,
        deleted_at: deleted_at.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL history tombstone timestamp {deleted_at}: {error}"
            ))
        })?,
        terminal_sequence: u64::try_from(terminal_sequence).map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL tombstone terminal sequence {terminal_sequence}: {error}"
            ))
        })?,
        terminal_event_id: terminal_event_id.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid PostgreSQL tombstone event id {terminal_event_id}: {error}"
            ))
        })?,
        terminal_event_key,
        history_sha256,
    })
}

pub(super) async fn lock_postgres_retention_guard_shared(
    transaction: &PostgresTransaction,
) -> Result<()> {
    super::lock_postgres_retention_guard_shared(transaction, RETENTION_GUARD_LOCK_ID).await
}

pub(super) async fn ensure_postgres_history_not_tombstoned(
    transaction: &PostgresTransaction,
    run_id: &str,
) -> Result<()> {
    let tombstoned = fetch_optional_postgres(
        transaction,
        sql_query::<String>("SELECT run_id FROM flow_history_tombstones WHERE run_id = ")
            .bind(run_id),
    )
    .await?
    .is_some();
    if tombstoned {
        return Err(FlowError::RunConflict {
            run_id: run_id.to_string(),
            reason: "history was pruned and its run ID is tombstoned".to_string(),
        });
    }
    Ok(())
}
