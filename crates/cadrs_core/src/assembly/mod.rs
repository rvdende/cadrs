//! Assemblies (P3B.1, `intro-to-assemblies.md` A2, A3, X1, X2): an Assembly tab holds
//! **instances** of parts. Each instance references a part of a Part Studio (its source), has a
//! placement ([`Pose`]) in the assembly, a number (`Motor Mount <1>`, `<2>`, … per source) and
//! flags (hidden, fixed). Instances stay live: they are drawn and measured from the source Part
//! Studio's current rebuild, so editing the studio changes every instance of its parts.
//!
//! The model grows by `serde(default)` fields: the mate features ([`mate`], P3B.2: mates between
//! [`connector`]s, and groups), placed by the [`solver`]; **subassembly** instances (P3B.4,
//! [`InstanceSource::Assembly`], see [`structure`]) and **folders** in both lists
//! ([`folders`]); P3B.6: the assembly's own [`crate::properties`] and its **Bill of Materials**
//! ([`bom`]).
//!
//! Every change goes through the commands in [`commands`] (one undo step each).
//!
//! Instances are measured and drawn as [`Part`]s in assembly coordinates ([`instance_parts`]):
//! the source part's mesh and exact mass properties, moved by the instance's pose (the
//! tessellation itself is the studio's cached one; nothing is rebuilt or re-tessellated).

pub mod bom;
pub mod commands;
pub mod connector;
pub mod context;
pub mod explode;
pub mod folders;
pub mod interference;
pub mod items;
pub mod managed_context;
pub mod mate;
pub mod positions;
pub mod relation;
pub mod replace;
pub mod replicate;
pub mod structure;
pub mod solver;
pub mod standard;
pub mod thumb;
pub mod vars;

use std::sync::Arc;

use cadrs_sketch::{PlaneFrame, Vec3};
use nalgebra::{Matrix3, Point3, Vector3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::document::{Document, PartProps};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::parts::Part;
use crate::rebuild::Build;
use crate::solid::Solid;

/// Identifies an instance within an assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId(pub Uuid);

impl InstanceId {
    /// The assembly's **Origin** as a mate's instance (P3B.7, A16.3: an instance fastened to the
    /// origin): ground, at the identity placement. It is not in the Instances list's model.
    pub const ORIGIN: InstanceId = InstanceId(Uuid::from_u128(0x0419_6f72_6967_696e));

    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// A deterministic id, for tests and scripted scenarios.
    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }

    /// The id the instance has as a [`Part`] of the assembly view (see [`instance_parts`]).
    pub fn part_id(self) -> PartId {
        PartId::new(FeatureId(self.0), 0)
    }

    /// The instance a part of the assembly view stands for.
    pub fn of_part(part: PartId) -> Self {
        Self(part.feature.0)
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A rigid placement: `p ↦ R p + t` (the instance's own coordinates to the assembly's).
/// The rotation is kept as a matrix so the 90° and 180° turns of the triad stay exact.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    /// The rotation's rows.
    pub rotation: [[f64; 3]; 3],
    /// mm.
    pub translation: Vec3,
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Pose {
    pub const IDENTITY: Pose = Pose {
        rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0; 3],
    };

    /// A translation by `t`.
    pub fn translation(t: Vec3) -> Self {
        Self { translation: t, ..Self::IDENTITY }
    }

    /// A turn by `angle` (radians, right-handed) about the axis through `point` along `axis`.
    /// Entries within 1e-12 of a whole number are rounded to it, so quarter and half turns
    /// about the model axes are exact.
    pub fn rotation_about(point: Vec3, axis: Vec3, angle: f64) -> Self {
        let a = Vector3::from(axis);
        let n = a.norm();
        if n < 1e-300 {
            return Self::IDENTITY;
        }
        let k = a / n;
        let (s, c) = angle.sin_cos();
        let kx = Matrix3::new(0.0, -k.z, k.y, k.z, 0.0, -k.x, -k.y, k.x, 0.0);
        let r = Matrix3::identity() * c + kx * s + k * k.transpose() * (1.0 - c);
        let rot = Self::clean(Self::from_matrix(r));
        // p ↦ R (p − point) + point.
        let pt = Vector3::from(point);
        let t = pt - Self::matrix(&rot) * pt;
        Self { rotation: rot, translation: [t.x, t.y, t.z] }
    }

    fn from_matrix(m: Matrix3<f64>) -> [[f64; 3]; 3] {
        [[m[(0, 0)], m[(0, 1)], m[(0, 2)]], [m[(1, 0)], m[(1, 1)], m[(1, 2)]], [m[(2, 0)], m[(2, 1)], m[(2, 2)]]]
    }

    fn matrix(r: &[[f64; 3]; 3]) -> Matrix3<f64> {
        Matrix3::new(r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2])
    }

    fn clean(mut r: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
        for row in &mut r {
            for x in row {
                let w = x.round();
                if (*x - w).abs() < 1e-12 {
                    *x = w;
                }
            }
        }
        r
    }

    /// The rotation as a matrix.
    pub fn rotation_matrix(&self) -> Matrix3<f64> {
        Self::matrix(&self.rotation)
    }

    /// Where the point `p` goes.
    pub fn apply(&self, p: Vec3) -> Vec3 {
        let q = self.rotation_matrix() * Vector3::from(p) + Vector3::from(self.translation);
        [q.x, q.y, q.z]
    }

    /// Where the direction `v` goes (rotated only).
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        let q = self.rotation_matrix() * Vector3::from(v);
        [q.x, q.y, q.z]
    }

    /// This pose followed by `after`: `p ↦ after(self(p))`.
    pub fn then(&self, after: &Pose) -> Pose {
        let r = after.rotation_matrix() * self.rotation_matrix();
        let t = after.rotation_matrix() * Vector3::from(self.translation) + Vector3::from(after.translation);
        Pose { rotation: Self::clean(Self::from_matrix(r)), translation: [t.x, t.y, t.z] }
    }

    /// The inverse placement.
    pub fn inverse(&self) -> Pose {
        let rt = self.rotation_matrix().transpose();
        let t = -(rt * Vector3::from(self.translation));
        Pose { rotation: Self::from_matrix(rt), translation: [t.x, t.y, t.z] }
    }

    /// A plane frame moved by this pose.
    pub fn frame(&self, f: &PlaneFrame) -> PlaneFrame {
        PlaneFrame { origin: self.apply(f.origin), u: self.rotate(f.u), v: self.rotate(f.v) }
    }
}

