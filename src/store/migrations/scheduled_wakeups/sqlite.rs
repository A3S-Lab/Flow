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

/// Indexes delayed activity retries the same way delayed step retries are
/// indexed. Published triggers stay untouched so already-applied checksums
/// remain valid.
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_RETRY_SQL: &str = r#"
WITH open_activity_retries AS (
    SELECT
        retrying.run_id,
        json_extract(retrying.event_json, '$.activity_id') AS subject_id,
        json_extract(retrying.event_json, '$.retry_after') AS scheduled_at,
        retrying.sequence AS created_sequence
    FROM flow_events AS retrying
    WHERE json_extract(retrying.event_json, '$.type') = 'activity_retrying'
      AND json_extract(retrying.event_json, '$.retry_after') IS NOT NULL
      AND NOT EXISTS (
          SELECT 1
          FROM flow_events AS later
          WHERE later.run_id = retrying.run_id
            AND later.sequence > retrying.sequence
            AND (
                (
                    json_extract(later.event_json, '$.type') IN (
                        'activity_started',
                        'activity_completed',
                        'activity_failed',
                        'activity_non_retryable',
                        'activity_unknown',
                        'activity_cancelled'
                    )
                    AND json_extract(later.event_json, '$.activity_id') =
                        json_extract(retrying.event_json, '$.activity_id')
                )
                OR (
                    json_extract(later.event_json, '$.type') = 'activity_retrying'
                    AND json_extract(later.event_json, '$.activity_id') =
                        json_extract(retrying.event_json, '$.activity_id')
                )
                OR json_extract(later.event_json, '$.type') IN (
                    'run_cancellation_requested',
                    'run_completed',
                    'run_failed',
                    'run_cancelled',
                    'run_timed_out',
                    'run_retry_exhausted',
                    'run_host_shutdown',
                    'run_continued_as_new'
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
FROM open_activity_retries
ORDER BY run_id, created_sequence
ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
    scheduled_at_key = excluded.scheduled_at_key,
    created_sequence = excluded.created_sequence;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_activity_retrying
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'activity_retrying'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 2
      AND subject_id = json_extract(NEW.event_json, '$.activity_id');

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
        json_extract(NEW.event_json, '$.activity_id'),
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
    WHERE json_extract(NEW.event_json, '$.retry_after') IS NOT NULL
    ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
        scheduled_at_key = excluded.scheduled_at_key,
        created_sequence = excluded.created_sequence;
END;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_activity_closed
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') IN (
    'activity_started',
    'activity_completed',
    'activity_failed',
    'activity_non_retryable',
    'activity_unknown',
    'activity_cancelled'
)
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 2
      AND subject_id = json_extract(NEW.event_json, '$.activity_id');
END;
"#;

/// Separates activity retries onto reserved wakeup kind 1 so a step and an
/// activity that share a subject id can both remain indexed.
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_KIND_SQL: &str = r#"
-- Drop writers before rebuilding the table; SQLite keeps cross-table triggers
-- alive across DROP TABLE and they fail while the name is missing.
DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_event;
DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_step_cancelled;
DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_activity_retrying;
DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_activity_closed;
DROP TRIGGER IF EXISTS flow_continue_as_new_cleanup_after_event;

CREATE TABLE flow_scheduled_wakeups_activity_kind (
    run_id TEXT NOT NULL,
    wakeup_kind BIGINT NOT NULL CHECK (wakeup_kind IN (0, 1, 2)),
    subject_id TEXT NOT NULL,
    scheduled_at_key TEXT NOT NULL,
    created_sequence BIGINT NOT NULL CHECK (created_sequence >= 1),
    PRIMARY KEY (run_id, wakeup_kind, subject_id)
);

INSERT INTO flow_scheduled_wakeups_activity_kind (
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
)
SELECT
    run_id,
    wakeup_kind,
    subject_id,
    scheduled_at_key,
    created_sequence
FROM flow_scheduled_wakeups
WHERE wakeup_kind IN (0, 2);

DROP TABLE flow_scheduled_wakeups;
ALTER TABLE flow_scheduled_wakeups_activity_kind RENAME TO flow_scheduled_wakeups;

CREATE INDEX IF NOT EXISTS idx_flow_scheduled_wakeups_due
ON flow_scheduled_wakeups (
    scheduled_at_key,
    wakeup_kind,
    run_id,
    subject_id
);

CREATE INDEX IF NOT EXISTS idx_flow_scheduled_wakeups_next
ON flow_scheduled_wakeups (
    run_id,
    scheduled_at_key,
    wakeup_kind,
    subject_id
);

CREATE TRIGGER IF NOT EXISTS flow_continue_as_new_cleanup_after_event
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'run_continued_as_new'
BEGIN
    DELETE FROM flow_active_hooks WHERE run_id = NEW.run_id;
    DELETE FROM flow_scheduled_wakeups WHERE run_id = NEW.run_id;
END;

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

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_step_cancelled
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'step_cancelled'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 2
      AND subject_id = json_extract(NEW.event_json, '$.step_id');
END;

-- Rebuild retry rows from history so a collided step/activity pair is restored.
DELETE FROM flow_scheduled_wakeups WHERE wakeup_kind IN (1, 2);

WITH open_step_retries AS (
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
                        'step_failed',
                        'step_cancelled',
                        'step_retrying'
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
                    'run_host_shutdown',
                    'run_continued_as_new'
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
FROM open_step_retries
ORDER BY run_id, created_sequence;

WITH open_activity_retries AS (
    SELECT
        retrying.run_id,
        json_extract(retrying.event_json, '$.activity_id') AS subject_id,
        json_extract(retrying.event_json, '$.retry_after') AS scheduled_at,
        retrying.sequence AS created_sequence
    FROM flow_events AS retrying
    WHERE json_extract(retrying.event_json, '$.type') = 'activity_retrying'
      AND json_extract(retrying.event_json, '$.retry_after') IS NOT NULL
      AND NOT EXISTS (
          SELECT 1
          FROM flow_events AS later
          WHERE later.run_id = retrying.run_id
            AND later.sequence > retrying.sequence
            AND (
                (
                    json_extract(later.event_json, '$.type') IN (
                        'activity_started',
                        'activity_completed',
                        'activity_failed',
                        'activity_non_retryable',
                        'activity_unknown',
                        'activity_cancelled'
                    )
                    AND json_extract(later.event_json, '$.activity_id') =
                        json_extract(retrying.event_json, '$.activity_id')
                )
                OR (
                    json_extract(later.event_json, '$.type') = 'activity_retrying'
                    AND json_extract(later.event_json, '$.activity_id') =
                        json_extract(retrying.event_json, '$.activity_id')
                )
                OR json_extract(later.event_json, '$.type') IN (
                    'run_cancellation_requested',
                    'run_completed',
                    'run_failed',
                    'run_cancelled',
                    'run_timed_out',
                    'run_retry_exhausted',
                    'run_host_shutdown',
                    'run_continued_as_new'
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
    1,
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
FROM open_activity_retries
ORDER BY run_id, created_sequence;

DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_activity_retrying;
DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_activity_closed;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_activity_retrying
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'activity_retrying'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 1
      AND subject_id = json_extract(NEW.event_json, '$.activity_id');

    INSERT INTO flow_scheduled_wakeups (
        run_id,
        wakeup_kind,
        subject_id,
        scheduled_at_key,
        created_sequence
    )
    SELECT
        NEW.run_id,
        1,
        json_extract(NEW.event_json, '$.activity_id'),
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
    WHERE json_extract(NEW.event_json, '$.retry_after') IS NOT NULL
    ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
        scheduled_at_key = excluded.scheduled_at_key,
        created_sequence = excluded.created_sequence;
END;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_activity_closed
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') IN (
    'activity_started',
    'activity_completed',
    'activity_failed',
    'activity_non_retryable',
    'activity_unknown',
    'activity_cancelled'
)
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 1
      AND subject_id = json_extract(NEW.event_json, '$.activity_id');
END;
"#;

/// Structured select timer arms are waits, but they are recorded on
/// `select_created` rather than `wait_created`. The scheduler index has to
/// see them, and a race completion has to drop every timer arm except the
/// winner. The winner is already removed by `wait_completed`.
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL: &str = r#"
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
    json_extract(arm.value, '$.arm_id'),
    CASE
        WHEN instr(json_extract(arm.value, '$.resume_at'), '.') = 0 THEN
            substr(
                json_extract(arm.value, '$.resume_at'),
                1,
                length(json_extract(arm.value, '$.resume_at')) - 1
            ) || '.000000000Z'
        ELSE
            substr(
                json_extract(arm.value, '$.resume_at'),
                1,
                instr(json_extract(arm.value, '$.resume_at'), '.')
            ) ||
            substr(
                substr(
                    json_extract(arm.value, '$.resume_at'),
                    instr(json_extract(arm.value, '$.resume_at'), '.') + 1,
                    length(json_extract(arm.value, '$.resume_at')) -
                        instr(json_extract(arm.value, '$.resume_at'), '.') - 1
                ) || '000000000',
                1,
                9
            ) || 'Z'
    END,
    created.sequence
FROM flow_events AS created,
     json_each(json_extract(created.event_json, '$.arms')) AS arm
WHERE json_extract(created.event_json, '$.type') = 'select_created'
  AND json_extract(arm.value, '$.type') = 'timer'
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = created.run_id
        AND later.sequence > created.sequence
        AND (
            (
                json_extract(later.event_json, '$.type') = 'wait_completed'
                AND json_extract(later.event_json, '$.wait_id') =
                    json_extract(arm.value, '$.arm_id')
            )
            OR (
                json_extract(later.event_json, '$.type') = 'select_completed'
                AND json_extract(later.event_json, '$.select_id') =
                    json_extract(created.event_json, '$.select_id')
                AND json_extract(arm.value, '$.arm_id') IS NOT
                    json_extract(later.event_json, '$.winning_arm_id')
            )
            OR json_extract(later.event_json, '$.type') IN (
                'run_cancellation_requested',
                'run_completed',
                'run_failed',
                'run_cancelled',
                'run_timed_out',
                'run_retry_exhausted',
                'run_host_shutdown',
                'run_continued_as_new'
            )
        )
  )
ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
    scheduled_at_key = excluded.scheduled_at_key,
    created_sequence = excluded.created_sequence;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_select_created
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'select_created'
BEGIN
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
        json_extract(arm.value, '$.arm_id'),
        CASE
            WHEN instr(json_extract(arm.value, '$.resume_at'), '.') = 0 THEN
                substr(
                    json_extract(arm.value, '$.resume_at'),
                    1,
                    length(json_extract(arm.value, '$.resume_at')) - 1
                ) || '.000000000Z'
            ELSE
                substr(
                    json_extract(arm.value, '$.resume_at'),
                    1,
                    instr(json_extract(arm.value, '$.resume_at'), '.')
                ) ||
                substr(
                    substr(
                        json_extract(arm.value, '$.resume_at'),
                        instr(json_extract(arm.value, '$.resume_at'), '.') + 1,
                        length(json_extract(arm.value, '$.resume_at')) -
                            instr(json_extract(arm.value, '$.resume_at'), '.') - 1
                    ) || '000000000',
                    1,
                    9
                ) || 'Z'
        END,
        NEW.sequence
    FROM json_each(json_extract(NEW.event_json, '$.arms')) AS arm
    WHERE json_extract(arm.value, '$.type') = 'timer'
    ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
        scheduled_at_key = excluded.scheduled_at_key,
        created_sequence = excluded.created_sequence;
END;

CREATE TRIGGER IF NOT EXISTS flow_scheduled_wakeups_after_select_completed
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'select_completed'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE run_id = NEW.run_id
      AND wakeup_kind = 0
      AND subject_id IN (
          SELECT json_extract(arm.value, '$.arm_id')
          FROM flow_events AS created,
               json_each(json_extract(created.event_json, '$.arms')) AS arm
          WHERE created.run_id = NEW.run_id
            AND json_extract(created.event_json, '$.type') = 'select_created'
            AND json_extract(created.event_json, '$.select_id') =
                json_extract(NEW.event_json, '$.select_id')
            AND json_extract(arm.value, '$.type') = 'timer'
            AND json_extract(arm.value, '$.arm_id') IS NOT
                json_extract(NEW.event_json, '$.winning_arm_id')
      );
END;
"#;
