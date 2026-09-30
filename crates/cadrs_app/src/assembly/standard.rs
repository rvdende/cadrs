//! **Standard content** in the Assembly tab (P3B.5, `intro-to-assemblies.md` A19, A21.2, A21.14;
//! `lesson-standard-content.png`, `ex3-step2.png`, `ex3-step14.png`), on the library and
//! placement of [`cadrs_core::assembly::standard`]:
//!
//! - The Insert dialog's **Standard content** tab ([`StdForm`] in [`StdMode::Insert`]): cascading
//!   **Standard / Category / Class / Component** dropdowns, the component's options (**Size**
//!   with **Auto-size**, Length, Thread length, Bearing face, Material, Finish), **Part number**
//!   and **Description** (auto-filled, editable, kept per document), a **preview** (the
//!   generated part drawn in an end and a side view), and **Insert** with **Insert closest to selection** /
//!   **Insert furthest from selection** (stacking).
//! - **Batch placement** (A19.6): hole edges, hole faces or faces with holes selected (before or
//!   while the dialog is open) → Insert puts one fastener on each, each with a Fastened mate.
//!   **Single placement** (A19.5): Insert with nothing selected arms the tool: hovering a hole's
//!   or shaft's edge shows the fastener there, **A** flips it, a click inserts it (each click
//!   another), Esc stops.
//! - **Auto-size** (A19.2): the size for the selected (or the next clicked) hole or shaft.
//! - **Edit standard content instance** (A19.9, [`StdMode::Edit`]): the same panel with the
//!   Standard, Category, Class and Component fixed; Size and Length change every selected
//!   instance at once (**Update**, then ✓).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight};
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::standard::{self, EditStandardContent, HoleSite, StandardPart, StandardSpec, Stacking};
use cadrs_core::{Element, ElementId};
use cadrs_ui::dialog_fields::{SelectChange, SelectState};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel};

use crate::viewport::{Pick, PlaneHighlight, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct StandardContentPlugin;

impl Plugin for StandardContentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Previews>()
            .add_systems(
                Update,
                (rebuild_panel, place_on_selection, hover_ghost, std_keys, read_properties)
                    .chain()
                    .before(crate::parts::PartsSet)
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<StdForm>();
            })
            .add_observer(on_select)
            .add_observer(on_button)
            .add_observer(on_edit_accept)
            .add_observer(on_edit_cancel);
    }
}

/// Where the panel is.
#[derive(Debug, Clone, PartialEq)]
pub enum StdMode {
    /// The Insert dialog's Standard content tab.
    Insert,
    /// Edit standard content instance, on these instances; `applied`: the undo steps its
    /// Updates made.
    Edit { instances: Vec<InstanceId>, applied: usize },
}

/// The standard content panel's state.
#[derive(Resource, Debug, Clone)]
pub struct StdForm {
    pub mode: StdMode,
    pub spec: StandardSpec,
    pub part_number: String,
    pub description: String,
    /// Single placement is armed (Insert with nothing selected).
    pub placing: bool,
    /// A flips the fastener being placed.
    pub flip: bool,
    /// Auto-size waits for a hole or shaft to be clicked.
    pub autosize_pick: bool,
    /// The configuration last applied by Update (edit mode).
    pub applied_spec: Option<StandardSpec>,
    /// Bumped to rebuild the panel's fields (their text is their own otherwise).
    pub refresh: u32,
    /// The hole edge the fastener being placed is shown on (a click on the shown fastener
    /// places it there).
    pub hover_site: Option<HoleSite>,
}

impl StdForm {
    fn new(mode: StdMode, spec: StandardSpec) -> Self {
        Self {
            mode,
            part_number: spec.part_number(),
            description: spec.description(),
            spec,
            placing: false,
            flip: false,
            autosize_pick: false,
            applied_spec: None,
            refresh: 0,
            hover_site: None,
        }
    }

    /// The course's first pick: ANSI inch / Bolts & screws / Hex bolts / Hex cap screw.
    pub fn insert_default() -> Self {
        Self::new(StdMode::Insert, StandardSpec::new("ANSI inch", "Bolts & screws", "Hex bolts", "Hex cap screw").expect("the library has hex cap screws"))
    }

