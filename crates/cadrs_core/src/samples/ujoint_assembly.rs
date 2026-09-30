//! The Universal Joint Assembly stand-in (P3C.5, D12, `fixtures/ujoint_assembly_standin.cadrs`).
//!
//! Onshape's public "Exercise: Universal Joint Assembly Drawing" can't be copied, so the drawing
//! exercise starts from this document, in inches, built through the command layer with fixed
//! ids (the fixture is regenerated exactly, `cadrs_core/tests/ujoint_assembly.rs`):
//!
//! - **Universal Joint** (Part Studio): the Ex1 flange ([`super::ujoint`]), renamed
//!   "Universal Joint Flange".
//! - **Universal Joint Components** (Part Studio), each part built at the origin with its axis
//!   along +Z and placed by its instances:
//!   - **Universal Joint Centre Block**: a 2.4 in cube (it sits in both flanges' slots, 2.6 wide);
//!   - **Graphite Phosphor Bronze Bushes**: a flanged bush, the Ø2.75 × 0.25 flange on the boss
//!     face (z 0) with four Ø.266 holes on the lug holes' Ø2.125 bolt circle at 45°, the Ø1.25
//!     sleeve 0.825 into the cross hole, a Ø.75 bore;
//!   - **Universal Joint Axle**: a Ø.75 pin from the block's face through the bush, standing 0.25
//!     out of it.
//! - **Universal Joint Assembly**: 2 flanges (the second turned over and a quarter turn, its cross
//!   hole across the first's at the same centre, z 3.75), the centre block, 4 bushes and 4 axles
//!   on the four boss faces, and **16 pan head machine screws 1/4-28 × 0.75** (standard content,
//!   ANSI inch, Stainless Steel), four through each bush's flange into its lug's holes, inserted
//!   on the bushes' hole edges with their Fastened mates (A19.5). The first flange is fixed; the
//!   other parts are placed (no mates of their own: the drawing only needs them in place).
//!
//! Every part has the **Part number** and **Description** of the course's BOM table
//! (`ex2-drawing.png`) as properties. The assembly's BOM starts with the default columns less
//! Name (Item, Quantity, Part number, Description): D12.2 adds Name and moves it left, which
//! gives the course's table.
//!
//! Decision (P3C.5): P3B.5's pan head machine screws had no 1/4-28 row; ASME B18.6.3 Table 17 gives
//! the fine thread the same head as 1/4-20 (A .492, H .144, J .075), so that row was added to
//! the library data rather than using the nearest size.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::appearance::Appearance;
use crate::assembly::bom::{BomColumn, SetBomSettings};
use crate::assembly::commands::{InsertInstance, SetInstancesFixed};
use crate::assembly::mate::MateId;
use crate::assembly::standard::{SetStandardProperties, StandardPart, StandardSpec, Stacking, plan_insert, site_of_edge};
use crate::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, AddExtrude, AddSketch, EditSketch, NewElementKind, RenameElement, RenamePart, SetExtrude, SetPartAppearance};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};

use super::gear_cover::{DocHistory, Studio};

/// mm per inch.
pub const IN: f64 = 25.4;

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0000 | n)
}

const fn inst(n: u128) -> InstanceId {
    InstanceId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_1000 | n)
}

pub const DOCUMENT_NAME: &str = "Universal Joint Assembly Drawing (stand-in)";
/// The flange's studio, the components' studio and the assembly.
pub const FLANGE_STUDIO: ElementId = ElementId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0101);
pub const COMPONENTS: ElementId = ElementId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0102);
pub const ASSEMBLY: ElementId = ElementId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0103);
pub const ASSEMBLY_NAME: &str = "Universal Joint Assembly";

const BLOCK_E: FeatureId = fid(0x12);
const BUSH_E: FeatureId = fid(0x22);
const SLEEVE_E: FeatureId = fid(0x24);
const AXLE_E: FeatureId = fid(0x32);

pub const BLOCK: PartId = PartId::new(BLOCK_E, 0);
pub const BUSH: PartId = PartId::new(BUSH_E, 0);
pub const AXLE: PartId = PartId::new(AXLE_E, 0);

/// The instances.
pub const FLANGES: [InstanceId; 2] = [inst(1), inst(2)];
pub const CENTRE_BLOCK: InstanceId = inst(3);
pub const BUSHES: [InstanceId; 4] = [inst(4), inst(5), inst(6), inst(7)];
pub const AXLES: [InstanceId; 4] = [inst(8), inst(9), inst(10), inst(11)];
/// The sixteen screws.
pub const fn screw(k: usize) -> InstanceId {
    inst(0x100 + k as u128)
}

