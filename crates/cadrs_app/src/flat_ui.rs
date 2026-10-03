//! Modelling in the flat (P3I.6, SM14; `reference/onshape/sheetmetal/raw/`
//! `help-sheet_metal_table.txt`, "Sketching on a flat pattern" and "Extruding a flat pattern
//! sketch"; lesson `14-modeling-in-the-flat-view`):
//!
//! - **New sketch** on the flat pattern ([`begin_flat_sketch`]): a sketch whose plane is the
//!   model's flat pattern plane (`cadrs_core::sheetmetal_flat`), so its coordinates are the
//!   flat's. It is drawn and edited **in the flat view** (lesson t0052–t0060): the panel opens
//!   on the model's flat, seen from the top, and the sketch tools, snapping, picking, glyphs and
//!   dimensions go through the flat view's camera (`crate::sketch_tools::SketchArea`). Nothing
//!   of it is drawn in the 3D view. The flat's bend centre lines are in the sketch from the
//!   start as used construction lines (`Link::FlatLine`), so the sketch can be dimensioned and
//!   constrained against them; the flat's outline and cut-out edges are snapped to and used when
//!   touched ([`flat_imprints`]). Both follow the flat when it changes.
//! - Sketch regions on the flat are picked in the flat view ([`region_at`]) and the picked ones
//!   (selected, or in the flat Extrude) are orange there and, wrapped onto the folded part's
//!   walls, in the 3D view ([`draw_picked_regions`]).
//! - The **abbreviated Extrude** (`help/feature-tools/extrude2_abbrev_dialogbox.png`): **Add |
//!   Remove** and *Faces and sketch regions to extrude*, in the applied-feature dialogs. Extrude
//!   (the toolbar button or Shift+E) with regions of a flat-pattern sketch selected opens it
//!   instead of the 3D Extrude ([`extrude_redirect`]). A new one starts on Remove when all its
//!   regions lie on material, else on Add.
//!
//! Names: `flat-extrude-dialog`, `flat-extrude-operation-0` (Add) / `-1` (Remove),
//! `flat-extrude-regions-field`.

use bevy::prelude::*;
use cadrs_core::document::{BodyType, RegionRef};
use cadrs_core::sheetmetal_flat::{FlatExtrudeFeature, flat_lines, flat_plane_id, sketch_target};
use cadrs_sketch::Link;
use cadrs_sketch::projection::Projected;
use cadrs_core::{Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::TabStrip;

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind};
use crate::applied_dialog::{Role, body_column, list};
use crate::parts::PartCache;
use crate::viewport::{Pick, Selection};

fn region_of(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRef> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRef::new(sketch, &r))
}

fn features(world: &World) -> Option<Vec<Feature>> {
    Some(world.get_resource::<ActiveDocument>()?.active_element()?.features().to_vec())
}

/// Whether every picked region lies on the flat's material (a new flat extrude then starts on
/// Remove; a region reaching off the sheet starts on Add).
fn all_on_material(features: &[Feature], cache: &PartCache, picks: &[(FeatureId, u32)]) -> bool {
    use cadrs_sheetmetal::poly::{P2, Polygon, overlap_area};
    let build = cadrs_core::rebuild::build(features);
    !picks.is_empty()
        && picks.iter().all(|(s, i)| {
            let Some((model, part)) = sketch_target(features, *s) else { return false };
            let Some(r) = cache.sketch_regions(*s).and_then(|r| r.regions.get(*i as usize)) else { return false };
            let ring = |l: &[cadrs_sketch::Vec2]| l.iter().map(|q| P2::new(q.x, q.y)).collect::<Vec<_>>();
            let poly = Polygon::with_holes(ring(&r.outer), r.holes.iter().map(|h| ring(h)).collect());
            let Some(flat) = build.sheet_metal.iter().rev().find(|c| c.feature == model).and_then(|c| c.flat.parts.get(part)) else { return false };
            let on: f64 = flat.outline.iter().map(|o| overlap_area(&poly, o)).sum();
            on >= poly.area() * (1.0 - 1e-6)
        })
}

