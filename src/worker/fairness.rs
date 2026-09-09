use crate::error::{FlowError, Result};

use super::FlowTask;

/// Maximum encoded length for an opaque processor partition key.
pub const MAX_FLOW_TASK_PARTITION_KEY_BYTES: usize = 256;

/// Reserved partition used for tasks that do not target a single run.
pub const UNSCOPED_FLOW_TASK_PARTITION: &str = "";

/// Validate a host-supplied opaque processor partition key.
///
/// Keys are opaque to Flow. Hosts may map tenant or namespace identities onto
/// them. Path separators and control characters are rejected so local-file
/// queues never treat a key as a filesystem path component.
pub fn validate_flow_task_partition_key(key: &str) -> Result<&str> {
    if key.len() > MAX_FLOW_TASK_PARTITION_KEY_BYTES {
        return Err(FlowError::InvalidTransition(format!(
            "flow task partition key exceeds {MAX_FLOW_TASK_PARTITION_KEY_BYTES} bytes"
        )));
    }
    if key
        .chars()
        .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
    {
        return Err(FlowError::InvalidTransition(
            "flow task partition key must not contain control characters or path separators"
                .to_string(),
        ));
    }
    Ok(key)
}

/// Resolve the opaque partition used for fair leasing.
///
/// An explicit key wins. Otherwise the targeted run ID is used so one hot run
/// cannot monopolize FIFO dispatch when partition fairness is enabled.
/// Compatibility-wide scans share [`UNSCOPED_FLOW_TASK_PARTITION`].
pub fn resolve_flow_task_partition(explicit: Option<&str>, task: &FlowTask) -> Result<String> {
    if let Some(key) = explicit {
        return Ok(validate_flow_task_partition_key(key)?.to_string());
    }
    Ok(task
        .target_run_id()
        .map(str::to_string)
        .unwrap_or_else(|| UNSCOPED_FLOW_TASK_PARTITION.to_string()))
}

/// Choose the next pending index under round-robin partition fairness.
///
/// Unique partition keys are visited in lexicographic order after the last
/// served key. Within one partition, the earliest pending index wins.
pub fn select_fair_pending_index(
    partition_keys: &[String],
    last_partition: &mut Option<String>,
) -> Option<usize> {
    if partition_keys.is_empty() {
        return None;
    }

    let mut unique = Vec::new();
    for key in partition_keys {
        if !unique.iter().any(|existing| existing == key) {
            unique.push(key.clone());
        }
    }
    unique.sort();

    let start = last_partition
        .as_ref()
        .and_then(|previous| unique.iter().position(|key| key == previous))
        .map(|index| (index + 1) % unique.len())
        .unwrap_or(0);

    for offset in 0..unique.len() {
        let key = &unique[(start + offset) % unique.len()];
        if let Some(index) = partition_keys.iter().position(|candidate| candidate == key) {
            *last_partition = Some(key.clone());
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::FlowTask;

    #[test]
    fn fair_selection_rotates_across_partitions() {
        let keys = vec![
            "a".to_string(),
            "a".to_string(),
            "b".to_string(),
            "a".to_string(),
        ];
        let mut last = None;
        assert_eq!(select_fair_pending_index(&keys, &mut last), Some(0));
        assert_eq!(last.as_deref(), Some("a"));
        assert_eq!(select_fair_pending_index(&keys, &mut last), Some(2));
        assert_eq!(last.as_deref(), Some("b"));
        assert_eq!(select_fair_pending_index(&keys, &mut last), Some(0));
        assert_eq!(last.as_deref(), Some("a"));
    }

    #[test]
    fn resolve_partition_prefers_explicit_key() {
        let task = FlowTask::DriveRun {
            run_id: "run-1".to_string(),
        };
        assert_eq!(
            resolve_flow_task_partition(Some("tenant-x"), &task).unwrap(),
            "tenant-x"
        );
        assert_eq!(resolve_flow_task_partition(None, &task).unwrap(), "run-1");
    }

    #[test]
    fn partition_key_rejects_path_separators() {
        assert!(validate_flow_task_partition_key("a/b").is_err());
        assert!(validate_flow_task_partition_key("a\\b").is_err());
        assert!(validate_flow_task_partition_key("ok").is_ok());
    }
}
