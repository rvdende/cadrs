//! Fixture, hand-derived-value and format tests for cadrs_idf (P3H.1).

use std::f64::consts::PI;
use std::path::PathBuf;

use cadrs_idf::*;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/idf")
}

fn read(rel: &str) -> String {
    let p = fixture_dir().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Parse a fixture pair, asserting there are no warnings.
fn pair(dir: &str, base: &str) -> (Board, Library) {
    let emn = parse_emn(&read(&format!("{dir}/{base}.emn"))).unwrap_or_else(|e| panic!("{dir}/{base}.emn: {e}"));
    let emp = parse_emp(&read(&format!("{dir}/{base}.emp"))).unwrap_or_else(|e| panic!("{dir}/{base}.emp: {e}"));
    assert!(emn.warnings.is_empty(), "{:?}", emn.warnings);
    assert!(emp.warnings.is_empty(), "{:?}", emp.warnings);
    (emn.value, emp.value)
}

const FIXTURES: &[(&str, &str)] = &[
    ("cell phone", "Cell phone"),
    ("secondary board", "secondary board"),
    ("vision controller", "Vision PCB"),
    ("vision controller thou", "Vision PCB"),
    ("v2 sample", "v2 sample"),
];

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[track_caller]
fn assert_close(a: f64, b: f64, tol: f64) {
    assert!(close(a, b, tol), "{a} != {b} (tol {tol})");
}

fn outline(b: &Board) -> &BoardOutline {
    b.outline.as_ref().expect("board has an outline")
}

// ---------------------------------------------------------------- parsing all fixtures

#[test]
fn every_fixture_parses() {
    let counts = [(0, 0), (20, 7), (29, 8), (29, 8), (5, 4)];
    for (&(dir, base), &(placements, packages)) in FIXTURES.iter().zip(&counts) {
        let (b, l) = pair(dir, base);
        assert_eq!(b.placements.len(), placements, "{dir}");
        assert_eq!(l.packages.len(), packages, "{dir}");
        assert!(outline(&b).loops[0].is_closed(), "{dir}");
        // Every placement's package is in the library.
        for p in &b.placements {
            assert!(l.package(&p.package, &p.part_number).is_some(), "{dir}: {} {}", p.package, p.part_number);
        }
        for pk in &l.packages {
            assert!(pk.loops.iter().all(Loop::is_closed), "{dir}: {}", pk.name);
        }
    }
}

#[test]
fn every_fixture_round_trips_in_both_versions() {
    for &(dir, base) in FIXTURES {
        let (b, l) = pair(dir, base);
        for v in [IdfVersion::V2, IdfVersion::V3] {
            let b2 = parse_emn(&write_emn(&b, v)).unwrap().value;
            assert_eq!(b2, b.for_version(v), "{dir} board {v:?}");
            let l2 = parse_emp(&write_emp(&l, v)).unwrap().value;
            assert_eq!(l2, l.for_version(v), "{dir} library {v:?}");
            // A second round trip is stable.
            assert_eq!(write_emn(&b2, v), write_emn(&b, v));
        }
    }
    // The 3.0 fixtures lose nothing going through 3.0.
    for &(dir, base) in &FIXTURES[..4] {
        let (b, l) = pair(dir, base);
        assert_eq!(parse_emn(&write_emn(&b, IdfVersion::V3)).unwrap().value, b);
        assert_eq!(parse_emp(&write_emp(&l, IdfVersion::V3)).unwrap().value, l);
    }
}

#[test]
fn serde_round_trip() {
    let (b, l) = pair("secondary board", "secondary board");
    let s = ron::to_string(&b).unwrap();
    assert_eq!(ron::from_str::<Board>(&s).unwrap(), b);
    let s = ron::to_string(&l).unwrap();
    assert_eq!(ron::from_str::<Library>(&s).unwrap(), l);
}

// ---------------------------------------------------------------- (a) cell phone

#[test]
fn cell_phone_outline_hand_values() {
    let (b, l) = pair("cell phone", "Cell phone");
    assert_eq!(b.name, "Cell phone");
    assert_eq!(b.units, Units::Mm);
    assert_eq!(b.header.version, IdfVersion::V3);
    // The course quirk: 0.062, in mm.
    assert_eq!(outline(&b).thickness, 0.062);
    assert_eq!(b.thickness_mm(), 0.062);
    assert_eq!(outline(&b).owner, Owner::Mcad);
    assert_eq!(outline(&b).loops.len(), 1);
    assert!(b.holes.is_empty() && b.placements.is_empty() && l.packages.is_empty());

    let lp = &outline(&b).loops[0];
    assert_eq!(lp.label, 0);
    assert!(lp.is_closed());
    let bb = lp.bbox();
    assert_eq!(bb.min, [-40.5, -73.0]);
    assert_eq!(bb.max, [40.5, 73.0]);
    assert_eq!(bb.width(), 81.0);
    assert_eq!(bb.height(), 146.0);

    // Four R8 90° corner arcs, centred 8 mm in from each corner.
    let arcs: Vec<_> = lp
        .segments()
        .filter_map(|s| match s {
            Segment::Arc { start, end, center, radius, sweep } => Some((start, end, center, radius, sweep)),
            _ => None,
        })
        .collect();
    assert_eq!(arcs.len(), 4);
    let (start, end, c, r, sweep) = arcs[0];
    assert_eq!((start, end, sweep), ([32.5, -73.0], [40.5, -65.0], 90.0));
    assert_close(c[0], 32.5, 1e-12);
    assert_close(c[1], -65.0, 1e-12);
    assert_close(r, 8.0, 1e-12);
    let want = [[32.5, -65.0], [32.5, 65.0], [-32.5, 65.0], [-32.5, -65.0]];
    for (a, w) in arcs.iter().zip(want) {
        assert_close(a.2[0], w[0], 1e-12);
        assert_close(a.2[1], w[1], 1e-12);
        assert_close(a.3, 8.0, 1e-12);
    }
    assert_eq!(lp.segments().filter(|s| matches!(s, Segment::Line { .. })).count(), 4);

    // Area = 81·146 − (4 − π)·8² = 11826 − 64(4 − π) ≈ 11771.06 mm², counter-clockwise.
    let area = 81.0 * 146.0 - (4.0 - PI) * 64.0;
    assert_close(area, 11771.061929829747, 1e-9);
    assert_close(lp.signed_area(), area, 1e-9);
    assert!(lp.signed_area() > 0.0);
}

#[test]
fn cell_phone_keepouts() {
    let (b, _) = pair("cell phone", "Cell phone");
    assert_eq!(b.place_keepouts.len(), 2);
    let battery = &b.place_keepouts[0];
    let antenna = &b.place_keepouts[1];
    for k in [battery, antenna] {
        assert_eq!((k.owner, k.side, k.height, k.min_height), (Owner::Mcad, Side::Bottom, Some(1.0), None));
    }
    // Battery 60 × 90; antenna 60 × 12 plus a 60 × 6 triangle.
    assert_eq!(battery.loops[0].area(), 5400.0);
    assert_eq!(antenna.loops[0].area(), 900.0);
    assert_eq!(antenna.loops[0].points.len() - 1, 5, "the antenna keep-out has 5 edges");
    // Both lie inside the board outline's bbox.
    let bb = outline(&b).loops[0].bbox();
    for k in &b.place_keepouts {
        let kb = k.loops[0].bbox();
        assert!(kb.min[0] >= bb.min[0] && kb.max[0] <= bb.max[0] && kb.min[1] >= bb.min[1] && kb.max[1] <= bb.max[1]);
    }
}

// ---------------------------------------------------------------- (b) secondary board

fn footprint(b: &Board, l: &Library, p: &Placement) -> BBox {
    loops_bbox(&b.placed_outline(p, l).expect("package in library"))
}

/// The ex3 keep-out: a 0.5 × 0.375 in corner profile with an R0.25 in fillet on its inner
/// corner, at the board's bottom-left corner, counter-clockwise.
fn ex3_keepout(b: &Board) -> Loop {
    let bb = outline(b).loops[0].bbox();
    let (x0, y0) = (bb.min[0], bb.min[1]);
    let (xr, yt, r) = (x0 + 12.7, y0 + 9.525, 6.35);
    Loop::from_triples(0, &[(x0, y0, 0.0), (xr, y0, 0.0), (xr, yt - r, 0.0), (xr - r, yt, 90.0), (x0, yt, 0.0), (x0, y0, 0.0)])
}

#[test]
fn secondary_board_hand_values() {
    let (b, l) = pair("secondary board", "secondary board");
    assert_eq!(b.name, "secondary board");
    assert_eq!(outline(&b).thickness, 0.84);
    let bb = outline(&b).loops[0].bbox();
    assert_eq!(bb.min, [-5.20972, -22.74338]);
    assert_eq!(bb.max, [45.59028, 15.35662]);
    assert_close(bb.width(), 50.8, 1e-9); // 2 in
    assert_close(bb.height(), 38.1, 1e-9); // 1.5 in
    assert!(b.holes.is_empty());
    let refdes: Vec<_> = b.placements.iter().map(|p| p.refdes.clone()).collect();
    assert_eq!(refdes, (0..20).map(|i| format!("X{i}")).collect::<Vec<_>>());
    assert_eq!(b.placements.iter().filter(|p| p.side == MountSide::Bottom).count(), 2);

    let x0 = b.placement("X0").unwrap();
    assert_eq!((x0.package.as_str(), x0.part_number.as_str()), ("BUTTON_EVQPUA02", "5209001"));
    assert_eq!((x0.x, x0.y, x0.mount_offset, x0.rotation), (24.47, -9.48, 0.0, 270.0));
    assert_eq!((x0.side, x0.status), (MountSide::Top, Status::Placed));
    let x1 = b.placement("X1").unwrap();
    assert_eq!((x1.package.as_str(), x1.part_number.as_str()), ("CRYSTAL_CX_4V", "4510219"));
    assert_eq!(x1.mount_offset, -5.55e-15);

    let bga: Vec<_> = b.placements_for("uBGA48_7.4X7.1").collect();
    assert_eq!(bga.len(), 1);
    let u = bga[0];
    assert_eq!((u.x, u.y, u.rotation, u.side, u.status), (4.064182376174947, -16.5, 90.0, MountSide::Top, Status::Placed));
    let pk = l.package("uBGA48_7.4X7.1", &u.part_number).unwrap();
    let pbb = loops_bbox(&pk.loops);
    assert_eq!((pbb.width(), pbb.height()), (7.4, 7.1));
    assert!(pk.height > 0.0);
    // Electrical PROP records.
    let r = l.package("1210_SR73K2E", "3302210").unwrap();
    assert_eq!(r.props[0], ("RESISTANCE".to_string(), "10000".to_string()));

    // Every footprint lies on the board, and no two top-side footprints overlap.
    let fps: Vec<_> = b.placements.iter().map(|p| (p, footprint(&b, &l, p))).collect();
    for (p, f) in &fps {
        assert!(f.min[0] >= bb.min[0] && f.max[0] <= bb.max[0] && f.min[1] >= bb.min[1] && f.max[1] <= bb.max[1], "{} off the board", p.refdes);
    }
    for (i, (p, f)) in fps.iter().enumerate() {
        for (q, g) in &fps[i + 1..] {
            if p.side == MountSide::Top && q.side == MountSide::Top {
                assert!(!f.overlaps(g), "{} overlaps {}", p.refdes, q.refdes);
            }
        }
    }
}

#[test]
fn secondary_board_keepout_interference_and_move() {
    let (mut b, l) = pair("secondary board", "secondary board");
    let ko = ex3_keepout(&b);
    assert!(ko.is_closed());
    // 0.5 × 0.375 in minus the fillet's corner: 12.7·9.525 − (1 − π/4)·6.35².
    assert_close(ko.signed_area(), 12.7 * 9.525 - (1.0 - PI / 4.0) * 6.35 * 6.35, 1e-9);
    let kbb = ko.bbox();

    // The uBGA's footprint (7.1 × 7.4 after the 90° turn) at its original position.
    let u = b.placement("X2").unwrap().clone();
    assert_eq!(u.package, "uBGA48_7.4X7.1");
    let f = footprint(&b, &l, &u);
    assert_close(f.min[0], 4.064182376174947 - 3.55, 1e-12);
    assert_close(f.max[1], -16.5 + 3.7, 1e-12);
    // It is the only top-side component over the keep-out.
    let hits: Vec<_> = b.placements.iter().filter(|p| p.side == MountSide::Top && footprint(&b, &l, p).overlaps(&kbb)).map(|p| p.refdes.clone()).collect();
    assert_eq!(hits, ["X2"]);
    // And it really overlaps the filleted profile, not just its box: (1.0, −19.0) is in both
    // (left of the fillet centre x = 1.14028, so the fillet doesn't remove it).
    assert!(f.min[0] < 1.0 && f.min[1] < -19.0 && kbb.min[0] < 1.0 && kbb.min[1] < -19.0);
    assert!(1.0 < kbb.max[0] - 6.35);

    // Move it +1 in (25.4 mm) in Y: it stays on the board and clears the keep-out and the rest.
    let bbb = outline(&b).loops[0].bbox();
    let moved_y = u.y + 25.4;
    let idx = b.placements.iter().position(|p| p.refdes == "X2").unwrap();
    b.placements[idx].y = moved_y;
    let f2 = footprint(&b, &l, &b.placements[idx]);
    assert!(f2.min[1] >= bbb.min[1] && f2.max[1] <= bbb.max[1]);
    assert!(!f2.overlaps(&kbb));
    for p in &b.placements {
        if p.refdes != "X2" && p.side == MountSide::Top {
            assert!(!footprint(&b, &l, p).overlaps(&f2), "moved X2 hits {}", p.refdes);
        }
    }

    // Add the keep-out and export IDF 3.0: the moved Y and a PLACE_KEEPOUT come back.
    b.place_keepouts.push(PlaceKeepout { owner: Owner::Mcad, side: Side::Top, height: Some(0.0), min_height: None, loops: vec![ko] });
    let (b2, _) = read_zip(&write_zip(&b, &l, IdfVersion::V3)).unwrap();
    let u2 = b2.placement("X2").unwrap();
    assert_eq!((u2.x, u2.y), (4.064182376174947, moved_y));
    assert_close(u2.y, 8.9, 1e-12);
    assert_eq!(b2.place_keepouts.len(), 1);
    assert_eq!(b2.place_keepouts[0].loops[0].points.len(), 6);
}

// ---------------------------------------------------------------- (c) vision controller

#[test]
fn vision_controller_hand_values() {
    let (b, l) = pair("vision controller", "Vision PCB");
    assert_eq!(b.name, "Vision PCB");
    assert_eq!(b.placements.len(), 29);
    let bb = outline(&b).loops[0].bbox();
    assert_eq!((bb.min, bb.max), ([0.0, 0.0], [101.6, 76.2])); // 4 × 3 in
    assert_eq!(b.thickness_mm(), 1.5748); // 62 thou

    // Four NPTH mounting holes, one near each corner.
    assert_eq!(b.holes.len(), 4);
    for h in &b.holes {
        assert_eq!((h.plating, &h.assoc, &h.kind, h.owner), (Plating::Npth, &HoleAssoc::Board, &Some(HoleKind::Mtg), Owner::Mcad));
        assert_eq!(h.dia, 3.175);
        assert!(h.x.min(101.6 - h.x) < 4.0 && h.y.min(76.2 - h.y) < 4.0);
    }
    // Route keepout circles around the holes.
    assert_eq!(b.route_keepouts.len(), 4);
    assert!(b.route_keepouts.iter().all(|k| k.loops[0].is_circle()));
    assert_close(b.route_keepouts[0].loops[0].area(), PI * 3.175 * 3.175, 1e-9);
    assert_eq!(b.notes[0].text, "Vision controller rev A");

    // Two long header strips, 50.8 mm (20 × 2.54).
    let hdr: Vec<_> = b.placements_for("HDR_1X20").collect();
    assert_eq!(hdr.len(), 2);
    for h in hdr {
        let f = footprint(&b, &l, h);
        assert_close(f.width(), 50.8, 1e-9);
        assert_close(f.height(), 2.54, 1e-9);
    }

    // Component A: U1, the large central square IC. Top face 15.24 × 15.24 mm = 232.2576 mm²
    // (0.6 in square, 0.36 in²).
    let a = b.placement("U1").unwrap();
    assert_eq!((a.x, a.y), (bb.width() / 2.0, bb.height() / 2.0));
    let loops = b.placed_outline(a, &l).unwrap();
    assert_close(loops[0].area(), 232.2576, 1e-9);
    assert_close(loops[0].area(), 15.24 * 15.24, 1e-9);
    // The other large square IC is smaller: 10.16 mm square.
    let u2 = b.placed_outline(b.placement("U2").unwrap(), &l).unwrap();
    assert_close(u2[0].area(), 10.16 * 10.16, 1e-9);
    let pk = l.package(&a.package, &a.part_number).unwrap();
    assert_eq!(pk.height, 1.6002);
}

#[test]
fn thou_and_mm_variants_are_the_same_board() {
    let (mm, lmm) = pair("vision controller", "Vision PCB");
    let (thou, lthou) = pair("vision controller thou", "Vision PCB");
    assert_eq!((mm.units, thou.units), (Units::Mm, Units::Thou));
    assert_eq!(thou.placement("U1").unwrap().x, 2000.0);
    let conv = thou.converted(Units::Mm);
    assert_boards_close(&conv, &mm, 1e-9);
    for (a, b) in lthou.packages.iter().zip(&lmm.packages) {
        let a = a.converted(Units::Mm);
        assert_eq!((&a.name, &a.part_number, a.units), (&b.name, &b.part_number, b.units));
        assert_close(a.height, b.height, 1e-9);
        assert_loops_close(&a.loops, &b.loops, 1e-9);
    }
    // And placing a component gives the same geometry from either file.
    let la = thou.placed_outline(thou.placement("U1").unwrap(), &lthou).unwrap();
    let lb = mm.placed_outline(mm.placement("U1").unwrap(), &lmm).unwrap();
    assert_loops_close(&la, &lb, 1e-9);
    // Back to thou is (nearly) the identity.
    assert_boards_close(&mm.converted(Units::Thou), &thou, 1e-7);
}

fn assert_loops_close(a: &[Loop], b: &[Loop], tol: f64) {
    assert_eq!(a.len(), b.len());
    for (la, lb) in a.iter().zip(b) {
        assert_eq!(la.label, lb.label);
        assert_eq!(la.points.len(), lb.points.len());
        for (p, q) in la.points.iter().zip(&lb.points) {
            assert_close(p.x, q.x, tol);
            assert_close(p.y, q.y, tol);
            assert_eq!(p.angle, q.angle);
        }
    }
}

fn assert_boards_close(a: &Board, b: &Board, tol: f64) {
    assert_eq!(a.units, b.units);
    assert_close(outline(a).thickness, outline(b).thickness, tol);
    assert_loops_close(&outline(a).loops, &outline(b).loops, tol);
    for (x, y) in a.route_keepouts.iter().zip(&b.route_keepouts) {
        assert_loops_close(&x.loops, &y.loops, tol);
    }
    assert_eq!(a.holes.len(), b.holes.len());
    for (x, y) in a.holes.iter().zip(&b.holes) {
        assert_close(x.dia, y.dia, tol);
        assert_close(x.x, y.x, tol);
        assert_close(x.y, y.y, tol);
    }
    assert_eq!(a.placements.len(), b.placements.len());
    for (x, y) in a.placements.iter().zip(&b.placements) {
        assert_eq!((&x.refdes, &x.package, x.rotation, x.side), (&y.refdes, &y.package, y.rotation, y.side));
        assert_close(x.x, y.x, tol);
        assert_close(x.y, y.y, tol);
    }
    for (x, y) in a.notes.iter().zip(&b.notes) {
        assert_close(x.text_height, y.text_height, tol);
    }
}

// ---------------------------------------------------------------- (d) IDF 2.0 sample

#[test]
fn v2_sample_reads_2_0_syntax() {
    let (b, l) = pair("v2 sample", "v2 sample");
    assert_eq!(b.header.version, IdfVersion::V2);
    assert_eq!(b.header.file_type, FileType::Board);
    assert_eq!((b.name.as_str(), b.units), ("v2_sample", Units::Thou));
    let o = outline(&b);
    assert_eq!((o.owner, o.thickness, o.loops.len()), (Owner::Unowned, 62.0, 2));
    // The circular cut-out as two 180° arcs: a 200 thou circle around (1000, 750).
    let cut = &o.loops[1];
    assert_eq!(cut.label, 1);
    assert!(cut.is_closed());
    assert_close(cut.area(), PI * 100.0 * 100.0, 1e-6);
    assert!(cut.signed_area() < 0.0, "cut-outs run clockwise");
    let cbb = cut.bbox();
    assert_close(cbb.min[1], 650.0, 1e-9);
    assert_close(cbb.max[1], 850.0, 1e-9);

    assert_eq!(b.other_outlines[0].id, "HEATSINK");
    assert_eq!(b.other_outlines[0].side, None);
    assert_eq!(b.route_outlines[0].layers, None);
    assert_eq!((b.place_outlines[0].side, b.place_outlines[0].height), (None, None));
    assert_eq!(b.route_keepouts[0].layers, Layers::All);
    assert_eq!(b.via_keepouts.len(), 1);
    let pk = &b.place_keepouts[0];
    assert_eq!((pk.side, pk.height, pk.min_height), (Side::Bottom, Some(250.0), Some(0.0)));
    assert_eq!((b.place_regions[0].side, b.place_regions[0].group.as_str()), (Side::Top, "memory"));
    assert!(b.has_keep_areas());

    assert_eq!(b.holes.len(), 4);
    assert_eq!(b.holes[2].assoc, HoleAssoc::Refdes("J1".into()));
    assert!(b.holes.iter().all(|h| h.kind.is_none() && h.owner == Owner::Unowned));

    let st: Vec<_> = b.placements.iter().map(|p| p.status).collect();
    assert_eq!(st, [Status::Placed, Status::Placed, Status::Fixed, Status::Unplaced, Status::Placed]);
    assert_eq!(b.placements[1].side, MountSide::Bottom);
    assert_eq!(b.placements[1].rotation, 90.0);
    assert!(b.placements.iter().all(|p| p.mount_offset == 0.0));
    assert_eq!(b.placements[4].refdes, "norefdes");

    // Library: per-package units, including 2.0's TNM (ten nanometres).
    assert_eq!(l.header.version, IdfVersion::V2);
    let lcc = l.package("lcc20", "IDT-54fct244LB.l").unwrap();
    assert_eq!(lcc.units, Units::Tnm);
    assert_close(lcc.height_mm(), 4.572, 1e-12);
    assert_eq!(l.package("conn_2", "pn-conn").unwrap().units, Units::Mm);
    let ext = l.package("extractor", "pn-extractor").unwrap();
    assert_eq!(ext.kind, PackageKind::Mechanical);
    assert_eq!(ext.units, Units::Thou);

    // Upgrading to 3.0: FIXED → MCAD, TNM → MM, 2.0 defaults made explicit.
    let b3 = b.for_version(IdfVersion::V3);
    assert_eq!(b3.placements[2].status, Status::Mcad);
    assert_eq!(b3.route_outlines[0].layers, Some(Layers::All));
    assert_eq!(b3.place_outlines[0].side, Some(Side::Both));
    let l3 = l.for_version(IdfVersion::V3);
    let lcc3 = l3.package("lcc20", "IDT-54fct244LB.l").unwrap();
    assert_eq!(lcc3.units, Units::Mm);
    assert_close(lcc3.height, 4.572, 1e-12);
    assert!(write_emp(&l, IdfVersion::V3).contains("lcc20 IDT-54fct244LB.l MM 4.572"));
}

// ---------------------------------------------------------------- units and placement geometry

#[test]
fn unit_conversions() {
    assert_eq!(Units::Thou.to_mm(1.0), 0.0254);
    assert_eq!(Units::Thou.to_mm(1000.0), 25.4);
    assert_close(Units::Thou.from_mm(25.4), 1000.0, 1e-9);
    assert_eq!(Units::Mm.to_mm(3.5), 3.5);
    assert_close(Units::Tnm.to_mm(100_000.0), 1.0, 1e-15);
    assert_eq!(Units::Thou.convert(4000.0, Units::Mm), 101.6);
    assert_eq!(Units::Tnm.convert(457200.0, Units::Mm), 4.572);
    assert_eq!(Units::Mm.convert(2.54, Units::Thou), 100.0);
}

fn box_package(x0: f64, y0: f64, x1: f64, y1: f64, units: Units) -> Package {
    Package {
        kind: PackageKind::Electrical,
        name: "P".into(),
        part_number: "PN".into(),
        units,
        height: 1.0,
        loops: vec![Loop::rect(0, x0, y0, x1, y1)],
        props: vec![],
    }
}

fn placement(x: f64, y: f64, rotation: f64, side: MountSide) -> Placement {
    Placement {
        package: "P".into(),
        part_number: "PN".into(),
        refdes: "U1".into(),
        x,
        y,
        mount_offset: 0.0,
        rotation,
        side,
        status: Status::Placed,
    }
}

#[test]
fn placing_a_rotated_package() {
    // A 3.2 × 2.5 box centred on its origin, turned 90° at (10, 20): 2.5 wide, 3.2 tall.
    let pk = box_package(-1.6, -1.25, 1.6, 1.25, Units::Mm);
    let l = placement(10.0, 20.0, 90.0, MountSide::Top).place_loops(&pk, Units::Mm);
    let bb = loops_bbox(&l);
    assert_eq!((bb.min, bb.max), ([8.75, 18.4], [11.25, 21.6]));
    // An off-centre package (origin at its left edge) shows the direction of rotation:
    // [0,4]×[0,1] turned +90° (counter-clockwise) goes to [−1,0]×[0,4].
    let pk = box_package(0.0, 0.0, 4.0, 1.0, Units::Mm);
    let bb = loops_bbox(&placement(10.0, 0.0, 90.0, MountSide::Top).place_loops(&pk, Units::Mm));
    assert_eq!((bb.min, bb.max), ([9.0, 0.0], [10.0, 4.0]));
    let bb = loops_bbox(&placement(10.0, 0.0, 270.0, MountSide::Top).place_loops(&pk, Units::Mm));
    assert_eq!((bb.min, bb.max), ([10.0, -4.0], [11.0, 0.0]));
    // Package in thou, board in mm, and the other way round.
    let pk_thou = box_package(0.0, 0.0, 100.0, 50.0, Units::Thou);
    let bb = loops_bbox(&placement(1.0, 1.0, 0.0, MountSide::Top).place_loops(&pk_thou, Units::Mm));
    assert_close(bb.max[0], 1.0 + 2.54, 1e-12);
    assert_close(bb.max[1], 1.0 + 1.27, 1e-12);
    let bb = loops_bbox(&placement(100.0, 0.0, 0.0, MountSide::Top).place_loops(&pk, Units::Thou));
    assert_close(bb.min[0], 2.54, 1e-12);
}

#[test]
fn bottom_side_mirrors_x() {
    let pk = box_package(0.0, 0.0, 4.0, 1.0, Units::Mm);
    // Bottom, no rotation: flipped about the local Y axis, so it extends to −X.
    let bb = loops_bbox(&placement(10.0, 0.0, 0.0, MountSide::Bottom).place_loops(&pk, Units::Mm));
    assert_eq!((bb.min, bb.max), ([6.0, 0.0], [10.0, 1.0]));
    // Bottom, rotated 90° in the component's (mirrored) frame: clockwise seen from the top,
    // [0,4]×[0,1] → [0,1]×[0,4] (spec Figure 1), the mirror image of the top-side result.
    let bb = loops_bbox(&placement(10.0, 0.0, 90.0, MountSide::Bottom).place_loops(&pk, Units::Mm));
    assert_eq!((bb.min, bb.max), ([10.0, 0.0], [11.0, 4.0]));
    // Mirroring flips the loop's orientation, keeps its area, and negates arc angles so arcs
    // keep their centres (mirrored).
    let arc = Package {
        loops: vec![Loop::from_triples(0, &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 90.0), (0.0, 0.0, 0.0)])],
        ..pk.clone()
    };
    let top = placement(0.0, 0.0, 0.0, MountSide::Top).place_loops(&arc, Units::Mm);
    let bot = placement(0.0, 0.0, 0.0, MountSide::Bottom).place_loops(&arc, Units::Mm);
    assert_close(top[0].signed_area(), PI / 4.0, 1e-12);
    assert_close(bot[0].signed_area(), -PI / 4.0, 1e-12);
    assert_eq!(bot[0].points[2].angle, -90.0);
    let centre = bot[0]
        .segments()
        .find_map(|s| match s {
            Segment::Arc { center, .. } => Some(center),
            _ => None,
        })
        .unwrap();
    assert_close(centre[0], 0.0, 1e-12);
    assert_close(centre[1], 0.0, 1e-12);
    assert_eq!(loops_bbox(&bot).min, [-1.0, 0.0]);
}

