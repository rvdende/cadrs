//! **Box select in the assembly view** (P3H.6, for PCB8/PCB10 "Group, box-select all the
//! components"; the sketch's box select, `reference/onshape/box_select.md`): pressing on empty
//! space and dragging draws a box. Left to right is a **window** (a blue box: the instances
//! entirely inside it), right to left a **crossing** (a dashed yellow box: every instance it
//! touches). Releasing selects them (Ctrl or Shift adds them to the selection). It works while
//! the Group dialog is open, whose Instances field follows the selection.
//!
//! Names: `asm-box-select` (the box).

use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;

use super::InstanceId;
use crate::parts::PartCache;
use crate::viewport::{Pick, Selection, ViewportArea, ViewportRect, ViewportView, pointer_over_viewport};
use crate::ActiveDocument;

/// A box being dragged: where it started (window px), and whether the press was on empty space.
#[derive(Resource, Default)]
pub struct AssemblyBox {
    start: Option<Vec2>,
    current: Option<Vec2>,
    active: bool,
}

impl AssemblyBox {
    pub fn active(&self) -> bool {
        self.active
    }
}

#[derive(Component)]
pub(super) struct BoxFill {
    built: Option<(Vec2, bool)>,
}

#[derive(Component)]
struct BoxDash;

/// Whether the corners `a`, `b` (viewport offsets) are a crossing box (dragged right to left).
fn crossing(a: Vec2, b: Vec2) -> bool {
    b.x < a.x
}

/// The parts a box from `a` to `b` (viewport offsets) selects: every shown part's projected
/// points inside the box (window), or its projected extent overlapping it (crossing), in the
/// view's order.
pub fn parts_in_box(cache: &PartCache, view: &crate::camera::ViewState, a: Vec2, b: Vec2) -> Vec<cadrs_core::PartId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let cross = crossing(a, b);
    let mut out = Vec::new();
    for part in cache.shown() {
        let pts = &part.solid.positions;
        if pts.is_empty() {
            continue;
        }
        let proj = pts.iter().map(|p| view.project(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)));
        let hit = if cross {
            let (mut pmin, mut pmax) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
            for q in proj {
                pmin = pmin.min(q);
                pmax = pmax.max(q);
            }
            pmin.x <= hi.x && pmax.x >= lo.x && pmin.y <= hi.y && pmax.y >= lo.y
        } else {
            proj.into_iter().all(|q| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y)
        };
        if hit && !out.contains(&part.id) {
            out.push(part.id);
        }
    }
    out
}

/// The instances a box from `a` to `b` (viewport offsets) selects ([`parts_in_box`]), as their
/// top-level instances.
pub fn instances_in_box(cache: &PartCache, view: &crate::camera::ViewState, a: Vec2, b: Vec2) -> Vec<InstanceId> {
    let mut out: Vec<InstanceId> = Vec::new();
    for p in parts_in_box(cache, view, a, b) {
        let i = InstanceId::of_part(p);
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

/// Starts, follows and ends a box drag in an assembly tab.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn box_select(
    mut inputs: MessageReader<PointerInput>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    hover: Res<bevy::picking::hover::HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    keys: Res<ButtonInput<KeyCode>>,
    triad: Res<super::triad::Triad>,
    busy: (
        Option<Res<super::mate_dialog::MateSession>>,
        Option<Res<super::insert::InsertSession>>,
        Option<Res<super::animate::Playback>>,
        Option<Res<super::connector_tool::ConnectorSession>>,
        Res<super::explode::ExplodeUi>,
        Res<super::drag::InstanceDrag>,
    ),
    mut state: ResMut<AssemblyBox>,
    mut selection: ResMut<Selection>,
    (composite, applied, mut pre, mut commands): (
        Option<Res<crate::composite_ui::CompositeSession>>,
        Option<Res<crate::applied::AppliedSession>>,
        ResMut<crate::parts::HoverParts>,
        Commands,
    ),
) {
    let in_assembly = doc.as_deref().is_some_and(|d| super::active_assembly(d).is_some());
    // P3H.6: in a Part Studio, the Composite part dialog and the Transform's Parts to transform
    // take a box's parts.
    let to_transform = applied.is_some_and(|s| s.field == crate::applied::AppliedField::TransformParts);
    let studio_box = !in_assembly && (composite.is_some() || to_transform);
    if !in_assembly && !studio_box {
        inputs.clear();
        *state = AssemblyBox::default();
        if !pre.2.is_empty() {
            pre.2.clear();
        }
        return;
    }
    let blocked = busy.0.is_some() || busy.1.is_some() || busy.2.is_some() || busy.3.is_some() || busy.4.editing();
    let over = pointer_over_viewport(&hover, &q_area);
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                *state = AssemblyBox::default();
                if !over || blocked || triad.dragging() || triad.hover.is_some() {
                    continue;
                }
                // Only a press on empty space starts a box (a press on an instance drags it).
                if crate::parts::pick_face(&cache, &view.view, rect.offset(pos)).is_some() {
                    continue;
                }
                state.start = Some(pos);
            }
            PointerAction::Move { .. } => {
                if let Some(s) = state.start {
                    state.current = Some(pos);
                    if !state.active && s.distance(pos) >= 4.0 && !busy.5.dragging() {
                        state.active = true;
                    }
                    // P3H.6 judge: what the box will select is pre-highlighted while it is dragged.
                    if state.active {
                        let want = preview_parts(&cache, &view.view, rect.offset(s), rect.offset(pos), studio_box);
                        if pre.2 != want {
                            pre.2 = want;
                        }
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) | PointerAction::Cancel => {
                let (Some(s), true) = (state.start, state.active) else {
                    *state = AssemblyBox::default();
                    continue;
                };
                *state = AssemblyBox::default();
                if matches!(input.action, PointerAction::Cancel) {
                    continue;
                }
                // (The viewport doesn't take a release this far from its press as a click.)
                if studio_box {
                    let parts = parts_in_box(&cache, &view.view, rect.offset(s), rect.offset(pos));
                    if to_transform {
                        commands.queue(move |world: &mut World| crate::transform_ui::add_parts(world, &parts));
                    } else {
                        commands.queue(move |world: &mut World| crate::composite_ui::pick_parts(world, &parts, false));
                    }
                    continue;
                }
                let found = instances_in_box(&cache, &view.view, rect.offset(s), rect.offset(pos));
                let add = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::ShiftLeft, KeyCode::ShiftRight]);
                let mut picks: Vec<Pick> = if add { selection.0.clone() } else { Vec::new() };
                for i in found {
                    let p = Pick::Part(i.part_id());
                    if !picks.iter().any(|q| super::instance_of(q) == Some(i)) {
                        picks.push(p);
                    }
                }
                if selection.0 != picks {
                    selection.0 = picks;
                }
            }
            _ => {}
        }
    }
    if !state.active && !pre.2.is_empty() {
        pre.2.clear();
    }
}

