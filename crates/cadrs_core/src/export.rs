//! Exporting parts to STEP (right-click a part → Export…, as in Onshape).
//!
//! - The kernel writes the parts' exact B-rep bodies (OpenCascade's STEP writer, AP214, mm) on
//!   the rebuild worker ([`crate::rebuild::export_step`]), one file per part or all of them in
//!   one file.
//! - **Y axis up**: the bodies are turned −90° about X first, so the Part Studio's Top (+Z)
//!   becomes +Y, for tools that expect Y up.
//! - Each STEP product is named after its part ([`name_products`]); OpenCascade's writer calls
//!   them all "Open CASCADE STEP translator …".
//! - File names: the name typed in the dialog (the Part Studio's by default) with " - <part
//!   name>" appended when the file holds one part ([`file_name`]), made safe for the file system
//!   and never overwriting a file already there ([`unique_path`]).

use std::path::{Path, PathBuf};

use crate::ids::PartId;

/// What to export.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRequest {
    /// The parts, with the names their STEP products get (their names in the Parts list).
    pub parts: Vec<(PartId, String)>,
    /// Turn the models so +Y is up (the Part Studio's +Z).
    pub y_up: bool,
    /// One file per part, else one file with every part.
    pub individual: bool,
}

/// One STEP file's contents.
#[derive(Debug, Clone, PartialEq)]
pub struct StepFile {
    /// The part's name when the file holds one part, else `None`.
    pub part: Option<String>,
    pub bytes: Vec<u8>,
}

/// The file name (without extension) of a file holding `part` (or several parts, with `None`).
pub fn file_name(base: &str, part: Option<&str>) -> String {
    let base = base.trim();
    let name = match part {
        Some(p) if base.is_empty() => p.to_string(),
        Some(p) => format!("{base} - {p}"),
        None if base.is_empty() => "Export".to_string(),
        None => base.to_string(),
    };
    sanitize(&name)
}

/// `name` with the characters Windows and Linux refuse in file names replaced by "_".
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Windows drops trailing dots and spaces.
    let s = s.trim_end_matches(['.', ' ']).trim_start();
    if s.is_empty() { "Export".into() } else { s.into() }
}

/// `<dir>/<stem>.step`, or `<dir>/<stem> (2).step`, … if that file already exists.
pub fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.step"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).step")))
        .find(|p| !p.exists())
        .expect("an unused name")
}

/// `<dir>/<stem>.<ext>`, or `<dir>/<stem> (2).<ext>`, … if that file already exists (P3F.2: the
/// other export formats).
pub fn unique_path_ext(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name")
}

/// Where exported files go: `$CADRS_EXPORT_DIR` if set (scenarios and tests), else the user's
/// Downloads folder, else their home folder.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CADRS_EXPORT_DIR").filter(|d| !d.is_empty()) {
        return Some(dir.into());
    }
    let dirs = directories::UserDirs::new()?;
    Some(
        dirs.download_dir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dirs.home_dir().to_path_buf()),
    )
}

/// Names the STEP file's products: the `i`-th `PRODUCT(...)` entity gets `names[i]` as its id
/// and name (the writer emits one product per shape, in order). Products past the end of
/// `names` are left as they are.
pub fn name_products(step: &str, names: &[&str]) -> String {
    const ENTITY: &str = "PRODUCT(";
    let mut out = String::with_capacity(step.len());
    let mut rest = step;
    let mut i = 0;
    while let Some(at) = rest.find(ENTITY) {
        let open = at + ENTITY.len();
        // `#7 = PRODUCT(`, not the tail of a longer entity name.
        let whole = rest[..at].ends_with([' ', '=']);
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        if !whole {
            continue;
        }
        let Some(name) = names.get(i) else {
            continue;
        };
        i += 1;
        // Replace the first two string arguments (id and name).
        let Some(after) = skip_strings(rest, 2) else {
            continue;
        };
        let quoted = step_string(name);
        out.push_str(&format!("{quoted},{quoted}"));
        rest = &rest[after..];
    }
    out.push_str(rest);
    out
}

