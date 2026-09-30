//! The **Properties** dialog of a part, a standard content part or an assembly (P3B.6,
//! `intro-to-assemblies.md` A20.5, A20.10, `test-drive.md` TD9.3, TD9.5): opened from the
//! Parts list, the instance menu (Properties…), a BOM row's menu and the assembly toolbar's
//! Assembly properties.
//!
//! It shows and edits the owner's [`cadrs_core::properties`]: Name, Part number (with a
//! **Generate** button that takes the document's next sequential number), Description,
//! Revision, Vendor, Unit of measure, Category, Material (the library's), the Appearance
//! swatch, the Mass with its **Override**, an assembly's **Subassembly BOM behavior** (A20.5)
//! and the document's custom properties, with **Add property** (a name and a kind: Text,
//! Number, Boolean) under them. **Save** writes every changed value as one undo step
//! ([`SetProperties`]); the BOM shows them at once (two-way, A20.10).
//!
//! Names: the dialog `properties-dialog`; fields `props-name`, `props-part-number`,
//! `props-description`, `props-revision`, `props-vendor`, `props-unit`, `props-category`,
//! `props-material`, `props-mass-override`, `props-mass`, `props-bom-behavior`,
//! `props-custom-<id>`, `props-new-name`, `props-new-kind`, `props-new-add`,
//! `props-generate-part-number`; buttons `props-save`, `props-cancel`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::properties::{
    self, AddPropertyDefinition, CATEGORIES, GenerateMissingPartNumbers, PropertyKey, PropertyKind, PropertyOwner, PropertyValue, SetProperties, SubassemblyBom,
    UNITS_OF_MEASURE,
};
use cadrs_core::material;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Checkbox, CheckboxState, DialogClose, Select, SelectState, form_row};

use crate::{ActiveDocument, AppState};

pub struct PropertiesDialogPlugin;

impl Plugin for PropertiesDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<PropertiesSession>();
        })
        .add_observer(on_button);
    }
}

/// The open Properties dialog: whose, and what was typed before it was rebuilt (a property
/// added).
#[derive(Resource, Debug, Clone)]
pub struct PropertiesSession {
    pub owner: PropertyOwner,
    draft: HashMap<String, String>,
}

#[derive(Component)]
struct PropsDialog;

const LABEL_W: f32 = 180.0;
/// Every value field's width (text fields and selects alike), so their edges line up; a
/// field's extra button (Generate) sits after it.
const FIELD_W: f32 = 290.0;
/// The Add property row's Add button.
const ADD_W: f32 = 48.0;

/// The text fields: (key, field name, label).
const TEXT_FIELDS: [(PropertyKey, &str, &str); 5] = [
    (PropertyKey::Name, "props-name", "Name"),
    (PropertyKey::PartNumber, "props-part-number", "Part number"),
    (PropertyKey::Description, "props-description", "Description"),
    (PropertyKey::Revision, "props-revision", "Revision"),
    (PropertyKey::Vendor, "props-vendor", "Vendor"),
];

/// The materials offered: none, the bundled library, the document's libraries.
fn materials(doc: &cadrs_core::Document) -> Vec<material::Material> {
    let mut out: Vec<material::Material> = material::LIBRARY.iter().map(|m| m.material()).collect();
    for l in &doc.material_libraries {
        for m in &l.materials {
            if let Some(x) = l.material(&m.name) {
                out.push(x);
            }
        }
    }
    out
}

/// A part's default name comes from its studio's rebuild.
fn build_of(world: &mut World, element: cadrs_core::ElementId) -> Option<std::sync::Arc<cadrs_core::rebuild::Build>> {
    let doc = world.get_resource::<ActiveDocument>()?.doc.clone();
    world.resource_mut::<crate::assembly::AssemblyParts>().build(&doc, element)
}

/// Opens the dialog for `owner`.
pub fn open_properties_dialog(world: &mut World, owner: PropertyOwner) {
    world.insert_resource(PropertiesSession { owner, draft: HashMap::new() });
    spawn(world);
}

/// Opens it for the owner of an instance of the active assembly.
pub fn open_for_instance(world: &mut World, instance: cadrs_core::assembly::InstanceId) {
    let Some(source) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model()?.instance(instance).map(|i| i.source)) else {
        return;
    };
    open_properties_dialog(world, source.into());
}

fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<PropsDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
    world.remove_resource::<PropertiesSession>();
}

