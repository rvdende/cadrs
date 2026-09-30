//! The Revolve dialog (P3.4, PS7), laid out as Onshape's (`training/intro-to-part-studios/
//! ex2-step5.png`):
//!
//! - **Solid | Surface | Thin**, then **New | Add | Remove | Intersect** (not for surfaces); a
//!   new revolve picks New or Add by itself, as an extrude does.
//! - **Faces and sketch regions to revolve** (one row per region, "Face of Sketch 3", or a whole
//!   sketch picked in the feature list), then the **Revolve axis** field ("Edge of Sketch 3")
//!   with the mate connector button beside it (P3.10, PS7.2: on, the next pick is a mate
//!   connector, an explicit one or the implicit one of a face, edge, vertex or the origin; an
//!   explicit connector can also be picked straight into the field).
//! - The **revolve type** (Full, Blind, Symmetric, Up to next / face / part / vertex) with the
//!   flip button (Blind and the "Up to" types); **Revolve angle** for Blind and Symmetric; the
//!   "Up to" target and its **Offset angle** option with its own flip.
//! - The **Thin** tab's Thickness 1 with Flip wall, Mid plane and Thickness 2 (hidden with Mid
//!   plane).
//! - **Second end position** (Blind and the "Up to" types): its own type, angle or target and
//!   offset.
//! - **Merge with all**, and the **Merge scope** below it (not for New).
//!
//! It shares the Extrude dialog's session ([`crate::extrude::ExtrudeSession`]): one selection
//! field at a time takes the viewport's picks; every change goes through the command layer.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::document::{
    BodyType, BooleanOp, EndType, Offset, REVOLVE_SECOND_ENDS, RevolveType, UpTo,
    default_angle_offset, default_revolve_second,
};
use cadrs_core::{Feature, FeatureId, PartId, RevolveFeature};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField,
    NumberFieldCommit, NumberFieldState, OptionRow, Select, SelectChange, SelectState,
    SelectionList, SelectionListActivate, SelectionListRemove, SelectionListState,
    TabStrip, TabStripSelect,
};

use crate::extrude::{ExtrudeField, ExtrudeSession};
use crate::extrude_dialog::flip_button_any;
use crate::parts::PartCache;
use crate::revolve::{rparams, set_rparams};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub(crate) struct RevolveDialogPlugin;

impl Plugin for RevolveDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_tab)
            .add_observer(on_select)
            .add_observer(on_checkbox)
            .add_observer(on_number)
            .add_observer(on_button)
            .add_observer(on_list_remove)
            .add_observer(on_list_activate);
    }
}

/// The dialog.
#[derive(Component)]
pub struct RevolveDialog;

/// What decides which rows the dialog has.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) struct RevolveLayout {
    body: BodyType,
    op: BooleanOp,
    kind: RevolveType,
    offset: bool,
    second: Option<(EndType, bool)>,
    mid_plane: bool,
    merge_all: bool,
    /// P3.10: the axis field's mate connector button is on.
    connector: bool,
}

impl RevolveLayout {
    fn of(r: &RevolveFeature, field: ExtrudeField) -> Self {
        Self {
            body: r.body,
            op: r.op,
            kind: r.kind,
            offset: r.offset.is_some(),
            second: r.second.as_ref().filter(|_| r.kind.one_sided()).map(|s| (s.end, s.offset.is_some())),
            mid_plane: r.thin.mid_plane,
            merge_all: r.merge_all,
            connector: field == ExtrudeField::AxisConnector,
        }
    }
}

/// What an interactive part of the dialog is for.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Body,
    Op,
    Input,
    Axis,
    Kind,
    Flip,
    Angle,
    UpTo,
    OffsetValue,
    OffsetFlip,
    SecondEndType,
    SecondFlip,
    SecondAngle,
    SecondUpTo,
    SecondOffsetValue,
    SecondOffsetFlip,
    Thickness1,
    Thickness2,
    FlipWall,
    MergeScope,
    /// P3.10: the axis field's mate connector button.
    AxisConnector,
}

/// What the dialog's selection fields list.
#[derive(Debug, Clone, Default, PartialEq)]
struct Labels {
    input: Vec<String>,
    /// P3D.1: which inputs no longer resolve.
    input_missing: Vec<bool>,
    axis: Vec<String>,
    up_to: Vec<String>,
    second_up_to: Vec<String>,
    merge_scope: Vec<String>,
}

