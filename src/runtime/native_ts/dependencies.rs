use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::error::{FlowError, Result};
use crate::protocol::{NativeTsDependencyManifest, NATIVE_DEPENDENCY_MANIFEST_PROTOCOL};

use super::source::DependencySource;

pub(in crate::runtime) const MAX_DEPENDENCY_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_DEPENDENCY_FILES: usize = 4096;
const MAX_LOGICAL_PATH_BYTES: usize = 4096;
const MAX_COMPILER_IDENTITY_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime) struct VerifiedDependencyGraph {
    pub(in crate::runtime) compiler_identity: String,
    pub(in crate::runtime) sources: Vec<DependencySource>,
}

pub(in crate::runtime) async fn verified_dependency_graph(
    bytes: &[u8],
    working_dir: &Path,
    entrypoint: &Path,
) -> Result<VerifiedDependencyGraph> {
    if bytes.is_empty() {
        return Err(manifest_error("compiler returned an empty document"));
    }
    if bytes.len() > MAX_DEPENDENCY_MANIFEST_BYTES {
        return Err(manifest_error(format!(
            "document is {} bytes, exceeding the {}-byte limit",
            bytes.len(),
            MAX_DEPENDENCY_MANIFEST_BYTES
        )));
    }

    let manifest: NativeTsDependencyManifest = serde_json::from_slice(bytes)
        .map_err(|error| manifest_error(format!("document is invalid JSON: {error}")))?;
    if manifest.protocol != NATIVE_DEPENDENCY_MANIFEST_PROTOCOL {
        return Err(manifest_error(format!(
            "protocol is {}, expected {NATIVE_DEPENDENCY_MANIFEST_PROTOCOL}",
            manifest.protocol
        )));
    }
    validate_compiler_identity(&manifest.compiler_identity)?;
    if manifest.files.is_empty() {
        return Err(manifest_error("file list cannot be empty"));
    }
    if manifest.files.len() > MAX_DEPENDENCY_FILES {
        return Err(manifest_error(format!(
            "file list contains {} entries, exceeding the {MAX_DEPENDENCY_FILES}-file limit",
            manifest.files.len()
        )));
    }

    let canonical_root = tokio::fs::canonicalize(working_dir)
        .await
        .map_err(|error| {
            manifest_error(format!(
                "working directory {} could not be resolved: {error}",
                working_dir.display()
            ))
        })?;
    let root_metadata = tokio::fs::metadata(&canonical_root)
        .await
        .map_err(|error| {
            manifest_error(format!(
                "working directory {} could not be inspected: {error}",
                canonical_root.display()
            ))
        })?;
    if !root_metadata.is_dir() {
        return Err(manifest_error(format!(
            "working directory {} is not a directory",
            canonical_root.display()
        )));
    }

    let canonical_entrypoint = tokio::fs::canonicalize(entrypoint).await.map_err(|error| {
        manifest_error(format!(
            "entrypoint {} could not be resolved: {error}",
            entrypoint.display()
        ))
    })?;
    if !canonical_entrypoint.starts_with(&canonical_root) {
        return Err(manifest_error(format!(
            "entrypoint {} is outside working directory {}",
            canonical_entrypoint.display(),
            canonical_root.display()
        )));
    }

    let mut previous: Option<&str> = None;
    let mut canonical_paths = HashSet::with_capacity(manifest.files.len());
    let mut sources = Vec::with_capacity(manifest.files.len());
    let mut contains_entrypoint = false;

    for logical_path in &manifest.files {
        validate_logical_path(logical_path)?;
        if previous.is_some_and(|value| value >= logical_path.as_str()) {
            return Err(manifest_error(
                "file list must be strictly sorted with no duplicates",
            ));
        }
        previous = Some(logical_path);

        let resolved = resolve_portable_path(&canonical_root, logical_path);
        let canonical = tokio::fs::canonicalize(&resolved).await.map_err(|error| {
            manifest_error(format!(
                "file {logical_path} could not be resolved: {error}"
            ))
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(manifest_error(format!(
                "file {logical_path} resolves outside working directory {}",
                canonical_root.display()
            )));
        }
        let metadata = tokio::fs::metadata(&canonical).await.map_err(|error| {
            manifest_error(format!(
                "file {logical_path} could not be inspected: {error}"
            ))
        })?;
        if !metadata.is_file() {
            return Err(manifest_error(format!(
                "file {logical_path} is not a regular file"
            )));
        }
        if !canonical_paths.insert(canonical.clone()) {
            return Err(manifest_error(format!(
                "file {logical_path} resolves to a dependency already listed"
            )));
        }
        contains_entrypoint |= canonical == canonical_entrypoint;
        // Keep the verified canonical target, not the unresolved manifest
        // path. A symlink retarget between validation and hashing must not let
        // the compiler redirect Flow outside the working directory.
        sources.push(DependencySource::new(logical_path.clone(), canonical));
    }

    if !contains_entrypoint {
        return Err(manifest_error(
            "file list does not contain the configured entrypoint",
        ));
    }

    Ok(VerifiedDependencyGraph {
        compiler_identity: manifest.compiler_identity,
        sources,
    })
}

fn validate_compiler_identity(identity: &str) -> Result<()> {
    if identity.is_empty() {
        return Err(manifest_error("compiler identity cannot be empty"));
    }
    if identity.len() > MAX_COMPILER_IDENTITY_BYTES {
        return Err(manifest_error(format!(
            "compiler identity is {} bytes, exceeding the {MAX_COMPILER_IDENTITY_BYTES}-byte limit",
            identity.len()
        )));
    }
    if identity.chars().any(char::is_control) {
        return Err(manifest_error(
            "compiler identity cannot contain control characters",
        ));
    }
    Ok(())
}

fn validate_logical_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(manifest_error("file path cannot be empty"));
    }
    if path.len() > MAX_LOGICAL_PATH_BYTES {
        return Err(manifest_error(format!(
            "file path is {} bytes, exceeding the {MAX_LOGICAL_PATH_BYTES}-byte limit",
            path.len()
        )));
    }
    if path.contains(['\\', '\0', ':']) || path.starts_with('/') {
        return Err(manifest_error(format!(
            "file path {path:?} is not a portable relative path"
        )));
    }

    let parsed = Path::new(path);
    if parsed
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
        || path.split('/').any(str::is_empty)
    {
        return Err(manifest_error(format!(
            "file path {path:?} is not normalized"
        )));
    }
    Ok(())
}

