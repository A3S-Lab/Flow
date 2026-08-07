use a3s_orm::Migration;

#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod scheduled_wakeups;
#[cfg(feature = "postgres")]
use scheduled_wakeups::POSTGRES_SCHEDULED_WAKEUPS_SQL;
#[cfg(feature = "sqlite")]
use scheduled_wakeups::SQLITE_SCHEDULED_WAKEUPS_SQL;

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

#[cfg(feature = "sqlite")]
const SQLITE_ACTIVE_HOOKS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_active_hooks (
    run_id TEXT NOT NULL,
    hook_id TEXT NOT NULL,
    token TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_sequence BIGINT NOT NULL CHECK (created_sequence >= 1),
    PRIMARY KEY (token),
    UNIQUE (run_id, hook_id)
);

INSERT INTO flow_active_hooks (
    run_id,
    hook_id,
    token,
    metadata_json,
    created_sequence
)
SELECT
    created.run_id,
    json_extract(created.event_json, '$.hook_id'),
    json_extract(created.event_json, '$.token'),
    json_quote(json_extract(created.event_json, '$.metadata')),
    created.sequence
FROM flow_events AS created
WHERE json_extract(created.event_json, '$.type') = 'hook_created'
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = created.run_id
        AND later.sequence > created.sequence
        AND (
            (
                json_extract(later.event_json, '$.type') IN (
                    'hook_received',
                    'hook_disposed'
                )
                AND json_extract(later.event_json, '$.hook_id') =
                    json_extract(created.event_json, '$.hook_id')
            )
            OR json_extract(later.event_json, '$.type') IN (
                'run_cancellation_requested',
                'run_completed',
                'run_failed',
                'run_cancelled',
                'run_timed_out',
                'run_retry_exhausted',
                'run_host_shutdown'
            )
        )
  )
ORDER BY created.run_id, created.sequence;

CREATE TRIGGER IF NOT EXISTS flow_active_hooks_after_hook_created
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'hook_created'
BEGIN
    SELECT RAISE(ABORT, 'flow active hook token conflict')
    WHERE EXISTS (
        SELECT 1
        FROM flow_active_hooks
        WHERE token = json_extract(NEW.event_json, '$.token')
          AND (
              run_id <> NEW.run_id
              OR hook_id <> json_extract(NEW.event_json, '$.hook_id')
          )
    );

    SELECT RAISE(ABORT, 'flow active hook identity conflict')
    WHERE EXISTS (
        SELECT 1
        FROM flow_active_hooks
        WHERE run_id = NEW.run_id
          AND hook_id = json_extract(NEW.event_json, '$.hook_id')
          AND token <> json_extract(NEW.event_json, '$.token')
    );

    INSERT OR IGNORE INTO flow_active_hooks (
        run_id,
        hook_id,
        token,
        metadata_json,
        created_sequence
    ) VALUES (
        NEW.run_id,
        json_extract(NEW.event_json, '$.hook_id'),
        json_extract(NEW.event_json, '$.token'),
        json_quote(json_extract(NEW.event_json, '$.metadata')),
        NEW.sequence
    );
END;

CREATE TRIGGER IF NOT EXISTS flow_active_hooks_after_hook_closed
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') IN ('hook_received', 'hook_disposed')
BEGIN
    DELETE FROM flow_active_hooks
    WHERE run_id = NEW.run_id
      AND hook_id = json_extract(NEW.event_json, '$.hook_id');
END;

CREATE TRIGGER IF NOT EXISTS flow_active_hooks_after_run_closed
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') IN (
    'run_cancellation_requested',
    'run_completed',
    'run_failed',
    'run_cancelled',
    'run_timed_out',
    'run_retry_exhausted',
    'run_host_shutdown'
)
BEGIN
    DELETE FROM flow_active_hooks WHERE run_id = NEW.run_id;
END;
"#;

#[cfg(feature = "postgres")]
const POSTGRES_ACTIVE_HOOKS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS flow_active_hooks (
    run_id TEXT NOT NULL,
    hook_id TEXT NOT NULL,
    token TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_sequence BIGINT NOT NULL CHECK (created_sequence >= 1),
    PRIMARY KEY (run_id, hook_id)
);

CREATE INDEX IF NOT EXISTS idx_flow_active_hooks_token
ON flow_active_hooks USING HASH (token);

INSERT INTO flow_active_hooks (
    run_id,
    hook_id,
    token,
    metadata_json,
    created_sequence
)
SELECT
    created.run_id,
    created.event_json::jsonb ->> 'hook_id',
    created.event_json::jsonb ->> 'token',
    (created.event_json::jsonb -> 'metadata')::text,
    created.sequence
