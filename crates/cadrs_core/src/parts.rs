//! A Part Studio's parts: extrudes make new parts ("Part 1", "Part 2", …) or add to, cut or
//! intersect existing ones; the Boolean feature combines them (P3.3). Parts are built by
//! [`crate::rebuild`] through the kernel.
//! Sketches on a part's face follow the face: [`refresh_face_planes`] recomputes their frames
//! after every edit.
//!
//! References to faces and edges (sketch planes on faces, Use and Pierce links) are persistent
//! names (P3.2, [`cadrs_kernel::naming`]). After every rebuild they are resolved again: by name,
//! else a renamed entity near where the reference was, else, as a geometric fallback, the
//! entity at that place. A reference that doesn't resolve is lost: its sketch shows the error
//! (S20.2) and keeps its last position; nothing is guessed. A reference found other than by its
//! exact name is repaired to the current name.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use cadrs_sketch::projection::LinkTarget;
use cadrs_sketch::{FaceName, FacePlane, PlaneFrame, PlaneRef};

use crate::document::{Feature, PartProps};
use crate::ids::{FeatureId, PartId};
use crate::rebuild;
use crate::solid::Solid;

/// A solid part or a surface (the Parts list's groups, PS2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PartKind {
    Solid,
    Surface,
}

/// A part: a body the features made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: PartId,
    /// The feature that made it (`id.feature`).
    pub feature: FeatureId,
    /// "Part 1", "Part 2", … (or "Surface 1", …) in the order they were made; a rename is in the
    /// Part Studio's [`PartProps`] (see [`display_name`]).
    pub name: String,
    pub kind: PartKind,
    /// How many parts and surfaces were made before it: its colour is this entry of the
    /// 8-colour palette (PS9.1, [`crate::appearance::palette`]), so it keeps its colour when
    /// another part is deleted.
    pub palette: u32,
    /// Its display mesh (shared with the rebuild cache).
    pub solid: Arc<Solid>,
    /// Volume, area and centroid from the exact geometry (`None` without a kernel).
    pub mass: Option<cadrs_kernel::MassProperties>,
    /// Every feature that made or changed it, in order.
    pub features: Vec<FeatureId>,
    /// The part a pattern or mirror copied it from (P3.8): it shows that part's appearance and
    /// has its material unless it has its own (PS9.6).
    pub source: Option<PartId>,
    /// A part a Derived feature brought in: its source part's settings (name, appearance,
    /// material, properties), which it shows unless it has its own ([`effective_props`]).
    pub derived: Option<Arc<PartProps>>,
}

/// The name a part shows: its rename, else its default name.
pub fn display_name<'a>(part: &'a Part, props: &'a [PartProps]) -> &'a str {
    props
        .iter()
        .find(|p| p.part == part.id)
        .and_then(|p| p.name.as_deref())
        .unwrap_or(&part.name)
}

/// Volume, surface area and centre of mass of several parts together (the Mass and section
/// properties panel's "Parts to measure", X7): the sums, and the volume-weighted centre (the
/// area-weighted one for surfaces only). `None` if a part has no exact properties.
pub fn combined_mass<'a>(
    parts: impl IntoIterator<Item = &'a Part>,
) -> Option<cadrs_kernel::MassProperties> {
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut moment = nalgebra::Vector3::zeros();
    let mut area_moment = nalgebra::Vector3::zeros();
    let mut any = false;
    let mut masses = Vec::new();
    for p in parts {
        let m = p.mass?;
        masses.push(m);
        any = true;
        volume += m.volume;
        area += m.surface_area;
        moment += m.center_of_mass.coords * m.volume;
        area_moment += m.center_of_mass.coords * m.surface_area;
    }
    if !any {
        return None;
    }
    let center = if volume > 0.0 {
        moment / volume
    } else if area > 0.0 {
        area_moment / area
    } else {
        nalgebra::Vector3::zeros()
    };
    let center = nalgebra::Point3::from(center);
    let inertia = masses.iter().map(|m| m.inertia_about(center)).sum();
    Some(cadrs_kernel::MassProperties {
        volume,
        surface_area: area,
        center_of_mass: center,
        inertia,
        length: masses.iter().map(|m| m.length).sum(),
    })
}

/// What the Mass and section properties panel shows for the parts measured (X7, PS10.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassReport {
    /// mm³ and mm², summed.
    pub volume: f64,
    pub surface_area: f64,
    /// Every part has a material: the mass (kg), the centre of mass (mm) and the inertia
    /// tensor about it (kg·mm², axes parallel to the model's; see
    /// [`cadrs_kernel::MassProperties::inertia`]). Onshape leaves these blank until the parts
    /// have materials (`ex1-step6.png`).
    pub mass: Option<MassOfMaterial>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassOfMaterial {
    pub mass: f64,
    pub center_of_mass: nalgebra::Point3<f64>,
    pub inertia: nalgebra::Matrix3<f64>,
}

