//! The CAM Studio dialogs: Insert component, Job, Machine, Setup, Tool, Toolpath, Work plane and
//! Edit component, as Onshape's (a feature dialog card at the top left of the view, ✓ and ✕).
//!
//! Each dialog is a [`Draft`] being edited: [`Draft::fields`] lays it out with [`form`],
//! [`Draft::set`] takes a changed field, [`Draft::pick`] a pick in the view (bodies,
//! workholding, drive edges, faces and holes, the face a work plane's Z follows), and
//! [`Draft::commit`] gives the edits ✓ makes, one undo step each. A toolpath previews live: its
//! draft is calculated (a moment after the last change) and drawn in the view; ✓ keeps the
//! result (the job cache).

use bevy::prelude::*;
use cadrs_core::cam::cadrs_cam::feeds::{self, WorkMaterial};
use cadrs_core::cam::cadrs_cam::machine::{Machine, MachineKind, catalog};
use cadrs_core::cam::cadrs_cam::post::LaserMode;
use cadrs_core::cam::cadrs_cam::strategy::{
    Compensation, CuttingMethod, Direction, Entry, HeightRef, HoleSelect, LeadKind, LinkAction,
    MachineBy, Params, Pattern, Side, SubPattern, Type,
};
use cadrs_core::cam::cadrs_cam::tool::{
    Hand, MachiningType, Profile, Template, Tool, ToolKind, ToolLibrary, Unit as LenUnit,
};
use cadrs_core::cam::cadrs_cam::toolpath::{Coolant, CycleKind};
use cadrs_core::cam::{
    CamId, CamStudio, Component, Edit, Job, Mating, NodeRef, OpKind, Operation, Origin, Role,
    Setup, StockDef, StockDirection, StockKind, ToolOp, ToolpathOp, WorkPlane, WorkPlaneType,
    geometry,
};
use cadrs_core::{ElementId, ElementKind, PartId};
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialog, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState};

use super::{CamRow, CamUi, active_cam_owned, edit, target_setup, toast};
use crate::viewport::{Pick, PickRequest, ViewportArea};
use crate::{ActiveDocument, AppState};
use cadrs_ui::form::{self, Field, FieldChange, FieldUnits, Unit, Value};

mod fields;
mod set;
#[cfg(test)]
mod tests;
mod toolpath_fields;
use fields::*;
use set::*;
use toolpath_fields::*;

pub struct DialogsPlugin;

impl Plugin for DialogsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                take_insert_parts,
                take_changes,
                take_picks,
                sync_dialog,
                live_preview,
            )
                .chain()
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<CamDialog>();
        })
        .add_observer(on_accept)
        .add_observer(on_cancel);
    }
}

/// The open dialog.
#[derive(Resource)]
pub struct CamDialog {
    pub draft: Draft,
    /// The pick list that takes the view's picks.
    pub picking: Option<String>,
    /// The toolpath's tab (Tool path, Link, Misc, Beam).
    pub tab: usize,
    /// The fields the dialog shows (rebuilt when they change).
    shown: Option<Vec<Field>>,
    /// When the draft last changed (the live preview waits a moment).
    changed: Option<web_time::Instant>,
}

impl CamDialog {
    /// A change the live preview hasn't taken yet (a toolpath's).
    pub fn waiting(&self) -> bool {
        (self.changed.is_some() && matches!(self.draft, Draft::Toolpath { .. }))
            || matches!(&self.draft, Draft::Insert(d) if d.others_pending)
    }
}

/// What a dialog edits.
#[derive(Debug, Clone)]
pub enum Draft {
    Insert(InsertDraft),
    Component(Box<Component>),
    Job {
        job: Box<Job>,
        is_new: bool,
    },
    Machine {
        job: CamId,
        machine: Box<Machine>,
        /// The catalogue entry it started from (`None`: as the job had it).
        from: Option<usize>,
        by_kind: bool,
    },
    Setup {
        job: CamId,
        setup: Box<Setup>,
        is_new: bool,
    },
    Tool {
        setup: CamId,
        after: Option<CamId>,
        op: Box<Operation>,
        is_new: bool,
        library: bool,
        filter: Option<MachiningType>,
    },
    Toolpath {
        setup: CamId,
        after: Option<CamId>,
        op: Box<Operation>,
        is_new: bool,
        /// The type picked (the patterns offered).
        kind: Type,
    },
    WorkPlane {
        setup: CamId,
        after: Option<CamId>,
        op: Box<Operation>,
        is_new: bool,
    },
}

