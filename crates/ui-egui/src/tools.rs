//! Modeling tools: what the left button does in the viewport.

use serde_json::json;
use structurecraft_geom::DVec3;

use crate::StructureCraftApp;
use crate::picking;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Click to pick, Shift-click to add or remove, drag a box to select what is inside.
    #[default]
    Select,
    /// Click to add a node.
    Node,
    /// Click points in turn; each click after the first draws a frame from the last point.
    Frame,
    /// Click a plan point to draw a column from the story below up to the working story.
    Column,
    /// Click corners; click the first again, double-click or press Enter to close the slab.
    Slab,
}

impl Tool {
    pub const ALL: [Self; 5] = [Self::Select, Self::Node, Self::Frame, Self::Column, Self::Slab];

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Node => "Node",
            Self::Frame => "Beam",
            Self::Column => "Column",
            Self::Slab => "Slab",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Select => "Click to select, Shift-click to add or remove, drag a box to select what is inside it.",
            Self::Node => "Click to add a node on the working story.",
            Self::Frame => "Click points in turn to draw connected beams; Esc starts over.",
            Self::Column => "Click a plan point to draw a column from the story below to the working story.",
            Self::Slab => "Click the corners; click the first corner, double-click or press Enter to close.",
        }
    }
}

#[derive(Default)]
pub struct ToolState {
    pub tool: Tool,
    /// Index of the working story in the grid's stories.
    pub story: usize,
    /// Points placed so far by the frame or slab tool.
    pub draft: Vec<DVec3>,
    pub box_start: Option<egui::Pos2>,
    /// Where the pointer snaps now, and whether to something (a node or grid intersection).
    pub snap: Option<(DVec3, bool)>,
}

fn point(p: DVec3) -> serde_json::Value {
    json!(p.to_array())
}

impl StructureCraftApp {
    /// Elevation of the working story, or 0 without stories.
    pub(crate) fn elevation(&self) -> f64 {
        self.session.model.grid().stories.get(self.tools.story).map_or(0.0, |s| s.elevation)
    }

    pub(crate) fn set_tool(&mut self, tool: Tool) {
        self.tools.tool = tool;
        self.tools.draft.clear();
        self.tools.box_start = None;
    }

    pub(crate) fn escape(&mut self) {
        if !self.tools.draft.is_empty() {
            self.tools.draft.clear();
        } else if self.tools.tool != Tool::Select {
            self.set_tool(Tool::Select);
        } else {
            self.selection.clear();
            self.selection_dirty = true;
        }
    }

    /// Close the slab being drawn.
    pub(crate) fn finish_draft(&mut self) {
        if self.tools.tool == Tool::Slab && self.tools.draft.len() >= 3 {
            let polygon: Vec<_> = self.tools.draft.iter().map(|p| point(*p)).collect();
            self.run("area.draw", json!({"polygons": [polygon]}));
            self.tools.draft.clear();
        }
    }

    /// A left click in the viewport at `p` (screen), with its snapped world point.
    pub(crate) fn click(&mut self, p: egui::Pos2, at: Option<DVec3>, shift: bool) {
        match self.tools.tool {
            Tool::Select => {
                let hit = picking::pick(&self.session.model, &self.cache, p);
                if !shift {
                    self.selection.clear();
                }
                if let Some(hit) = hit {
                    picking::toggle(&mut self.selection, hit);
                }
                self.selection_dirty = true;
            }
            Tool::Node => {
                if let Some(at) = at {
                    self.run("node.add", json!({"points": [point(at)]}));
                }
            }
            Tool::Frame => {
                let Some(at) = at else { return };
                if let Some(start) = self.tools.draft.last().copied() {
                    if start.distance(at) > structurecraft_model::MERGE_TOLERANCE {
                        self.run("frame.draw", json!({"segments": [[point(start), point(at)]]}));
                    }
                    self.tools.draft = vec![at];
                } else {
                    self.tools.draft.push(at);
                }
            }
            Tool::Column => {
                let Some(at) = at else { return };
                let stories = &self.session.model.grid().stories;
                let Some(below) = self.tools.story.checked_sub(1).and_then(|k| stories.get(k)).map(|s| s.elevation) else {
                    self.status = "Columns go up to the working story: pick a story above the base.".to_owned();
                    return;
                };
                let base = DVec3::new(at.x, at.y, below);
                let top = DVec3::new(at.x, at.y, self.elevation());
                self.run("frame.draw", json!({"segments": [[point(base), point(top)]], "role": "column"}));
            }
            Tool::Slab => {
                let Some(at) = at else { return };
                let closes =
                    self.tools.draft.len() >= 3 && self.tools.draft.first().is_some_and(|f| f.distance(at) <= structurecraft_model::MERGE_TOLERANCE);
                if closes {
                    self.finish_draft();
                } else if !self.tools.draft.iter().any(|d| d.distance(at) <= structurecraft_model::MERGE_TOLERANCE) {
                    self.tools.draft.push(at);
                }
            }
        }
    }
}
