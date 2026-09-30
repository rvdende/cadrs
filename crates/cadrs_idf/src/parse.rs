//! Parser for `.emn` (board/panel) and `.emp` (library) files, IDF 2.0 and 3.0.
//!
//! Tolerant where the spec leaves room: keywords in any case, `#` comment lines (anywhere, not
//! only column 1), blank lines, CRLF, any amount of whitespace, quoted strings with spaces, an
//! optional owner field, missing optional trailing fields, and records laid out as either
//! version. Unknown sections are skipped with a warning. Real errors carry the 1-based line.

use std::fmt;

use crate::geom::{Loop, LoopPoint};
use crate::model::*;

/// A parse error at a 1-based line (0 when there is no line, e.g. a zip error).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdfError {
    pub line: usize,
    pub message: String,
}

impl IdfError {
    pub fn new(line: usize, message: impl Into<String>) -> IdfError {
        IdfError { line, message: message.into() }
    }
}

impl fmt::Display for IdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 { write!(f, "{}", self.message) } else { write!(f, "line {}: {}", self.line, self.message) }
    }
}

impl std::error::Error for IdfError {}

/// A non-fatal problem (for example an unknown section that was skipped).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub line: usize,
    pub message: String,
}

/// A parsed value plus the warnings collected on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed<T> {
    pub value: T,
    pub warnings: Vec<Warning>,
}

type Result<T> = std::result::Result<T, IdfError>;

#[derive(Clone, Debug)]
struct Tok {
    text: String,
    quoted: bool,
}

#[derive(Clone, Debug)]
struct Rec {
    line: usize,
    toks: Vec<Tok>,
}

impl Rec {
    fn len(&self) -> usize {
        self.toks.len()
    }

    fn s(&self, i: usize, what: &str) -> Result<&str> {
        self.toks
            .get(i)
            .map(|t| t.text.as_str())
            .ok_or_else(|| IdfError::new(self.line, format!("missing {what}")))
    }

    fn opt(&self, i: usize) -> Option<&str> {
        self.toks.get(i).map(|t| t.text.as_str())
    }

    fn f(&self, i: usize, what: &str) -> Result<f64> {
        let s = self.s(i, what)?;
        num(s).ok_or_else(|| IdfError::new(self.line, format!("expected a number for {what}, found '{s}'")))
    }

    /// Rest of the record from token `i`, joined with spaces.
    fn rest(&self, i: usize) -> String {
        self.toks.iter().skip(i).map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    /// Number of leading tokens that parse as numbers.
    fn numeric_prefix(&self) -> usize {
        self.toks.iter().take_while(|t| !t.quoted && num(&t.text).is_some()).count()
    }

    fn keyword(&self) -> Option<String> {
        let t = self.toks.first()?;
        (!t.quoted && t.text.starts_with('.')).then(|| t.text[1..].to_ascii_uppercase())
    }
}

fn num(s: &str) -> Option<f64> {
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

fn tokenize(line: &str, lineno: usize) -> Result<Vec<Tok>> {
    let mut toks = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some(ch) => s.push(ch),
                    None => return Err(IdfError::new(lineno, "unterminated quoted string")),
                }
            }
            toks.push(Tok { text: s, quoted: true });
        } else {
            let mut s = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                s.push(ch);
                chars.next();
            }
            toks.push(Tok { text: s, quoted: false });
        }
    }
    Ok(toks)
}

fn records(text: &str) -> Result<Vec<Rec>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let toks = tokenize(line, i + 1)?;
        if !toks.is_empty() {
            out.push(Rec { line: i + 1, toks });
        }
    }
    Ok(out)
}

struct Section {
    name: String,
    head: Rec,
    body: Vec<Rec>,
}

