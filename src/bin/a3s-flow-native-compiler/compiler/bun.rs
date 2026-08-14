use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

const BUN_ENV: &str = "A3S_FLOW_BUN";
pub(crate) const BUN_SUPERVISOR_COMMAND: &str = "__run-bun-supervised";
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(10);
const BUN_FINGERPRINT_BUFFER_BYTES: usize = 64 * 1024;
const MAX_STABLE_BUN_READ_ATTEMPTS: usize = 3;
const BUN_IDENTITY_DOMAIN: &[u8] = b"a3s.flow.native_ts.compiler.backend.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
struct BunFileMetadata {
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

#[derive(Debug)]
pub(super) struct ResolvedBun {
    path: PathBuf,
    fingerprint: String,
    metadata: BunFileMetadata,
}

impl From<&fs::Metadata> for BunFileMetadata {
    fn from(metadata: &fs::Metadata) -> Self {
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

impl ResolvedBun {
    pub(super) fn resolve() -> Result<Self, String> {
        let configured = std::env::var_os(BUN_ENV).unwrap_or_else(|| OsString::from("bun"));
        let path = resolve_executable(&configured)?;
        let (fingerprint, metadata) = fingerprint_bun(&path)?;
        Ok(Self {
            path,
            fingerprint,
            metadata,
        })
    }

    pub(super) fn compiler_identity(&self) -> String {
        format!("bun-sha256:{}", self.fingerprint)
    }

    pub(super) fn run(
        &self,
        working_directory: &Path,
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<(), String> {
        run_bun(working_directory, &self.path, arguments)?;
        self.verify_unchanged()
    }

    fn verify_unchanged(&self) -> Result<(), String> {
        let current = bun_metadata(&self.path)?;
        if current != self.metadata {
            return Err(format!(
                "Bun executable {} changed while the compiler command was running",
                self.path.display()
            ));
        }
        Ok(())
    }
}

fn run_bun(
    working_directory: &Path,
    binary: &Path,
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not resolve the compiler executable: {error}"))?;
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let mut supervisor = Command::new(&executable)
        .arg(BUN_SUPERVISOR_COMMAND)
        .arg(working_directory)
        .arg(binary)
        .args(&arguments)
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start Bun supervisor {}: {error}",
                executable.display()
            )
        })?;
    // Keeping this pipe open is the supervisor's parent-liveness signal. If
    // Flow cancels or kills this compiler process, the operating system closes
    // the handle and the supervisor terminates and reaps Bun.
    let parent_liveness = supervisor.stdin.take().ok_or_else(|| {
        let _ = supervisor.kill();
        let _ = supervisor.wait();
        "Bun supervisor did not expose its parent-liveness pipe".to_string()
    })?;
    let status = supervisor.wait().map_err(|error| {
        let _ = supervisor.kill();
        let _ = supervisor.wait();
        format!("could not wait for Bun supervisor: {error}")
    })?;
    drop(parent_liveness);
    if !status.success() {
        return Err(format!(
            "Bun supervisor exited unsuccessfully with status {status}; Bun executable was {} (override with {BUN_ENV})",
            binary.display()
        ));
    }
    Ok(())
}

pub(crate) fn run_bun_supervised(
    working_directory: &Path,
    binary: &OsStr,
    arguments: &[OsString],
) -> Result<(), String> {
    let child = Command::new(binary)
        .args(arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            format!(
                "could not start Bun executable {}: {error}; set {BUN_ENV} to an explicit path",
                Path::new(binary).display()
            )
        })?;
    let parent_disconnected = Arc::new(AtomicBool::new(false));
    watch_parent_liveness(Arc::clone(&parent_disconnected));
    let status = wait_for_supervised_child(child, &parent_disconnected)?;
    if !status.success() {
        return Err(format!("Bun exited unsuccessfully with status {status}"));
    }
    Ok(())
}

fn watch_parent_liveness(parent_disconnected: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        let mut buffer = [0_u8; 1];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        parent_disconnected.store(true, Ordering::Release);
    });
}

fn wait_for_supervised_child(
    mut child: Child,
    parent_disconnected: &AtomicBool,
) -> Result<ExitStatus, String> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not inspect the Bun process: {error}"));
            }
        }

        if parent_disconnected.load(Ordering::Acquire) {
            let kill_result = child.kill();
            let wait_result = child.wait();
            if let Err(error) = kill_result {
                if wait_result.as_ref().is_err() {
                    return Err(format!(
                        "compiler parent disconnected, but Bun could not be terminated: {error}"
                    ));
                }
            }
            if let Err(error) = wait_result {
                return Err(format!(
                    "compiler parent disconnected, but Bun could not be reaped: {error}"
                ));
            }
            return Err("compiler parent disconnected while Bun was running".to_string());
        }

        thread::sleep(SUPERVISOR_POLL_INTERVAL);
    }
}

