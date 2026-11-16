//! Packing the model into vertex and instance bytes.

use std::collections::HashSet;

use structurecraft_geom::DVec3;
use structurecraft_model::{AreaId, AreaRole, FrameId, FrameLoadShape, FrameRole, Model, NodeId, PatternId, Restraint, local_axes};

/// An sRGB colour with straight alpha, 0–255.
pub type Rgba = [u8; 4];

/// Every colour the scene uses; the UI passes its theme tokens in.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub node: Rgba,
    pub support: Rgba,
    pub beam: Rgba,
    pub column: Rgba,
    pub brace: Rgba,
    pub slab: Rgba,
    pub wall: Rgba,
    pub selected: Rgba,
    pub selected_area: Rgba,
    pub load: Rgba,
}

/// What is selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub nodes: HashSet<NodeId>,
    pub frames: HashSet<FrameId>,
    pub areas: HashSet<AreaId>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.frames.is_empty() && self.areas.is_empty()
    }

    pub fn len(&self) -> usize {
        self.nodes.len() + self.frames.len() + self.areas.len()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.frames.clear();
        self.areas.clear();
    }

    /// Drop ids that no longer name anything (after undo, delete, open).
    pub fn retain_existing(&mut self, model: &Model) {
        self.nodes.retain(|id| model.nodes().contains(*id));
        self.frames.retain(|id| model.frames().contains(*id));
        self.areas.retain(|id| model.areas().contains(*id));
    }
}

/// Bytes per instance or vertex.
pub(crate) const POINT_STRIDE: u64 = 20; // position f32x3, colour u8x4, size f32
pub(crate) const LINE_STRIDE: u64 = 32; // a f32x3, b f32x3, colour u8x4, width f32
pub(crate) const AREA_STRIDE: u64 = 16; // position f32x3, colour u8x4

/// Packed buffers ready to upload, and the origin their positions are relative to.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    /// Changes whenever the content does, so the renderer uploads only then.
    pub version: u64,
    pub origin: DVec3,
    pub points: Vec<u8>,
    pub lines: Vec<u8>,
    pub triangles: Vec<u8>,
}

impl Scene {
    pub fn point_count(&self) -> u32 {
        (self.points.len() as u64 / POINT_STRIDE) as u32
    }

    pub fn line_count(&self) -> u32 {
        (self.lines.len() as u64 / LINE_STRIDE) as u32
    }

    pub fn vertex_count(&self) -> u32 {
        (self.triangles.len() as u64 / AREA_STRIDE) as u32
    }
}

/// Node square size and frame width, in physical pixels.
const NODE_SIZE: f32 = 5.0;
const SUPPORT_SIZE: f32 = 9.0;
const FRAME_WIDTH: f32 = 2.0;
const SELECTED_GROW: f32 = 3.0;
const LOAD_WIDTH: f32 = 1.5;

/// Above this many loads, each load is one line without a head (a distributed load one
/// instead of three arrows, a slab one instead of five), so a fully loaded large model packs
/// about 32 bytes a load.
const DENSE_LOADS: usize = 20_000;

pub struct SceneBuilder<'a> {
    pub model: &'a Model,
    pub palette: Palette,
    pub origin: DVec3,
}

