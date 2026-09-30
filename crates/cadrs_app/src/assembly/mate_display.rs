//! Mates in the view (P3B.3, `intro-to-assemblies.md` A14, X10: J, H):
//!
//! - Mates are **hidden by default** (A14.1). A shown mate is drawn at its connector as a
//!   connector glyph with a badge of its type's icon (`mate-glyph-<n>`, n its row). **J** shows
//!   every mate, or hides them all when any is shown. The eye on a mate's row, the mate menu's
//!   Show / Hide, and the instance menu's **Show mates / Hide mates** (every mate of the
//!   instance, A14.3) show and hide single mates.
//! - **Show mates mode** (**H**, A14.4): hovering an instance (in the view or the Instances list)
//!   shows its mates; clicking it pins them (click again to unpin). A chip at the top of the view
//!   says the mode is on.
//! - Hovering a mate's row highlights its instances and its glyph (A14.2); clicking a glyph
//!   selects its row (cross-highlight), and right-clicking it opens the mate menu, which has
//!   Edit (A14.5).

use std::collections::HashSet;

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use cadrs_core::assembly::connector::ConnectorFrame;
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind, MateType};
use cadrs_core::assembly::{Assembly, InstanceId, Pose};
use cadrs_ui::prelude::*;

use super::connectors::{ConnectorGizmos, ConnectorHaloGizmos, draw_glyph};
use crate::parts::HoverParts;
use crate::viewport::{Pick, PickRequest, PlaneHighlight, ViewportArea, ViewportRect, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct MateDisplayPlugin;

impl Plugin for MateDisplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MateDisplay>()
            .init_resource::<MateHover>()
            .init_resource::<MateSelection>()
            .add_systems(
                Update,
                (pin_in_show_mode, hover_parts, draw_mates)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .after(super::triad::TriadSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(PostUpdate, (place_badges, show_mode_chip).before(bevy::ui::UiSystems::Layout).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut d: ResMut<MateDisplay>, mut s: ResMut<MateSelection>| {
                *d = MateDisplay::default();
                s.0 = None;
            });
    }
}

/// Which mates the view shows.
#[derive(Resource, Debug, Clone, Default)]
pub struct MateDisplay {
    /// Mates shown (the eye, Show, J, Show mates on an instance).
    pub shown: HashSet<MateId>,
    /// Show mates mode (H).
    pub mode: bool,
    /// Instances whose mates the mode pinned (a click).
    pub pinned: HashSet<InstanceId>,
}

impl MateDisplay {
    /// J: all mates, or none when any is shown.
    pub fn toggle_all(&mut self, model: &Assembly) {
        if self.shown.is_empty() {
            self.shown = model.mates.iter().filter(|f| f.mate().is_some()).map(|f| f.id).collect();
        } else {
            self.shown.clear();
        }
    }

    /// Shows or hides every mate of `instances` (the instance menu's Show / Hide mates).
    pub fn set_for_instances(&mut self, model: &Assembly, instances: &[InstanceId], show: bool) {
        for f in model.mates.iter().filter(|f| f.mate().is_some() && instances.iter().any(|i| f.involves(*i))) {
            if show {
                self.shown.insert(f.id);
            } else {
                self.shown.remove(&f.id);
            }
        }
    }
}

/// The mate row under the pointer (or glyph), for highlighting its instances.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct MateHover(pub Option<MateId>);

/// The mate selected in the list (by its row, or by clicking its glyph).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct MateSelection(pub Option<MateId>);

/// The instance the pointer is over (in the view, or its row in the Instances list).
fn hovered_instance(h: &PlaneHighlight) -> Option<InstanceId> {
    h.viewport.as_ref().and_then(|p| p.part()).map(super::occurrence_of).or_else(|| match h.list {
        Some(Pick::Part(p)) => Some(super::occurrence_of(p)),
        _ => None,
    })
}

/// The mates shown now: shown ones, the hovered row's, and in show mates mode the hovered and
/// pinned instances'.
pub fn visible(model: &Assembly, d: &MateDisplay, h: &PlaneHighlight, hover: &MateHover) -> Vec<MateId> {
    let over = if d.mode { hovered_instance(h) } else { None };
    model
        .mates
        .iter()
        .filter(|f| f.mate().is_some())
        .filter(|f| {
            d.shown.contains(&f.id)
                || hover.0 == Some(f.id)
                || (d.mode && (over.is_some_and(|i| f.involves(i)) || d.pinned.iter().any(|i| f.involves(*i))))
        })
        .map(|f| f.id)
        .collect()
}

/// Where a mate is drawn: its first connector (Width: the middle of its width pair; Tangent: the
/// middle of its two entities).
pub fn mate_frame(model: &Assembly, preview: &std::collections::HashMap<InstanceId, Pose>, f: &MateFeature) -> Option<ConnectorFrame> {
    let MateKind::Mate(m) = &f.kind else { return None };
    let pose = |i: InstanceId| preview.get(&i).copied().or_else(|| model.instance(i).map(|x| x.pose));
    let world = |c: &cadrs_core::assembly::connector::MateConnector| Some(c.adjust(c.frame).moved(&pose(c.instance)?));
    match m.mate_type {
        MateType::Width | MateType::Tangent => {
            let (a, b) = (world(&m.connectors[0])?, world(&m.connectors[1])?);
            let o = [(a.origin[0] + b.origin[0]) / 2.0, (a.origin[1] + b.origin[1]) / 2.0, (a.origin[2] + b.origin[2]) / 2.0];
            Some(ConnectorFrame { origin: o, ..a })
        }
        // The pin of a pin slot, the part moving in a plane: where the motion shows.
        MateType::PinSlot | MateType::Planar | MateType::Parallel => world(&m.connectors[1]),
        _ => world(&m.connectors[0]),
    }
}

/// In show mates mode a click on an instance pins its mates (again: unpins); it only pins: the
/// click doesn't select (no selection, no triad).
fn pin_in_show_mode(
    mut picks: MessageReader<PickRequest>,
    mut d: ResMut<MateDisplay>,
    dialog: Option<Res<super::mate_dialog::MateSession>>,
    mut selection: ResMut<crate::viewport::Selection>,
    mut triad: ResMut<super::triad::Triad>,
) {
    for p in picks.read() {
        if !d.mode || dialog.is_some() {
            continue;
        }
        if let Some(i) = p.0.as_ref().and_then(super::instance_of) {
            if !d.pinned.remove(&i) {
                d.pinned.insert(i);
            }
            selection.0.retain(|q| super::instance_of(q) != Some(i));
            triad.frame = None;
        }
    }
}

/// A hovered mate row (or glyph) highlights the mate's instances (A14.2).
fn hover_parts(doc: Option<Res<ActiveDocument>>, hover: Res<MateHover>, mut parts: ResMut<HoverParts>) {
    let want: Vec<cadrs_core::PartId> = hover
        .0
        .and_then(|id| doc.as_ref()?.active_element()?.assembly_model()?.mate(id).map(|f| f.instances()))
        .unwrap_or_default()
        .into_iter()
        .map(super::occurrence_part)
        .collect();
    if parts.0 != want {
        parts.0 = want;
    }
}

const GLYPH: Color = Color::srgb(0.55, 0.58, 0.62);

#[allow(clippy::too_many_arguments)]
fn draw_mates(
    doc: Option<Res<ActiveDocument>>,
    asm: Res<super::AssemblyParts>,
    d: Res<MateDisplay>,
    h: Res<PlaneHighlight>,
    hover: Res<MateHover>,
    sel: Res<MateSelection>,
    view: Res<ViewportView>,
    mut line: Gizmos<ConnectorGizmos>,
    mut halo: Gizmos<ConnectorHaloGizmos>,
) {
    let Some(doc) = doc else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    // Connectors name parts at any depth (P3B.4).
    let flat = cadrs_core::assembly::structure::solver_model(&doc.doc, model);
    for id in visible(model, &d, &h, &hover) {
        let Some(f) = model.mate(id) else { continue };
        let Some(frame) = mate_frame(&flat, &asm.preview, f) else { continue };
        let ring = if sel.0 == Some(id) {
            crate::parts::SELECTED
        } else if hover.0 == Some(id) {
            crate::parts::HOVER
        } else {
            GLYPH
        };
        draw_glyph(&mut line, &mut halo, &view.view, &frame, ring);
    }
}

/// A mate's badge in the view.
#[derive(Component, Debug, Clone, Copy)]
struct MateBadge(MateId);

const BADGE: f32 = 20.0;

#[allow(clippy::too_many_arguments)]
fn place_badges(
    doc: Option<Res<ActiveDocument>>,
    asm: Res<super::AssemblyParts>,
    d: Res<MateDisplay>,
    h: Res<PlaneHighlight>,
    hover: Res<MateHover>,
    sel: Res<MateSelection>,
    view: Res<ViewportView>,
    rect: Res<ViewportRect>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q: Query<(Entity, &MateBadge, &mut Node, &mut BackgroundColor, &mut BorderColor)>,
    mut commands: Commands,
) {
    let model = doc.as_ref().and_then(|d| d.active_element()?.assembly_model());
    let flat = doc.as_ref().zip(model).map(|(d, m)| cadrs_core::assembly::structure::solver_model(&d.doc, m)).unwrap_or_default();
    let spots: Vec<(MateId, usize, Vec2, &'static str)> = model
        .map(|model| {
            visible(model, &d, &h, &hover)
                .into_iter()
                .filter_map(|id| {
                    let k = model.mates.iter().position(|f| f.id == id)?;
                    let f = &model.mates[k];
                    let frame = mate_frame(&flat, &asm.preview, f)?;
                    let at = rect.to_screen(view.view.project(super::connectors::v3(frame.origin))) - rect.0.min + Vec2::new(10.0, -BADGE - 8.0);
                    Some((id, k + 1, at, super::mates_list::type_icon(f.mate().map(|m| m.mate_type))))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut have: Vec<MateId> = Vec::new();
    for (e, b, mut n, mut bg, mut border) in &mut q {
        match spots.iter().find(|s| s.0 == b.0) {
            Some((id, _, at, _)) => {
                n.left = Val::Px(at.x);
                n.top = Val::Px(at.y);
                let (fill, edge) = if sel.0 == Some(*id) {
                    (theme.list_selected, crate::parts::SELECTED)
                } else if hover.0 == Some(*id) {
                    (theme.list_hover, crate::parts::HOVER)
                } else {
                    (Color::WHITE, Color::srgb_u8(0xa0, 0xa4, 0xa8))
                };
                bg.set_if_neq(BackgroundColor(fill));
                border.set_if_neq(BorderColor::all(edge));
                have.push(*id);
            }
            None => commands.entity(e).try_despawn(),
        }
    }
    let Some(area) = q_area.iter().next() else { return };
    for (id, k, at, icon_name) in spots {
        if have.contains(&id) {
            continue;
        }
        let e = commands
            .spawn((
                Name::new(format!("mate-glyph-{k}")),
                MateBadge(id),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x),
                    top: Val::Px(at.y),
                    width: Val::Px(BADGE),
                    height: Val::Px(BADGE),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(Color::srgb_u8(0xa0, 0xa4, 0xa8)),
                Hovered::default(),
                DespawnOnExit(AppState::Document),
                children![(icon(icon_name, 15.0, Color::srgb_u8(0x50, 0x55, 0x5a)), Pickable::IGNORE)],
            ))
            .observe(on_badge_click)
            .observe(on_badge_over)
            .observe(on_badge_out)
            .id();
        commands.entity(area).add_child(e);
    }
}

fn on_badge_click(click: On<Pointer<Click>>, q: Query<&MateBadge>, mut sel: ResMut<MateSelection>, mut commands: Commands) {
    let Ok(b) = q.get(click.entity) else { return };
    let id = b.0;
    match click.button {
        PointerButton::Primary => sel.0 = Some(id),
        PointerButton::Secondary => {
            sel.0 = Some(id);
            let at = click.pointer_location.position;
            commands.queue(move |world: &mut World| super::mates_list::open_mate_menu(world, at, id));
        }
        _ => {}
    }
}

fn on_badge_over(ev: On<Pointer<Over>>, q: Query<&MateBadge>, mut hover: ResMut<MateHover>) {
    if let Ok(b) = q.get(ev.entity) {
        hover.0 = Some(b.0);
    }
}

fn on_badge_out(ev: On<Pointer<Out>>, q: Query<&MateBadge>, mut hover: ResMut<MateHover>) {
    if let Ok(b) = q.get(ev.entity)
        && hover.0 == Some(b.0)
    {
        hover.0 = None;
    }
}

/// The "Show mates mode" chip at the top of the view.
#[derive(Component)]
struct ModeChip;

fn show_mode_chip(d: Res<MateDisplay>, theme: Res<Theme>, q: Query<Entity, With<ModeChip>>, q_area: Query<Entity, With<ViewportArea>>, mut commands: Commands) {
    let on = d.mode;
    match (on, q.iter().next()) {
        (true, None) => {
            let Some(area) = q_area.iter().next() else { return };
            let e = commands
                .spawn((
                    Name::new("show-mates-mode"),
                    ModeChip,
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(10.0),
                        left: Val::Percent(50.0),
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
                    BorderColor::all(theme.primary),
                    Pickable::IGNORE,
                    DespawnOnExit(AppState::Document),
                    children![
                        (icon("mate-fastened", 14.0, theme.primary), Pickable::IGNORE),
                        (theme.text("Show mates mode (H): hover an instance, click to pin", 11.5, bevy::text::FontWeight::MEDIUM, theme.foreground), Pickable::IGNORE),
                    ],
                ))
                .id();
            commands.entity(area).add_child(e);
        }
        (false, Some(e)) => commands.entity(e).try_despawn(),
        _ => {}
    }
}
