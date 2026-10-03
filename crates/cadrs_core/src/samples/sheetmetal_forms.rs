//! cadrs's own **sheet metal forms library** (P3I.9, SM20): "cadrs sheet metal forms", one Part
//! Studio per form, each built from ordinary features as a user would build a form:
//!
//! - Variable features `thickness` (driven by the sheet metal model when the form is placed) and
//!   the form's sizes (Length, Width, Height, Diameter) and angles (Angle);
//! - **Form profile**: a construction-only sketch on Top, the outline the flat pattern shows;
//! - **Add** and **Remove**: the parts it adds to and removes from the sheet (extrudes, revolves
//!   and a Boolean);
//! - **Form origin**: a mate connector at the origin (Z out of the sheet; the sheet lies below,
//!   `z` from `−thickness` to 0);
//! - **Tag 1**: the Tag (Form) feature naming them.
//!
//! The forms (our own shapes, none of Onshape's library):
//! - **Louver**: a hood pressed up out of a slot, an arc rising from one long side of the slot to
//!   its open lip, level at the Height; its ends closed by end walls sloping down to the sheet at
//!   the Angle (so the hood tapers off towards its ends, like a pressed louver);
//! - **Bridge lance**: a strip cut free along both long sides and raised the Height as a bridge
//!   with ramps at the Angle;
//! - **Dimple**: a round, flat-topped boss, its wall drafted in by [`DRAFT`] and rounded into the
//!   top, hollow underneath (a revolve; [`DimpleSection::added_volume`] is its exact volume);
//! - **Emboss**: a rectangular, flat-topped boss, its sides drafted in by [`DRAFT`], hollow
//!   underneath ([`EmbossSection::added_volume`]);
//! - **Extruded hole**: a hole with a collar of the Height drawn down out of the sheet.
//!
//! [`studio`] builds one form's features for given variables and thickness (what the Form
//! feature rebuilds from); [`document`] is the whole library as a document.


use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::{CommandError, History};
use crate::commands::{AddExtrude, AddFeature, AddRevolve, AddSketch, EditSketch, SetExtrude};
use crate::document::{AxisRef, BooleanFeature, BooleanKind, BooleanOp, Document, Element, ExtrudeFeature, Feature, FeatureKind, Offset, RevolveFeature, RevolveType};
use crate::ids::{DocumentId, ElementId, FeatureId, PartId};
use crate::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use crate::sheetmetal::plain;
use crate::sheetmetal_form::{FormVariable, LIBRARY_NAME, LibraryForm, TagFormFeature};
use crate::variables::VariableFeature;

use super::gear_cover::{DocHistory, Studio};

/// The library document.
pub const DOCUMENT: DocumentId = DocumentId::from_u128(0x5390_0000_0000_0000_0000_0000_0000_0100);

fn index(form: LibraryForm) -> u128 {
    LibraryForm::ALL.iter().position(|f| *f == form).unwrap_or(0) as u128
}

/// The form's Part Studio in the library document.
pub fn studio_id(form: LibraryForm) -> ElementId {
    ElementId::from_u128(0x5390_0000_0000_0000_0000_0000_0001_0000 | (index(form) << 8))
}

fn fid(form: LibraryForm, k: u128) -> FeatureId {
    FeatureId::from_u128(0x5390_0000_0000_0000_0000_0000_0002_0000 | (index(form) << 8) | k)
}

/// The form's parts: the add part and the remove part.
pub fn add_part(form: LibraryForm) -> PartId {
    PartId::new(fid(form, 0x21), 0)
}

pub fn remove_part(form: LibraryForm) -> PartId {
    PartId::new(fid(form, 0x31), 0)
}

/// The form's flat-view sketch and origin connector.
pub fn profile_sketch(form: LibraryForm) -> FeatureId {
    fid(form, 0x10)
}

pub fn origin_connector(form: LibraryForm) -> FeatureId {
    fid(form, 0x40)
}

fn value(vars: &[FormVariable], name: &str, form: LibraryForm) -> f64 {
    vars.iter()
        .find(|v| v.name == name)
        .map(|v| v.value)
        .unwrap_or_else(|| form.variables().into_iter().find(|v| v.name == name).map_or(1.0, |v| v.value))
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, construction: bool) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction,
        label: "Add rectangle",
    }
}