fn feature_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn up_to_label(features: &[Feature], cache: &PartCache, u: &UpTo) -> String {
    match u {
        UpTo::Face(f) => format!("Face of {}", feature_name(features, f.face.op)),
        UpTo::Part(p) => cache.part_name(*p).unwrap_or("Part").to_string(),
        UpTo::Vertex(v) => format!("Vertex of {}", feature_name(features, v.vertex.faces[0].op)),
    }
}

fn labels(features: &[Feature], cache: &PartCache, feature: FeatureId, r: &RevolveFeature) -> Labels {
    let name = |id: FeatureId| features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone());
    // A surface revolves the region's boundary curves: "Curves of Sketch 1" (P3.4 judge).
    let what = if r.body == cadrs_core::BodyType::Surface { "Curves" } else { "Face" };
    let mut input: Vec<String> = r.regions.iter().map(|x| format!("{what} of {}", name(x.sketch))).collect();
    input.extend(r.sketches.iter().map(|s| crate::extrude_dialog::whole_sketch_label(features, *s, r.body)));
    input.extend(r.faces.iter().map(|f| format!("Face of {}", feature_name(features, f.face.op))));
    let input_missing = crate::extrude_dialog::missing_marks(cache, feature, &mut input);
    let part_name = |p: &PartId| cache.part_name(*p).unwrap_or("Part").to_string();
    let merge_scope = if !r.merge_scope.is_empty() {
        r.merge_scope.iter().map(part_name).collect()
    } else {
        cache
            .contacts
            .get(&feature)
            .map(|c| match r.op {
                BooleanOp::Add => c.touches.iter().map(part_name).collect(),
                _ => c.overlaps.iter().map(part_name).collect(),
            })
            .unwrap_or_default()
    };
    Labels {
        input,
        input_missing,
        axis: r.axis.iter().map(|a| crate::revolve::axis_label(features, a)).collect(),
        up_to: r.up_to.iter().map(|u| up_to_label(features, cache, u)).collect(),
        second_up_to: r
            .second
            .iter()
            .flat_map(|s| s.up_to.iter())
            .map(|u| up_to_label(features, cache, u))
            .collect(),
        merge_scope,
    }
}

// ---------------------------------------------------------------------------------------------
// Building

fn list(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, role: Role, items: Vec<String>, active: bool) {
    b.spawn((
        role,
        SelectionList::new(name.to_string())
            .placeholder(placeholder)
            .items(items)
            .active(active)
            .build(t),
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.flex_grow = 0.0;
        n.margin = UiRect::vertical(Val::Px(2.0));
    });
}

/// A select row with an optional flip button after it.
fn select_row(b: &mut ChildSpawner, t: &Theme, name: &str, role: Role, options: &[&'static str], selected: usize, flip: Option<(Role, bool)>) {
    let mut select = Select::new(name.to_string());
    for o in options {
        select = select.option(*o, true);
    }
    b.spawn(Node {
        height: Val::Px(30.0),
        margin: UiRect::top(Val::Px(3.0)),
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn((role, select.selected(selected).build(t)));
        if let Some((flip_role, flipped)) = flip {
            flip_button_any(r, t, &format!("{name}-flip"), flip_role, flipped, "Opposite direction");
        }
    });
}

/// A number field with a flip button after it.
fn offset_row(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, role: Role, flip_role: Role, o: &Offset) {
    b.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        margin: UiRect::left(Val::Px(18.0)),
        ..default()
    })
    .with_children(|r| {
        r.spawn((
            role,
            NumberField::new(name.to_string(), label.to_string())
                .text(o.expr.clone())
                .label_width(82.0)
                .build(t),
        ))
        .entry::<Node>()
        .and_modify(|mut n| n.flex_grow = 1.0);
        flip_button_any(r, t, &format!("{name}-flip"), flip_role, o.flip, "Opposite direction");
    });
}

fn placeholder(end: EndType) -> &'static str {
    match end {
        EndType::UpToFace => "Up to face",
        EndType::UpToPart => "Up to part",
        _ => "Up to vertex",
    }
}

