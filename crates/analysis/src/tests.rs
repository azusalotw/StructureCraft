//! Validation against closed-form results from beam theory and statics only (no numbers from
//! other software).

use structurecraft_geom::{DQuat, DVec3};
use structurecraft_model::{
    Area, AreaLoad, AreaRole, Change, Frame, FrameId, FrameLoad, FrameLoadShape, LoadCase, LoadDirection, LoadPattern, Material, Model, Node, NodeId,
    NodeLoad, Op, PatternId, Releases, Restraint, Section, SectionShape, SlabSpan,
};

use super::*;

const E: f64 = 30e9;
const NU: f64 = 0.2;
const B: f64 = 0.2;
const H: f64 = 0.4;

fn i33() -> f64 {
    B * H.powi(3) / 12.0
}
fn i22() -> f64 {
    H * B.powi(3) / 12.0
}
fn area() -> f64 {
    B * H
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-300)
}

macro_rules! assert_close {
    ($a:expr, $b:expr) => {
        assert_close!($a, $b, 1e-9)
    };
    ($a:expr, $b:expr, $rel:expr) => {{
        let (a, b) = ($a, $b);
        assert!(close(a, b, $rel), "{} = {a:e}, expected {b:e}", stringify!($a));
    }};
}

/// A model with one material (density `rho`), one 0.2 × 0.4 m rectangular section, one pattern
/// and one case.
struct Build {
    m: Model,
    section: structurecraft_model::SectionId,
    pattern: PatternId,
}

impl Build {
    fn new(rho: f64, self_weight: f64) -> Self {
        let mut m = Model::empty();
        let mat = add(&mut m, Change::Materials(Op::Add(vec![Material { name: "M".into(), elastic_modulus: E, poisson_ratio: NU, density: rho }])));
        let section =
            add(&mut m, Change::Sections(Op::Add(vec![Section { name: "S".into(), material: mat, shape: SectionShape::Rectangle { b: B, h: H } }])));
        let pattern = add(&mut m, Change::Patterns(Op::Add(vec![LoadPattern { name: "P".into(), self_weight, ..Default::default() }])));
        add::<LoadCase>(&mut m, Change::Cases(Op::Add(vec![LoadCase { name: "C".into(), patterns: vec![(pattern, 1.0)] }])));
        Self { m, section, pattern }
    }

    fn node(&mut self, x: f64, y: f64, z: f64, r: Restraint) -> NodeId {
        add(&mut self.m, Change::Nodes(Op::Add(vec![Node { position: DVec3::new(x, y, z), restraint: r }])))
    }

    fn frame(&mut self, i: NodeId, j: NodeId, releases: Releases) -> FrameId {
        add(&mut self.m, Change::Frames(Op::Add(vec![Frame { i, j, section: self.section, releases, ..Frame::default() }])))
    }

    fn node_load(&mut self, node: NodeId, values: [f64; 6]) {
        add::<NodeLoad>(&mut self.m, Change::NodeLoads(Op::Add(vec![NodeLoad { node, pattern: self.pattern, values }])));
    }

    fn frame_load(&mut self, frame: FrameId, shape: FrameLoadShape, dir: LoadDirection, (start, end): (f64, f64), (va, vb): (f64, f64)) {
        let l = FrameLoad { frame, pattern: self.pattern, shape, direction: dir, start, end, value_start: va, value_end: vb };
        add::<FrameLoad>(&mut self.m, Change::FrameLoads(Op::Add(vec![l])));
    }

    fn uniform(&mut self, frame: FrameId, w: f64) {
        self.frame_load(frame, FrameLoadShape::Distributed, LoadDirection::Gravity, (0.0, 1.0), (w, w));
    }

    fn run(&self) -> (Analysis, CaseResult) {
        let a = analyze(&self.m).unwrap();
        let c = a.cases[0].clone();
        (a, c)
    }
}

