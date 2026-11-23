//! The **Machine Builder** panel (issue #278): the right strip's Machine Builder button docks it
//! beside the viewport of an assembly, to set the assembly up as a machine CAM can drive
//! ([`cadrs_core::assembly::machine`]).
//!
//! The panel edits a [`Draft`] of the assembly's machine definition; nothing reaches the
//! document until ✓, which stores it as one undo step ([`SetMachineDef`]). Top to bottom:
//!
//! - **Mounts**: where the tool goes on (the spindle's nose, Z out of it) and where the work
//!   goes on (the table, Z out of it). A click on a mount's field arms it: the next click in
//!   the view takes the connector there, as the mate dialog's picks do; the flip button turns
//!   its Z round. Each mount shows as a small triad in the view.
//! - **Axes**: with both mounts set, a row per axis the assembly's mates make between them
//!   ([`cadrs_core::assembly::machine::detect`]), the tool's side first: its side, its name
//!   (editable), its mate (hovering highlights the mate's instances), a reverse toggle and a
//!   **jog slider**. Expanding a row gives Set zero here, its home, its travel (or the mate's
//!   limits), Continuous for a turn, and its rates.
//! - **Actuators**: the free mates that move neither mount, each of which can be made an
//!   actuator with a name, named positions and a slider.
//! - **Tool change**: the position the axes go to, the magazine actuator and its pockets.
//! - **Enclosure**: the instances the CAM view draws see-through.
//! - **Controller**: the catalogue machine it starts from, its dialect, spindle, tool changer
//!   and post settings (the CAM Machine dialog's own fields,
//!   [`crate::cam::dialogs::controller_fields`]).
//! - **Issues**: what keeps the assembly from being a machine (errors) and what the machine
//!   leaves out (warnings); a click selects the mates and instances an issue is about.
//! - The footer: Home, Reset pose, Remove machine, ✓ and ✗.
//!
//! **Jogging** moves the machine in the view only: the draft's axis values go through
//! [`cadrs_core::assembly::machine::Resolved::poses`] into [`super::AssemblyParts::preview`],
//! live while a slider is dragged (the parts' meshes move; nothing is placed again) and
//! placed once it is let go. No undo step is made and the document is not written; closing
//! the panel clears the preview.
//!
//! Names: `machine-builder-panel` (its close button `machine-builder-panel-close`), the mounts
//! `machine-mount-tool|table` (with `-flip` and `-clear`), the axis rows `machine-axis-<n>`
//! (from 1, with `-expand`, `-side`, `-name`, `-mate`, `-reverse` and the slider `-jog`, whose
//! number field is `-jog-value`), a row's settings `cam-f-mb-ax<n>-zero|home-here|home|min|
//! max|from-mate|cont|rapid|feed`, the free joints `machine-free-<n>-add`, the actuators
//! `machine-actuator-<n>` (`-name`, `-remove`, `-jog`, `-add-position`, positions
//! `-pos-<p>-name|go|remove`), the tool change `machine-tc-set|go|clear|add-pocket|
//! pockets-from-positions`, `cam-f-mb-magazine` and `machine-tc-pocket-<n>-remove`, the
//! enclosure `machine-enclosure-add|clear`, `machine-controller-toggle`, the issues
//! `machine-issue-<n>`, and the footer `machine-home`, `machine-reset-pose`, `machine-remove`,
//! `machine-accept`, `machine-cancel`.

pub mod draft;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::machine::{self, Severity};
use cadrs_core::assembly::mate::{Dof, MateId};
use cadrs_core::assembly::{InstanceId, Pose};
use cadrs_ui::form::{self, Field, FieldChange, FieldUnits, Unit, Value};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;
use cadrs_ui::{JogSlider, JogSliderChange, JogSliderState, Tag, WheelScroll};

pub use draft::{Draft, Mount};

use super::connectors::{self, ConnectorGizmos, ConnectorHaloGizmos, entity_of};
use crate::parts::PartCache;
use crate::side_panel::{SidePanel, side_panel_header, side_panel_node};
use crate::viewport::{
    ActiveKind, PickFilterOverride, PickRequest, PlaneHighlight, Selection, ViewportArea,
    ViewportDrag, ViewportRect, ViewportView,
};
use crate::{ActiveDocument, AppState};

pub struct MachineBuilderPlugin;

impl Plugin for MachineBuilderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                sync_open,
                take_fields,
                take_picks,
                refresh,
                sync_preview,
                sync_panel,
                sync_sliders,
                draw_mounts,
            )
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<MachineBuilder>();
        })
        .add_observer(on_button)
        .add_observer(on_jog)
        .add_observer(on_name)
        .add_observer(on_over)
        .add_observer(on_out);
    }
}

/// The open Machine Builder.
#[derive(Resource)]
pub struct MachineBuilder {
    pub draft: Draft,
    /// The mount the next click in the view sets.
    pub picking: Option<Mount>,
    /// The axis rows that show their settings.
    pub expanded: HashSet<(MateId, Dof)>,
    /// The Controller section is open.
    pub controller_open: bool,
    /// The controller's catalogue list is grouped by machine type.
    by_kind: bool,
    /// A jog slider is being dragged: the preview is live ([`super::AssemblyParts::live`]).
    pub dragging: bool,
    /// The draft's rows, issues and machine are to be worked out again.
    stale: bool,
    /// The jog changed: the preview is to be worked out again.
    moved: bool,
    /// The preview the builder last gave the view.
    shown: HashMap<InstanceId, Pose>,
    /// The mounts' frames where the document has the machine (shown until it resolves).
    mounts: [Option<ConnectorFrame>; 2],
    /// The builder set the view's pick filter (and takes it back).
    filtering: bool,
}

impl MachineBuilder {
    fn new(element: ElementId, stored: Option<machine::MachineDef>) -> Self {
        MachineBuilder {
            draft: Draft::new(element, stored),
            picking: None,
            expanded: HashSet::new(),
            controller_open: false,
            by_kind: true,
            dragging: false,
            stale: true,
            moved: true,
            shown: HashMap::new(),
            mounts: [None; 2],
            filtering: false,
        }
    }

    /// A builder on a draft made elsewhere (the MCP's `jog_machine`), its jog kept.
    pub fn with_draft(draft: Draft) -> Self {
        MachineBuilder {
            draft,
            ..MachineBuilder::new(ElementId::new(), None)
        }
    }

    /// Back to where the document has the machine (it was just moved there).
    pub fn script_reset(&mut self) {
        self.draft.reset_pose();
        self.touch();
    }

    /// The definition changed: the rows and the preview follow.
    fn touch(&mut self) {
        self.stale = true;
        self.moved = true;
    }
}

/// Opens the assembly `element`'s tab with the Machine Builder docked (the CAM Machine
/// dialog's **Open Machine Builder**).
pub fn open_on(world: &mut World, element: ElementId) {
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.set_active(element);
    }
    *world.resource_mut::<SidePanel>() = SidePanel::MachineBuilder;
}

/// The machine definition the document has for `element`.
fn stored_def(doc: &ActiveDocument, element: ElementId) -> Option<machine::MachineDef> {
    doc.doc
        .element(element)?
        .assembly_model()?
        .machine
        .as_deref()
        .cloned()
}

