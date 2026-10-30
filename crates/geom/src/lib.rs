//! Geometry primitives shared by every layer (L0).
//!
//! The model works in `f64` metres; the GPU side converts to `f32` relative to a local origin so
//! large buildings keep their precision on screen.

mod aabb;

pub use aabb::Aabb;
pub use glam::{DMat4, DQuat, DVec2, DVec3};

/// A point in model space, in metres.
pub type Point3 = DVec3;

/// Lengths below this are treated as zero (metres).
pub const LENGTH_TOLERANCE: f64 = 1e-9;
