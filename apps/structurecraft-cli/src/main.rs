//! The headless CLI.

mod bench;

use std::io::{BufRead, Write};
use std::process::ExitCode;

use structurecraft_engine::{Session, registry};

const USAGE: &str = "\
usage: structurecraft-cli <command>

commands:
  commands              list every command with its summary
  run <ID> [JSON]       run one command on an empty model and print its output
  mcp                   serve the Model Context Protocol on stdin/stdout
  bench [BAYS] [STORIES]
                        time the core operations on a generated building of BAYS x BAYS bays
                        (default 70 x 70 bays, 100 stories: about 510,000 nodes)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("commands") => {
            for c in registry() {
                println!("{:<24} {}", c.id, c.summary);
            }
            Ok(())
        }
        Some("run") => run(args.get(1).map(String::as_str), args.get(2).map(String::as_str)),
        Some("mcp") => mcp(),
        Some("bench") => bench::run(args.get(1).map(String::as_str), args.get(2).map(String::as_str)),
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

fn run(id: Option<&str>, input: Option<&str>) -> Result<(), String> {
    let id = id.ok_or("run needs a command id")?;
    let input = input.map(serde_json::from_str).transpose().map_err(|e| format!("input is not JSON: {e}"))?.unwrap_or_default();
    let out = Session::new().execute(id, input).map_err(|e| e.to_string())?;
    println!("{out:#}");
    Ok(())
}

fn mcp() -> Result<(), String> {
    let mut session = Session::new();
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = structurecraft_mcp::handle_line(&mut session, &line) {
            let mut out = stdout.lock();
            writeln!(out, "{response}").and_then(|()| out.flush()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
