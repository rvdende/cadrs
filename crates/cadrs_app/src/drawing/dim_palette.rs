//! The dimension palette (D6.6, D2.6) and the Hole callout dialog (D6.7).
//!
//! - **Palette.** With one dimension selected, a small flyout button appears beside its text;
//!   it opens the palette, which formats that one dimension over the drawing properties:
//!   prefix and suffix text, symbols (Ø ± ° and the ⌴ ↧ ⌵ hole symbols, drawn as small vector
//!   glyphs since Inter lacks them), a tolerance (none, symmetric ±, deviation, limits) with its
//!   upper and lower values in the drawing's units, the precision and dual units. Each change is
//!   one undoable edit.
//! - **Hole callout dialog.** Right-click a hole callout → Edit…: a small card titled "Hole
//!   callout" with ✓ / ✗ and a **Prefix** field (`4x`), placed by the callout
//!   (`ex1-step8.png`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_drawing::annotation::{Annotation, AnnotationId, AnnotationKind, DimFormat, Tolerance, ValueKind};
use cadrs_drawing::{DrawingOp, ViewId};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, CheckboxChange, Select, SelectChange};

use super::annotations::{AnnTool, AnnotationScene, AnnotationUi, find_annotation};
use super::view_tools::edit_drawing;
use super::views::ViewCache;
use super::{DrawingUi, active_drawing, current_view, sheet_area, sheet_to_screen};
use crate::viewport::{ActiveKind, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct DimPalettePlugin;

impl Plugin for DimPalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            sync_palette
                .after(super::annotations::AnnotationDrawSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(
            Update,
            preview_callout
                .before(super::annotations::AnnotationDrawSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_observer(on_select)
        .add_observer(on_check)
        .add_observer(on_submit);
    }
}

/// The palette's root, for the dimension it formats.
#[derive(Component, Clone, PartialEq)]
struct PaletteRoot {
    view: ViewId,
    id: AnnotationId,
    format: DimFormat,
    open: bool,
    at: Vec2,
}

/// The selected dimension, if exactly one is selected.
fn selected_dimension(world: &World) -> Option<(ViewId, Annotation)> {
    let ui = world.resource::<AnnotationUi>();
    if ui.tool != AnnTool::None || ui.selected.len() != 1 || ui.drag.is_some_and(|d| d.moving) {
        return None;
    }
    let (v, id) = ui.selected[0];
    let doc = world.get_resource::<ActiveDocument>()?;
    let (_, d) = active_drawing(doc)?;
    let (_, a) = find_annotation(d, v, id)?;
    matches!(a.kind, AnnotationKind::Dimension(_)).then_some((v, a))
}

fn dim_format(a: &Annotation) -> Option<&DimFormat> {
    match &a.kind {
        AnnotationKind::Dimension(d) => Some(&d.format),
        _ => None,
    }
}

/// Where the palette button goes (screen px): above the right end of the dimension's text.
fn button_at(world: &World, view: ViewId, id: AnnotationId) -> Option<Vec2> {
    let scene = world.resource::<AnnotationScene>();
    let (_, _, g) = scene.items.iter().find(|(v, i, _)| *v == view && *i == id)?;
    let (_, max) = g.boxes.first()?;
    let doc = world.get_resource::<ActiveDocument>()?;
    let ui = world.resource::<DrawingUi>();
    let (_, sv) = current_view(doc, ui)?;
    let rect = world.resource::<ViewportRect>();
    let area = sheet_area(rect, ui);
    // Just above the text's top right corner, clear of the dimension line.
    Some(sheet_to_screen(sv, area, Vec2::new(max[0] as f32, max[1] as f32)) + Vec2::new(2.0, -24.0))
}

fn sync_palette(world: &mut World) {
    let kind = *world.resource::<ActiveKind>();
    let want = (kind == ActiveKind::Drawing)
        .then(|| selected_dimension(world))
        .flatten()
        .and_then(|(v, a)| {
            let at = button_at(world, v, a.id)?;
            Some(PaletteRoot {
                view: v,
                id: a.id,
                format: dim_format(&a)?.clone(),
                open: world.resource::<AnnotationUi>().palette_open,
                at: at.round(),
            })
        });
    let mut q = world.query::<(Entity, &PaletteRoot)>();
    let have: Vec<(Entity, PaletteRoot)> = q.iter(world).map(|(e, p)| (e, p.clone())).collect();
    if have.len() == 1 && want.as_ref() == Some(&have[0].1) {
        return;
    }
    // Only the button moved: move it, keep the panel (and its fields' focus).
    if let (Some(w), [(e, h)]) = (&want, have.as_slice())
        && (PaletteRoot { at: w.at, ..h.clone() }) == *w
    {
        if let Some(mut n) = world.get_mut::<Node>(*e) {
            n.left = Val::Px(w.at.x);
            n.top = Val::Px(w.at.y);
        }
        if let Some(mut p) = world.get_mut::<PaletteRoot>(*e) {
            p.at = w.at;
        }
        return;
    }
    for (e, _) in have {
        world.entity_mut(e).despawn();
    }
    let Some(w) = want else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let kind = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let (_, d) = active_drawing(doc)?;
        let (v, a) = find_annotation(d, w.view, w.id)?;
        let g = world.resource::<ViewCache>().geometry(&v)?;
        match &a.kind {
            AnnotationKind::Dimension(dim) => cadrs_drawing::annotation::measure(&v, &*g, dim).map(|m| (m.kind, d.style.clone())),
            _ => None,
        }
    })();
    let Some((value_kind, style)) = kind else {
        return;
    };
    let mut commands = world.commands();
    commands
        .spawn((
            Name::new("dimension-palette-root"),
            w.clone(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(w.at.x),
                top: Val::Px(w.at.y),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(4.0),
                ..default()
            },
            GlobalZIndex(cadrs_ui::z::DIALOG - 14),
            DespawnOnExit(AppState::Document),
        ))
        .with_children(|r| {
            r.spawn((
                Button::new("dimension-palette-button")
                    .icon("settings")
                    .icon_size(14.0)
                    .outline()
                    .tooltip("Dimension palette")
                    .build(&theme),
                observe(|_: On<Activate>, mut ui: ResMut<AnnotationUi>| {
                    ui.palette_open = !ui.palette_open;
                }),
            ))
            .insert((
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.3), Val::Px(0.0), Val::Px(1.0), Val::Px(0.0), Val::Px(3.0)),
            ));
            if w.open {
                palette_panel(r, &theme, &w.format, value_kind, &style);
            }
        });
    world.flush();
}

