/// A completed or already-cancelled descendant is not part of the open
/// cancellation tree. Timers created inside that descendant stay indexed when
/// an ancestor is cancelled. Published `0014` / `0016` included every historical
/// child of the cancelled scope.
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL: &str = r#"
DELETE FROM flow_scheduled_wakeups
WHERE rowid IN (
    WITH RECURSIVE cancelled_tree AS (
        SELECT
            cancelled.run_id AS run_id,
            cancelled.sequence AS cancelled_sequence,
            json_extract(cancelled.event_json, '$.scope_id') AS scope_id
        FROM flow_events AS cancelled
        WHERE json_extract(cancelled.event_json, '$.type') = 'scope_cancelled'
        UNION
        SELECT
            child.run_id,
            cancelled_tree.cancelled_sequence,
            json_extract(child.event_json, '$.scope_id')
        FROM cancelled_tree
        JOIN flow_events AS child
          ON child.run_id = cancelled_tree.run_id
         AND child.sequence < cancelled_tree.cancelled_sequence
         AND json_extract(child.event_json, '$.type') = 'scope_opened'
         AND json_extract(child.event_json, '$.parent_scope_id') = cancelled_tree.scope_id
         AND NOT EXISTS (
             SELECT 1
             FROM flow_events AS closed
             WHERE closed.run_id = child.run_id
               AND closed.sequence > child.sequence
               AND closed.sequence < cancelled_tree.cancelled_sequence
               AND json_extract(closed.event_json, '$.type') IN (
                   'scope_completed',
                   'scope_cancelled'
               )
               AND json_extract(closed.event_json, '$.scope_id') =
                   json_extract(child.event_json, '$.scope_id')
         )
    )
    SELECT wakeup.rowid
    FROM flow_scheduled_wakeups AS wakeup
    JOIN cancelled_tree
      ON cancelled_tree.run_id = wakeup.run_id
     AND cancelled_tree.cancelled_sequence > wakeup.created_sequence
     AND cancelled_tree.scope_id = (
         SELECT json_extract(opened.event_json, '$.scope_id')
         FROM flow_events AS opened
         WHERE opened.run_id = wakeup.run_id
           AND opened.sequence < wakeup.created_sequence
           AND json_extract(opened.event_json, '$.type') = 'scope_opened'
           AND NOT EXISTS (
               SELECT 1
               FROM flow_events AS closed
               WHERE closed.run_id = opened.run_id
                 AND closed.sequence > opened.sequence
                 AND closed.sequence < wakeup.created_sequence
                 AND json_extract(closed.event_json, '$.type') IN (
                     'scope_completed',
                     'scope_cancelled'
                 )
                 AND json_extract(closed.event_json, '$.scope_id') =
                     json_extract(opened.event_json, '$.scope_id')
           )
         ORDER BY opened.sequence DESC
         LIMIT 1
     )
    WHERE wakeup.wakeup_kind = 0
);

DROP TRIGGER IF EXISTS flow_scheduled_wakeups_after_scope_cancelled;

CREATE TRIGGER flow_scheduled_wakeups_after_scope_cancelled
AFTER INSERT ON flow_events
WHEN json_extract(NEW.event_json, '$.type') = 'scope_cancelled'
BEGIN
    DELETE FROM flow_scheduled_wakeups
    WHERE rowid IN (
        WITH RECURSIVE cancelled_tree(scope_id) AS (
            SELECT json_extract(NEW.event_json, '$.scope_id')
            UNION
            SELECT json_extract(child.event_json, '$.scope_id')
            FROM cancelled_tree
            JOIN flow_events AS child
              ON child.run_id = NEW.run_id
             AND child.sequence < NEW.sequence
             AND json_extract(child.event_json, '$.type') = 'scope_opened'
             AND json_extract(child.event_json, '$.parent_scope_id') = cancelled_tree.scope_id
             AND NOT EXISTS (
                 SELECT 1
                 FROM flow_events AS closed
                 WHERE closed.run_id = child.run_id
                   AND closed.sequence > child.sequence
                   AND closed.sequence < NEW.sequence
                   AND json_extract(closed.event_json, '$.type') IN (
                       'scope_completed',
                       'scope_cancelled'
                   )
                   AND json_extract(closed.event_json, '$.scope_id') =
                       json_extract(child.event_json, '$.scope_id')
             )
        )
        SELECT wakeup.rowid
        FROM flow_scheduled_wakeups AS wakeup
        WHERE wakeup.run_id = NEW.run_id
          AND wakeup.wakeup_kind = 0
          AND (
              SELECT json_extract(opened.event_json, '$.scope_id')
              FROM flow_events AS opened
              WHERE opened.run_id = wakeup.run_id
                AND opened.sequence < wakeup.created_sequence
                AND json_extract(opened.event_json, '$.type') = 'scope_opened'
                AND NOT EXISTS (
                    SELECT 1
                    FROM flow_events AS closed
                    WHERE closed.run_id = opened.run_id
                      AND closed.sequence > opened.sequence
                      AND closed.sequence < wakeup.created_sequence
                      AND json_extract(closed.event_json, '$.type') IN (
                          'scope_completed',
                          'scope_cancelled'
                      )
                      AND json_extract(closed.event_json, '$.scope_id') =
                          json_extract(opened.event_json, '$.scope_id')
                )
              ORDER BY opened.sequence DESC
              LIMIT 1
          ) IN (SELECT scope_id FROM cancelled_tree)
    );
