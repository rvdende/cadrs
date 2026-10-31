//! The Mirror and Offset sketch tools (`intro-to-sketching.md` S19), following Onshape:
//!
//! - **Mirror** (toolbar; Onshape gives it no key): pick the mirror line first (it turns
//!   yellow), then each entity picked is mirrored at once, as one undo step, linked to its
//!   source by a Symmetric constraint ([`SketchOp::Mirror`]). With a line and entities already
//!   selected, choosing Mirror mirrors them in the first-selected line. Once the line is
//!   picked, pressing on empty space and dragging draws a selection box (`box_select.md`: left
//!   to right takes the curves inside it, right to left those it crosses too), and releasing
//!   mirrors every curve it found as one undo step (#37). A prompt says which step is next
//!   ([`mirror_prompt`]); Enter or Esc ends the tool.
//! - **Offset (O):** click curves to add/remove them, or drag a curve to select its chain.
//!   Connected pieces join at their corners; disconnected profiles remain separate. Click
//!   empty space or press Enter to place and type the distance. Further selections reuse the
//!   session's driving distance. Move across the geometry to choose the side; the arrow is
//!   a direction indicator, not another selection/confirmation step.
//!
//! - **Offset of a face region** (P3D.4, IR6.10): in a sketch on a part face, clicking inside
//!   the face (where no entity is) offsets the face's outer loop, its edges taken from the
//!   kernel as exact curves and used as construction geometry ([`SketchOp::OffsetLoop`]); the
//!   preview, the side, the arrow and the distance box work as for a chain.
//!
//! Both change the sketch only through [`cadrs_core::commands::EditSketch`] commands.

use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::tr;
use std::collections::VecDeque;

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_sketch::edit::{OffsetShape, chain_of, offset_geometry};
use cadrs_sketch::geom::dist_point_segment;
use cadrs_sketch::hit::hit_test;
use cadrs_sketch::{
    CurveId, CurveKind, CurveRef, DimensionKind, PlaneFrame, Sketch, SketchEntity, SketchOp,
};

use crate::sketch::{ActiveSketchTool, PartStudioMode, SketchSession, SketchTool};
use crate::sketch_draw::SketchRubberGizmos;
use crate::sketch_tools::{
    QuickDimFlow, QuickDimTarget, SVec2, ScreenMap, SketchScreen, SketchSelection, SketchToolsSet,
    execute, over_viewport, session_sketch,
};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct SketchEditToolsPlugin;

impl Plugin for SketchEditToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OffsetTool>()
            .add_systems(
                Update,
                (edit_tools_keys, edit_tools_pointer)
                    .chain()
                    .before(crate::sketch::sketch_keys)
                    .in_set(SketchToolsSet)
                    .after(crate::sketch_tools::sketch_pointer)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                mirror_prompt
                    .after(SketchToolsSet)
                    .run_if(in_state(PartStudioMode::Sketching))
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), close_mirror_prompt)
            .add_systems(
                Update,
                draw_offset_preview
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), reset_offset);
    }
}

/// The Offset tool's state.
#[derive(Resource, Debug, Default)]
pub struct OffsetTool {
    /// The chain being offset (curve, runs backwards), empty until an entity is picked.
    pub chain: Vec<(CurveId, bool)>,
    additional: Vec<(Vec<(CurveId, bool)>, bool)>,
    /// Shared distance while this invocation of Offset remains active.
    distance_dimension: Option<cadrs_sketch::DimensionId>,
    /// Offset to the left of the chain's direction of travel.
    pub left: bool,
    /// The distance the preview is at (mm): the cursor's distance from the chain.
    pub distance: f64,
    /// The direction arrow on screen: its base and tip (px).
    pub arrow: Option<(Vec2, Vec2)>,
    press: Option<(Vec2, Option<SketchEntity>)>,
    cursor: Option<Vec2>,
    last_tool: SketchTool,
    /// P3D.4: offsetting the face region the sketch is on: its loop, and the sketch with the
    /// loop used (what `chain` is in).
    pub face: Option<FaceLoop>,
}

/// A face's outer loop being offset (P3D.4, IR6.10).
#[derive(Debug, Clone)]
pub struct FaceLoop {
    pub items: Vec<(cadrs_sketch::projection::Projected, cadrs_sketch::Link)>,
    pub sketch: Sketch,
}

impl OffsetTool {
    /// The sketch `chain` is in: the face's copy while offsetting a face region.
    fn chain_sketch<'a>(&'a self, sketch: &'a Sketch) -> &'a Sketch {
        self.face.as_ref().map_or(sketch, |f| &f.sketch)
    }

    fn clear(&mut self) {
        self.face = None;
        self.chain.clear();
        self.additional.clear();
        self.arrow = None;
        self.press = None;
    }
}

/// Partition only the picked curves, keeping the caller's first curve as the stable anchor.
/// Running chain_of on the selected subgraph avoids accidentally selecting an entire sketch
/// or joining unrelated profiles. A branch has no unambiguous offset corner.
fn selected_chains(
    sketch: &Sketch,
    picked: &[CurveId],
) -> Result<Vec<Vec<(CurveId, bool)>>, String> {
    let mut subset = sketch.clone();
    subset.curves.retain(|id, _| picked.contains(&id));
    for &id in picked {
        let Some((a, b)) = subset.curve_ends(id) else {
            continue;
        };
        for p in [a, b] {
            let count = subset
                .curves
                .keys()
                .filter(|c| {
                    subset.curve_ends(*c).is_some_and(|(x, y)| {
                        subset.curves[*c].construction == subset.curves[id].construction
                            && [x, y]
                                .iter()
                                .any(|q| subset.pos(*q).distance(subset.pos(p)) < 1e-7)
                    })
                })
                .count();
            if count > 2 {
                return Err(tr("sketch-offset-branches"));
            }
        }
    }
    let mut groups: Vec<Vec<(CurveId, bool)>> = Vec::new();
    for &id in picked {
        if !subset.curves.contains_key(id) || groups.iter().flatten().any(|(c, _)| *c == id) {
            continue;
        }
        groups.push(chain_of(&subset, id).0);
    }
    Ok(groups)
}

impl OffsetTool {
    fn groups(&self) -> impl Iterator<Item = (&[(CurveId, bool)], bool)> {
        std::iter::once((self.chain.as_slice(), self.left))
            .chain(self.additional.iter().map(|(c, l)| (c.as_slice(), *l)))
            .filter(|(c, _)| !c.is_empty())
    }

    fn pick(
        &mut self,
        sketch: &Sketch,
        curves: &[CurveId],
        toggle: bool,
        at: SVec2,
    ) -> Result<(), String> {
        let old: Vec<_> = self.groups().map(|(c, l)| (c.to_vec(), l)).collect();
        let mut picked: Vec<_> = old
            .iter()
            .flat_map(|(c, _)| c.iter().map(|(id, _)| *id))
            .collect();
        for &c in curves {
            if !sketch.curves.contains_key(c) {
                continue;
            }
            if toggle && picked.contains(&c) {
                picked.retain(|p| *p != c);
            } else if !picked.contains(&c) {
                picked.push(c);
            }
        }
        let chains = selected_chains(sketch, &picked)?;
        let mut groups: Vec<_> = chains
            .into_iter()
            .map(|chain| {
                // Preserve the physical side when adding a preceding or reversed segment changes
                // which way the ordered chain runs.
                let left = old
                    .iter()
                    .find_map(|(prev, left)| {
                        chain.iter().find_map(|(c, rev)| {
                            prev.iter()
                                .find(|(p, _)| p == c)
                                .map(|(_, was_rev)| *left ^ *rev ^ *was_rev)
                        })
                    })
                    .unwrap_or_else(|| side_at(sketch, &chain, at));
                (chain, left)
            })
            .collect();
        self.chain.clear();
        self.additional.clear();
        if !groups.is_empty() {
            let (chain, left) = groups.remove(0);
            self.chain = chain;
            self.left = left;
            self.additional = groups;
        }
        self.arrow = None;
        Ok(())
    }

    fn reference(&self, sketch: &Sketch) -> Option<cadrs_sketch::DimensionId> {
        self.distance_dimension.filter(|id| sketch.dimensions.get(*id).is_some_and(|d|
            matches!(d.kind, DimensionKind::Offset { source, target } if sketch.curves.contains_key(source) && sketch.curves.contains_key(target))))
    }
}

