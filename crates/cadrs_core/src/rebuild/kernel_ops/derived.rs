//! Rebuilding a Derived feature (P3G.4, DV3; the model is [`crate::derived`]). A child of
//! `rebuild::kernel_ops`, so it shares its helpers.
//!
//! - The source's features are rebuilt **inside** this rebuild ([`Rebuilder::sub_build`]), in the
//!   same kernel session and cache, so the source's bodies can be copied (bodies can't move
//!   between sessions) and a source built before is taken from the cache.
//! - Each location gives one copy, moved by the motion that takes the base frame (the source's
//!   origin, or its base mate connector) onto the location's frame.
//! - A copy's faces are renamed as a pattern instance's, under the Derived feature's id
//!   ([`crate::derived::derived_face`]); its parts, sketches, planes and mate connectors get ids
//!   of their own, so a derived duplicate of a studio never collides with the host's names.
//! - Derived part names are the source's (its renames too), made unique among the host's parts
//!   ("Part 1", "Part 1 (2)"), and the host's own numbering continues after them.

use super::*;
use cadrs_kernel::{Kernel, Motion};
use cadrs_sketch::{FeaturePlane, PlaneFrame, PlaneRef};
use nalgebra::{Matrix3, Vector3};

use crate::derived::{
    BoardCopy, DerivedBoard, DerivedFeature, DerivedOutput, DerivedPlacement, derived_entity,
    derived_face, derived_part,
};
use crate::ids::PartId;
use crate::pcb::board3d;
use cadrs_kernel::naming::FaceName;

fn v3(p: [f64; 3]) -> Vector3<f64> {
    Vector3::new(p[0], p[1], p[2])
}

fn arr(v: Vector3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The world frame (the origin with the Part Studio's axes).
const ORIGIN: PlaneFrame = PlaneFrame {
    origin: [0.0; 3],
    u: [1.0, 0.0, 0.0],
    v: [0.0, 1.0, 0.0],
};

/// The rigid motion that takes frame `base` onto frame `to` (origin onto origin, axes onto axes).
pub(crate) fn motion_between(base: &PlaneFrame, to: &PlaneFrame) -> Motion {
    let m = |f: &PlaneFrame| Matrix3::from_columns(&[v3(f.u), v3(f.v), v3(f.normal())]);
    let linear = m(to) * m(base).transpose();
    Motion {
        linear,
        translation: v3(to.origin) - linear * v3(base.origin),
    }
}

/// `f` moved by `m`.
pub(crate) fn moved_frame(m: &Motion, f: &PlaneFrame) -> PlaneFrame {
    PlaneFrame {
        origin: arr(m.linear * v3(f.origin) + m.translation),
        u: arr(m.linear * v3(f.u)),
        v: arr(m.linear * v3(f.v)),
    }
}

/// `base`, or `base (2)`, `base (3)`, … : the first not in `taken`.
fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base} ({n})"))
        .find(|n| !taken.contains(n))
        .expect("a free name")
}

