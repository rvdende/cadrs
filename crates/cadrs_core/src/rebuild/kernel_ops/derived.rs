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

use crate::derived::{DerivedFeature, DerivedOutput, DerivedPlacement, derived_entity};
use crate::ids::PartId;

fn v3(p: [f64; 3]) -> Vector3<f64> {
    Vector3::new(p[0], p[1], p[2])
}

fn arr(v: Vector3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The world frame (the origin with the Part Studio's axes).
const ORIGIN: PlaneFrame = PlaneFrame { origin: [0.0; 3], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };

/// The rigid motion that takes frame `base` onto frame `to` (origin onto origin, axes onto axes).
pub(crate) fn motion_between(base: &PlaneFrame, to: &PlaneFrame) -> Motion {
    let m = |f: &PlaneFrame| Matrix3::from_columns(&[v3(f.u), v3(f.v), v3(f.normal())]);
    let linear = m(to) * m(base).transpose();
    Motion { linear, translation: v3(to.origin) - linear * v3(base.origin) }
}

/// `f` moved by `m`.
pub(crate) fn moved_frame(m: &Motion, f: &PlaneFrame) -> PlaneFrame {
    PlaneFrame { origin: arr(m.linear * v3(f.origin) + m.translation), u: arr(m.linear * v3(f.u)), v: arr(m.linear * v3(f.v)) }
}

/// `base`, or `base (2)`, `base (3)`, … : the first not in `taken`.
fn unique(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|n| !taken.contains(n)).expect("a free name")
}

impl Rebuilder {
    /// A Derived feature: the source rebuilt, then its selected parts, sketches, planes and mate
    /// connectors copied onto each location.
    pub(in crate::rebuild) fn derived(&mut self, before: &[Feature], id: FeatureId, d: &DerivedFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = d.problem() {
            return Err(p.into());
        }
        let source = if d.source_name.is_empty() { "The source Part Studio".to_string() } else { d.source_name.clone() };
        if d.studio.is_empty() {
            return Err(format!("{source} has nothing to derive"));
        }
        let (sb, sstate) = self.sub_build(&d.studio)?;
        let base = match d.placement {
            DerivedPlacement::BaseOrigin => ORIGIN,
            DerivedPlacement::BaseConnector(Some(c)) => {
                crate::mate::frame(&c, &d.studio, &sb.parts, &sstate.connectors).map_err(|e| format!("The base mate connector: {e}"))?
            }
            DerivedPlacement::BaseConnector(None) => return Err("Select the base mate connector".into()),
        };
        let locations: Vec<PlaneFrame> = if d.locations.is_empty() {
            vec![ORIGIN]
        } else {
            d.locations.iter().map(|c| super::super::connector_frame(before, state, c)).collect::<Result<_, _>>()?
        };
        let motions: Vec<Motion> = locations.iter().map(|l| motion_between(&base, l)).collect();
        let several = motions.len() > 1;
        let mut next = (**state).clone();
        let mut out = DerivedOutput::default();
        let mut owned: Vec<BodyId> = Vec::new();
        let selected: Vec<PartState> = sstate.parts.iter().filter(|p| d.includes_part(p.part.id)).cloned().collect();
        let missing = if d.selection.all { 0 } else { d.selection.parts.iter().filter(|p| sstate.part(**p).is_none()).count() };
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
                let Some(body) = sp.body else { continue };
                let (b, h) = match self.kernel.transform_motion(body, m) {
                    Ok(r) => (r.bodies[0], r.history),
                    Err(e) => return fail(self, &owned, format!("Deriving {} failed: {e}", sp.part.name)),
                };
                owned.push(b);
                let names = match self.instance_names(b, &h, &sp.names, id.0, d.instance(k)) {
                    Ok(n) => n,
                    Err(e) => return fail(self, &owned, e),
                };
                let solid = match crate::brep::solid_of(&self.kernel, b, &names, &state.geoms, Some(id.0)) {
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
                if let Some(n) = name.strip_prefix("Part ").and_then(|x| x.parse::<u32>().ok()) {
                    next.next_part = next.next_part.max(n);
                }
                if let Some(n) = name.strip_prefix("Surface ").and_then(|x| x.parse::<u32>().ok()) {
                    next.next_surface = next.next_surface.max(n);
                }
                let pid = d.part_of(id, sp.part.id, k);
                next.parts.push(PartState {
                    part: Part {
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
                    },
                    body: Some(b),
                    names: Arc::new(names),
                });
                out.parts.push(pid);
            }
            let label = |n: &str| if several { format!("{n} ({})", k + 1) } else { n.to_string() };
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
                sketches.push(Feature { id: sid, name: name.clone(), kind: FeatureKind::Sketch(copy) });
                out.sketches.push((sid, name));
            }
            // Planes.
            for f in d.studio.iter().filter(|f| matches!(f.kind, FeatureKind::Plane(_)) && d.includes_plane(f.id)) {
                let Some(frame) = sstate.planes.get(&f.id) else { continue };
                let pid = derived_entity(id, f.id, k);
                next.planes.insert(pid, moved_frame(m, frame));
                out.planes.push((pid, label(&f.name)));
            }
            // Mate connectors.
            for f in d.studio.iter().filter(|f| matches!(f.kind, FeatureKind::MateConnector(_))) {
                let owner = sstate.connector_owners.get(&f.id).copied();
                if !d.includes_connector(f.id, owner) {
                    continue;
                }
                let Some(frame) = sstate.connectors.get(&f.id) else { continue };
                let cid = derived_entity(id, f.id, k);
                next.connectors.insert(cid, moved_frame(m, frame));
                if let Some(o) = owner.filter(|o| d.includes_part(*o) && sstate.part(*o).is_some()) {
                    next.connector_owners.insert(cid, d.part_of(id, o, k));
                }
                out.connectors.push((cid, label(&f.name)));
            }
        }
        // SM18.3 (P3I.8): sheet metal comes across with its parts. A copy at the source's own
        // place stays active sheet metal (its definition as it is: features after the Derived
        // feature can change it, the table and flat view show it); a copy placed elsewhere keeps
        // its flat and table, moved, as finished sheet metal.
        let mut contexts = (*next.sheet_metal).clone();
        for (k, m) in motions.iter().enumerate() {
            let identity = (m.linear - Matrix3::identity()).norm() < 1e-12 && m.translation.norm() < 1e-9;
            for c in sstate.sheet_metal.iter() {
                let parts: Vec<(PartId, Vec<cadrs_sheetmetal::WallId>)> =
                    c.parts.iter().filter(|(p, _)| d.includes_part(*p) && sstate.part(*p).is_some()).map(|(p, w)| (d.part_of(id, *p, k), w.clone())).collect();
                if parts.is_empty() {
                    continue;
                }
                let mut x = c.clone();
                x.feature = derived_entity(id, c.feature, k);
                let base = d.studio.iter().find(|f| f.id == c.feature).map_or_else(|| c.name.clone(), |f| f.name.clone());
                x.name = if several { format!("{base} ({})", k + 1) } else { base };
                x.parts = parts;
                x.editors = vec![id];
                if !identity {
                    x.model = cadrs_sheetmetal::model_edit::moved_model(&c.model, m.linear, m.translation);
                    x.def = None;
                    x.active = false;
                    x.forms.clear();
                }
                contexts.retain(|y| y.feature != x.feature);
                contexts.push(x);
            }
        }
        next.sheet_metal = Arc::new(contexts);
        if out.parts.is_empty() && out.sketches.is_empty() && out.planes.is_empty() && out.connectors.is_empty() {
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
            Some(if missing == 1 { format!("1 derived part no longer exists in {source}") } else { format!("{missing} derived parts no longer exist in {source}") })
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
}