fn sections(recs: Vec<Rec>) -> Result<Vec<Section>> {
    let mut out = Vec::new();
    let mut it = recs.into_iter();
    while let Some(head) = it.next() {
        let Some(name) = head.keyword() else {
            return Err(IdfError::new(head.line, format!("data outside a section: '{}'", head.rest(0))));
        };
        if name.starts_with("END_") {
            return Err(IdfError::new(head.line, format!(".{name} without a matching section start")));
        }
        let end = format!("END_{name}");
        let mut body = Vec::new();
        let mut closed = false;
        for r in it.by_ref() {
            match r.keyword() {
                Some(k) if k == end => {
                    closed = true;
                    break;
                }
                Some(k) => {
                    return Err(IdfError::new(r.line, format!("expected .{end} before .{k} (section .{name} started at line {})", head.line)));
                }
                None => body.push(r),
            }
        }
        if !closed {
            return Err(IdfError::new(head.line, format!("section .{name} has no .{end}")));
        }
        out.push(Section { name, head, body });
    }
    Ok(out)
}

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn parse_version(r: &Rec) -> Result<IdfVersion> {
    let s = r.s(1, "IDF version")?;
    match num(s) {
        Some(3.0) => Ok(IdfVersion::V3),
        Some(2.0) => Ok(IdfVersion::V2),
        _ => Err(IdfError::new(r.line, format!("unsupported IDF version '{s}' (2.0 and 3.0 are supported)"))),
    }
}

fn parse_units(r: &Rec, i: usize) -> Result<Units> {
    let s = r.s(i, "units")?;
    if eq(s, "MM") {
        Ok(Units::Mm)
    } else if eq(s, "THOU") {
        Ok(Units::Thou)
    } else if eq(s, "TNM") {
        Ok(Units::Tnm)
    } else {
        Err(IdfError::new(r.line, format!("unknown units '{s}' (expected MM, THOU or TNM)")))
    }
}

fn parse_owner(r: &Rec, i: usize) -> Result<Owner> {
    match r.opt(i) {
        None => Ok(Owner::Unowned),
        Some(s) if eq(s, "MCAD") => Ok(Owner::Mcad),
        Some(s) if eq(s, "ECAD") => Ok(Owner::Ecad),
        Some(s) if eq(s, "UNOWNED") => Ok(Owner::Unowned),
        Some(s) => Err(IdfError::new(r.line, format!("unknown owner '{s}' (expected MCAD, ECAD or UNOWNED)"))),
    }
}

fn parse_side(r: &Rec, i: usize) -> Result<Side> {
    let s = r.s(i, "board side")?;
    if eq(s, "TOP") {
        Ok(Side::Top)
    } else if eq(s, "BOTTOM") {
        Ok(Side::Bottom)
    } else if eq(s, "BOTH") {
        Ok(Side::Both)
    } else {
        Err(IdfError::new(r.line, format!("unknown board side '{s}' (expected TOP, BOTTOM or BOTH)")))
    }
}

fn parse_mount_side(r: &Rec, i: usize) -> Result<MountSide> {
    let s = r.s(i, "side")?;
    if eq(s, "TOP") {
        Ok(MountSide::Top)
    } else if eq(s, "BOTTOM") {
        Ok(MountSide::Bottom)
    } else {
        Err(IdfError::new(r.line, format!("unknown side '{s}' (expected TOP or BOTTOM)")))
    }
}

fn parse_layers(r: &Rec, i: usize) -> Result<Layers> {
    let s = r.s(i, "routing layers")?;
    Ok(match s.to_ascii_uppercase().as_str() {
        "TOP" => Layers::Top,
        "BOTTOM" => Layers::Bottom,
        "BOTH" => Layers::Both,
        "INNER" => Layers::Inner,
        "ALL" => Layers::All,
        _ => return Err(IdfError::new(r.line, format!("unknown routing layers '{s}'"))),
    })
}

