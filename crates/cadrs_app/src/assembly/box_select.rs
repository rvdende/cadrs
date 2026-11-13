//! **Box select in the assembly view and the Part Studio** (P3H.6, for PCB8/PCB10 "Group, box-select all the
//! components"; the sketch's box select, `reference/onshape/box_select.md`): pressing on empty
//! space and dragging draws a box. Left to right is a **window** (a blue box: the instances
//! entirely inside it), right to left a **crossing** (a dashed yellow box: every instance it
//! touches). Only the box is previewed while dragging; hit testing runs once on release.
//! Releasing selects them (Ctrl or Shift adds them to the selection). It works while
//! the Group dialog is open, whose Instances field follows the selection. In a Part Studio it
//! selects parts: with no dialog open and no sketch edited, or into the Composite part's and
//! the Transform's parts.
//!
//! Names: `asm-box-select` (the box).

use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;

use super::InstanceId;
use crate::ActiveDocument;
use crate::parts::PartCache;
use crate::viewport::{
    Pick, Selection, ViewportArea, ViewportRect, ViewportView, pointer_over_viewport,
};

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

use cadrs_ui::selection_box::crossing;

/// The parts a box from `a` to `b` (viewport offsets) selects: every shown part's projected
/// points inside the box (window), or its projected extent overlapping it (crossing), in the
/// view's order.
pub fn parts_in_box(
    cache: &PartCache,
    view: &crate::camera::ViewState,
    a: Vec2,
    b: Vec2,
) -> Vec<cadrs_core::PartId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let cross = crossing(a, b);
    let mut out = Vec::new();
    for part in cache.shown() {
        let pts = &part.solid.positions;
        if pts.is_empty() {
            continue;
        }
        let proj = pts
            .iter()
            .map(|p| view.project(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)));
        let hit = if cross {
            let (mut pmin, mut pmax) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
            for q in proj {
                pmin = pmin.min(q);
                pmax = pmax.max(q);
            }
            pmin.x <= hi.x && pmax.x >= lo.x && pmin.y <= hi.y && pmax.y >= lo.y
        } else {
            proj.into_iter()
                .all(|q| q.x >= lo.x && q.x <= hi.x && q.y >= lo.y && q.y <= hi.y)
        };
        if hit && !out.contains(&part.id) {
            out.push(part.id);
        }
    }
    out
}

