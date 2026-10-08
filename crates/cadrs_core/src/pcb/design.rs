//! Native boards (a [`cadrs_eda::Design`]: schematic + layout) as the studio's mechanical
//! board ([`PcbBoard`]): the outline from the layout's outline layer, one placement per
//! footprint and one package per footprint kind (its courtyard, else its pads, as a box).
//! The 3D view, Create assembly, Sync and IDF export work from that board, as for an imported
//! one.

use cadrs_eda::Design;
use cadrs_eda::footprint::Footprint;
use cadrs_eda::layer::{Layer, Side};
use cadrs_eda::units::{Bounds, Pt, to_mm};
use cadrs_idf::geom::Loop;
use cadrs_idf::{Board, BoardOutline, IdfVersion, Library, MountSide, Owner, Package, PackageKind, Placement, Status, Units};

use super::PcbBoard;

/// Package height when a footprint says nothing better (mm).
pub const DEFAULT_HEIGHT: f64 = 1.0;

fn mm2(p: Pt) -> [f64; 2] {
    [to_mm(p.x), to_mm(p.y)]
}

/// The layout's outline loops ([`cadrs_eda::outline::loops`]) as IDF loops: the largest is
/// the outline (label 0, counter-clockwise), the rest cut-outs (labels 1…, clockwise).
pub fn outline_loops(design: &Design) -> Vec<Loop> {
    let mut loops: Vec<Loop> = cadrs_eda::outline::loops(&design.board)
        .iter()
        .map(|edges| {
            let start = mm2(edges[0].a);
            let mut t = vec![(start[0], start[1], 0.0)];
            let n = edges.len();
            for (k, e) in edges.iter().enumerate() {
                // The last edge ends exactly on the first point.
                let p = if k + 1 == n { start } else { mm2(e.b) };
                let sweep = e.mid.and_then(|m| cadrs_eda::geom::arc_params(e.a, m, e.b)).map_or(0.0, |(_, _, _, s)| s.to_degrees());
                t.push((p[0], p[1], sweep));
            }
            Loop::from_triples(0, &t)
        })
        .collect();
    // Largest first; outline counter-clockwise, cut-outs clockwise.
    loops.sort_by(|a, b| b.area().total_cmp(&a.area()));
    for (i, l) in loops.iter_mut().enumerate() {
        l.label = i as u32;
        let ccw = l.signed_area() > 0.0;
        if (i == 0) != ccw && !l.is_circle() {
            *l = reversed(l);
        }
    }
    loops
}

/// The same loop walked the other way.
fn reversed(l: &Loop) -> Loop {
    let p = &l.points;
    let mut t = vec![(p[p.len() - 1].x, p[p.len() - 1].y, 0.0)];
    for i in (1..p.len()).rev() {
        t.push((p[i - 1].x, p[i - 1].y, -p[i].angle));
    }
    Loop::from_triples(l.label, &t)
}

/// A footprint's box for its package: the courtyard, else the pads and fabrication drawing.
fn footprint_box(f: &Footprint) -> Option<Bounds> {
    let on = |layers: &[Layer]| -> Option<Bounds> {
        let mut b: Option<Bounds> = None;
        for s in f.shapes.iter().filter(|s| layers.contains(&s.layer)) {
            for p in s.shape.geom.extent() {
                b = Some(Bounds::union(b, Bounds::of(p)));
            }
        }
        b
    };
    on(&[Layer::TopCourtyard]).or_else(|| {
        let mut b = on(&[Layer::TopFab]);
        for p in &f.pads {
            let r = p.size.w.max(p.size.h) / 2;
            b = Some(Bounds::union(b, Bounds::of(p.at).grow(r)));
        }
        b
    })
}

