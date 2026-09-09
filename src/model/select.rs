use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{FlowError, Result};

/// One durable arm of a structured select/race.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SelectArm {
    /// Race a durable timer wait.
    Timer {
        /// Stable arm identity reused as the timer wait id.
        arm_id: String,
        /// UTC time at which this timer becomes ready.
        resume_at: DateTime<Utc>,
    },
    /// Race a durable named-signal wait.
    Signal {
        /// Stable arm identity reused as the signal wait id.
        arm_id: String,
        /// Declared signal contract accepted by the wait.
        signal_name: String,
    },
}

impl SelectArm {
    /// Create a timer arm.
    pub fn timer(arm_id: impl Into<String>, resume_at: DateTime<Utc>) -> Self {
        Self::Timer {
            arm_id: arm_id.into(),
            resume_at,
        }
    }

    /// Create a signal arm.
    pub fn signal(arm_id: impl Into<String>, signal_name: impl Into<String>) -> Self {
        Self::Signal {
            arm_id: arm_id.into(),
            signal_name: signal_name.into(),
        }
    }

    /// Stable arm identity.
    pub fn arm_id(&self) -> &str {
        match self {
            Self::Timer { arm_id, .. } | Self::Signal { arm_id, .. } => arm_id,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.arm_id().trim().is_empty() {
            return Err(FlowError::InvalidTransition(
                "select arm id must not be empty".to_string(),
            ));
        }
        if let Self::Signal { signal_name, .. } = self {
            if signal_name.trim().is_empty() {
                return Err(FlowError::InvalidTransition(
                    "select signal name must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Materialized lifecycle state of a structured select/race.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum SelectStatus {
    /// Waiting for the first arm to complete.
    Open,
    /// One arm won and siblings were cancelled.
    Completed,
    /// The owning run cancelled the select before a winner.
    Cancelled,
}

/// Materialized state of one structured select/race.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SelectSnapshot {
    /// Replay-stable identity of the select.
    pub select_id: String,
    /// Arms recorded when the select was created.
    pub arms: Vec<SelectArm>,
    /// Current lifecycle state.
    pub status: SelectStatus,
    /// Winning arm identity when completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winning_arm_id: Option<String>,
}

impl SelectSnapshot {
    /// Returns whether the select is still waiting for a winner.
    pub fn is_open(&self) -> bool {
        self.status == SelectStatus::Open
    }

    /// Returns whether the select completed with a winner.
    pub fn is_completed(&self) -> bool {
        self.status == SelectStatus::Completed
    }
}

pub(crate) fn validate_select(select_id: &str, arms: &[SelectArm]) -> Result<()> {
    if select_id.trim().is_empty() {
        return Err(FlowError::InvalidTransition(
            "select id must not be empty".to_string(),
        ));
    }
    if arms.len() < 2 {
        return Err(FlowError::InvalidTransition(
            "select requires at least two arms".to_string(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for arm in arms {
        arm.validate()?;
        if !seen.insert(arm.arm_id().to_string()) {
            return Err(FlowError::InvalidTransition(format!(
                "select {select_id} has duplicate arm id {}",
                arm.arm_id()
            )));
        }
    }
    Ok(())
}
