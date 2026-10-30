//! Generated models: a regular frame building, for starting a model and for benchmarks.

use serde::Deserialize;
use serde_json::{Value, json};
use structurecraft_geom::DVec3;
use structurecraft_model::{Area, AreaRole, Change, Frame, FrameRole, Grid, Restraint};

use super::draw::nodes_at;
use super::{MAX_BATCH, check_batch, default_section, parse};
use crate::{CommandError, Session};

pub const SUMMARY: &str = "Generate a regular building: a plan grid of bays, stories of columns, beams both ways and slabs, fixed at the base.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "bays_x": {"type": "integer", "minimum": 1},
            "bays_y": {"type": "integer", "minimum": 1},
            "stories": {"type": "integer", "minimum": 1},
            "bay_x": {"type": "number", "exclusiveMinimum": 0, "description": "Bay width along X, m; default 8"},
            "bay_y": {"type": "number", "exclusiveMinimum": 0, "description": "Bay width along Y, m; default 8"},
            "story_height": {"type": "number", "exclusiveMinimum": 0, "description": "m; default 3.5"},
            "slabs": {"type": "boolean", "description": "Default true"},
        },
        "required": ["bays_x", "bays_y", "stories"],
        "additionalProperties": false,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    bays_x: usize,
    bays_y: usize,
    stories: usize,
    #[serde(default = "eight")]
    bay_x: f64,
    #[serde(default = "eight")]
    bay_y: f64,
    #[serde(default = "story_height")]
    story_height: f64,
    #[serde(default = "yes")]
    slabs: bool,
}

fn eight() -> f64 {
    8.0
}
fn story_height() -> f64 {
    3.5
}
fn yes() -> bool {
    true
}

pub fn building(session: &mut Session, input: Value) -> Result<Value, CommandError> {
    let Input { bays_x, bays_y, stories, bay_x, bay_y, story_height, slabs } = parse(input)?;
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if bays_x == 0 || bays_y == 0 || stories == 0 || !positive(bay_x) || !positive(bay_y) || !positive(story_height) {
        return Err(CommandError::InvalidInput("bays, stories and sizes must be positive".to_owned()));
    }
    let (nx, ny, nz) = (bays_x + 1, bays_y + 1, stories + 1);
    let nodes = nx.checked_mul(ny).and_then(|n| n.checked_mul(nz)).unwrap_or(usize::MAX);
    check_batch(nodes)?;
    check_batch(nodes.saturating_mul(4))?;
    let point = |i: usize, j: usize, k: usize| DVec3::new(i as f64 * bay_x, j as f64 * bay_y, k as f64 * story_height);
    let level = |k: usize| (0..ny).flat_map(move |j| (0..nx).map(move |i| point(i, j, k)));
    let base: Vec<DVec3> = level(0).collect();
    let above: Vec<DVec3> = (1..nz).flat_map(level).collect();
    debug_assert!(base.len() + above.len() <= MAX_BATCH);

    session.edit("Generate building", |tx| {
        tx.apply(Change::Grid(Grid::regular(bays_x, bay_x, bays_y, bay_y, stories, story_height)))?;
        let (mut ids, mut created) = nodes_at(tx, &base, Restraint::FIXED)?;
        let (upper, more) = nodes_at(tx, &above, Restraint::FREE)?;
        ids.extend(upper);
        created += more;
        let at = |i: usize, j: usize, k: usize| ids.get((k * ny + j) * nx + i).copied().unwrap_or_default();

        let (beam, column) = (default_section(tx.model(), false, false)?, default_section(tx.model(), false, true)?);
        let mut frames = Vec::new();
        for k in 1..nz {
            for j in 0..ny {
                for i in 0..nx {
                    let frame = |a, b, section, role| Frame { i: a, j: b, section, role, angle: 0.0 };
                    frames.push(frame(at(i, j, k - 1), at(i, j, k), column, FrameRole::Column));
                    if i + 1 < nx {
                        frames.push(frame(at(i, j, k), at(i + 1, j, k), beam, FrameRole::Beam));
                    }
                    if j + 1 < ny {
                        frames.push(frame(at(i, j, k), at(i, j + 1, k), beam, FrameRole::Beam));
                    }
                }
            }
        }
        let frame_count = tx.add_frames(frames)?.len();

        let mut area_count = 0;
        if slabs {
            let slab = default_section(tx.model(), true, false)?;
            let mut areas = Vec::with_capacity(bays_x * bays_y * stories);
            for k in 1..nz {
                for j in 0..bays_y {
                    for i in 0..bays_x {
                        let corners = [at(i, j, k), at(i + 1, j, k), at(i + 1, j + 1, k), at(i, j + 1, k)];
                        areas.push(Area { corners: corners.into_iter().collect(), section: slab, role: AreaRole::Slab });
                    }
                }
            }
            area_count = tx.add_areas(areas)?.len();
        }
        Ok(json!({"nodes_created": created, "frames": frame_count, "areas": area_count}))
    })
}
