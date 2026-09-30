//! Appearances (PS9, X9, P3.5): the Edit appearance dialog and the Appearances panel.
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
//! - **Appearances panel** (the palette icon on the viewport's right edge, PS9.7, PS2.10): the
//!   appearances of the parts (and their faces), features and sketches of the Part Studio, one
//!   row each with its swatch; double-click a row, or right-click → Edit appearance, to change
//!   it.

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
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    ColorMixer, ColorMixerChange, ColorMixerState, ColorSwatch, FeatureDialogAccept, FeatureDialogCancel,
    NumberField, NumberFieldCommit, NumberFieldState, Slider, SliderChange, SliderState, SwatchColor,
};

use crate::parts::PartCache;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct AppearancePlugin;

impl Plugin for AppearancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SidePanel>()
            .add_systems(
                Update,
                (appearance_keys, sync_appearance_dialog, sync_appearance_panel, sync_variables_panel, sync_custom_tables_panel, sync_panel_buttons)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| {
                commands.remove_resource::<AppearanceSession>();
            })
            .add_observer(on_swatch)
            .add_observer(on_mixer)
            .add_observer(on_field_commit)
            .add_observer(on_opacity)
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_button)
            .add_observer(on_custom_menu)
            .add_observer(on_custom_menu_action)
            .add_observer(on_panel_button)
            .add_observer(on_panel_row_double_click)
            .add_observer(on_panel_row_menu)
            .add_observer(on_panel_menu_action);
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
    let Looks { props, features, curves } = looks;
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

/// A preset swatch.
#[derive(Component, Debug, Clone, Copy)]
struct Preset;

/// A custom colour's swatch (its index in the document's list).
#[derive(Component, Debug, Clone, Copy)]
struct Custom(usize);

/// The custom colours' row.
#[derive(Component)]
struct CustomRow;

/// The swatch showing the current colour.
#[derive(Component)]
struct CurrentColor;

/// The opacity readout ("100%").
#[derive(Component)]
struct OpacityText;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Hex,
    R,
    G,
    B,
}

/// The appearance a target has now.
fn current(world: &World, element: ElementId, target: &AppearanceTarget) -> Appearance {
    let cache = world.resource::<PartCache>();
    let el = world.get_resource::<ActiveDocument>().and_then(|d| d.doc.element(element).cloned());
    let features = el.as_ref().map(|e| e.feature_appearances().to_vec()).unwrap_or_default();
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
            .or_else(|| cache.part_of_feature(*f).map(|p| appearance::palette(p.palette)))
            .unwrap_or(appearance::PALETTE[0]),
        AppearanceTarget::Sketch(f) => features.iter().find(|(x, _)| x == f).map(|(_, a)| *a).unwrap_or(SKETCH_GREY),
        AppearanceTarget::Curve(s, c) => el
            .as_ref()
            .and_then(|e| e.curve_appearances().iter().find(|(x, y, _)| x == s && y == c).map(|(_, _, a)| *a))
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
            let names: Vec<String> = parts.iter().filter_map(|p| cache.part_name(*p).map(str::to_string)).collect();
            ("Edit appearance".into(), names.join(", "))
        }
        AppearanceTarget::Faces(part, faces) => {
            let n = faces.len();
            let name = cache.part_name(*part).unwrap_or("part").to_string();
            (
                "Face appearance".into(),
                if n == 1 { format!("Face of {name}") } else { format!("{n} faces of {name}") },
            )
        }
        AppearanceTarget::Feature(f) => ("Feature appearance".into(), feature_name(*f)),
        AppearanceTarget::Sketch(f) => ("Sketch appearance".into(), feature_name(*f)),
        AppearanceTarget::Curve(s, _) => ("Curve appearance".into(), format!("Curve of {}", feature_name(*s))),
    }
}

