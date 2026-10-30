//! Workspace tooling: `cargo xtask <command>`.

mod layers;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers   enforce the crate dependency layering (docs/architecture.md)
  wasm     cargo check every library crate and the web app for wasm32-unknown-unknown
  ci       fmt --check, layers, clippy -D warnings, the tests, then the wasm check
           (stops at the first failing step)
";

/// Library crates that must build for the browser.
const WASM_PACKAGES: &[&str] = &[
    "structurecraft-engine",
    "structurecraft-geom",
    "structurecraft-gpu",
    "structurecraft-io",
    "structurecraft-mcp",
    "structurecraft-model",
    "structurecraft-render",
    "structurecraft-ui-egui",
    "structurecraft-web",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("layers") => layers::run(&root()),
        Some("wasm") => cmd_wasm(),
        Some("ci") => cmd_ci(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn cargo(args: &[&str]) -> Result<(), String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    eprintln!("$ cargo {}", args.join(" "));
    let status = Command::new(cargo).args(args).current_dir(root()).status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("`cargo {}` failed", args.join(" "))) }
}

fn cmd_wasm() -> Result<(), String> {
    let mut args = vec!["check", "--target", "wasm32-unknown-unknown"];
    for p in WASM_PACKAGES {
        args.extend(["-p", p]);
    }
    cargo(&args)
}

fn cmd_ci() -> Result<(), String> {
    cargo(&["fmt", "--all", "--check"])?;
    layers::run(&root())?;
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    cargo(&["test", "--workspace"])?;
    cmd_wasm()
}
