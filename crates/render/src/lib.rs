//! View math (L1): the camera and projections, independent of any GPU or UI toolkit so the
//! CLI, MCP server and tests can compute the same views as the window.

mod camera;

pub use camera::{Camera, Projection};