/// Insert part, workholding or stock.
#[derive(Debug, Clone)]
pub struct InsertDraft {
    pub role: Role,
    /// 0: this document; 1…: one of `others`.
    pub document: usize,
    /// The library's other documents with versions: (id, name).
    pub others: Vec<(cadrs_core::DocumentId, String)>,
    /// The library is still being read (`others` comes when it's done).
    pub others_pending: bool,
    /// The other document chosen, opened at its version.
    pub other: Option<OtherDoc>,
    /// The Part Studio (an index into the document's Part Studios).
    pub studio: usize,
    /// This document: 0 the workspace, 1… its versions. Another: an index into its versions.
    pub version: usize,
    /// The studio's parts: (id, name, ticked).
    pub parts: Vec<(PartId, String, bool)>,
}

/// Another document an insert takes parts from: its versions, newest last, and the Part
/// Studios of the one chosen.
#[derive(Debug, Clone)]
pub struct OtherDoc {
    pub versions: Vec<(cadrs_core::history_log::VersionId, String)>,
    pub studios: Vec<(ElementId, String)>,
}

/// The library's other documents that have a version to take parts from (`None` while the
/// library is still being read).
fn other_documents(world: &mut World) -> Option<Vec<(cadrs_core::DocumentId, String)>> {
    let b = crate::linked::Browse {
        location: crate::linked::Location::Mine,
        search: String::new(),
        opened: None,
        graph: false,
    };
    let this = world.resource::<ActiveDocument>().doc.id;
    let list = crate::linked::browser_list(world, &b);
    (!list.loading).then(|| {
        list.rows
            .into_iter()
            .filter(|r| r.versions && r.id != this)
            .map(|r| (r.id, r.name))
            .collect()
    })
}

/// Opens another document at a version (its newest when `None`): its versions and the Part
/// Studios it has there.
fn load_other(
    world: &mut World,
    document: cadrs_core::DocumentId,
    version: Option<cadrs_core::history_log::VersionId>,
) -> Option<OtherDoc> {
    let versions: Vec<(cadrs_core::history_log::VersionId, String)> =
        crate::linked::versions_of(world, document)
            .into_iter()
            .map(|(id, name, _)| (id, name))
            .collect();
    let opened = match crate::linked::open(world, document, version) {
        Ok(o) => o,
        Err(e) => {
            toast(world, &e);
            return None;
        }
    };
    let studios = opened
        .doc
        .elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| (e.id, e.name.clone()))
        .collect();
    Some(OtherDoc { versions, studios })
}

/// What a draft reads from the document.
pub struct Ctx<'a> {
    pub doc: &'a cadrs_core::Document,
    pub cam: &'a CamStudio,
    /// The studio's built parts.
    pub parts: &'a [cadrs_core::Part],
    /// This document's versions (name, id).
    pub versions: &'a [(String, cadrs_core::history_log::VersionId)],
}

impl Ctx<'_> {
    fn part_studios(&self) -> Vec<(ElementId, String)> {
        self.doc
            .elements
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
            .map(|e| (e.id, e.name.clone()))
            .collect()
    }

    fn part_name(&self, id: PartId) -> String {
        self.parts
            .iter()
            .find(|p| p.id == id)
            .map(|p| {
                let props = self
                    .cam
                    .component_of(id)
                    .map(|c| c.derived.props.clone())
                    .unwrap_or_default();
                cadrs_core::parts::display_name(p, &props).to_string()
            })
            .or_else(|| self.cam.component_of(id).map(|c| c.name.clone()))
            .unwrap_or_else(|| "(missing part)".into())
    }
}

// ---------------------------------------------------------------------------------------------
// Opening

fn ctx_parts(world: &World) -> Vec<cadrs_core::Part> {
    super::active_cam_id(world)
        .and_then(|e| super::compute::studio_parts(world, e))
        .unwrap_or_default()
}

fn versions(world: &World) -> Vec<(String, cadrs_core::history_log::VersionId)> {
    world
        .get_resource::<crate::history_panel::DocLog>()
        .and_then(|l| l.log.as_ref())
        .map(|l| {
            l.versions()
                .iter()
                .map(|v| (v.name().to_string(), v.id()))
                .collect()
        })
        .unwrap_or_default()
}

