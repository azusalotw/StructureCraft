//! Turning the model's loads into what the solver needs: every frame's loads grouped by frame,
//! and slab pressures handed to the frames along the slab's edges (or, where an edge has no
//! frame, to its corner nodes).

use rustc_hash::FxHashMap;
use structurecraft_geom::DVec3;
use structurecraft_model::{AreaRole, FrameLoad, Model, SlabSpan};

/// A load along a frame, from a slab: acting down, varying linearly from `w0` at `x0` to `w1`
/// at `x1` (N/m; metres from end i).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlabStrip {
    pub pattern: usize,
    pub x0: f64,
    pub x1: f64,
    pub w0: f64,
    pub w1: f64,
}

/// A force on a node from a slab edge with no frame under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerForce {
    pub node: u32,
    pub pattern: usize,
    pub force: DVec3,
}

/// Rows grouped by frame slot: the rows of frame `s` are `rows[start[s]..start[s + 1]]`.
#[derive(Debug, Default)]
pub struct ByFrame<T> {
    start: Vec<u32>,
    rows: Vec<T>,
}

impl<T: Copy> ByFrame<T> {
    /// From `(frame slot, row)` pairs, by counting sort: no per-frame allocation.
    pub fn new(slots: usize, items: &[(u32, T)]) -> Self {
        let mut start = vec![0u32; slots + 1];
        for (s, _) in items {
            if let Some(c) = start.get_mut(*s as usize + 1) {
                *c += 1;
            }
        }
        for i in 0..slots {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut rows: Vec<Option<T>> = vec![None; items.len()];
        for (s, row) in items {
            if let Some(at) = fill.get_mut(*s as usize) {
                if let Some(slot) = rows.get_mut(*at as usize) {
                    *slot = Some(*row);
                }
                *at += 1;
            }
        }
        Self { start, rows: rows.into_iter().flatten().collect() }
    }

    pub fn of(&self, slot: u32) -> &[T] {
        let s = slot as usize;
        match (self.start.get(s), self.start.get(s + 1)) {
            (Some(&a), Some(&b)) => self.rows.get(a as usize..b as usize).unwrap_or(&[]),
            _ => &[],
        }
    }
}

/// What the model's loads become, for patterns numbered by `pattern_column`.
pub struct Gathered {
    pub frame_loads: ByFrame<(usize, FrameLoad)>,
    pub strips: ByFrame<SlabStrip>,
    pub corner_forces: Vec<CornerForce>,
}

pub fn gather(model: &Model, pattern_column: &dyn Fn(structurecraft_model::PatternId) -> Option<usize>) -> Gathered {
    let slots = model.frames().capacity();
    let items: Vec<(u32, (usize, FrameLoad))> =
        model.frame_loads().iter().filter_map(|(_, l)| Some((l.frame.index(), (pattern_column(l.pattern)?, *l)))).collect();
    let frame_loads = ByFrame::new(slots, &items);
    drop(items);

    let mut strips = Vec::new();
    let mut corner_forces = Vec::new();
    if !model.area_loads().is_empty() {
        let edges = edge_frames(model);
        for (_, load) in model.area_loads().iter() {
            let Some(pattern) = pattern_column(load.pattern) else { continue };
            let Some(area) = model.areas().get(load.area) else { continue };
            if area.role != AreaRole::Slab || load.pressure == 0.0 {
                continue;
            }
            let corners: Vec<(u32, DVec3)> = area.corners.iter().filter_map(|c| model.nodes().get(*c).map(|n| (c.index(), n.position))).collect();
            if corners.len() < 3 || corners.len() != area.corners.len() {
                continue;
            }
            for (k, profile) in distribute(&corners, load.pressure, load.span).into_iter().enumerate() {
                let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
                let length = a.1.distance(b.1);
                match edges.get(&(a.0.min(b.0), a.0.max(b.0))) {
                    Some(&(frame, i)) => {
                        // Measured from the frame's end i, which may be either corner.
                        let at = |s: f64| if i == a.0 { s } else { length - s };
                        for w in profile.windows(2) {
                            let ((s0, w0), (s1, w1)) = (w[0], w[1]);
                            if s1 > s0 && (w0 != 0.0 || w1 != 0.0) {
                                let (x0, x1, w0, w1) = if i == a.0 { (at(s0), at(s1), w0, w1) } else { (at(s1), at(s0), w1, w0) };
                                strips.push((frame, SlabStrip { pattern, x0, x1, w0, w1 }));
                            }
                        }
                    }
                    None => {
                        // No frame along this edge: its load goes to its two corners by statics.
                        let (mut total, mut moment) = (0.0, 0.0);
                        for w in profile.windows(2) {
                            let ((s0, w0), (s1, w1)) = (w[0], w[1]);
                            let part = (w0 + w1) / 2.0 * (s1 - s0);
                            total += part;
                            if part != 0.0 {
                                moment += part * (s0 + (s1 - s0) * (w0 + 2.0 * w1) / (3.0 * (w0 + w1)));
                            }
                        }
                        if total != 0.0 && length > 0.0 {
                            let at_b = moment / length;
                            corner_forces.push(CornerForce { node: a.0, pattern, force: DVec3::new(0.0, 0.0, -(total - at_b)) });
                            corner_forces.push(CornerForce { node: b.0, pattern, force: DVec3::new(0.0, 0.0, -at_b) });
                        }
                    }
                }
            }
        }
    }
    Gathered { frame_loads, strips: ByFrame::new(slots, &strips), corner_forces }
}

/// The frame joining each pair of nodes (smaller slot first), with the slot of its end i.
fn edge_frames(model: &Model) -> FxHashMap<(u32, u32), (u32, u32)> {
    let mut map = FxHashMap::default();
    map.reserve(model.frames().len());
    for (id, f) in model.frames().iter() {
        let (a, b) = (f.i.index(), f.j.index());
        map.entry((a.min(b), a.max(b))).or_insert((id.index(), a));
    }
    map
}

/// How a slab's pressure `p` (Pa) loads each edge: for edge k (corner k to corner k + 1), a
/// piecewise-linear profile of (distance from corner k, N/m). The edges together carry exactly
/// `p` times the slab's area.
///
/// A four-sided slab follows the usual tributary rule: one-way slabs load their two long edges
/// evenly; two-way slabs split along 45° lines, so short edges get triangles and long edges
/// trapezoids. Other slabs give each edge the part of the slab between it and the centroid,
/// spread evenly along the edge.
pub fn distribute(corners: &[(u32, DVec3)], p: f64, span: SlabSpan) -> Vec<Vec<(f64, f64)>> {
    let n = corners.len();
    let point = |k: usize| corners[k % n].1;
    let lengths: Vec<f64> = (0..n).map(|k| point(k).distance(point(k + 1))).collect();
    let centroid = corners.iter().map(|c| c.1).sum::<DVec3>() / n.max(1) as f64;
    let area = (0..n).map(|k| (point(k) - centroid).cross(point(k + 1) - centroid)).sum::<DVec3>().length() / 2.0;
    let uniform = |k: usize, w: f64| vec![(0.0, w), (lengths[k], w)];
    let mut profiles: Vec<Vec<(f64, f64)>> = if n == 4 {
        let (a, b) = ((lengths[0] + lengths[2]) / 2.0, (lengths[1] + lengths[3]) / 2.0);
        let (short, long) = (a.min(b), a.max(b));
        let one_way = match span {
            SlabSpan::OneWay => true,
            SlabSpan::TwoWay => false,
            SlabSpan::Auto => long > 2.0 * short,
        };
        let long_edges = if a >= b { [0, 2] } else { [1, 3] };
        (0..4)
            .map(|k| {
                let w = p * short / 2.0;
                if one_way {
                    if long_edges.contains(&k) { uniform(k, w) } else { uniform(k, 0.0) }
                } else {
                    let l = lengths[k];
                    let ramp = (short / 2.0).min(l / 2.0);
                    vec![(0.0, 0.0), (ramp, w), (l - ramp, w), (l, 0.0)]
                }
            })
            .collect()
    } else {
        (0..n)
            .map(|k| {
                let part = (point(k) - centroid).cross(point(k + 1) - centroid).length() / 2.0;
                uniform(k, if lengths[k] > 0.0 { p * part / lengths[k] } else { 0.0 })
            })
            .collect()
    };
    // Scale so the edges carry exactly the pressure times the true area (slabs that are not
    // rectangles).
    let carried: f64 = profiles.iter().flat_map(|pr| pr.windows(2).map(|w| (w[0].1 + w[1].1) / 2.0 * (w[1].0 - w[0].0))).sum();
    if carried != 0.0 {
        let scale = p * area / carried;
        for pr in &mut profiles {
            for v in pr.iter_mut() {
                v.1 *= scale;
            }
        }
    }
    profiles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(a: f64, b: f64) -> Vec<(u32, DVec3)> {
        [(0.0, 0.0), (a, 0.0), (a, b), (0.0, b)].iter().enumerate().map(|(k, (x, y))| (k as u32, DVec3::new(*x, *y, 3.0))).collect()
    }

    fn total(profile: &[(f64, f64)]) -> f64 {
        profile.windows(2).map(|w| (w[0].1 + w[1].1) / 2.0 * (w[1].0 - w[0].0)).sum()
    }

    #[test]
    fn two_way_slab_gives_triangles_and_trapezoids() {
        let pr = distribute(&rect(8.0, 6.0), 1000.0, SlabSpan::Auto);
        // Short edges (6 m): triangles of peak 3 kN/m, so 9 kN each.
        assert!((total(&pr[1]) - 9000.0).abs() < 1e-9 && (total(&pr[3]) - 9000.0).abs() < 1e-9);
        // Long edges (8 m): trapezoids 3 m up, 2 m flat, so 15 kN each; 48 kN in all.
        assert!((total(&pr[0]) - 15000.0).abs() < 1e-9);
        assert_eq!(pr[0], vec![(0.0, 0.0), (3.0, 3000.0), (5.0, 3000.0), (8.0, 0.0)]);
    }

    #[test]
    fn one_way_slab_loads_its_long_edges() {
        let pr = distribute(&rect(3.0, 9.0), 2000.0, SlabSpan::Auto);
        assert_eq!(total(&pr[0]), 0.0);
        assert!((total(&pr[1]) - 27000.0).abs() < 1e-9 && (total(&pr[3]) - 27000.0).abs() < 1e-9);
        assert_eq!(pr[1], vec![(0.0, 3000.0), (9.0, 3000.0)]);
    }

    #[test]
    fn a_triangle_carries_its_whole_load() {
        let tri: Vec<(u32, DVec3)> = vec![(0, DVec3::ZERO), (1, DVec3::new(4.0, 0.0, 0.0)), (2, DVec3::new(0.0, 3.0, 0.0))];
        let pr = distribute(&tri, 500.0, SlabSpan::Auto);
        let sum: f64 = pr.iter().map(|p| total(p)).sum();
        assert!((sum - 500.0 * 6.0).abs() < 1e-9);
    }

    #[test]
    fn rows_group_by_frame() {
        let g = ByFrame::new(4, &[(2, 'a'), (0, 'b'), (2, 'c'), (9, 'x')]);
        assert_eq!(g.of(0), &['b']);
        assert!(g.of(1).is_empty());
        assert_eq!(g.of(2), &['a', 'c']);
        assert!(g.of(7).is_empty());
    }
}
