#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SQL: &str = r#"
LOCK TABLE flow_events IN SHARE ROW EXCLUSIVE MODE;

-- Reconcile the v0.8 active-hook projection while event inserts are blocked.
-- This closes the narrow backfill/trigger-install gap for rolling-upgrade
-- writers that did not yet participate in the ORM migration lock.
DELETE FROM flow_active_hooks;

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

CREATE OR REPLACE FUNCTION a3s_flow_normalize_wakeup_timestamp(timestamp_text TEXT)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
    dot_position INTEGER := strpos(timestamp_text, '.');
    fraction TEXT;
BEGIN
    IF right(timestamp_text, 1) <> 'Z' THEN
        RAISE EXCEPTION 'Flow scheduled wakeup timestamp must use the UTC Z suffix'
            USING ERRCODE = '22007';
    END IF;
    IF dot_position = 0 THEN
        RETURN left(timestamp_text, length(timestamp_text) - 1) || '.000000000Z';
    END IF;

    fraction := substring(
        timestamp_text
        FROM dot_position + 1
        FOR length(timestamp_text) - dot_position - 1
    );
    IF fraction !~ '^[0-9]{1,9}$' THEN
        RAISE EXCEPTION 'Flow scheduled wakeup timestamp has invalid fractional seconds'
            USING ERRCODE = '22007';
    END IF;
    RETURN left(timestamp_text, dot_position) ||
        left(rpad(fraction, 9, '0'), 9) || 'Z';
END;
$$;

CREATE TABLE IF NOT EXISTS flow_scheduled_wakeups (
    run_id TEXT NOT NULL,
    wakeup_kind BIGINT NOT NULL CHECK (wakeup_kind IN (0, 2)),
    subject_id TEXT NOT NULL,
    scheduled_at_key TEXT NOT NULL,
    created_sequence BIGINT NOT NULL CHECK (created_sequence >= 1),
    PRIMARY KEY (run_id, wakeup_kind, subject_id)
);

CREATE INDEX IF NOT EXISTS idx_flow_scheduled_wakeups_due
ON flow_scheduled_wakeups (
    scheduled_at_key,
    wakeup_kind,
    run_id,
    subject_id
);

CREATE INDEX IF NOT EXISTS idx_flow_scheduled_wakeups_next
ON flow_scheduled_wakeups (
    scheduled_at_key,
    run_id,
    wakeup_kind,
    subject_id
);

INSERT INTO flow_scheduled_wakeups (
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
)
SELECT
    created.run_id,
    0,
    created.event_json::jsonb ->> 'wait_id',
    a3s_flow_normalize_wakeup_timestamp(
        created.event_json::jsonb ->> 'resume_at'
    ),
    created.sequence
