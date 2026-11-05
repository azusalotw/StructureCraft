//! Changing what is there: undo, redo, delete, move, restrain.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_model::{Area, Change, Frame, Node, Op, Releases};

use super::draw::{AreaRoleName, RestraintName, RoleName};
use super::{Point, Ref, check_batch, ids, ids_schema, parse};
use crate::{CommandError, Session};

pub const DELETE_SUMMARY: &str =
    "Delete nodes, frames and areas; deleting a node also deletes the frames and areas on it, and any element's loads go with it.";
pub const MOVE_SUMMARY: &str = "Move nodes by an offset; the frames and areas on them follow.";
pub const RESTRAIN_SUMMARY: &str = "Set the support of nodes: free, pinned (translations held) or fixed (all six held).";

pub fn undo(session: &mut Session, _input: Value) -> Result<Value, CommandError> {
    Ok(json!({"undone": session.undo()?}))
}

pub fn redo(session: &mut Session, _input: Value) -> Result<Value, CommandError> {
    Ok(json!({"redone": session.redo()?}))
}

pub fn delete_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"nodes": ids_schema("node"), "frames": ids_schema("frame"), "areas": ids_schema("area")},
        "additionalProperties": false,
    })
}

pub fn delete(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(default)]
        nodes: Vec<Ref>,
        #[serde(default)]
        frames: Vec<Ref>,
        #[serde(default)]
        areas: Vec<Ref>,
    }
    let input: Input = parse(input)?;
    check_batch(input.nodes.len() + input.frames.len() + input.areas.len())?;
    let nodes = ids(&input.nodes)?;
    let (mut frames, mut areas) = session.model.dependents(&nodes);
    frames.extend(ids(&input.frames)?);
    areas.extend(ids(&input.areas)?);
    let (nodes, frames, areas) = (sorted(nodes), sorted(frames), sorted(areas));
    let loads = session.model.loads_on(&nodes, &frames, &areas, None);
    let counts = json!({"nodes": nodes.len(), "frames": frames.len(), "areas": areas.len()});
    session.edit("Delete", |tx| {
        // Loads first: an element with loads on it cannot go.
        for change in [
            (!loads.node_loads.is_empty()).then(|| Change::NodeLoads(Op::Remove(loads.node_loads))),
            (!loads.frame_loads.is_empty()).then(|| Change::FrameLoads(Op::Remove(loads.frame_loads))),
            (!loads.area_loads.is_empty()).then(|| Change::AreaLoads(Op::Remove(loads.area_loads))),
        ]
        .into_iter()
        .flatten()
        {
            tx.apply(change)?;
        }
        // Areas and frames first: a node in use cannot go.
        if !areas.is_empty() {
            tx.apply(Change::Areas(Op::Remove(areas)))?;
        }
        if !frames.is_empty() {
            tx.apply(Change::Frames(Op::Remove(frames)))?;
        }
        if !nodes.is_empty() {
            tx.apply(Change::Nodes(Op::Remove(nodes)))?;
        }
        Ok(json!({"deleted": counts}))
    })
}

fn sorted<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort_unstable();
    v.dedup();
    v
}

pub fn move_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"nodes": ids_schema("node"), "by": super::point_schema()},
        "required": ["nodes", "by"],
        "additionalProperties": false,
    })
}

pub fn move_nodes(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        nodes: Vec<Ref>,
        by: Point,
    }
    let Input { nodes, by } = parse(input)?;
    check_batch(nodes.len())?;
    let ids = ids::<Node>(&nodes)?;
    session.edit("Move nodes", |tx| {
        let rows = ids
            .iter()
            .map(|id| tx.model().nodes().get(*id).map(|n| (*id, Node { position: n.position + by.0, ..*n })))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CommandError::InvalidInput("a node to move does not exist".to_owned()))?;
        let moved = rows.len();
        if moved > 0 {
            tx.apply(Change::Nodes(Op::Set(rows)))?;
        }
        Ok(json!({"moved": moved}))
    })
}

pub fn restrain_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"nodes": ids_schema("node"), "restraint": {"enum": ["free", "pinned", "fixed"]}},
        "required": ["nodes", "restraint"],
        "additionalProperties": false,
    })
}

pub fn restrain(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        nodes: Vec<Ref>,
        restraint: RestraintName,
    }
    let Input { nodes, restraint } = parse(input)?;
    check_batch(nodes.len())?;
    let ids = ids::<Node>(&nodes)?;
    session.edit("Set supports", |tx| {
        let rows = ids
            .iter()
            .map(|id| tx.model().nodes().get(*id).map(|n| (*id, Node { restraint: restraint.into(), ..*n })))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CommandError::InvalidInput("a node to restrain does not exist".to_owned()))?;
        let changed = rows.len();
        if changed > 0 {
            tx.apply(Change::Nodes(Op::Set(rows)))?;
        }
        Ok(json!({"changed": changed}))
    })
}

