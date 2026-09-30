//! Update from this workspace (P3C.6, D13.2, X11).
//!
//! A drawing shows its studios as it kept them ([`cadrs_drawing::ModelSource`]), so editing a
//! Part Studio changes nothing on the sheet. This module watches the workspace:
//!
//! - **Dependency tracking.** For each studio the active drawing keeps, the workspace's state is
//!   rebuilt on the worker thread (cached, so usually free) whenever it changes, and the
//!   dependency hash of each part computed ([`cadrs_core::drawing_source::part_hash`]). A view is
//!   out of date when its part's hash differs from the one it shows
//!   ([`cadrs_drawing::Drawing::out_of_date`]); a change in another studio or another part of the
//!   same studio leaves it alone.
//! - **The gold icon.** While any view of the drawing is out of date, the toolbar's "Update from
//!   this workspace" button's glyph is gold, like Onshape's (`ex3-step8.png`), and its tooltip says how
//!   many views are out of date.
//! - **Updating.** A click on it or **Ctrl+Q** projects every out-of-date view from the
//!   workspace (on the worker thread), then applies one undoable
//!   [`cadrs_drawing::DrawingOp::Update`]: the
//!   studios' new states, the views' new hashes and their annotations re-measured, dangling ones
//!   frozen red where they were (see [`cadrs_drawing::update`]). The new projections go straight
//!   into the view cache, so the sheet shows them at once.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bevy::prelude::*;
use cadrs_core::drawing_source::{GONE, StudioState, source_of};
use cadrs_core::views::{PendingView, ViewGeometry};
use cadrs_core::{ElementId, ElementKind};
use cadrs_drawing::annotation::ViewModel;
use cadrs_drawing::{ModelSource, ObjectRef, ViewId};
use cadrs_ui::Tooltip;

use super::active_drawing;
use super::views::ViewCache;
use crate::ActiveDocument;
use crate::viewport::ActiveKind;

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Workspace>()
            .add_systems(
                Update,
                (track_workspace, drive_update, sync_update_button)
                    .chain()
                    .before(super::views::ViewsSet)
                    .run_if(in_state(crate::AppState::Document)),
            )
            .add_systems(OnExit(crate::AppState::Document), |mut w: ResMut<Workspace>| *w = Workspace::default());
    }
}

/// A studio's workspace state and its parts' dependency hashes.
struct Live {
    /// A hash of the state (what it was computed for).
    key: u64,
    state: Arc<StudioState>,
    build: Option<cadrs_core::rebuild::Pending>,
    /// The state as a drawing source, once its build is done.
    source: Option<ModelSource>,
}

/// An assembly's workspace state and its dependency hash (P3C.5): its studios are rebuilt on
/// the worker thread, then its drawing source is made.
struct LiveAsm {
    key: u64,
    state: Arc<cadrs_core::drawing_assembly::AssemblyState>,
    builds: Vec<(ElementId, cadrs_core::rebuild::Pending)>,
    done: HashMap<ElementId, Arc<cadrs_core::rebuild::Build>>,
    source: Option<ModelSource>,
}

/// What a view showed before the update.
enum Old {
    Known(Option<Arc<ViewGeometry>>),
    Pending(PendingView),
}

/// An update waiting for its projections.
struct PendingUpdate {
    drawing: ElementId,
    sources: Vec<ModelSource>,
    /// Each view: what it showed (projected now if it wasn't shown yet) and its new projection.
    views: Vec<(ViewId, Old, PendingView)>,
    done: Vec<(ViewId, Option<Arc<ViewGeometry>>, Arc<ViewGeometry>)>,
}

/// The workspace as the active drawing sees it.
#[derive(Resource, Default)]
pub struct Workspace {
    live: HashMap<ElementId, Live>,
    /// The assemblies the drawing shows or lists in BOM tables (P3C.5).
    assemblies: HashMap<ElementId, LiveAsm>,
    /// The active drawing's out-of-date BOM tables (P3C.5, D14.8).
    pub stale_boms: Vec<(cadrs_drawing::SheetId, cadrs_drawing::TableId)>,
    pending: Option<PendingUpdate>,
    /// The document changed since the workspace states were last compared.
    dirty: bool,
    /// The active drawing's out-of-date views.
    pub stale: Vec<ViewId>,
}

impl Workspace {
    /// The workspace's dependency hash of what a view of `r` shows, once it is known.
    pub fn live_hash(&self, r: &ObjectRef) -> Option<u64> {
        let el = ElementId(r.element);
        let src = match self.assemblies.get(&el) {
            Some(a) => a.source.as_ref()?,
            None => self.live.get(&el)?.source.as_ref()?,
        };
        Some(src.hash_of(r.part).unwrap_or(GONE))
    }