/// What an instance is an instance of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstanceSource {
    /// A part of a Part Studio of this document.
    Part { element: ElementId, part: PartId },
    /// Another Assembly tab of this document: a **subassembly** (P3B.4, A17).
    Assembly { element: ElementId },
    /// A whole Part Studio inserted as one **rigid** instance (P3B.8, A2.4): the parts listed
    /// on the instance ([`Instance::parts`]) move as one; its Edit adds or removes parts.
    Studio { element: ElementId },
}

impl InstanceSource {
    /// The Part Studio or Assembly tab it comes from.
    pub fn element(&self) -> ElementId {
        match self {
            InstanceSource::Part { element, .. } | InstanceSource::Assembly { element } | InstanceSource::Studio { element } => *element,
        }
    }

    /// The part, for a part instance.
    pub fn part(&self) -> Option<PartId> {
        match self {
            InstanceSource::Part { part, .. } => Some(*part),
            InstanceSource::Assembly { .. } | InstanceSource::Studio { .. } => None,
        }
    }

    pub fn is_assembly(&self) -> bool {
        matches!(self, InstanceSource::Assembly { .. })
    }

    /// A rigid Part Studio instance (A2.4).
    pub fn is_studio(&self) -> bool {
        matches!(self, InstanceSource::Studio { .. })
    }

    /// An instance that stands for several parts (a subassembly or a rigid Part Studio).
    pub fn is_composite(&self) -> bool {
        !matches!(self, InstanceSource::Part { .. })
    }
}

/// An instance of a part in an assembly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub source: InstanceSource,
    /// Its number among the instances of the same source: the `<n>` of `Motor Mount <1>`.
    pub index: u32,
    /// Its placement in the assembly.
    #[serde(default)]
    pub pose: Pose,
    /// Hidden (the eye, Hide, Y).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Fixed in place (A3.7): it can't be dragged, and the solver keeps it put (ground).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    /// Suppressed (X2, P3B.4): kept in the list (greyed), but not drawn, measured or solved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// A subassembly instance that is **flexible** (A16.2): its own mates act in this assembly,
    /// so its instances move as they would in its tab. Rigid (the default) moves as one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flexible: bool,
    /// A flexible subassembly's placements of its own instances here (their poses in the
    /// subassembly's coordinates, by their id in its tab), over the ones of its tab.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<(InstanceId, Pose)>,
    /// A rigid Part Studio instance's parts (P3B.8, A2.4), in the studio's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartId>,
    /// A rigid subassembly that **follows a Named position** of its tab (P3B.8, A16.2): its
    /// parts are where that position puts them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<positions::NamedPositionId>,
    /// The Replicate feature that made this instance (P3B.8, X16): listed under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replicate: Option<mate::MateId>,
    /// P3G.1 (DV1, ER1): the reference of a **linked** instance (another document's element, or
    /// this document's at a version). Its [`Instance::source`] then names the frozen copy in
    /// [`Document::linked`] ([`crate::external`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<crate::external::SourceRef>,
}

