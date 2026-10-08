//! The library browser's **JLCPCB parts** tab: search JLCPCB's catalogue, look at a part's
//! symbol and footprint, and add it (symbol, footprint and STEP model, from the JLCEDA/EasyEDA
//! official library) to the user library [`ONLINE_LIBRARY`] — usable at once, no rebuild.
//!
//! The network work runs on its own thread; [`poll`] picks up what finished. Scenarios can't
//! reach the network: `eda-lcsc-fixtures <dir>` (or `$CADRS_LCSC_FIXTURES`) makes the
//! catalogue the saved EasyEDA answers in `<dir>` (`C….json`), without 3D models.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use cadrs_easyeda::{Hit, Imported, PartInfo};

use super::libraries::{ONLINE_LIBRARY, UserLibraries};

/// A job's answer, filled in by its thread.
type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn spawn<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Slot<T> {
    let slot: Slot<T> = Arc::new(Mutex::new(None));
    let out = slot.clone();
    std::thread::spawn(move || {
        let r = f();
        if let Ok(mut s) = out.lock() {
            *s = Some(r);
        }
    });
    slot
}

fn take<T>(slot: &Option<Slot<T>>) -> Option<Result<T, String>> {
    slot.as_ref()?.lock().ok()?.take()
}

/// The online search's state.
#[derive(Resource, Default)]
pub struct Online {
    pub query: String,
    pub hits: Vec<Hit>,
    pub total: u64,
    /// What's going on, for the status line.
    pub status: String,
    /// Bumped when the hits change (the list redraws).
    pub generation: u64,
    /// The chosen part's LCSC number.
    pub chosen: Option<String>,
    /// The chosen part converted (for its previews), and its EasyEDA data.
    pub preview: Option<(String, cadrs_easyeda::Converted)>,
    /// Saved EasyEDA answers to use instead of the network.
    pub fixtures: Option<PathBuf>,
    search: Option<Slot<(Vec<Hit>, u64)>>,
    fetch: Option<Slot<(String, cadrs_easyeda::Converted)>>,
    import: Option<Slot<Imported>>,
    /// Use the part once it's added (OK), rather than only add it.
    then_accept: bool,
}

impl Online {
    pub fn busy(&self) -> bool {
        self.search.is_some() || self.import.is_some()
    }
}

pub fn register(app: &mut App) {
    let fixtures = std::env::var_os("CADRS_LCSC_FIXTURES").filter(|d| !d.is_empty()).map(PathBuf::from);
    app.insert_resource(Online { fixtures, ..default() });
}

/// The saved answer for a part, from the fixtures folder.
fn fixture(dir: &std::path::Path, lcsc: &str) -> Result<cadrs_easyeda::serde_json::Value, String> {
    let text = std::fs::read_to_string(dir.join(format!("{lcsc}.json"))).map_err(|_| format!("{lcsc}: not in the saved parts"))?;
    let v: cadrs_easyeda::serde_json::Value = cadrs_easyeda::serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(if v["result"].is_object() { v["result"].clone() } else { v })
}