/// The fields of an end: its angle, or its "Up to" target and offset angle.
#[allow(clippy::too_many_arguments)]
fn end_fields(
    b: &mut ChildSpawner,
    t: &Theme,
    prefix: &str,
    end: EndType,
    angle_expr: &str,
    offset: &Option<Offset>,
    up_to_items: Vec<String>,
    up_to_active: bool,
    roles: [Role; 4],
) {
    let [angle_role, up_to_role, offset_role, offset_flip_role] = roles;
    match end {
        EndType::Blind => {
            b.spawn((
                angle_role,
                NumberField::new(format!("{prefix}angle"), "Revolve angle")
                    .text(angle_expr.to_string())
                    .label_width(92.0)
                    .build(t),
            ));
        }
        EndType::UpToNext | EndType::ThroughAll => {}
        e => list(b, t, &format!("{prefix}up-to-field"), placeholder(e), up_to_role, up_to_items, up_to_active),
    }
    if end.is_up_to() {
        b.spawn(OptionRow::new(format!("{prefix}offset"), "Offset angle").checked(offset.is_some()).build(t));
        if let Some(o) = offset {
            offset_row(b, t, &format!("{prefix}offset-angle"), "Offset angle", offset_role, offset_flip_role, o);
        }
    }
}

fn revolve_dialog(theme: &Theme, title: &str, valid: bool, r: &RevolveFeature, field: ExtrudeField, labels: &Labels) -> impl Bundle {
    let tb = theme.clone();
    let tf = theme.clone();
    let r = r.clone();
    let layout = RevolveLayout::of(&r, field);
    let labels = labels.clone();
    (
        RevolveDialog,
        layout,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("revolve-dialog")
            .title(title)
            .valid(valid)
            .body_padding(UiRect::ZERO)
            .body(move |b| {
                let t = &tb;
                let body_index = match r.body {
                    BodyType::Solid => 0,
                    BodyType::Surface => 1,
                    BodyType::Thin => 2,
                };
                b.spawn((
                    Role::Body,
                    TabStrip::new("revolve-body-type")
                        .compact()
                        .tab("Solid")
                        .tab("Surface")
                        .tab("Thin")
                        .selected(body_index)
                        .build(t),
                ));
                if r.body != BodyType::Surface {
                    let op_index = BooleanOp::ALL.iter().position(|o| *o == r.op).unwrap_or(0);
                    let mut strip = TabStrip::new("revolve-operation").compact();
                    for op in BooleanOp::ALL {
                        strip = strip.tab(op.label());
                    }
                    b.spawn((Role::Op, strip.selected(op_index).build(t)));
                }
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(Val::Px(2.0), Val::Px(3.0), Val::Px(6.0), Val::ZERO),
                    ..default()
                })
                .with_children(|b| {
                    let input = if r.body == BodyType::Surface {
                        "Sketch curves to revolve"
                    } else {
                        "Faces and sketch regions to revolve"
                    };
                    list(b, t, "revolve-regions-field", input, Role::Input, labels.input.clone(), field == ExtrudeField::Input);
                    // The axis field with the mate connector button beside it (`ex2-step5`).
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Role::Axis,
                            SelectionList::new("revolve-axis-field")
                                .placeholder("Revolve axis")
                                .items(labels.axis.clone())
                                .active(matches!(field, ExtrudeField::Axis | ExtrudeField::AxisConnector))
                                .build(t),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| {
                            n.flex_grow = 1.0;
                            n.margin = UiRect::vertical(Val::Px(2.0));
                        });
                        // P3.10 (PS7.2): the next pick is a mate connector (implicit ones too).
                        row.spawn((
                            Role::AxisConnector,
                            IconButton::new("revolve-axis-mate-connector", "mate-connector")
                                .icon_size(16.0)
                                .selected(field == ExtrudeField::AxisConnector)
                                .tooltip("Select a mate connector")
                                .build(t),
                        ));
                    });
                    let kinds: Vec<&'static str> = RevolveType::ALL.iter().map(|k| k.label()).collect();
                    let kind_index = RevolveType::ALL.iter().position(|k| *k == r.kind).unwrap_or(0);
                    let flip = r.kind.one_sided().then_some((Role::Flip, r.flip));
                    select_row(b, t, "revolve-type", Role::Kind, &kinds, kind_index, flip);
                    if r.kind != RevolveType::Full {
                        end_fields(
                            b,
                            t,
                            "revolve-",
                            r.kind.end_type(),
                            &r.angle_expr,
                            &r.offset,
                            labels.up_to.clone(),
                            field == ExtrudeField::UpTo,
                            [Role::Angle, Role::UpTo, Role::OffsetValue, Role::OffsetFlip],
                        );
                    }
                    if r.body == BodyType::Thin {
                        b.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                Role::Thickness1,
                                NumberField::new("revolve-thickness1", "Thickness 1")
                                    .text(r.thin.thickness1_expr.clone())
                                    .label_width(70.0)
                                    .build(t),
                            ))
                            .entry::<Node>()
                            .and_modify(|mut n| n.flex_grow = 1.0);
                            flip_button_any(row, t, "revolve-flip-wall", Role::FlipWall, r.thin.flip_wall, "Flip wall");
                        });
                        b.spawn(OptionRow::new("revolve-mid-plane", "Mid plane").checked(r.thin.mid_plane).build(t));
                        if !r.thin.mid_plane {
                            b.spawn((
                                Role::Thickness2,
                                NumberField::new("revolve-thickness2", "Thickness 2")
                                    .text(r.thin.thickness2_expr.clone())
                                    .label_width(70.0)
                                    .build(t),
                            ));
                        }
                    }
                    if r.kind.one_sided() {
                        b.spawn(
                            OptionRow::new("revolve-second-end", "Second end position")
                                .chevron()
                                .checked(r.second.is_some())
                                .build(t),
                        );
                        if let Some(sec) = &r.second {
                            let ends: Vec<&'static str> = REVOLVE_SECOND_ENDS.iter().map(|e| e.label()).collect();
                            let i = REVOLVE_SECOND_ENDS.iter().position(|e| *e == sec.end).unwrap_or(0);
                            select_row(b, t, "revolve-second-end-type", Role::SecondEndType, &ends, i, Some((Role::SecondFlip, !r.flip)));
                            end_fields(
                                b,
                                t,
                                "revolve-second-",
                                sec.end,
                                &sec.depth_expr,
                                &sec.offset,
                                labels.second_up_to.clone(),
                                field == ExtrudeField::SecondUpTo,
                                [Role::SecondAngle, Role::SecondUpTo, Role::SecondOffsetValue, Role::SecondOffsetFlip],
                            );
                        }
                    }
                    if r.op != BooleanOp::New && r.body != BodyType::Surface {
                        b.spawn(OptionRow::new("revolve-merge-all", "Merge with all").checked(r.merge_all).build(t));
                        if !r.merge_all {
                            list(b, t, "revolve-merge-scope-field", "Merge scope", Role::MergeScope, labels.merge_scope.clone(), field == ExtrudeField::MergeScope);
                        }
                    }
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Node {
                    flex_grow: 1.0,
                    padding: UiRect::left(Val::Px(2.0)),
                    ..default()
                })
                .with_child(crate::feature_list::preview_slider(t, "revolve"));
                crate::extrude_dialog::final_button(f, t, "revolve");
                f.spawn((
                    Name::new("revolve-help"),
                    icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)),
                    Tooltip::new("Help"),
                ));
            })
            .build(theme),
    )
}

