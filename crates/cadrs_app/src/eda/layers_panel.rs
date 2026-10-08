//! The Layout's layers panel (KiCad's Appearance panel, its Layers tab): every copper layer and
//! the drawing layers with their colour; a click on a copper layer makes it the active one (what
//! routing and zones draw on, as PgUp/PgDn); the eye shows or hides a layer; "Dim other layers"
//! draws all but the active layer faint (high-contrast mode).
//!
//! Names: `eda-layers`, `eda-layer-<layer>` (e.g. `eda-layer-top-copper`), `eda-layer-<layer>-eye`,
//! `eda-layers-dim`, `eda-layers-toggle` (opens and closes it; it starts closed, a button).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_eda::layer::Layer;
use cadrs_eda::render::BoardTheme;
use cadrs_ui::prelude::*;

use super::layout_tools::LayoutState;
use super::{Eda2d, Mode};
use crate::AppState;
use crate::viewport::ViewportArea;

#[derive(Component)]
struct LayersPanel {
    /// What the rows were built for: layers, active, hidden, dim.
    shown: String,
}

#[derive(Component, Clone, Copy)]
struct LayerRow(Layer);

#[derive(Component, Clone, Copy)]
struct LayerEye(Layer);

#[derive(Component)]
struct DimButton;

pub fn register(app: &mut App) {
    app.add_systems(Update, sync.run_if(in_state(AppState::Document))).add_observer(on_row).add_observer(on_eye).add_observer(on_dim).add_observer(on_toggle);
}

/// The layers listed: the board's copper, then the drawing layers.
fn layers(world: &World) -> Vec<Layer> {
    let copper: Vec<Layer> = super::ui::current(world).map(|(_, _, d)| d.board.copper().collect()).unwrap_or_else(|| vec![Layer::TopCopper, Layer::BottomCopper]);
    let mut v = copper;
    v.extend([Layer::TopSilk, Layer::BottomSilk, Layer::TopFab, Layer::BottomFab, Layer::TopCourtyard, Layer::BottomCourtyard, Layer::Outline, Layer::Drawings, Layer::Comments]);
    v
}

fn color(c: [u8; 4]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

fn sync(world: &mut World) {
    let show = world.resource::<Eda2d>().board().is_some_and(|(_, _, m)| m == Mode::Layout);
    let mut q = world.query::<(Entity, &LayersPanel)>();
    let existing = q.iter(world).next().map(|(e, p)| (e, p.shown.clone()));
    if !show {
        if let Some((e, _)) = existing {
            world.commands().entity(e).despawn();
            world.flush();
        }
        return;
    }
    let list = layers(world);
    let s = world.resource::<LayoutState>();
    let state = format!("{list:?}{:?}{:?}{}{}{:?}", s.active, s.hidden, s.dim, s.layers_open, s.draw_layer);
    let (open, draw) = (s.layers_open, s.draw_layer);
    let (active, hidden, dim) = (s.active, s.hidden.clone(), s.dim);
    if existing.as_ref().is_some_and(|(_, shown)| *shown == state) {
        return;
    }
    if let Some((e, _)) = existing {
        world.commands().entity(e).despawn();
    }
    let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = qa.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let th = BoardTheme::default();
    world.commands().entity(area).with_children(|vp| {
        vp.spawn((
            Name::new("eda-layers"),
            LayersPanel { shown: state },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(36.0),
                bottom: Val::Px(12.0),
                width: if open { Val::Px(196.0) } else { Val::Auto },
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(4.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(t.background),
            BoxShadow::default(),
        ))
        .with_children(|p| {
            // Closed: just the button that opens it.
            if !open {
                p.spawn((ToolButton::new("eda-layers-toggle", "layers").icon_size(18.0).tooltip("Layers").build(&t), LayersToggle));
                return;
            }
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, margin: UiRect::new(Val::Px(4.0), Val::Px(0.0), Val::Px(0.0), Val::Px(2.0)), ..default() }).with_children(|h| {
                h.spawn((t.text("Layers", t.font_sm, FontWeight::MEDIUM, t.muted_foreground), Node { flex_grow: 1.0, ..default() }));
                h.spawn((ToolButton::new("eda-layers-toggle", "close").icon_size(14.0).tooltip("Close").build(&t), LayersToggle));
            });
            for l in &list {
                let slug = crate::pcb::slug(&l.name());
                // The routing layer and (when it isn't copper) the drawing layer.
                let is_active = *l == active || (*l == draw && !draw.is_copper());
                let visible = !hidden.contains(l);
                let row = format!("eda-layer-{slug}");
                p.spawn((
                    Name::new(row.clone()),
                    LayerRow(*l),
                    bevy::ui_widgets::Button,
                    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: Val::Px(6.0), height: Val::Px(22.0), padding: UiRect::horizontal(Val::Px(4.0)), border_radius: BorderRadius::all(Val::Px(4.0)), ..default() },
                    BackgroundColor(if is_active { t.selection } else { Color::NONE }),
                ))
                .with_children(|r| {
                    r.spawn((Node { width: Val::Px(12.0), height: Val::Px(12.0), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() }, BackgroundColor(color(th.layer(*l))), Pickable::IGNORE));
                    r.spawn((t.text(l.name(), t.font_sm, if is_active { FontWeight::MEDIUM } else { FontWeight::NORMAL }, if visible { t.foreground } else { t.muted_foreground }), Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE));
                    r.spawn((ToolButton::new(format!("{row}-eye"), if visible { "visible" } else { "hidden" }).icon_size(14.0).tooltip(if visible { "Hide" } else { "Show" }).build(&t), LayerEye(*l)));
                });
            }
            p.spawn((Node { height: Val::Px(1.0), margin: UiRect::vertical(Val::Px(4.0)), ..default() }, BackgroundColor(t.separator)));
            let mut dim_btn = cadrs_ui::Button::new("eda-layers-dim").label("Dim other layers").small();
            if dim {
                dim_btn = dim_btn.primary();
            }
            p.spawn((dim_btn.build(&t), DimButton));
        });
    });
    world.flush();
}

fn on_row(a: On<Activate>, q: Query<&LayerRow>, mut s: ResMut<LayoutState>) {
    let Ok(r) = q.get(a.entity) else { return };
    // Any layer is what the Draw and Text tools draw on; a copper one is also routed on.
    s.draw_layer = r.0;
    if r.0.is_copper() {
        s.active = r.0;
    }
}

fn on_eye(a: On<Activate>, q: Query<&LayerEye>, mut s: ResMut<LayoutState>) {
    if let Ok(e) = q.get(a.entity) {
        match s.hidden.iter().position(|l| *l == e.0) {
            Some(i) => {
                s.hidden.remove(i);
            }
            None => s.hidden.push(e.0),
        }
    }
}

fn on_dim(a: On<Activate>, q: Query<(), With<DimButton>>, mut s: ResMut<LayoutState>) {
    if q.get(a.entity).is_ok() {
        s.dim = !s.dim;
    }
}

#[derive(Component)]
struct LayersToggle;

fn on_toggle(a: On<Activate>, q: Query<(), With<LayersToggle>>, mut s: ResMut<LayoutState>) {
    if q.get(a.entity).is_ok() {
        s.layers_open = !s.layers_open;
    }
}
