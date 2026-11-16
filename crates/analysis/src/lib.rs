//! Linear static analysis of 3D frames by the direct stiffness method (L3).
//!
//! Frames are Euler-Bernoulli members with six degrees of freedom a node and optional end
//! releases. Slabs and walls carry no stiffness yet: a slab hands its pressure to the frames
//! along its edges. The global stiffness is assembled straight into a sparse lower triangle,
//! factored once by sparse Cholesky (`faer`, with fill-reducing ordering), and every load case
//! is solved from that one factor.
//!
//! Supports are taken out of the system rather than imposed with large springs, so the matrix
//! stays well conditioned. A degree of freedom no member gives stiffness to (the rotation of a
//! node where only pinned members meet) is held; a load on one is an error.

// Matrix code reads best with explicit indices.
#![allow(clippy::needless_range_loop)]

mod element;
mod loads;
mod section;

use faer::prelude::Solve;
use faer::sparse::{SparseColMat, SymbolicSparseColMat};
use faer::{Mat, Side};
use rustc_hash::FxHashMap;
use structurecraft_geom::DVec3;
use structurecraft_model::{CaseId, FrameId, FrameLoadShape, LoadPattern, Model, NodeId, PatternId, local_axes};

pub use element::{M12, Rigidity, V12};
pub use loads::distribute as slab_strips;
pub use section::SectionProperties;

use element::Rotation;
use loads::Gathered;

/// Standard gravity, m/s².
pub const GRAVITY: f64 = 9.806_65;

/// Why an analysis could not run.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AnalysisError {
    #[error("the model has no load cases")]
    NoCases,
    #[error("the model has no frames, so nothing carries load")]
    NoFrames,
    #[error("frame {frame:?}: {reason}")]
    Frame { frame: FrameId, reason: String },
    /// A load on a degree of freedom nothing resists (say a moment on a node where only pinned
    /// members meet), or on a node no frame reaches.
    #[error("node {node:?} is loaded along {} but nothing there resists it", DOF_NAMES.get(*dof).copied().unwrap_or("?"))]
    Unresisted { node: NodeId, dof: usize },
    /// The structure can move without straining: missing supports or too many releases.
    #[error("the structure is unstable (it can move without resisting): {0}")]
    Unstable(String),
    #[error("the model is too large to analyse: {0}")]
    TooLarge(String),
}

/// The six degrees of freedom of a node, in order.
pub const DOF_NAMES: [&str; 6] = ["ux", "uy", "uz", "rx", "ry", "rz"];

/// The results of every load case.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub cases: Vec<CaseResult>,
    /// Free degrees of freedom solved for.
    pub dofs: usize,
    /// Degrees of freedom held because no member gives them stiffness.
    pub held: usize,
    /// Entries of the stiffness matrix's lower triangle.
    pub nonzeros: usize,
    /// Per node slot: whether a frame reaches the node (only those have results).
    active: Vec<bool>,
    /// Per node slot: whether the node has a support.
    supported: Vec<bool>,
}

/// One load case's results, indexed by table slot.
#[derive(Debug, Clone, Default)]
pub struct CaseResult {
    pub case: CaseId,
    /// Per node slot: displacements along and rotations about the global axes (m, rad).
    displacements: Vec<[f64; 6]>,
    /// Per node slot: support reactions along and about the global axes (N, N·m); zero where
    /// the node is not supported.
    reactions: Vec<[f64; 6]>,
    /// Per frame slot: the forces the nodes put on the member's ends, in local axes, end i
    /// then end j (see [`end_forces_to_internal`]).
    end_forces: Vec<V12>,
}

impl Analysis {
    pub fn case(&self, id: CaseId) -> Option<&CaseResult> {
        self.cases.iter().find(|c| c.case == id)
    }

    /// Whether `node` has results (a frame reaches it).
    pub fn has_node(&self, node: NodeId) -> bool {
        self.active.get(node.index() as usize).copied().unwrap_or(false)
    }

