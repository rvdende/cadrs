//! The relation dialog (P3B.9, `intro-to-assemblies.md` A1.3, A1.4, X16), opened by the
//! toolbar's **Gear**, **Rack and pinion**, **Screw** and **Linear** relation buttons (or the
//! Relations button's menu) and by Edit… on a relation's row, as Onshape's:
//!
//! - The title ("Gear 1"), ✓ and ✕; the **relation type** dropdown.
//! - The **Mates** field: click mates in the Mate Features list (or their glyphs in the view) to
//!   fill it, in order (Gear: the driver first; Rack and pinion: the pinion's rotating mate, then
//!   the rack's sliding one; Screw: one Cylindrical mate); each row has its ✕. A mate that doesn't
//!   fit the type is refused with a red message.
//! - **Ratio** (Gear: the two numbers `a : b`, the first mate turns `a` while the second turns
//!   `b`; Linear: one number), **Distance per revolution** (Rack and pinion) or **Pitch** (Screw),
//!   with units and expressions; **Reverse direction**.
//! - ✓ adds the relation to the Mate Features list (one undo step); the assembly is solved with
//!   it, so dragging, Animate, Reset and Apply limit position turn and move the related mates
//!   together ([`cadrs_core::assembly::relation`]).
//!
//! Names: `relation-dialog`, `relation-type`, `relation-mates`, `relation-ratio-a`,
//! `relation-ratio-b`, `relation-ratio`, `relation-distance`, `relation-reverse` (its checkbox
//! `relation-reverse-checkbox`), `relation-message`.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::commands::{AddMateFeature, SetMateFeature};
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind, next_name};
use cadrs_core::assembly::relation::{self, Relation, RelationType};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit, NumberFieldState,
    OptionRow, Select, SelectChange, SelectState, SelectionList, SelectionListRemove, SelectionListState,
};

use super::mate_display::MateSelection;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct RelationDialogPlugin;

impl Plugin for RelationDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (follow_mate_selection, sync_relation_dialog).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<RelationSession>())
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_remove)
        .add_observer(on_select)
        .add_observer(on_number)
        .add_observer(on_checkbox);
    }
}

/// The Relations button's menu: the four relation types. The toolbar clips its children, so the
/// menu opens at the button's lower left corner on its own anchor, which gets the chosen item.
pub fn open_relations_menu(world: &mut World, button: Entity) {
    let theme = world.resource::<Theme>().clone();
    let at = world
        .get_entity(button)
        .ok()
        .and_then(|e| Some((*e.get::<ComputedNode>()?, *e.get::<bevy::ui::UiGlobalTransform>()?)))
        .map_or(Vec2::ZERO, |(n, t)| {
            let s = n.inverse_scale_factor();
            let size = n.size() * s;
            t.translation * s + Vec2::new(-size.x / 2.0, size.y / 2.0 + 2.0)
        });
    let mut menu = cadrs_ui::menu::Menu::new("relations-menu").min_width(190.0).item_height(24.0);
    for t in RelationType::ALL {
        menu = menu.item(cadrs_ui::menu::MenuItem::new(format!("relations-menu-{}", t.icon()), format!("{} relation", t.label())).icon(t.icon()));
    }
    let mut commands = world.commands();
    let anchor = cadrs_ui::menu::open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert(DespawnOnExit(AppState::Document)).observe(|ev: On<cadrs_ui::menu::MenuAction>, mut commands: Commands| {
        let Some(t) = ev.item.strip_prefix("relations-menu-").and_then(toolbar_type) else { return };
        commands.queue(move |world: &mut World| open(world, t));
    });
    world.flush();
}

/// The open relation dialog.
#[derive(Resource, Debug, Clone)]
pub struct RelationSession {
    pub element: ElementId,
    pub id: MateId,
    pub name: String,
    pub editing: bool,
    pub suppressed: bool,
    pub relation: Relation,
    /// A pick that was refused (the message says why).
    pub refused: Option<String>,
}

impl RelationSession {
    fn feature(&self) -> MateFeature {
        MateFeature { suppressed: self.suppressed, ..MateFeature::new(self.id, self.name.clone(), MateKind::Relation(self.relation.clone())) }
    }
}

#[derive(Component)]
struct RelationDialog;

/// What a field of the dialog is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Type,
    Mates,
    RatioA,
    RatioB,
    Ratio,
    Distance,
    Message,
}

/// The dialog's layout: rebuilt when the type changes.
#[derive(Component, Debug, Clone, PartialEq)]
struct Layout(RelationType, bool);

