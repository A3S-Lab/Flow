#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SQL: &str = r#"
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

WITH open_waits AS (
    SELECT
        created.run_id,
        json_extract(created.event_json, '$.wait_id') AS subject_id,
        json_extract(created.event_json, '$.resume_at') AS scheduled_at,
        created.sequence AS created_sequence
    FROM flow_events AS created
    WHERE json_extract(created.event_json, '$.type') = 'wait_created'
      AND NOT EXISTS (
          SELECT 1
          FROM flow_events AS later
          WHERE later.run_id = created.run_id
            AND later.sequence > created.sequence
            AND (
                (
                    json_extract(later.event_json, '$.type') = 'wait_completed'
                    AND json_extract(later.event_json, '$.wait_id') =
                        json_extract(created.event_json, '$.wait_id')
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
)
INSERT INTO flow_scheduled_wakeups (
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
)
SELECT
    run_id,
    0,
    subject_id,
    CASE
        WHEN instr(scheduled_at, '.') = 0 THEN
            substr(scheduled_at, 1, length(scheduled_at) - 1) || '.000000000Z'
        ELSE
            substr(scheduled_at, 1, instr(scheduled_at, '.')) ||
            substr(
                substr(
                    scheduled_at,
                    instr(scheduled_at, '.') + 1,
                    length(scheduled_at) - instr(scheduled_at, '.') - 1
                ) || '000000000',
                1,
                9
            ) || 'Z'
    END,
    created_sequence
FROM open_waits
ORDER BY run_id, created_sequence;

WITH open_retries AS (
    SELECT
        retrying.run_id,
        json_extract(retrying.event_json, '$.step_id') AS subject_id,
        json_extract(retrying.event_json, '$.retry_after') AS scheduled_at,
        retrying.sequence AS created_sequence
    FROM flow_events AS retrying
    WHERE json_extract(retrying.event_json, '$.type') = 'step_retrying'
      AND json_extract(retrying.event_json, '$.retry_after') IS NOT NULL
      AND NOT EXISTS (
          SELECT 1
          FROM flow_events AS later
          WHERE later.run_id = retrying.run_id
            AND later.sequence > retrying.sequence
            AND (
                (
                    json_extract(later.event_json, '$.type') IN (
                        'step_started',
                        'step_completed',
                        'step_failed'
                    )
                    AND json_extract(later.event_json, '$.step_id') =
                        json_extract(retrying.event_json, '$.step_id')
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
)
INSERT INTO flow_scheduled_wakeups (
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
)
SELECT
    run_id,
    2,
    subject_id,
    CASE
        WHEN instr(scheduled_at, '.') = 0 THEN
            substr(scheduled_at, 1, length(scheduled_at) - 1) || '.000000000Z'
        ELSE
            substr(scheduled_at, 1, instr(scheduled_at, '.')) ||
            substr(
                substr(
                    scheduled_at,
                    instr(scheduled_at, '.') + 1,
                    length(scheduled_at) - instr(scheduled_at, '.') - 1
                ) || '000000000',
                1,
                9
            ) || 'Z'
    END,
    created_sequence
FROM open_retries
ORDER BY run_id, created_sequence;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_event
AFTER INSERT ON flow_events
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND (
          json_extract(NEW.event_json, '$.type') IN (
              'run_cancellation_requested',
              'run_completed',
              'run_failed',
              'run_cancelled',
              'run_timed_out',
              'run_retry_exhausted',
              'run_host_shutdown'
          )
          OR (
              wakeup_kind = 0
              AND json_extract(NEW.event_json, '$.type') = 'wait_completed'
              AND subject_id = json_extract(NEW.event_json, '$.wait_id')
          )
          OR (
              wakeup_kind = 2
              AND json_extract(NEW.event_json, '$.type') IN (
                  'step_started',
                  'step_completed',
                  'step_failed',
                  'step_retrying'
              )
              AND subject_id = json_extract(NEW.event_json, '$.step_id')
          )
      );

    INSERT INTO flow_scheduled_wakeups (
        run_id,
        wakeup_kind,
        subject_id,
        scheduled_at_key,
        created_sequence
    )
    SELECT
        NEW.run_id,
        0,
        json_extract(NEW.event_json, '$.wait_id'),
        CASE
            WHEN instr(json_extract(NEW.event_json, '$.resume_at'), '.') = 0 THEN
                substr(
                    json_extract(NEW.event_json, '$.resume_at'),
                    1,
                    length(json_extract(NEW.event_json, '$.resume_at')) - 1
                ) || '.000000000Z'
            ELSE
                substr(
                    json_extract(NEW.event_json, '$.resume_at'),
                    1,
                    instr(json_extract(NEW.event_json, '$.resume_at'), '.')
                ) ||
                substr(
                    substr(
                        json_extract(NEW.event_json, '$.resume_at'),
                        instr(json_extract(NEW.event_json, '$.resume_at'), '.') + 1,
                        length(json_extract(NEW.event_json, '$.resume_at')) -
                            instr(json_extract(NEW.event_json, '$.resume_at'), '.') - 1
                    ) || '000000000',
                    1,
                    9
                ) || 'Z'
        END,
        NEW.sequence
    WHERE json_extract(NEW.event_json, '$.type') = 'wait_created'
    ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
        scheduled_at_key = excluded.scheduled_at_key,
        created_sequence = excluded.created_sequence;

    INSERT INTO flow_scheduled_wakeups (
        run_id,
        wakeup_kind,
        subject_id,
        scheduled_at_key,
        created_sequence
    )
    SELECT
        NEW.run_id,
        2,
        json_extract(NEW.event_json, '$.step_id'),
        CASE
            WHEN instr(json_extract(NEW.event_json, '$.retry_after'), '.') = 0 THEN
                substr(
                    json_extract(NEW.event_json, '$.retry_after'),
                    1,
                    length(json_extract(NEW.event_json, '$.retry_after')) - 1
                ) || '.000000000Z'
            ELSE
                substr(
                    json_extract(NEW.event_json, '$.retry_after'),
                    1,
                    instr(json_extract(NEW.event_json, '$.retry_after'), '.')
                ) ||
                substr(
                    substr(
                        json_extract(NEW.event_json, '$.retry_after'),
                        instr(json_extract(NEW.event_json, '$.retry_after'), '.') + 1,
                        length(json_extract(NEW.event_json, '$.retry_after')) -
                            instr(json_extract(NEW.event_json, '$.retry_after'), '.') - 1
                    ) || '000000000',
                    1,
                    9
                ) || 'Z'
        END,
        NEW.sequence
    WHERE json_extract(NEW.event_json, '$.type') = 'step_retrying'
      AND json_extract(NEW.event_json, '$.retry_after') IS NOT NULL
    ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
        scheduled_at_key = excluded.scheduled_at_key,
        created_sequence = excluded.created_sequence;
END;
"#;

/// Incremental projection migration for the `step_cancelled` event added by
/// batch terminal-settlement hardening. Keeping this as a new migration (and
/// not editing the published trigger above) preserves checksum compatibility
/// for databases that already applied the earlier schema.
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_CANCELLATION_SQL: &str = r#"
-- A process may have committed a retry wakeup before it had to cancel an
-- ambiguous sibling. Reconcile those histories before installing the trigger.
DELETE FROM flow_scheduled_wakeups
WHERE wakeup_kind = 2
  AND EXISTS (
      SELECT 1
      FROM flow_events AS cancelled
      WHERE cancelled.run_id = flow_scheduled_wakeups.run_id
        AND cancelled.sequence > flow_scheduled_wakeups.created_sequence
        AND json_extract(cancelled.event_json, '$.type') = 'step_cancelled'
        AND json_extract(cancelled.event_json, '$.step_id') =
            flow_scheduled_wakeups.subject_id
  );

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_step_cancelled
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'step_cancelled'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 2
      AND subject_id = json_extract(NEW.event_json, '$.step_id');
END;
"#;
