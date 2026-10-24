//! The CAM Studio tab (`cadrs_core::cam`): Onshape's CAM Studio workflow for mills, routers,
//! lasers, plasma tables and waterjets.
//!
//! - **Toolbar** (left to right, each active once the step before it is done): Insert part,
//!   workholding or stock · Create job · Create machine · Create setup · Create tool · Create
//!   toolpath · Create work plane · Regenerate · Preview operation · Post · Show stock.
//! - **Panel**: the **Jobs** tree (job → machine → setups → their tools, toolpaths and work
//!   planes; a toolpath's row shows its status: its time when calculated, out of date,
//!   calculating, failed) and the **Components** list (parts, workholding and stock with
//!   their reference and an eye). A click selects (a job: makes it the active one),
//!   double-click edits, right-click has Edit, Regenerate, Preview, Post, Move up/down,
//!   Duplicate, Suppress, Hide and Delete; Delete deletes the selection.
//! - **Dialogs** ([`dialogs`]): Insert component, Job, Machine (with its post settings), Setup,
//!   Tool (from the library or made here), Toolpath (Tool path, Link, Misc and Beam tabs; the
//!   toolpath previews live while it's edited) and Work plane. Every change is one
//!   `EditCam` command, so it undoes.
//! - **Computing** ([`compute`]): toolpaths are calculated off the main thread and kept by
//!   their inputs (the job cache).
//! - **View** ([`view`]): the stock, the work coordinate systems, the toolpaths (rapids red,
//!   cuts blue, entries green, leads cyan, links yellow, beam cuts orange).
//! - **Preview operation** ([`preview`]): Back plot, Verify (the stock as the toolpaths leave
//!   it, gouges red) and Simulate (with the tool and holder).
//! - **Post** ([`export`]): G-code for the job's machine, saved to a file.

pub mod compute;
pub mod dialogs;
pub mod export;
pub mod preview;
mod toolbar;
mod tree;
pub mod view;
pub use toolbar::*;
pub use tree::*;

use std::collections::HashSet;
use std::time::Instant;

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::cam::{CamId, CamStudio, Edit, EditCam, NodeRef, OpKind};
use cadrs_core::{ElementId, ElementKind};
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    ContextMenuRequested, MenuAction, Notification, TreeItem, TreeRowToggled, TreeToggle,
    open_context_menu, show_notification, toolbar_separator,
};

use crate::{ActiveDocument, AppState};

pub use compute::{Computed, Status};

/// The panel's width on a CAM Studio tab.
pub const PANEL_W: f32 = 290.0;

/// The CAM Studio dialogs' widget names: `cam-f-<key>`.
pub const FORM_PREFIX: &str = "cam-f-";

/// A number typed into a CAM Studio dialog, as the sketch's dimension fields read it:
/// expressions and units (`6*2`, `1/4 in`, `10 mm + 2`, `30 deg`).
fn read_number(text: &str, unit: cadrs_ui::form::Unit) -> Option<f64> {
    use cadrs_sketch::units::{Quantity, eval};
    use cadrs_ui::form::Unit;
    let v = match unit {
        Unit::Mm => eval(text, Quantity::Length).ok()?,
        Unit::Deg => eval(text, Quantity::Angle).ok()?,
        // (Its own suffix off: what's left is a plain expression.)
        _ => {
            let t = text.trim();
            let body = [" mm/min", "mm/min", " rpm", "rpm", " %", "%", " s", "s"]
                .iter()
                .find_map(|s| t.strip_suffix(s))
                .unwrap_or(t);
            eval(body, Quantity::Length).ok()?
        }
    };
    v.is_finite().then_some(v)
}

pub struct CamUiPlugin;

impl Plugin for CamUiPlugin {
    fn build(&self, app: &mut App) {
        cadrs_core::cam::geometry::register_tasks();
        app.init_resource::<CamUi>()
            .init_resource::<Computed>()
            .add_plugins((
                cadrs_ui::form::FormPlugin::new(FORM_PREFIX).eval(read_number),
                dialogs::DialogsPlugin,
                view::ViewPlugin,
                preview::PreviewPlugin,
                export::ExportPlugin,
            ))
            .add_systems(
                Update,
                (compute::drive, rebuild_tree, sync_toolbar, sync_parts)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                OnExit(AppState::Document),
                |mut ui: ResMut<CamUi>, mut c: ResMut<Computed>| {
                    *ui = CamUi::default();
                    c.clear();
                },
            )
            .add_observer(on_row_activate)
            .add_observer(on_row_toggle)
            .add_observer(on_row_eye)
            .add_observer(on_row_menu)
            .add_observer(on_menu_action)
            .add_observer(on_insert_menu_action)
            .add_systems(Update, delete_key.run_if(in_state(AppState::Document)))
            .add_systems(
                PostUpdate,
                flag_pending_work.run_if(in_state(AppState::Document)),
            );
    }
}

