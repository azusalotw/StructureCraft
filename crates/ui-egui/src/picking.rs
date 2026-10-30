//! Finding what is under the pointer, in screen space.
//!
//! Node positions are projected once per camera, viewport and model revision and kept; a click,
//! a box or a snap then reads those screen points. Clicks and boxes also walk the frames and
//! areas once, which for millions of elements is a few milliseconds per click, never per frame.

use egui::{Pos2, Rect};
use structurecraft_geom::{DVec2, DVec3};
use structurecraft_gpu::Selection;
use structurecraft_model::{AreaId, FrameId, Model, NodeId};
use structurecraft_render::Camera;

/// Pointer distance that still hits, in points.
pub const PICK_RADIUS: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Node(NodeId),
    Frame(FrameId),
    Area(AreaId),
}

#[derive(Default)]
pub struct ScreenCache {
    key: Option<(u64, Camera, Rect)>,
    /// Screen position and depth (0 near, 1 far) of each node slot (`None`: free slot or
    /// behind the eye).
    screen: Vec<Option<(Pos2, f32)>>,
}

impl ScreenCache {
    pub fn update(&mut self, model: &Model, camera: &Camera, rect: Rect, revision: u64) {
        let key = Some((revision, *camera, rect));
        if self.key == key {
            return;
        }
        self.key = key;
        let viewport = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
        let view_proj = camera.view_projection(viewport);
        self.screen.clear();
        self.screen.resize(model.nodes().capacity(), None);
        for (id, n) in model.nodes().iter() {
            let clip = view_proj * n.position.extend(1.0);
            if clip.w > 0.0
                && let Some(slot) = self.screen.get_mut(id.index() as usize)
            {
                let ndc = clip.truncate() / clip.w;
                let p = rect.min + egui::vec2(((ndc.x + 1.0) * 0.5 * viewport.x) as f32, ((1.0 - ndc.y) * 0.5 * viewport.y) as f32);
                *slot = Some((p, ndc.z as f32));
            }
        }
    }

    pub fn at(&self, id: NodeId) -> Option<Pos2> {
        self.screen.get(id.index() as usize).copied().flatten().map(|(p, _)| p)
    }

    fn depth(&self, id: NodeId) -> f32 {
        self.screen.get(id.index() as usize).copied().flatten().map_or(1.0, |(_, z)| z)
    }

    /// The nearest node within `radius` of `p`; of nodes that overlap on screen (a plan view
    /// of many stories), the one nearest the eye.
    pub fn nearest_node(&self, model: &Model, p: Pos2, radius: f32) -> Option<NodeId> {
        let mut best: Option<(f32, f32, u32)> = None;
        for (i, s) in self.screen.iter().enumerate() {
            if let Some((s, z)) = s {
                let d = s.distance(p);
                if d <= radius && best.is_none_or(|(bd, bz, _)| closer((d, *z), (bd, bz))) {
                    best = Some((d, *z, i as u32));
                }
            }
        }
        best.and_then(|(_, _, i)| model.nodes().id_at(i))
    }
}

/// Nearer on screen by more than half a point wins; otherwise nearer the eye.
fn closer((d, z): (f32, f32), (bd, bz): (f32, f32)) -> bool {
    if (d - bd).abs() > 0.5 { d < bd } else { z < bz }
}

fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