// ---------------------------------------------------------------- export, zip and 2.0 keep areas

#[test]
fn v2_export_drops_keep_areas() {
    let (b, l) = pair("cell phone", "Cell phone");
    assert!(b.has_keep_areas());
    // The spec's 2.0 has PLACE_KEEPOUT (max and min heights), so write_emn keeps it...
    let spec_v2 = write_emn(&b, IdfVersion::V2);
    assert!(spec_v2.contains(".PLACE_KEEPOUT\nBOTTOM 1 0\n"), "{spec_v2}");
    assert!(spec_v2.contains(".BOARD_OUTLINE\n0.062\n"));
    // ...but PCB Studio's "IDF 2.0" export drops every keep area (PCB9.7).
    let (b2, _) = read_zip(&write_zip(&b, &l, IdfVersion::V2)).unwrap();
    assert_eq!(b2.header.version, IdfVersion::V2);
    assert!(!b2.has_keep_areas());
    assert_eq!(b2.outline, b.for_version(IdfVersion::V2).outline);
    let entries = zip_entries(&write_zip(&b, &l, IdfVersion::V2)).unwrap();
    let emn = String::from_utf8(entries[0].1.clone()).unwrap();
    assert!(!emn.contains("KEEPOUT") && !emn.contains("PLACE_REGION") && !emn.contains("_OUTLINE MCAD"));
    // IDF 3.0 keeps them.
    let (b3, _) = read_zip(&write_zip(&b, &l, IdfVersion::V3)).unwrap();
    assert_eq!(b3.place_keepouts, b.place_keepouts);
    // The 2.0 sample's keep-ins and keep-outs of every kind go too.
    let (v2, lv2) = pair("v2 sample", "v2 sample");
    let (v2e, _) = read_zip(&write_zip(&v2, &lv2, IdfVersion::V2)).unwrap();
    assert!(!v2e.has_keep_areas());
    assert_eq!(v2e.other_outlines.len(), 1, "other outlines aren't keep areas");
    assert_eq!(v2e.holes.len(), 4);
}

