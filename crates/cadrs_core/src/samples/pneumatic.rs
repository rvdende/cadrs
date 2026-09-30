//! The "Pneumatic Cylinder" stand-in (P3B.2, `intro-to-assemblies.md` A15): Onshape's public
//! "Exercise: Pneumatic Cylinder" document can't be copied, so the exercise starts from these
//! parts, built from our own features in one Part Studio, "Cylinder parts", in inch and pound,
//! every part at its assembled position (axis Z; `intro-to-assemblies-gaps.md`, "Ex2 Pneumatic
//! Cylinder"). Sketches are on Top; heights come from starting offsets. Parts, in the order of the
//! course's parts list:
//!
//! | Part | Shape | Material |
//! |---|---|---|
//! | Barrel | tube Ø2.0/Ø1.75, z 0.75–5.75 | Polycarbonate |
//! | Top Cap | box 2.5 × 2.5, z 5.75–6.5, with a Ø0.5 hole, 4× Ø0.375 at (±0.95, ±0.95) and 6× Ø0.266 on r 0.55; spigot Ø1.5 (Ø0.5 bore) z 5.25–5.75 | Al 6061 |
//! | Structural Rod | Ø0.375 at (0.95, 0.95), z −0.5–7.0 | Steel |
//! | Piston & Rod | Ø1.5 z 1.25–2.0 + rod Ø0.5 z 2.0–8.0 | Steel |
//! | Retaining Plate | disc Ø1.5, z 6.5–6.625, with Ø0.5 and 6× Ø0.266 holes | Al 6061 |
//! | O-Ring 0.125 | ring Ø1.5/Ø1.75, z 0.75–0.875 | Nitrile (1.00 g/cm³) |
//! | O-Ring 0.185 | ring Ø1.5/Ø1.75, z 1.25–1.435 | Nitrile |
//! | Rear Cap | box 2.5 × 2.5, z 0–0.75, 4× Ø0.375 holes; spigot Ø1.5 z 0.75–1.25; recess Ø1.0 z 0–0.1 | Al 6061 |
//! | Rear Cap mount | Ø1.0, z −0.4–0.1 (fills the recess) | Al 6061 |
//!
//! Every id is fixed, so `fixtures/pneumatic_cylinder_standin.cadrs` is regenerated exactly
//! (`cadrs_core/tests/course_assemblies.rs`).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::appearance::Appearance;
use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenamePart, SetExtrude, SetPartAppearance, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::material::Material;

use super::gear_cover::{DocHistory, Studio};

/// mm per inch.
pub const IN: f64 = 25.4;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_0000 + n)
}

/// The Part Studio "Cylinder parts".
pub const STUDIO: ElementId = ElementId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_0101);

pub const BARREL_E: FeatureId = id(0x12);
pub const TOP_CAP_E: FeatureId = id(0x22);
pub const TOP_SPIGOT_E: FeatureId = id(0x24);
pub const ROD_E: FeatureId = id(0x32);
pub const PISTON_E: FeatureId = id(0x42);
pub const PISTON_ROD_E: FeatureId = id(0x44);
pub const PLATE_E: FeatureId = id(0x52);
pub const ORING_125_E: FeatureId = id(0x62);
pub const ORING_185_E: FeatureId = id(0x72);
pub const REAR_CAP_E: FeatureId = id(0x82);
pub const REAR_SPIGOT_E: FeatureId = id(0x84);
pub const RECESS_E: FeatureId = id(0x86);
pub const MOUNT_E: FeatureId = id(0x92);

pub const BARREL: PartId = PartId::new(BARREL_E, 0);
pub const TOP_CAP: PartId = PartId::new(TOP_CAP_E, 0);
pub const STRUCTURAL_ROD: PartId = PartId::new(ROD_E, 0);
pub const PISTON_ROD: PartId = PartId::new(PISTON_E, 0);
pub const RETAINING_PLATE: PartId = PartId::new(PLATE_E, 0);
pub const ORING_125: PartId = PartId::new(ORING_125_E, 0);
pub const ORING_185: PartId = PartId::new(ORING_185_E, 0);
pub const REAR_CAP: PartId = PartId::new(REAR_CAP_E, 0);
pub const REAR_CAP_MOUNT: PartId = PartId::new(MOUNT_E, 0);