/// Opens a dialog on a draft (closing any other first).
pub fn open(world: &mut World, draft: Draft) {
    close(world);
    let picking = match &draft {
        Draft::Job { job, .. } if job.bodies.is_empty() => None,
        _ => None,
    };
    world.insert_resource(CamDialog {
        draft,
        picking,
        tab: 0,
        shown: None,
        changed: Some(web_time::Instant::now()),
    });
}

/// Closes the dialog (no edits).
pub fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CamDialogRoot>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.entity_mut(e).despawn();
    }
    world.remove_resource::<CamDialog>();
    world
        .resource_mut::<crate::viewport::PickFilterOverride>()
        .0 = None;
    world
        .resource_mut::<super::compute::Computed>()
        .clear_preview();
}

pub fn insert_component(world: &mut World, role: Role) {
    let Some((_, _cam)) = active_cam_owned(world) else {
        return;
    };
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let studios: Vec<ElementId> = doc
        .elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
        .collect();
    let found = other_documents(world);
    let others_pending = found.is_none();
    let others = found.unwrap_or_default();
    if studios.is_empty() && others.is_empty() && !others_pending {
        toast(world, &tr("cam-no-part-studio"));
        return;
    }
    let mut d = InsertDraft {
        role,
        document: 0,
        others,
        others_pending,
        other: None,
        studio: 0,
        version: 0,
        parts: Vec::new(),
    };
    // A document without Part Studios of its own starts on the first other one.
    if studios.is_empty() && !d.others.is_empty() {
        d.document = 1;
        d.other = load_other(world, d.others[0].0, None);
        d.version = d
            .other
            .as_ref()
            .map_or(0, |o| o.versions.len().saturating_sub(1));
    }
    fill_parts(world, &mut d);
    open(world, Draft::Insert(d));
}

