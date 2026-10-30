//! Colour tokens: every colour the UI draws comes from here.

use egui::Color32;

pub const VIEWPORT_BACKGROUND: Color32 = Color32::from_rgb(0x1e, 0x22, 0x28);
pub const GRID_MINOR: Color32 = Color32::from_rgb(0x2c, 0x31, 0x39);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(0x3b, 0x42, 0x4d);
pub const AXIS_X: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4f);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x5f, 0xb8, 0x5a);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x4f, 0x8f, 0xe0);
pub const ERROR: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4f);

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
}