/// Build an atomic edit from existing sketch operations. Each disconnected profile is offset
/// independently; all copies share one driving dimension through EqualOffset constraints.
fn offset_operation(
    sketch: &Sketch,
    groups: &[(Vec<(CurveId, bool)>, bool)],
    distance: f64,
    reference: Option<cadrs_sketch::DimensionId>,
) -> Result<(SketchOp, cadrs_sketch::DimensionId), String> {
    use cadrs_sketch::{ConstraintOf, CurveRef};
    let mut trial = sketch.clone();
    let mut ops = Vec::new();
    let mut dim = reference.filter(|id| trial.dimensions.contains_key(*id));
    // Offsetting an existing copy starts a new dimension instead of creating a dependency
    // back onto itself (which would hold the copy still when its parent dimension changes).
    if let Some(id) = dim {
        let DimensionKind::Offset { source, target } = trial.dimensions[id].kind else {
            return Err(tr("sketch-offset-not-offset-dimension"));
        };
        let targets: Vec<_> = std::iter::once(target)
            .chain(trial.constraints.values().filter_map(|c| match c {
                ConstraintOf::EqualOffset(CurveRef::Curve(s), _, _, CurveRef::Curve(t))
                    if *s == source =>
                {
                    Some(*t)
                }
                _ => None,
            }))
            .collect();
        if groups
            .iter()
            .flat_map(|(c, _)| c)
            .any(|(c, _)| targets.contains(c))
        {
            dim = None;
        }
    }
    for (chain, left) in groups {
        if chain.is_empty() {
            continue;
        }
        let before: Vec<_> = trial.constraints.keys().collect();
        let made = cadrs_sketch::edit::offset(&mut trial, chain, distance, *left, (0.0, 0.0))?;
        let own = trial
            .dimensions
            .iter()
            .find_map(|(id, d)| {
                matches!(d.kind, DimensionKind::Offset { target, .. } if target == made[0])
                    .then_some(id)
            })
            .ok_or_else(|| tr("sketch-offset-missing-dimension"))?;
        ops.push(SketchOp::Offset {
            chain: chain.clone(),
            distance,
            left: *left,
            label: (0.0, 0.0),
        });
        if let Some(master) = dim {
            let DimensionKind::Offset { source, target } = trial.dimensions[master].kind else {
                return Err(tr("sketch-offset-not-offset-dimension"));
            };
            let constraints: Vec<_> = trial
                .constraints
                .iter()
                .filter_map(|(id, c)| {
                    (!before.contains(&id) && matches!(c, ConstraintOf::EqualOffset(..)))
                        .then_some(id)
                })
                .collect();
            trial.dimensions.remove(own);
            for c in &constraints {
                trial.constraints.remove(*c);
            }
            ops.push(SketchOp::Delete {
                curves: vec![],
                points: vec![],
                dimensions: vec![own],
                constraints,
            });
            let constraints: Vec<_> = chain
                .iter()
                .zip(&made)
                .map(|((c, _), made)| {
                    ConstraintOf::EqualOffset(
                        CurveRef::Curve(source),
                        CurveRef::Curve(target),
                        CurveRef::Curve(*c),
                        CurveRef::Curve(*made),
                    )
                })
                .collect();
            for c in &constraints {
                trial.add_constraint(*c);
            }
            ops.push(SketchOp::AddConstraint {
                constraints,
                label: "Offset",
            });
        } else {
            dim = Some(own);
        }
    }
    Ok((
        SketchOp::Batch(ops),
        dim.ok_or_else(|| tr("sketch-offset-nothing"))?,
    ))
}

fn offset_selection_error(commands: &mut Commands, message: String) {
    commands.queue(move |world: &mut World| {
        let theme = world.resource::<cadrs_ui::Theme>().clone();
        cadrs_ui::show_notification(
            &mut world.commands(),
            &theme,
            cadrs_ui::Notification::warning(message).name("sketch-offset-toast"),
        );
    });
}

/// Freeze the distance at the confirmation point, not at the previous mouse-move frame.
fn confirm_offset(
    sketch: &Sketch,
    map: &ScreenMap,
    off: &mut OffsetTool,
    pos: Vec2,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    if off.cursor.is_some() {
        update_cursor_preview(sketch, map, off, pos);
    } else {
        off.distance = off
            .reference(sketch)
            .and_then(|id| sketch.dimensions.get(id))
            .map(|d| d.value)
            .unwrap_or(off.distance.max(0.01));
    }
    let placed = if let Some(face) = off.face.clone() {
        place_face_offset(sketch, &face, map, off, pos, commands, target)
    } else {
        place_offset(sketch, map, off, pos, commands, target)
    };
    if placed {
        off.clear();
    }
}

/// The Mirror tool's prompt toast.
#[derive(Component)]
struct MirrorPrompt;

/// While the Mirror tool is on, a prompt says what to pick next: the mirror line, then the
/// entities to mirror (with how to finish).
#[allow(clippy::too_many_arguments)]
fn mirror_prompt(
    tool: Res<ActiveSketchTool>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    selection: Res<SketchSelection>,
    theme: Res<cadrs_ui::Theme>,
    q_prompt: Query<Entity, With<MirrorPrompt>>,
    mut shown: Local<Option<&'static str>>,
    mut commands: Commands,
) {
    let want = match session_sketch(session.as_deref(), doc.as_deref()) {
        Some(sketch) if tool.tool == SketchTool::Mirror => {
            Some(match mirror_axis(sketch, &selection) {
                None => "sketch-mirror-select-line",
                Some(_) => "sketch-mirror-select-entities",
            })
        }
        _ => None,
    };
    if want == *shown {
        return;
    }
    *shown = want;
    for e in &q_prompt {
        commands.entity(e).try_despawn();
    }
    if let Some(key) = want {
        let toast = cadrs_ui::show_notification(
            &mut commands,
            &theme,
            cadrs_ui::Notification::info(tr(key))
                .name("mirror-prompt")
                .autohide(false),
        );
        commands.entity(toast).insert(MirrorPrompt);
    }
}

fn close_mirror_prompt(q_prompt: Query<Entity, With<MirrorPrompt>>, mut commands: Commands) {
    for e in &q_prompt {
        commands.entity(e).try_despawn();
    }
}

fn reset_offset(mut off: ResMut<OffsetTool>) {
    off.clear();
    off.last_tool = SketchTool::Select;
    off.distance_dimension = None;
}

/// Keep the previous direction directly over a source curve to avoid boundary flicker.
const SIDE_DEAD_ZONE: f64 = 3.0;
/// The flip arrow's length (px).
const ARROW_LEN: f32 = 28.0;

/// Esc drops the Offset tool's picked chain first (the next Esc leaves the tool).
#[allow(clippy::too_many_arguments)]
fn edit_tools_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    keys: Res<ButtonInput<KeyCode>>,
    dialogs: (
        Query<(), With<cadrs_ui::DialogRoot>>,
        Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    ),
    tool: Res<ActiveSketchTool>,
    mut off: ResMut<OffsetTool>,
    mut selection: ResMut<SketchSelection>,
    mut flow: ResMut<QuickDimFlow>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    mut commands: Commands,
    frame: Res<bevy::diagnostic::FrameCount>,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    if typing
        || !dialogs.0.is_empty()
        || !dialogs.1.is_empty()
        || keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ])
    {
        keys_in.clear();
        return;
    }
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed || k.repeat || typing || flow.consumed(&frame) {
            continue;
        }
        if tool.tool != SketchTool::Offset || off.chain.is_empty() {
            continue;
        }
        match k.shortcut_code() {
            KeyCode::Escape => {
                off.clear();
                selection.0.clear();
                flow.consumed_frame = Some(frame.0);
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let (Some(sketch), Some(map), Some(session)) = (
                    session_sketch(session.as_deref(), doc.as_deref()),
                    screen.active,
                    session.as_deref(),
                ) {
                    let pos = off.cursor.unwrap_or(map.origin);
                    confirm_offset(
                        sketch,
                        &map,
                        &mut off,
                        pos,
                        &mut commands,
                        (session.element, session.feature),
                    );
                    if off.chain.is_empty() {
                        selection.0.clear();
                    }
                }
                flow.consumed_frame = Some(frame.0);
            }
            _ => {}
        }
    }
}

