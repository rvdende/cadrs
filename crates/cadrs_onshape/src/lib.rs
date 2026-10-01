//! Imports Onshape documents as editable cadrs documents.
//!
//! The input is the raw REST JSON that `tools/onshape/scrape.js` saves (one folder per Onshape
//! document: `document.json`, `elements.json`, and per Part Studio `features.json`,
//! `sketches.json`, …). The import replays each Part Studio's feature list through the cadrs
//! command layer, so the result is a normal document with its full, editable history. What
//! could not be translated is listed (assemblies: [`assembly`]) in a [`report::DocumentReport`].

pub mod assembly;
pub mod eval;
pub mod expr;
pub mod features;
pub mod ids;
pub mod import;
pub mod query;
pub mod raw;
pub mod refs;
pub mod report;
pub mod sketch;
pub mod studio;

pub use import::{Imported, Options, import_document};