/// A new flat extrude from the selection: its regions (and whole sketches).
pub fn initial(world: &World, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let features = features(world)?;
    let cache = world.resource::<PartCache>();
    let mut x = FlatExtrudeFeature::default();
    let mut regions = Vec::new();
    for p in picked {
        match *p {
            Pick::Region(s, i) if sketch_target(&features, s).is_some() => {
                regions.push((s, i));
                x.regions.extend(region_of(cache, s, i));
            }
            Pick::Feature(f) if sketch_target(&features, f).is_some() => {
                x.sketches.push(f);
                let n = cache.sketch_regions(f).map_or(0, |r| r.regions.len());
                regions.extend((0..n as u32).map(|i| (f, i)));
            }
            _ => {}
        }
    }
    x.remove = all_on_material(&features, cache, &regions);
    Some(("Extrude", FeatureKind::FlatExtrude(x), AppliedField::FlatRegions))
}

/// Extrude with regions or sketches of a flat-pattern sketch selected: the abbreviated flat
/// extrude instead (true if it started).
pub fn extrude_redirect(world: &mut World) -> bool {
    let Some(features) = features(world) else { return false };
    let flat = world.resource::<Selection>().0.iter().any(|p| match *p {
        Pick::Region(s, _) | Pick::Feature(s) => sketch_target(&features, s).is_some(),
        _ => false,
    });
    if flat {
        crate::applied::begin(world, AppliedKind::FlatExtrude);
    }
    flat
}

/// A pick into the regions field. False if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: AppliedField, pick: Pick) -> bool {
    let (FeatureKind::FlatExtrude(x), AppliedField::FlatRegions) = (kind, field) else { return false };
    let Some(features) = features(world) else { return false };
    let cache = world.resource::<PartCache>();
    match pick {
        Pick::Region(s, i) => {
            let Some(r) = region_of(cache, s, i) else { return false };
            match x.regions.iter().position(|y| y.sketch == r.sketch && y.curves == r.curves) {
                Some(k) => {
                    x.regions.remove(k);
                }
                None => x.regions.push(r),
            }
        }
        Pick::Feature(f) if features.iter().any(|g| g.id == f && g.sketch().is_some()) => match x.sketches.iter().position(|s| *s == f) {
            Some(k) => {
                x.sketches.remove(k);
            }
            None => x.sketches.push(f),
        },
        _ => return false,
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The dialog (called at the end of the applied dialogs' chain, from `crate::surfacing_ui`)

pub(crate) fn name(kind: &FeatureKind) -> Option<&'static str> {
    matches!(kind, FeatureKind::FlatExtrude(_)).then_some("flat-extrude")
}

pub(crate) fn layout(kind: &FeatureKind) -> Option<String> {
    match kind {
        FeatureKind::FlatExtrude(x) => Some(format!("flat-extrude {}", x.remove)),
        _ => None,
    }
}

pub(crate) fn items(features: &[Feature], kind: &FeatureKind, role: Role) -> Option<Vec<String>> {
    match (kind, role) {
        (FeatureKind::FlatExtrude(x), Role::FlatRegions) => {
            let name = |id: FeatureId| features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone());
            let mut v: Vec<String> = x.regions.iter().map(|r| format!("Face of {}", name(r.sketch))).collect();
            v.extend(x.sketches.iter().map(|s| crate::extrude_dialog::whole_sketch_label(features, *s, BodyType::Solid)));
            Some(v)
        }
        _ => None,
    }
}

pub(crate) fn list_field(role: Role) -> Option<AppliedField> {
    (role == Role::FlatRegions).then_some(AppliedField::FlatRegions)
}

