//! Finding nodes by position, so drawing reuses the node already at a point instead of
//! stacking a duplicate there. A hash grid with cells the size of the merge tolerance: a lookup
//! reads the 27 cells around the point, whatever the model's size.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use structurecraft_geom::DVec3;

use crate::NodeId;

/// Points closer than this are the same node (metres).
pub const MERGE_TOLERANCE: f64 = 1e-3;

type Cell = (i64, i64, i64);

#[derive(Debug, Clone, Default)]
pub struct NodeIndex {
    cells: FxHashMap<Cell, SmallVec<[NodeId; 1]>>,
}

fn cell_of(p: DVec3) -> Option<Cell> {
    let c = (p / MERGE_TOLERANCE).floor();
    // Positions beyond ±9e15 tolerances (9e12 m) are not indexed.
    let in_range = |v: f64| v.abs() < 9e15;
    (c.is_finite() && in_range(c.x) && in_range(c.y) && in_range(c.z)).then_some((c.x as i64, c.y as i64, c.z as i64))
}

impl NodeIndex {
    pub fn with_capacity(n: usize) -> Self {
        Self { cells: FxHashMap::with_capacity_and_hasher(n, Default::default()) }
    }

    pub fn insert(&mut self, id: NodeId, p: DVec3) {
        if let Some(c) = cell_of(p) {
            self.cells.entry(c).or_default().push(id);
        }
    }

    pub fn remove(&mut self, id: NodeId, p: DVec3) {
        let Some(c) = cell_of(p) else { return };
        if let Some(ids) = self.cells.get_mut(&c) {
            ids.retain(|i| *i != id);
            if ids.is_empty() {
                self.cells.remove(&c);
            }
        }
    }

    /// The nearest node within [`MERGE_TOLERANCE`] of `p`, given every node's position.
    pub fn find(&self, p: DVec3, position: impl Fn(NodeId) -> Option<DVec3>) -> Option<NodeId> {
        let (cx, cy, cz) = cell_of(p)?;
        let mut best: Option<(f64, NodeId)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(ids) = self.cells.get(&(cx + dx, cy + dy, cz + dz)) else { continue };
                    for id in ids {
                        let Some(q) = position(*id) else { continue };
                        let d = q.distance(p);
                        if d <= MERGE_TOLERANCE && best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, *id));
                        }
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }
}
