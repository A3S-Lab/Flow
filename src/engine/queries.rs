use crate::error::{FlowError, Result};
use crate::runtime::QueryInvocation;

use super::FlowEngine;

impl FlowEngine {
    /// Answer a declared read-only query without appending history.
    ///
    /// Queries are fenced by the run's runtime build because they execute
    /// workflow-owned code. Undeclared query names fail closed before the
    /// runtime is invoked.
    pub async fn query(
        &self,
        run_id: &str,
        query_name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if query_name.trim().is_empty() {
            return Err(FlowError::InvalidWorkflow(
                "workflow query name must not be empty".to_string(),
            ));
        }
        let history = self.store.list(run_id).await?;
        let snapshot = crate::model::project_run(run_id, &history)?;
        self.ensure_runtime_build_available(run_id, &snapshot.spec)?;
        if !snapshot.spec.accepts_query(query_name) {
            return Err(FlowError::InvalidWorkflow(format!(
                "workflow run {run_id} does not accept query {query_name}"
            )));
        }
        let invocation =
            QueryInvocation::new(run_id, snapshot.spec.clone(), query_name, input, history);
        self.runtime.run_query(invocation).await
    }
}
