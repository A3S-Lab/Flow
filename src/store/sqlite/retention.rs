use std::collections::{BTreeMap, BTreeSet};

use a3s_orm::{sql_query, SqliteTransaction};
use chrono::Utc;

use crate::error::{FlowError, Result};
use crate::model::FlowEventEnvelope;
use crate::store::retention::{history_checksum, plan_history_retention, validate_history_hold};
use crate::store::{
    FlowHistoryHold, FlowHistoryRetentionPolicy, FlowHistoryRetentionReport, FlowHistoryTombstone,
};

use super::{
    execute_sqlite, fetch_all_sqlite, fetch_optional_sqlite, latest_sqlite_sequence,
    map_sqlite_transaction, row_to_envelope, SqliteEventStore,
};

pub(super) use crate::store::retention::linked_flow_run_id;

impl SqliteEventStore {
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
                    ensure_sqlite_history_not_tombstoned(transaction, &run_id).await?;
                    if latest_sqlite_sequence(transaction, &run_id).await? == 0 {
                        return Err(FlowError::RunNotFound(run_id));
                    }
                    let existing = fetch_optional_sqlite(
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
                            });
                        }
                        None => {}
                    }
                    execute_sqlite(
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
        map_sqlite_transaction(result)
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
                    let rows = execute_sqlite(
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
        map_sqlite_transaction(result)
    }

    /// List durable audit holds for one run in stable hold-ID order.
    pub async fn history_holds(&self, run_id: &str) -> Result<Vec<FlowHistoryHold>> {
        let rows = fetch_all_sqlite(
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
        fetch_optional_sqlite(
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

    /// Delete complete eligible terminal histories in one immediate transaction.
    ///
    /// The scan preserves durable holds and linked components, writes a checksum
    /// tombstone, and only then deletes event rows. It never performs partial
    /// stream compaction.
    pub async fn prune_terminal_history(
        &self,
        policy: FlowHistoryRetentionPolicy,
    ) -> Result<FlowHistoryRetentionReport> {
        let result = self
            .executor
            .transaction(|transaction| {
                Box::pin(async move { prune_sqlite_history(transaction, &policy).await })
            })
            .await;
        map_sqlite_transaction(result)
    }
}

async fn prune_sqlite_history(
    transaction: &SqliteTransaction,
    policy: &FlowHistoryRetentionPolicy,
) -> Result<FlowHistoryRetentionReport> {
    let rows = fetch_all_sqlite(
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

    let hold_run_ids = fetch_all_sqlite(
        transaction,
        sql_query::<String>("SELECT DISTINCT run_id FROM flow_history_holds"),
    )
    .await?
    .into_iter()
    .collect::<BTreeSet<_>>();
    let mut plan = plan_history_retention(&histories, &hold_run_ids, policy, "SQLite")?;

    for run_id in &plan.deletable_run_ids {
        let history = histories.get(run_id).ok_or_else(|| {
            FlowError::Store(format!("retention lost SQLite history for {run_id}"))
        })?;
        let terminal = history.last().ok_or_else(|| {
            FlowError::Store(format!("retention found empty SQLite history for {run_id}"))
        })?;
        let terminal_sequence = i64::try_from(terminal.sequence).map_err(|error| {
            FlowError::Store(format!(
                "terminal sequence {} for {run_id} exceeds SQLite integer range: {error}",
                terminal.sequence
            ))
        })?;
        execute_sqlite(
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
            .bind(history_checksum(history)?)
            .append(")"),
        )
        .await?;
        execute_sqlite(
            transaction,
            sql_query::<()>("DELETE FROM flow_events WHERE run_id = ").bind(run_id.clone()),
        )
        .await?;
    }

    plan.report.deleted_run_ids = plan.deletable_run_ids.into_iter().collect();
    Ok(plan.report)
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
                "invalid SQLite history hold timestamp {created_at}: {error}"
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
                "invalid SQLite history tombstone timestamp {deleted_at}: {error}"
            ))
        })?,
        terminal_sequence: u64::try_from(terminal_sequence).map_err(|error| {
            FlowError::Store(format!(
                "invalid SQLite tombstone terminal sequence {terminal_sequence}: {error}"
            ))
        })?,
        terminal_event_id: terminal_event_id.parse().map_err(|error| {
            FlowError::Store(format!(
                "invalid SQLite tombstone event id {terminal_event_id}: {error}"
            ))
        })?,
        terminal_event_key,
        history_sha256,
    })
}

pub(super) async fn ensure_sqlite_history_not_tombstoned(
    transaction: &SqliteTransaction,
    run_id: &str,
) -> Result<()> {
    let tombstoned = fetch_optional_sqlite(
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