#[test]
fn zip_holds_a_folder_with_the_two_files() {
    let (b, l) = pair("cell phone", "Cell phone");
    let z = write_zip(&b, &l, IdfVersion::V3);
    assert_eq!(&z[..4], b"PK\x03\x04");
    let entries = zip_entries(&z).unwrap();
    let names: Vec<_> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Cell phone/Cell phone.emn", "Cell phone/Cell phone.emp"]);
    let emn = String::from_utf8(entries[0].1.clone()).unwrap();
    assert!(emn.starts_with(".HEADER\nBOARD_FILE 3.0 \"cadrs PCB Studio v0.1\" 2026/09/29.12:00:00 1\n\"Cell phone\" MM\n.END_HEADER\n.BOARD_OUTLINE MCAD\n0.062\n0 32.5 -73 0\n0 40.5 -65 90\n"), "{emn}");
    assert!(emn.contains(".DRILLED_HOLES\n.END_DRILLED_HOLES\n.PLACEMENT\n.END_PLACEMENT\n"));
    let emp = String::from_utf8(entries[1].1.clone()).unwrap();
    assert_eq!(emp, ".HEADER\nLIBRARY_FILE 3.0 \"cadrs PCB Studio v0.1\" 2026/09/29.12:00:00 1\n.END_HEADER\n");
    let (b2, l2) = read_zip(&z).unwrap();
    assert_eq!(b2, b);
    assert_eq!(l2, l);
    assert_eq!(zip_paths("secondary board").0, "secondary board/secondary board.emn");
    assert_eq!(safe_file_name("a/b:c"), "a_b_c");
}