/// The saved parts matching every word of `query`.
fn fixture_search(dir: &std::path::Path, query: &str) -> Result<(Vec<Hit>, u64), String> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    let hits: Vec<Hit> = files
        .iter()
        .filter_map(|p| fixture(dir, &p.file_stem()?.to_string_lossy()).ok())
        .map(|v| cadrs_easyeda::hit_of(&v))
        .filter(|h| {
            let hay = format!("{} {} {} {} {}", h.lcsc, h.mpn, h.manufacturer, h.package, h.category).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect();
    let n = hits.len() as u64;
    Ok((hits, n))
}

/// Starts a search for `query`.
pub fn search(w: &mut World, query: &str) {
    let query = query.trim().to_string();
    let mut o = w.resource_mut::<Online>();
    if query.is_empty() {
        o.status = "Type a part name, part number or LCSC number (C…), then Enter".into();
        return;
    }
    let fixtures = o.fixtures.clone();
    let q = query.clone();
    o.search = Some(spawn(move || match &fixtures {
        Some(dir) => fixture_search(dir, &q),
        None => cadrs_easyeda::search(&q, 1),
    }));
    o.query = query;
    o.status = "Searching JLCPCB…".into();
}

/// Chooses a part: fetches and converts it for the previews.
pub fn choose(w: &mut World, lcsc: &str) {
    let mut o = w.resource_mut::<Online>();
    o.chosen = Some(lcsc.to_string());
    if o.preview.as_ref().is_some_and(|p| p.0 == lcsc) {
        return;
    }
    let info = info_of(o.hits.iter().find(|h| h.lcsc == lcsc));
    let (fixtures, lcsc) = (o.fixtures.clone(), lcsc.to_string());
    o.fetch = Some(spawn(move || {
        let v = match &fixtures {
            Some(dir) => fixture(dir, &lcsc)?,
            None => cadrs_easyeda::api::component(&lcsc)?,
        };
        Ok((lcsc, cadrs_easyeda::convert(&v, ONLINE_LIBRARY, &info)?))
    }));
}

fn info_of(h: Option<&Hit>) -> PartInfo {
    h.map(|h| PartInfo { description: h.description.clone(), datasheet: h.datasheet.clone(), category: h.category.clone() }).unwrap_or_default()
}

/// Adds the chosen part to the user library; `then_accept`: then use it as the browser's choice.
pub fn add(w: &mut World, then_accept: bool) {
    let Some(dir) = w.resource::<UserLibraries>().library_dir(ONLINE_LIBRARY) else {
        w.resource_mut::<Online>().status = "There's no folder to keep libraries in".into();
        return;
    };
    let mut o = w.resource_mut::<Online>();
    let Some(lcsc) = o.chosen.clone() else {
        o.status = "Choose a part first".into();
        return;
    };
    if o.import.is_some() {
        return;
    }
    let info = info_of(o.hits.iter().find(|h| h.lcsc == lcsc));
    let fixtures = o.fixtures.clone();
    o.status = format!("Adding {lcsc} to {ONLINE_LIBRARY}…");
    o.then_accept = then_accept;
    o.import = Some(spawn(move || match &fixtures {
        Some(fx) => cadrs_easyeda::write_part(&fixture(fx, &lcsc)?, &dir, &info, None),
        None => cadrs_easyeda::import(&lcsc, &dir, &info),
    }));
}

/// What a finished job means for the browser.
pub enum Done {
    Nothing,
    /// New hits or a new preview: redraw.
    Redraw,
    /// A part was added: (symbol id, footprint id), and whether to use it now.
    Added(String, String, bool),
}

/// Picks up finished jobs.
pub fn poll(w: &mut World) -> Done {
    let Some(mut o) = w.get_resource_mut::<Online>() else { return Done::Nothing };
    let mut done = Done::Nothing;
    if let Some(r) = take(&o.search) {
        o.search = None;
        match r {
            Ok((hits, total)) => {
                o.status = match (hits.len(), total) {
                    (0, _) => format!("No parts match \"{}\"", o.query),
                    (n, t) if t as usize > n => format!("The first {n} of {t} parts"),
                    (n, _) => format!("{n} part{}", if n == 1 { "" } else { "s" }),
                };
                o.hits = hits;
                o.total = total;
            }
            Err(e) => o.status = format!("Search failed: {e}"),
        }
        o.generation += 1;
        done = Done::Redraw;
    }
    if let Some(r) = take(&o.fetch) {
        o.fetch = None;
        match r {
            Ok(p) => o.preview = Some(p),
            Err(e) => o.status = e,
        }
        done = Done::Redraw;
    }
    if let Some(r) = take(&o.import) {
        o.import = None;
        match r {
            Ok(i) => {
                o.status = match i.warnings.as_slice() {
                    [] => format!("Added {} to {ONLINE_LIBRARY}", i.symbol.split_once(':').map_or(i.symbol.as_str(), |s| s.1)),
                    w => format!("Added, but: {}", w.join("; ")),
                };
                done = Done::Added(i.symbol, i.footprint, o.then_accept);
                w.resource_mut::<UserLibraries>().reload();
            }
            Err(e) => {
                o.status = format!("Couldn't add it: {e}");
                done = Done::Redraw;
            }
        }
    }
    done
}

/// A hit's one-line detail and its longer description.
pub fn describe(h: &Hit) -> (String, String) {
    let class = if h.basic { "Basic" } else { "Extended" };
    let price = h.price.map(|p| format!(" · ${p:.4}")).unwrap_or_default();
    let detail = format!("{} · {class}{price}", if h.package.is_empty() { h.manufacturer.as_str() } else { h.package.as_str() });
    let mut text = format!("{} · {} {}\n", h.lcsc, h.manufacturer, h.mpn);
    if !h.description.is_empty() {
        text += &format!("{}\n", h.description);
    }
    if !h.category.is_empty() {
        text += &format!("{}\n", h.category);
    }
    if h.stock > 0 || h.price.is_some() {
        text += &format!("Stock {} · {class} part{price}\n", h.stock);
    }
    // The source on two lines: one is too wide to wrap.
    text += &cadrs_easyeda::SOURCE.replacen(" (", "\n(", 1).replacen("JLCEDA", "From the JLCEDA", 1);
    (detail, text)
}