/// The parts of the draft's Part Studio at its reference.
/// Lists the chosen studio's parts: rebuilt on a worker thread ([`take_insert_parts`] puts
/// them in the dialog when they come).
fn fill_parts(world: &mut World, d: &mut InsertDraft) {
    d.parts.clear();
    let Some(d2) = source_of(world, d) else {
        world.remove_resource::<InsertParts>();
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = cadrs_core::background::spawn("cam-insert-parts", move || {
        let parts = std::panic::catch_unwind(|| {
            cadrs_core::derived::source_parts(&d2.0).unwrap_or_default()
        })
        .unwrap_or_default();
        let _ = tx.send(parts);
    });
    if spawned.is_ok() {
        world.insert_resource(InsertParts {
            studio: d.studio,
            version: d.version,
            rx: std::sync::Mutex::new(rx),
        });
    }
}

/// An insert dialog's studio being rebuilt for its part list.
#[derive(Resource)]
pub struct InsertParts {
    studio: usize,
    version: usize,
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<Vec<(PartId, String)>>>,
}

/// Puts the rebuilt part list in the insert dialog (if it still shows that studio), and the
/// library's other documents once they're read.
fn take_insert_parts(world: &mut World) {
    let pending = world
        .get_resource::<CamDialog>()
        .is_some_and(|d| matches!(&d.draft, Draft::Insert(i) if i.others_pending));
    if pending && let Some(others) = other_documents(world) {
        let mut dlg = world.resource_mut::<CamDialog>();
        if let Draft::Insert(d) = &mut dlg.draft {
            d.others = others;
            d.others_pending = false;
        }
        dlg.shown = None;
    }
    let Some(mut job) = world.get_resource_mut::<InsertParts>() else {
        return;
    };
    let got = match job.rx.get_mut().map(|r| r.try_recv()) {
        Ok(Ok(p)) => p,
        Ok(Err(std::sync::mpsc::TryRecvError::Empty)) => return,
        _ => Vec::new(),
    };
    let (studio, version) = (job.studio, job.version);
    world.remove_resource::<InsertParts>();
    if let Some(mut dlg) = world.get_resource_mut::<CamDialog>()
        && let Draft::Insert(d) = &mut dlg.draft
        && d.studio == studio
        && d.version == version
    {
        d.parts = got
            .into_iter()
            .enumerate()
            .map(|(i, (id, name))| (id, name, i == 0 && d.role == Role::Part))
            .collect();
        dlg.shown = None;
    }
}

/// The Derived reference an insert draft makes (and the frozen copies a version needs).
fn source_of(
    world: &mut World,
    d: &InsertDraft,
) -> Option<(
    cadrs_core::derived::DerivedFeature,
    Vec<cadrs_core::external::LinkedElement>,
)> {
    let doc = world.resource::<ActiveDocument>().doc.clone();
    // Another document's Part Studio, at one of its versions.
    if d.document > 0 {
        let (other_id, _) = *d.others.get(d.document - 1)?;
        let o = d.other.as_ref()?;
        let element = o.studios.get(d.studio)?.0;
        let vid = o.versions.get(d.version)?.0;
        let log = world.resource::<crate::history_panel::DocLog>().log.clone();
        let got = {
            let mut res = crate::linked::resolver(world);
            cadrs_core::derived::resolve(
                &mut res.0,
                &doc,
                log.as_ref(),
                cadrs_core::derived::DerivedFeature::default(),
                cadrs_core::external::SourceRef::version(Some(other_id), element, vid),
            )
        };
        return match got {
            Ok(r) => Some((r.derived, r.links)),
            Err(e) => {
                toast(world, &e.to_string());
                None
            }
        };
    }
    let studios: Vec<ElementId> = doc
        .elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
        .collect();
    let element = *studios.get(d.studio)?;
    if d.version == 0 {
        let el = doc.element(element)?;
        let mut f = cadrs_core::derived::DerivedFeature::new(None, element);
        f.fill_from(el, "", "");
        return Some((f, Vec::new()));
    }
    let vs = versions(world);
    let (_, vid) = vs.get(d.version - 1)?.clone();
    let log = world.resource::<crate::history_panel::DocLog>().log.clone();
    let got = {
        let mut res = crate::linked::resolver(world);
        cadrs_core::derived::resolve(
            &mut res.0,
            &doc,
            log.as_ref(),
            cadrs_core::derived::DerivedFeature::default(),
            cadrs_core::external::SourceRef::version(None, element, vid),
        )
    };
    match got {
        Ok(r) => Some((r.derived, r.links)),
        Err(e) => {
            toast(world, &e.to_string());
            None
        }
    }
}

pub fn create_job(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let job = Job {
        id: CamId::new(),
        name: cam.next_name("Job"),
        bodies: Vec::new(),
        stock: StockDef::default(),
        workholdings: Vec::new(),
        machine: None,
        setups: Vec::new(),
        suppressed: false,
    };
    open(
        world,
        Draft::Job {
            job: Box::new(job),
            is_new: true,
        },
    );
    if let Some(mut d) = world.get_resource_mut::<CamDialog>() {
        d.picking = Some("bodies".into());
    }
}

pub fn create_machine(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let Some(job) = cam.active() else {
        return;
    };
    edit_machine(world, job.id);
}

/// Create or edit a job's machine.
pub fn edit_machine(world: &mut World, job: CamId) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let Some(j) = cam.job(job) else { return };
    let (machine, from) = match &j.machine {
        Some(m) => (m.clone(), None),
        None => (catalog().remove(0), Some(0)),
    };
    open(
        world,
        Draft::Machine {
            job,
            machine: Box::new(machine),
            from,
            by_kind: true,
        },
    );
}

pub fn create_setup(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    // The selected job, else the active one.
    let sel = world.resource::<CamUi>().selected.last().copied();
    let job = sel
        .and_then(|s| match cam.find(s) {
            Some(NodeRef::Job(j))
            | Some(NodeRef::Setup(j, _))
            | Some(NodeRef::Operation(j, ..)) => Some(cam.jobs[j].id),
            _ => None,
        })
        .or_else(|| cam.active().map(|j| j.id));
    let Some(job) = job else { return };
    if cam.job(job).is_some_and(|j| j.machine.is_none()) {
        toast(world, &tr("cam-create-machine-first"));
        return;
    }
    let n = cam.job(job).map_or(0, |j| j.setups.len()) as u32;
    let setup = Setup {
        id: CamId::new(),
        name: cam.next_name("Setup"),
        program_number: 1001 + n,
        mating: Mating::Automatic,
        wcs: WorkPlane::default(),
        operations: Vec::new(),
        suppressed: false,
    };
    open(
        world,
        Draft::Setup {
            job,
            setup: Box::new(setup),
            is_new: true,
        },
    );
}