const fn mate(k: usize) -> MateId {
    MateId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_2000 + k as u128)
}

/// The course's BOM (`ex2-drawing.png`): Name, Quantity, Part number, Description.
pub const COURSE_BOM: [(&str, u32, &str, &str); 5] = [
    ("Universal Joint Flange", 2, "MSB-0004", "Universal joint flange"),
    ("Universal Joint Centre Block", 1, "MSB-0001xC", "Universal Joint Center Block"),
    ("Graphite Phosphor Bronze Bushes", 4, "MSB-0002", "Bushes for main bearing"),
    ("Universal Joint Axle", 4, "MSB-0003", "Central Axle"),
    ("Pan head machine screw 1/4-28 x 0.75", 16, "STD-03923", "Pan head machine screw 1/4-28 x 0.75 Stainless Steel"),
];

/// Where the cross holes meet (z, in).
pub const CENTRE_Z: f64 = super::ujoint::CROSS_Z;
/// The block's side, the bush's flange and sleeve, the axle (in).
pub const BLOCK_SIDE: f64 = 2.4;
pub const BUSH_D: f64 = 2.75;
pub const BUSH_T: f64 = 0.25;
pub const SLEEVE_D: f64 = 1.25;
pub const SLEEVE_L: f64 = 0.825;
pub const BORE_D: f64 = 0.75;
pub const AXLE_L: f64 = super::ujoint::BOSS_X - BLOCK_SIDE / 2.0 + BUSH_T + AXLE_OUT;
/// How far the axle stands out of the bush (in).
pub const AXLE_OUT: f64 = 0.25;

/// The screws' configuration: ANSI inch pan head machine screw 1/4-28 × 0.75, Stainless Steel.
pub fn screw_spec() -> StandardSpec {
    let mut s = StandardSpec::new("ANSI inch", "Bolts & screws", "Machine screws", "Pan head machine screw").expect("in the library");
    s.size = "1/4-28".into();
    s.length = Some(0.75);
    s.material = "Stainless Steel".into();
    s.normalize();
    s
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: d / 2.0 * IN, construction: false }
}

/// A sketch on Top of `ops` and an extrude of the region at `seed` from z0 to z1 (in).
#[allow(clippy::too_many_arguments)]
fn feature(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, extrude: FeatureId, ops: Vec<SketchOp>, seed: (f64, f64), z: (f64, f64), op: BooleanOp, scope: &[PartId]) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: sketch, op: SketchOp::Batch(ops) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, &[v(seed.0, seed.1)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the stand-in is missing".into()));
    }
    let (z0, z1) = z;
    let fmt = |x: f64| format!("{} in", (x * 1000.0).round() / 1000.0);
    let mut x = super::extrude_of(regions, (z1 - z0) * IN);
    x.depth_expr = fmt(z1 - z0);
    x.op = op;
    x.merge_scope = scope.to_vec();
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs() * IN, expr: fmt(z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: extrude, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// The bush's four screw holes (in its own frame).
fn bush_holes() -> Vec<(f64, f64)> {
    let b = super::ujoint::BOLT_D / 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    vec![(b, b), (-b, b), (-b, -b), (b, -b)]
}

/// Adds the centre block, the bush and the axle to the Part Studio `el`.
pub fn build_components(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New};
    let h = BLOCK_SIDE / 2.0;
    let square = SketchOp::AddPolyline { points: vec![v(-h, -h), v(h, -h), v(h, h), v(-h, h)], closed: true, construction: false, label: "Add rectangle" };
    feature(s, el, fid(0x11), BLOCK_E, vec![square], (0.0, 0.0), (-h, h), New, &[])?;
    let mut flange = vec![circle(0.0, 0.0, BUSH_D), circle(0.0, 0.0, BORE_D)];
    flange.extend(bush_holes().into_iter().map(|(x, y)| circle(x, y, super::ujoint::HOLE_D)));
    feature(s, el, fid(0x21), BUSH_E, flange, (1.25, 0.0), (0.0, BUSH_T), New, &[])?;
    feature(s, el, fid(0x23), SLEEVE_E, vec![circle(0.0, 0.0, SLEEVE_D), circle(0.0, 0.0, BORE_D)], (0.5, 0.0), (-SLEEVE_L, 0.0), Add, &[BUSH])?;
    feature(s, el, fid(0x31), AXLE_E, vec![circle(0.0, 0.0, BORE_D)], (0.0, 0.0), (BUSH_T + AXLE_OUT - AXLE_L, BUSH_T + AXLE_OUT), New, &[])?;
    for (p, n) in [(BLOCK, COURSE_BOM[1].0), (BUSH, COURSE_BOM[2].0), (AXLE, COURSE_BOM[3].0)] {
        s.run(&RenamePart { element: el, part: p, name: n.into() })?;
    }
    for (parts, a) in [
        (vec![BLOCK], Appearance::rgb(178, 182, 188)),
        (vec![BUSH], Appearance::rgb(38, 72, 158)),
        (vec![AXLE], Appearance::rgb(150, 154, 160)),
    ] {
        s.run(&SetPartAppearance { element: el, parts, appearance: Some(a) })?;
    }
    Ok(())
}

