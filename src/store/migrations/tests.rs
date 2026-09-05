use super::*;
use sha2::{Digest, Sha256};

#[cfg(any(feature = "postgres", feature = "sqlite"))]
use crate::model::FlowEvent;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use crate::store::FlowEventStore;
#[cfg(feature = "postgres")]
use crate::store::PostgresEventStore;
#[cfg(feature = "sqlite")]
use crate::store::SqliteEventStore;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use a3s_orm::{sql_query, Database, Migrator};
#[cfg(feature = "postgres")]
use a3s_orm::{PostgresDialect, PostgresExecutor};
#[cfg(feature = "sqlite")]
use a3s_orm::{SqliteDialect, SqliteExecutor};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use chrono::{DateTime, Utc};
#[cfg(any(feature = "postgres", feature = "sqlite"))]
use serde_json::json;
#[cfg(feature = "postgres")]
use uuid::Uuid;

const EVENTS_CHECKSUM: &str = "65eb745fe0bd3137c38265bc73b965dace92d80cecc409268365a93c043b060f";
const RETENTION_CHECKSUM: &str = "958b010364cde3787e046a6bb85ce1ddc7d048624dad36bd39d85cc10664f5a0";
#[cfg(feature = "sqlite")]
const SQLITE_ACTIVE_HOOKS_CHECKSUM: &str =
    "6015035269c1890f397f2d0d27a9f7dd57119439fef4850a95ab3cb9d266802f";
#[cfg(feature = "sqlite")]
const SQLITE_SCHEDULED_WAKEUPS_CHECKSUM: &str =
    "20731e3c54a7c2b957e75a0853939571490fd053350e49b31b7a69184b3e27c9";
#[cfg(feature = "sqlite")]
const SQLITE_CONTINUE_AS_NEW_CHECKSUM: &str =
    "abd8e02b3da89fa50630ae951aa0e44e136b3c3f058eeb0c857dff78b84f4c57";
#[cfg(feature = "sqlite")]
const SQLITE_SCHEDULED_WAKEUPS_CANCELLATION_CHECKSUM: &str =
    "8d09073d28b842385d22b12720bb3fbfc728935daefd37e6ccf198f34705af5c";
const EVENT_ENVELOPE_SCHEMA_CHECKSUM: &str =
    "66b66243437158aaed793e75423b8ce8141c09bb5cc8f983bc4bce1a5b70bd09";
#[cfg(feature = "postgres")]
const POSTGRES_TASKS_CHECKSUM: &str =
    "6e73f57af6f836508a12b66eb8a00130ca0fbc62c99ca1a0bfa8c36f0b9d5b93";
#[cfg(feature = "postgres")]
const POSTGRES_ACTIVE_HOOKS_CHECKSUM: &str =
    "53616594237e1197443ed8dce7e5363856df44d306562e86988d10c7410badc3";
#[cfg(feature = "postgres")]
const POSTGRES_SCHEDULED_WAKEUPS_CHECKSUM: &str =
    "ce1d93170778989d526a508b4e8bd42be6e2d14bccfaed1a24b00492cad318da";
#[cfg(feature = "postgres")]
const POSTGRES_CONTINUE_AS_NEW_CHECKSUM: &str =
    "e22ca39f110328b4b83023f306b426e15d55c5b1ff378cce3e30b4d9e8726dc0";
#[cfg(feature = "postgres")]
const POSTGRES_SCHEDULED_WAKEUPS_CANCELLATION_CHECKSUM: &str =
    "23d0a21b972d67f6edb4fe27607847bb2e743055ede9e6dbd80deb8224203cba";

#[cfg(any(feature = "postgres", feature = "sqlite"))]
const LEGACY_EVENT_JSON: [&str; 4] = [
    r#"{"type":"run_created","spec":{"name":"release.schema-upgrade","version":"1","runtime":{"kind":"rust_embedded","entrypoint":"tests::schema_upgrade","export_name":"main"}},"input":{"release":"pre-v1"}}"#,
    r#"{"type":"run_started"}"#,
    r#"{"type":"hook_created","hook_id":"approval","token":"retained-upgrade-token","metadata":{"release":"pre-v1"}}"#,
    r#"{"type":"wait_created","wait_id":"release-timer","resume_at":"2200-08-19T00:00:00Z"}"#,
];

fn checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