// ---------------------------------------------------------------- course snippets as input

/// PCB6's exported `Cell phone.emn` as shown in the course (`ex1-step18-exported-emn.png`),
/// with the masked 5th outline point filled in by symmetry and a made-up date. Onshape's name
/// appears here only as input data; cadrs never writes it.
const COURSE_CELL_PHONE: &str = ".HEADER
BOARD_FILE 3.0 \"Onshape PCB Studio v1.2.112.0\" 2024/05/13.10:22:31 1
\"Cell phone\" MM
.END_HEADER
.BOARD_OUTLINE MCAD
0.0620000000000028
0 32.5 -73 0
0 40.5 -65 90
0 40.5 65 0
0 32.5 73 90
0 -32.5 73 0
0 -40.5 65 90
0 -40.5 -65 0
0 -32.5 -73 90
0 32.5 -73 0
.END_BOARD_OUTLINE
.DRILLED_HOLES
.END_DRILLED_HOLES
.PLACEMENT
.END_PLACEMENT
";

#[test]
fn course_cell_phone_snippet() {
    let b = parse_emn(COURSE_CELL_PHONE).unwrap().value;
    assert_eq!(b.header.source_system, "Onshape PCB Studio v1.2.112.0");
    assert_eq!(b.header.date, "2024/05/13.10:22:31");
    assert_eq!(outline(&b).thickness, 0.0620000000000028);
    let bb = outline(&b).loops[0].bbox();
    assert_eq!((bb.width(), bb.height()), (81.0, 146.0));
    // The same geometry as our fixture (a).
    let (ours, l) = pair("cell phone", "Cell phone");
    assert_eq!(outline(&b).loops, outline(&ours).loops);
    // Full-precision floats survive a write.
    assert!(write_emn(&b, IdfVersion::V3).contains("\n0.0620000000000028\n"));
    // Our export never writes Onshape's name.
    let z = write_zip(&b, &l, IdfVersion::V3);
    for (_, data) in zip_entries(&z).unwrap() {
        let s = String::from_utf8(data).unwrap();
        assert!(!s.to_lowercase().contains("onshape"), "{s}");
        assert!(s.contains("\"cadrs PCB Studio v0.1\" 2024/05/13.10:22:31 1"));
    }
}

