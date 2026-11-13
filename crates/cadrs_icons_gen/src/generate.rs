//! Generates the icon set.
//!
//!     cargo run -r -p cadrs_icons_gen                                # everything: icons/, icons/dark/, src/generated.rs
//!     cargo run -r -p cadrs_icons_gen -- --check                     # fail if any of those are out of date
//!     cargo run -r -p cadrs_icons_gen -- --only sketch --out DIR     # one module's SVGs into DIR (and DIR/dark)
//!
//! Each module in MODULES lists its icons in `icons()`. The run validates every icon (unique
//! names, palette colours allowed for its kind, well-formed XML) before writing.

use crate::common::{self, Def, Kind, f};
use crate::icons;
use crate::palette::Palette;
use resvg::{tiny_skia, usvg};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A module's name and its icon list.
type Module = (&'static str, fn() -> Vec<Def>);

/// Toolbar order: modules in this order, icons in each module in definition order.
pub const MODULES: &[Module] = &[
    ("part", icons::part::icons),
    ("sheet_metal", icons::sheet_metal::icons),
    ("assembly", icons::assembly::icons),
    ("sketch", icons::sketch::icons),
    ("glyphs", icons::glyphs::icons),
    ("ui_a", icons::ui_a::icons),
    ("ui_b", icons::ui_b::icons),
    ("eda", icons::eda::icons),
    ("cam", icons::cam::icons),
    ("platform", icons::platform::icons),
];

// Solid and sketch icons are drawn on the shared 24-unit grid but rarely fill it, which makes them
// hard to read at toolbar size. The fitting pass zooms each one's view box to its ink: a square
// around the ink plus FIT_MARGIN on every side, never zoomed in more than MAX_ZOOM. Line and glyph
// icons keep the standard UI padding.
const FIT_MARGIN: f64 = 1.0;
const MAX_ZOOM: f64 = 1.5;

struct Entry {
    module: &'static str,
    def: Def,
    svg: String,
}

fn render(def: &Def) -> String {
    let body = (def.draw)()
        .into_iter()
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n  ");
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\" \
         fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" \
         stroke-linecap=\"round\">\n  {body}\n</svg>\n",
        common::INK,
        def.kind.stroke()
    )
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every `#rrggbb` (six hex digits not followed by a word character), with its byte offset.
fn colours(svg: &str) -> Vec<(usize, &str)> {
    let b = svg.as_bytes();
    let mut out = Vec::new();
    for (i, _) in svg.match_indices('#') {
        let hex = b.get(i + 1..i + 7);
        if hex.is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit))
            && !b.get(i + 7).is_some_and(|&c| is_word(c))
        {
            out.push((i, &svg[i..i + 7]));
        }
    }
    out
}

fn validate(e: &Entry) -> Vec<String> {
    let (name, kind, svg) = (e.def.name, e.def.kind, &e.svg);
    let mut problems = Vec::new();
    if !name.split('-').all(|w| {
        !w.is_empty()
            && w.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    }) {
        problems.push("name must be kebab-case".to_owned());
    }
    if let Err(err) = usvg::roxmltree::Document::parse(svg) {
        problems.push(format!("bad XML: {err}"));
    }
    let mut seen: Vec<&str> = colours(svg).into_iter().map(|(_, c)| c).collect();
    seen.sort();
    seen.dedup();
    for colour in seen {
        if !kind.colours().contains(&colour) {
            problems.push(format!("colour {colour} not allowed for kind {kind:?}"));
        }
    }
    let b = svg.as_bytes();
    if svg
        .match_indices("id=")
        .any(|(i, _)| i == 0 || !is_word(b[i - 1]))
    {
        problems.push(
            "ids are not allowed (they clash when icons are inlined); avoid clipPath/defs"
                .to_owned(),
        );
    }
    problems
        .into_iter()
        .map(|p| format!("{}.{name}: {p}", e.module))
        .collect()
}

/// The dark variant: every light palette colour swapped for its dark slot, in one pass, so a dark
/// colour equal to a later light colour is not replaced again.
pub fn dark(svg: &str) -> String {
    let map: HashMap<String, String> = Palette::LIGHT
        .slots()
        .iter()
        .zip(Palette::DARK.slots())
        .map(|(l, d)| (l.to_hex(), d.to_hex()))
        .collect();
    let mut out = String::with_capacity(svg.len());
    let mut last = 0;
    for (i, c) in colours(svg) {
        if c.bytes().skip(1).all(|b| !b.is_ascii_uppercase()) {
            out.push_str(&svg[last..i]);
            out.push_str(map.get(c).map_or(c, String::as_str));
            last = i + 7;
        }
    }
    out.push_str(&svg[last..]);
    out
}