fn open(points: Vec<Vec2>) -> SketchOp {
    SketchOp::AddPolyline { points, closed: false, construction: false, label: "Add polyline" }
}

fn poly(points: Vec<Vec2>) -> SketchOp {
    SketchOp::AddPolyline { points, closed: true, construction: false, label: "Add polyline" }
}

fn circle(r: f64, construction: bool) -> SketchOp {
    SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: r, construction }
}

/// A sketch on `plane` with `ops`.
fn sketch(s: &mut dyn Studio, el: ElementId, id: FeatureId, plane: PlaneRef, ops: Vec<SketchOp>) -> Result<(), CommandError> {
    s.run(&AddSketch { element: el, feature: id, plane: Some(plane) })?;
    for op in ops {
        s.run(&EditSketch { element: el, feature: id, op })?;
    }
    Ok(())
}

/// An extrude (New) of the regions of `sketch` under `seeds`.
fn extrude(s: &mut dyn Studio, el: ElementId, sk: FeatureId, id: FeatureId, seeds: &[Vec2], set: impl FnOnce(&mut ExtrudeFeature)) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sk))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("form sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the form is missing".into()));
    }
    let mut e = super::extrude_of(regions, 1.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature: id, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature: id, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// A Top-plane extrude from `z0` to `z1`.
fn between(z0: f64, z1: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        let d = z1 - z0;
        e.depth = d;
        e.depth_expr = format!("{} mm", plain(d));
        if z0.abs() > 1e-12 {
            e.start_offset = Some(Offset { value: z0.abs(), expr: format!("{} mm", plain(z0.abs())), flip: z0 < 0.0 });
        }
    }
}

/// A drafted Top-plane extrude from `z0` to `z1`: its sides lean in by [`DRAFT`] as they rise
/// from `z0`.
fn drafted(z0: f64, z1: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        between(z0, z1)(e);
        e.draft = Some(crate::draft::ExtrudeDraft { angle: DRAFT, expr: format!("{} deg", plain(DRAFT)), flip: false });
    }
}

/// A symmetric extrude `width` wide that `op`s part `part` (Intersect: keeps what the two share).
fn scoped(op: BooleanOp, part: PartId, width: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        symmetric(width)(e);
        e.op = op;
        e.merge_all = false;
        e.merge_scope = vec![part];
    }
}

/// A whole-turn revolve (New) about Z of the region of `sketch` under `seed`.
fn revolve(s: &mut dyn Studio, el: ElementId, sk: FeatureId, id: FeatureId, seed: Vec2) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sk))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("form sketch not found".into()))?;
    let regions = super::region_refs(sk, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the form is missing".into()));
    }
    let axis = Some(AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin)));
    s.run(&AddRevolve { element: el, feature: id, revolve: RevolveFeature { regions, axis, kind: RevolveType::Full, op: BooleanOp::New, ..RevolveFeature::default() } })
}

/// The draft of the dimple's and the emboss's walls (degrees).
pub const DRAFT: f64 = 15.0;

/// The dimple's section (x ≥ 0 out from its axis, z up from the sheet's face): its wall leans in
/// by [`DRAFT`] from the base circle (radius `rb` at z = 0) and is rounded into the flat top
/// (z = `h`) with radius `ro` about (`cx`, `h − ro`); the inside is the same `t` in (radius
/// `ro − t` about the same centre, top at `h − t`).
#[derive(Debug, Clone, Copy)]
pub struct DimpleSection {
    pub rb: f64,
    pub h: f64,
    pub t: f64,
    pub ro: f64,
    pub cx: f64,
}

/// The dimple's section for a Diameter, Height and sheet thickness (kept buildable: at least
/// 4 t across, a little more than t high).
pub fn dimple(diameter: f64, height: f64, t: f64) -> DimpleSection {
    let h = height.max(t * 1.05);
    let rb = (diameter / 2.0).max(2.0 * t + h * DRAFT.to_radians().tan());
    let ro = t + 0.5 * t.min(h - t);
    let (sn, cs) = DRAFT.to_radians().sin_cos();
    let cz = h - ro;
    let cx = (rb * cs - ro - cz * sn) / cs;
    DimpleSection { rb, h, t, ro, cx }
}

impl DimpleSection {
    fn n(&self) -> Vec2 {
        let (sn, cs) = DRAFT.to_radians().sin_cos();
        Vec2::new(cs, sn)
    }