/// The material a part has, if any.
pub fn material(part: PartId, props: &[PartProps]) -> Option<&crate::material::Material> {
    props
        .iter()
        .find(|p| p.part == part)
        .and_then(|p| p.material.as_ref())
}

/// The material a part has, else the one of the part a pattern or mirror copied it from
/// (P3.8, PS9.6).
pub fn part_material<'a>(
    part: &'a Part,
    props: &'a [PartProps],
) -> Option<&'a crate::material::Material> {
    material(part.id, props)
        .or_else(|| material(part.source?, props))
        .or_else(|| part.derived.as_ref()?.material.as_ref())
}

/// A part's settings with what it inherits filled in: its own, over its Derived source part's
/// ([`Part::derived`]). `None` if it has neither.
pub fn effective_props(part: &Part, props: &[PartProps]) -> Option<PartProps> {
    let own = props.iter().find(|p| p.part == part.id);
    let Some(d) = part.derived.as_deref() else {
        return own.cloned();
    };
    let mut out = own.cloned().unwrap_or_else(|| PartProps::new(part.id));
    out.part = part.id;
    out.name = out.name.or_else(|| d.name.clone());
    out.appearance = out.appearance.or(d.appearance);
    if out.faces.is_empty() {
        out.faces = d.faces.clone();
    }
    out.material = out.material.or_else(|| d.material.clone());
    if out.properties.is_empty() {
        out.properties = d.properties.clone();
    }
    Some(out)
}

/// Mass properties of several parts with their materials: volumes and areas summed; with a
/// material on every part, the mass `Σ ρᵢVᵢ`, the mass-weighted centre `C = Σ mᵢcᵢ / M` and the
/// inertia `Σ ρᵢ (Jᵢ + Vᵢ(|dᵢ|²E − dᵢdᵢᵀ))` about it, where `Jᵢ` is part i's unit-density
/// tensor about its own centre `cᵢ` and `dᵢ = cᵢ − C` (parallel axes). `None` if a part has no
/// exact properties.
pub fn mass_report(parts: &[&Part], props: &[PartProps]) -> Option<MassReport> {
    let geo = combined_mass(parts.iter().copied())?;
    let mut mass = 0.0;
    let mut moment = nalgebra::Vector3::zeros();
    let mut all = !parts.is_empty();
    let mut dens = Vec::new();
    for p in parts {
        let m = p.mass?;
        // A mass override (P3B.6) stands for the density that gives it.
        let own = |id: PartId| {
            props
                .iter()
                .find(|x| x.part == id)
                .and_then(|x| x.properties.mass_override)
        };
        let rho = own(p.id)
            .or_else(|| own(p.source?))
            .or_else(|| p.derived.as_ref()?.properties.mass_override)
            .filter(|_| m.volume > 0.0)
            .map(|kg| kg / m.volume)
            .or_else(|| part_material(p, props).map(|mat| mat.density_kg_mm3()));
        match rho {
            Some(rho) => {
                mass += rho * m.volume;
                moment += m.center_of_mass.coords * rho * m.volume;
                dens.push((rho, m));
            }
            None => all = false,
        }
    }
    let with_material = (all && mass > 0.0).then(|| {
        let c = nalgebra::Point3::from(moment / mass);
        let inertia = dens.iter().map(|(rho, m)| m.inertia_about(c) * *rho).sum();
        MassOfMaterial {
            mass,
            center_of_mass: c,
            inertia,
        }
    });
    Some(MassReport {
        volume: geo.volume,
        surface_area: geo.surface_area,
        mass: with_material,
    })
}

/// The Mass properties panel's options (P3.10, X7): an overridden mass (kg) and a reference
/// mate connector's frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MassOptions {
    /// Override mass: the parts weigh this much (their densities scaled alike), so the centre
    /// of mass stays and the inertia scales by `override / mass`. It gives a mass without
    /// materials too (a uniform density).
    pub override_mass: Option<f64>,
    /// The reference frame: the centre of mass is given in its coordinates and the inertia (still
    /// about the centre of mass) along its axes, `Rᵀ I R` with R's columns the frame's axes.
    pub reference: Option<cadrs_sketch::PlaneFrame>,
}

