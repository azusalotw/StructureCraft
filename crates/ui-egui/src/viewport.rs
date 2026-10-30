//! The 3D viewport: the grid and previews with the egui painter, the model on the GPU.

use std::sync::Arc;

use structurecraft_geom::{DVec2, DVec3};
use structurecraft_gpu::{SceneBuilder, SceneCallback, Uniforms};
use web_time::Instant;

use crate::tools::Tool;
use crate::{StructureCraftApp, navigation, picking, theme};

/// The ground grid drawn while the model has no grid lines.
const GROUND_HALF_LINES: i32 = 20;
const GROUND_SPACING: f64 = 1.0;
const MAJOR_EVERY: i32 = 5;

impl StructureCraftApp {
    /// Rebuild the GPU scene when the model changed, and the selection overlay when it did.
    fn refresh_scene(&mut self) {
        let revision = self.session.revision();
        let model = &self.session.model;
        if self.scene_revision != Some(revision) {
            let start = Instant::now();
            let origin = model.bounds().center().unwrap_or(DVec3::ZERO);
            self.scene = Arc::new(SceneBuilder { model, palette: theme::SCENE, origin }.whole(revision));
            self.scene_build_ms = start.elapsed().as_secs_f64() * 1e3;
            self.scene_revision = Some(revision);
            self.selection_dirty = true;
        }
        if self.selection_dirty {
            self.selection_version += 1;
            let builder = SceneBuilder { model, palette: theme::SCENE, origin: self.scene.origin };
            self.selection_scene = Arc::new(builder.selected(&self.selection, self.selection_version));
            self.selection_dirty = false;
        }
    }

    pub(crate) fn viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        navigation::handle(ui, &response, &mut self.camera);
        let viewport = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
        if !(viewport.x > 0.0 && viewport.y > 0.0) {
            return;
        }
        self.refresh_scene();

        let painter = ui.painter_at(rect);
        self.draw_grid(&painter, rect, viewport);

