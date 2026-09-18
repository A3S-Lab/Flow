use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::error::{FlowError, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum JsonlTailRepair {
    None,
    AppendDelimiter,
    Truncate(u64),
}

#[derive(Debug)]
pub(crate) struct LoadedJsonl<T> {
    pub(crate) records: Vec<T>,
    pub(crate) tail_repair: JsonlTailRepair,
}

impl<T> LoadedJsonl<T> {
    pub(crate) fn empty() -> Self {
        Self {
            records: Vec::new(),
            tail_repair: JsonlTailRepair::None,
        }
    }
}

/// Decode a JSONL file while classifying only an unterminated final record as
/// repairable. Terminated or interior corruption remains an error.
pub(crate) async fn load_jsonl<T>(
    file: File,
    path: &Path,
    record_kind: &str,
) -> Result<LoadedJsonl<T>>
where
    T: DeserializeOwned,
{
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut line_no = 0usize;
    let mut valid_prefix_len = 0u64;
    let mut buffer = Vec::new();

    loop {
        buffer.clear();
        let bytes_read = reader.read_until(b'\n', &mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        line_no += 1;
        let terminated = buffer.last() == Some(&b'\n');
        let line = if terminated {
            &buffer[..buffer.len() - 1]
        } else {
            buffer.as_slice()
        };

        if line.iter().all(u8::is_ascii_whitespace) {
            if !terminated {
                return Ok(LoadedJsonl {
                    records,
                    tail_repair: JsonlTailRepair::Truncate(valid_prefix_len),
                });
            }
            valid_prefix_len = checked_file_offset(valid_prefix_len, bytes_read, path)?;
            continue;
        }

        let record = match serde_json::from_slice(line) {
            Ok(record) => record,
            Err(_) if !terminated => {
                return Ok(LoadedJsonl {
                    records,
                    tail_repair: JsonlTailRepair::Truncate(valid_prefix_len),
                });
            }
            Err(error) => {
                return Err(FlowError::Store(format!(
                    "failed to decode {record_kind} line {line_no} from {}: {error}",
                    path.display()
                )));
            }
        };
        records.push(record);
        valid_prefix_len = checked_file_offset(valid_prefix_len, bytes_read, path)?;
        if !terminated {
            return Ok(LoadedJsonl {
                records,
                tail_repair: JsonlTailRepair::AppendDelimiter,
            });
        }
    }

    Ok(LoadedJsonl {
        records,
        tail_repair: JsonlTailRepair::None,
    })
}

/// Decode JSONL until `limit` included records are collected, then stop reading.
///
/// `include` decides whether a decoded record belongs in the page. Records that
/// fail `include` still count toward `records_decoded` so callers can prove the
/// scan advanced past a cursor without buffering the remainder of the file.
pub(crate) async fn load_jsonl_page<T, P>(
    file: File,
    path: &Path,
    record_kind: &str,
    mut include: P,
    limit: usize,
) -> Result<(Vec<T>, usize)>
where
    T: DeserializeOwned,
    P: FnMut(&T) -> bool,
{
    if limit == 0 {
        return Err(FlowError::Store(
            "JSONL page limit must be at least 1".to_string(),
        ));
    }

    let mut reader = BufReader::new(file);
    let mut records = Vec::with_capacity(limit.min(64));
    let mut records_decoded = 0usize;
    let mut line_no = 0usize;
    let mut buffer = Vec::new();

    loop {
        if records.len() >= limit {
            break;
        }
        buffer.clear();
        let bytes_read = reader.read_until(b'\n', &mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        line_no += 1;
        let terminated = buffer.last() == Some(&b'\n');
        let line = if terminated {
            &buffer[..buffer.len() - 1]
        } else {
            buffer.as_slice()
        };

        if line.iter().all(u8::is_ascii_whitespace) {
            if !terminated {
                // Match load_jsonl: an unterminated blank tail is ignored for
                // read-only page scans; writers repair before the next append.
                break;
            }
            continue;
        }

        let record = match serde_json::from_slice::<T>(line) {
            Ok(record) => record,
            Err(_) if !terminated => {
                // Torn final record: stop without including it.
                break;
            }
            Err(error) => {
                return Err(FlowError::Store(format!(
                    "failed to decode {record_kind} line {line_no} from {}: {error}",
                    path.display()
                )));
            }
        };
        records_decoded = records_decoded.saturating_add(1);
        if include(&record) {
            records.push(record);
        }
        if !terminated {
            break;
        }
    }

    Ok((records, records_decoded))
}

pub(crate) async fn repair_jsonl_tail(path: &Path, repair: JsonlTailRepair) -> Result<()> {
    match repair {
        JsonlTailRepair::None => Ok(()),
        JsonlTailRepair::AppendDelimiter => {
            let mut file = OpenOptions::new().append(true).open(path).await?;
            file.write_all(b"\n").await?;
            file.flush().await?;
            file.sync_data().await?;
            Ok(())
        }
        JsonlTailRepair::Truncate(valid_prefix_len) => {
            let file = OpenOptions::new().write(true).open(path).await?;
            file.set_len(valid_prefix_len).await?;
            file.sync_data().await?;
            Ok(())
        }
    }
}

pub(crate) async fn append_jsonl_record<T>(path: &Path, record: &T) -> Result<()>
where
    T: Serialize,
{
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(&line).await?;
    file.flush().await?;
    file.sync_data().await?;
    Ok(())
}

fn checked_file_offset(current: u64, bytes_read: usize, path: &Path) -> Result<u64> {
    let bytes_read = u64::try_from(bytes_read).map_err(|_| {
        FlowError::Store(format!(
            "JSONL record length from {} exceeds the supported file offset",
            path.display()
        ))
    })?;
    current.checked_add(bytes_read).ok_or_else(|| {
        FlowError::Store(format!(
            "JSONL file {} exceeds the supported file offset",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tokio::io::AsyncWriteExt;

    #[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
    struct SeqRecord {
        sequence: u64,
    }

    async fn write_seq_jsonl(path: &Path, count: u64) {
        let mut file = File::create(path).await.expect("create jsonl");
        for sequence in 1..=count {
            let line = serde_json::to_vec(&SeqRecord { sequence }).expect("encode");
            file.write_all(&line).await.expect("write");
            file.write_all(b"\n").await.expect("newline");
        }
        file.flush().await.expect("flush");
    }

    #[tokio::test]
    async fn load_jsonl_page_stops_decoding_after_limit_is_filled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("events.jsonl");
        write_seq_jsonl(&path, 100).await;

        let file = File::open(&path).await.expect("open");
        let (records, records_decoded) = load_jsonl_page(
            file,
            &path,
            "event",
            |record: &SeqRecord| record.sequence > 50,
            10,
        )
        .await
        .expect("page");

        assert_eq!(
            records
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            (51..=60).collect::<Vec<_>>()
        );
        // Proves early stop: a full-file load would decode all 100 records.
        assert_eq!(records_decoded, 60);
    }
}
