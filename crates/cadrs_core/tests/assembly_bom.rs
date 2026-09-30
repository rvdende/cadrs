//! Part properties and the Bill of Materials (P3B.6, `intro-to-assemblies.md` A20, X13;
//! `test-drive.md` TD9) on the Ex3 end state (`samples::pneumatic_ex3`,
//! `fixtures/pneumatic_cylinder_ex3.cadrs`, `ex3-drawing.png`: Instances (26)).
//!
//! Counts on the Ex3 model: **structured, top level: 9 rows** (Rear Cap mount, Barrel, Rear Cap
//! subassembly, Top Cap subassembly, Structural Rod ×4, Piston & Rod, O-Ring 0.185 ×3, Hex cap
//! screw ×6, Hex nut ×8); **structured, expanded: 14 rows** (+ Rear Cap, O-Ring 0.125 ×2 under
//! the Rear Cap subassembly; Retaining Plate, Top Cap, O-Ring 0.125 ×2 under the Top Cap
//! subassembly); **flattened: 11 rows** (the two subassemblies' parts at the top, O-Ring 0.125
//! ×4), 31 parts in all.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::bom::{self, ApplyBomTemplate, Bom, BomColumn, BomOptions, BomRowKey, BomSettings, BomView, SaveBomTemplate, SetBomSettings};
use cadrs_core::properties::{
    self, AddPropertyDefinition, GenerateMissingPartNumbers, PropertyKey, PropertyKind, PropertyOwner, PropertyValue, SetProperties, SubassemblyBom,
};
use cadrs_core::rebuild::Build;
use cadrs_core::samples::pneumatic as pc;
use cadrs_core::samples::pneumatic_ex2 as ex2;
use cadrs_core::samples::pneumatic_ex3 as ex3;
use cadrs_core::{Document, ElementId, History};
use cadrs_sketch::units::{LengthUnit, MassUnit, Units};

struct Fx {
    doc: Document,
    h: History,
    builds: HashMap<ElementId, Arc<Build>>,
}

fn inch_pound() -> Units {
    Units { length: LengthUnit::Inch, mass: MassUnit::Pound, decimals: 3, ..Units::default() }
}

impl Fx {
    fn new() -> Self {
        Self { doc: ex3::document().unwrap(), h: History::default(), builds: HashMap::new() }
    }

    fn run(&mut self, cmd: &dyn cadrs_core::Command) {
        self.h.execute(&mut self.doc, cmd).unwrap_or_else(|e| panic!("{}: {e}", cmd.label()));
    }

    fn bom(&mut self, options: &BomOptions) -> Bom {
        let doc = &self.doc;
        let builds = &mut self.builds;
        bom::compute(doc, ex2::ASSEMBLY, options, &inch_pound(), |e| {
            if let std::collections::hash_map::Entry::Vacant(v) = builds.entry(e) {
                v.insert(cadrs_core::rebuild::build(doc.element(e)?.features()));
            }
            builds.get(&e).cloned()
        })
        .unwrap()
    }

    fn settings(&self) -> BomSettings {
        self.doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap().bom.clone()
    }

    fn set(&mut self, s: BomSettings) {
        self.run(&SetBomSettings { element: ex2::ASSEMBLY, settings: s, label: "BOM".into() });
    }
}

fn all() -> BomOptions {
    BomOptions { expand_all: true, ..Default::default() }
}

fn col(b: &Bom, c: BomColumn) -> usize {
    b.columns.iter().position(|x| *x == c).unwrap()
}

/// (item, name, quantity) of every row.
fn rows(b: &Bom) -> Vec<(String, String, u32)> {
    let (i, n) = (col(b, BomColumn::Item), col(b, BomColumn::Property(PropertyKey::Name)));
    b.rows.iter().map(|r| (r.cells[i].clone(), r.cells[n].clone(), r.quantity)).collect()
}

const PART: fn(cadrs_core::PartId) -> PropertyOwner = |part| PropertyOwner::Part { element: pc::STUDIO, part };

