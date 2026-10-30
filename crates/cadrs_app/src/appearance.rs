//! Appearances (PS9, X9, P3.5): the Edit appearance dialog.
//!
//! - **Edit appearance** (a part's context menu in the Parts list or the view; "Add appearance to
//!   face" on a face; "Add appearance to feature" and "Edit sketch appearance" on a feature-list
//!   row or a sketch in the view): a dialog at the top left of the viewport, like the feature
//!   dialogs, with the preset **swatches** (the part palette first), the document's **custom
//!   colours** (**+** saves the current colour; right-click one to **Delete** it or **Update
//!   color** to the current one), the **mixer** (a saturation/value square and a hue strip), the
//!   **Hex** code and **R G B** values, and an **Opacity** slider (PS9.3). The view shows the
//!   colour live; ✓ applies it as one undoable step, ✕ (or Esc) leaves things as they were.
//!   **Default** takes the appearance off (a part goes back to its palette colour).
//! - The Parts list's context menu has the 8 palette colours as swatches too (one click).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::appearance::{self, Appearance, SWATCHES};
use cadrs_core::commands::{
    SetCurveAppearance, SetCustomColors, SetFaceAppearance, SetFeatureAppearance, SetPartAppearance,
};
use cadrs_core::{ElementId, FeatureId, PartId};
use cadrs_sketch::{CurveId, FaceName};
use cadrs_ui::ShortcutKey;
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    ColorEditor, ColorEditorChange, ColorEditorCustom, ColorEditorDefault, ColorEditorSaveCustom,
    ColorEditorState, CustomColorSwatch, FeatureDialogAccept, FeatureDialogCancel,
};

use crate::parts::PartCache;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct AppearancePlugin;

impl Plugin for AppearancePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (appearance_keys, sync_appearance_dialog, sync_custom_colors)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
            commands.remove_resource::<AppearanceSession>();
        })
        .add_observer(on_color)
        .add_observer(on_default)
        .add_observer(on_save_custom)
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_custom_menu)
        .add_observer(on_custom_menu_action);
    }
}

/// What an appearance applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppearanceTarget {
    Parts(Vec<PartId>),
    Faces(PartId, Vec<FaceName>),
    /// A part feature: the faces it made (PS9.4).
    Feature(FeatureId),
    /// A sketch: its curves (PS9.5).
    Sketch(FeatureId),
    /// One curve of a sketch (PS9.5).
    Curve(FeatureId, CurveId),
}

/// The appearance settings of a Part Studio (the part props, the features' and sketches', the
/// single curves'), as the live preview edits copies of them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Looks {
    pub props: Vec<cadrs_core::PartProps>,
    pub features: Vec<(FeatureId, Appearance)>,
    pub curves: Vec<(FeatureId, CurveId, Appearance)>,
}

/// The grey of a sketch that is not being edited (`sketch_draw::accepted_edge`), its colour
/// until it gets an appearance.
pub const SKETCH_GREY: Appearance = Appearance::rgb(0xa8, 0xac, 0xaf);

/// Applies `a` to `target` in copies of a Part Studio's appearance settings (the live preview).
pub fn apply_to(target: &AppearanceTarget, a: Option<Appearance>, looks: &mut Looks) {
    let Looks {
        props,
        features,
        curves,
    } = looks;
    let prop = |props: &mut Vec<cadrs_core::PartProps>, part: PartId| -> usize {
        if let Some(i) = props.iter().position(|p| p.part == part) {
            return i;
        }
        props.push(cadrs_core::PartProps::new(part));
        props.len() - 1
    };
    match target {
        AppearanceTarget::Parts(parts) => {
            for p in parts {
                let i = prop(props, *p);
                props[i].appearance = a;
            }
        }
        AppearanceTarget::Faces(part, faces) => {
            let i = prop(props, *part);
            props[i].faces.retain(|(f, _)| !faces.contains(f));
            if let Some(a) = a {
                props[i].faces.extend(faces.iter().map(|f| (*f, a)));
            }
        }
        AppearanceTarget::Feature(f) | AppearanceTarget::Sketch(f) => {
            features.retain(|(x, _)| x != f);
            if let Some(a) = a {
                features.push((*f, a));
            }
        }
        AppearanceTarget::Curve(s, c) => {
            curves.retain(|(x, y, _)| !(x == s && y == c));
            if let Some(a) = a {
                curves.push((*s, *c, a));
            }
        }
    }
}

