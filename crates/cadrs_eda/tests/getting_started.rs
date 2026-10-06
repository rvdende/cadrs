//! The "Getting Started" course (docs/PLAN.md, GS1–GS26): each `gsNN_…` test checks one step
//! of the design that `cadrs_eda::getting_started` builds step by step.

use cadrs_eda::board_edit as be;
use cadrs_eda::connectivity::netlist;
use cadrs_eda::erc::{self, Rule};
use cadrs_eda::expr::{Unit, eval};
use cadrs_eda::footprint::MountKind;
use cadrs_eda::getting_started::*;
use cadrs_eda::layer::{Layer, Side};
use cadrs_eda::lib_edit as le;
use cadrs_eda::library::{LibraryTable, Scope};
use cadrs_eda::sch_edit::{self as se, SchItem};
use cadrs_eda::units::{Bounds, MIL, Pt, mm};
use cadrs_eda::{copper, drc, forward, outline, zone};
use uuid::Uuid;
#[test]
fn gs03_04_page_and_symbols() {
    let lib = LibraryTable::builtin();
    let d = gs04(&lib);
    let s = &d.schematic;
    assert_eq!(s.sheets[0].paper.name, "B");
    assert_eq!(s.sheets[0].paper.size.w, mm(431.8));
    assert_eq!(s.sheets[0].title_block.title, "Getting Started");
    let refs: Vec<&str> = s.sheets[0].symbols.iter().map(|x| x.reference()).collect();
    assert_eq!(refs, ["D1", "R1", "BT1"]);
    let values: Vec<&str> = s.sheets[0].symbols.iter().map(|x| x.value()).collect();
    assert_eq!(values, ["LED", "R_US", "Battery_Cell"]);
    // The symbol chooser's search: "R" finds the resistor first.
    assert_eq!(lib.search_symbols("R", false)[0].id, "Device:R");
    assert!(lib.search_symbols("", true).iter().any(|x| x.id == "power:GND"));
}

#[test]
fn gs05_select_move_rotate() {
    let lib = LibraryTable::builtin();
    let d = gs05(&lib);
    let s = &d.schematic;
    // D1 stands upright under R1: anode (2) above the cathode (1).
    let (a, k) = (pin(s, "D1", "2"), pin(s, "D1", "1"));
    assert_eq!(a.x, k.x);
    assert!(a.y > k.y);
    assert_eq!(pin(s, "R1", "2").x, a.x);
    // Clicking D1's reference text picks just that field.
    let d1 = sym(s, "D1");
    let f = s.sheets[0].symbols.iter().find(|x| x.id == d1).unwrap().field("Reference").unwrap().text.at;
    assert!(matches!(se::hit(s, 0, f, mm(0.5)), Some(SchItem::Field(id, ref name)) if id == d1 && name == "Reference"));
    // A box around the right-hand pair, left to right, takes both.
    let sel = se::box_select(s, 0, Bounds { min: p(190.0, 80.0), max: p(230.0, 125.0) }, false);
    assert_eq!(sel.iter().filter(|i| matches!(i, SchItem::Symbol(_))).count(), 2);
    // Delete then undo is the command layer's; delete works.
    let mut d2 = d.clone();
    se::delete_items(&mut d2.schematic, 0, &[SchItem::Symbol(d1)]);
    assert_eq!(d2.schematic.sheets[0].symbols.len(), 2);
}

#[test]
fn gs06_07_wires_power_labels() {
    let lib = LibraryTable::builtin();
    let d = gs07(&lib);
    let s = &d.schematic;
    let nl = netlist(s);
    let names = |net: &str| -> Vec<String> { nl.net(net).unwrap().part_pins().map(|p| format!("{}.{}", p.reference, p.number)).collect() };
    assert_eq!(names("VCC"), ["BT1.1", "R1.1"]);
    assert_eq!(names("GND"), ["BT1.2", "D1.1"]);
    assert_eq!(names("led"), ["D1.2", "R1.2"]);
    // T-junctions where VCC and GND join the loop.
    assert_eq!(s.sheets[0].junctions.len(), 2);
    // No pin is left alone.
    assert!(nl.nets.iter().all(|n| n.pins.len() > 1));
}