/// The text typed in a field (`<name>-field`).
fn field_texts(world: &mut World) -> HashMap<String, String> {
    let mut q = world.query::<(&Name, &EditableText)>();
    q.iter(world)
        .filter_map(|(n, t)| n.as_str().strip_prefix("props-").and_then(|x| x.strip_suffix("-field")).map(|x| (format!("props-{x}"), t.value().to_string())))
        .collect()
}

fn selects(world: &mut World) -> HashMap<String, usize> {
    let mut q = world.query::<(&Name, &SelectState)>();
    q.iter(world).filter(|(n, _)| n.as_str().starts_with("props-")).map(|(n, s)| (n.to_string(), s.selected)).collect()
}

fn checks(world: &mut World) -> HashMap<String, bool> {
    let mut q = world.query::<(&Name, &CheckboxState)>();
    q.iter(world).filter(|(n, _)| n.as_str().starts_with("props-")).map(|(n, s)| (n.to_string(), s.checked)).collect()
}

/// Everything the dialog shows now, as text by field name (the draft wins over the document).
fn values(world: &mut World, owner: PropertyOwner, draft: &HashMap<String, String>) -> HashMap<String, String> {
    let build = build_of(world, owner.element());
    let doc = &world.resource::<ActiveDocument>().doc;
    let mut v: HashMap<String, String> = HashMap::new();
    for (k, name, _) in TEXT_FIELDS {
        v.insert(name.into(), properties::text(doc, owner, k, build.as_deref()));
    }
    v.insert("props-unit".into(), properties::text(doc, owner, PropertyKey::UnitOfMeasure, None));
    v.insert("props-category".into(), properties::text(doc, owner, PropertyKey::Category, None));
    v.insert("props-material".into(), properties::text(doc, owner, PropertyKey::Material, None));
    v.insert("props-bom-behavior".into(), properties::text(doc, owner, PropertyKey::BomBehavior, None));
    for d in &doc.properties.definitions {
        v.insert(format!("props-custom-{}", d.id.0), properties::text(doc, owner, PropertyKey::Custom(d.id), None));
    }
    for (k, x) in draft {
        v.insert(k.clone(), x.clone());
    }
    v
}