/// [`mass_report`] with the panel's options (P3.10, X7).
pub fn mass_report_with(
    parts: &[&Part],
    props: &[PartProps],
    opts: MassOptions,
) -> Option<MassReport> {
    let mut r = mass_report(parts, props)?;
    if let Some(m) = opts.override_mass.filter(|m| *m > 0.0 && m.is_finite()) {
        r.mass = Some(match r.mass {
            Some(x) => MassOfMaterial {
                mass: m,
                center_of_mass: x.center_of_mass,
                inertia: x.inertia * (m / x.mass),
            },
            // No materials: a uniform density m / V over all the parts.
            None => {
                let geo = combined_mass(parts.iter().copied())?;
                let rho = m / geo.volume.max(1e-300);
                let c = geo.center_of_mass;
                let inertia = parts
                    .iter()
                    .filter_map(|p| p.mass)
                    .map(|x| x.inertia_about(c) * rho)
                    .sum();
                MassOfMaterial {
                    mass: m,
                    center_of_mass: c,
                    inertia,
                }
            }
        });
    }
    if let (Some(f), Some(x)) = (opts.reference, r.mass.as_mut()) {
        let axis = |a: [f64; 3]| nalgebra::Vector3::new(a[0], a[1], a[2]);
        let n = f.normal();
        let rot = nalgebra::Matrix3::from_columns(&[axis(f.u), axis(f.v), axis(n)]);
        let d = x.center_of_mass.coords - axis(f.origin);
        x.center_of_mass = nalgebra::Point3::from(rot.transpose() * d);
        x.inertia = rot.transpose() * x.inertia * rot;
    }
    // Round-off (a product of ~1e-12 against moments of ~1e4) reads as zero.
    if let Some(x) = r.mass.as_mut() {
        let scale = x.inertia.iter().fold(0.0f64, |a, v| a.max(v.abs()));
        x.inertia = x
            .inertia
            .map(|v| if v.abs() <= 1e-12 * scale { 0.0 } else { v });
    }
    Some(r)
}

impl Solid {
    /// Adds another solid's triangles, faces and edges.
    pub fn append(&mut self, other: Solid) {
        let base = self.positions.len() as u32;
        let tri_base = self.triangle_count();
        let face_base = self.faces.len();
        self.positions.extend(other.positions);
        self.normals.extend(other.normals);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + base));
        self.faces.extend(other.faces.into_iter().map(|mut f| {
            f.first_triangle += tri_base;
            f
        }));
        self.edges.extend(other.edges);
        self.vertices.extend(other.vertices);
        let run_base = self.rulings.iter().map(|r| r.run + 1).max().unwrap_or(0);
        self.rulings.extend(other.rulings.into_iter().map(|mut r| {
            r.face += face_base;
            r.run += run_base;
            r
        }));
        self.grids.extend(other.grids.into_iter().map(|mut g| {
            g.face += face_base;
            g
        }));
        self.connectors.extend(other.connectors);
    }
}

/// Every part of a Part Studio, in feature order.
pub fn parts(features: &[Feature]) -> Vec<Part> {
    rebuild::build(features).parts.clone()
}

/// True if an extrude uses the sketch (Onshape greys such a sketch out in the feature list and
/// hides it in the view).
pub fn sketch_consumed(features: &[Feature], sketch: FeatureId) -> bool {
    features
        .iter()
        .any(|f| f.input_sketches().contains(&sketch))
}

/// The part `extrude` made (from the features up to it), if it made one: the one with the face
/// `face` if it made several.
fn part_with(features: &[Feature], extrude: FeatureId, face: Option<&FaceName>) -> Option<Part> {
    let i = features.iter().position(|f| f.id == extrude)?;
    let build = rebuild::build(&features[..=i]);
    let mine = || {
        build
            .parts
            .iter()
            .filter(|p| p.feature == extrude || p.features.contains(&extrude))
    };
    face.and_then(|n| mine().find(|p| p.solid.face(n).is_some()))
        .or_else(|| mine().next())
        .cloned()
}

/// The part `extrude` made (from the features up to it), if it made one.
pub fn part_of(features: &[Feature], extrude: FeatureId) -> Option<Part> {
    part_with(features, extrude, None)
}

/// The frame of a planar face of the part made by `extrude`, if it still exists.
pub fn face_frame(features: &[Feature], extrude: FeatureId, face: &FaceName) -> Option<PlaneFrame> {
    let solid = &part_with(features, extrude, Some(face))?.solid;
    let name = solid.canonical_face(face);
    let i = solid.faces.iter().position(|f| f.name == name)?;
    solid.face_plane_as(i, face)
}

/// The name of a cap of the part `extrude` made: of its `index`-th region (in the extrude's
/// list), at the far end (`end`) or on the sketch plane.
pub fn cap_name(
    features: &[Feature],
    extrude: FeatureId,
    index: usize,
    end: bool,
) -> Option<FaceName> {
    let e = features.iter().find(|f| f.id == extrude)?.extrude()?;
    Some(crate::solid::cap_name(
        extrude.0,
        e.regions.get(index)?.key(),
        end,
    ))
}