#[test]
fn gs08_annotation() {
    let lib = LibraryTable::builtin();
    let mut d = gs07(&lib);
    let s = &mut d.schematic;
    // Annotated as placed; re-annotating numbers by position (left to right).
    se::place_symbol(s, 0, lib.symbol("Device:R").unwrap(), p(50.8, 50.8), Uuid::new_v4());
    assert!(s.sheets[0].symbols.iter().any(|x| x.reference() == "R2"));
    se::annotate(s, false);
    let r_left = s.sheets[0].symbols.iter().min_by_key(|x| (cadrs_eda::sch_edit::prefix(x.reference()) != "R", x.placement.at.x)).unwrap();
    assert_eq!(r_left.reference(), "R1");
    assert!(erc::check(s).iter().all(|v| v.rule != Rule::Unannotated && v.rule != Rule::DuplicateReference));
}

#[test]
fn gs09_10_values_and_footprints() {
    let lib = LibraryTable::builtin();
    let d = gs10(&lib);
    let s = &d.schematic;
    let r1 = s.sheets[0].symbols.iter().find(|x| x.reference() == "R1").unwrap();
    assert_eq!(r1.value(), "1k");
    assert_eq!(r1.footprint(), "Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal");
    // The assignment tool's filters narrow R1's choices.
    use cadrs_eda::assign::{Filters, candidates};
    let all = candidates(&lib, s, r1, &Filters::default()).len();
    let by_symbol = candidates(&lib, s, r1, &Filters { symbol_filters: true, ..Default::default() });
    assert!(by_symbol.len() < all && by_symbol.iter().all(|f| f.name().starts_with("R_")));
    let narrow = candidates(&lib, s, r1, &Filters { symbol_filters: true, pin_count: true, library: Some("Resistor_THT".into()), text: "DIN0309".into() });
    assert_eq!(narrow.len(), 1);
    assert_eq!(narrow[0].id, "Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal");
    let bt = s.sheets[0].symbols.iter().find(|x| x.reference() == "BT1").unwrap();
    assert!(candidates(&lib, s, bt, &Filters { pin_count: true, text: "1058".into(), ..Default::default() }).len() == 1);
}

#[test]
fn gs11_erc_needs_power_flags() {
    let lib = LibraryTable::builtin();
    let before = erc::check(&gs10(&lib).schematic);
    let rules: Vec<(Rule, String)> = before.iter().map(|v| (v.rule, v.items.join("; "))).collect();
    assert_eq!(before.len(), 2, "{rules:?}");
    assert!(before.iter().all(|v| v.rule == Rule::PowerPinNotDriven));
    assert_eq!(before[0].rule.message(), "Input Power pin not driven by any Output Power pins");
    assert!(before.iter().any(|v| v.items[0].starts_with("Symbol #PWR")));
    let after = erc::check(&gs11(&lib).schematic);
    assert_eq!(after, vec![], "{after:?}");
}

#[test]
fn gs12_bill_of_materials() {
    let lib = LibraryTable::builtin();
    let d = gs11(&lib);
    let csv = cadrs_eda::bom::csv(&d.schematic);
    assert_eq!(
        csv,
        "\"Reference\",\"Value\",\"Datasheet\",\"Footprint\",\"Qty\",\"DNP\"\n\
         \"BT1\",\"3V\",\"\",\"Battery:BatteryHolder_Keystone_1058_1x2032\",\"1\",\"\"\n\
         \"D1\",\"red\",\"\",\"LED_THT:LED_D5.0mm\",\"1\",\"\"\n\
         \"R1\",\"1k\",\"\",\"Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal\",\"1\",\"\"\n"
    );
}

#[test]
fn gs13_board_setup() {
    let lib = LibraryTable::builtin();
    let d = gs13(&lib);
    let b = &d.board;
    assert_eq!(b.copper_layers, 2);
    assert_eq!(b.stackup.thickness(), mm(1.6));
    let names: Vec<&str> = b.stackup.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["F.Silkscreen", "F.Paste", "F.Mask", "F.Cu", "Dielectric 1", "B.Cu", "B.Mask", "B.Paste", "B.Silkscreen"]);
    assert_eq!(b.stackup.layers[4].thickness, mm(1.51));
    let r = &b.rules;
    assert_eq!((r.min_track_width, r.min_annular_ring, r.min_via_diameter, r.hole_clearance, r.copper_edge_clearance), (mm(0.2), mm(0.1), mm(0.5), mm(0.25), mm(0.5)));
    assert_eq!((r.min_drill, r.hole_to_hole, r.min_uvia_diameter, r.min_uvia_drill, r.min_text_height, r.min_text_thickness), (mm(0.3), mm(0.25), mm(0.2), mm(0.1), mm(0.8), mm(0.08)));
    let c = r.class_of("anything");
    assert_eq!((c.name.as_str(), c.clearance, c.track_width, c.via_diameter, c.via_drill), ("Default", mm(0.2), mm(0.4), mm(0.6), mm(0.3)));
}