/// The instances a box from `a` to `b` (viewport offsets) selects ([`parts_in_box`]), as their
/// top-level instances.
pub fn instances_in_box(
    cache: &PartCache,
    view: &crate::camera::ViewState,
    a: Vec2,
    b: Vec2,
) -> Vec<InstanceId> {
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
        Res<crate::eda::Eda2d>,
    ),
    mut state: ResMut<AssemblyBox>,
    mut selection: ResMut<Selection>,
    (composite, applied, mut commands): (
        Option<Res<crate::composite_ui::CompositeSession>>,
        Option<Res<crate::applied::AppliedSession>>,
        Commands,
    ),
    (mode, q_dialogs, xyz, ring): (
        Option<Res<State<crate::sketch::PartStudioMode>>>,
        Query<(), With<cadrs_ui::FeatureDialogState>>,
        Res<crate::transform_ui::XyzArrows>,
        Res<crate::transform_ui::RotateRing>,
    ),
) {
    let in_assembly = doc
        .as_deref()
        .is_some_and(|d| super::active_assembly(d).is_some());
    // P3H.6: in a Part Studio, the Composite part dialog and the Transform's Parts to transform
    // take a box's parts.
    let to_transform =
        applied.is_some_and(|s| s.field == crate::applied::AppliedField::TransformParts);
    let studio_box = !in_assembly && (composite.is_some() || to_transform);
    // In a Part Studio with no dialog open and no sketch edited, a box selects parts.
    let in_studio = doc
        .as_deref()
        .and_then(|d| d.active_element())
        .is_some_and(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }));
    let plain_studio = !in_assembly
        && !studio_box
        && in_studio
        && mode.is_some_and(|m| *m.get() == crate::sketch::PartStudioMode::Modeling)
        && q_dialogs.is_empty();
    if !in_assembly && !studio_box && !plain_studio {
        inputs.clear();
        *state = AssemblyBox::default();
        return;
    }
    let blocked = busy.0.is_some()
        || busy.1.is_some()
        || busy.2.is_some()
        || busy.3.is_some()
        || busy.4.editing()
        || busy.6.plane().is_some();
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
                // A press on a Transform's manipulator (its arrows, its ring) is theirs: not
                // the start of a box that would add the parts it sweeps.
                if xyz.hovered.is_some() || xyz.drag.is_some() || ring.active() {
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
                    // Moving the rectangle must not scan geometry or update part highlights.
                    // Resolve the complete selection once, when the button is released.
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
                        commands.queue(move |world: &mut World| {
                            crate::transform_ui::add_parts(world, &parts)
                        });
                    } else {
                        commands.queue(move |world: &mut World| {
                            crate::composite_ui::pick_parts(world, &parts, false)
                        });
                    }
                    continue;
                }
                let add = keys.any_pressed([
                    KeyCode::ControlLeft,
                    KeyCode::ControlRight,
                    KeyCode::ShiftLeft,
                    KeyCode::ShiftRight,
                ]);
                let mut picks: Vec<Pick> = if add { selection.0.clone() } else { Vec::new() };
                if plain_studio {
                    for p in parts_in_box(&cache, &view.view, rect.offset(s), rect.offset(pos)) {
                        if !picks.contains(&Pick::Part(p)) {
                            picks.push(Pick::Part(p));
                        }
                    }
                    if selection.0 != picks {
                        selection.0 = picks;
                    }
                    continue;
                }
                let found = instances_in_box(&cache, &view.view, rect.offset(s), rect.offset(pos));
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
}