/// The open Edit appearance dialog: what it edits and the colour so far.
#[derive(Resource, Debug, Clone)]
pub struct AppearanceSession {
    pub element: ElementId,
    pub target: AppearanceTarget,
    /// The colour and opacity chosen so far (shown live).
    pub color: Appearance,
    /// Default was pressed: ✓ takes the appearance off.
    pub reset: bool,
    title: String,
    caption: String,
}

#[derive(Component)]
struct AppearanceDialog;

/// The appearance a target has now.
fn current(world: &World, element: ElementId, target: &AppearanceTarget) -> Appearance {
    let cache = world.resource::<PartCache>();
    let el = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element).cloned());
    let features = el
        .as_ref()
        .map(|e| e.feature_appearances().to_vec())
        .unwrap_or_default();
    match target {
        AppearanceTarget::Parts(parts) => parts
            .first()
            .and_then(|p| cache.part(*p))
            .map(|p| appearance::part_appearance(p, &cache.props))
            .unwrap_or(appearance::PALETTE[0]),
        AppearanceTarget::Faces(part, faces) => cache
            .part(*part)
            .zip(faces.first())
            .map(|(p, f)| appearance::face_appearance(p, f, &cache.props, &features).0)
            .unwrap_or(appearance::PALETTE[0]),
        AppearanceTarget::Feature(f) => features
            .iter()
            .find(|(x, _)| x == f)
            .map(|(_, a)| *a)
            // Its first part's palette colour (opaque) until it has one of its own.
            .or_else(|| {
                cache
                    .part_of_feature(*f)
                    .map(|p| appearance::palette(p.palette))
            })
            .unwrap_or(appearance::PALETTE[0]),
        AppearanceTarget::Sketch(f) => features
            .iter()
            .find(|(x, _)| x == f)
            .map(|(_, a)| *a)
            .unwrap_or(SKETCH_GREY),
        AppearanceTarget::Curve(s, c) => el
            .as_ref()
            .and_then(|e| {
                e.curve_appearances()
                    .iter()
                    .find(|(x, y, _)| x == s && y == c)
                    .map(|(_, _, a)| *a)
            })
            .or_else(|| features.iter().find(|(x, _)| x == s).map(|(_, a)| *a))
            .unwrap_or(SKETCH_GREY),
    }
}

/// The dialog's title and the line under it naming what it edits.
fn describe(world: &World, element: ElementId, target: &AppearanceTarget) -> (String, String) {
    let cache = world.resource::<PartCache>();
    let feature_name = |f: FeatureId| {
        world
            .get_resource::<ActiveDocument>()
            .and_then(|d| d.doc.element(element)?.feature(f).map(|x| x.name.clone()))
            .unwrap_or_default()
    };
    match target {
        AppearanceTarget::Parts(parts) => {
            let names: Vec<String> = parts
                .iter()
                .filter_map(|p| cache.part_name(*p).map(str::to_string))
                .collect();
            ("Edit appearance".into(), names.join(", "))
        }
        AppearanceTarget::Faces(part, faces) => {
            let n = faces.len();
            let name = cache.part_name(*part).unwrap_or("part").to_string();
            (
                "Face appearance".into(),
                if n == 1 {
                    format!("Face of {name}")
                } else {
                    format!("{n} faces of {name}")
                },
            )
        }
        AppearanceTarget::Feature(f) => ("Feature appearance".into(), feature_name(*f)),
        AppearanceTarget::Sketch(f) => ("Sketch appearance".into(), feature_name(*f)),
        AppearanceTarget::Curve(s, _) => (
            "Curve appearance".into(),
            format!("Curve of {}", feature_name(*s)),
        ),
    }
}

/// Opens the Edit appearance dialog for `target` in the active Part Studio (closing the Mass
/// properties panel, which sits in the same place).
pub fn open_appearance_dialog(world: &mut World, target: AppearanceTarget) {
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().map(|e| e.id))
    else {
        return;
    };
    close_dialogs(world);
    world.remove_resource::<crate::mass_props::MassPanel>();
    let color = current(world, element, &target);
    let (title, caption) = describe(world, element, &target);
    world.insert_resource(AppearanceSession {
        element,
        target,
        color,
        reset: false,
        title,
        caption,
    });
}

/// Closes the appearance and material dialogs without applying anything.
pub fn close_dialogs(world: &mut World) {
    world.remove_resource::<AppearanceSession>();
    crate::material_dialog::close(world);
    world
        .resource_mut::<PartCache>()
        .set_appearance_preview(None);
}