#[test]
fn gs14_update_pcb_from_schematic() {
    let lib = LibraryTable::builtin();
    let (d, rep) = gs14(&lib);
    assert_eq!(
        rep.messages,
        [
            "Processing symbol 'BT1:Battery:BatteryHolder_Keystone_1058_1x2032'.",
            "Processing symbol 'D1:LED_THT:LED_D5.0mm'.",
            "Processing symbol 'R1:Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal'.",
            "Add BT1 (footprint 'Battery:BatteryHolder_Keystone_1058_1x2032').",
            "Add D1 (footprint 'LED_THT:LED_D5.0mm').",
            "Add R1 (footprint 'Resistor_THT:R_Axial_DIN0309_L9.0mm_D3.2mm_P12.70mm_Horizontal').",
            "",
            "Total warnings: 0, errors: 0.",
        ]
    );
    let b = &d.board;
    assert_eq!(b.footprints.len(), 3);
    let net = |r: &str, n: &str| {
        let f = &b.footprints[be::footprint_index(b, r).unwrap()];
        f.footprint.pads.iter().find(|x| x.number == n).unwrap().net.clone().unwrap()
    };
    assert_eq!((net("BT1", "1"), net("BT1", "2")), ("VCC".into(), "GND".into()));
    assert_eq!((net("D1", "1"), net("D1", "2")), ("GND".into(), "led".into()));
    assert_eq!((net("R1", "1"), net("R1", "2")), ("VCC".into(), "led".into()));
    assert_eq!(b.nets, ["GND", "VCC", "led"]);
    assert_eq!(copper::ratsnest(b).len(), 3);
    // Running it again changes nothing.
    let mut again = d.clone();
    let rep2 = forward::update_pcb(&mut again, &lib, &forward::Options::default());
    assert!(rep2.messages.iter().all(|m| !m.starts_with("Add") && !m.starts_with("Change")));
    assert_eq!(again.board, d.board);
}

#[test]
fn gs15_16_outline_and_placement() {
    let lib = LibraryTable::builtin();
    let d = gs16(&lib);
    let b = &d.board;
    assert!((cadrs_eda::poly::area(&outline::board_region(b)) / 1e12 - 2250.0).abs() < 1e-6);
    let bt = &b.footprints[be::footprint_index(b, "BT1").unwrap()];
    assert_eq!(bt.placement.side, Side::Bottom);
    // Flipped left to right: GND (2) on the left, VCC (1) on the right, both on the back.
    assert!(pad(b, "BT1", "2").x < pad(b, "BT1", "1").x);
    assert!(copper::items(b).iter().filter(|c| c.net == "VCC" || c.net == "GND").all(|c| !matches!(c.item, copper::Item::Pad(id, _) if id == bt.id) || c.on(Layer::BottomCopper).is_some()));
    // D1 stands with pad 1 above pad 2; R1 lies with pad 2 (led) towards D1.
    assert!(pad(b, "D1", "1").y > pad(b, "D1", "2").y);
    assert_eq!(pad(b, "D1", "2").y, pad(b, "R1", "2").y);
    assert!(pad(b, "R1", "2").x < pad(b, "R1", "1").x);
    // Every pad inside the board.
    let region = outline::board_region(b);
    assert!(b.footprints.iter().all(|f| f.footprint.pads.iter().all(|x| cadrs_eda::poly::contains(&region, f.placement.apply(x.at)))));
    assert!(drc::check(b).violations.iter().all(|v| v.rule != drc::Rule::CourtyardOverlap));
}