/// A sketch plane on a face of the part `extrude` made, its axes turned upright
/// ([`cadrs_sketch::PlaneFrame::upright`]).
pub fn face_plane(features: &[Feature], extrude: FeatureId, face: FaceName) -> Option<PlaneRef> {
    // A piece of the face a later feature split off (a cut through it: `split` > 0) exists only
    // after that feature: then it is found in the parts `features` build (as the sketch's
    // regeneration finds it, [`regenerate`]).
    let part = part_with(features, extrude, Some(&face))
        .filter(|p| p.solid.face(&face).is_some())
        .or_else(|| {
            rebuild::build(features)
                .parts
                .iter()
                .find(|p| p.solid.face(&face).is_some())
                .cloned()
        })?;
    let named = part.solid.canonical_face(&face);
    let i = part.solid.faces.iter().position(|f| f.name == named)?;
    let frame = part.solid.face_plane_as(i, &face)?;
    let seed = part.solid.face_point(i);
    let frame = frame.upright();
    Some(PlaneRef::Face(FacePlane {
        feature: extrude.0,
        face,
        origin: frame.origin,
        u: frame.u,
        v: frame.v,
        seed,
        upright: true,
    }))
}

/// A sketch plane on the Plane feature `plane` (P3.7), with its frame as the features up to it
/// build it; `None` if it doesn't build.
pub fn plane_feature_ref(features: &[Feature], plane: FeatureId) -> Option<PlaneRef> {
    let i = features.iter().position(|f| f.id == plane)?;
    let frame = match plain_plane_frame(&features[..=i], plane, &mut Default::default()) {
        Some(f) => f,
        None => *rebuild::build(&features[..=i]).planes.get(&plane)?,
    };
    Some(PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(
        plane.0, frame,
    )))
}

/// Moves every sketch on a face to where its face is now (after an edit changed the part).
/// Features are regenerated in order, so a sketch on a face of a part that was itself
/// extruded from a sketch on a face follows along. A face that no longer exists leaves its
/// sketch where it was.
pub fn refresh_face_planes(features: &mut [Feature]) {
    regenerate(features);
}

/// True when the sketch `features[i]` sits on a part face whose extrude is gone (S20.2: the
/// sketch is in error). Cheap enough to ask every frame (no solid is built).
pub fn sketch_face_lost(features: &[Feature], i: usize) -> bool {
    let Some(PlaneRef::Face(fp)) = features
        .get(i)
        .and_then(|f| f.sketch())
        .and_then(|s| s.plane)
    else {
        return false;
    };
    // P3B.9: a face of the assembly context is checked against the context.
    if crate::assembly::context::is_context(FeatureId(fp.feature)) {
        return false;
    }
    !features[..i]
        .iter()
        .any(|f| f.id.0 == fp.feature && f.is_part_feature())
}

/// [`sketch_face_lost`], or the sketch's face no longer resolves on the rebuilt `parts` (a lost
/// reference: its part failed to build, or the face is gone and nothing lies in its plane).
pub fn sketch_face_lost_in(features: &[Feature], i: usize, parts: &[Part]) -> bool {
    if sketch_face_lost(features, i) {
        return true;
    }
    let Some(PlaneRef::Face(fp)) = features[i].sketch().and_then(|s| s.plane) else {
        return false;
    };
    if crate::assembly::context::is_context(FeatureId(fp.feature)) {
        return false;
    }
    if solid_for_face(parts.iter().map(|p| (p.feature, &*p.solid)), &fp).is_some() {
        return false;
    }
    // Not on the final parts: a later feature may have covered the face (P3.4: the Reducer
    // Coupling's second flange covers the revolve's end face its sketch is on). The sketch only
    // needs the face where it is in the list: the parts the features before it made (cached).
    let before = rebuild::build(&features[..i]);
    solid_for_face(before.parts.iter().map(|p| (p.feature, &*p.solid)), &fp).is_none()
}

/// The solid a sketch plane's face is on: among the parts its feature made the one where the
/// face resolves, else any part where it does (its part was joined to another by a boolean).
fn solid_for_face<'a>(
    solids: impl Iterator<Item = (FeatureId, &'a Solid)> + Clone,
    fp: &FacePlane,
) -> Option<&'a Solid> {
    solids
        .clone()
        .filter(|(id, _)| id.0 == fp.feature)
        .chain(solids.filter(|(id, _)| id.0 != fp.feature))
        .map(|(_, s)| s)
        .find(|s| face_of(s, fp).is_some())
}

/// The face a sketch plane is on, if it resolves (see the module docs): its index and whether
/// its name must be repaired.
fn face_of(solid: &Solid, fp: &FacePlane) -> Option<(usize, bool)> {
    let (i, m) = solid
        .resolve_face(&fp.face, Some(&fp.frame()), fp.seed)
        .ok()?;
    // Only a planar face can carry a sketch.
    solid.faces[i].plane?;
    Some((i, m != cadrs_kernel::naming::Match::Exact))
}

