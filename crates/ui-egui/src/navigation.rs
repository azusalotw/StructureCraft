//! The one camera path: every orbit, pan and zoom in the viewport goes through [`handle`].
//!
//! Left drag orbits, middle or right drag pans, the wheel or a pinch zooms.

use structurecraft_geom::DVec2;
use structurecraft_render::Camera;

const ORBIT_RADIANS_PER_POINT: f64 = 0.008;
const ZOOM_PER_SCROLL_POINT: f64 = 0.002;

pub fn handle(ui: &egui::Ui, response: &egui::Response, camera: &mut Camera) {
    let size = response.rect.size();
    let drag = response.drag_delta();
    let drag = DVec2::new(f64::from(drag.x), f64::from(drag.y));
    if response.dragged_by(egui::PointerButton::Primary) {
        camera.orbit(drag * ORBIT_RADIANS_PER_POINT);
    } else if response.dragged_by(egui::PointerButton::Middle) || response.dragged_by(egui::PointerButton::Secondary) {
        camera.pan(DVec2::new(drag.x / f64::from(size.y.max(1.0)), drag.y / f64::from(size.y.max(1.0))));
    }
    if response.hovered() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let factor = (f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp() * f64::from(pinch);
        if factor != 1.0 {
            camera.zoom(factor);
        }
    }
}