#[test]
fn gs17_routing() {
    let lib = LibraryTable::builtin();
    let d = gs17(&lib);
    let b = &d.board;
    assert_eq!(copper::ratsnest(b), vec![], "all routed");
    assert!(b.tracks.iter().all(|t| t.width == mm(0.4)));
    let on = |net: &str| b.tracks.iter().filter(|t| t.net == net).map(|t| t.layer).collect::<Vec<_>>();
    assert!(on("led").iter().all(|l| *l == Layer::TopCopper));
    assert!(on("GND").iter().all(|l| *l == Layer::BottomCopper));
    assert!(on("VCC").contains(&Layer::BottomCopper) && on("VCC").contains(&Layer::TopCopper));
    assert_eq!(b.vias.len(), 1);
    assert_eq!((b.vias[0].diameter, b.vias[0].drill), (mm(0.6), mm(0.3)));
    let rep = drc::check(b);
    assert_eq!(rep.errors().count(), 0, "{:#?}", rep.violations);
}

#[test]
fn gs18_zone_fill() {
    let lib = LibraryTable::builtin();
    let d = gs18(&lib);
    let b = &d.board;
    let z = &b.zones[0];
    let area = zone::filled_area(z, Layer::BottomCopper);
    // Most of the 45 × 50 back, less the 0.5 mm edge band and the clearances.
    assert!(area > 1800.0 && area < 2160.0, "{area}");
    // GND is one piece of copper with the zone; VCC and led stay apart from it.
    let items = copper::items(b);
    let gnd: Vec<_> = items.iter().filter(|c| c.net == "GND").cloned().collect();
    assert_eq!(copper::islands(&gnd).len(), 1);
    let fill: Vec<_> = items.iter().filter(|c| matches!(c.item, copper::Item::Zone(..))).collect();
    for c in items.iter().filter(|c| c.net == "VCC" || c.net == "led") {
        for f in &fill {
            if let (Some(a), Some(z)) = (c.on(Layer::BottomCopper), f.on(Layer::BottomCopper)) {
                assert!(cadrs_eda::poly::distance(a, z) >= mm(0.5) as f64 - 1000.0, "{:?}", c.item);
            }
        }
    }
    // Clipped by the board edge clearance.
    let inner = cadrs_eda::poly::inflate(&outline::board_region(b), -mm(0.49));
    for f in &fill {
        let outside = cadrs_eda::poly::difference(f.on(Layer::BottomCopper).unwrap(), &inner);
        assert!(cadrs_eda::poly::area(&outside) < 1e6);
    }
    assert_eq!(drc::check(b).errors().count(), 0);
}

#[test]
fn gs19_drc() {
    let lib = LibraryTable::builtin();
    let mut d = gs18(&lib);
    let b = &mut d.board;
    let clean = drc::check(b);
    assert_eq!((clean.errors().count(), clean.unconnected.len()), (0, 0), "{:#?}", clean.violations);
    // Drag R1 into the fill without refilling: per pad a clearance, a hole clearance and a
    // solder mask violation.
    let r1 = be::footprint_index(b, "R1").unwrap();
    be::drag_footprint(b, r1, p(0.0, 3.0));
    let rep = drc::check(b);
    let mut rules: Vec<drc::Rule> = rep.errors().map(|v| v.rule).collect();
    rules.sort_by_key(|r| *r as u8);
    assert_eq!(
        rules,
        [drc::Rule::Clearance, drc::Rule::Clearance, drc::Rule::HoleClearance, drc::Rule::HoleClearance, drc::Rule::MaskBridge, drc::Rule::MaskBridge],
        "{:#?}",
        rep.violations
    );
    let c = rep.errors().find(|v| v.rule == drc::Rule::Clearance).unwrap();
    assert!(c.message.starts_with("Clearance violation (zone clearance 0.5000 mm; actual 0.0000 mm)"), "{}", c.message);
    assert!(c.items.iter().any(|i| i.contains("of R1")) && c.items.iter().any(|i| i.starts_with("Zone [GND]")));
    assert_eq!(rep.unconnected.len(), 0, "the drag kept the tracks on");
    // Refill (B): clean again.
    zone::fill_all(b);
    assert_eq!(drc::check(b).errors().count(), 0);
}

