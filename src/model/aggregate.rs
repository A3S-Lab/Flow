use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

use super::JsonValue;

/// One durable contribution into a named per-item aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ItemAggregateContribution {
    /// Replay-stable aggregate identity.
    pub aggregate_id: String,
    /// Replay-stable item identity within the aggregate.
    pub item_id: String,
    /// Application-defined item value (outcome, partial result, etc.).
    pub value: JsonValue,
}

impl ItemAggregateContribution {
    /// Create a per-item aggregate contribution.
    pub fn new(
        aggregate_id: impl Into<String>,
        item_id: impl Into<String>,
        value: JsonValue,
    ) -> Self {
        Self {
            aggregate_id: aggregate_id.into(),
            item_id: item_id.into(),
            value,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.aggregate_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "item aggregate id must not be empty".to_string(),
            ));
        }
        if self.item_id.trim().is_empty() {
            return Err(FlowError::InvalidTransition(format!(
                "item aggregate {} item id must not be empty",
                self.aggregate_id
            )));
        }
        Ok(())
    }
}

/// Materialized state of one named per-item aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ItemAggregateSnapshot {
    /// Replay-stable aggregate identity.
    pub aggregate_id: String,
    /// Item contributions in durable record order.
    pub entries: Vec<ItemAggregateEntry>,
}

/// One recorded item inside an aggregate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ItemAggregateEntry {
    /// Replay-stable item identity.
    pub item_id: String,
    /// Application-defined item value.
    pub value: JsonValue,
}

impl ItemAggregateSnapshot {
    /// Number of recorded items.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the aggregate has no recorded items.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Lookup one item value by identity.
    pub fn get(&self, item_id: &str) -> Option<&JsonValue> {
        self.entries
            .iter()
            .find(|entry| entry.item_id == item_id)
            .map(|entry| &entry.value)
    }

    /// Values in durable record order.
    pub fn values(&self) -> impl Iterator<Item = &JsonValue> + '_ {
        self.entries.iter().map(|entry| &entry.value)
    }
}
