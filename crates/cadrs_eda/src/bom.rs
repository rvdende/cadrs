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

/// The LCSC part number fields JLCPCB reads (the first a symbol has): `LCSC`, `LCSC Part`,
/// `LCSC Part #`, `JLCPCB Part #`.
const LCSC_FIELDS: [&str; 4] = ["LCSC", "LCSC Part", "LCSC Part #", "JLCPCB Part #"];

/// JLCPCB's assembly BOM: Comment (the value), Designator (the references), Footprint (its name
/// without the library), LCSC Part #; one row per value, footprint and part number, parts not
/// fitted (DNP) left out.
pub fn jlcpcb_csv(sch: &Schematic) -> String {
    let mut rows: Vec<(String, Vec<String>, String, String)> = vec![];
    for s in sch.sheets.iter().flat_map(|s| &s.symbols) {
        let def = sch.symbol(&s.symbol);
        if !s.in_bom || s.dnp || def.is_some_and(|d| d.power) || s.reference().starts_with('#') {
            continue;
        }
        let footprint = s.footprint().rsplit(':').next().unwrap_or("").to_string();
        let lcsc = LCSC_FIELDS.iter().find_map(|n| s.field(n).map(|f| f.value().trim().to_string()).filter(|v| !v.is_empty())).unwrap_or_default();
        let key = (s.value().to_string(), footprint, lcsc);
        match rows.iter_mut().find(|r| (&r.0, &r.2, &r.3) == (&key.0, &key.1, &key.2)) {
            Some(r) => r.1.push(s.reference().into()),
            None => rows.push((key.0, vec![s.reference().into()], key.1, key.2)),
        }
    }
    for r in &mut rows {
        r.1.sort_by_key(|x| natural(x));
    }
    rows.sort_by_key(|r| natural(&r.1[0]));
    let mut out = String::from("Comment,Designator,Footprint,LCSC Part #\n");
    for (value, refs, fp, lcsc) in rows {
        out.push_str(&[quote(&value), quote(&refs.join(",")), quote(&fp), quote(&lcsc)].join(","));
        out.push('\n');
    }
    out
}