impl Rebuilder {
    /// A Derived feature: the source rebuilt, then its selected parts, sketches, planes and mate
    /// connectors copied onto each location.
    pub(in crate::rebuild) fn derived(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        d: &DerivedFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = d.problem() {
            return Err(p.into());
        }
        if let Some(b) = &d.board {
            return self.derived_board(before, id, d, b, state);
        }
        let source = if d.source_name.is_empty() {
            "The source Part Studio".to_string()
        } else {
            d.source_name.clone()
        };
        if d.studio.is_empty() {
            return Err(format!("{source} has nothing to derive"));
        }
        let (sb, sstate) = self.sub_build(&d.studio)?;
        let base = match d.placement {
            DerivedPlacement::BaseOrigin => ORIGIN,
            DerivedPlacement::BaseConnector(Some(c)) => {
                crate::mate::frame(&c, &d.studio, &sb.parts, &sstate.connectors)
                    .map_err(|e| format!("The base mate connector: {e}"))?
            }
            DerivedPlacement::BaseConnector(None) => {
                return Err("Select the base mate connector".into());
            }
        };
        let locations: Vec<PlaneFrame> = if d.locations.is_empty() {
            vec![ORIGIN]
        } else {
            d.locations
                .iter()
                .map(|c| super::super::connector_frame(before, state, c))
                .collect::<Result<_, _>>()?
        };
        let motions: Vec<Motion> = locations.iter().map(|l| motion_between(&base, l)).collect();
        let several = motions.len() > 1;
        let mut next = (**state).clone();
        let mut out = DerivedOutput::default();
        let mut owned: Vec<BodyId> = Vec::new();
        let selected: Vec<PartState> = sstate
            .parts
            .iter()
            .filter(|p| d.includes_part(p.part.id))
            .cloned()
            .collect();
        let missing = if d.selection.all {
            0
        } else {
            d.selection
                .parts
                .iter()
                .filter(|p| sstate.part(**p).is_none())
                .count()
        };
        let mut taken: Vec<String> = next.parts.iter().map(|p| p.part.name.clone()).collect();
        let fail = |this: &mut Self, owned: &[BodyId], e: String| -> Result<Output, String> {
            for b in owned {
                this.kernel.release(*b);
            }
            Err(e)
        };
        let mut sketches: Vec<Feature> = Vec::new();
        for (k, m) in motions.iter().enumerate() {
            for sp in &selected {
                // A mesh part (a derived board's component in the source): its solid moved and
                // renamed as this copy's.
                let Some(body) = sp.body else {
                    let base_name = crate::parts::display_name(&sp.part, &d.props).to_string();
                    let name = unique(&base_name, &taken);
                    taken.push(name.clone());
                    let pid = d.part_of(id, sp.part.id, k);
                    let solid = board3d::renamed_by(&board3d::moved(&sp.part.solid, m), |n| {
                        d.face_of(id, &n, k)
                    });
                    next.parts.push(PartState {
                        part: Arc::new(Part {
                            id: pid,
                            feature: id,
                            name,
                            solid: Arc::new(solid),
                            mass: None,
                            features: vec![id],
                            source: None,
                            derived: None,
                            ..(*sp.part).clone()
                        }),
                        body: None,
                        names: Arc::default(),
                    });
                    out.parts.push(pid);
                    continue;
                };
                let (b, h) = match self.kernel.transform_motion(body, m) {
                    Ok(r) => (r.bodies[0], r.history),
                    Err(e) => {
                        return fail(
                            self,
                            &owned,
                            format!("Deriving {} failed: {e}", sp.part.name),
                        );
                    }
                };
                owned.push(b);
                let names = match self.instance_names(b, &h, &sp.names, id.0, d.instance(k)) {
                    Ok(n) => n,
                    Err(e) => return fail(self, &owned, e),
                };
                let solid = match crate::brep::solid_of(
                    &self.kernel,
                    b,
                    &names,
                    &state.geoms,
                    Some(id.0),
                ) {
                    Ok(s) => s,
                    Err(e) => return fail(self, &owned, e),
                };
                let mass = match self.kernel.mass_properties(b) {
                    Ok(m) => m,
                    Err(e) => return fail(self, &owned, e.to_string()),
                };
                // The source's name (its rename, if it has one), unique in the host.
                let base_name = crate::parts::display_name(&sp.part, &d.props).to_string();
                let name = unique(&base_name, &taken);
                taken.push(name.clone());
                if let Some(n) = name
                    .strip_prefix("Part ")
                    .and_then(|x| x.parse::<u32>().ok())
                {
                    next.next_part = next.next_part.max(n);
                }
                if let Some(n) = name
                    .strip_prefix("Surface ")
                    .and_then(|x| x.parse::<u32>().ok())
                {
                    next.next_surface = next.next_surface.max(n);
                }
                let pid = d.part_of(id, sp.part.id, k);
                next.parts.push(PartState {
                    part: Arc::new(Part {
                        id: pid,
                        feature: id,
                        name,
                        kind: sp.part.kind,
                        palette: sp.part.palette,
                        solid: Arc::new(solid),
                        mass: Some(mass),
                        features: vec![id],
                        source: None,
                        // The source part's settings are copied into the host's (`crate::derived::refresh`).
                        derived: None,
                    }),
                    body: Some(b),
                    names: Arc::new(names),
                });
                out.parts.push(pid);
            }
            let label = |n: &str| {
                if several {
                    format!("{n} ({})", k + 1)
                } else {
                    n.to_string()
                }
            };
            // Sketches (the source's own and those its Derived features brought in), placed.
            for f in d.studio.iter().chain(sb.derived_sketches.iter()) {
                let Some(sk) = f.sketch() else { continue };
                let Some(plane) = sk.plane else { continue };
                let nested = d.selection.all && sb.derived_sketches.iter().any(|x| x.id == f.id);
                if !(d.includes_sketch(f.id) || nested) {
                    continue;
                }
                let sid = derived_entity(id, f.id, k);
                let frame = moved_frame(m, &plane.frame());
                let mut copy = sk.clone();
                copy.plane = Some(PlaneRef::Feature(FeaturePlane::new(id.0, frame)));
                let name = label(&f.name);
                sketches.push(Feature {
                    id: sid,
                    name: name.clone(),
                    kind: FeatureKind::Sketch(copy),
                    suppress_by: None,
                });
                out.sketches.push((sid, name));
            }
            // Planes.
            for f in d
                .studio
                .iter()
                .filter(|f| matches!(f.kind, FeatureKind::Plane(_)) && d.includes_plane(f.id))
            {
                let Some(frame) = sstate.planes.get(&f.id) else {
                    continue;
                };
                let pid = derived_entity(id, f.id, k);
                next.planes.insert(pid, moved_frame(m, frame));
                out.planes.push((pid, label(&f.name)));
            }
            // Mate connectors.
            for f in d
                .studio
                .iter()
                .filter(|f| matches!(f.kind, FeatureKind::MateConnector(_)))
            {
                let owner = sstate.connector_owners.get(&f.id).copied();
                if !d.includes_connector(f.id, owner) {
                    continue;
                }
                let Some(frame) = sstate.connectors.get(&f.id) else {
                    continue;
                };
                let cid = derived_entity(id, f.id, k);
                next.connectors.insert(cid, moved_frame(m, frame));
                if let Some(o) = owner.filter(|o| d.includes_part(*o) && sstate.part(*o).is_some())
                {
                    next.connector_owners.insert(cid, d.part_of(id, o, k));
                }
                out.connectors.push((cid, label(&f.name)));
            }
        }
        // A derived board in the source comes across with its parts: each copy's board frame
        // moved, its parts and footprints under this copy's ids (the layout view and the edit
        // follow the source's Derived feature).
        for (k, m) in motions.iter().enumerate() {
            for src in sstate.derived.values() {
                for bc in src
                    .boards
                    .iter()
                    .filter(|bc| d.includes_part(bc.part) && sstate.part(bc.part).is_some())
                {
                    let f = moved_frame(m, &bc.frame);
                    out.boards.push(BoardCopy {
                        part: d.part_of(id, bc.part, k),
                        frame: f,
                        thickness: bc.thickness,
                    });
                }
                for (p, fp) in src
                    .footprints
                    .iter()
                    .filter(|(p, _)| d.includes_part(*p) && sstate.part(*p).is_some())
                {
                    out.footprints.push((d.part_of(id, *p, k), *fp));
                }
                // A component a Transform there moved apart from its board: the same move, seen
                // from this copy.
                for (p, e) in src
                    .moved
                    .iter()
                    .filter(|(p, _)| d.includes_part(*p) && sstate.part(*p).is_some())
                {
                    out.moved
                        .push((d.part_of(id, *p, k), m.inverse().then(e).then(m)));
                }
            }
        }
        // SM18.3 (P3I.8): sheet metal comes across with its parts. A copy at the source's own
        // place stays active sheet metal (its definition as it is: features after the Derived
        // feature can change it, the table and flat view show it); a copy placed elsewhere keeps
        // its flat and table, moved, as finished sheet metal.
        let mut contexts = (*next.sheet_metal).clone();
        for (k, m) in motions.iter().enumerate() {
            let identity =
                (m.linear - Matrix3::identity()).norm() < 1e-12 && m.translation.norm() < 1e-9;
            for c in sstate.sheet_metal.iter() {
                let parts: Vec<(PartId, Vec<cadrs_sheetmetal::WallId>)> = c
                    .parts
                    .iter()
                    .filter(|(p, _)| d.includes_part(*p) && sstate.part(*p).is_some())
                    .map(|(p, w)| (d.part_of(id, *p, k), w.clone()))
                    .collect();
                if parts.is_empty() {
                    continue;
                }
                let mut x = c.clone();
                x.feature = derived_entity(id, c.feature, k);
                let base = d
                    .studio
                    .iter()
                    .find(|f| f.id == c.feature)
                    .map_or_else(|| c.name.clone(), |f| f.name.clone());
                x.name = if several {
                    format!("{base} ({})", k + 1)
                } else {
                    base
                };
                x.parts = parts;
                x.editors = vec![id];
                if !identity {
                    x.model = cadrs_sheetmetal::model_edit::moved_model(
                        &c.model,
                        m.linear,
                        m.translation,
                    );
                    x.def = None;
                    x.active = false;
                    x.forms.clear();
                }
                contexts.retain(|y| y.feature != x.feature);
                contexts.push(x);
            }
        }
        next.sheet_metal = Arc::new(contexts);
        if out.parts.is_empty()
            && out.sketches.is_empty()
            && out.planes.is_empty()
            && out.connectors.is_empty()
        {
            return fail(self, &owned, format!("{source} has nothing to derive"));
        }
        let mut all = (*next.derived).clone();
        all.insert(id, out);
        next.derived = Arc::new(all);
        if !sketches.is_empty() {
            let mut s = (*next.derived_sketches).clone();
            s.extend(sketches);
            next.derived_sketches = Arc::new(s);
        }
        let warning = if missing > 0 {
            Some(if missing == 1 {
                format!("1 derived part no longer exists in {source}")
            } else {
                format!("{missing} derived parts no longer exist in {source}")
            })
        } else if !sb.errors.is_empty() {
            Some(format!("{source} has features that fail"))
        } else {
            None
        };
        Ok(Output {
            state: Arc::new(next),
            error: None,
            warning,
            contacts: None,
            owned,
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        })
    }

