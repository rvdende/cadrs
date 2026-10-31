//! Feature-list management (P3.9; intro-to-part-studios.md PS2.4–PS2.5, PS11.2, PS13):
//!
//! - **Rollback bar** (PS13.1): the grey bar under the last built feature. Dragging it (a grey
//!   ghost shows where it goes) or the menu's *Roll to here* / *Roll to end* moves
//!   it ([`SetRollback`], one undo step); the features below it are not built and are greyed.
//! - **Suppress / Unsuppress** (menu): through [`SetSuppressed`]; a suppressed feature is
//!   greyed and struck through, and not built.
//! - **Show dependencies** (PS11.2, menu): the feature's parents (above, amber) and children
//!   (below, green) are highlighted, with a legend under the header; Esc or its ✕ ends it.
//! - **Dialog preview slider** (PS13.2): the slider in every feature dialog's footer switches
//!   the view between the Part Studio *before* the feature (left half) and *after* it (right
//!   half). **Final** (PS13.3) shows only when the edited feature isn't the last one built.

use bevy::prelude::*;
use cadrs_core::commands::{SetRollback, SetSuppressed};
use cadrs_core::{ElementId, FeatureId};
use cadrs_ui::i18n::{Translated, tr};
use cadrs_ui::prelude::*;

use crate::document::{FeatureRow, tab_node_name};
use crate::feature_folders::FolderRow;
use crate::parts::{PartCache, PartOverride};
use crate::viewport::{Pick, Selection};
use crate::{ActiveDocument, AppState};

pub struct FeatureListPlugin;

impl Plugin for FeatureListPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowDependencies>()
            .init_resource::<DialogPreview>()
            .init_resource::<BarDrag>()
            .add_systems(
                Update,
                (
                    sync_dependency_legend,
                    end_dependencies,
                    apply_dialog_preview.before(crate::parts::PartsSet),
                    sync_preview_sliders,
                    sync_final_buttons,
                    place_bar_ghost,
                )
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), reset)
            .add_observer(on_preview_slider)
            .add_observer(on_legend_close)
            .add_observer(on_bar_drag_start)
            .add_observer(on_bar_drag)
            .add_observer(on_bar_drag_end);
    }
}

/// The feature whose dependencies are shown (PS11.2).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct ShowDependencies(pub Option<FeatureId>);

/// The dialog's preview slider is on "before" (PS13.2).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct DialogPreview {
    pub before: bool,
}

/// The rollback bar in the feature list.
#[derive(Component, Debug, Clone, Copy)]
pub struct RollbackBar;

/// A feature dialog's before/after slider.
#[derive(Component, Debug, Clone, Copy)]
pub struct RollbackSlider;

/// A feature dialog's Final button (hidden on the last feature).
#[derive(Component, Debug, Clone, Copy)]
pub struct FinalButton;

/// The legend of Show dependencies.
#[derive(Component, Debug, Clone, Copy)]
struct DependencyLegend;

#[derive(Component, Debug, Clone, Copy)]
struct BarGhost;

/// The rollback bar being dragged: where the pointer is and the index it would go to.
#[derive(Resource, Debug, Default)]
struct BarDrag {
    active: bool,
    pointer: Vec2,
    target: Option<usize>,
}

/// Rows below the rollback bar and suppressed rows: grey text and icons.
pub const ROLLED_BACK_FG: Color = Color::srgb(0.651, 0.651, 0.651);
pub const ROLLED_BACK_ICON: Color = Color::srgb(0.769, 0.769, 0.769);