    pub fn is_supported(&self, node: NodeId) -> bool {
        self.supported.get(node.index() as usize).copied().unwrap_or(false)
    }
}

impl CaseResult {
    pub fn displacement(&self, node: NodeId) -> Option<[f64; 6]> {
        self.displacements.get(node.index() as usize).copied()
    }

    pub fn reaction(&self, node: NodeId) -> Option<[f64; 6]> {
        self.reactions.get(node.index() as usize).copied()
    }

    /// The internal forces at end i and end j: P, V2, V3, T, M2, M3.
    pub fn frame_forces(&self, frame: FrameId) -> Option<[f64; 12]> {
        self.end_forces.get(frame.index() as usize).map(end_forces_to_internal)
    }
}

/// From the forces the nodes put on a member (local axes, end i then end j) to the internal
/// forces at its ends: P (tension positive), V2, V3, T, M2, M3, acting on the face whose
/// outward normal is +1. At end j that face is the end itself; at end i it faces into the
/// member, so the signs flip. A beam sagging under gravity has positive M3.
pub fn end_forces_to_internal(f: &V12) -> [f64; 12] {
    let mut out = [0.0; 12];
    for k in 0..6 {
        out[k] = -f[k];
        out[k + 6] = f[k + 6];
    }
    out
}

/// One frame, ready to assemble.
struct Member {
    nodes: [u32; 2],
    rotation: Rotation,
    /// Condensed local stiffness.
    k: M12,
}

/// Everything about the model the solver reads, set up once.
struct Setup<'a> {
    model: &'a Model,
    /// Case results go in this order.
    cases: Vec<CaseId>,
    /// `factors[case][pattern column]`.
    factors: Vec<Vec<f64>>,
    /// Pattern column → the pattern.
    patterns: Vec<LoadPattern>,
    pattern_slots: FxHashMap<PatternId, usize>,
    gathered: Gathered,
}

impl Setup<'_> {
    fn pattern_column(&self, id: PatternId) -> Option<usize> {
        self.pattern_slots.get(&id).copied()
    }

    /// The frame's stiffness and, in `loads[case]`, its equivalent nodal loads in local axes,
    /// both condensed for its releases.
    fn member(&self, id: FrameId, loads: &mut [V12]) -> Result<Member, AnalysisError> {
        let m = self.model;
        let bad = |reason: &str| AnalysisError::Frame { frame: id, reason: reason.to_owned() };
        let f = m.frames().get(id).ok_or_else(|| bad("missing"))?;
        let (a, b) = (m.nodes().get(f.i).ok_or_else(|| bad("end i is missing"))?, m.nodes().get(f.j).ok_or_else(|| bad("end j is missing"))?);
        let l = a.position.distance(b.position);
        let rotation = local_axes(a.position, b.position, f.angle).ok_or_else(|| bad("its ends coincide"))?;
        let section = m.sections().get(f.section).ok_or_else(|| bad("its section is missing"))?;
        let material = m.materials().get(section.material).ok_or_else(|| bad("its material is missing"))?;
        let props = SectionProperties::of(&section.shape).ok_or_else(|| bad("its section is a shell section"))?;
        let e = material.elastic_modulus;
        let g = e / (2.0 * (1.0 + material.poisson_ratio));
        let rig = Rigidity { axial: e * props.area, torsion: g * props.torsion, bending2: e * props.i22, bending3: e * props.i33 };
        let mut k = element::stiffness(&rig, l);

        for v in loads.iter_mut() {
            *v = [0.0; 12];
        }
        let mut add = |pattern: usize, f: &dyn Fn(&mut V12)| {
            let mut one = [0.0; 12];
            f(&mut one);
            for (case, v) in loads.iter_mut().enumerate() {
                let factor = self.factors[case][pattern];
                if factor != 0.0 {
                    for (x, y) in v.iter_mut().zip(&one) {
                        *x += factor * y;
                    }
                }
            }
        };
        let weight = material.density * GRAVITY * props.area;
        for (p, pattern) in self.patterns.iter().enumerate() {
            if pattern.self_weight != 0.0 && weight != 0.0 {
                let q = element::to_local(&rotation, DVec3::NEG_Z * weight * pattern.self_weight);
                add(p, &|v| element::add_distributed(v, q, q, 0.0, l, l));
            }
        }
        for (p, load) in self.gathered.frame_loads.of(id.index()) {
            // `vector` gives the global direction, local axes included.
            let dir = element::to_local(&rotation, load.direction.vector(&rotation));
            match load.shape {
                FrameLoadShape::Distributed => {
                    let (qa, qb) = (dir * load.value_start, dir * load.value_end);
                    add(*p, &|v| element::add_distributed(v, qa, qb, load.start * l, load.end * l, l));
                }
                FrameLoadShape::Point => {
                    let q = dir * load.value_start;
                    add(*p, &|v| element::add_point(v, q, load.start * l, l));
                }
            }
        }
        for strip in self.gathered.strips.of(id.index()) {
            let down = element::to_local(&rotation, DVec3::NEG_Z);
            let (qa, qb) = (down * strip.w0, down * strip.w1);
            add(strip.pattern, &|v| element::add_distributed(v, qa, qb, strip.x0.clamp(0.0, l), strip.x1.clamp(0.0, l), l));
        }
        element::condense(&mut k, loads, f.releases.0).map_err(|dof| {
            bad(&format!("its releases leave it free to move ({} at end {})", RELEASE_NAMES[dof % 6], if dof < 6 { 'i' } else { 'j' }))
        })?;
        Ok(Member { nodes: [f.i.index(), f.j.index()], rotation, k })
    }
}