    /// An update is being made.
    pub fn updating(&self) -> bool {
        self.pending.is_some()
    }
}

fn state_key(s: &StudioState) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{s:?}").hash(&mut h);
    h.finish()
}

/// Keeps the workspace hashes of the active drawing's studios current and finds its
/// out-of-date views.
fn track_workspace(doc: Option<Res<ActiveDocument>>, kind: Res<ActiveKind>, mut ws: ResMut<Workspace>) {
    // Edits made in another tab are seen when the drawing is shown again.
    if doc.as_ref().is_some_and(|d| d.is_changed()) {
        ws.dirty = true;
    }
    let Some(doc) = doc.filter(|_| *kind == ActiveKind::Drawing) else {
        if !ws.stale.is_empty() {
            ws.stale.clear();
        }
        return;
    };
    let Some((_, d)) = active_drawing(&doc) else {
        return;
    };
    let dirty = std::mem::take(&mut ws.dirty);
    // Assemblies (P3C.5): the ones with a source, and the ones BOM tables list.
    let mut asms: Vec<ElementId> = d.sources.iter().map(|s| ElementId(s.element)).filter(|e| cadrs_core::drawing_assembly::is_assembly(&doc.doc, *e)).collect();
    for b in d.sheets.iter().flat_map(|s| s.tables.iter()).filter_map(|t| t.bom.as_ref()) {
        let e = ElementId(b.assembly);
        if !asms.contains(&e) {
            asms.push(e);
        }
    }
    for id in &asms {
        if dirty || !ws.assemblies.contains_key(id) {
            let Some(state) = cadrs_core::drawing_assembly::AssemblyState::of(&doc.doc, *id) else {
                ws.assemblies.remove(id);
                continue;
            };
            // The assembly's own settings and properties count too (the BOM reads them).
            let extra = doc.doc.element(*id).and_then(|e| e.assembly_model()).map(|a| format!("{:?}{:?}", a.bom, a.properties)).unwrap_or_default();
            let props = format!("{:?}{:?}", doc.doc.standard_content.iter().map(|p| (&p.part_number, &p.description)).collect::<Vec<_>>(), doc.doc.properties);
            let key = {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{state:?}{extra}{props}").hash(&mut h);
                h.finish()
            };
            if ws.assemblies.get(id).is_none_or(|l| l.key != key) {
                let builds = state.studios.iter().map(|(e, st)| (*e, cadrs_core::rebuild::request(st.features.clone()))).collect();
                ws.assemblies.insert(*id, LiveAsm { key, state: Arc::new(state), builds, done: HashMap::new(), source: None });
            }
        }
        if let Some(l) = ws.assemblies.get_mut(id) {
            let mut i = 0;
            while i < l.builds.len() {
                if let Some(b) = l.builds[i].1.poll() {
                    let (e, _) = l.builds.remove(i);
                    l.done.insert(e, b);
                } else {
                    i += 1;
                }
            }
            if l.builds.is_empty() && l.source.is_none() {
                l.source = Some(cadrs_core::drawing_assembly::source_of(&doc.doc, *id, &l.state, &l.done));
            }
        }
    }
    let studios: Vec<ElementId> = d.sources.iter().map(|s| ElementId(s.element)).filter(|e| !asms.contains(e)).collect();
    for id in &studios {
        let Some(el) = doc.doc.element(*id) else {
            ws.live.remove(id);
            continue;
        };
        if !matches!(el.kind, ElementKind::PartStudio { .. }) {
            continue;
        }
        // The state is compared only when the document changed (or on first sight).
        if dirty || !ws.live.contains_key(id) {
            let Some(state) = StudioState::of(el) else { continue };
            let key = state_key(&state);
            if ws.live.get(id).is_none_or(|l| l.key != key) {
                let build = Some(cadrs_core::rebuild::request(state.features.clone()));
                ws.live.insert(*id, Live { key, state: Arc::new(state), build, source: None });
            }
        }
        if let Some(l) = ws.live.get_mut(id)
            && let Some(p) = &mut l.build
            && let Some(b) = p.poll()
        {
            l.source = Some(source_of(*id, &l.state, &b));
            l.build = None;
        }
    }
    let stale = d.out_of_date(&|r| ws.live_hash(r));
    if ws.stale != stale {
        ws.stale = stale;
    }
    let stale_boms = d.stale_boms(&|r| ws.live_hash(r));
    if ws.stale_boms != stale_boms {
        ws.stale_boms = stale_boms;
    }
}

