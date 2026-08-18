use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::PathBuf;

use a3s_flow::DifyAppDsl;

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| {
            IoError::new(
                ErrorKind::InvalidInput,
                "usage: cargo run --example dify_import -- <app.dify.yml>",
            )
        })?;
    let source = std::fs::read_to_string(&path)?;
    let document = DifyAppDsl::from_yaml(&source)?;
    let plan = document.graph().execution_plan()?;

    println!("app={}", document.app().name());
    println!("dsl_version={}", document.version());
    println!("compatibility={:?}", document.compatibility()?);
    println!("nodes={}", document.graph().nodes().len());
    println!("edges={}", document.graph().edges().len());
    println!("scopes={}", plan.scopes().len());
    println!("execution_digest={}", document.execution_digest()?);
    Ok(())
}
