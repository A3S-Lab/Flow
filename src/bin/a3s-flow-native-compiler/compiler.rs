use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use a3s_flow::{NativeTsDependencyManifest, NATIVE_RUNTIME_PROTOCOL};
use serde::Deserialize;

mod bun;
mod temporary;

use bun::ResolvedBun;
pub(super) use bun::{run_bun_supervised, BUN_SUPERVISOR_COMMAND};
use temporary::TemporaryWorkspace;
pub(super) use temporary::{guard_temporary_workspace, TEMPORARY_GUARD_COMMAND};

const COMPILER_INPUT_FILES: &[&str] = &[
    "bun.lock",
    "bun.lockb",
    "bunfig.toml",
    "package.json",
    "tsconfig.json",
];

#[derive(Debug, Deserialize)]
struct BunMetafile {
    inputs: BTreeMap<String, BunInput>,
}

#[derive(Debug, Deserialize)]
struct BunInput {
    bytes: u64,
}

pub(super) fn dependency_manifest(entrypoint: &Path) -> Result<NativeTsDependencyManifest, String> {
    let root = canonical_working_directory()?;
    let bun = ResolvedBun::resolve()?;
    let entrypoint = canonical_source(entrypoint, &root, "entrypoint")?;
    let entrypoint_logical = logical_path(&root, &entrypoint)?;
    let temporary = TemporaryWorkspace::create()?;
    let bundle = temporary.path().join("dependency-scan.js");
    let metafile = temporary.path().join("metafile.json");

    bun.run(
        &root,
        [
            OsString::from("build"),
            portable_argument(&entrypoint_logical),
            OsString::from("--target=bun"),
            OsString::from("--packages=bundle"),
            OsString::from("--reject-unresolved"),
            OsString::from("--sourcemap=none"),
            option("outfile", &bundle),
            option("metafile", &metafile),
        ],
    )?;

    let bytes = fs::read(&metafile).map_err(|error| {
        format!(
            "could not read Bun metafile {}: {error}",
            metafile.display()
        )
    })?;
    let metadata: BunMetafile = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Bun metafile {} is invalid JSON: {error}",
            metafile.display()
        )
    })?;
    if metadata.inputs.is_empty() {
        return Err("Bun dependency scan returned no source inputs".to_string());
    }

    let mut files = BTreeSet::new();
    for (input, details) in metadata.inputs {
        let source = canonical_source(Path::new(&input), &root, "dependency")?;
        let actual_bytes = fs::metadata(&source)
            .map_err(|error| format!("could not inspect dependency {}: {error}", source.display()))?
            .len();
        if details.bytes != actual_bytes {
            return Err(format!(
                "Bun dependency {} changed during scanning: metafile bytes={}, current bytes={actual_bytes}",
                source.display(),
                details.bytes
            ));
        }
        insert_logical_path(&mut files, &root, &source)?;
        include_compiler_inputs(&mut files, &root, &source)?;
    }

    if !files.contains(&entrypoint_logical) {
        return Err("Bun dependency scan did not include the entrypoint".to_string());
    }

    Ok(NativeTsDependencyManifest::new(
        bun.compiler_identity(),
        files.into_iter().collect(),
    ))
}

pub(super) fn compile(entrypoint: &Path, output: &Path) -> Result<(), String> {
    let root = canonical_working_directory()?;
    let bun = ResolvedBun::resolve()?;
    let entrypoint = canonical_source(entrypoint, &root, "entrypoint")?;
    let output = absolute_output(output)?;
    if output.exists() {
        return Err(format!(
            "refusing to overwrite existing compiler output {}",
            output.display()
        ));
    }
    let parent = output.parent().ok_or_else(|| {
        format!(
            "compiler output {} does not have a parent directory",
            output.display()
        )
    })?;
    if !parent.is_dir() {
        return Err(format!(
            "compiler output directory {} does not exist",
            parent.display()
        ));
    }

    let temporary = TemporaryWorkspace::create()?;
    let bootstrap = temporary.path().join("runtime.ts");
    fs::write(&bootstrap, runtime_bootstrap(&entrypoint)?).map_err(|error| {
        format!(
            "could not write runtime bootstrap {}: {error}",
            bootstrap.display()
        )
    })?;

    if let Err(error) = bun.run(
        &root,
        [
            OsString::from("build"),
            bootstrap.as_os_str().to_os_string(),
            OsString::from("--compile"),
            OsString::from("--target=bun"),
            OsString::from("--packages=bundle"),
            OsString::from("--reject-unresolved"),
            OsString::from("--sourcemap=none"),
            OsString::from("--no-compile-autoload-dotenv"),
            OsString::from("--no-compile-autoload-bunfig"),
            OsString::from("--no-compile-autoload-tsconfig"),
            OsString::from("--no-compile-autoload-package-json"),
            option("outfile", &output),
        ],
    ) {
        remove_failed_output(&output);
        return Err(error);
    }
    let metadata = match fs::metadata(&output) {
        Ok(metadata) => metadata,
        Err(error) => {
            remove_failed_output(&output);
            return Err(format!(
                "Bun did not produce compiler output {}: {error}",
                output.display()
            ));
        }
    };
    if !metadata.is_file() || metadata.len() == 0 {
        remove_failed_output(&output);
        return Err(format!(
            "Bun compiler output {} is not a non-empty regular file",
            output.display()
        ));
    }
    Ok(())
}

