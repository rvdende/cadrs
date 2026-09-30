//! A document being built through the command layer.

use cadrs_core::command::{Command, CommandError, History};
use cadrs_core::document::Document;

/// Runs commands on a document (with undo history, as the app would).
pub trait Studio {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError>;
    fn document(&self) -> &Document;
}

/// A document and its history.
pub struct DocStudio {
    pub doc: Document,
    pub history: History,
}

impl DocStudio {
    pub fn new(doc: Document) -> Self {
        // The import is replayed from scratch each time: no need to keep undo steps around.
        Self { doc, history: History::new(1) }
    }
}

impl Studio for DocStudio {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError> {
        self.history.execute(&mut self.doc, c)
    }

    fn document(&self) -> &Document {
        &self.doc
    }
}