const RELEASE_NAMES: [&str; 6] = ["P", "V2", "V3", "T", "M2", "M3"];

/// No degree of freedom gets a number.
const NONE: u32 = u32::MAX;

/// Analyse every load case of `model`.
pub fn analyze(model: &Model) -> Result<Analysis, AnalysisError> {
    solve(&setup(model)?)
}

fn setup(model: &Model) -> Result<Setup<'_>, AnalysisError> {
    if model.cases().is_empty() {
        return Err(AnalysisError::NoCases);
    }
    if model.frames().is_empty() {
        return Err(AnalysisError::NoFrames);
    }
    let patterns: Vec<(PatternId, LoadPattern)> = model.patterns().iter().map(|(id, p)| (id, p.clone())).collect();
    let pattern_slots: FxHashMap<PatternId, usize> = patterns.iter().enumerate().map(|(k, (id, _))| (*id, k)).collect();
    let cases: Vec<CaseId> = model.cases().ids().collect();
    let factors: Vec<Vec<f64>> = model
        .cases()
        .iter()
        .map(|(_, c)| {
            let mut row = vec![0.0; patterns.len()];
            for (p, f) in &c.patterns {
                if let Some(&k) = pattern_slots.get(p) {
                    row[k] += f;
                }
            }
            row
        })
        .collect();
    let gathered = loads::gather(model, &|id| pattern_slots.get(&id).copied());
    Ok(Setup { model, cases, factors, patterns: patterns.into_iter().map(|(_, p)| p).collect(), pattern_slots, gathered })
}

