use a3s_orm::Migration;

const EVENTS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_events (
    run_id TEXT NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence >= 1),
    event_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    event_json TEXT NOT NULL,
    PRIMARY KEY (run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_flow_events_run_id_sequence
ON flow_events (run_id, sequence);
"#;

#[cfg(feature = "postgres")]
const POSTGRES_TASKS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_tasks (
    queue_name TEXT NOT NULL,
    task_id TEXT NOT NULL,
    task_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'inflight')),
    enqueued_at_nanos BIGINT NOT NULL,
    leased_at_nanos BIGINT,
    lease_id TEXT,
    updated_at_nanos BIGINT NOT NULL,
    PRIMARY KEY (queue_name, task_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_flow_tasks_queue_lease
ON flow_tasks (queue_name, lease_id)
WHERE lease_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_flow_tasks_pending_order
ON flow_tasks (queue_name, status, enqueued_at_nanos, task_id);

CREATE TABLE IF NOT EXISTS flow_task_dead_letters (
    queue_name TEXT NOT NULL,
    dead_letter_id TEXT NOT NULL,
    lease_id TEXT NOT NULL,
    task_json TEXT NOT NULL,
    reason TEXT NOT NULL,
    dead_lettered_at_nanos BIGINT NOT NULL,
    leased_at_nanos BIGINT,
    PRIMARY KEY (queue_name, dead_letter_id)
);

CREATE INDEX IF NOT EXISTS idx_flow_task_dead_letters_queue_time
ON flow_task_dead_letters (queue_name, dead_lettered_at_nanos, dead_letter_id);
"#;

#[cfg(any(feature = "postgres", feature = "sqlite"))]
const RETENTION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_history_holds (
    run_id TEXT NOT NULL,
    hold_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (run_id, hold_id)
);

CREATE TABLE IF NOT EXISTS flow_history_tombstones (
    run_id TEXT PRIMARY KEY,
    deleted_at TEXT NOT NULL,
    terminal_sequence BIGINT NOT NULL CHECK (terminal_sequence >= 1),
    terminal_event_id TEXT NOT NULL,
    terminal_event_key TEXT NOT NULL,
    history_sha256 TEXT NOT NULL
);
"#;

#[cfg(feature = "sqlite")]
pub(crate) fn sqlite_migrations() -> Vec<Migration> {
    vec![
        Migration::new(
            "a3s-flow-0001-events",
            "create Flow event history",
            EVENTS_SQL,
        ),
        Migration::new(
            "a3s-flow-0002-retention",
            "create Flow history retention guards and tombstones",
            RETENTION_SQL,
        ),
    ]
}

#[cfg(feature = "postgres")]
pub(crate) fn postgres_migrations() -> Vec<Migration> {
    vec![
        Migration::new(
            "a3s-flow-0001-events",
            "create Flow event history",
            EVENTS_SQL,
        ),
        Migration::new(
            "a3s-flow-0002-tasks",
            "create Flow task dispatch tables",
            POSTGRES_TASKS_SQL,
        ),
        Migration::new(
            "a3s-flow-0003-retention",
            "create Flow history retention guards and tombstones",
            RETENTION_SQL,
        ),
    ]
}
