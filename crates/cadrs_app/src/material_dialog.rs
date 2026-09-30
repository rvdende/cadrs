//! The Assign material dialog (PS10.1–10.3, X9), like Onshape's (`training/intro-to-part-studios/
//! ex4-step17.png`): select parts, right-click → **Assign material…**.
//!
//! - **Library** tab: the library dropdown (the bundled [`cadrs_core::material::LIBRARY_NAME`]
//!   and the document's own libraries; its **+** makes a new, empty one, P3.6), the material
//!   dropdown (closed: the chosen
//!   material; open: a search box that filters the list as you type), and the chosen material's
//!   properties: Name, Density,
//!   Poisson's ratio, Young's modulus, the tensile and compressive yield and ultimate strengths,
//!   in the workspace units (kg/m³ and MPa in a metric workspace, lb/in³ and Psi in inches, as
//!   the course's screenshot shows them).
//! - **Custom** tab: your own name and values, stored with the part; with a custom library
//!   chosen, **Add to library** saves the material in it (PS10.3), saved with the document.
//! - ✓ assigns the material to the parts (one undoable step); **Remove material** takes it
//!   off. The Mass properties panel then shows the mass, centre of mass and inertia (PS10.4).
//! - P3B.6: the BOM's **material picker** (double-click a Material cell, A20.10) and the
//!   Properties dialog open it on property owners ([`open_material_for`]): ✓ sets their
//!   Material property ([`cadrs_core::properties::SetProperties`]), wherever the part lives.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use cadrs_core::commands::{SetMaterialLibraries, SetPartMaterial};
use cadrs_core::material::{self, LIBRARY_NAME, Material, MaterialLibrary};
use cadrs_core::{ElementId, PartId};
use cadrs_sketch::units::{LengthUnit, Units};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, NumberField, NumberFieldCommit, NumberFieldState,
    Select, TabStrip, TabStripSelect, TabStripState,
};

use crate::parts::PartCache;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState, WorkspaceUnits};

pub struct MaterialDialogPlugin;

impl Plugin for MaterialDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (material_keys, read_search, sync_material_dialog)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<MaterialSession>();
        })
        .add_observer(on_tab)
        .add_observer(on_item)
        .add_observer(on_choose)
        .add_observer(on_custom_commit)
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_remove)
        .add_observer(on_library)
        .add_observer(on_add_library)
        .add_observer(on_add_to_library);
    }
}

/// The open Assign material dialog.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct MaterialSession {
    pub element: ElementId,
    pub parts: Vec<PartId>,
    /// 0: Library, 1: Custom.
    pub tab: usize,
    /// The library material chosen.
    pub chosen: Option<String>,
    /// The search box's text.
    pub query: String,
    /// The material dropdown is open.
    pub open: bool,
    /// The Custom tab's material.
    pub custom: Material,
    /// Some part has a material now (Remove material is offered).
    pub has_material: bool,
    /// The library shown: 0 the bundled one, `i + 1` the document's `i`-th (PS10.3).
    pub library: usize,
    /// The document's libraries (a copy, updated with every change).
    pub libraries: Vec<MaterialLibrary>,
    /// P3B.6: the parts it sets the Material property of (the BOM's picker), instead of
    /// `parts` of the active Part Studio.
    pub owners: Vec<cadrs_core::properties::PropertyOwner>,
}

/// Opens the dialog for `parts` of the active Part Studio, on the material they have.
pub fn open_material_dialog(world: &mut World, parts: Vec<PartId>) {
    let props = world.resource::<PartCache>().props.clone();
    let now = parts.iter().find_map(|p| cadrs_core::parts::material(*p, &props)).cloned();
    open_with(world, parts, Vec::new(), now);
}

/// Opens the dialog for property owners (the BOM's material picker, P3B.6), on the material the
/// first has.
pub fn open_material_for(world: &mut World, owners: Vec<cadrs_core::properties::PropertyOwner>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let now = owners.iter().find_map(|o| cadrs_core::properties::material(&doc.doc, *o));
    if owners.is_empty() {
        return;
    }
    open_with(world, Vec::new(), owners, now);
}