/// The toolbar's relation buttons.
pub fn toolbar_type(name: &str) -> Option<RelationType> {
    RelationType::ALL.into_iter().find(|t| t.icon() == name)
}

/// Opens a new relation dialog of type `t` (a toolbar button). A mate selected in the list is
/// taken as the first mate.
pub fn open(world: &mut World, t: RelationType) {
    world.remove_resource::<super::mate_dialog::MateSession>();
    world.remove_resource::<super::group_dialog::GroupSession>();
    super::animate::stop(world);
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let name = next_name(&model.mates, t.label());
    let mut relation = Relation::new(t, Vec::new());
    if let Some(m) = world.resource::<MateSelection>().0
        && model.mate(m).and_then(|f| f.mate()).is_some_and(|x| t.accepts(0, x.mate_type))
    {
        relation.mates.push(m);
    }
    world.insert_resource(RelationSession { element, id: MateId::new(), name, editing: false, suppressed: false, relation, refused: None });
}

/// Opens it on an existing relation (Edit…, double-click).
pub fn edit(world: &mut World, id: MateId) {
    world.remove_resource::<super::mate_dialog::MateSession>();
    world.remove_resource::<super::group_dialog::GroupSession>();
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(f) = doc.active_element().and_then(|e| e.assembly_model()?.mate(id).cloned()) else { return };
    let MateKind::Relation(relation) = f.kind else { return };
    world.resource_mut::<MateSelection>().0 = None;
    world.insert_resource(RelationSession { element, id, name: f.name, editing: true, suppressed: f.suppressed, relation, refused: None });
}

/// A mate clicked in the list or the view goes into the Mates field.
fn follow_mate_selection(mut selection: ResMut<MateSelection>, session: Option<ResMut<RelationSession>>, doc: Option<Res<ActiveDocument>>) {
    let Some(mut s) = session else { return };
    if !selection.is_changed() {
        return;
    }
    let Some(picked) = selection.0 else { return };
    // The pick is taken: the row doesn't stay selected behind the dialog.
    selection.0 = None;
    let Some(model) = doc.as_ref().and_then(|d| d.active_element()?.assembly_model()) else { return };
    let Some(f) = model.mate(picked) else { return };
    if f.id == s.id || s.relation.mates.contains(&picked) {
        return;
    }
    let t = s.relation.relation_type;
    let k = s.relation.mates.len();
    if k >= t.mate_count() {
        // Full: the new pick replaces the last one.
        s.relation.mates.pop();
    }
    let k = s.relation.mates.len();
    match f.mate() {
        Some(m) if t.accepts(k, m.mate_type) => {
            s.relation.mates.push(picked);
            s.refused = None;
        }
        Some(m) => {
            // Rack and pinion takes the rack first too: the pinion goes first.
            if t == RelationType::RackPinion && k == 0 && t.accepts(1, m.mate_type) && !t.accepts(0, m.mate_type) {
                s.relation.mates.push(picked);
                s.refused = None;
                return;
            }
            s.refused = Some(format!("{} is a {} mate", f.name, m.mate_type.label()));
        }
        None => s.refused = Some(format!("{} is not a mate", f.name)),
    }
}

/// Rack and pinion given the rack first: swapped so the pinion comes first.
fn ordered(mut r: Relation, model: &cadrs_core::assembly::Assembly) -> Relation {
    if r.relation_type == RelationType::RackPinion && r.mates.len() == 2 {
        let t = |id: MateId| model.mate(id).and_then(|f| f.mate()).map(|m| m.mate_type);
        if let (Some(a), Some(b)) = (t(r.mates[0]), t(r.mates[1]))
            && !RelationType::RackPinion.accepts(0, a)
            && RelationType::RackPinion.accepts(0, b)
        {
            r.mates.swap(0, 1);
        }
    }
    r
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<&Role>, mut commands: Commands) {
    if q.get(ev.entity) != Ok(&Role::Mates) {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<RelationSession>()
            && i < s.relation.mates.len()
        {
            s.relation.mates.remove(i);
        }
    });
}

fn on_select(ev: On<SelectChange>, q: Query<&Role>, mut commands: Commands) {
    if q.get(ev.entity) != Ok(&Role::Type) {
        return;
    }
    let Some(t) = RelationType::ALL.get(ev.index).copied() else { return };
    commands.queue(move |world: &mut World| {
        let names = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().map(|m| m.mates.clone())).unwrap_or_default();
        let Some(mut s) = world.get_resource_mut::<RelationSession>() else { return };
        if s.relation.relation_type == t {
            return;
        }
        s.relation.relation_type = t;
        s.relation.mates.truncate(t.mate_count());
        s.refused = None;
        if !s.editing {
            s.name = next_name(&names, t.label());
        }
    });
}

