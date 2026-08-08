use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::engine::FlowEngine;
use crate::error::Result;
use crate::model::{ScheduledWakeupKind, WorkflowRunSuspension};
use crate::worker::{FlowTask, FlowTaskDispatcher};

/// Result of one scheduler scan and its targeted per-run dispatches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowSchedulerTick {
    pub due_waits: Vec<(String, String)>,
    pub due_retries: Vec<(String, String)>,
    pub enqueued_tasks: usize,
}

impl FlowSchedulerTick {
    pub fn has_due_work(&self) -> bool {
        !self.due_waits.is_empty() || !self.due_retries.is_empty()
    }
}

/// Scheduler that scans durable state once and enqueues one task per due run.
#[derive(Clone)]
pub struct FlowScheduler {
    engine: FlowEngine,
    dispatcher: Arc<dyn FlowTaskDispatcher>,
}

impl FlowScheduler {
    pub fn new(engine: FlowEngine, dispatcher: Arc<dyn FlowTaskDispatcher>) -> Self {
        Self { engine, dispatcher }
    }

    pub fn engine(&self) -> &FlowEngine {
        &self.engine
    }

    pub fn dispatcher(&self) -> Arc<dyn FlowTaskDispatcher> {
        Arc::clone(&self.dispatcher)
    }

    /// Backward-compatible name for [`Self::dispatcher`].
    #[deprecated(since = "0.4.4", note = "use dispatcher()")]
    pub fn queue(&self) -> Arc<dyn FlowTaskDispatcher> {
        self.dispatcher()
    }

    /// Return the earliest wait or delayed retry that can wake the scheduler.
    pub async fn next_wakeup(&self, now: DateTime<Utc>) -> Result<Option<WorkflowRunSuspension>> {
        self.engine.next_wakeup(now).await
    }

    /// Return how long the host can sleep before the next scheduled wake-up.
    ///
    /// Due or overdue wake-ups return `Duration::ZERO`. Active hooks are not
    /// represented because external callbacks are pushed into the queue by the
    /// callback router instead of time.
    pub async fn next_wakeup_delay(&self, now: DateTime<Utc>) -> Result<Option<Duration>> {
        let Some(wakeup) = self.next_wakeup(now).await? else {
            return Ok(None);
        };
        let Some(scheduled_at) = wakeup.scheduled_at() else {
            return Ok(None);
        };
        Ok(Some(
            scheduled_at
                .signed_duration_since(now)
                .to_std()
                .unwrap_or(Duration::ZERO),
        ))
    }

    pub async fn enqueue_due_work(&self, now: DateTime<Utc>) -> Result<FlowSchedulerTick> {
        let due = self.engine.list_due_wakeups(now).await?;
        let due_waits = due
            .iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Wait)
            .map(|wakeup| (wakeup.run_id.clone(), wakeup.subject_id.clone()))
            .collect::<Vec<_>>();
        let due_retries = due
            .iter()
            .filter(|wakeup| wakeup.kind == ScheduledWakeupKind::Retry)
            .map(|wakeup| (wakeup.run_id.clone(), wakeup.subject_id.clone()))
            .collect::<Vec<_>>();
        let mut enqueued_tasks = 0usize;

        let mut targets = BTreeMap::new();
        for wakeup in due {
            match targets.entry(wakeup.run_id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(wakeup.runtime_build_id);
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get() != &wakeup.runtime_build_id =>
                {
                    return Err(crate::FlowError::Store(format!(
                        "scheduled wakeups for run {} disagree on runtime build identity",
                        entry.key()
                    )));
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
        for required_build_id in targets.values() {
            self.dispatcher
                .ensure_runtime_build_route(required_build_id.as_ref())?;
        }
        for (run_id, required_build_id) in targets {
            self.dispatcher
                .dispatch_for_runtime_build(
                    required_build_id.as_ref(),
                    FlowTask::ResumeScheduledRun { run_id, now },
                )
                .await?;
            enqueued_tasks += 1;
        }

        Ok(FlowSchedulerTick {
            due_waits,
            due_retries,
            enqueued_tasks,
        })
    }
}