#[test]
fn the_ex3_fixture_is_current() {
    // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test assembly_bom`.
    let doc = ex3::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pneumatic_cylinder_ex3.cadrs");
    let text = ron::ser::to_string_pretty(&pc::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/pneumatic_cylinder_ex3.cadrs is out of date");
    // Instances (26) as `ex3-drawing.png` counts them: 12 rows at the top level plus the 14
    // fasteners in Hardware.
    let asm = doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert_eq!(asm.instances.len(), 26);
    assert_eq!(asm.folders[0].name, "Hardware");
    assert_eq!(asm.folders[0].features.len(), 14);
    // P3B.5 judge: the Top Cap subassembly numbers its O-rings <1>, <2> (`ex3-step16.png`).
    let top = doc.element(ex3::TOP_SUB_EL).unwrap().assembly_model().unwrap();
    let idx: Vec<u32> = top.instances.iter().filter(|i| i.source.part() == Some(pc::ORING_125)).map(|i| i.index).collect();
    assert_eq!(idx, [1, 2]);
}

/// A20.4: flattened vs structured counts on the Ex3 model.
#[test]
fn structured_and_flattened_counts_on_ex3() {
    let mut fx = Fx::new();
    let top = fx.bom(&BomOptions::default());
    assert_eq!(
        rows(&top),
        [
            ("1", "Rear Cap mount", 1),
            ("2", "Barrel", 1),
            ("3", "Rear Cap subassembly", 1),
            ("4", "Top Cap subassembly", 1),
            ("5", "Structural Rod", 4),
            ("6", "Piston & Rod", 1),
            ("7", "O-Ring 0.185", 3),
            ("8", "Hex cap screw 1/4-28 x 0.75", 6),
            ("9", "Hex nut 3/8-16", 8),
        ]
        .map(|(a, b, c)| (a.to_string(), b.to_string(), c))
    );
    assert_eq!(top.total_quantity, 26);
    assert!(top.rows[2].has_children && !top.rows[2].expanded);
    // Expanded: the subassemblies' components under them, numbered 3.1, 3.2, 4.1, …
    let expanded = fx.bom(&all());
    assert_eq!(expanded.rows.len(), 14);
    let items: Vec<String> = rows(&expanded).into_iter().map(|r| format!("{} {} x{}", r.0, r.1, r.2)).collect();
    assert_eq!(&items[2..8], ["3 Rear Cap subassembly x1", "3.1 Rear Cap x1", "3.2 O-Ring 0.125 x2", "4 Top Cap subassembly x1", "4.1 Retaining Plate x1", "4.2 Top Cap x1"]);
    assert_eq!(items[8], "4.3 O-Ring 0.125 x2");
    assert_eq!(expanded.rows[3].depth, 1);
    // Only one subassembly expanded (double-clicking its item number).
    let one = fx.bom(&BomOptions { expanded: vec![BomRowKey { path: vec![], owner: PropertyOwner::Assembly { element: ex3::TOP_SUB_EL } }], ..Default::default() });
    assert_eq!(one.rows.len(), 12);
    // Flattened: every part at the top level, counted over the tree.
    let mut s = fx.settings();
    s.view = BomView::Flattened;
    fx.set(s);
    let flat = fx.bom(&BomOptions::default());
    let names: Vec<(String, u32)> = rows(&flat).into_iter().map(|r| (r.1, r.2)).collect();
    assert_eq!(flat.rows.len(), 11);
    assert_eq!(
        names,
        [
            ("Rear Cap mount", 1),
            ("Barrel", 1),
            ("Rear Cap", 1),
            ("O-Ring 0.125", 4),
            ("Retaining Plate", 1),
            ("Top Cap", 1),
            ("Structural Rod", 4),
            ("Piston & Rod", 1),
            ("O-Ring 0.185", 3),
            ("Hex cap screw 1/4-28 x 0.75", 6),
            ("Hex nut 3/8-16", 8),
        ]
        .map(|(a, b)| (a.to_string(), b))
    );
    assert_eq!(flat.total_quantity, 31);
    // Hovering a row highlights its parts: the four O-Ring 0.125, in two subassemblies.
    assert_eq!(flat.rows[3].occurrences.len(), 4);
    assert_eq!(flat.rows[3].instances, vec![ex3::REAR_SUB, ex3::TOP_SUB]);
}

/// A20.5: Subassembly BOM behavior.
#[test]
fn subassembly_bom_behaviour() {
    let mut fx = Fx::new();
    let top_sub = PropertyOwner::Assembly { element: ex3::TOP_SUB_EL };
    let set = |fx: &mut Fx, b: SubassemblyBom| {
        fx.run(&SetProperties { owners: vec![top_sub], values: vec![(PropertyKey::BomBehavior, PropertyValue::BomBehavior(b))], label: "BOM behavior".into() });
    };
    // Show assembly only: one line, its parts not listed (structured and flattened).
    set(&mut fx, SubassemblyBom::AssemblyOnly);
    let b = fx.bom(&all());
    assert_eq!(b.rows.len(), 11);
    assert!(!b.rows[3].has_children);
    let mut s = fx.settings();
    s.view = BomView::Flattened;
    fx.set(s.clone());
    let f = fx.bom(&BomOptions::default());
    let names: Vec<(String, u32)> = rows(&f).into_iter().map(|r| (r.1, r.2)).collect();
    assert!(names.contains(&("Top Cap subassembly".into(), 1)));
    assert!(names.contains(&("O-Ring 0.125".into(), 2)));
    assert_eq!(f.rows.len(), 10);
    // Show components only: its parts as if inserted at the top level.
    s.view = BomView::Structured;
    fx.set(s);
    set(&mut fx, SubassemblyBom::ComponentsOnly);
    let b = fx.bom(&BomOptions::default());
    let names: Vec<String> = rows(&b).into_iter().map(|r| r.1).collect();
    assert_eq!(&names[..7], ["Rear Cap mount", "Barrel", "Rear Cap subassembly", "Retaining Plate", "Top Cap", "O-Ring 0.125", "Structural Rod"]);
    assert_eq!(b.rows.len(), 11);
    assert_eq!(fx.bom(&all()).rows.len(), 13);
    // Back to the default, as one undo step each.
    fx.h.undo(&mut fx.doc);
    fx.h.undo(&mut fx.doc);
    fx.h.undo(&mut fx.doc);
    fx.h.undo(&mut fx.doc);
    assert_eq!(fx.bom(&all()).rows.len(), 14);
}

/// A20.10 / TD9.3: editing a BOM cell writes the part's property, and a property edit shows in
/// the BOM (two-way).
#[test]
fn bom_cells_and_properties_are_the_same_data() {
    let mut fx = Fx::new();
    let barrel = PART(pc::BARREL);
    // A cell edited in the BOM: the part's Part number.
    let b = fx.bom(&BomOptions::default());
    let pn = col(&b, BomColumn::Property(PropertyKey::PartNumber));
    let row = b.rows.iter().position(|r| r.key.owner == barrel).unwrap();
    assert_eq!(b.rows[row].cells[pn], "");
    fx.run(&SetProperties { owners: vec![b.rows[row].key.owner], values: vec![(PropertyKey::PartNumber, PropertyValue::Text("CYL-100".into()))], label: "Edit Part number".into() });
    assert_eq!(properties::text(&fx.doc, barrel, PropertyKey::PartNumber, None), "CYL-100");
    assert_eq!(properties::properties(&fx.doc, barrel).part_number.as_deref(), Some("CYL-100"));
    // The Properties dialog's edit (Description, Vendor) shows in the BOM.
    fx.run(&SetProperties {
        owners: vec![barrel],
        values: vec![(PropertyKey::Description, PropertyValue::Text("Aluminum tube, 1.5 bore".into())), (PropertyKey::Vendor, PropertyValue::Text("Acme".into()))],
        label: "Properties".into(),
    });
    let mut s = fx.settings();
    s.columns.push(BomColumn::Property(PropertyKey::Vendor));
    fx.set(s);
    let b = fx.bom(&BomOptions::default());
    let (d, v) = (col(&b, BomColumn::Property(PropertyKey::Description)), col(&b, BomColumn::Property(PropertyKey::Vendor)));
    assert_eq!(b.rows[row].cells[d], "Aluminum tube, 1.5 bore");
    assert_eq!(b.rows[row].cells[v], "Acme");
    // A standard content part's cells are its configuration's (A19.3), editable too.
    let screw = b.rows.iter().find(|r| r.cells[col(&b, BomColumn::Property(PropertyKey::Name))].starts_with("Hex cap")).unwrap().key.owner;
    assert_eq!(properties::text(&fx.doc, screw, PropertyKey::PartNumber, None), "HCS-1/4-28-0.75-SS");
    fx.run(&SetProperties { owners: vec![screw], values: vec![(PropertyKey::PartNumber, PropertyValue::Text("HCS-2528".into()))], label: "Edit".into() });
    assert_eq!(fx.doc.standard_content[0].part_number, "HCS-2528");
    // The Material cell's picker: the part's material (and its mass follows).
    let steel = cadrs_core::material::library("Steel").or_else(|| cadrs_core::material::search("steel").first().map(|m| m.material())).unwrap();
    fx.run(&SetProperties { owners: vec![barrel], values: vec![(PropertyKey::Material, PropertyValue::Material(Some(steel.clone())))], label: "Material".into() });
    let b = fx.bom(&BomOptions::default());
    assert_eq!(b.rows[row].cells[col(&b, BomColumn::Property(PropertyKey::Material))], steel.name);
    // An assembly's name is its tab's.
    let top_sub = PropertyOwner::Assembly { element: ex3::TOP_SUB_EL };
    fx.run(&SetProperties { owners: vec![top_sub], values: vec![(PropertyKey::Name, PropertyValue::Text("Top Cap assembly".into()))], label: "Name".into() });
    assert_eq!(fx.doc.element(ex3::TOP_SUB_EL).unwrap().name, "Top Cap assembly");
    // Rules: an empty name, a wrong unit of measure and an assembly's material are refused.
    for (o, k, v) in [
        (barrel, PropertyKey::Name, PropertyValue::Text("  ".into())),
        (barrel, PropertyKey::UnitOfMeasure, PropertyValue::Text("Furlong".into())),
        (top_sub, PropertyKey::Material, PropertyValue::Material(Some(steel.clone()))),
    ] {
        assert!(fx.h.execute(&mut fx.doc, &SetProperties { owners: vec![o], values: vec![(k, v)], label: "x".into() }).is_err());
    }
    // Every edit is one undo step: back to the start.
    while fx.h.can_undo() {
        fx.h.undo(&mut fx.doc);
    }
    assert_eq!(fx.doc, ex3::document().unwrap());
}

/// Custom per-document properties (text, number, boolean, list) and the mass override.
#[test]
fn custom_properties_and_mass_override() {
    let mut fx = Fx::new();
    let barrel = PART(pc::BARREL);
    fx.run(&AddPropertyDefinition { name: "Cost".into(), kind: PropertyKind::Number });
    fx.run(&AddPropertyDefinition { name: "Finish".into(), kind: PropertyKind::List(vec!["Anodized".into(), "Plain".into()]) });
    assert!(fx.h.execute(&mut fx.doc, &AddPropertyDefinition { name: "Vendor".into(), kind: PropertyKind::Text }).is_err(), "a built-in name");
    let cost = PropertyKey::Custom(fx.doc.properties.definitions[0].id);
    let finish = PropertyKey::Custom(fx.doc.properties.definitions[1].id);
    assert!(fx.h.execute(&mut fx.doc, &SetProperties { owners: vec![barrel], values: vec![(cost, PropertyValue::Text("cheap".into()))], label: "x".into() }).is_err());
    fx.run(&SetProperties { owners: vec![barrel], values: vec![(cost, PropertyValue::Text("12.5".into())), (finish, PropertyValue::Text("anodized".into()))], label: "x".into() });
    assert_eq!(properties::text(&fx.doc, barrel, finish, None), "Anodized");
    let mut s = fx.settings();
    s.columns.extend([BomColumn::Property(cost), BomColumn::Property(PropertyKey::Mass)]);
    fx.set(s);
    let b = fx.bom(&BomOptions::default());
    assert_eq!(b.labels[6], "Cost");
    let row = b.rows.iter().position(|r| r.key.owner == barrel).unwrap();
    assert_eq!(b.rows[row].cells[6], "12.5");
    let computed = b.rows[row].unit_mass.unwrap();
    // A mass override: the BOM, and the Mass properties panel's sums, use it.
    fx.run(&SetProperties { owners: vec![barrel], values: vec![(PropertyKey::Mass, PropertyValue::Mass(Some(2.0 * 0.453_592_37)))], label: "Mass".into() });
    let b = fx.bom(&BomOptions::default());
    assert_eq!(b.rows[row].cells[7], "2.000 lb");
    let total_before = computed;
    let asm = fx.doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
    let build = cadrs_core::rebuild::build(fx.doc.element(pc::STUDIO).unwrap().features());
    let docref = &fx.doc;
    let (parts, props) = cadrs_core::assembly::instance_parts(docref, &asm, |e| if e == pc::STUDIO { Some(build.clone()) } else { Some(cadrs_core::rebuild::build(docref.element(e)?.features())) });
    let m = cadrs_core::assembly::mass_report(&parts, &props, &[ex2::BARREL]).unwrap();
    assert!((m.mass.unwrap().mass - 2.0 * 0.453_592_37).abs() < 1e-9, "{} vs {total_before}", m.mass.unwrap().mass);
    // Removing a definition takes its values and columns.
    let id = fx.doc.properties.definitions[0].id;
    fx.run(&properties::RemovePropertyDefinition { id });
    assert!(!fx.settings().columns.contains(&BomColumn::Property(cost)));
    assert_eq!(properties::text(&fx.doc, barrel, cost, None), "");
}

/// A20.11: Generate missing part numbers fills unique sequential numbers, skips the numbers in
/// use and keeps the part numbers already there.
#[test]
fn generate_missing_part_numbers() {
    let mut fx = Fx::new();
    let barrel = PART(pc::BARREL);
    let rod = PART(pc::STRUCTURAL_ROD);
    fx.run(&SetProperties { owners: vec![barrel], values: vec![(PropertyKey::PartNumber, PropertyValue::Text("PRT-000002".into()))], label: "x".into() });
    fx.run(&SetProperties { owners: vec![rod], values: vec![(PropertyKey::PartNumber, PropertyValue::Text("ROD-7".into()))], label: "x".into() });
    let owners = fx.bom(&all()).owners();
    fx.run(&GenerateMissingPartNumbers { owners: owners.clone() });
    let pn = |fx: &Fx, o: PropertyOwner| properties::text(&fx.doc, o, PropertyKey::PartNumber, None);
    assert_eq!(pn(&fx, barrel), "PRT-000002");
    assert_eq!(pn(&fx, rod), "ROD-7");
    // In BOM order: Rear Cap mount, (Barrel kept), Rear Cap subassembly, Rear Cap, O-Ring 0.125,
    // Top Cap subassembly, …; the fasteners keep their library numbers.
    assert_eq!(pn(&fx, PART(pc::REAR_CAP_MOUNT)), "PRT-000001");
    assert_eq!(pn(&fx, PropertyOwner::Assembly { element: ex3::REAR_SUB_EL }), "PRT-000003");
    assert_eq!(pn(&fx, PART(pc::REAR_CAP)), "PRT-000004");
    assert_eq!(pn(&fx, PART(pc::ORING_125)), "PRT-000005");
    assert_eq!(pn(&fx, PropertyOwner::Assembly { element: ex3::TOP_SUB_EL }), "PRT-000006");
    assert_eq!(pn(&fx, PART(pc::ORING_185)), "PRT-000010");
    let all_numbers: Vec<String> = owners.iter().map(|o| pn(&fx, *o)).collect();
    assert!(all_numbers.iter().all(|n| !n.is_empty()));
    let mut unique = all_numbers.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), all_numbers.len(), "unique: {all_numbers:?}");
    assert!(all_numbers.contains(&"HCS-1/4-28-0.75-SS".to_string()));
    assert_eq!(fx.doc.properties.numbering.next, 11);
    // Again: nothing is missing, nothing changes.
    let before = fx.doc.clone();
    fx.run(&GenerateMissingPartNumbers { owners });
    assert_eq!(fx.doc, before);
}