/// Apply an add and return the one new id.
fn add<T>(m: &mut Model, change: Change) -> structurecraft_model::Id<T> {
    let inverse = m.apply(change).unwrap();
    let ids: Vec<u64> = match &inverse {
        Change::Nodes(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Frames(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Areas(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Materials(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Sections(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Patterns(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::Cases(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::NodeLoads(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::FrameLoads(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        Change::AreaLoads(Op::Remove(v)) => v.iter().map(|i| pack(i.index(), i.generation())).collect(),
        other => panic!("not an add: {other:?}"),
    };
    let id = ids[0];
    structurecraft_model::Id::new(id as u32, (id >> 32) as u32)
}

fn pack(index: u32, generation: u32) -> u64 {
    u64::from(generation) << 32 | u64::from(index)
}

const FREE: Restraint = Restraint::FREE;
const FIXED: Restraint = Restraint::FIXED;
/// Translations held, and rotation about X so the beam cannot spin.
const PIN: Restraint = Restraint(0b001_111);
/// Y and Z held: a roller along X.
const ROLLER: Restraint = Restraint(0b000_110);

#[test]
fn cantilever_with_a_tip_load() {
    let (l, p) = (5.0, 10e3);
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FREE));
    let f = b.frame(n0, n1, Releases::NONE);
    b.node_load(n1, [0.0, 0.0, -p, 0.0, 0.0, 0.0]);
    let (a, c) = b.run();
    let ei = E * i33();
    let u = c.displacement(n1).unwrap();
    assert_close!(u[2], -p * l.powi(3) / (3.0 * ei));
    assert_close!(u[4], p * l * l / (2.0 * ei));
    let r = c.reaction(n0).unwrap();
    assert_close!(r[2], p);
    assert_close!(r[4], -p * l);
    let forces = c.frame_forces(f).unwrap();
    // Hogging at the root, nothing at the tip; the shear is constant.
    assert_close!(forces[5], -p * l);
    assert!(forces[11].abs() < 1e-6);
    assert_close!(forces[1], forces[7]);
    assert_eq!((a.dofs, a.held), (6, 0));
}

#[test]
fn cantilever_under_its_own_weight() {
    let (l, rho) = (4.0, 2500.0);
    let mut b = Build::new(rho, 1.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(0.0, l, 0.0, FREE));
    b.frame(n0, n1, Releases::NONE);
    let (_, c) = b.run();
    let w = rho * GRAVITY * area();
    assert_close!(c.displacement(n1).unwrap()[2], -w * l.powi(4) / (8.0 * E * i33()));
    assert_close!(c.reaction(n0).unwrap()[2], w * l);
}

#[test]
fn simply_supported_beam_with_a_uniform_load() {
    let (l, w) = (8.0, 12e3);
    let mut b = Build::new(0.0, 0.0);
    let n0 = b.node(0.0, 0.0, 0.0, PIN);
    let mid = b.node(l / 2.0, 0.0, 0.0, FREE);
    let n2 = b.node(l, 0.0, 0.0, ROLLER);
    let f0 = b.frame(n0, mid, Releases::NONE);
    let f1 = b.frame(mid, n2, Releases::NONE);
    b.uniform(f0, w);
    b.uniform(f1, w);
    let (_, c) = b.run();
    let ei = E * i33();
    assert_close!(c.displacement(mid).unwrap()[2], -5.0 * w * l.powi(4) / (384.0 * ei));
    assert_close!(c.displacement(n0).unwrap()[4], w * l.powi(3) / (24.0 * ei));
    assert_close!(c.reaction(n0).unwrap()[2], w * l / 2.0);
    assert_close!(c.reaction(n2).unwrap()[2], w * l / 2.0);
    // Sagging at midspan.
    assert_close!(c.frame_forces(f0).unwrap()[11], w * l * l / 8.0);
    assert_close!(c.frame_forces(f1).unwrap()[5], w * l * l / 8.0);
}

#[test]
fn fixed_beam_with_uniform_point_and_triangular_loads() {
    let l = 6.0;
    let w = 5e3;
    let ei = E * i33();
    // Uniform: end moments wL²/12, midspan deflection wL⁴/384EI.
    let mut b = Build::new(0.0, 0.0);
    let (n0, mid, n2) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l / 2.0, 0.0, 0.0, FREE), b.node(l, 0.0, 0.0, FIXED));
    let f0 = b.frame(n0, mid, Releases::NONE);
    let f1 = b.frame(mid, n2, Releases::NONE);
    b.uniform(f0, w);
    b.uniform(f1, w);
    let (_, c) = b.run();
    assert_close!(c.displacement(mid).unwrap()[2], -w * l.powi(4) / (384.0 * ei));
    assert_close!(c.frame_forces(f0).unwrap()[5], -w * l * l / 12.0);
    assert_close!(c.frame_forces(f1).unwrap()[11], -w * l * l / 12.0);

    // Point load P at a: end moments Pab²/L² and Pa²b/L².
    let (p, a) = (20e3, 2.0);
    let bb = l - a;
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FIXED));
    let f = b.frame(n0, n1, Releases::NONE);
    b.frame_load(f, FrameLoadShape::Point, LoadDirection::Gravity, (a / l, a / l), (p, p));
    let (_, c) = b.run();
    let forces = c.frame_forces(f).unwrap();
    assert_close!(forces[5], -p * a * bb * bb / (l * l));
    assert_close!(forces[11], -p * a * a * bb / (l * l));
    assert_close!(c.reaction(n0).unwrap()[2], p * bb * bb * (3.0 * a + bb) / l.powi(3));

    // Triangular, zero at i to w at j: end moments wL²/30 and wL²/20.
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FIXED));
    let f = b.frame(n0, n1, Releases::NONE);
    b.frame_load(f, FrameLoadShape::Distributed, LoadDirection::Gravity, (0.0, 1.0), (0.0, w));
    let (_, c) = b.run();
    let forces = c.frame_forces(f).unwrap();
    assert_close!(forces[5], -w * l * l / 30.0);
    assert_close!(forces[11], -w * l * l / 20.0);
}

#[test]
fn partial_trapezoidal_load_matches_integrated_point_loads() {
    // A load from 1.5 m to 4.5 m rising from 2 to 7 kN/m on a 6 m fixed beam: the end moments
    // are the integrals of the point-load formulas, done here by Simpson's rule.
    let (l, a, bx, wa, wb) = (6.0, 1.5, 4.5, 2e3, 7e3);
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FIXED));
    let f = b.frame(n0, n1, Releases::NONE);
    b.frame_load(f, FrameLoadShape::Distributed, LoadDirection::Gravity, (a / l, bx / l), (wa, wb));
    let (_, c) = b.run();
    let n = 2000;
    let h = (bx - a) / n as f64;
    let (mut mi, mut mj) = (0.0, 0.0);
    for k in 0..=n {
        let x = a + k as f64 * h;
        let wgt = if k == 0 || k == n {
            1.0
        } else if k % 2 == 1 {
            4.0
        } else {
            2.0
        };
        let q = wa + (wb - wa) * (x - a) / (bx - a);
        mi += wgt * q * x * (l - x).powi(2) / (l * l);
        mj += wgt * q * x * x * (l - x) / (l * l);
    }
    let forces = c.frame_forces(f).unwrap();
    assert_close!(forces[5], -mi * h / 3.0, 1e-9);
    assert_close!(forces[11], -mj * h / 3.0, 1e-9);
}