fn spawn(world: &mut World) {
    let Some(session) = world.get_resource::<PropertiesSession>().cloned() else { return };
    let owner = session.owner;
    // Rebuilt (a property added): the old one goes first.
    let mut q = world.query_filtered::<Entity, With<PropsDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.entity_mut(e).despawn();
    }
    let v = values(world, owner, &session.draft);
    let build = build_of(world, owner.element());
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let units = doc.units;
    let mass_now = properties::mass(&doc, owner, &mut |e| build_of(world, e));
    let own = properties::properties(&doc, owner);
    let appearance = properties::appearance(&doc, owner, build.as_deref());
    let is_asm = owner.is_assembly();
    let is_std = matches!(owner, PropertyOwner::Part { element, .. } if doc.standard_part(element).is_some());
    let mats = materials(&doc);
    let defs = doc.properties.definitions.clone();
    let title = if is_asm { "Assembly properties" } else { "Part properties" };
    let override_on = session.draft.get("props-mass-override").map(|x| x == "true").unwrap_or(own.mass_override.is_some());
    let mass_text = session.draft.get("props-mass").cloned().unwrap_or_else(|| mass_now.map(|m| format!("{:.*}", units.decimals as usize, m / units.mass.kg())).unwrap_or_default());
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("properties-dialog")
            .title(title)
            .width(540.0)
            .body(move |b| {
                let t = &tb;
                let text_row = |b: &mut ChildSpawner, name: &'static str, label: &str, value: String, disabled: bool| {
                    b.spawn(form_row(t, format!("{name}-row"), label, LABEL_W)).with_children(|r| {
                        r.spawn(TextInput::new(name).value(value).width(Val::Px(FIELD_W)).height(26.0).disabled(disabled).build(t));
                    });
                };
                for (key, name, label) in TEXT_FIELDS {
                    let value = v.get(name).cloned().unwrap_or_default();
                    if key == PropertyKey::PartNumber {
                        b.spawn(form_row(t, "props-part-number-row", label, LABEL_W)).with_children(|r| {
                            r.spawn(TextInput::new(name).value(value).width(Val::Px(FIELD_W)).height(26.0).build(t));
                            r.spawn(IconButton::new("props-generate-part-number", "tag-new").tooltip("Generate next part number").build(t));
                        });
                    } else {
                        text_row(b, name, label, value, false);
                    }
                }
                // Unit of measure and Category.
                let unit = v.get("props-unit").cloned().unwrap_or_default();
                let mut s = Select::new("props-unit").width(Val::Px(FIELD_W)).bordered();
                for u in UNITS_OF_MEASURE {
                    s = s.option(u, true);
                }
                let sel = UNITS_OF_MEASURE.iter().position(|u| *u == unit).unwrap_or(0);
                b.spawn(form_row(t, "props-unit-row", "Unit of measure", LABEL_W)).with_children(|r| fixed(r.spawn(s.selected(sel).build(t))));
                let cat = v.get("props-category").cloned().unwrap_or_default();
                let mut cats: Vec<String> = vec![String::new()];
                cats.extend(CATEGORIES.iter().map(|c| c.to_string()));
                if !cats.contains(&cat) {
                    cats.push(cat.clone());
                }
                let mut s = Select::new("props-category").width(Val::Px(FIELD_W)).bordered();
                for c in &cats {
                    s = s.option(if c.is_empty() { "–".to_string() } else { c.clone() }, true);
                }
                let sel = cats.iter().position(|c| *c == cat).unwrap_or(0);
                b.spawn(form_row(t, "props-category-row", "Category", LABEL_W)).with_children(|r| fixed(r.spawn(s.selected(sel).build(t))));
                if !is_asm {
                    // Material: none or a library material (Assign material… has the rest).
                    let now = v.get("props-material").cloned().unwrap_or_default();
                    let mut s = Select::new("props-material").width(Val::Px(FIELD_W)).bordered();
                    s = s.option("No material", true);
                    let mut sel = 0;
                    for (i, m) in mats.iter().enumerate() {
                        s = s.option(m.name.clone(), true);
                        if m.name == now {
                            sel = i + 1;
                        }
                    }
                    b.spawn(form_row(t, "props-material-row", "Material", LABEL_W)).with_children(|r| fixed(r.spawn(s.selected(sel).build(t))));
                    // Appearance: its colour (Edit appearance… changes it).
                    b.spawn(form_row(t, "props-appearance-row", "Appearance", LABEL_W)).with_children(|r| {
                        let c = appearance.map(|a| Color::srgba_u8(a.rgb[0], a.rgb[1], a.rgb[2], a.alpha)).unwrap_or(Color::NONE);
                        r.spawn((
                            Name::new("props-appearance"),
                            Node { width: Val::Px(40.0), height: Val::Px(18.0), border: UiRect::all(Val::Px(1.0)), ..default() },
                            BackgroundColor(c),
                            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.3)),
                        ));
                        let hex = appearance.map(|a| format!("#{:02X}{:02X}{:02X}", a.rgb[0], a.rgb[1], a.rgb[2])).unwrap_or_default();
                        r.spawn(t.text(hex, t.font_base, FontWeight::NORMAL, t.muted_foreground));
                    });
                }
                // Mass: computed, or the override typed.
                b.spawn(form_row(t, "props-mass-row", format!("Mass ({})", units.mass.symbol()), LABEL_W)).with_children(|r| {
                    r.spawn(TextInput::new("props-mass").value(mass_text).width(Val::Px(FIELD_W)).height(26.0).build(t));
                });
                b.spawn(form_row(t, "props-mass-override-row", "", LABEL_W)).with_children(|r| {
                    r.spawn(Checkbox::new("props-mass-override").label("Override the computed mass").checked(override_on).build(t));
                });
                if is_asm {
                    let now = v.get("props-bom-behavior").cloned().unwrap_or_default();
                    let mut s = Select::new("props-bom-behavior").width(Val::Px(FIELD_W)).bordered();
                    for b in SubassemblyBom::ALL {
                        s = s.option(b.label(), true);
                    }
                    let sel = SubassemblyBom::ALL.iter().position(|b| b.label() == now).unwrap_or(0);
                    b.spawn(form_row(t, "props-bom-behavior-row", "Subassembly BOM behavior", LABEL_W)).with_children(|r| fixed(r.spawn(s.selected(sel).build(t))));
                }
                if is_std {
                    b.spawn(t.text("Standard content: Part number and Description are the configuration's.", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                }
                // Custom properties of the document.
                if !defs.is_empty() {
                    b.spawn((t.text("Custom properties", t.font_base, FontWeight::BOLD, t.foreground), Node { margin: UiRect::top(Val::Px(4.0)), ..default() }));
                }
                for d in &defs {
                    let name = format!("props-custom-{}", d.id.0);
                    let value = v.get(&name).cloned().unwrap_or_default();
                    let label = format!("{} ({})", d.name, d.kind.label());
                    match &d.kind {
                        PropertyKind::Boolean | PropertyKind::List(_) => {
                            let choices: Vec<String> = match &d.kind {
                                PropertyKind::List(c) => c.clone(),
                                _ => vec!["true".into(), "false".into()],
                            };
                            let mut s = Select::new(name.clone()).width(Val::Px(FIELD_W)).bordered().option("–", true);
                            for c in &choices {
                                s = s.option(c.clone(), true);
                            }
                            let sel = choices.iter().position(|c| *c == value).map_or(0, |i| i + 1);
                            b.spawn(form_row(t, format!("{name}-row"), label, LABEL_W)).with_children(|r| fixed(r.spawn(s.selected(sel).build(t))));
                        }
                        _ => {
                            b.spawn(form_row(t, format!("{name}-row"), label, LABEL_W)).with_children(|r| {
                                r.spawn(TextInput::new(name.clone()).value(value).width(Val::Px(FIELD_W)).height(26.0).build(t));
                            });
                        }
                    }
                }
                // Add property: a name and a kind.
                b.spawn(form_row(t, "props-new-row", "Add property", LABEL_W)).with_children(|r| {
                    // Name, kind and Add together as wide as a field.
                    r.spawn(TextInput::new("props-new-name").placeholder("Name").width(Val::Px(FIELD_W - 90.0 - ADD_W - 24.0)).height(26.0).build(t));
                    fixed(r.spawn(Select::new("props-new-kind").width(Val::Px(90.0)).bordered().option("Text", true).option("Number", true).option("Boolean", true).build(t)));
                    r.spawn(Button::new("props-new-add").label("Add").small().build(t)).entry::<Node>().and_modify(|mut n| {
                        n.width = Val::Px(ADD_W);
                        n.justify_content = JustifyContent::Center;
                    });
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(Button::new("props-save").label("Save").primary().build(t));
                f.spawn(Button::new("props-cancel").label("Cancel").build(t));
            })
            .build(&theme),
        PropsDialog,
        DespawnOnExit(AppState::Document),
        observe(|_: On<DialogClose>, mut commands: Commands| {
            commands.remove_resource::<PropertiesSession>();
        }),
    ));
    world.flush();
}

