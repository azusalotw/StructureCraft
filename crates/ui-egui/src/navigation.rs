//! The one camera path: every orbit, pan and zoom in the viewport goes through [`handle`].
//!
//! Right drag orbits, middle drag (or Shift + right drag) pans, the wheel or a pinch zooms
//! towards the target. The left button belongs to the active tool.

use structurecraft_geom::DVec2;
use structurecraft_render::Camera;

const ORBIT_RADIANS_PER_POINT: f64 = 0.008;
const ZOOM_PER_SCROLL_POINT: f64 = 0.002;

/// Returns whether the camera moved.
pub fn handle(ui: &egui::Ui, response: &egui::Response, camera: &mut Camera) -> bool {
    let before = *camera;
    let size = response.rect.size();
    let drag = response.drag_delta();
    let drag = DVec2::new(f64::from(drag.x), f64::from(drag.y));
    let shift = ui.input(|i| i.modifiers.shift);
    let pan = response.dragged_by(egui::PointerButton::Middle) || (shift && response.dragged_by(egui::PointerButton::Secondary));
    if pan {
        let h = f64::from(size.y.max(1.0));
        camera.pan(DVec2::new(drag.x / h, drag.y / h));
    } else if response.dragged_by(egui::PointerButton::Secondary) {
        camera.orbit(drag * ORBIT_RADIANS_PER_POINT);
    }
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp() * f64::from(pinch);
        if factor != 1.0 {
            camera.zoom(factor);
        }
    }
    *camera != before
}

/// Preset views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Plan,
    Front,
    Side,
    Iso,
}

/// Plan and elevations are orthographic, as drawings are; the 3D view is in perspective.
pub fn preset(camera: &mut Camera, view: Preset) {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
    use structurecraft_render::Projection;
    camera.projection = if view == Preset::Iso { Projection::Perspective } else { Projection::Orthographic };
    let (yaw, pitch) = match view {
        Preset::Plan => (-FRAC_PI_2, FRAC_PI_2 - 1e-3),
        Preset::Front => (-FRAC_PI_2, 0.0),
        Preset::Side => (0.0, 0.0),
        Preset::Iso => (-FRAC_PI_4, 0.5),
    };
    camera.yaw = yaw;
    camera.pitch = pitch;
}