impl Instance {
    /// A new instance of `source` at `pose` (its number is given when it is inserted).
    pub fn new(id: InstanceId, source: InstanceSource, pose: Pose) -> Self {
        Self {
            id,
            source,
            index: 0,
            pose,
            hidden: false,
            fixed: false,
            suppressed: false,
            flexible: false,
            overrides: Vec::new(),
            parts: Vec::new(),
            follow: None,
            replicate: None,
            link: None,
        }
    }

    /// A rigid instance of the whole Part Studio `element` (A2.4) with `parts`.
    pub fn studio(id: InstanceId, element: ElementId, parts: Vec<PartId>, pose: Pose) -> Self {
        Self { parts, ..Self::new(id, InstanceSource::Studio { element }, pose) }
    }

    /// Its placement of its subassembly's instance `child` (an override when flexible).
    pub fn child_pose(&self, child: &Instance) -> Pose {
        self.overrides.iter().find(|(i, _)| *i == child.id).map(|(_, p)| *p).unwrap_or(child.pose)
    }

    /// Its name, `<part name> <n>`, from the source part's current name.
    pub fn name(&self, part_name: &str) -> String {
        format!("{part_name} <{}>", self.index)
    }
}

/// An Assembly tab's contents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Assembly {
    /// The instances, in the Instances list's order (insertion order).
    #[serde(default)]
    pub instances: Vec<Instance>,
    /// The Mate Features list: mates and groups, in creation order (P3B.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mates: Vec<mate::MateFeature>,
    /// Folders of the Instances list (P3B.4, A18): the P3.9 folder model, each a run of
    /// instances (their ids as [`FeatureId`]s, see [`folders`]); an empty folder is listed last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<crate::document::FeatureFolder>,
    /// Folders of the Mate Features list (A18.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mate_folders: Vec<crate::document::FeatureFolder>,
    /// The assembly's own properties: Part number, Description, …, its Subassembly BOM
    /// behavior (P3B.6, [`crate::properties`]).
    #[serde(default, skip_serializing_if = "crate::properties::Properties::is_empty")]
    pub properties: crate::properties::Properties,
    /// Its BOM's columns, view and suppressed rows (P3B.6, [`bom`]).
    #[serde(default, skip_serializing_if = "bom::BomSettings::is_default")]
    pub bom: bom::BomSettings,
    /// Its own explicit mate connectors (P3B.7, A22.2; the Mate connector tool, Ctrl+M).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<connector::LocalConnector>,
    /// Its **Named positions** (P3B.8, A1.8, [`positions`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_positions: Vec<positions::NamedPosition>,
    /// Its **Exploded views** (P3B.8, A1.8, [`explode`]): view states only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exploded_views: Vec<explode::ExplodedView>,
    /// Its **Items** (P3B.8, A1.7, [`items`]): BOM-only items with no geometry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<items::Item>,
}

