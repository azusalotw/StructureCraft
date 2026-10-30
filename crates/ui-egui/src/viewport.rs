//! The 3D viewport.
//!
//! Until the wgpu renderer lands (step 3 of the plan: instanced nodes and frames, batched area
//! meshes, GPU picking) the viewport draws the ground grid and axes with the egui painter.

use structurecraft_geom::{DVec2, DVec3};
use structurecraft_render::Camera;

use crate::{navigation, theme};

const GRID_HALF_LINES: i32 = 20;
const GRID_SPACING: f64 = 1.0;
const MAJOR_EVERY: i32 = 5;

pub fn show(ui: &mut egui::Ui, camera: &mut Camera) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    navigation::handle(ui, &response, camera);
    let painter = ui.painter_at(rect);
    let viewport = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
    if !(viewport.x > 0.0 && viewport.y > 0.0) {
        return;
    }
    let to_screen = |p: DVec3| camera.project(p, viewport).map(|px| rect.min + egui::vec2(px.x as f32, px.y as f32));
    let line = |a: DVec3, b: DVec3, width: f32, colour: egui::Color32| {
        if let (Some(a), Some(b)) = (to_screen(a), to_screen(b)) {
            painter.line_segment([a, b], egui::Stroke::new(width, colour));
        }
    };
    let extent = f64::from(GRID_HALF_LINES) * GRID_SPACING;
    for i in -GRID_HALF_LINES..=GRID_HALF_LINES {
        if i == 0 {
            continue;
        }
        let colour = if i % MAJOR_EVERY == 0 { theme::GRID_MAJOR } else { theme::GRID_MINOR };
        let t = f64::from(i) * GRID_SPACING;
        line(DVec3::new(t, -extent, 0.0), DVec3::new(t, extent, 0.0), 1.0, colour);
        line(DVec3::new(-extent, t, 0.0), DVec3::new(extent, t, 0.0), 1.0, colour);
    }
    line(DVec3::ZERO, DVec3::X * extent, 2.0, theme::AXIS_X);
    line(DVec3::ZERO, DVec3::Y * extent, 2.0, theme::AXIS_Y);
    line(DVec3::ZERO, DVec3::Z * extent * 0.5, 2.0, theme::AXIS_Z);
}