fn on_number(ev: On<NumberFieldCommit>, q: Query<&Role>, mut q_state: Query<&mut NumberFieldState>, units: Res<crate::WorkspaceUnits>, mut commands: Commands) {
    let Ok(role) = q.get(ev.entity).copied() else { return };
    let text = ev.text.trim().to_string();
    let q = if role == Role::Distance { Quantity::Length } else { Quantity::Count };
    let v = match units.0.eval(&text, q) {
        Ok(v) if v.is_finite() && (role == Role::Distance || v.abs() > 1e-12) => v,
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
    let enter = ev.enter;
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource_mut::<RelationSession>() else { return };
        match role {
            Role::RatioA => s.relation.ratio.0 = v,
            Role::RatioB => s.relation.ratio.1 = v,
            Role::Ratio => s.relation.ratio = (1.0, v),
            Role::Distance => s.relation.distance = v,
            _ => {}
        }
        if enter {
            world.resource_mut::<InputFocus>().clear();
        }
    });
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("relation-reverse-checkbox") {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<RelationSession>() {
            s.relation.reverse = on;
        }
    });
}

/// ✓: the relation is added (or replaced) and the assembly solved with it, one undo step.
pub fn accept(world: &mut World) {
    let Some(mut s) = world.get_resource::<RelationSession>().cloned() else { return };
    let Some((mut model, solids)) = super::mate_dialog::model_and_solids(world) else { return };
    let doc_model = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()).unwrap_or_default();
    s.relation = ordered(s.relation.clone(), &doc_model);
    if relation::check(&s.relation, &doc_model.mates).is_some() {
        return;
    }
    let feature = s.feature();
    // The flattened model keeps the top level's mate ids: solve with the relation in it.
    model.mates.retain(|f| f.id != s.id);
    model.mates.push(feature.clone());
    let sol = cadrs_core::assembly::solve(&model, &solids, &Default::default());
    let poses = if sol.converged { sol.changed(&model) } else { Vec::new() };
    let element = s.element;
    let ok = if s.editing {
        super::run(world, &SetMateFeature { element, feature, poses })
    } else {
        super::run(world, &AddMateFeature { element, feature, poses })
    };
    if ok {
        world.remove_resource::<RelationSession>();
        world.resource_mut::<MateSelection>().0 = None;
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<RelationDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<RelationDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<RelationSession>();
            world.resource_mut::<MateSelection>().0 = None;
        });
    }
}

