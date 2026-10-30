//! The structural model (L2): what a StructureCraft file holds.
//!
//! Step 2 of the plan fills this in with struct-of-arrays storage for nodes, frames (beams,
//! columns, braces) and areas (slabs, walls), keyed by generational ids so models of hundreds of
//! thousands of nodes stay cache friendly and the solver can read them without copying.

mod units;

use serde::{Deserialize, Serialize};
use structurecraft_geom::Aabb;

pub use units::{ForceUnit, LengthUnit, Units};

/// A structural model.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    /// The units the user sees; the model itself always stores SI (N, m).
    pub display_units: Units,
}

impl Model {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }

    /// The extent of everything in the model; empty until the model has nodes.
    pub fn bounds(&self) -> Aabb {
        Aabb::EMPTY
    }
}
