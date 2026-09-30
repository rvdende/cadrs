//! Sketch text (S16): the Text tool's dialog, its live preview and Edit text.
//!
//! The Text tool draws a box like the corner rectangle (`sketch_tools`, [`DrawState::TextBox`]);
//! its lower edge is the baseline and its height the capitals' height (Onshape's help:
//! "the lower left corner and the height define the text position and size"). The Text dialog
//! ([`cadrs_ui::TextDialog`]) then opens with the text field focused; while it is open the text
//! is previewed in the sketch. ✓ or Enter adds it ([`SketchOp::AddText`]); ✕ or Esc drops it.
//! Right-clicking a text (or its box) offers **Edit text**, which reopens the dialog with its
//! text and style ([`SketchOp::EditText`]), and Delete.
//!
//! [`DrawState::TextBox`]: crate::sketch_tools::DrawState::TextBox

use bevy::prelude::*;
use cadrs_sketch::text::{TextFont, TextStyle, layout};
use cadrs_sketch::{SketchEntity, SketchOp, TextId, Vec2 as SVec2};
use cadrs_ui::menu::{Menu, MenuAction, MenuItem};
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, TextCancel, TextDialog, TextDialogState, TextSubmit, Theme};

use crate::sketch::{PartStudioMode, SketchSession};
use crate::sketch_draw::SketchRubberGizmos;
use crate::sketch_tools::{SketchSelection, world_sketch};
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

pub struct SketchTextPlugin;

impl Plugin for SketchTextPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextEditing>()
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_submit)
            .add_observer(on_field_cancel)
            .add_observer(on_text_menu)
            .add_systems(
                Update,
                draw_text_preview
                    .after(crate::sketch_draw::SketchDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(PartStudioMode::Sketching), close_on_exit);
    }
}

/// What the open text dialog makes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextMode {
    /// A new text in a box drawn from `origin` (its lower-left corner) along `dir`, `height`
    /// high. `width` is the drawn box's, shown until text is typed (the text then sets it).
    New {
        origin: SVec2,
        dir: SVec2,
        height: f64,
        width: f64,
    },
    /// Edit text.
    Edit(TextId),
}

/// The open text dialog.
#[derive(Resource, Debug, Default)]
pub struct TextEditing {
    pub open: Option<(Entity, TextMode)>,
    /// The frame the dialog closed in: its Enter or Esc is not the sketch's.
    pub closed_frame: Option<u32>,
}

/// Marks the text dialog's wrapper.
#[derive(Component)]
struct TextDialogRoot;

/// The fonts on offer with the weight the dialog previews them in.
fn fonts() -> Vec<(&'static str, u16)> {
    TextFont::ALL
        .iter()
        .map(|f| {
            let w = match f {
                TextFont::Inter => 400,
                TextFont::InterMedium => 500,
                TextFont::InterSemiBold => 600,
                TextFont::InterBlack => 900,
            };
            (f.label(), w)
        })
        .collect()
}

fn style_of(s: &TextDialogState) -> TextStyle {
    TextStyle {
        text: s.text.clone(),
        font: TextFont::ALL.get(s.font).copied().unwrap_or_default(),
        bold: s.bold,
        italic: s.italic,
        mirror_h: s.flip_h,
        mirror_v: s.flip_v,
    }
}

/// Opens the Text dialog (closing one that is open).
pub fn open_text_dialog(world: &mut World, mode: TextMode) {
    close_dialog(world);
    let start = match mode {
        TextMode::New { .. } => TextStyle::default(),
        TextMode::Edit(id) => match world_sketch(world).and_then(|s| s.texts.get(id)) {
            Some(t) => t.style.clone(),
            None => return,
        },
    };
    let theme = world.resource::<Theme>().clone();
    let Some(area) = world
        .query_filtered::<Entity, With<ViewportArea>>()
        .iter(world)
        .next()
    else {
        return;
    };
    let font = TextFont::ALL.iter().position(|f| *f == start.font).unwrap_or(0);
    let dialog = TextDialog::new("text-dialog", &fonts())
        .state(start.text, font, start.bold, start.italic, start.mirror_h, start.mirror_v)
        .build(&theme);
    // Beside the sketch dialog.
    let wrapper = world
        .spawn((
            Name::new("text-dialog-host"),
            TextDialogRoot,
            DespawnOnExit(AppState::Document),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(250.0),
                top: Val::Px(0.0),
                ..default()
            },
        ))
        .id();
    let d = world.spawn(dialog).id();
    world.entity_mut(wrapper).add_child(d);
    world.entity_mut(area).add_child(wrapper);
    world.resource_mut::<TextEditing>().open = Some((d, mode));
}