/// Starts updating the active drawing from the workspace (the Update button, Ctrl+Q): projects
/// its out-of-date views; [`drive_update`] applies the update when they are done.
pub fn start_update(world: &mut World) {
    let Some((drawing, d)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc).map(|(id, d)| (id, d.clone())))
    else {
        return;
    };
    let ws = world.resource::<Workspace>();
    if ws.pending.is_some() || (ws.stale.is_empty() && ws.stale_boms.is_empty()) {
        return;
    }
    // Only BOM tables are out of date (P3C.5): their new rows, at once.
    if ws.stale.is_empty() {
        let op = world
            .get_resource::<ActiveDocument>()
            .map(|doc| cadrs_core::drawing_source::with_bom_tables(&doc.doc, &d, cadrs_drawing::DrawingOp::Batch { ops: Vec::new(), label: "Update from this workspace".into() }));
        if let Some(op) = op
            && let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
            && let Err(e) = doc.execute(&cadrs_core::commands::EditDrawing { element: drawing, op })
        {
            warn!("Update refused: {e}");
        }
        return;
    }
    let stale = ws.stale.clone();
    let mut sources: Vec<ModelSource> = Vec::new();
    let mut work = Vec::new();
    let mut asm_work = Vec::new();
    for id in stale {
        let Some((_, v)) = d.view(id) else { continue };
        let studio = ElementId(v.reference.element);
        // An assembly view (P3C.5).
        if let Some(live) = ws.assemblies.get(&studio) {
            let Some(src) = &live.source else { continue };
            if !sources.iter().any(|s| s.element == src.element) {
                sources.push(src.clone());
            }
            asm_work.push((id, v.clone(), live.state.clone()));
            continue;
        }
        let Some(live) = ws.live.get(&studio) else { continue };
        let Some(src) = &live.source else { continue };
        if !sources.iter().any(|s| s.element == src.element) {
            sources.push(src.clone());
        }
        work.push((id, v.clone(), live.state.clone()));
    }
    let cache = world.resource::<ViewCache>();
    let asm_views: Vec<(ViewId, Old, PendingView)> = asm_work
        .into_iter()
        .map(|(id, v, state)| {
            use cadrs_core::drawing_assembly::{AssemblyState, request};
            let old = match cache.geometry(&v) {
                Some(g) => Old::Known(Some(g)),
                None => match d.source(v.reference.element).and_then(|s| AssemblyState::parse(&s.snapshot)) {
                    Some(st) => Old::Pending(request(st, &v)),
                    None => Old::Known(None),
                },
            };
            (id, old, request((*state).clone(), &v))
        })
        .collect();
    let mut views: Vec<(ViewId, Old, PendingView)> = work
        .into_iter()
        .map(|(id, v, state)| {
            // What the view showed: its cached geometry, else a projection of the drawing's state
            // (a view on a sheet not shown since the drawing was opened).
            let old = match cache.geometry(&v) {
                Some(g) => Old::Known(Some(g)),
                None => match d.source(v.reference.element).and_then(|s| StudioState::parse(&s.snapshot)) {
                    Some(st) => Old::Pending(cadrs_core::views::request(st.features.clone(), ViewCache::request(&st, &v))),
                    None => Old::Known(None),
                },
            };
            let req = ViewCache::request(&state, &v);
            (id, old, cadrs_core::views::request(state.features.clone(), req))
        })
        .collect();
    views.extend(asm_views);
    world.resource_mut::<Workspace>().pending = Some(PendingUpdate { drawing, sources, views, done: Vec::new() });
}