/// Opens the builder with its panel and closes it when the panel goes (or the tab changes),
/// taking its preview with it; follows the document while it is open.
fn sync_open(
    doc: Option<Res<ActiveDocument>>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    builder: Option<ResMut<MachineBuilder>>,
    mut parts: ResMut<super::AssemblyParts>,
    mut filter: ResMut<PickFilterOverride>,
    mut commands: Commands,
) {
    let element = doc
        .as_ref()
        .filter(|_| *open == SidePanel::MachineBuilder && *kind == ActiveKind::Assembly)
        .and_then(|d| super::active_assembly(d));
    match (element, builder) {
        (Some(element), Some(mut b)) if b.draft.element == element => {
            let Some(doc) = doc.filter(|d| d.is_changed()) else {
                return;
            };
            // The document's definition changed under the builder (✓, an undo): a draft
            // nobody has edited follows it.
            let now = stored_def(&doc, element);
            if now != b.draft.stored {
                if !b.draft.changed() {
                    b.draft.def = now.clone().unwrap_or_default();
                }
                b.draft.stored = now;
            }
            b.touch();
        }
        (Some(element), other) => {
            if other.is_some() {
                parts.preview.clear();
            }
            let stored = doc.as_ref().and_then(|d| stored_def(d, element));
            commands.insert_resource(MachineBuilder::new(element, stored));
        }
        (None, Some(b)) => {
            parts.preview.clear();
            if b.filtering {
                filter.0 = None;
            }
            commands.remove_resource::<MachineBuilder>();
        }
        (None, None) => {}
    }
}

/// The assembly as the machine reads it, and its occurrences' solids for the connectors.
fn flat_and_solids(
    world: &mut World,
    element: ElementId,
) -> Option<(
    cadrs_core::assembly::Assembly,
    HashMap<InstanceId, std::sync::Arc<cadrs_core::Solid>>,
)> {
    world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let asm = doc.doc.element(element)?.assembly_model()?;
        let flat = machine::flat_model(&doc.doc, asm);
        let solids =
            cadrs_core::assembly::occurrence_solids(&doc.doc, asm, |e| parts.build(&doc.doc, e));
        Some((flat, solids))
    })
}

/// Works the draft out again after an edit or a change of the document.
fn refresh(world: &mut World) {
    let Some(element) = world
        .get_resource::<MachineBuilder>()
        .filter(|b| b.stale)
        .map(|b| b.draft.element)
    else {
        return;
    };
    let Some((flat, solids)) = flat_and_solids(world, element) else {
        return;
    };
    let mut b = world.resource_mut::<MachineBuilder>();
    let frames = machine::frames_on(&solids);
    b.draft.refresh(&flat, &frames);
    // The mounts where the document has them, for the triads of a machine that does not
    // resolve yet.
    let at = |c: Option<&MateConnector>| {
        let c = c?;
        let f = frames(c)?;
        if c.is_origin() {
            return Some(f);
        }
        Some(f.moved(&flat.instance(c.instance)?.pose))
    };
    b.mounts = [
        at(b.draft.def.tool_mount.as_ref()),
        at(b.draft.def.table_mount.as_ref()),
    ];
    b.stale = false;
    b.moved = true;
}

/// Gives the view the jog's preview (unless a drag, the triad, an animation or the mate
/// dialog has the preview just now).
#[allow(clippy::too_many_arguments)]
fn sync_preview(
    builder: Option<ResMut<MachineBuilder>>,
    mut parts: ResMut<super::AssemblyParts>,
    drag: Res<super::drag::InstanceDrag>,
    triad: Res<super::triad::Triad>,
    playback: Option<Res<super::animate::Playback>>,
    mate: Option<Res<super::mate_dialog::MateSession>>,
) {
    let Some(mut b) = builder else { return };
    if drag.dragging() || triad.dragging() || playback.is_some() || mate.is_some() {
        return;
    }
    if b.moved {
        b.shown = b.draft.poses().into_iter().collect();
        b.moved = false;
    }
    if parts.preview != b.shown {
        parts.preview = b.shown.clone();
    }
}

// ---------------------------------------------------------------------------------------------
// The panel

#[derive(Component)]
struct MachineBuilderPanel;

#[derive(Component)]
struct PanelBody;

/// What the panel was built from.
#[derive(Component, Debug, Clone, PartialEq)]
struct PanelKey(String);

/// What a button of the panel does.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum Act {
    PickMount(Mount),
    FlipMount(Mount),
    ClearMount(Mount),
    Expand(usize),
    Reverse(usize),
    AddActuator(usize),
    RemoveActuator(usize),
    AddPosition(usize),
    GoPosition(usize, usize),
    RemovePosition(usize, usize),
    ToolChangeSet,
    ToolChangeGo,
    ToolChangeClear,
    AddPocket,
    PocketsFromPositions,
    RemovePocket(usize),
    EnclosureAdd,
    EnclosureClear,
    ToggleController,
    Issue(usize),
    Home,
    ResetPose,
    Remove,
    Accept,
    Cancel,
}

/// A name edited in the panel.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum NameField {
    Axis(usize),
    Actuator(usize),
    Position(usize, usize),
}

/// A jog slider of the panel.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum Jog {
    Axis(usize),
    Actuator(usize),
}

/// A row whose hover highlights its mate's instances.
#[derive(Component, Debug, Clone, Copy)]
struct HoverMate(MateId);

/// Rebuilds the panel when what it shows changes (not for a jog: the sliders follow in place).
fn sync_panel(world: &mut World) {
    let mut q = world.query_filtered::<(Entity, &PanelKey), With<MachineBuilderPanel>>();
    let panels: Vec<(Entity, PanelKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    if !world.contains_resource::<MachineBuilder>() {
        for (e, _) in panels {
            world.entity_mut(e).despawn();
        }
        return;
    }
    if world.resource::<MachineBuilder>().stale {
        return;
    }
    let labels = mount_labels(world);
    let b = world.resource::<MachineBuilder>();
    let key = PanelKey(format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{}{}{}{:?}",
        b.draft.def,
        b.draft.free,
        b.draft.issues,
        b.draft.axis_info,
        b.draft.actuator_info,
        b.picking,
        {
            let mut e: Vec<_> = b.expanded.iter().map(|k| format!("{k:?}")).collect();
            e.sort();
            e
        },
        b.controller_open,
        b.by_kind,
        b.draft.changed(),
        labels,
    ));
    if panels.iter().any(|(_, k)| *k == key) {
        return;
    }
    // The scroll stays where it was through a rebuild.
    let mut q_scroll = world.query_filtered::<&ScrollPosition, With<PanelBody>>();
    let scroll = q_scroll.iter(world).next().cloned().unwrap_or_default();
    for (e, _) in panels {
        world.entity_mut(e).despawn();
    }
    build_panel(world, key, labels, scroll);
}

/// The two mounts' connectors as the fields name them.
fn mount_labels(world: &World) -> [Option<String>; 2] {
    let (Some(b), Some(doc), Some(cache)) = (
        world.get_resource::<MachineBuilder>(),
        world.get_resource::<ActiveDocument>(),
        world.get_resource::<PartCache>(),
    ) else {
        return [None, None];
    };
    [Mount::Tool, Mount::Table].map(|m| {
        b.draft
            .mount(m)
            .map(|c| super::connector_tool::connector_label(doc, cache, c))
    })
}