    fn center(&self) -> Vec2 {
        Vec2::new(self.cx, self.h - self.ro)
    }

    /// The point of the line `off` in from the outer wall at height `z`.
    fn wall_at(&self, off: f64, z: f64) -> Vec2 {
        let (sn, cs) = DRAFT.to_radians().sin_cos();
        Vec2::new(self.rb - (off + z * sn) / cs, z)
    }

    fn ops(&self, rad: f64, top: f64, bottom: f64, below: Option<f64>) -> Vec<SketchOp> {
        let c = self.center();
        let n = self.n();
        let tangent = Vec2::new(c.x + rad * n.x, c.y + rad * n.y);
        let off = self.ro - rad;
        let foot = self.wall_at(off, bottom);
        let mut pts = vec![Vec2::new(c.x, top), Vec2::new(0.0, top), Vec2::new(0.0, below.unwrap_or(bottom))];
        if let Some(z) = below {
            pts.push(Vec2::new(foot.x, z));
        }
        pts.push(foot);
        pts.push(tangent);
        vec![SketchOp::AddArc { center: c, start: tangent, end: Vec2::new(c.x, top), construction: false }, open(pts)]
    }

    /// The outside: down to the sheet's underside (a straight foot below the face).
    pub fn outer_ops(&self) -> Vec<SketchOp> {
        self.ops(self.ro, self.h, 0.0, Some(-self.t))
    }

    /// The inside, from `gap` under the sheet.
    pub fn inner_ops(&self, gap: f64) -> Vec<SketchOp> {
        self.ops(self.ro - self.t, self.h - self.t, -self.t - gap, None)
    }

    /// The volume the dimple adds to the sheet: its outside above the face less its inside
    /// above the sheet's underside (Pappus: 2π times the section's first moment about the
    /// axis, each boundary piece integrated exactly).
    pub fn added_volume(&self) -> f64 {
        let moment = |rad: f64, top: f64, bottom: f64| {
            // ∫∫ x dA over the section between z = bottom and its top, by Green's theorem:
            // ∮ x²/2 dz, anticlockwise.
            let c = self.center();
            let n = self.n();
            let tangent = Vec2::new(c.x + rad * n.x, c.y + rad * n.y);
            let foot = self.wall_at(self.ro - rad, bottom);
            let line = |a: Vec2, b: Vec2| (b.y - a.y) * (a.x * a.x + a.x * b.x + b.x * b.x) / 6.0;
            // The arc from the wall's tangent (angle DRAFT) to the top (90°):
            // ∫ (cx + r cos θ)²/2 · r cos θ dθ.
            let (t0, t1) = (DRAFT.to_radians(), std::f64::consts::FRAC_PI_2);
            let f = |th: f64| {
                let (sn, cs) = th.sin_cos();
                0.5 * rad * (c.x * c.x * sn + c.x * rad * (th + sn * cs) + rad * rad * (sn - sn.powi(3) / 3.0))
            };
            line(Vec2::new(0.0, top), Vec2::new(0.0, bottom)) + line(Vec2::new(0.0, bottom), foot) + line(foot, tangent) + (f(t1) - f(t0)) + line(Vec2::new(c.x, top), Vec2::new(0.0, top))
        };
        std::f64::consts::TAU * (moment(self.ro, self.h, 0.0) - moment(self.ro - self.t, self.h - self.t, -self.t))
    }
}

/// The emboss: a `l` × `w` (at the sheet's face) boss `h` high, its sides drafted in by
/// [`DRAFT`], hollowed `t` inside them.
#[derive(Debug, Clone, Copy)]
pub struct EmbossSection {
    pub l: f64,
    pub w: f64,
    pub h: f64,
    pub t: f64,
}

/// The emboss for a Length, Width, Height and thickness (kept buildable).
pub fn emboss(l: f64, w: f64, h: f64, t: f64) -> EmbossSection {
    let h = h.max(t * 1.05);
    let min = 4.0 * t + 2.0 * h * DRAFT.to_radians().tan();
    EmbossSection { l: l.max(min), w: w.max(min), h, t }
}

impl EmbossSection {
    fn tan(&self) -> f64 {
        DRAFT.to_radians().tan()
    }