/// Regenerates a Part Studio after an edit, in feature order (T5):
///
/// - sketches on a face move to where the face is now (a face that no longer exists leaves
///   its sketch where it was);
/// - a sketch on a face gets the edges of the part faces in its plane as imprints (S21.1),
///   unless "Disable imprinting" is set (S21.2);
/// - projected curves and pierced points move to where their links put them now, and the
///   sketch is solved again (S20.1); links whose source is gone are marked broken (S20.2).
///
/// Features are regenerated in order, so a sketch on a face of a part that was itself
/// extruded from a sketch on a face follows along. Only sketches that depend on parts ask for
/// them: the parts before such a sketch are rebuilt (mostly from the rebuild cache) with the
/// sketches before it already regenerated.
/// The frame of Plane feature `plane` (among `features`) when it is made from default planes and
/// such Plane features alone, without a rebuild; `None` when it needs parts (a face, an edge, a
/// point), is not among `features`, or fails. `memo` keeps the frames worked out.
fn plain_plane_frame(
    features: &[Feature],
    plane: FeatureId,
    memo: &mut std::collections::HashMap<FeatureId, Option<cadrs_sketch::PlaneFrame>>,
) -> Option<cadrs_sketch::PlaneFrame> {
    if let Some(f) = memo.get(&plane) {
        return *f;
    }
    let i = features.iter().position(|f| f.id == plane)?;
    let frame = (|| {
        let crate::FeatureKind::Plane(x) = &features[i].kind else {
            return None;
        };
        if x.problem().is_some() {
            return None;
        }
        let mut geoms = Vec::new();
        for e in &x.entities {
            let crate::plane::PlaneEntity::Plane(r) = e else {
                return None;
            };
            let f = match r {
                PlaneRef::Feature(other) => {
                    plain_plane_frame(&features[..i], FeatureId(other.feature), memo)?
                }
                PlaneRef::Face(_) => return None,
                default => default.frame(),
            };
            geoms.push(crate::plane::RefGeom::Plane(f));
        }
        crate::plane::frame(x, &geoms).ok()
    })();
    memo.insert(plane, frame);
    frame
}

pub fn regenerate(features: &mut [Feature]) {
    regenerate_with(features, &[]);
}

/// The solids of a Part Studio's assembly contexts its sketches may name, by feature id and the
/// context they are in ([`regenerate_with`]).
pub type ContextSolids = Vec<(FeatureId, u32, Arc<Solid>)>;

/// The app's switch ([`defer_regeneration`]).
static DEFER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