fn resolve_portable_path(root: &Path, logical_path: &str) -> PathBuf {
    logical_path
        .split('/')
        .fold(root.to_path_buf(), |path, component| path.join(component))
}

fn manifest_error(message: impl Into<String>) -> FlowError {
    FlowError::Runtime(format!(
        "native TypeScript dependency manifest {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::verified_dependency_graph;
    use crate::protocol::NativeTsDependencyManifest;

    #[tokio::test]
    async fn dependency_manifest_accepts_one_sorted_graph_under_the_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let source_dir = directory.path().join("src");
        tokio::fs::create_dir(&source_dir).await.unwrap();
        let entrypoint = source_dir.join("main.ts");
        tokio::fs::write(&entrypoint, b"export { value } from './shared';\n")
            .await
            .unwrap();
        tokio::fs::write(source_dir.join("shared.ts"), b"export const value = 1;\n")
            .await
            .unwrap();
        let bytes = serde_json::to_vec(&NativeTsDependencyManifest::new(
            "test-compiler-v1",
            vec!["src/main.ts".to_string(), "src/shared.ts".to_string()],
        ))
        .unwrap();

        let sources = verified_dependency_graph(&bytes, directory.path(), &entrypoint)
            .await
            .unwrap();

        assert_eq!(sources.compiler_identity, "test-compiler-v1");
        assert_eq!(sources.sources.len(), 2);
    }

    #[tokio::test]
    async fn dependency_manifest_rejects_protocol_order_traversal_and_missing_entrypoint() {
        let directory = tempfile::tempdir().unwrap();
        let entrypoint = directory.path().join("main.ts");
        tokio::fs::write(&entrypoint, b"export const value = 1;\n")
            .await
            .unwrap();
        tokio::fs::write(
            directory.path().join("shared.ts"),
            b"export const shared = 1;\n",
        )
        .await
        .unwrap();

        let cases = [
            serde_json::json!({
                "protocol": "a3s.flow.native_ts.dependencies.v2",
                "compilerIdentity": "test-compiler-v1",
                "files": ["main.ts"]
            }),
            serde_json::json!({
                "protocol": "a3s.flow.native_ts.dependencies.v1",
                "compilerIdentity": "test-compiler-v1",
                "files": ["shared.ts", "main.ts"]
            }),
            serde_json::json!({
                "protocol": "a3s.flow.native_ts.dependencies.v1",
                "compilerIdentity": "test-compiler-v1",
                "files": ["../main.ts"]
            }),
            serde_json::json!({
                "protocol": "a3s.flow.native_ts.dependencies.v1",
                "compilerIdentity": "test-compiler-v1",
                "files": ["shared.ts"]
            }),
            serde_json::json!({
                "protocol": "a3s.flow.native_ts.dependencies.v1",
                "compilerIdentity": "",
                "files": ["main.ts"]
            }),
        ];

        for value in cases {
            let error = verified_dependency_graph(
                &serde_json::to_vec(&value).unwrap(),
                directory.path(),
                &entrypoint,
            )
            .await
            .unwrap_err();
            assert!(
                error.to_string().contains("dependency manifest"),
                "unexpected error: {error}"
            );
        }
    }
}
