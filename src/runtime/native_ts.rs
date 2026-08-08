use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use tokio::sync::Mutex;

use crate::error::{FlowError, Result};

const MAX_STABLE_READ_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Default)]
pub(super) struct CompilerIdentityCache {
    cached: Arc<Mutex<Option<CachedCompilerIdentity>>>,
}

#[derive(Debug, Clone)]
struct CachedCompilerIdentity {
    path: PathBuf,
    metadata: CompilerMetadata,
    fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompilerMetadata {
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

            let contents = tokio::fs::read(&path).await.map_err(|error| {
                FlowError::Runtime(format!(
                    "native TypeScript compiler {} could not be fingerprinted: {error}",
                    path.display()
                ))
            })?;
            let after = compiler_metadata(&path).await?;
            if before != after {
                continue;
            }

            let fingerprint = super::stable_hash([
                b"a3s.flow.native_ts.compiler.v1".as_slice(),
                contents.as_slice(),
            ]);
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

async fn compiler_metadata(path: &Path) -> Result<CompilerMetadata> {
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
    Ok(CompilerMetadata::from(&metadata))
}

impl From<&std::fs::Metadata> for CompilerMetadata {
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

impl CompilerMetadata {
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