    fn is_edit(&self) -> bool {
        matches!(self.mode, StdMode::Edit { .. })
    }

    /// The spec changed: the part number and description follow it.
    fn respec(&mut self, spec: StandardSpec) {
        self.part_number = spec.part_number();
        self.description = spec.description();
        self.spec = spec;
    }
}

/// The container the panel is built into (in the Insert dialog, or the Edit dialog's body).
#[derive(Component)]
pub struct StdPanelHost;

/// The Edit standard content dialog.
#[derive(Component)]
struct StdEditDialog;

/// The preview's size (logical px; drawn at twice that).
const PREVIEW_W: u32 = 216;
const PREVIEW_H: u32 = 100;

/// A generated configuration: its studio, rebuild and preview image.
type Preview = (Element, Arc<cadrs_core::rebuild::Build>, Option<Handle<Image>>);

/// Generated configurations.
#[derive(Resource, Default)]
struct Previews(HashMap<ElementId, Preview>);

impl Previews {
    fn get(&mut self, spec: &StandardSpec) -> Option<(Element, Arc<cadrs_core::rebuild::Build>)> {
        let id = spec.element_id();
        if let std::collections::hash_map::Entry::Vacant(e) = self.0.entry(id) {
            let el = standard::generate(spec).ok()?;
            let b = cadrs_core::rebuild::build(el.features());
            e.insert((el, b, None));
        }
        self.0.get(&id).map(|(e, b, _)| (e.clone(), b.clone()))
    }

