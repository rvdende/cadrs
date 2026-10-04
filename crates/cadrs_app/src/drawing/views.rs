//! Drawing views on the sheet (P3C.2, X6): generating them, drawing them and finding them under
//! the pointer.
//!
//! - **Generation.** Each view's lines come from the kernel's hidden-line removal on the rebuild
//!   worker ([`cadrs_core::views::request`]), cached per view in [`ViewCache`] under the
//!   referenced Part Studio's state (a hash of its features, part settings and appearances), the
//!   part, the view frame and whether it is shaded. The state is the one the drawing keeps
//!   (P3C.6, [`cadrs_drawing::ModelSource`]): editing the studio changes nothing here until the
//!   drawing is updated (see [`super::update`]); a drawing without a source for the studio (made
//!   before P3C.6) shows the workspace. Changing a view's display (hidden lines, tangent edges)
//!   needs no new projection; moving it none either. While a projection runs, the view keeps
//!   its last lines, or shows a dashed placeholder with "Generating view…".
//! - **Drawing.** Visible edges are medium solid lines (about 2 px at the fitted zoom), hidden
//!   edges thin black dashes, tangent edges solid or phantom (long, short, short), all at least
//!   a device pixel wide; shown sketches blue, over the edges (see `cadrs_drawing::view`). A shaded
//!   view is a 2D mesh of the part's display triangles in its appearance colours, depth-tested
//!   per pixel (each corner's depth along the line of sight), with its visible edges over it. A
//!   hovered view is drawn light orange, a selected one orange
//!   (`lesson-view-context-menu.png`).
//! - **Picking.** [`view_at`] finds the view under a sheet point (its bounds, padded), and
//!   [`edge_at`] the straight projected edge nearest a point (auxiliary views and Align view).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::FontWeight;
use cadrs_core::drawing_source::StudioState;
use cadrs_core::views::{ViewGeometry, ViewRequest};
use cadrs_core::{ElementId, ElementKind, PartId};
use cadrs_drawing::view::{LineKind, dashes, view_lines};
use cadrs_drawing::{Drawing, View, ViewId};
use cadrs_kernel::ProjCurve;
use cadrs_ui::Theme;

use super::{DRAWING_LAYER, DrawingUi, active_drawing};
use crate::ActiveDocument;
use crate::viewport::ActiveKind;

pub struct ViewsPlugin;

/// The systems that generate and draw the views (after the view tools).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewsSet;

impl Plugin for ViewsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ViewCache>()
            .init_resource::<ViewScene>()
            .init_gizmo_group::<ViewThin>()
            .init_gizmo_group::<ViewMedium>()
            .add_systems(Startup, configure_view_gizmos)
            .add_systems(
                Update,
                (drive_view_cache, rebuild_view_scene, sync_view_entities, size_view_lines, draw_view_strokes)
                    .chain()
                    .in_set(ViewsSet)
                    .after(super::size_sheet_texts)
                    .run_if(in_state(crate::AppState::Document)),
            )
            .add_systems(OnExit(crate::AppState::Document), |mut c: ResMut<ViewCache>, mut s: ResMut<ViewScene>| {
                *c = ViewCache::default();
                *s = ViewScene::default();
            });
    }
}

/// Thin view lines: hidden, tangent and phantom edges, sketches.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ViewThin;

/// Medium view lines: visible edges.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ViewMedium;

fn configure_view_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (cfg, _) = store.config_mut::<ViewThin>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
    let (cfg, _) = store.config_mut::<ViewMedium>();
    cfg.render_layers = RenderLayers::layer(DRAWING_LAYER);
}

/// View line widths follow the zoom: thin lines are 0.25 mm on paper and at least 1.25 px, so a
/// dashed hidden line reads as black, not grey; visible (medium) lines 0.5 mm and at least
/// 2 px, clearly heavier (`ex1-step6.png`, `ex1-drawing.png`).
fn size_view_lines(doc: Option<Res<ActiveDocument>>, ui: Res<DrawingUi>, mut store: ResMut<GizmoConfigStore>) {
    let Some(doc) = doc else {
        return;
    };
    let Some((_, view)) = super::current_view(&doc, &ui) else {
        return;
    };
    let thin = (0.25 * view.ppm).clamp(1.25, 6.0);
    let medium = (0.5 * view.ppm).clamp(2.0, 10.0);
    if store.config_mut::<ViewThin>().0.line.width != thin {
        store.config_mut::<ViewThin>().0.line.width = thin;
    }
    if store.config_mut::<ViewMedium>().0.line.width != medium {
        store.config_mut::<ViewMedium>().0.line.width = medium;
    }
}

/// View line colours.
pub fn ink() -> Color {
    Color::srgb_u8(0x00, 0x00, 0x00)
}

/// Sketches shown over a view (D7.4): thin dark grey, unlike the blue of a view being placed.
/// Shown sketches: blue, drawn over the view's edges (a sketch usually lies on the view's own
/// outline; in dark grey under the edges it didn't show at all: Final regression judge,
/// `course_drw_views` 16).
fn sketch_color() -> Color {
    Color::srgb_u8(0x2b, 0x64, 0xc0)
}

