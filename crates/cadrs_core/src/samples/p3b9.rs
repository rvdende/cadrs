//! The P3B.9 stand-ins (`intro-to-assemblies.md` A1.4, X15, X16): the course only names
//! relations, interference, Replace instances and Edit in context, so each is shown on a small
//! document of our own, in mm unless stated. Every id is fixed, so the fixtures are regenerated
//! exactly (`cadrs_core/tests/assembly_p3b9.rs`).
//!
//! - [`relations_document`] (`fixtures/relations_standin.cadrs`): Part Studio **Mechanisms**
//!   with every part where the assembly has it, on a fixed **Base** plate 300 × 200 × 10
//!   (z −10…0): a 12-tooth **Gear 12T** (pitch radius 20) and a 24-tooth **Gear 24T** (pitch
//!   radius 40) meshing on Revolutes 1 and 2; a 10-tooth **Pinion** (pitch radius 15) on Revolute
//!   3 over a **Rack** on Slider 1 (along X); a fixed Ø12 **Screw** with a hex **Nut** on
//!   Cylindrical 1; two slides, **Slide A** and **Slide B**, on Sliders 2 and 3 (along X; Slide
//!   A limited to ±25 mm). No relations yet: `course_asm_relations` adds them.
//! - [`interference_document`] (`fixtures/interference_standin.cadrs`): two 50 × 30 × 20 Blocks,
//!   the second at (40, 10, 5), so they share 10 × 20 × 15 = 3000 mm³; a 60 × 40 × 10 Plate with
//!   a Ø8 hole and a Ø10 Pin through it, sharing π(5² − 4²)·10 = 90π ≈ 282.743 mm³.
//! - [`context_document`] (`fixtures/context_standin.cadrs`): a **Base** plate 140 × 90 × 10
//!   (z −10…0) with two Ø10 holes at x ±40, and a **Cover** plate 120 × 80 × 6 on top (fixed; the
//!   Base's top face shows round it),
//!   whose holes `course_asm_edit_in_context` makes in context from the Base's.
//! - [`replace_document`] (`fixtures/flange_replace.cadrs`): the Replicate flange stand-in
//!   ([`super::flange`], inch) with two more Part Studios: **Long Screw** (the screw with a
//!   1.5 in shank: its rim under the head matches, so Fastened 1 is kept) and **Dowel** (a plain
//!   Ø0.3 in rod: nothing matches the Ø0.25 rim, so Fastened 1 goes).

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::assembly::commands::{AddMateFeature, InsertInstance, SetInstancesFixed};
use crate::assembly::connector::{ConnectorFrame, MateConnector};
use crate::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateLimits, MateType};
use crate::assembly::{Instance, InstanceId, InstanceSource, Pose};
use crate::command::{CommandError, History};
use crate::commands::{AddElement, AddExtrude, AddSketch, EditSketch, NewElementKind, RenamePart, SetExtrude, SetPartMaterial};
use crate::document::{BooleanOp, Document, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};

use super::gear_cover::{DocHistory, Studio};

const fn fid(n: u128) -> FeatureId {
    FeatureId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_0000 + n)
}

const fn eid(n: u128) -> ElementId {
    ElementId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_0100 + n)
}

const fn iid(n: u128) -> InstanceId {
    InstanceId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_1000 + n)
}

const fn mid(n: u128) -> MateId {
    MateId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_2000 + n)
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn poly(points: Vec<Vec2>) -> SketchOp {
    SketchOp::AddPolyline { points, closed: true, construction: false, label: "Add polygon" }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    poly(vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)])
}

fn circle(x: f64, y: f64, r: f64) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: r, construction: false }
}