/// The byte offset just after `n` comma-separated STEP strings at the start of `s` (before the
/// comma that follows the last one). The writer breaks long lines between arguments.
fn skip_strings(s: &str, n: usize) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let skip_space = |i: &mut usize| {
        while b.get(*i).is_some_and(|c| c.is_ascii_whitespace()) {
            *i += 1;
        }
    };
    for k in 0..n {
        skip_space(&mut i);
        if k > 0 {
            if b.get(i) != Some(&b',') {
                return None;
            }
            i += 1;
            skip_space(&mut i);
        }
        if b.get(i) != Some(&b'\'') {
            return None;
        }
        i += 1;
        loop {
            match b.get(i)? {
                // '' is an escaped quote.
                b'\'' if b.get(i + 1) == Some(&b'\'') => i += 2,
                b'\'' => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
    }
    Some(i)
}

/// A STEP string literal: quotes doubled, non-ASCII as `\X2\…\X0\` (ISO 10303-21 UTF-16).
fn step_string(s: &str) -> String {
    let mut out = String::from("'");
    let mut wide = String::new();
    let flush = |out: &mut String, wide: &mut String| {
        if !wide.is_empty() {
            out.push_str("\\X2\\");
            out.push_str(wide);
            out.push_str("\\X0\\");
            wide.clear();
        }
    };
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            flush(&mut out, &mut wide);
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                c => out.push(c),
            }
        } else if !c.is_control() {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                wide.push_str(&format!("{u:04X}"));
            }
        }
    }
    flush(&mut out, &mut wide);
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_append_the_part() {
        assert_eq!(file_name("Part Studio 1", Some("Part 1")), "Part Studio 1 - Part 1");
        assert_eq!(file_name("Part Studio 1", None), "Part Studio 1");
        assert_eq!(file_name("  ", Some("Part 1")), "Part 1");
        assert_eq!(file_name("", None), "Export");
        assert_eq!(file_name("a/b: c?", Some("x|y")), "a_b_ c_ - x_y");
        assert_eq!(sanitize("name. "), "name");
    }

    #[test]
    fn unique_paths_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("cadrs-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a"), dir.join("a.step"));
        std::fs::write(dir.join("a.step"), "").unwrap();
        assert_eq!(unique_path(&dir, "a"), dir.join("a (2).step"));
        std::fs::write(dir.join("a (2).step"), "").unwrap();
        assert_eq!(unique_path(&dir, "a"), dir.join("a (3).step"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn products_get_the_part_names() {
        // As OpenCascade writes them: spaces around "=", a line break between arguments.
        let step = "#6 = PRODUCT_DEFINITION_FORMATION('','',#7);\n\
                    #7 = PRODUCT('Open CASCADE STEP translator 7.8 1',\n  'Open CASCADE STEP translator 7.8 1','',(#8));\n\
                    #20 = PRODUCT('Open CASCADE STEP translator 7.8 2',\n  'Open CASCADE STEP translator 7.8 2','',(#8));\n\
                    #30 = PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#7));\n";
        let named = name_products(step, &["Part 1", "Bob's bracket"]);
        assert_eq!(
            named,
            "#6 = PRODUCT_DEFINITION_FORMATION('','',#7);\n\
             #7 = PRODUCT('Part 1','Part 1','',(#8));\n\
             #20 = PRODUCT('Bob''s bracket','Bob''s bracket','',(#8));\n\
             #30 = PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#7));\n"
        );
        // Extra products keep their names.
        assert_eq!(name_products(step, &["A"]).matches("Open CASCADE").count(), 2);
    }

    #[test]
    fn non_ascii_names_are_encoded() {
        assert_eq!(step_string("Ø10"), "'\\X2\\00D8\\X0\\10'");
        assert_eq!(step_string("a\\b"), "'a\\\\b'");
    }
}