/// Add | Remove, then the regions.
pub(crate) fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items_of: &dyn Fn(Role) -> Vec<String>) {
    let FeatureKind::FlatExtrude(x) = kind else { return };
    b.spawn((Role::OpTab, TabStrip::new("flat-extrude-operation").compact().tab("Add").tab("Remove").selected(usize::from(x.remove)).build(t)));
    body_column(b, |b| {
        list(b, t, "flat-extrude-regions-field", "Faces and sketch regions to extrude", Role::FlatRegions, items_of(Role::FlatRegions), field == AppliedField::FlatRegions);
    });
}

pub(crate) fn tab(k: &mut FeatureKind, role: Role, i: usize) {
    if let (FeatureKind::FlatExtrude(x), Role::OpTab) = (k, role) {
        x.remove = i == 1;
    }
}

pub(crate) fn remove(k: &mut FeatureKind, role: Role, i: usize) {
    let (FeatureKind::FlatExtrude(x), Role::FlatRegions) = (k, role) else { return };
    if i < x.regions.len() {
        x.regions.remove(i);
    } else if i - x.regions.len() < x.sketches.len() {
        x.sketches.remove(i - x.regions.len());
    }
}

// ---------------------------------------------------------------------------------------------
// New sketch on the flat pattern

/// The plane of a model's flat-pattern part as the last rebuild has it.
pub fn flat_plane(world: &World, model: FeatureId, part: usize) -> Option<cadrs_sketch::PlaneRef> {
    let id = flat_plane_id(model, part);
    let frame = *world.resource::<PartCache>().planes.get(&FeatureId(id))?;
    Some(cadrs_sketch::PlaneRef::Feature(cadrs_sketch::FeaturePlane::new(id, frame)))
}

/// New sketch on the flat pattern of `model`'s part `part` (SM14.1): in the flat view (the
/// panel opens on the model's flat), with the flat's bend centre lines used as construction.
pub fn begin_flat_sketch(world: &mut World, model: FeatureId, part: usize) {
    let Some(plane) = flat_plane(world, model, part) else {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::warning("The sheet metal model has no flat pattern to sketch on").name("flat-sketch-toast"));
        world.flush();
        return;
    };
    crate::sheetmetal_table::show_flat(world, model);
    crate::sketch::begin_sketch_on(world, Some(plane));
    let Some(s) = world.get_resource::<crate::sketch::SketchSession>().map(|s| (s.element, s.feature)) else { return };
    // The bend lines, to dimension and constrain against (the lesson's slot is placed from a
    // bend line).
    let items: Vec<(Projected, Link)> = flat_part(world.resource::<PartCache>(), model, part)
        .map(|flat| {
            flat_lines(flat)
                .into_iter()
                .filter_map(|((a, b), bend)| bend.map(|j| (Projected::Line(a, b), Link::FlatLine { model: model.0, part: part as u8, bend: Some(j) })))
                .collect()
        })
        .unwrap_or_default();
    if items.is_empty() {
        return;
    }
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&cadrs_core::commands::EditSketch { element: s.0, feature: s.1, op: cadrs_sketch::SketchOp::UseConstruction { items } })
    {
        warn!("cannot use the bend lines: {e}");
    }
}

/// Part `part` of `model`'s flat pattern, as the last rebuild has it.
fn flat_part(cache: &PartCache, model: FeatureId, part: usize) -> Option<&cadrs_sheetmetal::flat::FlatPart> {
    cache.sheet_metal.iter().rev().find(|c| c.feature == model)?.flat.parts.get(part)
}