/// A sketch on Top of `ops` and its extrude from `z0` to `z1` (mm), New: one part, named.
#[allow(clippy::too_many_arguments)]
fn part(s: &mut dyn Studio, el: ElementId, n: u128, ops: Vec<SketchOp>, seed: Vec2, z: (f64, f64), name: &str, material: &str) -> Result<PartId, CommandError> {
    let (sk, ex) = (fid(n), fid(n + 1));
    s.run(&AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: sk, op: SketchOp::Batch(ops) })?;
    let g = s
        .document()
        .element(el)
        .and_then(|x| x.feature(sk))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(format!("no region for {name}")));
    }
    let (z0, z1) = z;
    let mut x = super::extrude_of(regions, z1 - z0);
    x.op = BooleanOp::New;
    if z0 != 0.0 {
        x.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", z0.abs()), flip: z0 < 0.0 });
    }
    s.run(&AddExtrude { element: el, feature: ex, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: ex, extrude: x, label: "Extrude".into() })?;
    let p = PartId::new(ex, 0);
    s.run(&RenamePart { element: el, part: p, name: name.into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![p], material: crate::material::library(material) })?;
    Ok(p)
}

/// A spur gear outline: `n` teeth on the pitch radius `rp` about (`cx`, `cy`), the first tooth
/// at `phase` (radians) from +X; trapezoid teeth, addendum m, dedendum 1.25 m (m = 2 rp / n).
pub fn gear_outline(cx: f64, cy: f64, n: usize, rp: f64, phase: f64) -> Vec<Vec2> {
    let m = 2.0 * rp / n as f64;
    let (rt, rr) = (rp + m, rp - 1.25 * m);
    let p = std::f64::consts::TAU / n as f64;
    let at = |r: f64, a: f64| v(cx + r * a.cos(), cy + r * a.sin());
    let mut out = Vec::new();
    for k in 0..n {
        let a = phase + k as f64 * p;
        out.push(at(rr, a - 0.27 * p));
        out.push(at(rt, a - 0.12 * p));
        out.push(at(rt, a + 0.12 * p));
        out.push(at(rr, a + 0.27 * p));
    }
    out
}

/// A rack outline along X from `x0` to `x1`: teeth of pitch `pitch` centred at `xc` + k·pitch,
/// pitch line `y`, module `m`, a 10 mm back.
pub fn rack_outline(x0: f64, x1: f64, xc: f64, y: f64, pitch: f64, m: f64) -> Vec<Vec2> {
    let (yt, yr) = (y + m, y - 1.25 * m);
    let yb = yr - 10.0;
    let mut out = vec![v(x0, yb), v(x1, yb), v(x1, yr)];
    let kmax = ((x1 - xc) / pitch).floor() as i64;
    let kmin = ((x0 - xc) / pitch).ceil() as i64;
    for k in (kmin..=kmax).rev() {
        let c = xc + k as f64 * pitch;
        if c + 0.27 * pitch > x1 || c - 0.27 * pitch < x0 {
            continue;
        }
        out.push(v(c + 0.27 * pitch, yr));
        out.push(v(c + 0.12 * pitch, yt));
        out.push(v(c - 0.12 * pitch, yt));
        out.push(v(c - 0.27 * pitch, yr));
    }
    out.push(v(x0, yr));
    out
}

fn hexagon(cx: f64, cy: f64, across_flats: f64) -> Vec<Vec2> {
    let r = across_flats / 3f64.sqrt();
    (0..6).map(|k| {
        let a = (k as f64 * 60.0).to_radians();
        v(cx + r * a.cos(), cy + r * a.sin())
    }).collect()
}

fn file_doc(name: &str, n: u128, units_inch: bool) -> Document {
    let mut doc = Document::empty(name);
    doc.id = crate::ids::DocumentId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_0000 + n);
    if units_inch {
        doc.units = cadrs_sketch::units::Units { length: cadrs_sketch::units::LengthUnit::Inch, mass: cadrs_sketch::units::MassUnit::Pound, ..Default::default() };
    }
    doc
}

fn studio(doc: &mut Document, el: ElementId, name: &str) {
    let mut e = crate::document::Element::part_studio(name);
    e.id = el;
    doc.elements.push(e);
}