    /// A Derived feature of a PCB Studio board: the board's
    /// solid and a mesh part per component, made in the board frame once and copied onto each
    /// location, the board's chosen point ([`board3d::BoardAnchor`]) and face on it.
    fn derived_board(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        d: &DerivedFeature,
        b: &DerivedBoard,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        let layout = &b.layout;
        let o = &b.options;
        if !matches!(d.placement, DerivedPlacement::BaseOrigin) {
            return Err("A board is placed by its board point (Base origin): pick the point, a footprint say, in the dialog".into());
        }
        let locations: Vec<PlaneFrame> = if let Some(s) = b.on_sketch {
            // A board made from a sketch lies on the sketch's plane.
            let plane = before.iter().find(|f| f.id == s).and_then(|f| f.sketch()).and_then(|sk| sk.plane).ok_or("The board lies on its outline's sketch, which must come before this feature (it was moved below it, or deleted)")?;
            vec![plane.frame()]
        } else if d.locations.is_empty() {
            vec![ORIGIN]
        } else {
            d.locations
                .iter()
                .map(|c| super::super::connector_frame(before, state, c))
                .collect::<Result<Vec<_>, _>>()?
        };
        let (body, curves, mut warnings) = self.board_body_cached(layout)?;
        if let Some(p) = &b.outline_problem {
            warnings.push(format!("The outline no longer follows its sketch: {p}"));
        }
        if let Some(p) = &b.source_problem {
            warnings.push(p.clone());
        }
        let (components, w) = board3d::components(layout, o);
        warnings.extend(w);
        let owner = cadrs_kernel::naming::stable_hash(id.0.as_bytes());
        let skins = if o.skins {
            board3d::skins(layout, b.hash, owner)
        } else {
            Vec::new()
        };
        let thickness = board3d::thickness(layout);
        // The board's own ids: its part and faces stand for the nil footprint.
        let board_src = PartId::new(FeatureId(uuid::Uuid::nil()), 0);
        let base = PlaneFrame {
            origin: board3d::anchor_point(layout, o),
            u: [1.0, 0.0, 0.0],
            v: [0.0, 1.0, 0.0],
        };
        let several = locations.len() > 1;
        let mut next = (**state).clone();
        let mut out = DerivedOutput::default();
        let mut owned: Vec<BodyId> = Vec::new();
        let mut taken: Vec<String> = next.parts.iter().map(|p| p.part.name.clone()).collect();
        let mask = cadrs_eda::render::made::MASK;
        let green = crate::appearance::Appearance::rgb(mask[0], mask[1], mask[2]);
        let mut palette = next.next_part;
        let mut failure = None;
        // Each earlier copy's board and components; each copy's components and frame.
        let mut earlier: Vec<(PartId, Arc<crate::solid::Solid>)> = Vec::new();
        type Placed = Vec<(PartId, Arc<crate::solid::Solid>)>;
        let mut copies: Vec<(Placed, PlaneFrame)> = Vec::new();
        for (k, loc) in locations.iter().enumerate() {
            let m = motion_between(&base, loc);
            let label = |n: &str| {
                if several {
                    format!("{n} ({})", k + 1)
                } else {
                    n.to_string()
                }
            };
            // The board: its faces named as the board's own (under the nil op), then as this
            // copy's, under the Derived feature (as a pattern instance's).
            let b2 = match self.kernel.transform_motion(body, &m) {
                Ok(r) => r.bodies[0],
                Err(e) => {
                    failure = Some(format!("Placing {} failed: {e}", b.board_name));
                    break;
                }
            };
            owned.push(b2);
            let made = (|| -> Result<(cadrs_kernel::naming::BodyNames, crate::solid::Solid, cadrs_kernel::MassProperties), String> {
                let own = board3d::board_face_names(&self.kernel, b2, uuid::Uuid::nil(), &m, &curves)?;
                let faces = own.iter().map(|n| derived_face(id, n, k)).collect();
                let names = self.linked_names(b2, faces)?;
                let solid = crate::brep::solid_of(&self.kernel, b2, &names, &state.geoms, Some(id.0))?;
                let mass = self.kernel.mass_properties(b2).map_err(|e| e.to_string())?;
                Ok((names, solid, mass))
            })();
            let (names, mut solid, mass) = match made {
                Ok(x) => x,
                Err(e) => {
                    failure = Some(format!("Placing {} failed: {e}", b.board_name));
                    break;
                }
            };
            solid.looks = solid.faces.iter().map(|f| (f.name, green)).collect();
            solid.images = skins
                .iter()
                .map(|i| i.mapped(|p| board3d::apply(&m, p)))
                .collect();
            let name = unique(&label(&b.board_name), &taken);
            taken.push(name.clone());
            let pid = derived_part(id, board_src, k);
            next.parts.push(PartState {
                part: Arc::new(Part {
                    id: pid,
                    feature: id,
                    name,
                    kind: PartKind::Solid,
                    palette,
                    solid: Arc::new(solid),
                    mass: Some(mass),
                    features: vec![id],
                    source: None,
                    derived: None,
                }),
                body: Some(b2),
                names: Arc::new(names),
            });
            palette += 1;
            out.parts.push(pid);
            let o3 = board3d::apply(&m, [0.0; 3]);
            let (u, v) = (m.linear * Vector3::x(), m.linear * Vector3::y());
            out.boards.push(BoardCopy {
                part: pid,
                frame: PlaneFrame {
                    origin: o3,
                    u: [u.x, u.y, u.z],
                    v: [v.x, v.y, v.z],
                },
                thickness,
            });
            // The components (mesh parts: no kernel body); those running into a part before
            // the feature, or into an earlier copy (the board placed on several locations too
            // close together), are reported (on their meshes, §7.3).
            let before_parts: Vec<(PartId, Arc<crate::solid::Solid>)> = state
                .parts
                .iter()
                .map(|p| (p.part.id, p.part.solid.clone()))
                .chain(earlier.iter().cloned())
                .collect();
            if let Some(p) = next.parts.last() {
                earlier.push((p.part.id, p.part.solid.clone()));
            }
            let frame = PlaneFrame {
                origin: board3d::apply(&m, [0.0; 3]),
                u: {
                    let u = m.linear * Vector3::x();
                    [u.x, u.y, u.z]
                },
                v: {
                    let v = m.linear * Vector3::y();
                    [v.x, v.y, v.z]
                },
            };
            let mut placed: Vec<(PartId, Arc<crate::solid::Solid>)> = Vec::new();
            for c in &components {
                let place = c.motion.then(&m);
                let (solid, mass) = self.placed_component(id, k, c, &place);
                let name = unique(&label(&c.name), &taken);
                taken.push(name.clone());
                let pid = derived_part(id, PartId::new(FeatureId(c.footprint), 0), k);
                placed.push((pid, solid.clone()));
                next.parts.push(PartState {
                    part: Arc::new(Part {
                        id: pid,
                        feature: id,
                        name,
                        kind: PartKind::Solid,
                        palette,
                        solid,
                        mass,
                        features: vec![id],
                        source: None,
                        derived: None,
                    }),
                    body: None,
                    names: Arc::default(),
                });
                palette += 1;
                out.parts.push(pid);
                out.footprints.push((pid, c.footprint));
            }
            earlier.extend(placed.iter().cloned());
            copies.push((placed.clone(), frame));
            let comps: Vec<(PartId, &Arc<crate::solid::Solid>)> =
                placed.iter().map(|(n, s)| (*n, s)).collect();
            let others: Vec<(PartId, &Arc<crate::solid::Solid>)> =
                before_parts.iter().map(|(n, s)| (*n, s)).collect();
            out.collisions
                .extend(crate::pcb::fit::collisions(&comps, &others, &frame));
        }
        // A copy's components into a later copy's board (the other way round, and component
        // against component, the loop checked).
        for (k, (comps, frame)) in copies.iter().enumerate() {
            let later: Vec<(PartId, &Arc<crate::solid::Solid>)> = out.boards[k + 1..]
                .iter()
                .filter_map(|b| {
                    next.parts
                        .iter()
                        .find(|p| p.part.id == b.part)
                        .map(|p| (b.part, &p.part.solid))
                })
                .collect();
            if later.is_empty() {
                continue;
            }
            let c: Vec<(PartId, &Arc<crate::solid::Solid>)> =
                comps.iter().map(|(p, s)| (*p, s)).collect();
            out.collisions
                .extend(crate::pcb::fit::collisions(&c, &later, frame));
        }
        // (The board's body stays: it is kept for the next rebuild, `board_body_cached`.)
        if let Some(e) = failure {
            for b in owned {
                self.kernel.release(b);
            }
            return Err(e);
        }
        // The components in the way of a part before the feature (by their default names here;
        // the app names them as its Parts list does).
        let name = |p: PartId| {
            next.parts
                .iter()
                .find(|x| x.part.id == p)
                .map(|x| x.part.name.clone())
                .unwrap_or_default()
        };
        for c in &out.collisions {
            warnings.push(crate::pcb::fit::collision_text(
                &name(c.component),
                &name(c.other),
                c,
            ));
        }
        let mut all = (*next.derived).clone();
        all.insert(id, out);
        next.derived = Arc::new(all);
        Ok(Output {
            state: Arc::new(next),
            error: None,
            warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
            contacts: None,
            owned,
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        })
    }