fn on_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(a.entity) else { return };
    match n.as_str() {
        "props-save" => commands.queue(save),
        "props-cancel" => commands.queue(close),
        "props-new-add" => commands.queue(add_property),
        "props-generate-part-number" => commands.queue(generate_part_number),
        _ => {}
    }
}

/// What the dialog's fields hold now, as a draft (kept when it is rebuilt).
fn read_draft(world: &mut World) -> HashMap<String, String> {
    let mut d = field_texts(world);
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let sel = selects(world);
    if let Some(i) = sel.get("props-unit") {
        d.insert("props-unit".into(), UNITS_OF_MEASURE.get(*i).unwrap_or(&"Each").to_string());
    }
    if let Some(i) = sel.get("props-material") {
        let mats = materials(&doc);
        d.insert("props-material".into(), if *i == 0 { String::new() } else { mats.get(i - 1).map(|m| m.name.clone()).unwrap_or_default() });
    }
    if let Some(i) = sel.get("props-bom-behavior") {
        d.insert("props-bom-behavior".into(), SubassemblyBom::ALL.get(*i).copied().unwrap_or_default().label().to_string());
    }
    for def in &doc.properties.definitions {
        let name = format!("props-custom-{}", def.id.0);
        if let Some(i) = sel.get(&name) {
            let choices: Vec<String> = match &def.kind {
                PropertyKind::List(c) => c.clone(),
                _ => vec!["true".into(), "false".into()],
            };
            d.insert(name, if *i == 0 { String::new() } else { choices.get(i - 1).cloned().unwrap_or_default() });
        }
    }
    // The category select: its options are rebuilt from the text.
    if let Some(i) = sel.get("props-category") {
        let mut q = world.query::<(&Name, &SelectState)>();
        if let Some((_, s)) = q.iter(world).find(|(n, _)| n.as_str() == "props-category") {
            let label = s.options.get(*i).map(|o| o.0.clone()).unwrap_or_default();
            d.insert("props-category".into(), if label == "–" { String::new() } else { label });
        }
    }
    if let Some(c) = checks(world).get("props-mass-override") {
        d.insert("props-mass-override".into(), c.to_string());
    }
    d
}