#[test]
fn a_released_end_makes_a_propped_cantilever() {
    let (l, w) = (5.0, 8e3);
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FIXED));
    let f = b.frame(n0, n1, Releases::new(0, Releases::MOMENTS));
    b.uniform(f, w);
    let (_, c) = b.run();
    let forces = c.frame_forces(f).unwrap();
    assert_close!(forces[5], -w * l * l / 8.0);
    assert!(forces[11].abs() < 1e-6);
    assert_close!(c.reaction(n1).unwrap()[2], 3.0 * w * l / 8.0);
    assert_close!(c.reaction(n0).unwrap()[2], 5.0 * w * l / 8.0);
}

#[test]
fn axial_and_torsional_stiffness() {
    let (l, p, t) = (3.5, 50e3, 4e3);
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(1.0, 2.0, 0.0, FIXED), b.node(1.0, 2.0, l, FREE));
    let f = b.frame(n0, n1, Releases::NONE);
    b.node_load(n1, [0.0, 0.0, -p, 0.0, 0.0, t]);
    let (_, c) = b.run();
    let u = c.displacement(n1).unwrap();
    assert_close!(u[2], -p * l / (E * area()));
    let g = E / (2.0 * (1.0 + NU));
    let j = SectionProperties::of(&SectionShape::Rectangle { b: B, h: H }).unwrap().torsion;
    assert_close!(u[5], t * l / (g * j));
    let forces = c.frame_forces(f).unwrap();
    // Compression, and the torque the column carries.
    assert_close!(forces[0], -p);
    assert_close!(forces[3], t);
}