/// Hover: pale amber, distinct from the selection's orange (P3C.6).
fn hover_color() -> Color {
    Color::srgb_u8(0xf7, 0xb9, 0x5e)
}

fn selected_color() -> Color {
    Color::srgb_u8(0xe8, 0x74, 0x0c)
}

/// The colour of views being placed (the ghost following the cursor).
fn ghost_color() -> Color {
    Color::srgb_u8(0x5b, 0x8f, 0xd6)
}

// ---------------------------------------------------------------------------------------------
// The cache

/// What a view's projection depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewKey {
    element: ElementId,
    /// The studio's features (and, for shaded views, its appearances).
    state: u64,
    part: Option<PartId>,
    frame: [i64; 6],
    shaded: bool,
    /// The section or broken-out cut (P3C.8), hashed.
    cut: u64,
    /// Show part intersections (assembly views).
    intersections: bool,
    /// A flat pattern view (P3I.7).
    flat: bool,
}

/// The hash of a view's cut (0 without one).
fn cut_hash(v: &View) -> u64 {
    match v.effective_cut() {
        None => 0,
        Some(c) => {
            let mut h = DefaultHasher::new();
            format!("{c:?}").hash(&mut h);
            h.finish()
        }
    }
}

/// What a view is projected from: a Part Studio's state, or an assembly's (P3C.5).
#[derive(Clone)]
enum Shown {
    Studio(Arc<StudioState>),
    Assembly(Arc<cadrs_core::drawing_assembly::AssemblyState>),
}

enum Entry {
    Pending(cadrs_core::views::PendingView),
    Ready(Arc<ViewGeometry>),
    Failed(String),
}

/// Projected views by [`ViewKey`], and the last geometry each view showed.
#[derive(Resource, Default)]
pub struct ViewCache {
    entries: HashMap<ViewKey, (Entry, u64)>,
    /// The studio hashes, recomputed when the document changes.
    hashes: HashMap<(ElementId, bool), u64>,
    /// The hashes of the drawings' studio snapshots (by the snapshot string's address and
    /// length), recomputed when the document changes.
    snapshot_hashes: HashMap<(usize, usize), u64>,
    /// Parsed snapshots by hash.
    parsed: HashMap<u64, Arc<StudioState>>,
    /// Parsed assembly snapshots by hash, and the workspace's assembly states (P3C.5).
    parsed_assemblies: HashMap<u64, Arc<cadrs_core::drawing_assembly::AssemblyState>>,
    live_assemblies: HashMap<ElementId, (u64, Arc<cadrs_core::drawing_assembly::AssemblyState>)>,
    /// The last geometry a view had (shown while a new one is made).
    last: HashMap<ViewId, Arc<ViewGeometry>>,
    frame: u64,
}

/// A Part Studio's features, part settings and feature appearances.
pub struct StudioRef<'a> {
    pub id: ElementId,
    pub features: &'a [cadrs_core::Feature],
    pub props: &'a [cadrs_core::document::PartProps],
    pub appearances: &'a [(cadrs_core::FeatureId, cadrs_core::Appearance)],
}

/// The Part Studio a view or sheet references.
pub fn studio<'a>(doc: &'a cadrs_core::Document, r: &cadrs_drawing::ObjectRef) -> Option<StudioRef<'a>> {
    let el = doc.element(ElementId(r.element))?;
    match &el.kind {
        ElementKind::PartStudio { .. } => Some(StudioRef {
            id: el.id,
            features: el.features(),
            props: el.part_props(),
            appearances: el.feature_appearances(),
        }),
        _ => None,
    }
}

/// The part a reference names, if it names one.
pub fn part_of(r: &cadrs_drawing::ObjectRef) -> Option<PartId> {
    r.part.map(|(f, index)| PartId {
        feature: cadrs_core::FeatureId(f),
        index,
    })
}

fn studio_hash(s: &StudioRef, shaded: bool) -> u64 {
    let mut h = DefaultHasher::new();
    format!("{:?}", s.features).hash(&mut h);
    if shaded {
        format!("{:?}{:?}", s.props, s.appearances).hash(&mut h);
    }
    h.finish()
}

/// Cached entries unused for this many frames are dropped.
const KEEP_FRAMES: u64 = 600;

/// The hash a snapshot string is cached under.
pub fn snapshot_hash(snapshot: &str) -> u64 {
    let mut h = DefaultHasher::new();
    snapshot.hash(&mut h);
    h.finish()
}