/// The tint of a rolled-back or suppressed row's icon: the grey for glyph icons; full-colour
/// icons keep their colours, so they fade through the tint's alpha instead.
pub fn rolled_back_icon(full_colour: bool) -> Color {
    if full_colour {
        Color::WHITE.with_alpha(0.4)
    } else {
        ROLLED_BACK_ICON
    }
}
/// Show dependencies: parents (amber) and children (green; blue is the selection).
pub const PARENT_BG: Color = Color::srgb(0.992, 0.906, 0.776);
pub const CHILD_BG: Color = Color::srgb(0.843, 0.941, 0.863);
/// The legend's swatches (P3.11, P3.9 judge: the pale row colours were hard to read at 10 px):
/// the same hues, saturated.
pub const PARENT_SWATCH: Color = Color::srgb(0.910, 0.659, 0.220);
pub const CHILD_SWATCH: Color = Color::srgb(0.337, 0.694, 0.408);

/// What the rows show besides the features themselves (part of their rebuild key).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListState {
    /// The number of features above the rollback bar.
    pub bar: usize,
    /// Suppressed by Suppress or by their variable (IR5.5).
    pub suppressed: Vec<FeatureId>,
    /// IR5.5: the features with a suppression variable, and how the row's tag reads it
    /// ("#withHole").
    pub suppress_vars: Vec<(FeatureId, String)>,
    /// Show dependencies: the feature, its parents and its children.
    pub dependency_of: Option<FeatureId>,
    pub parents: Vec<FeatureId>,
    pub children: Vec<FeatureId>,
}

impl ListState {
    /// True if the feature at `index` isn't built (below the bar, or suppressed).
    pub fn inactive(&self, index: usize, id: FeatureId) -> bool {
        index >= self.bar || self.suppressed.contains(&id)
    }

    /// The row's background at rest for Show dependencies.
    pub fn dependency_background(&self, id: FeatureId) -> Option<Color> {
        if self.parents.contains(&id) {
            Some(PARENT_BG)
        } else if self.children.contains(&id) {
            Some(CHILD_BG)
        } else {
            None
        }
    }
}

/// The list state of a Part Studio.
pub fn list_state(
    el: &cadrs_core::Element,
    cache: &PartCache,
    over: &PartOverride,
    deps: &ShowDependencies,
) -> ListState {
    let features = el.features();
    // While a feature before the end is edited, the bar sits under it (PS13.1), or above it
    // while the dialog's slider is on "before" (PS13.2).
    let index_of = |f: FeatureId| features.iter().position(|x| x.id == f);
    let mut bar = el.rollback_index();
    if let Some(i) = over.rollback_to.and_then(index_of) {
        bar = bar.min(i + 1);
    }
    if let Some(i) = over.before.and_then(index_of) {
        bar = bar.min(i);
    }
    let dependency_of = deps.0.filter(|id| el.feature(*id).is_some());
    let (parents, children) = match dependency_of {
        Some(id) => (
            cadrs_core::feature_list::parents_with(features, &cache.parts, &cache.uses, id),
            cadrs_core::feature_list::children_with(features, &cache.parts, &cache.uses, id),
        ),
        None => (Vec::new(), Vec::new()),
    };
    ListState {
        bar,
        suppressed: el.all_suppressed(),
        suppress_vars: el
            .features()
            .iter()
            .filter_map(|f| Some((f.id, f.suppress_by.as_ref()?.label())))
            .collect(),
        dependency_of,
        parents,
        children,
    }
}

/// The rollback bar row (5 px, grey; `margin_top` above it), with a taller invisible grip for
/// dragging.
pub fn rollback_bar(t: &Theme, at_end: bool) -> impl Bundle {
    (
        Name::new("rollback-bar"),
        RollbackBar,
        Node {
            height: Val::Px(5.0),
            flex_shrink: 0.0,
            margin: if at_end {
                UiRect::new(Val::Px(5.0), Val::Px(9.0), Val::Px(8.0), Val::ZERO)
            } else {
                UiRect::new(Val::Px(5.0), Val::Px(9.0), Val::Px(3.0), Val::Px(3.0))
            },
            ..default()
        },
        BackgroundColor(t.rollback_bar),
        Tooltip::new(tr("view-rollback-bar-tooltip")),
        children![(
            Name::new("rollback-bar-grip"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(-4.0),
                height: Val::Px(13.0),
                ..default()
            },
        )],
    )
}