/// The four boss faces' placements (a part's +Z out of the boss, its origin on the face): the
/// first flange's at ±X, the second's at ±Y.
pub fn boss_poses() -> [Pose; 4] {
    use std::f64::consts::FRAC_PI_2;
    let c = CENTRE_Z * IN;
    let x = super::ujoint::BOSS_X * IN;
    [
        Pose::rotation_about([0.0; 3], [0.0, 1.0, 0.0], FRAC_PI_2).then(&Pose::translation([x, 0.0, c])),
        Pose::rotation_about([0.0; 3], [0.0, 1.0, 0.0], -FRAC_PI_2).then(&Pose::translation([-x, 0.0, c])),
        Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], -FRAC_PI_2).then(&Pose::translation([0.0, x, c])),
        Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], FRAC_PI_2).then(&Pose::translation([0.0, -x, c])),
    ]
}

/// The second flange's placement: turned over (a half turn about Y), a quarter turn about Z and
/// lifted so its cross hole's centre meets the first's.
pub fn second_flange() -> Pose {
    use std::f64::consts::{FRAC_PI_2, PI};
    Pose::rotation_about([0.0; 3], [0.0, 1.0, 0.0], PI)
        .then(&Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], FRAC_PI_2))
        .then(&Pose::translation([0.0, 0.0, 2.0 * CENTRE_Z * IN]))
}

fn set_props(s: &mut dyn Studio, owner: PropertyOwner, number: &str, description: &str) -> Result<(), CommandError> {
    s.run(&SetProperties {
        owners: vec![owner],
        values: vec![
            (PropertyKey::PartNumber, PropertyValue::Text(number.into())),
            (PropertyKey::Description, PropertyValue::Text(description.into())),
        ],
        label: "Set properties".into(),
    })
}