END;
"#;

/// A completed or already-cancelled descendant is not part of the open
/// cancellation tree. Timers created inside that descendant stay indexed when
/// an ancestor is cancelled.
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL: &str = r#"
CREATE OR REPLACE FUNCTION a3s_flow_drop_scoped_wakeups()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    WITH RECURSIVE cancelled_tree(scope_id) AS (
        SELECT NEW.event_json::jsonb ->> 'scope_id'
        UNION
        SELECT child.event_json::jsonb ->> 'scope_id'
        FROM cancelled_tree
        JOIN flow_events AS child
          ON child.run_id = NEW.run_id
         AND child.sequence < NEW.sequence
         AND child.event_json::jsonb ->> 'type' = 'scope_opened'
         AND child.event_json::jsonb ->> 'parent_scope_id' = cancelled_tree.scope_id
         AND NOT EXISTS (
             SELECT 1
             FROM flow_events AS closed
             WHERE closed.run_id = child.run_id
               AND closed.sequence > child.sequence
               AND closed.sequence < NEW.sequence
               AND closed.event_json::jsonb ->> 'type' IN (
                   'scope_completed',
                   'scope_cancelled'
               )
               AND closed.event_json::jsonb ->> 'scope_id' =
                   child.event_json::jsonb ->> 'scope_id'
         )
    )
    DELETE FROM flow_scheduled_wakeups AS wakeup
    WHERE wakeup.run_id = NEW.run_id
      AND wakeup.wakeup_kind = 0
      AND (
          SELECT created.event_json::jsonb ->> 'scope_id'
          FROM flow_events AS created
          WHERE created.run_id = wakeup.run_id
            AND created.sequence < wakeup.created_sequence
            AND created.event_json::jsonb ->> 'type' = 'scope_opened'
            AND NOT EXISTS (
                SELECT 1
                FROM flow_events AS closed
                WHERE closed.run_id = created.run_id
                  AND closed.sequence > created.sequence
                  AND closed.sequence < wakeup.created_sequence
                  AND closed.event_json::jsonb ->> 'type' IN (
                      'scope_completed',
                      'scope_cancelled'
                  )
                  AND closed.event_json::jsonb ->> 'scope_id' =
                      created.event_json::jsonb ->> 'scope_id'
            )
          ORDER BY created.sequence DESC
          LIMIT 1
      ) IN (SELECT scope_id FROM cancelled_tree);
    RETURN NEW;
END;
$$;

WITH RECURSIVE cancelled_tree AS (
    SELECT
        cancelled.run_id,
        cancelled.sequence AS cancelled_sequence,
        cancelled.event_json::jsonb ->> 'scope_id' AS scope_id
    FROM flow_events AS cancelled
    WHERE cancelled.event_json::jsonb ->> 'type' = 'scope_cancelled'
    UNION
    SELECT
        child.run_id,
        cancelled_tree.cancelled_sequence,
        child.event_json::jsonb ->> 'scope_id'
    FROM cancelled_tree
    JOIN flow_events AS child
      ON child.run_id = cancelled_tree.run_id
     AND child.sequence < cancelled_tree.cancelled_sequence
     AND child.event_json::jsonb ->> 'type' = 'scope_opened'
     AND child.event_json::jsonb ->> 'parent_scope_id' = cancelled_tree.scope_id
     AND NOT EXISTS (
         SELECT 1
         FROM flow_events AS closed
         WHERE closed.run_id = child.run_id
           AND closed.sequence > child.sequence
           AND closed.sequence < cancelled_tree.cancelled_sequence
           AND closed.event_json::jsonb ->> 'type' IN (
               'scope_completed',
               'scope_cancelled'
           )
           AND closed.event_json::jsonb ->> 'scope_id' =
               child.event_json::jsonb ->> 'scope_id'
     )
)
DELETE FROM flow_scheduled_wakeups AS wakeup
USING cancelled_tree
WHERE wakeup.run_id = cancelled_tree.run_id
  AND cancelled_tree.cancelled_sequence > wakeup.created_sequence
  AND wakeup.wakeup_kind = 0
  AND cancelled_tree.scope_id = (
      SELECT created.event_json::jsonb ->> 'scope_id'
      FROM flow_events AS created
      WHERE created.run_id = wakeup.run_id
        AND created.sequence < wakeup.created_sequence
        AND created.event_json::jsonb ->> 'type' = 'scope_opened'
        AND NOT EXISTS (
            SELECT 1
            FROM flow_events AS closed
            WHERE closed.run_id = created.run_id
              AND closed.sequence > created.sequence
              AND closed.sequence < wakeup.created_sequence
              AND closed.event_json::jsonb ->> 'type' IN (
                  'scope_completed',
                  'scope_cancelled'
              )
              AND closed.event_json::jsonb ->> 'scope_id' =
                  created.event_json::jsonb ->> 'scope_id'
        )
      ORDER BY created.sequence DESC
      LIMIT 1
  );
"#;