#[test]
fn weak_axis_bending_uses_i22() {
    // A beam along X loaded sideways (along Y) bends about its local 2 axis.
    let (l, p) = (4.0, 3e3);
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(l, 0.0, 0.0, FREE));
    b.frame(n0, n1, Releases::NONE);
    b.node_load(n1, [0.0, p, 0.0, 0.0, 0.0, 0.0]);
    let (_, c) = b.run();
    assert_close!(c.displacement(n1).unwrap()[1], p * l.powi(3) / (3.0 * E * i22()));
}

#[test]
fn two_span_continuous_beam() {
    // Equal spans under a uniform load: reactions 3wL/8, 10wL/8, 3wL/8; support moment wL²/8.
    let (l, w) = (5.0, 10e3);
    let mut b = Build::new(0.0, 0.0);
    let n0 = b.node(0.0, 0.0, 0.0, PIN);
    let n1 = b.node(l, 0.0, 0.0, ROLLER);
    let n2 = b.node(2.0 * l, 0.0, 0.0, ROLLER);
    let f0 = b.frame(n0, n1, Releases::NONE);
    let f1 = b.frame(n1, n2, Releases::NONE);
    b.uniform(f0, w);
    b.uniform(f1, w);
    let (_, c) = b.run();
    assert_close!(c.reaction(n0).unwrap()[2], 3.0 * w * l / 8.0);
    assert_close!(c.reaction(n1).unwrap()[2], 10.0 * w * l / 8.0);
    assert_close!(c.reaction(n2).unwrap()[2], 3.0 * w * l / 8.0);
    assert_close!(c.frame_forces(f0).unwrap()[11], -w * l * l / 8.0);
}

#[test]
fn tripod_truss_by_statics() {
    let (h, r, p) = (4.0, 3.0, 90e3);
    let mut b = Build::new(0.0, 0.0);
    let apex = b.node(0.0, 0.0, h, FREE);
    // Pinned members: both moments released at both ends, torsion at one.
    let truss = Releases::new(Releases::MOMENTS, Releases::MOMENTS | Releases::T);
    let mut members = Vec::new();
    for k in 0..3 {
        let t = k as f64 * std::f64::consts::TAU / 3.0;
        let base = b.node(r * t.cos(), r * t.sin(), 0.0, Restraint::PINNED);
        members.push(b.frame(base, apex, truss));
    }
    b.node_load(apex, [0.0, 0.0, -p, 0.0, 0.0, 0.0]);
    let (a, c) = b.run();
    let len = (h * h + r * r).sqrt();
    for f in members {
        // Each bar carries a third of the load along its own line, in compression.
        assert_close!(c.frame_forces(f).unwrap()[6], -p * len / (3.0 * h));
    }
    // Nothing gives the apex or the bars' bases rotational stiffness: those are held.
    assert!(a.held >= 3, "held {}", a.held);
}