/// The tolerance types, in the dropdown's order.
const TOLERANCES: [&str; 4] = ["None", "Symmetric", "Deviation", "Limits"];

/// The precision choices: the drawing's, then 0–4 decimals.
const PRECISIONS: [&str; 6] = ["Default", "0", "0.1", "0.12", "0.123", "0.1234"];

fn tolerance_index(t: &Tolerance) -> usize {
    match t {
        Tolerance::None => 0,
        Tolerance::Symmetric(_) => 1,
        Tolerance::Deviation { .. } => 2,
        Tolerance::Limits { .. } => 3,
    }
}

/// A tolerance value as the fields show it (drawing units or degrees).
fn value_text(v: f64, kind: ValueKind, style: &cadrs_drawing::DrawingStyle) -> String {
    let v = if kind == ValueKind::Angle { v } else { v / style.units.mm() };
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

fn palette_panel(r: &mut ChildSpawnerCommands, t: &Theme, f: &DimFormat, kind: ValueKind, style: &cadrs_drawing::DrawingStyle) {
    let (upper, lower) = match f.tolerance {
        Tolerance::None => (None, None),
        Tolerance::Symmetric(v) => (Some(v), None),
        Tolerance::Deviation { upper, lower } | Tolerance::Limits { upper, lower } => (Some(upper), Some(lower)),
    };
    let label_w = 72.0;
    let row = |p: &mut ChildSpawnerCommands, name: &str, label: &str| {
        p.spawn((
            Name::new(format!("{name}-row")),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                min_height: Val::Px(28.0),
                ..default()
            },
        ))
        .with_children(|r| {
            r.spawn((
                t.text(label, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node {
                    width: Val::Px(label_w),
                    ..default()
                },
            ));
        })
        .id()
    };
    r.spawn((
        Name::new("dimension-palette"),
        Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(248.0),
            padding: UiRect::all(Val::Px(8.0)),
            row_gap: Val::Px(4.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(t.background),
        BorderColor::all(Color::srgb_u8(0xd8, 0xd8, 0xd8)),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.3), Val::Px(1.0), Val::Px(2.0), Val::Px(0.0), Val::Px(6.0)),
    ))
    .with_children(|p| {
        p.spawn(t.text("Dimension", t.font_base, FontWeight::BOLD, t.foreground));
        let e = row(p, "dim-palette-prefix", "Prefix");
        p.commands().entity(e).with_child(
            TextInput::new("dim-palette-prefix")
                .value(f.prefix.clone())
                .width(Val::Px(150.0))
                .build(t),
        );
        let e = row(p, "dim-palette-suffix", "Suffix");
        p.commands().entity(e).with_child(
            TextInput::new("dim-palette-suffix")
                .value(f.suffix.clone())
                .width(Val::Px(150.0))
                .build(t),
        );
        let e = row(p, "dim-palette-symbols", "Symbols");
        p.commands().entity(e).with_children(|r| {
            for (name, sym, tip) in SYMBOLS {
                r.spawn((
                    Name::new(format!("dim-palette-symbol-{name}")),
                    Node {
                        width: Val::Px(22.0),
                        height: Val::Px(22.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(t.input_border),
                    BackgroundColor(t.background),
                    Interaction::default(),
                    cadrs_ui::Tooltip::new(tip),
                    observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| insert_symbol(w, sym));
                    }),
                ))
                .with_children(|b| symbol_glyph(b, t, sym));
            }
        });
        let e = row(p, "dim-palette-tolerance", "Tolerance");
        let mut sel = Select::new("dim-palette-tolerance").width(Val::Px(150.0));
        for o in TOLERANCES {
            sel = sel.option(o, true);
        }
        p.commands().entity(e).with_child(sel.selected(tolerance_index(&f.tolerance)).build(t));
        let e = row(p, "dim-palette-upper", if matches!(f.tolerance, Tolerance::Symmetric(_)) { "±" } else { "Upper" });
        p.commands().entity(e).with_child(
            TextInput::new("dim-palette-upper")
                .value(upper.map(|v| value_text(v, kind, style)).unwrap_or_default())
                .disabled(upper.is_none())
                .width(Val::Px(150.0))
                .build(t),
        );
        let e = row(p, "dim-palette-lower", "Lower");
        p.commands().entity(e).with_child(
            TextInput::new("dim-palette-lower")
                .value(lower.map(|v| value_text(v, kind, style)).unwrap_or_default())
                .disabled(lower.is_none())
                .width(Val::Px(150.0))
                .build(t),
        );
        let e = row(p, "dim-palette-precision", "Precision");
        let mut sel = Select::new("dim-palette-precision").width(Val::Px(150.0));
        for o in PRECISIONS {
            sel = sel.option(o, true);
        }
        p.commands()
            .entity(e)
            .with_child(sel.selected(f.precision.map_or(0, |d| d as usize + 1)).build(t));
        if kind != ValueKind::Angle {
            let dual = f.dual.unwrap_or(style.show_dual);
            p.spawn(
                Checkbox::new("dim-palette-dual")
                    .label(format!("Dual units ({})", style.dual_units.symbol()))
                    .checked(dual)
                    .build(t),
            );
        }
    });
}