    /// The outside's half sizes at its start (the sheet's underside).
    pub fn outer_base(&self) -> (f64, f64) {
        (self.l / 2.0 + self.t * self.tan(), self.w / 2.0 + self.t * self.tan())
    }

    /// The inside's half sizes at its start (`gap` under the sheet): `t` in from the outer
    /// sides, square to them.
    pub fn inner_base(&self, gap: f64) -> (f64, f64) {
        let d = self.t / DRAFT.to_radians().cos() - (self.t + gap) * self.tan();
        (self.l / 2.0 - d, self.w / 2.0 - d)
    }

    /// The volume it adds to the sheet: the outside above the face (a frustum of a rectangle,
    /// by the prismoidal formula) less the inside above the underside.
    pub fn added_volume(&self) -> f64 {
        let prismoid = |a: f64, b: f64, height: f64| {
            let s = height * self.tan();
            let area = |k: f64| 4.0 * (a - k * s) * (b - k * s);
            height / 6.0 * (area(0.0) + 4.0 * area(0.5) + area(1.0))
        };
        let d = self.t / DRAFT.to_radians().cos() - self.t * self.tan();
        prismoid(self.l / 2.0, self.w / 2.0, self.h) - prismoid(self.l / 2.0 - d, self.w / 2.0 - d, self.h)
    }
}

/// A symmetric extrude `width` wide.
fn symmetric(width: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e: &mut ExtrudeFeature| {
        e.symmetric = true;
        e.depth = width;
        e.depth_expr = format!("{} mm", plain(width));
    }
}

