//! Loads in the UI: the patterns, cases and combinations in the model panel, assigning loads to
//! the selection in the properties panel, and the arrows of one pattern in the 3D view.
//!
//! The UI shows kN, kN/m, kN·m and kPa; commands and the model take SI.

use std::sync::Arc;

use serde_json::{Value, json};
use structurecraft_engine::format_id;
use structurecraft_gpu::{Scene, SceneBuilder};
use structurecraft_model::{AreaRole, CombinationKind, FrameLoadShape, LoadDirection, PatternId, PatternKind, Releases, SlabSpan};

use crate::{StructureCraftApp, theme};

const KILO: f64 = 1e3;

/// What the load forms hold between frames.
pub(crate) struct LoadsState {
    /// The pattern whose arrows the 3D view shows.
    pub(crate) shown: Option<PatternId>,
    /// The pattern the properties panel assigns to.
    active: Option<PatternId>,
    node_force: [f64; 3],
    node_moment: [f64; 3],
    frame_value: f64,
    frame_direction: LoadDirection,
    pressure: f64,
    span: SlabSpan,
    new_pattern: String,
    new_kind: PatternKind,
    new_self_weight: f64,
    new_combo: String,
    combo_kind: CombinationKind,
    combo_factors: Vec<f64>,
    pub(crate) scene: Arc<Scene>,
    scene_key: Option<(u64, Option<PatternId>)>,
    /// Bumped on every rebuild, so the renderer uploads the new arrows.
    scene_version: u64,
}

impl Default for LoadsState {
    fn default() -> Self {
        Self {
            shown: None,
            active: None,
            node_force: [0.0, 0.0, -10.0],
            node_moment: [0.0; 3],
            frame_value: 10.0,
            frame_direction: LoadDirection::Gravity,
            pressure: 2.0,
            span: SlabSpan::Auto,
            new_pattern: String::new(),
            new_kind: PatternKind::Live,
            new_self_weight: 0.0,
            new_combo: String::new(),
            combo_kind: CombinationKind::Linear,
            combo_factors: Vec::new(),
            scene: Arc::default(),
            scene_key: None,
            scene_version: 0,
        }
    }
}

fn direction_label(d: LoadDirection) -> &'static str {
    match d {
        LoadDirection::Gravity => "Gravity (down)",
        LoadDirection::X => "Global X",
        LoadDirection::Y => "Global Y",
        LoadDirection::Z => "Global Z",
        LoadDirection::Local1 => "Local 1",
        LoadDirection::Local2 => "Local 2",
        LoadDirection::Local3 => "Local 3",
    }
}

fn span_label(s: SlabSpan) -> &'static str {
    match s {
        SlabSpan::Auto => "Auto",
        SlabSpan::OneWay => "One-way",
        SlabSpan::TwoWay => "Two-way",
    }
}

fn span_name(s: SlabSpan) -> &'static str {
    match s {
        SlabSpan::Auto => "auto",
        SlabSpan::OneWay => "one_way",
        SlabSpan::TwoWay => "two_way",
    }
}

fn direction_name(d: LoadDirection) -> &'static str {
    match d {
        LoadDirection::Gravity => "gravity",
        LoadDirection::X => "x",
        LoadDirection::Y => "y",
        LoadDirection::Z => "z",
        LoadDirection::Local1 => "local1",
        LoadDirection::Local2 => "local2",
        LoadDirection::Local3 => "local3",
    }
}

fn kind_name(k: PatternKind) -> Value {
    serde_json::to_value(k).unwrap_or(Value::Null)
}