/// The target's live preview in the view while the dialog is open.
fn preview(world: &mut World) {
    let p = world
        .get_resource::<AppearanceSession>()
        .map(|s| (s.target.clone(), if s.reset { None } else { Some(s.color) }));
    world.resource_mut::<PartCache>().set_appearance_preview(p);
}

fn with_session(commands: &mut Commands, f: impl FnOnce(&mut AppearanceSession) + Send + 'static) {
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<AppearanceSession>() {
            f(&mut s);
            s.reset = false;
        }
        preview(world);
    });
}

/// A colour picked in the dialog's editor shows live.
fn on_color(ev: On<ColorEditorChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity)
        .is_ok_and(|n| n.as_str() == "appearance-editor")
    {
        let [r, g, b, a] = ev.rgba;
        with_session(&mut commands, move |s| {
            s.color = Appearance {
                rgb: [r, g, b],
                alpha: a,
            }
        });
    }
}

/// **+** in any colour editor saves its colour among the document's custom colours (one undo
/// step).
fn on_save_custom(ev: On<ColorEditorSaveCustom>, mut commands: Commands) {
    let c = Appearance::rgb(ev.rgb[0], ev.rgb[1], ev.rgb[2]);
    commands.queue(move |world: &mut World| {
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
            let mut colors = doc.doc.custom_colors.clone();
            if !colors.contains(&c) {
                colors.push(c);
                let _ = doc.execute(&SetCustomColors { colors });
            }
        }
    });
}

/// Every colour editor shows the document's custom colours.
fn sync_custom_colors(doc: Option<Res<ActiveDocument>>, mut q: Query<&mut ColorEditorCustom>) {
    let custom: Vec<[u8; 3]> = doc
        .map(|d| d.doc.custom_colors.iter().map(|c| c.rgb).collect())
        .unwrap_or_default();
    for mut c in &mut q {
        if c.0 != custom {
            c.0 = custom.clone();
        }
    }
}

/// ✓: the colour becomes the target's appearance (one undo step).
fn on_accept(
    ev: On<FeatureDialogAccept>,
    q: Query<(), With<AppearanceDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

pub fn accept(world: &mut World) {
    let Some(s) = world.remove_resource::<AppearanceSession>() else {
        return;
    };
    world
        .resource_mut::<PartCache>()
        .set_appearance_preview(None);
    let a = (!s.reset).then_some(s.color);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let element = s.element;
    let r = match s.target {
        AppearanceTarget::Parts(parts) => doc.execute(&SetPartAppearance {
            element,
            parts,
            appearance: a,
        }),
        AppearanceTarget::Faces(part, faces) => doc.execute(&SetFaceAppearance {
            element,
            part,
            faces,
            appearance: a,
        }),
        AppearanceTarget::Feature(feature) => doc.execute(&SetFeatureAppearance {
            element,
            feature,
            appearance: a,
            label: "Feature appearance".into(),
        }),
        AppearanceTarget::Sketch(feature) => doc.execute(&SetFeatureAppearance {
            element,
            feature,
            appearance: a,
            label: "Sketch appearance".into(),
        }),
        AppearanceTarget::Curve(sketch, curve) => doc.execute(&SetCurveAppearance {
            element,
            sketch,
            curve,
            appearance: a,
        }),
    };
    if let Err(e) = r {
        warn!("appearance: {e}");
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<AppearanceDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(close_dialogs);
    }
}

fn appearance_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<AppearanceSession>>,
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
        if k.state == ButtonState::Pressed && k.shortcut_code() == KeyCode::Escape {
            commands.queue(close_dialogs);
        }
    }
}

