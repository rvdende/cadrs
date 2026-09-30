//! Import ECAD files (PCB4.3–4.4): which of the picked files belong together, and reading them.
//!
//! An IDF board is a `.emn` board file and a `.emp` library file (PCB4.2). The user picks both
//! (or several pairs) in one go. [`pair_files`] pairs every `.emn` with the `.emp` of the same
//! base name (case-insensitive), or, when exactly one board and one library were picked, with
//! that library whatever its name. A board without a library still imports: its components
//! show as placeholder boxes, with a warning. Other files are skipped with a warning.

use std::path::{Path, PathBuf};

use super::board::PcbBoard;
use super::{BoardSource, PcbStudio};

/// One board to import: its `.emn` and the `.emp` that goes with it, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePair {
    pub emn: PathBuf,
    pub emp: Option<PathBuf>,
}

/// The pairs [`pair_files`] found, and what it had to say about the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pairing {
    pub pairs: Vec<FilePair>,
    pub warnings: Vec<String>,
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// A file's name for messages.
pub fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Pairs the picked files (see the module docs). No `.emn` at all is an error.
pub fn pair_files(paths: &[PathBuf]) -> Result<Pairing, String> {
    let mut out = Pairing::default();
    let emns: Vec<&PathBuf> = paths.iter().filter(|p| ext(p) == "emn").collect();
    let emps: Vec<&PathBuf> = paths.iter().filter(|p| ext(p) == "emp").collect();
    for p in paths.iter().filter(|p| !matches!(ext(p).as_str(), "emn" | "emp")) {
        out.warnings.push(format!("{} is not an IDF file (.emn or .emp); skipped", file_name(p)));
    }
    if emns.is_empty() {
        return Err(if emps.is_empty() {
            "Choose an IDF board file (.emn) and its library file (.emp)".into()
        } else {
            "Choose the board file (.emn) too: a library file (.emp) alone has no board".into()
        });
    }
    let mut used = vec![false; emps.len()];
    for emn in &emns {
        let same = emps.iter().position(|e| stem(e) == stem(emn));
        let emp = match same {
            Some(i) => Some(i),
            None if emns.len() == 1 && emps.len() == 1 => Some(0),
            None => None,
        };
        if let Some(i) = emp {
            used[i] = true;
        } else {
            out.warnings.push(format!("No library file (.emp) for {}: its components are shown as placeholder boxes", file_name(emn)));
        }
        out.pairs.push(FilePair { emn: (*emn).clone(), emp: emp.map(|i| emps[i].clone()) });
    }
    for (i, e) in emps.iter().enumerate() {
        if !used[i] {
            out.warnings.push(format!("{} has no board file (.emn) with the same name; skipped", file_name(e)));
        }
    }
    Ok(out)
}

/// A board read from IDF text, with the parser's warnings.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadBoard {
    pub board: PcbBoard,
    pub warnings: Vec<String>,
}

/// Reads a board from its `.emn` text and, if there is one, its `.emp` text. `fallback_name`
/// names a board whose file has an empty name (the file's base name). Without a library every
/// component is a placeholder box (see `cadrs_pcb::geometry`), with a warning.
pub fn read_board(emn: &str, emp: Option<&str>, fallback_name: &str) -> Result<ReadBoard, String> {
    let b = cadrs_idf::parse_emn(emn).map_err(|e| format!(".emn: {e}"))?;
    let mut warnings: Vec<String> = b.warnings.iter().map(|w| format!(".emn line {}: {}", w.line, w.message)).collect();
    let mut board = b.value;
    if board.name.trim().is_empty() {
        board.name = fallback_name.to_string();
    }
    let library = match emp {
        Some(text) => {
            let l = cadrs_idf::parse_emp(text).map_err(|e| format!(".emp: {e}"))?;
            warnings.extend(l.warnings.iter().map(|w| format!(".emp line {}: {}", w.line, w.message)));
            l.value
        }
        None => cadrs_idf::Library::new(board.header.version),
    };
    let pcb = PcbBoard::new(&board, &library);
    let missing: Vec<&str> = {
        let mut m: Vec<&str> = pcb
            .board
            .placements
            .iter()
            .filter(|p| !pcb.library.packages.iter().any(|k| k.name == p.package))
            .map(|p| p.package.as_str())
            .collect();
        m.sort();
        m.dedup();
        m
    };
    if !missing.is_empty() && emp.is_some() {
        warnings.push(format!("Not in the library, shown as placeholder boxes: {}", missing.join(", ")));
    }
    Ok(ReadBoard { board: pcb, warnings })
}

/// One board read from files, ready for [`super::ImportBoard`].
#[derive(Clone, Debug, PartialEq)]
pub struct Imported {
    pub board: PcbBoard,
    pub source: BoardSource,
    pub warnings: Vec<String>,
}

/// Reads one pair from disk.
pub fn read_pair(pair: &FilePair) -> Result<Imported, String> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", file_name(p)));
    let emn = read(&pair.emn)?;
    let emp = pair.emp.as_deref().map(read).transpose()?;
    let fallback = pair.emn.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Board".into());
    let r = read_board(&emn, emp.as_deref(), &fallback).map_err(|e| format!("{}: {e}", file_name(&pair.emn)))?;
    Ok(Imported {
        board: r.board,
        source: BoardSource::Idf { emn: file_name(&pair.emn), emp: pair.emp.as_deref().map(file_name) },
        warnings: r.warnings,
    })
}

/// What importing a set of picked files gives: the boards read, and every warning and error
/// (each error names its file; the other boards still import).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub boards: Vec<Imported>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Pairs and reads the picked files.
pub fn import_files(paths: &[PathBuf]) -> ImportReport {
    let mut r = ImportReport::default();
    match pair_files(paths) {
        Err(e) => r.errors.push(e),
        Ok(p) => {
            r.warnings.extend(p.warnings);
            for pair in &p.pairs {
                match read_pair(pair) {
                    Ok(mut b) => {
                        r.warnings.append(&mut b.warnings);
                        r.boards.push(b);
                    }
                    Err(e) => r.errors.push(e),
                }
            }
        }
    }
    r
}

/// "No file chosen" / "Vision PCB.emn" / "2 files chosen": the Choose Files label (PCB4.3).
pub fn chosen_label(paths: &[PathBuf]) -> String {
    match paths {
        [] => "No file chosen".into(),
        [one] => file_name(one),
        many => format!("{} files chosen", many.len()),
    }
}

/// The name the next board would get in `studio` (for a toast).
pub fn import_name(studio: &PcbStudio, board: &PcbBoard) -> String {
    studio.free_name(board.name())
}