#[test]
fn turning_the_model_about_z_turns_the_results() {
    let build = |turn: f64| {
        let q = DQuat::from_rotation_z(turn);
        let at = |x: f64, y: f64, z: f64| q * DVec3::new(x, y, z) + DVec3::new(10.0, -3.0, 0.0);
        let mut b = Build::new(2400.0, 1.0);
        let p = [at(0.0, 0.0, 0.0), at(6.0, 0.0, 0.0), at(6.0, 4.0, 0.0), at(0.0, 4.0, 0.0)];
        let base: Vec<NodeId> = p.iter().map(|v| b.node(v.x, v.y, v.z, FIXED)).collect();
        let top: Vec<NodeId> = p.iter().map(|v| b.node(v.x, v.y, v.z + 3.0, FREE)).collect();
        for k in 0..4 {
            // A column's axis 2 follows global X, so turn its section with the model.
            let column = b.frame(base[k], top[k], Releases::NONE);
            let mut row = *b.m.frames().get(column).unwrap();
            row.angle = turn;
            b.m.apply(Change::Frames(Op::Set(vec![(column, row)]))).unwrap();
            let f = b.frame(top[k], top[(k + 1) % 4], Releases::NONE);
            b.uniform(f, 1e3 * (k + 1) as f64);
        }
        b.frame(base[0], top[2], Releases::new(Releases::MOMENTS, Releases::MOMENTS));
        let push = q * DVec3::new(5e3, 2e3, 0.0);
        b.node_load(top[1], [push.x, push.y, 0.0, 0.0, 0.0, 0.0]);
        let (_, c) = b.run();
        top.iter().map(|n| c.displacement(*n).unwrap()).collect::<Vec<_>>()
    };
    let turn = 0.73;
    let (a, b) = (build(0.0), build(turn));
    let q = DQuat::from_rotation_z(turn);
    for (u, v) in a.iter().zip(&b) {
        let (t, r) = (q * DVec3::new(u[0], u[1], u[2]), q * DVec3::new(u[3], u[4], u[5]));
        for (x, y) in t.to_array().iter().zip(&v[..3]).chain(r.to_array().iter().zip(&v[3..])) {
            assert!((x - y).abs() < 1e-9 * u.iter().fold(0.0f64, |m, v| m.max(v.abs())), "{x} vs {y}");
        }
    }
}

#[test]
fn slab_loads_reach_the_supports() {
    for (lx, ly, span) in [(8.0, 6.0, SlabSpan::Auto), (9.0, 3.0, SlabSpan::Auto), (6.0, 6.0, SlabSpan::OneWay)] {
        let p = 4e3;
        let mut b = Build::new(0.0, 0.0);
        let material = b.m.materials().ids().next().unwrap();
        let shell = add(&mut b.m, Change::Sections(Op::Add(vec![Section { name: "Slab".into(), material, shape: SectionShape::Shell { t: 0.2 } }])));
        let corners = [(0.0, 0.0), (lx, 0.0), (lx, ly), (0.0, ly)];
        let base: Vec<NodeId> = corners.iter().map(|(x, y)| b.node(*x, *y, 0.0, FIXED)).collect();
        let top: Vec<NodeId> = corners.iter().map(|(x, y)| b.node(*x, *y, 3.0, FREE)).collect();
        let mut beams = Vec::new();
        for k in 0..4 {
            b.frame(base[k], top[k], Releases::NONE);
            beams.push(b.frame(top[k], top[(k + 1) % 4], Releases::NONE));
        }
        let slab = add(&mut b.m, Change::Areas(Op::Add(vec![Area { corners: top.iter().copied().collect(), section: shell, role: AreaRole::Slab }])));
        add::<AreaLoad>(&mut b.m, Change::AreaLoads(Op::Add(vec![AreaLoad { area: slab, pattern: b.pattern, pressure: p, span }])));
        let (_, c) = b.run();
        let total: f64 = base.iter().map(|n| c.reaction(*n).unwrap()[2]).sum();
        assert_close!(total, p * lx * ly);
        if ly * 2.0 < lx {
            // One-way: the short beams (along Y) carry no slab load, so no shear at their ends
            // beyond what the frame action gives; the long beams carry p·ly/2 each.
            let shear: f64 = [beams[0], beams[2]].iter().map(|f| c.frame_forces(*f).unwrap()[1] - c.frame_forces(*f).unwrap()[7]).sum();
            assert_close!(shear.abs(), p * lx * ly, 1e-9);
        }
    }
}