fn migration_rows(migrations: &[Migration]) -> Vec<(String, String)> {
    migrations
        .iter()
        .map(|migration| {
            (
                migration.version().to_string(),
                checksum(migration.up_sql()),
            )
        })
        .collect()
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("fixture timestamp")
        .with_timezone(&Utc)
}

#[cfg(any(feature = "postgres", feature = "sqlite"))]
fn assert_upgraded_history(history: &[crate::model::FlowEventEnvelope]) {
    assert_eq!(history.len(), LEGACY_EVENT_JSON.len());
    let FlowEvent::RunCreated { spec, .. } = &history[0].event else {
        panic!("legacy history must start with run_created");
    };
    assert!(spec.runtime_build_id.is_none());
    assert!(spec.patch_markers.is_empty());
    assert!(spec.signal_names.is_empty());
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_migrations_keep_every_published_checksum() {
    let migrations = sqlite_migrations();
    assert_eq!(
        migration_rows(&migrations),
        vec![
            ("a3s-flow-0001-events".into(), EVENTS_CHECKSUM.into()),
            ("a3s-flow-0002-retention".into(), RETENTION_CHECKSUM.into()),
            (
                "a3s-flow-0003-active-hooks".into(),
                SQLITE_ACTIVE_HOOKS_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0004-scheduled-wakeups".into(),
                SQLITE_SCHEDULED_WAKEUPS_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0005-continue-as-new".into(),
                SQLITE_CONTINUE_AS_NEW_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0006-step-cancellation-wakeup".into(),
                SQLITE_SCHEDULED_WAKEUPS_CANCELLATION_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0007-event-envelope-schema".into(),
                EVENT_ENVELOPE_SCHEMA_CHECKSUM.into(),
            ),
        ]
    );
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_migrations_keep_every_published_checksum() {
    let migrations = postgres_migrations();
    assert_eq!(
        migration_rows(&migrations),
        vec![
            ("a3s-flow-0001-events".into(), EVENTS_CHECKSUM.into()),
            ("a3s-flow-0002-tasks".into(), POSTGRES_TASKS_CHECKSUM.into(),),
            ("a3s-flow-0003-retention".into(), RETENTION_CHECKSUM.into()),
            (
                "a3s-flow-0004-active-hooks".into(),
                POSTGRES_ACTIVE_HOOKS_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0005-scheduled-wakeups".into(),
                POSTGRES_SCHEDULED_WAKEUPS_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0006-continue-as-new".into(),
                POSTGRES_CONTINUE_AS_NEW_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0007-step-cancellation-wakeup".into(),
                POSTGRES_SCHEDULED_WAKEUPS_CANCELLATION_CHECKSUM.into(),
            ),
            (
                "a3s-flow-0008-event-envelope-schema".into(),
                EVENT_ENVELOPE_SCHEMA_CHECKSUM.into(),
            ),
        ]
    );
}

#[cfg(feature = "sqlite")]
async fn insert_sqlite_legacy_history(executor: &SqliteExecutor, run_id: &str) {
    let database = Database::new(SqliteDialect, executor.clone());
    for (index, event_json) in LEGACY_EVENT_JSON.iter().enumerate() {
        let sequence = i64::try_from(index + 1).unwrap();
        database
            .execute(
                sql_query::<()>(
                    "INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json) VALUES (",
                )
                .bind(run_id)
                .append(", ")
                .bind(sequence)
                .append(", ")
                .bind(format!("00000000-0000-4000-9000-{sequence:012}"))
                .append(", ")
                .bind(format!("2026-08-19T00:00:0{sequence}Z"))
                .append(", ")
                .bind(*event_json)
                .append(")"),
            )
            .await
            .unwrap();
    }
}

#[cfg(feature = "sqlite")]
async fn sqlite_applied_migrations(executor: &SqliteExecutor) -> Vec<(String, String)> {
    Database::new(SqliteDialect, executor.clone())
        .fetch_all_as(sql_query::<(String, String)>(
            "SELECT version, checksum FROM a3s_orm_migrations ORDER BY version",
        ))
        .await
        .unwrap()
        .rows
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_upgrades_every_supported_pre_v1_schema_baseline() {
    let baselines = [
        ("v0.5.0", 1usize),
        ("v0.6.0-v0.7.1", 2),
        ("v0.8.0", 3),
        ("v0.9.0-v0.13.1", 4),
    ];
    let migrations = sqlite_migrations();
    let expected_rows = migration_rows(&migrations);

    for (release, prefix_len) in baselines {
        let executor = SqliteExecutor::open_in_memory().await.unwrap();
        Migrator::new(executor.clone())
            .run(migrations[..prefix_len].iter().cloned())
            .await
            .unwrap();
        let run_id = format!("sqlite-upgrade-{release}");
        insert_sqlite_legacy_history(&executor, &run_id).await;

        let store = SqliteEventStore::from_executor(executor.clone())
            .await
            .unwrap();
        assert_eq!(sqlite_applied_migrations(&executor).await, expected_rows);
        assert_upgraded_history(&store.list(&run_id).await.unwrap());
        assert!(store
            .list_active_hooks()
            .await
            .unwrap()
            .iter()
            .any(|hook| hook.run_id == run_id && hook.hook.hook_id == "approval"));
        assert!(store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .iter()
            .any(|wakeup| wakeup.run_id == run_id && wakeup.subject_id == "release-timer"));

        let successor_run_id = format!("{run_id}-successor").replace('.', "-");
        store
            .append(
                &run_id,
                FlowEvent::RunContinuedAsNew {
                    successor_run_id,
                    input: json!({ "generation": 1 }),
                },
            )
            .await
            .unwrap();
        assert!(store
            .list_active_hooks()
            .await
            .unwrap()
            .iter()
            .all(|hook| hook.run_id != run_id));
        assert!(store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .iter()
            .all(|wakeup| wakeup.run_id != run_id));
    }
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_failed_migration_rolls_back_schema_and_preserves_history() {
    let executor = SqliteExecutor::open_in_memory().await.unwrap();
    let migrations = sqlite_migrations();
    Migrator::new(executor.clone())
        .run(migrations[..1].iter().cloned())
        .await
        .unwrap();
    insert_sqlite_legacy_history(&executor, "sqlite-failed-upgrade").await;
    let mut invalid = migrations[..1].to_vec();
    invalid.push(Migration::new(
        "a3s-flow-9999-invalid",
        "exercise migration transaction rollback",
        "CREATE TABLE flow_migration_rollback_probe (id INTEGER); invalid SQL;",
    ));

    assert!(Migrator::new(executor.clone()).run(invalid).await.is_err());
    assert_eq!(sqlite_applied_migrations(&executor).await.len(), 1);
    let probe_tables = Database::new(SqliteDialect, executor.clone())
        .fetch_all_as(
            sql_query::<String>("SELECT name FROM sqlite_master WHERE type = 'table' AND name = ")
                .bind("flow_migration_rollback_probe"),
        )
        .await
        .unwrap()
        .rows;
    assert!(probe_tables.is_empty());
    let store = SqliteEventStore::from_executor(executor).await.unwrap();
    assert_upgraded_history(&store.list("sqlite-failed-upgrade").await.unwrap());
}

#[cfg(feature = "postgres")]
fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

#[cfg(feature = "postgres")]
fn schema_scoped_url(postgres_url: &str, schema: &str) -> String {
    let separator = if postgres_url.contains('?') { '&' } else { '?' };
    format!("{postgres_url}{separator}options=-csearch_path%3D{schema}")
}

#[cfg(feature = "postgres")]
async fn insert_postgres_legacy_history(executor: &PostgresExecutor, run_id: &str) {
    let database = Database::new(PostgresDialect, executor.clone());
    for (index, event_json) in LEGACY_EVENT_JSON.iter().enumerate() {
        let sequence = i64::try_from(index + 1).unwrap();
        database
            .execute(
                sql_query::<()>(
                    "INSERT INTO flow_events (run_id, sequence, event_id, timestamp, event_json) VALUES (",
                )
                .bind(run_id)
                .append(", ")
                .bind(sequence)
                .append(", ")
                .bind(format!("00000000-0000-4000-a000-{sequence:012}"))
                .append(", ")
                .bind(format!("2026-08-19T00:00:0{sequence}Z"))
                .append(", ")
                .bind(*event_json)
                .append(")"),
            )
            .await
            .unwrap();
    }
}

#[cfg(feature = "postgres")]
async fn postgres_applied_migrations(executor: &PostgresExecutor) -> Vec<(String, String)> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(sql_query::<(String, String)>(
            "SELECT version, checksum FROM a3s_orm_migrations ORDER BY version",
        ))
        .await
        .unwrap()
        .rows
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_upgrades_every_supported_pre_v1_schema_baseline() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let baselines = [
        ("v0.5.0-v0.7.1", 3usize),
        ("v0.8.0", 4),
        ("v0.9.0-v0.13.1", 5),
    ];
    let migrations = postgres_migrations();
    let expected_rows = migration_rows(&migrations);
    let base_executor = PostgresExecutor::connect_no_tls(&postgres_url, 2).unwrap();

    for (release, prefix_len) in baselines {
        let schema = format!("flow_v1_upgrade_{}", Uuid::new_v4().simple());
        base_executor
            .connection()
            .await
            .unwrap()
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let scoped_executor =
            PostgresExecutor::connect_no_tls(&schema_scoped_url(&postgres_url, &schema), 2)
                .unwrap();
        Migrator::new(scoped_executor.clone())
            .run(migrations[..prefix_len].iter().cloned())
            .await
            .unwrap();
        let run_id = format!("postgres-upgrade-{release}");
        insert_postgres_legacy_history(&scoped_executor, &run_id).await;

        let store = PostgresEventStore::from_executor(scoped_executor.clone())
            .await
            .unwrap();
        assert_eq!(
            postgres_applied_migrations(&scoped_executor).await,
            expected_rows
        );
        assert_upgraded_history(&store.list(&run_id).await.unwrap());
        assert!(store
            .list_active_hooks()
            .await
            .unwrap()
            .iter()
            .any(|hook| hook.run_id == run_id && hook.hook.hook_id == "approval"));
        assert!(store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .iter()
            .any(|wakeup| wakeup.run_id == run_id && wakeup.subject_id == "release-timer"));

        store
            .append(
                &run_id,
                FlowEvent::RunContinuedAsNew {
                    successor_run_id: format!("{run_id}-successor"),
                    input: json!({ "generation": 1 }),
                },
            )
            .await
            .unwrap();
        assert!(store
            .list_active_hooks()
            .await
            .unwrap()
            .iter()
            .all(|hook| hook.run_id != run_id));
        assert!(store
            .list_due_wakeups(timestamp("2300-01-01T00:00:00Z"))
            .await
            .unwrap()
            .iter()
            .all(|wakeup| wakeup.run_id != run_id));

        drop(store);
        drop(scoped_executor);
        base_executor
            .connection()
            .await
            .unwrap()
            .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_failed_migration_rolls_back_schema_and_preserves_history() {
    let Some(postgres_url) = postgres_url_from_env() else {
        return;
    };
    let schema = format!("flow_v1_rollback_{}", Uuid::new_v4().simple());
    let base_executor = PostgresExecutor::connect_no_tls(&postgres_url, 2).unwrap();
    base_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();
    let scoped_executor =
        PostgresExecutor::connect_no_tls(&schema_scoped_url(&postgres_url, &schema), 2).unwrap();
    let migrations = postgres_migrations();
    Migrator::new(scoped_executor.clone())
        .run(migrations[..3].iter().cloned())
        .await
        .unwrap();
    insert_postgres_legacy_history(&scoped_executor, "postgres-failed-upgrade").await;
    let mut invalid = migrations[..3].to_vec();
    invalid.push(Migration::new(
        "a3s-flow-9999-invalid",
        "exercise migration transaction rollback",
        "CREATE TABLE flow_migration_rollback_probe (id BIGINT); invalid SQL;",
    ));

    assert!(Migrator::new(scoped_executor.clone())
        .run(invalid)
        .await
        .is_err());
    assert_eq!(postgres_applied_migrations(&scoped_executor).await.len(), 3);
    let probe_tables = Database::new(PostgresDialect, scoped_executor.clone())
        .fetch_all_as(
            sql_query::<String>(
                "SELECT table_name FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = ",
            )
            .bind("flow_migration_rollback_probe"),
        )
        .await
        .unwrap()
        .rows;
    assert!(probe_tables.is_empty());
    let store = PostgresEventStore::from_executor(scoped_executor.clone())
        .await
        .unwrap();
    assert_upgraded_history(&store.list("postgres-failed-upgrade").await.unwrap());

    drop(store);
    drop(scoped_executor);
    base_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}
