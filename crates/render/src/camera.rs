use serde::{Deserialize, Serialize};
use structurecraft_geom::{Aabb, DMat4, DVec2, DVec3};

/// How the camera projects the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

/// An orbit camera around a target point. Z is up, as in structural models.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub target: DVec3,
    /// Distance from the target to the eye, metres.
    pub distance: f64,
    /// Rotation about Z, radians.
    pub yaw: f64,
    /// Elevation above the XY plane, radians, kept inside (-π/2, π/2).
    pub pitch: f64,
    /// Vertical field of view, radians (perspective only).
    pub fov_y: f64,
    pub projection: Projection,
}

const MIN_DISTANCE: f64 = 1e-3;
const MAX_DISTANCE: f64 = 1e7;
const PITCH_LIMIT: f64 = std::f64::consts::FRAC_PI_2 - 1e-3;

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            distance: 30.0,
            yaw: -std::f64::consts::FRAC_PI_4,
            pitch: 0.5,
            fov_y: 45f64.to_radians(),
            projection: Projection::Perspective,
        }
    }
}

impl Camera {
    pub fn eye(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + DVec3::new(cp * cy, cp * sy, sp) * self.distance
    }

    pub fn view(&self) -> DMat4 {
        DMat4::look_at_rh(self.eye(), self.target, DVec3::Z)
    }

    /// The projection for a viewport of the given aspect ratio (width / height), with a reversed
    /// infinite depth range for perspective so large models keep depth precision.
    pub fn projection_matrix(&self, aspect: f64) -> DMat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        match self.projection {
            Projection::Perspective => DMat4::perspective_infinite_reverse_rh(self.fov_y, aspect, self.distance * 1e-4),
            Projection::Orthographic => {
                let h = self.distance * (self.fov_y * 0.5).tan();
                let w = h * aspect;
                DMat4::orthographic_rh(-w, w, -h, h, -self.distance * 100.0, self.distance * 100.0)
            }
        }
    }

    /// Model point → viewport pixels (origin top left), or `None` behind the eye.
    pub fn project(&self, p: DVec3, viewport: DVec2) -> Option<DVec2> {
        let clip = self.projection_matrix(viewport.x / viewport.y) * self.view() * p.extend(1.0);
        if !(clip.w > 0.0) {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(DVec2::new((ndc.x + 1.0) * 0.5 * viewport.x, (1.0 - ndc.y) * 0.5 * viewport.y))
    }

    /// Rotate by a drag of `delta` radians (x: yaw, y: pitch).
    pub fn orbit(&mut self, delta: DVec2) {
        if delta.is_finite() {
            self.yaw = (self.yaw - delta.x).rem_euclid(std::f64::consts::TAU);
            self.pitch = (self.pitch + delta.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }
    }

    /// Move the target in the view plane by `delta` viewport fractions.
    pub fn pan(&mut self, delta: DVec2) {
        if !delta.is_finite() {
            return;
        }
        let forward = (self.target - self.eye()).normalize_or_zero();
        let right = forward.cross(DVec3::Z).normalize_or_zero();
        let up = right.cross(forward);
        let scale = self.distance * (self.fov_y * 0.5).tan() * 2.0;
        self.target += (-right * delta.x + up * delta.y) * scale;
    }

    /// Zoom by a factor (> 1 moves closer).
    pub fn zoom(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.distance = (self.distance / factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }

    /// Frame a bounding box; an empty box leaves the camera as it is.
    pub fn fit(&mut self, bounds: &Aabb) {
        if let Some(c) = bounds.center() {
            self.target = c;
            let r = bounds.radius().max(1.0);
            self.distance = (r / (self.fov_y * 0.5).sin()).clamp(MIN_DISTANCE, MAX_DISTANCE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_projects_to_viewport_centre() {
        let cam = Camera::default();
        let px = cam.project(cam.target, DVec2::new(800.0, 600.0)).unwrap();
        assert!((px - DVec2::new(400.0, 300.0)).length() < 1e-6);
    }

    #[test]
    fn pitch_stays_off_the_poles_and_hostile_input_is_ignored() {
        let mut cam = Camera::default();
        cam.orbit(DVec2::new(0.0, 100.0));
        assert!(cam.pitch < std::f64::consts::FRAC_PI_2);
        let before = cam;
        cam.orbit(DVec2::splat(f64::NAN));
        cam.zoom(-1.0);
        cam.pan(DVec2::splat(f64::INFINITY));
        assert_eq!(cam, before);
    }

    #[test]
    fn fit_centres_on_bounds() {
        let mut cam = Camera::default();
        cam.fit(&Aabb::from_points([DVec3::ZERO, DVec3::new(10.0, 10.0, 30.0)]));
        assert_eq!(cam.target, DVec3::new(5.0, 5.0, 15.0));
    }
}