fn insert(doc: &mut Document, h: &mut History, asm: ElementId, i: InstanceId, el: ElementId, p: PartId, pose: Pose) -> Result<(), CommandError> {
    h.execute(doc, &InsertInstance { element: asm, instance: Instance::new(i, InstanceSource::Part { element: el, part: p }, pose) })
}

/// A mate of `t` between `a` and `b` at the same frame (`at`, Z along `z`, X along `x`) in both
/// parts' coordinates (every part is where its studio has it).
#[allow(clippy::too_many_arguments)]
fn mate_at(n: u128, name: &str, t: MateType, a: InstanceId, b: InstanceId, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> MateFeature {
    let f = ConnectorFrame::new(at, z, x);
    MateFeature::new(mid(n), name, MateKind::Mate(Mate::new(t, MateConnector::at(a, f), MateConnector::at(b, f))))
}

fn assembly_first(doc: &mut Document, asm: ElementId) {
    if let Some(k) = doc.elements.iter().position(|e| e.id == asm) {
        let a = doc.elements.remove(k);
        doc.elements.insert(0, a);
    }
}

// ---------------------------------------------------------------------------------------------
// Relations

pub const MECHANISMS: ElementId = eid(1);
pub const RELATIONS_ASSEMBLY: ElementId = eid(2);

pub const BASE: InstanceId = iid(1);
pub const GEAR_12: InstanceId = iid(2);
pub const GEAR_24: InstanceId = iid(3);
pub const PINION: InstanceId = iid(4);
pub const RACK: InstanceId = iid(5);
pub const SCREW: InstanceId = iid(6);
pub const NUT: InstanceId = iid(7);
pub const SLIDE_A: InstanceId = iid(8);
pub const SLIDE_B: InstanceId = iid(9);

pub const REVOLUTE_1: MateId = mid(1);
pub const REVOLUTE_2: MateId = mid(2);
pub const REVOLUTE_3: MateId = mid(3);
pub const SLIDER_1: MateId = mid(4);
pub const CYLINDRICAL_1: MateId = mid(5);
pub const SLIDER_2: MateId = mid(6);
pub const SLIDER_3: MateId = mid(7);

/// The gears' centres and pitch radii, the pinion's, the screw's axis.
pub const GEAR_12_AT: [f64; 2] = [-100.0, 40.0];
pub const GEAR_24_AT: [f64; 2] = [-40.0, 40.0];
pub const PINION_AT: [f64; 2] = [60.0, 60.0];
pub const PINION_R: f64 = 15.0;
pub const SCREW_AT: [f64; 2] = [110.0, -40.0];
/// The screw's pitch the scenario gives the Screw relation (mm).
pub const PITCH: f64 = 5.0;

/// "Mechanisms (stand-in)": see the module doc.
pub fn relations_document() -> Result<Document, CommandError> {
    let mut doc = file_doc("Relations (stand-in)", 1, false);
    let mut h = History::default();
    studio(&mut doc, MECHANISMS, "Mechanisms");
    let parts = {
        let s = &mut DocHistory(&mut doc, &mut h);
        let el = MECHANISMS;
        let steel = "Steel";
        let base = part(s, el, 0x10, vec![rect(-150.0, -110.0, 150.0, 90.0)], v(0.0, 0.0), (-10.0, 0.0), "Base", "Aluminum - 6061")?;
        let [gx, gy] = GEAR_12_AT;
        // Each gear and the pinion has a marker hole off its axis, so its turn shows.
        let g12 = part(s, el, 0x20, vec![poly(gear_outline(gx, gy, 12, 20.0, 0.0)), circle(gx, gy, 4.0), circle(gx, gy + 11.0, 2.5)], v(gx + 12.0, gy), (0.0, 8.0), "Gear 12T", steel)?;
        let [gx, gy] = GEAR_24_AT;
        let g24 = part(s, el, 0x30, vec![poly(gear_outline(gx, gy, 24, 40.0, std::f64::consts::PI / 24.0)), circle(gx, gy, 4.0), circle(gx, gy + 26.0, 4.0)], v(gx + 20.0, gy), (0.0, 8.0), "Gear 24T", steel)?;
        let [px, py] = PINION_AT;
        let pin = part(s, el, 0x40, vec![poly(gear_outline(px, py, 10, PINION_R, 0.0)), circle(px, py, 3.0), circle(px, py + 7.5, 1.8)], v(px + 7.0, py), (0.0, 10.0), "Pinion", steel)?;
        let pitch = std::f64::consts::TAU * PINION_R / 10.0;
        let rack = part(s, el, 0x50, vec![poly(rack_outline(12.0, 140.0, px, py - PINION_R, pitch, 3.0))], v(60.0, py - PINION_R - 10.0), (0.0, 10.0), "Rack", steel)?;
        let [sx, sy] = SCREW_AT;
        let screw = part(s, el, 0x60, vec![circle(sx, sy, 6.0)], v(sx, sy), (0.0, 90.0), "Screw", steel)?;
        let nut = part(s, el, 0x70, vec![poly(hexagon(sx, sy, 22.0)), circle(sx, sy, 6.0)], v(sx + 9.0, sy), (20.0, 32.0), "Nut", "Brass")?;
        let a = part(s, el, 0x80, vec![rect(-130.0, -60.0, -90.0, -40.0)], v(-110.0, -50.0), (0.0, 15.0), "Slide A", "Aluminum - 6061")?;
        let b = part(s, el, 0x90, vec![rect(-130.0, -95.0, -90.0, -75.0)], v(-110.0, -85.0), (0.0, 15.0), "Slide B", "Aluminum - 6061")?;
        [base, g12, g24, pin, rack, screw, nut, a, b]
    };
    let asm = RELATIONS_ASSEMBLY;
    h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("Mechanisms Assembly".into()), after: None })?;
    let ids = [BASE, GEAR_12, GEAR_24, PINION, RACK, SCREW, NUT, SLIDE_A, SLIDE_B];
    for (i, p) in ids.iter().zip(parts) {
        insert(&mut doc, &mut h, asm, *i, MECHANISMS, p, Pose::IDENTITY)?;
    }
    h.execute(&mut doc, &SetInstancesFixed { element: asm, instances: vec![BASE, SCREW], fixed: true })?;
    let (up, x) = ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let along_x = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let mut mates = vec![
        mate_at(1, "Revolute 1", MateType::Revolute, GEAR_12, BASE, [GEAR_12_AT[0], GEAR_12_AT[1], 0.0], up, x),
        mate_at(2, "Revolute 2", MateType::Revolute, GEAR_24, BASE, [GEAR_24_AT[0], GEAR_24_AT[1], 0.0], up, x),
        mate_at(3, "Revolute 3", MateType::Revolute, PINION, BASE, [PINION_AT[0], PINION_AT[1], 0.0], up, x),
        mate_at(4, "Slider 1", MateType::Slider, RACK, BASE, [PINION_AT[0], PINION_AT[1] - PINION_R, 0.0], along_x.0, along_x.1),
        mate_at(5, "Cylindrical 1", MateType::Cylindrical, NUT, SCREW, [SCREW_AT[0], SCREW_AT[1], 20.0], up, x),
        mate_at(6, "Slider 2", MateType::Slider, SLIDE_A, BASE, [-110.0, -50.0, 0.0], along_x.0, along_x.1),
        mate_at(7, "Slider 3", MateType::Slider, SLIDE_B, BASE, [-110.0, -85.0, 0.0], along_x.0, along_x.1),
    ];
    if let MateKind::Mate(m) = &mut mates[5].kind {
        m.limits = Some(MateLimits { z: Some((-25.0, 25.0)), ..Default::default() });
    }
    for f in mates {
        h.execute(&mut doc, &AddMateFeature { element: asm, feature: f, poses: Vec::new() })?;
    }
    assembly_first(&mut doc, asm);
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Interference

