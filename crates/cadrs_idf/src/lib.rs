//! cadrs_idf: IDF 2.0 / 3.0 (Intermediate Data Format) parser, writer and zip export for PCB
//! Studio (stage 3H, P3H.1). No Bevy.
//!
//! An IDF board is two files: the board file `.emn` (outline, keep areas, drilled holes,
//! notes, component placements) and the library file `.emp` (each package's outline and
//! height). See [`model`] for the data model (X2), [`geom`] for loop geometry, [`parse_emn`] /
//! [`parse_emp`] to read, [`write_emn`] / [`write_emp`] to write, and [`write_zip`] for PCB
//! Studio's "Export this board to IDF" (PCB9.7–9.8).
//!
//! # Spec notes
//!
//! Checked against *Intermediate Data Format, Mechanical Data Exchange Specification*,
//! Version 2.0 Revision 3 (January 5, 1993) and Version 3.0 Revision 1 (October 31, 1996),
//! both public and non-proprietary.
//!
//! **Both versions** share the sectioned, free-format layout: `.SECTION` / `.END_SECTION`
//! keywords (case-insensitive), one record per line, blank-separated fields, `"..."` for
//! strings with blanks, `#` comment lines. Loops are `label x y angle` records; 0 = line,
//! other = arc with that included angle, positive counter-clockwise. The header is
//! `BOARD_FILE <ver> "<source>" <date> <file version>` then `<board name> <units>` (library
//! files have no second record). The spec's date format is `yyyy/mm/dd.hh:mm:ss`, although both
//! specs' own examples write `mm/dd/yy.hh:mm:ss`; the date is kept as a string. Bottom-side
//! components are flipped about their local Y axis, and rotations are counter-clockwise in the
//! component's own frame (see [`Placement::place_loops`]).
//!
//! **IDF 2.0 already has keep areas.** Its board file has HEADER, BOARD_OUTLINE,
//! OTHER_OUTLINE, ROUTE_OUTLINE, PLACE_OUTLINE, ROUTE_KEEPOUT, VIA_KEEPOUT, PLACE_KEEPOUT,
//! PLACE_REGION, DRILLED_HOLES and PLACEMENT. So the course's statement (PCB9.7) that "IDF
//! 2.0 can't carry keep-in/keep-out areas" is **not true of the format**; it describes Onshape
//! PCB Studio's 2.0 exporter. What 2.0 lacks, compared with 3.0:
//! - owner fields (MCAD/ECAD/UNOWNED) on section keywords and holes;
//! - PANEL_FILE / PANEL_OUTLINE and the NOTES section;
//! - 360° circles (2.0 writes a circle as two 180° arcs, as its own example does);
//! - OTHER_OUTLINE's board side; ROUTE_OUTLINE's layer record (2.0: all layers);
//!   PLACE_OUTLINE's side/height record (2.0: both sides); the INNER routing layer;
//! - PLACE_KEEPOUT takes *maximum and minimum* heights in 2.0, a single height in 3.0;
//! - drilled holes have 5 fields in 2.0 (no hole type PIN/VIA/MTG/TOOL, no owner, no PANEL);
//! - placements have no mounting offset in 2.0 (`x y rotation side status`), and the status is
//!   PLACED/UNPLACED/FIXED (may be blank = placed) instead of 3.0's PLACED/UNPLACED/MCAD/ECAD;
//! - library PROP records.
//!
//! **Units:** 2.0 allows `MM`, `THOU` and `TNM` (ten nanometres, 1e-8 m); 3.0 dropped TNM and
//! allows only `MM` and `THOU`. There is no `INCH` in either version. Library units are per
//! package in both.
//!
//! **Our export decisions.** [`write_emn`] follows the spec of the chosen version (a 2.0 file
//! keeps its keepouts in 2.0 syntax). [`write_zip`] is PCB Studio's export: it stamps our own
//! source string ([`CADRS_SOURCE`], never "Onshape"), and for IDF 2.0 it also drops every keep
//! area (route/place outlines, route/via/place keepouts, place regions), to match what the
//! course documents for PCB Studio's 2.0 export and what its UI tells the user ("IDF 3.0
//! supports keep-outs"). [`export_board`] is that projection on its own.

pub mod geom;
pub mod model;
pub mod parse;
pub mod write;
pub mod zip;

pub use geom::{BBox, Loop, LoopPoint, P2, Segment, loops_bbox};
pub use model::*;
pub use parse::{IdfError, Parsed, Warning, parse_emn, parse_emp};
pub use write::{fmt_num, write_emn, write_emp};
pub use zip::zip_entries;

/// Parse a board file and its library together (warnings are dropped; use [`parse_emn`] /
/// [`parse_emp`] to see them). Library errors are prefixed with `.emp:`.
pub fn read_pair(emn: &str, emp: &str) -> Result<(Board, Library), IdfError> {
    let board = parse_emn(emn).map_err(|e| IdfError::new(e.line, format!(".emn: {}", e.message)))?.value;
    let library = parse_emp(emp).map_err(|e| IdfError::new(e.line, format!(".emp: {}", e.message)))?.value;
    Ok((board, library))
}

/// The board as PCB Studio exports it at `version`: our source string in the header, and for
/// IDF 2.0 no keep areas (see the crate docs).
pub fn export_board(board: &Board, version: IdfVersion) -> Board {
    let mut b = if version == IdfVersion::V2 { board.without_keep_areas() } else { board.clone() };
    b.header.source_system = CADRS_SOURCE.to_string();
    b.for_version(version)
}

/// The library as PCB Studio exports it (our source string, same date as the board).
pub fn export_library(library: &Library, board: &Board, version: IdfVersion) -> Library {
    let mut l = library.clone();
    l.header.source_system = CADRS_SOURCE.to_string();
    l.header.date = board.header.date.clone();
    l.for_version(version)
}

/// Entry paths inside the export zip: `<name>/<name>.emn` and `<name>/<name>.emp`.
pub fn zip_paths(board_name: &str) -> (String, String) {
    let n = safe_file_name(board_name);
    (format!("{n}/{n}.emn"), format!("{n}/{n}.emp"))
}

/// A board name usable as a file/folder name (path separators and control characters become
/// `_`; empty becomes `board`).
pub fn safe_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() || s == "." || s == ".." { "board".to_string() } else { s }
}

/// "Export this board to IDF" (PCB9.7–9.8): a zip holding `<board>/<board>.emn` and
/// `<board>/<board>.emp`, so it unpacks to a folder named after the board.
pub fn write_zip(board: &Board, library: &Library, version: IdfVersion) -> Vec<u8> {
    let b = export_board(board, version);
    let l = export_library(library, &b, version);
    let (emn, emp) = zip_paths(&b.name);
    let dos = zip::dos_datetime(&b.header.date);
    zip::build_zip(&[(emn, write_emn(&b, version).into_bytes()), (emp, write_emp(&l, version).into_bytes())], dos)
}

/// Read a zip holding one `.emn` and one `.emp` (in any folder).
pub fn read_zip(bytes: &[u8]) -> Result<(Board, Library), IdfError> {
    let entries = zip_entries(bytes)?;
    let find = |ext: &str| {
        entries
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase().ends_with(ext))
            .map(|(_, d)| String::from_utf8_lossy(d).into_owned())
            .ok_or_else(|| IdfError::new(0, format!("zip: no {ext} file")))
    };
    read_pair(&find(".emn")?, &find(".emp")?)
}