impl ViewCache {
    /// The studio state `v` shows in drawing `d` (the drawing's snapshot, else the workspace):
    /// its studio, its state hash and, when `want` is set, the state itself.
    fn state_of(&mut self, doc: &cadrs_core::Document, d: Option<&Drawing>, v: &View, want: bool) -> Option<(ElementId, u64, Option<Arc<StudioState>>)> {
        if let Some(src) = d.and_then(|d| d.source(v.reference.element)) {
            let key = (src.snapshot.as_ptr() as usize, src.snapshot.len());
            let h = *self.snapshot_hashes.entry(key).or_insert_with(|| snapshot_hash(&src.snapshot));
            let state = if want {
                if !self.parsed.contains_key(&h) {
                    if self.parsed.len() > 16 {
                        self.parsed.clear();
                    }
                    self.parsed.insert(h, Arc::new(StudioState::parse(&src.snapshot)?));
                }
                self.parsed.get(&h).cloned()
            } else {
                None
            };
            return Some((ElementId(src.element), h, state));
        }
        let s = studio(doc, &v.reference)?;
        let state = *self
            .hashes
            .entry((s.id, v.shaded))
            .or_insert_with(|| studio_hash(&s, v.shaded));
        let st = want.then(|| {
            Arc::new(StudioState {
                features: s.features.to_vec(),
                props: s.props.to_vec(),
                appearances: s.appearances.to_vec(),
            })
        });
        Some((s.id, state, st))
    }

    /// The assembly state view `v` shows (P3C.5): the drawing's snapshot, else the workspace's.
    fn assembly_of(&mut self, doc: &cadrs_core::Document, d: Option<&Drawing>, v: &View) -> Option<(ElementId, u64, Arc<cadrs_core::drawing_assembly::AssemblyState>)> {
        use cadrs_core::drawing_assembly::AssemblyState;
        let el = ElementId(v.reference.element);
        if let Some(src) = d.and_then(|d| d.source(v.reference.element)) {
            let key = (src.snapshot.as_ptr() as usize, src.snapshot.len());
            let h = *self.snapshot_hashes.entry(key).or_insert_with(|| snapshot_hash(&src.snapshot));
            if !self.parsed_assemblies.contains_key(&h) {
                if self.parsed_assemblies.len() > 8 {
                    self.parsed_assemblies.clear();
                }
                self.parsed_assemblies.insert(h, Arc::new(AssemblyState::parse(&src.snapshot)?));
            }
            return self.parsed_assemblies.get(&h).map(|st| (el, h, st.clone()));
        }
        if let std::collections::hash_map::Entry::Vacant(e) = self.live_assemblies.entry(el) {
            let st = AssemblyState::of(doc, el)?;
            let h = snapshot_hash(&st.to_snapshot());
            e.insert((h, Arc::new(st)));
        }
        self.live_assemblies.get(&el).map(|(h, st)| (el, *h, st.clone()))
    }

    fn key_of(&mut self, doc: &cadrs_core::Document, d: Option<&Drawing>, v: &View) -> Option<ViewKey> {
        if cadrs_core::drawing_assembly::is_assembly(doc, ElementId(v.reference.element)) {
            let (element, state, _) = self.assembly_of(doc, d, v)?;
            return Some(ViewKey { element, state, part: None, frame: v.frame.key(), shaded: v.shaded, cut: 0, intersections: v.part_intersections, flat: false });
        }
        let (element, state, _) = self.state_of(doc, d, v, false)?;
        Some(ViewKey {
            element,
            state,
            part: part_of(&v.reference),
            frame: v.frame.key(),
            shaded: v.shaded,
            cut: cut_hash(v),
            intersections: false,
            flat: v.flat.is_some(),
        })
    }

    /// The key a view would have with the studio state `snapshot` (an update's new state).
    pub fn key_with(snapshot: &str, v: &View) -> ViewKey {
        ViewKey {
            element: ElementId(v.reference.element),
            state: snapshot_hash(snapshot),
            part: part_of(&v.reference),
            frame: v.frame.key(),
            shaded: v.shaded,
            cut: cut_hash(v),
            intersections: false,
            flat: v.flat.is_some(),
        }
    }

    /// The key an assembly view would have with the assembly state `snapshot` (P3C.5).
    pub fn assembly_key_with(snapshot: &str, v: &View) -> ViewKey {
        ViewKey { element: ElementId(v.reference.element), state: snapshot_hash(snapshot), part: None, frame: v.frame.key(), shaded: v.shaded, cut: 0, intersections: v.part_intersections, flat: false }
    }

    /// Stores a projection made elsewhere (an update's) under `key`.
    pub fn insert(&mut self, key: ViewKey, g: Arc<ViewGeometry>) {
        let frame = self.frame;
        self.entries.insert(key, (Entry::Ready(g), frame));
    }

    /// The request that projects `v` from `state`.
    pub fn request(state: &StudioState, v: &View) -> ViewRequest {
        cadrs_core::drawing_source::view_request(state, v)
    }

