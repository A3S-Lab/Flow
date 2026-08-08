use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::{FlowError, Result};

const ARTIFACT_FILE_NAME: &str = "artifact";
const ARTIFACT_MANIFEST_FILE_NAME: &str = "manifest.json";
const ARTIFACT_MANIFEST_FORMAT: &str = "a3s.flow.native_ts.cache.v1";
const MAX_ARTIFACT_MANIFEST_BYTES: u64 = 16 * 1024;
const COMPILER_FINGERPRINT_DOMAIN: &[u8] = b"a3s.flow.native_ts.compiler.v2";
const ARTIFACT_FINGERPRINT_DOMAIN: &[u8] = b"a3s.flow.native_ts.artifact.contents.v1";
const MAX_STABLE_READ_ATTEMPTS: usize = 3;
const MAX_PUBLISH_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Default)]
pub(super) struct CompilerIdentityCache {
    cached: Arc<Mutex<Option<CachedCompilerIdentity>>>,
}

#[derive(Debug, Clone)]
struct CachedCompilerIdentity {
    path: PathBuf,
    metadata: FileMetadata,
    fingerprint: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ArtifactCache {
    validated: Arc<Mutex<HashMap<PathBuf, CachedArtifactValidation>>>,
}

#[derive(Debug, Clone)]
struct CachedArtifactValidation {
    cache_key: String,
    artifact_metadata: FileMetadata,
    manifest_metadata: FileMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileMetadata {
    length: u64,
    modified: Option<SystemTime>,
    #[cfg(not(unix))]
    created: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactManifest {
    format: String,
    cache_key: String,
    length: u64,
    fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ArtifactCacheState {
    Missing,
    Invalid(String),
    Valid,
}

pub(super) struct TemporaryCacheEntryGuard {
    path: PathBuf,
    binary: PathBuf,
    armed: bool,
}

impl CompilerIdentityCache {
    pub(super) async fn resolve_and_fingerprint(
        &self,
        configured: &Path,
    ) -> Result<(PathBuf, String)> {
        let path = resolve_compiler_binary(configured).await?;
        let mut cached = self.cached.lock().await;

        for _ in 0..MAX_STABLE_READ_ATTEMPTS {
            let before = compiler_metadata(&path).await?;
            if let Some(identity) = cached.as_ref() {
                if identity.path == path
                    && identity.metadata == before
                    && before.can_cache_fingerprint()
                {
                    return Ok((path, identity.fingerprint.clone()));
                }
            }

            let Some(fingerprint) = fingerprint_file(
                &path,
                COMPILER_FINGERPRINT_DOMAIN,
                before.length,
                "compiler",
            )
            .await?
            else {
                continue;
            };
            let after = compiler_metadata(&path).await?;
            if before != after {
                continue;
            }

            *cached = Some(CachedCompilerIdentity {
                path: path.clone(),
                metadata: after,
                fingerprint: fingerprint.clone(),
            });
            return Ok((path, fingerprint));
        }

        Err(FlowError::Runtime(format!(
            "native TypeScript compiler {} changed while its cache identity was being calculated",
            path.display()
        )))
    }
}

impl ArtifactCache {
    pub(super) async fn inspect(
        &self,
        entry: &Path,
        cache_key: &str,
    ) -> Result<ArtifactCacheState> {
        let entry_metadata = match tokio::fs::symlink_metadata(entry).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ArtifactCacheState::Missing);
            }
            Err(error) => {
                return Err(cache_io_error("inspect", entry, error));
            }
        };
        if !entry_metadata.is_dir() || entry_metadata.file_type().is_symlink() {
            return Ok(ArtifactCacheState::Invalid(
                "cache entry is not a regular directory".to_string(),
            ));
        }

        let artifact = artifact_binary_path(entry);
        let manifest_path = artifact_manifest_path(entry);
        for _ in 0..MAX_STABLE_READ_ATTEMPTS {
            let artifact_metadata = match cache_file_metadata(&artifact, true).await? {
                Ok(metadata) => metadata,
                Err(reason) => return Ok(ArtifactCacheState::Invalid(reason)),
            };
            let manifest_metadata = match cache_file_metadata(&manifest_path, false).await? {
                Ok(metadata) => metadata,
                Err(reason) => return Ok(ArtifactCacheState::Invalid(reason)),
            };
            if manifest_metadata.length == 0 {
                return Ok(ArtifactCacheState::Invalid(
                    "integrity manifest is empty".to_string(),
                ));
            }
            if manifest_metadata.length > MAX_ARTIFACT_MANIFEST_BYTES {
                return Ok(ArtifactCacheState::Invalid(format!(
                    "integrity manifest is {} bytes, exceeding the {}-byte limit",
                    manifest_metadata.length, MAX_ARTIFACT_MANIFEST_BYTES
                )));
            }

            if artifact_metadata.can_cache_fingerprint()
                && manifest_metadata.can_cache_fingerprint()
            {
                let validated = self.validated.lock().await;
                if validated.get(entry).is_some_and(|cached| {
                    cached.cache_key == cache_key
                        && cached.artifact_metadata == artifact_metadata
                        && cached.manifest_metadata == manifest_metadata
                }) {
                    return Ok(ArtifactCacheState::Valid);
                }
            }

            let manifest_bytes = match tokio::fs::read(&manifest_path).await {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(cache_io_error("read manifest for", &manifest_path, error));
                }
            };
            let manifest: ArtifactManifest = match serde_json::from_slice(&manifest_bytes) {
                Ok(manifest) => manifest,
                Err(error) => {
                    return Ok(ArtifactCacheState::Invalid(format!(
                        "integrity manifest is invalid JSON: {error}"
                    )));
                }
            };
            if manifest.format != ARTIFACT_MANIFEST_FORMAT {
                return Ok(ArtifactCacheState::Invalid(format!(
                    "integrity manifest format is {}, expected {ARTIFACT_MANIFEST_FORMAT}",
                    manifest.format
                )));
            }
            if manifest.cache_key != cache_key {
                return Ok(ArtifactCacheState::Invalid(
                    "integrity manifest belongs to a different cache key".to_string(),
                ));
            }
            if manifest.length != artifact_metadata.length {
                return Ok(ArtifactCacheState::Invalid(format!(
                    "artifact length is {}, integrity manifest records {}",
                    artifact_metadata.length, manifest.length
                )));
            }

            let Some(fingerprint) = fingerprint_file(
                &artifact,
                ARTIFACT_FINGERPRINT_DOMAIN,
                artifact_metadata.length,
                "cached artifact",
            )
            .await?
            else {
                continue;
            };
            let artifact_after = match cache_file_metadata(&artifact, true).await? {
                Ok(metadata) => metadata,
                Err(reason) => return Ok(ArtifactCacheState::Invalid(reason)),
            };
            let manifest_after = match cache_file_metadata(&manifest_path, false).await? {
                Ok(metadata) => metadata,
                Err(reason) => return Ok(ArtifactCacheState::Invalid(reason)),
            };
            if artifact_metadata != artifact_after || manifest_metadata != manifest_after {
                continue;
            }
            if fingerprint != manifest.fingerprint {
                return Ok(ArtifactCacheState::Invalid(
                    "artifact contents do not match the integrity manifest".to_string(),
                ));
            }

            self.validated.lock().await.insert(
                entry.to_path_buf(),
                CachedArtifactValidation {
                    cache_key: cache_key.to_string(),
                    artifact_metadata: artifact_after,
                    manifest_metadata: manifest_after,
                },
            );
            return Ok(ArtifactCacheState::Valid);
        }