/// Adds the assembly "Universal Joint Assembly" (its instances, the screws with their mates, the
/// BOM columns) to `doc`, whose studios are built.
pub fn build_assembly(doc: &mut Document, h: &mut History) -> Result<(), CommandError> {
    h.execute(doc, &AddElement { id: ASSEMBLY, kind: NewElementKind::Assembly, name: Some(ASSEMBLY_NAME.into()), after: None })?;
    let flange = InstanceSource::Part { element: FLANGE_STUDIO, part: super::ujoint::PART };
    let comp = |part| InstanceSource::Part { element: COMPONENTS, part };
    let mut add = |doc: &mut Document, id, source, pose| h.execute(doc, &InsertInstance { element: ASSEMBLY, instance: Instance::new(id, source, pose) });
    add(doc, FLANGES[0], flange, Pose::IDENTITY)?;
    add(doc, FLANGES[1], flange, second_flange())?;
    add(doc, CENTRE_BLOCK, comp(BLOCK), Pose::translation([0.0, 0.0, CENTRE_Z * IN]))?;
    let poses = boss_poses();
    for (i, p) in BUSHES.iter().zip(poses) {
        add(doc, *i, comp(BUSH), p)?;
    }
    for (i, p) in AXLES.iter().zip(poses) {
        add(doc, *i, comp(AXLE), p)?;
    }
    h.execute(doc, &SetInstancesFixed { element: ASSEMBLY, instances: vec![FLANGES[0]], fixed: true })?;
    // The screws: on each bush's four hole edges on its outer face (z = BUSH_T in its frame).
    let asm = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).cloned().ok_or(CommandError::ElementNotFound(ASSEMBLY))?;
    let mut builds: HashMap<ElementId, Arc<crate::rebuild::Build>> = HashMap::new();
    for o in assembly::structure::occurrences(doc, &asm) {
        if let std::collections::hash_map::Entry::Vacant(e) = builds.entry(o.element)
            && let Some(el) = doc.element(o.element)
        {
            e.insert(crate::rebuild::build(el.features()));
        }
    }
    let solids = assembly::occurrence_solids(doc, &asm, |e| builds.get(&e).cloned());
    let mut sites = Vec::new();
    for b in BUSHES {
        let solid = solids.get(&b).ok_or_else(|| CommandError::Invalid("no bush".into()))?;
        let mut here: Vec<_> = solid
            .edges
            .iter()
            .filter(|e| e.circle.is_some_and(|c| (2.0 * c.radius - super::ujoint::HOLE_D * IN).abs() < 1e-3 && (c.center[2] - BUSH_T * IN).abs() < 1e-3))
            .filter_map(|e| site_of_edge(solid, b, &e.name))
            .collect();
        here.sort_by(|a, b| {
            let c = |s: &crate::assembly::standard::HoleSite| solid.edge(&s.edge).and_then(|e| e.circle).map(|c| (c.center[0], c.center[1])).unwrap_or_default();
            c(a).partial_cmp(&c(b)).unwrap_or(std::cmp::Ordering::Equal)
        });
        if here.len() != 4 {
            return Err(CommandError::Invalid(format!("a bush has {} screw holes", here.len())));
        }
        sites.extend(here);
    }
    let part = StandardPart::new(&screw_spec())?;
    let mut cmd = plan_insert(doc, ASSEMBLY, part, &sites, false, Stacking::Plain, &solids)?;
    for (k, ins) in cmd.inserts.iter_mut().enumerate() {
        ins.instance = screw(k);
        ins.mate = mate(k);
    }
    let screw_el = cmd.part.element.id;
    h.execute(doc, &cmd)?;
    h.execute(doc, &SetStandardProperties { element: screw_el, part_number: COURSE_BOM[4].2.into(), description: COURSE_BOM[4].3.into() })?;
    // The BOM's columns before D12.2: the default ones less Name.
    let mut settings = doc.element(ASSEMBLY).and_then(|e| e.assembly_model()).map(|a| a.bom.clone()).unwrap_or_default();
    settings.columns = vec![
        BomColumn::Item,
        BomColumn::Quantity,
        BomColumn::Property(PropertyKey::PartNumber),
        BomColumn::Property(PropertyKey::Description),
    ];
    h.execute(doc, &SetBomSettings { element: ASSEMBLY, settings, label: "BOM columns".into() })?;
    // The title block's second line, like the course's "Made by Onshape".
    h.execute(
        doc,
        &SetProperties {
            owners: vec![PropertyOwner::Assembly { element: ASSEMBLY }],
            values: vec![(PropertyKey::Description, PropertyValue::Text("Made by cadrs".into()))],
            label: "Description".into(),
        },
    )?;
    Ok(())
}

/// A new document holding the stand-in (`fixtures/ujoint_assembly_standin.cadrs`).
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(DOCUMENT_NAME);
    doc.id = crate::ids::DocumentId::from_u128(0x0a1f_2a00_0000_0000_0000_0000_0000_0100);
    doc.units.length = cadrs_sketch::units::LengthUnit::Inch;
    let mut el = crate::document::Element::part_studio("Universal Joint");
    el.id = FLANGE_STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    super::ujoint::build(&mut doc, &mut h, FLANGE_STUDIO)?;
    {
        let mut s = DocHistory(&mut doc, &mut h);
        s.run(&RenamePart { element: FLANGE_STUDIO, part: super::ujoint::PART, name: COURSE_BOM[0].0.into() })?;
        s.run(&SetPartAppearance { element: FLANGE_STUDIO, parts: vec![super::ujoint::PART], appearance: Some(Appearance::rgb(232, 172, 36)) })?;
        set_props(&mut s, PropertyOwner::Part { element: FLANGE_STUDIO, part: super::ujoint::PART }, COURSE_BOM[0].2, COURSE_BOM[0].3)?;
        s.run(&AddElement { id: COMPONENTS, kind: NewElementKind::PartStudio, name: Some("Universal Joint Components".into()), after: Some(FLANGE_STUDIO) })?;
        build_components(&mut s, COMPONENTS)?;
        for (i, p) in [(1, BLOCK), (2, BUSH), (3, AXLE)] {
            set_props(&mut s, PropertyOwner::Part { element: COMPONENTS, part: p }, COURSE_BOM[i].2, COURSE_BOM[i].3)?;
        }
        s.run(&RenameElement { id: FLANGE_STUDIO, name: "Universal Joint".into() })?;
    }
    build_assembly(&mut doc, &mut h)?;
    Ok(doc)
}
