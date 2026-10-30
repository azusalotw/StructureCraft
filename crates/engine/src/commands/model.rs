//! Whole-model commands: new, open, save, info.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::Model;

use super::{UNTITLED, parse};
use crate::{CommandError, Session};

pub const INFO_SUMMARY: &str = "Describe the open model: its name, units, element counts, extent and undo state.";
pub const NEW_SUMMARY: &str = "Replace the open model with a new one holding default concrete sections; forgets the undo history.";
pub const OPEN_SUMMARY: &str = "Open a .stc model file, replacing the open model.";
pub const SAVE_SUMMARY: &str = "Save the open model to a .stc file.";

pub fn new_schema() -> Value {
    json!({"type": "object", "properties": {"name": {"type": "string", "description": "Model name"}}, "additionalProperties": false})
}

pub fn path_schema() -> Value {
    json!({"type": "object", "properties": {"path": {"type": "string", "description": "File path"}}, "required": ["path"], "additionalProperties": false})
}

pub fn info(session: &mut Session, _input: Value) -> Result<Value, CommandError> {
    let m = &session.model;
    let b = m.bounds();
    let bounds = (!b.is_empty()).then(|| json!({"min": b.min.to_array(), "max": b.max.to_array()}));
    let (undo, redo) = session.history();
    Ok(json!({
        "name": m.name(),
        "display_units": m.display_units(),
        "counts": {
            "nodes": m.nodes().len(),
            "frames": m.frames().len(),
            "areas": m.areas().len(),
            "materials": m.materials().len(),
            "sections": m.sections().len(),
        },
        "bounds": bounds,
        "dirty": session.dirty,
        "undo": undo,
        "redo": redo,
    }))
}

pub fn new(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        name: Option<String>,
    }
    let Input { name } = parse(input)?;
    session.replace_model(Model::new(name.unwrap_or_else(|| UNTITLED.to_owned())));
    info(session, Value::Null)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathInput {
    path: String,
}

pub fn open(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    let PathInput { path } = parse(input)?;
    let bytes = std::fs::read(&path).map_err(|e| CommandError::Io(format!("cannot read {path}: {e}")))?;
    session.replace_model(structurecraft_io::from_bytes(&bytes)?);
    info(session, Value::Null)
}

pub fn save(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    let PathInput { path } = parse(input)?;
    let bytes = structurecraft_io::to_bytes(&session.model)?;
    std::fs::write(&path, &bytes).map_err(|e| CommandError::Io(format!("cannot write {path}: {e}")))?;
    session.dirty = false;
    Ok(json!({"path": path, "bytes": bytes.len()}))
}
