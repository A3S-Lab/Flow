use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::sync::Mutex;

use crate::error::{FlowError, Result};
use crate::jsonl::{
    append_jsonl_record, load_jsonl, repair_jsonl_tail, JsonlTailRepair, LoadedJsonl,
};

use super::{A3sFlowEvent, A3sFlowEventSink};

/// JSONL-backed A3S Flow event sink for local audit logs.
///
/// `A3sFlowEventSink::emit` is intentionally best-effort because observers run
/// after the event store commit. Write failures are recorded in `last_error()`
/// and logged, while the workflow event store remains the source of truth. An
/// unterminated malformed tail is treated as a torn append and truncated before
/// the next write. A complete final record missing its delimiter is preserved;
/// terminated or interior corruption remains an error. Writes through one sink
/// instance are serialized, but separate instances or processes are not
/// coordinated; use a hosted sink when multiple writers share one audit log.
#[derive(Debug)]
pub struct LocalFileA3sFlowEventSink {
    path: PathBuf,
    state: Mutex<LocalFileSinkState>,
    last_error: Mutex<Option<String>>,
}

#[derive(Debug, Default)]
struct LocalFileSinkState {
    prepared: bool,
}

impl LocalFileA3sFlowEventSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            state: Mutex::new(LocalFileSinkState::default()),
            last_error: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }

    pub async fn events(&self) -> Result<Vec<A3sFlowEvent>> {
        let mut state = self.state.lock().await;
        match self.load_events().await {
            Ok(loaded) => {
                state.prepared = loaded.tail_repair == JsonlTailRepair::None;
                Ok(loaded.records)
            }
            Err(error) => {
                state.prepared = false;
                Err(error)
            }
        }
    }

    async fn load_events(&self) -> Result<LoadedJsonl<A3sFlowEvent>> {
        let file = match File::open(&self.path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedJsonl::empty());
            }
            Err(error) => return Err(FlowError::Io(error)),
        };
        load_jsonl(file, &self.path, "audit event").await
    }

    async fn prepare_for_append(&self) -> Result<()> {
        let loaded = self.load_events().await?;
        repair_jsonl_tail(&self.path, loaded.tail_repair).await
    }

    async fn append_event(&self, event: &A3sFlowEvent) -> Result<()> {
        if let Some(parent) = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        append_jsonl_record(&self.path, event).await
    }
}

#[async_trait]
impl A3sFlowEventSink for LocalFileA3sFlowEventSink {
    async fn emit(&self, event: A3sFlowEvent) {
        let mut state = self.state.lock().await;
        let result = async {
            if !state.prepared {
                self.prepare_for_append().await?;
            }
            // If this future is cancelled during the write, the next append
            // must rescan the file for a torn tail before trusting it again.
            state.prepared = false;
            self.append_event(&event).await
        }
        .await;
        state.prepared = result.is_ok();

        match result {
            Ok(()) => {
                *self.last_error.lock().await = None;
            }
            Err(error) => {
                let message = error.to_string();
                tracing::warn!(
                    error = %message,
                    path = %self.path.display(),
                    "failed to emit flow audit event"
                );
                *self.last_error.lock().await = Some(message);
            }
        }
    }
}