pub fn create_tool(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let ui = world.resource::<CamUi>().clone();
    let Some((setup, after)) = target_setup(&cam, &ui) else {
        toast(world, &tr("cam-create-setup-first"));
        return;
    };
    let Some((job, _)) = cam.setup(setup) else {
        return;
    };
    let kind = job.machine.as_ref().map_or(MachineKind::Mill, |m| m.kind);
    let tk = ToolKind::for_machine(kind)[0];
    let tool = Tool::new(
        tk,
        if tk.is_beam() {
            default_kerf(kind)
        } else {
            6.0
        },
    );
    let op = Operation {
        id: CamId::new(),
        name: tool.name.clone(),
        kind: OpKind::Tool(Box::new(ToolOp {
            tool,
            number: cam.next_tool_number(job.id),
        })),
        suppressed: false,
        hidden: false,
    };
    let library = cam.library.tools.iter().any(|t| t.kind.fits(kind));
    open(
        world,
        Draft::Tool {
            setup,
            after,
            op: Box::new(op),
            is_new: true,
            library,
            filter: None,
        },
    );
}

fn default_kerf(kind: MachineKind) -> f64 {
    match kind {
        MachineKind::Laser => 0.15,
        MachineKind::Plasma => 1.5,
        MachineKind::Waterjet => 0.9,
        _ => 6.0,
    }
}

pub fn create_toolpath(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let ui = world.resource::<CamUi>().clone();
    let Some((setup, after)) = target_setup(&cam, &ui) else {
        toast(world, &tr("cam-create-setup-first"));
        return;
    };
    let Some((job, s)) = cam.setup(setup) else {
        return;
    };
    let kind = job.machine.as_ref().map_or(MachineKind::Mill, |m| m.kind);
    // The tool it will use: the last above where it goes.
    let at = after
        .and_then(|a| s.operations.iter().position(|o| o.id == a))
        .map_or(s.operations.len(), |i| i + 1);
    let Some(tool) = s.operations[..at]
        .iter()
        .rev()
        .filter(|o| !o.suppressed)
        .find_map(|o| o.tool())
    else {
        toast(world, &tr("cam-create-tool-first"));
        return;
    };
    let types: Vec<Type> = Type::ALL
        .into_iter()
        .filter(|t| !Pattern::available(*t, tool.tool.kind, kind).is_empty())
        .collect();
    let Some(&t) = types.first() else {
        toast(world, &tr("cam-tool-no-patterns"));
        return;
    };
    let pattern = Pattern::available(t, tool.tool.kind, kind)[0];
    let params = Params::new(pattern, &tool.tool, kind);
    let op = Operation {
        id: CamId::new(),
        name: cam.next_name(pattern.label()),
        kind: OpKind::Toolpath(Box::new(ToolpathOp {
            params,
            name_edited: false,
        })),
        suppressed: false,
        hidden: false,
    };
    open(
        world,
        Draft::Toolpath {
            setup,
            after,
            op: Box::new(op),
            is_new: true,
            kind: t,
        },
    );
}

pub fn create_work_plane(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let ui = world.resource::<CamUi>().clone();
    let Some((setup, after)) = target_setup(&cam, &ui) else {
        toast(world, &tr("cam-create-setup-first"));
        return;
    };
    let wcs = cam.setup(setup).map(|(_, s)| s.wcs).unwrap_or_default();
    let op = Operation {
        id: CamId::new(),
        name: cam.next_name("Work plane"),
        kind: OpKind::WorkPlane(Box::new(WorkPlane { datum: None, ..wcs })),
        suppressed: false,
        hidden: false,
    };
    open(
        world,
        Draft::WorkPlane {
            setup,
            after,
            op: Box::new(op),
            is_new: true,
        },
    );
}

