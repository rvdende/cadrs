//! Update PCB from schematic (GS14, F8): the layout takes the schematic's parts and nets.
//!
//! Each part with a footprint gets a footprint on the board, linked to its symbol (by id, or
//! by reference when relinking). New footprints are placed side by side from a point; changed
//! footprints are swapped in place; reference and value follow the symbol; every pad gets the
//! net of its symbol pin. The change log reads like the tool's report.

use crate::Design;
use crate::board::PlacedFootprint;
use crate::connectivity::netlist;
use crate::footprint::FootprintPlacement;
use crate::library::LibraryTable;
use crate::symbol::fields;
use crate::units::{Nm, Pt, mm};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Link footprints to symbols by reference instead of by id.
    pub relink_by_reference: bool,
    pub replace_footprints: bool,
    pub delete_unused: bool,
    pub update_fields: bool,
    /// Where new footprints go: left to right from here.
    pub place_at: Pt,
}

impl Default for Options {
    fn default() -> Self {
        Options { relink_by_reference: false, replace_footprints: true, delete_unused: false, update_fields: true, place_at: Pt::ZERO }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub messages: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    /// Footprints added (their references).
    pub added: Vec<String>,
}

fn set_text_field(fp: &mut crate::footprint::Footprint, name: &str, value: &str) {
    match fp.field_mut(name) {
        Some(f) => f.text.text.text = value.into(),
        None => {
            if let Some(mut f) = fp.fields.first().cloned() {
                f.name = name.into();
                f.text.text.text = value.into();
                f.text.text.visible = false;
                fp.fields.push(f);
            }
        }
    }
}

fn width(fp: &crate::footprint::Footprint) -> Nm {
    let mut b: Option<crate::units::Bounds> = None;
    for s in &fp.shapes {
        for p in s.shape.geom.extent() {
            b = Some(crate::units::Bounds::union(b, crate::units::Bounds::of(p)));
        }
    }
    for p in &fp.pads {
        b = Some(crate::units::Bounds::union(b, crate::units::Bounds::of(p.at).grow(p.size.w.max(p.size.h) / 2)));
    }
    b.map_or(mm(5.0), |b| b.size().w)
}

/// Brings the board up to date with the schematic.
pub fn update_pcb(d: &mut Design, lib: &LibraryTable, opts: &Options) -> Report {
    let mut rep = Report::default();
    let nl = netlist(&d.schematic);
    let mut next_x = opts.place_at.x;
    let mut linked: BTreeSet<uuid::Uuid> = BTreeSet::new();
    let mut symbols: Vec<_> = d.schematic.sheets.iter().flat_map(|s| s.symbols.clone()).collect();
    symbols.sort_by_key(|s| crate::connectivity::natural(s.reference()));
    // The report lists every symbol processed, then what changed.
    let mut actions: Vec<String> = vec![];
    for s in &symbols {
        let def = d.schematic.symbol(&s.symbol);
        if !s.on_board || def.is_some_and(|x| x.power) {
            continue;
        }
        let (r, fpid) = (s.reference().to_string(), s.footprint().to_string());
        rep.messages.push(format!("Processing symbol '{r}:{fpid}'."));
        if fpid.is_empty() {
            rep.errors.push(format!("No footprint assigned to {r}."));
            continue;
        }
        let existing = d.board.footprints.iter().position(|f| if opts.relink_by_reference { f.reference() == r } else { f.symbol == Some(s.id) });
        let Some(def_fp) = lib.footprint(&fpid) else {
            rep.errors.push(format!("Cannot add {r} (no footprint '{fpid}' found)."));
            continue;
        };
        // New footprints' nets aren't reported (their "Add" line says it).
        let is_new = existing.is_none();
        let i = match existing {
            Some(i) => {
                if opts.replace_footprints && d.board.footprints[i].footprint.id != fpid {
                    let old = d.board.footprints[i].footprint.id.clone();
                    let mut fp = def_fp.clone();
                    set_text_field(&mut fp, fields::REFERENCE, &r);
                    d.board.footprints[i].footprint = fp;
                    actions.push(format!("Change {r} footprint from '{old}' to '{fpid}'."));
                }
                d.board.footprints[i].symbol = Some(s.id);
                i
            }
            None => {
                let mut fp = def_fp.clone();
                set_text_field(&mut fp, fields::REFERENCE, &r);
                let w = width(&fp);
                let at = Pt::new(next_x + w / 2, opts.place_at.y);
                next_x += w + mm(2.0);
                d.board.footprints.push(PlacedFootprint {
                    id: uuid::Uuid::new_v4(),
                    footprint: fp,
                    placement: FootprintPlacement { at, angle: 0.0, side: Default::default() },
                    locked: false,
                    symbol: Some(s.id),
                });
                actions.push(format!("Add {r} (footprint '{fpid}')."));
                rep.added.push(r.clone());
                d.board.footprints.len() - 1
            }
        };
        linked.insert(d.board.footprints[i].id);
        let f = &mut d.board.footprints[i];
        if opts.update_fields {
            if !is_new && f.reference() != r {
                actions.push(format!("Change {} reference designator to {r}.", f.reference()));
            }
            set_text_field(&mut f.footprint, fields::REFERENCE, &r);
            let value = s.value().to_string();
            if !is_new && f.footprint.field(fields::VALUE).is_some_and(|v| v.text.text.text != value) {
                actions.push(format!("Change {r} value to '{value}'."));
            }
            set_text_field(&mut f.footprint, fields::VALUE, &value);
            f.footprint.attrs.dnp = s.dnp;
            f.footprint.attrs.exclude_from_bom = !s.in_bom;
        }
        // Nets, pad by pad.
        let symdef = def;
        for pad in &mut f.footprint.pads {
            if pad.number.is_empty() {
                continue;
            }
            let net = nl.net_of(&r, &pad.number).map(str::to_string);
            if pad.net != net {
                if let Some(n) = net.as_ref().filter(|_| !is_new) {
                    actions.push(format!("Connect {r} pad {} to {n}.", pad.number));
                }
                pad.net = net;
            }
            if let Some(pin) = symdef.and_then(|d| d.pins.iter().find(|p| p.number == pad.number)) {
                pad.pin_function = pin.name.clone();
            }
        }
    }
    if opts.delete_unused {
        let gone: Vec<String> = d.board.footprints.iter().filter(|f| f.symbol.is_some() && !linked.contains(&f.id)).map(|f| f.reference().to_string()).collect();
        for r in &gone {
            actions.push(format!("Remove {r}."));
        }
        d.board.footprints.retain(|f| f.symbol.is_none() || linked.contains(&f.id));
    }
    let mut nets: BTreeSet<String> = d.board.footprints.iter().flat_map(|f| f.footprint.pads.iter().filter_map(|p| p.net.clone())).collect();
    nets.extend(d.board.tracks.iter().map(|t| t.net.clone()));
    nets.remove("");
    d.board.nets = nets.into_iter().collect();
    rep.messages.extend(actions);
    rep.messages.push(String::new());
    rep.messages.push(format!("Total warnings: {}, errors: {}.", rep.warnings.len(), rep.errors.len()));
    rep
}