    fn image(&mut self, spec: &StandardSpec, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let id = spec.element_id();
        self.get(spec)?;
        let entry = self.0.get_mut(&id)?;
        if let Some(h) = &entry.2 {
            return Some(h.clone());
        }
        // Two orthographic line views, end and side (P3B.5 judge, `lesson-standard-content.png`).
        let part = entry.1.parts.first()?;
        let img = cadrs_core::assembly::thumb::line_views(&part.solid, 2 * PREVIEW_W, 2 * PREVIEW_H);
        let (w, h) = img.dimensions();
        let image = Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            img.into_raw(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        let handle = images.add(image);
        entry.2 = Some(handle.clone());
        Some(handle)
    }
}

/// The library part for the form: the document's (with its stored part number) if it has
/// this configuration, else generated; with the form's Part number and Description.
fn standard_part(world: &mut World, form: &StdForm) -> Option<StandardPart> {
    let existing = world.resource::<ActiveDocument>().doc.standard_part(form.spec.element_id()).cloned();
    let element = match existing {
        Some(p) => p.element,
        None => world.resource_mut::<Previews>().get(&form.spec)?.0,
    };
    Some(StandardPart { spec: form.spec.clone(), part_number: form.part_number.clone(), description: form.description.clone(), element })
}

// ---------------------------------------------------------------------------------------------
// The panel

/// A label and its control, Onshape's narrow dialog rows.
fn row(p: &mut ChildSpawnerCommands, t: &Theme, name: &str, label: &str, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn((
        Name::new(format!("{name}-row")),
        Node { height: Val::Px(24.0), align_items: AlignItems::Center, column_gap: Val::Px(6.0), padding: UiRect::horizontal(Val::Px(8.0)), ..default() },
    ))
    .with_children(|r| {
        r.spawn((t.text(label, 11.0, FontWeight::NORMAL, Color::srgb_u8(0x55, 0x55, 0x55)), Node { width: Val::Px(66.0), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
        f(r);
    });
}

fn select(r: &mut ChildSpawnerCommands, t: &Theme, name: &str, options: &[String], selected: &str) {
    let mut s = cadrs_ui::dialog_fields::Select::new(name.to_string());
    for o in options {
        s = s.option(o.clone(), true);
    }
    let i = options.iter().position(|o| o == selected).unwrap_or(0);
    r.spawn(s.selected(i).build(t)).entry::<Node>().and_modify(|mut n| n.height = Val::Px(22.0));
}

/// A fixed value (edit mode's Standard … Component, and the options it can't change).
fn fixed(r: &mut ChildSpawnerCommands, t: &Theme, name: &str, value: &str) {
    r.spawn((
        Name::new(name.to_string()),
        Node { flex_grow: 1.0, height: Val::Px(22.0), align_items: AlignItems::Center, padding: UiRect::left(Val::Px(3.0)), border: UiRect::bottom(Val::Px(1.0)), ..default() },
        BorderColor::all(Color::srgb_u8(0xe0, 0xe0, 0xe0)),
        children![(t.text(value.to_string(), t.font_base, FontWeight::MEDIUM, Color::srgb_u8(0xa0, 0xa0, 0xa0)), Pickable::IGNORE)],
    ));
}

fn separator(p: &mut ChildSpawnerCommands) {
    p.spawn((Node { height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(4.0)), ..default() }, BackgroundColor(Color::srgb_u8(0xe6, 0xe6, 0xe6))));
}

/// What the panel was built from.
type PanelKey = (StandardSpec, bool, bool, bool, u32);

#[allow(clippy::too_many_arguments)]
fn rebuild_panel(
    form: Option<Res<StdForm>>,
    q_host: Query<(Entity, Ref<StdPanelHost>)>,
    theme: Res<Theme>,
    mut previews: ResMut<Previews>,
    mut images: ResMut<Assets<Image>>,
    mut last: Local<Option<PanelKey>>,
    mut commands: Commands,
) {
    let Some(form) = form else {
        *last = None;
        return;
    };
    let Some((host, added)) = q_host.iter().next().map(|(e, r)| (e, r.is_added())) else { return };
    // Typing in the Part number / Description fields changes the form, not the key: the fields
    // keep their own text; a spec, mode or refresh change rebuilds them.
    let key: PanelKey = (form.spec.clone(), form.is_edit(), form.placing, form.autosize_pick, form.refresh);
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let t = theme.clone();
    let lib = standard::library();
    let spec = form.spec.clone();
    let Some((st, comp)) = spec.component_def() else { return };
    let edit = form.is_edit();
    let preview = previews.image(&spec, &mut images);
    commands.entity(host).despawn_children();
    commands.entity(host).with_children(|p| {
        p.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::vertical(Val::Px(4.0)), ..default() }).with_children(|p| {
            let cat = st.category(&spec.category);
            let class = cat.and_then(|c| c.class(&spec.class));
            let levels: [(&str, &str, Vec<String>, &str); 4] = [
                ("std-standard", "Standard", lib.standards.iter().map(|s| s.name.clone()).collect(), &spec.standard),
                ("std-category", "Category", st.categories.iter().map(|c| c.name.clone()).collect(), &spec.category),
                ("std-class", "Class", cat.map(|c| c.classes.iter().map(|c| c.name.clone()).collect()).unwrap_or_default(), &spec.class),
                ("std-component", "Component", class.map(|c| c.components.iter().map(|c| c.name.clone()).collect()).unwrap_or_default(), &spec.component),
            ];
            for (name, label, options, value) in levels {
                row(p, &t, name, label, |r| if edit { fixed(r, &t, name, value) } else { select(r, &t, name, &options, value) });
            }
            if let StdMode::Edit { instances, .. } = &form.mode
                && instances.len() > 1
            {
                p.spawn((
                    Name::new("std-edit-count"),
                    t.text(format!("{} instances: Size and Length change on all of them", instances.len()), 10.5, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(4.0), Val::Px(0.0)), ..default() },
                ));
            }
            separator(p);
            let sizes: Vec<String> = comp.sizes.iter().map(|s| s.name.clone()).collect();
            row(p, &t, "std-size", "Size", |r| {
                select(r, &t, "std-size", &sizes, &spec.size);
                if !edit {
                    r.spawn(
                        IconButton::new("std-autosize", "measure")
                            .icon_size(15.0)
                            .selected(form.autosize_pick)
                            .tooltip("Auto-size: pick a hole or shaft (bolts round down, nuts round up)")
                            .build(&t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(22.0);
                        n.height = Val::Px(22.0);
                    });
                }
            });
            if let Some(l) = spec.length {
                let lengths: Vec<String> = comp.lengths.iter().map(|x| standard::fmt_len(*x)).collect();
                row(p, &t, "std-length", "Length", |r| select(r, &t, "std-length", &lengths, &standard::fmt_len(l)));
                let tl = standard::fmt_len(spec.thread_length.unwrap_or(l));
                row(p, &t, "std-thread-length", "Thread length", |r| if edit { fixed(r, &t, "std-thread-length", &tl) } else { select(r, &t, "std-thread-length", std::slice::from_ref(&tl), &tl) });
            }
            let mut option = |name: &str, label: &str, list: &[String], value: &Option<String>| {
                if let Some(v) = value {
                    row(p, &t, name, label, |r| if edit { fixed(r, &t, name, v) } else { select(r, &t, name, list, v) });
                }
            };
            option("std-bearing-face", "Bearing face", &comp.bearing_faces, &spec.bearing_face);
            option("std-material", "Material", &comp.materials, &Some(spec.material.clone()));
            option("std-finish", "Finish", &comp.finishes, &spec.finish);
            separator(p);
            // Part number and Description (A19.3): auto-filled, editable, per document.
            row(p, &t, "std-part-number", "Part number", |r| {
                r.spawn(TextInput::new("std-part-number").value(form.part_number.clone()).height(22.0).width(Val::Percent(100.0)).font(11.0, FontWeight::NORMAL).build(&t))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 1.0;
                        n.min_width = Val::Px(0.0);
                    });
                r.spawn(IconButton::new("std-generate-part-number", "tag-new").icon_size(14.0).tooltip("Use the library's part number").build(&t))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(22.0);
                        n.height = Val::Px(22.0);
                    });
            });
            row(p, &t, "std-description", "Description", |r| {
                r.spawn(TextInput::new("std-description").value(form.description.clone()).height(22.0).width(Val::Percent(100.0)).font(11.0, FontWeight::NORMAL).build(&t))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 1.0;
                        n.min_width = Val::Px(0.0);
                    });
            });
            separator(p);
            // The preview (A19.4): the generated part drawn in two orthographic views.
            p.spawn((Name::new("std-preview"), Node { justify_content: JustifyContent::Center, height: Val::Px(PREVIEW_H as f32 + 4.0), ..default() })).with_children(|b| {
                if let Some(img) = preview.clone() {
                    b.spawn((Node { width: Val::Px(PREVIEW_W as f32), height: Val::Px(PREVIEW_H as f32), ..default() }, ImageNode::new(img), Pickable::IGNORE));
                }
            });
            p.spawn(Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, column_gap: Val::Px(4.0), padding: UiRect::vertical(Val::Px(4.0)), ..default() }).with_children(|b| {
                if edit {
                    b.spawn(cadrs_ui::Button::new("std-update").label("Update").outline().small().build(&t));
                } else {
                    b.spawn(cadrs_ui::Button::new("std-insert").label("Insert").outline().small().selected(form.placing).tooltip("Insert on the selected holes, or pick holes one by one").build(&t));
                    for (name, icon, tip) in [
                        ("std-insert-closest", "arrow-down", "Insert closest to selection"),
                        ("std-insert-furthest", "arrow-up", "Insert furthest from selection"),
                    ] {
                        b.spawn(IconButton::new(name, icon).icon_size(15.0).tooltip(tip).build(&t)).entry::<Node>().and_modify(|mut n| {
                            n.width = Val::Px(24.0);
                            n.height = Val::Px(24.0);
                        });
                    }
                }
            });
            if form.placing || form.autosize_pick {
                let hint = if form.autosize_pick { "Click a hole or shaft edge to size it" } else { "Click hole edges to insert (A flips, Esc stops)" };
                p.spawn((Name::new("std-hint"), t.text(hint, 10.5, FontWeight::NORMAL, t.muted_foreground), Node { align_self: AlignSelf::Center, margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
            }
        });
    });
}