/// Edit… on a row (and a double-click).
pub fn edit_row(world: &mut World, row: CamRow) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    match row {
        CamRow::Component(id) => {
            if let Some(c) = cam.component(id) {
                open(world, Draft::Component(Box::new(c.clone())));
            }
        }
        CamRow::Job(id) => {
            if let Some(j) = cam.job(id) {
                open(
                    world,
                    Draft::Job {
                        job: Box::new(j.clone()),
                        is_new: false,
                    },
                );
            }
        }
        CamRow::Machine(id) => edit_machine(world, id),
        CamRow::Setup(id) => {
            if let Some((j, s)) = cam.setup(id) {
                open(
                    world,
                    Draft::Setup {
                        job: j.id,
                        setup: Box::new(s.clone()),
                        is_new: false,
                    },
                );
            }
        }
        CamRow::Operation(id) => {
            let Some((job, s, o)) = cam.operation(id) else {
                return;
            };
            let kind = job.machine.as_ref().map_or(MachineKind::Mill, |m| m.kind);
            let draft = match &o.kind {
                OpKind::Tool(_) => Draft::Tool {
                    setup: s.id,
                    after: None,
                    op: Box::new(o.clone()),
                    is_new: false,
                    library: false,
                    filter: None,
                },
                OpKind::Toolpath(tp) => Draft::Toolpath {
                    setup: s.id,
                    after: None,
                    op: Box::new(o.clone()),
                    is_new: false,
                    kind: tp.params.pattern.type_of(),
                },
                OpKind::WorkPlane(_) => Draft::WorkPlane {
                    setup: s.id,
                    after: None,
                    op: Box::new(o.clone()),
                    is_new: false,
                },
            };
            let _ = kind;
            open(world, draft);
        }
    }
}

/// A component at a version: to the newest version of its document.
pub fn update_component(world: &mut World, id: CamId) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let Some(c) = cam.component(id).cloned() else {
        return;
    };
    let Some(src) = c.derived.source else { return };
    let vs = versions(world);
    let Some((name, vid)) = vs.last().cloned() else {
        toast(world, &tr("cam-document-no-version"));
        return;
    };
    if c.derived.version_name == name {
        toast(world, &tr("cam-component-newest"));
        return;
    }
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let log = world.resource::<crate::history_panel::DocLog>().log.clone();
    let got = {
        let mut res = crate::linked::resolver(world);
        cadrs_core::derived::resolve(
            &mut res.0,
            &doc,
            log.as_ref(),
            c.derived.clone(),
            cadrs_core::external::SourceRef::version(src.document, src.element, vid),
        )
    };
    match got {
        Ok(r) => {
            let mut c2 = c.clone();
            c2.derived = r.derived;
            edit(
                world,
                Edit::SetComponent {
                    component: Box::new(c2),
                    links: r.links,
                },
            );
        }
        Err(e) => toast(world, &e.to_string()),
    }
}

// ---------------------------------------------------------------------------------------------
// Systems

/// Builds the context drafts read (outside the borrow of the dialog).
fn with_ctx<R>(world: &mut World, f: impl FnOnce(&mut World, &Ctx) -> R) -> Option<R> {
    let (_, cam) = active_cam_owned(world)?;
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let parts = ctx_parts(world);
    let vs = versions(world);
    let ctx = Ctx {
        doc: &doc,
        cam: &cam,
        parts: &parts,
        versions: &vs,
    };
    Some(f(world, &ctx))
}

fn take_changes(
    mut changes: MessageReader<FieldChange>,
    mut files: MessageReader<cadrs_ui::FilePicked>,
    mut commands: Commands,
) {
    let c: Vec<FieldChange> = changes.read().cloned().collect();
    let f: Vec<std::path::PathBuf> = files
        .read()
        .filter(|m| m.tag == "cam-library-import")
        .map(|m| m.path.clone())
        .collect();
    if c.is_empty() && f.is_empty() {
        return;
    }
    commands.queue(move |w: &mut World| apply_changes(w, c, f));
}

fn apply_changes(world: &mut World, changes: Vec<FieldChange>, files: Vec<std::path::PathBuf>) {
    for p in files {
        import_library(world, p);
    }
    if changes.is_empty() || !world.contains_resource::<CamDialog>() {
        return;
    }
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    for c in changes {
        let Some(mut d) = world.remove_resource::<CamDialog>() else {
            return;
        };
        // A click on a select or field (not a button or pick list) changes nothing.
        if c.value == Value::Activate && !is_pick_key(&c.key) && !is_button_key(&c.key) {
            world.insert_resource(d);
            continue;
        }
        match (c.key.as_str(), &c.value) {
            ("tab", Value::Index(i)) => d.tab = *i,
            (k, Value::Activate) if is_pick_key(k) => {
                d.picking = if d.picking.as_deref() == Some(k) {
                    None
                } else {
                    Some(k.to_string())
                };
            }
            ("p-z", Value::Index(i)) if *i >= AXES.len() => {
                d.picking = Some("p-face".into());
            }
            ("axis", Value::Index(1)) => {
                d.draft.set(world, &c.key, &c.value, &cam);
                d.picking = Some("axis-face".into());
            }
            _ => {
                d.draft.set(world, &c.key, &c.value, &cam);
            }
        }
        d.changed = Some(web_time::Instant::now());
        // (Built again: a typed value that couldn't be read shows the old one.)
        d.shown = None;
        world.insert_resource(d);
    }
}

