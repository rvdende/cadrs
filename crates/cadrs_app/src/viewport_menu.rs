//! Right-click menus in the viewport (S1.2, S1.5, S2.3). A right press and release that does
//! not move opens a menu at the pointer (a right press that moves orbits instead,
//! `shortcuts.md`). Styled like the other context menus, which were tuned against Onshape's
//! (`dimension/dimension-driven-01.png`).
//!
//! - **Modeling**, on a default plane or a planar part face: **New sketch** (the plane fills
//!   the sketch dialog's plane field), hide the plane, view normal to it, zoom to fit. On a face
//!   also **Add appearance to face…**, the part's **Edit appearance…** and **Assign material…**
//!   (P3.5).
//! - **Modeling**, on a sketch's region or curve: **Edit…**, **Rename**, **Delete**, **Edit sketch
//!   appearance…** and, on a curve, **Edit curve appearance…** (P3.5), view normal to the sketch
//!   plane, zoom to fit.
//! - **Modeling**, on empty space: zoom to fit, show or hide the planes (P), and **Select →
//!   Create selection…** (X12, [`crate::create_selection`]).
//! - **Sketching**, off a glyph or dimension (those have their own menus): Confirm the sketch,
//!   Show all constraints, Zoom to fit and **View normal to sketch plane**.

use bevy::prelude::*;
use cadrs_core::FeatureId;
use cadrs_ui::menu::{Menu, MenuAction, MenuItem};
use cadrs_ui::{Theme, open_context_menu};

use crate::appearance::AppearanceTarget;
use crate::viewport::{Pick, PlaneKind, PlanesVisible, Selection, ViewportView, plane_normal};
use crate::{ActiveDocument, AppState};

pub struct ViewportMenuPlugin;

impl Plugin for ViewportMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_viewport_menu);
    }
}

/// What a viewport menu was opened on.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum MenuTarget {
    Plane(PlaneKind),
    Face(cadrs_core::PartId, cadrs_sketch::FaceName),
    Sketch(FeatureId),
    Empty,
    /// Inside the sketch being edited.
    Sketching,
}

fn feature_name(world: &World, id: FeatureId) -> String {
    world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element()?.feature(id).map(|f| f.name.clone()))
        .unwrap_or_else(|| "sketch".into())
}