/// "12.5 mm", "30°".
fn fmt_axis(v: f64, rotary: bool) -> String {
    let n = format!("{v:.2}");
    let n = n.trim_end_matches('0').trim_end_matches('.');
    let n = if n == "-0" { "0" } else { n };
    if rotary {
        format!("{n}°")
    } else {
        format!("{n} mm")
    }
}

/// A section's heading.
fn heading(p: &mut ChildSpawnerCommands, t: &Theme, key: &'static str) {
    p.spawn((
        t.text(tr(key), 12.0, FontWeight::BOLD, t.foreground),
        cadrs_ui::i18n::Translated(key),
        Node {
            margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(10.0), Val::Px(3.0)),
            ..default()
        },
    ));
}

/// A small grey note.
fn note(p: &mut ChildSpawnerCommands, t: &Theme, name: &str, text: String) {
    p.spawn((
        Name::new(name.to_string()),
        t.text(text, 11.0, FontWeight::NORMAL, t.muted_foreground),
        Node {
            margin: UiRect::vertical(Val::Px(2.0)),
            max_width: Val::Percent(100.0),
            ..default()
        },
    ))
    .insert(TextLayout::linebreak(LineBreak::WordBoundary));
}

/// A row of the panel: its children in a line.
fn line() -> Node {
    Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        min_height: Val::Px(26.0),
        width: Val::Percent(100.0),
        ..default()
    }
}

/// A row's text that gives way to the rest of the row.
fn row_text(t: &Theme, text: String, color: Color) -> impl Bundle {
    (
        t.text(text, 11.5, FontWeight::NORMAL, color),
        Node {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_width: Val::Px(0.0),
            overflow: Overflow::clip(),
            ..default()
        },
    )
}

fn small_icon(name: &str, icon: &'static str, tip: String, t: &Theme, act: Act) -> impl Bundle {
    (
        IconButton::new(name.to_string(), icon)
            .tooltip(tip)
            .build(t),
        act,
    )
}

/// The settings of axis `i` (an expanded row), as form fields keyed `mb-ax<n>-…`.
fn axis_fields(d: &Draft, i: usize) -> Vec<Field> {
    let a = &d.def.axes[i];
    let n = i + 1;
    let key = |k: &str| format!("mb-ax{n}-{k}");
    let unit = if a.is_rotary() { Unit::Deg } else { Unit::Mm };
    let mut f = vec![
        Field::button(
            &key("zero"),
            &tr("asm-machine-set-zero-here"),
            "machine-jog",
        ),
        Field::number(&key("home"), &tr("asm-machine-home"), a.home, unit),
        Field::button(
            &key("home-here"),
            &tr("asm-machine-set-home-here"),
            "machine-home",
        ),
    ];
    if a.is_rotary() {
        f.push(Field::check(
            &key("cont"),
            &tr("asm-machine-continuous"),
            a.continuous,
        ));
    }
    if !a.continuous {
        let unlimited = a.max - a.min >= machine::detect::UNLIMITED;
        f.push(Field::number(
            &key("min"),
            &tr("asm-machine-travel-min"),
            if unlimited { 0.0 } else { a.min },
            unit,
        ));
        f.push(Field::number(
            &key("max"),
            &tr("asm-machine-travel-max"),
            if unlimited { 0.0 } else { a.max },
            unit,
        ));
        if a.limits_from_mate {
            f.push(Field::note(tr("asm-machine-travel-follows-mate")));
        } else {
            f.push(Field::button(
                &key("from-mate"),
                &tr("asm-machine-from-mate-limits"),
                "cam-axis-limit",
            ));
        }
    }
    let (rapid, feed) = if a.is_rotary() {
        ("asm-machine-rapid-rotary", "asm-machine-feed-rotary")
    } else {
        ("cam-rapid-rate", "cam-fastest-feed")
    };
    let rate = if a.is_rotary() {
        Unit::None
    } else {
        Unit::Feed
    };
    f.push(Field::number(&key("rapid"), &tr(rapid), a.rapid, rate));
    f.push(Field::number(&key("feed"), &tr(feed), a.max_feed, rate));
    f
}

/// The Tool change section's and the Controller section's form fields.
fn tool_change_fields(d: &Draft) -> Vec<Field> {
    let mut options = vec![tr("asm-machine-no-magazine")];
    options.extend(d.def.actuators.iter().map(|a| a.name.clone()));
    vec![Field::select(
        "mb-magazine",
        &tr("asm-machine-magazine"),
        options,
        d.magazine().map_or(0, |k| k + 1),
    )]
}

fn controller_fields(b: &MachineBuilder) -> Vec<Field> {
    let mut f = Vec::new();
    let rotary = b.draft.def.axes.iter().any(|a| a.is_rotary());
    crate::cam::dialogs::controller_fields(
        &mut f,
        &b.draft.def.controller,
        b.by_kind,
        false,
        rotary,
    );
    f
}

