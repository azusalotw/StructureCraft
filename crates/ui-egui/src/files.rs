//! File > New, Open, Save and Save as, with a "save changes?" guard and the window title.
//!
//! The desktop uses native dialogs and runs `model.open` / `model.save` on the chosen path. The
//! browser has no file system: its pickers hand over bytes a frame or more later, through a
//! channel, and the session reads and writes them with `open_bytes` / `save_bytes`.

use serde_json::Value;

use crate::StructureCraftApp;

const FILTER_NAME: &str = "StructureCraft model";
const EXTENSION: &str = "stc";

/// What waits on the unsaved-changes question.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    New,
    Open,
    Quit,
}

#[derive(Default)]
pub(crate) struct Files {
    /// The open file: a path on the desktop, a file name in the browser.
    pub(crate) current: Option<String>,
    /// The action the "save changes?" window asks about.
    pending: Option<Action>,
    /// Set once the user agreed to quit, so the next close request goes through.
    quitting: bool,
    title: String,
    #[cfg(target_arch = "wasm32")]
    inbox: web::Inbox,
}

impl Files {
    /// Whether the "save changes?" window is up (it takes the keyboard).
    pub(crate) fn asking(&self) -> bool {
        self.pending.is_some()
    }
}

/// `name.stc` for a model name, for the save dialog.
fn default_file_name(model_name: &str) -> String {
    let stem: String = model_name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    let stem = stem.trim();
    format!("{}.{EXTENSION}", if stem.is_empty() { "model" } else { stem })
}

/// The last part of a path, for the title bar.
fn display_name(current: &str) -> &str {
    current.rsplit(['/', '\\']).next().unwrap_or(current)
}

impl StructureCraftApp {
    /// New, Open or Quit, after asking about unsaved changes.
    pub(crate) fn request(&mut self, action: Action) {
        if self.session.dirty {
            self.files.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Action) {
        match action {
            Action::New => self.new_model(),
            Action::Open => self.open_dialog(),
            Action::Quit => self.files.quitting = true,
        }
    }

    fn new_model(&mut self) {
        if self.run("model.new", Value::Null).is_some() {
            self.selection.clear();
            self.tools.story = 0;
            self.camera = Default::default();
            self.files.current = None;
        }
    }

    /// After a model is read: forget the old selection and frame the new model.
    pub(crate) fn opened(&mut self, current: String) {
        self.selection.clear();
        self.tools.story = self.session.model.grid().stories.len().saturating_sub(1);
        self.fit();
        self.files.current = Some(current);
    }

    /// File > Save: to the open file, or through the dialog for a new model.
    pub(crate) fn save(&mut self, then: Option<Action>) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.files.current.clone() {
            if self.save_to(&path) {
                self.then(then);
            }
            return;
        }
        self.save_as(then);
    }

    fn then(&mut self, then: Option<Action>) {
        if let Some(action) = then {
            self.perform(action);
        }
    }