        if self.gpu {
            let ppp = f64::from(ui.ctx().pixels_per_point());
            let uniforms =
                Uniforms { view_proj: self.camera.view_projection(viewport), viewport: [(viewport.x * ppp) as f32, (viewport.y * ppp) as f32] };
            let callback = SceneCallback { uniforms, model: self.scene.clone(), selection: self.selection_scene.clone() };
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, callback));
        }

        self.handle_tools(ui, &response, rect);
        self.draw_overlay(&painter, rect, viewport, response.hover_pos());
    }

    fn handle_tools(&mut self, ui: &egui::Ui, response: &egui::Response, rect: egui::Rect) {
        let z = self.elevation();
        let hover = response.hover_pos();
        // Node screen positions are needed to snap while drawing and to pick on a click or a
        // box; orbiting a large model in the select tool never pays for projecting it.
        let picking = response.clicked() || response.drag_stopped_by(egui::PointerButton::Primary);
        if picking || (self.tools.tool != Tool::Select && hover.is_some()) {
            self.cache.update(&self.session.model, &self.camera, rect, self.session.revision());
        }
        self.tools.snap = match (self.tools.tool, hover) {
            (Tool::Select, _) | (_, None) => None,
            (_, Some(p)) => picking::snap(&self.session.model, &self.cache, &self.camera, rect, p, z),
        };
        let shift = ui.input(|i| i.modifiers.shift);
        if self.tools.tool == Tool::Select {
            if response.drag_started_by(egui::PointerButton::Primary) {
                self.tools.box_start = response.interact_pointer_pos();
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let (Some(a), Some(b)) = (self.tools.box_start.take(), response.interact_pointer_pos())
            {
                let found = picking::in_box(&self.session.model, &self.cache, egui::Rect::from_two_pos(a, b));
                if !shift {
                    self.selection.clear();
                }
                self.selection.nodes.extend(found.nodes);
                self.selection.frames.extend(found.frames);
                self.selection.areas.extend(found.areas);
                self.selection_dirty = true;
            }
        }
        if response.double_clicked() && self.tools.tool == Tool::Slab {
            self.finish_draft();
        } else if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            let at = self.tools.snap.map(|(w, _)| w);
            self.click(p, at, shift);
        }
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: egui::Rect, viewport: DVec2) {
        let to_screen = |p: DVec3| self.camera.project(p, viewport).map(|px| rect.min + egui::vec2(px.x as f32, px.y as f32));
        let line = |a: DVec3, b: DVec3, width: f32, colour: egui::Color32| {
            if let (Some(a), Some(b)) = (to_screen(a), to_screen(b)) {
                painter.line_segment([a, b], egui::Stroke::new(width, colour));
            }
        };
        let grid = self.session.model.grid();
        let z = self.elevation();
        if grid.x.is_empty() || grid.y.is_empty() {
            let extent = f64::from(GROUND_HALF_LINES) * GROUND_SPACING;
            for i in -GROUND_HALF_LINES..=GROUND_HALF_LINES {
                if i == 0 {
                    continue;
                }
                let colour = if i % MAJOR_EVERY == 0 { theme::GRID_MAJOR } else { theme::GRID_MINOR };
                let t = f64::from(i) * GROUND_SPACING;
                line(DVec3::new(t, -extent, z), DVec3::new(t, extent, z), 1.0, colour);
                line(DVec3::new(-extent, t, z), DVec3::new(extent, t, z), 1.0, colour);
            }
        } else {
            let (x0, x1) = span(grid.x.iter().map(|l| l.coordinate));
            let (y0, y1) = span(grid.y.iter().map(|l| l.coordinate));
            let overhang = ((x1 - x0).max(y1 - y0) * 0.05).max(1.0);
            let font = egui::FontId::proportional(12.0);
            // Labels closer than this to the last one drawn are skipped, so dense grids stay legible.
            const LABEL_GAP: f32 = 24.0;
            let mut last: Option<egui::Pos2> = None;
            for l in &grid.x {
                let (a, b) = (DVec3::new(l.coordinate, y0 - overhang, z), DVec3::new(l.coordinate, y1 + overhang, z));
                line(a, b, 1.0, theme::GRID_LINE);
                if let Some(s) = to_screen(a)
                    && last.is_none_or(|p| p.distance(s) >= LABEL_GAP)
                {
                    painter.text(s, egui::Align2::CENTER_TOP, &l.label, font.clone(), theme::GRID_LABEL);
                    last = Some(s);
                }
            }
            last = None;
            for l in &grid.y {
                let (a, b) = (DVec3::new(x0 - overhang, l.coordinate, z), DVec3::new(x1 + overhang, l.coordinate, z));
                line(a, b, 1.0, theme::GRID_LINE);
                if let Some(s) = to_screen(a)
                    && last.is_none_or(|p| p.distance(s) >= LABEL_GAP)
                {
                    painter.text(s, egui::Align2::RIGHT_CENTER, &l.label, font.clone(), theme::GRID_LABEL);
                    last = Some(s);
                }
            }
        }
        let axis = GROUND_SPACING * 3.0;
        line(DVec3::ZERO, DVec3::X * axis, 2.0, theme::AXIS_X);
        line(DVec3::ZERO, DVec3::Y * axis, 2.0, theme::AXIS_Y);
        line(DVec3::ZERO, DVec3::Z * axis, 2.0, theme::AXIS_Z);
    }

    fn draw_overlay(&self, painter: &egui::Painter, rect: egui::Rect, viewport: DVec2, hover: Option<egui::Pos2>) {
        let to_screen = |p: DVec3| self.camera.project(p, viewport).map(|px| rect.min + egui::vec2(px.x as f32, px.y as f32));
        let preview = egui::Stroke::new(1.5_f32, theme::PREVIEW);
        let cursor = self.tools.snap.and_then(|(w, _)| to_screen(w));
        let mut points: Vec<egui::Pos2> = self.tools.draft.iter().filter_map(|p| to_screen(*p)).collect();
        if let Some(c) = cursor
            && !points.is_empty()
        {
            points.push(c);
        }
        if self.tools.tool == Tool::Slab && points.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(points.clone(), theme::BOX_SELECT_FILL, preview));
        } else if points.len() >= 2 {
            painter.add(egui::Shape::line(points, preview));
        }
        if let Some((w, snapped)) = self.tools.snap
            && let Some(s) = to_screen(w)
        {
            let colour = if snapped { theme::SNAP } else { theme::PREVIEW };
            painter.circle_stroke(s, if snapped { 6.0 } else { 3.0 }, egui::Stroke::new(1.5_f32, colour));
            if self.tools.tool == Tool::Column {
                let below = self.tools.story.checked_sub(1).and_then(|k| self.session.model.grid().stories.get(k));
                if let Some(base) = below.and_then(|b| to_screen(DVec3::new(w.x, w.y, b.elevation))) {
                    painter.line_segment([base, s], preview);
                }
            }
        }
        if let (Some(a), Some(b)) = (self.tools.box_start, hover) {
            let r = egui::Rect::from_two_pos(a, b);
            painter.rect(r, 0.0, theme::BOX_SELECT_FILL, egui::Stroke::new(1.0_f32, theme::BOX_SELECT_STROKE), egui::StrokeKind::Inside);
        }
    }
}

fn span(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
}
