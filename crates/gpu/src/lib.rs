//! GPU drawing of the model (L5), inside egui's render pass through a paint callback.
//!
//! The whole model is three draws: nodes as instanced screen-space squares, frames as
//! instanced screen-space quads (constant pixel width at any zoom), and areas as one
//! translucent triangle list. Instance data is packed once when the model changes, never per
//! frame; per frame only a 96-byte uniform is written. Positions are f32 relative to an origin
//! near the model, so precision holds for large sites.
//!
//! Selected elements are a second, small set of the same buffers drawn on top.

mod renderer;
mod scene;

pub use renderer::{SceneCallback, Uniforms, ViewRenderer};
pub use scene::{Palette, Rgba, Scene, SceneBuilder, Selection};