    /// The board's solid in the board frame, made once per outline, holes and thickness: a
    /// footprint moved on the layout doesn't make the kernel extrude the board again.
    /// The body stays the cache's: callers copy it.
    fn board_body_cached(
        &mut self,
        layout: &cadrs_eda::board::Board,
    ) -> Result<(BodyId, Vec<board3d::TaggedCurve>, Vec<String>), String> {
        let plan = board3d::board_profile(layout)?;
        // (The curves' tags too: the same holes made by other pads name their walls otherwise.)
        let tags: Vec<u64> = plan.curves.iter().map(|c| c.tag).collect();
        let key = cadrs_kernel::naming::stable_hash(
            format!("{:?}/{tags:?}/{}", plan.regions, board3d::thickness(layout)).as_bytes(),
        );
        if let Some(i) = self.board_bodies.iter().position(|b| b.0 == key) {
            let hit = self.board_bodies.remove(i);
            let out = (hit.1, hit.2.clone(), hit.3.clone());
            self.board_bodies.insert(0, hit);
            return Ok(out);
        }
        let (body, curves, warnings) = board3d::board_body(&mut self.kernel, layout)?;
        self.board_bodies
            .insert(0, (key, body, curves.clone(), warnings.clone()));
        while self.board_bodies.len() > 8 {
            if let Some((_, b, ..)) = self.board_bodies.pop() {
                self.kernel.release(b);
            }
        }
        Ok((body, curves, warnings))
    }