    /// Before the panels: messages from the browser's pickers, the close guard, the title and
    /// the "save changes?" window.
    pub(crate) fn files_frame(&mut self, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        self.web_inbox(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.session.dirty && !self.files.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.files.pending = Some(Action::Quit);
        }
        if self.files.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let name = self.files.current.as_deref().map_or_else(|| self.session.model.name().to_owned(), |c| display_name(c).to_owned());
        let title = format!("{name}{} - StructureCraft", if self.session.dirty { "*" } else { "" });
        if title != self.files.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.files.title = title;
        }
        self.save_changes_window(ctx);
    }

    fn save_changes_window(&mut self, ctx: &egui::Context) {
        let Some(action) = self.files.pending else { return };
        let name = self.files.current.as_deref().map_or_else(|| self.session.model.name().to_owned(), |c| display_name(c).to_owned());
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("save_changes")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Save changes?");
            ui.label(format!("“{name}” has changes that are not saved."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.files.pending = None;
                }
            });
        });
        if modal.should_close() {
            self.files.pending = None;
        }
        match choice {
            Some(true) => {
                self.files.pending = None;
                self.save(Some(action));
            }
            Some(false) => {
                self.files.pending = None;
                self.perform(action);
            }
            None => {}
        }
    }

    /// Ctrl/Cmd+N, O, S and Shift+S.
    pub(crate) fn file_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut as K, Modifiers as M};
        let (save_as, save, open, new) = ctx.input_mut(|i| {
            // Shift+S first: the plain shortcut would also match it.
            let save_as = i.consume_shortcut(&K::new(M::COMMAND | M::SHIFT, Key::S));
            (
                save_as,
                i.consume_shortcut(&K::new(M::COMMAND, Key::S)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::O)),
                i.consume_shortcut(&K::new(M::COMMAND, Key::N)),
            )
        });
        if save_as {
            self.save_as(None);
        } else if save {
            self.save(None);
        }
        if open {
            self.request(Action::Open);
        }
        if new {
            self.request(Action::New);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl StructureCraftApp {
    fn dialog(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new().add_filter(FILTER_NAME, &[EXTENSION]);
        match self.files.current.as_deref().map(std::path::Path::new).and_then(std::path::Path::parent) {
            Some(dir) if !dir.as_os_str().is_empty() => dialog.set_directory(dir),
            _ => dialog,
        }
    }

    fn open_dialog(&mut self) {
        let Some(path) = self.dialog().set_title("Open model").pick_file() else { return };
        match path.to_str() {
            Some(path) => self.open(path),
            None => self.status = format!("cannot open {}: the path is not valid UTF-8", path.display()),
        }
    }

    /// File > Save as. Cancelling the dialog cancels `then` too.
    pub(crate) fn save_as(&mut self, then: Option<Action>) {
        let name = default_file_name(self.session.model.name());
        let Some(mut path) = self.dialog().set_title("Save model").set_file_name(name).save_file() else { return };
        if path.extension().is_none() {
            path.set_extension(EXTENSION);
        }
        let Some(path) = path.to_str().map(str::to_owned) else {
            self.status = format!("cannot save to {}: the path is not valid UTF-8", path.display());
            return;
        };
        if self.save_to(&path) {
            self.then(then);
        }
    }

    fn save_to(&mut self, path: &str) -> bool {
        let saved = self.run("model.save", serde_json::json!({ "path": path })).is_some();
        if saved {
            self.files.current = Some(path.to_owned());
            self.status = format!("Saved {}", display_name(path));
        }
        saved
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::sync::mpsc::{Receiver, Sender, channel};

    use super::{Action, EXTENSION, FILTER_NAME};
    use crate::StructureCraftApp;

    /// What a browser picker hands back.
    pub(super) enum Done {
        Opened { name: String, bytes: Vec<u8> },
        Saved { name: String, then: Option<Action> },
        Failed(String),
    }

    pub(super) struct Inbox {
        tx: Sender<Done>,
        rx: Receiver<Done>,
        /// To wake the UI when a picker finishes, since nothing else may repaint it.
        ctx: Option<egui::Context>,
    }

    impl Default for Inbox {
        fn default() -> Self {
            let (tx, rx) = channel();
            Self { tx, rx, ctx: None }
        }
    }

    impl Inbox {
        fn sender(&self) -> impl Fn(Done) + 'static {
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            move |done| {
                tx.send(done).ok();
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            }
        }
    }

    fn dialog() -> rfd::AsyncFileDialog {
        rfd::AsyncFileDialog::new().add_filter(FILTER_NAME, &[EXTENSION])
    }

    impl StructureCraftApp {
        pub(super) fn open_dialog(&mut self) {
            let send = self.files.inbox.sender();
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(file) = dialog().pick_file().await {
                    let bytes = file.read().await;
                    send(Done::Opened { name: file.file_name(), bytes });
                }
            });
        }

        /// File > Save as: the browser downloads the file.
        pub(crate) fn save_as(&mut self, then: Option<Action>) {
            let bytes = match self.session.save_bytes() {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.status = e.to_string();
                    return;
                }
            };
            let name = self.files.current.clone().unwrap_or_else(|| super::default_file_name(self.session.model.name()));
            let send = self.files.inbox.sender();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = dialog().set_file_name(name).save_file().await else { return };
                let done = match file.write(&bytes).await {
                    Ok(()) => Done::Saved { name: file.file_name(), then },
                    Err(e) => Done::Failed(format!("cannot save: {e}")),
                };
                send(done);
            });
        }

        pub(super) fn web_inbox(&mut self, ctx: &egui::Context) {
            if self.files.inbox.ctx.is_none() {
                self.files.inbox.ctx = Some(ctx.clone());
            }
            while let Ok(done) = self.files.inbox.rx.try_recv() {
                match done {
                    Done::Opened { name, bytes } => match self.session.open_bytes(&bytes) {
                        Ok(()) => {
                            self.status.clear();
                            self.opened(name);
                        }
                        Err(e) => self.status = format!("cannot open {name}: {e}"),
                    },
                    Done::Saved { name, then } => {
                        self.session.dirty = false;
                        self.status = format!("Saved {name}");
                        self.files.current = Some(name);
                        self.then(then);
                    }
                    Done::Failed(message) => self.status = message,
                }
            }
        }
    }
}