/// The Mirror and Offset tools' clicks.
#[allow(clippy::too_many_arguments)]
fn edit_tools_pointer(
    mut inputs: MessageReader<PointerInput>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    mut tool: ResMut<ActiveSketchTool>,
    mut selection: ResMut<SketchSelection>,
    mut off: ResMut<OffsetTool>,
    flow: Res<QuickDimFlow>,
    cache: Res<crate::parts::PartCache>,
    mut commands: Commands,
) {
    let (Some(map), Some(sketch), Some(s)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
        session.as_deref(),
    ) else {
        inputs.clear();
        return;
    };
    let target = (s.element, s.feature);
    // Switching tools: leaving Mirror or Offset drops their picks; choosing Mirror with a
    // line and entities selected mirrors them.
    if off.last_tool != tool.tool {
        let from = off.last_tool;
        off.last_tool = tool.tool;
        if matches!(from, SketchTool::Mirror | SketchTool::Offset) {
            off.clear();
            off.distance_dimension = None;
            selection.0.clear();
        }
        match tool.tool {
            SketchTool::Mirror => match mirror_axis(sketch, &selection) {
                Some(axis) if selection.0.len() > 1 => {
                    let curves: Vec<CurveId> = selection.0[1..]
                        .iter()
                        .filter_map(|e| match *e {
                            SketchEntity::Curve(c) => Some(c),
                            _ => None,
                        })
                        .collect();
                    selection.0.clear();
                    if !curves.is_empty() {
                        execute(&mut commands, target, SketchOp::Mirror { axis, curves });
                    }
                    tool.tool = SketchTool::Select;
                    off.last_tool = SketchTool::Select;
                }
                Some(_) => {}
                None => selection.0.clear(),
            },
            SketchTool::Offset => {
                off.distance_dimension = None;
                let picked: Vec<_> = selection
                    .0
                    .iter()
                    .filter_map(|e| match e {
                        SketchEntity::Curve(c) => Some(*c),
                        _ => None,
                    })
                    .collect();
                let at = off
                    .cursor
                    .and_then(|p| map.to_sketch(p))
                    .unwrap_or_default();
                if let Err(e) = off.pick(sketch, &picked, false, at) {
                    offset_selection_error(&mut commands, e);
                }
                selection.0 = off
                    .groups()
                    .flat_map(|(c, _)| c.iter().map(|(c, _)| SketchEntity::Curve(*c)))
                    .collect();
            }
            _ => {}
        }
    }
    if flow.open.is_some() {
        inputs.clear();
        return;
    }
    if tool.tool == SketchTool::Offset
        && off.face.is_none()
        && off
            .groups()
            .any(|(c, _)| c.iter().any(|(id, _)| !sketch.curves.contains_key(*id)))
    {
        let at = off
            .cursor
            .and_then(|p| map.to_sketch(p))
            .unwrap_or_default();
        let _ = off.pick(sketch, &[], false, at);
        selection.0 = off
            .groups()
            .flat_map(|(c, _)| c.iter().map(|(c, _)| SketchEntity::Curve(*c)))
            .collect();
    }
    let active = matches!(tool.tool, SketchTool::Mirror | SketchTool::Offset);
    let over = over_viewport(&hover_map, &q_area);
    let hit_at = |pos: Vec2| {
        hit_test(sketch, SVec2::new(pos.x as f64, pos.y as f64), |p| {
            map.to_screen64(p)
        })
        .map(|h| h.entity)
        // The Mirror tool mirrors in an edge-on default plane too (#31).
        .or_else(|| {
            (tool.tool == SketchTool::Mirror)
                .then(|| map.axis_at(pos))
                .flatten()
        })
    };
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse || !active {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Move { .. } if over => off.cursor = Some(pos),
            PointerAction::Press(PointerButton::Primary) => {
                if over {
                    off.cursor = Some(pos);
                }
                off.press = over.then(|| (pos, hit_at(pos)));
            }
            PointerAction::Release(PointerButton::Primary) => {
                off.cursor = Some(pos);
                let Some((down, hit)) = off.press.take() else {
                    continue;
                };
                if !over {
                    continue;
                }
                let moved = down.distance(pos) > crate::sketch_tools::DRAG_THRESHOLD;
                if tool.tool == SketchTool::Mirror {
                    if !moved {
                        mirror_pick(sketch, hit, &mut selection, &mut commands, target);
                    }
                    continue;
                }
                // Source curves always edit the selection; they never place an offset.
                if off.face.is_none()
                    && let Some(SketchEntity::Curve(c)) = hit
                {
                    let curves = if moved {
                        chain_of(sketch, c)
                            .0
                            .into_iter()
                            .map(|(id, _)| id)
                            .collect()
                    } else {
                        vec![c]
                    };
                    let at = map.to_sketch(pos).unwrap_or_default();
                    if let Err(e) = off.pick(sketch, &curves, !moved, at) {
                        offset_selection_error(&mut commands, e);
                    }
                    selection.0 = off
                        .groups()
                        .flat_map(|(c, _)| c.iter().map(|(c, _)| SketchEntity::Curve(*c)))
                        .collect();
                    continue;
                }
                if off.chain.is_empty() {
                    let at = map.to_sketch(pos).unwrap_or_default();
                    if !moved
                        && hit.is_none()
                        && let Some(items) = face_loop_items(doc.as_deref(), s, &cache)
                        && cadrs_sketch::face_offset::loop_contains(&items, at)
                    {
                        let mut temp = sketch.clone();
                        if let Ok(chain) = cadrs_sketch::face_offset::use_loop(&mut temp, &items) {
                            off.left = side_at(&temp, &chain, at);
                            off.chain = chain;
                            off.face = Some(FaceLoop {
                                items,
                                sketch: temp,
                            });
                            selection.0.clear();
                        }
                    }
                    continue;
                }
                if moved || hit.is_some() {
                    continue;
                }
                confirm_offset(sketch, &map, &mut off, pos, &mut commands, target);
                if off.chain.is_empty() {
                    selection.0.clear();
                }
            }
            PointerAction::Cancel => {
                off.clear();
                selection.0.clear();
            }
            _ => {}
        }
    }
    if tool.tool == SketchTool::Offset && !off.chain.is_empty() {
        if let Some(pos) = off.cursor {
            update_cursor_preview(sketch, &map, &mut off, pos);
        }
    } else {
        off.arrow = None;
    }
}

/// The cursor chooses direction on every profile. Distance comes from the nearest selected
/// profile (or the shared driving dimension), so moving near a disconnected selection does
/// not measure from a distant, first-picked line. Arrow geometry is purely informational.
fn update_cursor_preview(sketch: &Sketch, map: &ScreenMap, off: &mut OffsetTool, pos: Vec2) {
    let Some(cursor) = map.to_sketch(pos) else {
        return;
    };
    let sketch_for_chain = off.chain_sketch(sketch);
    let groups: Vec<_> = off.groups().collect();
    if groups.is_empty() {
        off.arrow = None;
        return;
    }
    let nearest = |chain: &[(CurveId, bool)]| {
        chain
            .iter()
            .map(|(c, _)| piece_distance(sketch_for_chain, *c, cursor, true))
            .fold(f64::INFINITY, f64::min)
    };
    let active = groups
        .iter()
        .min_by(|(a, _), (b, _)| nearest(a).total_cmp(&nearest(b)))
        .unwrap()
        .0;
    let distance = off
        .reference(sketch)
        .and_then(|id| sketch.dimensions.get(id))
        .map(|d| d.value)
        .unwrap_or_else(|| distance_to_chain(sketch_for_chain, active, cursor).max(0.01));
    let sides: Vec<_> = groups
        .iter()
        .map(|(chain, left)| {
            if distance_to_chain(sketch_for_chain, chain, cursor) * map.px_per_mm() as f64
                <= SIDE_DEAD_ZONE
            {
                *left
            } else {
                side_at(sketch_for_chain, chain, cursor)
            }
        })
        .collect();
    let arrow = arrow_for(sketch_for_chain, map, &off.chain, distance, sides[0]);
    off.left = sides[0];
    for ((_, left), side) in off.additional.iter_mut().zip(sides.into_iter().skip(1)) {
        *left = side;
    }
    off.distance = distance;
    off.arrow = arrow;
}

fn is_line(s: &Sketch, c: CurveId) -> bool {
    matches!(
        s.curves.get(c).map(|c| c.kind),
        Some(CurveKind::Line { .. })
    )
}

/// The mirror line `e` gives: a line, or a sketch axis (an edge-on default plane).
fn mirror_line(s: &Sketch, e: SketchEntity) -> Option<CurveRef> {
    match e {
        SketchEntity::Curve(c) if is_line(s, c) => Some(CurveRef::Curve(c)),
        _ => e.axis(),
    }
}