fn open_with(world: &mut World, parts: Vec<PartId>, owners: Vec<cadrs_core::properties::PropertyOwner>, now: Option<Material>) {
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element().map(|e| e.id)) else {
        return;
    };
    crate::appearance::close_dialogs(world);
    world.remove_resource::<crate::mass_props::MassPanel>();
    let libraries = world.get_resource::<ActiveDocument>().map(|d| d.doc.material_libraries.clone()).unwrap_or_default();
    // A material from one of the document's libraries opens that library.
    let library = now
        .as_ref()
        .and_then(|m| m.library.as_deref())
        .and_then(|l| libraries.iter().position(|x| x.name == l))
        .map_or(0, |i| i + 1);
    let (tab, chosen, custom) = match &now {
        Some(m) if m.library.is_some() => (0, Some(m.name.clone()), Material::custom("Custom material", m.density)),
        Some(m) => (1, None, m.clone()),
        None => (0, None, Material::custom("Custom material", 1000.0)),
    };
    world.insert_resource(MaterialSession {
        element,
        parts,
        tab,
        chosen,
        query: String::new(),
        open: false,
        custom,
        has_material: now.is_some(),
        library,
        libraries,
        owners,
    });
}

/// Closes the dialog without assigning anything.
pub fn close(world: &mut World) {
    world.remove_resource::<MaterialSession>();
}

#[derive(Component)]
struct MaterialDialog;

#[derive(Component)]
struct LibraryBody;

#[derive(Component)]
struct CustomBody;

#[derive(Component)]
struct MaterialList;

/// The closed material dropdown.
#[derive(Component)]
struct MaterialChoose;

#[derive(Component)]
struct ChooseLabel;

/// The open dropdown: the search box and the list.
#[derive(Component)]
struct MaterialPopup;

/// What the closed dropdown shows.
fn choose_label(s: &MaterialSession) -> String {
    s.chosen.clone().unwrap_or_else(|| "Select a material".into())
}

#[derive(Component)]
struct MaterialProps;

/// The search text the list was last scrolled for (choosing a material keeps the scroll).
#[derive(Component, Default)]
struct LastQuery(String);

/// A library material's row.
#[derive(Component, Debug, Clone)]
struct MaterialItem(String);

/// A Custom tab field.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum CustomField {
    Name,
    Density,
    Poisson,
    Youngs,
    TensileYield,
    UltimateTensile,
    CompressiveYield,
    UltimateCompressive,
}

/// "Psi" in an imperial workspace, else "MPa"; and Pa per unit.
fn stress_unit(units: &Units) -> (&'static str, f64) {
    match units.length {
        // "Psi", as the course's `ex4-step17.png` writes it.
        LengthUnit::Inch | LengthUnit::Foot | LengthUnit::Yard => ("Psi", 6_894.757_293_168),
        _ => ("MPa", 1e6),
    }
}

/// A material's property rows in `units`: (label, value).
pub fn property_rows(m: &Material, units: &Units) -> Vec<(String, String)> {
    let (density, du) = units.density(m.density);
    let (su, pa) = stress_unit(units);
    let stress = |v: Option<f64>| v.map(|v| trim(v / pa, 3)).unwrap_or_default();
    vec![
        ("Name".into(), m.name.clone()),
        (format!("Density ({du})"), trim(density, 3)),
        ("Poisson's ratio".into(), m.poisson.map(|v| trim(v, 3)).unwrap_or_default()),
        (format!("Young's modulus ({su})"), stress(m.youngs_modulus)),
        (format!("Tensile yield strength ({su})"), stress(m.tensile_yield)),
        (format!("Ultimate tensile strength ({su})"), stress(m.ultimate_tensile)),
        (format!("Compressive yield strength ({su})"), stress(m.compressive_yield)),
        (format!("Ultimate compressive strength ({su})"), stress(m.ultimate_compressive)),
    ]
}

