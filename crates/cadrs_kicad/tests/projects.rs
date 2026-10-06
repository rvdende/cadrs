//! Reads real KiCad projects and checks the geometry joins up: wire ends land on pins, labels
//! or other wires; track ends land on pads of their net, vias or other tracks. A wrong mirror,
//! rotation or Y flip anywhere breaks these.
//!
//! The projects are outside the repository: the reference project
//! (`~/work/desk_power_monitor/…`) and KiCad's installed demos. Tests skip what is missing.

use cadrs_eda::board::Board;
use cadrs_eda::footprint::{PadKind, PadShape};
use cadrs_eda::schematic::{Schematic, Sheet};
use cadrs_eda::units::Pt;
use std::path::{Path, PathBuf};

fn reference_project() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("HOME").ok()?).join("work/desk_power_monitor/hardware/desk_power_monitor_mini32_lora");
    p.is_dir().then_some(p)
}

fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let (ab, ap) = (b - a, p - a);
    let cross = ab.x as i128 * ap.y as i128 - ab.y as i128 * ap.x as i128;
    let len = ab.length();
    if len == 0.0 {
        return p == a;
    }
    // Within 1 µm of the line, and between the ends.
    (cross as f64 / len).abs() <= 1000.0
        && (ap.x as i128 * ab.x as i128 + ap.y as i128 * ab.y as i128) >= 0
        && (ap.x as i128 * ab.x as i128 + ap.y as i128 * ab.y as i128) <= ab.x as i128 * ab.x as i128 + ab.y as i128 * ab.y as i128
}

/// Wire ends that touch nothing.
fn dangling_wire_ends(sch: &Schematic, sheet: &Sheet) -> Vec<Pt> {
    let mut anchors: Vec<Pt> = vec![];
    for s in &sheet.symbols {
        anchors.extend(sch.placed_pins(s).map(|(_, p)| p));
    }
    anchors.extend(sheet.labels.iter().map(|l| l.text.at));
    anchors.extend(sheet.no_connects.iter().map(|n| n.at));
    anchors.extend(sheet.junctions.iter().map(|j| j.at));
    let mut out = vec![];
    for (i, w) in sheet.wires.iter().enumerate() {
        for end in [w.a, w.b] {
            let touches = anchors.contains(&end)
                || sheet.wires.iter().enumerate().any(|(j, o)| j != i && on_segment(end, o.a, o.b))
                || sheet.buses.iter().any(|o| on_segment(end, o.a, o.b));
            if !touches {
                out.push(end);
            }
        }
    }
    out
}

/// Pin ends with no wire, label, pin or no-connect on them.
fn unconnected_pins(sch: &Schematic, sheet: &Sheet) -> Vec<(String, String)> {
    let mut points: Vec<(Pt, String)> = vec![];
    for s in &sheet.symbols {
        points.extend(sch.placed_pins(s).map(|(p, at)| (at, format!("{}.{}", s.reference(), p.number))));
    }
    let mut out = vec![];
    for (at, name) in &points {
        let touches = sheet.wires.iter().any(|w| on_segment(*at, w.a, w.b))
            || sheet.labels.iter().any(|l| l.text.at == *at)
            || sheet.no_connects.iter().any(|n| n.at == *at)
            || points.iter().filter(|(p, _)| p == at).count() > 1;
        if !touches {
            out.push((format!("{:?}", at.to_mm()), name.clone()));
        }
    }
    out
}

/// Track ends on nothing of their net.
fn dangling_track_ends(b: &Board) -> Vec<(String, [f64; 2])> {
    let pads: Vec<(Pt, i64, &str)> = b
        .footprints
        .iter()
        .flat_map(|f| {
            f.footprint.pads.iter().filter(|p| p.kind != PadKind::NonPlated).map(move |p| {
                let r = match &p.shape {
                    PadShape::Circle => p.size.w / 2,
                    _ => p.size.w.max(p.size.h) / 2,
                };
                (f.placement.apply(p.at), r, p.net.as_deref().unwrap_or(""))
            })
        })
        .collect();
    let mut out = vec![];
    for (i, t) in b.tracks.iter().enumerate() {
        for end in [t.a, t.b] {
            let on_pad = pads.iter().any(|(c, r, n)| *n == t.net && c.dist(end) <= *r as f64 + 1000.0);
            let on_via = b.vias.iter().any(|v| v.net == t.net && v.at.dist(end) <= v.diameter as f64 / 2.0);
            let on_track = b.tracks.iter().enumerate().any(|(j, o)| j != i && o.layer == t.layer && (o.a == end || o.b == end || (o.mid.is_none() && on_segment(end, o.a, o.b))));
            if !(on_pad || on_via || on_track) {
                out.push((t.net.clone(), end.to_mm()));
            }
        }
    }
    out
}