    /// Makes sure `v` is being generated; returns its geometry if it is ready (or the last one it
    /// had), and whether that is current.
    pub fn ensure(&mut self, doc: &cadrs_core::Document, d: Option<&Drawing>, v: &View) -> (Option<Arc<ViewGeometry>>, bool) {
        let Some(key) = self.key_of(doc, d, v) else {
            return (None, false);
        };
        let frame = self.frame;
        if !self.entries.contains_key(&key) {
            let shown = if key.part.is_none() && cadrs_core::drawing_assembly::is_assembly(doc, key.element) {
                self.assembly_of(doc, d, v).map(|(_, _, st)| Shown::Assembly(st))
            } else {
                self.state_of(doc, d, v, true).and_then(|(_, _, st)| st.map(Shown::Studio))
            };
            let pending = match shown {
                Some(Shown::Studio(state)) => Some(cadrs_core::views::request(state.features.clone(), Self::request(&state, v))),
                Some(Shown::Assembly(state)) => Some(cadrs_core::drawing_assembly::request((*state).clone(), v)),
                None => None,
            };
            if let Some(pending) = pending {
                self.entries.insert(key, (Entry::Pending(pending), frame));
            }
        }
        let Some((entry, used)) = self.entries.get_mut(&key) else {
            return (None, false);
        };
        *used = frame;
        if let Entry::Pending(p) = entry
            && let Some(r) = p.poll()
        {
            *entry = match r {
                Ok(g) => Entry::Ready(g),
                Err(e) => Entry::Failed(e),
            };
        }
        match entry {
            Entry::Ready(g) => {
                let g = g.clone();
                self.last.insert(v.id, g.clone());
                (Some(g), true)
            }
            _ => (self.last.get(&v.id).cloned(), false),
        }
    }

    /// The geometry `v` shows now (current or last), without starting anything.
    pub fn geometry(&self, v: &View) -> Option<Arc<ViewGeometry>> {
        self.last.get(&v.id).cloned()
    }

    /// Why the view could not be generated, if it failed.
    pub fn error(&mut self, doc: &cadrs_core::Document, d: Option<&Drawing>, v: &View) -> Option<String> {
        let key = self.key_of(doc, d, v)?;
        match self.entries.get(&key) {
            Some((Entry::Failed(e), _)) => Some(e.clone()),
            _ => None,
        }
    }

    /// Whether any view is still being generated.
    pub fn busy(&self) -> bool {
        self.entries.values().any(|(e, _)| matches!(e, Entry::Pending(_)))
    }
}

/// Starts and polls the projections of the active sheet's views (and the ghost being placed).
fn drive_view_cache(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    ui: Res<DrawingUi>,
    mut cache: ResMut<ViewCache>,
    mut pending: ResMut<cadrs_ui::PendingWork>,
) {
    cache.frame += 1;
    // Scripted screenshots wait while the shown sheet's views are being projected (P3C.7).
    let busy = *kind == ActiveKind::Drawing && cache.busy();
    if pending.0 != busy {
        pending.0 = busy;
    }
    let Some(doc) = doc else {
        return;
    };
    if doc.is_changed() {
        cache.hashes.clear();
        cache.snapshot_hashes.clear();
        cache.live_assemblies.clear();
    }
    if *kind != ActiveKind::Drawing {
        return;
    }
    let Some((id, d)) = active_drawing(&doc) else {
        return;
    };
    let Some(sheet) = d.sheets.get(ui.sheet_index(id, d)) else {
        return;
    };
    for v in &sheet.views {
        cache.ensure(&doc.doc, Some(d), v);
    }
    if let Some(g) = &ui.ghost {
        cache.ensure(&doc.doc, Some(d), g);
    }
    let frame = cache.frame;
    cache.entries.retain(|_, (_, used)| *used + KEEP_FRAMES > frame);
    let live: std::collections::HashSet<ViewId> = d.sheets.iter().flat_map(|s| s.views.iter().map(|v| v.id)).collect();
    let ghost = ui.ghost.as_ref().map(|g| g.id);
    cache.last.retain(|v, _| live.contains(v) || Some(*v) == ghost);
}

// ---------------------------------------------------------------------------------------------
// Picking

/// A view's bounds on the sheet `(min, max)`: its geometry's, or a small box at its anchor.
pub fn sheet_bounds(v: &View, g: Option<&ViewGeometry>) -> ([f64; 2], [f64; 2]) {
    // Crop, detail and break views: what they show (P3C.8).
    if v.clipped()
        && let Some(b) = g.and_then(|g| cadrs_drawing::view_kinds::shown_bounds(v, g))
    {
        return b;
    }
    let b = g.and_then(|g| {
        g.projection
            .bounds()
            .map(|(lo, hi)| ([lo.x, lo.y], [hi.x, hi.y]))
            .or(g.bounds)
    });
    match b {
        Some((lo, hi)) => {
            let corners = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]].map(|p| v.to_sheet(p));
            let mut min = corners[0];
            let mut max = corners[0];
            for c in corners {
                min = [min[0].min(c[0]), min[1].min(c[1])];
                max = [max[0].max(c[0]), max[1].max(c[1])];
            }
            (min, max)
        }
        None => ([v.anchor[0] - 20.0, v.anchor[1] - 15.0], [v.anchor[0] + 20.0, v.anchor[1] + 15.0]),
    }
}