/// `v` with at most `d` decimals, trailing zeros dropped ("0.033", "213205.474", "0").
fn trim(v: f64, d: usize) -> String {
    let s = format!("{v:.d$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// The material ✓ would assign, if it is complete.
fn chosen_material(s: &MaterialSession) -> Option<Material> {
    match s.tab {
        0 => find(s, s.chosen.as_deref()?),
        _ => (s.custom.density > 0.0 && !s.custom.name.trim().is_empty()).then(|| s.custom.clone()),
    }
}

/// A material of the library shown, by name.
fn find(s: &MaterialSession, name: &str) -> Option<Material> {
    match s.library {
        0 => material::library(name),
        i => s.libraries.get(i - 1)?.material(name),
    }
}

/// The names of the library shown's materials matching the search.
fn search(s: &MaterialSession) -> Vec<String> {
    match s.library {
        0 => material::search(&s.query).iter().map(|m| m.name.to_string()).collect(),
        i => {
            let q = s.query.to_lowercase();
            s.libraries
                .get(i - 1)
                .map(|l| l.materials.iter().filter(|m| m.name.to_lowercase().contains(&q)).map(|m| m.name.clone()).collect())
                .unwrap_or_default()
        }
    }
}

/// The library dropdown chose a library.
fn on_library(ev: On<cadrs_ui::SelectChange>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "material-library") {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<MaterialSession>()
            && s.library != i
        {
            s.library = i;
            s.chosen = None;
        }
    });
}

/// Stores the libraries (one undoable step) and shows the dialog again with them.
fn save_libraries(world: &mut World, libraries: Vec<MaterialLibrary>, label: &str) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    if let Err(e) = doc.execute(&SetMaterialLibraries { libraries: libraries.clone(), label: label.into() }) {
        warn!("material library: {e}");
        return false;
    }
    if let Some(mut s) = world.get_resource_mut::<MaterialSession>() {
        s.libraries = libraries;
    }
    // Rebuilt with the new library list.
    let mut q = world.query_filtered::<Entity, With<MaterialDialog>>();
    let dialogs: Vec<Entity> = q.iter(world).collect();
    for e in dialogs {
        world.entity_mut(e).despawn();
    }
    true
}

/// The library's **+**: a new, empty library, shown.
fn on_add_library(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(a.entity).is_ok_and(|n| n.as_str() == "material-add-library") {
        return;
    }
    commands.queue(|world: &mut World| {
        let Some(s) = world.get_resource::<MaterialSession>().cloned() else { return };
        let mut libraries = s.libraries.clone();
        let name = material::next_library_name(&libraries);
        libraries.push(MaterialLibrary { name: name.clone(), materials: Vec::new() });
        let n = libraries.len();
        if save_libraries(world, libraries, &format!("Create material library {name}"))
            && let Some(mut s) = world.get_resource_mut::<MaterialSession>()
        {
            s.library = n;
            s.chosen = None;
        }
    });
}

/// The Custom tab's **Add to library**: the material saved in the library shown, and chosen.
fn on_add_to_library(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(a.entity).is_ok_and(|n| n.as_str() == "material-add-to-library") {
        return;
    }
    commands.queue(|world: &mut World| {
        let Some(s) = world.get_resource::<MaterialSession>().cloned() else { return };
        if s.library == 0 || s.custom.name.trim().is_empty() || s.custom.density <= 0.0 {
            return;
        }
        let mut libraries = s.libraries.clone();
        let Some(lib) = libraries.get_mut(s.library - 1) else { return };
        let m = Material { library: None, ..s.custom.clone() };
        match lib.materials.iter_mut().find(|x| x.name == m.name) {
            Some(x) => *x = m.clone(),
            None => lib.materials.push(m.clone()),
        }
        let label = format!("Add {} to {}", m.name, lib.name);
        if save_libraries(world, libraries, &label)
            && let Some(mut s) = world.get_resource_mut::<MaterialSession>()
        {
            s.tab = 0;
            s.chosen = Some(m.name);
        }
    });
}

fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "material-tabs") {
        let i = ev.index;
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<MaterialSession>() {
                s.tab = i;
            }
        });
    }
}

/// The dropdown opens (clearing the search and focusing it) or closes.
fn on_choose(a: On<Activate>, q: Query<(), With<MaterialChoose>>, mut commands: Commands) {
    if !q.contains(a.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let Some(mut s) = world.get_resource_mut::<MaterialSession>() else {
            return;
        };
        s.open = !s.open;
        let open = s.open;
        let mut q = world.query::<(Entity, &Name, &mut EditableText)>();
        let mut field = None;
        for (e, n, mut t) in q.iter_mut(world) {
            if n.as_str() == "material-search-field" {
                t.editor_mut().set_text("");
                field = Some(e);
            }
        }
        if let Some(mut s) = world.get_resource_mut::<MaterialSession>() {
            s.query.clear();
        }
        if let (true, Some(e), Some(mut f)) = (open, field, world.get_resource_mut::<bevy::input_focus::InputFocus>()) {
            f.set(e, bevy::input_focus::FocusCause::Pressed);
        }
    });
}

fn on_item(a: On<Activate>, q: Query<&MaterialItem>, mut commands: Commands) {
    if let Ok(item) = q.get(a.entity) {
        let name = item.0.clone();
        commands.queue(move |world: &mut World| {
            if let Some(mut s) = world.get_resource_mut::<MaterialSession>() {
                s.chosen = Some(name);
                s.open = false;
            }
        });
    }
}

fn on_custom_commit(ev: On<NumberFieldCommit>, q: Query<&CustomField>, mut commands: Commands) {
    let Ok(f) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.trim().to_string();
    commands.queue(move |world: &mut World| {
        let units = world.resource::<WorkspaceUnits>().0;
        let Some(mut s) = world.get_resource_mut::<MaterialSession>() else {
            return;
        };
        let (_, pa) = stress_unit(&units);
        let num = text.split_whitespace().next().and_then(|t| t.parse::<f64>().ok());
        let m = &mut s.custom;
        match f {
            CustomField::Name => {
                if !text.is_empty() {
                    m.name = text;
                }
            }
            CustomField::Density => {
                if let Some(v) = num.filter(|v| *v > 0.0) {
                    m.density = units.density_to_si(v);
                }
            }
            CustomField::Poisson => m.poisson = num,
            CustomField::Youngs => m.youngs_modulus = num.map(|v| v * pa),
            CustomField::TensileYield => m.tensile_yield = num.map(|v| v * pa),
            CustomField::UltimateTensile => m.ultimate_tensile = num.map(|v| v * pa),
            CustomField::CompressiveYield => m.compressive_yield = num.map(|v| v * pa),
            CustomField::UltimateCompressive => m.ultimate_compressive = num.map(|v| v * pa),
        }
        s.set_changed();
    });
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<MaterialDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| assign(world, false));
    }
}

fn on_remove(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).is_ok_and(|n| n.as_str() == "material-remove") {
        commands.queue(|world: &mut World| assign(world, true));
    }
}

/// ✓ (or Remove material): one undoable step.
pub fn assign(world: &mut World, remove: bool) {
    let Some(s) = world.get_resource::<MaterialSession>().cloned() else {
        return;
    };
    let material = if remove {
        None
    } else {
        match chosen_material(&s) {
            Some(m) => Some(m),
            None => return,
        }
    };
    world.remove_resource::<MaterialSession>();
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let result = if s.owners.is_empty() {
        doc.execute(&SetPartMaterial { element: s.element, parts: s.parts, material })
    } else {
        use cadrs_core::properties::{PropertyKey, PropertyValue, SetProperties};
        doc.execute(&SetProperties { owners: s.owners, values: vec![(PropertyKey::Material, PropertyValue::Material(material))], label: "Assign material".into() })
    };
    if let Err(e) = result {
        warn!("material: {e}");
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<MaterialDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(close);
    }
}