fn remove_failed_output(path: &Path) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!(
                "a3s-flow-native-compiler: could not remove failed output {}: {error}",
                path.display()
            );
        }
    }
}

fn canonical_working_directory() -> Result<PathBuf, String> {
    let current = std::env::current_dir()
        .map_err(|error| format!("could not read the compiler working directory: {error}"))?;
    fs::canonicalize(&current).map_err(|error| {
        format!(
            "could not resolve compiler working directory {}: {error}",
            current.display()
        )
    })
}

fn canonical_source(path: &Path, root: &Path, label: &str) -> Result<PathBuf, String> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let canonical = fs::canonicalize(&resolved)
        .map_err(|error| format!("could not resolve {label} {}: {error}", resolved.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "{label} {} is outside compiler working directory {}",
            canonical.display(),
            root.display()
        ));
    }
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("could not inspect {label} {}: {error}", canonical.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "{label} {} is not a regular file",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn absolute_output(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()
        .map_err(|error| format!("could not read current directory: {error}"))?
        .join(path))
}

fn option(name: &str, value: &Path) -> OsString {
    let mut option = OsString::from("--");
    option.push(name);
    option.push("=");
    option.push(value.as_os_str());
    option
}

fn insert_logical_path(
    files: &mut BTreeSet<String>,
    root: &Path,
    source: &Path,
) -> Result<(), String> {
    files.insert(logical_path(root, source)?);
    Ok(())
}

fn include_compiler_inputs(
    files: &mut BTreeSet<String>,
    root: &Path,
    source: &Path,
) -> Result<(), String> {
    let mut directory = source.parent();
    while let Some(candidate) = directory {
        if !candidate.starts_with(root) {
            break;
        }
        for name in COMPILER_INPUT_FILES {
            let config = candidate.join(name);
            if config.is_file() {
                insert_logical_path(
                    files,
                    root,
                    &fs::canonicalize(&config).map_err(|error| {
                        format!(
                            "could not resolve compiler input {}: {error}",
                            config.display()
                        )
                    })?,
                )?;
            }
        }
        if candidate == root {
            break;
        }
        directory = candidate.parent();
    }
    Ok(())
}

fn logical_path(root: &Path, source: &Path) -> Result<String, String> {
    let relative = source.strip_prefix(root).map_err(|_| {
        format!(
            "source {} is outside compiler working directory {}",
            source.display(),
            root.display()
        )
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(format!(
                "source {} does not have a normalized relative path",
                source.display()
            ));
        };
        let text = component.to_str().ok_or_else(|| {
            format!(
                "source {} cannot be represented in the UTF-8 dependency protocol",
                source.display()
            )
        })?;
        if text.contains(['/', '\\', ':', '\0']) {
            return Err(format!(
                "source {} uses a non-portable path component",
                source.display()
            ));
        }
        parts.push(text);
    }
    if parts.is_empty() {
        return Err(format!(
            "source {} has an empty logical path",
            source.display()
        ));
    }
    Ok(parts.join("/"))
}