/// The palette's symbol buttons: (name, symbol, tooltip).
const SYMBOLS: [(&str, char, &str); 6] = [
    ("diameter", 'Ø', "Diameter"),
    ("plus-minus", '±', "Plus/minus"),
    ("degree", '°', "Degree"),
    ("counterbore", '⌴', "Counterbore"),
    ("countersink", '⌵', "Countersink"),
    ("depth", '↧', "Depth"),
];

/// A symbol on a button: Inter's glyph, or (for ⌴ ⌵ ↧, which Inter lacks) small UI boxes.
fn symbol_glyph(b: &mut ChildSpawnerCommands, t: &Theme, c: char) {
    let ink = t.foreground;
    let line = |w: f32, h: f32, left: f32, top: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(w),
                height: Val::Px(h),
                ..default()
            },
            BackgroundColor(ink),
            Pickable::IGNORE,
        )
    };
    match c {
        '⌴' => {
            b.spawn((
                Node {
                    width: Val::Px(10.0),
                    height: Val::Px(9.0),
                    border: UiRect::new(Val::Px(1.2), Val::Px(1.2), Val::ZERO, Val::Px(1.2)),
                    ..default()
                },
                BorderColor::all(ink),
                Pickable::IGNORE,
            ));
        }
        '⌵' | '↧' => {
            b.spawn((
                Node {
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|g| {
                // A bar from `a` to `b` (px in the 12 × 12 box), turned about its middle.
                let slant = |g: &mut ChildSpawnerCommands, a: Vec2, b: Vec2| {
                    let c = (a + b) / 2.0;
                    // A little past each end, so two bars meet without a gap.
                    let len = a.distance(b) + 1.2;
                    let w = 1.3;
                    // Clockwise from vertical (the node's rotation is clockwise).
                    let ang = (b.x - a.x).atan2(b.y - a.y);
                    g.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(c.x - w / 2.0),
                            top: Val::Px(c.y - len / 2.0),
                            width: Val::Px(w),
                            height: Val::Px(len),
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::radians(-ang)),
                        BackgroundColor(ink),
                        Pickable::IGNORE,
                    ));
                };
                if c == '↧' {
                    // The depth arrow: a shaft, a V head and a base line.
                    g.spawn(line(1.2, 10.4, 5.4, 0.0));
                    slant(g, Vec2::new(2.8, 6.6), Vec2::new(6.0, 10.4));
                    slant(g, Vec2::new(9.2, 6.6), Vec2::new(6.0, 10.4));
                    g.spawn(line(9.0, 1.2, 1.5, 10.8));
                } else {
                    // The countersink V (vector, like the sheet's).
                    slant(g, Vec2::new(1.2, 1.5), Vec2::new(6.0, 11.0));
                    slant(g, Vec2::new(10.8, 1.5), Vec2::new(6.0, 11.0));
                }
            });
        }
        _ => {
            b.spawn((t.text(c.to_string(), t.font_base, FontWeight::NORMAL, ink), Pickable::IGNORE));
        }
    }
}

