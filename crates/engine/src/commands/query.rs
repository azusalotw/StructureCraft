//! Reading the model: one element, or a page of a table.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::{Area, Frame, Material, Model, Node, Section, SectionShape};

use super::{Ref, parse};
use crate::ids::format_id;
use crate::{CommandError, Session};

pub const GET_SUMMARY: &str = "Describe elements by id (nodes, frames, areas, materials, sections).";
pub const LIST_SUMMARY: &str = "List one kind of element, a page at a time.";

const PAGE_LIMIT: usize = 10_000;

pub fn get_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"ids": {"type": "array", "items": {"type": "string"}, "description": "Ids of any kind, e.g. N3:1, F7:1, S0:1"}},
        "required": ["ids"],
        "additionalProperties": false,
    })
}

pub fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {"enum": ["nodes", "frames", "areas", "materials", "sections"]},
            "offset": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": PAGE_LIMIT},
        },
        "required": ["kind"],
        "additionalProperties": false,
    })
}

fn node(m: &Model, n: &Node) -> Value {
    let _ = m;
    json!({"position": n.position.to_array(), "restraint": n.restraint.0})
}

fn frame(f: &Frame) -> Value {
    json!({"i": format_id(f.i), "j": format_id(f.j), "section": format_id(f.section), "role": f.role, "angle": f.angle})
}

fn area(a: &Area) -> Value {
    json!({"corners": a.corners.iter().map(|c| format_id(*c)).collect::<Vec<_>>(), "section": format_id(a.section), "role": a.role})
}

fn material(m: &Material) -> Value {
    json!(m)
}

fn section(s: &Section) -> Value {
    let shape: &SectionShape = &s.shape;
    json!({"name": s.name, "material": format_id(s.material), "shape": shape})
}

fn describe(m: &Model, id: &str) -> Value {
    let found = match id.chars().next() {
        Some('N') => Ref::from(id).id().ok().and_then(|i| m.nodes().get(i)).map(|n| node(m, n)),
        Some('F') => Ref::from(id).id().ok().and_then(|i| m.frames().get(i)).map(frame),
        Some('A') => Ref::from(id).id().ok().and_then(|i| m.areas().get(i)).map(area),
        Some('M') => Ref::from(id).id().ok().and_then(|i| m.materials().get(i)).map(material),
        Some('S') => Ref::from(id).id().ok().and_then(|i| m.sections().get(i)).map(section),
        _ => None,
    };
    match found {
        Some(Value::Object(mut o)) => {
            o.insert("id".to_owned(), json!(id));
            Value::Object(o)
        }
        _ => json!({"id": id, "error": "no such element"}),
    }
}

pub fn get(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        ids: Vec<String>,
    }
    let Input { ids } = parse(input)?;
    if ids.len() > PAGE_LIMIT {
        return Err(CommandError::InvalidInput(format!("at most {PAGE_LIMIT} ids at once")));
    }
    Ok(json!({"elements": ids.iter().map(|id| describe(&session.model, id)).collect::<Vec<_>>()}))
}

pub fn list(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        kind: Kind,
        #[serde(default)]
        offset: usize,
        #[serde(default = "page")]
        limit: usize,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Kind {
        Nodes,
        Frames,
        Areas,
        Materials,
        Sections,
    }
    fn page() -> usize {
        1000
    }
    let Input { kind, offset, limit } = parse(input)?;
    let limit = limit.clamp(1, PAGE_LIMIT);
    let m = &session.model;
    fn with_id(id: String, v: Value) -> Value {
        match v {
            Value::Object(mut o) => {
                o.insert("id".to_owned(), Value::String(id));
                Value::Object(o)
            }
            v => v,
        }
    }
    let (total, items): (usize, Vec<Value>) = match kind {
        Kind::Nodes => (m.nodes().len(), m.nodes().iter().skip(offset).take(limit).map(|(id, n)| with_id(format_id(id), node(m, n))).collect()),
        Kind::Frames => (m.frames().len(), m.frames().iter().skip(offset).take(limit).map(|(id, f)| with_id(format_id(id), frame(f))).collect()),
        Kind::Areas => (m.areas().len(), m.areas().iter().skip(offset).take(limit).map(|(id, a)| with_id(format_id(id), area(a))).collect()),
        Kind::Materials => {
            (m.materials().len(), m.materials().iter().skip(offset).take(limit).map(|(id, x)| with_id(format_id(id), material(x))).collect())
        }
        Kind::Sections => {
            (m.sections().len(), m.sections().iter().skip(offset).take(limit).map(|(id, s)| with_id(format_id(id), section(s))).collect())
        }
    };
    Ok(json!({"total": total, "offset": offset, "items": items}))
}