/// The dropdowns: a higher level resets the ones below it to their first choices.
fn on_select(ev: On<SelectChange>, q: Query<(&Name, &SelectState)>, form: Option<ResMut<StdForm>>) {
    let Some(mut form) = form else { return };
    let Ok((name, state)) = q.get(ev.entity) else { return };
    let Some(value) = state.options.get(ev.index).map(|o| o.0.clone()) else { return };
    let mut s = form.spec.clone();
    let lib = standard::library();
    match name.as_str() {
        "std-standard" => {
            let Some(st) = lib.standard(&value) else { return };
            let c = &st.categories[0];
            let k = &c.classes[0];
            s = StandardSpec::new(&value, &c.name, &k.name, &k.components[0].name).unwrap_or(s);
        }
        "std-category" => {
            let Some(c) = lib.standard(&s.standard).and_then(|st| st.category(&value)) else { return };
            let k = &c.classes[0];
            s = StandardSpec::new(&s.standard, &value, &k.name, &k.components[0].name).unwrap_or(s);
        }
        "std-class" => {
            let Some(k) = lib.standard(&s.standard).and_then(|st| st.category(&s.category)).and_then(|c| c.class(&value)) else { return };
            s = StandardSpec::new(&s.standard, &s.category, &value, &k.components[0].name).unwrap_or(s);
        }
        "std-component" => {
            let mut n = StandardSpec::new(&s.standard, &s.category, &s.class, &value).unwrap_or(s.clone());
            // Keep the material where the new component has it.
            n.material = s.material.clone();
            n.normalize();
            s = n;
        }
        "std-size" => s.size = value,
        "std-length" => s.length = value.parse().ok(),
        "std-material" => s.material = value,
        "std-bearing-face" => s.bearing_face = Some(value),
        "std-finish" => s.finish = Some(value),
        _ => return,
    }
    s.normalize();
    if s != form.spec {
        form.respec(s);
    }
}

