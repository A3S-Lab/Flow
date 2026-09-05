use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::FlowTask;

/// Versioned protocol exchanged by Flow-compatible workers and hosts.
pub const FLOW_WORKER_PROTOCOL: &str = "a3s.flow.worker.v1";

/// Execution capabilities advertised by one Flow worker.
///
/// This contract describes only kernel execution semantics. Queue admission,
/// tenant fairness, placement, and processor lifecycle remain host-owned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowWorkerCapabilities {
    /// Worker protocol version.
    pub protocol: String,
    /// Task wire kinds understood by the worker.
    #[serde(default)]
    pub task_types: Vec<String>,
    /// Whether lease fencing tokens are enforced.
    #[serde(default)]
    pub lease_fencing: bool,
    /// Whether active leases can be heartbeated.
    #[serde(default)]
    pub heartbeats: bool,
    /// Whether the worker can apply a bounded drain budget.
    #[serde(default)]
    pub bounded_drain: bool,
}

impl FlowWorkerCapabilities {
    /// Return the capabilities implemented by this Flow release.
    pub fn current() -> Self {
        Self {
            protocol: FLOW_WORKER_PROTOCOL.to_string(),
            task_types: FlowTask::supported_kinds()
                .iter()
                .map(|kind| (*kind).to_string())
                .collect(),
            lease_fencing: true,
            heartbeats: true,
            bounded_drain: true,
        }
    }

    /// Return whether this worker understands one task wire kind.
    pub fn supports_task(&self, task_type: &str) -> bool {
        self.task_types.iter().any(|kind| kind == task_type)
    }

    /// Negotiate required capabilities against an offered worker.
    ///
    /// Negotiation is fail-closed: a protocol mismatch or any missing required
    /// capability returns a typed error before the host leases work.
    pub fn negotiate(required: &Self, offered: &Self) -> Result<()> {
        if required.protocol != offered.protocol {
            return Err(FlowError::UnsupportedWorkerProtocol {
                required: required.protocol.clone(),
                offered: offered.protocol.clone(),
            });
        }

        let mut missing = Vec::new();
        if required.lease_fencing && !offered.lease_fencing {
            missing.push("lease_fencing".to_string());
        }
        if required.heartbeats && !offered.heartbeats {
            missing.push("heartbeats".to_string());
        }
        if required.bounded_drain && !offered.bounded_drain {
            missing.push("bounded_drain".to_string());
        }
        for task_type in &required.task_types {
            if !offered.supports_task(task_type) {
                missing.push(format!("task_type:{task_type}"));
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(FlowError::WorkerCapabilityUnavailable(missing.join(", ")))
        }
    }
}