/// A Mirror click: the first pick must be a line (the mirror line, kept selected); each
/// entity picked after it is mirrored at once.
fn mirror_pick(
    sketch: &Sketch,
    hit: Option<SketchEntity>,
    selection: &mut SketchSelection,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    match (mirror_axis(sketch, selection), hit) {
        (None, Some(e)) if mirror_line(sketch, e).is_some() => {
            selection.0 = vec![e];
        }
        (Some(axis), Some(SketchEntity::Curve(c))) if CurveRef::Curve(c) != axis => {
            execute(
                commands,
                target,
                SketchOp::Mirror {
                    axis,
                    curves: vec![c],
                },
            );
        }
        _ => {}
    }
}

/// The Mirror tool's line: the first selected entity, once a line (or an axis) is picked.
pub fn mirror_axis(sketch: &Sketch, selection: &SketchSelection) -> Option<CurveRef> {
    selection.0.first().and_then(|e| mirror_line(sketch, *e))
}

/// The curves the Mirror tool mirrors from what a selection box found: every curve but the
/// mirror line itself and those already mirrored in it (points, texts and the origin have no
/// mirror image here).
pub fn mirror_targets(sketch: &Sketch, axis: CurveRef, found: &[SketchEntity]) -> Vec<CurveId> {
    found
        .iter()
        .filter_map(|e| match *e {
            SketchEntity::Curve(c)
                if CurveRef::Curve(c) != axis
                    && !cadrs_sketch::edit::mirrored_in(sketch, axis, c) =>
            {
                Some(c)
            }
            _ => None,
        })
        .collect()
}

/// A Mirror selection box released (#37): every curve it found is mirrored in the picked line
/// at once, as one undo step. Nothing happens without a line, or an empty box.
pub fn mirror_box(
    sketch: &Sketch,
    selection: &SketchSelection,
    found: &[SketchEntity],
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) {
    let Some(axis) = mirror_axis(sketch, selection) else {
        return;
    };
    let curves = mirror_targets(sketch, axis, found);
    if !curves.is_empty() {
        execute(commands, target, SketchOp::Mirror { axis, curves });
    }
}

/// Which side of the chain `p` is on: true for the left of its direction of travel (for an arc
/// running counter-clockwise, or a circle, the inside).
pub fn side_at(s: &Sketch, chain: &[(CurveId, bool)], p: SVec2) -> bool {
    if chain.len() > 1
        && let (Some((a, b)), Some((c, d))) = (
            s.curve_ends(chain[0].0),
            s.curve_ends(chain[chain.len() - 1].0),
        )
    {
        let start = if chain[0].1 { b } else { a };
        let end = if chain[chain.len() - 1].1 { c } else { d };
        if s.pos(start).distance(s.pos(end)) < 1e-7 {
            let mut polygon = Vec::new();
            for &(curve, reversed) in chain {
                let mut points = cadrs_sketch::hit::curve_polyline(s, curve);
                if reversed {
                    points.reverse();
                }
                polygon.extend(points);
            }
            let area = cadrs_sketch::geom::polygon_area(&polygon);
            if area.abs() > 1e-12 {
                return cadrs_sketch::geom::point_in_polygon(p, &polygon) == (area > 0.0);
            }
        }
    }
    let nearest = chain
        .iter()
        .min_by(|a, b| piece_distance(s, a.0, p, true).total_cmp(&piece_distance(s, b.0, p, true)))
        .copied();
    let Some((c, reversed)) = nearest else {
        return true;
    };
    match s.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (from, to) = if reversed {
                (s.pos(b), s.pos(a))
            } else {
                (s.pos(a), s.pos(b))
            };
            (to - from).cross(p - from) > 0.0
        }
        Some(CurveKind::Arc { center, start, .. }) => {
            let inside = p.distance(s.pos(center)) < s.pos(start).distance(s.pos(center));
            inside != reversed
        }
        Some(CurveKind::Circle { center, radius }) => p.distance(s.pos(center)) < radius,
        // A closed ellipse (or offset ellipse) runs counter-clockwise for a positive minor radius,
        // and left of a counter-clockwise run is inside.
        Some(CurveKind::Ellipse { minor, .. } | CurveKind::EllipseOffset { minor, .. }) => {
            let inside = s.ellipse_geom(c).is_some_and(|g| g.implicit(p) < 0.0);
            inside == ((minor > 0.0) != reversed)
        }
        Some(CurveKind::Spline { .. }) => true,
        // Not offset (the Offset tool refuses them).
        Some(CurveKind::Bezier { .. } | CurveKind::EllipseArc { .. }) => true,
        None => true,
    }
}

/// The distance from `p` to a curve (`bounded`: to the segment or arc itself; otherwise to its
/// infinite line or whole circle).
fn piece_distance(s: &Sketch, c: CurveId, p: SVec2, bounded: bool) -> f64 {
    match s.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (a, b) = (s.pos(a), s.pos(b));
            if bounded {
                dist_point_segment(p, a, b)
            } else {
                let f = cadrs_sketch::dimension::foot(p, a, b);
                p.distance(f)
            }
        }
        Some(CurveKind::Arc { .. }) => {
            let g = s.arc_geom(c).unwrap_or(cadrs_sketch::ArcGeom {
                center: p,
                radius: 0.0,
                start_angle: 0.0,
                sweep: 0.0,
            });
            if bounded {
                g.distance(p)
            } else {
                (p.distance(g.center) - g.radius).abs()
            }
        }
        Some(CurveKind::Circle { center, radius }) => (p.distance(s.pos(center)) - radius).abs(),
        Some(CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. }) => {
            s.ellipse_geom(c).map_or(f64::INFINITY, |g| g.distance(p))
        }
        Some(CurveKind::EllipseArc { .. }) => s
            .ellipse_arc_geom(c)
            .map_or(f64::INFINITY, |g| g.distance(p)),
        Some(CurveKind::Spline { .. }) => s
            .spline_spans(c)
            .and_then(|sp| cadrs_sketch::spline::nearest(&sp, p))
            .map_or(f64::INFINITY, |(_, _, d)| d),
        Some(CurveKind::Bezier { .. }) => s.bezier_geom(c).map_or(f64::INFINITY, |g| g.distance(p)),
        None => f64::INFINITY,
    }
}

/// The offset distance for the cursor at `p`: its distance from the nearest piece of the
/// chain (measured square to it).
pub fn distance_to_chain(s: &Sketch, chain: &[(CurveId, bool)], p: SVec2) -> f64 {
    let nearest = chain
        .iter()
        .min_by(|a, b| piece_distance(s, a.0, p, true).total_cmp(&piece_distance(s, b.0, p, true)))
        .copied();
    nearest.map_or(0.0, |(c, _)| piece_distance(s, c, p, false))
}

/// The middle of a shape and of its source curve.
fn shape_mid(s: &Sketch, sh: &OffsetShape) -> SVec2 {
    let pts = sh.polyline(s);
    match *sh {
        OffsetShape::Line(a, b) => a.midpoint(b),
        OffsetShape::Arc { geom, .. } => geom.mid(),
        OffsetShape::Circle { .. } | OffsetShape::Ellipse { .. } => {
            pts.first().copied().unwrap_or_default()
        }
    }
}

/// The flip arrow: from the middle of the first offset piece, pointing away from its source
/// (screen px).
fn arrow_for(
    s: &Sketch,
    map: &ScreenMap,
    chain: &[(CurveId, bool)],
    distance: f64,
    left: bool,
) -> Option<(Vec2, Vec2)> {
    let shapes = offset_geometry(s, chain, distance, left).ok()?;
    let first = shapes.first()?;
    let mid = shape_mid(s, first);
    // The point of the source nearest the offset's middle.
    let (c, _) = chain[0];
    let src = match s.curves.get(c)?.kind {
        CurveKind::Line { a, b } => cadrs_sketch::dimension::foot(mid, s.pos(a), s.pos(b)),
        CurveKind::Arc { center, start, .. } => {
            let o = s.pos(center);
            o + (mid - o).normalize() * o.distance(s.pos(start))
        }
        CurveKind::Circle { center, radius } => {
            let o = s.pos(center);
            o + (mid - o).normalize() * radius
        }
        CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
            s.ellipse_geom(c)?.closest(mid)
        }
        CurveKind::Spline { .. } | CurveKind::EllipseArc { .. } => return None,
        CurveKind::Bezier { .. } => {
            let g = s.bezier_geom(c)?;
            g.point_at(g.nearest_t(mid))
        }
    };
    let (base, from) = (map.to_screen(mid), map.to_screen(src));
    let dir = (base - from).normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    Some((base, base + dir * ARROW_LEN))
}