/// PCB10's visible `.PLACEMENT` records (`ex3-step14-exported-emn-placement.png`), with the
/// "~0" mount offset written as the noise value the course describes.
const COURSE_EX3_PLACEMENT: &str = ".HEADER
BOARD_FILE 3.0 \"Onshape PCB Studio v1.2.112.0\" 2024/05/13.10:40:02 1
\"secondary board\" MM
.END_HEADER
.BOARD_OUTLINE MCAD
0.84
0 -5.20972 -22.74338 0
0 45.59028 -22.74338 0
0 45.59028 15.35662 0
0 -5.20972 15.35662 0
0 -5.20972 -22.74338 0
.END_BOARD_OUTLINE
.DRILLED_HOLES
.END_DRILLED_HOLES
.PLACEMENT
BUTTON_EVQPUA02 5209001 X0
24.47 -9.48 0 270 TOP PLACED
CRYSTAL_CX_4V 4510219 X1
32.37 3.10 -5.55e-15 270 TOP PLACED
uBGA48_7.4X7.1 7401048 X2
4.064182376174947 8.9 0 90 TOP PLACED
.END_PLACEMENT
";

#[test]
fn course_ex3_placement_snippet() {
    let b = parse_emn(COURSE_EX3_PLACEMENT).unwrap().value;
    let x0 = &b.placements[0];
    assert_eq!(
        (x0.package.as_str(), x0.part_number.as_str(), x0.refdes.as_str(), x0.x, x0.y, x0.mount_offset, x0.rotation, x0.side, x0.status),
        ("BUTTON_EVQPUA02", "5209001", "X0", 24.47, -9.48, 0.0, 270.0, MountSide::Top, Status::Placed)
    );
    let x1 = &b.placements[1];
    assert_eq!((x1.y, x1.mount_offset), (3.1, -5.55e-15));
    assert_eq!(b.placements[2].x, 4.064182376174947);
    let out = write_emn(&b, IdfVersion::V3);
    assert!(out.contains("BUTTON_EVQPUA02 5209001 X0\n24.47 -9.48 0 270 TOP PLACED\n"), "{out}");
    assert!(out.contains("32.37 3.1 -5.55e-15 270 TOP PLACED\n"));
    assert!(out.contains("uBGA48_7.4X7.1 7401048 X2\n4.064182376174947 8.9 0 90 TOP PLACED\n"));
    assert_eq!(parse_emn(&out).unwrap().value, b);
    // 2.0 has no mount offset field.
    assert!(write_emn(&b, IdfVersion::V2).contains("BUTTON_EVQPUA02 5209001 X0\n24.47 -9.48 270 TOP PLACED\n"));
}