#[test]
fn gs21_fabrication_outputs() {
    use cadrs_eda::fab;
    let lib = LibraryTable::builtin();
    let d = gs18(&lib);
    let b = &d.board;
    let files = fab::gerbers(b, "getting-started");
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for want in ["F_Cu", "B_Cu", "F_Mask", "B_Mask", "F_Paste", "B_Paste", "F_Silkscreen", "B_Silkscreen", "Edge_Cuts"] {
        assert!(names.contains(&format!("getting-started-{want}.gbr").as_str()), "{want}");
    }
    for (name, text) in &files {
        let doc = gerber_parser::parse(std::io::BufReader::new(text.as_bytes())).unwrap_or_else(|(_, e)| panic!("{name}: {e:?}"));
        assert!(doc.errors().is_empty(), "{name}: {:?}", doc.errors());
        assert!(text.contains("%TF.FileFunction,"), "{name}");
    }
    let get = |suffix: &str| files.iter().find(|(n, _)| n.ends_with(&format!("-{suffix}.gbr"))).unwrap().1.clone();
    let front = get("F_Cu");
    // Round pads (D1 pad 2, R1's two) and the via are flashed; D1's square pad is a region.
    assert_eq!(front.matches("D03*").count(), 4, "{front}");
    assert!(front.contains("%TF.FileFunction,Copper,L1,Top*%"));
    let back = get("B_Cu");
    // The zone fill: dark regions, its holes cleared, then the other copper.
    assert!(back.contains("%LPC*%") && back.matches("G36*").count() > 3);
    assert!(get("Edge_Cuts").matches("D01*").count() >= 4);
    // Silkscreen text: R1 and D1 on the front, BT1 (mirrored) on the back.
    assert!(get("F_Silkscreen").matches("D01*").count() > 20);
    assert!(get("B_Silkscreen").matches("D01*").count() > 10);
    // Paste only where SMD pads are: BT1's two on the back, none on the front.
    assert_eq!(get("F_Paste").matches("G36*").count() + get("F_Paste").matches("D03*").count(), 0);
    assert_eq!(get("B_Paste").matches("G36*").count(), 2);
    let job = fab::job_file(b, "getting-started");
    assert!(job.contains("\"LayerNumber\": 2") && job.contains("\"X\": 45.0000"));
    // Drill: D1's two (0.9), R1's two (0.8), the via (0.3); nothing unplated.
    let pth = fab::excellon(b, true);
    assert!(pth.contains("T1C0.300\nT2C0.800\nT3C0.900\n"), "{pth}");
    assert_eq!(pth.lines().filter(|l| l.starts_with('X')).count(), 5);
    assert_eq!(fab::excellon(b, false).lines().filter(|l| l.starts_with('X')).count(), 0);
}

#[test]
fn gs22_libraries() {
    let lib = project_library();
    assert!(lib.symbol("getting-started:M2011S3A1W03").is_some());
    assert!(lib.footprint(SW_FP).is_some());
    let l = lib.library("getting-started").unwrap();
    assert_eq!(l.scope, Scope::Project);
    // The new part is found by its keywords.
    assert_eq!(lib.search_symbols("toggle", false)[0].name(), "M2011S3A1W03");
}

#[test]
fn gs23_switch_symbol() {
    let s = switch_symbol();
    assert_eq!(s.field("Reference").unwrap().value(), "SW?");
    assert!(!s.show_pin_names);
    let pins: Vec<(&str, &str, Pt, f64)> = s.pins.iter().map(|x| (x.number.as_str(), x.name.as_str(), x.at, x.angle)).collect();
    assert_eq!(pins, [("2", "A", Pt::new(-200 * MIL, 0), 0.0), ("3", "B", Pt::new(200 * MIL, 0), 180.0)]);
    assert!(le::off_grid_pins(&s).is_empty());
    assert_eq!(s.graphics.len(), 3);
    let mut off = s.clone();
    off.pins[0].at = Pt::mm(-5.0, 0.0);
    assert_eq!(le::off_grid_pins(&off), ["2"]);
}

#[test]
fn gs24_switch_footprint() {
    let f = switch_footprint();
    assert_eq!(f.pads.iter().map(|x| (x.number.as_str(), x.at)).collect::<Vec<_>>(), [("2", p(0.0, -4.7)), ("3", p(0.0, -9.4))]);
    assert!(f.pads.iter().all(|x| x.size.w == mm(2.22) && x.drill.unwrap().size.w == mm(1.62)));
    assert_eq!(f.attrs.mount, MountKind::ThroughHole);
    assert_eq!(le::check_footprint(&f), Vec::<String>::new());
    let cy = le::layer_bounds(&f, Layer::TopCourtyard).unwrap();
    assert_eq!((cy.min, cy.max), (p(-4.2, -11.45), p(4.2, 2.05)));
    // Without the courtyard the check complains.
    let mut bare = f.clone();
    bare.shapes.retain(|s| s.layer != Layer::TopCourtyard);
    assert_eq!(le::check_footprint(&bare), ["Missing courtyard"]);
}