pub fn close_dialog(world: &mut World) {
    let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
    let mut editing = world.resource_mut::<TextEditing>();
    if editing.open.take().is_some() {
        editing.closed_frame = Some(frame);
    }
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, With<TextDialogRoot>>()
        .iter(world)
        .collect();
    for e in roots {
        world.entity_mut(e).despawn();
    }
}

fn close_on_exit(mut commands: Commands) {
    commands.queue(close_dialog);
}

/// ✓ / Enter: adds or changes the text.
fn accept(world: &mut World, dialog: Entity) {
    let Some((d, mode)) = world.resource::<TextEditing>().open else {
        return;
    };
    if d != dialog {
        return;
    }
    let Some(state) = world.get::<TextDialogState>(d).cloned() else {
        return;
    };
    let style = style_of(&state);
    close_dialog(world);
    if style.text.trim().is_empty() {
        return;
    }
    let Some(s) = world.get_resource::<SketchSession>().cloned() else {
        return;
    };
    let op = match mode {
        TextMode::New {
            origin, dir, height, ..
        } => SketchOp::AddText {
            origin,
            dir,
            height,
            style,
        },
        TextMode::Edit(id) => SketchOp::EditText { id, style },
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&cadrs_core::commands::EditSketch {
            element: s.element,
            feature: s.feature,
            op,
        })
    {
        warn!("cannot set the text: {e}");
    }
}

/// Enter outside the text field: accepts the open dialog.
pub fn accept_open(world: &mut World) {
    if let Some((d, _)) = world.resource::<TextEditing>().open {
        accept(world, d);
    }
}

fn is_ours(world: &World, e: Entity) -> bool {
    world.resource::<TextEditing>().open.is_some_and(|(d, _)| d == e)
}

fn on_accept(ev: On<FeatureDialogAccept>, mut commands: Commands) {
    let e = ev.entity;
    commands.queue(move |world: &mut World| {
        if is_ours(world, e) {
            accept(world, e);
        }
    });
}

fn on_cancel(ev: On<FeatureDialogCancel>, mut commands: Commands) {
    let e = ev.entity;
    commands.queue(move |world: &mut World| {
        if is_ours(world, e) {
            close_dialog(world);
        }
    });
}

/// Enter in the text field bubbles to the dialog.
fn on_submit(ev: On<TextSubmit>, editing: Res<TextEditing>, mut commands: Commands) {
    if let Some((d, _)) = editing.open
        && ev.entity == d
    {
        commands.queue(move |world: &mut World| accept(world, d));
    }
}

fn on_field_cancel(ev: On<TextCancel>, editing: Res<TextEditing>, mut commands: Commands) {
    if let Some((d, _)) = editing.open
        && ev.entity == d
    {
        commands.queue(close_dialog);
    }
}

