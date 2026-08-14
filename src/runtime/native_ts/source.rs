use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use super::{FileMetadata, MAX_STABLE_READ_ATTEMPTS};
use crate::error::{FlowError, Result};
use crate::model::WorkflowSpec;

const SOURCE_FINGERPRINT_DOMAIN: &[u8] = b"a3s.flow.native_ts.source.contents.v1";
const SOURCE_GRAPH_DOMAIN: &[u8] = b"a3s.flow.native_ts.source.graph.v1";
const SOURCE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct DependencySource {
    logical_path: String,
    path: PathBuf,
}

impl DependencySource {
    pub(in crate::runtime) fn new(logical_path: String, path: PathBuf) -> Self {
        Self { logical_path, path }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceIdentityMode {
    EntrypointOnly,
    DependencyGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceFileSnapshot {
    metadata: FileMetadata,
    fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct SourceSnapshot {
    mode: SourceIdentityMode,
    sources: Vec<DependencySource>,
    files: Vec<SourceFileSnapshot>,
}

impl SourceSnapshot {
    pub(in crate::runtime) async fn read(
        path: &Path,
        spec: &WorkflowSpec,
    ) -> Result<(String, Self)> {
        for _ in 0..MAX_STABLE_READ_ATTEMPTS {
            let before = source_metadata(path).await?;
            let mut file = open_source_file(path).await?;
            let mut source_hasher = source_hasher(spec, before.length);
            let mut fingerprint_hasher = fingerprint_hasher(before.length);
            let bytes_read =
                stream_source(path, &mut file, &mut source_hasher, &mut fingerprint_hasher).await?;
            let after = source_metadata(path).await?;
            if before != after || bytes_read != after.length {
                continue;
            }

            return Ok((
                super::super::hex_lower(&source_hasher.finalize()),
                Self {
                    mode: SourceIdentityMode::EntrypointOnly,
                    sources: vec![DependencySource::new(
                        spec.runtime.entrypoint.clone(),
                        path.to_path_buf(),
                    )],
                    files: vec![SourceFileSnapshot {
                        metadata: after,
                        fingerprint: super::super::hex_lower(&fingerprint_hasher.finalize()),
                    }],
                },
            ));
        }

        Err(source_changed_error(path))
    }

    pub(in crate::runtime) async fn read_dependency_graph(
        sources: Vec<DependencySource>,
        spec: &WorkflowSpec,
    ) -> Result<(String, Self)> {
        if sources.is_empty() {
            return Err(FlowError::Runtime(
                "native TypeScript dependency manifest cannot be empty".to_string(),
            ));
        }

        let mut files = Vec::with_capacity(sources.len());
        for source in &sources {
            files.push(read_source_file(&source.path).await?);
        }
        let source_hash = dependency_graph_hash(spec, &sources, &files)?;

        Ok((
            source_hash,
            Self {
                mode: SourceIdentityMode::DependencyGraph,
                sources,
                files,
            },
        ))
    }

    pub(in crate::runtime) async fn still_matches(&self, spec: &WorkflowSpec) -> Result<bool> {
        let (_, current) = match self.mode {
            SourceIdentityMode::EntrypointOnly => {
                let source = self.sources.first().ok_or_else(|| {
                    FlowError::Runtime(
                        "native TypeScript entrypoint snapshot has no source".to_string(),
                    )
                })?;
                Self::read(&source.path, spec).await?
            }
            SourceIdentityMode::DependencyGraph => {
                Self::read_dependency_graph(self.sources.clone(), spec).await?
            }
        };
        Ok(self == &current)
    }

    pub(in crate::runtime) async fn still_matches_dependency_graph(
        &self,
        sources: Vec<DependencySource>,
        spec: &WorkflowSpec,
    ) -> Result<bool> {
        let (_, current) = Self::read_dependency_graph(sources, spec).await?;
        Ok(self == &current)
    }
}

async fn read_source_file(path: &Path) -> Result<SourceFileSnapshot> {
    for _ in 0..MAX_STABLE_READ_ATTEMPTS {
        let before = source_metadata(path).await?;
        let mut file = open_source_file(path).await?;
        let mut fingerprint_hasher = fingerprint_hasher(before.length);
        let mut ignored_hasher = Sha256::new();
        let bytes_read = stream_source(
            path,
            &mut file,
            &mut ignored_hasher,
            &mut fingerprint_hasher,
        )
        .await?;

        let after = source_metadata(path).await?;
        if before != after || bytes_read != after.length {
            continue;
        }

        return Ok(SourceFileSnapshot {
            metadata: after,
            fingerprint: super::super::hex_lower(&fingerprint_hasher.finalize()),
        });
    }

    Err(source_changed_error(path))
}

async fn open_source_file(path: &Path) -> Result<tokio::fs::File> {
    tokio::fs::File::open(path).await.map_err(|error| {
        FlowError::Runtime(format!(
            "native TypeScript source {} could not be read: {error}",
            path.display()
        ))
    })
}

async fn stream_source(
    path: &Path,
    file: &mut tokio::fs::File,
    first_hasher: &mut Sha256,
    second_hasher: &mut Sha256,
) -> Result<u64> {
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
            return Ok(bytes_read);
        }
        bytes_read = bytes_read.checked_add(count as u64).ok_or_else(|| {
            FlowError::Runtime(format!(
                "native TypeScript source {} is too large to fingerprint",
                path.display()
            ))
        })?;
        first_hasher.update(&buffer[..count]);
        second_hasher.update(&buffer[..count]);
    }
}

fn source_changed_error(path: &Path) -> FlowError {
    FlowError::Runtime(format!(
        "native TypeScript source {} changed repeatedly while it was being read",
        path.display()
    ))
}

fn dependency_graph_hash(
    spec: &WorkflowSpec,
    sources: &[DependencySource],
    files: &[SourceFileSnapshot],
) -> Result<String> {
    if sources.len() != files.len() {
        return Err(FlowError::Runtime(
            "native TypeScript dependency snapshot is internally inconsistent".to_string(),
        ));
    }

    let mut hasher = Sha256::new();
    for part in [
        SOURCE_GRAPH_DOMAIN,
        spec.name.as_bytes(),
        spec.version.as_bytes(),
        spec.runtime.entrypoint.as_bytes(),
        spec.runtime.export_name.as_bytes(),
    ] {
        super::super::update_stable_hash_part(&mut hasher, part);
    }
    hasher.update((sources.len() as u64).to_le_bytes());

    for (source, file) in sources.iter().zip(files) {
        super::super::update_stable_hash_part(&mut hasher, source.logical_path.as_bytes());
        hasher.update(file.metadata.length.to_le_bytes());
        super::super::update_stable_hash_part(&mut hasher, file.fingerprint.as_bytes());
    }

    Ok(super::super::hex_lower(&hasher.finalize()))
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
    use super::{DependencySource, SourceSnapshot};
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

    #[tokio::test]
    async fn dependency_graph_hash_changes_with_imported_source() {
        let directory = tempfile::tempdir().unwrap();
        let entrypoint = directory.path().join("workflow.ts");
        let dependency = directory.path().join("shared.ts");
        tokio::fs::write(&entrypoint, b"export { value } from './shared';\n")
            .await
            .unwrap();
        tokio::fs::write(&dependency, b"export const value = 1;\n")
            .await
            .unwrap();
        let spec = WorkflowSpec::native_ts("graph.workflow", "1.0.0", "workflow.ts", "main");
        let sources = vec![
            DependencySource::new("shared.ts".to_string(), dependency.clone()),
            DependencySource::new("workflow.ts".to_string(), entrypoint),
        ];

        let (first_hash, first_snapshot) =
            SourceSnapshot::read_dependency_graph(sources.clone(), &spec)
                .await
                .unwrap();
        tokio::fs::write(&dependency, b"export const value = 2;\n")
            .await
            .unwrap();
        let (second_hash, _) = SourceSnapshot::read_dependency_graph(sources, &spec)
            .await
            .unwrap();

        assert_ne!(first_hash, second_hash);
        assert!(!first_snapshot.still_matches(&spec).await.unwrap());
    }
}
