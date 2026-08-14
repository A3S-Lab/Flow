mod compiler;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use a3s_flow::NativeTsCompilerCapabilities;

enum Command {
    Capabilities,
    Dependencies {
        entrypoint: PathBuf,
    },
    Compile {
        entrypoint: PathBuf,
        output: PathBuf,
    },
    Help,
    Version,
    RunBunSupervised {
        working_directory: PathBuf,
        binary: OsString,
        arguments: Vec<OsString>,
    },
    GuardTemporaryWorkspace {
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("a3s-flow-native-compiler: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    match parse_command(args)? {
        Command::Capabilities => {
            write_json(&NativeTsCompilerCapabilities::current())?;
        }
        Command::Dependencies { entrypoint } => {
            write_json(&compiler::dependency_manifest(&entrypoint)?)?;
        }
        Command::Compile { entrypoint, output } => {
            compiler::compile(&entrypoint, &output)?;
        }
        Command::Help => print!("{HELP}"),
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
        Command::RunBunSupervised {
            working_directory,
            binary,
            arguments,
        } => compiler::run_bun_supervised(&working_directory, &binary, &arguments)?,
        Command::GuardTemporaryWorkspace { path } => compiler::guard_temporary_workspace(&path)?,
    }
    Ok(())
}

fn write_json(value: &impl serde::Serialize) -> Result<(), String> {
    serde_json::to_writer(std::io::stdout().lock(), value)
        .map_err(|error| format!("could not encode compiler response: {error}"))?;
    println!();
    Ok(())
}

fn parse_command(args: Vec<OsString>) -> Result<Command, String> {
    let Some(command) = args.first().and_then(|value| value.to_str()) else {
        return Ok(Command::Help);
    };
    match command {
        compiler::TEMPORARY_GUARD_COMMAND if args.len() == 2 => {
            Ok(Command::GuardTemporaryWorkspace {
                path: PathBuf::from(&args[1]),
            })
        }
        compiler::BUN_SUPERVISOR_COMMAND if args.len() >= 3 => Ok(Command::RunBunSupervised {
            working_directory: PathBuf::from(&args[1]),
            binary: args[2].clone(),
            arguments: args[3..].to_vec(),
        }),
        "-h" | "--help" | "help" if args.len() == 1 => Ok(Command::Help),
        "-V" | "--version" | "version" if args.len() == 1 => Ok(Command::Version),
        "capabilities" if args.len() == 1 => Ok(Command::Capabilities),
        "dependencies" if args.len() == 2 => Ok(Command::Dependencies {
            entrypoint: PathBuf::from(&args[1]),
        }),
        "compile" if args.len() == 4 && args[2] == "-o" => Ok(Command::Compile {
            entrypoint: PathBuf::from(&args[1]),
            output: PathBuf::from(&args[3]),
        }),
        "compile" => {
            Err("usage: a3s-flow-native-compiler compile <entrypoint.ts> -o <artifact>".to_string())
        }
        "dependencies" => {
            Err("usage: a3s-flow-native-compiler dependencies <entrypoint.ts>".to_string())
        }
        "capabilities" => Err("usage: a3s-flow-native-compiler capabilities".to_string()),
        other => Err(format!("unknown command {other:?}; run with --help")),
    }
}

const HELP: &str = "A3S Flow Native TypeScript compiler

Usage:
  a3s-flow-native-compiler capabilities
  a3s-flow-native-compiler dependencies <entrypoint.ts>
  a3s-flow-native-compiler compile <entrypoint.ts> -o <artifact>

Environment:
  A3S_FLOW_BUN  Bun executable to use (default: bun)
";

#[cfg(test)]
mod tests {
    use super::{parse_command, Command};
    use std::ffi::OsString;
    use std::path::Path;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parser_accepts_the_closed_compiler_surface() {
        assert!(matches!(
            parse_command(args(&["capabilities"])).unwrap(),
            Command::Capabilities
        ));
        match parse_command(args(&["dependencies", "src/main.ts"])).unwrap() {
            Command::Dependencies { entrypoint } => {
                assert_eq!(entrypoint, Path::new("src/main.ts"));
            }
            _ => panic!("unexpected command"),
        }
        match parse_command(args(&["compile", "src/main.ts", "-o", "artifact"])).unwrap() {
            Command::Compile { entrypoint, output } => {
                assert_eq!(entrypoint, Path::new("src/main.ts"));
                assert_eq!(output, Path::new("artifact"));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parser_rejects_ambiguous_or_extra_arguments() {
        for values in [
            vec!["unknown"],
            vec!["capabilities", "extra"],
            vec!["dependencies"],
            vec!["compile", "main.ts", "--output", "artifact"],
            vec!["compile", "main.ts", "-o"],
        ] {
            assert!(parse_command(args(&values)).is_err(), "{values:?}");
        }
    }
}