impl Assembly {
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
            && self.mates.is_empty()
            && self.folders.is_empty()
            && self.mate_folders.is_empty()
            && self.properties.is_empty()
            && self.bom.is_default()
            && self.connectors.is_empty()
            && self.named_positions.is_empty()
            && self.exploded_views.is_empty()
            && self.items.is_empty()
    }

    pub fn local_connector(&self, id: connector::LocalConnectorId) -> Option<&connector::LocalConnector> {
        self.connectors.iter().find(|c| c.id == id)
    }

    /// The mates that fasten an instance to the Origin (A16.3), not suppressed.
    pub fn fastened_to_origin(&self) -> Vec<InstanceId> {
        let mut out = Vec::new();
        for f in self.mates.iter().filter(|f| !f.suppressed) {
            let Some(m) = f.mate() else { continue };
            if m.mate_type != mate::MateType::Fastened {
                continue;
            }
            let [a, b] = &m.connectors;
            for (x, y) in [(a, b), (b, a)] {
                if y.is_origin() && !x.is_origin() && !out.contains(&x.instance) {
                    out.push(x.instance);
                }
            }
        }
        out
    }

    /// The instances a mate feature involves: a relation's are its mates' (P3B.9).
    pub fn feature_instances(&self, f: &mate::MateFeature) -> Vec<InstanceId> {
        match &f.kind {
            mate::MateKind::Relation(r) => {
                let mut out: Vec<InstanceId> = Vec::new();
                for m in r.mates.iter().filter_map(|id| self.mate(*id)) {
                    for i in m.instances() {
                        if !out.contains(&i) {
                            out.push(i);
                        }
                    }
                }
                out
            }
            _ => f.instances(),
        }
    }

    /// Drops the relations whose mates are gone (after a delete).
    pub fn drop_orphan_relations(&mut self) {
        let ids: Vec<mate::MateId> = self.mates.iter().map(|f| f.id).collect();
        self.mates.retain(|f| f.relation().is_none_or(|r| r.mates.iter().all(|m| ids.contains(m))));
    }

    pub fn mate(&self, id: mate::MateId) -> Option<&mate::MateFeature> {
        self.mates.iter().find(|m| m.id == id)
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    pub fn instance_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.instances.iter_mut().find(|i| i.id == id)
    }

    /// The number the next instance of `source` gets: one more than the highest in use.
    pub fn next_index(&self, source: &InstanceSource) -> u32 {
        self.instances.iter().filter(|i| i.source == *source).map(|i| i.index).max().unwrap_or(0) + 1
    }

    /// The number `inst` gets in a document `doc`: one more than the highest of every instance
    /// of the same part, whatever it comes from (P3G.1, as Onshape numbers them, `ex1-step8.png`):
    /// this document's workspace, a version of it, or a version of another document are one
    /// sequence (Part 1 <1> in the workspace, <2> at V1, <3> at V2).
    pub fn next_index_of(&self, doc: crate::ids::DocumentId, inst: &Instance) -> u32 {
        let key = identity(doc, inst);
        self.instances.iter().filter(|i| identity(doc, i) == key).map(|i| i.index).max().unwrap_or(0) + 1
    }
}

/// What numbers an instance: the source document, the source element (in that document) and
/// the part, and the kind of source.
fn identity(doc: crate::ids::DocumentId, inst: &Instance) -> (crate::ids::DocumentId, ElementId, Option<PartId>, u8) {
    let kind = match inst.source {
        InstanceSource::Part { .. } => 0,
        InstanceSource::Assembly { .. } => 1,
        InstanceSource::Studio { .. } => 2,
    };
    match inst.link {
        Some(r) => (r.document_or(doc), r.element, inst.source.part(), kind),
        None => (doc, inst.source.element(), inst.source.part(), kind),
    }
}

/// The name a source shows: a part's rename in its Part Studio, else its default name; a
/// subassembly's tab name.
pub fn source_part_name(doc: &Document, source: &InstanceSource, build: Option<&Build>) -> String {
    let (element, part) = match *source {
        InstanceSource::Part { element, part } => (element, part),
        InstanceSource::Assembly { element } => {
            return doc.element(element).map(|e| e.name.clone()).unwrap_or_else(|| "Assembly".into());
        }
        InstanceSource::Studio { element } => {
            return doc.element(element).map(|e| e.name.clone()).unwrap_or_else(|| "Part Studio".into());
        }
    };
    let props = doc.element(element).map(|e| e.part_props()).unwrap_or(&[]);
    if let Some(n) = props.iter().find(|p| p.part == part).and_then(|p| p.name.clone()) {
        return n;
    }
    build
        .and_then(|b| b.names.iter().find(|(p, _)| *p == part).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| "Part".into())
}

/// Moves a mesh (and its face frames, edges, vertices and silhouette data) by `pose`.
pub fn transform_solid(s: &Solid, pose: &Pose) -> Solid {
    let p = |v: &Vec3| pose.apply(*v);
    let r = |v: &Vec3| pose.rotate(*v);
    let mut out = s.clone();
    out.positions.iter_mut().for_each(|v| *v = p(v));
    out.normals.iter_mut().for_each(|v| *v = r(v));
    for f in &mut out.faces {
        f.plane = f.plane.map(|fr| pose.frame(&fr));
        f.center = f.center.map(|c| p(&c));
        f.axis = f.axis.map(|(o, d)| (p(&o), r(&d)));
        for l in &mut f.loops {
            l.iter_mut().for_each(|v| *v = p(v));
        }
    }
    for e in &mut out.edges {
        e.points.iter_mut().for_each(|v| *v = p(v));
        if let Some(c) = e.circle.as_mut() {
            c.center = p(&c.center);
            c.normal = r(&c.normal);
        }
    }
    for v in &mut out.vertices {
        v.point = p(&v.point);
    }
    for ru in &mut out.rulings {
        ru.start = p(&ru.start);
        ru.end = p(&ru.end);
        ru.normal = r(&ru.normal);
    }
    for g in &mut out.grids {
        for row in &mut g.rows {
            for (q, n) in row {
                *q = p(q);
                *n = r(n);
            }
        }
    }
    for c in &mut out.connectors {
        c.frame = pose.frame(&c.frame);
    }
    for a in &mut out.face_aliases {
        a.plane = a.plane.map(|fr| pose.frame(&fr));
    }
    out
}