fn put_vec(out: &mut Vec<u8>, p: DVec3, origin: DVec3) {
    let r = p - origin;
    for v in [r.x as f32, r.y as f32, r.z as f32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

impl SceneBuilder<'_> {
    fn point(&self, out: &mut Vec<u8>, p: DVec3, colour: Rgba, size: f32) {
        put_vec(out, p, self.origin);
        out.extend_from_slice(&colour);
        out.extend_from_slice(&size.to_le_bytes());
    }

    fn line(&self, out: &mut Vec<u8>, a: DVec3, b: DVec3, colour: Rgba, width: f32) {
        put_vec(out, a, self.origin);
        put_vec(out, b, self.origin);
        out.extend_from_slice(&colour);
        out.extend_from_slice(&width.to_le_bytes());
    }

    /// `scratch` is reused across calls so packing allocates nothing per area.
    fn polygon(&self, out: &mut Vec<u8>, scratch: &mut Vec<DVec3>, corners: impl Iterator<Item = DVec3>, colour: Rgba) {
        // A fan: exact for the convex outlines slabs and walls almost always are.
        scratch.clear();
        scratch.extend(corners);
        let corners = &*scratch;
        let Some(first) = corners.first().copied() else { return };
        for pair in corners.windows(2).skip(1) {
            if let [b, c] = pair {
                for p in [first, *b, *c] {
                    put_vec(out, p, self.origin);
                    out.extend_from_slice(&colour);
                }
            }
        }
    }

    fn node_colour(&self, r: Restraint) -> (Rgba, f32) {
        if r == Restraint::FREE { (self.palette.node, NODE_SIZE) } else { (self.palette.support, SUPPORT_SIZE) }
    }

    fn frame_colour(&self, role: FrameRole) -> Rgba {
        match role {
            FrameRole::Beam => self.palette.beam,
            FrameRole::Column => self.palette.column,
            FrameRole::Brace => self.palette.brace,
        }
    }

    fn position(&self, id: NodeId) -> Option<DVec3> {
        self.model.nodes().get(id).map(|n| n.position)
    }

    /// Everything in the model.
    pub fn whole(&self, version: u64) -> Scene {
        let m = self.model;
        let mut points = Vec::with_capacity(m.nodes().len() * POINT_STRIDE as usize);
        for (_, n) in m.nodes().iter() {
            let (colour, size) = self.node_colour(n.restraint);
            self.point(&mut points, n.position, colour, size);
        }
        let mut lines = Vec::with_capacity(m.frames().len() * LINE_STRIDE as usize);
        for (_, f) in m.frames().iter() {
            if let (Some(a), Some(b)) = (self.position(f.i), self.position(f.j)) {
                self.line(&mut lines, a, b, self.frame_colour(f.role), FRAME_WIDTH);
            }
        }
        let mut triangles = Vec::with_capacity(m.areas().len() * 6 * AREA_STRIDE as usize);
        let mut scratch = Vec::new();
        for (_, a) in m.areas().iter() {
            let colour = if a.role == AreaRole::Wall { self.palette.wall } else { self.palette.slab };
            self.polygon(&mut triangles, &mut scratch, a.corners.iter().filter_map(|c| self.position(*c)), colour);
        }
        Scene { version, origin: self.origin, points, lines, triangles }
    }

    /// An arrow whose head is at `tip`, pointing along `dir` (unit), `len` long; only the
    /// shaft when `dense`.
    fn arrow(&self, out: &mut Vec<u8>, tip: DVec3, dir: DVec3, len: f64, dense: bool) {
        let tail = tip - dir * len;
        self.line(out, tail, tip, self.palette.load, LOAD_WIDTH);
        if dense {
            return;
        }
        let side = dir.cross(if dir.z.abs() > 0.9 { DVec3::X } else { DVec3::Z }).normalize_or_zero();
        let head = (len * 0.25).min(len);
        for s in [1.0, -1.0] {
            self.line(out, tip, tip - dir * head + side * (s * head * 0.45), self.palette.load, LOAD_WIDTH);
        }
    }

    /// The loads of one pattern as arrows, scaled so the largest of each kind (node forces,
    /// distributed, point, pressure) is 0.4 of the average frame long.
    pub fn loads(&self, pattern: PatternId, version: u64) -> Scene {
        let m = self.model;
        let mut lines = Vec::new();
        let (mut length, mut frames) = (0.0, 0usize);
        for (_, f) in m.frames().iter() {
            if let (Some(a), Some(b)) = (self.position(f.i), self.position(f.j)) {
                length += a.distance(b);
                frames += 1;
            }
        }
        let reference = if frames > 0 { 0.4 * length / frames as f64 } else { (m.bounds().radius() * 0.1).max(0.5) };
        let node_loads: Vec<_> = m.node_loads().iter().map(|(_, l)| l).filter(|l| l.pattern == pattern).collect();
        let frame_loads: Vec<_> = m.frame_loads().iter().map(|(_, l)| l).filter(|l| l.pattern == pattern).collect();
        let area_loads: Vec<_> = m.area_loads().iter().map(|(_, l)| l).filter(|l| l.pattern == pattern).collect();
        let force = |v: [f64; 6]| DVec3::new(v[0], v[1], v[2]);
        let largest = |it: &mut dyn Iterator<Item = f64>| it.fold(0.0_f64, |a, v| a.max(v.abs()));
        let max_node = largest(&mut node_loads.iter().map(|l| force(l.values).length()));
        let max_spread =
            largest(&mut frame_loads.iter().filter(|l| l.shape == FrameLoadShape::Distributed).flat_map(|l| [l.value_start, l.value_end]));
        let max_point = largest(&mut frame_loads.iter().filter(|l| l.shape == FrameLoadShape::Point).map(|l| l.value_start));
        let max_pressure = largest(&mut area_loads.iter().map(|l| l.pressure));
        let count = node_loads.len() + frame_loads.len() + area_loads.len();
        let dense = count > DENSE_LOADS;
        let scale = |v: f64, max: f64| if max > 0.0 { reference * v / max } else { 0.0 };

        for l in node_loads {
            let f = force(l.values);
            if let (Some(p), Some(dir)) = (self.position(l.node), f.try_normalize()) {
                self.arrow(&mut lines, p, dir, scale(f.length(), max_node), dense);
            }
        }
        for l in frame_loads {
            let Some(frame) = m.frames().get(l.frame) else { continue };
            let (Some(a), Some(b)) = (self.position(frame.i), self.position(frame.j)) else { continue };
            let Some(axes) = local_axes(a, b, frame.angle) else { continue };
            let dir = l.direction.vector(&axes);
            let at = |t: f64| a + (b - a) * t;
            match l.shape {
                FrameLoadShape::Point => {
                    let len = scale(l.value_start, max_point);
                    self.arrow(&mut lines, at(l.start), dir * len.signum(), len.abs(), dense);
                }
                FrameLoadShape::Distributed => {
                    let steps: &[f64] = if dense { &[0.5] } else { &[0.0, 0.5, 1.0] };
                    let mut tails = Vec::with_capacity(steps.len());
                    for k in steps {
                        let t = l.start + (l.end - l.start) * k;
                        let len = scale(l.value_start + (l.value_end - l.value_start) * k, max_spread);
                        self.arrow(&mut lines, at(t), dir * len.signum(), len.abs(), dense);
                        tails.push(at(t) - dir * len);
                    }
                    for pair in tails.windows(2) {
                        if let [p, q] = pair {
                            self.line(&mut lines, *p, *q, self.palette.load, LOAD_WIDTH);
                        }
                    }
                }
            }
        }
        for l in area_loads {
            let Some(area) = m.areas().get(l.area) else { continue };
            let corners: Vec<DVec3> = area.corners.iter().filter_map(|c| self.position(*c)).collect();
            if corners.is_empty() {
                continue;
            }
            let centre = corners.iter().copied().sum::<DVec3>() / corners.len() as f64;
            let len = scale(l.pressure, max_pressure);
            let dir = DVec3::NEG_Z * len.signum();
            self.arrow(&mut lines, centre, dir, len.abs(), dense);
            if !dense {
                for c in corners.iter().take(4) {
                    self.arrow(&mut lines, centre + (*c - centre) * 0.5, dir, len.abs(), dense);
                }
            }
        }
        Scene { version, origin: self.origin, points: Vec::new(), lines, triangles: Vec::new() }
    }

    /// Only what is selected, drawn larger in the selection colour.
    pub fn selected(&self, selection: &Selection, version: u64) -> Scene {
        let (mut points, mut lines, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
        for id in &selection.nodes {
            if let Some(n) = self.model.nodes().get(*id) {
                let (_, size) = self.node_colour(n.restraint);
                self.point(&mut points, n.position, self.palette.selected, size + SELECTED_GROW);
            }
        }
        for id in &selection.frames {
            if let Some(f) = self.model.frames().get(*id)
                && let (Some(a), Some(b)) = (self.position(f.i), self.position(f.j))
            {
                self.line(&mut lines, a, b, self.palette.selected, FRAME_WIDTH + SELECTED_GROW);
            }
        }
        let mut scratch = Vec::new();
        for id in &selection.areas {
            if let Some(a) = self.model.areas().get(*id) {
                self.polygon(&mut triangles, &mut scratch, a.corners.iter().filter_map(|c| self.position(*c)), self.palette.selected_area);
            }
        }
        Scene { version, origin: self.origin, points, lines, triangles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use structurecraft_model::{Area, Change, Frame, Node, Op};

    const P: Palette = Palette {
        node: [1, 1, 1, 255],
        support: [2, 2, 2, 255],
        beam: [3, 3, 3, 255],
        column: [4, 4, 4, 255],
        brace: [5, 5, 5, 255],
        slab: [6, 6, 6, 80],
        wall: [7, 7, 7, 80],
        selected: [8, 8, 8, 255],
        selected_area: [9, 9, 9, 120],
        load: [10, 10, 10, 255],
    };

    #[test]
    fn packs_every_element() {
        let mut m = Model::new("t");
        let pts = [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)];
        let Ok(Change::Nodes(Op::Remove(ids))) =
            m.apply(Change::Nodes(Op::Add(pts.iter().map(|(x, y)| Node { position: DVec3::new(*x, *y, 3.0), ..Node::default() }).collect())))
        else {
            panic!()
        };
        let beam = m.sections().ids().next().unwrap();
        let slab = m.sections().iter().find(|(_, s)| s.shape.is_shell()).unwrap().0;
        m.apply(Change::Frames(Op::Add(vec![Frame { i: ids[0], j: ids[1], section: beam, ..Frame::default() }]))).unwrap();
        m.apply(Change::Areas(Op::Add(vec![Area { corners: ids.iter().copied().collect(), section: slab, ..Area::default() }]))).unwrap();
        let s = SceneBuilder { model: &m, palette: P, origin: DVec3::new(0.0, 0.0, 3.0) }.whole(1);
        assert_eq!((s.point_count(), s.line_count(), s.vertex_count()), (4, 1, 6));
        // The first node sits at the origin.
        assert_eq!(&s.points[..12], &[0u8; 12]);
    }

    #[test]
    fn loads_become_arrows() {
        let mut m = Model::new("t");
        let Ok(Change::Nodes(Op::Remove(ids))) = m.apply(Change::Nodes(Op::Add(vec![
            Node { position: DVec3::ZERO, ..Node::default() },
            Node { position: DVec3::new(6.0, 0.0, 0.0), ..Node::default() },
        ]))) else {
            panic!()
        };
        let beam = m.sections().ids().next().unwrap();
        let Ok(Change::Frames(Op::Remove(f))) =
            m.apply(Change::Frames(Op::Add(vec![Frame { i: ids[0], j: ids[1], section: beam, ..Frame::default() }])))
        else {
            panic!()
        };
        let (dead, live) = {
            let mut p = m.patterns().ids();
            (p.next().unwrap(), p.next().unwrap())
        };
        let udl = structurecraft_model::FrameLoad { frame: f[0], pattern: dead, end: 1.0, value_start: 1e4, value_end: 1e4, ..Default::default() };
        m.apply(Change::FrameLoads(Op::Add(vec![udl]))).unwrap();
        m.apply(Change::NodeLoads(Op::Add(vec![structurecraft_model::NodeLoad {
            node: ids[1],
            pattern: live,
            values: [1e3, 0.0, 0.0, 0.0, 0.0, 0.0],
        }])))
        .unwrap();
        let b = SceneBuilder { model: &m, palette: P, origin: DVec3::ZERO };
        // Three arrows of three lines, and two lines joining their tails.
        assert_eq!(b.loads(dead, 1).line_count(), 11);
        assert_eq!(b.loads(live, 1).line_count(), 3);
    }
}
