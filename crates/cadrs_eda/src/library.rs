//! Symbol and footprint libraries (GS22): named collections, listed in a library table. A
//! table has global libraries (every design) and project libraries (this design); a design
//! refers to parts as `library:name`. Libraries can be switched off without being removed.

use crate::footprint::Footprint;
use crate::symbol::Symbol;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    Global,
    Project,
}

/// A library of symbols and footprints. Ids inside are `name:item`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub description: String,
    pub scope: Scope,
    pub enabled: bool,
    pub symbols: Vec<Symbol>,
    pub footprints: Vec<Footprint>,
}

impl Library {
    pub fn new(name: impl Into<String>, scope: Scope) -> Library {
        Library { name: name.into(), scope, enabled: true, ..Default::default() }
    }

    /// Adds or replaces a symbol, giving it this library's prefix.
    pub fn put_symbol(&mut self, mut s: Symbol) {
        s.id = format!("{}:{}", self.name, s.name());
        match self.symbols.iter_mut().find(|x| x.id == s.id) {
            Some(x) => *x = s,
            None => self.symbols.push(s),
        }
    }

    /// Adds or replaces a footprint, giving it this library's prefix.
    pub fn put_footprint(&mut self, mut f: Footprint) {
        f.id = format!("{}:{}", self.name, f.name());
        match self.footprints.iter_mut().find(|x| x.id == f.id) {
            Some(x) => *x = f,
            None => self.footprints.push(f),
        }
    }
}

/// The libraries a design can use, project ones first when names clash.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryTable {
    pub libraries: Vec<Library>,
}

fn split(id: &str) -> (&str, &str) {
    id.split_once(':').unwrap_or(("", id))
}

impl LibraryTable {
    /// The built-in libraries: the folders of [`builtin_dirs`] (loaded once, then copied).
    pub fn builtin() -> LibraryTable {
        static ALL: std::sync::OnceLock<Vec<Library>> = std::sync::OnceLock::new();
        let libraries = ALL.get_or_init(|| {
            let (libs, errors) = load_libraries(&builtin_dirs(), Scope::Global);
            for e in errors {
                eprintln!("libraries: {e}");
            }
            libs
        });
        LibraryTable { libraries: libraries.clone() }
    }

    fn ordered(&self) -> impl Iterator<Item = &Library> {
        let project = self.libraries.iter().filter(|l| l.enabled && l.scope == Scope::Project);
        let global = self.libraries.iter().filter(|l| l.enabled && l.scope == Scope::Global);
        project.chain(global)
    }

    pub fn library(&self, name: &str) -> Option<&Library> {
        self.ordered().find(|l| l.name == name)
    }

    pub fn library_mut(&mut self, name: &str, scope: Scope) -> Option<&mut Library> {
        self.libraries.iter_mut().find(|l| l.name == name && l.scope == scope)
    }

    /// Adds a library (or replaces the one of that name and scope).
    pub fn add(&mut self, lib: Library) {
        match self.libraries.iter_mut().find(|l| l.name == lib.name && l.scope == lib.scope) {
            Some(l) => *l = lib,
            None => self.libraries.push(lib),
        }
    }

    /// Adds a library's parts to the library of that name and scope (made if missing); its
    /// parts replace ones of the same name.
    pub fn merge(&mut self, lib: Library) {
        match self.libraries.iter_mut().find(|l| l.name == lib.name && l.scope == lib.scope) {
            Some(l) => {
                lib.symbols.into_iter().for_each(|s| l.put_symbol(s));
                lib.footprints.into_iter().for_each(|f| l.put_footprint(f));
            }
            None => self.libraries.push(lib),
        }
    }

    pub fn symbol(&self, id: &str) -> Option<&Symbol> {
        let (lib, name) = split(id);
        self.ordered().filter(|l| lib.is_empty() || l.name == lib).flat_map(|l| &l.symbols).find(|s| s.name() == name)
    }