/// Mass properties moved by `pose`: the same volume and area, the centre moved, and the
/// inertia tensor turned with the part (`R J Rᵀ`, still about the centre of mass).
pub fn transform_mass(m: &cadrs_kernel::MassProperties, pose: &Pose) -> cadrs_kernel::MassProperties {
    let r = pose.rotation_matrix();
    let c = pose.apply([m.center_of_mass.x, m.center_of_mass.y, m.center_of_mass.z]);
    cadrs_kernel::MassProperties {
        volume: m.volume,
        surface_area: m.surface_area,
        center_of_mass: Point3::from(c),
        inertia: r * m.inertia * r.transpose(),
    }
}

/// The instances of an assembly as parts in assembly coordinates (for drawing, picking and
/// measuring): each part instance is its source part moved by the instance's pose, named
/// `<part> <n>`, with the source part's settings (appearance, material) and the instance's
/// hidden flag; a subassembly instance is every part inside it (its occurrences,
/// [`structure::occurrences`], as parts `PartId(instance, k)`). Suppressed instances, and those
/// whose source is gone (its studio deleted, or the part no longer built), are left out.
///
/// `build_of` gives each source Part Studio's current rebuild.
pub fn instance_parts(
    doc: &Document,
    asm: &Assembly,
    mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>,
) -> (Vec<Part>, Vec<PartProps>) {
    let mut parts = Vec::new();
    let mut props = Vec::new();
    for o in structure::occurrences(doc, asm) {
        let (element, part) = (o.element, o.part);
        let Some(build) = build_of(element) else {
            continue;
        };
        let Some(src) = build.part(part) else {
            continue;
        };
        let name = format!("{} <{}>", source_part_name(doc, &InstanceSource::Part { element, part }, Some(&build)), o.index);
        let id = o.view_part;
        let src_props = doc.element(element).and_then(|e| crate::parts::effective_props(src, e.part_props()));
        let mut p = PartProps::new(id);
        p.name = Some(name.clone());
        p.hidden = o.hidden;
        if let Some(sp) = src_props {
            p.appearance = sp.appearance;
            p.faces = sp.faces;
            p.material = sp.material;
            p.properties = sp.properties;
        }
        parts.push(Part {
            id,
            feature: src.feature,
            name,
            kind: src.kind,
            palette: src.palette,
            solid: Arc::new(transform_solid(&src.solid, &o.pose)),
            mass: src.mass.as_ref().map(|m| transform_mass(m, &o.pose)),
            features: src.features.clone(),
            // The instance's props already carry the source part's appearance and material.
            source: None,
            derived: None,
        });
        props.push(p);
    }
    (parts, props)
}

/// The source part's solid of every instance (in the part's own coordinates), for resolving mate
/// connectors. `build_of` gives each source Part Studio's current rebuild.
pub fn source_solids(
    asm: &Assembly,
    mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>,
) -> std::collections::HashMap<InstanceId, Arc<Solid>> {
    let mut out = std::collections::HashMap::new();
    for inst in &asm.instances {
        if let Some(part) = inst.source.part()
            && let Some(b) = build_of(inst.source.element())
            && let Some(p) = b.part(part)
        {
            out.insert(inst.id, p.solid.clone());
        }
    }
    out
}

/// The source part's solid of every occurrence of `asm` (by occurrence id, see
/// [`structure::occurrences`]): the solids the connectors of [`structure::solver_model`] resolve
/// on.
pub fn occurrence_solids(
    doc: &Document,
    asm: &Assembly,
    mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>,
) -> std::collections::HashMap<InstanceId, Arc<Solid>> {
    let mut out = std::collections::HashMap::new();
    for o in structure::occurrences(doc, asm) {
        if let Some(b) = build_of(o.element)
            && let Some(p) = b.part(o.part)
        {
            out.insert(o.id, p.solid.clone());
        }
    }
    out
}

