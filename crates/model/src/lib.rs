//! The structural model (L2): what a StructureCraft file holds.
//!
//! Elements live in generational [`Table`]s: one contiguous `Vec` per kind, addressed by ids
//! that go stale when their element is deleted. The model changes only through [`Change`]s,
//! and applying one returns the change that undoes it, so the undo history stores edits, not
//! copies of a model that may hold hundreds of thousands of nodes.

mod change;
mod elements;
mod grid;
mod loads;
mod spatial;
mod table;
mod units;

use structurecraft_geom::{Aabb, DVec3};

pub use change::{Change, ModelError, Op};
pub use elements::{
    AREA_CORNERS, Area, AreaId, AreaRole, Corners, Frame, FrameId, FrameRole, Material, MaterialId, Node, NodeId, Releases, Restraint, Section,
    SectionId, SectionShape,
};
pub use grid::{Grid, GridLine, Story};
pub use loads::{
    AreaLoad, AreaLoadId, CaseId, Combination, CombinationKind, ComboId, FrameLoad, FrameLoadId, FrameLoadShape, LoadCase, LoadDirection,
    LoadPattern, MAX_LOAD, MAX_TERMS, NodeLoad, NodeLoadId, PatternId, PatternKind, SlabSpan, local_axes,
};
pub use spatial::{MERGE_TOLERANCE, NodeIndex};
pub use table::{Id, SlotError, Table};
pub use units::{ForceUnit, LengthUnit, Units};

/// No coordinate may be farther than this from the origin (metres).
pub const MAX_COORDINATE: f64 = 1e7;

/// A structural model.
#[derive(Debug, Clone)]
pub struct Model {
    pub(crate) name: String,
    /// The units the user sees; the model itself always stores SI (N, m).
    pub(crate) display_units: Units,
    pub(crate) grid: Grid,
    pub(crate) nodes: Table<Node>,
    pub(crate) frames: Table<Frame>,
    pub(crate) areas: Table<Area>,
    pub(crate) materials: Table<Material>,
    pub(crate) sections: Table<Section>,
    pub(crate) patterns: Table<LoadPattern>,
    pub(crate) cases: Table<LoadCase>,
    pub(crate) combinations: Table<Combination>,
    pub(crate) node_loads: Table<NodeLoad>,
    pub(crate) frame_loads: Table<FrameLoad>,
    pub(crate) area_loads: Table<AreaLoad>,
    /// For each node slot, how many frame ends and area corners use it.
    pub(crate) node_uses: Vec<u32>,
    pub(crate) index: NodeIndex,
}

impl Default for Model {
    fn default() -> Self {
        Self::empty()
    }
}

impl PartialEq for Model {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.display_units == other.display_units
            && self.grid == other.grid
            && self.nodes == other.nodes
            && self.frames == other.frames
            && self.areas == other.areas
            && self.materials == other.materials
            && self.sections == other.sections
            && self.patterns == other.patterns
            && self.cases == other.cases
            && self.combinations == other.combinations
            && self.node_loads == other.node_loads
            && self.frame_loads == other.frame_loads
            && self.area_loads == other.area_loads
    }
}

/// The parts of a model, as a file holds them. [`Model::from_parts`] checks them.
#[derive(Debug, Clone, Default)]
pub struct Parts {
    pub name: String,
    pub display_units: Units,
    pub grid: Grid,
    pub nodes: Table<Node>,
    pub frames: Table<Frame>,
    pub areas: Table<Area>,
    pub materials: Table<Material>,
    pub sections: Table<Section>,
    pub patterns: Table<LoadPattern>,
    pub cases: Table<LoadCase>,
    pub combinations: Table<Combination>,
    pub node_loads: Table<NodeLoad>,
    pub frame_loads: Table<FrameLoad>,
    pub area_loads: Table<AreaLoad>,
}

/// The loads that sit on some nodes, frames and areas (see [`Model::loads_on`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadsOn {
    pub node_loads: Vec<NodeLoadId>,
    pub frame_loads: Vec<FrameLoadId>,
    pub area_loads: Vec<AreaLoadId>,
}

