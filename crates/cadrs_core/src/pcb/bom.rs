//! The PCB Studio BOM (PCB3.9, PCB4.7): the active board's components grouped by part number
//! and package, with their quantity and designators.
//!
//! - Rows are ordered by their first designator (natural order: C2 before C10), and each row's
//!   designators likewise.
//! - [`designator_list`] writes a row's designators as a comma list, with runs of three or more
//!   consecutive numbers of one prefix as a range: `C1-C10`, `J1, J2`, `R1-R3, R7`.

use std::cmp::Ordering;

use super::board::{ItemId, PcbBoard};
use crate::library::natural_cmp;

/// One BOM row: every component of one part number and package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BomRow {
    pub package: String,
    pub part_number: String,
    /// The components, in designator order.
    pub items: Vec<ItemId>,
    /// Their designators, in the same order.
    pub refdes: Vec<String>,
}

impl BomRow {
    pub fn quantity(&self) -> usize {
        self.items.len()
    }

    /// The Designator cell: [`designator_list`] of the row.
    pub fn designators(&self) -> String {
        designator_list(&self.refdes)
    }
}

/// A designator's prefix and number: `R10` → ("R", Some(10)).
fn split(r: &str) -> (&str, Option<u64>) {
    let i = r.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (p, n) = r.split_at(i);
    // "R01" is not in a run with "R2".
    let n = if n.is_empty() || (n.len() > 1 && n.starts_with('0')) { None } else { n.parse().ok() };
    (p, n)
}

/// The designators as a comma list with ranges (see the module docs). `refdes` must be in
/// natural order.
pub fn designator_list(refdes: &[String]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < refdes.len() {
        let (p, n) = split(&refdes[i]);
        let mut j = i;
        if let Some(n) = n {
            while j + 1 < refdes.len() {
                let (q, m) = split(&refdes[j + 1]);
                if q == p && m == Some(n + (j + 1 - i) as u64) {
                    j += 1;
                } else {
                    break;
                }
            }
        }
        if j - i >= 2 {
            parts.push(format!("{}-{}", refdes[i], refdes[j]));
        } else {
            parts.extend(refdes[i..=j].iter().cloned());
        }
        i = j + 1;
    }
    parts.join(", ")
}

/// The board's BOM (see the module docs).
pub fn bom(board: &PcbBoard) -> Vec<BomRow> {
    let mut rows: Vec<BomRow> = Vec::new();
    let mut comps: Vec<(ItemId, &cadrs_idf::Placement)> = board.components().collect();
    comps.sort_by(|a, b| natural_cmp(&a.1.refdes, &b.1.refdes).then_with(|| a.0.cmp(&b.0)));
    for (id, p) in comps {
        match rows.iter_mut().find(|r| r.part_number == p.part_number && r.package == p.package) {
            Some(r) => {
                r.items.push(id);
                r.refdes.push(p.refdes.clone());
            }
            None => rows.push(BomRow { package: p.package.clone(), part_number: p.part_number.clone(), items: vec![id], refdes: vec![p.refdes.clone()] }),
        }
    }
    rows.sort_by(|a, b| match (a.refdes.first(), b.refdes.first()) {
        (Some(x), Some(y)) => natural_cmp(x, y),
        _ => Ordering::Equal,
    });
    rows
}