fn build_panel(
    world: &mut World,
    key: PanelKey,
    labels: [Option<String>; 2],
    scroll: ScrollPosition,
) {
    let t = world.resource::<Theme>().clone();
    let b = world.resource::<MachineBuilder>();
    let d = b.draft.clone();
    let picking = b.picking;
    let expanded = b.expanded.clone();
    let controller_open = b.controller_open;
    let controller = controller_open.then(|| controller_fields(b));
    let tool_change = tool_change_fields(&d);
    // Every number field's unit, for the form to read typed values back.
    let mut all_fields: Vec<Field> = tool_change.clone();
    all_fields.extend(controller.iter().flatten().cloned());
    let axis_forms: Vec<Option<Vec<Field>>> = d
        .def
        .axes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            expanded
                .contains(&(a.mate, a.dof))
                .then(|| axis_fields(&d, i))
        })
        .collect();
    all_fields.extend(axis_forms.iter().flatten().flatten().cloned());
    world.insert_resource(FieldUnits::of(&all_fields));

    let panel = world
        .spawn((
            Name::new("machine-builder-panel"),
            MachineBuilderPanel,
            key,
            DespawnOnExit(AppState::Document),
            side_panel_node(&t),
        ))
        .id();
    world.entity_mut(panel).entry::<Node>().and_modify(|mut n| {
        n.width = Val::Px(340.0);
        n.flex_direction = FlexDirection::Column;
    });
    let prefix = crate::cam::FORM_PREFIX;
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        side_panel_header(
            p,
            &t,
            &tr("asm-machine-builder"),
            "machine-builder-panel-close",
        );
        p.spawn((
            Name::new("machine-builder-body"),
            PanelBody,
            WheelScroll,
            scroll,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(2.0), Val::Px(8.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|p| {
            // --- Mounts
            heading(p, &t, "asm-machine-mounts");
            for (which, slug, icon, label, hint, text) in [
                (
                    Mount::Tool,
                    "tool",
                    "machine-tool-mount",
                    "asm-machine-tool-mount",
                    "asm-machine-tool-mount-hint",
                    &labels[0],
                ),
                (
                    Mount::Table,
                    "table",
                    "machine-table-mount",
                    "asm-machine-table-mount",
                    "asm-machine-table-mount-hint",
                    &labels[1],
                ),
            ] {
                let armed = picking == Some(which);
                p.spawn((Name::new(format!("machine-mount-{slug}-row")), line()))
                    .with_children(|r| {
                        r.spawn(cadrs_ui::icon::icon_in(
                            icon,
                            16.0,
                            t.tool_foreground,
                            Node {
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                        r.spawn((
                            t.text(tr(label), 11.5, FontWeight::NORMAL, t.foreground),
                            Node {
                                width: Val::Px(78.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                        let shown = match (armed, text) {
                            (true, _) => tr("asm-machine-pick-in-view"),
                            (false, Some(l)) => l.clone(),
                            (false, None) => tr("asm-machine-mount-unset"),
                        };
                        let mut field = cadrs_ui::Button::new(format!("machine-mount-{slug}"))
                            .label(shown)
                            .small()
                            .width(Val::Px(0.0))
                            .tooltip(tr(hint));
                        field = if armed {
                            field.primary()
                        } else {
                            field.outline()
                        };
                        r.spawn((field.build(&t), Act::PickMount(which)))
                            .entry::<Node>()
                            .and_modify(|mut n| {
                                n.flex_grow = 1.0;
                                n.min_width = Val::Px(0.0);
                                n.overflow = Overflow::clip();
                            });
                        if text.is_some() {
                            r.spawn(small_icon(
                                &format!("machine-mount-{slug}-flip"),
                                "flip-direction",
                                tr("asm-machine-flip-mount"),
                                &t,
                                Act::FlipMount(which),
                            ));
                            r.spawn(small_icon(
                                &format!("machine-mount-{slug}-clear"),
                                "close",
                                tr("asm-machine-clear-mount"),
                                &t,
                                Act::ClearMount(which),
                            ));
                        }
                    });
            }

            // --- Axes
            heading(p, &t, "asm-machine-axes");
            if !d.mounted() {
                note(
                    p,
                    &t,
                    "machine-axes-empty",
                    tr("asm-machine-axes-need-mounts"),
                );
            } else if d.def.axes.is_empty() {
                note(p, &t, "machine-axes-empty", tr("asm-machine-no-axes"));
            }
            for (i, a) in d.def.axes.iter().enumerate().filter(|_| d.mounted()) {
                let n = i + 1;
                let row = format!("machine-axis-{n}");
                let info = &d.axis_info[i];
                let open = axis_forms[i].is_some();
                p.spawn((
                    Name::new(row.clone()),
                    HoverMate(a.mate),
                    bevy::picking::hover::Hovered::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        padding: UiRect::vertical(Val::Px(2.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(t.panel_border),
                ))
                .with_children(|c| {
                    c.spawn(line()).with_children(|r| {
                        r.spawn(small_icon(
                            &format!("{row}-expand"),
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            tr("asm-machine-axis-settings"),
                            &t,
                            Act::Expand(i),
                        ));
                        let (side, colour) = match a.side {
                            machine::Side::Tool => ("asm-machine-side-tool", t.primary),
                            machine::Side::Table => ("asm-machine-side-table", t.muted_foreground),
                        };
                        r.spawn(
                            Tag::new(format!("{row}-side"), tr(side))
                                .color(colour)
                                .outline()
                                .build(&t),
                        );
                        r.spawn((
                            TextInput::new(format!("{row}-name"))
                                .value(a.name.clone())
                                .height(22.0)
                                .width(Val::Px(40.0))
                                .select_all_on_focus()
                                .build(&t),
                            NameField::Axis(i),
                            Tooltip::new(tr("asm-machine-axis-name")),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| n.flex_shrink = 0.0);
                        r.spawn((
                            Name::new(format!("{row}-kind")),
                            cadrs_ui::icon::icon_in(
                                if info.rotary {
                                    "machine-axis-rotary"
                                } else {
                                    "machine-axis-linear"
                                },
                                16.0,
                                t.tool_foreground,
                                Node {
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ),
                            Tooltip::new(if info.rotary {
                                tr("asm-machine-axis-rotary")
                            } else {
                                tr("asm-machine-axis-linear")
                            }),
                        ));
                        r.spawn((
                            Name::new(format!("{row}-mate")),
                            row_text(&t, info.name.clone(), t.muted_foreground),
                            Tooltip::new(tr_with("asm-machine-axis-mate", &[("mate", &info.name)])),
                        ));
                        let mut rev =
                            IconButton::new(format!("{row}-reverse"), "machine-axis-flip")
                                .tooltip(tr("asm-machine-reverse"));
                        if a.reversed {
                            rev = rev.selected(true);
                        }
                        r.spawn((rev.build(&t), Act::Reverse(i)));
                    });
                    let track = d.axis_track(i);
                    let mut jog = JogSlider::new(format!("{row}-jog"))
                        .range(track.min, track.max)
                        .value(d.value(i))
                        .home(a.home)
                        .unit(if info.rotary { "°" } else { " mm" })
                        .tooltip(tr_with("asm-machine-jog", &[("axis", &a.name)]));
                    if let Some((lo, hi)) = track.limits {
                        jog = jog.limits(lo, hi);
                    }
                    c.spawn(Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::left(Val::Px(4.0)),
                        ..default()
                    })
                    .with_children(|s| {
                        s.spawn((jog.build(&t), Jog::Axis(i)));
                    });
                    if let Some(fields) = &axis_forms[i] {
                        let fields = fields.clone();
                        let t2 = t.clone();
                        c.spawn((
                            Name::new(format!("{row}-settings")),
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                width: Val::Percent(100.0),
                                padding: UiRect::new(
                                    Val::Px(18.0),
                                    Val::ZERO,
                                    Val::Px(2.0),
                                    Val::Px(6.0),
                                ),
                                ..default()
                            },
                            Children::spawn(bevy::ecs::spawn::SpawnWith(
                                move |c: &mut ChildSpawner| {
                                    form::spawn_fields(c, &t2, prefix, &fields)
                                },
                            )),
                        ));
                    }
                });
            }

            // --- Actuators
            heading(p, &t, "asm-machine-actuators");
            if d.def.actuators.is_empty() && d.free.is_empty() {
                note(
                    p,
                    &t,
                    "machine-actuators-empty",
                    tr("asm-machine-no-actuators"),
                );
            }
            for (k, a) in d.def.actuators.iter().enumerate() {
                let n = k + 1;
                let row = format!("machine-actuator-{n}");
                let info = d.actuator_info[k].as_ref();
                let rotary = a.dof.is_angle();
                p.spawn((
                    Name::new(row.clone()),
                    HoverMate(a.mate),
                    bevy::picking::hover::Hovered::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        padding: UiRect::vertical(Val::Px(2.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(t.panel_border),
                ))
                .with_children(|c| {
                    c.spawn(line()).with_children(|r| {
                        r.spawn(cadrs_ui::icon::icon_in(
                            "machine-actuator",
                            16.0,
                            t.tool_foreground,
                            Node {
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                        r.spawn((
                            TextInput::new(format!("{row}-name"))
                                .value(a.name.clone())
                                .height(22.0)
                                .width(Val::Px(110.0))
                                .select_all_on_focus()
                                .build(&t),
                            NameField::Actuator(k),
                            Tooltip::new(tr("asm-machine-actuator-name")),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| n.flex_shrink = 0.0);
                        r.spawn(row_text(
                            &t,
                            info.map_or_else(
                                || tr("asm-machine-actuator-lost"),
                                |i| i.name.clone(),
                            ),
                            if info.is_some() {
                                t.muted_foreground
                            } else {
                                t.danger
                            },
                        ));
                        r.spawn(small_icon(
                            &format!("{row}-remove"),
                            "remove-circle",
                            tr("asm-machine-remove-actuator"),
                            &t,
                            Act::RemoveActuator(k),
                        ));
                    });
                    let track = d.actuator_track(k);
                    c.spawn(Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::left(Val::Px(4.0)),
                        ..default()
                    })
                    .with_children(|s| {
                        s.spawn((
                            JogSlider::new(format!("{row}-jog"))
                                .range(track.min, track.max)
                                .value(d.actuator_value(k))
                                .unit(if rotary { "°" } else { " mm" })
                                .disabled(info.is_none())
                                .tooltip(tr_with("asm-machine-jog", &[("axis", &a.name)]))
                                .build(&t),
                            Jog::Actuator(k),
                        ));
                    });
                    for (pi, (name, value)) in a.positions.iter().enumerate() {
                        let pos = format!("{row}-pos-{}", pi + 1);
                        c.spawn(line()).with_children(|r| {
                            r.spawn((
                                TextInput::new(format!("{pos}-name"))
                                    .value(name.clone())
                                    .height(22.0)
                                    .width(Val::Px(130.0))
                                    .select_all_on_focus()
                                    .build(&t),
                                NameField::Position(k, pi),
                            ))
                            .entry::<Node>()
                            .and_modify(|mut n| {
                                n.flex_shrink = 0.0;
                                n.margin.left = Val::Px(18.0);
                            });
                            r.spawn(row_text(&t, fmt_axis(*value, rotary), t.foreground));
                            r.spawn(small_icon(
                                &format!("{pos}-go"),
                                "machine-jog",
                                tr_with("asm-machine-go-to-position", &[("name", name)]),
                                &t,
                                Act::GoPosition(k, pi),
                            ));
                            r.spawn(small_icon(
                                &format!("{pos}-remove"),
                                "close",
                                tr("asm-machine-remove-position"),
                                &t,
                                Act::RemovePosition(k, pi),
                            ));
                        });
                    }
                    c.spawn(line()).with_children(|r| {
                        r.spawn((
                            cadrs_ui::Button::new(format!("{row}-add-position"))
                                .label(tr("asm-machine-add-position-here"))
                                .icon("plus")
                                .small()
                                .ghost()
                                .build(&t),
                            Act::AddPosition(k),
                        ));
                    });
                });
            }
            for (k, f) in d.free.iter().enumerate() {
                let row = format!("machine-free-{}", k + 1);
                p.spawn((
                    Name::new(row.clone()),
                    HoverMate(f.mate),
                    bevy::picking::hover::Hovered::default(),
                    line(),
                ))
                .with_children(|r| {
                    r.spawn(cadrs_ui::icon::icon_in(
                        if f.dof.is_angle() {
                            "machine-axis-rotary"
                        } else {
                            "machine-axis-linear"
                        },
                        16.0,
                        t.muted_foreground,
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    r.spawn(row_text(&t, f.name.clone(), t.foreground));
                    r.spawn((
                        cadrs_ui::Button::new(format!("{row}-add"))
                            .label(tr("asm-machine-add-actuator"))
                            .icon("machine-actuator")
                            .small()
                            .outline()
                            .tooltip(tr("asm-machine-add-actuator-tooltip"))
                            .build(&t),
                        Act::AddActuator(k),
                    ));
                });
            }

            // --- Tool change
            heading(p, &t, "asm-machine-tool-change");
            let position = if d.def.tool_change.position.is_empty() {
                tr("asm-machine-tool-change-unset")
            } else {
                d.def
                    .tool_change
                    .position
                    .iter()
                    .map(|(n, v)| {
                        let rotary = d.def.axis_named(n).is_some_and(|a| a.is_rotary());
                        format!("{n} {}", fmt_axis(*v, rotary))
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            p.spawn((Name::new("machine-tc-row"), line()))
                .with_children(|r| {
                    r.spawn(cadrs_ui::icon::icon_in(
                        "machine-tool-change",
                        16.0,
                        t.tool_foreground,
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    r.spawn((
                        Name::new("machine-tc-position"),
                        row_text(&t, position, t.foreground),
                    ));
                });
            p.spawn(line()).with_children(|r| {
                r.spawn((
                    cadrs_ui::Button::new("machine-tc-set")
                        .label(tr("asm-machine-set-from-current"))
                        .small()
                        .outline()
                        .disabled(d.def.axes.is_empty())
                        .tooltip(tr("asm-machine-set-from-current-tooltip"))
                        .build(&t),
                    Act::ToolChangeSet,
                ));
                if !d.def.tool_change.position.is_empty() {
                    r.spawn((
                        cadrs_ui::Button::new("machine-tc-go")
                            .label(tr("asm-machine-go-there"))
                            .small()
                            .ghost()
                            .build(&t),
                        Act::ToolChangeGo,
                    ));
                    r.spawn(small_icon(
                        "machine-tc-clear",
                        "close",
                        tr("asm-machine-clear-tool-change"),
                        &t,
                        Act::ToolChangeClear,
                    ));
                }
            });
            {
                let fields = tool_change.clone();
                let t2 = t.clone();
                p.spawn((
                    Name::new("machine-tc-magazine"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    Children::spawn(bevy::ecs::spawn::SpawnWith(move |c: &mut ChildSpawner| {
                        form::spawn_fields(c, &t2, prefix, &fields)
                    })),
                ));
            }
            if let Some(k) = d.magazine() {
                let rotary = d.def.actuators[k].dof.is_angle();
                p.spawn((
                    Name::new("machine-tc-pockets"),
                    Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(4.0),
                        row_gap: Val::Px(3.0),
                        align_items: AlignItems::Center,
                        margin: UiRect::vertical(Val::Px(3.0)),
                        ..default()
                    },
                ))
                .with_children(|r| {
                    for (pi, pocket) in d.def.tool_change.pockets.iter().enumerate() {
                        r.spawn((
                            cadrs_ui::Button::new(format!(
                                "machine-tc-pocket-{}-remove",
                                pocket.number
                            ))
                            .label(format!(
                                "T{} {}",
                                pocket.number,
                                fmt_axis(pocket.value, rotary)
                            ))
                            .small()
                            .outline()
                            .tooltip(tr("asm-machine-remove-pocket"))
                            .build(&t),
                            Act::RemovePocket(pi),
                        ));
                    }
                });
                p.spawn(line()).with_children(|r| {
                    r.spawn((
                        cadrs_ui::Button::new("machine-tc-add-pocket")
                            .label(tr("asm-machine-add-pocket-here"))
                            .icon("plus")
                            .small()
                            .ghost()
                            .build(&t),
                        Act::AddPocket,
                    ));
                    if !d.def.actuators[k].positions.is_empty() {
                        r.spawn((
                            cadrs_ui::Button::new("machine-tc-pockets-from-positions")
                                .label(tr("asm-machine-pockets-from-positions"))
                                .small()
                                .ghost()
                                .build(&t),
                            Act::PocketsFromPositions,
                        ));
                    }
                });
            }

            // --- Enclosure
            heading(p, &t, "asm-machine-enclosure");
            p.spawn(line()).with_children(|r| {
                r.spawn((
                    Name::new("machine-enclosure-count"),
                    row_text(
                        &t,
                        match d.def.enclosure.len() {
                            0 => tr("asm-machine-enclosure-none"),
                            n => {
                                tr_with("asm-machine-enclosure-count", &[("count", &n.to_string())])
                            }
                        },
                        t.muted_foreground,
                    ),
                ));
                r.spawn((
                    cadrs_ui::Button::new("machine-enclosure-add")
                        .label(tr("asm-machine-enclosure-add"))
                        .small()
                        .outline()
                        .tooltip(tr("asm-machine-enclosure-tooltip"))
                        .build(&t),
                    Act::EnclosureAdd,
                ));
                if !d.def.enclosure.is_empty() {
                    r.spawn(small_icon(
                        "machine-enclosure-clear",
                        "close",
                        tr("asm-machine-enclosure-clear"),
                        &t,
                        Act::EnclosureClear,
                    ));
                }
            });

            // --- Controller
            p.spawn((
                Name::new("machine-controller-row"),
                Node {
                    margin: UiRect::top(Val::Px(8.0)),
                    ..line()
                },
            ))
            .with_children(|r| {
                r.spawn(small_icon(
                    "machine-controller-toggle",
                    if controller_open {
                        "chevron-down"
                    } else {
                        "chevron-right"
                    },
                    tr("asm-machine-controller-settings"),
                    &t,
                    Act::ToggleController,
                ));
                r.spawn(t.text(
                    tr("asm-machine-controller"),
                    12.0,
                    FontWeight::BOLD,
                    t.foreground,
                ));
                r.spawn((
                    Name::new("machine-controller-summary"),
                    row_text(
                        &t,
                        format!(
                            "{} · {}",
                            d.def.controller.model,
                            d.def.controller.dialect.label()
                        ),
                        t.muted_foreground,
                    ),
                ));
            });
            if let Some(fields) = controller {
                let t2 = t.clone();
                p.spawn((
                    Name::new("machine-controller"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        width: Val::Percent(100.0),
                        padding: UiRect::left(Val::Px(4.0)),
                        ..default()
                    },
                    Children::spawn(bevy::ecs::spawn::SpawnWith(move |c: &mut ChildSpawner| {
                        form::spawn_fields(c, &t2, prefix, &fields)
                    })),
                ));
            }

            // --- Issues
            if !d.issues.is_empty() {
                heading(p, &t, "asm-machine-issues");
            }
            for (k, issue) in d.issues.iter().enumerate() {
                let error = issue.severity == Severity::Error;
                let (icon, colour) = if error {
                    ("error-filled", t.danger)
                } else {
                    ("warning-filled", t.warning_icon)
                };
                p.spawn((
                    Name::new(format!("machine-issue-{}", k + 1)),
                    bevy::ui_widgets::Button,
                    bevy::picking::hover::Hovered::default(),
                    Act::Issue(k),
                    Tooltip::new(tr("asm-machine-issue-tooltip")),
                    Node {
                        align_items: AlignItems::FlexStart,
                        column_gap: Val::Px(6.0),
                        padding: UiRect::vertical(Val::Px(3.0)),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                ))
                .with_children(|r| {
                    r.spawn((
                        cadrs_ui::icon::icon_in(
                            icon,
                            14.0,
                            colour,
                            Node {
                                flex_shrink: 0.0,
                                margin: UiRect::top(Val::Px(1.0)),
                                ..default()
                            },
                        ),
                        Pickable::IGNORE,
                    ));
                    r.spawn((
                        t.text(
                            issue.text.clone(),
                            11.5,
                            FontWeight::NORMAL,
                            if error { t.danger } else { t.foreground },
                        ),
                        Node {
                            flex_shrink: 1.0,
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .insert(TextLayout::linebreak(LineBreak::WordBoundary));
                });
            }
        });

        // --- Footer
        p.spawn((
            Name::new("machine-builder-footer"),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                border: UiRect::top(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(t.panel_border),
        ))
        .with_children(|f| {
            f.spawn((
                cadrs_ui::Button::new("machine-home")
                    .label(tr("asm-machine-go-home"))
                    .icon("machine-home")
                    .small()
                    .outline()
                    .disabled(d.resolved.is_none())
                    .tooltip(tr("asm-machine-go-home-tooltip"))
                    .build(&t),
                Act::Home,
            ));
            f.spawn((
                cadrs_ui::Button::new("machine-reset-pose")
                    .label(tr("asm-machine-reset-pose"))
                    .small()
                    .outline()
                    .tooltip(tr("asm-machine-reset-pose-tooltip"))
                    .build(&t),
                Act::ResetPose,
            ));
            f.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if d.stored.is_some() {
                f.spawn(small_icon(
                    "machine-remove",
                    "delete",
                    tr("asm-machine-remove"),
                    &t,
                    Act::Remove,
                ));
            }
            f.spawn((
                cadrs_ui::Button::new("machine-accept")
                    .icon("check")
                    .small()
                    .primary()
                    .square()
                    .disabled(!d.changed())
                    .tooltip(tr("asm-machine-accept"))
                    .build(&t),
                Act::Accept,
            ));
            f.spawn((
                cadrs_ui::Button::new("machine-cancel")
                    .icon("close")
                    .small()
                    .outline()
                    .square()
                    .tooltip(tr("asm-machine-cancel"))
                    .build(&t),
                Act::Cancel,
            ));
        });
    });
    // Beside the viewport.
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else {
        return;
    };
    let at = world
        .get::<Children>(parent)
        .and_then(|c| c.iter().position(|e| e == area))
        .map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
    world.flush();
}

/// The sliders follow the draft (Home, Reset pose, a scripted jog).
fn sync_sliders(builder: Option<Res<MachineBuilder>>, mut q: Query<(&Jog, &mut JogSliderState)>) {
    let Some(b) = builder.filter(|b| b.is_changed()) else {
        return;
    };
    for (jog, mut s) in &mut q {
        let (v, home) = match *jog {
            Jog::Axis(i) => (b.draft.value(i), b.draft.def.axes.get(i).map(|a| a.home)),
            Jog::Actuator(k) => (b.draft.actuator_value(k), None),
        };
        if (s.value - v).abs() > 1e-9 || (home.is_some() && s.home != home) {
            s.value = v;
            if home.is_some() {
                s.home = home;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// What the panel's widgets do

fn on_jog(ev: On<JogSliderChange>, q: Query<&Jog>, builder: Option<ResMut<MachineBuilder>>) {
    let (Ok(jog), Some(mut b)) = (q.get(ev.entity), builder) else {
        return;
    };
    match *jog {
        Jog::Axis(i) => b.draft.jog(i, ev.value),
        Jog::Actuator(k) => b.draft.jog_actuator(k, ev.value),
    }
    b.dragging = !ev.done;
    b.moved = true;
}

fn on_name(ev: On<TextSubmit>, q: Query<&NameField>, builder: Option<ResMut<MachineBuilder>>) {
    let (Ok(field), Some(mut b)) = (q.get(ev.entity), builder) else {
        return;
    };
    let _ = match *field {
        NameField::Axis(i) => b.draft.rename_axis(i, &ev.value),
        NameField::Actuator(k) => b.draft.rename_actuator(k, &ev.value),
        NameField::Position(k, p) => b.draft.rename_position(k, p, &ev.value),
    };
    // (Built again either way: a name that was refused shows the old one.)
    b.touch();
}

fn on_over(
    ev: On<PointerOver>,
    q: Query<&HoverMate>,
    mut hover: ResMut<super::mate_display::MateHover>,
) {
    if let Ok(m) = q.get(ev.entity) {
        hover.0 = Some(m.0);
    }
}

fn on_out(
    ev: On<PointerOut>,
    q: Query<&HoverMate>,
    mut hover: ResMut<super::mate_display::MateHover>,
) {
    if let Ok(m) = q.get(ev.entity)
        && hover.0 == Some(m.0)
    {
        hover.0 = None;
    }
}

fn on_button(a: On<Activate>, q: Query<&Act>, q_name: Query<&Name>, mut commands: Commands) {
    if let Ok(act) = q.get(a.entity).copied() {
        commands.queue(move |world: &mut World| run(world, act));
        return;
    }
    // The panel's own close button drops the draft with it.
    if q_name
        .get(a.entity)
        .is_ok_and(|n| n.as_str() == "machine-builder-panel-close")
    {
        commands.queue(|world: &mut World| {
            *world.resource_mut::<SidePanel>() = SidePanel::None;
        });
    }
}

/// Runs a button of the panel.
fn run(world: &mut World, act: Act) {
    if !world.contains_resource::<MachineBuilder>() {
        return;
    }
    match act {
        Act::Accept => {
            accept(world);
            return;
        }
        Act::Remove => {
            let cmd = world.resource::<MachineBuilder>().draft.remove_command();
            if super::run(world, &cmd) {
                *world.resource_mut::<SidePanel>() = SidePanel::None;
            }
            return;
        }
        Act::Cancel => {
            *world.resource_mut::<SidePanel>() = SidePanel::None;
            return;
        }
        Act::Issue(k) => {
            select_issue(world, k);
            return;
        }
        Act::EnclosureAdd => {
            let picked = super::selected_instances(world.resource::<Selection>());
            if picked.is_empty() {
                crate::cam::toast(world, &tr("asm-machine-enclosure-select-first"));
                return;
            }
            let mut b = world.resource_mut::<MachineBuilder>();
            b.draft.set_enclosure(Some(&picked));
            b.touch();
            return;
        }
        _ => {}
    }
    let mut b = world.resource_mut::<MachineBuilder>();
    apply(&mut b, act);
}

/// The buttons that only change the builder.
fn apply(b: &mut MachineBuilder, act: Act) {
    let d = &mut b.draft;
    match act {
        Act::PickMount(m) => {
            b.picking = (b.picking != Some(m)).then_some(m);
            return;
        }
        Act::FlipMount(m) => d.flip_mount(m),
        Act::ClearMount(m) => d.set_mount(m, None),
        Act::Expand(i) => {
            if let Some(a) = d.def.axes.get(i) {
                let key = (a.mate, a.dof);
                if !b.expanded.remove(&key) {
                    b.expanded.insert(key);
                }
            }
            return;
        }
        Act::Reverse(i) => d.reverse_axis(i),
        Act::AddActuator(k) => {
            d.add_actuator(k);
        }
        Act::RemoveActuator(k) => d.remove_actuator(k),
        Act::AddPosition(k) => d.add_position_here(k),
        Act::GoPosition(k, p) => d.go_to_position(k, p),
        Act::RemovePosition(k, p) => d.remove_position(k, p),
        Act::ToolChangeSet => d.tool_change_here(),
        Act::ToolChangeGo => d.go_to_tool_change(),
        Act::ToolChangeClear => d.clear_tool_change(),
        Act::AddPocket => d.add_pocket_here(),
        Act::PocketsFromPositions => d.pockets_from_positions(),
        Act::RemovePocket(p) => d.remove_pocket(p),
        Act::EnclosureClear => d.set_enclosure(None),
        Act::ToggleController => {
            b.controller_open = !b.controller_open;
            return;
        }
        Act::Home => d.go_home(),
        Act::ResetPose => d.reset_pose(),
        Act::Accept | Act::Remove | Act::Cancel | Act::Issue(_) | Act::EnclosureAdd => return,
    }
    b.touch();
}

/// ✓: the draft becomes the assembly's machine definition, one undo step. The panel stays,
/// its jog as it was.
pub fn accept(world: &mut World) {
    let Some(b) = world.get_resource::<MachineBuilder>() else {
        return;
    };
    if !b.draft.changed() {
        return;
    }
    let cmd = b.draft.command();
    let done = world.resource_mut::<ActiveDocument>().execute(&cmd);
    match done {
        Ok(()) => {
            let mut b = world.resource_mut::<MachineBuilder>();
            b.draft.stored = Some(b.draft.def.clone());
            b.touch();
        }
        // A definition the assembly cannot take (two axes of one name): why.
        Err(e) => crate::cam::toast(world, &e.to_string()),
    }
}

/// A click on an issue: its mates and instances selected.
fn select_issue(world: &mut World, k: usize) {
    let Some(issue) = world
        .resource::<MachineBuilder>()
        .draft
        .issues
        .get(k)
        .cloned()
    else {
        return;
    };
    let mut instances = issue.instances.clone();
    if let Some(mate) = issue.mates.first().copied() {
        world.resource_mut::<super::mate_display::MateSelection>().0 = Some(mate);
        // A mate's own instances too, so the view shows where it is.
        if let Some(asm) = world
            .get_resource::<ActiveDocument>()
            .and_then(|d| d.active_element()?.assembly_model().cloned())
        {
            for m in &issue.mates {
                if let Some(f) = asm.mate(*m) {
                    instances.extend(f.instances());
                }
            }
        }
    }
    instances.sort();
    instances.dedup();
    world.resource_mut::<Selection>().0 = instances
        .into_iter()
        .map(|i| crate::viewport::Pick::Part(super::occurrence_part(i)))
        .collect();
}

/// A changed form field of the panel: an axis's settings (`mb-ax<n>-…`), the magazine, or
/// the controller's.
fn set_field(b: &mut MachineBuilder, key: &str, v: &Value) {
    if let Some(rest) = key.strip_prefix("mb-ax") {
        let Some((n, what)) = rest.split_once('-') else {
            return;
        };
        let Some(i) = n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) else {
            return;
        };
        let d = &mut b.draft;
        match (what, v) {
            ("zero", Value::Activate) => d.zero_here(i),
            ("home-here", Value::Activate) => d.home_here(i),
            ("from-mate", Value::Activate) => d.travel_from_mate(i),
            ("home", Value::Number(x)) => d.set_home(i, *x),
            ("min", Value::Number(x)) => d.set_travel(i, false, *x),
            ("max", Value::Number(x)) => d.set_travel(i, true, *x),
            ("cont", Value::Bool(on)) => d.set_continuous(i, *on),
            ("rapid", Value::Number(x)) => d.set_rapid(i, *x),
            ("feed", Value::Number(x)) => d.set_max_feed(i, *x),
            // (A click on a field, or text that could not be read: shown again as it was.)
            _ => {}
        }
        b.touch();
        return;
    }
    match (key, v) {
        ("mb-magazine", Value::Index(i)) => b.draft.set_magazine(i.checked_sub(1)),
        (_, Value::Activate) => return,
        _ => {
            let mut from = None;
            let c = &mut b.draft.def.controller;
            crate::cam::dialogs::set_controller(c, &mut b.by_kind, &mut from, key, v);
            // (A catalogue machine's travel is not the assembly's: its axes give it.)
            c.chain = None;
        }
    }
    b.touch();
}

fn take_fields(mut changes: MessageReader<FieldChange>, builder: Option<ResMut<MachineBuilder>>) {
    let Some(mut b) = builder else {
        changes.clear();
        return;
    };
    for c in changes.read() {
        set_field(&mut b, &c.key, &c.value);
    }
}

// ---------------------------------------------------------------------------------------------
// The view: picking a mount, the mounts' triads

/// A click in the view while a mount is armed: the connector there becomes the mount (an
/// explicit connector near the pointer, the Origin, else the implicit point of the face, edge
/// or vertex under it), as the mate dialog takes its connectors.
#[allow(clippy::too_many_arguments)]
fn take_picks(
    mut picks: MessageReader<PickRequest>,
    builder: Option<ResMut<MachineBuilder>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    cache: Res<PartCache>,
    shown: Res<crate::pattern::MateConnectorsShown>,
    mut filter: ResMut<PickFilterOverride>,
) {
    let (Some(mut b), Some(doc)) = (builder, doc) else {
        picks.clear();
        return;
    };
    // The view's clicks are the builder's while a mount is armed (they select otherwise).
    let want = b.picking.is_some();
    if want != b.filtering {
        filter.0 = want.then(super::pick_filter);
        b.filtering = want;
    }
    let Some(which) = b.picking else {
        picks.clear();
        return;
    };
    for p in picks.read() {
        let at = rect.offset(vdrag.pointer());
        let explicit = shown
            .0
            .then(|| {
                let list = super::connector_tool::explicit_connectors(&doc, &mut parts, &cache);
                super::connector_tool::nearest_explicit(&list, &view.view, at)
            })
            .flatten()
            .map(|(c, _)| c);
        let origin = (p.0 == Some(crate::viewport::Pick::Origin)).then(MateConnector::origin);
        let implicit = || {
            let (instance, entity) = p.0.as_ref().and_then(entity_of)?;
            let pts = connectors::entity_points(&doc, &mut parts, instance, entity)?;
            let best = pts.nearest(&view.view, at)?;
            Some(MateConnector::implicit(instance, &best))
        };
        let Some(conn) = explicit.or(origin).or_else(implicit) else {
            continue;
        };
        b.draft.set_mount(which, Some(fixed(conn)));
        // The other mount next, while it is still to be set.
        let other = match which {
            Mount::Tool => Mount::Table,
            Mount::Table => Mount::Tool,
        };
        b.picking = b.draft.mount(other).is_none().then_some(other);
        b.touch();
        break;
    }
}

/// A picked connector as a mount: at the frame it has now on its instance, whatever it was
/// picked on. A job's machine is worked out from the document alone
/// ([`cadrs_core::cam::machine_of`], which has no built parts to find a face on), where a
/// connector that names a face, an edge or a source connector counts as lost; a fixed frame
/// (as the Origin and the assembly's own connectors) always resolves.
pub(crate) fn fixed(c: MateConnector) -> MateConnector {
    use cadrs_core::assembly::connector::ConnectorAnchor;
    match c.anchor {
        ConnectorAnchor::Frame | ConnectorAnchor::Local { .. } => c,
        _ if c.is_origin() => c,
        _ => MateConnector::at(c.instance, c.local_frame(None)),
    }
}

/// The mounts' triads (the tool mount's ring orange, the table mount's blue), and while a
/// mount is armed the points of what is under the pointer with the connector a click takes.
#[allow(clippy::too_many_arguments)]
fn draw_mounts(
    builder: Option<Res<MachineBuilder>>,
    doc: Option<Res<ActiveDocument>>,
    mut parts: ResMut<super::AssemblyParts>,
    highlight: Res<PlaneHighlight>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    vdrag: Res<ViewportDrag>,
    mut halo: Gizmos<ConnectorHaloGizmos>,
    mut line: Gizmos<ConnectorGizmos>,
) {
    let (Some(b), Some(doc)) = (builder, doc) else {
        return;
    };
    let v = view.view;
    let now = b
        .draft
        .resolved
        .as_ref()
        .map(|r| r.mounts(&b.draft.values()));
    let frames = match now {
        Some((tool, table)) => [tool, table],
        None => b.mounts,
    };
    for (f, ring) in frames.iter().zip([
        Color::srgb_u8(0xf0, 0x8c, 0x1e),
        Color::srgb_u8(0x22, 0x55, 0xe0),
    ]) {
        if let Some(f) = f {
            connectors::draw_glyph(&mut line, &mut halo, &v, f, ring);
        }
    }
    if b.picking.is_none() {
        return;
    }
    let Some((instance, entity)) = highlight.viewport.as_ref().and_then(entity_of) else {
        return;
    };
    let Some(pts) = connectors::entity_points(&doc, &mut parts, instance, entity) else {
        return;
    };
    let dots: Vec<Vec3> = pts
        .points
        .iter()
        .map(|p| connectors::v3(pts.world(p).origin))
        .collect();
    connectors::draw_points(&mut halo, &mut line, &v, &dots);
    if let Some(best) = pts.nearest(&v, rect.offset(vdrag.pointer())) {
        connectors::draw_glyph(
            &mut line,
            &mut halo,
            &v,
            &pts.world(&best),
            crate::parts::HOVER,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Scripted steps

/// Scripted steps (`Custom("machine …")`): `open` docks the builder on the active assembly;
/// `jog <axis> <value>` jogs an axis by name (an actuator by its name too); `home` and `reset`
/// are the footer's buttons; `accept` is ✓; `expand <axis>` opens an axis row's settings;
/// `close` closes the panel.
pub fn script(world: &mut World, arg: &str) {
    let words: Vec<&str> = arg.split_whitespace().collect();
    if words == ["open"] {
        *world.resource_mut::<SidePanel>() = SidePanel::MachineBuilder;
        return;
    }
    if words == ["close"] {
        *world.resource_mut::<SidePanel>() = SidePanel::None;
        return;
    }
    if words == ["accept"] {
        accept(world);
        return;
    }
    let Some(mut b) = world.get_resource_mut::<MachineBuilder>() else {
        warn!("machine {arg}: the Machine Builder is not open");
        return;
    };
    match words.as_slice() {
        ["jog", name, value] => {
            let Ok(v) = value.parse::<f64>() else {
                warn!("machine {arg}: not a number");
                return;
            };
            if let Some(i) = b.draft.axis_named(name) {
                b.draft.jog(i, v);
            } else if let Some(k) = b.draft.def.actuators.iter().position(|a| a.name == *name) {
                b.draft.jog_actuator(k, v);
            } else {
                warn!("machine {arg}: no such axis");
                return;
            }
            b.moved = true;
        }
        ["home"] => apply(&mut b, Act::Home),
        ["reset"] => apply(&mut b, Act::ResetPose),
        ["expand", name] => match b.draft.axis_named(name) {
            Some(i) => apply(&mut b, Act::Expand(i)),
            None => warn!("machine {arg}: no such axis"),
        },
        _ => warn!("machine {arg}: not a Machine Builder step"),
    }
}
