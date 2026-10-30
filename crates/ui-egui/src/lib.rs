//! The user interface (L5): one egui app for the desktop and the browser.
//!
//! The window is a menu bar, the model tree on the left, properties on the right, a status bar
//! and the 3D viewport in the middle. Every action runs a registry command on the session.

mod navigation;
pub mod theme;
mod viewport;

use structurecraft_engine::Session;
use structurecraft_render::Camera;

pub struct StructureCraftApp {
    session: Session,
    camera: Camera,
    status: String,
}

impl Default for StructureCraftApp {
    fn default() -> Self {
        Self { session: Session::new(), camera: Camera::default(), status: String::new() }
    }
}

impl StructureCraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        Self::default()
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    fn run(&mut self, id: &str, input: serde_json::Value) {
        self.status = match self.session.execute(id, input) {
            Ok(_) => String::new(),
            Err(e) => {
                log::warn!("{id}: {e}");
                e.to_string()
            }
        };
    }

    /// The whole window; the one outer container every host draws.
    pub fn ui(&mut self, ctx: &egui::Context) {
        let (undo, redo) = ctx.input_mut(|i| {
            let redo = i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z))
                || i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y));
            (i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z)), redo)
        });
        if undo {
            self.run("edit.undo", serde_json::Value::Null);
        }
        if redo {
            self.run("edit.redo", serde_json::Value::Null);
        }
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New model").clicked() {
                        self.run("model.new", serde_json::Value::Null);
                        self.camera = Camera::default();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let (undo, redo) = self.session.history();
                    if ui.add_enabled(undo.is_some(), egui::Button::new(format!("Undo {}", undo.unwrap_or_default()))).clicked() {
                        self.run("edit.undo", serde_json::Value::Null);
                        ui.close();
                    }
                    if ui.add_enabled(redo.is_some(), egui::Button::new(format!("Redo {}", redo.unwrap_or_default()))).clicked() {
                        self.run("edit.redo", serde_json::Value::Null);
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Fit all").clicked() {
                        self.camera.fit(&self.session.model.bounds());
                        ui.close();
                    }
                });
            });
        });
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.session.model.name());
                ui.separator();
                let u = self.session.model.display_units();
                ui.label(format!("{:?}, {:?}", u.force, u.length));
                if !self.status.is_empty() {
                    ui.separator();
                    ui.colored_label(theme::ERROR, &self.status);
                }
            });
        });
        egui::SidePanel::left("model_tree").default_width(220.0).show(ctx, |ui| {
            ui.heading("Model");
            let m = &self.session.model;
            egui::Grid::new("model_counts").num_columns(2).show(ui, |ui| {
                for (label, n) in [
                    ("Nodes", m.nodes().len()),
                    ("Frames", m.frames().len()),
                    ("Areas", m.areas().len()),
                    ("Sections", m.sections().len()),
                    ("Materials", m.materials().len()),
                ] {
                    ui.label(label);
                    ui.label(n.to_string());
                    ui.end_row();
                }
            });
        });
        egui::SidePanel::right("properties").default_width(260.0).show(ctx, |ui| {
            ui.heading("Properties");
            ui.label("Select an element to see its properties.");
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::VIEWPORT_BACKGROUND)).show(ctx, |ui| {
            viewport::show(ui, &mut self.camera);
        });
    }
}

impl eframe::App for StructureCraftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}