impl StructureCraftApp {
    /// Rebuild the arrows when the model or the shown pattern changed.
    pub(crate) fn refresh_loads_scene(&mut self) {
        let m = &self.session.model;
        if self.loads.shown.is_some_and(|p| !m.patterns().contains(p)) {
            self.loads.shown = None;
        }
        let key = (self.session.revision(), self.loads.shown);
        if self.loads.scene_key == Some(key) {
            return;
        }
        let builder = SceneBuilder { model: m, palette: theme::SCENE, origin: self.scene.origin };
        self.loads.scene_version += 1;
        let version = self.loads.scene_version;
        self.loads.scene = Arc::new(match self.loads.shown {
            Some(p) => builder.loads(p, version),
            None => Scene { version, origin: self.scene.origin, ..Scene::default() },
        });
        self.loads.scene_key = Some(key);
    }

    /// The "Show loads" picker in the tool bar.
    pub(crate) fn shown_pattern_picker(&mut self, ui: &mut egui::Ui) {
        let m = &self.session.model;
        let name = |p: Option<PatternId>| p.and_then(|p| m.patterns().get(p)).map_or("None".to_owned(), |p| p.name.clone());
        let mut shown = self.loads.shown;
        egui::ComboBox::from_label("Loads").selected_text(name(shown)).show_ui(ui, |ui| {
            ui.selectable_value(&mut shown, None, "None");
            for (id, p) in m.patterns().iter() {
                ui.selectable_value(&mut shown, Some(id), &p.name);
            }
        });
        if shown != self.loads.shown {
            self.loads.shown = shown;
            // Assign to what is on screen.
            if shown.is_some() {
                self.loads.active = shown;
            }
        }
    }

