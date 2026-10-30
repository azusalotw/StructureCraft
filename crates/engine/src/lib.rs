//! The session and the command registry (L4).
//!
//! Everything a user or agent can do is a command with a JSON input and output, so the desktop
//! app, the browser app, the CLI and the MCP server all run exactly the same code. Commands never
//! open dialogs; front ends ask the user first and pass the answer in.

mod commands;

use serde_json::Value;
use structurecraft_model::Model;

pub use commands::registry;

/// One open model and the state around it.
#[derive(Debug)]
pub struct Session {
    pub model: Model,
    /// Set by any command that changes the model; cleared by save.
    pub dirty: bool,
}

/// A command's description, as the UI, `--help` and MCP `tools/list` show it.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Dotted id, `area.verb`, e.g. `model.new`.
    pub id: &'static str,
    /// One sentence for menus, help and agents.
    pub summary: &'static str,
    /// JSON Schema of the input (an object).
    pub input_schema: fn() -> Value,
    pub run: fn(&mut Session, Value) -> Result<Value, CommandError>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("unknown command `{0}`")]
    Unknown(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] structurecraft_io::Error),
}

impl Default for Session {
    fn default() -> Self {
        Self { model: Model::new(commands::UNTITLED), dirty: false }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run a command by id. Hostile input yields an error, never a panic.
    pub fn execute(&mut self, id: &str, input: Value) -> Result<Value, CommandError> {
        let spec = registry().iter().find(|c| c.id == id).ok_or_else(|| CommandError::Unknown(id.to_owned()))?;
        let input = if input.is_null() { Value::Object(Default::default()) } else { input };
        if !input.is_object() {
            return Err(CommandError::InvalidInput("expected a JSON object".to_owned()));
        }
        (spec.run)(self, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_ids_are_unique_and_sorted() {
        let ids: Vec<_> = registry().iter().map(|c| c.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn hostile_params_never_panic() {
        let inputs = [json!(null), json!(1), json!("x"), json!([]), json!({}), json!({"name": 5}), json!({"name": "\u{0}".repeat(1000)})];
        for spec in registry() {
            assert!((spec.input_schema)().is_object(), "{} has no schema", spec.id);
            for input in &inputs {
                let _ = Session::new().execute(spec.id, input.clone());
            }
        }
        assert!(matches!(Session::new().execute("no.such", json!({})), Err(CommandError::Unknown(_))));
    }

    #[test]
    fn new_model_sets_its_name() {
        let mut s = Session::new();
        s.execute("model.new", json!({"name": "Office"})).unwrap();
        assert_eq!(s.model.name, "Office");
        assert_eq!(s.execute("model.info", json!({})).unwrap()["name"], "Office");
    }
}
