//! One frame element: its 12×12 stiffness in local axes (Euler-Bernoulli, six degrees of
//! freedom a node), the nodal loads equivalent to loads along it, end releases by static
//! condensation, and the rotation between local and global axes.
//!
//! Local degrees of freedom, end i then end j: u1, u2, u3, θ1, θ2, θ3. A force vector in the
//! same order holds forces along and moments about the local axes.

use structurecraft_geom::DVec3;

pub type M12 = [[f64; 12]; 12];
pub type V12 = [f64; 12];

/// The stiffnesses of a prismatic member.
#[derive(Debug, Clone, Copy)]
pub struct Rigidity {
    /// E A, N.
    pub axial: f64,
    /// G J, N·m².
    pub torsion: f64,
    /// E I22, N·m².
    pub bending2: f64,
    /// E I33, N·m².
    pub bending3: f64,
}

/// The local stiffness matrix of a member of length `l`.
pub fn stiffness(r: &Rigidity, l: f64) -> M12 {
    let mut k = [[0.0; 12]; 12];
    for (a, b, s) in [(0, 6, r.axial / l), (3, 9, r.torsion / l)] {
        k[a][a] = s;
        k[b][b] = s;
        k[a][b] = -s;
        k[b][a] = -s;
    }
    // Bending in the 1-2 plane: deflection u2, slope θ3 = du2/dx.
    bending(&mut k, [1, 5, 7, 11], r.bending3, l, 1.0);
    // Bending in the 1-3 plane: deflection u3, slope −θ2 = du3/dx.
    bending(&mut k, [2, 4, 8, 10], r.bending2, l, -1.0);
    k
}

/// Add the beam stiffness for deflection and slope at both ends (`dofs`), where the slope dof
/// is `sign` times the derivative of the deflection.
fn bending(k: &mut M12, dofs: [usize; 4], ei: f64, l: f64, sign: f64) {
    let (a, b, c, d) = (12.0 * ei / l.powi(3), 6.0 * ei / (l * l), 4.0 * ei / l, 2.0 * ei / l);
    let beam = [[a, b, -a, b], [b, c, -b, d], [-a, -b, a, -b], [b, d, -b, c]];
    let s = [1.0, sign, 1.0, sign];
    for r in 0..4 {
        for col in 0..4 {
            k[dofs[r]][dofs[col]] += beam[r][col] * s[r] * s[col];
        }
    }
}

/// Add the nodal loads equivalent to a force `q` (local components, N) at distance `x` from
/// end i. For a prismatic Euler-Bernoulli member these are the fixed-end reactions, reversed.
pub fn add_point(f: &mut V12, q: DVec3, x: f64, l: f64) {
    let t = x / l;
    let (n1, n2, n3, n4) = hermite(t, l);
    f[0] += q.x * (1.0 - t);
    f[6] += q.x * t;
    f[1] += q.y * n1;
    f[5] += q.y * n2;
    f[7] += q.y * n3;
    f[11] += q.y * n4;
    f[2] += q.z * n1;
    f[4] -= q.z * n2;
    f[8] += q.z * n3;
    f[10] -= q.z * n4;
}

/// Add the nodal loads equivalent to a load varying linearly from `qa` at `a` to `qb` at `b`
/// (local components, N/m; distances from end i, m). Three Gauss points integrate the cubic
/// shape functions times the linear load exactly.
pub fn add_distributed(f: &mut V12, qa: DVec3, qb: DVec3, a: f64, b: f64, l: f64) {
    const POINTS: [(f64, f64); 3] = [(-0.774_596_669_241_483_4, 5.0 / 9.0), (0.0, 8.0 / 9.0), (0.774_596_669_241_483_4, 5.0 / 9.0)];
    let half = (b - a) / 2.0;
    if !(half > 0.0) {
        return;
    }
    for (xi, w) in POINTS {
        let s = (xi + 1.0) / 2.0;
        add_point(f, (qa + (qb - qa) * s) * (w * half), a + half * (xi + 1.0), l);
    }
}

/// Cubic Hermite shape functions at `t = x / l`: deflection and slope at end i, then at end j.
fn hermite(t: f64, l: f64) -> (f64, f64, f64, f64) {
    let (t2, t3) = (t * t, t * t * t);
    (1.0 - 3.0 * t2 + 2.0 * t3, l * (t - 2.0 * t2 + t3), 3.0 * t2 - 2.0 * t3, l * (t3 - t2))
}

/// Remove the released degrees of freedom (`released`, bit per local dof) from `k` and from each
/// load vector by static condensation; their rows and columns become zero. `Err` with the dof
/// when the released set leaves the member a mechanism (say both shears at one end).
pub fn condense(k: &mut M12, loads: &mut [V12], released: u16) -> Result<(), usize> {
    if released == 0 {
        return Ok(());
    }
    let scale = (0..12).map(|i| k[i][i]).fold(0.0, f64::max);
    for r in (0..12).filter(|r| released & (1 << r) != 0) {
        let pivot = k[r][r];
        if !(pivot > scale * 1e-12) {
            return Err(r);
        }
        let row = k[r];
        for i in (0..12).filter(|&i| i != r && row[i] != 0.0) {
            let m = k[i][r] / pivot;
            for j in 0..12 {
                k[i][j] -= m * row[j];
            }
            for f in loads.iter_mut() {
                f[i] -= m * f[r];
            }
        }
        for i in 0..12 {
            k[r][i] = 0.0;
            k[i][r] = 0.0;
        }
        for f in loads.iter_mut() {
            f[r] = 0.0;
        }
    }
    Ok(())
}