/// The view of sheet `index` under the sheet point `p` (mm), the topmost (last placed) first.
pub fn view_at(d: &Drawing, index: usize, cache: &ViewCache, p: Vec2, pad: f64) -> Option<ViewId> {
    let sheet = d.sheets.get(index)?;
    sheet.views.iter().rev().find_map(|v| {
        let g = cache.geometry(v);
        let (lo, hi) = sheet_bounds(v, g.as_deref());
        let (x, y) = (p.x as f64, p.y as f64);
        (x >= lo[0] - pad && x <= hi[0] + pad && y >= lo[1] - pad && y <= hi[1] + pad).then_some(v.id)
    })
}

/// The straight visible or hidden edge of `v` nearest the sheet point `p` within `tol` (sheet
/// mm): its index and its direction in the view's 2D frame.
pub fn edge_at(v: &View, g: &ViewGeometry, p: Vec2, tol: f64) -> Option<(usize, [f64; 2])> {
    let local = v.from_sheet([p.x as f64, p.y as f64]);
    let tol_model = tol / v.scale.factor();
    let mut best: Option<(f64, usize, [f64; 2])> = None;
    for (i, e) in g.projection.edges.iter().enumerate() {
        let ProjCurve::Line { start, end } = e.curve else {
            continue;
        };
        let d = end - start;
        let len2 = d.norm_squared();
        if len2 < 1e-12 {
            continue;
        }
        let t = (((local[0] - start.x) * d.x + (local[1] - start.y) * d.y) / len2).clamp(0.0, 1.0);
        let q = start + d * t;
        let dist = ((local[0] - q.x).powi(2) + (local[1] - q.y).powi(2)).sqrt();
        if dist <= tol_model && best.is_none_or(|(b, _, _)| dist < b) {
            best = Some((dist, i, [d.x, d.y]));
        }
    }
    best.map(|(_, i, d)| (i, d))
}

// ---------------------------------------------------------------------------------------------
// The scene

/// A polyline to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<Vec2>,
    pub medium: bool,
    pub color: Color,
}

/// What the views were last drawn from, and their strokes.
#[derive(Resource, Default)]
pub struct ViewScene {
    key: Option<SceneKey>,
    pub strokes: Vec<Stroke>,
    /// Views without geometry yet: their placeholder boxes (min, max).
    placeholders: Vec<(ViewId, [f64; 2], [f64; 2], String)>,
    /// Shaded views to show: (view, geometry, placement).
    shaded: Vec<(View, Arc<ViewGeometry>)>,
}

#[derive(Clone, PartialEq)]
struct SceneKey {
    views: Vec<View>,
    geometry: Vec<Option<usize>>,
    hovered: Option<ViewId>,
    selected: Vec<ViewId>,
    ghost: Option<View>,
    ghost_geometry: Option<usize>,
    highlight_edge: Option<(ViewId, usize)>,
    sketches: Vec<(ViewId, u64)>,
    errors: Vec<Option<String>>,
    style: cadrs_drawing::DrawingStyle,
    tool_strokes: Vec<Vec<[f64; 2]>>,
}

/// The views of the active sheet as drawn now: the drag preview's anchors applied.
pub fn shown_views(d: &Drawing, index: usize, ui: &DrawingUi) -> Vec<View> {
    let Some(sheet) = d.sheets.get(index) else {
        return Vec::new();
    };
    sheet
        .views
        .iter()
        .map(|v| {
            let mut v = v.clone();
            if let Some((_, a)) = ui.drag_preview.iter().find(|(id, _)| *id == v.id) {
                v.anchor = *a;
            }
            // A bend note being dragged (P3I.7).
            if let Some((_, f)) = ui.flat_preview.as_ref().filter(|(id, _)| *id == v.id) {
                v.flat = Some(f.clone());
            }
            v
        })
        .collect()
}

fn geom_ptr(g: &Option<Arc<ViewGeometry>>) -> Option<usize> {
    g.as_ref().map(|g| Arc::as_ptr(g) as usize)
}

/// The sketches of `v`'s studio it shows, as model polylines.
fn sketch_polylines(doc: &cadrs_core::Document, v: &View) -> Vec<Vec<[f64; 3]>> {
    let Some(s) = studio(doc, &v.reference) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in s.features {
        if !v.sketches.contains(&f.id.0) {
            continue;
        }
        let Some(sk) = f.sketch() else { continue };
        let Some(plane) = sk.plane else { continue };
        let frame = plane.frame();
        for (id, c) in sk.geometry.curves.iter() {
            if c.construction {
                continue;
            }
            let pts = cadrs_sketch::hit::curve_polyline(&sk.geometry, id);
            out.push(pts.into_iter().map(|p| frame.to_world(p)).collect());
        }
    }
    out
}

