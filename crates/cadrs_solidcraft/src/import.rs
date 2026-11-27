//! Writing a translated recipe into a cadrs document store, as a normal editable document.
//!
//! The document is saved through [`cadrs_core::Store`], the same store the app and the Onshape
//! importer write (one folder per document, `document.ron` inside), and read back to prove it
//! loads.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use cadrs_core::{Document, DocumentId, DocumentMeta, Store};
use serde_json::Value;

use crate::recipe::{Recipe, Stop};

/// What [`import_text`] wrote.
pub struct Imported {
    /// The new document's id (also on disk under the store's root).
    pub id: DocumentId,
    /// The document's name, as the app lists it.
    pub name: String,
    /// How many features the translated Part Studio has.
    pub features: usize,
    /// Where cadrs and the recipe differ, one line each.
    pub notes: Vec<String>,
}

/// Translates a recipe and saves it to `store` as a document owned by `user`.
///
/// The document is named after `name` when one is given, else after the recipe's `name`, else
/// `SolidCraft part`. The store is checked by loading the document back.
pub fn import_text(
    recipe: &Value,
    store: &Store,
    user: &str,
    name: Option<&str>,
) -> Result<Imported, Stop> {
    let built =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Recipe::from_recipe(recipe)));
    let r = match built {
        Ok(r) => r?,
        Err(_) => return Err(Stop::Failed("panicked while translating the recipe".into())),
    };
    let notes = r.notes().to_vec();
    let mut doc: Document = r.into_document();
    if let Some(name) = name.or_else(|| recipe["name"].as_str()) {
        doc.name = name.to_string();
    } else {
        doc.name = "SolidCraft part".to_string();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let meta = DocumentMeta::new(user, now);
    store
        .save(&doc, &meta)
        .map_err(|e| Stop::Failed(format!("saving the document: {e}")))?;
    store
        .load(doc.id)
        .map_err(|e| Stop::Failed(format!("the saved document does not load: {e}")))?;
    let features = doc.elements.first().map_or(0, |e| e.features().len());
    Ok(Imported {
        id: doc.id,
        name: doc.name.clone(),
        features,
        notes,
    })
}

/// Reads a `recipe.json` and imports it, as [`import_text`] does.
pub fn import_file(path: &Path, store: &Store, user: &str) -> Result<Imported, Stop> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Stop::Failed(format!("{}: {e}", path.display())))?;
    let recipe: Value = serde_json::from_str(&text)
        .map_err(|e| Stop::Failed(format!("{}: not JSON: {e}", path.display())))?;
    import_text(&recipe, store, user, None)
}
