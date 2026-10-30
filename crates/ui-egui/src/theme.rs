//! Colour tokens: every colour the UI draws comes from here.

use egui::Color32;
use structurecraft_gpu::Palette;

pub const VIEWPORT_BACKGROUND: Color32 = Color32::from_rgb(0x1e, 0x22, 0x28);
pub const GRID_MINOR: Color32 = Color32::from_rgb(0x2c, 0x31, 0x39);
pub const GRID_MAJOR: Color32 = Color32::from_rgb(0x3b, 0x42, 0x4d);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x55, 0x5f, 0x6d);
pub const GRID_LABEL: Color32 = Color32::from_rgb(0x9a, 0xa5, 0xb4);
pub const AXIS_X: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4f);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x5f, 0xb8, 0x5a);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x4f, 0x8f, 0xe0);
pub const ERROR: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x4f);
pub const PREVIEW: Color32 = Color32::from_rgb(0xf2, 0xc9, 0x4c);
pub const SNAP: Color32 = Color32::from_rgb(0x4c, 0xe0, 0xf2);
pub const BOX_SELECT_FILL: Color32 = Color32::from_rgba_premultiplied(0x10, 0x30, 0x48, 0x40);
pub const BOX_SELECT_STROKE: Color32 = Color32::from_rgb(0x4c, 0xa8, 0xf2);

/// The 3D scene's colours (sRGB, straight alpha).
pub const SCENE: Palette = Palette {
    node: [0xd8, 0xdc, 0xe2, 255],
    support: [0x4c, 0xe0, 0x8a, 255],
    beam: [0x6f, 0xa8, 0xdc, 255],
    column: [0xe8, 0x9b, 0x4f, 255],
    brace: [0xc0, 0x7a, 0xd8, 255],
    slab: [0x7a, 0x86, 0x96, 70],
    wall: [0x96, 0x86, 0x7a, 70],
    selected: [0xf2, 0xc9, 0x4c, 255],
    selected_area: [0xf2, 0xc9, 0x4c, 110],
};

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
}
