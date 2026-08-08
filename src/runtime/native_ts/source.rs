use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use super::{FileMetadata, MAX_STABLE_READ_ATTEMPTS};
use crate::error::{FlowError, Result};
use crate::model::WorkflowSpec;

const SOURCE_FINGERPRINT_DOMAIN: &[u8] = b"a3s.flow.native_ts.source.contents.v1";
const SOURCE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct SourceSnapshot {
    metadata: FileMetadata,
    fingerprint: String,
}

impl SourceSnapshot {
    pub(in crate::runtime) async fn read(
        path: &Path,
        spec: &WorkflowSpec,
    ) -> Result<(String, Self)> {
        for _ in 0..MAX_STABLE_READ_ATTEMPTS {
            let before = source_metadata(path).await?;
            let mut file = tokio::fs::File::open(path).await.map_err(|error| {
                FlowError::Runtime(format!(
                    "native TypeScript source {} could not be read: {error}",
                    path.display()
                ))
            })?;
            let mut source_hasher = source_hasher(spec, before.length);
            let mut fingerprint_hasher = fingerprint_hasher(before.length);
            let mut bytes_read = 0_u64;
            let mut buffer = vec![0_u8; SOURCE_READ_BUFFER_BYTES];

            loop {
                let count = file.read(&mut buffer).await.map_err(|error| {
                    FlowError::Runtime(format!(
                        "native TypeScript source {} could not be read: {error}",
                        path.display()
                    ))
                })?;
                if count == 0 {
                    break;
                }
                bytes_read = bytes_read.checked_add(count as u64).ok_or_else(|| {
                    FlowError::Runtime(format!(
                        "native TypeScript source {} is too large to fingerprint",
                        path.display()
                    ))
                })?;
                source_hasher.update(&buffer[..count]);
                fingerprint_hasher.update(&buffer[..count]);
            }

            let after = source_metadata(path).await?;
            if before != after || bytes_read != after.length {
                continue;
            }

            return Ok((
                super::super::hex_lower(&source_hasher.finalize()),
                Self {
                    metadata: after,
                    fingerprint: super::super::hex_lower(&fingerprint_hasher.finalize()),
                },
            ));
        }

        Err(FlowError::Runtime(format!(
            "native TypeScript source {} changed repeatedly while it was being read",
            path.display()
        )))
    }

    pub(in crate::runtime) async fn still_matches(
        &self,
        path: &Path,
        spec: &WorkflowSpec,
    ) -> Result<bool> {
        let (_, current) = Self::read(path, spec).await?;
        Ok(self == &current)
    }
}

fn source_hasher(spec: &WorkflowSpec, source_length: u64) -> Sha256 {
    let mut hasher = Sha256::new();
    for part in [
        b"source".as_slice(),
        spec.name.as_bytes(),
        spec.version.as_bytes(),
        spec.runtime.entrypoint.as_bytes(),
        spec.runtime.export_name.as_bytes(),
    ] {
        super::super::update_stable_hash_part(&mut hasher, part);
    }
    hasher.update(source_length.to_le_bytes());
    hasher
}

fn fingerprint_hasher(source_length: u64) -> Sha256 {
    let mut hasher = Sha256::new();
    super::super::update_stable_hash_part(&mut hasher, SOURCE_FINGERPRINT_DOMAIN);
    hasher.update(source_length.to_le_bytes());
    hasher
}

async fn source_metadata(path: &Path) -> Result<FileMetadata> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        FlowError::Runtime(format!(
            "native TypeScript source {} could not be inspected: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(FlowError::Runtime(format!(
            "native TypeScript source {} is not a regular file",
            path.display()
        )));
    }
    Ok(FileMetadata::from(&metadata))
}

#[cfg(test)]
mod tests {
    use super::SourceSnapshot;
    use crate::model::WorkflowSpec;

    #[tokio::test]
    async fn source_hash_uses_portable_u64_length_prefixes() {
        let directory = tempfile::tempdir().unwrap();
        let entrypoint = directory.path().join("workflow.ts");
        tokio::fs::write(
            &entrypoint,
            b"export async function main() { return 42; }\n",
        )
        .await
        .unwrap();
        let spec = WorkflowSpec::native_ts("portable.workflow", "1.2.3", "workflow.ts", "main");

        let (source_hash, _) = SourceSnapshot::read(&entrypoint, &spec).await.unwrap();

        assert_eq!(
            source_hash,
            "1f0e35a1cadd3012364a35a196c3e8ee9823191faa6254ac004a62963f32c814"
        );
    }
}
