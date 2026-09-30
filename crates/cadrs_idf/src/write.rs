//! Writer for `.emn` and `.emp`, IDF 2.0 and 3.0.
//!
//! The writer writes [`Board::for_version`] / [`Library::for_version`], so whatever a version
//! can't hold is dropped or converted in one documented place. Numbers use Rust's shortest
//! round-trip formatting (`{}`; `{:e}` for very small or very large magnitudes), so
//! `parse(write(x))` gives back bit-identical `f64`s.

use std::fmt::Write as _;

use crate::geom::Loop;
use crate::model::*;

/// Format a number with the shortest representation that parses back to the same `f64`.
/// Integers print without a decimal point (`0`, `270`), tiny values in exponent form
/// (`-5.55e-15`), everything else in full precision (`0.0620000000000028`). `-0.0` prints `0`.
pub fn fmt_num(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let a = v.abs();
    if !(1e-5..1e16).contains(&a) { format!("{v:e}") } else { format!("{v}") }
}

/// Quote a string value if it needs it (whitespace, empty, or a leading `"`, `#` or `.`).
/// IDF can't escape `"` inside a string, so it is replaced with `'`.
pub fn quote(s: &str) -> String {
    let s = clean(s);
    if s.is_empty() || s.chars().any(char::is_whitespace) || s.starts_with('#') || s.starts_with('.') {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// Replace what a record can't hold: `"` becomes `'`, line breaks become spaces.
fn clean(s: &str) -> String {
    s.chars().map(|c| if c == '"' { '\'' } else if c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

fn header_line(out: &mut String, h: &Header) {
    let date = if h.date.is_empty() { DEFAULT_DATE } else { h.date.as_str() };
    let _ = writeln!(
        out,
        "{} {} \"{}\" {} {}",
        h.file_type.keyword(),
        h.version.as_str(),
        clean(&h.source_system),
        quote(date),
        h.file_version
    );
}

fn section_open(out: &mut String, name: &str, owner: Owner, v: IdfVersion) {
    match v {
        IdfVersion::V3 => {
            let _ = writeln!(out, ".{name} {}", owner.keyword());
        }
        IdfVersion::V2 => {
            let _ = writeln!(out, ".{name}");
        }
    }
}

fn write_loops(out: &mut String, loops: &[Loop]) {
    for l in loops {
        for p in &l.points {
            let _ = writeln!(out, "{} {} {} {}", l.label, fmt_num(p.x), fmt_num(p.y), fmt_num(p.angle));
        }
    }
}

/// Write a board file. See [`Board::for_version`] for what each version keeps.
pub fn write_emn(board: &Board, version: IdfVersion) -> String {
    let b = board.for_version(version);
    let v = version;
    let mut o = String::new();
    o.push_str(".HEADER\n");
    header_line(&mut o, &b.header);
    let _ = writeln!(o, "\"{}\" {}", clean(&b.name), b.units.keyword());
    o.push_str(".END_HEADER\n");

    if let Some(ol) = &b.outline {
        let name = if b.header.file_type == FileType::Panel { "PANEL_OUTLINE" } else { "BOARD_OUTLINE" };
        section_open(&mut o, name, ol.owner, v);
        let _ = writeln!(o, "{}", fmt_num(ol.thickness));
        write_loops(&mut o, &ol.loops);
        let _ = writeln!(o, ".END_{name}");
    }
    for s in &b.other_outlines {
        section_open(&mut o, "OTHER_OUTLINE", s.owner, v);
        match s.side {
            Some(side) => {
                let _ = writeln!(o, "{} {} {}", quote(&s.id), fmt_num(s.thickness), side.keyword());
            }
            None => {
                let _ = writeln!(o, "{} {}", quote(&s.id), fmt_num(s.thickness));
            }
        }
        write_loops(&mut o, &s.loops);
        o.push_str(".END_OTHER_OUTLINE\n");
    }
    for s in &b.route_outlines {
        section_open(&mut o, "ROUTE_OUTLINE", s.owner, v);
        if let Some(l) = s.layers {
            let _ = writeln!(o, "{}", l.keyword());
        }
        write_loops(&mut o, &s.loops);
        o.push_str(".END_ROUTE_OUTLINE\n");
    }
    for s in &b.place_outlines {
        section_open(&mut o, "PLACE_OUTLINE", s.owner, v);
        if let Some(side) = s.side {
            match s.height {
                Some(h) => {
                    let _ = writeln!(o, "{} {}", side.keyword(), fmt_num(h));
                }
                None => {
                    let _ = writeln!(o, "{}", side.keyword());
                }
            }
        }
        write_loops(&mut o, &s.loops);
        o.push_str(".END_PLACE_OUTLINE\n");
    }
    for s in &b.route_keepouts {
        section_open(&mut o, "ROUTE_KEEPOUT", s.owner, v);
        let _ = writeln!(o, "{}", s.layers.keyword());
        write_loops(&mut o, &s.loops);
        o.push_str(".END_ROUTE_KEEPOUT\n");
    }
    for s in &b.via_keepouts {
        section_open(&mut o, "VIA_KEEPOUT", s.owner, v);
        write_loops(&mut o, &s.loops);
        o.push_str(".END_VIA_KEEPOUT\n");
    }
    for s in &b.place_keepouts {
        section_open(&mut o, "PLACE_KEEPOUT", s.owner, v);
        let mut rec = s.side.keyword().to_string();
        if let Some(h) = s.height {
            let _ = write!(rec, " {}", fmt_num(h));
            if let Some(m) = s.min_height {
                let _ = write!(rec, " {}", fmt_num(m));
            }
        }
        let _ = writeln!(o, "{rec}");
        write_loops(&mut o, &s.loops);
        o.push_str(".END_PLACE_KEEPOUT\n");
    }
    for s in &b.place_regions {
        section_open(&mut o, "PLACE_REGION", s.owner, v);
        let _ = writeln!(o, "{} {}", s.side.keyword(), quote(&s.group));
        write_loops(&mut o, &s.loops);
        o.push_str(".END_PLACE_REGION\n");
    }

    o.push_str(".DRILLED_HOLES\n");
    for h in &b.holes {
        let assoc = match &h.assoc {
            HoleAssoc::Board => "BOARD".to_string(),
            HoleAssoc::NoRefdes => "NOREFDES".to_string(),
            HoleAssoc::Panel => "PANEL".to_string(),
            HoleAssoc::Refdes(r) => quote(r),
        };
        let _ = write!(o, "{} {} {} {} {}", fmt_num(h.dia), fmt_num(h.x), fmt_num(h.y), h.plating.keyword(), assoc);
        if v == IdfVersion::V3 {
            let kind = match &h.kind {
                Some(HoleKind::Pin) => "PIN".to_string(),
                Some(HoleKind::Via) => "VIA".to_string(),
                Some(HoleKind::Mtg) => "MTG".to_string(),
                Some(HoleKind::Tool) => "TOOL".to_string(),
                Some(HoleKind::Other(s)) => quote(s),
                None => "OTHER".to_string(),
            };
            let _ = write!(o, " {} {}", kind, h.owner.keyword());
        }
        o.push('\n');
    }
    o.push_str(".END_DRILLED_HOLES\n");

    if !b.notes.is_empty() {
        o.push_str(".NOTES\n");
        for n in &b.notes {
            let _ = writeln!(
                o,
                "{} {} {} {} \"{}\"",
                fmt_num(n.x),
                fmt_num(n.y),
                fmt_num(n.text_height),
                fmt_num(n.text_length),
                clean(&n.text)
            );
        }
        o.push_str(".END_NOTES\n");
    }

    o.push_str(".PLACEMENT\n");
    for p in &b.placements {
        let _ = writeln!(o, "{} {} {}", quote(&p.package), quote(&p.part_number), quote(&p.refdes));
        match v {
            IdfVersion::V3 => {
                let _ = writeln!(
                    o,
                    "{} {} {} {} {} {}",
                    fmt_num(p.x),
                    fmt_num(p.y),
                    fmt_num(p.mount_offset),
                    fmt_num(p.rotation),
                    p.side.keyword(),
                    p.status.keyword()
                );
            }
            IdfVersion::V2 => {
                let _ = writeln!(
                    o,
                    "{} {} {} {} {}",
                    fmt_num(p.x),
                    fmt_num(p.y),
                    fmt_num(p.rotation),
                    p.side.keyword(),
                    p.status.keyword()
                );
            }
        }
    }
    o.push_str(".END_PLACEMENT\n");
    o
}

/// Write a library file. See [`Library::for_version`].
pub fn write_emp(library: &Library, version: IdfVersion) -> String {
    let l = library.for_version(version);
    let mut o = String::new();
    o.push_str(".HEADER\n");
    header_line(&mut o, &l.header);
    o.push_str(".END_HEADER\n");
    for p in &l.packages {
        let name = match p.kind {
            PackageKind::Electrical => "ELECTRICAL",
            PackageKind::Mechanical => "MECHANICAL",
        };
        let _ = writeln!(o, ".{name}");
        let _ = writeln!(o, "{} {} {} {}", quote(&p.name), quote(&p.part_number), p.units.keyword(), fmt_num(p.height));
        write_loops(&mut o, &p.loops);
        for (k, val) in &p.props {
            let _ = writeln!(o, "PROP {} {}", quote(k), quote(val));
        }
        let _ = writeln!(o, ".END_{name}");
    }
    o
}