    /// A component's mesh part placed by `place` (footprint frame → Part Studio), its faces
    /// named for copy `k` of the Derived feature `id`: kept while the component, its place and
    /// its model are the same, so a rebuild after one footprint moved moves only that one.
    /// (With its volume properties, [`crate::pcb::mesh_solid::mesh_mass`].)
    fn placed_component(
        &mut self,
        id: FeatureId,
        k: usize,
        c: &board3d::Component,
        place: &cadrs_kernel::Motion,
    ) -> (
        Arc<crate::solid::Solid>,
        Option<cadrs_kernel::MassProperties>,
    ) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (id, k, c.footprint).hash(&mut h);
        place
            .linear
            .iter()
            .chain(place.translation.iter())
            .for_each(|x| x.to_bits().hash(&mut h));
        let key = h.finish();
        if let Some((src, made, mass)) = self.placed_components.get(&key)
            && Arc::ptr_eq(src, &c.solid)
        {
            return (made.clone(), *mass);
        }
        let made = Arc::new(board3d::renamed_by(&board3d::moved(&c.solid, place), |n| {
            derived_face(
                id,
                &FaceName {
                    op: c.footprint,
                    ..n
                },
                k,
            )
        }));
        let mass = crate::pcb::mesh_solid::mesh_mass(&made);
        if self.placed_components.len() > 4096 {
            self.placed_components.clear();
        }
        self.placed_components
            .insert(key, (c.solid.clone(), made.clone(), mass));
        (made, mass)
    }
}