/// Solves the assembly's mates ([`solver::solve`]) with its connectors resolved on `solids`
/// (see [`source_solids`]) and Tangent propagation applied ([`propagate_tangents`]).
pub fn solve(
    asm: &Assembly,
    solids: &std::collections::HashMap<InstanceId, Arc<Solid>>,
    opts: &solver::SolveOptions,
) -> solver::Solution {
    let asm = resolve_local_connectors(asm, solids);
    let asm = propagate_tangents(&asm, solids);
    let frame = |c: &connector::MateConnector| c.local_frame(solids.get(&c.instance).map(|s| &**s));
    solver::solve(&asm, &frame, opts)
}

/// P3G.5 (ex-dv4, the gap file's Risks): the mates of `asm` (the solver's model, see
/// [`structure::solver_model`]) with a connector whose entity is gone from its part (`solids`,
/// by occurrence): its face or edge was lost in an update, or the part itself. Suppressed mates
/// are skipped. Such a mate is an error; it doesn't hold its instances at the stored frame.
pub fn lost_mates(asm: &Assembly, solids: &std::collections::HashMap<InstanceId, Arc<Solid>>) -> Vec<mate::MateId> {
    asm.mates
        .iter()
        .filter(|f| !f.suppressed)
        .filter(|f| f.mate().is_some_and(|m| m.all_connectors().any(|c| !c.resolves(solids.get(&c.instance).map(|s| &**s)))))
        .map(|f| f.id)
        .collect()
}

/// The source solids of every occurrence of the assembly `element` of `doc`, each Part Studio
/// (or linked copy) rebuilt once.
pub fn document_occurrence_solids(doc: &Document, element: ElementId) -> std::collections::HashMap<InstanceId, Arc<Solid>> {
    let Some(asm) = doc.element(element).and_then(|e| e.assembly_model()) else { return Default::default() };
    let mut builds: std::collections::HashMap<ElementId, Arc<Build>> = std::collections::HashMap::new();
    occurrence_solids(doc, asm, |e| {
        if let Some(b) = builds.get(&e) {
            return Some(b.clone());
        }
        let el = doc.element(e)?;
        el.assembly_model().is_none().then_some(())?;
        let b = crate::rebuild::build(el.features());
        builds.insert(e, b.clone());
        Some(b)
    })
}

/// P3G.5 (ex-dv4, ER6.17): re-solves the mates of the assembly `element` after its sources
/// changed (a linked reference updated): the instances move where the mates now put them (the
/// Topplate rises with longer pistons), and a mate whose entity is gone ([`lost_mates`]) is
/// left out, so it doesn't hold anything at its old frame. Returns those lost mates. Nothing
/// moves when the solve doesn't converge.
pub fn resolve_after_update(doc: &mut Document, element: ElementId) -> Vec<mate::MateId> {
    let Some(asm) = doc.element(element).and_then(|e| e.assembly_model()).cloned() else { return Vec::new() };
    if !asm.mates.iter().any(|f| f.mate().is_some() && !f.suppressed) {
        return Vec::new();
    }
    let solids = document_occurrence_solids(doc, element);
    let mut model = structure::solver_model(doc, &asm);
    let lost = lost_mates(&model, &solids);
    model.mates.retain(|f| !lost.contains(&f.id) && f.relation().is_none_or(|r| r.mates.iter().all(|m| !lost.contains(m))));
    let sol = solve(&model, &solids, &solver::SolveOptions::default());
    if sol.converged {
        let poses = sol.changed(&model);
        let _ = structure::place(doc, element, &poses);
    }
    lost
}

/// Drags instances within their mates ([`solver::drag`], A16.4).
pub fn drag(
    asm: &Assembly,
    solids: &std::collections::HashMap<InstanceId, Arc<Solid>>,
    pulls: &[solver::Pull],
) -> solver::Solution {
    let asm = resolve_local_connectors(asm, solids);
    let asm = propagate_tangents(&asm, solids);
    let frame = |c: &connector::MateConnector| c.local_frame(solids.get(&c.instance).map(|s| &**s));
    solver::drag(&asm, &frame, pulls)
}