/// Loop point records `label x y angle`, grouped into loops. A new loop starts when the label
/// changes or the current loop is already closed.
fn parse_loops(recs: &[Rec]) -> Result<Vec<Loop>> {
    let mut loops: Vec<Loop> = Vec::new();
    for r in recs {
        let ls = r.s(0, "loop label")?;
        let label: u32 = ls
            .parse()
            .ok()
            .or_else(|| num(ls).filter(|v| v.fract() == 0.0 && *v >= 0.0).map(|v| v as u32))
            .ok_or_else(|| IdfError::new(r.line, format!("expected a loop label (integer), found '{ls}'")))?;
        let p = LoopPoint::new(r.f(1, "X coordinate")?, r.f(2, "Y coordinate")?, r.f(3, "included angle")?);
        match loops.last_mut() {
            Some(l) if l.label == label && !l.is_closed() => l.points.push(p),
            _ => loops.push(Loop { label, points: vec![p] }),
        }
    }
    Ok(loops)
}

/// True if the record starts like a loop point (`int num num num`).
fn is_point_record(r: &Rec) -> bool {
    r.numeric_prefix() >= 4
}

fn parse_header(sec: &Section, expect: &[FileType]) -> Result<(Header, Option<(String, Units)>)> {
    let r = sec.body.first().ok_or_else(|| IdfError::new(sec.head.line, "empty .HEADER"))?;
    let ft = r.s(0, "file type")?;
    let file_type = if eq(ft, "BOARD_FILE") {
        FileType::Board
    } else if eq(ft, "PANEL_FILE") {
        FileType::Panel
    } else if eq(ft, "LIBRARY_FILE") {
        FileType::Library
    } else {
        return Err(IdfError::new(r.line, format!("unknown file type '{ft}'")));
    };
    if !expect.contains(&file_type) {
        let want: Vec<_> = expect.iter().map(|f| f.keyword()).collect();
        return Err(IdfError::new(r.line, format!("file type is {ft}, expected {}", want.join(" or "))));
    }
    let version = parse_version(r)?;
    let source_system = r.opt(2).unwrap_or("").to_string();
    let date = r.opt(3).unwrap_or("").to_string();
    let file_version = match r.opt(4) {
        None => 1,
        Some(s) => s.parse().map_err(|_| IdfError::new(r.line, format!("expected a file version number, found '{s}'")))?,
    };
    let header = Header { file_type, version, source_system, date, file_version };
    let board = if file_type == FileType::Library {
        None
    } else {
        let r3 = sec.body.get(1).ok_or_else(|| IdfError::new(r.line, "missing board name and units record"))?;
        Some((r3.s(0, "board name")?.to_string(), parse_units(r3, 1)?))
    };
    Ok((header, board))
}

fn first_section_is_header(secs: &[Section]) -> Result<()> {
    match secs.first() {
        Some(s) if s.name == "HEADER" => Ok(()),
        Some(s) => Err(IdfError::new(s.head.line, format!("expected .HEADER as the first section, found .{}", s.name))),
        None => Err(IdfError::new(1, "empty file (no .HEADER)")),
    }
}