/// Typing in Part number / Description.
fn read_properties(q: Query<(&Name, &EditableText)>, form: Option<ResMut<StdForm>>, mut seen: Local<Option<(StandardSpec, u32)>>) {
    let Some(mut form) = form else {
        *seen = None;
        return;
    };
    // The fields show the old configuration until the panel is rebuilt (an Auto-size click
    // changed it this frame): don't read their old text back over the new Part number.
    let now = (form.spec.clone(), form.refresh);
    if seen.as_ref() != Some(&now) {
        *seen = Some(now);
        return;
    }
    for (n, t) in &q {
        let v = t.value().to_string();
        match n.as_str() {
            "std-part-number-field" if form.part_number != v => form.part_number = v,
            "std-description-field" if form.description != v => form.description = v,
            _ => {}
        }
    }
}

fn on_button(a: On<Activate>, q: Query<&Name>, form: Option<Res<StdForm>>, mut commands: Commands) {
    if form.is_none() {
        return;
    }
    let Ok(name) = q.get(a.entity) else { return };
    let name = name.as_str().to_string();
    if !name.starts_with("std-") {
        return;
    }
    commands.queue(move |world: &mut World| act(world, &name));
}

fn act(world: &mut World, name: &str) {
    match name {
        "std-insert" | "std-insert-closest" | "std-insert-furthest" => {
            let stacking = match name {
                "std-insert-closest" => Stacking::Closest,
                "std-insert-furthest" => Stacking::Furthest,
                _ => Stacking::Plain,
            };
            let sites = selected_sites(world);
            debug!("standard content: {} site(s) from {:?}", sites.len(), world.resource::<Selection>().0);
            if sites.is_empty() {
                if stacking == Stacking::Plain
                    && let Some(mut f) = world.get_resource_mut::<StdForm>()
                {
                    f.placing = !f.placing;
                    f.autosize_pick = false;
                } else {
                    let theme = world.resource::<Theme>().clone();
                    let mut commands = world.commands();
                    cadrs_ui::toast::show_toast(&mut commands, &theme, "Select a hole or a fastener's hole edge first");
                }
                return;
            }
            // The selection stays, so a washer or nut can be stacked on the same holes next
            // (Insert closest / furthest, A19.7).
            let flip = world.get_resource::<StdForm>().is_some_and(|f| f.flip);
            insert_at(world, &sites, flip, stacking);
        }
        "std-autosize" => {
            let sites = selected_sites(world);
            match sites.first() {
                Some(site) => autosize(world, site),
                None => {
                    if let Some(mut f) = world.get_resource_mut::<StdForm>() {
                        f.autosize_pick = !f.autosize_pick;
                        f.placing = false;
                    }
                }
            }
        }
        "std-generate-part-number" => {
            if let Some(mut f) = world.get_resource_mut::<StdForm>() {
                f.part_number = f.spec.part_number();
                f.refresh += 1;
            }
        }
        "std-update" => update(world),
        _ => {}
    }
}