/// **Default** takes the appearance off.
fn on_default(ev: On<ColorEditorDefault>, q: Query<&Name>, mut commands: Commands) {
    if !q
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == "appearance-editor")
    {
        return;
    }
    commands.queue(|world: &mut World| {
        let default = world.get_resource::<AppearanceSession>().map(|s| {
            let mut looks = world.resource::<PartCache>().looks();
            apply_to(&s.target, None, &mut looks);
            (s.element, s.target.clone(), looks.props, looks.features)
        });
        let Some((element, target, props, feats)) = default else {
            return;
        };
        // The colour it goes back to.
        let color = {
            let cache = world.resource::<PartCache>();
            match &target {
                AppearanceTarget::Parts(p) => p
                    .first()
                    .and_then(|p| cache.part(*p))
                    .map(|p| appearance::part_appearance(p, &props)),
                AppearanceTarget::Faces(p, f) => cache
                    .part(*p)
                    .zip(f.first())
                    .map(|(p, f)| appearance::face_appearance(p, f, &props, &feats).0),
                AppearanceTarget::Feature(f) => cache
                    .part_of_feature(*f)
                    .map(|p| appearance::part_appearance(p, &props)),
                AppearanceTarget::Sketch(_) => Some(SKETCH_GREY),
                AppearanceTarget::Curve(s, _) => Some(
                    feats
                        .iter()
                        .find(|(x, _)| x == s)
                        .map(|(_, a)| *a)
                        .unwrap_or(SKETCH_GREY),
                ),
            }
        };
        let _ = element;
        if let Some(mut s) = world.get_resource_mut::<AppearanceSession>() {
            if let Some(c) = color {
                s.color = c;
            }
            s.reset = true;
        }
        preview(world);
    });
}

/// The menu of a custom colour.
#[derive(Component, Debug, Clone, Copy)]
struct CustomMenuFor(CustomColorSwatch);

fn on_custom_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&CustomColorSwatch>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(c) = q.get(ev.entity) else {
        return;
    };
    // It opens down from the pointer, as a context menu does, so the swatch rows and the
    // "Custom" label above stay in view (P3.6).
    let menu = Menu::new("custom-color-menu")
        .min_width(140.0)
        .item_height(22.0)
        .text_only()
        .item(MenuItem::new("custom-color-update", "Update color"))
        .item(MenuItem::new("custom-color-delete", "Delete"));
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((CustomMenuFor(*c), DespawnOnExit(AppState::Document)));
}

fn on_custom_menu_action(
    ev: On<MenuAction>,
    q: Query<&CustomMenuFor, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(target) = q.get(ev.entity) else {
        return;
    };
    let (i, editor) = (target.0.index, target.0.editor);
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| {
        // Update takes the colour of the editor the swatch is in.
        let current = world
            .get(editor)
            .map(|s: &ColorEditorState| Appearance::rgb(s.rgba[0], s.rgba[1], s.rgba[2]));
        let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
            return;
        };
        let mut colors = doc.doc.custom_colors.clone();
        if i >= colors.len() {
            return;
        }
        match item.as_str() {
            "custom-color-delete" => {
                colors.remove(i);
            }
            "custom-color-update" => match current {
                Some(c) => colors[i] = c,
                None => return,
            },
            _ => return,
        }
        let _ = doc.execute(&SetCustomColors { colors });
    });
}

fn appearance_dialog(t: &Theme, s: &AppearanceSession, custom: Vec<Appearance>) -> impl Bundle {
    let tb = t.clone();
    let color = s.color;
    let caption = s.caption.clone();
    (
        AppearanceDialog,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("appearance-dialog")
            .title(s.title.clone())
            .valid(true)
            .width(292.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|c| {
                    c.spawn((
                        Name::new("appearance-target"),
                        t.text(caption.clone(), 12.0, FontWeight::MEDIUM, t.foreground),
                    ));
                    c.spawn(
                        ColorEditor::new("appearance")
                            .presets(SWATCHES.iter().map(|a| a.rgb))
                            .custom(custom.iter().map(|a| a.rgb).collect())
                            .color([color.rgb[0], color.rgb[1], color.rgb[2], color.alpha])
                            .opacity()
                            .default_button()
                            .build(t),
                    );
                });
            })
            .build(t),
    )
}

/// Spawns, updates and removes the dialog with the session; its editor follows the session's
/// colour (Default puts it back).
fn sync_appearance_dialog(
    session: Option<Res<AppearanceSession>>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_dialog: Query<Entity, With<AppearanceDialog>>,
    mut q_editor: Query<(&Name, &mut ColorEditorState)>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for e in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if q_dialog.is_empty() {
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let custom = doc
            .as_ref()
            .map(|d| d.doc.custom_colors.clone())
            .unwrap_or_default();
        let d = commands.spawn(appearance_dialog(&theme, &s, custom)).id();
        commands.entity(area).add_child(d);
        return;
    }
    if !s.is_changed() {
        return;
    }
    let c = s.color;
    let want = [c.rgb[0], c.rgb[1], c.rgb[2], c.alpha];
    for (n, mut st) in &mut q_editor {
        if n.as_str() == "appearance-editor" && st.rgba != want {
            st.rgba = want;
        }
    }
}