/// Parse a board or panel file (`.emn`).
pub fn parse_emn(text: &str) -> Result<Parsed<Board>> {
    let secs = sections(records(text)?)?;
    first_section_is_header(&secs)?;
    let (header, nb) = parse_header(&secs[0], &[FileType::Board, FileType::Panel])?;
    let (name, units) = nb.expect("board header has a name");
    let mut b = Board::new(&name, units, header.version);
    b.header = header;
    let mut warnings = Vec::new();
    for sec in &secs[1..] {
        let head = &sec.head;
        let body = &sec.body;
        match sec.name.as_str() {
            "BOARD_OUTLINE" | "PANEL_OUTLINE" => {
                let r = body.first().ok_or_else(|| IdfError::new(head.line, "missing thickness"))?;
                let o = BoardOutline { owner: parse_owner(head, 1)?, thickness: r.f(0, "thickness")?, loops: parse_loops(&body[1..])? };
                if b.outline.is_some() {
                    warnings.push(Warning { line: head.line, message: format!("extra .{} ignored", sec.name) });
                } else {
                    b.outline = Some(o);
                }
            }
            "OTHER_OUTLINE" => {
                let r = body.first().ok_or_else(|| IdfError::new(head.line, "missing outline identifier record"))?;
                let side = if r.len() > 2 { Some(parse_mount_side(r, 2)?) } else { None };
                b.other_outlines.push(OtherOutline {
                    owner: parse_owner(head, 1)?,
                    id: r.s(0, "outline identifier")?.to_string(),
                    thickness: r.f(1, "extrude thickness")?,
                    side,
                    loops: parse_loops(&body[1..])?,
                });
            }
            "ROUTE_OUTLINE" => {
                let (layers, pts) = match body.first() {
                    Some(r) if !is_point_record(r) => (Some(parse_layers(r, 0)?), &body[1..]),
                    _ => (None, &body[..]),
                };
                b.route_outlines.push(RouteOutline { owner: parse_owner(head, 1)?, layers, loops: parse_loops(pts)? });
            }
            "PLACE_OUTLINE" => {
                let (side, height, pts) = match body.first() {
                    Some(r) if !is_point_record(r) => {
                        let h = if r.len() > 1 { Some(r.f(1, "outline height")?) } else { None };
                        (Some(parse_side(r, 0)?), h, &body[1..])
                    }
                    _ => (None, None, &body[..]),
                };
                b.place_outlines.push(PlaceOutline { owner: parse_owner(head, 1)?, side, height, loops: parse_loops(pts)? });
            }
            "ROUTE_KEEPOUT" => {
                let r = body.first().ok_or_else(|| IdfError::new(head.line, "missing routing layers"))?;
                b.route_keepouts.push(RouteKeepout { owner: parse_owner(head, 1)?, layers: parse_layers(r, 0)?, loops: parse_loops(&body[1..])? });
            }
            "VIA_KEEPOUT" => {
                b.via_keepouts.push(ViaKeepout { owner: parse_owner(head, 1)?, loops: parse_loops(body)? });
            }
            "PLACE_KEEPOUT" => {
                let r = body.first().ok_or_else(|| IdfError::new(head.line, "missing board side"))?;
                let height = if r.len() > 1 { Some(r.f(1, "keepout height")?) } else { None };
                let min_height = if r.len() > 2 { Some(r.f(2, "minimum height")?) } else { None };
                b.place_keepouts.push(PlaceKeepout {
                    owner: parse_owner(head, 1)?,
                    side: parse_side(r, 0)?,
                    height,
                    min_height,
                    loops: parse_loops(&body[1..])?,
                });
            }
            "PLACE_REGION" => {
                let r = body.first().ok_or_else(|| IdfError::new(head.line, "missing board side"))?;
                b.place_regions.push(PlaceRegion {
                    owner: parse_owner(head, 1)?,
                    side: parse_side(r, 0)?,
                    group: r.opt(1).unwrap_or("").to_string(),
                    loops: parse_loops(&body[1..])?,
                });
            }
            "DRILLED_HOLES" => {
                for r in body {
                    b.holes.push(parse_hole(r)?);
                }
            }
            "NOTES" => {
                for r in body {
                    b.notes.push(Note {
                        x: r.f(0, "note X")?,
                        y: r.f(1, "note Y")?,
                        text_height: r.f(2, "text height")?,
                        text_length: r.f(3, "text length")?,
                        text: r.rest(4),
                    });
                }
            }
            "PLACEMENT" => {
                if body.len() % 2 != 0 {
                    let last = body.last().expect("odd length is non-empty");
                    return Err(IdfError::new(last.line, "placement records come in pairs (package part refdes / x y ... side status)"));
                }
                for pair in body.chunks(2) {
                    b.placements.push(parse_placement(&pair[0], &pair[1])?);
                }
            }
            "HEADER" => return Err(IdfError::new(head.line, "second .HEADER")),
            other => warnings.push(Warning { line: head.line, message: format!("unknown section .{other} skipped") }),
        }
    }
    Ok(Parsed { value: b, warnings })
}