/// The flat's lines a sketch on it snaps to and uses when touched (its outline, cut-outs and
/// bend lines), as imprints linked to them; `None` unless `sketch` is a sketch on a flat
/// pattern.
pub fn flat_imprints(doc: &ActiveDocument, cache: &PartCache, sketch: FeatureId) -> Option<Vec<cadrs_sketch::Imprint>> {
    let features = doc.active_element()?.features();
    let (model, part) = sketch_target(features, sketch)?;
    let flat = flat_part(cache, model, part)?;
    Some(
        flat_lines(flat)
            .into_iter()
            .enumerate()
            .map(|(i, ((a, b), bend))| cadrs_sketch::Imprint {
                id: cadrs_sketch::synthetic_curve(1, i as u32),
                shape: cadrs_sketch::ImprintShape::Line(a, b),
                link: Some(Link::FlatLine { model: model.0, part: part as u8, bend }),
            })
            .collect(),
    )
}

/// The flat view's scene point `p` on a region of a sketch on the shown model's flat: that
/// region (the smallest one there), to pick.
pub fn region_at(world: &World, p: cadrs_sheetmetal::poly::P2) -> Option<Pick> {
    let t = world.resource::<crate::sheetmetal_table::SmTable>();
    let scene = t.scene.as_ref()?;
    let features = features(world)?;
    let cache = world.resource::<PartCache>();
    let mut best: Option<(f64, Pick)> = None;
    for sr in &cache.regions {
        let Some((_, part)) = sketch_target(&features, sr.sketch) else { continue };
        let shift = scene.shifts.get(part).copied().unwrap_or_else(cadrs_sheetmetal::poly::V2::zeros);
        let q = cadrs_sketch::Vec2::new(p.x - shift.x, p.y - shift.y);
        let Some(i) = cadrs_sketch::region::region_at(&sr.regions, q) else { continue };
        let area = sr.regions[i].area();
        if best.as_ref().is_none_or(|(a, _)| area < *a) {
            best = Some((area, Pick::Region(sr.sketch, i as u32)));
        }
    }
    best.map(|(_, p)| p)
}

// ---------------------------------------------------------------------------------------------
// Picked regions of sketches on the flat: orange in the flat view, and wrapped onto the folded
// part in 3D

pub struct FlatUiPlugin;

impl Plugin for FlatUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (keep_flat_view, draw_picked_regions.after(crate::parts::PartsSet)).run_if(in_state(crate::AppState::Document)));
    }
}

/// While a sketch on the flat is edited, the flat view stays open on its model (closing the
/// panel would leave the sketch nowhere to draw: it isn't shown in 3D); the panel toggle says
/// so. Its constraint glyphs show on hover only, as Onshape's flat sketch starts (lesson t0052:
/// Show constraints off); the setting is put back when the sketch closes.
#[allow(clippy::too_many_arguments)]
fn keep_flat_view(
    session: Option<Res<crate::sketch::SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    kind: Res<crate::viewport::ActiveKind>,
    mut open: ResMut<crate::appearance::SidePanel>,
    mut table: ResMut<crate::sheetmetal_table::SmTable>,
    mut settings: ResMut<crate::sketch::SketchViewSettings>,
    theme: Res<Theme>,
    mut memo: Local<Option<(FeatureId, bool)>>,
    mut commands: Commands,
) {
    let target = session.as_ref().and_then(|s| {
        let features = doc.as_ref()?.active_element()?.features();
        sketch_target(features, s.feature).map(|t| (s.feature, t))
    });
    let Some((sketch, (model, _))) = target.filter(|_| *kind == crate::viewport::ActiveKind::PartStudio) else {
        if let Some((_, was)) = memo.take()
            && settings.show_constraints != was
        {
            settings.show_constraints = was;
        }
        return;
    };
    if memo.is_none_or(|(f, _)| f != sketch) {
        let was = memo.map_or(settings.show_constraints, |(_, w)| w);
        *memo = Some((sketch, was));
        settings.show_constraints = false;
    }
    if *open != crate::appearance::SidePanel::SheetMetal {
        let closing = *open == crate::appearance::SidePanel::None;
        *open = crate::appearance::SidePanel::SheetMetal;
        if closing {
            cadrs_ui::show_notification(&mut commands, &theme, cadrs_ui::Notification::info("The flat view stays open while a sketch on the flat pattern is edited").name("flat-sketch-panel-toast"));
        }
    }
    if table.context != Some(model) {
        table.context = Some(model);
    }
}