/// Applies an update once its projections are done.
fn drive_update(world: &mut World) {
    let finished = {
        let mut ws = world.resource_mut::<Workspace>();
        let Some(p) = &mut ws.pending else { return };
        let mut i = 0;
        while i < p.views.len() {
            if let Old::Pending(o) = &p.views[i].1
                && let Some(r) = o.poll()
            {
                p.views[i].1 = Old::Known(r.ok());
            }
            if matches!(p.views[i].1, Old::Pending(_)) {
                i += 1;
                continue;
            }
            match p.views[i].2.poll() {
                Some(r) => {
                    let (id, old, _) = p.views.remove(i);
                    let old = match old {
                        Old::Known(g) => g,
                        Old::Pending(_) => None,
                    };
                    match r {
                        Ok(g) => p.done.push((id, old, g)),
                        Err(e) => warn!("Update: a view didn't project: {e}"),
                    }
                }
                None => i += 1,
            }
        }
        p.views.is_empty()
    };
    if !finished {
        return;
    }
    let Some(p) = world.resource_mut::<Workspace>().pending.take() else { return };
    let Some(d) = world.get_resource::<ActiveDocument>().and_then(|doc| doc.doc.element(p.drawing)?.drawing_data().cloned()) else {
        return;
    };
    let models: Vec<(ViewId, Option<&dyn ViewModel>, &dyn ViewModel)> = p
        .done
        .iter()
        .map(|(id, old, new)| (*id, old.as_deref().map(|g| g as &dyn ViewModel), &**new as &dyn ViewModel))
        .collect();
    let op = cadrs_drawing::update::update_op(&d, p.sources.clone(), &models);
    // With the BOM tables of the assemblies (P3C.5, D14.8), in the same step.
    let op = match world.get_resource::<ActiveDocument>() {
        Some(doc) => cadrs_core::drawing_source::with_bom_tables(&doc.doc, &d, op),
        None => op,
    };
    // The new projections are what the views show after the update.
    {
        let mut cache = world.resource_mut::<ViewCache>();
        for (id, _, g) in &p.done {
            if let Some((_, v)) = d.view(*id)
                && let Some(src) = p.sources.iter().find(|s| s.element == v.reference.element)
            {
                let key = if src.assembly.is_some() { ViewCache::assembly_key_with(&src.snapshot, v) } else { ViewCache::key_with(&src.snapshot, v) };
                cache.insert(key, g.clone());
            }
        }
    }
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&cadrs_core::commands::EditDrawing { element: p.drawing, op })
    {
        warn!("Update refused: {e}");
    }
    // Annotations of the old model are no longer selected or dragged.
    let mut ann = world.resource_mut::<super::annotations::AnnotationUi>();
    ann.drag = None;
    ann.drag_preview = None;
}

/// The Update button's normal colours (kept when it turns gold).
#[derive(Component, Clone)]
struct NormalVisuals(cadrs_ui::Visuals);

/// Onshape's gold (`ex3-step8.png`), a shade deeper so the glyph reads on white.
pub fn gold() -> Color {
    Color::srgb_u8(0xe6, 0xa5, 0x00)
}

/// Gold while the drawing is out of date, with a tooltip that says so.
#[allow(clippy::type_complexity)]
fn sync_update_button(
    ws: Res<Workspace>,
    mut q: Query<(Entity, &Name, &mut cadrs_ui::Visuals, Option<&NormalVisuals>, Option<&Tooltip>)>,
    mut commands: Commands,
) {
    for (e, name, mut vis, normal, tip) in &mut q {
        if name.as_str() != "drawing-update" {
            continue;
        }
        let Some(normal) = normal.cloned() else {
            commands.entity(e).insert(NormalVisuals(vis.clone()));
            continue;
        };
        let n = ws.stale.len() + ws.stale_boms.len();
        // Gold glyph on the usual button (`ex3-step8.png`: the icon itself turns gold).
        let want = if n > 0 {
            let mut v = normal.0.clone();
            v.foreground = cadrs_ui::StateColors::new(gold(), Color::srgb_u8(0xc9, 0x8e, 0x00), Color::srgb_u8(0xc9, 0x8e, 0x00), gold());
            v
        } else {
            normal.0.clone()
        };
        if *vis != want {
            *vis = want;
        }
        let text = match (n, ws.updating()) {
            (_, true) => "Updating from this workspace… (Ctrl+Q)".to_string(),
            (0, _) => "Update from this workspace: the drawing is up to date (Ctrl+Q)".to_string(),
            (_, _) if !ws.stale_boms.is_empty() => {
                let v = ws.stale.len();
                let b = ws.stale_boms.len();
                let views = match v {
                    0 => String::new(),
                    1 => "1 view, ".to_string(),
                    v => format!("{v} views, "),
                };
                let tables = if b == 1 { "1 BOM table".to_string() } else { format!("{b} BOM tables") };
                format!("Update from this workspace: {views}{tables} out of date (Ctrl+Q)")
            }
            (1, _) => "Update from this workspace: 1 view is out of date (Ctrl+Q)".to_string(),
            (n, _) => format!("Update from this workspace: {n} views are out of date (Ctrl+Q)"),
        };
        let want_tip = Tooltip::new(text);
        if tip != Some(&want_tip) {
            commands.entity(e).insert(want_tip);
        }
    }
}