/// The Show dependencies legend under the features header (hidden until used).
pub fn dependency_legend(p: &mut ChildSpawnerCommands, t: &Theme) {
    let chip = |c: Color| {
        (
            Node {
                width: Val::Px(11.0),
                height: Val::Px(11.0),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(c),
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            Pickable::IGNORE,
        )
    };
    p.spawn((
        Name::new("feature-dependencies-legend"),
        DependencyLegend,
        Node {
            height: Val::Px(22.0),
            flex_shrink: 0.0,
            margin: UiRect::new(Val::Px(6.0), Val::Px(9.0), Val::Px(1.0), Val::Px(2.0)),
            padding: UiRect::horizontal(Val::Px(6.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgb_u8(0xf1, 0xf3, 0xf5)),
    ))
    .with_children(|l| {
        l.spawn(chip(PARENT_SWATCH));
        l.spawn((
            t.text(
                tr("view-dependencies-parents"),
                t.font_xs,
                FontWeight::MEDIUM,
                t.foreground,
            ),
            Translated("view-dependencies-parents"),
            Pickable::IGNORE,
        ));
        l.spawn(chip(CHILD_SWATCH));
        l.spawn((
            t.text(
                tr("view-dependencies-children"),
                t.font_xs,
                FontWeight::MEDIUM,
                t.foreground,
            ),
            Translated("view-dependencies-children"),
            Node {
                flex_grow: 1.0,
                ..default()
            },
            Pickable::IGNORE,
        ));
        l.spawn(
            cadrs_ui::IconButton::new("feature-dependencies-close", "close")
                .small()
                .icon_size(14.0)
                .tooltip(tr("view-dependencies-stop-tooltip"))
                .build(t),
        );
    });
}

/// A footer's before/after slider (PS13.2), on "after" (as Onshape draws it, `ex1-step4.png`).
pub fn preview_slider(t: &Theme, name: &str) -> impl Bundle {
    (
        RollbackSlider,
        cadrs_ui::Slider::new(format!("{name}-rollback-slider"))
            .value(0.7)
            .tooltip(tr("view-preview-slider-tooltip"))
            .build(t),
    )
}

// ---------------------------------------------------------------------------------------------
// Dependencies

fn sync_dependency_legend(
    deps: Res<ShowDependencies>,
    mut q: Query<&mut Node, With<DependencyLegend>>,
) {
    let want = if deps.0.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut n in &mut q {
        if n.display != want {
            n.display = want;
        }
    }
}

fn on_legend_close(
    a: On<bevy::ui_widgets::Activate>,
    q: Query<&Name>,
    mut deps: ResMut<ShowDependencies>,
) {
    if q.get(a.entity)
        .is_ok_and(|n| n.as_str() == "feature-dependencies-close")
    {
        deps.0 = None;
    }
}

/// Esc ends Show dependencies (when nothing else is open).
fn end_dependencies(
    keys: Res<ButtonInput<KeyCode>>,
    mut deps: ResMut<ShowDependencies>,
    doc: Option<Res<ActiveDocument>>,
) {
    if deps.0.is_none() {
        return;
    }
    let gone = doc
        .as_ref()
        .and_then(|d| d.active_element())
        .is_none_or(|el| deps.0.is_some_and(|f| el.feature(f).is_none()));
    if gone || keys.just_pressed(KeyCode::Escape) {
        deps.0 = None;
    }
}

/// The context menu's Show dependencies.
pub fn show_dependencies(world: &mut World, id: FeatureId) {
    world.resource_mut::<ShowDependencies>().0 = Some(id);
}

// ---------------------------------------------------------------------------------------------
// Suppress and roll back

/// The features a row's menu acts on: the selected features if the row is one of them, else
/// the row's feature.
pub fn menu_targets(world: &World, id: FeatureId) -> Vec<FeatureId> {
    let picked: Vec<FeatureId> = world
        .resource::<Selection>()
        .0
        .iter()
        .filter_map(|p| match p {
            Pick::Feature(f) => Some(*f),
            _ => None,
        })
        .collect();
    if picked.contains(&id) {
        picked
    } else {
        vec![id]
    }
}

/// Suppresses (or unsuppresses) the menu's features (one undo step).
pub fn set_suppressed(world: &mut World, id: FeatureId, suppressed: bool) {
    let targets = menu_targets(world, id);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let what = match targets.as_slice() {
        [one] => el.feature(*one).map(|f| f.name.clone()).unwrap_or_default(),
        many => format!("{} features", many.len()),
    };
    let verb = if suppressed { "Suppress" } else { "Unsuppress" };
    let _ = doc.execute(&SetSuppressed {
        element,
        features: targets,
        suppressed,
        label: format!("{verb} {what}"),
    });
}

/// Moves the rollback bar to just below `id` (`None`: to the end).
pub fn roll_to(world: &mut World, id: Option<FeatureId>) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let index = match id {
        Some(f) => match el.features().iter().position(|x| x.id == f) {
            Some(i) => Some(i + 1),
            None => return,
        },
        None => None,
    };
    let index = index.filter(|i| *i < el.features().len());
    if index.unwrap_or(el.features().len()) == el.rollback_index() {
        return;
    }
    let _ = doc.execute(&SetRollback { element, index });
}

fn on_bar_drag_start(
    ev: On<PointerDragStart>,
    q: Query<(), With<RollbackBar>>,
    mut drag: ResMut<BarDrag>,
) {
    if ev.button == PointerButton::Primary && q.contains(ev.entity) {
        *drag = BarDrag {
            active: true,
            pointer: ev.pointer.position,
            target: None,
        };
    }
}

fn on_bar_drag(ev: On<PointerDrag>, q: Query<(), With<RollbackBar>>, mut drag: ResMut<BarDrag>) {
    if drag.active && q.contains(ev.entity) {
        drag.pointer = ev.pointer.position;
    }
}

fn on_bar_drag_end(
    ev: On<PointerDragEnd>,
    q: Query<(), With<RollbackBar>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let drag = std::mem::take(&mut *world.resource_mut::<BarDrag>());
        let Some(to) = drag.target else { return };
        let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
            return;
        };
        let Some(el) = doc.active_element() else {
            return;
        };
        let element = el.id;
        if to == el.rollback_index() {
            return;
        }
        let n = el.features().len();
        let _ = doc.execute(&SetRollback {
            element,
            index: (to < n).then_some(to),
        });
    });
}