fn portable_argument(logical_path: &str) -> OsString {
    if std::path::MAIN_SEPARATOR == '/' {
        return OsString::from(logical_path);
    }
    OsString::from(logical_path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn runtime_bootstrap(entrypoint: &Path) -> Result<String, String> {
    let specifier = module_specifier(entrypoint)?;
    let specifier = serde_json::to_string(&specifier)
        .map_err(|error| format!("could not encode entrypoint URL: {error}"))?;
    let protocol = serde_json::to_string(NATIVE_RUNTIME_PROTOCOL)
        .map_err(|error| format!("could not encode runtime protocol: {error}"))?;

    Ok(RUNTIME_BOOTSTRAP
        .replace("__A3S_ENTRYPOINT__", &specifier)
        .replace("__A3S_PROTOCOL__", &protocol))
}

fn module_specifier(path: &Path) -> Result<String, String> {
    let raw = path.to_str().ok_or_else(|| {
        format!(
            "entrypoint {} cannot be represented as a UTF-8 module specifier",
            path.display()
        )
    })?;
    #[cfg(windows)]
    let raw = raw
        .strip_prefix(r"\\?\UNC\")
        .map(|path| format!(r"\\{path}"))
        .unwrap_or_else(|| raw.strip_prefix(r"\\?\").unwrap_or(raw).to_string());
    #[cfg(not(windows))]
    let raw = raw.to_string();
    let normalized = raw.replace('\\', "/");
    Ok(normalized)
}

const RUNTIME_BOOTSTRAP: &str = r#"import * as flowModule from __A3S_ENTRYPOINT__;

const protocol = __A3S_PROTOCOL__;
let request;

try {
  if (!process.argv.includes("--a3s-flow-runtime")) {
    throw new Error("missing --a3s-flow-runtime");
  }

  request = JSON.parse(await Bun.stdin.text());
  if (request?.protocol !== protocol) {
    throw new Error("unsupported native runtime protocol");
  }
  if (request?.kind !== "workflow" && request?.kind !== "step" && request?.kind !== "activity") {
    throw new Error("native runtime request kind must be workflow, step, or activity");
  }

  let handler;
  if (request.kind === "workflow") {
    if (typeof request.exportName !== "string") {
      throw new Error("workflow exportName must be a string");
    }
    handler = Reflect.get(flowModule, request.exportName);
  } else {
    const handlerName = request?.payload?.[request.kind === "activity" ? "activity_name" : "step_name"];
    if (typeof handlerName !== "string") {
      throw new Error(`${request.kind} payload handler name must be a string`);
    }
    const steps = Reflect.get(flowModule, request.kind === "activity" ? "activities" : "steps");
    handler =
      steps && Object.prototype.hasOwnProperty.call(steps, handlerName)
        ? Reflect.get(steps, handlerName)
        : Reflect.get(flowModule, handlerName);
  }

  if (typeof handler !== "function") {
    throw new Error("requested native TypeScript handler is not exported");
  }
  const output = (await handler(request.payload)) ?? null;
  process.stdout.write(
    JSON.stringify({ protocol, kind: request.kind, ok: true, output }) + "\n",
  );
} catch (error) {
  const kind = request?.kind === "step" || request?.kind === "activity" ? request.kind : "workflow";
  const message =
    error instanceof Error ? error.message : "native TypeScript handler failed";
  process.stdout.write(
    JSON.stringify({
      protocol,
      kind,
      ok: false,
      error: message.slice(0, 4096),
    }) + "\n",
  );
}
"#;

#[cfg(test)]
mod tests {
    use super::{module_specifier, runtime_bootstrap};
    use a3s_flow::NATIVE_RUNTIME_PROTOCOL;
    use std::path::Path;

    #[test]
    fn module_specifiers_are_absolute_and_use_portable_separators() {
        let specifier = module_specifier(Path::new(if cfg!(windows) {
            r"C:\workspace\flow source#1.ts"
        } else {
            "/workspace/flow source#1.ts"
        }))
        .unwrap();

        assert!(!specifier.contains('\\'));
        assert!(specifier.ends_with("flow source#1.ts"));
    }

    #[test]
    fn bootstrap_binds_the_entrypoint_and_runtime_protocol() {
        let path = if cfg!(windows) {
            Path::new(r"C:\workspace\main.ts")
        } else {
            Path::new("/workspace/main.ts")
        };
        let bootstrap = runtime_bootstrap(path).unwrap();

        assert!(bootstrap.contains(NATIVE_RUNTIME_PROTOCOL));
        assert!(!bootstrap.contains("__A3S_ENTRYPOINT__"));
        assert!(!bootstrap.contains("__A3S_PROTOCOL__"));
        assert!(bootstrap.contains("step_name"));
        assert!(bootstrap.contains("activity_name"));
        assert!(bootstrap.contains("--a3s-flow-runtime"));
    }
}
