//! Changes: the only way a model is edited. Applying a change checks all of it first, so a
//! change either applies whole or not at all, and returns its inverse.

use std::collections::HashSet;

use structurecraft_geom::DVec3;

use crate::spatial::MERGE_TOLERANCE;
use crate::{AREA_CORNERS, Area, Frame, Grid, Id, MAX_COORDINATE, Material, Model, Node, Section, SlotError, Table, Units};

/// An edit of one table.
#[derive(Debug, Clone, PartialEq)]
pub enum Op<T> {
    /// New rows under new ids.
    Add(Vec<T>),
    /// Rows back under the ids they had (the inverse of `Remove`).
    Restore(Vec<(Id<T>, T)>),
    Remove(Vec<Id<T>>),
    /// Replace rows.
    Set(Vec<(Id<T>, T)>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Nodes(Op<Node>),
    Frames(Op<Frame>),
    Areas(Op<Area>),
    Materials(Op<Material>),
    Sections(Op<Section>),
    Grid(Grid),
    Name(String),
    Units(Units),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("no {kind} {index}")]
    Missing { kind: &'static str, index: u32 },
    #[error("{kind} {index} is taken")]
    Occupied { kind: &'static str, index: u32 },
    #[error("{kind} {index} is listed twice")]
    Duplicate { kind: &'static str, index: u32 },
    #[error("{kind} {index} is still used by {by}")]
    InUse { kind: &'static str, index: u32, by: &'static str },
    #[error("too many {0}")]
    Full(&'static str),
    #[error("{0}")]
    Invalid(String),
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ModelError> {
    Err(ModelError::Invalid(message.into()))
}

/// What the generic table edit needs to know about each kind of row.
trait Kind: Default + Clone + Sized {
    const NAME: &'static str;
    fn table(m: &Model) -> &Table<Self>;
    fn table_mut(m: &mut Model) -> &mut Table<Self>;
    fn check(m: &Model, row: &Self) -> Result<(), ModelError>;
    /// Refuse removing rows something else still refers to.
    fn check_remove(m: &Model, ids: &[Id<Self>]) -> Result<(), ModelError>;
    fn added(_m: &mut Model, _id: Id<Self>, _row: &Self) {}
    fn removed(_m: &mut Model, _id: Id<Self>, _row: &Self) {}
}

pub(crate) fn check_node(n: &Node) -> Result<(), ModelError> {
    if !(n.position.is_finite() && n.position.abs().max_element() <= MAX_COORDINATE) {
        return invalid(format!("node position {:?} is outside ±{MAX_COORDINATE} m", n.position.to_array()));
    }
    Ok(())
}

fn position(m: &Model, id: Id<Node>) -> Result<DVec3, ModelError> {
    m.nodes.get(id).map(|n| n.position).ok_or(ModelError::Missing { kind: Node::NAME, index: id.index() })
}

fn section_of(m: &Model, id: Id<Section>, shell: bool) -> Result<(), ModelError> {
    let s = m.sections.get(id).ok_or(ModelError::Missing { kind: Section::NAME, index: id.index() })?;
    match (s.shape.is_shell(), shell) {
        (true, false) => invalid(format!("section `{}` is for slabs and walls, not frames", s.name)),
        (false, true) => invalid(format!("section `{}` is for frames, not slabs and walls", s.name)),
        _ => Ok(()),
    }
}

pub(crate) fn check_frame(m: &Model, f: &Frame) -> Result<(), ModelError> {
    let (a, b) = (position(m, f.i)?, position(m, f.j)?);
    if a.distance(b) <= MERGE_TOLERANCE {
        return invalid("a frame's two ends must be different points");
    }
    if !f.angle.is_finite() {
        return invalid("frame angle must be finite");
    }
    section_of(m, f.section, false)
}

pub(crate) fn check_area(m: &Model, a: &Area) -> Result<(), ModelError> {
    if !AREA_CORNERS.contains(&a.corners.len()) {
        return invalid(format!("an area needs {} to {} corners", AREA_CORNERS.start(), AREA_CORNERS.end()));
    }
    for (k, c) in a.corners.iter().enumerate() {
        position(m, *c)?;
        // At most 64 corners: comparing pairs beats hashing.
        if a.corners.iter().skip(k + 1).any(|d| d == c) {
            return invalid("an area's corners must be different nodes");
        }
    }
    section_of(m, a.section, true)
}

pub(crate) fn check_material(mat: &Material) -> Result<(), ModelError> {
    if mat.name.trim().is_empty() {
        return invalid("a material needs a name");
    }
    if !(mat.elastic_modulus.is_finite() && mat.elastic_modulus > 0.0) {
        return invalid("elastic modulus must be positive");
    }
    if !(mat.poisson_ratio > -1.0 && mat.poisson_ratio < 0.5) {
        return invalid("Poisson's ratio must be between -1 and 0.5");
    }
    if !(mat.density.is_finite() && mat.density >= 0.0) {
        return invalid("density must be zero or more");
    }
    Ok(())
}

pub(crate) fn check_section(m: &Model, s: &Section) -> Result<(), ModelError> {
    if s.name.trim().is_empty() {
        return invalid("a section needs a name");
    }
    if !m.materials.contains(s.material) {
        return Err(ModelError::Missing { kind: Material::NAME, index: s.material.index() });
    }
    if !s.shape.is_valid() {
        return invalid(format!("section `{}` has a dimension that is not positive", s.name));
    }
    Ok(())
}

impl Kind for Node {
    const NAME: &'static str = "node";
    fn table(m: &Model) -> &Table<Self> {
        &m.nodes
    }
    fn table_mut(m: &mut Model) -> &mut Table<Self> {
        &mut m.nodes
    }
    fn check(_m: &Model, row: &Self) -> Result<(), ModelError> {
        check_node(row)
    }
    fn check_remove(m: &Model, ids: &[Id<Self>]) -> Result<(), ModelError> {
        match ids.iter().find(|id| m.node_uses(**id) > 0) {
            Some(id) => Err(ModelError::InUse { kind: Self::NAME, index: id.index(), by: "frames or areas" }),
            None => Ok(()),
        }
    }
    fn added(m: &mut Model, id: Id<Self>, row: &Self) {
        m.index.insert(id, row.position);
        let i = id.index() as usize;
        if i >= m.node_uses.len() {
            m.node_uses.resize(i + 1, 0);
        }
    }
    fn removed(m: &mut Model, id: Id<Self>, row: &Self) {
        m.index.remove(id, row.position);
    }
}

impl Kind for Frame {
    const NAME: &'static str = "frame";
    fn table(m: &Model) -> &Table<Self> {
        &m.frames
    }
    fn table_mut(m: &mut Model) -> &mut Table<Self> {
        &mut m.frames
    }
    fn check(m: &Model, row: &Self) -> Result<(), ModelError> {
        check_frame(m, row)
    }
    fn check_remove(_m: &Model, _ids: &[Id<Self>]) -> Result<(), ModelError> {
        Ok(())
    }
    fn added(m: &mut Model, _id: Id<Self>, row: &Self) {
        m.use_nodes([row.i, row.j], 1);
    }
    fn removed(m: &mut Model, _id: Id<Self>, row: &Self) {
        m.use_nodes([row.i, row.j], -1);
    }
}

impl Kind for Area {
    const NAME: &'static str = "area";
    fn table(m: &Model) -> &Table<Self> {
        &m.areas
    }
    fn table_mut(m: &mut Model) -> &mut Table<Self> {
        &mut m.areas
    }
    fn check(m: &Model, row: &Self) -> Result<(), ModelError> {
        check_area(m, row)
    }
    fn check_remove(_m: &Model, _ids: &[Id<Self>]) -> Result<(), ModelError> {
        Ok(())
    }
    fn added(m: &mut Model, _id: Id<Self>, row: &Self) {
        m.use_nodes(row.corners.iter().copied(), 1);
    }
    fn removed(m: &mut Model, _id: Id<Self>, row: &Self) {
        m.use_nodes(row.corners.iter().copied(), -1);
    }
}

impl Kind for Material {
    const NAME: &'static str = "material";
    fn table(m: &Model) -> &Table<Self> {
        &m.materials
    }
    fn table_mut(m: &mut Model) -> &mut Table<Self> {
        &mut m.materials
    }
    fn check(_m: &Model, row: &Self) -> Result<(), ModelError> {
        check_material(row)
    }
    fn check_remove(m: &Model, ids: &[Id<Self>]) -> Result<(), ModelError> {
        let ids: HashSet<_> = ids.iter().collect();
        match m.sections.iter().find(|(_, s)| ids.contains(&s.material)) {
            Some((_, s)) => Err(ModelError::InUse { kind: Self::NAME, index: s.material.index(), by: "sections" }),
            None => Ok(()),
        }
    }
}

impl Kind for Section {
    const NAME: &'static str = "section";
    fn table(m: &Model) -> &Table<Self> {
        &m.sections
    }
    fn table_mut(m: &mut Model) -> &mut Table<Self> {
        &mut m.sections
    }
    fn check(m: &Model, row: &Self) -> Result<(), ModelError> {
        check_section(m, row)
    }
    fn check_remove(m: &Model, ids: &[Id<Self>]) -> Result<(), ModelError> {
        let ids: HashSet<_> = ids.iter().collect();
        if let Some((_, f)) = m.frames.iter().find(|(_, f)| ids.contains(&f.section)) {
            return Err(ModelError::InUse { kind: Self::NAME, index: f.section.index(), by: "frames" });
        }
        match m.areas.iter().find(|(_, a)| ids.contains(&a.section)) {
            Some((_, a)) => Err(ModelError::InUse { kind: Self::NAME, index: a.section.index(), by: "areas" }),
            None => Ok(()),
        }
    }
}

/// Each id at most once, and every id live (`live`) or restorable (`!live`). Costs
/// O(k log k) in the ids given, not in the size of the table.
fn check_ids<T: Kind>(m: &Model, ids: impl Iterator<Item = Id<T>>, live: bool) -> Result<(), ModelError> {
    let table = T::table(m);
    let mut indices = Vec::with_capacity(ids.size_hint().0);
    for id in ids {
        let ok = if live { table.contains(id) } else { table.can_restore(id) };
        if !ok {
            let index = id.index();
            return Err(if live { ModelError::Missing { kind: T::NAME, index } } else { ModelError::Occupied { kind: T::NAME, index } });
        }
        indices.push(id.index());
    }
    indices.sort_unstable();
    match indices.windows(2).find(|w| w.first() == w.get(1)) {
        Some(w) => Err(ModelError::Duplicate { kind: T::NAME, index: w.first().copied().unwrap_or_default() }),
        None => Ok(()),
    }
}

fn slot_error<T: Kind>(e: SlotError) -> ModelError {
    match e {
        SlotError::Missing(index) => ModelError::Missing { kind: T::NAME, index },
        SlotError::Occupied(index) => ModelError::Occupied { kind: T::NAME, index },
        SlotError::Full => ModelError::Full(T::NAME),
    }
}

fn apply_op<T: Kind>(m: &mut Model, op: Op<T>) -> Result<Op<T>, ModelError> {
    match op {
        Op::Add(rows) => {
            for row in &rows {
                T::check(m, row)?;
            }
            T::table_mut(m).reserve(rows.len());
            let mut ids = Vec::with_capacity(rows.len());
            for row in rows {
                let id = T::table_mut(m).insert(row.clone()).map_err(slot_error::<T>)?;
                T::added(m, id, &row);
                ids.push(id);
            }
            Ok(Op::Remove(ids))
        }
        Op::Restore(rows) => {
            check_ids(m, rows.iter().map(|(id, _)| *id), false)?;
            for (_, row) in &rows {
                T::check(m, row)?;
            }
            let ids = rows.iter().map(|(id, _)| *id).collect();
            for (id, row) in rows {
                T::table_mut(m).restore(id, row.clone()).map_err(slot_error::<T>)?;
                T::added(m, id, &row);
            }
            Ok(Op::Remove(ids))
        }
        Op::Remove(ids) => {
            check_ids(m, ids.iter().copied(), true)?;
            T::check_remove(m, &ids)?;
            let mut removed = Vec::with_capacity(ids.len());
            // Removed last to first so a restore (in reverse) refills the free list in order.
            for id in ids.into_iter().rev() {
                let row = T::table_mut(m).remove(id).map_err(slot_error::<T>)?;
                T::removed(m, id, &row);
                removed.push((id, row));
            }
            removed.reverse();
            Ok(Op::Restore(removed))
        }
        Op::Set(rows) => {
            check_ids(m, rows.iter().map(|(id, _)| *id), true)?;
            for (_, row) in &rows {
                T::check(m, row)?;
            }
            let mut old = Vec::with_capacity(rows.len());
            for (id, row) in rows {
                let Some(slot) = T::table_mut(m).get_mut(id) else {
                    return Err(ModelError::Missing { kind: T::NAME, index: id.index() });
                };
                let previous = std::mem::replace(slot, row.clone());
                T::removed(m, id, &previous);
                T::added(m, id, &row);
                old.push((id, previous));
            }
            Ok(Op::Set(old))
        }
    }
}

impl Model {
    /// Apply a change whole, or not at all, and return the change that undoes it.
    pub fn apply(&mut self, change: Change) -> Result<Change, ModelError> {
        Ok(match change {
            Change::Nodes(op) => Change::Nodes(apply_op(self, op)?),
            Change::Frames(op) => Change::Frames(apply_op(self, op)?),
            Change::Areas(op) => Change::Areas(apply_op(self, op)?),
            Change::Materials(op) => Change::Materials(apply_op(self, op)?),
            Change::Sections(op) => Change::Sections(apply_op(self, op)?),
            Change::Grid(grid) => Change::Grid(self.set_grid(grid)?),
            Change::Name(name) => Change::Name(std::mem::replace(&mut self.name, name)),
            Change::Units(units) => Change::Units(std::mem::replace(&mut self.display_units, units)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameRole, NodeId, SectionId};

    fn node(x: f64, y: f64, z: f64) -> Node {
        Node { position: DVec3::new(x, y, z), ..Node::default() }
    }

    fn section(m: &Model, name: &str) -> SectionId {
        m.sections().iter().find(|(_, s)| s.name.starts_with(name)).map(|(id, _)| id).unwrap()
    }

    fn add_nodes(m: &mut Model, rows: Vec<Node>) -> Vec<NodeId> {
        match m.apply(Change::Nodes(Op::Add(rows))).unwrap() {
            Change::Nodes(Op::Remove(ids)) => ids,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn undo_and_redo_restore_the_same_model() {
        let mut m = Model::new("t");
        let ids = add_nodes(&mut m, vec![node(0.0, 0.0, 0.0), node(0.0, 0.0, 3.0)]);
        let before = m.clone();
        let frame = Frame { i: ids[0], j: ids[1], section: section(&m, "Column"), role: FrameRole::Column, angle: 0.0 };
        let undo = m.apply(Change::Frames(Op::Add(vec![frame]))).unwrap();
        let after = m.clone();
        assert_eq!(m.node_uses(ids[0]), 1);
        let redo = m.apply(undo).unwrap();
        assert_eq!(m, before);
        assert_eq!(m.node_uses(ids[0]), 0);
        m.apply(redo).unwrap();
        assert_eq!(m, after);
    }

    #[test]
    fn used_nodes_and_sections_cannot_be_removed() {
        let mut m = Model::new("t");
        let ids = add_nodes(&mut m, vec![node(0.0, 0.0, 0.0), node(6.0, 0.0, 0.0)]);
        let beam = section(&m, "Beam");
        m.apply(Change::Frames(Op::Add(vec![Frame { i: ids[0], j: ids[1], section: beam, ..Frame::default() }]))).unwrap();
        assert!(matches!(m.apply(Change::Nodes(Op::Remove(vec![ids[0]]))), Err(ModelError::InUse { .. })));
        assert!(matches!(m.apply(Change::Sections(Op::Remove(vec![beam]))), Err(ModelError::InUse { .. })));
    }

    #[test]
    fn a_bad_row_leaves_the_model_untouched() {
        let mut m = Model::new("t");
        let before = m.clone();
        let rows = vec![node(0.0, 0.0, 0.0), node(f64::NAN, 0.0, 0.0)];
        assert!(m.apply(Change::Nodes(Op::Add(rows))).is_err());
        assert_eq!(m, before);
        let ids = add_nodes(&mut m, vec![node(0.0, 0.0, 0.0)]);
        assert!(matches!(m.apply(Change::Nodes(Op::Remove(vec![ids[0], ids[0]]))), Err(ModelError::Duplicate { .. })));
        assert_eq!(m.nodes().len(), 1);
    }

    #[test]
    fn frames_and_areas_check_their_references() {
        let mut m = Model::new("t");
        let ids = add_nodes(&mut m, vec![node(0.0, 0.0, 0.0), node(1.0, 0.0, 0.0), node(1.0, 1.0, 0.0)]);
        let (beam, slab) = (section(&m, "Beam"), section(&m, "Slab"));
        let zero_length = Frame { i: ids[0], j: ids[0], section: beam, ..Frame::default() };
        assert!(m.apply(Change::Frames(Op::Add(vec![zero_length]))).is_err());
        let slab_on_frame = Frame { i: ids[0], j: ids[1], section: slab, ..Frame::default() };
        assert!(m.apply(Change::Frames(Op::Add(vec![slab_on_frame]))).is_err());
        let two_corners = Area { corners: ids[..2].iter().copied().collect(), section: slab, ..Area::default() };
        assert!(m.apply(Change::Areas(Op::Add(vec![two_corners]))).is_err());
        let triangle = Area { corners: ids.iter().copied().collect(), section: slab, ..Area::default() };
        m.apply(Change::Areas(Op::Add(vec![triangle]))).unwrap();
        assert_eq!(m.node_uses(ids[2]), 1);
    }

    #[test]
    fn moving_a_node_moves_it_in_the_index() {
        let mut m = Model::new("t");
        let ids = add_nodes(&mut m, vec![node(0.0, 0.0, 0.0)]);
        assert_eq!(m.node_at(DVec3::new(0.0, 0.0, 0.0005)), Some(ids[0]));
        m.apply(Change::Nodes(Op::Set(vec![(ids[0], node(5.0, 0.0, 0.0))]))).unwrap();
        assert_eq!(m.node_at(DVec3::ZERO), None);
        assert_eq!(m.node_at(DVec3::new(5.0, 0.0, 0.0)), Some(ids[0]));
    }
}