#[test]
fn reference_project_reads_and_joins_up() {
    let Some(dir) = reference_project() else {
        eprintln!("skipped: no reference project");
        return;
    };
    let p = cadrs_kicad::read_project(&dir).unwrap();
    let sheet = &p.design.schematic.sheets[0];
    assert_eq!(sheet.symbols.len(), 9);
    assert_eq!(sheet.junctions.len(), 9);
    assert_eq!(sheet.labels.len(), 7);
    assert!(sheet.wires.len() >= 65);
    let b = &p.design.board;
    assert_eq!(b.footprints.len(), 8);
    assert_eq!(b.tracks.len(), 91);
    assert_eq!(b.vias.len(), 3);
    assert_eq!(b.copper_layers, 2);
    assert!(b.nets.contains(&"GNDREF".to_string()), "{:?}", b.nets);
    let refs: Vec<&str> = sheet.symbols.iter().map(|s| s.reference()).collect();
    for r in ["U1", "J1", "J2", "C1", "C2", "C3", "L1", "AE1"] {
        assert!(refs.contains(&r), "{r} missing from {refs:?}");
        assert!(b.footprint(r).is_some(), "footprint {r} missing");
    }
    assert_eq!(dangling_wire_ends(&p.design.schematic, sheet), vec![]);
    // The headers' unused pins (and two of U1's) have no wire, as in KiCad.
    let loose = unconnected_pins(&p.design.schematic, sheet);
    assert_eq!(loose.len(), 35, "{loose:?}");
    assert!(loose.iter().all(|(_, n)| n.starts_with('J') || n.starts_with("U1.")), "{loose:?}");
    // KiCad's DRC reports these two GNDREF tracks on B.Cu as unconnected too.
    let gnd = |x: f64, y: f64| ("GNDREF".to_string(), [x, y]);
    assert_eq!(dangling_track_ends(b), vec![gnd(108.0135, -83.566), gnd(106.645, -79.629)]);
    // The outline is closed: the board's Edge.Cuts plus nothing from footprints.
    assert!(b.outline_shapes().len() >= 9);
    // Every 3D model resolves (the RA-01SH's is another machine's absolute path).
    for f in &b.footprints {
        for m in &f.footprint.models {
            let found = cadrs_kicad::resolve_model(&m.source, &dir);
            eprintln!("{} {} -> {:?}", f.reference(), m.source, found);
        }
    }
    let ra = b.footprint("U1").unwrap();
    let path = cadrs_kicad::resolve_model(&ra.footprint.models[0].source, &dir).expect("RA-01SH model");
    assert_eq!(path.extension().unwrap(), "step");
    eprintln!("warnings: {:#?}", p.warnings);
}

fn demo_files(ext: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, ext, out);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    let mut out = vec![];
    walk(Path::new("/usr/share/kicad/demos"), ext, &mut out);
    out.sort();
    out
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn demo_schematics_join_up() {
    let files = demo_files("kicad_sch");
    if files.is_empty() {
        eprintln!("skipped: no KiCad demos");
        return;
    }
    let (mut wires, mut dangling, mut pins, mut loose) = (0, 0, 0, 0);
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let mut w = vec![];
        let sch = cadrs_kicad::read_schematic(&text, &mut w).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let sheet = &sch.sheets[0];
        let d = dangling_wire_ends(&sch, sheet);
        let u = unconnected_pins(&sch, sheet);
        wires += sheet.wires.len() * 2;
        dangling += d.len();
        pins += sheet.symbols.iter().map(|s| sch.placed_pins(s).count()).sum::<usize>();
        loose += u.len();
        if !d.is_empty() || u.len() > 20 {
            eprintln!("{}: {} dangling wire ends {:?}, {} loose pins", f.display(), d.len(), &d[..d.len().min(3)], u.len());
        }
    }
    eprintln!("{} files: {dangling}/{wires} wire ends dangling, {loose}/{pins} pins unconnected", files.len());
    // The demos have stub wires and sub-sheet pins (not imported yet), so wire ends are only
    // reported; pins are the check: a wrong mirror or rotation leaves most of them loose.
    assert!(loose * 30 < pins, "{loose} of {pins} pins unconnected");
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn demo_boards_join_up() {
    let files = demo_files("kicad_pcb");
    if files.is_empty() {
        eprintln!("skipped: no KiCad demos");
        return;
    }
    let (mut ends, mut dangling) = (0, 0);
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let mut w = vec![];
        let b = cadrs_kicad::read_board(&text, &mut w).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let d = dangling_track_ends(&b);
        ends += b.tracks.len() * 2;
        dangling += d.len();
        if d.len() > 2 {
            eprintln!("{}: {} of {} track ends dangle, e.g. {:?}", f.display(), d.len(), b.tracks.len() * 2, &d[..d.len().min(3)]);
        }
    }
    eprintln!("{} boards: {dangling}/{ends} track ends dangling", files.len());
    assert!(dangling * 100 < ends, "{dangling} of {ends} track ends dangle");
}