/// The hole sites of the selection (edges; faces: their holes), on the active assembly.
fn selected_sites(world: &mut World) -> Vec<HoleSite> {
    let picks = world.resource::<Selection>().0.clone();
    sites_of(world, &picks)
}

fn sites_of(world: &mut World, picks: &[Pick]) -> Vec<HoleSite> {
    if !picks.iter().any(|p| matches!(p, Pick::Edge(..) | Pick::Face(..))) {
        return Vec::new();
    }
    let solids = solids(world);
    let mut out = Vec::new();
    for p in picks {
        match p {
            Pick::Edge(part, edge) => {
                let occ = super::occurrence_of(*part);
                if let Some(s) = solids.get(&occ).and_then(|s| standard::site_of_edge(s, occ, edge)) {
                    out.push(s);
                }
            }
            Pick::Face(part, face) => {
                let occ = super::occurrence_of(*part);
                if let Some(s) = solids.get(&occ) {
                    out.extend(standard::sites_of_face(s, occ, face));
                }
            }
            _ => {}
        }
    }
    out
}

fn solids(world: &mut World) -> HashMap<InstanceId, Arc<cadrs_core::Solid>> {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return HashMap::new() };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()).cloned() else { return HashMap::new() };
    let d = doc.doc.clone();
    let mut parts = world.resource_mut::<super::AssemblyParts>();
    cadrs_core::assembly::occurrence_solids(&d, &model, |e| parts.build(&d, e))
}

/// Inserts the form's configuration on `sites` (one undo step); the dialog counts them.
fn insert_at(world: &mut World, sites: &[HoleSite], flip: bool, stacking: Stacking) -> bool {
    let Some(form) = world.get_resource::<StdForm>().cloned() else { return false };
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else { return false };
    let Some(part) = standard_part(world, &form) else { return false };
    let solids = solids(world);
    let plan = match standard::plan_insert(&world.resource::<ActiveDocument>().doc, element, part, sites, flip, stacking, &solids) {
        Ok(p) => p,
        Err(e) => {
            warn!("standard content: {e}");
            return false;
        }
    };
    let ids: Vec<InstanceId> = plan.inserts.iter().map(|i| i.instance).collect();
    if !super::run(world, &plan) {
        return false;
    }
    if let Some(mut s) = world.get_resource_mut::<super::insert::InsertSession>() {
        s.inserted.extend(ids);
        s.std_steps += 1;
    }
    true
}

/// Auto-size (A19.2) from a site.
fn autosize(world: &mut World, site: &HoleSite) {
    let Some(mut f) = world.get_resource_mut::<StdForm>() else { return };
    let Some((st, comp)) = f.spec.component_def() else { return };
    f.autosize_pick = false;
    if let Some(size) = standard::auto_size(st.unit, comp, site.diameter, &f.spec.size) {
        let s = f.spec.with(Some(&size), None);
        f.respec(s);
        f.refresh += 1;
    }
}

/// Single placement and auto-size picks: a hole edge clicked (selected) while armed.
fn place_on_selection(world: &mut World) {
    let Some(form) = world.get_resource::<StdForm>().cloned() else { return };
    if !(form.placing || form.autosize_pick) {
        return;
    }
    let picks = world.resource::<Selection>().0.clone();
    let edges: Vec<Pick> = picks.iter().filter(|p| matches!(p, Pick::Edge(..) | Pick::Face(..))).copied().collect();
    if edges.is_empty() {
        return;
    }
    // A click on the fastener shown on a hole places it on that hole.
    let on_ghost = edges.iter().any(|p| p.part().is_some_and(|q| InstanceId::of_part(q) == InstanceId::from_u128(GHOST)));
    let sites = if on_ghost { form.hover_site.into_iter().collect() } else { sites_of(world, &edges) };
    world.resource_mut::<Selection>().0.retain(|p| !edges.contains(p));
    let Some(site) = sites.first().copied() else { return };
    if form.autosize_pick {
        autosize(world, &site);
    } else {
        insert_at(world, &[site], form.flip, Stacking::Plain);
    }
}