#[derive(Component)]
struct FlatRegionFill;

/// The wash entity, its mesh, and the regions and scene it was built for.
type FillState = (Entity, Handle<Mesh>, Vec<(FeatureId, usize)>, Option<u64>);

/// The regions of sketches on the flat to show orange: the selected ones (or picked in the
/// sketch being edited), and the flat Extrude's while its dialog is open.
fn picked_regions(world_features: &[Feature], cache: &PartCache, selected: &crate::region_select::SelectedRegions, extrude: Option<&FlatExtrudeFeature>) -> Vec<(FeatureId, usize)> {
    let mut out: Vec<(FeatureId, usize)> = selected.0.iter().copied().filter(|(s, _)| sketch_target(world_features, *s).is_some()).collect();
    if let Some(x) = extrude {
        for sr in &cache.regions {
            for (i, r) in sr.regions.iter().enumerate() {
                let mut curves = r.curves.clone();
                curves.sort();
                curves.dedup();
                let picked = x.sketches.contains(&sr.sketch) || x.regions.iter().any(|q| q.sketch == sr.sketch && q.curves == curves && r.contains(q.seed));
                if picked && !out.contains(&(sr.sketch, i)) {
                    out.push((sr.sketch, i));
                }
            }
        }
    }
    out
}

/// A flat point on the folded part: on the wall whose material it lies on (bend regions are
/// left out), on both of the wall's faces.
fn folded(ctx: &cadrs_core::sheetmetal::SheetMetalContext, flat: &cadrs_sheetmetal::flat::FlatPart, p: cadrs_sheetmetal::poly::P2) -> Option<(usize, [Vec3; 2])> {
    use cadrs_sheetmetal::flat::PieceSource;
    let t = ctx.model.params.thickness;
    flat.pieces.iter().enumerate().find_map(|(k, piece)| {
        let PieceSource::Wall(w) = piece.source else { return None };
        if !piece.polygon.contains(p) {
            return None;
        }
        let wall = ctx.model.wall(w)?;
        let q = flat.placement(w)?.inverse()?.apply(p);
        let a = wall.surface.point(q);
        let n = wall.surface.normal()?;
        let b = a + n * t;
        let v = |x: cadrs_sheetmetal::model::P3| Vec3::new(x.x as f32, x.y as f32, x.z as f32);
        Some((k, [v(a), v(b)]))
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_picked_regions(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    selected: Res<crate::region_select::SelectedRegions>,
    applied: Option<Res<crate::applied::AppliedSession>>,
    table: Res<crate::sheetmetal_table::SmTable>,
    mut flat_lines_g: Gizmos<crate::sheetmetal_table::FlatHighlightGizmos>,
    mut ghost: Gizmos<crate::extrude::RegionGizmos>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<Option<FillState>>,
    q_fill: Query<(), With<FlatRegionFill>>,
    mut commands: Commands,
) {
    if state.as_ref().is_some_and(|(e, ..)| !q_fill.contains(*e)) {
        *state = None;
    }
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else { return };
    let features = el.features();
    let extrude = applied.as_ref().and_then(|s| el.feature(s.feature)).and_then(|f| match &f.kind {
        FeatureKind::FlatExtrude(x) => Some(x),
        _ => None,
    });
    let picked = picked_regions(features, &cache, &selected, extrude);
    let orange = Color::srgb_u8(0xe8, 0x9a, 0x2c);
    let scene = table.scene.clone();
    let scene_key = table.scene.as_ref().map(|s| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}{}", s.shifts, s.thickness).hash(&mut h);
        h.finish()
    });
    let mut tris: Vec<[f32; 3]> = Vec::new();
    for (sk, i) in &picked {
        let Some((model, part)) = sketch_target(features, *sk) else { continue };
        let Some(r) = cache.sketch_regions(*sk).and_then(|r| r.regions.get(*i)) else { continue };
        // The flat view: the outline and an orange wash on the sheet's top.
        if let Some(scene) = scene.as_ref() {
            let shift = scene.shifts.get(part).copied().unwrap_or_else(cadrs_sheetmetal::poly::V2::zeros);
            let z = scene.thickness as f32 + 2e-3;
            let at = |q: &cadrs_sketch::Vec2| Vec3::new((q.x + shift.x) as f32, (q.y + shift.y) as f32, z);
            for ring in std::iter::once(&r.outer).chain(&r.holes) {
                flat_lines_g.linestrip(ring.iter().chain(ring.first()).map(at), orange);
            }
            let (v, idx) = r.triangulate();
            tris.extend(idx.into_iter().map(|k| at(&v[k as usize]).to_array()));
        }
        // The folded part: the outline wrapped onto the walls it lies on.
        let Some(ctx) = cache.sheet_metal.iter().rev().find(|c| c.feature == model) else { continue };
        let Some(flat) = ctx.flat.parts.get(part) else { continue };
        for ring in std::iter::once(&r.outer).chain(&r.holes) {
            let n = ring.len();
            let mut runs: [Vec<Vec3>; 2] = [Vec::new(), Vec::new()];
            let mut wall = None;
            for k in 0..=n {
                let (a, b) = (ring[k % n], ring[(k + 1) % n]);
                let steps = if k == n { 1 } else { ((a.distance(b) / 1.0).ceil() as usize).clamp(1, 200) };
                for s in 0..steps {
                    if k == n && s > 0 {
                        break;
                    }
                    let f = s as f64 / steps as f64;
                    let q = cadrs_sheetmetal::poly::P2::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
                    match folded(ctx, flat, q) {
                        Some((w, pts)) if wall.is_none_or(|x| x == w) => {
                            wall = Some(w);
                            for side in 0..2 {
                                runs[side].push(pts[side]);
                            }
                        }
                        other => {
                            for run in &mut runs {
                                if run.len() > 1 {
                                    ghost.linestrip(run.drain(..), orange);
                                }
                                run.clear();
                            }
                            wall = other.as_ref().map(|(w, _)| *w);
                            if let Some((_, pts)) = other {
                                for side in 0..2 {
                                    runs[side].push(pts[side]);
                                }
                            }
                        }
                    }
                }
            }
            for run in &mut runs {
                if run.len() > 1 {
                    ghost.linestrip(run.drain(..), orange);
                }
            }
        }
    }
    // The wash: one mesh on the flat view's layer, rebuilt when the picked regions change.
    if state.as_ref().is_some_and(|(_, _, k, s)| *k == picked && *s == scene_key) {
        return;
    }
    let mesh = {
        use bevy::asset::RenderAssetUsages;
        use bevy::mesh::{Indices, PrimitiveTopology};
        let positions = if tris.is_empty() { vec![[0.0; 3]; 3] } else { tris };
        let n = positions.len() as u32;
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n as usize])
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_indices(Indices::U32((0..n).collect()))
    };
    match state.as_mut() {
        Some((_, handle, k, s)) => {
            if let Some(mut m) = meshes.get_mut(&*handle) {
                *m = mesh;
            }
            *k = picked;
            *s = scene_key;
        }
        None => {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba_u8(0xfd, 0xc0, 0x5a, 0x99),
                unlit: true,
                cull_mode: None,
                double_sided: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            let e = commands
                .spawn((
                    Name::new("flat-region-fill"),
                    FlatRegionFill,
                    Mesh3d(handle.clone()),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    bevy::camera::visibility::RenderLayers::layer(crate::sheetmetal_table::FLAT_LAYER),
                    DespawnOnExit(crate::AppState::Document),
                ))
                .id();
            *state = Some((e, handle, picked, scene_key));
        }
    }
}