/// Modeling: the menu for what was right-clicked (`pick`, from the modeling pick filter).
pub fn open_modeling_menu(world: &mut World, at: Vec2, pick: Option<Pick>) {
    // A sketch curve under the pointer (P3.5: its own appearance): only where a sketch is what
    // was picked, or nothing.
    let curve = match pick {
        Some(Pick::Region(..)) | Some(Pick::Feature(_)) | Some(Pick::SketchCurve(..)) | None => {
            let offset = world.resource::<crate::viewport::ViewportRect>().offset(at);
            let view = world.resource::<ViewportView>().view;
            crate::parts::pick_sketch_curve(world.resource::<crate::parts::PartCache>(), &view, offset)
                .map(|(s, c, _)| MenuCurve(s, c))
        }
        _ => None,
    };
    let pick = match (pick, curve) {
        (None, Some(MenuCurve(s, _))) => Some(Pick::Feature(s)),
        (p, _) => p,
    };
    let target = match pick {
        Some(Pick::Plane(k)) => MenuTarget::Plane(k),
        Some(Pick::Face(f, tag)) => MenuTarget::Face(f, tag),
        Some(Pick::Region(s, _)) | Some(Pick::Feature(s)) | Some(Pick::SketchCurve(s, _)) | Some(Pick::SketchPoint(s, _)) => {
            MenuTarget::Sketch(s)
        }
        Some(Pick::Origin) | Some(Pick::Edge(..)) | Some(Pick::Vertex(..)) | Some(Pick::Part(_)) | Some(Pick::Assembly) | Some(Pick::Instance(..)) | None => {
            MenuTarget::Empty
        }
    };
    let planar_face = match target {
        MenuTarget::Face(f, tag) => world
            .get_resource::<ActiveDocument>()
            .and_then(|d| crate::parts::face_plane_of(d.active_element()?, f.feature, tag))
            .is_some(),
        _ => false,
    };
    // What was right-clicked is selected while its menu is open (it shows which plane, face or
    // sketch the menu is for).
    // A sketch is selected as a whole (its edges highlighted), however it was picked: a
    // region right-clicked does not stay selected as a region (no Area readout).
    // One curve right-clicked: only that curve is highlighted (P3.6; the menu's Edit curve
    // appearance… is for it).
    if let Some(p) = pick.filter(|p| *p != Pick::Origin) {
        let p = match (p, curve) {
            (Pick::Region(s, _) | Pick::Feature(s) | Pick::SketchCurve(s, _), Some(MenuCurve(cs, c))) if cs == s => {
                Pick::SketchCurve(s, c)
            }
            (Pick::Region(s, _), _) => Pick::Feature(s),
            (p, _) => p,
        };
        world.resource_mut::<Selection>().0 = vec![p];
    }
    let planes_shown = world.resource::<PlanesVisible>().any();
    let theme = world.resource::<Theme>().clone();
    let menu = Menu::new("viewport-context-menu").min_width(200.0).item_height(23.0);
    // P3F.2 judge: the face's and sketch's menus have an icon column (Export as DXF/DWG… with
    // the file-export icon); the plane's stays text only.
    let menu = match target {
        MenuTarget::Plane(k) => menu
            .text_only()
            .item(MenuItem::new("viewport-new-sketch", "New sketch"))
            .separator()
            .item(MenuItem::new("viewport-hide-plane", format!("Hide {} plane", k.name())))
            .separator()
            .item(MenuItem::new("viewport-normal-to", "View normal to plane"))
            .item(MenuItem::new("viewport-zoom-to-fit", "Zoom to fit")),
        MenuTarget::Face(..) => menu
            .item(MenuItem::new("viewport-new-sketch", "New sketch").icon("sketch").disabled(!planar_face))
            .separator()
            .item(MenuItem::new("viewport-face-appearance", "Add appearance to face…").icon("appearance"))
            .item(MenuItem::new("viewport-part-appearance", "Edit appearance…").icon("appearance"))
            .item(MenuItem::new("viewport-part-material", "Assign material…").icon("material-library"))
            .separator()
            // P3F.2 (P3.2): a flat face as DXF or DWG, for cutting machines.
            .item(MenuItem::new("viewport-export-face", "Export as DXF/DWG…").icon("file-export").disabled(!planar_face))
            .separator()
            .item(MenuItem::new("viewport-normal-to", "View normal to face").disabled(!planar_face))
            .item(MenuItem::new("viewport-zoom-to-fit", "Zoom to fit")),
        MenuTarget::Sketch(s) => {
            let name = feature_name(world, s);
            menu.item(MenuItem::new("viewport-edit-sketch", format!("Edit {name}…")).icon("edit"))
                .item(MenuItem::new("viewport-rename-sketch", "Rename"))
                .item(MenuItem::new("viewport-delete-sketch", "Delete"))
                .separator()
                .item(MenuItem::new("viewport-sketch-appearance", "Edit sketch appearance…").icon("appearance"))
                .item(
                    MenuItem::new("viewport-curve-appearance", "Edit curve appearance…")
                        .disabled(curve.is_none_or(|c| c.0 != s)),
                )
                .separator()
                .item(MenuItem::new("viewport-export-sketch", "Export as DXF/DWG…").icon("file-export"))
                .separator()
                .item(MenuItem::new("viewport-normal-to", "View normal to sketch plane"))
                .item(MenuItem::new("viewport-zoom-to-fit", "Zoom to fit"))
        }
        MenuTarget::Empty | MenuTarget::Sketching => menu
            .text_only()
            .item(MenuItem::new("viewport-zoom-to-fit", "Zoom to fit"))
            .item(MenuItem::new(
                "viewport-toggle-planes",
                if planes_shown { "Hide planes" } else { "Show planes" },
            ))
            .separator()
            .item(
                MenuItem::new("viewport-select", "Select")
                    .submenu(vec![MenuItem::new("viewport-create-selection", "Create selection…").into()]),
            ),
    };
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((target, DespawnOnExit(AppState::Document)));
    if let Some(c) = curve {
        commands.entity(anchor).insert(c);
    }
    world.flush();
}

/// Sketching: the sketch's own menu (off glyphs and dimensions).
pub fn open_sketch_menu(world: &mut World, at: Vec2) {
    let Some(s) = world.get_resource::<crate::sketch::SketchSession>() else {
        return;
    };
    let name = feature_name(world, s.feature);
    let theme = world.resource::<Theme>().clone();
    // Grouped like the dimension menu (`dimension/dimension-driven-01.png`).
    let menu = Menu::new("sketch-context-menu")
        .min_width(200.0)
        .item_height(23.0)
        .text_only()
        .item(MenuItem::new("sketch-menu-confirm", format!("Confirm {name}")))
        .separator()
        .item(MenuItem::new("sketch-menu-copy", "Copy sketch").disabled(true))
        .separator()
        .item(MenuItem::new("sketch-menu-show-all", "Show all"))
        .separator()
        .item(MenuItem::new("sketch-menu-select", "Select").disabled(true).submenu(vec![]))
        .item(MenuItem::new("sketch-menu-select-other", "Select other…").disabled(true))
        .separator()
        .item(MenuItem::new("sketch-menu-comment", "Add comment").disabled(true))
        .separator()
        .item(MenuItem::new("viewport-zoom-to-fit", "Zoom to fit"))
        .item(MenuItem::new("viewport-normal-to", "View normal to sketch plane"));
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((MenuTarget::Sketching, DespawnOnExit(AppState::Document)));
    world.flush();
}