/// Ghost id of the fastener being placed.
const GHOST: u128 = 0x5c0e_7000_0000_0000_0000_0000_0000_0000;

/// The fastener being placed, as a part of the view: drawn translucent over the scene, never
/// hidden by the part it goes into (A flips it into the hole; P3B.5 judge).
pub fn is_ghost(part: cadrs_core::PartId) -> bool {
    InstanceId::of_part(part) == InstanceId::from_u128(GHOST)
}

/// While placing: the fastener shown on the hovered hole edge.
fn hover_ghost(world: &mut World) {
    let form = world.get_resource::<StdForm>().cloned();
    let hovered = world.resource::<PlaneHighlight>().viewport;
    // Over the fastener shown: it stays where it is.
    if hovered.and_then(|p| p.part()).is_some_and(|q| InstanceId::of_part(q) == InstanceId::from_u128(GHOST)) && form.as_ref().is_some_and(|f| f.placing) {
        return;
    }
    let want = match (&form, hovered) {
        (Some(f), Some(p @ Pick::Edge(..))) if f.placing => {
            let sites = sites_of(world, &[p]);
            if let Some(mut f) = world.get_resource_mut::<StdForm>() {
                let site = sites.first().copied();
                if f.hover_site != site {
                    f.hover_site = site;
                }
            }
            sites.first().copied().and_then(|site| {
                let (el, _) = world.resource_mut::<Previews>().get(&f.spec)?;
                let element = world.get_resource::<ActiveDocument>().and_then(super::active_assembly)?;
                let part = StandardPart { spec: f.spec.clone(), part_number: String::new(), description: String::new(), element: el.clone() };
                let solids = solids(world);
                let plan = standard::plan_insert(&world.resource::<ActiveDocument>().doc, element, part, &[site], f.flip, Stacking::Plain, &solids).ok()?;
                let mut g = cadrs_core::assembly::Instance::new(InstanceId::from_u128(GHOST), InstanceSourceOf::of(&el), plan.inserts[0].pose);
                g.index = 0;
                Some((el, g))
            })
        }
        _ => None,
    };
    let mut parts = world.resource_mut::<super::AssemblyParts>();
    let has = parts.ghosts.iter().any(|g| g.id == InstanceId::from_u128(GHOST));
    match want {
        Some((el, g)) => {
            if !parts.extra.iter().any(|e| e.id == el.id) {
                parts.extra.push(el);
            }
            if parts.ghosts.first() != Some(&g) || parts.ghosts.len() != 1 {
                parts.ghosts = vec![g];
            }
        }
        None if has => parts.ghosts.retain(|g| g.id != InstanceId::from_u128(GHOST)),
        None => {}
    }
}

struct InstanceSourceOf;

impl InstanceSourceOf {
    fn of(el: &Element) -> cadrs_core::assembly::InstanceSource {
        cadrs_core::assembly::InstanceSource::Part { element: el.id, part: standard::PART }
    }
}