impl Model {
    /// A model with nothing in it, not even materials.
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            display_units: Units::default(),
            grid: Grid::default(),
            nodes: Table::default(),
            frames: Table::default(),
            areas: Table::default(),
            materials: Table::default(),
            sections: Table::default(),
            patterns: Table::default(),
            cases: Table::default(),
            combinations: Table::default(),
            node_loads: Table::default(),
            frame_loads: Table::default(),
            area_loads: Table::default(),
            node_uses: Vec::new(),
            index: NodeIndex::default(),
        }
    }

    /// A new model with a concrete material, a beam section, a column section and a slab
    /// section, so drawing works straight away, and Dead (with self-weight) and Live load
    /// patterns, each with its own case.
    pub fn new(name: impl Into<String>) -> Self {
        let mut m = Self { name: name.into(), ..Self::empty() };
        let concrete = Material { name: "Concrete".to_owned(), elastic_modulus: 25e9, poisson_ratio: 0.2, density: 2400.0 };
        let Ok(concrete) = m.materials.insert(concrete) else { return m };
        for (name, shape) in [
            ("Beam 300x600", SectionShape::Rectangle { b: 0.3, h: 0.6 }),
            ("Column 500x500", SectionShape::Rectangle { b: 0.5, h: 0.5 }),
            ("Slab 150", SectionShape::Shell { t: 0.15 }),
        ] {
            let _ = m.sections.insert(Section { name: name.to_owned(), material: concrete, shape });
        }
        for (name, kind, self_weight) in [("Dead", PatternKind::Dead, 1.0), ("Live", PatternKind::Live, 0.0)] {
            if let Ok(p) = m.patterns.insert(LoadPattern { name: name.to_owned(), kind, self_weight }) {
                let _ = m.cases.insert(LoadCase { name: name.to_owned(), patterns: vec![(p, 1.0)] });
            }
        }
        m
    }

    /// A model from a file's parts, after checking every reference and value.
    pub fn from_parts(parts: Parts) -> Result<Self, ModelError> {
        let mut m = Self { name: parts.name, display_units: parts.display_units, ..Self::empty() };
        m.set_grid(parts.grid)?;
        m.materials = parts.materials;
        for (_, row) in m.materials.iter() {
            change::check_material(row)?;
        }
        m.sections = parts.sections;
        for (_, row) in m.sections.iter() {
            change::check_section(&m, row)?;
        }
        m.nodes = parts.nodes;
        m.node_uses = vec![0; m.nodes.capacity()];
        m.index = NodeIndex::with_capacity(m.nodes.len());
        for (id, row) in m.nodes.iter() {
            change::check_node(row)?;
            m.index.insert(id, row.position);
        }
        m.frames = parts.frames;
        m.areas = parts.areas;
        let mut used = Vec::new();
        for (_, f) in m.frames.iter() {
            change::check_frame(&m, f)?;
            used.extend([f.i, f.j]);
        }
        for (_, a) in m.areas.iter() {
            change::check_area(&m, a)?;
            used.extend(a.corners.iter().copied());
        }
        m.use_nodes(used, 1);
        m.patterns = parts.patterns;
        for (_, row) in m.patterns.iter() {
            change::check_pattern(row)?;
        }
        m.cases = parts.cases;
        for (_, row) in m.cases.iter() {
            change::check_case(&m, row)?;
        }
        m.combinations = parts.combinations;
        for (_, row) in m.combinations.iter() {
            change::check_combination(&m, row)?;
        }
        m.node_loads = parts.node_loads;
        for (_, row) in m.node_loads.iter() {
            change::check_node_load(&m, row)?;
        }
        m.frame_loads = parts.frame_loads;
        for (_, row) in m.frame_loads.iter() {
            change::check_frame_load(&m, row)?;
        }
        m.area_loads = parts.area_loads;
        for (_, row) in m.area_loads.iter() {
            change::check_area_load(&m, row)?;
        }
        Ok(m)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn display_units(&self) -> Units {
        self.display_units
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn nodes(&self) -> &Table<Node> {
        &self.nodes
    }

    pub fn frames(&self) -> &Table<Frame> {
        &self.frames
    }

    pub fn areas(&self) -> &Table<Area> {
        &self.areas
    }

    pub fn materials(&self) -> &Table<Material> {
        &self.materials
    }

    pub fn sections(&self) -> &Table<Section> {
        &self.sections
    }

    pub fn patterns(&self) -> &Table<LoadPattern> {
        &self.patterns
    }

    pub fn cases(&self) -> &Table<LoadCase> {
        &self.cases
    }

    pub fn combinations(&self) -> &Table<Combination> {
        &self.combinations
    }

    pub fn node_loads(&self) -> &Table<NodeLoad> {
        &self.node_loads
    }

    pub fn frame_loads(&self) -> &Table<FrameLoad> {
        &self.frame_loads
    }

    pub fn area_loads(&self) -> &Table<AreaLoad> {
        &self.area_loads
    }

    /// The loads on any of these nodes, frames and areas, optionally only those of one
    /// pattern: one pass over each load table.
    pub fn loads_on(&self, nodes: &[NodeId], frames: &[FrameId], areas: &[AreaId], pattern: Option<PatternId>) -> LoadsOn {
        fn marks<T>(ids: &[Id<T>], capacity: usize) -> Vec<bool> {
            let mut marked = vec![false; if ids.is_empty() { 0 } else { capacity }];
            for id in ids {
                if let Some(m) = marked.get_mut(id.index() as usize) {
                    *m = true;
                }
            }
            marked
        }
        fn hits<T, L>(table: &Table<L>, marked: &[bool], of: impl Fn(&L) -> (Id<T>, PatternId), pattern: Option<PatternId>) -> Vec<Id<L>> {
            if marked.is_empty() {
                return Vec::new();
            }
            table
                .iter()
                .filter(|(_, l)| {
                    let (on, p) = of(l);
                    marked.get(on.index() as usize).copied().unwrap_or(false) && pattern.is_none_or(|q| q == p)
                })
                .map(|(id, _)| id)
                .collect()
        }
        // A stale id marks its slot too; the slot's live row is a different element, so check.
        let nodes: Vec<_> = nodes.iter().copied().filter(|id| self.nodes.contains(*id)).collect();
        let frames: Vec<_> = frames.iter().copied().filter(|id| self.frames.contains(*id)).collect();
        let areas: Vec<_> = areas.iter().copied().filter(|id| self.areas.contains(*id)).collect();
        LoadsOn {
            node_loads: hits(&self.node_loads, &marks(&nodes, self.nodes.capacity()), |l| (l.node, l.pattern), pattern),
            frame_loads: hits(&self.frame_loads, &marks(&frames, self.frames.capacity()), |l| (l.frame, l.pattern), pattern),
            area_loads: hits(&self.area_loads, &marks(&areas, self.areas.capacity()), |l| (l.area, l.pattern), pattern),
        }
    }

    /// How many frame ends and area corners use a node.
    pub fn node_uses(&self, id: NodeId) -> u32 {
        if self.nodes.contains(id) { self.node_uses.get(id.index() as usize).copied().unwrap_or(0) } else { 0 }
    }

    /// The node within [`MERGE_TOLERANCE`] of `p`, if any.
    pub fn node_at(&self, p: DVec3) -> Option<NodeId> {
        self.index.find(p, |id| self.nodes.get(id).map(|n| n.position))
    }

    /// The frames and areas that use any of `nodes` (one pass over both tables).
    pub fn dependents(&self, nodes: &[NodeId]) -> (Vec<FrameId>, Vec<AreaId>) {
        if nodes.iter().all(|n| self.node_uses(*n) == 0) {
            return (Vec::new(), Vec::new());
        }
        let mut marked = vec![false; self.nodes.capacity()];
        for n in nodes {
            if let Some(m) = marked.get_mut(n.index() as usize) {
                *m = self.nodes.contains(*n);
            }
        }
        let hit = |n: &NodeId| marked.get(n.index() as usize).copied().unwrap_or(false);
        let frames = self.frames.iter().filter(|(_, f)| hit(&f.i) || hit(&f.j)).map(|(id, _)| id).collect();
        let areas = self.areas.iter().filter(|(_, a)| a.corners.iter().any(hit)).map(|(id, _)| id).collect();
        (frames, areas)
    }

    /// The extent of every node; empty when there are none.
    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.nodes.iter().map(|(_, n)| n.position))
    }

    pub(crate) fn use_nodes(&mut self, ids: impl IntoIterator<Item = NodeId>, delta: i32) {
        for id in ids {
            let i = id.index() as usize;
            if i >= self.node_uses.len() {
                self.node_uses.resize(i + 1, 0);
            }
            if let Some(u) = self.node_uses.get_mut(i) {
                *u = u.saturating_add_signed(delta);
            }
        }
    }

    fn set_grid(&mut self, grid: Grid) -> Result<Grid, ModelError> {
        if !grid.is_valid() {
            return Err(ModelError::Invalid("grid coordinates must be finite and stories sorted by elevation".to_owned()));
        }
        Ok(std::mem::replace(&mut self.grid, grid))
    }
}