/// The ink bounds (x0, y0, x1, y1) of an SVG, in view-box units. Bounds come from rendering at 10x
/// and finding pixels with any coverage, so strokes, caps and joins are included exactly.
fn ink_bounds(svg: &str) -> (f64, f64, f64, f64) {
    const SCALE: f32 = 10.0;
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("validated SVG");
    let side = (24.0 * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(side, side).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, p) in pixmap.pixels().iter().enumerate() {
        if p.alpha() > 8 {
            let (x, y) = (i as u32 % side, i as u32 / side);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    // Through f32 and its shortest decimal form, which the committed view boxes were fitted with.
    let unit = |v: u32| (v as f32 / SCALE).to_string().parse::<f64>().unwrap();
    (unit(x0), unit(y0), unit(x1), unit(y1))
}

/// Zooms each fitted icon's view box to its ink.
fn fit(entries: &mut [Entry]) {
    for e in entries
        .iter_mut()
        .filter(|e| matches!(e.def.kind, Kind::Solid | Kind::Sketch))
    {
        let (x0, y0, x1, y1) = ink_bounds(&e.svg);
        let side = 24f64.min(((x1 - x0).max(y1 - y0) + 2.0 * FIT_MARGIN).max(24.0 / MAX_ZOOM));
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let vb = format!(
            "{} {} {} {}",
            f(cx - side / 2.0),
            f(cy - side / 2.0),
            f(side),
            f(side)
        );
        e.svg = e
            .svg
            .replacen(r#"viewBox="0 0 24 24""#, &format!(r#"viewBox="{vb}""#), 1);
    }
}

fn rust(entries: &[Entry]) -> String {
    let mut lines = vec![
        "// Generated by `cargo run -r -p cadrs_icons_gen`. Do not edit.".to_owned(),
        String::new(),
        "icons! {".to_owned(),
    ];
    for e in entries {
        let konst = e.def.name.to_uppercase().replace('-', "_");
        let title = e.def.title.replace('\\', "\\\\").replace('"', "\\\"");
        lines.push(format!(
            r#"    {konst} => ("{}", {}, "{title}"),"#,
            e.def.name,
            e.def.kind.rust()
        ));
    }
    lines.push("}".to_owned());
    lines.join("\n") + "\n"
}

pub fn main(root: &Path, args: &[String]) -> Result<(), String> {
    let (mut only, mut out, mut check) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--only" => only = Some(it.next().ok_or("--only needs a module")?.clone()),
            "--out" => out = Some(PathBuf::from(it.next().ok_or("--out needs a directory")?)),
            "--check" => check = true,
            _ => {
                return Err(format!(
                    "unknown argument {a}\nusage: cargo run -r -p cadrs_icons_gen -- [--only MODULE] [--out DIR] [--check]"
                ));
            }
        }
    }

    let modules: Vec<_> = match &only {
        Some(m) => vec![
            *MODULES
                .iter()
                .find(|(n, _)| n == m)
                .ok_or_else(|| format!("no module {m}"))?,
        ],
        None => MODULES.to_vec(),
    };
    let (mut entries, mut problems, mut seen) = (Vec::new(), Vec::new(), HashMap::new());
    for (module, icons) in modules {
        for def in icons() {
            if let Some(first) = seen.get(def.name) {
                problems.push(format!(
                    "{module}.{}: duplicate of {first}.{}",
                    def.name, def.name
                ));
                continue;
            }
            seen.insert(def.name, module);
            // Report every broken icon, not just the first.
            let svg = match std::panic::catch_unwind(|| render(&def)) {
                Ok(svg) => svg,
                Err(_) => {
                    problems.push(format!("{module}.{}: panicked", def.name));
                    continue;
                }
            };
            let e = Entry { module, def, svg };
            problems.extend(validate(&e));
            entries.push(e);
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    fit(&mut entries);

    // Every file the run owns, with its new contents (None to delete).
    let dir = out.unwrap_or_else(|| root.join("icons"));
    let mut files: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    if only.is_none() {
        // A full run owns the output directory: remove files for icons that no longer exist.
        for sub in [dir.clone(), dir.join("dark")] {
            for entry in std::fs::read_dir(&sub).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|x| x == "svg")
                    && !seen.contains_key(p.file_stem().unwrap().to_str().unwrap())
                {
                    files.insert(p, None);
                }
            }
        }
    }
    for e in &entries {
        files.insert(dir.join(format!("{}.svg", e.def.name)), Some(e.svg.clone()));
        files.insert(
            dir.join("dark").join(format!("{}.svg", e.def.name)),
            Some(dark(&e.svg)),
        );
    }
    if only.is_none() && dir == root.join("icons") {
        files.insert(root.join("src").join("generated.rs"), Some(rust(&entries)));
    }

    if check {
        let stale: Vec<String> = files
            .iter()
            .filter(|(p, want)| std::fs::read_to_string(p).ok() != **want)
            .map(|(p, _)| p.strip_prefix(root).unwrap_or(p).display().to_string())
            .collect();
        if !stale.is_empty() {
            return Err(format!(
                "out of date (run `cargo run -r -p cadrs_icons_gen`):\n  {}",
                stale.join("\n  ")
            ));
        }
        println!("{} icons up to date", entries.len());
        return Ok(());
    }
    std::fs::create_dir_all(dir.join("dark")).map_err(|e| e.to_string())?;
    for (p, contents) in &files {
        match contents {
            Some(c) => std::fs::write(p, c),
            None => std::fs::remove_file(p),
        }
        .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    println!("{} icons -> {}", entries.len(), dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_the_crate() {
        let ours = [
            common::INK,
            common::TOP,
            common::SOFT,
            common::MID,
            common::SHADE,
            common::ACCENT,
        ];
        let crate_light: Vec<String> = Palette::LIGHT.slots().iter().map(|c| c.to_hex()).collect();
        assert_eq!(ours.to_vec(), crate_light);
    }

    #[test]
    fn dark_swaps_in_one_pass() {
        assert_eq!(
            dark(r##"<a stroke="#262626" fill="#ffffff" x="#2563ebz"/>"##),
            r##"<a stroke="#e4e4e7" fill="#a0a0a2" x="#2563ebz"/>"##
        );
    }

    #[test]
    fn formats_coordinates() {
        assert_eq!(f(-0.001), "0");
        assert_eq!(f(2.5), "2.5");
        assert_eq!(f(3.0), "3");
        assert_eq!(f(0.125), "0.12");
    }
}