        Ok(ArtifactCacheState::Invalid(
            "cache entry changed repeatedly while it was being validated".to_string(),
        ))
    }

    pub(super) async fn prepare(&self, entry: &Path, cache_key: &str) -> Result<()> {
        let artifact = artifact_binary_path(entry);
        for _ in 0..MAX_STABLE_READ_ATTEMPTS {
            let before = compiled_artifact_metadata(&artifact).await?;
            let Some(fingerprint) = fingerprint_file(
                &artifact,
                ARTIFACT_FINGERPRINT_DOMAIN,
                before.length,
                "compiler output",
            )
            .await?
            else {
                continue;
            };
            let after = compiled_artifact_metadata(&artifact).await?;
            if before != after {
                continue;
            }

            let manifest = ArtifactManifest {
                format: ARTIFACT_MANIFEST_FORMAT.to_string(),
                cache_key: cache_key.to_string(),
                length: after.length,
                fingerprint,
            };
            let manifest_bytes = serde_json::to_vec(&manifest)?;
            let manifest_path = artifact_manifest_path(entry);
            tokio::fs::write(&manifest_path, manifest_bytes)
                .await
                .map_err(|error| cache_io_error("write manifest for", &manifest_path, error))?;
            return Ok(());
        }

        Err(FlowError::Runtime(format!(
            "native TypeScript compiler output {} changed while its integrity manifest was being created",
            artifact.display()
        )))
    }