    pub fn footprint(&self, id: &str) -> Option<&Footprint> {
        let (lib, name) = split(id);
        self.ordered().filter(|l| lib.is_empty() || l.name == lib).flat_map(|l| &l.footprints).find(|f| f.name() == name)
    }

    /// Symbols matching `filter` (every word in the id, keywords or description, any case),
    /// power symbols only when `power`. Exact name matches first.
    pub fn search_symbols(&self, filter: &str, power: bool) -> Vec<&Symbol> {
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let mut hits: Vec<&Symbol> = self
            .ordered()
            .flat_map(|l| &l.symbols)
            .filter(|s| !power || s.power)
            .filter(|s| {
                let hay = format!(
                    "{} {} {}",
                    s.id,
                    s.keywords,
                    s.field(crate::symbol::fields::DESCRIPTION).map_or("", |f| f.value())
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        let f = filter.trim().to_lowercase();
        hits.sort_by_key(|s| (s.name().to_lowercase() != f, !s.name().to_lowercase().starts_with(&f), s.id.clone()));
        hits
    }

    /// Every footprint, library by library.
    pub fn footprints(&self) -> impl Iterator<Item = &Footprint> {
        self.ordered().flat_map(|l| &l.footprints)
    }

    /// Footprints matching `filter` (every word in the id, keywords or description, any case)
    /// and, when `globs` has any, one of those footprint filters (a symbol's: `R_*`,
    /// `Connector*:*_1x??_*`) on the name or the full id. Exact name matches first.
    pub fn search_footprints(&self, filter: &str, globs: &[String]) -> Vec<&Footprint> {
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let mut hits: Vec<&Footprint> = self
            .footprints()
            .filter(|f| globs.is_empty() || globs.iter().any(|g| glob(g, f.name()) || glob(g, &f.id)))
            .filter(|f| {
                let hay = format!("{} {} {}", f.id, f.keywords, f.description).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        let f = filter.trim().to_lowercase();
        hits.sort_by_key(|x| (x.name().to_lowercase() != f, !x.name().to_lowercase().starts_with(&f), x.id.clone()));
        hits
    }

    /// The enabled libraries in lookup order (project ones first).
    pub fn enabled(&self) -> impl Iterator<Item = &Library> {
        self.ordered()
    }
}

// ---------------------------------------------------------------------------------------------
// Library folders
//
// A library is a folder named after it: `library.ron` (its description), one
// `<name>.symbol.ron` per symbol and one `<name>.footprint.ron` per footprint, and any 3D model
// files its footprints name (by file name, relative to the folder). A folder of libraries is
// a folder of those. Parts are read when the app starts, so adding a file adds a part.

const LIBRARY_FILE: &str = "library.ron";
const SYMBOL_EXT: &str = ".symbol.ron";
const FOOTPRINT_EXT: &str = ".footprint.ron";

/// What `library.ron` holds.
#[derive(Default, Serialize, Deserialize)]
struct LibraryInfo {
    #[serde(default)]
    description: String,
}

/// The folders the built-in libraries load from, in order: `$CADRS_LIBRARIES` (a path list)
/// when set; else `libraries/` beside the executable or two levels up (`target/release/`), and
/// the repository's `libraries/` this was built from.
pub fn builtin_dirs() -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("CADRS_LIBRARIES") {
        return std::env::split_paths(&v).collect();
    }
    let mut dirs = vec![];
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("libraries"));
        dirs.extend(exe_dir.parent().and_then(Path::parent).map(|d| d.join("libraries")));
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries"));
    // The first that exists (the same folder can show up twice).
    dirs.into_iter().find(|d| d.join("Device").is_dir() || d.is_dir()).into_iter().collect()
}

fn ron_config() -> ron::ser::PrettyConfig {
    ron::ser::PrettyConfig::default().struct_names(false).indentor("  ").depth_limit(3)
}

/// A file name for a part name (`/` and the like can't be in one).
pub fn file_stem(name: &str) -> String {
    name.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect()
}

/// Every id in a footprint set to `id()` (nil when written, so files don't change on rewrite;
/// fresh when read).
fn set_footprint_ids(f: &mut Footprint, mut id: impl FnMut() -> uuid::Uuid) {
    f.pads.iter_mut().for_each(|p| p.id = id());
    f.shapes.iter_mut().for_each(|s| s.id = id());
    f.texts.iter_mut().for_each(|t| t.id = id());
    f.fields.iter_mut().for_each(|t| t.text.id = id());
    f.zones.iter_mut().for_each(|z| z.id = id());
}

/// Writes a library folder (creating it), one file per symbol and footprint. Model files the
/// footprints name by a path inside `dir` are written relative to it.
pub fn save_library(lib: &Library, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, text: String| std::fs::write(dir.join(name), text).map_err(|e| format!("{name}: {e}"));
    write(LIBRARY_FILE, ron::ser::to_string_pretty(&LibraryInfo { description: lib.description.clone() }, ron_config()).map_err(|e| e.to_string())?)?;
    for s in &lib.symbols {
        write_symbol(dir, s)?;
    }
    for f in &lib.footprints {
        write_footprint(dir, f)?;
    }
    Ok(())
}


/// Makes `dir` a library folder (with its `library.ron`) unless it is one already.
pub fn ensure_library(dir: &Path, description: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let info = dir.join(LIBRARY_FILE);
    if info.is_file() {
        return Ok(());
    }
    let text = ron::ser::to_string_pretty(&LibraryInfo { description: description.into() }, ron_config()).map_err(|e| e.to_string())?;
    std::fs::write(&info, text).map_err(|e| format!("{}: {e}", info.display()))
}
/// Writes one symbol into a library folder.
pub fn write_symbol(dir: &Path, s: &Symbol) -> Result<(), String> {
    let mut s = s.clone();
    s.id = s.name().to_string();
    let text = ron::ser::to_string_pretty(&s, ron_config()).map_err(|e| e.to_string())?;
    let name = format!("{}{SYMBOL_EXT}", file_stem(&s.id));
    std::fs::write(dir.join(&name), text).map_err(|e| format!("{name}: {e}"))
}

/// Writes one footprint into a library folder.
pub fn write_footprint(dir: &Path, f: &Footprint) -> Result<(), String> {
    let mut f = f.clone();
    f.id = f.name().to_string();
    set_footprint_ids(&mut f, uuid::Uuid::nil);
    for m in &mut f.models {
        if let Ok(rel) = Path::new(&m.source).strip_prefix(dir) {
            m.source = rel.to_string_lossy().into_owned();
        }
        m.blob = None;
    }
    let text = ron::ser::to_string_pretty(&f, ron_config()).map_err(|e| e.to_string())?;
    let name = format!("{}{FOOTPRINT_EXT}", file_stem(&f.id));
    std::fs::write(dir.join(&name), text).map_err(|e| format!("{name}: {e}"))
}

/// Reads a library folder (named after the folder). Parts that don't parse are skipped and
/// reported. Model files beside the parts get their full path as `source`.
pub fn load_library(dir: &Path, scope: Scope) -> (Library, Vec<String>) {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut lib = Library::new(name, scope);
    let mut errors = vec![];
    if let Ok(text) = std::fs::read_to_string(dir.join(LIBRARY_FILE)) {
        match ron::from_str::<LibraryInfo>(&text) {
            Ok(info) => lib.description = info.description,
            Err(e) => errors.push(format!("{}: {e}", dir.join(LIBRARY_FILE).display())),
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    for path in files {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = || std::fs::read_to_string(&path).map_err(|e| e.to_string());
        if file.ends_with(SYMBOL_EXT) {
            match text().and_then(|t| ron::from_str::<Symbol>(&t).map_err(|e| e.to_string())) {
                Ok(s) => lib.put_symbol(s),
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        } else if file.ends_with(FOOTPRINT_EXT) {
            match text().and_then(|t| ron::from_str::<Footprint>(&t).map_err(|e| e.to_string())) {
                Ok(mut f) => {
                    set_footprint_ids(&mut f, uuid::Uuid::new_v4);
                    for m in &mut f.models {
                        let p = dir.join(&m.source);
                        if !m.source.is_empty() && Path::new(&m.source).is_relative() && p.is_file() {
                            m.source = std::path::absolute(&p).unwrap_or(p).to_string_lossy().into_owned();
                        }
                    }
                    lib.put_footprint(f)
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
    }
    (lib, errors)
}

/// Every library in these folders of libraries (each sub-folder one library), by name; a
/// later folder's library of the same name adds to it.
pub fn load_libraries(dirs: &[PathBuf], scope: Scope) -> (Vec<Library>, Vec<String>) {
    let mut out: Vec<Library> = vec![];
    let mut errors = vec![];
    for dir in dirs {
        let mut subs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for sub in subs {
            let (lib, e) = load_library(&sub, scope);
            errors.extend(e);
            match out.iter_mut().find(|l| l.name == lib.name) {
                Some(l) => {
                    lib.symbols.into_iter().for_each(|s| l.put_symbol(s));
                    lib.footprints.into_iter().for_each(|f| l.put_footprint(f));
                }
                None => out.push(lib),
            }
        }
    }
    (out, errors)
}

/// Glob match with `*` and `?`, ignoring case (footprint filters).
pub fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some(b'*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some(b'?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob("R_*", "R_Axial_DIN0309"));
        assert!(glob("LED*", "led_d5.0mm"));
        assert!(!glob("C_*", "R_0805"));
        assert!(glob("*:LED_*", "LED_THT:LED_D5.0mm"));
        assert!(glob("R_?805*", "R_0805_2012Metric"));
    }

    #[test]
    fn project_libraries_win_and_disabled_ones_hide() {
        let mut t = LibraryTable::builtin();
        assert!(t.symbol("Device:LED").is_some());
        assert!(t.symbol("LED").is_some());
        let mut mine = Library::new("Device", Scope::Project);
        let mut led = t.symbol("Device:LED").unwrap().clone();
        led.keywords = "mine".into();
        mine.put_symbol(led);
        t.add(mine);
        assert_eq!(t.symbol("Device:LED").unwrap().keywords, "mine");
        t.library_mut("Device", Scope::Project).unwrap().enabled = false;
        // The project "Device" is off: the global one shows again.
        assert_ne!(t.symbol("Device:LED").unwrap().keywords, "mine");
        let hits = t.search_symbols("r", false);
        assert_eq!(hits[0].name(), "R");
        assert!(t.search_symbols("", true).iter().all(|s| s.power));
    }

    #[test]
    fn footprint_search_takes_symbol_filters() {
        let t = LibraryTable::builtin();
        let r = t.symbol("Device:R").unwrap();
        let hits = t.search_footprints("0805", &r.footprint_filters);
        assert!(!hits.is_empty() && hits.iter().all(|f| f.name().starts_with("R_")), "{:?}", hits.iter().map(|f| &f.id).collect::<Vec<_>>());
        let conn = t.symbol("Connector:Conn_01x04").unwrap();
        let hits = t.search_footprints("", &conn.footprint_filters);
        assert!(hits.iter().any(|f| f.name() == "PinHeader_1x04_P2.54mm_Vertical"));
        assert!(hits.iter().all(|f| f.name().contains("_1x")));
    }
}

/// The built-in library files (`libraries/`): well-formed, broad, and as KiCad has them.
#[cfg(test)]
mod builtin_tests {
    use super::*;
    use crate::footprint::{Pad, PadShape};
    use crate::layer::Layer;
    use crate::symbol::{PinType, fields};
    use crate::units::{Pt, SCHEMATIC_GRID};

    fn libraries() -> Vec<Library> {
        LibraryTable::builtin().libraries
    }

    #[test]
    fn symbol_pins_sit_on_the_grid() {
        for lib in libraries() {
            for s in &lib.symbols {
                for pn in &s.pins {
                    assert_eq!((pn.at.x % SCHEMATIC_GRID, pn.at.y % SCHEMATIC_GRID), (0, 0), "{} pin {}", s.id, pn.number);
                }
                assert!(s.field(fields::REFERENCE).is_some() && s.field(fields::VALUE).is_some());
            }
        }
    }

    #[test]
    fn files_round_trip() {
        let t = LibraryTable::builtin();
        let lib = t.library("Package_SO").unwrap();
        let dir = std::env::temp_dir().join(format!("cadrs-lib-{}", uuid::Uuid::new_v4())).join("Package_SO");
        save_library(lib, &dir).unwrap();
        let (back, errors) = load_library(&dir, Scope::Global);
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!((back.name.as_str(), &back.description), (lib.name.as_str(), &lib.description));
        assert_eq!(back.footprints.len(), lib.footprints.len());
        // The same parts, but for the ids (fresh on every read).
        let a = &lib.footprints[0];
        let b = back.footprints.iter().find(|f| f.id == a.id).unwrap();
        assert_eq!((&a.pads.len(), &a.pads[0].at, &a.models), (&b.pads.len(), &b.pads[0].at, &b.models));
        assert_ne!(a.pads[0].id, b.pads[0].id);
        assert!(!b.pads[0].id.is_nil());
    }
    #[test]
    fn course_parts_exist() {
        let t = LibraryTable::builtin();
        for id in ["Device:LED", "Device:R_US", "Device:Battery_Cell", "power:VCC", "power:GND", "power:PWR_FLAG"] {
            assert!(t.symbol(id).is_some(), "{id}");
        }
        for id in ["Battery:BatteryHolder_Keystone_1058_1x2032", "LED_THT:LED_D5.0mm", "Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal"] {
            let f = t.footprint(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(f.pads.len(), 2);
        }
        let r = t.footprint("Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal").unwrap();
        assert_eq!(r.pads[1].at, Pt::mm(12.7, 0.0));
        assert!(t.symbol("power:GND").unwrap().power);
        assert_eq!(t.symbol("power:PWR_FLAG").unwrap().pins[0].kind, PinType::PowerOut);
    }

    use crate::units::to_mm;

    /// The pad's box (mm).
    fn pad_box(pd: &Pad) -> (f64, f64, f64, f64) {
        let (mut w, mut h) = (to_mm(pd.size.w), to_mm(pd.size.h));
        if (pd.angle.rem_euclid(180.0) - 90.0).abs() < 1.0 {
            std::mem::swap(&mut w, &mut h);
        }
        let (x, y) = (to_mm(pd.at.x), to_mm(pd.at.y));
        (x - w / 2.0, y - h / 2.0, x + w / 2.0, y + h / 2.0)
    }

    fn box_dist(b: (f64, f64, f64, f64), x: f64, y: f64) -> f64 {
        let dx = (b.0 - x).max(x - b.2).max(0.0);
        let dy = (b.1 - y).max(y - b.3).max(0.0);
        dx.hypot(dy)
    }

    #[test]
    fn libraries_are_broad() {
        let all = libraries();
        let symbols: usize = all.iter().map(|l| l.symbols.len()).sum();
        let footprints: usize = all.iter().map(|l| l.footprints.len()).sum();
        assert!(symbols > 120, "{symbols} symbols");
        assert!(footprints > 300, "{footprints} footprints");
        let t = LibraryTable::builtin();
        for id in ["Device:Antenna", "Device:C", "Device:L", "Device:Q_NPN_BEC", "Device:Crystal", "power:GNDREF", "power:+3V3", "Connector:Conn_02x10_Odd_Even", "Switch:SW_Push", "Regulator_Linear:AMS1117-3.3", "LED:WS2812B"] {
            assert!(t.symbol(id).is_some(), "{id}");
        }
        for id in ["Capacitor_SMD:C_0805_2012Metric", "Inductor_SMD:L_0805_2012Metric", "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical", "Package_TO_SOT_SMD:SOT-223-3_TabPin2", "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm", "Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm"] {
            assert!(t.footprint(id).is_some(), "{id}");
        }
    }

    #[test]
    fn footprints_are_well_formed() {
        for lib in libraries() {
            for f in &lib.footprints {
                assert_eq!(crate::lib_edit::check_footprint(f), Vec::<String>::new(), "{}", f.id);
                assert!(f.models.first().and_then(|m| m.body.as_ref()).is_some() || f.id.starts_with("MountingHole") || f.id.starts_with("TestPoint"), "{} has no 3D body", f.id);
                // Pads of different numbers don't overlap.
                for (i, a) in f.pads.iter().enumerate() {
                    for b in &f.pads[i + 1..] {
                        if a.number == b.number {
                            continue;
                        }
                        let (p, q) = (pad_box(a), pad_box(b));
                        let overlap = p.0 < q.2 && q.0 < p.2 && p.1 < q.3 && q.1 < p.3;
                        assert!(!overlap, "{}: pads {} and {} overlap", f.id, a.number, b.number);
                    }
                }
                // Silkscreen stays off the copper.
                for s in f.shapes.iter().filter(|s| s.layer == Layer::TopSilk) {
                    let (pts, closed) = crate::poly::geom_points(&s.shape.geom);
                    let n = pts.len();
                    let segs = if closed { n } else { n.saturating_sub(1) };
                    for k in 0..segs {
                        let (a, b) = (pts[k], pts[(k + 1) % n]);
                        for t in 0..=20 {
                            let u = t as f64 / 20.0;
                            let (x, y) = (to_mm(a.x) + (to_mm(b.x) - to_mm(a.x)) * u, to_mm(a.y) + (to_mm(b.y) - to_mm(a.y)) * u);
                            for pd in &f.pads {
                                let to_pad = if pd.shape == PadShape::Circle { (x - to_mm(pd.at.x)).hypot(y - to_mm(pd.at.y)) - to_mm(pd.size.w) / 2.0 } else { box_dist(pad_box(pd), x, y) };
                                let d = to_pad - to_mm(s.shape.stroke.width) / 2.0;
                                assert!(d > 0.05, "{}: silkscreen {d:.3} mm from pad {}", f.id, pd.number);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn footprints_match_kicad_pin_positions() {
        let t = LibraryTable::builtin();
        let at = |id: &str, n: &str| t.footprint(id).unwrap().pads.iter().find(|p| p.number == n).unwrap().at;
        // Pin headers: pin 1 at the origin, 2 beside it, rows going down.
        let h = "Connector_PinHeader_2.54mm:PinHeader_2x10_P2.54mm_Vertical";
        assert_eq!((at(h, "1"), at(h, "2"), at(h, "20")), (Pt::ZERO, Pt::mm(2.54, 0.0), Pt::mm(2.54, -22.86)));
        // SOIC-8: pin 1 top-left, 8 top-right.
        let so = "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm";
        assert_eq!((at(so, "1"), at(so, "4"), at(so, "8")), (Pt::mm(-2.475, 1.905), Pt::mm(-2.475, -1.905), Pt::mm(2.475, 1.905)));
        // SOT-23: 1 and 2 on the left, 3 on the right.
        let sot = "Package_TO_SOT_SMD:SOT-23";
        assert_eq!((at(sot, "1"), at(sot, "3")), (Pt::mm(-1.1375, 0.95), Pt::mm(1.1375, 0.0)));
        // QFN-32: 32 pins + the exposed pad 33.
        let q = t.footprint("Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm").unwrap();
        assert_eq!(q.pads.len(), 33);
        assert_eq!(at("Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm", "1"), Pt::mm(-2.45, 1.75));
    }
}