/// A20.8: Suppress from this BOM excludes the row and its quantity; show excluded lists it with
/// "–"; unsuppress.
#[test]
fn suppress_from_bom() {
    let mut fx = Fx::new();
    let b0 = fx.bom(&all());
    let rods = b0.rows.iter().find(|r| r.key.owner == PART(pc::STRUCTURAL_ROD)).unwrap().key.clone();
    let mut s = fx.settings();
    s.excluded.push(rods.clone());
    fx.set(s.clone());
    let b = fx.bom(&all());
    assert_eq!(b.rows.len(), b0.rows.len() - 1);
    assert_eq!(b.total_quantity, b0.total_quantity - 4);
    // The items after it are renumbered.
    assert_eq!(rows(&b)[9].1, "Piston & Rod");
    assert_eq!(rows(&b)[9].0, "5");
    // Show excluded: listed, "–", no number, not counted.
    s.show_excluded = true;
    fx.set(s.clone());
    let b = fx.bom(&all());
    assert_eq!(b.rows.len(), b0.rows.len());
    let r = b.rows.iter().find(|r| r.key == rods).unwrap();
    assert!(r.excluded && r.item.is_none());
    assert_eq!(r.cells[0], "–");
    assert_eq!(b.total_quantity, b0.total_quantity - 4);
    // A subassembly suppressed takes its components along.
    let sub = BomRowKey { path: vec![], owner: PropertyOwner::Assembly { element: ex3::REAR_SUB_EL } };
    s.excluded.push(sub);
    fx.set(s.clone());
    let b = fx.bom(&all());
    assert!(b.rows.iter().filter(|r| r.key.path == vec![ex3::REAR_SUB_EL]).all(|r| r.excluded));
    // Unsuppress.
    s.excluded.clear();
    s.show_excluded = false;
    fx.set(s);
    assert_eq!(fx.bom(&all()), b0);
}