FROM flow_events AS created
WHERE created.event_json::jsonb ->> 'type' = 'hook_created'
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = created.run_id
        AND later.sequence > created.sequence
        AND (
            (
                later.event_json::jsonb ->> 'type' IN (
                    'hook_received',
                    'hook_disposed'
                )
                AND later.event_json::jsonb ->> 'hook_id' =
                    created.event_json::jsonb ->> 'hook_id'
            )
            OR later.event_json::jsonb ->> 'type' IN (
                'run_cancellation_requested',
                'run_completed',
                'run_failed',
                'run_cancelled',
                'run_timed_out',
                'run_retry_exhausted',
                'run_host_shutdown'
            )
        )
  )
ORDER BY created.run_id, created.sequence;

DO $$
BEGIN
    IF EXISTS (
        SELECT token
        FROM flow_active_hooks
        GROUP BY token
        HAVING COUNT(*) > 1
    ) THEN
        RAISE EXCEPTION 'existing Flow history contains duplicate active hook tokens'
            USING ERRCODE = '23505';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION a3s_flow_project_active_hook()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    event_type TEXT := NEW.event_json::jsonb ->> 'type';
    event_hook_id TEXT;
    event_token TEXT;
    existing_run_id TEXT;
    existing_hook_id TEXT;
BEGIN
    IF event_type = 'hook_created' THEN
        event_hook_id := NEW.event_json::jsonb ->> 'hook_id';
        event_token := NEW.event_json::jsonb ->> 'token';

        PERFORM pg_advisory_xact_lock(hashtext(event_token), 2);

        SELECT run_id, hook_id
        INTO existing_run_id, existing_hook_id
        FROM flow_active_hooks
        WHERE token = event_token
        ORDER BY run_id, hook_id
        LIMIT 1;

        IF FOUND AND (
            existing_run_id <> NEW.run_id
            OR existing_hook_id <> event_hook_id
        ) THEN
            RAISE EXCEPTION 'flow active hook token conflict'
                USING ERRCODE = '23505';
        END IF;

        INSERT INTO flow_active_hooks (
            run_id,
            hook_id,
            token,
            metadata_json,
            created_sequence
        ) VALUES (
            NEW.run_id,
            event_hook_id,
            event_token,
            (NEW.event_json::jsonb -> 'metadata')::text,
            NEW.sequence
        ) ON CONFLICT (run_id, hook_id) DO UPDATE
          SET token = EXCLUDED.token
        WHERE flow_active_hooks.token = EXCLUDED.token
        RETURNING flow_active_hooks.run_id INTO existing_run_id;

        IF NOT FOUND THEN
            RAISE EXCEPTION 'flow active hook identity conflict'
                USING ERRCODE = '23505';
        END IF;
    ELSIF event_type IN ('hook_received', 'hook_disposed') THEN
        DELETE FROM flow_active_hooks
        WHERE run_id = NEW.run_id
          AND hook_id = NEW.event_json::jsonb ->> 'hook_id';
    ELSIF event_type IN (
        'run_cancellation_requested',
        'run_completed',
        'run_failed',
        'run_cancelled',
        'run_timed_out',
        'run_retry_exhausted',
        'run_host_shutdown'
    ) THEN
        DELETE FROM flow_active_hooks WHERE run_id = NEW.run_id;
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS flow_active_hooks_after_event ON flow_events;

CREATE TRIGGER flow_active_hooks_after_event
AFTER INSERT ON flow_events
FOR EACH ROW
EXECUTE FUNCTION a3s_flow_project_active_hook();
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
        Migration::new(
            "a3s-flow-0003-active-hooks",
            "create the indexed active hook projection",
            SQLITE_ACTIVE_HOOKS_SQL,
        ),
        Migration::new(
            "a3s-flow-0004-scheduled-wakeups",
            "create the indexed scheduled wakeup projection",
            SQLITE_SCHEDULED_WAKEUPS_SQL,
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
        Migration::new(
            "a3s-flow-0004-active-hooks",
            "create the indexed active hook projection",
            POSTGRES_ACTIVE_HOOKS_SQL,
        ),
        Migration::new(
            "a3s-flow-0005-scheduled-wakeups",
            "reconcile active hooks and create the scheduled wakeup projection",
            POSTGRES_SCHEDULED_WAKEUPS_SQL,
        ),
    ]
}
