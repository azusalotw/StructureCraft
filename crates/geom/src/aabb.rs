use glam::DVec3;
use serde::{Deserialize, Serialize};

/// An axis-aligned bounding box. An empty box has `min > max` and contains nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    pub const EMPTY: Self = Self { min: DVec3::splat(f64::INFINITY), max: DVec3::splat(f64::NEG_INFINITY) };

    pub fn from_points(points: impl IntoIterator<Item = DVec3>) -> Self {
        points.into_iter().fold(Self::EMPTY, |b, p| b.including(p))
    }

    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }

    /// The box grown to contain `p`; non-finite points are ignored.
    #[must_use]
    pub fn including(self, p: DVec3) -> Self {
        if !p.is_finite() {
            return self;
        }
        Self { min: self.min.min(p), max: self.max.max(p) }
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        if other.is_empty() {
            return self;
        }
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub fn center(&self) -> Option<DVec3> {
        (!self.is_empty()).then(|| (self.min + self.max) * 0.5)
    }

    /// Half the diagonal; 0 for an empty box.
    pub fn radius(&self) -> f64 {
        if self.is_empty() { 0.0 } else { (self.max - self.min).length() * 0.5 }
    }

    pub fn contains(&self, p: DVec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_box_contains_nothing() {
        assert!(Aabb::EMPTY.is_empty());
        assert!(!Aabb::EMPTY.contains(DVec3::ZERO));
        assert_eq!(Aabb::EMPTY.radius(), 0.0);
        assert_eq!(Aabb::EMPTY.center(), None);
    }

    #[test]
    fn grows_to_fit_points_and_ignores_nan() {
        let b = Aabb::from_points([DVec3::new(1.0, 2.0, 3.0), DVec3::new(-1.0, 0.0, 5.0), DVec3::splat(f64::NAN)]);
        assert_eq!(b.min, DVec3::new(-1.0, 0.0, 3.0));
        assert_eq!(b.max, DVec3::new(1.0, 2.0, 5.0));
        assert_eq!(b.center(), Some(DVec3::new(0.0, 1.0, 4.0)));
    }
}
