//! Footprint assignment (GS10): which footprints suit a placed symbol, with the assignment
//! tool's filters, and setting the choice.

use crate::footprint::Footprint;
use crate::library::{LibraryTable, glob};
use crate::schematic::{PlacedSymbol, Schematic};
use crate::symbol::fields;
use std::collections::BTreeSet;

/// The assignment tool's filters, each optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filters {
    /// The symbol's own footprint filters (`R_*`).
    pub symbol_filters: bool,
    /// As many pads (distinct numbers) as the symbol has pins.
    pub pin_count: bool,
    /// Only this library.
    pub library: Option<String>,
    /// Every word in the footprint's id, description or keywords.
    pub text: String,
}

/// Distinct pad numbers of a footprint (mechanical pads without a number don't count).
pub fn pad_count(f: &Footprint) -> usize {
    f.pads.iter().filter(|p| !p.number.is_empty()).map(|p| p.number.as_str()).collect::<BTreeSet<_>>().len()
}

/// The footprints `filters` leave for `s`.
pub fn candidates<'a>(table: &'a LibraryTable, sch: &Schematic, s: &PlacedSymbol, filters: &Filters) -> Vec<&'a Footprint> {
    let def = sch.symbol(&s.symbol);
    let pins = def.map_or(0, |d| d.pins.iter().map(|p| p.number.as_str()).collect::<BTreeSet<_>>().len());
    let words: Vec<String> = filters.text.split_whitespace().map(str::to_lowercase).collect();
    table
        .footprints()
        .filter(|f| {
            if filters.symbol_filters
                && let Some(d) = def
                && !d.footprint_filters.is_empty()
                && !d.footprint_filters.iter().any(|pat| glob(pat, &f.id) || glob(pat, f.name()))
            {
                return false;
            }
            if filters.pin_count && pad_count(f) != pins {
                return false;
            }
            if let Some(lib) = &filters.library
                && !f.id.starts_with(&format!("{lib}:"))
            {
                return false;
            }
            let hay = format!("{} {} {}", f.id, f.description, f.keywords).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

/// Assigns `footprint` (`lib:name`) to the symbol with reference `reference`.
pub fn assign(sch: &mut Schematic, reference: &str, footprint: &str) -> bool {
    let Some(id) = sch.sheets.iter().flat_map(|s| &s.symbols).find(|s| s.reference() == reference).map(|s| s.id) else { return false };
    crate::sch_edit::set_field(sch, id, fields::FOOTPRINT, footprint)
}
