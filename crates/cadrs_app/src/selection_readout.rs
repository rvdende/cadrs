//! The selection readout (P3.2, a debugging aid): while modeling, the selected faces, edges,
//! vertices and parts are listed at the top left of the viewport in selection fields, named as
//! Onshape's feature dialogs name them: "Face of Extrude 1", "Edge of Extrude 1", "Vertex of
//! Extrude 1" (`training/intro-to-part-studios/lesson-fillet-and-chamfer.png`). A field's ✕
//! deselects it.
//!
//! Onshape has no such panel (selection fields live in the feature dialogs), so it is off unless
//! [`SelectionReadoutEnabled`] is set: the scenario command `selection-readout` turns it on for
//! the naming scenarios (P3.3, after the P3.2 judge).

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_ui::{SelectionField, SelectionFieldClear, Theme};

use crate::parts::{PartCache, pick_label};
use crate::viewport::{ActiveKind, Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct SelectionReadoutPlugin;

impl Plugin for SelectionReadoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionReadoutEnabled>().add_systems(
            Update,
            sync_readout
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// Shows the readout (off by default; for scenarios and debugging).
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct SelectionReadoutEnabled(pub bool);

/// The readout card.
#[derive(Component)]
struct SelectionReadout;

/// A field of the readout: the pick it shows.
#[derive(Component, Clone, Copy)]
struct ReadoutField(Pick);

/// At most this many fields; more are summarised ("+3 more").
const MAX_FIELDS: usize = 5;

#[allow(clippy::too_many_arguments)]
fn sync_readout(
    selection: Res<Selection>,
    cache: Res<PartCache>,
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    theme: Res<Theme>,
    sessions: (
        Option<Res<crate::sketch::SketchSession>>,
        Option<Res<crate::extrude::ExtrudeSession>>,
    ),
    enabled: Res<SelectionReadoutEnabled>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_card: Query<Entity, With<SelectionReadout>>,
    mut last: Local<Option<Vec<(Pick, String)>>>,
    mut commands: Commands,
) {
    let modeling = enabled.0 && sessions.0.is_none() && sessions.1.is_none() && *kind == ActiveKind::PartStudio;
    let features = doc
        .as_deref()
        .and_then(|d| d.active_element())
        .map(|e| e.features().to_vec())
        .unwrap_or_default();
    let items: Vec<(Pick, String)> = if modeling {
        selection
            .0
            .iter()
            .filter(|p| p.part().is_some())
            .filter_map(|p| Some((*p, pick_label(&features, &cache, *p)?)))
            .collect()
    } else {
        Vec::new()
    };
    if last.as_ref() == Some(&items) && q_card.is_empty() == items.is_empty() {
        return;
    }
    *last = Some(items.clone());
    for e in &q_card {
        commands.entity(e).try_despawn();
    }
    if items.is_empty() {
        return;
    }
    let Some(area) = q_area.iter().next() else {
        return;
    };
    let t = theme.clone();
    let card = commands
        .spawn((
            Name::new("selection-readout"),
            SelectionReadout,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(8.0),
                top: Val::Px(8.0),
                width: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(t.border),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.12),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(6.0),
            ),
        ))
        .with_children(|c| {
            c.spawn((
                t.text("Selection", t.font_sm, FontWeight::BOLD, t.foreground),
                Pickable::IGNORE,
            ));
            for (i, (pick, label)) in items.iter().take(MAX_FIELDS).enumerate() {
                let caption = match pick {
                    Pick::Face(..) => "Face",
                    Pick::Edge(..) => "Edge",
                    Pick::Vertex(..) => "Vertex",
                    _ => "Part",
                };
                c.spawn((
                    SelectionField::new(format!("selection-field-{i}"))
                        .placeholder(caption)
                        .value(Some(label.clone()))
                        .width(Val::Percent(100.0))
                        .build(&t),
                    ReadoutField(*pick),
                ))
                .observe(on_clear);
            }
            if items.len() > MAX_FIELDS {
                c.spawn((
                    t.text(
                        format!("+{} more", items.len() - MAX_FIELDS),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Pickable::IGNORE,
                ));
            }
        })
        .id();
    commands.entity(area).add_child(card);
}

/// A field's ✕ deselects its entity.
fn on_clear(ev: On<SelectionFieldClear>, q: Query<&ReadoutField>, mut selection: ResMut<Selection>) {
    if let Ok(f) = q.get(ev.entity) {
        selection.0.retain(|p| *p != f.0);
    }
}