#[test]
fn number_formatting() {
    assert_eq!(fmt_num(0.0), "0");
    assert_eq!(fmt_num(-0.0), "0");
    assert_eq!(fmt_num(270.0), "270");
    assert_eq!(fmt_num(-73.0), "-73");
    assert_eq!(fmt_num(0.062), "0.062");
    assert_eq!(fmt_num(0.0620000000000028), "0.0620000000000028");
    assert_eq!(fmt_num(-5.55e-15), "-5.55e-15");
    assert_eq!(fmt_num(4.064182376174947), "4.064182376174947");
    assert_eq!(fmt_num(1e20), "1e20");
    for v in [0.1 + 0.2, 1.0 / 3.0, -1e-300, 123456789.123, f64::MIN_POSITIVE, 5e-324] {
        assert_eq!(fmt_num(v).parse::<f64>().unwrap(), v);
    }
}

// ---------------------------------------------------------------- tolerance and errors

#[test]
fn tolerant_input() {
    let text = "# a comment\r\n\r\n.header\r\n  board_file   3.0  \"My ECAD  tool\"  2026/01/02.03:04:05 7 \r\n\"a board\"\tmm\r\n.end_header\r\n\
# comment between sections\r\n.board_outline\r\n1.6\r\n0 0 0 0\r\n0 10 0 0\r\n0 10 10 0\r\n0 0 0 0\r\n.end_board_outline\r\n\
.WEIRD_SECTION stuff\r\nwhatever 1 2\r\n.END_WEIRD_SECTION\r\n\
.notes\r\n1 2 0.5 4 \"hello   world\"\r\n.end_notes\r\n.placement\r\n\"pkg with space\" \"PN 1\" R1\r\n1 2 0 45 bottom ecad\r\n.end_placement\r\n";
    let p = parse_emn(text).unwrap();
    let b = p.value;
    assert_eq!(b.header.source_system, "My ECAD  tool");
    assert_eq!(b.header.file_version, 7);
    assert_eq!((b.name.as_str(), b.units), ("a board", Units::Mm));
    let o = outline(&b);
    assert_eq!((o.owner, o.thickness, o.loops[0].points.len()), (Owner::Unowned, 1.6, 4));
    assert_eq!(b.notes[0].text, "hello   world");
    let pl = &b.placements[0];
    assert_eq!((pl.package.as_str(), pl.part_number.as_str(), pl.side, pl.status), ("pkg with space", "PN 1", MountSide::Bottom, Status::Ecad));
    assert_eq!(p.warnings.len(), 1);
    assert_eq!(p.warnings[0].line, 15);
    assert!(p.warnings[0].message.contains("WEIRD_SECTION"));
    // Quoted names survive a round trip.
    assert_eq!(parse_emn(&write_emn(&b, IdfVersion::V3)).unwrap().value, b);
}

