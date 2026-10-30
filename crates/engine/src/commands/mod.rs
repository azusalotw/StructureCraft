//! The command registry, sorted by id, and what the commands share.

mod draw;
mod edit;
mod generate;
mod library;
mod model;
mod query;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use structurecraft_geom::DVec3;
use structurecraft_model::{Id, MAX_COORDINATE, Model, SectionId};

use crate::ids::{ElementRef, format_id, parse_id};
use crate::{CommandError, CommandSpec};

/// The name of a model nobody has named.
pub(crate) const UNTITLED: &str = "Untitled";

/// The most elements one command may create (generated models included).
pub(crate) const MAX_BATCH: usize = 5_000_000;

static REGISTRY: &[CommandSpec] = &[
    CommandSpec { id: "area.draw", summary: draw::AREA_SUMMARY, input_schema: draw::area_schema, run: draw::area },
    CommandSpec { id: "edit.redo", summary: "Redo the last undone edit.", input_schema: empty_schema, run: edit::redo },
    CommandSpec { id: "edit.undo", summary: "Undo the last edit.", input_schema: empty_schema, run: edit::undo },
    CommandSpec { id: "element.delete", summary: edit::DELETE_SUMMARY, input_schema: edit::delete_schema, run: edit::delete },
    CommandSpec { id: "element.get", summary: query::GET_SUMMARY, input_schema: query::get_schema, run: query::get },
    CommandSpec { id: "element.list", summary: query::LIST_SUMMARY, input_schema: query::list_schema, run: query::list },
    CommandSpec { id: "frame.draw", summary: draw::FRAME_SUMMARY, input_schema: draw::frame_schema, run: draw::frame },
    CommandSpec { id: "generate.building", summary: generate::SUMMARY, input_schema: generate::schema, run: generate::building },
    CommandSpec { id: "grid.set", summary: library::GRID_SUMMARY, input_schema: library::grid_schema, run: library::grid },
    CommandSpec { id: "material.add", summary: library::MATERIAL_SUMMARY, input_schema: library::material_schema, run: library::material },
    CommandSpec { id: "model.info", summary: model::INFO_SUMMARY, input_schema: empty_schema, run: model::info },
    CommandSpec { id: "model.new", summary: model::NEW_SUMMARY, input_schema: model::new_schema, run: model::new },
    CommandSpec { id: "model.open", summary: model::OPEN_SUMMARY, input_schema: model::path_schema, run: model::open },
    CommandSpec { id: "model.save", summary: model::SAVE_SUMMARY, input_schema: model::path_schema, run: model::save },
    CommandSpec { id: "node.add", summary: draw::NODE_SUMMARY, input_schema: draw::node_schema, run: draw::node },
    CommandSpec { id: "node.move", summary: edit::MOVE_SUMMARY, input_schema: edit::move_schema, run: edit::move_nodes },
    CommandSpec { id: "node.restrain", summary: edit::RESTRAIN_SUMMARY, input_schema: edit::restrain_schema, run: edit::restrain },
    CommandSpec { id: "section.add", summary: library::SECTION_SUMMARY, input_schema: library::section_schema, run: library::section },
];

/// Every command, sorted by id.
pub fn registry() -> &'static [CommandSpec] {
    REGISTRY
}

fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn point_schema() -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": "[x, y, z] in metres"})
}

fn ids_schema(kind: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": format!("{kind} ids, e.g. \"{}12:1\"", &kind[..1].to_uppercase())})
}

fn parse<T: DeserializeOwned>(input: Value) -> Result<T, CommandError> {
    serde_json::from_value(input).map_err(|e| CommandError::InvalidInput(e.to_string()))
}

/// A point in metres, finite and inside the model's extent.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "[f64; 3]")]
pub(crate) struct Point(pub DVec3);

impl TryFrom<[f64; 3]> for Point {
    type Error = String;
    fn try_from(v: [f64; 3]) -> Result<Self, String> {
        let p = DVec3::from_array(v);
        if p.is_finite() && p.abs().max_element() <= MAX_COORDINATE {
            Ok(Self(p))
        } else {
            Err(format!("point {v:?} is outside ±{MAX_COORDINATE} m"))
        }
    }
}

/// An element id in a command's input.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct Ref(String);

impl From<&str> for Ref {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl Ref {
    pub(crate) fn id<T: ElementRef>(&self) -> Result<Id<T>, CommandError> {
        parse_id(&self.0).ok_or_else(|| CommandError::InvalidInput(format!("`{}` is not a {} id", self.0, T::PREFIX)))
    }
}

pub(crate) fn ids<T: ElementRef>(refs: &[Ref]) -> Result<Vec<Id<T>>, CommandError> {
    refs.iter().map(Ref::id).collect()
}

pub(crate) fn id_list<T: ElementRef>(ids: &[Id<T>]) -> Value {
    ids.iter().map(|id| Value::String(format_id(*id))).collect()
}

pub(crate) fn check_batch(n: usize) -> Result<(), CommandError> {
    if n > MAX_BATCH { Err(CommandError::InvalidInput(format!("at most {MAX_BATCH} elements at once"))) } else { Ok(()) }
}

/// The section new frames (`shell == false`) or areas get when the input names none: the first
/// of its kind, or for columns the second frame section when there is one.
pub(crate) fn default_section(m: &Model, shell: bool, column: bool) -> Result<SectionId, CommandError> {
    let mut of_kind = m.sections().iter().filter(|(_, s)| s.shape.is_shell() == shell).map(|(id, _)| id);
    let first = of_kind.next();
    let pick = if column { of_kind.next().or(first) } else { first };
    pick.ok_or_else(|| CommandError::InvalidInput(format!("the model has no {} section", if shell { "slab or wall" } else { "frame" })))
}