/// Add property: a new custom property definition (its own undo step); the dialog is rebuilt
/// with it, keeping what was typed.
fn add_property(world: &mut World) {
    let mut draft = read_draft(world);
    let name = draft.remove("props-new-name").unwrap_or_default();
    let kind = match selects(world).get("props-new-kind") {
        Some(1) => PropertyKind::Number,
        Some(2) => PropertyKind::Boolean,
        _ => PropertyKind::Text,
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    if let Err(e) = doc.execute(&AddPropertyDefinition { name, kind }) {
        warn!("property: {e}");
        return;
    }
    if let Some(mut s) = world.get_resource_mut::<PropertiesSession>() {
        s.draft = draft;
    }
    spawn(world);
}

/// Generate: the document's next free sequential part number, into the field (saved with Save).
fn generate_part_number(world: &mut World) {
    let Some(s) = world.get_resource::<PropertiesSession>().cloned() else { return };
    let mut draft = read_draft(world);
    let doc = world.resource::<ActiveDocument>().doc.clone();
    // As if the part had none.
    let mut probe = doc.clone();
    let _ = properties::set(&mut probe, s.owner, PropertyKey::PartNumber, &PropertyValue::Text(String::new()));
    let plan = GenerateMissingPartNumbers { owners: vec![s.owner] }.plan(&probe);
    if let Some((_, n)) = plan.first() {
        draft.insert("props-part-number".into(), n.clone());
        if let Some(mut s) = world.get_resource_mut::<PropertiesSession>() {
            s.draft = draft;
        }
        spawn(world);
    }
}

/// Save: every changed value, one undo step.
fn save(world: &mut World) {
    let Some(s) = world.get_resource::<PropertiesSession>().cloned() else { return };
    let owner = s.owner;
    let draft = read_draft(world);
    let now = values(world, owner, &HashMap::new());
    let doc = world.resource::<ActiveDocument>().doc.clone();
    let units = doc.units;
    let mut out: Vec<(PropertyKey, PropertyValue)> = Vec::new();
    let changed = |name: &str| draft.get(name).is_some_and(|x| now.get(name).is_none_or(|y| x.trim() != y.trim()));
    for (key, name, _) in TEXT_FIELDS {
        if changed(name) {
            out.push((key, PropertyValue::Text(draft[name].clone())));
        }
    }
    if changed("props-unit") {
        out.push((PropertyKey::UnitOfMeasure, PropertyValue::Text(draft["props-unit"].clone())));
    }
    if changed("props-category") {
        out.push((PropertyKey::Category, PropertyValue::Text(draft["props-category"].clone())));
    }
    if changed("props-material") {
        let m = materials(&doc).into_iter().find(|m| m.name == draft["props-material"]);
        out.push((PropertyKey::Material, PropertyValue::Material(m)));
    }
    if changed("props-bom-behavior") {
        let b = SubassemblyBom::ALL.iter().copied().find(|b| b.label() == draft["props-bom-behavior"]).unwrap_or_default();
        out.push((PropertyKey::BomBehavior, PropertyValue::BomBehavior(b)));
    }
    for d in &doc.properties.definitions {
        let name = format!("props-custom-{}", d.id.0);
        if changed(&name) {
            out.push((PropertyKey::Custom(d.id), PropertyValue::Text(draft[&name].clone())));
        }
    }
    // The mass override: on with a value, or off.
    let was = properties::properties(&doc, owner).mass_override;
    let on = draft.get("props-mass-override").is_some_and(|x| x == "true");
    let typed = draft.get("props-mass").and_then(|x| x.split_whitespace().next()?.parse::<f64>().ok()).map(|v| v * units.mass.kg());
    let want = if on { typed } else { None };
    let differs = match (was, want) {
        (Some(a), Some(b)) => (a - b).abs() > 1e-9 * a.abs().max(1e-9),
        (None, None) => false,
        _ => true,
    };
    if differs {
        out.push((PropertyKey::Mass, PropertyValue::Mass(want)));
    }
    if !out.is_empty()
        && let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = d.execute(&SetProperties { owners: vec![owner], values: out, label: "Edit properties".into() })
    {
        warn!("properties: {e}");
        return;
    }
    close(world);
}

/// A select keeps its given width (selects grow to fill their row by default), so it lines up
/// with the text fields.
fn fixed(mut e: EntityWorldMut) {
    if let Some(mut n) = e.get_mut::<Node>() {
        n.flex_grow = 0.0;
    }
}