/// The sketch curve a viewport menu was opened on (Edit curve appearance…).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct MenuCurve(FeatureId, cadrs_sketch::CurveId);

fn on_viewport_menu(ev: On<MenuAction>, q: Query<(&MenuTarget, Option<&MenuCurve>)>, mut commands: Commands) {
    let Ok((target, curve)) = q.get(ev.entity) else {
        return;
    };
    let (target, curve) = (*target, curve.copied());
    let item = ev.item.clone();
    if item == "viewport-curve-appearance"
        && let Some(MenuCurve(s, c)) = curve
    {
        commands.queue(move |world: &mut World| {
            crate::appearance::open_appearance_dialog(world, AppearanceTarget::Curve(s, c))
        });
        return;
    }
    commands.queue(move |world: &mut World| act(world, target, &item));
}

fn act(world: &mut World, target: MenuTarget, item: &str) {
    match (item, target) {
        ("viewport-new-sketch", MenuTarget::Plane(k)) => {
            world.resource_mut::<Selection>().0 = vec![Pick::Plane(k)];
            crate::sketch::begin_sketch(world);
        }
        ("viewport-new-sketch", MenuTarget::Face(f, tag)) => {
            world.resource_mut::<Selection>().0 = vec![Pick::Face(f, tag)];
            crate::sketch::begin_sketch(world);
        }
        ("viewport-hide-plane", MenuTarget::Plane(k)) => {
            world.resource_mut::<PlanesVisible>().set(k, false);
        }
        ("viewport-toggle-planes", _) => world.resource_mut::<PlanesVisible>().toggle_all(),
        ("viewport-create-selection", _) => crate::create_selection::open(world),
        ("viewport-zoom-to-fit", _) => crate::viewport::zoom_to_fit(world),
        ("viewport-normal-to", MenuTarget::Sketching) => crate::viewport::normal_to_sketch(world),
        ("viewport-normal-to", _) => {
            let normal = match target {
                MenuTarget::Plane(k) => Some(k.normal()),
                MenuTarget::Face(f, tag) => world
                    .get_resource::<ActiveDocument>()
                    .and_then(|d| crate::parts::face_plane_of(d.active_element()?, f.feature, tag))
                    .map(plane_normal),
                MenuTarget::Sketch(s) => world
                    .get_resource::<ActiveDocument>()
                    .and_then(|d| d.active_element()?.feature(s)?.sketch()?.plane)
                    .map(plane_normal),
                _ => None,
            };
            if let Some(n) = normal {
                let mut view = world.resource_mut::<ViewportView>();
                let to = view.target().normal_to(n);
                view.animate_to(to);
            }
        }
        ("viewport-edit-sketch", MenuTarget::Sketch(s)) => crate::document::edit_feature(world, s),
        ("viewport-rename-sketch", MenuTarget::Sketch(s)) => {
            crate::document::rename_feature(world, s)
        }
        ("viewport-delete-sketch", MenuTarget::Sketch(s)) => {
            crate::sketch::delete_features(world, &[s]);
            world
                .resource_mut::<Selection>()
                .0
                .retain(|p| !matches!(p, Pick::Feature(f) | Pick::Region(f, _) if *f == s));
        }
        ("viewport-face-appearance", MenuTarget::Face(part, face)) => {
            crate::appearance::open_appearance_dialog(world, AppearanceTarget::Faces(part, vec![face]));
        }
        ("viewport-part-appearance", MenuTarget::Face(part, _)) => {
            crate::appearance::open_appearance_dialog(world, AppearanceTarget::Parts(vec![part]));
        }
        ("viewport-part-material", MenuTarget::Face(part, _)) => {
            crate::material_dialog::open_material_dialog(world, vec![part]);
        }
        ("viewport-export-face", MenuTarget::Face(part, face)) => {
            crate::export_dialog::open(world, crate::export_dialog::ExportSource::Face(part, face));
        }
        ("viewport-export-sketch", MenuTarget::Sketch(s)) => {
            crate::export_dialog::open(world, crate::export_dialog::ExportSource::Sketch(s));
        }
        ("viewport-sketch-appearance", MenuTarget::Sketch(s)) => {
            crate::appearance::open_appearance_dialog(world, AppearanceTarget::Sketch(s));
        }
        ("sketch-menu-confirm", _) => crate::sketch::accept_sketch(world),
        ("sketch-menu-show-all", _) => {
            world
                .resource_mut::<crate::sketch::SketchViewSettings>()
                .show_constraints = true;
        }
        _ => {}
    }
}