/// **Tangent propagation** (A11.2): each Tangent mate with propagation on is solved against the
/// faces tangent-continuous with its picked faces ([`connector::tangent_faces`]): of those, the
/// face on which the contact lies at the current placements (the point of its surface nearest
/// the other entity is on the face, not past its edges), then the one nearest to tangent. So a
/// roller dragged along a flat top, a fillet and a slope, or a pin along a curved slot and round
/// its end, stays in contact with the chain. Mates without propagation keep their picks.
pub fn propagate_tangents(asm: &Assembly, solids: &std::collections::HashMap<InstanceId, Arc<Solid>>) -> Assembly {
    use connector::{ConnectorAnchor, EntityRef, MateConnector};
    let mut out = asm.clone();
    for f in &mut out.mates {
        let mate::MateKind::Mate(m) = &mut f.kind else { continue };
        if m.mate_type != mate::MateType::Tangent || !m.propagate {
            continue;
        }
        let pose = |c: &MateConnector| asm.instance(c.instance).map(|i| i.pose).unwrap_or_default();
        // Each side's candidates: (connector, world frame).
        let options = |c: &MateConnector| -> Vec<(MateConnector, connector::ConnectorFrame)> {
            let mut list = vec![*c];
            if let (ConnectorAnchor::Surface { entity: EntityRef::Face(face), .. }, Some(s)) = (c.anchor, solids.get(&c.instance)) {
                for g in connector::tangent_faces(s, &face).into_iter().skip(1) {
                    if let Some((frame, kind)) = connector::surface_of(s, &EntityRef::Face(g)) {
                        list.push(MateConnector { frame, anchor: ConnectorAnchor::Surface { entity: EntityRef::Face(g), kind }, ..*c });
                    }
                }
            }
            list.into_iter()
                .map(|c| {
                    let local = c.local_frame(solids.get(&c.instance).map(|s| &**s));
                    (c, local.moved(&pose(&c)))
                })
                .collect()
        };
        let (a, b) = (options(&m.connectors[0]), options(&m.connectors[1]));
        if a.len() == 1 && b.len() == 1 {
            continue;
        }
        // How far the contact would fall off the candidate face: the point of its (unbounded)
        // surface nearest the other entity, measured to the face's triangles. Only for a side
        // with a choice.
        let off_face = |c: &MateConnector, f: &connector::ConnectorFrame, other: [f64; 3], choice: bool| -> f64 {
            let (true, ConnectorAnchor::Surface { entity: EntityRef::Face(face), kind }, Some(s)) = (choice, c.anchor, solids.get(&c.instance)) else {
                return 0.0;
            };
            // Along a cylinder's axis the contact runs the length of the face: measure at the
            // face's middle, so only the way round counts.
            let other = match kind {
                connector::SurfaceKind::Cylinder { .. } => {
                    let d = [other[0] - f.origin[0], other[1] - f.origin[1], other[2] - f.origin[2]];
                    let t = d[0] * f.z[0] + d[1] * f.z[1] + d[2] * f.z[2];
                    [other[0] - f.z[0] * t, other[1] - f.z[1] * t, other[2] - f.z[2] * t]
                }
                _ => other,
            };
            let contact = connector::nearest_on_surface(kind, f, other);
            connector::face_distance(s, &face, pose(c).inverse().apply(contact))
        };
        let (choose_a, choose_b) = (a.len() > 1, b.len() > 1);
        let mut best: Option<(f64, MateConnector, MateConnector)> = None;
        for (ca, fa) in &a {
            for (cb, fb) in &b {
                let (Some(ka), Some(kb)) = (ca.surface_kind(), cb.surface_kind()) else { continue };
                if !solver::tangent_supported(ka, kb) {
                    continue;
                }
                let e = solver::tangent_error(ka, fa, kb, fb, m.flip);
                let score = off_face(ca, fa, fb.origin, choose_a) + off_face(cb, fb, fa.origin, choose_b) + 0.25 * e;
                if best.as_ref().is_none_or(|(bs, ..)| score < bs - 1e-9) {
                    best = Some((score, *ca, *cb));
                }
            }
        }
        if let Some((_, ca, cb)) = best {
            m.connectors = [ca, cb];
        }
    }
    out
}

/// The degrees of freedom each instance has left ([`solver::dof_counts`]).
pub fn instance_dofs(
    asm: &Assembly,
    solids: &std::collections::HashMap<InstanceId, Arc<Solid>>,
) -> std::collections::HashMap<InstanceId, u32> {
    let asm = resolve_local_connectors(asm, solids);
    let frame = |c: &connector::MateConnector| c.local_frame(solids.get(&c.instance).map(|s| &**s));
    solver::dof_counts(&asm, &frame)
}

/// The frame of an assembly's own connector (its owner instance's coordinates), resolved on
/// `solids`.
pub fn local_connector_frame(
    asm: &Assembly,
    id: connector::LocalConnectorId,
    solids: &std::collections::HashMap<InstanceId, Arc<Solid>>,
) -> Option<(InstanceId, connector::ConnectorFrame)> {
    let l = asm.local_connector(id)?;
    Some((l.connector.instance, l.connector.local_frame(solids.get(&l.connector.instance).map(|s| &**s))))
}