fn material_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<MaterialSession>>,
    menus: Query<(), With<cadrs_ui::menu::MenuPopup>>,
    mut menu_open: Local<bool>,
    mut commands: Commands,
) {
    // Esc closes an open menu first (a custom colour's), not the dialog: the menu may already
    // be gone this frame, so last frame's state counts too.
    let was_open = std::mem::replace(&mut *menu_open, !menus.is_empty());
    if session.is_none() || was_open || *menu_open {
        keys.clear();
        return;
    }
    for k in keys.read() {
        if k.state == ButtonState::Pressed && k.key_code == KeyCode::Escape {
            commands.queue(|world: &mut World| {
                match world.get_resource_mut::<MaterialSession>() {
                    Some(mut s) if s.open => s.open = false,
                    _ => close(world),
                }
            });
        }
    }
}

/// The search box filters as you type.
fn read_search(q: Query<(&Name, &EditableText)>, session: Option<ResMut<MaterialSession>>) {
    let Some(mut s) = session else {
        return;
    };
    if let Some((_, t)) = q.iter().find(|(n, _)| n.as_str() == "material-search-field") {
        let v = t.value().to_string();
        if s.query != v {
            s.query = v;
        }
    }
}

fn prop_row(p: &mut ChildSpawner, t: &Theme, i: usize, label: String, value: String) {
    p.spawn((
        Name::new(format!("material-prop-{i}")),
        Node {
            height: Val::Px(22.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            border: UiRect::bottom(Val::Px(1.0)),
            column_gap: Val::Px(8.0),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(0xe4, 0xe4, 0xe4)),
    ))
    .with_children(|r| {
        r.spawn(t.text(label, 11.0, FontWeight::NORMAL, Color::srgb_u8(0x6b, 0x6b, 0x6b)));
        r.spawn(t.text(value, 12.0, FontWeight::MEDIUM, t.tool_foreground));
    });
}

fn spawn_list(p: &mut ChildSpawner, t: &Theme, s: &MaterialSession) {
    let hits = search(s);
    if hits.is_empty() {
        let empty = if s.library > 0 && s.query.is_empty() {
            "No materials yet: add one in the Custom tab"
        } else {
            "No materials match"
        };
        p.spawn(t.text(empty, 11.5, FontWeight::NORMAL, t.muted_foreground));
    }
    for name in hits {
        let selected = s.chosen.as_deref() == Some(name.as_str());
        p.spawn((
            ListItem::new(format!("material-item-{}", slug(&name)))
                .label(name.clone())
                .height(ROW)
                .padding_left(8.0)
                .selected(selected)
                .build(t),
            MaterialItem(name),
        ))
        .entry::<Node>()
        .and_modify(|mut n| n.flex_shrink = 0.0);
    }
}

/// A material row's height.
const ROW: f32 = 22.0;

/// The list scrolled so the chosen material shows (two rows above it).
fn list_scroll(s: &MaterialSession) -> ScrollPosition {
    let i = search(s).iter().position(|m| s.chosen.as_deref() == Some(m.as_str())).unwrap_or(0);
    ScrollPosition(Vec2::new(0.0, (i as f32 - 2.0).max(0.0) * ROW))
}

fn spawn_props(p: &mut ChildSpawner, t: &Theme, s: &MaterialSession, units: &Units) {
    match s.chosen.as_deref().and_then(|n| find(s, n)) {
        Some(m) => {
            for (i, (l, v)) in property_rows(&m, units).into_iter().enumerate() {
                prop_row(p, t, i, l, v);
            }
        }
        None => {
            p.spawn(t.text("Choose a material", 11.5, FontWeight::NORMAL, t.muted_foreground));
        }
    }
}

fn custom_text(s: &MaterialSession, f: CustomField, units: &Units) -> String {
    let rows = property_rows(&s.custom, units);
    match f {
        CustomField::Name => rows[0].1.clone(),
        CustomField::Density => rows[1].1.clone(),
        CustomField::Poisson => rows[2].1.clone(),
        CustomField::Youngs => rows[3].1.clone(),
        CustomField::TensileYield => rows[4].1.clone(),
        CustomField::UltimateTensile => rows[5].1.clone(),
        CustomField::CompressiveYield => rows[6].1.clone(),
        CustomField::UltimateCompressive => rows[7].1.clone(),
    }
}

const CUSTOM_FIELDS: [(CustomField, &str); 8] = [
    (CustomField::Name, "material-custom-name"),
    (CustomField::Density, "material-custom-density"),
    (CustomField::Poisson, "material-custom-poisson"),
    (CustomField::Youngs, "material-custom-youngs"),
    (CustomField::TensileYield, "material-custom-tensile-yield"),
    (CustomField::UltimateTensile, "material-custom-ultimate-tensile"),
    (CustomField::CompressiveYield, "material-custom-compressive-yield"),
    (CustomField::UltimateCompressive, "material-custom-ultimate-compressive"),
];

fn material_dialog(t: &Theme, s: &MaterialSession, units: Units) -> impl Bundle {
    let tb = t.clone();
    let s = s.clone();
    let labels: Vec<String> = property_rows(&s.custom, &units).into_iter().map(|(l, _)| l).collect();
    (
        MaterialDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("material-dialog")
            .title("Material")
            .valid(chosen_material(&s).is_some())
            .plain_title()
            .width(310.0)
            .body_padding(UiRect::ZERO)
            .body(move |b| {
                let t = &tb;
                b.spawn(TabStrip::new("material-tabs").compact().tab("Library").tab("Custom").selected(s.tab).build(t));
                b.spawn((
                    Name::new("material-library-body"),
                    LibraryBody,
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        display: if s.tab == 0 { Display::Flex } else { Display::None },
                        ..default()
                    },
                ))
                .with_children(|c| {
                    c.spawn(Node {
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|r| {
                        let mut select = Select::new("material-library").width(Val::Px(250.0)).bordered().option(LIBRARY_NAME, true);
                        for l in &s.libraries {
                            select = select.option(l.name.clone(), true);
                        }
                        r.spawn(select.selected(s.library).build(t));
                        r.spawn(IconButton::new("material-add-library", "plus").tooltip("Create a material library").build(t));
                    });
                    // The material dropdown (closed: the chosen material and a ▾, as
                    // `ex4-step17`); open, a search box and the list over the properties.
                    c.spawn((MaterialChoose, cadrs_ui::ListItem::new("material-choose").build(t)))
                    .insert(Node {
                        height: Val::Px(26.0),
                        padding: UiRect::horizontal(Val::Px(6.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    })
                    .insert(cadrs_ui::Visuals {
                        background: cadrs_ui::StateColors::new(t.background, t.list_hover, t.list_active, t.background),
                        border: cadrs_ui::StateColors::all(Color::srgb_u8(0xc8, 0xc8, 0xc8)),
                        foreground: cadrs_ui::StateColors::all(t.foreground),
                        focus_ring: t.focus_ring,
                    })
                    .with_children(|r| {
                        r.spawn((
                            Name::new("material-choose-label"),
                            ChooseLabel,
                            t.text(choose_label(&s), 12.5, FontWeight::NORMAL, t.foreground),
                            Pickable::IGNORE,
                        ));
                        r.spawn((cadrs_ui::icon("caret-down", 12.0, t.foreground), Pickable::IGNORE));
                    });
                    c.spawn((
                        Name::new("material-popup"),
                        MaterialPopup,
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(72.0),
                            left: Val::Px(8.0),
                            right: Val::Px(8.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(4.0),
                            padding: UiRect::all(Val::Px(4.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            display: if s.open { Display::Flex } else { Display::None },
                            ..default()
                        },
                        BackgroundColor(t.popover),
                        BorderColor::all(t.border),
                        BoxShadow::new(t.shadow, Val::ZERO, Val::Px(2.0), Val::ZERO, Val::Px(8.0)),
                        GlobalZIndex(cadrs_ui::z::MENU),
                    ))
                    .with_children(|pop| {
                        pop.spawn(
                            TextInput::new("material-search")
                                .placeholder("Search materials")
                                .width(Val::Percent(100.0))
                                .height(24.0)
                                .build(t),
                        );
                        pop.spawn((
                            Name::new("material-list"),
                            MaterialList,
                            Node {
                                flex_direction: FlexDirection::Column,
                                max_height: Val::Px(176.0),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            list_scroll(&s),
                        ))
                        .with_children(|l| spawn_list(l, t, &s));
                    });
                    c.spawn((
                        Name::new("material-props"),
                        MaterialProps,
                        Node {
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                    ))
                    .with_children(|p| spawn_props(p, t, &s, &units));
                });
                b.spawn((
                    Name::new("material-custom-body"),
                    CustomBody,
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(2.0),
                        display: if s.tab == 1 { Display::Flex } else { Display::None },
                        ..default()
                    },
                ))
                .with_children(|c| {
                    for (i, (f, name)) in CUSTOM_FIELDS.iter().enumerate() {
                        c.spawn((
                            NumberField::new(*name, labels[i].clone())
                                .label_width(170.0)
                                .text(custom_text(&s, *f, &units))
                                .build(t),
                            *f,
                        ));
                    }
                    // PS10.3: with one of the document's libraries shown, the material can be
                    // saved in it.
                    if let Some(lib) = s.library.checked_sub(1).and_then(|i| s.libraries.get(i)) {
                        c.spawn(Node {
                            justify_content: JustifyContent::FlexEnd,
                            padding: UiRect::top(Val::Px(6.0)),
                            ..default()
                        })
                        .with_child(
                            cadrs_ui::Button::new("material-add-to-library")
                                .label(format!("Add to {}", lib.name))
                                .size(ButtonSize::Small)
                                .build(t),
                        );
                    }
                });
                if s.has_material {
                    b.spawn(Node {
                        justify_content: JustifyContent::FlexEnd,
                        padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::ZERO, Val::Px(8.0)),
                        ..default()
                    })
                    .with_child(
                        cadrs_ui::Button::new("material-remove")
                            .label("Remove material")
                            .size(ButtonSize::Small)
                            .build(t),
                    );
                }
            })
            .build(t),
    )
}

/// Spawns, updates and removes the dialog with the session.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_material_dialog(
    session: Option<Res<MaterialSession>>,
    units: Res<WorkspaceUnits>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_dialog: Query<Entity, With<MaterialDialog>>,
    mut q_state: Query<&mut FeatureDialogState, With<MaterialDialog>>,
    mut q_bodies: Query<(&mut Node, Has<LibraryBody>), (Or<(With<LibraryBody>, With<CustomBody>)>, Without<MaterialPopup>)>,
    mut q_popup: Query<(&mut Node, Has<MaterialPopup>), (With<MaterialPopup>, Without<LibraryBody>, Without<CustomBody>)>,
    mut q_choose_label: Query<&mut Text, With<ChooseLabel>>,
    mut q_tabs: Query<(&Name, &mut TabStripState)>,
    q_list: Query<Entity, With<MaterialList>>,
    q_props: Query<Entity, With<MaterialProps>>,
    mut q_fields: Query<(&CustomField, &mut NumberFieldState)>,
    mut last: Local<Option<(usize, Option<String>, String, Units, usize)>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for e in &q_dialog {
            commands.entity(e).try_despawn();
        }
        *last = None;
        return;
    };
    if q_dialog.is_empty() {
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let d = commands.spawn(material_dialog(&theme, &s, units.0)).id();
        commands.entity(area).add_child(d);
        *last = Some((s.tab, s.chosen.clone(), s.query.clone(), units.0, s.library));
        return;
    }
    if !s.is_changed() && !units.is_changed() {
        return;
    }
    for mut st in &mut q_state {
        let valid = chosen_material(&s).is_some();
        if st.valid != valid {
            st.valid = valid;
        }
    }
    for (mut n, _) in &mut q_popup {
        let want = if s.open { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    for mut t in &mut q_choose_label {
        let want = choose_label(&s);
        if t.0 != want {
            t.0 = want;
        }
    }
    for (mut n, library) in &mut q_bodies {
        let want = if (s.tab == 0) == library { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
    }
    for (n, mut ts) in &mut q_tabs {
        if n.as_str() == "material-tabs" && ts.selected != s.tab {
            ts.selected = s.tab;
        }
    }
    let key = (s.tab, s.chosen.clone(), s.query.clone(), units.0, s.library);
    let changed = last.as_ref() != Some(&key);
    let list_changed = last.as_ref().is_none_or(|l| l.1 != key.1 || l.2 != key.2 || l.4 != key.4);
    let props_changed = last.as_ref().is_none_or(|l| l.1 != key.1 || l.3 != key.3 || l.4 != key.4);
    *last = Some(key);
    if changed {
        let t = theme.clone();
        if list_changed {
            for e in &q_list {
                let (t, s) = (t.clone(), s.clone());
                commands.entity(e).despawn_children();
                commands.queue(move |world: &mut World| {
                    if let Ok(mut x) = world.get_entity_mut(e) {
                        x.with_children(|l| spawn_list(l, &t, &s));
                        if s.query != x.get::<LastQuery>().map(|q| q.0.clone()).unwrap_or_default() {
                            x.insert((list_scroll(&s), LastQuery(s.query.clone())));
                        }
                    }
                });
            }
        }
        if props_changed {
            for e in &q_props {
                let (t, s, u) = (t.clone(), s.clone(), units.0);
                commands.entity(e).despawn_children();
                commands.queue(move |world: &mut World| {
                    if let Ok(mut x) = world.get_entity_mut(e) {
                        x.with_children(|p| spawn_props(p, &t, &s, &u));
                    }
                });
            }
        }
    }
    for (f, mut st) in &mut q_fields {
        let text = custom_text(&s, *f, &units.0);
        if st.text != text {
            st.text = text;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_sketch::units::MassUnit;

    /// The course's Polypropylene in an inch/pound workspace reads as `ex4-step17.png`.
    #[test]
    fn polypropylene_reads_like_the_course() {
        let pp = material::library("Polypropylene").unwrap();
        let inch = Units::new(LengthUnit::Inch, 3).with_mass(MassUnit::Pound);
        let rows = property_rows(&pp, &inch);
        assert_eq!(rows[0], ("Name".into(), "Polypropylene".into()));
        assert_eq!(rows[1], ("Density (lb/in\u{b3})".into(), "0.033".into()));
        assert_eq!(rows[2], ("Poisson's ratio".into(), "0.43".into()));
        assert_eq!(rows[3].0, "Young's modulus (Psi)");
        assert_eq!(rows[3].1, "213205.474");
        assert_eq!(rows[4].1, "4728.23");
        assert_eq!(rows[5].1, "10819.815");
        assert_eq!(rows[6].1, "1450.377");
        assert_eq!(rows[7].1, "0");
        let mm = Units::default();
        assert_eq!(property_rows(&pp, &mm)[1], ("Density (kg/m\u{b3})".into(), "913.437".into()));
        assert_eq!(property_rows(&pp, &mm)[3], ("Young's modulus (MPa)".into(), "1470".into()));
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Aluminum - 6061"), "aluminum-6061");
        assert_eq!(slug("Nylon 6/6"), "nylon-6-6");
    }
}