    /// Patterns, cases and combinations, in the model panel.
    pub(crate) fn loads_tree(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Load patterns", |ui| {
            let rows: Vec<_> = self.session.model.patterns().iter().map(|(id, p)| (id, p.name.clone(), p.kind, p.self_weight)).collect();
            let mut delete = None;
            egui::Grid::new("patterns").num_columns(3).show(ui, |ui| {
                for (id, name, kind, self_weight) in &rows {
                    ui.label(name);
                    let sw = if *self_weight != 0.0 { format!("{}, self-weight ×{self_weight}", kind.label()) } else { kind.label().to_owned() };
                    ui.label(egui::RichText::new(sw).weak());
                    if ui.small_button("×").on_hover_text("Delete the pattern and its loads").clicked() {
                        delete = Some(*id);
                    }
                    ui.end_row();
                }
            });
            if let Some(id) = delete {
                self.run("load.delete", json!({"patterns": [format_id(id)]}));
            }
            ui.separator();
            let s = &mut self.loads;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.new_pattern).hint_text("New pattern").desired_width(100.0));
                egui::ComboBox::from_id_salt("new_kind").selected_text(s.new_kind.label()).show_ui(ui, |ui| {
                    for k in PatternKind::ALL {
                        ui.selectable_value(&mut s.new_kind, k, k.label());
                    }
                });
            });
            let mut add = false;
            ui.horizontal(|ui| {
                ui.label("Self-weight ×");
                ui.add(egui::DragValue::new(&mut s.new_self_weight).range(0.0..=10.0).speed(0.1));
                add = ui.add_enabled(!s.new_pattern.trim().is_empty(), egui::Button::new("Add")).clicked();
            });
            if add {
                let input = json!({"name": s.new_pattern.trim(), "kind": kind_name(s.new_kind), "self_weight": s.new_self_weight});
                if self.run("load.pattern.add", input).is_some() {
                    self.loads.new_pattern.clear();
                }
            }
        });
        ui.collapsing("Load cases", |ui| {
            let m = &self.session.model;
            let pattern = |p: PatternId| m.patterns().get(p).map_or("?", |p| p.name.as_str());
            let rows: Vec<_> = m
                .cases()
                .iter()
                .map(|(id, c)| {
                    let terms: Vec<String> = c.patterns.iter().map(|(p, f)| format!("{f}×{}", pattern(*p))).collect();
                    (id, c.name.clone(), terms.join(" + "))
                })
                .collect();
            let mut delete = None;
            egui::Grid::new("cases").num_columns(3).show(ui, |ui| {
                for (id, name, terms) in &rows {
                    ui.label(name);
                    ui.label(egui::RichText::new(terms).weak());
                    if ui.small_button("×").on_hover_text("Delete the case").clicked() {
                        delete = Some(*id);
                    }
                    ui.end_row();
                }
            });
            if let Some(id) = delete {
                self.run("load.delete", json!({"cases": [format_id(id)]}));
            }
        });
        ui.collapsing("Load combinations", |ui| self.combinations_tree(ui));
    }

    fn combinations_tree(&mut self, ui: &mut egui::Ui) {
        let m = &self.session.model;
        let case_name = |k| m.cases().get(k).map_or("?".to_owned(), |c| c.name.clone());
        let rows: Vec<_> = m
            .combinations()
            .iter()
            .map(|(id, c)| {
                let join = if c.kind == CombinationKind::Envelope { " | " } else { " + " };
                let terms: Vec<String> = c.cases.iter().map(|(k, f)| format!("{f}×{}", case_name(*k))).collect();
                (id, c.name.clone(), terms.join(join))
            })
            .collect();
        let cases: Vec<_> = m.cases().iter().map(|(id, c)| (id, c.name.clone())).collect();
        let mut delete = None;
        egui::Grid::new("combinations").num_columns(3).show(ui, |ui| {
            for (id, name, terms) in &rows {
                ui.label(name);
                ui.label(egui::RichText::new(terms).weak());
                if ui.small_button("×").on_hover_text("Delete the combination").clicked() {
                    delete = Some(*id);
                }
                ui.end_row();
            }
        });
        if let Some(id) = delete {
            self.run("load.delete", json!({"combinations": [format_id(id)]}));
        }
        ui.separator();
        let s = &mut self.loads;
        s.combo_factors.resize(cases.len(), 0.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.new_combo).hint_text("New combination").desired_width(110.0));
            egui::ComboBox::from_id_salt("combo_kind").selected_text(format!("{:?}", s.combo_kind)).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.combo_kind, CombinationKind::Linear, "Linear");
                ui.selectable_value(&mut s.combo_kind, CombinationKind::Envelope, "Envelope");
            });
        });
        egui::Grid::new("combo_factors").num_columns(2).show(ui, |ui| {
            for ((_, name), factor) in cases.iter().zip(s.combo_factors.iter_mut()) {
                ui.label(name);
                ui.add(egui::DragValue::new(factor).range(-100.0..=100.0).speed(0.05).max_decimals(3));
                ui.end_row();
            }
        });
        let terms: Vec<Value> =
            cases.iter().zip(&s.combo_factors).filter(|(_, f)| **f != 0.0).map(|((id, _), f)| json!({"case": format_id(*id), "factor": f})).collect();
        let ready = !s.new_combo.trim().is_empty() && !terms.is_empty();
        if ui.add_enabled(ready, egui::Button::new("Add combination")).on_hover_text("Cases with a factor other than 0").clicked() {
            let kind = if s.combo_kind == CombinationKind::Envelope { "envelope" } else { "linear" };
            let input = json!({"name": s.new_combo.trim(), "kind": kind, "cases": terms});
            if self.run("load.combination.set", input).is_some() {
                self.loads.new_combo.clear();
                self.loads.combo_factors.iter_mut().for_each(|f| *f = 0.0);
            }
        }
    }

    /// Loads and releases for the selection, in the properties panel.
    pub(crate) fn selection_loads(&mut self, ui: &mut egui::Ui) {
        let s = self.selection.clone();
        let m = &self.session.model;
        if !s.frames.is_empty() {
            ui.separator();
            ui.label(egui::RichText::new("End releases").strong());
            let ends: Vec<Releases> = s.frames.iter().filter_map(|id| m.frames().get(*id)).map(|f| f.releases).collect();
            let frames: Vec<String> = s.frames.iter().map(|id| format_id(*id)).collect();
            let mut set = None;
            ui.horizontal(|ui| {
                for (end, key) in [(0, "release_i"), (1, "release_j")] {
                    let bits = |r: &Releases| if end == 0 { r.i() } else { r.j() };
                    let all = ends.iter().all(|r| bits(r) & Releases::MOMENTS == Releases::MOMENTS);
                    let mut pinned = all;
                    let label = if end == 0 { "Pinned at i" } else { "Pinned at j" };
                    if ui.checkbox(&mut pinned, label).on_hover_text("Releases both bending moments at that end").changed() {
                        set = Some((key, if pinned { "pinned" } else { "none" }));
                    }
                }
            });
            if let Some((key, value)) = set {
                self.run("frame.set", json!({"frames": frames, key: value}));
            }
        }

        let m = &self.session.model;
        if m.patterns().is_empty() {
            return;
        }
        if self.loads.active.is_none_or(|p| !m.patterns().contains(p)) {
            self.loads.active = self.loads.shown.or_else(|| m.patterns().ids().next());
        }
        ui.separator();
        ui.label(egui::RichText::new("Loads").strong());
        let name = |p: Option<PatternId>| p.and_then(|p| m.patterns().get(p)).map_or(String::new(), |p| p.name.clone());
        let patterns: Vec<_> = m.patterns().iter().map(|(id, p)| (id, p.name.clone())).collect();
        let mut active = self.loads.active;
        ui.horizontal(|ui| {
            ui.label("Pattern");
            egui::ComboBox::from_id_salt("active_pattern").selected_text(name(active)).show_ui(ui, |ui| {
                for (id, n) in &patterns {
                    ui.selectable_value(&mut active, Some(*id), n);
                }
            });
        });
        self.loads.active = active;
        let Some(pattern) = active else { return };
        // Assigning shows what was assigned.
        let mut ran = false;
        let pattern_ref = format_id(pattern);

        if !s.nodes.is_empty() {
            let st = &mut self.loads;
            ui.label(format!("On {} nodes", s.nodes.len()));
            egui::Grid::new("node_load").num_columns(4).show(ui, |ui| {
                ui.label("F (kN)");
                for v in &mut st.node_force {
                    ui.add(egui::DragValue::new(v).speed(0.5).max_decimals(3));
                }
                ui.end_row();
                ui.label("M (kN·m)");
                for v in &mut st.node_moment {
                    ui.add(egui::DragValue::new(v).speed(0.5).max_decimals(3));
                }
                ui.end_row();
            });
            let nodes: Vec<String> = s.nodes.iter().map(|id| format_id(*id)).collect();
            let (force, moment) = (st.node_force.map(|v| v * KILO), st.node_moment.map(|v| v * KILO));
            let (assign, remove) = buttons(ui, "node");
            if assign {
                ran |= self.run("load.node", json!({"nodes": nodes, "pattern": pattern_ref, "force": force, "moment": moment})).is_some();
            } else if remove {
                ran |= self.run("load.node", json!({"nodes": nodes, "pattern": pattern_ref, "mode": "delete"})).is_some();
            }
        }
        if !s.frames.is_empty() {
            let st = &mut self.loads;
            ui.label(format!("On {} frames", s.frames.len()));
            ui.horizontal(|ui| {
                ui.label("Uniform (kN/m)");
                ui.add(egui::DragValue::new(&mut st.frame_value).speed(0.5).max_decimals(3));
                egui::ComboBox::from_id_salt("frame_direction").selected_text(direction_label(st.frame_direction)).show_ui(ui, |ui| {
                    for d in LoadDirection::ALL {
                        ui.selectable_value(&mut st.frame_direction, d, direction_label(d));
                    }
                });
            });
            let frames: Vec<String> = s.frames.iter().map(|id| format_id(*id)).collect();
            let (value, direction) = (st.frame_value * KILO, direction_name(st.frame_direction));
            let (assign, remove) = buttons(ui, "frame");
            if assign {
                let input = json!({"frames": frames, "pattern": pattern_ref, "direction": direction, "value": value});
                ran |= self.run("load.frame", input).is_some();
            } else if remove {
                ran |= self.run("load.frame", json!({"frames": frames, "pattern": pattern_ref, "mode": "delete"})).is_some();
            }
        }
        let m = &self.session.model;
        let slabs: Vec<String> =
            s.areas.iter().filter(|id| m.areas().get(**id).is_some_and(|a| a.role == AreaRole::Slab)).map(|id| format_id(*id)).collect();
        if !slabs.is_empty() {
            let st = &mut self.loads;
            ui.label(format!("On {} slabs", slabs.len()));
            ui.horizontal(|ui| {
                ui.label("Pressure (kPa)");
                ui.add(egui::DragValue::new(&mut st.pressure).speed(0.1).max_decimals(3));
                egui::ComboBox::from_id_salt("span").selected_text(span_label(st.span)).show_ui(ui, |ui| {
                    for v in SlabSpan::ALL {
                        ui.selectable_value(&mut st.span, v, span_label(v));
                    }
                });
            });
            let (pressure, span) = (st.pressure * KILO, span_name(st.span));
            let (assign, remove) = buttons(ui, "area");
            if assign {
                ran |= self.run("load.area", json!({"areas": slabs, "pattern": pattern_ref, "pressure": pressure, "span": span})).is_some();
            } else if remove {
                ran |= self.run("load.area", json!({"areas": slabs, "pattern": pattern_ref, "mode": "delete"})).is_some();
            }
        }
        if ran {
            self.loads.shown = Some(pattern);
        }
        self.selection_load_summary(ui, pattern);
    }

    /// What the selection already carries in the active pattern.
    fn selection_load_summary(&self, ui: &mut egui::Ui, pattern: PatternId) {
        let m = &self.session.model;
        let s = &self.selection;
        if s.len() > 1 {
            return;
        }
        let nodes: Vec<_> = s.nodes.iter().copied().collect();
        let frames: Vec<_> = s.frames.iter().copied().collect();
        let areas: Vec<_> = s.areas.iter().copied().collect();
        let on = m.loads_on(&nodes, &frames, &areas, Some(pattern));
        for id in on.node_loads {
            if let Some(l) = m.node_loads().get(id) {
                let v = l.values.map(|x| x / KILO);
                ui.label(format!("F ({:.2}, {:.2}, {:.2}) kN  M ({:.2}, {:.2}, {:.2}) kN·m", v[0], v[1], v[2], v[3], v[4], v[5]));
            }
        }
        for id in on.frame_loads {
            if let Some(l) = m.frame_loads().get(id) {
                let d = direction_label(l.direction);
                ui.label(match l.shape {
                    FrameLoadShape::Distributed if l.value_start == l.value_end && l.start == 0.0 && l.end == 1.0 => {
                        format!("{:.2} kN/m, {d}", l.value_start / KILO)
                    }
                    FrameLoadShape::Distributed => {
                        format!("{:.2} to {:.2} kN/m from {:.2} to {:.2}, {d}", l.value_start / KILO, l.value_end / KILO, l.start, l.end)
                    }
                    FrameLoadShape::Point => format!("{:.2} kN at {:.2}, {d}", l.value_start / KILO, l.start),
                });
            }
        }
        for id in on.area_loads {
            if let Some(l) = m.area_loads().get(id) {
                ui.label(format!("{:.2} kPa, {}", l.pressure / KILO, span_label(l.span)));
            }
        }
    }
}

/// "Assign" and "Remove" for one kind of element.
fn buttons(ui: &mut egui::Ui, salt: &str) -> (bool, bool) {
    ui.push_id(salt, |ui| {
        ui.horizontal(|ui| {
            let assign = ui.button("Assign").on_hover_text("Replace this pattern's load on the selection").clicked();
            let remove = ui.button("Remove").on_hover_text("Remove this pattern's loads from the selection").clicked();
            (assign, remove)
        })
        .inner
    })
    .inner
}
