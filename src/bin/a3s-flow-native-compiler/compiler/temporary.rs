use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::Duration;

use uuid::Uuid;

pub(crate) const TEMPORARY_GUARD_COMMAND: &str = "__guard-temporary-workspace";
const TEMPORARY_PREFIX: &str = "a3s-flow-native-compiler-";
const CLEANUP_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const MAX_CLEANUP_ATTEMPTS: usize = 40;

pub(super) struct TemporaryWorkspace {
    path: PathBuf,
    guard: Option<Child>,
    guard_signal: Option<ChildStdin>,
}

impl TemporaryWorkspace {
    pub(super) fn create() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("{TEMPORARY_PREFIX}{}", Uuid::new_v4()));
        fs::create_dir(&path).map_err(|error| {
            format!(
                "could not create compiler temporary directory {}: {error}",
                path.display()
            )
        })?;

        let executable = std::env::current_exe().map_err(|error| {
            remove_workspace_with_retries(&path).ok();
            format!("could not resolve compiler executable for temporary cleanup: {error}")
        })?;
        let mut guard = Command::new(&executable)
            .arg(TEMPORARY_GUARD_COMMAND)
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                remove_workspace_with_retries(&path).ok();
                format!(
                    "could not start temporary-workspace guard {}: {error}",
                    executable.display()
                )
            })?;
        let Some(guard_signal) = guard.stdin.take() else {
            let _ = guard.kill();
            let _ = guard.wait();
            remove_workspace_with_retries(&path).ok();
            return Err("temporary-workspace guard did not expose its liveness pipe".to_string());
        };

        Ok(Self {
            path,
            guard: Some(guard),
            guard_signal: Some(guard_signal),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        if let Some(mut signal) = self.guard_signal.take() {
            let _ = signal.write_all(b"D");
            let _ = signal.flush();
        }
        if let Some(mut guard) = self.guard.take() {
            if let Err(error) = guard.wait() {
                let _ = guard.kill();
                let _ = guard.wait();
                eprintln!(
                    "a3s-flow-native-compiler: could not wait for temporary-workspace guard: {error}"
                );
            }
        }
        if let Err(error) = remove_workspace_with_retries(&self.path) {
            eprintln!(
                "a3s-flow-native-compiler: could not remove temporary directory {}: {error}",
                self.path.display()
            );
        }
    }
}

pub(crate) fn guard_temporary_workspace(path: &Path) -> Result<(), String> {
    let mut input = io::stdin().lock();
    guard_temporary_workspace_from(path, &mut input)
}

fn guard_temporary_workspace_from(path: &Path, input: &mut impl Read) -> Result<(), String> {
    let path = validate_guarded_workspace(path)?;
    let mut buffer = [0_u8; 1];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => return remove_workspace_with_retries(&path),
            Ok(_) if buffer[0] == b'D' => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return remove_workspace_with_retries(&path),
        }
    }
}

fn validate_guarded_workspace(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("temporary-workspace guard requires an absolute path".to_string());
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "temporary-workspace guard path has no UTF-8 file name".to_string())?;
    let identifier = file_name
        .strip_prefix(TEMPORARY_PREFIX)
        .ok_or_else(|| "temporary-workspace guard path has an invalid prefix".to_string())?;
    Uuid::parse_str(identifier)
        .map_err(|_| "temporary-workspace guard path has an invalid identifier".to_string())?;

    let expected_parent = fs::canonicalize(std::env::temp_dir())
        .map_err(|error| format!("could not resolve system temporary directory: {error}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "temporary-workspace guard path has no parent".to_string())?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        format!(
            "could not resolve temporary-workspace parent {}: {error}",
            parent.display()
        )
    })?;
    if canonical_parent != expected_parent {
        return Err(format!(
            "temporary-workspace guard path {} is outside the system temporary directory",
            path.display()
        ));
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "temporary-workspace guard path {} cannot be a symlink",
            path.display()
        )),
        Ok(metadata) if !metadata.is_dir() => Err(format!(
            "temporary-workspace guard path {} is not a directory",
            path.display()
        )),
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(format!(
            "could not inspect temporary-workspace guard path {}: {error}",
            path.display()
        )),
    }
}

fn remove_workspace_with_retries(path: &Path) -> Result<(), String> {
    let mut last_error = None;
    for attempt in 0..MAX_CLEANUP_ATTEMPTS {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < MAX_CLEANUP_ATTEMPTS {
            thread::sleep(CLEANUP_RETRY_INTERVAL);
        }
    }
    let error = last_error
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown cleanup error".to_string());
    Err(format!("could not remove {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        guard_temporary_workspace_from, remove_workspace_with_retries, validate_guarded_workspace,
        TEMPORARY_PREFIX,
    };
    use std::io::Cursor;
    use uuid::Uuid;

    #[test]
    fn cleanup_accepts_only_uuid_workspaces_directly_under_system_temp() {
        let path = std::env::temp_dir().join(format!("{TEMPORARY_PREFIX}{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("partial"), b"partial").unwrap();

        assert_eq!(validate_guarded_workspace(&path).unwrap(), path);
        remove_workspace_with_retries(&path).unwrap();
        assert!(!path.exists());

        assert!(validate_guarded_workspace(&std::env::temp_dir().join("unrelated")).is_err());
    }

    #[test]
    fn guard_cleans_on_disconnect_and_preserves_on_disarm() {
        let disconnected =
            std::env::temp_dir().join(format!("{TEMPORARY_PREFIX}{}", Uuid::new_v4()));
        std::fs::create_dir(&disconnected).unwrap();
        guard_temporary_workspace_from(&disconnected, &mut Cursor::new([])).unwrap();
        assert!(!disconnected.exists());

        let disarmed = std::env::temp_dir().join(format!("{TEMPORARY_PREFIX}{}", Uuid::new_v4()));
        std::fs::create_dir(&disarmed).unwrap();
        guard_temporary_workspace_from(&disarmed, &mut Cursor::new(b"D")).unwrap();
        assert!(disarmed.is_dir());
        remove_workspace_with_retries(&disarmed).unwrap();
    }
}