/// Opens the Edit appearance dialog for `target` in the active Part Studio (closing the Mass
/// properties panel, which sits in the same place).
pub fn open_appearance_dialog(world: &mut World, target: AppearanceTarget) {
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element().map(|e| e.id)) else {
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
    world.resource_mut::<PartCache>().set_appearance_preview(None);
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

/// A preset or custom swatch.
type SwatchFilter = Or<(With<Preset>, With<Custom>)>;

fn on_swatch(
    a: On<Activate>,
    q: Query<&SwatchColor, SwatchFilter>,
    button: Res<cadrs_ui::menu::LastPointerButton>,
    mut commands: Commands,
) {
    let Ok(c) = q.get(a.entity) else {
        return;
    };
    // A right-click on a custom colour opens its menu; it doesn't pick the colour.
    if button.0 == bevy::picking::pointer::PointerButton::Secondary {
        return;
    }
    let s = c.0.to_srgba();
    let rgb = [s.red, s.green, s.blue].map(|v| (v * 255.0).round() as u8);
    with_session(&mut commands, move |s| s.color.rgb = rgb);
}

fn on_mixer(ev: On<ColorMixerChange>, mut commands: Commands) {
    let c = Appearance::from_hsv(ev.hue, ev.saturation, ev.value);
    with_session(&mut commands, move |s| s.color.rgb = c.rgb);
}

fn on_field_commit(ev: On<NumberFieldCommit>, q: Query<&Field>, mut commands: Commands) {
    let Ok(field) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.clone();
    let enter = ev.enter;
    commands.queue(move |world: &mut World| {
        // Enter ends the edit: the field lets go of the focus and its caret (P3.5 judge).
        if enter && let Some(mut f) = world.get_resource_mut::<bevy::input_focus::InputFocus>() {
            f.clear();
        }
        let Some(mut s) = world.get_resource_mut::<AppearanceSession>() else {
            return;
        };
        match field {
            Field::Hex => {
                if let Some(a) = Appearance::from_hex(&text) {
                    s.color.rgb = a.rgb;
                    s.reset = false;
                }
            }
            Field::R | Field::G | Field::B => {
                if let Ok(v) = text.trim().parse::<f64>() {
                    let i = match field {
                        Field::R => 0,
                        Field::G => 1,
                        _ => 2,
                    };
                    s.color.rgb[i] = v.round().clamp(0.0, 255.0) as u8;
                    s.reset = false;
                }
            }
        }
        // Invalid text goes back to the current value.
        s.set_changed();
        preview(world);
    });
}

fn on_opacity(ev: On<SliderChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "appearance-opacity") {
        let alpha = (ev.value * 255.0).round() as u8;
        with_session(&mut commands, move |s| s.color.alpha = alpha);
    }
}

/// ✓: the colour becomes the target's appearance (one undo step).
fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<AppearanceDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

pub fn accept(world: &mut World) {
    let Some(s) = world.remove_resource::<AppearanceSession>() else {
        return;
    };
    world.resource_mut::<PartCache>().set_appearance_preview(None);
    let a = (!s.reset).then_some(s.color);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let element = s.element;
    let r = match s.target {
        AppearanceTarget::Parts(parts) => doc.execute(&SetPartAppearance { element, parts, appearance: a }),
        AppearanceTarget::Faces(part, faces) => doc.execute(&SetFaceAppearance { element, part, faces, appearance: a }),
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
        AppearanceTarget::Curve(sketch, curve) => doc.execute(&SetCurveAppearance { element, sketch, curve, appearance: a }),
    };
    if let Err(e) = r {
        warn!("appearance: {e}");
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<AppearanceDialog>>, mut commands: Commands) {
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
        if k.state == ButtonState::Pressed && k.key_code == KeyCode::Escape {
            commands.queue(close_dialogs);
        }
    }
}