/// Places the offset at the preview's distance, with its dimension's label at the cursor, and
/// opens the distance box.
fn place_offset(
    sketch: &Sketch,
    map: &ScreenMap,
    off: &OffsetTool,
    pos: Vec2,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) -> bool {
    let groups: Vec<_> = off.groups().map(|(c, l)| (c.to_vec(), l)).collect();
    let result = offset_operation(sketch, &groups, off.distance, off.reference(sketch));
    let (mut op, dim) = match result {
        Ok(r) => r,
        Err(e) => {
            offset_selection_error(commands, e);
            return false;
        }
    };
    let mut trial = sketch.clone();
    if let Err(e) = op.apply(&mut trial) {
        offset_selection_error(commands, e);
        return false;
    }
    if off.reference(sketch) != Some(dim) {
        let cursor = map.to_sketch(pos).unwrap_or_default();
        let label =
            cadrs_sketch::dimension::label_params(&trial, trial.dimensions[dim].kind, cursor)
                .unwrap_or_default();
        op = SketchOp::Batch(vec![
            op,
            SketchOp::MoveDimensionLabel {
                id: dim,
                offset: label.0,
                along: label.1,
            },
        ]);
    }
    execute(commands, target, op);
    commands.queue(move |world: &mut World| {
        if crate::sketch_tools::world_sketch(world).is_some_and(|s| s.dimensions.contains_key(dim))
        {
            world.resource_mut::<OffsetTool>().distance_dimension = Some(dim);
            crate::sketch_tools::open_quick_dims(world, VecDeque::from([QuickDimTarget::Dim(dim)]));
        }
    });
    true
}

/// The outer loop of the part face the sketch being edited lies on, as exact curves with their
/// links (P3D.4, IR6.10); `None` for a sketch on a plane.
fn face_loop_items(
    doc: Option<&ActiveDocument>,
    s: &SketchSession,
    cache: &crate::parts::PartCache,
) -> Option<Vec<(cadrs_sketch::projection::Projected, cadrs_sketch::Link)>> {
    let el = doc?.doc.element(s.element)?;
    let features = el.features();
    let i = features.iter().position(|f| f.id == s.feature)?;
    let plane = features[i].sketch()?.plane?;
    let items = cadrs_core::repair::face_loop(&features[..i], &cache.parts, &plane);
    (!items.is_empty()).then_some(items)
}

/// Places the offset of a face's outer loop (P3D.4, IR6.10) at the preview's distance, with
/// its dimension's label at the cursor, and opens the distance box.
fn place_face_offset(
    sketch: &Sketch,
    face: &FaceLoop,
    map: &ScreenMap,
    off: &OffsetTool,
    pos: Vec2,
    commands: &mut Commands,
    target: (cadrs_core::ElementId, cadrs_core::FeatureId),
) -> bool {
    let (distance, left) = (off.distance, off.left);
    let cursor = map.to_sketch(pos).unwrap_or_default();
    let mut trial = face.sketch.clone();
    let label = match cadrs_sketch::edit::offset(&mut trial, &off.chain, distance, left, (0.0, 0.0))
    {
        Ok(made) => trial
            .dimensions
            .values()
            .find(|d| matches!(d.kind, DimensionKind::Offset { target, .. } if target == made[0]))
            .and_then(|d| cadrs_sketch::dimension::label_params(&trial, d.kind, cursor))
            .unwrap_or((0.0, 0.0)),
        Err(e) => {
            warn!("cannot offset: {e}");
            commands.queue(offset_failed_toast);
            return false;
        }
    };
    let before: Vec<cadrs_sketch::DimensionId> = sketch.dimensions.keys().collect();
    execute(
        commands,
        target,
        SketchOp::OffsetLoop {
            items: face.items.clone(),
            distance,
            left,
            label,
        },
    );
    commands.queue(move |world: &mut World| {
        let Some(s) = crate::sketch_tools::world_sketch(world) else {
            return;
        };
        let id = s.dimensions.iter().find_map(|(k, d)| {
            (matches!(d.kind, DimensionKind::Offset { .. }) && !before.contains(&k)).then_some(k)
        });
        if let Some(id) = id {
            world.resource_mut::<OffsetTool>().distance_dimension = Some(id);
            crate::sketch_tools::open_quick_dims(world, VecDeque::from([QuickDimTarget::Dim(id)]));
        }
    });
    true
}

/// P3D.1 (IR5.3): the yellow warning when an offset can't be made at the distance asked
/// (`ex1-step10.png`).
pub(crate) fn offset_failed_toast(world: &mut World) {
    let theme = world.resource::<cadrs_ui::Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::show_notification(
        &mut commands,
        &theme,
        cadrs_ui::Notification::warning(tr("sketch-offset-failed"))
            .name("sketch-offset-toast")
            // Right of the sketch dialog's title (`ex1-step10.png`).
            .at(212.0, 6.0),
    );
    world.flush();
}

/// P3D.1 (IR5.3): true if giving the Offset dimension `id` the value in `op` leaves an offset
/// that can't exist: the solve can't hold the dimension, or an offset circle, arc or ellipse
/// would have no radius left (offset inward past its center).
pub(crate) fn offset_value_fails(s: &Sketch, id: cadrs_sketch::DimensionId, op: &SketchOp) -> bool {
    let Some(DimensionKind::Offset { source, target }) = s.dimensions.get(id).map(|d| d.kind)
    else {
        return false;
    };
    use cadrs_sketch::{ConstraintOf, CurveRef};
    let mut pairs = vec![(source, target)];
    for c in s.constraints.values() {
        if let ConstraintOf::EqualOffset(
            CurveRef::Curve(a),
            CurveRef::Curve(b),
            CurveRef::Curve(c),
            CurveRef::Curve(d),
        ) = *c
            && a == source
            && b == target
        {
            pairs.push((c, d));
        }
    }
    let radius = |g: &Sketch, c: CurveId| match g.curves.get(c).map(|c| c.kind) {
        Some(CurveKind::Circle { radius, .. }) => Some(radius),
        Some(CurveKind::Arc { .. }) => g.arc_geom(c).map(|a| a.radius),
        _ => None,
    };
    let mut trial = s.clone();
    if op.apply(&mut trial).is_err() {
        return true;
    }
    if cadrs_sketch::solve::conflicts(&trial).contains(&cadrs_sketch::solve::Source::Dimension(id))
    {
        return true;
    }
    pairs.into_iter().any(|(source, target)| {
        let side = |g: &Sketch| Some((radius(g, target)? - radius(g, source)?).signum());
        if let (Some(a), Some(b)) = (side(s), side(&trial))
            && a != b
        {
            return true;
        }
        let bad = |r: f64| !(r.is_finite() && r > 1e-9);
        match trial.curves.get(target).map(|c| c.kind) {
            Some(CurveKind::Circle { radius, .. }) => bad(radius),
            Some(CurveKind::Arc { .. }) => trial.arc_geom(target).is_none_or(|g| bad(g.radius)),
            Some(CurveKind::EllipseOffset { .. }) => trial.ellipse_geom(target).is_none(),
            Some(_) => false,
            None => true,
        }
    })
}

/// The offset preview: the offset curves in the rubber-band blue and the flip arrow.
#[allow(clippy::too_many_arguments)]
fn draw_offset_preview(
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    screen: Res<SketchScreen>,
    tool: Res<ActiveSketchTool>,
    off: Res<OffsetTool>,
    mut gizmos: Gizmos<SketchRubberGizmos>,
) {
    if tool.tool != SketchTool::Offset || off.chain.is_empty() {
        return;
    }
    let (Some(map), Some(sketch)) = (
        screen.active,
        session_sketch(session.as_deref(), doc.as_deref()),
    ) else {
        return;
    };
    let frame = map.plane.frame();
    let sketch = off.chain_sketch(sketch);
    let color = Color::srgb_u8(0x44, 0x9c, 0xcd);
    for (chain, left) in off.groups() {
        if let Ok(shapes) = offset_geometry(sketch, chain, off.distance, left) {
            for sh in &shapes {
                gizmos.linestrip(
                    sh.polyline(sketch).into_iter().map(|p| world(&frame, p)),
                    color,
                );
            }
        }
    }
    // The arrow: a shaft and a head, drawn on the sketch plane from its screen position.
    if let Some((base, tip)) = off.arrow {
        let dir = (tip - base).normalize_or_zero();
        let side = dir.perp();
        let head = [
            tip - dir * 9.0 + side * 5.0,
            tip,
            tip - dir * 9.0 - side * 5.0,
        ];
        let to_w = |p: Vec2| map.to_sketch(p).map(|q| world(&frame, q));
        let arrow = cadrs_ui::by_scheme(
            Color::srgb_u8(0x33, 0x33, 0x33),
            Color::srgb_u8(0xd0, 0xd3, 0xd8),
        );
        if let (Some(a), Some(b)) = (to_w(base), to_w(tip)) {
            gizmos.line(a, b, arrow);
        }
        let pts: Vec<Vec3> = head.iter().filter_map(|p| to_w(*p)).collect();
        if pts.len() == 3 {
            gizmos.linestrip(pts, arrow);
        }
    }
}