/// Adds the form's features to the Part Studio `el` (see the module docs).
pub fn build_in(s: &mut dyn Studio, el: ElementId, form: LibraryForm, vars: &[FormVariable], t: f64) -> Result<(), CommandError> {
    let v = |n: &str| value(vars, n, form);
    // The variables.
    let mut all = vec![FormVariable::length("thickness", t)];
    for d in form.variables() {
        all.push(if d.angle { FormVariable::angle(&d.name, v(&d.name)) } else { FormVariable::length(&d.name, v(&d.name)) });
    }
    for (k, x) in all.iter().enumerate() {
        let mut var = VariableFeature::length(&x.name, &x.expr);
        if x.angle {
            var.var_type = crate::variables::VariableType::Angle;
            let _ = var.evaluate(&cadrs_sketch::units::Units::default(), &[]);
        }
        if var.value == 0.0 {
            var.value = x.value;
        }
        s.run(&AddFeature { element: el, feature: fid(form, 0x01 + k as u128), base_name: "Variable".into(), kind: FeatureKind::Variable(var) })?;
    }
    let (sk_add, sk_rem) = (fid(form, 0x20), fid(form, 0x30));
    let (add, rem) = (add_part(form).feature, remove_part(form).feature);
    let gap = 0.5;
    match form {
        LibraryForm::Louver => {
            let (l, w, h) = (v("Length"), v("Width"), v("Height").max(t * 1.05));
            let a = v("Angle").clamp(20.0, 85.0).to_radians();
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, true)])?;
            // The hood's section on Right (y, z): an arc about one centre rising from the sheet
            // at y = −w/2 to its level lip (y = w/2, z = h), `t` thick.
            let r = (w * w + h * h) / (2.0 * h);
            let (cy, cz) = (w / 2.0, h - r);
            let ts = (r - h).atan2(-w);
            let ri = r - t;
            let inner_end = Vec2::new(cy + ri * ts.cos(), cz + ri * ts.sin());
            // Outside: the space under the hood's outer surface, down to the sheet's underside,
            // `l` long, its ends cut back at the Angle from the sheet (the end walls' slope).
            let outer = vec![
                SketchOp::AddArc { center: Vec2::new(cy, cz), start: Vec2::new(w / 2.0, h), end: Vec2::new(-w / 2.0, 0.0), construction: false },
                open(vec![Vec2::new(-w / 2.0, 0.0), Vec2::new(-w / 2.0, -t), Vec2::new(w / 2.0, -t), Vec2::new(w / 2.0, h)]),
            ];
            sketch(s, el, sk_add, PlaneRef::Right, outer)?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, -t / 2.0)], symmetric(l))?;
            let top = h + t + 1.0;
            let wide = w + 2.0 * t + 10.0;
            let taper = |x0: f64| {
                let run = top / a.tan();
                poly(vec![
                    Vec2::new(-x0, -t - 1.0),
                    Vec2::new(x0, -t - 1.0),
                    Vec2::new(x0, 0.0),
                    Vec2::new(x0 - run, top),
                    Vec2::new(-x0 + run, top),
                    Vec2::new(-x0, 0.0),
                ])
            };
            sketch(s, el, fid(form, 0x22), PlaneRef::Front, vec![taper(l / 2.0)])?;
            extrude(s, el, fid(form, 0x22), fid(form, 0x23), &[Vec2::new(0.0, 0.0)], scoped(BooleanOp::Intersect, add_part(form), wide))?;
            // Inside: the space under the hood's underside, open at the lip and below the sheet,
            // its ends cut back `t` (square to the slope) inside the outer ones; taken away, it
            // leaves the hood and its two sloped end walls.
            let x0 = l / 2.0 - t / a.sin();
            let inner = vec![
                SketchOp::AddArc { center: Vec2::new(cy, cz), start: Vec2::new(w / 2.0, h - t), end: inner_end, construction: false },
                open(vec![
                    inner_end,
                    Vec2::new(inner_end.x, -t - 1.0),
                    Vec2::new(w / 2.0 + t + 1.0, -t - 1.0),
                    Vec2::new(w / 2.0 + t + 1.0, h - t),
                    Vec2::new(w / 2.0, h - t),
                ]),
            ];
            let cavity = PartId::new(fid(form, 0x25), 0);
            sketch(s, el, fid(form, 0x24), PlaneRef::Right, inner)?;
            extrude(s, el, fid(form, 0x24), cavity.feature, &[Vec2::new(w / 2.0 + 0.5, -t - 0.5)], symmetric(l + 2.0))?;
            sketch(s, el, fid(form, 0x26), PlaneRef::Front, vec![taper(x0)])?;
            extrude(s, el, fid(form, 0x26), fid(form, 0x27), &[Vec2::new(0.0, 0.0)], scoped(BooleanOp::Intersect, cavity, wide))?;
            let cut = BooleanFeature { op: BooleanKind::Subtract, tools: vec![cavity], targets: vec![add_part(form)], ..Default::default() };
            s.run(&AddFeature { element: el, feature: fid(form, 0x28), base_name: "Boolean".into(), kind: FeatureKind::Boolean(cut) })?;
            // The opening: from where the hood's underside clears the sheet to its lip, between
            // the end walls.
            let clear = std::f64::consts::PI - ((r - h) / ri).clamp(-1.0, 1.0).asin();
            let c = (cy + ri * clear.cos()) + w / 2.0 + 0.02 * w;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-x0, -w / 2.0 + c, x0, w / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, (c + w) / 2.0 - w / 2.0)], between(-t - gap, 0.0))?;
        }
        LibraryForm::Lance => {
            let (l, w) = (v("Length"), v("Width"));
            let a = v("Angle").clamp(15.0, 90.0).to_radians();
            // The ramps rise at the Angle; the Height is kept short enough for both to fit.
            let h = v("Height").max(t * 1.05).min((l / 2.0 - t) * a.tan());
            let run = h / a.tan();
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-l / 2.0, -w / 2.0, l / 2.0, w / 2.0, true)])?;
            let pts = vec![
                Vec2::new(-l / 2.0, 0.0),
                Vec2::new(-l / 2.0 + run, h),
                Vec2::new(l / 2.0 - run, h),
                Vec2::new(l / 2.0, 0.0),
                Vec2::new(l / 2.0, -t),
                Vec2::new(l / 2.0 - run, h - t),
                Vec2::new(-l / 2.0 + run, h - t),
                Vec2::new(-l / 2.0, -t),
            ];
            sketch(s, el, sk_add, PlaneRef::Front, vec![poly(pts)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, h - t / 2.0)], symmetric(w))?;
            let c = t * 1.5;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-l / 2.0 + c, -w / 2.0, l / 2.0 - c, w / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - gap, 0.0))?;
        }
        LibraryForm::Dimple => {
            let d = dimple(v("Diameter"), v("Height"), t);
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![circle(d.rb, true)])?;
            // Half sections on Front (x, z), turned a whole turn about Z: the outside (wall
            // drafted in, rounded into the top) and the inside, `t` in from it.
            sketch(s, el, sk_add, PlaneRef::Front, d.outer_ops())?;
            revolve(s, el, sk_add, add, Vec2::new(0.25 * d.cx, 0.5 * (d.h - d.t)))?;
            sketch(s, el, sk_rem, PlaneRef::Front, d.inner_ops(gap))?;
            revolve(s, el, sk_rem, rem, Vec2::new(0.25 * d.cx, 0.5 * (d.h - 2.0 * d.t - gap)))?;
        }
        LibraryForm::Emboss => {
            let e = emboss(v("Length"), v("Width"), v("Height"), t);
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![rect(-e.l / 2.0, -e.w / 2.0, e.l / 2.0, e.w / 2.0, true)])?;
            // A boss whose sides are drafted in as they rise, hollowed `t` inside them.
            let (a0, b0) = e.outer_base();
            sketch(s, el, sk_add, PlaneRef::Top, vec![rect(-a0, -b0, a0, b0, false)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(0.0, 0.0)], drafted(-t, e.h))?;
            let (a1, b1) = e.inner_base(gap);
            sketch(s, el, sk_rem, PlaneRef::Top, vec![rect(-a1, -b1, a1, b1, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], drafted(-t - gap, e.h - t))?;
        }
        LibraryForm::ExtrudedHole => {
            let (d, h) = (v("Diameter"), v("Height"));
            sketch(s, el, profile_sketch(form), PlaneRef::Top, vec![circle(d / 2.0, true)])?;
            sketch(s, el, sk_add, PlaneRef::Top, vec![circle(d / 2.0 + t, false), circle(d / 2.0, false)])?;
            extrude(s, el, sk_add, add, &[Vec2::new(d / 2.0 + t / 2.0, 0.0)], between(-t - h, 0.0))?;
            sketch(s, el, sk_rem, PlaneRef::Top, vec![circle(d / 2.0, false)])?;
            extrude(s, el, sk_rem, rem, &[Vec2::new(0.0, 0.0)], between(-t - h - gap, gap))?;
        }
    }
    let mc = MateConnectorFeature { origin: Some(ConnectorOrigin::Origin), ..Default::default() };
    s.run(&AddFeature { element: el, feature: origin_connector(form), base_name: "Mate connector".into(), kind: FeatureKind::MateConnector(mc) })?;
    let tag = TagFormFeature {
        add: vec![add_part(form)],
        remove: vec![remove_part(form)],
        sketch: Some(profile_sketch(form)),
        origin: Some(ConnectorRef::Feature(origin_connector(form))),
        ..Default::default()
    };
    s.run(&AddFeature { element: el, feature: fid(form, 0x50), base_name: "Tag".into(), kind: FeatureKind::TagForm(tag) })?;
    // The names a form author would give them.
    rename(s, el, profile_sketch(form), "Form profile")?;
    rename(s, el, origin_connector(form), "Form origin")?;
    Ok(())
}

