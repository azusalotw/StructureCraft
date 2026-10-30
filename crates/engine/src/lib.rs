//! The session and the command registry (L4).
//!
//! Everything a user or agent can do is a command with a JSON input and output, so the desktop
//! app, the browser app, the CLI and the MCP server all run exactly the same code. Commands never
//! open dialogs; front ends ask the user first and pass the answer in.
//!
//! A command that edits the model does so in one [`Tx`]: if any part fails, everything it did is
//! rolled back, and if it succeeds its inverse changes become one undo step.

mod commands;
mod ids;

use serde_json::Value;
use structurecraft_model::{Change, Model, ModelError, Op};

pub use commands::registry;
pub use ids::{ElementRef, format_id, parse_id};

/// Undo steps kept; the oldest is dropped beyond this.
pub const UNDO_LIMIT: usize = 200;

/// One undoable edit: the changes that reverse it, in the order they were made.
#[derive(Debug)]
struct Step {
    label: &'static str,
    changes: Vec<Change>,
}

/// One open model and the state around it.
#[derive(Debug)]
pub struct Session {
    pub model: Model,
    /// Set by any command that changes the model; cleared by new, open and save.
    pub dirty: bool,
    undo: Vec<Step>,
    redo: Vec<Step>,
    revision: u64,
}

/// A command's description, as the UI, `--help` and MCP `tools/list` show it.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Dotted id, `area.verb`, e.g. `model.new`.
    pub id: &'static str,
    /// One sentence for menus, help and agents.
    pub summary: &'static str,
    /// JSON Schema of the input (an object).
    pub input_schema: fn() -> Value,
    pub run: fn(&mut Session, Value) -> Result<Value, CommandError>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("unknown command `{0}`")]
    Unknown(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    File(#[from] structurecraft_io::Error),
    #[error("{0}")]
    Io(String),
    #[error("nothing to {0}")]
    NothingTo(&'static str),
}

impl Default for Session {
    fn default() -> Self {
        Self::with_model(Model::new(commands::UNTITLED))
    }
}

/// `fn $name(rows) -> ids`: apply an `Op::Add` of one kind and return the new rows' ids.
macro_rules! add_fn {
    ($name:ident, $variant:ident, $row:ident) => {
        pub fn $name(
            &mut self,
            rows: Vec<structurecraft_model::$row>,
        ) -> Result<Vec<structurecraft_model::Id<structurecraft_model::$row>>, ModelError> {
            if rows.is_empty() {
                return Ok(Vec::new());
            }
            match self.apply(Change::$variant(Op::Add(rows)))? {
                Change::$variant(Op::Remove(ids)) => Ok(ids.clone()),
                _ => Err(ModelError::Invalid("an add did not return its ids".to_owned())),
            }
        }
    };
}

/// The edits of one command, rolled back if the command fails.
pub struct Tx<'a> {
    model: &'a mut Model,
    inverse: Vec<Change>,
}

impl Tx<'_> {
    pub fn model(&self) -> &Model {
        self.model
    }

    pub fn apply(&mut self, change: Change) -> Result<&Change, ModelError> {
        let inverse = self.model.apply(change)?;
        self.inverse.push(inverse);
        self.inverse.last().ok_or(ModelError::Invalid("lost an edit".to_owned()))
    }

    add_fn!(add_nodes, Nodes, Node);
    add_fn!(add_frames, Frames, Frame);
    add_fn!(add_areas, Areas, Area);
    add_fn!(add_materials, Materials, Material);
    add_fn!(add_sections, Sections, Section);

    fn roll_back(self) {
        for change in self.inverse.into_iter().rev() {
            if let Err(e) = self.model.apply(change) {
                log::error!("could not roll back an edit: {e}");
            }
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_model(model: Model) -> Self {
        Self { model, dirty: false, undo: Vec::new(), redo: Vec::new(), revision: 0 }
    }

    /// Replace the model and forget the undo history.
    pub fn replace_model(&mut self, model: Model) {
        let revision = self.revision + 1;
        *self = Self::with_model(model);
        self.revision = revision;
    }

    /// Changes whenever the model does, so views rebuild only then.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Run a command by id. Hostile input yields an error, never a panic.
    pub fn execute(&mut self, id: &str, input: Value) -> Result<Value, CommandError> {
        let spec = registry().iter().find(|c| c.id == id).ok_or_else(|| CommandError::Unknown(id.to_owned()))?;
        let input = if input.is_null() { Value::Object(Default::default()) } else { input };
        if !input.is_object() {
            return Err(CommandError::InvalidInput("expected a JSON object".to_owned()));
        }
        (spec.run)(self, input)
    }

    /// Run `f` as one undoable edit named `label`.
    pub fn edit<R>(&mut self, label: &'static str, f: impl FnOnce(&mut Tx<'_>) -> Result<R, CommandError>) -> Result<R, CommandError> {
        let mut tx = Tx { model: &mut self.model, inverse: Vec::new() };
        match f(&mut tx) {
            Ok(out) => {
                let changes = tx.inverse;
                if !changes.is_empty() {
                    self.push_undo(Step { label, changes });
                    self.redo.clear();
                    self.dirty = true;
                    self.revision += 1;
                }
                Ok(out)
            }
            Err(e) => {
                tx.roll_back();
                Err(e)
            }
        }
    }

    fn push_undo(&mut self, step: Step) {
        if self.undo.len() >= UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(step);
    }

    /// Apply a step's changes last to first; the result is the step that reverses it.
    fn replay(&mut self, step: Step) -> Result<Step, CommandError> {
        let mut tx = Tx { model: &mut self.model, inverse: Vec::new() };
        for change in step.changes.into_iter().rev() {
            if let Err(e) = tx.apply(change) {
                tx.roll_back();
                return Err(e.into());
            }
        }
        let changes = tx.inverse;
        Ok(Step { label: step.label, changes })
    }

    pub fn undo(&mut self) -> Result<&'static str, CommandError> {
        let step = self.undo.pop().ok_or(CommandError::NothingTo("undo"))?;
        let label = step.label;
        let redo = self.replay(step)?;
        self.redo.push(redo);
        self.dirty = true;
        self.revision += 1;
        Ok(label)
    }

    pub fn redo(&mut self) -> Result<&'static str, CommandError> {
        let step = self.redo.pop().ok_or(CommandError::NothingTo("redo"))?;
        let label = step.label;
        let undo = self.replay(step)?;
        self.push_undo(undo);
        self.dirty = true;
        self.revision += 1;
        Ok(label)
    }

    /// The labels of the next undo and redo steps.
    pub fn history(&self) -> (Option<&'static str>, Option<&'static str>) {
        (self.undo.last().map(|s| s.label), self.redo.last().map(|s| s.label))
    }
}

#[cfg(test)]
mod tests;