/// Changes the selected dimension's format with `f`.
fn set_format(world: &mut World, label: &str, f: impl FnOnce(&mut DimFormat, ValueKind, &cadrs_drawing::DrawingStyle)) {
    let Some((view, a)) = selected_dimension(world) else {
        return;
    };
    let Some((v, style)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| d.view(view).map(|(_, v)| (v.clone(), d.style.clone())))
    else {
        return;
    };
    let Some(g) = world.resource::<ViewCache>().geometry(&v) else {
        return;
    };
    let mut a = a;
    let AnnotationKind::Dimension(d) = &mut a.kind else {
        return;
    };
    let Some(m) = cadrs_drawing::annotation::measure(&v, &*g, d) else {
        return;
    };
    let before = d.format.clone();
    f(&mut d.format, m.kind, &style);
    if d.format != before {
        edit_drawing(world, DrawingOp::SetAnnotation { view, annotation: a, label: label.into() });
    }
}

fn insert_symbol(world: &mut World, c: char) {
    set_format(world, "Edit dimension prefix", |f, _, _| f.prefix.push(c));
}

/// A default tolerance: .005 in, 0.05 mm, 0.5°.
fn default_tolerance(kind: ValueKind, style: &cadrs_drawing::DrawingStyle) -> f64 {
    match kind {
        ValueKind::Angle => 0.5,
        _ if style.units == cadrs_sketch::units::LengthUnit::Inch => 0.005 * 25.4,
        _ => 0.05,
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    match name.as_str() {
        "dim-palette-tolerance" => commands.queue(move |w: &mut World| {
            set_format(w, "Edit dimension tolerance", |f, kind, style| {
                let t = default_tolerance(kind, style);
                let (up, lo) = match f.tolerance {
                    Tolerance::None => (t, -t),
                    Tolerance::Symmetric(v) => (v, -v),
                    Tolerance::Deviation { upper, lower } | Tolerance::Limits { upper, lower } => (upper, lower),
                };
                f.tolerance = match i {
                    1 => Tolerance::Symmetric(up.abs()),
                    2 => Tolerance::Deviation { upper: up, lower: lo },
                    3 => Tolerance::Limits { upper: up, lower: lo },
                    _ => Tolerance::None,
                };
            });
        }),
        "dim-palette-precision" => commands.queue(move |w: &mut World| {
            set_format(w, "Edit dimension precision", |f, _, _| {
                f.precision = (i > 0).then(|| (i - 1) as u8);
            });
        }),
        _ => {}
    }
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "dim-palette-dual") {
        let on = ev.checked;
        commands.queue(move |w: &mut World| {
            set_format(w, "Edit dimension dual units", |f, _, _| f.dual = Some(on));
        });
    }
}