/// **+** saves the current colour; **Default** takes the appearance off.
fn on_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else {
        return;
    };
    match name.as_str() {
        "appearance-add-custom" => commands.queue(|world: &mut World| {
            let Some(c) = world.get_resource::<AppearanceSession>().map(|s| s.color.with_alpha(255)) else {
                return;
            };
            if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
                let mut colors = doc.doc.custom_colors.clone();
                if !colors.contains(&c) {
                    colors.push(c);
                    let _ = doc.execute(&SetCustomColors { colors });
                }
            }
        }),
        "appearance-reset" => commands.queue(|world: &mut World| {
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
                    AppearanceTarget::Feature(f) => cache.part_of_feature(*f).map(|p| appearance::part_appearance(p, &props)),
                    AppearanceTarget::Sketch(_) => Some(SKETCH_GREY),
                    AppearanceTarget::Curve(s, _) => {
                        Some(feats.iter().find(|(x, _)| x == s).map(|(_, a)| *a).unwrap_or(SKETCH_GREY))
                    }
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
        }),
        _ => {}
    }
}

/// The menu of a custom colour.
#[derive(Component, Debug, Clone, Copy)]
struct CustomMenuFor(usize);

fn on_custom_menu(ev: On<ContextMenuRequested>, q: Query<&Custom>, theme: Res<Theme>, mut commands: Commands) {
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
    commands.entity(anchor).insert((CustomMenuFor(c.0), DespawnOnExit(AppState::Document)));
}

fn on_custom_menu_action(ev: On<MenuAction>, q: Query<&CustomMenuFor, With<ContextMenuAnchor>>, mut commands: Commands) {
    let Ok(target) = q.get(ev.entity) else {
        return;
    };
    let i = target.0;
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| {
        let current = world.get_resource::<AppearanceSession>().map(|s| s.color.with_alpha(255));
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

fn section(p: &mut ChildSpawner, t: &Theme, label: &str) {
    p.spawn((
        t.text(label.to_string(), 11.0, FontWeight::MEDIUM, t.muted_foreground),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ));
}

fn srgb(a: Appearance) -> Color {
    Color::srgb_u8(a.rgb[0], a.rgb[1], a.rgb[2])
}

fn spawn_custom_row(r: &mut ChildSpawner, t: &Theme, custom: &[Appearance], color: Appearance) {
    for (i, c) in custom.iter().enumerate() {
        r.spawn((
            ColorSwatch::new(format!("appearance-custom-{i}"), srgb(*c))
                .size(20.0)
                .selected(c.rgb == color.rgb)
                .tooltip(c.hex())
                .build(t),
            Custom(i),
            ContextMenuTarget,
        ));
    }
    r.spawn(
        IconButton::new("appearance-add-custom", "plus")
            .tooltip("Save this color")
            .build(t),
    );
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
                    section(c, t, "Colors");
                    for row in SWATCHES.chunks(8).enumerate() {
                        c.spawn(Node {
                            column_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|r| {
                            for (k, a) in row.1.iter().enumerate() {
                                let i = row.0 * 8 + k;
                                r.spawn((
                                    ColorSwatch::new(format!("appearance-swatch-{i}"), srgb(*a))
                                        .size(24.0)
                                        .selected(a.rgb == color.rgb)
                                        .tooltip(a.hex())
                                        .build(t),
                                    Preset,
                                ));
                            }
                        });
                    }
                    section(c, t, "Custom colors");
                    c.spawn((
                        Name::new("appearance-custom-row"),
                        CustomRow,
                        Node {
                            column_gap: Val::Px(6.0),
                            align_items: AlignItems::Center,
                            min_height: Val::Px(24.0),
                            ..default()
                        },
                    ))
                    .with_children(|r| spawn_custom_row(r, t, &custom, color));
                    section(c, t, "Mixer");
                    let [h, sat, v] = color.hsv();
                    c.spawn(ColorMixer::new("appearance-mixer").size(270.0, 96.0).hsv(h, sat, v).build(t));
                    c.spawn(Node {
                        column_gap: Val::Px(8.0),
                        align_items: AlignItems::Center,
                        margin: UiRect::top(Val::Px(2.0)),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            Name::new("appearance-current"),
                            CurrentColor,
                            Node {
                                width: Val::Px(30.0),
                                height: Val::Px(30.0),
                                flex_shrink: 0.0,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(srgb(color).with_alpha(color.alpha as f32 / 255.0)),
                            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                        ));
                        r.spawn((
                            NumberField::new("appearance-hex", "Hex").label_width(26.0).text(color.hex()).build(t),
                            Field::Hex,
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| n.width = Val::Px(110.0));
                    });
                    c.spawn(Node {
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|r| {
                        for (name, label, f, v) in [
                            ("appearance-r", "R", Field::R, color.rgb[0]),
                            ("appearance-g", "G", Field::G, color.rgb[1]),
                            ("appearance-b", "B", Field::B, color.rgb[2]),
                        ] {
                            r.spawn((NumberField::new(name, label).label_width(12.0).text(v.to_string()).build(t), f))
                                .entry::<Node>()
                                .and_modify(|mut n| {
                                    n.flex_grow = 1.0;
                                    n.flex_basis = Val::Px(0.0);
                                });
                        }
                    });
                    c.spawn(Node {
                        column_gap: Val::Px(8.0),
                        align_items: AlignItems::Center,
                        height: Val::Px(24.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text("Opacity", 11.0, FontWeight::NORMAL, Color::srgb_u8(0x6b, 0x6b, 0x6b)),
                            Node {
                                width: Val::Px(52.0),
                                ..default()
                            },
                        ));
                        r.spawn(
                            Slider::new("appearance-opacity")
                                .value(color.alpha as f32 / 255.0)
                                .width(160.0)
                                .tooltip("Opacity")
                                .build(t),
                        );
                        r.spawn((
                            Name::new("appearance-opacity-value"),
                            OpacityText,
                            t.text(format!("{}%", color.opacity_percent()), 12.0, FontWeight::MEDIUM, t.foreground),
                        ));
                    });
                    c.spawn(Node {
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_child(
                        cadrs_ui::Button::new("appearance-reset")
                            .label("Default")
                            .size(ButtonSize::Small)
                            .build(t),
                    );
                });
            })
            .build(t),
    )
}