pub const BLOCKS: ElementId = eid(11);
pub const PLATE_STUDIO: ElementId = eid(12);
pub const PIN_STUDIO: ElementId = eid(13);
pub const INTERFERENCE_ASSEMBLY: ElementId = eid(14);
pub const BLOCK_1: InstanceId = iid(11);
pub const BLOCK_2: InstanceId = iid(12);
pub const PLATE: InstanceId = iid(13);
pub const PIN: InstanceId = iid(14);
/// Where the second block is: it shares 10 × 20 × 15 mm with the first.
pub const BLOCK_2_AT: [f64; 3] = [40.0, 10.0, 5.0];
/// The blocks' and the plate's and pin's shared volumes (mm³).
pub const BLOCKS_OVERLAP: f64 = 3000.0;
pub const PIN_OVERLAP: f64 = 90.0 * std::f64::consts::PI;

/// "Interference (stand-in)": see the module doc.
pub fn interference_document() -> Result<Document, CommandError> {
    let mut doc = file_doc("Interference (stand-in)", 2, false);
    let mut h = History::default();
    studio(&mut doc, BLOCKS, "Block");
    studio(&mut doc, PLATE_STUDIO, "Plate");
    studio(&mut doc, PIN_STUDIO, "Pin");
    let (block, plate, pin) = {
        let s = &mut DocHistory(&mut doc, &mut h);
        let block = part(s, BLOCKS, 0x110, vec![rect(0.0, 0.0, 50.0, 30.0)], v(25.0, 15.0), (0.0, 20.0), "Block", "Aluminum - 6061")?;
        let plate = part(s, PLATE_STUDIO, 0x120, vec![rect(0.0, 0.0, 60.0, 40.0), circle(30.0, 20.0, 4.0)], v(5.0, 5.0), (0.0, 10.0), "Plate", "Aluminum - 6061")?;
        let pin = part(s, PIN_STUDIO, 0x130, vec![circle(0.0, 0.0, 5.0)], v(0.0, 0.0), (0.0, 25.0), "Pin", "Steel")?;
        (block, plate, pin)
    };
    let asm = INTERFERENCE_ASSEMBLY;
    h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("Interference Assembly".into()), after: None })?;
    insert(&mut doc, &mut h, asm, BLOCK_1, BLOCKS, block, Pose::IDENTITY)?;
    insert(&mut doc, &mut h, asm, BLOCK_2, BLOCKS, block, Pose::translation(BLOCK_2_AT))?;
    insert(&mut doc, &mut h, asm, PLATE, PLATE_STUDIO, plate, Pose::translation([100.0, 0.0, 0.0]))?;
    insert(&mut doc, &mut h, asm, PIN, PIN_STUDIO, pin, Pose::translation([130.0, 20.0, -8.0]))?;
    h.execute(&mut doc, &SetInstancesFixed { element: asm, instances: vec![BLOCK_1, PLATE], fixed: true })?;
    assembly_first(&mut doc, asm);
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Edit in context