#[test]
fn gs25_switch_in_schematic_and_board() {
    let (d, _, rep) = gs25();
    assert!(rep.messages.contains(&format!("Add SW1 (footprint '{SW_FP}').")), "{:?}", rep.messages);
    assert!(rep.messages.contains(&"Connect BT1 pad 1 to Net-(BT1-+).".to_string()), "{:?}", rep.messages);
    let s = &d.schematic;
    assert_eq!(erc::check(s), vec![]);
    let nl = netlist(s);
    let pins = |n: &str| nl.net(n).unwrap().part_pins().map(|x| format!("{}.{}", x.reference, x.number)).collect::<Vec<_>>();
    assert_eq!(pins("Net-(BT1-+)"), ["BT1.1", "SW1.2"]);
    assert_eq!(pins("VCC"), ["R1.1", "SW1.3"]);
    let sw = s.sheets[0].symbols.iter().find(|x| x.reference() == "SW1").unwrap();
    assert_eq!(sw.footprint(), SW_FP);
    let b = &d.board;
    assert_eq!(copper::ratsnest(b), vec![]);
    let rep = drc::check(b);
    assert_eq!(rep.errors().count(), 0, "{:#?}", rep.violations);
    assert!(rep.violations.is_empty(), "{:#?}", rep.violations);
    assert!(cadrs_eda::bom::csv(s).contains("\"SW1\",\"M2011S3A1W03\""));
}

#[test]
fn gs26_3d_models() {
    let (d, _, _) = gs25();
    let b = &d.board;
    let sw = &b.footprints[be::footprint_index(b, "SW1").unwrap()];
    let m = &sw.footprint.models[0];
    assert_eq!(m.source, "getting-started.3dshapes/Switch_Toggle_SPST_NKK_M2011S3A1x03.step");
    assert_eq!((m.rotation, m.scale, m.opacity), ([0.0, 0.0, 90.0], [1.0; 3], 1.0));
    // Every library footprint names a model, even before the file exists.
    assert!(b.footprints.iter().all(|f| !f.footprint.models.is_empty()));
}

#[test]
fn gs23_24_editor_details() {
    // Insert repeats the last pin 100 mil lower, numbered on.
    let mut s = le::new_symbol("X", "SW", false);
    le::add_pin(&mut s, &le::PinProps::new("A", "2", Pt::new(-200 * MIL, 0), le::Orientation::Right));
    let b = le::repeat_pin(&mut s).unwrap();
    assert_eq!((s.pins[b].number.as_str(), s.pins[b].at), ("3", Pt::new(-200 * MIL, -100 * MIL)));
    // A second pad copies the first, numbered on; sizes typed as expressions.
    let mut f = le::new_footprint("F", "F", MountKind::ThroughHole);
    let a = le::add_pad(&mut f, p(0.0, -4.7));
    f.pads[a].number = "2".into();
    let ring = eval("1.62 + 2*0.15", Unit::Mm).unwrap();
    f.pads[a].size = cadrs_eda::units::Size::new(ring, ring);
    let b = le::add_pad(&mut f, p(0.0, -9.4));
    assert_eq!((f.pads[b].number.as_str(), f.pads[b].size.w), ("3", mm(1.92)));
}

#[test]
fn gs25_select_connected_through_the_via() {
    let lib = LibraryTable::builtin();
    let d = gs18(&lib);
    let b = &d.board;
    let t = b.tracks.iter().find(|t| t.net == "VCC" && t.layer == Layer::BottomCopper).unwrap().id;
    // U: the back track up to the via; U again: through it to R1.
    assert_eq!(be::select_connected(b, t, false).len(), 1);
    assert_eq!(be::select_connected(b, t, true).len(), 4);
}

/// A board imported without its schematic has no sheet; editing gives it one first (placing a
/// symbol on sheet 0 panicked).
#[test]
fn a_design_without_sheets_gets_one_to_edit() {
    let lib = LibraryTable::builtin();
    let mut d = cadrs_eda::Design::default();
    d.ensure_sheet();
    d.ensure_sheet();
    assert_eq!(d.schematic.sheets.len(), 1);
    se::place_symbol(&mut d.schematic, 0, lib.symbol("Device:Battery_Cell").unwrap(), Pt::mm(50.0, 50.0), Uuid::new_v4());
    assert_eq!(d.schematic.sheets[0].symbols[0].reference(), "BT1");
}