/// A component's 3D preview as a design: its footprint at the origin (reference `REF`) on a
/// board patch 2 mm larger than the footprint all round.
pub fn footprint_patch(fp: &Footprint) -> Design {
    use cadrs_eda::board::{BoardShape, PlacedFootprint};
    use cadrs_eda::graphics::{Fill, Geom, Shape, Stroke};
    let b = footprint_box(fp).unwrap_or_else(|| Bounds::of(Pt::ZERO).grow(cadrs_eda::units::mm(2.0))).grow(cadrs_eda::units::mm(2.0));
    let mut d = Design::new();
    d.board.shapes.push(BoardShape {
        id: uuid::Uuid::new_v4(),
        shape: Shape { geom: Geom::Rect { a: b.min, b: b.max }, stroke: Stroke::width(cadrs_eda::units::mm(0.05)), fill: Fill::None },
        layer: Layer::Outline,
        locked: false,
        net: String::new(),
    });
    let mut f = fp.clone();
    if let Some(r) = f.fields.iter_mut().find(|x| x.name == cadrs_eda::symbol::fields::REFERENCE) {
        r.text.text.text = "REF".into();
    }
    d.board.footprints.push(PlacedFootprint { id: uuid::Uuid::new_v4(), footprint: f, placement: Default::default(), locked: false, symbol: None });
    d
}

