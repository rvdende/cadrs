//! The scraped Onshape data on disk (`<raw>/<document id>/…`).

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A scraped Onshape document folder.
#[derive(Debug, Clone)]
pub struct RawDocument {
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
}

/// One element (tab) of a document.
#[derive(Debug, Clone)]
pub struct RawElement {
    pub id: String,
    pub name: String,
    /// `PARTSTUDIO`, `ASSEMBLY`, `BLOB`, `DRAWING`, `BILLOFMATERIALS`, …
    pub kind: String,
    pub dir: PathBuf,
}

pub fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Every scraped document under `raw` (folders with a `document.json`), by name.
pub fn documents(raw: &Path) -> Vec<RawDocument> {
    let mut out: Vec<RawDocument> = std::fs::read_dir(raw)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let doc = read_json(&dir.join("document.json"))?;
            Some(RawDocument {
                id: doc["id"].as_str()?.to_string(),
                name: doc["name"].as_str().unwrap_or("Untitled").to_string(),
                dir,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    out
}

impl RawDocument {
    /// The document's elements, in tab order.
    pub fn elements(&self) -> Vec<RawElement> {
        read_json(&self.dir.join("elements.json"))
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|e| {
                let id = e["id"].as_str()?.to_string();
                Some(RawElement {
                    name: e["name"].as_str().unwrap_or_default().to_string(),
                    kind: e["elementType"].as_str().unwrap_or_default().to_string(),
                    dir: self.dir.join(&id),
                    id,
                })
            })
            .collect()
    }

    /// The raw `document.json`.
    pub fn json(&self) -> Option<Value> {
        read_json(&self.dir.join("document.json"))
    }
}

impl RawElement {
    pub fn features(&self) -> Option<Value> {
        read_json(&self.dir.join("features.json"))
    }

    pub fn sketches(&self) -> Option<Value> {
        read_json(&self.dir.join("sketches.json"))
    }
}

/// The other documents a document's Part Studios derive parts from.
pub fn derived_sources(doc: &RawDocument) -> Vec<String> {
    let mut out = Vec::new();
    for el in doc.elements() {
        for f in el.features().and_then(|f| f["features"].as_array().cloned()).unwrap_or_default() {
            if f["featureType"].as_str() != Some("importDerived") {
                continue;
            }
            let ns = f["parameters"]
                .as_array()
                .and_then(|ps| ps.iter().find(|p| p["parameterId"] == "partStudio"))
                .and_then(|p| p["namespace"].as_str())
                .unwrap_or_default();
            for part in ns.split("::") {
                if let Some(d) = part.strip_prefix('d')
                    && d != doc.id
                    && !out.contains(&d.to_string())
                {
                    out.push(d.to_string());
                }
            }
        }
    }
    out
}

/// `docs` in an order where a document others derive from comes before them (so its import is
/// in the store when theirs resolves); otherwise as given.
pub fn derive_order(docs: Vec<RawDocument>) -> Vec<RawDocument> {
    let deps: Vec<Vec<String>> = docs.iter().map(derived_sources).collect();
    let mut left: Vec<usize> = (0..docs.len()).collect();
    let mut order = Vec::new();
    while !left.is_empty() {
        let ready = left.iter().position(|&i| deps[i].iter().all(|d| !left.iter().any(|&j| docs[j].id == *d)));
        order.push(left.remove(ready.unwrap_or(0)));
    }
    let mut slots: Vec<Option<RawDocument>> = docs.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}