fn err_line(text: &str) -> (usize, String) {
    let e = parse_emn(text).unwrap_err();
    (e.line, e.message)
}

const HEAD: &str = ".HEADER\nBOARD_FILE 3.0 \"x\" 2026/09/29.12:00:00 1\nb MM\n.END_HEADER\n";

#[test]
fn errors_carry_line_numbers() {
    // Bad number in a loop point.
    let (line, msg) = err_line(&format!("{HEAD}.BOARD_OUTLINE MCAD\n1.6\n0 0 0 0\n0 1O 0 0\n.END_BOARD_OUTLINE\n"));
    assert_eq!(line, 8);
    assert!(msg.contains("'1O'"), "{msg}");
    // Missing section end.
    let (line, msg) = err_line(&format!("{HEAD}.BOARD_OUTLINE MCAD\n1.6\n0 0 0 0\n"));
    assert_eq!(line, 5);
    assert!(msg.contains("END_BOARD_OUTLINE"), "{msg}");
    // A section starting inside another.
    let (line, _) = err_line(&format!("{HEAD}.DRILLED_HOLES\n.PLACEMENT\n.END_PLACEMENT\n"));
    assert_eq!(line, 6);
    // Unterminated string.
    let (line, msg) = err_line(".HEADER\nBOARD_FILE 3.0 \"x 2026 1\nb MM\n.END_HEADER\n");
    assert_eq!(line, 2);
    assert!(msg.contains("unterminated"));
    // Unknown units, unsupported version, wrong file type.
    assert_eq!(err_line(".HEADER\nBOARD_FILE 3.0 \"x\" d 1\nb INCH\n.END_HEADER\n").0, 3);
    assert!(err_line(".HEADER\nBOARD_FILE 1.0 \"x\" d 1\nb MM\n.END_HEADER\n").1.contains("unsupported IDF version"));
    assert!(err_line(".HEADER\nLIBRARY_FILE 3.0 \"x\" d 1\n.END_HEADER\n").1.contains("expected BOARD_FILE"));
    // Odd number of placement records.
    assert_eq!(err_line(&format!("{HEAD}.PLACEMENT\np n R1\n1 2 0 0 TOP PLACED\np n R2\n.END_PLACEMENT\n")).0, 8);
    // Bad enum value.
    let (line, msg) = err_line(&format!("{HEAD}.PLACEMENT\np n R1\n1 2 0 0 SIDEWAYS PLACED\n.END_PLACEMENT\n"));
    assert_eq!(line, 7);
    assert!(msg.contains("SIDEWAYS"));
    // Data outside a section, and no header.
    assert_eq!(err_line(&format!("{HEAD}0 1 2 3\n")).0, 5);
    assert!(err_line(".BOARD_OUTLINE\n1\n.END_BOARD_OUTLINE\n").1.contains("HEADER"));
    // Library errors are prefixed by read_pair.
    let e = read_pair(HEAD, ".HEADER\nLIBRARY_FILE 3.0 \"x\" d 1\n.END_HEADER\n.ELECTRICAL\np n MM tall\n.END_ELECTRICAL\n").unwrap_err();
    assert_eq!(e.line, 5);
    assert!(e.to_string().starts_with("line 5: .emp:"), "{e}");
    // Corrupt zip.
    assert!(read_zip(b"not a zip at all, clearly").is_err());
}
