//! Symbol and footprint libraries (GS22): named collections, listed in a library table. A
//! table has global libraries (every design) and project libraries (this design); a design
//! refers to parts as `library:name`. Libraries can be switched off without being removed.

use crate::footprint::Footprint;
use crate::symbol::Symbol;
use serde::{Deserialize, Serialize};

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
    /// The built-in libraries ([`crate::stdlib`]).
    pub fn builtin() -> LibraryTable {
        LibraryTable { libraries: crate::stdlib::libraries() }
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
}