/// A flips the fastener being placed.
fn std_keys(
    mut keys: MessageReader<KeyboardInput>,
    form: Option<ResMut<StdForm>>,
    focus: Res<bevy::input_focus::InputFocus>,
    q_fields: Query<(), With<cadrs_ui::input::TextInputField>>,
) {
    let Some(mut form) = form else {
        keys.clear();
        return;
    };
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys.read() {
        if k.state != ButtonState::Pressed || typing {
            continue;
        }
        match k.key_code {
            KeyCode::KeyA if form.placing => form.flip = !form.flip,
            // Esc (stops placing) is the Insert dialog's key handler's.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Edit standard content instance

/// Opens **Edit standard content instance** (A19.9) on `instances` (standard content instances
/// of the active assembly; the first one's configuration is shown).
pub fn open_edit_dialog(world: &mut World, instances: Vec<InstanceId>) {
    if world.contains_resource::<StdForm>() {
        return;
    }
    let spec = {
        let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
        let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
        let Some(first) = instances.iter().find_map(|i| standard::standard_of(&doc.doc, &model.instance(*i)?.source)) else { return };
        first.spec.clone()
    };
    let instances: Vec<InstanceId> = {
        let doc = world.resource::<ActiveDocument>();
        let model = doc.active_element().and_then(|e| e.assembly_model());
        instances.into_iter().filter(|i| model.and_then(|m| m.instance(*i)).is_some_and(|x| standard::standard_of(&doc.doc, &x.source).is_some())).collect()
    };
    let n = instances.len();
    let mut form = StdForm::new(StdMode::Edit { instances, applied: 0 }, spec.clone());
    form.applied_spec = Some(spec);
    world.insert_resource(form);
    let theme = world.resource::<Theme>().clone();
    let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q.iter(world).next() else { return };
    // How many instances it edits at once (A19.9 bulk edit; Final part 3).
    let title = if n > 1 { format!("Edit standard content ({n})") } else { "Edit standard content".to_string() };
    let d = world
        .spawn((
            StdEditDialog,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("std-edit-dialog")
                .title(title)
                .plain_title()
                .width(262.0)
                .body_padding(UiRect::ZERO)
                .body(|b| {
                    b.spawn((Name::new("std-panel"), StdPanelHost, Node { flex_direction: FlexDirection::Column, ..default() }));
                })
                .build(&theme),
        ))
        .id();
    world.entity_mut(area).add_child(d);
}

/// Update: the form's Size and Length to every instance (one undo step).
fn update(world: &mut World) {
    let Some(form) = world.get_resource::<StdForm>().cloned() else { return };
    let StdMode::Edit { instances, .. } = &form.mode else { return };
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else { return };
    let cmd = {
        let doc = &world.resource::<ActiveDocument>().doc;
        EditStandardContent::new(doc, element, instances, Some(&form.spec.size), form.spec.length)
    };
    let cmd = match cmd {
        Ok(c) => c,
        Err(e) => {
            warn!("edit standard content: {e}");
            return;
        }
    };
    let before = world.resource::<ActiveDocument>().history.undo_len();
    super::run(world, &cmd);
    let made = world.resource::<ActiveDocument>().history.undo_len() > before;
    // Its Part number and Description.
    let el = form.spec.element_id();
    let changed_props = world.resource::<ActiveDocument>().doc.standard_part(el).is_some_and(|p| p.part_number != form.part_number || p.description != form.description);
    let props_made = changed_props
        && super::run(world, &standard::SetStandardProperties { element: el, part_number: form.part_number.clone(), description: form.description.clone() });
    if let Some(mut f) = world.get_resource_mut::<StdForm>() {
        if let StdMode::Edit { applied, .. } = &mut f.mode {
            *applied += made as usize + props_made as usize;
        }
        f.applied_spec = Some(f.spec.clone());
    }
}

fn close_edit(world: &mut World) {
    world.remove_resource::<StdForm>();
    let dialogs: Vec<Entity> = world.query_filtered::<Entity, With<StdEditDialog>>().iter(world).collect();
    for d in dialogs {
        world.entity_mut(d).despawn();
    }
}

fn on_edit_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<StdEditDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            // ✓ applies an Update still pending.
            if let Some(f) = world.get_resource::<StdForm>()
                && f.applied_spec.as_ref() != Some(&f.spec)
            {
                update(world);
            }
            close_edit(world);
        });
    }
}

fn on_edit_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<StdEditDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            // ✕ undoes the Updates.
            if let Some(StdMode::Edit { applied, .. }) = world.get_resource::<StdForm>().map(|f| f.mode.clone()) {
                let mut doc = world.resource_mut::<ActiveDocument>();
                for _ in 0..applied {
                    doc.undo();
                }
            }
            close_edit(world);
        });
    }
}
