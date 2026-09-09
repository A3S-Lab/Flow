use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

/// Minimum number of physical run shards when sharding is enabled.
pub const MIN_FLOW_RUN_SHARD_COUNT: u32 = 2;

/// Maximum number of physical run shards supported by built-in stores.
pub const MAX_FLOW_RUN_SHARD_COUNT: u32 = 256;

/// Stable routing layout that maps workflow run IDs onto physical shards.
///
/// A run always lands on the same shard for a given `shard_count`. Built-in
/// stores keep each run's events, checkpoints, and partition indexes inside
/// that shard so append/replay cost stays local while cross-run link and hook
/// checks can still see the whole store.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct FlowRunShardLayout {
    shard_count: u32,
}

impl FlowRunShardLayout {
    /// Single-shard layout used by unsharded built-in stores.
    pub const fn single() -> Self {
        Self { shard_count: 1 }
    }

    /// Create a multi-shard layout.
    ///
    /// `shard_count` must be between [`MIN_FLOW_RUN_SHARD_COUNT`] and
    /// [`MAX_FLOW_RUN_SHARD_COUNT`].
    pub fn new(shard_count: u32) -> Result<Self> {
        if !(MIN_FLOW_RUN_SHARD_COUNT..=MAX_FLOW_RUN_SHARD_COUNT).contains(&shard_count) {
            return Err(FlowError::InvalidTransition(format!(
                "run shard count must be between {MIN_FLOW_RUN_SHARD_COUNT} and {MAX_FLOW_RUN_SHARD_COUNT}, got {shard_count}"
            )));
        }
        Ok(Self { shard_count })
    }

    /// Number of physical shards in this layout.
    pub const fn shard_count(self) -> u32 {
        self.shard_count
    }

    /// Return whether this layout spreads runs across more than one shard.
    pub const fn is_sharded(self) -> bool {
        self.shard_count > 1
    }

    /// Stable zero-based shard index for `run_id`.
    pub fn shard_index(self, run_id: &str) -> u32 {
        debug_assert!(self.shard_count >= 1);
        (fnv1a64(run_id.as_bytes()) % u64::from(self.shard_count)) as u32
    }

    /// Directory or table suffix for a shard index, for example `s00`.
    pub fn shard_name(self, shard_index: u32) -> Result<String> {
        if shard_index >= self.shard_count {
            return Err(FlowError::InvalidTransition(format!(
                "shard index {shard_index} is outside layout with {} shards",
                self.shard_count
            )));
        }
        Ok(format!("s{shard_index:02}"))
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_index_is_stable_and_bounded() {
        let layout = FlowRunShardLayout::new(8).unwrap();
        let first = layout.shard_index("run-a");
        let second = layout.shard_index("run-a");
        assert_eq!(first, second);
        assert!(first < 8);
        assert!(layout.shard_index("run-b") < 8);
    }

    #[test]
    fn multi_shard_layout_rejects_invalid_counts() {
        assert!(FlowRunShardLayout::new(1).is_err());
        assert!(FlowRunShardLayout::new(0).is_err());
        assert!(FlowRunShardLayout::new(MAX_FLOW_RUN_SHARD_COUNT + 1).is_err());
        assert!(FlowRunShardLayout::new(4).unwrap().is_sharded());
        assert!(!FlowRunShardLayout::single().is_sharded());
    }

    #[test]
    fn enough_run_ids_occupy_multiple_shards() {
        let layout = FlowRunShardLayout::new(4).unwrap();
        let mut seen = [false; 4];
        for index in 0..64 {
            let shard = layout.shard_index(&format!("spread-{index}")) as usize;
            seen[shard] = true;
        }
        assert!(seen.iter().filter(|occupied| **occupied).count() >= 2);
    }
}