/// Draws the box ([`cadrs_ui::selection_box`]): window blue, crossing dashed yellow.
pub(super) fn draw_box(
    state: Res<AssemblyBox>,
    rect: Res<ViewportRect>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut boxes: cadrs_ui::selection_box::Boxes,
    mut commands: Commands,
) {
    let span = match (state.start, state.current, state.active) {
        (Some(s), Some(c), true) => Some((s - rect.0.min, c - rect.0.min)),
        _ => None,
    };
    cadrs_ui::selection_box::sync(
        &mut commands,
        &mut boxes,
        "asm-box-select",
        q_area.iter().next(),
        span,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::picking::{backend::HitData, hover::HoverMap, pointer::Location};
    use cadrs_core::{
        Document, Element, FeatureId, PartId,
        parts::{Part, PartKind},
        solid::Solid,
    };

    fn scene(assembly: bool) -> (App, Entity, Pick) {
        let mut doc = Document::empty("Box selection");
        doc.elements.push(if assembly {
            Element::assembly("Assembly")
        } else {
            Element::part_studio("Parts")
        });
        let element = doc.elements[0].id;
        let mut cache = PartCache::default();
        cache.assembly = assembly.then_some(element);
        for i in 0..54 {
            let feature = FeatureId::new();
            let x = (i % 9) as f64 * 10.0 - 40.0;
            let y = (i / 9) as f64 * 10.0 - 25.0;
            cache.parts.push(Part {
                id: PartId::new(feature, 0),
                feature,
                name: format!("Part {i}"),
                kind: PartKind::Solid,
                palette: i,
                mass: None,
                features: vec![feature],
                source: None,
                derived: None,
                solid: std::sync::Arc::new(Solid {
                    positions: vec![[x - 1.0, y - 1.0, 0.0], [x + 1.0, y + 1.0, 0.0]],
                    ..default()
                }),
            });
        }
        let existing = Pick::Part(PartId::new(FeatureId::new(), 0));
        let mut app = App::new();
        app.add_message::<PointerInput>()
            .insert_resource(ActiveDocument::new(doc))
            .insert_resource(cache)
            .insert_resource(ViewportView {
                view: crate::camera::ViewState {
                    scale: 1.0,
                    ..crate::camera::ViewState::standard(crate::camera::StandardView::Top)
                },
                ..default()
            })
            .insert_resource(ViewportRect(Rect::new(0.0, 0.0, 200.0, 200.0)))
            .insert_resource(State::new(crate::sketch::PartStudioMode::Modeling))
            .insert_resource(Selection(vec![existing]))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::triad::Triad>()
            .init_resource::<super::super::explode::ExplodeUi>()
            .init_resource::<super::super::drag::InstanceDrag>()
            .init_resource::<crate::eda::Eda2d>()
            .init_resource::<crate::parts::HoverParts>()
            .init_resource::<AssemblyBox>()
            .add_systems(Update, box_select);
        let area = app.world_mut().spawn(ViewportArea).id();
        let mut hover = HoverMap::default();
        hover
            .entry(PointerId::Mouse)
            .or_default()
            .insert(area, HitData::new(area, 0.0, None, None));
        app.insert_resource(hover);
        (app, area, existing)
    }

    fn pointer(app: &mut App, area: Entity, action: PointerAction, position: Vec2) {
        let target = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(area))
            .normalize(None)
            .unwrap();
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location { target, position },
            action,
        ));
        app.update();
    }

    fn drag(app: &mut App, area: Entity, crossing: bool) -> Vec2 {
        let start = if crossing {
            Vec2::splat(160.0)
        } else {
            Vec2::splat(40.0)
        };
        let end = if crossing {
            Vec2::splat(40.0)
        } else {
            Vec2::splat(160.0)
        };
        pointer(
            app,
            area,
            PointerAction::Press(PointerButton::Primary),
            start,
        );
        for at in [Vec2::splat(100.0), end] {
            pointer(app, area, PointerAction::Move { delta: at - start }, at);
            assert!(app.world().resource::<AssemblyBox>().active());
            assert_eq!(
                *app.world().resource::<crate::parts::HoverParts>(),
                crate::parts::HoverParts::default(),
                "dragging must not pre-highlight the enclosed parts"
            );
        }
        end
    }

    #[test]
    fn fifty_four_parts_are_selected_only_on_release() {
        for assembly in [false, true] {
            for crossing in [false, true] {
                let (mut app, area, existing) = scene(assembly);
                let end = drag(&mut app, area, crossing);
                assert_eq!(app.world().resource::<Selection>().0, vec![existing]);
                pointer(
                    &mut app,
                    area,
                    PointerAction::Release(PointerButton::Primary),
                    end,
                );
                let selection = &app.world().resource::<Selection>().0;
                assert_eq!(selection.len(), 54);
                assert!(!selection.contains(&existing));
                assert!(!app.world().resource::<AssemblyBox>().active());
            }
        }
    }

    #[test]
    fn additive_box_selection_is_committed_on_release_and_cancel_keeps_selection() {
        for key in [KeyCode::ControlLeft, KeyCode::ShiftLeft] {
            let (mut app, area, existing) = scene(true);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            let end = drag(&mut app, area, false);
            assert_eq!(app.world().resource::<Selection>().0, vec![existing]);
            pointer(
                &mut app,
                area,
                PointerAction::Release(PointerButton::Primary),
                end,
            );
            assert_eq!(app.world().resource::<Selection>().0.len(), 55);
            assert!(app.world().resource::<Selection>().0.contains(&existing));
        }
        let (mut app, area, existing) = scene(false);
        let end = drag(&mut app, area, false);
        pointer(&mut app, area, PointerAction::Cancel, end);
        assert_eq!(app.world().resource::<Selection>().0, vec![existing]);
        assert!(!app.world().resource::<AssemblyBox>().active());
    }
}