/// The mates' connectors on the assembly's own explicit connectors ([`connector::ConnectorAnchor::Local`])
/// replaced by fixed frames where those connectors are now (the mate's own flip, reorient and
/// edits on top); a connector whose explicit connector was deleted keeps its last frame.
pub fn resolve_local_connectors(asm: &Assembly, solids: &std::collections::HashMap<InstanceId, Arc<Solid>>) -> Assembly {
    use connector::{ConnectorAnchor, MateConnector};
    // P3B.8: a Replicate's copied mates are solved as mates.
    let mut out = replicate::expand(asm);
    let resolve = |c: &mut MateConnector| {
        if let ConnectorAnchor::Local { id } = c.anchor {
            if let Some((instance, frame)) = local_connector_frame(asm, id, solids) {
                c.instance = instance;
                c.frame = frame;
            }
            c.anchor = ConnectorAnchor::Frame;
        }
    };
    for f in &mut out.mates {
        if let mate::MateKind::Mate(m) = &mut f.kind {
            m.connectors.iter_mut().for_each(resolve);
            m.tabs.iter_mut().for_each(resolve);
        }
    }
    out
}

/// **Move to origin** (A3.3): the instance moved (not turned) so that the triad origin `at`
/// (assembly coordinates) lands on the assembly origin.
pub fn moved_to_origin(pose: &Pose, at: Vec3) -> Pose {
    pose.then(&Pose::translation([-at[0], -at[1], -at[2]]))
}

/// **Align with Z** / **Anti-align with Z** (A3.4): the instance turned about the triad origin
/// `at` by the smallest turn that brings the triad axis `axis` (assembly coordinates) onto +Z
/// (or −Z). When the axis points the other way already, the half turn goes about `fallback`
/// (the triad's X axis), so a triad whose Z is +Z anti-aligns by a half turn about its X.
pub fn aligned_with_z(pose: &Pose, at: Vec3, axis: Vec3, fallback: Vec3, anti: bool) -> Pose {
    let a = Vector3::from(axis).normalize();
    let b = Vector3::new(0.0, 0.0, if anti { -1.0 } else { 1.0 });
    let c = a.dot(&b).clamp(-1.0, 1.0);
    let turn = if c > 1.0 - 1e-12 {
        return *pose;
    } else if c < -1.0 + 1e-12 {
        let f = Vector3::from(fallback);
        // The fallback made perpendicular to the axis.
        let f = f - a * f.dot(&a);
        let f = if f.norm() < 1e-9 { a.cross(&Vector3::x()).try_normalize(1e-9).unwrap_or(Vector3::y()) } else { f.normalize() };
        Pose::rotation_about(at, [f.x, f.y, f.z], std::f64::consts::PI)
    } else {
        let k = a.cross(&b);
        Pose::rotation_about(at, [k.x, k.y, k.z], c.acos())
    };
    pose.then(&turn)
}

/// The instance turned by `angle` (radians) about the triad axis `axis` through `at` (the
/// triad ring's rotate 90° / 180°, A3.4).
pub fn rotated(pose: &Pose, at: Vec3, axis: Vec3, angle: f64) -> Pose {
    pose.then(&Pose::rotation_about(at, axis, angle))
}

/// Mass properties of instances (or of the whole assembly: every instance), in assembly
/// coordinates: see [`crate::parts::mass_report`] (sums, the mass-weighted centre, and the
/// inertia about it by the parallel-axis theorem).
pub fn mass_report(parts: &[Part], props: &[PartProps], instances: &[InstanceId]) -> Option<crate::parts::MassReport> {
    // A subassembly instance is all of its parts.
    let list: Vec<&Part> = parts.iter().filter(|p| instances.contains(&InstanceId::of_part(p.id))).collect();
    crate::parts::mass_report(&list, props)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-12, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn half_turns_are_exact_and_compose() {
        let r = Pose::rotation_about([0.0; 3], [1.0, 0.0, 0.0], std::f64::consts::PI);
        assert_eq!(r.rotation, [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
        let t = Pose::translation([0.0, -44.45, 0.0]);
        let both = t.then(&r);
        close(both.apply([0.0, 54.1, 28.9]), [0.0, -(54.1 - 44.45), -28.9]);
        close(both.inverse().apply(both.apply([1.0, 2.0, 3.0])), [1.0, 2.0, 3.0]);
        let q = Pose::rotation_about([1.0, 1.0, 0.0], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
        close(q.apply([2.0, 1.0, 5.0]), [1.0, 2.0, 5.0]);
    }
}