std::thread_local! {
    /// Set while a command applies ([`Deferring`]).
    static IN_COMMAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The Part Studios whose sketches a command left ([`take_deferred`]), with their context
/// solids.
static DEFERRED: std::sync::Mutex<Vec<(crate::ids::ElementId, ContextSolids)>> =
    std::sync::Mutex::new(Vec::new());

/// Has a command leave the sketches on parts whose regeneration would rebuild features (a face
/// moved by an edit above it) as they are, for the caller to regenerate later off its thread
/// ([`take_deferred`], [`regenerate_with`]), instead of waiting for those rebuilds: an edit near
/// the top of pi7zoll-gehäuse froze the app 2.7 s, its 21 sketches on faces following.
pub fn defer_regeneration(on: bool) {
    DEFER.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// While it lives, the commands applied on this thread leave their sketches if
/// [`defer_regeneration`] is on ([`crate::History::execute`] holds one; an undo restores
/// sketches as they were made).
pub struct Deferring(bool);

impl Deferring {
    pub fn enter() -> Self {
        Self(IN_COMMAND.with(|c| c.replace(true)))
    }
}

impl Drop for Deferring {
    fn drop(&mut self) {
        IN_COMMAND.with(|c| c.set(self.0));
    }
}

/// The Part Studios whose sketches commands left since the last call, each with the context
/// solids to regenerate them against: [`regenerate_with`] their features, then put the sketches
/// that changed in the document ([`take_regenerated`]).
pub fn take_deferred() -> Vec<(crate::ids::ElementId, ContextSolids)> {
    DEFERRED
        .lock()
        .map(|mut d| std::mem::take(&mut *d))
        .unwrap_or_default()
}

/// The sketches of `element` a regeneration changed ([`take_deferred`]): those of `before`
/// (its features as they were regenerated) that `after` (as regenerated) has otherwise, put in
/// `doc` where the feature is still as it was. Whether any was.
pub fn take_regenerated(
    doc: &mut crate::document::Document,
    element: crate::ids::ElementId,
    before: &[Feature],
    after: &[Feature],
) -> bool {
    let Some(features) = doc.element_mut(element).and_then(|e| e.features_mut()) else {
        return false;
    };
    let mut any = false;
    for (was, now) in before.iter().zip(after) {
        if was == now {
            continue;
        }
        if let Some(f) = features.iter_mut().find(|f| f.id == now.id)
            && *f == *was
        {
            *f = now.clone();
            any = true;
        }
    }
    any
}

/// [`regenerate`] with extra reference solids by feature id and the assembly context they are in
/// (P3B.9: a Part Studio's assembly contexts, [`crate::assembly::context`]), which sketch faces
/// and links may name. A sketch whose features above and itself are as when it was last
/// regenerated is left as it is ([`REGENERATED`]).
pub fn regenerate_with(features: &mut [Feature], context: &[(FeatureId, u32, Arc<Solid>)]) {
    regenerate_or_leave(features, context, false);
}

/// [`regenerate_with`] from a command: with [`defer_regeneration`] on, the sketches from the
/// first one whose regeneration rebuilds features on are left as they are, and `element` noted
/// for [`take_deferred`]. `studio` is the Variable Studios' variables the list names
/// ([`crate::document::Element::studio_features`]): the rebuilds see them above the list, as
/// every rebuild of it does (a sketch on a face of an extrude sized by one stayed where it was).
pub(crate) fn regenerate_studio(
    studio: &[Feature],
    features: &mut [Feature],
    context: &[(FeatureId, u32, Arc<Solid>)],
    element: crate::ids::ElementId,
) {
    let defer = DEFER.load(std::sync::atomic::Ordering::Relaxed) && IN_COMMAND.with(|c| c.get());
    let done = if studio.is_empty() || !features.iter().any(needs_parts) {
        regenerate_or_leave(features, context, defer)
    } else {
        let mut all: Vec<Feature> = studio.to_vec();
        all.extend(features.iter().cloned());
        let done = regenerate_or_leave(&mut all, context, defer);
        for (f, now) in features.iter_mut().zip(all.drain(studio.len()..)) {
            *f = now;
        }
        done
    };
    if !done && let Ok(mut d) = DEFERRED.lock() {
        d.retain(|(e, _)| *e != element);
        d.push((element, context.to_vec()));
    }
}

/// [`regenerate_with`], leaving the sketches from the first one that needs a rebuild on if
/// `leave`. Whether none was left.
fn regenerate_or_leave(
    features: &mut [Feature],
    context: &[(FeatureId, u32, Arc<Solid>)],
    leave: bool,
) -> bool {
    let Some(last) = features.iter().rposition(needs_parts) else {
        return true;
    };
    // (Locked only to look and to note: not while a sketch's rebuilds run, which a
    // regeneration in the background takes.)
    let noted = |id: FeatureId| {
        REGENERATED
            .lock()
            .ok()
            .and_then(|d| d.as_ref().and_then(|d| d.get(&id).copied()))
    };
    let mut frames = std::collections::HashMap::new();
    // The chain key of the features above the one at hand (the rebuild's: every parameter).
    let mut above = rebuild::CHAIN_SEED;
    for i in 0..=last {
        if !needs_parts(&features[i]) {
            above = rebuild::chain_key(above, &features[i]);
            continue;
        }
        // A sketch on a Plane feature made from default planes alone (the usual offset plane)
        // needs no parts: its frame is worked out here, without waiting for a rebuild (with
        // many such sketches, one rebuild each made every edit of a big Part Studio take
        // seconds).
        let only_plane = features[i].sketch().and_then(|sk| match sk.plane {
            Some(PlaneRef::Feature(fp))
                if sk.geometry.links().is_empty()
                    && sk.geometry.broken.is_empty()
                    && sk.geometry.imprint.is_empty() =>
            {
                Some(fp)
            }
            _ => None,
        });
        if let Some(fp) = only_plane
            && let Some(frame) =
                plain_plane_frame(&features[..i], FeatureId(fp.feature), &mut frames)
        {
            let new = PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(fp.feature, frame));
            if let Some(sk) = features[i].sketch_mut()
                && sk.plane != Some(new)
            {
                sk.plane = Some(new);
            }
            above = rebuild::chain_key(above, &features[i]);
            continue;
        }
        let id = features[i].id;
        let key = rebuild::chain_key(above, &features[i]);
        let unchanged = context.is_empty() && noted(id) == Some(key);
        if unchanged {
            above = key;
            continue;
        }
        if leave {
            return false;
        }
        let built = regenerate_sketch(features, i, context);
        let key = rebuild::chain_key(above, &features[i]);
        if built && let Ok(mut d) = REGENERATED.lock() {
            d.get_or_insert_with(Default::default).insert(id, key);
        }
        above = key;
    }
    true
}

/// Whether a feature is a sketch regenerated against the parts: on a face or a Plane feature, or
/// with links, broken links or imprints.
fn needs_parts(f: &Feature) -> bool {
    f.sketch().is_some_and(|sk| {
        matches!(sk.plane, Some(PlaneRef::Face(_) | PlaneRef::Feature(_)))
            || !sk.geometry.links().is_empty()
            || !sk.geometry.broken.is_empty()
            || !sk.geometry.imprint.is_empty()
    })
}

/// Notes every Part Studio's sketches of `doc` as regenerated as they are ([`REGENERATED`]): a
/// stored document's were, when it was last edited, and an undo step's when it was made. (Not
/// after its Variable Studios' values reach it: a value changed since changes the keys.)
pub fn note_regenerated(doc: &crate::document::Document) {
    let Ok(mut done) = REGENERATED.lock() else {
        return;
    };
    let done = done.get_or_insert_with(Default::default);
    for el in &doc.elements {
        let features = el.features();
        let Some(last) = features.iter().rposition(needs_parts) else {
            continue;
        };
        // (The keys of the list the regeneration rebuilds: after the Variable Studios'
        // variables, [`regenerate_studio`].)
        let mut above = el
            .studio_features()
            .iter()
            .fold(rebuild::CHAIN_SEED, rebuild::chain_key);
        for f in &features[..=last] {
            above = rebuild::chain_key(above, f);
            if needs_parts(f) {
                done.insert(f.id, above);
            }
        }
    }
}

/// Each sketch regenerated against the parts ([`regenerate_with`]), by the chain key of the
/// features above it and itself as it left it: a sketch's regeneration depends on nothing else
/// (but an assembly context's solids), so one whose key is the same is as it would be made. An
/// edit regenerated every sketch on a face, a rebuild of the features above each: deleting the
/// last of pi7zoll-gehäuse's features took a second, 21 rebuilds asked of the kernel process.
static REGENERATED: std::sync::Mutex<Option<std::collections::HashMap<FeatureId, u64>>> =
    std::sync::Mutex::new(None);

/// Regenerates the sketch at `i` against the parts the features above it make (and the
/// `context` solids): its face's frame, its imprints and its links. Whether those parts were
/// built (else it is left as it was).
fn regenerate_sketch(
    features: &mut [Feature],
    i: usize,
    context: &[(FeatureId, u32, std::sync::Arc<Solid>)],
) -> bool {
    let build = rebuild::build(&features[..i]);
    // A rebuild that failed whole (the kernel process gone, the rebuild thread stopped) leaves
    // the sketch as it was, not regenerated.
    let built = !build.stopped
        && (!build.times.is_empty() || !features[..i].iter().any(Feature::is_part_feature));
    let (before, rest) = features.split_at_mut(i);
    let Some(sk) = rest[0].sketch_mut() else {
        return built;
    };
    let refs: Vec<(FeatureId, &Solid)> = build
        .parts
        .iter()
        .map(|p| (p.feature, &*p.solid))
        .chain(context.iter().map(|(f, _, s)| (*f, &**s)))
        .collect();
    // The face it is on (a lost face leaves the sketch where it was).
    if let Some(PlaneRef::Face(fp)) = sk.plane
        && let Some(solid) = solid_for_face(refs.iter().copied(), &fp)
        && let Some((i, _renamed)) = face_of(solid, &fp)
        && let Some(frame) = solid.face_plane_as(i, &fp.face)
    {
        // A face merged into another keeps being referred to by its own name (an alias of
        // the merged face's), with the frame it had: the sketch stays where it was.
        let alias = solid.canonical_face(&fp.face) != fp.face;
        let fp = FacePlane {
            face: if alias { fp.face } else { solid.faces[i].name },
            seed: solid.face_point(i),
            ..fp
        };
        let new = PlaneRef::Face(fp.with_frame(frame));
        if sk.plane != Some(new) {
            sk.plane = Some(new);
        }
    }
    // A Plane feature: where it is now (a failed plane leaves the sketch where it was).
    if let Some(PlaneRef::Feature(fp)) = sk.plane
        && let Some(frame) = build.planes.get(&FeatureId(fp.feature))
    {
        let new = PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(fp.feature, *frame));
        if sk.plane != Some(new) {
            sk.plane = Some(new);
        }
    }
    let Some(plane) = sk.plane else {
        return built;
    };
    let frame = plane.frame();
    // Imprints.
    // MC1.7: on a context face, the geometry of that context only (another context's ghost
    // may lie in the same plane); else the studio's own parts.
    let imprint = match plane {
        PlaneRef::Face(fp) if !sk.disable_imprinting => {
            let on = context
                .iter()
                .find(|(f, ..)| f.0 == fp.feature)
                .map(|(_, c, _)| *c);
            let own: Vec<(FeatureId, &Solid)> = refs
                .iter()
                .copied()
                .filter(|(f, _)| {
                    !crate::assembly::context::is_context(*f)
                        || on.is_some_and(|n| context.iter().any(|(g, c, _)| g == f && *c == n))
                })
                .collect();
            crate::links::imprint(&own, &frame)
        }
        _ => Vec::new(),
    };
    if sk.geometry.imprint != imprint {
        sk.geometry.imprint = imprint;
    }
    // Links.
    let links = sk.geometry.links();
    if links.is_empty() && sk.geometry.broken.is_empty() {
        return built;
    }
    let ctx = crate::links::LinkContext {
        solids: refs,
        features: before,
    };
    let mut g = sk.geometry.clone();
    let mut broken = std::collections::BTreeSet::new();
    for (k, target, link) in links {
        // What the link refers to now; a repaired name is written back.
        let (at, pierce) = match target {
            LinkTarget::Curve(c) => (crate::links::curve_samples(&g, c), false),
            LinkTarget::Point(p) => (vec![g.pos(p)], true),
        };
        // A Plane feature's trace follows the plane (S12.10).
        let stored = link;
        let link = match link {
            cadrs_sketch::Link::Plane(PlaneRef::Feature(fp)) => {
                match build.planes.get(&FeatureId(fp.feature)) {
                    Some(f) => cadrs_sketch::Link::Plane(PlaneRef::Feature(
                        cadrs_sketch::FeaturePlane::new(fp.feature, *f),
                    )),
                    None => link,
                }
            }
            l => l,
        };
        // A flat pattern's line (P3I.6, SM14): where the flat has it now.
        if let cadrs_sketch::Link::FlatLine { model, part, bend } = link {
            let shape = crate::sheetmetal_flat::flat_line(
                &build.sheet_metal,
                FeatureId(model),
                part as usize,
                bend,
                &at,
            );
            let ok = match (target, shape) {
                (LinkTarget::Curve(c), Some(shape)) => g.set_projected(c, shape),
                _ => false,
            };
            if !ok {
                broken.insert(k);
            }
            continue;
        }
        let Some(current) = ctx.resolve(link, &frame, &at, pierce) else {
            broken.insert(k);
            continue;
        };
        if current != stored {
            g.set_link(k, current);
        }
        let link = current;
        let ok = match target {
            LinkTarget::Curve(c) => ctx
                .shape(link, &frame)
                .is_some_and(|shape| g.set_projected(c, shape)),
            LinkTarget::Point(p) => match ctx.pierce(link, &frame, g.pos(p)) {
                Some(at) => {
                    g.set_pierced(p, at);
                    true
                }
                None => false,
            },
        };
        if !ok {
            broken.insert(k);
        }
    }
    g.broken = broken;
    if g != sk.geometry {
        sk.geometry = follow_links(&sk.geometry, g);
    }
    built
}