fn is_button_key(k: &str) -> bool {
    matches!(k, "lib-import" | "lib-export")
}

fn is_pick_key(k: &str) -> bool {
    matches!(
        k,
        "bodies" | "workholdings" | "drive" | "p-face" | "axis-face"
    )
}

fn take_picks(
    mut picks: MessageReader<PickRequest>,
    dialog: Option<Res<CamDialog>>,
    mut commands: Commands,
) {
    let v: Vec<Pick> = picks.read().filter_map(|p| p.0).collect();
    if v.is_empty() || dialog.is_none_or(|d| d.picking.is_none()) {
        return;
    }
    commands.queue(move |w: &mut World| apply_picks(w, v));
}

fn apply_picks(world: &mut World, picks: Vec<Pick>) {
    let Some(key) = world
        .get_resource::<CamDialog>()
        .and_then(|d| d.picking.clone())
    else {
        return;
    };
    for pick in picks {
        let frame = with_ctx(world, op_frame).flatten();
        with_ctx(world, |w, ctx| {
            if let Some(mut d) = w.remove_resource::<CamDialog>() {
                d.draft.pick(ctx, &key, pick, frame);
                if key == "p-face" || key == "axis-face" {
                    d.picking = None;
                }
                d.changed = Some(web_time::Instant::now());
                d.shown = None;
                w.insert_resource(d);
            }
        });
    }
}

/// The frame a toolpath draft works in (hole picks are read in it).
fn op_frame(world: &mut World, ctx: &Ctx) -> Option<cadrs_core::cam::cadrs_cam::Frame> {
    let d = world.get_resource::<CamDialog>()?;
    let (cam, op) = d.draft.applied(ctx.cam)?;
    let (job, _, _) = cam.operation(op)?;
    let (_, wp) = cam.work_plane_for(op)?;
    let params = cam.operation(op)?.2.toolpath()?.params.clone();
    Some(cadrs_core::cam::program::toolpath_frame(
        &cam, job, wp, &params, ctx.parts,
    ))
}

#[derive(Component)]
pub struct CamDialogRoot;