    pub(super) async fn discard(&self, entry: &Path) -> Result<()> {
        self.validated.lock().await.remove(entry);
        let quarantine = temporary_sibling_path(entry, "invalid")?;
        match tokio::fs::rename(entry, &quarantine).await {
            Ok(()) => {
                if let Err(error) = remove_cache_path(&quarantine).await {
                    tracing::warn!(
                        path = %quarantine.display(),
                        %error,
                        "failed to remove quarantined native TypeScript cache entry"
                    );
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(FlowError::Runtime(format!(
                "native TypeScript cache entry {} could not be quarantined: {error}",
                entry.display()
            ))),
        }
    }

    pub(super) async fn publish(
        &self,
        temporary: &Path,
        entry: &Path,
        cache_key: &str,
    ) -> Result<()> {
        let mut last_rename_error = None;
        for _ in 0..MAX_PUBLISH_ATTEMPTS {
            match tokio::fs::rename(temporary, entry).await {
                Ok(()) => {
                    self.validated.lock().await.remove(entry);
                    return Ok(());
                }
                Err(error) => last_rename_error = Some(error),
            }

            match self.inspect(entry, cache_key).await? {
                ArtifactCacheState::Valid => {
                    remove_cache_path(temporary).await?;
                    return Ok(());
                }
                ArtifactCacheState::Missing => continue,
                ArtifactCacheState::Invalid(reason) => {
                    tracing::warn!(
                        path = %entry.display(),
                        %reason,
                        "replacing invalid native TypeScript cache entry during publication"
                    );
                    self.discard(entry).await?;
                }
            }
        }

        let rename_error = last_rename_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown rename failure".to_string());
        Err(FlowError::Runtime(format!(
            "native TypeScript cache entry {} could not be published atomically: {rename_error}",
            entry.display()
        )))
    }
}

impl TemporaryCacheEntryGuard {
    pub(super) async fn create(entry: &Path) -> Result<Self> {
        let path = temporary_sibling_path(entry, "tmp")?;
        tokio::fs::create_dir(&path).await.map_err(|error| {
            FlowError::Runtime(format!(
                "temporary native TypeScript cache entry {} could not be created: {error}",
                path.display()
            ))
        })?;
        Ok(Self {
            binary: artifact_binary_path(&path),
            path,
            armed: true,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn binary(&self) -> &Path {
        &self.binary
    }

    pub(super) async fn remove(&mut self) {
        if let Err(error) = remove_cache_path(&self.path).await {
            tracing::warn!(
                path = %self.path.display(),
                %error,
                "failed to remove temporary native TypeScript cache entry"
            );
        }
        self.armed = false;
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryCacheEntryGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let path = self.path.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let _cleanup = runtime.spawn(async move {
                    if let Err(error) = remove_cache_path(&path).await {
                        tracing::warn!(
                            path = %path.display(),
                            %error,
                            "failed to remove cancelled native TypeScript cache entry"
                        );
                    }
                });
            }
            Err(error) => tracing::warn!(
                path = %self.path.display(),
                %error,
                "failed to schedule cancelled native TypeScript cache cleanup"
            ),
        }
    }
}

pub(super) fn artifact_binary_path(entry: &Path) -> PathBuf {
    entry.join(ARTIFACT_FILE_NAME)
}

fn artifact_manifest_path(entry: &Path) -> PathBuf {
    entry.join(ARTIFACT_MANIFEST_FILE_NAME)
}

async fn resolve_compiler_binary(configured: &Path) -> Result<PathBuf> {
    if configured.components().count() != 1 {
        return absolute_from_current_dir(configured);
    }

    let search_path = std::env::var_os("PATH").ok_or_else(|| {
        FlowError::Runtime(format!(
            "native TypeScript compiler {} could not be resolved because PATH is unset",
            configured.display()
        ))
    })?;
    let current_dir = std::env::current_dir()?;
    let file_names = executable_file_names(configured.as_os_str());
    for directory in std::env::split_paths(&search_path) {
        let directory = if directory.is_absolute() {
            directory
        } else {
            current_dir.join(directory)
        };
        for file_name in &file_names {
            let candidate = directory.join(file_name);
            let Ok(metadata) = tokio::fs::metadata(&candidate).await else {
                continue;
            };
            if compiler_is_executable(&metadata) {
                return Ok(candidate);
            }
        }
    }

    Err(FlowError::Runtime(format!(
        "native TypeScript compiler {} was not found in PATH",
        configured.display()
    )))
}

fn absolute_from_current_dir(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

#[cfg(not(windows))]
fn executable_file_names(name: &OsStr) -> Vec<OsString> {
    vec![name.to_os_string()]
}

#[cfg(windows)]
fn executable_file_names(name: &OsStr) -> Vec<OsString> {
    if Path::new(name).extension().is_some() {
        return vec![name.to_os_string()];
    }

    let mut names = Vec::new();
    let extensions =
        std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
    for extension in extensions.to_string_lossy().split(';') {
        if extension.is_empty() {
            continue;
        }
        let mut file_name = name.to_os_string();
        file_name.push(extension);
        names.push(file_name);
    }
    names
}

#[cfg(unix)]
fn compiler_is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn compiler_is_executable(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
}

#[cfg(unix)]
fn artifact_is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn artifact_is_executable(_metadata: &std::fs::Metadata) -> bool {
    true
}

async fn compiler_metadata(path: &Path) -> Result<FileMetadata> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        FlowError::Runtime(format!(
            "native TypeScript compiler {} could not be inspected: {error}",
            path.display()
        ))
    })?;
    if !compiler_is_executable(&metadata) {
        return Err(FlowError::Runtime(format!(
            "native TypeScript compiler {} is not an executable file",
            path.display()
        )));
    }
    Ok(FileMetadata::from(&metadata))
}

async fn compiled_artifact_metadata(path: &Path) -> Result<FileMetadata> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        FlowError::Runtime(format!(
            "native TypeScript compiler did not produce a usable artifact {}: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(FlowError::Runtime(format!(
            "native TypeScript compiler output {} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(FlowError::Runtime(format!(
            "native TypeScript compiler output {} is empty",
            path.display()
        )));
    }
    if !artifact_is_executable(&metadata) {
        return Err(FlowError::Runtime(format!(
            "native TypeScript compiler output {} is not executable",
            path.display()
        )));
    }
    Ok(FileMetadata::from(&metadata))
}

