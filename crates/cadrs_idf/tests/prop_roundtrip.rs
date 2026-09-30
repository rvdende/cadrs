//! Property tests: parse(write(x, v)) == x.for_version(v), exactly, for generated boards and
//! libraries in IDF 2.0 and 3.0.

use cadrs_idf::*;
use proptest::prelude::*;

fn coord() -> impl Strategy<Value = f64> {
    prop_oneof![
        8 => -1e6..1e6f64,
        1 => Just(0.0),
        1 => Just(-5.55e-15),
        1 => (-1000i32..1000).prop_map(f64::from),
    ]
}

fn positive() -> impl Strategy<Value = f64> {
    prop_oneof![0.001..1000.0f64, Just(0.062), Just(1.6)]
}

fn sweep() -> impl Strategy<Value = f64> {
    prop_oneof![4 => Just(0.0), 1 => Just(90.0), 1 => Just(-180.0), 2 => -359.0..359.0f64]
}

/// A closed polygon/arc loop whose intermediate points never touch the start (so the parser
/// can't split it early), or a 360° circle.
fn a_loop(label: u32) -> impl Strategy<Value = Loop> {
    let poly = prop::collection::vec((coord(), coord(), sweep()), 3..8).prop_filter_map("touches start", move |pts| {
        let (x0, y0) = (pts[0].0, pts[0].1);
        if pts[1..].iter().any(|p| (p.0 - x0).abs() <= 1e-6 && (p.1 - y0).abs() <= 1e-6) {
            return None;
        }
        let mut v: Vec<LoopPoint> = pts.iter().map(|&(x, y, a)| LoopPoint::new(x, y, a)).collect();
        v[0].angle = 0.0;
        let last = pts[pts.len() - 1].2;
        v.push(LoopPoint::new(x0, y0, last));
        Some(Loop::new(label, v))
    });
    let circle = (coord(), coord(), 0.01..100.0f64).prop_map(move |(x, y, r)| Loop::circle(label, x, y, r));
    prop_oneof![4 => poly, 1 => circle]
}

fn loops() -> impl Strategy<Value = Vec<Loop>> {
    (1usize..4).prop_flat_map(|n| (0..n as u32).map(a_loop).collect::<Vec<_>>())
}

fn name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_][A-Za-z0-9_ .\\-]{0,11}"
}

fn text() -> impl Strategy<Value = String> {
    "[ !#-~]{0,16}"
}

fn owner() -> impl Strategy<Value = Owner> {
    prop_oneof![Just(Owner::Ecad), Just(Owner::Mcad), Just(Owner::Unowned)]
}

fn side() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Top), Just(Side::Bottom), Just(Side::Both)]
}

fn mount_side() -> impl Strategy<Value = MountSide> {
    prop_oneof![Just(MountSide::Top), Just(MountSide::Bottom)]
}

fn layers() -> impl Strategy<Value = Layers> {
    prop_oneof![Just(Layers::Top), Just(Layers::Bottom), Just(Layers::Both), Just(Layers::Inner), Just(Layers::All)]
}

fn units() -> impl Strategy<Value = Units> {
    prop_oneof![Just(Units::Mm), Just(Units::Thou), Just(Units::Tnm)]
}

fn header(ft: FileType) -> impl Strategy<Value = Header> {
    let ft = prop_oneof![Just(ft), Just(if ft == FileType::Board { FileType::Panel } else { ft })];
    (ft, "[ !#-~]{0,20}", prop_oneof![Just(String::new()), "[0-9/.:]{1,19}", Just("2026/09/29.12:00:00".to_string())], 0u32..100)
        .prop_map(|(file_type, source_system, date, file_version)| Header { file_type, version: IdfVersion::V3, source_system, date, file_version })
}

fn hole() -> impl Strategy<Value = DrilledHole> {
    let assoc = prop_oneof![
        Just(HoleAssoc::Board),
        Just(HoleAssoc::NoRefdes),
        Just(HoleAssoc::Panel),
        name().prop_filter("reserved", |s| !["BOARD", "NOREFDES", "PANEL"].iter().any(|r| r.eq_ignore_ascii_case(s))).prop_map(HoleAssoc::Refdes),
    ];
    let kind = prop_oneof![
        Just(None),
        Just(Some(HoleKind::Pin)),
        Just(Some(HoleKind::Via)),
        Just(Some(HoleKind::Mtg)),
        Just(Some(HoleKind::Tool)),
        name()
            .prop_filter("reserved", |s| !["PIN", "VIA", "MTG", "TOOL"].iter().any(|r| r.eq_ignore_ascii_case(s)))
            .prop_map(|s| Some(HoleKind::Other(s))),
    ];
    (positive(), coord(), coord(), prop_oneof![Just(Plating::Pth), Just(Plating::Npth)], assoc, kind, owner())
        .prop_map(|(dia, x, y, plating, assoc, kind, owner)| DrilledHole { dia, x, y, plating, assoc, kind, owner })
}