pub const FRAME_SET_SUMMARY: &str = "Change the section, role or end releases of frames.";
pub const AREA_SET_SUMMARY: &str = "Change the section or role of areas.";

pub fn frame_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "frames": ids_schema("frame"),
            "section": {"type": "string"},
            "role": {"enum": ["beam", "column", "brace"]},
            "release_i": release_schema("end i"),
            "release_j": release_schema("end j"),
        },
        "required": ["frames"],
        "additionalProperties": false,
    })
}

fn release_schema(end: &str) -> Value {
    json!({
        "description": format!("What {end} does not carry: none, pinned (both moments), or a list of p, v2, v3, t, m2, m3 (local axes)"),
        "oneOf": [{"enum": ["none", "pinned"]}, {"type": "array", "items": {"enum": Releases::NAMES}}],
    })
}

/// One end's releases as a command gives them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ReleaseSpec {
    Named(String),
    Bits(Vec<String>),
}

impl ReleaseSpec {
    fn bits(&self) -> Result<u16, CommandError> {
        let bad =
            |s: &str| CommandError::InvalidInput(format!("unknown release `{s}`: use none, pinned or a list of {}", Releases::NAMES.join(", ")));
        match self {
            Self::Named(s) if s == "none" => Ok(0),
            Self::Named(s) if s == "pinned" => Ok(Releases::MOMENTS),
            Self::Named(s) => Err(bad(s)),
            Self::Bits(names) => {
                names.iter().try_fold(0, |bits, n| Releases::NAMES.iter().position(|x| x == n).map(|k| bits | 1 << k).ok_or_else(|| bad(n)))
            }
        }
    }
}

pub fn area_set_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"areas": ids_schema("area"), "section": {"type": "string"}, "role": {"enum": ["slab", "wall"]}},
        "required": ["areas"],
        "additionalProperties": false,
    })
}

pub fn frame_set(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        frames: Vec<Ref>,
        #[serde(default)]
        section: Option<Ref>,
        #[serde(default)]
        role: Option<RoleName>,
        #[serde(default)]
        release_i: Option<ReleaseSpec>,
        #[serde(default)]
        release_j: Option<ReleaseSpec>,
    }
    let Input { frames, section, role, release_i, release_j } = parse(input)?;
    check_batch(frames.len())?;
    let ids = ids::<Frame>(&frames)?;
    let section = section.as_ref().map(Ref::id).transpose()?;
    let release_i = release_i.as_ref().map(ReleaseSpec::bits).transpose()?;
    let release_j = release_j.as_ref().map(ReleaseSpec::bits).transpose()?;
    let releases = |r: Releases| Releases::new(release_i.unwrap_or(r.i()), release_j.unwrap_or(r.j()));
    session.edit("Change frames", |tx| {
        let rows = ids
            .iter()
            .map(|id| {
                tx.model().frames().get(*id).map(|f| {
                    let (section, role) = (section.unwrap_or(f.section), role.map_or(f.role, Into::into));
                    (*id, Frame { section, role, releases: releases(f.releases), ..*f })
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| CommandError::InvalidInput("a frame to change does not exist".to_owned()))?;
        let changed = rows.len();
        if changed > 0 {
            tx.apply(Change::Frames(Op::Set(rows)))?;
        }
        Ok(json!({"changed": changed}))
    })
}

pub fn area_set(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        areas: Vec<Ref>,
        #[serde(default)]
        section: Option<Ref>,
        #[serde(default)]
        role: Option<AreaRoleName>,
    }
    let Input { areas, section, role } = parse(input)?;
    check_batch(areas.len())?;
    let ids = ids::<Area>(&areas)?;
    let section = section.as_ref().map(Ref::id).transpose()?;
    session.edit("Change areas", |tx| {
        let rows =
            ids.iter()
                .map(|id| {
                    tx.model().areas().get(*id).map(|a| {
                        (*id, Area { corners: a.corners.clone(), section: section.unwrap_or(a.section), role: role.map_or(a.role, Into::into) })
                    })
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| CommandError::InvalidInput("an area to change does not exist".to_owned()))?;
        let changed = rows.len();
        if changed > 0 {
            tx.apply(Change::Areas(Op::Set(rows)))?;
        }
        Ok(json!({"changed": changed}))
    })
}