fn resolve_executable(configured: &OsStr) -> Result<PathBuf, String> {
    let configured_path = Path::new(configured);
    let has_directory = configured_path.is_absolute()
        || configured_path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
    if has_directory {
        for candidate in executable_candidates(configured_path) {
            if let Ok(path) = canonical_executable(&candidate) {
                return Ok(path);
            }
        }
        return Err(format!(
            "Bun executable {} could not be resolved to an executable file; set {BUN_ENV} to an explicit path",
            configured_path.display()
        ));
    }

    let Some(path_value) = std::env::var_os("PATH") else {
        return Err(format!(
            "Bun executable {} was not found because PATH is unset; set {BUN_ENV} to an explicit path",
            configured_path.display()
        ));
    };
    for directory in std::env::split_paths(&path_value) {
        for candidate in executable_candidates(&directory.join(configured_path)) {
            if let Ok(path) = canonical_executable(&candidate) {
                return Ok(path);
            }
        }
    }

    Err(format!(
        "Bun executable {} was not found on PATH; set {BUN_ENV} to an explicit path",
        configured_path.display()
    ))
}

fn executable_candidates(path: &Path) -> Vec<PathBuf> {
    let candidates = vec![path.to_path_buf()];
    #[cfg(not(windows))]
    return candidates;

    #[cfg(windows)]
    {
        if path.extension().is_some() {
            return candidates;
        }
        let mut candidates = candidates;
        let path_extensions =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        for extension in path_extensions.to_string_lossy().split(';') {
            let extension = extension.trim().trim_start_matches('.');
            if extension.is_empty() {
                continue;
            }
            let mut candidate = path.to_path_buf();
            candidate.set_extension(extension);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
        candidates
    }
}

fn canonical_executable(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        format!(
            "could not resolve Bun executable {}: {error}",
            path.display()
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        format!(
            "could not inspect Bun executable {}: {error}",
            canonical.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "Bun executable {} is not a regular file",
            canonical.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "Bun executable {} does not have an execute permission bit",
                canonical.display()
            ));
        }
    }
    Ok(canonical)
}

fn fingerprint_bun(path: &Path) -> Result<(String, BunFileMetadata), String> {
    for _ in 0..MAX_STABLE_BUN_READ_ATTEMPTS {
        let before = bun_metadata(path)?;
        let mut file = fs::File::open(path).map_err(|error| {
            format!("could not read Bun executable {}: {error}", path.display())
        })?;
        let mut hasher = Sha256::new();
        hasher.update((BUN_IDENTITY_DOMAIN.len() as u64).to_le_bytes());
        hasher.update(BUN_IDENTITY_DOMAIN);
        hasher.update(before.length.to_le_bytes());
        let mut bytes_read = 0_u64;
        let mut buffer = vec![0_u8; BUN_FINGERPRINT_BUFFER_BYTES];
        loop {
            let count = file.read(&mut buffer).map_err(|error| {
                format!(
                    "could not fingerprint Bun executable {}: {error}",
                    path.display()
                )
            })?;
            if count == 0 {
                break;
            }
            bytes_read = bytes_read.checked_add(count as u64).ok_or_else(|| {
                format!(
                    "Bun executable {} is too large to fingerprint",
                    path.display()
                )
            })?;
            hasher.update(&buffer[..count]);
        }
        let after = bun_metadata(path)?;
        if before == after && bytes_read == after.length {
            return Ok((hex_lower(&hasher.finalize()), after));
        }
    }

    Err(format!(
        "Bun executable {} changed repeatedly while it was being fingerprinted",
        path.display()
    ))
}

fn bun_metadata(path: &Path) -> Result<BunFileMetadata, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        format!(
            "could not inspect Bun executable {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "Bun executable {} is not a regular file",
            path.display()
        ));
    }
    Ok(BunFileMetadata::from(&metadata))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{fingerprint_bun, wait_for_supervised_child};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn backend_fingerprint_changes_with_executable_contents() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("bun-test-binary");
        std::fs::write(&executable, b"first backend").unwrap();
        let first = fingerprint_bun(&executable).unwrap().0;
        std::fs::write(&executable, b"second backend").unwrap();
        let second = fingerprint_bun(&executable).unwrap().0;

        assert_ne!(first, second);
        assert!(first.len() == 64 && second.len() == 64);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn supervisor_terminates_and_reaps_bun_when_its_parent_disconnects() {
        let child = blocking_test_child();
        let started = Instant::now();

        let error = wait_for_supervised_child(child, &AtomicBool::new(true)).unwrap_err();

        assert!(error.contains("parent disconnected"));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "supervisor must not wait for the blocking child to exit naturally"
        );
    }

    #[cfg(unix)]
    fn blocking_test_child() -> Child {
        Command::new("sh")
            .args(["-c", "exec sleep 30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(windows)]
    fn blocking_test_child() -> Child {
        Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
}