/// Rows are the local axes in global coordinates, so `R v` turns a global vector local.
pub type Rotation = [DVec3; 3];

pub fn to_local(r: &Rotation, v: DVec3) -> DVec3 {
    DVec3::new(r[0].dot(v), r[1].dot(v), r[2].dot(v))
}

pub fn to_global(r: &Rotation, v: DVec3) -> DVec3 {
    r[0] * v.x + r[1] * v.y + r[2] * v.z
}

/// `Tᵀ k T`: the stiffness in global axes.
pub fn rotate_stiffness(r: &Rotation, k: &M12) -> M12 {
    let rm = [r[0].to_array(), r[1].to_array(), r[2].to_array()];
    let mut out = [[0.0; 12]; 12];
    for bi in 0..4 {
        for bj in 0..4 {
            // tmp = k_block · R
            let mut tmp = [[0.0; 3]; 3];
            for (a, row) in tmp.iter_mut().enumerate() {
                for (c, v) in row.iter_mut().enumerate() {
                    *v = (0..3).map(|b| k[bi * 3 + a][bj * 3 + b] * rm[b][c]).sum();
                }
            }
            for a in 0..3 {
                for c in 0..3 {
                    out[bi * 3 + a][bj * 3 + c] = (0..3).map(|b| rm[b][a] * tmp[b][c]).sum();
                }
            }
        }
    }
    out
}

/// `T v` (global to local) or `Tᵀ v` (local to global), three components at a time.
pub fn rotate_vector(r: &Rotation, v: &V12, global_to_local: bool) -> V12 {
    let mut out = [0.0; 12];
    for b in 0..4 {
        let part = DVec3::new(v[b * 3], v[b * 3 + 1], v[b * 3 + 2]);
        let turned = if global_to_local { to_local(r, part) } else { to_global(r, part) };
        out[b * 3..b * 3 + 3].copy_from_slice(&turned.to_array());
    }
    out
}

pub fn mul(k: &M12, v: &V12) -> V12 {
    let mut out = [0.0; 12];
    for (o, row) in out.iter_mut().zip(k) {
        *o = row.iter().zip(v).map(|(a, b)| a * b).sum();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rigidity {
        Rigidity { axial: 3.0e9, torsion: 2.0e7, bending2: 5.0e7, bending3: 9.0e7 }
    }

    #[test]
    fn rigid_motions_cost_nothing() {
        let l = 4.0;
        let k = stiffness(&rig(), l);
        // Translation along each axis, and rotation about 3 and 2 through end i.
        let modes: [V12; 5] = [
            [1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0.],
            [0., 1., 0., 0., 0., 0., 0., 1., 0., 0., 0., 0.],
            [0., 0., 1., 0., 0., 0., 0., 0., 1., 0., 0., 0.],
            [0., 0., 0., 0., 0., 1., 0., l, 0., 0., 0., 1.],
            [0., 0., 0., 0., 1., 0., 0., 0., -l, 0., 1., 0.],
        ];
        for m in modes {
            assert!(mul(&k, &m).iter().all(|f| f.abs() < 1e-3), "{:?}", mul(&k, &m));
        }
    }

    #[test]
    fn uniform_load_gives_textbook_fixed_end_forces() {
        let (l, w) = (6.0, -2.0);
        let mut f = [0.0; 12];
        add_distributed(&mut f, DVec3::new(0.0, w, 0.0), DVec3::new(0.0, w, 0.0), 0.0, l, l);
        assert!((f[1] - w * l / 2.0).abs() < 1e-12 && (f[7] - w * l / 2.0).abs() < 1e-12);
        assert!((f[5] - w * l * l / 12.0).abs() < 1e-12 && (f[11] + w * l * l / 12.0).abs() < 1e-12);
        let mut g = [0.0; 12];
        add_distributed(&mut g, DVec3::new(0.0, 0.0, w), DVec3::new(0.0, 0.0, w), 0.0, l, l);
        assert!((g[4] + w * l * l / 12.0).abs() < 1e-12 && (g[10] - w * l * l / 12.0).abs() < 1e-12);
    }

    #[test]
    fn releasing_one_shear_at_both_ends_is_a_mechanism() {
        let mut k = stiffness(&rig(), 3.0);
        assert!(condense(&mut k, &mut [], 1 << 1 | 1 << 7).is_err());
        let mut k = stiffness(&rig(), 3.0);
        assert!(condense(&mut k, &mut [], 0b110000).is_ok());
        assert!(k[4].iter().all(|v| *v == 0.0) && k[5].iter().all(|v| *v == 0.0));
    }
}