/// The sketch `old` with its used geometry where `moved` (`old` with its links updated, not
/// solved) puts it, solved. When the used geometry jumps, it moves there in steps of at most
/// half a millimetre (at most 64), the rest following as in a drag: an offset is held by its
/// distance, not its side, so a used edge jumping further than the offset would otherwise
/// leave the offset on its far side.
fn follow_links(old: &cadrs_sketch::Sketch, moved: cadrs_sketch::Sketch) -> cadrs_sketch::Sketch {
    use cadrs_sketch::CurveKind;
    let changed: std::collections::HashMap<
        cadrs_sketch::PointId,
        (cadrs_sketch::Vec2, cadrs_sketch::Vec2),
    > = moved
        .points
        .iter()
        .filter_map(|(k, p)| {
            old.points
                .get(k)
                .filter(|o| o.pos != p.pos)
                .map(|o| (k, (o.pos, p.pos)))
        })
        .collect();
    let far = changed
        .values()
        .map(|(a, b)| a.distance(*b))
        .fold(0.0, f64::max);
    let steps = ((far / 0.5).ceil() as usize).clamp(1, 64);
    let mut prev = old.clone();
    for k in 1..=steps {
        let t = k as f64 / steps as f64;
        let mut s = moved.clone();
        for (id, p) in s.points.iter_mut() {
            p.pos = match changed.get(&id) {
                Some((a, b)) => *a + (*b - *a) * t,
                None => prev.points.get(id).map_or(p.pos, |q| q.pos),
            };
        }
        for (id, c) in s.curves.iter_mut() {
            let (Some(o), Some(q)) = (old.curves.get(id), prev.curves.get(id)) else {
                continue;
            };
            c.kind = match (o.kind, c.kind) {
                // A used circle's radius changes with its source.
                (
                    CurveKind::Circle { radius: r0, .. },
                    CurveKind::Circle { center, radius: r1 },
                ) if r0 != r1 => CurveKind::Circle {
                    center,
                    radius: r0 + (r1 - r0) * t,
                },
                _ if o.kind == c.kind => q.kind,
                (_, kind) => kind,
            };
        }
        cadrs_sketch::solve::solve(&mut s);
        prev = s;
    }
    prev
}