/// The CAM Studio panel's view state (not part of the document).
#[derive(Resource, Debug, Clone)]
pub struct CamUi {
    /// The selected rows.
    pub selected: Vec<CamId>,
    /// Collapsed jobs and setups.
    pub collapsed: HashSet<CamId>,
    /// Show the active job's stock in the view.
    pub show_stock: bool,
    /// The last row clicked and when (double-clicks edit).
    last_click: Option<(CamId, Instant)>,
    /// Frame the parts once they're built (the first ones were just inserted).
    pub fit_pending: bool,
}

impl Default for CamUi {
    fn default() -> Self {
        CamUi {
            selected: Vec::new(),
            collapsed: HashSet::new(),
            show_stock: true,
            last_click: None,
            fit_pending: false,
        }
    }
}

/// Scripted steps and screenshots wait while toolpaths are calculated (and while a dialog's
/// live preview is about to be).
fn flag_pending_work(
    computed: Res<Computed>,
    dialog: Option<Res<dialogs::CamDialog>>,
    preview: Option<Res<preview::PreviewState>>,
    insert: Option<Res<dialogs::InsertParts>>,
    mut pending: ResMut<cadrs_ui::PendingWork>,
    mut why: ResMut<cadrs_ui::PendingWhy>,
) {
    let waiting = dialog.is_some_and(|d| d.waiting());
    if computed.busy() || waiting {
        pending.0 = true;
        why.add("cam toolpaths");
    }
    if insert.is_some() {
        pending.0 = true;
        why.add("cam insert parts");
    }
    if preview.is_some_and(|p| p.checking()) {
        pending.0 = true;
        why.add("cam verify");
    }
}

/// Scripted steps (`Custom("cam …")`): `select job|setup|op N` selects the Nth job, the active
/// job's Nth setup or its first setup's Nth operation (from 0); `play` and `pause` the preview;
/// `preview-end` jumps it to the end; `preview-at F` to a fraction of it; `fit` zooms the view
/// to the studio's parts and stock.
pub fn script(world: &mut World, arg: &str) {
    let words: Vec<&str> = arg.split_whitespace().collect();
    let Some((_, cam)) = active_cam_owned(world) else {
        warn!("cam {arg}: no CAM Studio");
        return;
    };
    match words.as_slice() {
        ["select", what, n] => {
            let Ok(n) = n.parse::<usize>() else { return };
            let id = match *what {
                "job" => cam.jobs.get(n).map(|j| j.id),
                "setup" => cam.active().and_then(|j| j.setups.get(n)).map(|s| s.id),
                "op" => cam
                    .active()
                    .and_then(|j| j.setups.first())
                    .and_then(|s| s.operations.get(n))
                    .map(|o| o.id),
                "component" => cam.components.get(n).map(|c| c.id),
                _ => None,
            };
            match id {
                Some(id) => world.resource_mut::<CamUi>().selected = vec![id],
                None => warn!("cam {arg}: nothing there"),
            }
        }
        ["play"] | ["pause"] | ["preview-end"] | ["preview-at", _] => {
            let Some(mut s) = world.get_resource_mut::<preview::PreviewState>() else {
                warn!("cam {arg}: no preview");
                return;
            };
            match words[0] {
                "play" => s.playing = true,
                "pause" => s.playing = false,
                "preview-end" => {
                    s.playing = false;
                    s.seek(1.0);
                }
                _ => {
                    s.playing = false;
                    if let Ok(f) = words[1].parse::<f64>() {
                        s.seek(f);
                    }
                }
            }
        }
        ["fit"] => crate::viewport::zoom_to_fit(world),
        _ => warn!("cam {arg}: not a CAM Studio step"),
    }
}

/// The active tab's CAM Studio.
pub fn active_cam(doc: &ActiveDocument) -> Option<(ElementId, &CamStudio)> {
    let el = doc.active_element()?;
    match &el.kind {
        ElementKind::CamStudio(c) => Some((el.id, c)),
        _ => None,
    }
}

/// The active tab's id when it is a CAM Studio.
pub fn active_cam_id(world: &World) -> Option<ElementId> {
    active_cam(world.get_resource::<ActiveDocument>()?).map(|(e, _)| e)
}

/// A copy of the active CAM Studio (for work that also needs the world).
pub fn active_cam_owned(world: &World) -> Option<(ElementId, CamStudio)> {
    active_cam(world.get_resource::<ActiveDocument>()?).map(|(e, c)| (e, c.clone()))
}

/// Runs an edit on the active CAM Studio; a refused one shows why.
pub fn edit(world: &mut World, e: Edit) -> bool {
    let Some(element) = active_cam_id(world) else {
        return false;
    };
    let r = world
        .resource_mut::<ActiveDocument>()
        .execute(&EditCam::new(element, e));
    match r {
        Ok(()) => true,
        Err(err) => {
            toast(world, &err.to_string());
            false
        }
    }
}