/// A20.9: the top-level assembly row with the totals (quantity, mass).
#[test]
fn top_level_row_totals() {
    let mut fx = Fx::new();
    let mut s = fx.settings();
    s.top_level_row = true;
    s.columns.push(BomColumn::Property(PropertyKey::Mass));
    fx.set(s);
    let b = fx.bom(&BomOptions::default());
    let t = &b.rows[0];
    assert!(t.top_level && t.item.is_none());
    assert_eq!(t.quantity, 26);
    assert_eq!(t.cells[col(&b, BomColumn::Property(PropertyKey::Name))], "Cylinder assembly");
    // The mass is the sum over the rows, and the whole assembly's.
    let sum: f64 = b.rows[1..].iter().map(|r| r.unit_mass.unwrap() * r.quantity as f64).sum();
    assert!((t.unit_mass.unwrap() - sum).abs() < 1e-9);
    let asm = PropertyOwner::Assembly { element: ex2::ASSEMBLY };
    let whole = properties::mass(&fx.doc, asm, &mut |e| Some(cadrs_core::rebuild::build(fx.doc.element(e)?.features()))).unwrap();
    assert!((whole - sum).abs() < 1e-9);
}

/// A20.3: double-clicking a header sorts; the order otherwise follows the Instances list.
#[test]
fn sorting_by_a_column() {
    let mut fx = Fx::new();
    let b = fx.bom(&BomOptions { sort: Some((BomColumn::Quantity, false)), ..Default::default() });
    let q: Vec<u32> = b.rows.iter().map(|r| r.quantity).collect();
    assert_eq!(q, [8, 6, 4, 3, 1, 1, 1, 1, 1]);
    // Item numbers stay with their items.
    assert_eq!(b.rows[0].item.as_deref(), Some("9"));
    let b = fx.bom(&BomOptions { sort: Some((BomColumn::Property(PropertyKey::Name), true)), ..Default::default() });
    assert_eq!(rows(&b)[0].1, "Barrel");
}

