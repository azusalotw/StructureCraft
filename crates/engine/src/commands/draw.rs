//! Drawing: nodes, frames and areas from points. A point within the merge tolerance of an
//! existing node (or of another point in the same command) uses that node.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_geom::DVec3;
use structurecraft_model::{Area, AreaRole, Frame, FrameRole, Id, Node, NodeId, NodeIndex, Restraint};

use super::{Point, Ref, check_batch, default_section, id_list, parse, point_schema};
use crate::{CommandError, Session, Tx};

pub const NODE_SUMMARY: &str = "Add nodes at points; a point on an existing node reuses it.";
pub const FRAME_SUMMARY: &str = "Draw frames (beams, columns, braces) between pairs of points, creating or reusing nodes at their ends.";
pub const AREA_SUMMARY: &str = "Draw areas (slabs, walls) through polygons of points, creating or reusing nodes at their corners.";

pub fn node_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "points": {"type": "array", "items": point_schema()},
            "restraint": {"enum": ["free", "pinned", "fixed"], "description": "For the new nodes; default free"},
        },
        "required": ["points"],
        "additionalProperties": false,
    })
}

pub fn frame_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "segments": {"type": "array", "items": {"type": "array", "items": point_schema(), "minItems": 2, "maxItems": 2}},
            "section": {"type": "string", "description": "Section id; default: the first frame section (the second for columns)"},
            "role": {"enum": ["beam", "column", "brace"], "description": "Default: from each frame's direction"},
        },
        "required": ["segments"],
        "additionalProperties": false,
    })
}

pub fn area_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "polygons": {"type": "array", "items": {"type": "array", "items": point_schema(), "minItems": 3}},
            "section": {"type": "string", "description": "Section id; default: the first slab or wall section"},
            "role": {"enum": ["slab", "wall"], "description": "Default: slab"},
        },
        "required": ["polygons"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestraintName {
    #[default]
    Free,
    Pinned,
    Fixed,
}

impl From<RestraintName> for Restraint {
    fn from(r: RestraintName) -> Self {
        match r {
            RestraintName::Free => Restraint::FREE,
            RestraintName::Pinned => Restraint::PINNED,
            RestraintName::Fixed => Restraint::FIXED,
        }
    }
}

/// The node at each point: an existing one within the merge tolerance, or one added in a single
/// batch (points that merge with each other share it). Returns the ids and how many are new.
pub(crate) fn nodes_at(tx: &mut Tx<'_>, points: &[DVec3], restraint: Restraint) -> Result<(Vec<NodeId>, usize), CommandError> {
    check_batch(points.len())?;
    enum At {
        Old(NodeId),
        New(usize),
    }
    let mut pending: Vec<DVec3> = Vec::new();
    let mut pending_index = NodeIndex::default();
    let mut at = Vec::with_capacity(points.len());
    for p in points {
        if let Some(id) = tx.model().node_at(*p) {
            at.push(At::Old(id));
        } else if let Some(k) = pending_index.find(*p, |k| pending.get(k.index() as usize).copied()) {
            at.push(At::New(k.index() as usize));
        } else {
            let k = pending.len();
            pending_index.insert(Id::new(k as u32, 1), *p);
            pending.push(*p);
            at.push(At::New(k));
        }
    }
    let created = pending.len();
    let new_ids = tx.add_nodes(pending.into_iter().map(|position| Node { position, restraint }).collect())?;
    let ids = at
        .into_iter()
        .map(|a| match a {
            At::Old(id) => Some(id),
            At::New(k) => new_ids.get(k).copied(),
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CommandError::InvalidInput("lost a new node".to_owned()))?;
    Ok((ids, created))
}

pub fn node(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        points: Vec<Point>,
        #[serde(default)]
        restraint: RestraintName,
    }
    let Input { points, restraint } = parse(input)?;
    let points: Vec<_> = points.into_iter().map(|p| p.0).collect();
    session.edit("Add nodes", |tx| {
        let (ids, created) = nodes_at(tx, &points, restraint.into())?;
        Ok(json!({"nodes": id_list(&ids), "created": created}))
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RoleName {
    Beam,
    Column,
    Brace,
}

pub fn frame(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        segments: Vec<[Point; 2]>,
        #[serde(default)]
        section: Option<Ref>,
        #[serde(default)]
        role: Option<RoleName>,
    }
    let Input { segments, section, role } = parse(input)?;
    check_batch(segments.len())?;
    let section = section.as_ref().map(Ref::id).transpose()?;
    let points: Vec<DVec3> = segments.iter().flat_map(|[a, b]| [a.0, b.0]).collect();
    session.edit("Draw frames", |tx| {
        let (nodes, created) = nodes_at(tx, &points, Restraint::FREE)?;
        let mut rows = Vec::with_capacity(segments.len());
        for ([a, b], &[i, j]) in segments.iter().zip(nodes.as_chunks::<2>().0) {
            let role = match role {
                Some(RoleName::Beam) => FrameRole::Beam,
                Some(RoleName::Column) => FrameRole::Column,
                Some(RoleName::Brace) => FrameRole::Brace,
                None => FrameRole::from_direction(b.0 - a.0),
            };
            let section = match section {
                Some(s) => s,
                None => default_section(tx.model(), false, role == FrameRole::Column)?,
            };
            rows.push(Frame { i, j, section, role, angle: 0.0 });
        }
        let frames = tx.add_frames(rows)?;
        Ok(json!({"frames": id_list(&frames), "nodes_created": created}))
    })
}

pub fn area(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        polygons: Vec<Vec<Point>>,
        #[serde(default)]
        section: Option<Ref>,
        #[serde(default)]
        role: Option<AreaRoleName>,
    }
    #[derive(Debug, Clone, Copy, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum AreaRoleName {
        Slab,
        Wall,
    }
    let Input { polygons, section, role } = parse(input)?;
    check_batch(polygons.iter().map(Vec::len).sum())?;
    let section = section.as_ref().map(Ref::id).transpose()?;
    let role = match role {
        Some(AreaRoleName::Wall) => AreaRole::Wall,
        _ => AreaRole::Slab,
    };
    let points: Vec<DVec3> = polygons.iter().flatten().map(|p| p.0).collect();
    session.edit("Draw areas", |tx| {
        let (nodes, created) = nodes_at(tx, &points, Restraint::FREE)?;
        let section = match section {
            Some(s) => s,
            None => default_section(tx.model(), true, false)?,
        };
        let mut rows = Vec::with_capacity(polygons.len());
        let mut rest = nodes.as_slice();
        for polygon in &polygons {
            let (corners, tail) = rest.split_at_checked(polygon.len()).ok_or_else(|| CommandError::InvalidInput("lost a corner".to_owned()))?;
            rest = tail;
            rows.push(Area { corners: corners.iter().copied().collect(), section, role });
        }
        let areas = tx.add_areas(rows)?;
        Ok(json!({"areas": id_list(&areas), "nodes_created": created}))
    })
}