pub const CONTEXT_BASE_STUDIO: ElementId = eid(21);
pub const COVER_STUDIO: ElementId = eid(22);
pub const CONTEXT_ASSEMBLY: ElementId = eid(23);
pub const CONTEXT_BASE: InstanceId = iid(21);
pub const COVER: InstanceId = iid(22);
pub const COVER_PART: PartId = PartId::new(fid(0x221), 0);
pub const CONTEXT_BASE_PART: PartId = PartId::new(fid(0x211), 0);
/// The Base's holes (x, mm) and their radius.
pub const HOLES_X: [f64; 2] = [-40.0, 40.0];
pub const HOLE_R: f64 = 5.0;

/// "Edit in context (stand-in)": see the module doc.
pub fn context_document() -> Result<Document, CommandError> {
    let mut doc = file_doc("Edit in context (stand-in)", 3, false);
    let mut h = History::default();
    studio(&mut doc, CONTEXT_BASE_STUDIO, "Base");
    studio(&mut doc, COVER_STUDIO, "Cover");
    let (base, cover) = {
        let s = &mut DocHistory(&mut doc, &mut h);
        let base = part(
            s,
            CONTEXT_BASE_STUDIO,
            0x210,
            vec![rect(-70.0, -45.0, 70.0, 45.0), circle(HOLES_X[0], 0.0, HOLE_R), circle(HOLES_X[1], 0.0, HOLE_R)],
            v(0.0, 20.0),
            (-10.0, 0.0),
            "Base",
            "Aluminum - 6061",
        )?;
        let cover = part(s, COVER_STUDIO, 0x220, vec![rect(-60.0, -40.0, 60.0, 40.0)], v(0.0, 0.0), (0.0, 6.0), "Cover", "Aluminum - 6061")?;
        (base, cover)
    };
    let asm = CONTEXT_ASSEMBLY;
    h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("Assembly 1".into()), after: None })?;
    insert(&mut doc, &mut h, asm, CONTEXT_BASE, CONTEXT_BASE_STUDIO, base, Pose::IDENTITY)?;
    insert(&mut doc, &mut h, asm, COVER, COVER_STUDIO, cover, Pose::IDENTITY)?;
    h.execute(&mut doc, &SetInstancesFixed { element: asm, instances: vec![COVER], fixed: true })?;
    assembly_first(&mut doc, asm);
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Replace instances