/// A20.7: Export to CSV matches the golden file; Copy table is tab-separated.
#[test]
fn csv_export_matches_the_golden_file() {
    let mut fx = Fx::new();
    let mut s = fx.settings();
    s.top_level_row = true;
    s.columns.push(BomColumn::Property(PropertyKey::Mass));
    fx.set(s);
    fx.run(&SetProperties {
        owners: vec![PART(pc::BARREL)],
        values: vec![(PropertyKey::Description, PropertyValue::Text("Tube, 1.5\" bore, anodized".into()))],
        label: "x".into(),
    });
    let b = fx.bom(&all());
    let csv = b.to_csv();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ex3_bom.csv");
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &csv).unwrap();
    }
    let golden = std::fs::read_to_string(&path).expect("tests/fixtures/ex3_bom.csv");
    assert_eq!(csv, golden);
    assert!(csv.contains("\"Tube, 1.5\"\" bore, anodized\""));
    let tsv = b.to_tsv();
    assert_eq!(tsv.lines().count(), 16);
    assert!(tsv.lines().next().unwrap().starts_with("Item\tQuantity\tPart number"));
}

/// A20.6, A20.7: columns added, moved and removed, saved as a template and applied again; the
/// template survives a save and load of the document.
#[test]
fn template_round_trip() {
    let mut fx = Fx::new();
    let mut s = fx.settings();
    s.columns.push(BomColumn::Property(PropertyKey::Vendor));
    // Move Vendor left twice, remove Material.
    let n = s.columns.len();
    s.columns.swap(n - 1, n - 2);
    s.columns.swap(n - 2, n - 3);
    s.columns.retain(|c| *c != BomColumn::Property(PropertyKey::Material));
    s.view = BomView::Flattened;
    s.top_level_row = true;
    fx.set(s.clone());
    fx.run(&SaveBomTemplate { element: ex2::ASSEMBLY, name: "Purchasing".into() });
    assert_eq!(fx.doc.properties.bom_templates.len(), 1);
    // Back to the default layout, then the template again.
    fx.set(BomSettings::default());
    assert_eq!(fx.bom(&BomOptions::default()).labels, ["Item", "Quantity", "Part number", "Name", "Description", "Material"]);
    fx.run(&ApplyBomTemplate { element: ex2::ASSEMBLY, name: "Purchasing".into() });
    assert_eq!(fx.settings(), s);
    assert_eq!(fx.bom(&BomOptions::default()).labels, ["Item", "Quantity", "Part number", "Name", "Vendor", "Description"]);
    // Saved with the document.
    let text = ron::ser::to_string_pretty(&pc::file(fx.doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    let back: cadrs_core::DocumentFile = ron::from_str(&text).unwrap();
    assert_eq!(back.document, fx.doc);
    assert_eq!(back.document.properties.bom_templates[0].columns, s.columns);
    // Undo: the template goes back to the default layout; the template stays saved until its
    // own step is undone.
    fx.h.undo(&mut fx.doc);
    assert_eq!(fx.settings(), BomSettings::default());
    fx.h.undo(&mut fx.doc);
    fx.h.undo(&mut fx.doc);
    assert!(fx.doc.properties.bom_templates.is_empty());
    // A duplicate column is refused.
    let mut bad = BomSettings::default();
    bad.columns.push(BomColumn::Quantity);
    assert!(fx.h.execute(&mut fx.doc, &SetBomSettings { element: ex2::ASSEMBLY, settings: bad, label: "x".into() }).is_err());
}

/// The built-in Default template puts the default columns back (keeping suppressed rows), as
/// one undo step, without being saved in the document.
#[test]
fn builtin_default_template() {
    use cadrs_core::assembly::bom::BomTemplate;
    let mut fx = Fx::new();
    let mut s = fx.settings();
    s.columns.retain(|c| *c != BomColumn::Property(PropertyKey::Material));
    s.columns.push(BomColumn::Property(PropertyKey::Vendor));
    s.view = BomView::Flattened;
    fx.set(s.clone());
    fx.run(&ApplyBomTemplate { element: ex2::ASSEMBLY, name: BomTemplate::DEFAULT_NAME.into() });
    assert_eq!(fx.settings().columns, bom::default_columns());
    assert_eq!(fx.settings().view, BomView::Structured);
    assert!(fx.doc.properties.bom_templates.is_empty());
    fx.h.undo(&mut fx.doc);
    assert_eq!(fx.settings(), s);
}

/// A document from before P3B.6 loads with the default properties and BOM (serde defaults).
#[test]
fn old_documents_load_with_defaults() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pneumatic_cylinder_ex2.cadrs");
    let stored = cadrs_core::Store::load_path(&path).unwrap();
    assert!(stored.document.properties.is_default());
    let asm = stored.document.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap();
    assert!(asm.bom.is_default() && asm.properties.is_empty());
}