fn sketch_hash(doc: &cadrs_core::Document, v: &View) -> u64 {
    if v.sketches.is_empty() {
        return 0;
    }
    let mut h = DefaultHasher::new();
    if let Some(s) = studio(doc, &v.reference) {
        for f in s.features.iter().filter(|f| v.sketches.contains(&f.id.0)) {
            format!("{:?}", f.kind).hash(&mut h);
        }
    }
    h.finish()
}

fn push_line(strokes: &mut Vec<Stroke>, pts: &[[f64; 2]], kind: LineKind, color: Color) {
    let v = |p: &[f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    let medium = kind == LineKind::Visible;
    match kind.pattern() {
        Some(pattern) => {
            for d in dashes(pts, pattern) {
                strokes.push(Stroke {
                    points: d.iter().map(v).collect(),
                    medium,
                    color,
                });
            }
        }
        None => strokes.push(Stroke {
            points: pts.iter().map(v).collect(),
            medium,
            color,
        }),
    }
}

/// The strokes of one view's decorations (P3C.8): hatching, threads, break lines, cutting
/// lines and detail circles of its children.
fn decor_strokes(strokes: &mut Vec<Stroke>, d: &cadrs_drawing::view_kinds::Decor, color: Color) {
    let v2 = |p: &[f64; 2]| Vec2::new(p[0] as f32, p[1] as f32);
    for (lines, medium) in [(&d.thin, false), (&d.medium, true)] {
        for l in lines {
            strokes.push(Stroke { points: l.iter().map(v2).collect(), medium, color });
        }
    }
}

/// The strokes of one view.
fn view_strokes(strokes: &mut Vec<Stroke>, v: &View, g: &ViewGeometry, color: Color, sketches: &[Vec<[f64; 3]>], highlight: Option<usize>) {
    for l in view_lines(v, &g.projection) {
        // A shaded view shows its visible edges only.
        if v.shaded && l.kind == LineKind::Hidden {
            continue;
        }
        let c = if Some(l.edge) == highlight { Color::srgb_u8(0x1f, 0x7a, 0xe0) } else { color };
        push_line(strokes, &l.points, l.kind, c);
    }
    // A flat pattern's bend lines, in their own pens unless highlighted (P3I.7).
    strokes.extend(super::flat_views::bend_strokes(v, g, (color != ink()).then_some(color)));
    if !sketches.is_empty() {
        let frame = v.frame.view_frame();
        let sketch_color = sketch_color();
        for poly in sketches {
            let pts: Vec<[f64; 2]> = poly
                .iter()
                .map(|p| {
                    let q = frame.to_2d(&nalgebra::Point3::new(p[0], p[1], p[2]));
                    v.to_sheet([q.x, q.y])
                })
                .collect();
            // With the medium lines, after the edges: on top of the outline it traces.
            strokes.push(Stroke { points: pts.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect(), medium: true, color: sketch_color });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_view_scene(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    ui: Res<DrawingUi>,
    mut cache: ResMut<ViewCache>,
    mut scene: ResMut<ViewScene>,
) {
    let Some(doc) = doc.filter(|_| *kind == ActiveKind::Drawing) else {
        if scene.key.is_some() {
            *scene = ViewScene::default();
        }
        return;
    };
    let Some((id, d)) = active_drawing(&doc) else {
        if scene.key.is_some() {
            *scene = ViewScene::default();
        }
        return;
    };
    let index = ui.sheet_index(id, d);
    let views = shown_views(d, index, &ui);
    let geometry: Vec<Option<Arc<ViewGeometry>>> = views.iter().map(|v| cache.geometry(v)).collect();
    let ghost_geometry = ui.ghost.as_ref().and_then(|g| cache.geometry(g));
    let errors: Vec<Option<String>> = views.iter().map(|v| cache.error(&doc.doc, Some(d), v)).collect();
    let key = SceneKey {
        views: views.clone(),
        geometry: geometry.iter().map(geom_ptr).collect(),
        hovered: ui.hovered,
        selected: ui.selected.clone(),
        ghost: ui.ghost.clone(),
        ghost_geometry: geom_ptr(&ghost_geometry),
        highlight_edge: ui.highlight_edge,
        sketches: views.iter().map(|v| (v.id, sketch_hash(&doc.doc, v))).collect(),
        errors: errors.clone(),
        style: d.style.clone(),
        tool_strokes: ui.tool_strokes.clone(),
    };
    // Children being placed show their cutting line or circle on their parent too.
    let avoid = d.sheets.get(index).map(cadrs_drawing::view_kinds::label_avoid).unwrap_or_default();
    let mut decor_views = views.clone();
    if let Some(g) = &ui.ghost {
        decor_views.push(g.clone());
    }
    if scene.key.as_ref() == Some(&key) {
        return;
    }
    let mut strokes = Vec::new();
    let mut placeholders = Vec::new();
    let mut shaded = Vec::new();
    for ((v, g), err) in views.iter().zip(&geometry).zip(&errors) {
        let color = if ui.selected.contains(&v.id) {
            selected_color()
        } else if ui.hovered == Some(v.id) {
            hover_color()
        } else {
            ink()
        };
        match g {
            Some(g) => {
                if v.shaded && !g.shaded.is_empty() {
                    shaded.push((v.clone(), g.clone()));
                }
                let sketches = sketch_polylines(&doc.doc, v);
                let hl = ui.highlight_edge.filter(|(id, _)| *id == v.id).map(|(_, e)| e);
                view_strokes(&mut strokes, v, g, color, &sketches, hl);
                let decor = cadrs_drawing::view_kinds::view_decor(&d.style, &decor_views, v, Some(&**g), &avoid);
                decor_strokes(&mut strokes, &decor, color);
            }
            None => {
                let (lo, hi) = sheet_bounds(v, None);
                let rect = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]], [lo[0], lo[1]]];
                push_line(&mut strokes, &rect, LineKind::Hidden, Color::srgb_u8(0x9a, 0x9a, 0x9a));
                let label = match err {
                    Some(e) => format!("View failed: {e}"),
                    None => "Generating view…".to_string(),
                };
                placeholders.push((v.id, lo, hi, label));
            }
        }
    }
    if let (Some(gv), Some(g)) = (&ui.ghost, &ghost_geometry) {
        let mut gv = gv.clone();
        gv.hidden_lines = false;
        view_strokes(&mut strokes, &gv, g, ghost_color(), &[], None);
        let decor = cadrs_drawing::view_kinds::view_decor(&d.style, &[], &gv, Some(&**g), &avoid);
        decor_strokes(&mut strokes, &decor, ghost_color());
    } else if let Some(gv) = &ui.ghost {
        let (lo, hi) = sheet_bounds(gv, None);
        let rect = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]], [lo[0], lo[1]]];
        push_line(&mut strokes, &rect, LineKind::Hidden, ghost_color());
    }
    for l in &ui.tool_strokes {
        push_line(&mut strokes, l, LineKind::Visible, ghost_color());
    }
    scene.key = Some(key);
    scene.strokes = strokes;
    scene.placeholders = placeholders;
    scene.shaded = shaded;
}