/// Enter in a palette field or the Hole callout dialog's prefix.
fn on_submit(ev: On<TextSubmit>, q: Query<(&Name, Option<&ChildOf>)>, q_names: Query<&Name>, mut commands: Commands) {
    // The field entity is inside the named TextInput frame.
    let Ok((name, parent)) = q.get(ev.entity) else {
        return;
    };
    let frame = parent.and_then(|p| q_names.get(p.parent()).ok()).map(|n| n.as_str().to_string());
    let name = frame.unwrap_or_else(|| name.as_str().trim_end_matches("-field").to_string());
    let value = ev.value.clone();
    match name.as_str() {
        "dim-palette-prefix" => commands.queue(move |w: &mut World| {
            set_format(w, "Edit dimension prefix", |f, _, _| f.prefix = value);
        }),
        "dim-palette-suffix" => commands.queue(move |w: &mut World| {
            set_format(w, "Edit dimension suffix", |f, _, _| f.suffix = value);
        }),
        "dim-palette-upper" | "dim-palette-lower" => {
            let upper = name == "dim-palette-upper";
            commands.queue(move |w: &mut World| {
                set_format(w, "Edit dimension tolerance", |f, kind, style| {
                    let Ok(v) = value.trim().trim_start_matches('+').parse::<f64>() else {
                        return;
                    };
                    let v = if kind == ValueKind::Angle { v } else { v * style.units.mm() };
                    f.tolerance = match (f.tolerance, upper) {
                        (Tolerance::Symmetric(_), _) => Tolerance::Symmetric(v.abs()),
                        (Tolerance::Deviation { lower, .. }, true) => Tolerance::Deviation { upper: v, lower },
                        (Tolerance::Deviation { upper: u, .. }, false) => Tolerance::Deviation { upper: u, lower: v },
                        (Tolerance::Limits { lower, .. }, true) => Tolerance::Limits { upper: v, lower },
                        (Tolerance::Limits { upper: u, .. }, false) => Tolerance::Limits { upper: u, lower: v },
                        (t, _) => t,
                    };
                });
            });
        }
        "hole-callout-prefix" => commands.queue(apply_callout_dialog),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Hole callout dialog

#[derive(Component, Clone, Copy)]
struct CalloutDialog {
    view: ViewId,
    id: AnnotationId,
}

/// Edit… on a hole callout (D6.7): the Hole callout card with its Prefix field.
pub fn open_callout_dialog(world: &mut World, view: ViewId, id: AnnotationId) {
    let mut q = world.query_filtered::<Entity, With<CalloutDialog>>();
    let old: Vec<Entity> = q.iter(world).collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
    let Some((_, a)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| find_annotation(d, view, id))
    else {
        return;
    };
    let AnnotationKind::HoleCallout(c) = &a.kind else {
        return;
    };
    let prefix = c.prefix.clone();
    // Above the callout's text, like `ex1-step8.png`.
    let at = (|| {
        let scene = world.resource::<AnnotationScene>();
        let (_, _, g) = scene.items.iter().find(|(v, i, _)| *v == view && *i == id)?;
        let (min, max) = g.boxes.first()?;
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let (_, sv) = current_view(doc, ui)?;
        let area = sheet_area(world.resource::<ViewportRect>(), ui);
        let p = sheet_to_screen(sv, area, Vec2::new(min[0] as f32, max[1] as f32));
        Some(p + Vec2::new(0.0, -78.0))
    })()
    .unwrap_or(Vec2::new(600.0, 300.0));
    let t = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    commands
        .spawn((
            Name::new("hole-callout-dialog"),
            CalloutDialog { view, id },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x.max(4.0)),
                top: Val::Px(at.y.max(4.0)),
                width: Val::Px(190.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(Color::srgb_u8(0xe0, 0xe0, 0xe0)),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.35), Val::Px(1.0), Val::Px(2.0), Val::Px(0.0), Val::Px(6.0)),
            GlobalZIndex(cadrs_ui::z::DIALOG - 10),
            DespawnOnExit(AppState::Document),
        ))
        .with_children(|c| {
            c.spawn((
                Node {
                    height: Val::Px(28.0),
                    padding: UiRect::horizontal(Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(2.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(t.separator),
            ))
            .with_children(|h| {
                h.spawn((
                    t.text("Hole callout", t.font_base, FontWeight::BOLD, t.foreground),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
                // A green ✓ like the feature dialogs' (`ex1-step8.png`).
                let accept = cadrs_ui::Visuals {
                    background: cadrs_ui::StateColors::new(t.accept, t.accept.darker(0.04), t.accept.darker(0.08), t.accept_disabled),
                    border: cadrs_ui::StateColors::all(Color::NONE),
                    foreground: cadrs_ui::StateColors::all(Color::WHITE),
                    focus_ring: t.focus_ring,
                };
                h.spawn((
                    cadrs_ui::Button::new("hole-callout-ok")
                        .icon("check-bold")
                        .icon_size(14.0)
                        .tooltip("Accept (Enter)")
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_callout_dialog);
                    }),
                ))
                .insert((
                    accept,
                    Node {
                        width: Val::Px(22.0),
                        height: Val::Px(20.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                ));
                h.spawn((
                    cadrs_ui::Button::new("hole-callout-cancel")
                        .icon("x-bold")
                        .icon_size(14.0)
                        .ghost()
                        .tooltip("Cancel")
                        .build(&t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_callout_dialog);
                    }),
                ))
                .insert(TextColor(t.cancel));
            });
            c.spawn(Node {
                padding: UiRect::all(Val::Px(6.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(t.text("Prefix", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                r.spawn(
                    TextInput::new("hole-callout-prefix")
                        .value(prefix)
                        .width(Val::Px(120.0))
                        .autofocus()
                        .build(&t),
                );
            });
        });
    world.flush();
}

/// While the Hole callout dialog is open, its callout shows the prefix as typed, in orange.
fn preview_callout(
    q_dialog: Query<&CalloutDialog>,
    q_fields: Query<(&Name, &bevy::text::EditableText)>,
    doc: Option<Res<ActiveDocument>>,
    mut ui: ResMut<AnnotationUi>,
) {
    let want = (|| {
        let d = q_dialog.iter().next()?;
        let typed = q_fields.iter().find(|(n, _)| n.as_str() == "hole-callout-prefix-field")?.1.value().to_string();
        let (_, dr) = active_drawing(doc.as_deref()?)?;
        let (_, mut a) = find_annotation(dr, d.view, d.id)?;
        if let AnnotationKind::HoleCallout(c) = &mut a.kind {
            c.prefix = typed.trim().to_string();
        }
        Some((d.view, a))
    })();
    if ui.callout_preview != want {
        ui.callout_preview = want;
    }
}

fn close_callout_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CalloutDialog>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn apply_callout_dialog(world: &mut World) {
    let mut q = world.query::<&CalloutDialog>();
    let Some(dialog) = q.iter(world).next().copied() else {
        return;
    };
    let value = {
        let mut qt = world.query::<(&Name, &bevy::text::EditableText)>();
        qt.iter(world)
            .find(|(n, _)| n.as_str() == "hole-callout-prefix-field")
            .map(|(_, t)| t.value().to_string())
    };
    close_callout_dialog(world);
    let Some(value) = value else {
        return;
    };
    let Some((_, mut a)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| find_annotation(d, dialog.view, dialog.id))
    else {
        return;
    };
    if let AnnotationKind::HoleCallout(c) = &mut a.kind {
        let p = value.trim().to_string();
        if c.prefix == p {
            return;
        }
        c.prefix = p;
    }
    edit_drawing(world, DrawingOp::SetAnnotation { view: dialog.view, annotation: a, label: "Edit hole callout".into() });
    world.resource_mut::<AnnotationUi>().selected.clear();
}