/// The text as it will be, in the sketch, while the dialog is open.
fn draw_text_preview(
    editing: Res<TextEditing>,
    q: Query<&TextDialogState>,
    session: Option<Res<SketchSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut g: Gizmos<SketchRubberGizmos>,
) {
    let Some((d, mode)) = editing.open else { return };
    let Ok(state) = q.get(d) else { return };
    let Some(sketch) = crate::sketch_tools::session_sketch(session.as_deref(), doc.as_deref()) else {
        return;
    };
    let Some(frame) = crate::sketch_tools::session_plane(session.as_deref(), doc.as_deref()).map(|p| p.frame())
    else {
        return;
    };
    let style = style_of(state);
    let empty = style.text.trim().is_empty();
    let (origin, ex, ey) = match mode {
        TextMode::New {
            origin,
            dir,
            height,
            width,
        } => {
            // The box as drawn until there is text (not a square).
            let w = if empty { width } else { layout(&style).width * height };
            (origin, dir * w, dir.perp() * height)
        }
        TextMode::Edit(id) => {
            let Some(t) = sketch.texts.get(id) else { return };
            let [a, b, _, dd] = t.corners.map(|p| sketch.pos(p));
            let h = a.distance(dd);
            let dir = (b - a).normalize();
            (a, dir * (layout(&style).width * h), dd - a)
        }
    };
    let l = layout(&style);
    let lw = if empty && matches!(mode, TextMode::New { .. }) {
        1.0
    } else {
        l.width
    };
    let w = |p: SVec2| {
        let q = frame.to_world(origin + ex * (p.x / lw.max(1e-9)) + ey * p.y);
        Vec3::new(q[0] as f32, q[1] as f32, q[2] as f32)
    };
    let color = Color::srgb_u8(0x46, 0x9c, 0xcd);
    for c in &l.contours {
        let mut pts: Vec<Vec3> = c.iter().map(|p| w(*p)).collect();
        pts.push(pts[0]);
        g.linestrip(pts, color);
    }
    // The box, dashed.
    let corners = [
        SVec2::ZERO,
        SVec2::new(lw, 0.0),
        SVec2::new(lw, 1.0),
        SVec2::new(0.0, 1.0),
    ];
    for i in 0..4 {
        let (a, b) = (w(corners[i]), w(corners[(i + 1) % 4]));
        let n = 24;
        for k in (0..n).step_by(2) {
            g.line(a.lerp(b, k as f32 / n as f32), a.lerp(b, (k + 1) as f32 / n as f32), color);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Context menu

#[derive(Component)]
struct TextMenuFor(TextId);

/// The text a right-click hit: on its outline, or on its box.
pub fn text_under(sketch: &cadrs_sketch::Sketch, hit: Option<SketchEntity>) -> Option<TextId> {
    match hit? {
        SketchEntity::Text(t) => Some(t),
        SketchEntity::Curve(c) => cadrs_sketch::text::text_of_curve(sketch, c),
        SketchEntity::Point(p) => cadrs_sketch::text::text_of_point(sketch, p),
        _ => None,
    }
}

/// Right-click on a text: Edit text, Delete.
pub fn open_text_menu(world: &mut World, id: TextId, at: Vec2) {
    let theme = world.resource::<Theme>().clone();
    world.resource_mut::<SketchSelection>().0 = vec![SketchEntity::Text(id)];
    let menu = Menu::new("sketch-text-menu")
        .min_width(180.0)
        .item_height(23.0)
        .item(MenuItem::new("sketch-text-edit", "Edit text").icon("edit"))
        .item(MenuItem::new("sketch-text-delete", "Delete").icon("delete"));
    let mut commands = world.commands();
    let anchor = cadrs_ui::open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((TextMenuFor(id), DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_text_menu(ev: On<MenuAction>, q: Query<&TextMenuFor>, mut commands: Commands) {
    let Ok(anchor) = q.get(ev.entity) else { return };
    let id = anchor.0;
    match ev.item.as_str() {
        "sketch-text-edit" => {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<SketchSelection>().0.clear();
                open_text_dialog(world, TextMode::Edit(id));
            });
        }
        "sketch-text-delete" => {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<SketchSelection>().0.clear();
                let Some(line) = world_sketch(world).and_then(|s| s.texts.get(id)).map(|t| t.lines[0]) else {
                    return;
                };
                let Some(s) = world.get_resource::<SketchSession>().cloned() else { return };
                if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
                    let _ = doc.execute(&cadrs_core::commands::EditSketch {
                        element: s.element,
                        feature: s.feature,
                        op: SketchOp::Delete {
                            curves: vec![line],
                            points: vec![],
                            dimensions: vec![],
                            constraints: vec![],
                        },
                    });
                }
            });
        }
        _ => {}
    }
}