/// The parts a box will select, for its pre-highlight: in an assembly every part of the
/// instances it selects, in a Part Studio the parts themselves.
fn preview_parts(cache: &PartCache, view: &crate::camera::ViewState, a: Vec2, b: Vec2, studio: bool) -> Vec<cadrs_core::PartId> {
    if studio {
        return parts_in_box(cache, view, a, b);
    }
    let found = instances_in_box(cache, view, a, b);
    cache.shown().filter(|p| found.contains(&InstanceId::of_part(p.id))).map(|p| p.id).collect()
}

/// Draws the box: window a blue outline over a blue fill, crossing a dashed yellow outline over
/// a yellow fill (the sketch box's colours).
#[allow(clippy::type_complexity)]
pub(super) fn draw_box(
    state: Res<AssemblyBox>,
    rect: Res<ViewportRect>,
    mut q: Query<(Entity, &mut BoxFill, &mut Node, &mut BackgroundColor, &mut BorderColor)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let (Some(start), Some(cur), true) = (state.start, state.current, state.active) else {
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let cross = crossing(start, cur);
    let lo = start.min(cur).round();
    let hi = start.max(cur).round();
    let size = hi - lo;
    let (fill, stroke) = if cross {
        (Color::srgba_u8(0xff, 0xe0, 0x45, 102), Color::srgb_u8(0xf0, 0xb4, 0x00))
    } else {
        (Color::srgba_u8(0x24, 0x93, 0xcc, 94), Color::srgb_u8(0x5a, 0x8c, 0xcb))
    };
    let local = lo - rect.0.min;
    let node = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(local.x),
        top: Val::Px(local.y),
        width: Val::Px(size.x),
        height: Val::Px(size.y),
        border: if cross { UiRect::ZERO } else { UiRect::all(Val::Px(1.0)) },
        ..default()
    };
    let (e, rebuild) = match q.iter_mut().next() {
        Some((e, mut b, mut n, mut bg, mut border)) => {
            if *n != node {
                *n = node;
            }
            bg.set_if_neq(BackgroundColor(fill));
            border.set_if_neq(BorderColor::all(stroke));
            let want = Some((size, cross));
            let rebuild = b.built != want;
            b.built = want;
            (e, rebuild)
        }
        None => {
            let Some(area) = q_area.iter().next() else { return };
            let e = commands
                .spawn((Name::new("asm-box-select"), BoxFill { built: Some((size, cross)) }, node, BackgroundColor(fill), BorderColor::all(stroke), Pickable::IGNORE))
                .id();
            commands.entity(area).add_child(e);
            (e, true)
        }
    };
    if !rebuild {
        return;
    }
    commands.entity(e).despawn_related::<Children>();
    if !cross {
        return;
    }
    // Dashes: 4 px on, 3 px off, along each side.
    let mut dashes: Vec<(f32, f32, f32, f32)> = Vec::new();
    let run = |len: f32, place: &mut dyn FnMut(f32, f32)| {
        let mut t = 0.0;
        while t < len {
            place(t, (len - t).min(4.0));
            t += 7.0;
        }
    };
    run(size.x, &mut |t, l| {
        dashes.push((t, 0.0, l, 1.0));
        dashes.push((t, size.y - 1.0, l, 1.0));
    });
    run(size.y, &mut |t, l| {
        dashes.push((0.0, t, 1.0, l));
        dashes.push((size.x - 1.0, t, 1.0, l));
    });
    commands.entity(e).with_children(|p| {
        for (x, y, w, h) in dashes {
            p.spawn((
                BoxDash,
                Node { position_type: PositionType::Absolute, left: Val::Px(x), top: Val::Px(y), width: Val::Px(w), height: Val::Px(h), ..default() },
                BackgroundColor(stroke),
                Pickable::IGNORE,
            ));
        }
    });
}
