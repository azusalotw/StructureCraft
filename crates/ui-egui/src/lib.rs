//! The user interface (L6): one egui app for the desktop and the browser.
//!
//! The window is a menu bar, a tool bar, the model panel on the left, properties on the right,
//! a status bar and the 3D viewport in the middle. Every edit runs a registry command on the
//! session, so the UI does exactly what the CLI and MCP server can.

mod navigation;
mod panels;
mod picking;
pub mod theme;
mod tools;
mod viewport;

use std::sync::Arc;

use structurecraft_engine::Session;
use structurecraft_gpu::{Scene, Selection};
use structurecraft_render::Camera;

pub use structurecraft_gpu::ViewRenderer;
pub use tools::Tool;

pub struct StructureCraftApp {
    session: Session,
    camera: Camera,
    status: String,
    /// Whether the wgpu scene renderer is installed (it is not under a non-wgpu backend).
    gpu: bool,
    scene: Arc<Scene>,
    scene_revision: Option<u64>,
    /// Milliseconds the last scene rebuild took.
    scene_build_ms: f64,
    selection: Selection,
    selection_scene: Arc<Scene>,
    selection_version: u64,
    selection_dirty: bool,
    cache: picking::ScreenCache,
    tools: tools::ToolState,
    generate: Option<panels::GenerateForm>,
}

impl Default for StructureCraftApp {
    fn default() -> Self {
        Self {
            session: Session::new(),
            camera: Camera::default(),
            status: String::new(),
            gpu: false,
            scene: Arc::default(),
            scene_revision: None,
            scene_build_ms: 0.0,
            selection: Selection::default(),
            selection_scene: Arc::default(),
            selection_version: 0,
            selection_dirty: false,
            cache: picking::ScreenCache::default(),
            tools: tools::ToolState::default(),
            generate: None,
        }
    }
}

impl StructureCraftApp {
    /// `msaa_samples` must be the window's multisampling (1 in the browser).
    pub fn new(cc: &eframe::CreationContext<'_>, msaa_samples: u32) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self::default();
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let depth = Some(eframe::wgpu::TextureFormat::Depth32Float);
            let renderer = ViewRenderer::new(&rs.device, rs.target_format, depth, msaa_samples);
            rs.renderer.write().callback_resources.insert(renderer);
            app.gpu = true;
        } else {
            log::warn!("no wgpu render state: the 3D view shows the grid only");
        }
        app
    }

    /// Open a model file, as if from File > Open.
    pub fn open(&mut self, path: &str) {
        if self.run("model.open", serde_json::json!({"path": path})).is_some() {
            self.tools.story = self.session.model.grid().stories.len().saturating_sub(1);
            self.fit();
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Run a command; failures land in the status bar.
    fn run(&mut self, id: &str, input: serde_json::Value) -> Option<serde_json::Value> {
        match self.session.execute(id, input) {
            Ok(out) => {
                self.status.clear();
                self.selection.retain_existing(&self.session.model);
                self.selection_dirty = true;
                Some(out)
            }
            Err(e) => {
                log::warn!("{id}: {e}");
                self.status = e.to_string();
                None
            }
        }
    }

    fn fit(&mut self) {
        self.camera.fit(&self.session.model.bounds());
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        use egui::{Key, KeyboardShortcut as K, Modifiers as M};
        let (undo, redo, delete, escape, fit, enter) = ctx.input_mut(|i| {
            let redo = i.consume_shortcut(&K::new(M::COMMAND | M::SHIFT, Key::Z)) || i.consume_shortcut(&K::new(M::COMMAND, Key::Y));
            (
                i.consume_shortcut(&K::new(M::COMMAND, Key::Z)),
                redo,
                i.consume_key(M::NONE, Key::Delete) || i.consume_key(M::NONE, Key::Backspace),
                i.consume_key(M::NONE, Key::Escape),
                i.consume_key(M::NONE, Key::F),
                i.consume_key(M::NONE, Key::Enter),
            )
        });
        if undo {
            self.run("edit.undo", serde_json::Value::Null);
        }
        if redo {
            self.run("edit.redo", serde_json::Value::Null);
        }
        if delete {
            self.delete_selection();
        }
        if escape {
            self.escape();
        }
        if fit {
            self.fit();
        }
        if enter {
            self.finish_draft();
        }
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        let ids = |v: Vec<String>| serde_json::Value::from(v);
        let s = &self.selection;
        let input = serde_json::json!({
            "nodes": ids(s.nodes.iter().map(|id| structurecraft_engine::format_id(*id)).collect()),
            "frames": ids(s.frames.iter().map(|id| structurecraft_engine::format_id(*id)).collect()),
            "areas": ids(s.areas.iter().map(|id| structurecraft_engine::format_id(*id)).collect()),
        });
        if self.run("element.delete", input).is_some() {
            self.selection.clear();
        }
    }

    /// The whole window; the one outer container every host draws.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.shortcuts(ctx);
        self.menu_bar(ctx);
        self.tool_bar(ctx);
        self.status_bar(ctx);
        self.model_panel(ctx);
        self.properties_panel(ctx);
        self.generate_window(ctx);
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::VIEWPORT_BACKGROUND)).show(ctx, |ui| self.viewport(ui));
    }
}

impl eframe::App for StructureCraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}