/// The texts of the value fields: ratio a, ratio b, the single ratio, the distance.
fn texts(r: &Relation, units: &cadrs_sketch::units::Units) -> [String; 4] {
    let num = |v: f64| {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    [num(r.ratio.0), num(r.ratio.1), num(r.ratio.1 / r.ratio.0), units.with_unit(r.distance, Quantity::Length)]
}

fn relation_dialog(theme: &Theme, s: &RelationSession, items: Vec<String>, tx: [String; 4], message: Option<String>, valid: bool) -> impl Bundle {
    let tb = theme.clone();
    let t = s.relation.relation_type;
    let reverse = s.relation.reverse;
    let placeholder = if t.mate_count() == 1 { "Cylindrical mate" } else { "Mates" };
    let active = items.len() < t.mate_count();
    (
        RelationDialog,
        Layout(t, message.is_some()),
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("relation-dialog")
            .title(s.name.clone())
            .valid(valid)
            .width(262.0)
            .body(move |b| {
                let t0 = &tb;
                let mut select = Select::new("relation-type");
                for rt in RelationType::ALL {
                    select = select.option(rt.label(), true);
                }
                let i = RelationType::ALL.iter().position(|x| *x == t).unwrap_or(0);
                b.spawn(Node { height: Val::Px(28.0), align_items: AlignItems::Center, ..default() }).with_child((Role::Type, select.selected(i).build(t0)));
                b.spawn((
                    Role::Mates,
                    SelectionList::new("relation-mates").placeholder(placeholder).item_icon(t.icon(), "Mate").tint_filled().items(items).active(active).error(message.is_some()).build(t0),
                ))
                .entry::<Node>()
                .and_modify(|mut n| {
                    n.flex_grow = 0.0;
                    n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(4.0));
                });
                if let Some(m) = &message {
                    b.spawn((Role::Message, Name::new("relation-message"), t0.text(m.clone(), t0.font_sm, FontWeight::NORMAL, Color::srgb(0.80, 0.16, 0.14))));
                }
                let field = |b: &mut ChildSpawner, role: Role, name: &str, label: &str, text: &str, width: f32| {
                    b.spawn((role, NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(width).build(t0)));
                };
                match t {
                    RelationType::Gear => {
                        // "Ratio  2 : 1".
                        b.spawn((Name::new("relation-ratio"), Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() })).with_children(|r| {
                            let col = |grow: f32| Node { flex_grow: grow, flex_basis: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() };
                            r.spawn(col(1.0)).with_children(|c| field(c, Role::RatioA, "relation-ratio-a", "Ratio", &tx[0], 40.0));
                            r.spawn(t0.text(":", t0.font_base, FontWeight::BOLD, t0.foreground));
                            r.spawn(col(0.55)).with_children(|c| field(c, Role::RatioB, "relation-ratio-b", "", &tx[1], 0.0));
                        });
                    }
                    RelationType::Linear => field(b, Role::Ratio, "relation-ratio", "Ratio", &tx[2], 84.0),
                    RelationType::RackPinion => field(b, Role::Distance, "relation-distance", "Distance per revolution", &tx[3], 128.0),
                    RelationType::Screw => field(b, Role::Distance, "relation-distance", "Pitch", &tx[3], 84.0),
                }
                b.spawn(OptionRow::new("relation-reverse", "Reverse direction").checked(reverse).build(t0));
            })
            .footer(|f| {
                f.spawn(Node { flex_grow: 1.0, ..default() });
                f.spawn((Name::new("relation-help"), icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
            })
            .build(theme),
    )
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_relation_dialog(
    session: Option<Res<RelationSession>>,
    doc: Option<Res<ActiveDocument>>,
    units: Res<crate::WorkspaceUnits>,
    theme: Res<Theme>,
    mut q_dialog: Query<(Entity, &Layout, &mut FeatureDialogState), With<RelationDialog>>,
    mut q_list: Query<(&Role, &mut SelectionListState)>,
    mut q_select: Query<(&Role, &mut SelectState)>,
    mut q_num: Query<(&Role, &mut NumberFieldState)>,
    mut q_text: Query<(&Role, &mut Text)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(doc) = doc else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    let items: Vec<String> = s.relation.mates.iter().map(|m| model.mate(*m).map(|f| f.name.clone()).unwrap_or_else(|| "Mate".into())).collect();
    let r = ordered(s.relation.clone(), model);
    let why = relation::check(&r, &model.mates);
    let valid = why.is_none();
    // Only a refused pick, or a wrong mate in a full field, is shown as an error; an empty
    // field is just incomplete.
    let message = s.refused.clone().or_else(|| why.filter(|_| s.relation.mates.len() == s.relation.relation_type.mate_count()));
    let tx = texts(&s.relation, &units.0);
    let layout = Layout(s.relation.relation_type, message.is_some());
    match q_dialog.iter_mut().next() {
        Some((_, l, mut st)) if *l == layout => {
            if st.valid != valid || st.title != s.name {
                st.valid = valid;
                st.title = s.name.clone();
            }
        }
        other => {
            if let Some((e, ..)) = other {
                commands.entity(e).try_despawn();
            }
            let Some(area) = q_area.iter().next() else { return };
            let d = commands.spawn(relation_dialog(&theme, &s, items, tx, message, valid)).id();
            commands.entity(area).add_child(d);
            return;
        }
    }
    let active = items.len() < s.relation.relation_type.mate_count();
    for (role, mut st) in &mut q_list {
        if *role == Role::Mates && (st.items != items || st.active != active) {
            st.items = items.clone();
            st.active = active;
        }
    }
    for (role, mut st) in &mut q_select {
        let want = RelationType::ALL.iter().position(|x| *x == s.relation.relation_type).unwrap_or(0);
        if *role == Role::Type && st.selected != want {
            st.selected = want;
        }
    }
    for (role, mut st) in &mut q_num {
        let want = match role {
            Role::RatioA => &tx[0],
            Role::RatioB => &tx[1],
            Role::Ratio => &tx[2],
            Role::Distance => &tx[3],
            _ => continue,
        };
        if !st.error && st.text != *want {
            st.text = want.clone();
        }
    }
    if let Some(m) = message {
        for (role, mut text) in &mut q_text {
            if *role == Role::Message && text.0 != m {
                text.0 = m.clone();
            }
        }
    }
}