/// Spawns, updates and removes the dialog with the session.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_appearance_dialog(
    session: Option<Res<AppearanceSession>>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_dialog: Query<Entity, With<AppearanceDialog>>,
    mut q_fields: Query<(&Field, &mut NumberFieldState)>,
    mut q_mixer: Query<&mut ColorMixerState>,
    mut q_slider: Query<(&Name, &mut SliderState)>,
    mut q_current: Query<&mut BackgroundColor, With<CurrentColor>>,
    mut q_opacity: Query<&mut Text, With<OpacityText>>,
    mut q_swatches: Query<(&SwatchColor, &mut cadrs_ui::SwatchSelected), Or<(With<Preset>, With<Custom>)>>,
    q_custom_row: Query<Entity, With<CustomRow>>,
    mut last_custom: Local<Vec<Appearance>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for e in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let custom = doc.as_ref().map(|d| d.doc.custom_colors.clone()).unwrap_or_default();
    if q_dialog.is_empty() {
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let d = commands.spawn(appearance_dialog(&theme, &s, custom.clone())).id();
        commands.entity(area).add_child(d);
        *last_custom = custom;
        return;
    }
    let c = s.color;
    // The custom colours' row follows the document's list.
    if *last_custom != custom
        && let Some(row) = q_custom_row.iter().next()
    {
        let t = theme.clone();
        let list = custom.clone();
        commands.entity(row).despawn_children();
        commands.queue(move |world: &mut World| {
            if let Ok(mut e) = world.get_entity_mut(row) {
                e.with_children(|r| spawn_custom_row(r, &t, &list, c));
            }
        });
        *last_custom = custom;
    }
    if !s.is_changed() {
        return;
    }
    for (f, mut st) in &mut q_fields {
        let text = match f {
            Field::Hex => c.hex(),
            Field::R => c.rgb[0].to_string(),
            Field::G => c.rgb[1].to_string(),
            Field::B => c.rgb[2].to_string(),
        };
        if st.text != text {
            st.text = text;
        } else {
            // Rewrite the shown text (an invalid entry goes back).
            st.set_changed();
        }
    }
    let [h, sat, v] = c.hsv();
    for mut m in &mut q_mixer {
        // Keep the hue while the colour is grey (it has none of its own).
        let hue = if sat == 0.0 || v == 0.0 { m.hue } else { h };
        let want = ColorMixerState { hue, saturation: sat, value: v };
        let close = (m.hue - want.hue).abs() < 0.6 && (m.saturation - sat).abs() < 0.003 && (m.value - v).abs() < 0.003;
        if !close {
            *m = want;
        }
    }
    for (n, mut sl) in &mut q_slider {
        if n.as_str() == "appearance-opacity" {
            let want = c.alpha as f32 / 255.0;
            if (sl.value - want).abs() > 0.003 {
                sl.value = want;
            }
        }
    }
    for mut bg in &mut q_current {
        bg.0 = srgb(c).with_alpha(c.alpha as f32 / 255.0);
    }
    for mut t in &mut q_opacity {
        t.0 = format!("{}%", c.opacity_percent());
    }
    // The ring follows the colour.
    for (sc, mut sel) in &mut q_swatches {
        let want = sc.0.to_srgba().to_u8_array()[..3] == c.rgb;
        if sel.0 != want {
            sel.0 = want;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Appearances panel

/// The side panel open on the viewport's right (PS2.10): the Appearances panel (PS9.7), the
/// Variables panel (a placeholder until variables exist, P3F.4), the Custom tables panel (P3.11)
/// or, in an assembly, the Bill of Materials (P3B.6, [`crate::assembly::bom_panel`]). One at a
/// time.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidePanel {
    #[default]
    None,
    Appearances,
    Variables,
    /// P3.11 (PS2.10): the Custom tables panel.
    CustomTables,
    Bom,
    /// P3B.8: an assembly's Named positions ([`crate::assembly::named_positions`]).
    NamedPositions,
    /// P3B.8: an assembly's Exploded views ([`crate::assembly::explode`]).
    ExplodedViews,
    /// P3F.5: the Simulation panel ([`crate::simulation_ui`]).
    Simulation,
}

#[derive(Component)]
struct VariablesPanel;

#[derive(Component)]
struct CustomTablesPanel;

#[derive(Component)]
struct AppearancePanel;

/// A row of the panel: what it edits.
#[derive(Component, Debug, Clone)]
struct PanelRow(AppearanceTarget);

fn on_panel_button(a: On<Activate>, q: Query<&Name>, mut open: ResMut<SidePanel>) {
    let Ok(n) = q.get(a.entity) else {
        return;
    };
    let toggle = |open: &mut SidePanel, p: SidePanel| *open = if *open == p { SidePanel::None } else { p };
    match n.as_str() {
        "panel-appearance" => toggle(&mut open, SidePanel::Appearances),
        "panel-variables" => toggle(&mut open, SidePanel::Variables),
        "panel-custom-tables" => toggle(&mut open, SidePanel::CustomTables),
        "panel-bom" | "assembly-bom" => toggle(&mut open, SidePanel::Bom),
        "panel-named-positions" => toggle(&mut open, SidePanel::NamedPositions),
        "panel-exploded-views" => toggle(&mut open, SidePanel::ExplodedViews),
        "appearance-panel-close"
        | "variables-panel-close"
        | "custom-tables-panel-close"
        | "bom-panel-close"
        | "named-positions-panel-close"
        | "exploded-views-panel-close" => *open = SidePanel::None,
        _ => {}
    }
}

/// The side panels' frame: docked full height on the viewport's right, beside it rather than
/// over it, so the graphics area (and the view cube in its corner) shrinks to make room (P3.6,
/// as Onshape's).
pub(crate) fn side_panel_node(t: &Theme) -> impl Bundle {
    (
        Node {
            width: Val::Px(240.0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            border: UiRect::new(Val::Px(1.0), Val::Px(1.0), Val::ZERO, Val::ZERO),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(t.background),
        BorderColor::all(t.panel_border),
    )
}

/// The strip's button of the open panel shows pressed (P3.6).
fn sync_panel_buttons(open: Res<SidePanel>, q: Query<(Entity, &Name, Has<cadrs_ui::style::Selected>)>, mut commands: Commands) {
    if !open.is_changed() {
        return;
    }
    for (e, n, selected) in &q {
        let want = match n.as_str() {
            "panel-appearance" => *open == SidePanel::Appearances,
            "panel-variables" => *open == SidePanel::Variables,
            "panel-custom-tables" => *open == SidePanel::CustomTables,
            "panel-bom" | "assembly-bom" => *open == SidePanel::Bom,
            "panel-named-positions" => *open == SidePanel::NamedPositions,
            "panel-exploded-views" => *open == SidePanel::ExplodedViews,
            "panel-simulation" => *open == SidePanel::Simulation,
            _ => continue,
        };
        if want && !selected {
            commands.entity(e).try_insert(cadrs_ui::style::Selected);
        } else if !want && selected {
            commands.entity(e).try_remove::<cadrs_ui::style::Selected>();
        }
    }
}

pub(crate) fn side_panel_header(p: &mut ChildSpawnerCommands, t: &Theme, title: &str, close: &'static str) {
    p.spawn(Node {
        height: Val::Px(32.0),
        padding: UiRect::new(Val::Px(10.0), Val::Px(4.0), Val::ZERO, Val::ZERO),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        border: UiRect::bottom(Val::Px(1.0)),
        ..default()
    })
    .insert(BorderColor::all(t.panel_border))
    .with_children(|h| {
        h.spawn(t.text(title.to_string(), 13.0, FontWeight::BOLD, t.foreground));
        h.spawn(IconButton::new(close, "close").tooltip("Close").build(t));
    });
}

/// The Variables panel: the Variable table's columns, empty until the Variable feature exists
/// (P3F.4).
fn sync_variables_panel(
    open: Res<SidePanel>,
    theme: Res<Theme>,
    q_area: Query<(Entity, &ChildOf), With<ViewportArea>>,
    q_children: Query<&Children>,
    q_panel: Query<Entity, With<VariablesPanel>>,
    mut commands: Commands,
) {
    let want = *open == SidePanel::Variables;
    if want != q_panel.is_empty() {
        return;
    }
    for e in &q_panel {
        commands.entity(e).try_despawn();
    }
    let (true, Some(area)) = (want, q_area.iter().next()) else {
        return;
    };
    let t = theme.clone();
    let panel = commands
        .spawn((Name::new("variables-panel"), VariablesPanel, DespawnOnExit(AppState::Document), side_panel_node(&t)))
        .with_children(|p| {
            side_panel_header(p, &t, "Variables", "variables-panel-close");
            p.spawn((
                Node {
                    height: Val::Px(24.0),
                    padding: UiRect::horizontal(Val::Px(10.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(t.panel_border),
            ))
            .with_children(|h| {
                for (label, w) in [("Name", 76.0), ("Value (expression)", 140.0)] {
                    h.spawn((
                        t.text(label, 11.0, FontWeight::MEDIUM, t.muted_foreground),
                        Node {
                            width: Val::Px(w),
                            ..default()
                        },
                    ));
                }
            });
            // P3F.4: the variables, filled by `variables_ui`.
            p.spawn((
                Name::new("variables-panel-rows"),
                crate::variables_ui::VariableTableRows,
                Node { flex_direction: FlexDirection::Column, ..default() },
            ));
        })
        .id();
    dock_beside_viewport(&mut commands, area, &q_children, panel);
}

/// The Custom tables panel (P3.11, PS2.10): Onshape lists here the tables that FeatureScript
/// table features define (a custom table is written in FeatureScript and added to the
/// document); cadrs has no FeatureScript, so the panel says so, with its Add button disabled.
fn sync_custom_tables_panel(
    open: Res<SidePanel>,
    theme: Res<Theme>,
    q_area: Query<(Entity, &ChildOf), With<ViewportArea>>,
    q_children: Query<&Children>,
    q_panel: Query<Entity, With<CustomTablesPanel>>,
    mut commands: Commands,
) {
    let want = *open == SidePanel::CustomTables;
    if want != q_panel.is_empty() {
        return;
    }
    for e in &q_panel {
        commands.entity(e).try_despawn();
    }
    let (true, Some(area)) = (want, q_area.iter().next()) else {
        return;
    };
    let t = theme.clone();
    let panel = commands
        .spawn((Name::new("custom-tables-panel"), CustomTablesPanel, DespawnOnExit(AppState::Document), side_panel_node(&t)))
        .with_children(|p| {
            side_panel_header(p, &t, "Custom tables", "custom-tables-panel-close");
            p.spawn((
                Name::new("custom-tables-panel-empty"),
                t.text(
                    "No custom tables in this Part Studio. Custom tables are defined by FeatureScript table features, which cadrs doesn't have.",
                    11.5,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Node { margin: UiRect::all(Val::Px(12.0)), width: Val::Px(214.0), ..default() },
            ))
            .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            p.spawn(Node { margin: UiRect::horizontal(Val::Px(12.0)), ..default() }).with_children(|r| {
                r.spawn(
                    cadrs_ui::Button::new("custom-tables-add").label("Add custom table…")
                        .variant(cadrs_ui::ButtonVariant::Secondary)
                        .disabled(true)
                        .build(&t),
                );
            });
        })
        .id();
    dock_beside_viewport(&mut commands, area, &q_children, panel);
}

/// Puts a side panel right after the viewport area in its row.
pub(crate) fn dock_beside_viewport(commands: &mut Commands, area: (Entity, &ChildOf), q_children: &Query<&Children>, panel: Entity) {
    let (area, parent) = (area.0, area.1.parent());
    let at = q_children
        .get(parent)
        .ok()
        .and_then(|c| c.iter().position(|e| e == area))
        .map_or(0, |i| i + 1);
    commands.entity(parent).insert_children(at, &[panel]);
}

fn on_panel_row_double_click(ev: On<DoubleClick>, q: Query<&PanelRow>, mut commands: Commands) {
    if let Ok(row) = q.get(ev.entity) {
        let target = row.0.clone();
        commands.queue(move |world: &mut World| open_appearance_dialog(world, target));
    }
}

#[derive(Component, Debug, Clone)]
struct PanelMenuFor(AppearanceTarget);

fn on_panel_row_menu(ev: On<ContextMenuRequested>, q: Query<&PanelRow>, theme: Res<Theme>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    let menu = Menu::new("appearance-row-menu")
        .min_width(150.0)
        .item_height(22.0)
        .text_only()
        .item(MenuItem::new("appearance-row-edit", "Edit appearance…"));
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands.entity(anchor).insert((PanelMenuFor(row.0.clone()), DespawnOnExit(AppState::Document)));
}

fn on_panel_menu_action(ev: On<MenuAction>, q: Query<&PanelMenuFor, With<ContextMenuAnchor>>, mut commands: Commands) {
    if let Ok(t) = q.get(ev.entity)
        && ev.item == "appearance-row-edit"
    {
        let target = t.0.clone();
        commands.queue(move |world: &mut World| open_appearance_dialog(world, target));
    }
}

/// One panel row: (group, label, colour, target).
type PanelEntry = (usize, String, Appearance, AppearanceTarget);

/// The rows the panel lists: every part (and each face with its own appearance), every feature
/// with an appearance, and every sketch.
fn panel_entries(doc: &ActiveDocument, cache: &PartCache) -> Vec<PanelEntry> {
    let Some(el) = doc.active_element() else {
        return Vec::new();
    };
    let features = cache.appearances_now();
    let props = cache.props_now();
    let mut out = Vec::new();
    for p in &cache.parts {
        let name = cache.part_name(p.id).unwrap_or(&p.name).to_string();
        out.push((0, name.clone(), appearance::part_appearance(p, &props), AppearanceTarget::Parts(vec![p.id])));
        if let Some(pp) = props.iter().find(|x| x.part == p.id) {
            for (f, a) in &pp.faces {
                out.push((0, format!("    Face of {name}"), *a, AppearanceTarget::Faces(p.id, vec![*f])));
            }
        }
    }
    for f in el.features() {
        if f.sketch().is_some() {
            let a = features.iter().find(|(x, _)| *x == f.id).map(|(_, a)| *a).unwrap_or(SKETCH_GREY);
            out.push((2, f.name.clone(), a, AppearanceTarget::Sketch(f.id)));
            for (_, c, a) in cache.curves_now().iter().filter(|(s, _, _)| *s == f.id) {
                out.push((2, format!("    Curve of {}", f.name), *a, AppearanceTarget::Curve(f.id, *c)));
            }
        } else if let Some((_, a)) = features.iter().find(|(x, _)| *x == f.id) {
            out.push((1, f.name.clone(), *a, AppearanceTarget::Feature(f.id)));
        }
    }
    // Parts, then features, then sketches (each in list order).
    out.sort_by_key(|e| e.0);
    out
}

#[allow(clippy::too_many_arguments)]
fn sync_appearance_panel(
    open: Res<SidePanel>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    q_area: Query<(Entity, &ChildOf), With<ViewportArea>>,
    q_children: Query<&Children>,
    q_panel: Query<Entity, With<AppearancePanel>>,
    mut last: Local<Option<Vec<(usize, String, Appearance)>>>,
    mut commands: Commands,
) {
    let entries = match (&doc, *open == SidePanel::Appearances) {
        (Some(d), true) => panel_entries(d, &cache),
        _ => {
            for e in &q_panel {
                commands.entity(e).try_despawn();
            }
            *last = None;
            return;
        }
    };
    let key: Vec<(usize, String, Appearance)> = entries.iter().map(|(g, l, a, _)| (*g, l.clone(), *a)).collect();
    if last.as_ref() == Some(&key) && !q_panel.is_empty() {
        return;
    }
    *last = Some(key);
    for e in &q_panel {
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let t = theme.clone();
    let panel = commands
        .spawn((
            Name::new("appearance-panel"),
            AppearancePanel,
            DespawnOnExit(AppState::Document),
            side_panel_node(&t),
        ))
        .with_children(|p| {
            side_panel_header(p, &t, "Appearances", "appearance-panel-close");
            p.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::vertical(Val::Px(4.0)),
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .with_children(|list| {
                let titles = ["Parts", "Features", "Sketches"];
                let mut group = usize::MAX;
                for (i, (g, label, a, target)) in entries.into_iter().enumerate() {
                    if g != group {
                        group = g;
                        list.spawn((
                            t.text(titles[g], 11.0, FontWeight::MEDIUM, t.muted_foreground),
                            Node {
                                margin: UiRect::new(Val::Px(10.0), Val::ZERO, Val::Px(6.0), Val::Px(2.0)),
                                ..default()
                            },
                        ));
                    }
                    let color = srgb(a).with_alpha(a.alpha as f32 / 255.0);
                    let swatch_name = format!("appearance-row-{i}-swatch");
                    list.spawn((
                        ListItem::new(format!("appearance-row-{i}"))
                            .label(label)
                            .height(24.0)
                            .padding_left(10.0)
                            .content(move |r| {
                                r.spawn((
                                    Name::new(swatch_name),
                                    Node {
                                        width: Val::Px(16.0),
                                        height: Val::Px(16.0),
                                        margin: UiRect::new(Val::Auto, Val::Px(8.0), Val::ZERO, Val::ZERO),
                                        flex_shrink: 0.0,
                                        border: UiRect::all(Val::Px(1.0)),
                                        border_radius: BorderRadius::all(Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(color),
                                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                                    Pickable::IGNORE,
                                ));
                            })
                            .build(&t),
                        PanelRow(target),
                        ContextMenuTarget,
                        cadrs_ui::DoubleClickable,
                    ));
                }
            });
        })
        .id();
    dock_beside_viewport(&mut commands, area, &q_children, panel);
}