/// A shaded view's mesh, and what it was built from.
#[derive(Component)]
struct ShadedMesh {
    view: ViewId,
    geometry: usize,
}

/// A placeholder's label.
#[derive(Component)]
struct PlaceholderLabel(ViewId, String);

/// Spawns, moves and removes the shaded meshes and placeholder labels.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_view_entities(
    scene: Res<ViewScene>,
    theme: Res<Theme>,
    ui: Res<DrawingUi>,
    doc: Option<Res<ActiveDocument>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut q_mesh: Query<(Entity, &ShadedMesh, &mut Transform), Without<PlaceholderLabel>>,
    mut q_label: Query<(Entity, &PlaceholderLabel, &mut Transform, &mut TextFont), Without<ShadedMesh>>,
    mut commands: Commands,
) {
    let layer = RenderLayers::layer(DRAWING_LAYER);
    // Shaded meshes.
    let mut keep = Vec::new();
    for (e, m, mut t) in &mut q_mesh {
        match scene.shaded.iter().find(|(v, g)| v.id == m.view && Arc::as_ptr(g) as usize == m.geometry) {
            Some((v, _)) => {
                keep.push(v.id);
                let want = view_transform(v);
                if *t != want {
                    *t = want;
                }
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (v, g) in &scene.shaded {
        if keep.contains(&v.id) {
            continue;
        }
        // Each corner at its depth (nearer is higher), so the 2D pipeline's depth test keeps
        // the nearest face at every pixel (no painter's-order artefacts where triangles
        // overlap in depth).
        let (dmin, dmax) = g
            .shaded
            .iter()
            .flat_map(|t| t.depths)
            .fold((f64::MAX, f64::MIN), |(lo, hi), d| (lo.min(d), hi.max(d)));
        let span = (dmax - dmin).max(1e-9);
        let mut positions = Vec::with_capacity(g.shaded.len() * 3);
        let mut colors = Vec::with_capacity(g.shaded.len() * 3);
        for t in &g.shaded {
            for k in 0..3 {
                let z = (0.4 * (dmax - t.depths[k]) / span) as f32;
                positions.push([t.points[k][0] as f32, t.points[k][1] as f32, z]);
                colors.push(t.colors[k]);
            }
        }
        let count = positions.len() as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32((0..count).collect()));
        commands.spawn((
            Name::new(format!("shaded-view-{}", v.name.to_lowercase())),
            ShadedMesh {
                view: v.id,
                geometry: Arc::as_ptr(g) as usize,
            },
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(materials.add(ColorMaterial::from(Color::WHITE))),
            view_transform(v),
            layer.clone(),
        ));
    }
    // Placeholder labels, sized like the sheet text.
    let ppm = doc
        .as_deref()
        .and_then(|d| super::current_view(d, &ui))
        .map(|(_, v)| v.ppm)
        .unwrap_or(1.0);
    let em = 3.0 / super::CAP_HEIGHT;
    let px = (em * ppm).max(1.0).round();
    let mut have = Vec::new();
    for (e, l, mut t, mut font) in &mut q_label {
        match scene.placeholders.iter().find(|p| p.0 == l.0 && p.3 == l.1) {
            Some((_, lo, hi, _)) => {
                have.push(l.0);
                let pos = Vec3::new(((lo[0] + hi[0]) / 2.0) as f32, ((lo[1] + hi[1]) / 2.0) as f32, 1.0);
                let s = em / px;
                let want = Transform::from_translation(pos).with_scale(Vec3::new(s, s, 1.0));
                if *t != want {
                    *t = want;
                }
                let fs = bevy::text::FontSize::Px(px);
                if font.font_size != fs {
                    font.font_size = fs;
                }
            }
            None => commands.entity(e).despawn(),
        }
    }
    for (id, lo, hi, label) in &scene.placeholders {
        if have.contains(id) {
            continue;
        }
        let pos = Vec3::new(((lo[0] + hi[0]) / 2.0) as f32, ((lo[1] + hi[1]) / 2.0) as f32, 1.0);
        commands.spawn((
            Name::new("view-placeholder"),
            PlaceholderLabel(*id, label.clone()),
            Text2d::new(label.clone()),
            theme.font(px, FontWeight::NORMAL),
            TextColor(Color::srgb_u8(0x70, 0x70, 0x70)),
            Anchor::CENTER,
            Transform::from_translation(pos).with_scale(Vec3::new(em / px, em / px, 1.0)),
            layer.clone(),
        ));
    }
}

/// Model 2D → sheet: the view's anchor, rotation and scale.
fn view_transform(v: &View) -> Transform {
    let k = v.scale.factor() as f32;
    Transform {
        translation: Vec3::new(v.anchor[0] as f32, v.anchor[1] as f32, -0.5),
        rotation: Quat::from_rotation_z(v.rotation as f32),
        scale: Vec3::new(k, k, 1.0),
    }
}

fn draw_view_strokes(scene: Res<ViewScene>, notes: Option<Res<super::notes::NoteScene>>, mut thin: Gizmos<ViewThin>, mut medium: Gizmos<ViewMedium>) {
    let rects: &[(Vec2, Vec2)] = notes.as_deref().map_or(&[], |n| &n.table_rects);
    for s in &scene.strokes {
        if s.points.len() < 2 {
            continue;
        }
        let (min, max) = s.points.iter().fold((Vec2::MAX, Vec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        let touches = rects.iter().any(|(lo, hi)| min.x <= hi.x && max.x >= lo.x && min.y <= hi.y && max.y >= lo.y);
        if !touches {
            let pts = s.points.iter().copied();
            if s.medium {
                medium.linestrip_2d(pts, s.color);
            } else {
                thin.linestrip_2d(pts, s.color);
            }
            continue;
        }
        // Tables are opaque: each segment loses its parts inside a table's box.
        for w in s.points.windows(2) {
            for (a, b) in outside_rects(w[0], w[1], rects) {
                if s.medium {
                    medium.line_2d(a, b, s.color);
                } else {
                    thin.line_2d(a, b, s.color);
                }
            }
        }
    }
}

/// The parts of segment `a`–`b` outside every box of `rects`.
pub(crate) fn outside_rects(a: Vec2, b: Vec2, rects: &[(Vec2, Vec2)]) -> Vec<(Vec2, Vec2)> {
    // Parameter intervals covered by the boxes (Liang–Barsky), then their complement.
    let d = b - a;
    let mut cut: Vec<(f32, f32)> = Vec::new();
    for (lo, hi) in rects {
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let mut ok = true;
        for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
            if p.abs() < 1e-12 {
                if q < 0.0 {
                    ok = false;
                    break;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if ok && t0 < t1 {
            cut.push((t0, t1));
        }
    }
    cut.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let mut t = 0.0f32;
    for (c0, c1) in cut {
        if c0 > t {
            out.push((a + d * t, a + d * c0));
        }
        t = t.max(c1);
    }
    if t < 1.0 {
        out.push((a + d * t, b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::outside_rects;
    use bevy::prelude::Vec2;

    /// Tables are opaque (P3C wrap-up): a view line loses its part inside a table's box.
    #[test]
    fn view_lines_are_cut_out_of_tables() {
        let rects = [(Vec2::new(10.0, -5.0), Vec2::new(20.0, 5.0)), (Vec2::new(15.0, -1.0), Vec2::new(30.0, 1.0))];
        let parts = outside_rects(Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), &rects);
        assert_eq!(parts, vec![(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)), (Vec2::new(30.0, 0.0), Vec2::new(40.0, 0.0))]);
        // A line clear of the boxes stays whole; one inside is gone.
        assert_eq!(outside_rects(Vec2::new(0.0, 10.0), Vec2::new(40.0, 10.0), &rects).len(), 1);
        assert!(outside_rects(Vec2::new(11.0, 0.0), Vec2::new(19.0, 2.0), &rects).is_empty());
    }
}
