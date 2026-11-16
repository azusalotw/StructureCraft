//! The rows of the model's tables. All quantities are SI: metres, newtons, pascals, kilograms.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use structurecraft_geom::DVec3;

use crate::table::Id;

pub type NodeId = Id<Node>;
pub type FrameId = Id<Frame>;
pub type AreaId = Id<Area>;
pub type MaterialId = Id<Material>;
pub type SectionId = Id<Section>;

/// Which of a node's six degrees of freedom are held: bits 0–2 translation along X, Y, Z,
/// bits 3–5 rotation about X, Y, Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct Restraint(pub u8);

impl Restraint {
    pub const FREE: Self = Self(0);
    pub const PINNED: Self = Self(0b000_111);
    pub const FIXED: Self = Self(0b111_111);

    pub fn from_bits(bits: u8) -> Self {
        Self(bits & 0b111_111)
    }

    pub fn holds(self, dof: usize) -> bool {
        dof < 6 && self.0 & (1 << dof) != 0
    }
}

/// A joint.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Node {
    pub position: DVec3,
    pub restraint: Restraint,
}

/// What a frame is for; drawing picks one from its direction and the user can change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameRole {
    #[default]
    Beam,
    Column,
    Brace,
}

impl FrameRole {
    pub const ALL: [Self; 3] = [Self::Beam, Self::Column, Self::Brace];

    /// Vertical members are columns, horizontal ones beams, the rest braces.
    pub fn from_direction(d: DVec3) -> Self {
        let len = d.length();
        if !(len > 0.0) {
            return Self::Beam;
        }
        let vertical = (d.z / len).abs();
        if vertical > 0.999 {
            Self::Column
        } else if vertical < 0.001 {
            Self::Beam
        } else {
            Self::Brace
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Beam => 0,
            Self::Column => 1,
            Self::Brace => 2,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

/// Which internal forces a frame's ends do not carry, in the member's local axes: bits 0–5 at
/// end i and 6–11 at end j, each end in the order axial (P), shear along 2 (V2), shear along
/// 3 (V3), torsion (T), moment about 2 (M2), moment about 3 (M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
pub struct Releases(pub u16);

impl Releases {
    pub const NONE: Self = Self(0);
    pub const P: u16 = 1;
    pub const V2: u16 = 1 << 1;
    pub const V3: u16 = 1 << 2;
    pub const T: u16 = 1 << 3;
    pub const M2: u16 = 1 << 4;
    pub const M3: u16 = 1 << 5;
    /// Both bending moments: a pin.
    pub const MOMENTS: u16 = Self::M2 | Self::M3;
    /// The names of the six bits of one end, in order.
    pub const NAMES: [&'static str; 6] = ["p", "v2", "v3", "t", "m2", "m3"];

    /// From the bits of end i and end j (six bits each).
    pub fn new(i: u16, j: u16) -> Self {
        Self((i & 0x3f) | (j & 0x3f) << 6)
    }

    pub fn i(self) -> u16 {
        self.0 & 0x3f
    }

    pub fn j(self) -> u16 {
        self.0 >> 6 & 0x3f
    }

    /// Whether local degree of freedom `dof` (0–11, end i first) is released.
    pub fn releases(self, dof: usize) -> bool {
        dof < 12 && self.0 & (1 << dof) != 0
    }

    /// Releasing the same axial force or torsion at both ends leaves the member free to slide
    /// or spin, so the model refuses it.
    pub fn is_valid(self) -> bool {
        self.0 >> 12 == 0 && self.i() & self.j() & (Self::P | Self::T) == 0
    }
}

/// A line element from node `i` to node `j`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub i: NodeId,
    pub j: NodeId,
    pub section: SectionId,
    pub role: FrameRole,
    /// Rotation of the section about the member axis, radians.
    pub angle: f64,
    pub releases: Releases,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaRole {
    #[default]
    Slab,
    Wall,
}

impl AreaRole {
    pub const ALL: [Self; 2] = [Self::Slab, Self::Wall];

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Slab => 0,
            Self::Wall => 1,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.get(usize::from(v)).copied()
    }
}

/// Most areas are quadrilaterals or triangles: up to four corners stay inline.
pub type Corners = SmallVec<[NodeId; 4]>;

/// The fewest and most corners an area may have.
pub const AREA_CORNERS: std::ops::RangeInclusive<usize> = 3..=64;

/// A planar shell element (slab or wall) through its corner nodes, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Area {
    pub corners: Corners,
    pub section: SectionId,
    pub role: AreaRole,
}

/// An isotropic linear-elastic material.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Young's modulus, Pa.
    pub elastic_modulus: f64,
    pub poisson_ratio: f64,
    /// kg/m³.
    pub density: f64,
}

/// A section's shape, in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum SectionShape {
    /// Solid rectangle, width `b` along the local 2 axis, depth `h` along 3.
    Rectangle { b: f64, h: f64 },
    /// Solid circle.
    Circle { d: f64 },
    /// Doubly symmetric I: overall depth, flange width, flange and web thickness.
    ISection { h: f64, b: f64, tf: f64, tw: f64 },
    /// A shell of uniform thickness (slabs and walls).
    Shell { t: f64 },
}

impl Default for SectionShape {
    fn default() -> Self {
        Self::Rectangle { b: 0.3, h: 0.5 }
    }
}

impl SectionShape {
    pub fn is_shell(&self) -> bool {
        matches!(self, Self::Shell { .. })
    }

    /// Every dimension positive and finite.
    pub fn is_valid(&self) -> bool {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        match *self {
            Self::Rectangle { b, h } => ok(b) && ok(h),
            Self::Circle { d } => ok(d),
            Self::ISection { h, b, tf, tw } => ok(h) && ok(b) && ok(tf) && ok(tw) && 2.0 * tf < h && tw < b,
            Self::Shell { t } => ok(t),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Section {
    pub name: String,
    pub material: MaterialId,
    pub shape: SectionShape,
}