async fn cache_file_metadata(
    path: &Path,
    require_executable: bool,
) -> Result<std::result::Result<FileMetadata, String>> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Err(format!("{} is missing", path.display())));
        }
        Err(error) => return Err(cache_io_error("inspect", path, error)),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(Err(format!("{} is not a regular file", path.display())));
    }
    if require_executable && metadata.len() == 0 {
        return Ok(Err(format!("{} is empty", path.display())));
    }
    if require_executable && !artifact_is_executable(&metadata) {
        return Ok(Err(format!("{} is not executable", path.display())));
    }
    Ok(Ok(FileMetadata::from(&metadata)))
}

async fn fingerprint_file(
    path: &Path,
    domain: &[u8],
    expected_length: u64,
    subject: &str,
) -> Result<Option<String>> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(FlowError::Runtime(format!(
                "native TypeScript {subject} {} could not be fingerprinted: {error}",
                path.display()
            )));
        }
    };
    let mut hasher = Sha256::new();
    hasher.update((domain.len() as u64).to_le_bytes());
    hasher.update(domain);
    hasher.update(expected_length.to_le_bytes());
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).await.map_err(|error| {
            FlowError::Runtime(format!(
                "native TypeScript {subject} {} could not be fingerprinted: {error}",
                path.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(Some(super::hex_lower(&hasher.finalize())))
}

fn temporary_sibling_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = path.file_name().ok_or_else(|| {
        FlowError::Runtime(format!(
            "native TypeScript cache entry {} has no file name",
            path.display()
        ))
    })?;
    let temporary_name = format!(
        ".{}.{}.{}",
        file_name.to_string_lossy(),
        Uuid::new_v4(),
        suffix
    );
    Ok(path.with_file_name(temporary_name))
}

async fn remove_cache_path(path: &Path) -> Result<()> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(cache_io_error("inspect for removal", path, error)),
    };
    let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        tokio::fs::remove_dir_all(path).await
    } else {
        tokio::fs::remove_file(path).await
    };
    result.map_err(|error| cache_io_error("remove", path, error))
}

