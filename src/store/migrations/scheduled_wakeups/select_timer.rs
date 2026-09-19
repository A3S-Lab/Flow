/// Structured select timer arms are waits, but they are recorded on
/// `select_created` rather than `wait_created`. The scheduler index has to
/// see them, and a race completion has to drop every timer arm except the
/// winner. The winner is already removed by `wait_completed`.
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL: &str = r#"
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
    arm ->> 'arm_id',
    a3s_flow_normalize_wakeup_timestamp(arm ->> 'resume_at'),
    created.sequence
FROM flow_events AS created
CROSS JOIN LATERAL jsonb_array_elements(created.event_json::jsonb -> 'arms') AS arm
WHERE created.event_json::jsonb ->> 'type' = 'select_created'
  AND arm ->> 'type' = 'timer'
  AND NOT EXISTS (
      SELECT 1
      FROM flow_events AS later
      WHERE later.run_id = created.run_id
        AND later.sequence > created.sequence
        AND (
            (
                later.event_json::jsonb ->> 'type' = 'wait_completed'
                AND later.event_json::jsonb ->> 'wait_id' = arm ->> 'arm_id'
            )
            OR (
                later.event_json::jsonb ->> 'type' = 'select_completed'
                AND later.event_json::jsonb ->> 'select_id' =
                    created.event_json::jsonb ->> 'select_id'
                AND (arm ->> 'arm_id') IS DISTINCT FROM
                    (later.event_json::jsonb ->> 'winning_arm_id')
            )
            OR later.event_json::jsonb ->> 'type' IN (
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
    scheduled_at_key = EXCLUDED.scheduled_at_key,
    created_sequence = EXCLUDED.created_sequence;

CREATE OR REPLACE FUNCTION a3s_flow_project_select_timer_wakeup()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    event_type TEXT := NEW.event_json::jsonb ->> 'type';
BEGIN
    IF event_type = 'select_created' THEN
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
            arm ->> 'arm_id',
            a3s_flow_normalize_wakeup_timestamp(arm ->> 'resume_at'),
            NEW.sequence
        FROM jsonb_array_elements(NEW.event_json::jsonb -> 'arms') AS arm
        WHERE arm ->> 'type' = 'timer'
        ON CONFLICT (run_id, wakeup_kind, subject_id) DO UPDATE SET
            scheduled_at_key = EXCLUDED.scheduled_at_key,
            created_sequence = EXCLUDED.created_sequence;
    ELSIF event_type = 'select_completed' THEN
        DELETE FROM flow_scheduled_wakeups AS wakeup
        USING flow_events AS created,
              LATERAL jsonb_array_elements(created.event_json::jsonb -> 'arms') AS arm
        WHERE created.run_id = NEW.run_id
          AND created.event_json::jsonb ->> 'type' = 'select_created'
          AND created.event_json::jsonb ->> 'select_id' =
              NEW.event_json::jsonb ->> 'select_id'
          AND arm ->> 'type' = 'timer'
          AND wakeup.run_id = NEW.run_id
          AND wakeup.wakeup_kind = 0
          AND wakeup.subject_id = arm ->> 'arm_id'
          AND wakeup.subject_id IS DISTINCT FROM
              (NEW.event_json::jsonb ->> 'winning_arm_id');
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_select ON flow_events;

CREATE TRIGGER flow_scheduled_wakeups_after_select
AFTER INSERT ON flow_events
FOR EACH ROW
WHEN ((NEW.event_json::jsonb ->> 'type') IN ('select_created', 'select_completed'))
EXECUTE FUNCTION a3s_flow_project_select_timer_wakeup();
"#;
