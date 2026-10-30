//! Reference data: the grid and stories, materials, sections.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::{Change, Grid, GridLine, Material, Section, SectionShape, Story};

use super::{Ref, id_list, parse};
use crate::{CommandError, Session};

pub const GRID_SUMMARY: &str = "Replace the plan grid lines and stories.";
pub const MATERIAL_SUMMARY: &str = "Add an isotropic linear-elastic material (SI units: Pa, kg/m³).";
pub const SECTION_SUMMARY: &str = "Add a section: a rectangle, circle or I shape for frames, or a shell thickness for slabs and walls (metres).";

pub fn grid_schema() -> Value {
    let lines = json!({"type": "array", "items": {"type": "object", "properties": {"label": {"type": "string"}, "coordinate": {"type": "number"}}, "required": ["label", "coordinate"]}});
    json!({
        "type": "object",
        "properties": {
            "x": lines,
            "y": lines,
            "stories": {"type": "array", "items": {"type": "object", "properties": {"name": {"type": "string"}, "elevation": {"type": "number"}}, "required": ["name", "elevation"]}, "description": "Sorted by elevation, lowest first"},
        },
        "additionalProperties": false,
    })
}

pub fn grid(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        x: Vec<GridLine>,
        #[serde(default)]
        y: Vec<GridLine>,
        #[serde(default)]
        stories: Vec<Story>,
    }
    let Input { x, y, stories } = parse(input)?;
    session.edit("Set grid", |tx| {
        tx.apply(Change::Grid(Grid { x, y, stories }))?;
        Ok(json!({}))
    })
}

pub fn material_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "elastic_modulus": {"type": "number", "description": "Pa"},
            "poisson_ratio": {"type": "number"},
            "density": {"type": "number", "description": "kg/m³"},
        },
        "required": ["name", "elastic_modulus", "poisson_ratio", "density"],
        "additionalProperties": false,
    })
}

pub fn material(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    let material: Material = parse(input)?;
    session.edit("Add material", |tx| Ok(json!({"material": id_list(&tx.add_materials(vec![material])?)[0]})))
}

pub fn section_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "material": {"type": "string", "description": "Material id; default: the first material"},
            "shape": {
                "oneOf": [
                    {"type": "object", "properties": {"shape": {"const": "rectangle"}, "b": {"type": "number"}, "h": {"type": "number"}}, "required": ["shape", "b", "h"]},
                    {"type": "object", "properties": {"shape": {"const": "circle"}, "d": {"type": "number"}}, "required": ["shape", "d"]},
                    {"type": "object", "properties": {"shape": {"const": "i_section"}, "h": {"type": "number"}, "b": {"type": "number"}, "tf": {"type": "number"}, "tw": {"type": "number"}}, "required": ["shape", "h", "b", "tf", "tw"]},
                    {"type": "object", "properties": {"shape": {"const": "shell"}, "t": {"type": "number"}}, "required": ["shape", "t"]},
                ],
            },
        },
        "required": ["name", "shape"],
        "additionalProperties": false,
    })
}

pub fn section(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        name: String,
        #[serde(default)]
        material: Option<Ref>,
        shape: SectionShape,
    }
    let Input { name, material, shape } = parse(input)?;
    let material = match material {
        Some(m) => m.id()?,
        None => session.model.materials().ids().next().ok_or_else(|| CommandError::InvalidInput("the model has no material".to_owned()))?,
    };
    session.edit("Add section", |tx| Ok(json!({"section": id_list(&tx.add_sections(vec![Section { name, material, shape }])?)[0]})))
}