/// The parts with their names, in the parts list's order.
pub const PARTS: [(PartId, &str); 9] = [
    (BARREL, "Barrel"),
    (TOP_CAP, "Top Cap"),
    (STRUCTURAL_ROD, "Structural Rod"),
    (PISTON_ROD, "Piston & Rod"),
    (RETAINING_PLATE, "Retaining Plate"),
    (ORING_125, "O-Ring 0.125"),
    (ORING_185, "O-Ring 0.185"),
    (REAR_CAP, "Rear Cap"),
    (REAR_CAP_MOUNT, "Rear Cap mount"),
];

/// The flange holes (the rods' positions), in.
pub const ROD_XY: f64 = 0.95;
/// Where the retaining-plate screw holes are (radius, in).
pub const SCREW_R: f64 = 0.55;
/// The slider's travel: the piston's top (2.0) up to the Top Cap's spigot (5.25), in.
pub const TRAVEL: f64 = 3.25;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x * IN, y * IN)
}

fn circle(x: f64, y: f64, d: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: d / 2.0 * IN, construction: false }
}

fn square(half: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![v(-half, -half), v(half, -half), v(half, half), v(-half, half)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

/// The four flange holes (Ø0.375 at (±0.95, ±0.95)).
fn flange_holes() -> Vec<SketchOp> {
    [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].map(|(sx, sy)| circle(sx * ROD_XY, sy * ROD_XY, 0.375)).to_vec()
}

/// The six Ø0.266 screw holes on r 0.55.
fn screw_holes() -> Vec<SketchOp> {
    (0..6)
        .map(|k| {
            let a = k as f64 * std::f64::consts::PI / 3.0;
            circle(SCREW_R * a.cos(), SCREW_R * a.sin(), 0.266)
        })
        .collect()
}

struct Ex<'a> {
    sketch: FeatureId,
    feature: FeatureId,
    ops: Vec<SketchOp>,
    /// A point (in) inside the region to extrude.
    seed: (f64, f64),
    /// z from and to (in).
    z: (f64, f64),
    op: BooleanOp,
    scope: &'a [PartId],
}

fn feature(s: &mut dyn Studio, el: ElementId, e: Ex) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: e.sketch, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: e.sketch, op: SketchOp::Batch(e.ops) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(e.sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(e.sketch, &g, &[v(e.seed.0, e.seed.1)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the stand-in is missing".into()));
    }
    let (z0, z1) = e.z;
    let fmt = |x: f64| format!("{} in", (x * 1000.0).round() / 1000.0);
    let mut x = super::extrude_of(regions, (z1 - z0) * IN);
    x.depth_expr = fmt(z1 - z0);
    x.op = e.op;
    x.merge_scope = e.scope.to_vec();
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs() * IN, expr: fmt(z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: e.feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: e.feature, extrude: x, label: "Extrude".into() })?;
    Ok(())
}

/// Adds the nine parts, their names, materials and looks to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    let ring = |d: f64| vec![circle(0.0, 0.0, 1.75), circle(0.0, 0.0, d)];
    let mut top = vec![square(1.25), circle(0.0, 0.0, 0.5)];
    top.extend(flange_holes());
    top.extend(screw_holes());
    let mut plate = vec![circle(0.0, 0.0, 1.5), circle(0.0, 0.0, 0.5)];
    plate.extend(screw_holes());
    let mut rear = vec![square(1.25)];
    rear.extend(flange_holes());
    let list = [
        Ex { sketch: id(0x11), feature: BARREL_E, ops: vec![circle(0.0, 0.0, 2.0), circle(0.0, 0.0, 1.75)], seed: (0.93, 0.0), z: (0.75, 5.75), op: New, scope: &[] },
        Ex { sketch: id(0x21), feature: TOP_CAP_E, ops: top, seed: (0.0, 0.9), z: (5.75, 6.5), op: New, scope: &[] },
        Ex { sketch: id(0x23), feature: TOP_SPIGOT_E, ops: vec![circle(0.0, 0.0, 1.5), circle(0.0, 0.0, 0.5)], seed: (0.5, 0.0), z: (5.25, 5.75), op: Add, scope: &[TOP_CAP] },
        Ex { sketch: id(0x31), feature: ROD_E, ops: vec![circle(ROD_XY, ROD_XY, 0.375)], seed: (ROD_XY, ROD_XY), z: (-0.5, 7.0), op: New, scope: &[] },
        Ex { sketch: id(0x41), feature: PISTON_E, ops: vec![circle(0.0, 0.0, 1.5)], seed: (0.0, 0.0), z: (1.25, 2.0), op: New, scope: &[] },
        Ex { sketch: id(0x43), feature: PISTON_ROD_E, ops: vec![circle(0.0, 0.0, 0.5)], seed: (0.0, 0.0), z: (2.0, 8.0), op: Add, scope: &[PISTON_ROD] },
        Ex { sketch: id(0x51), feature: PLATE_E, ops: plate, seed: (0.4, 0.2), z: (6.5, 6.625), op: New, scope: &[] },
        Ex { sketch: id(0x61), feature: ORING_125_E, ops: ring(1.5), seed: (0.8, 0.0), z: (0.75, 0.875), op: New, scope: &[] },
        Ex { sketch: id(0x71), feature: ORING_185_E, ops: ring(1.5), seed: (0.8, 0.0), z: (1.25, 1.435), op: New, scope: &[] },
        Ex { sketch: id(0x81), feature: REAR_CAP_E, ops: rear, seed: (0.0, 0.0), z: (0.0, 0.75), op: New, scope: &[] },
        Ex { sketch: id(0x83), feature: REAR_SPIGOT_E, ops: vec![circle(0.0, 0.0, 1.5)], seed: (0.0, 0.0), z: (0.75, 1.25), op: Add, scope: &[REAR_CAP] },
        Ex { sketch: id(0x85), feature: RECESS_E, ops: vec![circle(0.0, 0.0, 1.0)], seed: (0.0, 0.0), z: (0.0, 0.1), op: Remove, scope: &[REAR_CAP] },
        Ex { sketch: id(0x91), feature: MOUNT_E, ops: vec![circle(0.0, 0.0, 1.0)], seed: (0.0, 0.0), z: (-0.4, 0.1), op: New, scope: &[] },
    ];
    for e in list {
        feature(s, el, e)?;
    }
    for (part, name) in PARTS {
        s.run(&RenamePart { element: el, part, name: name.into() })?;
    }
    let al = crate::material::library("Aluminum - 6061");
    let steel = crate::material::library("Steel");
    let pc = crate::material::library("Polycarbonate");
    let nbr = Some(Material::custom("Nitrile Rubber", 1000.0));
    for (parts, m) in [
        (vec![TOP_CAP, RETAINING_PLATE, REAR_CAP, REAR_CAP_MOUNT], al),
        (vec![STRUCTURAL_ROD, PISTON_ROD], steel),
        (vec![BARREL], pc),
        (vec![ORING_125, ORING_185], nbr),
    ] {
        s.run(&SetPartMaterial { element: el, parts, material: m })?;
    }
    // Looks: the clear barrel, dark steel, black rubber.
    for (parts, a) in [
        (vec![BARREL], Appearance::rgb(196, 222, 206).with_alpha(110)),
        (vec![STRUCTURAL_ROD], Appearance::rgb(58, 60, 64)),
        (vec![PISTON_ROD], Appearance::rgb(64, 84, 82)),
        (vec![ORING_125, ORING_185], Appearance::rgb(44, 44, 46)),
    ] {
        s.run(&SetPartAppearance { element: el, parts, appearance: Some(a) })?;
    }
    Ok(())
}

/// A new document holding the stand-in: "Exercise: Pneumatic Cylinder (stand-in)" with its Part
/// Studio "Cylinder parts" (inch, pound), as `fixtures/pneumatic_cylinder_standin.cadrs` stores
/// it.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty("Exercise: Pneumatic Cylinder (stand-in)");
    doc.id = crate::ids::DocumentId::from_u128(0x3b02_0000_0000_0000_0000_0000_0000_0100);
    doc.units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Inch,
        mass: cadrs_sketch::units::MassUnit::Pound,
        ..Default::default()
    };
    let mut el = crate::document::Element::part_studio("Cylinder parts");
    el.id = STUDIO;
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), STUDIO)?;
    Ok(doc)
}

/// The stand-in as a document file (`fixtures/pneumatic_cylinder_standin.cadrs`).
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
