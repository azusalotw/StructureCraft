//! Section properties of frame shapes, about the member's local axes. Depth runs along local
//! axis 2 and width along 3, so I33 is the strong axis of a beam bending under gravity.

use structurecraft_model::SectionShape;

/// Area, torsion constant and the two second moments of area, m², m⁴.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionProperties {
    pub area: f64,
    /// Saint-Venant torsion constant.
    pub torsion: f64,
    /// About local axis 2 (bending that moves the member along 3).
    pub i22: f64,
    /// About local axis 3 (bending that moves the member along 2).
    pub i33: f64,
}

impl SectionProperties {
    /// `None` for a shell, which is not a frame section.
    pub fn of(shape: &SectionShape) -> Option<Self> {
        match *shape {
            SectionShape::Rectangle { b, h } => {
                Some(Self { area: b * h, torsion: rectangle_torsion(b, h), i22: h * b.powi(3) / 12.0, i33: b * h.powi(3) / 12.0 })
            }
            SectionShape::Circle { d } => {
                let i = std::f64::consts::PI * d.powi(4) / 64.0;
                Some(Self { area: std::f64::consts::PI * d * d / 4.0, torsion: 2.0 * i, i22: i, i33: i })
            }
            SectionShape::ISection { h, b, tf, tw } => {
                let web = h - 2.0 * tf;
                Some(Self {
                    area: 2.0 * b * tf + web * tw,
                    // Thin-walled open section: the sum of b t³ / 3 over the flanges and the web.
                    torsion: (2.0 * b * tf.powi(3) + web * tw.powi(3)) / 3.0,
                    i22: (2.0 * tf * b.powi(3) + web * tw.powi(3)) / 12.0,
                    i33: (b * h.powi(3) - (b - tw) * web.powi(3)) / 12.0,
                })
            }
            SectionShape::Shell { .. } => None,
        }
    }
}

/// Saint-Venant torsion constant of a solid rectangle, from the series solution truncated to
/// its usual closed form (within 0.5 % for every aspect ratio).
fn rectangle_torsion(b: f64, h: f64) -> f64 {
    let (long, short) = if b >= h { (b, h) } else { (h, b) };
    let r = short / long;
    long * short.powi(3) * (1.0 / 3.0 - 0.21 * r * (1.0 - r.powi(4) / 12.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_circle_and_i() {
        let r = SectionProperties::of(&SectionShape::Rectangle { b: 0.3, h: 0.6 }).unwrap();
        assert!((r.area - 0.18).abs() < 1e-15);
        assert!((r.i33 - 0.3 * 0.216 / 12.0).abs() < 1e-15);
        assert!((r.i22 - 0.6 * 0.027 / 12.0).abs() < 1e-15);
        // A square's torsion constant is 0.1406 a⁴ (Timoshenko & Goodier).
        let s = SectionProperties::of(&SectionShape::Rectangle { b: 1.0, h: 1.0 }).unwrap();
        assert!((s.torsion - 0.1406).abs() < 1e-3);
        let c = SectionProperties::of(&SectionShape::Circle { d: 2.0 }).unwrap();
        assert!((c.torsion - std::f64::consts::PI / 2.0).abs() < 1e-12);
        let i = SectionProperties::of(&SectionShape::ISection { h: 0.4, b: 0.2, tf: 0.02, tw: 0.01 }).unwrap();
        assert!((i.area - (2.0 * 0.2 * 0.02 + 0.36 * 0.01)).abs() < 1e-15);
        assert!(i.i33 > i.i22);
        assert!(SectionProperties::of(&SectionShape::Shell { t: 0.2 }).is_none());
    }
}