#[test]
fn slab_edge_without_a_beam_loads_its_corners() {
    let p = 2e3;
    let mut b = Build::new(0.0, 0.0);
    let material = b.m.materials().ids().next().unwrap();
    let shell = add(&mut b.m, Change::Sections(Op::Add(vec![Section { name: "Slab".into(), material, shape: SectionShape::Shell { t: 0.2 } }])));
    let corners = [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)];
    let base: Vec<NodeId> = corners.iter().map(|(x, y)| b.node(*x, *y, 0.0, FIXED)).collect();
    let top: Vec<NodeId> = corners.iter().map(|(x, y)| b.node(*x, *y, 3.0, FREE)).collect();
    for k in 0..4 {
        b.frame(base[k], top[k], Releases::NONE);
    }
    b.frame(top[0], top[1], Releases::NONE);
    let slab = add(&mut b.m, Change::Areas(Op::Add(vec![Area { corners: top.iter().copied().collect(), section: shell, role: AreaRole::Slab }])));
    add::<AreaLoad>(&mut b.m, Change::AreaLoads(Op::Add(vec![AreaLoad { area: slab, pattern: b.pattern, pressure: p, span: SlabSpan::TwoWay }])));
    let (_, c) = b.run();
    let total: f64 = base.iter().map(|n| c.reaction(*n).unwrap()[2]).sum();
    assert_close!(total, p * 25.0);
}