pub const LONG_SCREW_STUDIO: ElementId = eid(31);
pub const DOWEL_STUDIO: ElementId = eid(32);
pub const LONG_SCREW_PART: PartId = PartId::new(fid(0x312), 0);
pub const DOWEL_PART: PartId = PartId::new(fid(0x322), 0);

/// The flange stand-in with the Long Screw and Dowel studios (inch): see the module doc.
pub fn replace_document() -> Result<Document, CommandError> {
    use super::step_stool::{circle as circle_in, extrude as extrude_in, sketch as sketch_in, v as v_in};
    let mut doc = super::flange::document()?;
    doc.name = "Replace instances (stand-in)".into();
    doc.id = crate::ids::DocumentId::from_u128(0x3b09_0000_0000_0000_0000_0000_0000_0004);
    let mut h = History::default();
    studio(&mut doc, LONG_SCREW_STUDIO, "Long Screw");
    studio(&mut doc, DOWEL_STUDIO, "Dowel");
    let s = &mut DocHistory(&mut doc, &mut h);
    let el = LONG_SCREW_STUDIO;
    let g = sketch_in(s, el, fid(0x311), PlaneRef::Top, vec![circle_in(0.0, 0.0, super::flange::HOLE_D)])?;
    extrude_in(s, el, fid(0x311), &g, fid(0x312), &[v_in(0.0, 0.0)], (-1.5, 0.0), BooleanOp::New, &[], false)?;
    let g = sketch_in(s, el, fid(0x313), PlaneRef::Top, vec![circle_in(0.0, 0.0, 0.4)])?;
    extrude_in(s, el, fid(0x313), &g, fid(0x314), &[v_in(0.0, 0.0)], (0.0, 0.2), BooleanOp::Add, &[LONG_SCREW_PART], false)?;
    s.run(&RenamePart { element: el, part: LONG_SCREW_PART, name: "Long Screw".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![LONG_SCREW_PART], material: crate::material::library("Steel") })?;
    let el = DOWEL_STUDIO;
    let g = sketch_in(s, el, fid(0x321), PlaneRef::Top, vec![circle_in(0.0, 0.0, 0.3)])?;
    extrude_in(s, el, fid(0x321), &g, fid(0x322), &[v_in(0.0, 0.0)], (-1.0, 0.0), BooleanOp::New, &[], false)?;
    s.run(&RenamePart { element: el, part: DOWEL_PART, name: "Dowel".into() })?;
    s.run(&SetPartMaterial { element: el, parts: vec![DOWEL_PART], material: crate::material::library("Steel") })?;
    Ok(doc)
}

/// Any of the stand-ins as a document file.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
