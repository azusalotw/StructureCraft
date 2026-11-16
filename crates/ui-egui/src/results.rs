//! Analysis in the UI: Analyze > Run analysis, and the numbers for the selected node or frame
//! in the properties panel. Diagrams on the model come later.

use serde_json::{Value, json};
use structurecraft_engine::{Analysis, AnalysisError, AnalysisState, analyze, describe_analysis_error, format_id};

use crate::StructureCraftApp;

/// The UI shows kN, kN·m, mm and mrad.
const KILO: f64 = 1e3;
const MILLI: f64 = 1e3;

type Outcome = (u64, structurecraft_model::Model, Result<Analysis, AnalysisError>, f64);

#[derive(Default)]
pub(crate) struct ResultsState {
    /// The case or combination shown, by id; the first case when unset.
    of: Option<String>,
    /// A line about the last run, for the status bar.
    pub(crate) summary: String,
    /// The analysis running on another thread (desktop only).
    running: Option<std::sync::mpsc::Receiver<Outcome>>,
}

impl StructureCraftApp {
    /// Analyze > Run analysis (F5): the same solver as `analysis.run`, on a copy of the model.
    /// The desktop app solves on another thread so the window stays live; the browser solves
    /// in place.
    pub(crate) fn run_analysis(&mut self) {
        if self.results.running.is_some() {
            return;
        }
        let revision = self.session.revision();
        let model = self.session.model.clone();
        let solve = move || {
            let start = web_time::Instant::now();
            let result = analyze(&model);
            (revision, model, result, start.elapsed().as_secs_f64())
        };
        self.status.clear();
        if cfg!(target_arch = "wasm32") {
            let outcome = solve();
            self.finish_analysis(outcome);
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("analysis".to_owned()).spawn(move || {
            let _ = send.send(solve());
        });
        match spawned {
            Ok(_) => {
                self.results.running = Some(receive);
                self.results.summary = "Analysing…".to_owned();
            }
            Err(e) => self.status = format!("could not start the analysis: {e}"),
        }
    }

    /// Pick up a finished background analysis; called every frame.
    pub(crate) fn poll_analysis(&mut self, ctx: &egui::Context) {
        let Some(receive) = &self.results.running else { return };
        match receive.try_recv() {
            Ok(outcome) => {
                self.results.running = None;
                self.finish_analysis(outcome);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.results.running = None;
                self.results.summary.clear();
                self.status = "the analysis stopped unexpectedly".to_owned();
            }
        }
    }

    fn finish_analysis(&mut self, (revision, model, result, seconds): Outcome) {
        match result {
            Ok(a) => {
                self.results.summary = format!("Analysed {} load cases, {} degrees of freedom, in {seconds:.1} s", a.cases.len(), a.dofs);
                self.session.set_analysis(revision, a);
            }
            Err(e) => {
                self.results.summary.clear();
                self.status = describe_analysis_error(&model, e).to_string();
            }
        }
    }

    pub(crate) fn analysing(&self) -> bool {
        self.results.running.is_some()
    }

    /// The results of the one selected node or frame.
    pub(crate) fn selection_results(&mut self, ui: &mut egui::Ui) {
        let s = &self.selection;
        let one = s.len() == 1;
        let node = s.nodes.iter().next().copied().filter(|_| one);
        let frame = s.frames.iter().next().copied().filter(|_| one);
        if node.is_none() && frame.is_none() {
            return;
        }
        ui.separator();
        ui.label(egui::RichText::new("Analysis results").strong());
        match self.session.analysis_state() {
            AnalysisState::None => {
                ui.label(egui::RichText::new("Run Analyze > Run analysis (F5) to see results.").weak());
                return;
            }
            AnalysisState::Stale => {
                ui.label(egui::RichText::new("The model changed: run the analysis again.").weak());
                return;
            }
            AnalysisState::Current => {}
        }
        let m = &self.session.model;
        let mut options: Vec<(String, String)> = m.cases().iter().map(|(id, c)| (format_id(id), c.name.clone())).collect();
        options.extend(m.combinations().iter().map(|(id, c)| (format_id(id), c.name.clone())));
        if !self.results.of.as_ref().is_some_and(|of| options.iter().any(|(id, _)| id == of)) {
            self.results.of = options.first().map(|(id, _)| id.clone());
        }
        let Some(of) = self.results.of.clone() else { return };
        let shown = options.iter().find(|(id, _)| *id == of).map_or("", |(_, n)| n.as_str());
        egui::ComboBox::from_id_salt("results_of").selected_text(shown).show_ui(ui, |ui| {
            for (id, name) in &options {
                ui.selectable_value(&mut self.results.of, Some(id.clone()), name);
            }
        });

        let read = |session: &mut structurecraft_engine::Session, input: Value| session.execute("analysis.results", input).ok();
        if let Some(id) = node {
            let n = format_id(id);
            let u = read(&mut self.session, json!({"of": of, "kind": "displacements", "nodes": [n]}));
            let r = read(&mut self.session, json!({"of": of, "kind": "reactions", "nodes": [n]}));
            let rows = [("Displacement (mm)", u.as_ref(), MILLI, ["ux", "uy", "uz"]), ("Rotation (mrad)", u.as_ref(), MILLI, ["rx", "ry", "rz"])];
            egui::Grid::new("node_results").num_columns(4).show(ui, |ui| {
                for (k, (label, out, scale, names)) in rows.iter().enumerate() {
                    values_row(ui, label, *out, k * 3, *scale, names);
                }
                if r.as_ref().is_some_and(|r| r["total"].as_u64() == Some(1)) {
                    values_row(ui, "Reaction (kN)", r.as_ref(), 0, 1.0 / KILO, &["Fx", "Fy", "Fz"]);
                    values_row(ui, "Reaction (kN·m)", r.as_ref(), 3, 1.0 / KILO, &["Mx", "My", "Mz"]);
                }
            });
            if u.as_ref().is_some_and(|u| u["total"].as_u64() == Some(0)) {
                ui.label(egui::RichText::new("No frame reaches this node, so it has no results.").weak());
            }
        }
        if let Some(id) = frame {
            let f = read(&mut self.session, json!({"of": of, "kind": "frame_forces", "frames": [format_id(id)]}));
            egui::Grid::new("frame_results").num_columns(7).striped(true).show(ui, |ui| {
                ui.label("");
                for h in ["P", "V2", "V3", "T", "M2", "M3"] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                for (end, at) in [("End i", 0), ("End j", 6)] {
                    for (key, label) in envelope_keys(f.as_ref()) {
                        ui.label(if label.is_empty() { end.to_owned() } else { format!("{end} {label}") });
                        for k in 0..6 {
                            let v = f.as_ref().and_then(|f| f["items"][0][key][at + k].as_f64());
                            ui.label(v.map_or("–".to_owned(), |v| rounded(v / KILO, 2)));
                        }
                        ui.end_row();
                    }
                }
            });
            ui.label(egui::RichText::new("kN and kN·m, local axes; tension and sagging positive").weak());
        }
    }
}

/// `values` for a case or linear combination, `max` and `min` for an envelope.
fn envelope_keys(out: Option<&Value>) -> Vec<(&'static str, &'static str)> {
    if out.is_some_and(|o| o["items"][0].get("max").is_some()) { vec![("max", "max"), ("min", "min")] } else { vec![("values", "")] }
}

/// One grid row: a label and three values from `out` starting at `at`, times `scale`.
fn values_row(ui: &mut egui::Ui, label: &str, out: Option<&Value>, at: usize, scale: f64, names: &[&str; 3]) {
    for (key, tag) in envelope_keys(out) {
        ui.label(if tag.is_empty() { label.to_owned() } else { format!("{label} {tag}") });
        for (k, name) in names.iter().enumerate() {
            let v = out.and_then(|o| o["items"][0][key][at + k].as_f64());
            ui.label(format!("{name} {}", v.map_or("–".to_owned(), |v| rounded(v * scale, 3))));
        }
        ui.end_row();
    }
}

/// `v` to `decimals` places, without a minus sign on a value that rounds to zero.
fn rounded(v: f64, decimals: usize) -> String {
    let text = format!("{v:.decimals$}");
    if text.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { text.trim_start_matches('-').to_owned() } else { text }
}
