//! Loads. A pattern names a source of load (dead, live, wind) and may carry the structure's
//! own weight; loads on nodes, frames and areas each belong to one pattern. A case scales and
//! sums patterns into what one analysis solves; a combination scales and sums cases, or
//! envelopes them, after the analysis. All values are SI: newtons, metres, pascals.

use serde::{Deserialize, Serialize};
use structurecraft_geom::DVec3;

use crate::elements::{AreaId, FrameId, NodeId};
use crate::table::Id;

pub type PatternId = Id<LoadPattern>;
pub type CaseId = Id<LoadCase>;
pub type ComboId = Id<Combination>;
pub type NodeLoadId = Id<NodeLoad>;
pub type FrameLoadId = Id<FrameLoad>;
pub type AreaLoadId = Id<AreaLoad>;

/// The most patterns one case sums, or cases one combination takes.
pub const MAX_TERMS: usize = 1000;

/// No load value may be larger than this (N, N/m, N·m or Pa).
pub const MAX_LOAD: f64 = 1e15;

/// What kind of load a pattern holds; design codes factor each kind differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    #[default]
    Dead,
    SuperimposedDead,
    Live,
    RoofLive,
    Snow,
    Wind,
    Seismic,
    Other,
}

impl PatternKind {
    pub const ALL: [Self; 8] = [Self::Dead, Self::SuperimposedDead, Self::Live, Self::RoofLive, Self::Snow, Self::Wind, Self::Seismic, Self::Other];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dead => "Dead",
            Self::SuperimposedDead => "Superimposed dead",
            Self::Live => "Live",
            Self::RoofLive => "Roof live",
            Self::Snow => "Snow",
            Self::Wind => "Wind",
            Self::Seismic => "Seismic",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LoadPattern {
    pub name: String,
    pub kind: PatternKind,
    /// How many times the structure's own weight this pattern adds (usually 1 or 0).
    pub self_weight: f64,
}

/// A linear static case: the sum of its patterns, each times its factor.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadCase {
    pub name: String,
    pub patterns: Vec<(PatternId, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombinationKind {
    /// The sum of the cases, each times its factor.
    #[default]
    Linear,
    /// The largest and smallest of the factored cases, value by value.
    Envelope,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Combination {
    pub name: String,
    pub kind: CombinationKind,
    pub cases: Vec<(CaseId, f64)>,
}

/// Forces and moments on a node, along and about the global axes: Fx, Fy, Fz (N), Mx, My, Mz (N·m).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NodeLoad {
    pub node: NodeId,
    pub pattern: PatternId,
    pub values: [f64; 6],
}

/// Which way a frame or area load acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadDirection {
    /// Down: along −Z, a positive value pushes down.
    #[default]
    Gravity,
    X,
    Y,
    Z,
    /// Along the member's local axes (see [`local_axes`]).
    Local1,
    Local2,
    Local3,
}

impl LoadDirection {
    pub const ALL: [Self; 7] = [Self::Gravity, Self::X, Self::Y, Self::Z, Self::Local1, Self::Local2, Self::Local3];

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Gravity => 0,
            Self::X => 1,
            Self::Y => 2,
            Self::Z => 3,
            Self::Local1 => 4,
            Self::Local2 => 5,
            Self::Local3 => 6,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }

    /// The unit vector a positive value acts along, for a member with these local axes.
    pub fn vector(self, axes: &[DVec3; 3]) -> DVec3 {
        match self {
            Self::Gravity => DVec3::NEG_Z,
            Self::X => DVec3::X,
            Self::Y => DVec3::Y,
            Self::Z => DVec3::Z,
            Self::Local1 => axes[0],
            Self::Local2 => axes[1],
            Self::Local3 => axes[2],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameLoadShape {
    /// Spread from `start` to `end`, varying linearly from `value_start` to `value_end` (N/m).
    #[default]
    Distributed,
    /// One force `value_start` (N) at `start`.
    Point,
}

impl FrameLoadShape {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Distributed => 0,
            Self::Point => 1,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Distributed),
            1 => Some(Self::Point),
            _ => None,
        }
    }
}

/// A load along a frame. Positions are fractions of its length from end i, 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameLoad {
    pub frame: FrameId,
    pub pattern: PatternId,
    pub shape: FrameLoadShape,
    pub direction: LoadDirection,
    pub start: f64,
    pub end: f64,
    pub value_start: f64,
    pub value_end: f64,
}

/// How a slab hands its load to the frames along its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlabSpan {
    /// One-way when the long side is more than twice the short side, two-way otherwise.
    #[default]
    Auto,
    OneWay,
    TwoWay,
}

impl SlabSpan {
    pub const ALL: [Self; 3] = [Self::Auto, Self::OneWay, Self::TwoWay];

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::OneWay => 1,
            Self::TwoWay => 2,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

/// A uniform pressure on an area, acting down (Pa).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AreaLoad {
    pub area: AreaId,
    pub pattern: PatternId,
    pub pressure: f64,
    pub span: SlabSpan,
}

/// A frame's local axes: 1 from end i to end j; for a vertical member 2 points along global X,
/// otherwise 2 is the upward direction square to 1; 3 completes a right-handed set. `angle`
/// turns 2 and 3 about 1. `None` when the ends coincide.
pub fn local_axes(i: DVec3, j: DVec3, angle: f64) -> Option<[DVec3; 3]> {
    let a1 = (j - i).try_normalize()?;
    let a2 = if a1.z.abs() > 1.0 - 1e-3 { (DVec3::X - a1 * a1.x).try_normalize()? } else { (DVec3::Z - a1 * a1.z).try_normalize()? };
    let a3 = a1.cross(a2);
    let (s, c) = angle.sin_cos();
    Some([a1, a2 * c + a3 * s, a3 * c - a2 * s])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-12
    }

    #[test]
    fn local_axes_of_beams_columns_and_braces() {
        let [a1, a2, a3] = local_axes(DVec3::ZERO, DVec3::new(6.0, 0.0, 0.0), 0.0).unwrap();
        assert!(close(a1, DVec3::X) && close(a2, DVec3::Z) && close(a3, DVec3::NEG_Y));
        let [a1, a2, a3] = local_axes(DVec3::ZERO, DVec3::new(0.0, 0.0, 3.0), 0.0).unwrap();
        assert!(close(a1, DVec3::Z) && close(a2, DVec3::X) && close(a3, DVec3::Y));
        let [a1, a2, a3] = local_axes(DVec3::ZERO, DVec3::new(3.0, 4.0, 5.0), 0.7).unwrap();
        for (u, v) in [(a1, a2), (a2, a3), (a1, a3)] {
            assert!(u.dot(v).abs() < 1e-12);
        }
        assert!(close(a1.cross(a2), a3));
        assert!(local_axes(DVec3::ONE, DVec3::ONE, 0.0).is_none());
    }

    #[test]
    fn a_quarter_turn_swaps_two_and_three() {
        let [_, a2, a3] = local_axes(DVec3::ZERO, DVec3::X, std::f64::consts::FRAC_PI_2).unwrap();
        assert!(close(a2, DVec3::NEG_Y) && close(a3, DVec3::NEG_Z));
    }
}
