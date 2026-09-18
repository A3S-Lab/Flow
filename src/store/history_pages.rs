use std::future::Future;

use crate::error::{FlowError, Result};
use crate::model::{
    project_run, project_run_from_snapshot, FlowEventEnvelope, WorkflowRunSnapshot,
};

use super::MAX_FLOW_HISTORY_PAGE_SIZE;

/// Fold an existing durable history one bounded page at a time.
///
/// `fetch_page(after_sequence, limit)` must return the next exclusive-cursor
/// page ordered by sequence. An empty first page means the run has no events
/// yet. A short page ends the fold. Callers that already hold a transaction
/// should pass a fetcher bound to that transaction so validation and append
/// share one snapshot of history.
pub(crate) async fn fold_history_pages<F, Fut>(
    run_id: &str,
    mut fetch_page: F,
) -> Result<Option<WorkflowRunSnapshot>>
where
    F: FnMut(u64, usize) -> Fut,
    Fut: Future<Output = Result<Vec<FlowEventEnvelope>>>,
{
    let mut snapshot = None;
    let mut after_sequence = 0u64;
    loop {
        let page = fetch_page(after_sequence, MAX_FLOW_HISTORY_PAGE_SIZE).await?;
        if page.is_empty() {
            return Ok(snapshot);
        }
        let page_last = page.last().expect("non-empty page").sequence;
        let expected = after_sequence.checked_add(1).ok_or_else(|| {
            FlowError::Store(format!(
                "history page sequence overflow for workflow run {run_id}"
            ))
        })?;
        if page.first().expect("non-empty page").sequence != expected {
            return Err(FlowError::Store(format!(
                "history page for {run_id} is not contiguous at sequence {}; expected {expected}",
                page.first().expect("non-empty page").sequence
            )));
        }
        if page_last <= after_sequence {
            return Err(FlowError::Store(format!(
                "history page for {run_id} did not advance past sequence {after_sequence}"
            )));
        }
        snapshot = Some(match snapshot {
            None => project_run(run_id, &page)?,
            Some(current) => project_run_from_snapshot(run_id, current, &page)?,
        });
        after_sequence = page_last;
        if page.len() < MAX_FLOW_HISTORY_PAGE_SIZE {
            break;
        }
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FlowEvent, WorkflowProgress, WorkflowSpec};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    fn envelope(run_id: &str, sequence: u64, event: FlowEvent) -> FlowEventEnvelope {
        FlowEventEnvelope::new(run_id, sequence, Uuid::new_v4(), chrono::Utc::now(), event)
    }

    fn seed(run_id: &str, count: usize) -> Vec<FlowEventEnvelope> {
        let mut events = Vec::with_capacity(count);
        events.push(envelope(
            run_id,
            1,
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded(
                    "history.pages",
                    "1",
                    "tests::history_pages",
                    "main",
                ),
                input: json!({}),
            },
        ));
        if count >= 2 {
            events.push(envelope(run_id, 2, FlowEvent::RunStarted));
        }
        for index in 3..=count {
            events.push(envelope(
                run_id,
                index as u64,
                FlowEvent::RunProgressRecorded {
                    progress: WorkflowProgress::new(format!("pad-{}", index), index as u64),
                },
            ));
        }
        events
    }

    #[tokio::test]
    async fn fold_history_pages_loads_each_window_instead_of_one_shot() {
        let run_id = "paged-fold";
        let history = seed(run_id, MAX_FLOW_HISTORY_PAGE_SIZE + 1);
        let calls = AtomicUsize::new(0);
        let snapshot = fold_history_pages(run_id, |after, limit| {
            calls.fetch_add(1, Ordering::SeqCst);
            let page: Vec<_> = history
                .iter()
                .filter(|envelope| envelope.sequence > after)
                .take(limit)
                .cloned()
                .collect();
            async move { Ok(page) }
        })
        .await
        .expect("paged fold must succeed")
        .expect("run must exist");

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            snapshot.last_sequence,
            (MAX_FLOW_HISTORY_PAGE_SIZE + 1) as u64
        );
        assert_eq!(
            snapshot,
            project_run(run_id, &history).expect("full projection")
        );
    }
}