// ---------------------------------------------------------------------------------------------
// Keeping it in step

/// Spawns, updates and removes the dialog.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn sync_revolve_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ExtrudeSession>>,
    arrow: Res<crate::revolve::AngleArrow>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    focus: Res<InputFocus>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &RevolveLayout, &mut FeatureDialogState), With<RevolveDialog>>,
    mut q_lists: Query<(&Role, &mut SelectionListState)>,
    mut q_numbers: Query<(Entity, &Role, &mut NumberFieldState)>,
    mut q_selects: Query<(&Role, &mut SelectState)>,
    q_edit: Query<&cadrs_ui::NumberFieldEdit>,
    mut commands: Commands,
) {
    let revolve = session.as_ref().zip(doc.as_ref()).and_then(|(s, d)| {
        let el = d.doc.element(s.element)?;
        let f = el.feature(s.feature)?;
        f.revolve()?;
        Some((el, f))
    });
    let (Some(s), Some((el, feature))) = (session.as_ref(), revolve) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(r) = feature.revolve() else { return };
    let shown = arrow.drag.as_ref().map_or(r, |d| &d.revolve);
    let failed = cache.errors.contains_key(&s.feature);
    let valid = feature.is_valid() && !failed && !cache.rebuilding;
    let labels = labels(el.features(), &cache, s.feature, r);
    let layout = RevolveLayout::of(r, s.field);
    let current = q_dialog.iter().next().map(|(ent, l, _)| (ent, l.clone()));
    match current {
        Some((_, ref l)) if *l == layout => {}
        other => {
            if let Some((ent, _)) = other {
                commands.entity(ent).try_despawn();
            }
            let Some(area) = q_area.iter().next() else {
                return;
            };
            let dialog = commands
                .spawn(revolve_dialog(&theme, &feature.name, valid, shown, s.field, &labels))
                .id();
            commands.entity(area).add_child(dialog);
            return;
        }
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState {
            title: feature.name.clone(),
            valid,
            error: false,
        };
        if *st != want {
            *st = want;
        }
    }
    for (role, mut l) in &mut q_lists {
        let (items, active) = match role {
            Role::Input => (&labels.input, s.field == ExtrudeField::Input),
            Role::Axis => (&labels.axis, matches!(s.field, ExtrudeField::Axis | ExtrudeField::AxisConnector)),
            Role::UpTo => (&labels.up_to, s.field == ExtrudeField::UpTo),
            Role::SecondUpTo => (&labels.second_up_to, s.field == ExtrudeField::SecondUpTo),
            Role::MergeScope => (&labels.merge_scope, s.field == ExtrudeField::MergeScope),
            _ => continue,
        };
        let red = if matches!(role, Role::Input) { labels.input_missing.clone() } else { Vec::new() };
        let want = SelectionListState {
            items: items.clone(),
            active,
            error: !red.is_empty(),
            red_items: false,
            red,
        };
        if *l != want {
            *l = want;
        }
    }
    let editing = focus.get().and_then(|f| q_edit.get(f).ok()).map(|e| e.0);
    for (entity, role, mut n) in &mut q_numbers {
        let text = match role {
            Role::Angle => shown.angle_expr.clone(),
            Role::OffsetValue => shown.offset.as_ref().map(|o| o.expr.clone()).unwrap_or_default(),
            Role::SecondAngle => shown.second.as_ref().map(|s| s.depth_expr.clone()).unwrap_or_default(),
            Role::SecondOffsetValue => shown
                .second
                .as_ref()
                .and_then(|s| s.offset.as_ref())
                .map(|o| o.expr.clone())
                .unwrap_or_default(),
            Role::Thickness1 => shown.thin.thickness1_expr.clone(),
            Role::Thickness2 => shown.thin.thickness2_expr.clone(),
            _ => continue,
        };
        if editing == Some(entity) || n.error {
            continue;
        }
        let want = NumberFieldState { text, error: false };
        if *n != want {
            *n = want;
        }
    }
    for (role, mut sel) in &mut q_selects {
        let i = match role {
            Role::Kind => RevolveType::ALL.iter().position(|k| *k == shown.kind).unwrap_or(0),
            Role::SecondEndType => match &shown.second {
                Some(sec) => REVOLVE_SECOND_ENDS.iter().position(|e| *e == sec.end).unwrap_or(0),
                None => continue,
            },
            _ => continue,
        };
        if sel.selected != i {
            sel.selected = i;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<RevolveDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(crate::extrude::accept_extrude);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<RevolveDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(crate::extrude::cancel_extrude);
    }
}

fn change(commands: &mut Commands, label: &'static str, f: impl FnOnce(&mut RevolveFeature) + Send + 'static) {
    commands.queue(move |world: &mut World| {
        if let Some(mut r) = rparams(world) {
            let before = r.clone();
            f(&mut r);
            if r != before {
                set_rparams(world, r, label);
            }
        }
    });
}

fn set_field(commands: &mut Commands, field: ExtrudeField) {
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>()
            && s.field != field
        {
            s.field = field;
        }
    });
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&Role>, mut commands: Commands) {
    let index = ev.index;
    match q.get(ev.entity) {
        Ok(Role::Body) => {
            let body = [BodyType::Solid, BodyType::Surface, BodyType::Thin][index.min(2)];
            change(&mut commands, "Body type", move |r| {
                r.body = body;
                if body == BodyType::Surface {
                    r.op = BooleanOp::New;
                }
            });
        }
        Ok(Role::Op) => {
            let op = BooleanOp::ALL[index.min(3)];
            commands.queue(|world: &mut World| {
                if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
                    s.op_auto = false;
                }
            });
            change(&mut commands, op.label(), move |r| r.op = op);
        }
        _ => {}
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Role>, mut commands: Commands) {
    match q.get(ev.entity) {
        Ok(Role::Kind) => {
            let kind = RevolveType::ALL[ev.index.min(RevolveType::ALL.len() - 1)];
            change(&mut commands, "Revolve type", move |r| {
                if r.kind != kind {
                    r.kind = kind;
                    r.up_to = None;
                    if !kind.end_type().is_up_to() || !kind.one_sided() {
                        r.offset = None;
                    }
                    if !kind.one_sided() {
                        r.second = None;
                    }
                }
            });
            if kind.end_type().needs_target() && kind.one_sided() {
                set_field(&mut commands, ExtrudeField::UpTo);
            }
        }
        Ok(Role::SecondEndType) => {
            let end = REVOLVE_SECOND_ENDS[ev.index.min(REVOLVE_SECOND_ENDS.len() - 1)];
            change(&mut commands, "Second end type", move |r| {
                if let Some(s) = &mut r.second
                    && s.end != end
                {
                    s.end = end;
                    s.up_to = None;
                    if !end.is_up_to() {
                        s.offset = None;
                    }
                }
            });
            if end.needs_target() {
                set_field(&mut commands, ExtrudeField::SecondUpTo);
            }
        }
        _ => {}
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let on = ev.checked;
    match name.as_str() {
        "revolve-offset-checkbox" => change(&mut commands, "Offset angle", move |r| {
            r.offset = on.then(default_angle_offset);
        }),
        "revolve-second-offset-checkbox" => change(&mut commands, "Offset angle", move |r| {
            if let Some(s) = &mut r.second {
                s.offset = on.then(default_angle_offset);
            }
        }),
        "revolve-second-end-checkbox" => change(&mut commands, "Second end position", move |r| {
            r.second = on.then(default_revolve_second);
        }),
        "revolve-mid-plane-checkbox" => change(&mut commands, "Mid plane", move |r| r.thin.mid_plane = on),
        "revolve-merge-all-checkbox" => change(&mut commands, "Merge with all", move |r| r.merge_all = on),
        _ => {}
    }
}

/// A change to a revolve's parameters, applied later.
type Change = Box<dyn FnOnce(&mut RevolveFeature) + Send>;

fn on_number(
    ev: On<NumberFieldCommit>,
    q: Query<&Role>,
    mut q_state: Query<&mut NumberFieldState>,
    units: Res<crate::WorkspaceUnits>,
    vars: Res<crate::variables_ui::ActiveVariables>,
    mut commands: Commands,
) {
    let Ok(role) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.trim().to_string();
    let angle = matches!(role, Role::Angle | Role::SecondAngle | Role::OffsetValue | Role::SecondOffsetValue);
    let q = if angle { Quantity::Angle } else { Quantity::Length };
    let parsed = vars.eval(&units.0, &text, q);
    let zero_ok = matches!(role, Role::OffsetValue | Role::SecondOffsetValue | Role::Thickness2);
    let max = if matches!(role, Role::Angle | Role::SecondAngle) { 360.0 } else { f64::INFINITY };
    let v = match parsed {
        Ok(v) if v.is_finite() && (v > 0.0 || (zero_ok && v >= 0.0)) && v <= max + 1e-9 => v,
        _ => {
            if let Ok(mut s) = q_state.get_mut(ev.entity) {
                s.text = text;
                s.error = true;
            }
            return;
        }
    };
    if let Ok(mut s) = q_state.get_mut(ev.entity) {
        s.error = false;
    }
    // A bare number gets its unit ("90" → "90 deg", "0.1" → "0.1 in").
    let expr = if text.parse::<f64>().is_ok() {
        units.0.with_unit(v, q)
    } else {
        text
    };
    let enter = ev.enter;
    let (label, f): (&'static str, Change) = match role {
        Role::Angle => ("Revolve angle", Box::new(move |r| {
            r.angle = v;
            r.angle_expr = expr;
        })),
        Role::OffsetValue => ("Offset angle", Box::new(move |r| {
            if let Some(o) = &mut r.offset {
                o.value = v;
                o.expr = expr;
            }
        })),
        Role::SecondAngle => ("Second angle", Box::new(move |r| {
            if let Some(s) = &mut r.second {
                s.depth = v;
                s.depth_expr = expr;
            }
        })),
        Role::SecondOffsetValue => ("Offset angle", Box::new(move |r| {
            if let Some(o) = r.second.as_mut().and_then(|s| s.offset.as_mut()) {
                o.value = v;
                o.expr = expr;
            }
        })),
        Role::Thickness1 => ("Thickness 1", Box::new(move |r| {
            r.thin.thickness1 = v;
            r.thin.thickness1_expr = expr;
        })),
        Role::Thickness2 => ("Thickness 2", Box::new(move |r| {
            r.thin.thickness2 = v;
            r.thin.thickness2_expr = expr;
        })),
        _ => return,
    };
    commands.queue(move |world: &mut World| {
        if let Some(mut r) = rparams(world) {
            let before = r.clone();
            f(&mut r);
            if r != before {
                set_rparams(world, r, label);
            }
        }
        if enter {
            world.resource_mut::<InputFocus>().clear();
            crate::extrude::accept_extrude(world);
        }
    });
}

fn on_button(a: On<Activate>, q: Query<&Role>, mut commands: Commands) {
    match q.get(a.entity) {
        Ok(Role::AxisConnector) => commands.queue(|world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<ExtrudeSession>() {
                s.field = if s.field == ExtrudeField::AxisConnector { ExtrudeField::Axis } else { ExtrudeField::AxisConnector };
            }
        }),
        Ok(Role::Flip) | Ok(Role::SecondFlip) => change(&mut commands, "Flip direction", |r| r.flip = !r.flip),
        Ok(Role::OffsetFlip) => change(&mut commands, "Flip offset", |r| {
            if let Some(o) = &mut r.offset {
                o.flip = !o.flip;
            }
        }),
        Ok(Role::SecondOffsetFlip) => change(&mut commands, "Flip offset", |r| {
            if let Some(o) = r.second.as_mut().and_then(|s| s.offset.as_mut()) {
                o.flip = !o.flip;
            }
        }),
        Ok(Role::FlipWall) => change(&mut commands, "Flip wall", |r| r.thin.flip_wall = !r.thin.flip_wall),
        _ => {}
    }
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&Role>, mut commands: Commands) {
    let i = ev.index;
    match q.get(ev.entity) {
        Ok(Role::Input) => change(&mut commands, "Remove selection", move |r| {
            let (n, m) = (r.regions.len(), r.sketches.len());
            if i < n {
                r.regions.remove(i);
            } else if i - n < m {
                r.sketches.remove(i - n);
            } else if i - n - m < r.faces.len() {
                r.faces.remove(i - n - m);
            }
        }),
        Ok(Role::Axis) => change(&mut commands, "Remove selection", |r| r.axis = None),
        Ok(Role::UpTo) => change(&mut commands, "Remove selection", |r| r.up_to = None),
        Ok(Role::SecondUpTo) => change(&mut commands, "Remove selection", |r| {
            if let Some(s) = &mut r.second {
                s.up_to = None;
            }
        }),
        Ok(Role::MergeScope) => {
            commands.queue(move |world: &mut World| {
                let Some(s) = world.get_resource::<ExtrudeSession>().map(|s| s.feature) else {
                    return;
                };
                let Some(mut r) = rparams(world) else { return };
                if r.merge_scope.is_empty() {
                    let auto = world.resource::<PartCache>().contacts.get(&s).map(|c| match r.op {
                        BooleanOp::Add => c.touches.clone(),
                        _ => c.overlaps.clone(),
                    });
                    r.merge_scope = auto.unwrap_or_default();
                }
                if i < r.merge_scope.len() {
                    r.merge_scope.remove(i);
                    set_rparams(world, r, "Remove from merge scope");
                }
            });
        }
        _ => {}
    }
}

fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&Role>, mut commands: Commands) {
    let field = match q.get(ev.entity) {
        Ok(Role::Input) => ExtrudeField::Input,
        Ok(Role::Axis) => ExtrudeField::Axis,
        Ok(Role::UpTo) => ExtrudeField::UpTo,
        Ok(Role::SecondUpTo) => ExtrudeField::SecondUpTo,
        Ok(Role::MergeScope) => ExtrudeField::MergeScope,
        _ => return,
    };
    set_field(&mut commands, field);
}