#[test]
fn unstable_and_unresisted_models_are_errors() {
    // No supports.
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FREE), b.node(3.0, 0.0, 0.0, FREE));
    b.frame(n0, n1, Releases::NONE);
    b.node_load(n1, [0.0, 0.0, -1e3, 0.0, 0.0, 0.0]);
    assert!(matches!(analyze(&b.m), Err(AnalysisError::Unstable(_))), "{:?}", analyze(&b.m).map(|a| a.dofs));

    // A moment on the pinned end of a member: nothing resists it.
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(3.0, 0.0, 0.0, Restraint(0b111)));
    b.frame(n0, n1, Releases::new(0, Releases::MOMENTS));
    b.node_load(n1, [0.0, 0.0, 0.0, 0.0, 5e3, 0.0]);
    assert_eq!(analyze(&b.m).err(), Some(AnalysisError::Unresisted { node: n1, dof: 4 }));

    // A load on a node no frame reaches.
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1, lonely) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(3.0, 0.0, 0.0, FREE), b.node(9.0, 0.0, 0.0, FREE));
    b.frame(n0, n1, Releases::NONE);
    b.node_load(lonely, [1e3, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(analyze(&b.m).err(), Some(AnalysisError::Unresisted { node: lonely, dof: 0 }));

    // Shear released at both ends of the only member.
    let mut b = Build::new(0.0, 0.0);
    let (n0, n1) = (b.node(0.0, 0.0, 0.0, FIXED), b.node(3.0, 0.0, 0.0, FIXED));
    let f = b.frame(n0, n1, Releases::new(Releases::V2, Releases::V2));
    assert!(matches!(analyze(&b.m), Err(AnalysisError::Frame { frame, .. }) if frame == f));

    assert_eq!(analyze(&Model::empty()).err(), Some(AnalysisError::NoCases));
    let b = Build::new(0.0, 0.0);
    assert_eq!(analyze(&b.m).err(), Some(AnalysisError::NoFrames));
}

#[test]
fn sparse_assembly_matches_a_dense_solve() {
    // A small irregular space frame: assemble a dense matrix here, independently of the sparse
    // layout, and compare the displacements.
    let mut b = Build::new(2000.0, 1.0);
    let pts = [(0.0, 0.0, 0.0), (5.0, 0.5, 0.0), (1.0, 4.0, 0.0), (0.5, 0.2, 3.0), (5.2, 0.4, 3.3), (1.1, 4.2, 2.9), (2.5, 2.0, 5.5)];
    let nodes: Vec<NodeId> = pts.iter().enumerate().map(|(k, (x, y, z))| b.node(*x, *y, *z, if k < 3 { FIXED } else { FREE })).collect();
    let bars = [(0, 3), (1, 4), (2, 5), (3, 4), (4, 5), (5, 3), (3, 6), (4, 6), (5, 6), (0, 4)];
    let mut frames = Vec::new();
    for (k, (i, j)) in bars.iter().enumerate() {
        let rel = if k == 9 { Releases::new(Releases::MOMENTS, Releases::MOMENTS | Releases::T) } else { Releases::NONE };
        frames.push(b.frame(nodes[*i], nodes[*j], rel));
    }
    b.node_load(nodes[6], [3e3, -2e3, -10e3, 1e3, 0.0, 500.0]);
    b.frame_load(frames[3], FrameLoadShape::Distributed, LoadDirection::Local3, (0.2, 0.9), (1e3, -4e3));
    b.frame_load(frames[7], FrameLoadShape::Point, LoadDirection::X, (0.3, 0.3), (2e3, 2e3));
    let (_, c) = b.run();

    let s = setup(&b.m).unwrap();
    // Free dofs: the four upper nodes, six each.
    let index = |n: NodeId| nodes.iter().position(|x| *x == n).filter(|k| *k >= 3).map(|k| (k - 3) * 6);
    let n = 24;
    let mut k = vec![vec![0.0; n]; n];
    let mut f = vec![0.0; n];
    let mut loads = vec![[0.0; 12]];
    for fr in &frames {
        let member = s.member(*fr, &mut loads).unwrap();
        let kg = element::rotate_stiffness(&member.rotation, &member.k);
        let fg = element::rotate_vector(&member.rotation, &loads[0], false);
        let ends = [b.m.frames().get(*fr).unwrap().i, b.m.frames().get(*fr).unwrap().j];
        for a in 0..12 {
            let Some(ra) = index(ends[a / 6]) else { continue };
            f[ra + a % 6] += fg[a];
            for bb in 0..12 {
                if let Some(rb) = index(ends[bb / 6]) {
                    k[ra + a % 6][rb + bb % 6] += kg[a][bb];
                }
            }
        }
    }
    for (d, v) in [3e3, -2e3, -10e3, 1e3, 0.0, 500.0].iter().enumerate() {
        f[18 + d] += v;
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..n {
        let piv = (col..n).max_by(|a, b| k[*a][col].abs().total_cmp(&k[*b][col].abs())).unwrap();
        k.swap(col, piv);
        f.swap(col, piv);
        for r in col + 1..n {
            let m = k[r][col] / k[col][col];
            for cc in col..n {
                k[r][cc] -= m * k[col][cc];
            }
            f[r] -= m * f[col];
        }
    }
    let mut u = vec![0.0; n];
    for r in (0..n).rev() {
        u[r] = (f[r] - (r + 1..n).map(|cc| k[r][cc] * u[cc]).sum::<f64>()) / k[r][r];
    }
    for (kk, node) in nodes.iter().enumerate().skip(3) {
        let got = c.displacement(*node).unwrap();
        for d in 0..6 {
            let want = u[(kk - 3) * 6 + d];
            assert!((got[d] - want).abs() <= 1e-9 * u.iter().fold(0.0f64, |m, v| m.max(v.abs())), "node {kk} dof {d}: {} vs {want}", got[d]);
        }
    }
}