fn solve(s: &Setup<'_>) -> Result<Analysis, AnalysisError> {
    let m = s.model;
    let ncase = s.cases.len();
    let node_slots = m.nodes().capacity();

    // Number the free degrees of freedom of every node a frame reaches, in slot order.
    let mut active = vec![false; node_slots];
    for (_, f) in m.frames().iter() {
        for n in [f.i, f.j] {
            if let Some(a) = active.get_mut(n.index() as usize) {
                *a = true;
            }
        }
    }
    let mut supported = vec![false; node_slots];
    let mut dof = vec![[NONE; 6]; node_slots];
    let mut ndof: usize = 0;
    for (id, node) in m.nodes().iter() {
        let slot = id.index() as usize;
        supported[slot] = node.restraint.0 != 0;
        if !active[slot] {
            continue;
        }
        for (d, out) in dof[slot].iter_mut().enumerate() {
            if !node.restraint.holds(d) {
                *out = u32::try_from(ndof).map_err(|_| AnalysisError::TooLarge("more than 4 billion degrees of freedom".to_owned()))?;
                ndof += 1;
            }
        }
    }

    // Which nodes each node is joined to (the higher slot of each pair), sorted.
    let mut pairs: Vec<(u32, u32)> = m.frames().iter().map(|(_, f)| (f.i.index().min(f.j.index()), f.i.index().max(f.j.index()))).collect();
    pairs.sort_unstable();
    pairs.dedup();
    let mut nb_start = vec![0usize; node_slots + 1];
    for (a, _) in &pairs {
        nb_start[*a as usize + 1] += 1;
    }
    for i in 0..node_slots {
        nb_start[i + 1] += nb_start[i];
    }
    let neighbours: Vec<u32> = pairs.iter().map(|p| p.1).collect();
    drop(pairs);
    let free = |slot: u32| dof[slot as usize].iter().filter(|d| **d != NONE).count();
    // Free dofs of the neighbours listed before each neighbour entry.
    let mut nb_before = vec![0u32; neighbours.len()];
    for n in 0..node_slots {
        let mut acc = 0u32;
        for k in nb_start[n]..nb_start[n + 1] {
            nb_before[k] = acc;
            acc += free(neighbours[k]) as u32;
        }
    }

    // Column pointers of the lower triangle, one column per free dof, and the row indices.
    let mut col_ptr = vec![0usize; ndof + 1];
    for n in 0..node_slots {
        let nb_free: usize = (nb_start[n]..nb_start[n + 1]).map(|k| free(neighbours[k])).sum();
        let own: Vec<u32> = dof[n].iter().copied().filter(|d| *d != NONE).collect();
        for (fa, &c) in own.iter().enumerate() {
            col_ptr[c as usize + 1] = own.len() - fa + nb_free;
        }
    }
    for c in 0..ndof {
        col_ptr[c + 1] += col_ptr[c];
    }
    let nnz = col_ptr[ndof];
    let mut row_idx = vec![0usize; nnz];
    for n in 0..node_slots {
        let own: Vec<u32> = dof[n].iter().copied().filter(|d| *d != NONE).collect();
        for (fa, &c) in own.iter().enumerate() {
            let mut at = col_ptr[c as usize];
            for &r in &own[fa..] {
                row_idx[at] = r as usize;
                at += 1;
            }
            for k in nb_start[n]..nb_start[n + 1] {
                for &r in dof[neighbours[k] as usize].iter().filter(|d| **d != NONE) {
                    row_idx[at] = r as usize;
                    at += 1;
                }
            }
        }
    }

    // The position in `values` of the entry (row dof of node `rn`, column dof of node `cn`),
    // with `rn` at or after `cn` in slot order and `fr`, `fc` their free-dof ranks.
    let position = |c: u32, cn: u32, fc: usize, rn: u32, fr: usize| -> Option<usize> {
        let base = *col_ptr.get(c as usize)?;
        if rn == cn {
            return (fr >= fc).then_some(base + fr - fc);
        }
        let own = free(cn);
        let list = neighbours.get(nb_start[cn as usize]..nb_start[cn as usize + 1])?;
        let k = list.binary_search(&rn).ok()? + nb_start[cn as usize];
        Some(base + own - fc + nb_before[k] as usize + fr)
    };

    // Assemble the stiffness and the load vectors.
    let mut values = vec![0.0f64; nnz];
    let mut rhs = Mat::<f64>::zeros(ndof, ncase);
    let mut loads = vec![[0.0; 12]; ncase];
    // Total applied force per case, supports included, for the equilibrium check.
    let mut applied = vec![DVec3::ZERO; ncase];
    for (id, _) in m.frames().iter() {
        let member = s.member(id, &mut loads)?;
        let kg = element::rotate_stiffness(&member.rotation, &member.k);
        // Rank among free dofs of each local dof (0..12), or None when held by a support.
        let mut rank = [None; 12];
        for (e, &n) in member.nodes.iter().enumerate() {
            let mut r = 0;
            for d in 0..6 {
                if dof[n as usize][d] != NONE {
                    rank[e * 6 + d] = Some(r);
                    r += 1;
                }
            }
        }
        for a in 0..12 {
            let Some(ra) = rank[a] else { continue };
            let na = member.nodes[a / 6];
            let col = dof[na as usize][a % 6];
            for b in 0..12 {
                let Some(rb) = rank[b] else { continue };
                let nb = member.nodes[b / 6];
                // Keep the lower triangle: the column's node first in slot order.
                if (na < nb || (na == nb && ra <= rb))
                    && let Some(p) = position(col, na, ra, nb, rb)
                {
                    values[p] += kg[b][a];
                }
            }
        }
        for (c, v) in loads.iter().enumerate() {
            let g = element::rotate_vector(&member.rotation, v, false);
            for (e, &n) in member.nodes.iter().enumerate() {
                applied[c] += DVec3::new(g[e * 6], g[e * 6 + 1], g[e * 6 + 2]);
                for d in 0..6 {
                    let i = dof[n as usize][d];
                    if i != NONE {
                        rhs[(i as usize, c)] += g[e * 6 + d];
                    }
                }
            }
        }
    }
    let node_forces = node_forces(s);
    for (slot, pattern, values6) in &node_forces {
        for c in 0..ncase {
            let factor = s.factors[c][*pattern];
            if factor == 0.0 {
                continue;
            }
            applied[c] += DVec3::new(values6[0], values6[1], values6[2]) * factor;
            let slot = *slot as usize;
            for d in 0..6 {
                let v = values6[d] * factor;
                if v == 0.0 {
                    continue;
                }
                let i = dof.get(slot).map_or(NONE, |x| x[d]);
                if i != NONE {
                    rhs[(i as usize, c)] += v;
                    continue;
                }
                // Not free: a support takes it, unless no frame reaches the node.
                let node = m.nodes().id_at(slot as u32).unwrap_or_default();
                if !m.nodes().get(node).is_some_and(|n| n.restraint.holds(d)) {
                    return Err(AnalysisError::Unresisted { node, dof: d });
                }
            }
        }
    }

    // Hold the dofs no member stiffens, and refuse loads on them.
    let max_diag = (0..ndof).map(|c| values[col_ptr[c]]).fold(0.0, f64::max);
    let mut held = 0;
    let mut held_dofs = Vec::new();
    for c in 0..ndof {
        let d = values[col_ptr[c]];
        if !(d > max_diag * 1e-14) {
            if (0..ncase).any(|k| rhs[(c, k)] != 0.0) {
                let (node, d) = locate(m, &dof, c);
                return Err(AnalysisError::Unresisted { node, dof: d });
            }
            values[col_ptr[c]] = 1.0;
            held += 1;
            held_dofs.push(c);
        }
    }

    // Factor once and solve every case.
    if ndof > 0 {
        let symbolic = SymbolicSparseColMat::new_checked(ndof, ndof, col_ptr, None, row_idx);
        let k = SparseColMat::new(symbolic, values);
        let llt = k.sp_cholesky(Side::Lower).map_err(|_| AnalysisError::Unstable("check the supports and end releases".to_owned()))?;
        llt.solve_in_place(rhs.as_mut());
    }
    for c in &held_dofs {
        for k in 0..ncase {
            rhs[(*c, k)] = 0.0;
        }
    }
    for c in 0..ndof {
        for k in 0..ncase {
            if !rhs[(c, k)].is_finite() {
                return Err(AnalysisError::Unstable("the displacements are not finite".to_owned()));
            }
        }
    }

    // Displacements, end forces and reactions.
    let mut results: Vec<CaseResult> = s
        .cases
        .iter()
        .map(|&case| CaseResult {
            case,
            displacements: vec![[0.0; 6]; node_slots],
            reactions: vec![[0.0; 6]; node_slots],
            end_forces: vec![[0.0; 12]; m.frames().capacity()],
        })
        .collect();
    for (slot, d) in dof.iter().enumerate() {
        for (k, &i) in d.iter().enumerate() {
            if i != NONE {
                for (c, r) in results.iter_mut().enumerate() {
                    r.displacements[slot][k] = rhs[(i as usize, c)];
                }
            }
        }
    }
    for (id, _) in m.frames().iter() {
        let member = s.member(id, &mut loads)?;
        for (c, r) in results.iter_mut().enumerate() {
            let mut ug = [0.0; 12];
            for (e, &n) in member.nodes.iter().enumerate() {
                ug[e * 6..e * 6 + 6].copy_from_slice(&r.displacements[n as usize]);
            }
            let ul = element::rotate_vector(&member.rotation, &ug, true);
            let ku = element::mul(&member.k, &ul);
            let mut f = [0.0; 12];
            for k in 0..12 {
                f[k] = ku[k] - loads[c][k];
            }
            r.end_forces[id.index() as usize] = f;
            let g = element::rotate_vector(&member.rotation, &f, false);
            for (e, &n) in member.nodes.iter().enumerate() {
                if supported[n as usize] {
                    for k in 0..6 {
                        r.reactions[n as usize][k] += g[e * 6 + k];
                    }
                }
            }
        }
    }
    for (slot, pattern, values6) in &node_forces {
        if !supported.get(*slot as usize).copied().unwrap_or(false) {
            continue;
        }
        for (c, r) in results.iter_mut().enumerate() {
            let factor = s.factors[c][*pattern];
            for k in 0..6 {
                r.reactions[*slot as usize][k] -= values6[k] * factor;
            }
        }
    }
    // Keep only the restrained components.
    for (id, node) in m.nodes().iter() {
        let slot = id.index() as usize;
        for r in results.iter_mut() {
            for k in 0..6 {
                if !node.restraint.holds(k) {
                    r.reactions[slot][k] = 0.0;
                }
            }
        }
    }

    // Equilibrium: the reactions must balance the applied loads. A structure that can move
    // freely (no supports, or a mechanism the factorisation missed) fails here.
    for (c, r) in results.iter().enumerate() {
        let reaction: DVec3 = r.reactions.iter().map(|v| DVec3::new(v[0], v[1], v[2])).sum();
        let scale = applied[c].length().max(reaction.length());
        if (applied[c] + reaction).length() > 1e-6 * scale {
            return Err(AnalysisError::Unstable("the supports do not balance the loads; check the supports and end releases".to_owned()));
        }
    }

    Ok(Analysis { cases: results, dofs: ndof, held, nonzeros: nnz, active, supported })
}

/// Every force on a node: the model's node loads and the slab edges with no frame under them,
/// as (node slot, pattern column, Fx Fy Fz Mx My Mz).
fn node_forces(s: &Setup<'_>) -> Vec<(u32, usize, [f64; 6])> {
    let mut out: Vec<(u32, usize, [f64; 6])> =
        s.model.node_loads().iter().filter_map(|(_, l)| Some((l.node.index(), s.pattern_column(l.pattern)?, l.values))).collect();
    out.extend(s.gathered.corner_forces.iter().map(|c| (c.node, c.pattern, [c.force.x, c.force.y, c.force.z, 0.0, 0.0, 0.0])));
    out
}

/// The node and local dof of global dof `c`.
fn locate(m: &Model, dof: &[[u32; 6]], c: usize) -> (NodeId, usize) {
    for (slot, d) in dof.iter().enumerate() {
        if let Some(k) = d.iter().position(|&i| i as usize == c && i != NONE) {
            return (m.nodes().id_at(slot as u32).unwrap_or_default(), k);
        }
    }
    (NodeId::default(), 0)
}

#[cfg(test)]
mod tests;