fn world(frame: &PlaneFrame, p: SVec2) -> Vec3 {
    let w = frame.to_world(p);
    Vec3::new(w[0] as f32, w[1] as f32, w[2] as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P3D.1 (IR5.3): a circle of radius 6 offset 3 inward can take 2, not 10 (that would
    /// put it on the other side, radius 16): "Offset could not be created at this distance."
    #[test]
    fn an_impossible_offset_distance_is_refused() {
        let mut s = Sketch::new();
        SketchOp::AddCircle {
            center: SVec2::new(0.0, 0.0),
            radius: 6.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().next().unwrap();
        let chain = vec![(c, false)];
        let left = side_at(&s, &chain, SVec2::new(3.0, 0.0));
        SketchOp::Offset {
            chain,
            distance: 3.0,
            left,
            label: (0.0, 0.0),
        }
        .apply(&mut s)
        .unwrap();
        let (id, _) = s
            .dimensions
            .iter()
            .find(|(_, d)| matches!(d.kind, DimensionKind::Offset { .. }))
            .unwrap();
        let inner = s
            .curves
            .iter()
            .find(|(k, _)| *k != c)
            .map(|(_, v)| v.kind.scalar().unwrap())
            .unwrap();
        assert!((inner - 3.0).abs() < 1e-6, "{inner}");
        assert!(!offset_value_fails(
            &s,
            id,
            &SketchOp::SetDimensionValue { id, value: 2.0 }
        ));
        assert!(offset_value_fails(
            &s,
            id,
            &SketchOp::SetDimensionValue { id, value: 10.0 }
        ));
    }

    /// #37: a box dragged over a mirror line and two circles mirrors the circles in one
    /// step; the line itself is left alone.
    #[test]
    fn a_box_mirrors_every_curve_it_finds_but_the_line() {
        let mut s = Sketch::new();
        SketchOp::AddPolyline {
            points: vec![SVec2::new(0.0, -20.0), SVec2::new(0.0, 20.0)],
            closed: false,
            construction: false,
            label: "Add line",
        }
        .apply(&mut s)
        .unwrap();
        let line = s.curves.keys().next().unwrap();
        SketchOp::AddCircle {
            center: SVec2::new(-10.0, 5.0),
            radius: 2.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        SketchOp::AddCircle {
            center: SVec2::new(-10.0, -5.0),
            radius: 2.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let selection = SketchSelection(vec![SketchEntity::Curve(line)]);
        let axis = mirror_axis(&s, &selection).unwrap();
        assert_eq!(axis, CurveRef::Curve(line));
        // A window (left to right) around everything, 1 px per mm, y up.
        let found = cadrs_sketch::hit::box_select(
            &s,
            SVec2::new(-15.0, -25.0),
            SVec2::new(5.0, 25.0),
            false,
            |p| SVec2::new(p.x, -p.y),
        );
        assert!(found.contains(&SketchEntity::Curve(line)));
        let curves = mirror_targets(&s, axis, &found);
        assert_eq!(curves.len(), 2);
        assert!(!curves.contains(&line));
        SketchOp::Mirror { axis, curves }.apply(&mut s).unwrap();
        let circles: Vec<SVec2> = s
            .curves
            .values()
            .filter_map(|c| match c.kind {
                CurveKind::Circle { center, .. } => Some(s.pos(center)),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 4);
        for c in [SVec2::new(10.0, 5.0), SVec2::new(10.0, -5.0)] {
            assert!(circles.iter().any(|p| p.distance(c) < 1e-6), "{circles:?}");
        }
        // A second box over the circles and their copies mirrors nothing again.
        let again = cadrs_sketch::hit::box_select(
            &s,
            SVec2::new(-15.0, -25.0),
            SVec2::new(15.0, 25.0),
            false,
            |p| SVec2::new(p.x, -p.y),
        );
        assert!(mirror_targets(&s, axis, &again).is_empty());
        // Without a line picked, a box mirrors nothing.
        assert!(mirror_axis(&s, &SketchSelection(vec![])).is_none());
    }

    #[test]
    fn offset_side_and_distance_follow_the_cursor() {
        let mut s = Sketch::new();
        SketchOp::AddArc {
            center: SVec2::new(0.0, 0.0),
            start: SVec2::new(75.0, 0.0),
            end: SVec2::new(-75.0, 0.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let arc = s.curves.keys().next().unwrap();
        let chain = vec![(arc, false)];
        // Inside the (counter-clockwise) arc: its left.
        assert!(side_at(&s, &chain, SVec2::new(0.0, 70.0)));
        assert!(!side_at(&s, &chain, SVec2::new(0.0, 80.0)));
        assert!((distance_to_chain(&s, &chain, SVec2::new(0.0, 35.0)) - 40.0).abs() < 1e-9);
    }
    fn line(s: &mut Sketch, a: (f64, f64), b: (f64, f64)) -> CurveId {
        let a = s.add_point(SVec2::new(a.0, a.1));
        let b = s.add_point(SVec2::new(b.0, b.1));
        s.curves.insert(cadrs_sketch::Curve {
            kind: CurveKind::Line { a, b },
            construction: false,
        })
    }

    fn profile() -> (Sketch, [CurveId; 3]) {
        let mut s = Sketch::new();
        let a = line(&mut s, (0.0, 0.0), (40.0, 0.0));
        let b = line(&mut s, (40.0, 30.0), (40.0, 0.0)); // reversed
        let c = line(&mut s, (80.0, 0.0), (100.0, 0.0));
        (s, [a, b, c])
    }

    #[test]
    fn clicking_adds_removes_and_orders_only_selected_curves() {
        let (mut s, [a, b, c]) = profile();
        let extension = line(&mut s, (40.0, 30.0), (60.0, 30.0));
        let mut off = OffsetTool::default();
        off.pick(&s, &[b], true, SVec2::new(39.0, 15.0)).unwrap();
        let original_left = off.left;
        off.pick(&s, &[a], true, SVec2::new(20.0, 1.0)).unwrap();
        assert_eq!(off.chain.len(), 2);
        assert!(!off.chain.iter().any(|(id, _)| *id == extension));
        let reversed = off.chain.iter().find(|(id, _)| *id == b).unwrap().1;
        assert_eq!(
            off.left ^ reversed,
            original_left,
            "reordering the chain must preserve its physical side"
        );
        let shapes = offset_geometry(&s, &off.chain, 5.0, off.left).unwrap();
        assert_eq!(end_of(&shapes[0]), start_of(&shapes[1]));
        off.pick(&s, &[c], true, SVec2::new(90.0, 1.0)).unwrap();
        assert_eq!(off.groups().count(), 2);
        off.pick(&s, &[a], true, SVec2::ZERO).unwrap();
        assert_eq!(off.groups().flat_map(|(c, _)| c).count(), 2);
        off.pick(&s, &[b, c], true, SVec2::ZERO).unwrap();
        assert!(off.chain.is_empty());
        fn start_of(s: &OffsetShape) -> SVec2 {
            match *s {
                OffsetShape::Line(a, _) => a,
                _ => panic!("line"),
            }
        }
        fn end_of(s: &OffsetShape) -> SVec2 {
            match *s {
                OffsetShape::Line(_, b) => b,
                _ => panic!("line"),
            }
        }
    }

    #[test]
    fn loops_are_ordered_and_branch_selection_is_refused_without_losing_picks() {
        let (mut s, [a, b, _]) = profile();
        let branch = line(&mut s, (40.0, 0.0), (60.0, -10.0));
        let mut off = OffsetTool::default();
        off.pick(&s, &[a, b], false, SVec2::new(20.0, 2.0)).unwrap();
        assert!(off.pick(&s, &[branch], true, SVec2::ZERO).is_err());
        assert_eq!(off.chain.len(), 2);
        let (mut s, [a, b, _]) = profile();
        let c = line(&mut s, (0.0, 30.0), (40.0, 30.0));
        let d = line(&mut s, (0.0, 0.0), (0.0, 30.0));
        let chains = selected_chains(&s, &[c, a, d, b]).unwrap();
        assert_eq!(chains.len(), 1);
        assert_eq!(chains[0].len(), 4);
        assert_eq!(offset_geometry(&s, &chains[0], 2.0, true).unwrap().len(), 4);
    }

    #[test]
    fn disconnected_profiles_share_one_dimension_and_keep_sources_still() {
        let (mut s, ids) = profile();
        let before = s.clone();
        let mut off = OffsetTool::default();
        off.pick(&s, &ids, false, SVec2::new(20.0, 2.0)).unwrap();
        let groups: Vec<_> = off.groups().map(|(c, l)| (c.to_vec(), l)).collect();
        let (op, dim) = offset_operation(&s, &groups, 5.0, None).unwrap();
        op.apply(&mut s).unwrap();
        assert_eq!(s.curves.len(), 6);
        assert_eq!(s.dimensions.len(), 1);
        assert!(cadrs_sketch::solve::conflicts(&s).is_empty());
        SketchOp::SetDimensionValue {
            id: dim,
            value: 8.0,
        }
        .apply(&mut s)
        .unwrap();
        for (p, old) in &before.points {
            assert!(s.pos(p).distance(old.pos) < 1e-6, "a source point moved");
        }
        assert!(cadrs_sketch::solve::conflicts(&s).is_empty());
        for constraint in s.constraints.values() {
            if let cadrs_sketch::ConstraintOf::EqualOffset(
                _,
                _,
                cadrs_sketch::CurveRef::Curve(src),
                cadrs_sketch::CurveRef::Curve(dst),
            ) = *constraint
            {
                let kind = DimensionKind::Offset {
                    source: src,
                    target: dst,
                };
                assert!((cadrs_sketch::dimension::measure(&s, kind).unwrap() - 8.0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn later_picks_reuse_distance_but_offsetting_a_copy_starts_a_new_dimension() {
        let (mut s, [a, _, c]) = profile();
        let (op, dim) = offset_operation(&s, &[(vec![(a, false)], true)], 4.0, None).unwrap();
        op.apply(&mut s).unwrap();
        let (op, same) = offset_operation(&s, &[(vec![(c, false)], true)], 4.0, Some(dim)).unwrap();
        assert_eq!(dim, same);
        op.apply(&mut s).unwrap();
        assert_eq!(s.dimensions.len(), 1);
        let DimensionKind::Offset { target, .. } = s.dimensions[dim].kind else {
            panic!("offset")
        };
        let (op, next) =
            offset_operation(&s, &[(vec![(target, false)], true)], 4.0, Some(dim)).unwrap();
        assert_ne!(next, dim);
        op.apply(&mut s).unwrap();
        assert_eq!(s.dimensions.len(), 2);
        assert!(cadrs_sketch::solve::conflicts(&s).is_empty());
    }

    #[test]
    fn shared_distance_rejects_an_impossible_secondary_circle() {
        let mut s = Sketch::new();
        let a = line(&mut s, (0.0, 20.0), (40.0, 20.0));
        SketchOp::AddCircle {
            center: SVec2::new(80.0, 0.0),
            radius: 6.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().find(|id| *id != a).unwrap();
        let (op, dim) = offset_operation(
            &s,
            &[(vec![(a, false)], true), (vec![(c, false)], true)],
            3.0,
            None,
        )
        .unwrap();
        op.apply(&mut s).unwrap();
        assert!(!offset_value_fails(
            &s,
            dim,
            &SketchOp::SetDimensionValue {
                id: dim,
                value: 2.0
            }
        ));
        assert!(offset_value_fails(
            &s,
            dim,
            &SketchOp::SetDimensionValue {
                id: dim,
                value: 10.0
            }
        ));
        let before = s.clone();
        assert!(
            offset_operation(
                &s,
                &[(vec![(a, false)], true), (vec![(c, false)], true)],
                10.0,
                None
            )
            .is_err()
        );
        assert_eq!(s, before, "invalid preview must leave the sketch unchanged");
    }

    fn pointer_app() -> (App, Entity, [CurveId; 3]) {
        use crate::sketch_tools::{SketchArea, SketchDraw};
        use cadrs_core::{Document, Feature, FeatureId, FeatureKind, document::SketchFeature};
        let (geometry, ids) = profile();
        let mut doc = Document::new("Offset interaction");
        let element = doc.elements[0].id;
        let feature = FeatureId::new();
        doc.elements[0].features_mut().unwrap().push(Feature::new(
            feature,
            "Sketch",
            FeatureKind::Sketch(SketchFeature {
                plane: Some(cadrs_sketch::PlaneRef::Top),
                disable_imprinting: true,
                geometry,
            }),
        ));
        let mut app = App::new();
        app.add_message::<PointerInput>()
            .add_message::<KeyboardInput>()
            .insert_resource(ActiveDocument::new(doc))
            .insert_resource(SketchSession {
                element,
                feature,
                is_new: false,
                mark: 0,
                before: None,
                waiting_for_plane: false,
                plane_extent: Vec2::splat(100.0),
                prompted: false,
                show_final: false,
            })
            .insert_resource(ActiveSketchTool {
                tool: SketchTool::Offset,
                construction: false,
            })
            .insert_resource(SketchScreen {
                active: Some(ScreenMap {
                    plane: cadrs_sketch::PlaneRef::Top,
                    origin: Vec2::splat(200.0),
                    x: Vec2::new(5.0, 0.0),
                    y: Vec2::new(0.0, -5.0),
                    w: Vec2::ZERO,
                }),
                fallback: None,
            })
            .init_resource::<OffsetTool>()
            .init_resource::<SketchSelection>()
            .init_resource::<QuickDimFlow>()
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::diagnostic::FrameCount>()
            .init_resource::<crate::parts::PartCache>()
            .init_resource::<cadrs_ui::Theme>()
            .init_resource::<crate::WorkspaceUnits>()
            .init_resource::<SketchArea>()
            .init_resource::<SketchDraw>()
            .init_resource::<crate::viewport::ViewportRect>()
            .add_systems(Update, (edit_tools_keys, edit_tools_pointer).chain());
        let area = app.world_mut().spawn(ViewportArea).id();
        let mut hover = HoverMap::default();
        hover.entry(PointerId::Mouse).or_default().insert(
            area,
            bevy::picking::backend::HitData::new(area, 0.0, None, None),
        );
        app.insert_resource(hover);
        (app, area, ids)
    }

    fn pointer(app: &mut App, area: Entity, action: PointerAction, xy: (f32, f32)) {
        use bevy::picking::pointer::Location;
        let target = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(area))
            .normalize(None)
            .unwrap();
        let position = Vec2::new(200.0 + xy.0 * 5.0, 200.0 - xy.1 * 5.0);
        app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location { target, position },
            action,
        ));
        app.world_mut()
            .resource_mut::<bevy::diagnostic::FrameCount>()
            .0 += 1;
        app.update();
    }

    fn click(app: &mut App, area: Entity, xy: (f32, f32)) {
        pointer(app, area, PointerAction::Press(PointerButton::Primary), xy);
        pointer(
            app,
            area,
            PointerAction::Release(PointerButton::Primary),
            xy,
        );
    }

    #[test]
    fn individual_clicks_do_not_place_until_blank_space_and_the_edit_undoes_atomically() {
        let (mut app, area, ids) = pointer_app();
        let before = app.world().resource::<ActiveDocument>().doc.clone();
        click(&mut app, area, (20.0, 0.0));
        click(&mut app, area, (40.0, 15.0));
        assert_eq!(app.world().resource::<OffsetTool>().chain.len(), 2);
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            0
        );
        assert!(app.world().resource::<QuickDimFlow>().open.is_none());
        click(&mut app, area, (20.0, 0.0));
        assert_eq!(
            app.world().resource::<OffsetTool>().chain,
            vec![(ids[1], false)]
        );
        click(&mut app, area, (20.0, 0.0));
        click(&mut app, area, (30.0, 8.0));
        assert!(app.world().resource::<OffsetTool>().chain.is_empty());
        assert!(app.world().resource::<QuickDimFlow>().open.is_some());
        let mut doc = app.world_mut().resource_mut::<ActiveDocument>();
        assert_eq!(doc.history.undo_len(), 1);
        let after = doc.doc.clone();
        assert_eq!(
            after.elements[0].features()[0]
                .sketch()
                .unwrap()
                .geometry
                .curves
                .len(),
            5
        );
        doc.undo().unwrap();
        assert_eq!(doc.doc, before);
        doc.redo().unwrap();
        assert_eq!(doc.doc, after);
    }

    #[test]
    fn preselection_drag_chain_and_escape_are_supported() {
        let (mut app, area, ids) = pointer_app();
        app.world_mut().resource_mut::<SketchSelection>().0 =
            vec![SketchEntity::Curve(ids[0]), SketchEntity::Curve(ids[2])];
        app.update();
        assert_eq!(app.world().resource::<OffsetTool>().groups().count(), 2);
        pointer(&mut app, area, PointerAction::Cancel, (30.0, 8.0));
        assert!(app.world().resource::<OffsetTool>().chain.is_empty());
        pointer(
            &mut app,
            area,
            PointerAction::Press(PointerButton::Primary),
            (20.0, 0.0),
        );
        pointer(
            &mut app,
            area,
            PointerAction::Release(PointerButton::Primary),
            (20.0, 2.0),
        );
        assert_eq!(app.world().resource::<OffsetTool>().chain.len(), 2);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: bevy::input::keyboard::Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: area,
        });
        app.world_mut()
            .resource_mut::<bevy::diagnostic::FrameCount>()
            .0 += 1;
        app.update();
        assert!(app.world().resource::<OffsetTool>().chain.is_empty());
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            0
        );
    }

    #[test]
    fn enter_confirms_the_offset_and_consumes_the_sketch_accept_key() {
        let (mut app, area, _) = pointer_app();
        click(&mut app, area, (20.0, 0.0));
        pointer(
            &mut app,
            area,
            PointerAction::Move { delta: Vec2::ZERO },
            (20.0, 8.0),
        );
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: bevy::input::keyboard::Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: area,
        });
        app.world_mut()
            .resource_mut::<bevy::diagnostic::FrameCount>()
            .0 += 1;
        app.update();
        assert!(app.world().resource::<QuickDimFlow>().open.is_some());
        let frame = app.world().resource::<bevy::diagnostic::FrameCount>();
        assert!(app.world().resource::<QuickDimFlow>().consumed(frame));
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            1
        );
    }

    #[test]
    fn preselection_can_confirm_without_moving_the_pointer_and_dialogs_block_enter() {
        let (mut app, area, ids) = pointer_app();
        app.world_mut().resource_mut::<SketchSelection>().0 = vec![SketchEntity::Curve(ids[0])];
        app.update();
        let dialog = app.world_mut().spawn(cadrs_ui::DialogRoot).id();
        let enter = |app: &mut App| {
            app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: bevy::input::keyboard::Key::Enter,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: area,
            });
            app.world_mut()
                .resource_mut::<bevy::diagnostic::FrameCount>()
                .0 += 1;
            app.update();
        };
        enter(&mut app);
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            0
        );
        app.world_mut().despawn(dialog);
        enter(&mut app);
        assert!(app.world().resource::<QuickDimFlow>().open.is_some());
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            1
        );
    }

    #[test]
    fn moving_across_a_line_changes_direction_without_an_arrow_click() {
        let (mut app, area, _) = pointer_app();
        click(&mut app, area, (20.0, 0.0));
        for (y, expected) in [
            (5.0, true),
            (0.1, true),
            (-5.0, false),
            (-0.1, false),
            (8.0, true),
        ] {
            pointer(
                &mut app,
                area,
                PointerAction::Move { delta: Vec2::ZERO },
                (20.0, y),
            );
            assert_eq!(
                app.world().resource::<OffsetTool>().left,
                expected,
                "cursor at y={y}"
            );
        }
        // This lies on the old clickable arrow shaft. It must place the offset now.
        click(&mut app, area, (20.0, 10.0));
        assert_eq!(
            app.world().resource::<ActiveDocument>().history.undo_len(),
            1
        );
        assert!(app.world().resource::<QuickDimFlow>().open.is_some());
    }

    #[test]
    fn final_click_determines_direction_even_without_a_move_event() {
        let (mut app, area, _) = pointer_app();
        click(&mut app, area, (20.0, 0.0));
        pointer(
            &mut app,
            area,
            PointerAction::Move { delta: Vec2::ZERO },
            (20.0, 5.0),
        );
        click(&mut app, area, (20.0, -8.0));
        let sketch = crate::sketch_tools::world_sketch(app.world()).unwrap();
        let DimensionKind::Offset { target, .. } = sketch.dimensions.values().next().unwrap().kind
        else {
            panic!("offset")
        };
        let (a, b) = sketch.curve_ends(target).unwrap();
        assert!((sketch.pos(a).y + 8.0).abs() < 1e-5);
        assert!((sketch.pos(b).y + 8.0).abs() < 1e-5);
    }

    #[test]
    fn nearest_disconnected_profile_controls_distance_and_each_profile_faces_the_cursor() {
        let mut s = Sketch::new();
        let a = line(&mut s, (0.0, 0.0), (40.0, 0.0));
        let b = line(&mut s, (40.0, 20.0), (0.0, 20.0));
        let mut off = OffsetTool::default();
        off.pick(&s, &[a, b], false, SVec2::new(20.0, 1.0)).unwrap();
        let (app, _, _) = pointer_app();
        let map = app.world().resource::<SketchScreen>().active.unwrap();
        update_cursor_preview(&s, &map, &mut off, map.to_screen(SVec2::new(20.0, 23.0)));
        assert!(
            (off.distance - 3.0).abs() < 1e-6,
            "distance must be measured from the upper profile"
        );
        update_cursor_preview(&s, &map, &mut off, map.to_screen(SVec2::new(20.0, 10.0)));
        let groups: Vec<_> = off.groups().collect();
        assert!(groups[0].1, "lower line moves up toward cursor");
        assert!(groups[1].1, "reversed upper line moves down toward cursor");
        let (op, dim) = offset_operation(&s, &[(vec![(a, false)], true)], 4.0, None).unwrap();
        op.apply(&mut s).unwrap();
        off.distance_dimension = Some(dim);
        update_cursor_preview(&s, &map, &mut off, map.to_screen(SVec2::new(20.0, -8.0)));
        assert_eq!(
            off.distance, 4.0,
            "crossing a source must preserve the session's typed distance"
        );
        assert!(!off.left);
    }

    #[test]
    fn closed_loops_follow_containment_regardless_of_winding_or_concavity() {
        let outline = [
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 30.0),
            (20.0, 30.0),
            (20.0, 10.0),
            (10.0, 10.0),
            (10.0, 30.0),
            (0.0, 30.0),
        ];
        let mut s = Sketch::new();
        let ids: Vec<_> = (0..outline.len())
            .map(|i| line(&mut s, outline[i], outline[(i + 1) % outline.len()]))
            .collect();
        let forward = selected_chains(&s, &ids).unwrap().remove(0);
        let reverse: Vec<_> = forward.iter().rev().map(|(c, rev)| (*c, !rev)).collect();
        for p in [SVec2::new(5.0, 20.0), SVec2::new(15.0, 5.0)] {
            assert!(side_at(&s, &forward, p));
            assert!(!side_at(&s, &reverse, p));
        }
        for p in [
            SVec2::new(15.0, 20.0),
            SVec2::new(15.0, 35.0),
            SVec2::new(-5.0, 15.0),
        ] {
            assert!(!side_at(&s, &forward, p));
            assert!(side_at(&s, &reverse, p));
        }
    }

    #[test]
    fn circle_and_face_loop_previews_switch_between_cursor_sides() {
        let mut s = Sketch::new();
        SketchOp::AddCircle {
            center: SVec2::ZERO,
            radius: 6.0,
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let c = s.curves.keys().next().unwrap();
        let mut off = OffsetTool::default();
        off.pick(&s, &[c], false, SVec2::new(0.0, 7.0)).unwrap();
        let (app, _, _) = pointer_app();
        let map = app.world().resource::<SketchScreen>().active.unwrap();
        for face in [false, true] {
            if face {
                off.face = Some(FaceLoop {
                    items: vec![],
                    sketch: s.clone(),
                });
            }
            update_cursor_preview(&s, &map, &mut off, map.to_screen(SVec2::new(0.0, 2.0)));
            assert!(off.left);
            assert!((off.distance - 4.0).abs() < 1e-6);
            update_cursor_preview(&s, &map, &mut off, map.to_screen(SVec2::new(0.0, 8.0)));
            assert!(!off.left);
            assert!((off.distance - 2.0).abs() < 1e-6);
        }
    }
}