fn parse_hole(r: &Rec) -> Result<DrilledHole> {
    let pl = r.s(3, "plating style")?;
    let plating = if eq(pl, "PTH") {
        Plating::Pth
    } else if eq(pl, "NPTH") {
        Plating::Npth
    } else {
        return Err(IdfError::new(r.line, format!("unknown plating '{pl}' (expected PTH or NPTH)")));
    };
    let a = r.s(4, "associated part")?;
    let assoc = if eq(a, "BOARD") {
        HoleAssoc::Board
    } else if eq(a, "NOREFDES") {
        HoleAssoc::NoRefdes
    } else if eq(a, "PANEL") {
        HoleAssoc::Panel
    } else {
        HoleAssoc::Refdes(a.to_string())
    };
    let kind = r.opt(5).map(|k| match k.to_ascii_uppercase().as_str() {
        "PIN" => HoleKind::Pin,
        "VIA" => HoleKind::Via,
        "MTG" => HoleKind::Mtg,
        "TOOL" => HoleKind::Tool,
        _ => HoleKind::Other(k.to_string()),
    });
    Ok(DrilledHole {
        dia: r.f(0, "hole diameter")?,
        x: r.f(1, "hole X")?,
        y: r.f(2, "hole Y")?,
        plating,
        assoc,
        kind,
        owner: parse_owner(r, 6)?,
    })
}

fn parse_placement(a: &Rec, r: &Rec) -> Result<Placement> {
    // 3.0: x y offset rotation side status; 2.0: x y rotation side [status].
    let n = r.numeric_prefix();
    let (mount_offset, rot_i) = if n >= 4 { (r.f(2, "mounting offset")?, 3) } else { (0.0, 2) };
    let side_i = rot_i + 1;
    let status = match r.opt(side_i + 1) {
        None => Status::Placed,
        Some(s) => match s.to_ascii_uppercase().as_str() {
            "PLACED" => Status::Placed,
            "UNPLACED" => Status::Unplaced,
            "MCAD" => Status::Mcad,
            "ECAD" => Status::Ecad,
            "FIXED" => Status::Fixed,
            _ => return Err(IdfError::new(r.line, format!("unknown placement status '{s}'"))),
        },
    };
    Ok(Placement {
        package: a.s(0, "package name")?.to_string(),
        part_number: a.s(1, "part number")?.to_string(),
        refdes: a.s(2, "reference designator")?.to_string(),
        x: r.f(0, "placement X")?,
        y: r.f(1, "placement Y")?,
        mount_offset,
        rotation: r.f(rot_i, "rotation")?,
        side: parse_mount_side(r, side_i)?,
        status,
    })
}

/// Parse a library file (`.emp`).
pub fn parse_emp(text: &str) -> Result<Parsed<Library>> {
    let secs = sections(records(text)?)?;
    first_section_is_header(&secs)?;
    let (header, _) = parse_header(&secs[0], &[FileType::Library])?;
    let mut lib = Library { header, packages: vec![] };
    let mut warnings = Vec::new();
    for sec in &secs[1..] {
        let kind = match sec.name.as_str() {
            "ELECTRICAL" => PackageKind::Electrical,
            "MECHANICAL" => PackageKind::Mechanical,
            "HEADER" => return Err(IdfError::new(sec.head.line, "second .HEADER")),
            other => {
                warnings.push(Warning { line: sec.head.line, message: format!("unknown section .{other} skipped") });
                continue;
            }
        };
        let r = sec.body.first().ok_or_else(|| IdfError::new(sec.head.line, "missing geometry name record"))?;
        let mut pts = Vec::new();
        let mut props = Vec::new();
        for rec in &sec.body[1..] {
            if rec.toks.first().is_some_and(|t| !t.quoted && eq(&t.text, "PROP")) {
                props.push((rec.s(1, "property name")?.to_string(), rec.rest(2)));
            } else {
                pts.push(rec.clone());
            }
        }
        lib.packages.push(Package {
            kind,
            name: r.s(0, "geometry name")?.to_string(),
            part_number: r.s(1, "part number")?.to_string(),
            units: parse_units(r, 2)?,
            height: r.f(3, "component height")?,
            loops: parse_loops(&pts)?,
            props,
        });
    }
    Ok(Parsed { value: lib, warnings })
}