/// How near the feature list's top or bottom edge (px) a dragged rollback bar scrolls the list.
const BAR_SCROLL_EDGE: f32 = 32.0;
/// How fast it scrolls (px a second) with the pointer an edge's depth past the edge or more.
const BAR_SCROLL_SPEED: f32 = 900.0;

/// While the bar is dragged: where it would go (the gap between the shown rows nearest the
/// pointer), and a grey ghost bar there. With the pointer near the list's top or bottom, or
/// past it, the list scrolls that way, faster the further in, so a drop place out of sight can
/// be reached (the ghost stays in the list, never over the Parts list below it).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_bar_ghost(
    mut drag: ResMut<BarDrag>,
    doc: Option<Res<ActiveDocument>>,
    q_rows: Query<(
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Option<&FeatureRow>,
        Option<&FolderRow>,
    )>,
    mut q_ghost: Query<(Entity, &mut Node), With<BarGhost>>,
    mut q_scroll: Query<(
        &Name,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        &mut ScrollPosition,
    )>,
    time: Res<Time>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if !drag.active {
        for (e, _) in &mut q_ghost {
            commands.entity(e).try_despawn();
        }
        return;
    }
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else {
        return;
    };
    // The list's shown part (logical px), scrolled toward the pointer near its edges.
    let Some((_, list, list_at, mut pos)) = q_scroll
        .iter_mut()
        .find(|(n, ..)| n.as_str() == "feature-scroll")
    else {
        return;
    };
    let s = list.inverse_scale_factor();
    let half = list.size().y * s / 2.0;
    let (view_top, view_bottom) = (
        list_at.translation.y * s - half,
        list_at.translation.y * s + half,
    );
    let y = drag.pointer.y;
    let depth = if y < view_top + BAR_SCROLL_EDGE {
        y - (view_top + BAR_SCROLL_EDGE)
    } else if y > view_bottom - BAR_SCROLL_EDGE {
        y - (view_bottom - BAR_SCROLL_EDGE)
    } else {
        0.0
    };
    let max = ((list.content_size().y - list.size().y) * s).max(0.0);
    if depth != 0.0 && max > 0.0 {
        let speed = (depth / BAR_SCROLL_EDGE).clamp(-2.0, 2.0) / 2.0 * BAR_SCROLL_SPEED;
        let to = (pos.y + speed * time.delta_secs()).clamp(0.0, max);
        if to != pos.y {
            pos.y = to;
        }
    }
    let order: Vec<FeatureId> = el.features().iter().map(|f| f.id).collect();
    // The rows on screen, top to bottom: (top, bottom, left, index before, index after).
    let mut rows: Vec<(f32, f32, f32, usize, usize)> = Vec::new();
    for (node, t, feature, folder) in &q_rows {
        let (before, after) = match (feature, folder) {
            (Some(f), _) => match order.iter().position(|x| *x == f.0) {
                Some(i) => (i, i + 1),
                None => continue,
            },
            (_, Some(f)) => {
                let Some(folder) = el.folders().iter().find(|x| x.id == f.0) else {
                    continue;
                };
                let idx: Vec<usize> = folder
                    .features
                    .iter()
                    .filter_map(|x| order.iter().position(|o| o == x))
                    .collect();
                let (Some(a), Some(b)) = (idx.iter().min(), idx.iter().max()) else {
                    continue;
                };
                (*a, if folder.open { *a } else { b + 1 })
            }
            _ => continue,
        };
        let size = node.size() * node.inverse_scale_factor();
        if size.y <= 0.0 {
            continue;
        }
        let c = t.translation * node.inverse_scale_factor();
        // Only the rows shown: one scrolled out of sight isn't a place to drop on.
        if c.y + size.y / 2.0 <= view_top || c.y - size.y / 2.0 >= view_bottom {
            continue;
        }
        rows.push((
            c.y - size.y / 2.0,
            c.y + size.y / 2.0,
            c.x - size.x / 2.0,
            before,
            after,
        ));
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    if rows.is_empty() {
        return;
    }
    let k = rows
        .iter()
        .position(|(top, bottom, ..)| y < (top + bottom) / 2.0)
        .unwrap_or(rows.len());
    let (to, line_y, left) = match rows.get(k) {
        Some((top, _, left, before, _)) => (*before, *top, *left),
        None => {
            let last = rows[rows.len() - 1];
            (last.4, last.1, last.2)
        }
    };
    drag.target = Some(to);
    let line_y = line_y.min(view_bottom - 2.0).max(view_top + 3.0);
    let top = Val::Px(line_y - 3.0);
    let lx = Val::Px(left + 3.0);
    match q_ghost.iter_mut().next() {
        Some((_, mut n)) => {
            if n.top != top || n.left != lx {
                n.top = top;
                n.left = lx;
            }
        }
        None => {
            commands.spawn((
                Name::new("rollback-bar-ghost"),
                BarGhost,
                Node {
                    position_type: PositionType::Absolute,
                    top,
                    left: lx,
                    width: Val::Px(172.0),
                    height: Val::Px(5.0),
                    ..default()
                },
                BackgroundColor(theme.rollback_bar.with_alpha(0.75)),
                GlobalZIndex(cadrs_ui::z::DIALOG - 20),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
            ));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Dialogs: the before/after slider and Final

/// The feature whose dialog is open: (element, feature).
#[allow(clippy::type_complexity)]
fn dialog_feature(
    world_like: (
        &Option<Res<crate::extrude::ExtrudeSession>>,
        &Option<Res<crate::applied::AppliedSession>>,
        &Option<Res<crate::boolean::BooleanSession>>,
    ),
) -> Option<(ElementId, FeatureId)> {
    let (e, a, b) = world_like;
    a.as_ref()
        .map(|s| (s.element, s.feature))
        .or(e.as_ref().map(|s| (s.element, s.feature)))
        .or(b.as_ref().map(|s| (s.element, s.feature)))
}

fn on_preview_slider(
    ev: On<cadrs_ui::SliderChange>,
    q: Query<(), With<RollbackSlider>>,
    mut preview: ResMut<DialogPreview>,
) {
    if q.contains(ev.entity) {
        let before = ev.value < 0.5;
        if preview.before != before {
            preview.before = before;
        }
    }
}

/// "Before": the dialog's feature and everything after it are left out of the view.
fn apply_dialog_preview(
    extrude: Option<Res<crate::extrude::ExtrudeSession>>,
    applied: Option<Res<crate::applied::AppliedSession>>,
    boolean: Option<Res<crate::boolean::BooleanSession>>,
    mut preview: ResMut<DialogPreview>,
    mut over: ResMut<PartOverride>,
) {
    let feature = dialog_feature((&extrude, &applied, &boolean)).map(|(_, f)| f);
    if feature.is_none() && preview.before {
        preview.before = false;
    }
    let want = feature.filter(|_| preview.before);
    if over.before != want {
        over.before = want;
    }
}

/// A dialog rebuilt while on "before" keeps its slider there.
fn sync_preview_sliders(
    preview: Res<DialogPreview>,
    mut q: Query<&mut cadrs_ui::SliderState, With<RollbackSlider>>,
) {
    for mut s in &mut q {
        let before = s.value < 0.5;
        if before != preview.before {
            s.value = if preview.before { 0.0 } else { 0.7 };
        }
    }
}

/// Final shows only on a feature that isn't the last one built (PS13.3).
fn sync_final_buttons(
    extrude: Option<Res<crate::extrude::ExtrudeSession>>,
    applied: Option<Res<crate::applied::AppliedSession>>,
    boolean: Option<Res<crate::boolean::BooleanSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut q: Query<&mut Visibility, With<FinalButton>>,
) {
    let last = dialog_feature((&extrude, &applied, &boolean))
        .zip(doc.as_ref())
        .is_none_or(|((element, feature), d)| {
            d.doc.element(element).is_none_or(|el| {
                el.features()[..el.rollback_index()].last().map(|f| f.id) == Some(feature)
            })
        });
    let want = if last {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut v in &mut q {
        v.set_if_neq(want);
    }
}

/// The last feature above the rollback bar, for the dialogs' rollback (Final).
pub fn last_built(el: &cadrs_core::Element) -> Option<FeatureId> {
    el.features()[..el.rollback_index()].last().map(|f| f.id)
}

/// True if a feature can be edited: not below the rollback bar and not suppressed.
pub fn editable(el: &cadrs_core::Element, id: FeatureId) -> bool {
    !el.is_rolled_back(id) && !el.is_suppressed(id)
}

/// A row's name, as the rows name themselves ("feature-extrude-1").
pub fn row_name(name: &str) -> String {
    tab_node_name(name).replacen("tab-", "feature-", 1)
}

fn reset(
    mut deps: ResMut<ShowDependencies>,
    mut preview: ResMut<DialogPreview>,
    mut drag: ResMut<BarDrag>,
) {
    deps.0 = None;
    preview.before = false;
    *drag = BarDrag::default();
}