fn rename(s: &mut dyn Studio, el: ElementId, f: FeatureId, name: &str) -> Result<(), CommandError> {
    s.run(&crate::commands::RenameFeature { element: el, feature: f, name: name.into() })
}

/// One form's Part Studio features for these variables and this sheet thickness.
pub fn studio(form: LibraryForm, vars: &[FormVariable], thickness: f64) -> Result<Vec<Feature>, CommandError> {
    let mut doc = Document::empty(LIBRARY_NAME);
    let mut el = Element::part_studio(form.label());
    el.id = studio_id(form);
    doc.elements.push(el);
    let mut h = History::default();
    build_in(&mut DocHistory(&mut doc, &mut h), studio_id(form), form, vars, thickness)?;
    Ok(doc.element(studio_id(form)).map(|e| e.features().to_vec()).unwrap_or_default())
}

/// The library as a document: one Part Studio per form, at its default sizes and 1 mm thick.
pub fn document() -> Result<Document, CommandError> {
    let mut doc = Document::empty(LIBRARY_NAME);
    doc.id = DOCUMENT;
    let mut h = History::default();
    for form in LibraryForm::ALL {
        let mut el = Element::part_studio(form.label());
        el.id = studio_id(form);
        doc.elements.push(el);
        build_in(&mut DocHistory(&mut doc, &mut h), studio_id(form), form, &form.variables(), 1.0)?;
    }
    Ok(doc)
}

/// The library as a document file.
pub fn file(document: Document) -> crate::store::DocumentFile {
    super::gear_cover::file(document)
}
