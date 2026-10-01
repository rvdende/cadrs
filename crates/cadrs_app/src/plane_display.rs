//! Plane features in the view (P3.7, PS12.2; `ex4-step5.png`): a translucent square about the
//! plane's origin, sized to the parts (P3.11; at most the default planes' size), with its outline (orange when hovered, blue
//! when selected) and its name along its top-left edge, drawn like the default planes'
//! ([`crate::viewport`]). A plane hidden with its eye in the feature list is not drawn.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::FeatureId;
use cadrs_sketch::PlaneFrame;
use cadrs_ui::prelude::*;

use crate::parts::PartCache;
use crate::viewport::{
    ActiveKind, AffineInner, HighlightGizmos, HoverGizmos, PLANE_HALF, Pick, PlaneHighlight, PlaneMaterials, Selection,
    ViewportArea, ViewportRect, ViewportView, label_alpha, place_affine, readable_axes,
};
use crate::{ActiveDocument, AppState};

pub struct PlaneDisplayPlugin;

impl Plugin for PlaneDisplayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_plane_quads, draw_plane_feature_edges, draw_curve_features)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(
            PostUpdate,
            place_plane_feature_labels
                .before(bevy::ui::UiSystems::Layout)
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// A Plane feature's square.
#[derive(Component, Debug, Clone, Copy)]
pub struct PlaneQuad {
    pub feature: FeatureId,
    frame: [Vec3; 3],
    /// Half its side (P3.11: sized to the parts, [`PartCache::plane_half`]).
    half: f32,
}

/// A Plane feature's label (UI, over the view).
#[derive(Component, Debug, Clone, Copy)]
struct PlaneFeatureLabel {
    feature: FeatureId,
    frame: [Vec3; 3],
    half: f32,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// Origin, u, v of a frame.
fn axes(f: &PlaneFrame) -> [Vec3; 3] {
    [v3(f.origin), v3(f.u).normalize_or_zero(), v3(f.v).normalize_or_zero()]
}

/// The planes to show: the Part Studio's built Plane features, less the ones hidden with their
/// eye, with their names.
fn shown(doc: Option<&ActiveDocument>, cache: &PartCache, kind: ActiveKind) -> Vec<(FeatureId, [Vec3; 3], String, f32)> {
    if kind != ActiveKind::PartStudio {
        return Vec::new();
    }
    let Some(el) = doc.and_then(|d| d.active_element()) else {
        return Vec::new();
    };
    el
        .features()
        .iter()
        .filter(|f| matches!(f.kind, cadrs_core::FeatureKind::Plane(_)))
        .filter(|f| el.sketch_visibility(f.id) != Some(false))
        .filter_map(|f| {
            let frame = cache.planes.get(&f.id)?;
            let (center, half) = cache.plane_square(frame);
            let mut a = axes(frame);
            a[0] = v3(center);
            Some((f.id, a, f.name.clone(), half))
        })
        // P3G.4: the planes Derived features brought in, drawn and picked the same way.
        .chain(el.features().iter().filter_map(|f| cache.derived.get(&f.id)).flat_map(|d| d.planes.iter()).filter(|(id, _)| el.sketch_visibility(*id) != Some(false)).filter_map(|(id, name)| {
            let frame = cache.planes.get(id)?;
            let (center, half) = cache.plane_square(frame);
            let mut a = axes(frame);
            a[0] = v3(center);
            Some((*id, a, name.clone(), half))
        }))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn sync_plane_quads(
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    kind: Res<ActiveKind>,
    selection: Res<Selection>,
    materials: Option<Res<PlaneMaterials>>,
    theme: Res<Theme>,
    mut meshes: ResMut<Assets<Mesh>>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(Entity, &PlaneQuad, &mut MeshMaterial3d<StandardMaterial>)>,
    q_labels: Query<(Entity, &PlaneFeatureLabel, &Children)>,
    mut q_text: Query<&mut Text>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    let Some(m) = materials else { return };
    let want = shown(doc.as_deref(), &cache, *kind);
    let quad = quad.get_or_insert_with(|| meshes.add(Rectangle::new(PLANE_HALF * 2.0, PLANE_HALF * 2.0))).clone();
    // Squares.
    for (e, pq, mut mat) in &mut q {
        match want.iter().find(|(id, f, _, h)| *id == pq.feature && *f == pq.frame && *h == pq.half) {
            None => commands.entity(e).despawn(),
            Some(_) => {
                let target = if selection.contains(Pick::Feature(pq.feature)) { &m.selected } else { &m.normal };
                if mat.0 != *target {
                    mat.0 = target.clone();
                }
            }
        }
    }
    for (id, f, _, h) in &want {
        if q.iter().any(|(_, pq, _)| pq.feature == *id && pq.frame == *f && pq.half == *h) {
            continue;
        }
        let n = f[1].cross(f[2]);
        let rotation = Quat::from_mat3(&Mat3::from_cols(f[1], f[2], n));
        let k = *h / PLANE_HALF;
        commands.spawn((
            Name::new(format!("plane-feature-{}", id.0)),
            PlaneQuad { feature: *id, frame: *f, half: *h },
            Mesh3d(quad.clone()),
            MeshMaterial3d(m.normal.clone()),
            Transform { translation: f[0], rotation, scale: Vec3::new(k, k, 1.0) },
            DespawnOnExit(AppState::Document),
        ));
    }
    // Labels.
    let Some(area) = q_area.iter().next() else { return };
    for (e, l, children) in &q_labels {
        match want.iter().find(|(id, f, _, h)| *id == l.feature && *f == l.frame && *h == l.half) {
            None => commands.entity(e).despawn(),
            Some((_, _, name, _)) => {
                if let Some(mut t) = children.first().and_then(|c| q_text.get_mut(*c).ok())
                    && t.0 != *name
                {
                    t.0 = name.clone();
                }
            }
        }
    }
    for (id, f, name, h) in &want {
        if q_labels.iter().any(|(_, l, _)| l.feature == *id && l.frame == *f && l.half == *h) {
            continue;
        }
        let label = commands
            .spawn((
                Name::new(format!("plane-feature-label-{}", id.0)),
                PlaneFeatureLabel { feature: *id, frame: *f, half: *h },
                Node { position_type: PositionType::Absolute, ..default() },
                // Under the dialogs that share the viewport area.
                ZIndex(-1),
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .with_child((
                AffineInner,
                theme.text(name.clone(), 17.0, FontWeight::SEMIBOLD, theme.plane_label),
                Pickable::IGNORE,
            ))
            .id();
        commands.entity(area).add_child(label);
    }
}

/// The squares' outlines: thin blue-grey, orange when hovered, blue when selected.
#[allow(clippy::too_many_arguments)]
fn draw_plane_feature_edges(
    mut gizmos: Gizmos,
    mut hl_gizmos: Gizmos<HighlightGizmos>,
    mut hover_gizmos: Gizmos<HoverGizmos>,
    q: Query<&PlaneQuad>,
    theme: Res<Theme>,
    highlight: Res<PlaneHighlight>,
    selection: Res<Selection>,
    section: Res<crate::section_view::SectionClip>,
) {
    // P3E.3a: cut by a section view's plane.
    let clip = section.plane;
    use crate::section_view::clipped_line as cut;
    for pq in &q {
        let [o, u, v] = pq.frame;
        let h = pq.half;
        let corners = [o + (-u - v) * h, o + (u - v) * h, o + (u + v) * h, o + (-u + v) * h];
        let p = Pick::Feature(pq.feature);
        let (selected, hovered) = (selection.contains(p), highlight.is_hovered(p));
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            if hovered {
                cut(&mut hover_gizmos, clip, a, b, theme.highlight);
            } else if selected {
                cut(&mut hl_gizmos, clip, a, b, theme.selection_3d);
            } else {
                cut(&mut gizmos, clip, a, b, theme.plane_edge);
            }
        }
    }
}

/// Curve features (a Helix) as polylines: selected (in the list) or hovered like planes. A
/// curve hidden with its eye in the feature list is not drawn.
#[allow(clippy::too_many_arguments)]
fn draw_curve_features(
    mut gizmos: Gizmos,
    mut hl_gizmos: Gizmos<HighlightGizmos>,
    mut hover_gizmos: Gizmos<HoverGizmos>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    highlight: Res<PlaneHighlight>,
    selection: Res<Selection>,
) {
    if *kind != ActiveKind::PartStudio {
        return;
    }
    let Some(el) = doc.as_deref().and_then(|d| d.active_element()) else { return };
    for f in el.features() {
        if !matches!(f.kind, cadrs_core::FeatureKind::Helix(_)) || el.sketch_visibility(f.id) == Some(false) {
            continue;
        }
        let Some(g) = cache.curves.get(&f.id) else { continue };
        let pts: Vec<Vec3> = g.polyline().into_iter().map(v3).collect();
        let p = Pick::Feature(f.id);
        if highlight.is_hovered(p) {
            hover_gizmos.linestrip(pts, theme.highlight);
        } else if selection.contains(p) {
            hl_gizmos.linestrip(pts, theme.selection_3d);
        } else {
            gizmos.linestrip(pts, Color::srgb_u8(0x30, 0x30, 0x30));
        }
    }
}

/// A mesh in world coordinates that hides a plane label behind it: sketch region fills and the
/// selected-region wash (labels are drawn over the view, so they test these by hand).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct LabelOccluder;

/// The sketch region fills a ray from the eye meets closer than `t` (they would be in front of a
/// label drawn at `t`): the fills are in world coordinates.
pub(crate) fn fill_in_front(
    meshes: &Assets<Mesh>,
    fills: &Query<(&Mesh3d, &InheritedVisibility), With<LabelOccluder>>,
    ro: Vec3,
    rd: Vec3,
    t: f32,
) -> bool {
    let eps = 1e-3 * t.abs().max(1.0);
    fills.iter().filter(|(_, v)| v.get()).any(|(m, _)| {
        let Some(mesh) = meshes.get(&m.0) else { return false };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            return false;
        };
        let tri = |i: usize| Vec3::from_array(pos[i]);
        let hit = |a: Vec3, b: Vec3, c: Vec3| {
            // Möller–Trumbore.
            let (e1, e2) = (b - a, c - a);
            let pv = rd.cross(e2);
            let det = e1.dot(pv);
            if det.abs() < 1e-12 {
                return false;
            }
            let tv = ro - a;
            let u = tv.dot(pv) / det;
            let qv = tv.cross(e1);
            let v = rd.dot(qv) / det;
            let d = e2.dot(qv) / det;
            (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && d < t - eps
        };
        match mesh.indices() {
            Some(idx) => {
                let idx: Vec<usize> = idx.iter().collect();
                idx.chunks_exact(3).any(|c| hit(tri(c[0]), tri(c[1]), tri(c[2])))
            }
            None => (0..pos.len() / 3).any(|i| hit(tri(3 * i), tri(3 * i + 1), tri(3 * i + 2))),
        }
    })
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_plane_feature_labels(
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    theme: Res<Theme>,
    cache: Res<PartCache>,
    meshes: Res<Assets<Mesh>>,
    fills: Query<(&Mesh3d, &InheritedVisibility), With<LabelOccluder>>,
    doc: Option<Res<ActiveDocument>>,
    mut q: Query<(&PlaneFeatureLabel, &Children, &mut Node, &mut UiTransform, &mut Visibility), Without<AffineInner>>,
    mut q_inner: Query<(&ComputedNode, &mut UiTransform, &mut TextColor), With<AffineInner>>,
    section: Res<crate::section_view::SectionClip>,
) {
    let v = view.view;
    let cut = section.plane;
    // The screen boxes of the labels placed so far: a label doesn't go over another
    // (`course_ps12_planes` 14: Plane 3's moved label met Plane 1's).
    let mut placed: Vec<Rect> = Vec::new();
    let mut labels: Vec<_> = q.iter_mut().collect();
    // In feature-list order (the earlier plane keeps its place).
    let order = |f: FeatureId| {
        doc.as_deref()
            .and_then(|d| d.active_element())
            .and_then(|el| el.features().iter().position(|g| g.id == f))
            .unwrap_or(usize::MAX)
    };
    labels.sort_by_key(|(l, ..)| order(l.feature));
    for (label, children, mut node, mut transform, mut vis) in labels {
        let Some(&child) = children.first() else { continue };
        let Ok((inner_node, mut inner_t, mut color)) = q_inner.get_mut(child) else { continue };
        let [o, pu, pv] = label.frame;
        let (u, w) = readable_axes(&v, pu, pv);
        let a = v.project_vector(u) * v.scale;
        let b = v.project_vector(-w) * v.scale;
        let alpha = label_alpha(pu.cross(pv).dot(v.back()).abs());
        if alpha <= 0.0 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let size = inner_node.size() * inner_node.inverse_scale_factor();
        let pad = Vec2::new(3.0, 1.0);
        // The label goes in the square's top-left corner; when that corner is out of the view
        // (under the toolbar, `course_ps12_planes` 14) or behind a part or a sketch's region
        // fill, it moves to the top-right, bottom-left or bottom-right corner, still inside the
        // square and reading the same way, and hides only when none is clear.
        let span = (size + pad * 2.0) * v.scale;
        let inside = Rect::from_corners(Vec2::new(0.0, 4.0), rect.0.size());
        let half = label.half;
        let corners = [
            Vec2::ZERO,
            Vec2::new(2.0 * half - span.x, 0.0),
            Vec2::new(0.0, 2.0 * half - span.y),
            Vec2::new(2.0 * half - span.x, 2.0 * half - span.y),
        ];
        let clear = corners.iter().find_map(|d| {
            let start = o + (w - u) * half + u * d.x - w * d.y;
            // P3E.3a judge: not on the side a section view removed.
            if cut.is_some_and(|(co, cn)| cn.dot(start - co) > 0.0) {
                return None;
            }
            let corner = rect.to_screen(v.project(start)) - rect.0.min;
            let pts = [Vec2::ZERO, Vec2::new(size.x, 0.0), Vec2::new(0.0, size.y), size].map(|q| corner + a * (pad.x + q.x) + b * (pad.y + q.y));
            if pts.iter().any(|p| !inside.contains(*p)) {
                return None;
            }
            let screen = pts[1..].iter().fold(Rect::from_corners(pts[0], pts[0]), |r, p| r.union_point(*p));
            if placed.iter().any(|r| !r.inflate(2.0).intersect(screen).is_empty()) {
                return None;
            }
            // Parts or region fills in front of the label hide it (it is drawn over the view,
            // not in it): its corners and middle, on the plane, each along its ray.
            let world = |q: Vec2| start + (u * (pad.x + q.x) - w * (pad.y + q.y)) * v.scale;
            let behind = [Vec2::ZERO, Vec2::new(size.x, 0.0), Vec2::new(0.0, size.y), size, size / 2.0].iter().any(|q| {
                let p = world(*q);
                let (ro, rd) = v.ray(v.project(p));
                let t = (p - ro).dot(rd);
                crate::parts::pick_face(&cache, &v, v.project(p)).is_some_and(|(_, _, hit)| hit < t - 1e-3 * t.abs().max(1.0))
                    || fill_in_front(&meshes, &fills, ro, rd, t)
            });
            (!behind).then_some((corner, screen))
        });
        let Some((corner, screen)) = clear else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        placed.push(screen);
        vis.set_if_neq(Visibility::Inherited);
        place_affine(&mut node, &mut transform, &mut inner_t, size, corner, a, b, pad);
        let c = theme.plane_label.with_alpha(alpha);
        if color.0 != c {
            color.0 = c;
        }
    }
}