fn placement() -> impl Strategy<Value = Placement> {
    let status = prop_oneof![Just(Status::Placed), Just(Status::Unplaced), Just(Status::Mcad), Just(Status::Ecad), Just(Status::Fixed)];
    (name(), name(), name(), coord(), coord(), prop_oneof![Just(0.0), Just(-5.55e-15), 0.0..5.0f64], -360.0..360.0f64, mount_side(), status).prop_map(
        |(package, part_number, refdes, x, y, mount_offset, rotation, side, status)| Placement {
            package,
            part_number,
            refdes,
            x,
            y,
            mount_offset,
            rotation,
            side,
            status,
        },
    )
}

fn board() -> impl Strategy<Value = Board> {
    let outline = prop::option::of((owner(), positive(), loops()).prop_map(|(owner, thickness, loops)| BoardOutline { owner, thickness, loops }));
    let other = prop::collection::vec(
        (owner(), name(), positive(), prop::option::of(mount_side()), loops())
            .prop_map(|(owner, id, thickness, side, loops)| OtherOutline { owner, id, thickness, side, loops }),
        0..2,
    );
    let route = prop::collection::vec(
        (owner(), prop::option::of(layers()), loops()).prop_map(|(owner, layers, loops)| RouteOutline { owner, layers, loops }),
        0..2,
    );
    let place = prop::collection::vec(
        (owner(), prop::option::of(side()), prop::option::of(positive()), loops())
            .prop_map(|(owner, side, height, loops)| PlaceOutline { owner, side, height, loops }),
        0..2,
    );
    let rko = prop::collection::vec((owner(), layers(), loops()).prop_map(|(owner, layers, loops)| RouteKeepout { owner, layers, loops }), 0..2);
    let vko = prop::collection::vec((owner(), loops()).prop_map(|(owner, loops)| ViaKeepout { owner, loops }), 0..2);
    let pko = prop::collection::vec(
        (owner(), side(), prop::option::of(positive()), prop::option::of(positive()), loops())
            .prop_map(|(owner, side, height, min_height, loops)| PlaceKeepout { owner, side, height, min_height, loops }),
        0..2,
    );
    let reg = prop::collection::vec(
        (owner(), side(), name(), loops()).prop_map(|(owner, side, group, loops)| PlaceRegion { owner, side, group, loops }),
        0..2,
    );
    let notes = prop::collection::vec(
        (coord(), coord(), positive(), positive(), text()).prop_map(|(x, y, text_height, text_length, text)| Note { x, y, text_height, text_length, text }),
        0..3,
    );
    (
        (header(FileType::Board), name(), units(), outline),
        (other, route, place, rko, vko, pko, reg),
        (prop::collection::vec(hole(), 0..4), notes, prop::collection::vec(placement(), 0..6)),
    )
        .prop_map(|((header, name, units, outline), (other, route, place, rko, vko, pko, reg), (holes, notes, placements))| Board {
            header,
            name,
            units,
            outline,
            other_outlines: other,
            route_outlines: route,
            place_outlines: place,
            route_keepouts: rko,
            via_keepouts: vko,
            place_keepouts: pko,
            place_regions: reg,
            holes,
            notes,
            placements,
        })
}

fn library() -> impl Strategy<Value = Library> {
    let prop_rec = ("[A-Z_]{1,10}", prop_oneof![text(), "[0-9.]{1,6}"]);
    let pkg = (
        prop_oneof![Just(PackageKind::Electrical), Just(PackageKind::Mechanical)],
        name(),
        name(),
        units(),
        positive(),
        loops(),
        prop::collection::vec(prop_rec, 0..3),
    )
        .prop_map(|(kind, name, part_number, units, height, loops, props)| Package { kind, name, part_number, units, height, loops, props });
    (header(FileType::Library), prop::collection::vec(pkg, 0..4)).prop_map(|(header, packages)| Library { header, packages })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn board_round_trips(b in board()) {
        for v in [IdfVersion::V3, IdfVersion::V2] {
            let text = write_emn(&b, v);
            let parsed = parse_emn(&text).map_err(|e| TestCaseError::fail(format!("{e}\n{text}")))?;
            prop_assert!(parsed.warnings.is_empty());
            prop_assert_eq!(&parsed.value, &b.for_version(v), "{:?}\n{}", v, text);
            // Projection is idempotent.
            prop_assert_eq!(parsed.value.for_version(v), parsed.value.clone());
        }
    }

    #[test]
    fn library_round_trips(l in library()) {
        for v in [IdfVersion::V3, IdfVersion::V2] {
            let text = write_emp(&l, v);
            let parsed = parse_emp(&text).map_err(|e| TestCaseError::fail(format!("{e}\n{text}")))?;
            prop_assert_eq!(&parsed.value, &l.for_version(v), "{:?}\n{}", v, text);
        }
    }

    #[test]
    fn zip_round_trips(b in board(), l in library()) {
        for v in [IdfVersion::V3, IdfVersion::V2] {
            let (b2, l2) = read_zip(&write_zip(&b, &l, v)).map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(b2, export_board(&b, v));
            prop_assert_eq!(l2, export_library(&l, &export_board(&b, v), v));
        }
    }

    #[test]
    fn numbers_round_trip(v in any::<f64>().prop_filter("finite", |v| v.is_finite())) {
        let s = fmt_num(v);
        prop_assert_eq!(s.parse::<f64>().unwrap(), v);
    }
}