FROM flow_events AS created
WHERE created.event_json::jsonb ->> 'type' = 'wait_created'
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = created.run_id
        AND later.sequence > created.sequence
        AND (
            (
                later.event_json::jsonb ->> 'type' = 'wait_completed'
                AND later.event_json::jsonb ->> 'wait_id' =
                    created.event_json::jsonb ->> 'wait_id'
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

INSERT INTO flow_scheduled_wakeups (
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
)
SELECT
    retrying.run_id,
    2,
    retrying.event_json::jsonb ->> 'step_id',
    a3s_flow_normalize_wakeup_timestamp(
        retrying.event_json::jsonb ->> 'retry_after'
    ),
    retrying.sequence
FROM flow_events AS retrying
WHERE retrying.event_json::jsonb ->> 'type' = 'step_retrying'
  AND retrying.event_json::jsonb ->> 'retry_after' IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = retrying.run_id
        AND later.sequence > retrying.sequence
        AND (
            (
                later.event_json::jsonb ->> 'type' IN (
                    'step_started',
                    'step_completed',
                    'step_failed'
                )
                AND later.event_json::jsonb ->> 'step_id' =
                    retrying.event_json::jsonb ->> 'step_id'
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
ORDER BY retrying.run_id, retrying.sequence;

CREATE OR REPLACE FUNCTION a3s_flow_project_scheduled_wakeup()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    event_type TEXT := NEW.event_json::jsonb ->> 'type';
    event_subject_id TEXT;
    event_scheduled_at TEXT;
BEGIN
    IF event_type = 'wait_created' THEN
        event_subject_id := NEW.event_json::jsonb ->> 'wait_id';
        event_scheduled_at := a3s_flow_normalize_wakeup_timestamp(
            NEW.event_json::jsonb ->> 'resume_at'
        );
        INSERT INTO flow_scheduled_wakeups (
            run_id,
            wakeup_kind,
            subject_id,
            scheduled_at_key,
            created_sequence
        ) VALUES (
            NEW.run_id,
            0,
            event_subject_id,
            event_scheduled_at,
            NEW.sequence
        ) ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
            scheduled_at_key = EXCLUDED.scheduled_at_key,
            created_sequence = EXCLUDED.created_sequence;
    ELSIF event_type = 'wait_completed' THEN
        DELETE FROM flow_scheduled_wakeups
        WHERE run_id = NEW.run_id
          AND wakeup_kind = 0
          AND subject_id = NEW.event_json::jsonb ->> 'wait_id';
    ELSIF event_type = 'step_retrying' THEN
        event_subject_id := NEW.event_json::jsonb ->> 'step_id';
        DELETE FROM flow_scheduled_wakeups
        WHERE run_id = NEW.run_id
          AND wakeup_kind = 2
          AND subject_id = event_subject_id;

        IF NEW.event_json::jsonb ->> 'retry_after' IS NOT NULL THEN
            event_scheduled_at := a3s_flow_normalize_wakeup_timestamp(
                NEW.event_json::jsonb ->> 'retry_after'
            );
            INSERT INTO flow_scheduled_wakeups (
                run_id,
                wakeup_kind,
                subject_id,
                scheduled_at_key,
                created_sequence
            ) VALUES (
                NEW.run_id,
                2,
                event_subject_id,
                event_scheduled_at,
                NEW.sequence
            );
        END IF;
    ELSIF event_type IN ('step_started', 'step_completed', 'step_failed') THEN
        DELETE FROM flow_scheduled_wakeups
        WHERE run_id = NEW.run_id
          AND wakeup_kind = 2
          AND subject_id = NEW.event_json::jsonb ->> 'step_id';
    ELSIF event_type IN (
        'run_cancellation_requested',
        'run_completed',
        'run_failed',
        'run_cancelled',
        'run_timed_out',
        'run_retry_exhausted',
        'run_host_shutdown'
    ) THEN
        DELETE FROM flow_scheduled_wakeups WHERE run_id = NEW.run_id;
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_event ON flow_events;

CREATE TRIGGER flow_scheduled_wakeups_after_event
AFTER INSERT ON flow_events
FOR EACH ROW
EXECUTE FUNCTION a3s_flow_project_scheduled_wakeup();
"#;

/// Incremental projection migration for the `step_cancelled` event added by
/// batch terminal-settlement hardening. The published scheduled-wakeup
/// migration remains immutable so existing checksum ledgers continue to
/// validate during rolling upgrades.
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_CANCELLATION_SQL: &str = r#"
LOCK TABLE flow_events IN SHARE ROW EXCLUSIVE MODE;

DELETE FROM flow_scheduled_wakeups AS wakeup
WHERE wakeup.wakeup_kind = 2
  AND EXISTS (
      SELECT 1
      FROM flow_events AS cancelled
      WHERE cancelled.run_id = wakeup.run_id
        AND cancelled.sequence > wakeup.created_sequence
        AND cancelled.event_json::jsonb ->> 'type' = 'step_cancelled'
        AND cancelled.event_json::jsonb ->> 'step_id' = wakeup.subject_id
  );

CREATE OR REPLACE FUNCTION a3s_flow_cancel_scheduled_wakeup()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.event_json::jsonb ->> 'type' = 'step_cancelled' THEN
        DELETE FROM flow_scheduled_wakeups
        WHERE run_id = NEW.run_id
          AND wakeup_kind = 2
          AND subject_id = NEW.event_json::jsonb ->> 'step_id';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_step_cancelled ON flow_events;

CREATE TRIGGER flow_scheduled_wakeups_after_step_cancelled
AFTER INSERT ON flow_events
FOR EACH ROW
EXECUTE FUNCTION a3s_flow_cancel_scheduled_wakeup();
"#;