/// The studio board for a design named `name`.
pub fn pcb_board(name: &str, design: &Design) -> PcbBoard {
    let mut board = Board::new(name, Units::Mm, IdfVersion::V3);
    board.outline = Some(BoardOutline { owner: Owner::Ecad, thickness: to_mm(design.board.thickness), loops: outline_loops(design) });
    let mut library = Library::new(IdfVersion::V3);
    for f in &design.board.footprints {
        let fp = &f.footprint;
        let package = fp.id.clone();
        let part_number = fp.field(cadrs_eda::symbol::fields::VALUE).map(|v| v.text.text.text.clone()).unwrap_or_default();
        if library.package(&package, &part_number).is_none() {
            let b = footprint_box(fp).unwrap_or_else(|| Bounds::of(Pt::ZERO).grow(cadrs_eda::units::mm(0.5)));
            let (lo, hi) = (mm2(b.min), mm2(b.max));
            library.packages.push(Package {
                kind: PackageKind::Electrical,
                name: package.clone(),
                part_number: part_number.clone(),
                units: Units::Mm,
                height: DEFAULT_HEIGHT,
                loops: vec![Loop::rect(0, lo[0], lo[1], hi[0], hi[1])],
                props: vec![],
            });
        }
        let at = mm2(f.placement.at);
        let bottom = f.placement.side == Side::Bottom;
        board.placements.push(Placement {
            package,
            part_number,
            refdes: f.reference().to_string(),
            x: at[0],
            y: at[1],
            mount_offset: 0.0,
            // IDF mirrors bottom parts about their Y axis, cadrs about X: 180° apart.
            rotation: if bottom { cadrs_eda::units::normalize_deg(180.0 - f.placement.angle) } else { f.placement.angle },
            side: if bottom { MountSide::Bottom } else { MountSide::Top },
            status: Status::Placed,
        });
    }
    PcbBoard::new(&board, &library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_eda::board::{Board as Layout, BoardShape};
    use cadrs_eda::graphics::{Geom, Shape, Stroke};
    use cadrs_eda::units::mm;

    #[test]
    fn footprint_patch_is_a_board_under_the_part() {
        let lib = cadrs_eda::library::LibraryTable::builtin();
        let fp = lib.footprint("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm").unwrap();
        let d = footprint_patch(fp);
        let b = pcb_board("part", &d);
        assert_eq!(b.board.placements.len(), 1);
        assert_eq!(b.board.placements[0].refdes, "REF");
        let o = &b.board.outline.as_ref().unwrap().loops[0];
        let xs: Vec<f64> = o.points.iter().map(|p| p.x).collect();
        let (lo, hi) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
        // The courtyard (±3.70 mm across the pads) and 2 mm more.
        assert!((hi - 5.7).abs() < 0.02 && (lo + 5.7).abs() < 0.02, "{lo} {hi}");
    }

    fn shape(geom: Geom) -> BoardShape {
        BoardShape { id: uuid::Uuid::new_v4(), shape: Shape { geom, stroke: Stroke::default(), fill: Default::default() }, layer: Layer::Outline, locked: false, net: String::new() }
    }

    #[test]
    fn new_board_outline_is_its_rectangle() {
        let d = Design { board: Layout::with_rect_outline(mm(100.0), mm(80.0)), ..Default::default() };
        let b = pcb_board("Board 1", &d);
        let o = b.board.outline.as_ref().unwrap();
        assert_eq!(o.loops.len(), 1);
        assert!((o.loops[0].area() - 8000.0).abs() < 1e-9);
        assert!((o.thickness - 1.6).abs() < 1e-12);
    }

    #[test]
    fn chains_lines_and_arcs_in_any_order_and_direction() {
        // A 10 × 10 square with its top-right corner rounded (R2), edges shuffled and one
        // reversed; plus a round hole.
        let p = |x, y| Pt::mm(x, y);
        let c = 2.0 - 2.0 * std::f64::consts::FRAC_1_SQRT_2;
        let mut d = Design::default();
        d.board.shapes = vec![
            shape(Geom::Line { a: p(10.0, 8.0), b: p(10.0, 0.0) }),
            shape(Geom::Line { a: p(0.0, 0.0), b: p(0.0, 10.0) }),
            shape(Geom::Arc { start: p(8.0, 10.0), mid: p(10.0 - c, 10.0 - c), end: p(10.0, 8.0) }),
            shape(Geom::Line { a: p(10.0, 0.0), b: p(0.0, 0.0) }),
            // Misses the arc's start by 8 µm, as drawn outlines do.
            shape(Geom::Line { a: p(0.0, 10.0), b: p(7.994167, 10.005833) }),
            shape(Geom::Circle { center: p(5.0, 5.0), radius: mm(1.0) }),
        ];
        let loops = outline_loops(&d);
        assert_eq!(loops.len(), 2);
        let corner = 4.0 - std::f64::consts::PI;
        // (The 8 µm miss tilts the top edge: about 0.03 mm² more.)
        assert!((loops[0].area() - (100.0 - corner)).abs() < 0.05, "{}", loops[0].area());
        assert!(loops[0].signed_area() > 0.0);
        assert_eq!(loops[1].label, 1);
    }
}

#[cfg(test)]
mod course {
    use super::*;
    use cadrs_eda::library::LibraryTable;

    /// GS20: the course's finished board as the studio's mechanical board (the 3D view):
    /// its 45 × 50 mm outline, and BT1 on the bottom, mirrored as IDF mirrors bottom parts.
    #[test]
    fn gs20_board_in_3d() {
        let lib = LibraryTable::builtin();
        let d = cadrs_eda::getting_started::gs18(&lib);
        let b = pcb_board("getting-started", &d);
        let o = b.board.outline.as_ref().unwrap();
        assert_eq!(o.loops.len(), 1);
        assert!((o.loops[0].area() - 2250.0).abs() < 1e-6);
        assert!((o.thickness - 1.6).abs() < 1e-12);
        let refs: Vec<&str> = b.board.placements.iter().map(|p| p.refdes.as_str()).collect();
        assert_eq!(refs, ["BT1", "D1", "R1"]);
        let bt = b.board.placement("BT1").unwrap();
        assert_eq!(bt.side, MountSide::Bottom);
        // Placed the same way: every pad of every package lands where the layout has it.
        for f in &d.board.footprints {
            let p = b.board.placement(f.reference()).unwrap();
            let pkg = b.library.package(&p.package, &p.part_number).unwrap();
            let placed = p.place_loops(pkg, Units::Mm);
            let bb = cadrs_idf::loops_bbox(&placed);
            let fb = footprint_box(&f.footprint).unwrap();
            let corners = [fb.min, fb.max, Pt::new(fb.min.x, fb.max.y), Pt::new(fb.max.x, fb.min.y)].map(|c| mm2(f.placement.apply(c)));
            for c in corners {
                assert!(c[0] >= bb.min[0] - 1e-6 && c[0] <= bb.max[0] + 1e-6 && c[1] >= bb.min[1] - 1e-6 && c[1] <= bb.max[1] + 1e-6, "{} {c:?} {bb:?}", f.reference());
            }
        }
    }
}
