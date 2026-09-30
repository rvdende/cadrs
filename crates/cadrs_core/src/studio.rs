//! [`Studio`]: something commands run on (a document with its history in tests and samples, or
//! the app's open document), so sample builders such as `cadrs_pcb::sample` and the course
//! stand-ins work the same in tests and in the app.

use crate::command::{Command, CommandError, History};
use crate::document::Document;

/// Something commands run on: a document with its history (tests), or the app's open document.
pub trait Studio {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError>;
    fn document(&self) -> &Document;
}

/// A document and its undo history.
pub struct DocHistory<'a>(pub &'a mut Document, pub &'a mut History);

impl Studio for DocHistory<'_> {
    fn run(&mut self, c: &dyn Command) -> Result<(), CommandError> {
        self.1.execute(self.0, c)
    }
    fn document(&self) -> &Document {
        self.0
    }
}
