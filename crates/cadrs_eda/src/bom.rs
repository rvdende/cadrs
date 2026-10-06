//! Bill of materials (GS12): one row per part kind, grouped by value, footprint, datasheet and
//! do-not-populate; power ports, flags and parts left out of the BOM are skipped.

use crate::connectivity::natural;
use crate::schematic::Schematic;
use crate::symbol::fields;

#[derive(Clone, Debug, PartialEq)]
pub struct BomRow {
    pub references: Vec<String>,
    pub value: String,
    pub footprint: String,
    pub datasheet: String,
    pub dnp: bool,
}

impl BomRow {
    pub fn qty(&self) -> usize {
        self.references.len()
    }
}

/// The rows, in reference order.
pub fn rows(sch: &Schematic) -> Vec<BomRow> {
    let mut rows: Vec<BomRow> = vec![];
    for s in sch.sheets.iter().flat_map(|s| &s.symbols) {
        let def = sch.symbol(&s.symbol);
        if !s.in_bom || def.is_some_and(|d| d.power) || s.reference().starts_with('#') {
            continue;
        }
        let datasheet = s.field(fields::DATASHEET).map_or("", |f| f.value());
        let datasheet = if datasheet == "~" { "" } else { datasheet };
        let key = (s.value(), s.footprint(), datasheet, s.dnp);
        match rows.iter_mut().find(|r| (r.value.as_str(), r.footprint.as_str(), r.datasheet.as_str(), r.dnp) == key) {
            Some(r) => r.references.push(s.reference().into()),
            None => rows.push(BomRow { references: vec![s.reference().into()], value: key.0.into(), footprint: key.1.into(), datasheet: key.2.into(), dnp: s.dnp }),
        }
    }
    for r in &mut rows {
        r.references.sort_by_key(|x| natural(x));
    }
    rows.sort_by_key(|r| natural(&r.references[0]));
    rows
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// CSV with the columns of a spreadsheet BOM: Reference, Value, Datasheet, Footprint, Qty, DNP.
pub fn csv(sch: &Schematic) -> String {
    let mut out = String::from("\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\",\"DNP\"\n");
    for r in rows(sch) {
        out.push_str(&[
            quote(&r.references.join(",")),
            quote(&r.value),
            quote(&r.datasheet),
            quote(&r.footprint),
            quote(&r.qty().to_string()),
            quote(if r.dnp { "DNP" } else { "" }),
        ]
        .join(","));
        out.push('\n');
    }
    out
}