/// Builds the dialog, and builds it again when its fields change.
fn sync_dialog(world: &mut World) {
    if !world.contains_resource::<CamDialog>() {
        return;
    }
    let Some((fields, title, valid, filter)) = with_ctx(world, |w, ctx| {
        let d = w.resource::<CamDialog>();
        let fields = d.draft.fields(ctx, d.picking.as_deref(), d.tab);
        let valid = d.draft.problem(ctx).is_none();
        let filter = d.picking.as_ref().map(|k| d.draft.filter(k));
        (fields, d.draft.title(), valid, filter)
    }) else {
        return;
    };
    // A toolpath's live preview: what its calculation warns of, under its fields.
    let mut fields = fields;
    if let Some(d) = world.get_resource::<CamDialog>()
        && let Draft::Toolpath { op, .. } = &d.draft
        && let Some((pop, Ok(done))) = world.resource::<super::Computed>().preview()
        && pop == op.id
        && !done.toolpath.warnings.is_empty()
    {
        fields.push(Field::section(&tr("cam-warnings")));
        for w in &done.toolpath.warnings {
            fields.push(Field::note(format!("⚠ {w}")));
        }
    }
    // Picks go to the dialog while a pick list takes them.
    let want = filter;
    if world.resource::<crate::viewport::PickFilterOverride>().0 != want {
        world
            .resource_mut::<crate::viewport::PickFilterOverride>()
            .0 = want;
    }
    let mut q = world.query_filtered::<Entity, With<CamDialogRoot>>();
    let existing = q.iter(world).next();
    let same = world.resource::<CamDialog>().shown.as_ref() == Some(&fields);
    if same && existing.is_some() {
        if let Some(e) = existing {
            let state = FeatureDialogState {
                title: title.clone(),
                valid,
                error: !valid,
            };
            if let Some(mut s) = world.get_mut::<FeatureDialogState>(e)
                && *s != state
            {
                *s = state;
            }
        }
        return;
    }
    if let Some(e) = existing {
        world.entity_mut(e).despawn();
    }
    let mut q_area = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let t2 = theme.clone();
    let fs = fields.clone();
    world.insert_resource(FieldUnits::of(&fields));
    let mut commands = world.commands();
    let d = commands
        .spawn((
            CamDialogRoot,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("cam-dialog")
                .title(title)
                .valid(valid)
                .width(330.0)
                .body(move |b| {
                    b.spawn((
                        Name::new("cam-dialog-body"),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            width: Val::Percent(100.0),
                            max_height: Val::Vh(82.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(|c| form::spawn_fields(c, &t2, super::FORM_PREFIX, &fs));
                })
                .build(&theme),
        ))
        .id();
    commands.entity(area).add_child(d);
    world.flush();
    world.resource_mut::<CamDialog>().shown = Some(fields);
}

fn on_accept(
    ev: On<FeatureDialogAccept>,
    q: Query<(), With<CamDialogRoot>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<CamDialogRoot>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

/// ✓: the draft's edits (and a toolpath's calculation), then closes.
pub fn accept(world: &mut World) {
    if world.contains_resource::<InsertParts>() {
        toast(world, &tr("cam-still-reading-parts"));
        return;
    }
    // Text typed without Enter counts too.
    sync_typed_text(world);
    let Some(d) = world.remove_resource::<CamDialog>() else {
        return;
    };
    let problem = with_ctx(world, |_, ctx| d.draft.problem(ctx)).flatten();
    if let Some(p) = problem {
        toast(world, &p);
        world.insert_resource(d);
        return;
    }
    let edits = with_ctx(world, |w, ctx| d.draft.commit(ctx, w)).unwrap_or_default();
    let first_parts = matches!(d.draft, Draft::Insert(_))
        && active_cam_owned(world).is_some_and(|(_, c)| c.components.is_empty());
    let mut ok = true;
    for e in edits {
        ok &= edit(world, e);
    }
    if ok && first_parts {
        // The first parts in: frame them once they're built.
        world.resource_mut::<super::CamUi>().fit_pending = true;
    }
    if ok {
        match &d.draft {
            Draft::Toolpath { op, .. } => {
                world.resource_mut::<super::Computed>().request([op.id]);
                world.resource_mut::<CamUi>().selected = vec![op.id];
            }
            Draft::Tool { op, .. } | Draft::WorkPlane { op, .. } => {
                world.resource_mut::<CamUi>().selected = vec![op.id];
            }
            Draft::Setup { setup, .. } => world.resource_mut::<CamUi>().selected = vec![setup.id],
            Draft::Job { job, .. } => world.resource_mut::<CamUi>().selected = vec![job.id],
            _ => {}
        }
    }
    world.insert_resource(d);
    close(world);
}

/// Reads text fields typed into without Enter.
fn sync_typed_text(world: &mut World) {
    let mut q = world
        .query_filtered::<(&Name, &bevy::text::EditableText), With<cadrs_ui::TextInputField>>();
    let typed: Vec<(String, String)> = q
        .iter(world)
        .filter_map(|(n, t)| {
            let key = form::key_of(super::FORM_PREFIX, n.as_str())?.strip_suffix("-field")?;
            Some((key.to_string(), t.value().to_string()))
        })
        .collect();
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    for (k, v) in typed {
        if let Some(mut d) = world.remove_resource::<CamDialog>() {
            d.draft.set(world, &k, &Value::Text(v), &cam);
            world.insert_resource(d);
        }
    }
}

/// The toolpath being edited is calculated a moment after its last change.
fn live_preview(world: &mut World) {
    let Some(d) = world.get_resource::<CamDialog>() else {
        return;
    };
    let Some(t) = d.changed else { return };
    if t.elapsed().as_secs_f32() < 0.35 {
        return;
    }
    let draft = d.draft.clone();
    world.resource_mut::<CamDialog>().changed = None;
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let Some((applied, op)) = draft.applied(&cam) else {
        return;
    };
    super::compute::request_preview(world, applied, op);
}
