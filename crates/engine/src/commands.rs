//! The command registry, sorted by id.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::Model;

use crate::{CommandError, CommandSpec, Session};

/// The name of a model nobody has named.
pub(crate) const UNTITLED: &str = "Untitled";

static REGISTRY: &[CommandSpec] = &[
    CommandSpec {
        id: "model.info",
        summary: "Describe the open model: its name, units and extent.",
        input_schema: || json!({"type": "object", "properties": {}}),
        run: model_info,
    },
    CommandSpec {
        id: "model.new",
        summary: "Replace the open model with an empty one.",
        input_schema: || json!({"type": "object", "properties": {"name": {"type": "string", "description": "Model name"}}}),
        run: model_new,
    },
];

/// Every command, sorted by id.
pub fn registry() -> &'static [CommandSpec] {
    REGISTRY
}

fn parse<T: for<'de> Deserialize<'de>>(input: Value) -> Result<T, CommandError> {
    serde_json::from_value(input).map_err(|e| CommandError::InvalidInput(e.to_string()))
}

fn model_info(session: &mut Session, _input: Value) -> Result<Value, CommandError> {
    let m = &session.model;
    let b = m.bounds();
    let bounds = (!b.is_empty()).then(|| json!({"min": b.min.to_array(), "max": b.max.to_array()}));
    Ok(json!({"name": m.name, "display_units": m.display_units, "bounds": bounds, "dirty": session.dirty}))
}

fn model_new(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        name: Option<String>,
    }
    let Input { name } = parse(input)?;
    session.model = Model::new(name.unwrap_or_else(|| UNTITLED.to_owned()));
    session.dirty = false;
    model_info(session, Value::Null)
}
