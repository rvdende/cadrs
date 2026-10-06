//! The reference project redrawn in cadrs ([`cadrs_eda::power_monitor`], built with cadrs'
//! editing operations, nothing read from KiCad) against the KiCad files themselves: the same
//! nets with the same members, the same footprints in the same places on the same nets, the
//! same copper, the same DRC findings. Skips when the project isn't on this machine.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cadrs_eda::Design;
use cadrs_eda::units::Pt;

fn reference_project() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("HOME").ok()?).join("work/desk_power_monitor/hardware/desk_power_monitor_mini32_lora");
    p.is_dir().then_some(p)
}

/// Net name (KiCad's sheet prefix `/` dropped) → its pins as `REF.number`.
fn schematic_nets(d: &Design) -> BTreeMap<String, BTreeSet<String>> {
    cadrs_eda::connectivity::netlist(&d.schematic).nets.into_iter().map(|n| (n.name.trim_start_matches('/').to_string(), n.pins.iter().map(|p| format!("{}.{}", p.reference, p.number)).collect())).collect()
}

/// Footprint → (position µm, angle, side, its pads' nets).
#[allow(clippy::type_complexity)]
fn board_parts(d: &Design) -> BTreeMap<String, ((i64, i64), i64, String, BTreeMap<String, String>)> {
    d.board
        .footprints
        .iter()
        .map(|f| {
            let at = f.placement.at;
            let pads = f.footprint.pads.iter().filter(|p| !p.number.is_empty()).map(|p| (p.number.clone(), p.net.clone().unwrap_or_default().trim_start_matches('/').to_string())).collect();
            (f.reference().to_string(), ((at.x / 1000, at.y / 1000), f.placement.angle.round() as i64, format!("{:?}", f.placement.side), pads))
        })
        .collect()
}

/// Every track as (layer, ends in µm in a fixed order, width µm), sorted.
fn tracks(d: &Design) -> Vec<(String, (i64, i64), (i64, i64), i64)> {
    let um = |p: Pt| (p.x / 1000, p.y / 1000);
    let mut v: Vec<_> = d.board.tracks.iter().map(|t| {
        let (a, b) = if (t.a.x, t.a.y) <= (t.b.x, t.b.y) { (t.a, t.b) } else { (t.b, t.a) };
        (format!("{:?}", t.layer), um(a), um(b), t.width / 1000)
    }).collect();
    v.sort();
    v
}

#[test]
fn redrawn_power_monitor_matches_kicad() {
    let Some(dir) = reference_project() else {
        eprintln!("skipped: no reference project");
        return;
    };
    let kicad = cadrs_kicad::read_project(&dir).unwrap().design;
    let (ours, _) = cadrs_eda::power_monitor::design();

    // The schematic: the same nets, named the same, joining the same pins.
    let (a, b) = (schematic_nets(&ours), schematic_nets(&kicad));
    for (name, pins) in &b {
        assert_eq!(a.get(name), Some(pins), "net {name}: ours {:?}", a.get(name));
    }
    assert_eq!(a.len(), b.len(), "extra nets: {:?}", a.keys().filter(|k| !b.contains_key(*k)).collect::<Vec<_>>());

    // The board: every footprint in the same place, its pads on the same nets.
    let (a, b) = (board_parts(&ours), board_parts(&kicad));
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (r, theirs) in &b {
        let mine = &a[r];
        assert_eq!((mine.0, mine.1, &mine.2), (theirs.0, theirs.1, &theirs.2), "{r} placement");
        assert_eq!(mine.3, theirs.3, "{r} pad nets");
    }

    // The same copper.
    assert_eq!(tracks(&ours), tracks(&kicad));
    let vias = |d: &Design| d.board.vias.iter().map(|v| (v.at.x / 1000, v.at.y / 1000, v.diameter / 1000, v.drill / 1000)).collect::<BTreeSet<_>>();
    assert_eq!(vias(&ours), vias(&kicad));

    // The same DRC findings (KiCad's own DRC reports the same two loose ground tracks).
    let findings = |d: &Design| {
        let r = cadrs_eda::drc::check(&d.board);
        // The redraw gave the RA-01SH footprint the courtyard the imported one lacks.
        let mut v: Vec<String> = r.violations.iter().filter(|v| v.rule != cadrs_eda::drc::Rule::MissingCourtyard).map(|v| format!("{:?}", v.rule)).collect();
        v.sort();
        (v, r.unconnected.len())
    };
    let (mine, theirs) = (findings(&ours), findings(&kicad));
    eprintln!("ours {mine:?}\nkicad {theirs:?}");
    assert_eq!(mine, theirs);
}

/// The redraw's JLCPCB placement file agrees with the one the project shipped, for the parts that
/// one lists (their values were changed to 0R before ordering).
#[test]
fn redrawn_power_monitor_places_like_the_jlcpcb_files() {
    let Some(dir) = reference_project() else {
        eprintln!("skipped: no reference project");
        return;
    };
    let Ok(theirs) = std::fs::read_to_string(dir.join("jlcpcb/CPL.csv")) else { return };
    let (ours, _) = cadrs_eda::power_monitor::design();
    let mine = cadrs_eda::fab::jlcpcb_cpl(&ours.board);
    assert_eq!(mine.lines().next(), theirs.lines().next(), "header");
    let without_value = |l: &str| {
        let mut f: Vec<&str> = l.split(',').collect();
        f.remove(1);
        f.join(",")
    };
    for line in theirs.lines().skip(1) {
        let r = line.split(',').next().unwrap();
        let m = mine.lines().find(|l| l.split(',').next() == Some(r)).unwrap_or_else(|| panic!("{r} missing from\n{mine}"));
        assert_eq!(without_value(m), without_value(line));
    }
}