fn inside(p: Pos2, polygon: &[Pos2]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for (i, a) in polygon.iter().enumerate() {
        if let Some(b) = polygon.get(j)
            && (a.y > p.y) != (b.y > p.y)
            && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// What is under `p`: a node first, then the nearest frame, then an area containing it.
pub fn pick(model: &Model, cache: &ScreenCache, p: Pos2) -> Option<Hit> {
    if let Some(n) = cache.nearest_node(model, p, PICK_RADIUS) {
        return Some(Hit::Node(n));
    }
    let mut best: Option<(f32, f32, FrameId)> = None;
    for (id, f) in model.frames().iter() {
        if let (Some(a), Some(b)) = (cache.at(f.i), cache.at(f.j)) {
            let d = segment_distance(p, a, b);
            let z = cache.depth(f.i).min(cache.depth(f.j));
            if d <= PICK_RADIUS && best.is_none_or(|(bd, bz, _)| closer((d, z), (bd, bz))) {
                best = Some((d, z, id));
            }
        }
    }
    if let Some((_, _, id)) = best {
        return Some(Hit::Frame(id));
    }
    let mut corners = Vec::new();
    model.areas().iter().find_map(|(id, a)| {
        corners.clear();
        corners.extend(a.corners.iter().filter_map(|c| cache.at(*c)));
        (corners.len() == a.corners.len() && inside(p, &corners)).then_some(Hit::Area(id))
    })
}

/// Everything wholly inside `area`.
pub fn in_box(model: &Model, cache: &ScreenCache, area: Rect) -> Selection {
    let hit = |id: NodeId| cache.at(id).is_some_and(|s| area.contains(s));
    Selection {
        nodes: model.nodes().ids().filter(|id| hit(*id)).collect(),
        frames: model.frames().iter().filter(|(_, f)| hit(f.i) && hit(f.j)).map(|(id, _)| id).collect(),
        areas: model.areas().iter().filter(|(_, a)| a.corners.iter().all(|c| hit(*c))).map(|(id, _)| id).collect(),
    }
}

/// Toggle (or add) a hit in a selection.
pub fn toggle(selection: &mut Selection, hit: Hit) {
    fn flip<T: std::hash::Hash + Eq>(set: &mut std::collections::HashSet<T>, v: T) {
        if !set.remove(&v) {
            set.insert(v);
        }
    }
    match hit {
        Hit::Node(id) => flip(&mut selection.nodes, id),
        Hit::Frame(id) => flip(&mut selection.frames, id),
        Hit::Area(id) => flip(&mut selection.areas, id),
    }
}

/// The world point a pixel snaps to on the working plane at `z`: a node, then a grid
/// intersection, both within the pick radius; otherwise the plane point rounded to 1 cm.
pub fn snap(model: &Model, cache: &ScreenCache, camera: &Camera, rect: Rect, p: Pos2, z: f64) -> Option<(DVec3, bool)> {
    if let Some(n) = cache.nearest_node(model, p, PICK_RADIUS * 1.5) {
        return model.nodes().get(n).map(|n| (n.position, true));
    }
    let viewport = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
    let to_screen = |w: DVec3| camera.project(w, viewport).map(|s| rect.min + egui::vec2(s.x as f32, s.y as f32));
    let grid = model.grid();
    let mut best: Option<(f32, DVec3)> = None;
    for x in &grid.x {
        for y in &grid.y {
            let w = DVec3::new(x.coordinate, y.coordinate, z);
            if let Some(s) = to_screen(w) {
                let d = s.distance(p);
                if d <= PICK_RADIUS * 1.5 && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, w));
                }
            }
        }
    }
    if let Some((_, w)) = best {
        return Some((w, true));
    }
    let local = p - rect.min;
    let w = camera.on_plane(DVec2::new(f64::from(local.x), f64::from(local.y)), viewport, z)?;
    Some((DVec3::new((w.x * 100.0).round() / 100.0, (w.y * 100.0).round() / 100.0, z), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_helpers() {
        let sq = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(10.0, 10.0), Pos2::new(0.0, 10.0)];
        assert!(inside(Pos2::new(5.0, 5.0), &sq));
        assert!(!inside(Pos2::new(15.0, 5.0), &sq));
        assert!(!inside(Pos2::new(5.0, 5.0), &[]));
        assert_eq!(segment_distance(Pos2::new(5.0, 3.0), Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)), 3.0);
        assert_eq!(segment_distance(Pos2::new(-4.0, 3.0), Pos2::new(0.0, 0.0), Pos2::new(0.0, 0.0)), 5.0);
    }
}