fn cache_io_error(operation: &str, path: &Path, error: std::io::Error) -> FlowError {
    FlowError::Runtime(format!(
        "native TypeScript cache could not {operation} {}: {error}",
        path.display()
    ))
}

impl From<&std::fs::Metadata> for FileMetadata {
    fn from(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(not(unix))]
            created: metadata.created().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

impl FileMetadata {
    #[cfg(unix)]
    fn can_cache_fingerprint(&self) -> bool {
        true
    }

    #[cfg(not(unix))]
    fn can_cache_fingerprint(&self) -> bool {
        self.modified.is_some() || self.created.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_artifact_cache_key;
    use std::path::Path;

    #[test]
    fn artifact_cache_key_covers_the_compile_environment() {
        let identity =
            |source, compiler, compiler_fingerprint, working_dir, entrypoint, protocol| {
                native_artifact_cache_key(
                    source,
                    Path::new(compiler),
                    compiler_fingerprint,
                    Path::new(working_dir),
                    Path::new(entrypoint),
                    protocol,
                )
            };
        let baseline = identity(
            "source-a",
            "/compiler-a",
            "fingerprint-a",
            "/workspace-a",
            "/workspace-a/workflow.ts",
            "protocol-a",
        );
        let variants = [
            (
                "source-b",
                "/compiler-a",
                "fingerprint-a",
                "/workspace-a",
                "/workspace-a/workflow.ts",
                "protocol-a",
            ),
            (
                "source-a",
                "/compiler-b",
                "fingerprint-a",
                "/workspace-a",
                "/workspace-a/workflow.ts",
                "protocol-a",
            ),
            (
                "source-a",
                "/compiler-a",
                "fingerprint-b",
                "/workspace-a",
                "/workspace-a/workflow.ts",
                "protocol-a",
            ),
            (
                "source-a",
                "/compiler-a",
                "fingerprint-a",
                "/workspace-b",
                "/workspace-a/workflow.ts",
                "protocol-a",
            ),
            (
                "source-a",
                "/compiler-a",
                "fingerprint-a",
                "/workspace-a",
                "/workspace-b/workflow.ts",
                "protocol-a",
            ),
            (
                "source-a",
                "/compiler-a",
                "fingerprint-a",
                "/workspace-a",
                "/workspace-a/workflow.ts",
                "protocol-b",
            ),
        ];

        for (source, compiler, compiler_fingerprint, working_dir, entrypoint, protocol) in variants
        {
            assert_ne!(
                identity(
                    source,
                    compiler,
                    compiler_fingerprint,
                    working_dir,
                    entrypoint,
                    protocol,
                ),
                baseline
            );
        }
    }
}
