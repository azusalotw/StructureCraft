//! The panels around the viewport: menus, tools, model, properties, status.

use serde_json::{Value, json};
use structurecraft_engine::format_id;
use structurecraft_model::{FrameRole, Restraint, SectionId};

use crate::navigation::{self, Preset};
use crate::tools::Tool;
use crate::{StructureCraftApp, theme};

/// The Generate building window's fields.
pub struct GenerateForm {
    bays_x: u32,
    bays_y: u32,
    stories: u32,
    bay_x: f64,
    bay_y: f64,
    story_height: f64,
    slabs: bool,
}

impl Default for GenerateForm {
    fn default() -> Self {
        Self { bays_x: 4, bays_y: 3, stories: 5, bay_x: 8.0, bay_y: 8.0, story_height: 3.5, slabs: true }
    }
}

fn restraint_name(r: Restraint) -> &'static str {
    match r {
        Restraint::FREE => "free",
        Restraint::PINNED => "pinned",
        Restraint::FIXED => "fixed",
        _ => "custom",
    }
}

impl StructureCraftApp {
    pub(crate) fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New model").clicked() {
                        self.run("model.new", Value::Null);
                        self.selection.clear();
                        self.tools.story = 0;
                        self.camera = Default::default();
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    let (undo, redo) = self.session.history();
                    if ui.add_enabled(undo.is_some(), egui::Button::new(format!("Undo {}", undo.unwrap_or_default()))).clicked() {
                        self.run("edit.undo", Value::Null);
                        ui.close();
                    }
                    if ui.add_enabled(redo.is_some(), egui::Button::new(format!("Redo {}", redo.unwrap_or_default()))).clicked() {
                        self.run("edit.redo", Value::Null);
                        ui.close();
                    }
                    ui.separator();
                    if ui.add_enabled(!self.selection.is_empty(), egui::Button::new("Delete selected")).clicked() {
                        self.delete_selection();
                        ui.close();
                    }
                });
                ui.menu_button("Model", |ui| {
                    if ui.button("Generate building…").clicked() {
                        self.generate = Some(GenerateForm::default());
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Fit all (F)").clicked() {
                        self.fit();
                        ui.close();
                    }
                    for (label, view) in [("Plan", Preset::Plan), ("Front", Preset::Front), ("Side", Preset::Side), ("3D", Preset::Iso)] {
                        if ui.button(label).clicked() {
                            navigation::preset(&mut self.camera, view);
                            ui.close();
                        }
                    }
                });
            });
        });
    }

    pub(crate) fn tool_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tool_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for tool in Tool::ALL {
                    if ui.selectable_label(self.tools.tool == tool, tool.label()).on_hover_text(tool.hint()).clicked() {
                        self.set_tool(tool);
                    }
                }
                ui.separator();
                let stories = self.session.model.grid().stories.clone();
                if stories.is_empty() {
                    ui.label("Working plane: z = 0");
                } else {
                    self.tools.story = self.tools.story.min(stories.len() - 1);
                    let current = stories.get(self.tools.story).map(|s| format!("{} ({:.2} m)", s.name, s.elevation)).unwrap_or_default();
                    egui::ComboBox::from_label("Story").selected_text(current).show_ui(ui, |ui| {
                        for (k, s) in stories.iter().enumerate().rev() {
                            ui.selectable_value(&mut self.tools.story, k, format!("{} ({:.2} m)", s.name, s.elevation));
                        }
                    });
                }
                ui.separator();
                for (label, view) in [("Plan", Preset::Plan), ("Front", Preset::Front), ("Side", Preset::Side), ("3D", Preset::Iso)] {
                    if ui.button(label).clicked() {
                        navigation::preset(&mut self.camera, view);
                    }
                }
                if ui.button("Fit").clicked() {
                    self.fit();
                }
            });
        });
    }

    pub(crate) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.session.model.name());
                ui.separator();
                let u = self.session.model.display_units();
                ui.label(format!("{:?}, {:?}", u.force, u.length));
                ui.separator();
                ui.label(self.tools.tool.hint());
                if let Some((w, _)) = self.tools.snap {
                    ui.separator();
                    ui.label(format!("x {:.3}  y {:.3}  z {:.3} m", w.x, w.y, w.z));
                }
                if !self.status.is_empty() {
                    ui.separator();
                    ui.colored_label(theme::ERROR, &self.status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("scene {:.1} ms", self.scene_build_ms)).on_hover_text("Time the last rebuild of the GPU buffers took");
                });
            });
        });
    }

    pub(crate) fn model_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("model_tree").default_width(220.0).show(ctx, |ui| {
            ui.heading("Model");
            let m = &self.session.model;
            egui::Grid::new("model_counts").num_columns(2).show(ui, |ui| {
                for (label, n) in [("Nodes", m.nodes().len()), ("Frames", m.frames().len()), ("Areas", m.areas().len())] {
                    ui.label(label);
                    ui.label(n.to_string());
                    ui.end_row();
                }
            });
            ui.separator();
            ui.collapsing("Sections", |ui| {
                for (_, s) in m.sections().iter() {
                    ui.label(&s.name);
                }
            });
            ui.collapsing("Materials", |ui| {
                for (_, mat) in m.materials().iter() {
                    ui.label(format!("{}  E = {:.1} GPa", mat.name, mat.elastic_modulus / 1e9));
                }
            });
            ui.collapsing("Stories", |ui| {
                for s in m.grid().stories.iter().rev() {
                    ui.label(format!("{}  {:.2} m", s.name, s.elevation));
                }
            });
        });
    }

    fn section_picker(&mut self, ui: &mut egui::Ui, current: Option<SectionId>, shell: bool) -> Option<SectionId> {
        let sections: Vec<(SectionId, String)> =
            self.session.model.sections().iter().filter(|(_, s)| s.shape.is_shell() == shell).map(|(id, s)| (id, s.name.clone())).collect();
        let text = current.and_then(|c| sections.iter().find(|(id, _)| *id == c)).map_or("(mixed)".to_owned(), |(_, n)| n.clone());
        let mut picked = None;
        egui::ComboBox::from_id_salt(("section", shell)).selected_text(text).show_ui(ui, |ui| {
            for (id, name) in &sections {
                if ui.selectable_label(current == Some(*id), name).clicked() {
                    picked = Some(*id);
                }
            }
        });
        picked
    }

    pub(crate) fn properties_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties").default_width(260.0).show(ctx, |ui| {
            ui.heading("Properties");
            if self.selection.is_empty() {
                ui.label("Select elements to see and change their properties.");
                return;
            }
            let s = self.selection.clone();
            ui.label(format!("{} nodes, {} frames, {} areas selected", s.nodes.len(), s.frames.len(), s.areas.len()));
            let m = &self.session.model;
            if s.len() == 1 {
                ui.separator();
                if let Some(id) = s.nodes.iter().next()
                    && let Some(n) = m.nodes().get(*id)
                {
                    ui.label(format!("Node {}", format_id(*id)));
                    ui.label(format!("x {:.3}  y {:.3}  z {:.3} m", n.position.x, n.position.y, n.position.z));
                    ui.label(format!("Used by {} frame ends and area corners", m.node_uses(*id)));
                }
                if let Some(id) = s.frames.iter().next()
                    && let Some(f) = m.frames().get(*id)
                    && let (Some(a), Some(b)) = (m.nodes().get(f.i), m.nodes().get(f.j))
                {
                    ui.label(format!("Frame {}", format_id(*id)));
                    ui.label(format!("Length {:.3} m", a.position.distance(b.position)));
                }
                if let Some(id) = s.areas.iter().next()
                    && let Some(a) = m.areas().get(*id)
                {
                    ui.label(format!("Area {} with {} corners", format_id(*id), a.corners.len()));
                }
            }
            let ids = |v: Vec<String>| Value::from(v);
            if !s.nodes.is_empty() {
                ui.separator();
                let current: std::collections::HashSet<_> =
                    s.nodes.iter().filter_map(|id| m.nodes().get(*id)).map(|n| restraint_name(n.restraint)).collect();
                let text = if current.len() == 1 { current.iter().next().copied().unwrap_or("free") } else { "(mixed)" };
                let mut picked = None;
                ui.horizontal(|ui| {
                    ui.label("Support");
                    egui::ComboBox::from_id_salt("restraint").selected_text(text).show_ui(ui, |ui| {
                        for name in ["free", "pinned", "fixed"] {
                            if ui.selectable_label(text == name, name).clicked() {
                                picked = Some(name);
                            }
                        }
                    });
                });
                if let Some(name) = picked {
                    let nodes = ids(s.nodes.iter().map(|id| format_id(*id)).collect());
                    self.run("node.restrain", json!({"nodes": nodes, "restraint": name}));
                }
            }
            if !s.frames.is_empty() {
                ui.separator();
                let m = &self.session.model;
                let sections: std::collections::HashSet<_> = s.frames.iter().filter_map(|id| m.frames().get(*id)).map(|f| f.section).collect();
                let roles: std::collections::HashSet<_> = s.frames.iter().filter_map(|id| m.frames().get(*id)).map(|f| f.role).collect();
                let current = (sections.len() == 1).then(|| sections.iter().next().copied()).flatten();
                let frames = ids(s.frames.iter().map(|id| format_id(*id)).collect());
                let mut picked = None;
                ui.horizontal(|ui| {
                    ui.label("Frame section");
                    picked = self.section_picker(ui, current, false);
                });
                if let Some(section) = picked {
                    self.run("frame.set", json!({"frames": frames.clone(), "section": format_id(section)}));
                }
                let role_text = if roles.len() == 1 { roles.iter().next().map_or("", |r| role_name(*r)) } else { "(mixed)" };
                let mut role = None;
                ui.horizontal(|ui| {
                    ui.label("Role");
                    egui::ComboBox::from_id_salt("role").selected_text(role_text).show_ui(ui, |ui| {
                        for r in FrameRole::ALL {
                            if ui.selectable_label(role_text == role_name(r), role_name(r)).clicked() {
                                role = Some(r);
                            }
                        }
                    });
                });
                if let Some(r) = role {
                    self.run("frame.set", json!({"frames": frames, "role": role_name(r)}));
                }
            }
            if !s.areas.is_empty() {
                ui.separator();
                let m = &self.session.model;
                let sections: std::collections::HashSet<_> = s.areas.iter().filter_map(|id| m.areas().get(*id)).map(|a| a.section).collect();
                let current = (sections.len() == 1).then(|| sections.iter().next().copied()).flatten();
                let mut picked = None;
                ui.horizontal(|ui| {
                    ui.label("Area section");
                    picked = self.section_picker(ui, current, true);
                });
                if let Some(section) = picked {
                    let areas = ids(s.areas.iter().map(|id| format_id(*id)).collect());
                    self.run("area.set", json!({"areas": areas, "section": format_id(section)}));
                }
            }
            ui.separator();
            if ui.button("Delete selected").clicked() {
                self.delete_selection();
            }
        });
    }

    pub(crate) fn generate_window(&mut self, ctx: &egui::Context) {
        let Some(form) = self.generate.as_mut() else { return };
        let mut open = true;
        let mut go = false;
        egui::Window::new("Generate building").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("generate").num_columns(2).show(ui, |ui| {
                ui.label("Bays along X");
                ui.add(egui::DragValue::new(&mut form.bays_x).range(1..=500));
                ui.end_row();
                ui.label("Bays along Y");
                ui.add(egui::DragValue::new(&mut form.bays_y).range(1..=500));
                ui.end_row();
                ui.label("Stories");
                ui.add(egui::DragValue::new(&mut form.stories).range(1..=300));
                ui.end_row();
                ui.label("Bay width X (m)");
                ui.add(egui::DragValue::new(&mut form.bay_x).range(0.5..=100.0).speed(0.1));
                ui.end_row();
                ui.label("Bay width Y (m)");
                ui.add(egui::DragValue::new(&mut form.bay_y).range(0.5..=100.0).speed(0.1));
                ui.end_row();
                ui.label("Story height (m)");
                ui.add(egui::DragValue::new(&mut form.story_height).range(1.0..=20.0).speed(0.05));
                ui.end_row();
                ui.label("Slabs");
                ui.checkbox(&mut form.slabs, "");
                ui.end_row();
            });
            let nodes = (u64::from(form.bays_x) + 1) * (u64::from(form.bays_y) + 1) * (u64::from(form.stories) + 1);
            ui.label(format!("{nodes} nodes"));
            go = ui.button("Generate").clicked();
        });
        if go {
            let f = self.generate.take().unwrap_or_default();
            let input = json!({
                "bays_x": f.bays_x, "bays_y": f.bays_y, "stories": f.stories,
                "bay_x": f.bay_x, "bay_y": f.bay_y, "story_height": f.story_height, "slabs": f.slabs,
            });
            if self.run("generate.building", input).is_some() {
                self.tools.story = self.session.model.grid().stories.len().saturating_sub(1);
                self.fit();
            }
        } else if !open {
            self.generate = None;
        }
    }
}

fn role_name(r: FrameRole) -> &'static str {
    match r {
        FrameRole::Beam => "beam",
        FrameRole::Column => "column",
        FrameRole::Brace => "brace",
    }
}