/// A warning toast.
pub fn toast(world: &mut World, text: &str) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    show_notification(
        &mut commands,
        &theme,
        Notification::warning(text.to_string()).name("cam-toast"),
    );
    world.flush();
}

/// The setup new tools and toolpaths go into, and the operation they go after: the selected
/// setup or operation's, else the active job's last setup (at its end).
pub fn target_setup(cam: &CamStudio, ui: &CamUi) -> Option<(CamId, Option<CamId>)> {
    if let Some(&sel) = ui.selected.last() {
        match cam.find(sel) {
            Some(NodeRef::Setup(j, s)) => return Some((cam.jobs[j].setups[s].id, None)),
            Some(NodeRef::Operation(j, s, _)) => {
                return Some((cam.jobs[j].setups[s].id, Some(sel)));
            }
            _ => {}
        }
    }
    let job = cam.active()?;
    job.setups.last().map(|s| (s.id, None))
}

/// What [`sync_parts`] last set up: the hidden and stock components, the parts' generation.
type PartsKey = (Vec<CamId>, Vec<CamId>, u64);

/// Hidden components' parts are left out of the view; stock components are see-through.
fn sync_parts(
    doc: Option<Res<ActiveDocument>>,
    mut cache: ResMut<crate::parts::PartCache>,
    ui: Res<CamUi>,
    mut last: Local<Option<PartsKey>>,
    mut commands: Commands,
) {
    let ui_fit = |c: &crate::parts::PartCache| ui.fit_pending && !c.parts.is_empty();
    let Some(doc) = doc else { return };
    let Some((el, cam)) = active_cam(&doc) else {
        *last = None;
        return;
    };
    if cache.settled().is_none_or(|(id, _, _)| id != el) {
        return;
    }
    let hidden: Vec<CamId> = cam
        .components
        .iter()
        .filter(|c| c.hidden)
        .map(|c| c.id)
        .collect();
    let stock: Vec<CamId> = cam
        .components
        .iter()
        .filter(|c| c.role == cadrs_core::cam::Role::Stock)
        .map(|c| c.id)
        .collect();
    let key = (hidden.clone(), stock.clone(), cache.generation);
    if last.as_ref() == Some(&key) {
        return;
    }
    let shown: Vec<cadrs_core::PartId> = cache
        .parts
        .iter()
        .filter(|p| cam.component_of(p.id).is_none_or(|c| !c.hidden))
        .map(|p| p.id)
        .collect();
    let see: Vec<cadrs_core::PartId> = cache
        .parts
        .iter()
        .filter(|p| {
            cam.component_of(p.id)
                .is_some_and(|c| stock.contains(&c.id))
        })
        .map(|p| p.id)
        .collect();
    cache.isolate(if hidden.is_empty() { None } else { Some(shown) });
    if ui_fit(&cache) {
        commands.queue(|w: &mut World| {
            if std::mem::take(&mut w.resource_mut::<CamUi>().fit_pending) {
                crate::viewport::zoom_to_fit(w);
            }
        });
    }
    if !see.is_empty() {
        cache.set_transparent(&see, true);
    }
    *last = Some((hidden, stock, cache.generation));
}

#[cfg(test)]
mod number_tests {
    use super::read_number;
    use cadrs_ui::form::Unit;

    #[test]
    fn typed_numbers_read_as_expressions() {
        // As the dialogs read them: the expression first, the plain reading after.
        let f = cadrs_ui::form::FormSettings {
            prefix: super::FORM_PREFIX.into(),
            eval: Some(read_number),
        };
        let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        assert!(near(f.read("6*2", Unit::Mm), 12.0));
        assert!(near(f.read("1/4 in", Unit::Mm), 6.35));
        assert!(near(f.read("0.25 in", Unit::Mm), 6.35));
        assert!(near(f.read("10 mm + 2", Unit::Mm), 12.0));
        assert!(near(f.read("30 deg", Unit::Deg), 30.0));
        assert!(near(f.read("1200 mm/min", Unit::Feed), 1200.0));
        assert!(near(f.read("2000*1.5 rpm", Unit::Rpm), 3000.0));
        assert_eq!(f.read("six", Unit::Mm), None);
    }
}

#[cfg(test)]
mod icon_tests {
    use cadrs_core::cam::cadrs_cam::{MachineKind, Pattern, ToolKind};

    /// Every pattern, tool and machine names an icon icon-rs has.
    #[test]
    fn cam_icons_exist() {
        let names = Pattern::ALL
            .iter()
            .map(|p| p.icon())
            .chain(ToolKind::ALL.iter().map(|t| t.icon()))
            .chain(MachineKind::ALL.iter().map(|m| m.icon()));
        for n in names {
            assert!(cadrs_ui::icon::has_icon(n), "{n}");
        }
    }
}
