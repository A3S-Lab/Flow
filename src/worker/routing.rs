use async_trait::async_trait;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::engine::FlowEngine;
use crate::error::{FlowError, Result};
use crate::runtime_build::RuntimeBuildId;

use super::{FlowTask, FlowTaskDispatcher};

/// Routes pinned Flow tasks to dispatchers serving exact runtime builds.
///
/// A route can point at an A3S Boot manager, a compatibility queue, or another
/// host dispatcher. Register the same dispatcher under every build it can
/// execute. Unpinned histories require a separate explicit route.
#[derive(Clone, Default)]
pub struct RuntimeBuildTaskRouter {
    routes: BTreeMap<RuntimeBuildId, Arc<dyn FlowTaskDispatcher>>,
    unpinned_route: Option<Arc<dyn FlowTaskDispatcher>>,
}

impl fmt::Debug for RuntimeBuildTaskRouter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeBuildTaskRouter")
            .field("runtime_build_ids", &self.routes.keys().collect::<Vec<_>>())
            .field("has_unpinned_route", &self.unpinned_route.is_some())
            .finish()
    }
}

impl RuntimeBuildTaskRouter {
    /// Create a router with no pinned or legacy routes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the only dispatcher used for an exact runtime build.
    pub fn with_route(
        mut self,
        runtime_build_id: RuntimeBuildId,
        dispatcher: Arc<dyn FlowTaskDispatcher>,
    ) -> Result<Self> {
        if self.routes.contains_key(&runtime_build_id) {
            return Err(FlowError::InvalidWorkerConfiguration(format!(
                "runtime build route {runtime_build_id} is already registered"
            )));
        }
        self.routes.insert(runtime_build_id, dispatcher);
        Ok(self)
    }

    /// Register the explicit dispatcher for legacy unpinned histories.
    pub fn with_unpinned_route(mut self, dispatcher: Arc<dyn FlowTaskDispatcher>) -> Result<Self> {
        if self.unpinned_route.is_some() {
            return Err(FlowError::InvalidWorkerConfiguration(
                "an unpinned runtime build route is already registered".to_string(),
            ));
        }
        self.unpinned_route = Some(dispatcher);
        Ok(self)
    }

    /// Iterate over the exact pinned build routes.
    pub fn runtime_build_ids(&self) -> impl Iterator<Item = &RuntimeBuildId> {
        self.routes.keys()
    }

    /// Return whether a legacy unpinned route is registered.
    pub fn has_unpinned_route(&self) -> bool {
        self.unpinned_route.is_some()
    }

    /// Resolve the persisted build for `run_id`, verify the task target, and
    /// dispatch it through the matching route.
    pub async fn dispatch_for_run(
        &self,
        engine: &FlowEngine,
        run_id: &str,
        task: FlowTask,
    ) -> Result<()> {
        let task_run_id = task.target_run_id().ok_or_else(|| {
            FlowError::InvalidTransition(
                "runtime build routing requires a Flow task with an explicit run id".to_string(),
            )
        })?;
        if task_run_id != run_id {
            return Err(FlowError::InvalidTransition(format!(
                "Flow task targets run {task_run_id} but build routing requested {run_id}"
            )));
        }
        let required_build_id = engine.runtime_build_id(run_id).await?;
        self.dispatch_for_runtime_build(required_build_id.as_ref(), task)
            .await
    }

    fn route(
        &self,
        required_build_id: Option<&RuntimeBuildId>,
    ) -> Result<&Arc<dyn FlowTaskDispatcher>> {
        match required_build_id {
            Some(build_id) => self.routes.get(build_id),
            None => self.unpinned_route.as_ref(),
        }
        .ok_or_else(|| FlowError::RuntimeBuildRouteNotFound {
            required_build_id: required_build_id.cloned(),
        })
    }
}

#[async_trait]
impl FlowTaskDispatcher for RuntimeBuildTaskRouter {
    async fn dispatch(&self, task: FlowTask) -> Result<()> {
        self.route(None)?.dispatch(task).await
    }

    fn has_runtime_build_route(&self, required_build_id: Option<&RuntimeBuildId>) -> bool {
        match required_build_id {
            Some(build_id) => self.routes.contains_key(build_id),
            None => self.unpinned_route.is_some(),
        }
    }

    async fn dispatch_for_runtime_build(
        &self,
        required_build_id: Option<&RuntimeBuildId>,
        task: FlowTask,
    ) -> Result<()> {
        self.route(required_build_id)?.dispatch(task).await
    }
}
