//! What a tab is before switching to it: its icon and type name, its thumbnail (a Part
//! Studio's or an Assembly's parts drawn small) and, while the pointer is on its tab in the tab
//! bar, a card above the tab with the thumbnail, the name, the type and a line about what is
//! in it ("12 features", "4 instances").
//!
//! Names: `tab-preview` (the card), `tab-preview-name`.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{FontStyle, FontWeight};
use cadrs_core::{ElementId, ElementKind};
use cadrs_ui::Theme;
use cadrs_ui::i18n::{tr, tr_with};

use crate::document::TabButton;
use crate::{ActiveDocument, AppState};

pub struct TabPreviewPlugin;

impl Plugin for TabPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TabThumbs>()
            .add_systems(
                Update,
                (show_preview, fill_thumbs)
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut th: ResMut<TabThumbs>| {
                *th = TabThumbs::default();
            });
    }
}

/// A tab's icon.
pub fn tab_icon(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "part-studio",
        ElementKind::Assembly => "assembly",
        ElementKind::Drawing(_) => "details",
        ElementKind::Render(_) => "render-studio",
        ElementKind::PcbStudio(_) => "pcb-studio",
        ElementKind::CamStudio(_) => "cam-studio",
        ElementKind::VariableStudio(_) => "variable-studio",
    }
}

/// A tab's type's catalog key: `tr(tab_type_key(el))` is the type as its card's second line
/// says it.
pub fn tab_type_key(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "common-part-studio",
        ElementKind::Assembly => "common-assembly",
        ElementKind::Drawing(_) => "common-drawing",
        ElementKind::Render(_) => "common-render-studio",
        ElementKind::PcbStudio(_) => "shell-tab-pcb-studio",
        ElementKind::CamStudio(_) => "shell-tab-cam-studio",
        ElementKind::VariableStudio(_) => "shell-tab-variable-studio",
    }
}

/// A tab's type in English (the window's title; [`tab_type_key`] is the translated one's key).
pub fn tab_type(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "Part Studio",
        ElementKind::Assembly => "Assembly",
        ElementKind::Drawing(_) => "Drawing",
        ElementKind::Render(_) => "Render Studio",
        ElementKind::PcbStudio(_) => "PCB Studio",
        ElementKind::CamStudio(_) => "CAM Studio",
        ElementKind::VariableStudio(_) => "Variable Studio",
    }
}

/// What is in a tab, in a few words, where that is quick to tell.
fn tab_detail(el: &cadrs_core::Element) -> Option<String> {
    let count = |n: usize, one: &str, many: &str| {
        tr_with(
            if n == 1 { one } else { many },
            &[("count", &n.to_string())],
        )
    };
    match &el.kind {
        // Counted as the feature list counts them, the default geometry's four with them.
        ElementKind::PartStudio { .. } => Some(count(
            4 + el.features().len(),
            "shell-tab-preview-feature",
            "shell-tab-preview-features",
        )),
        ElementKind::Assembly => el.assembly_model().map(|a| {
            count(
                a.instances.len(),
                "shell-tab-preview-instance",
                "shell-tab-preview-instances",
            )
        }),
        ElementKind::Drawing(d) => Some(count(
            d.sheets.len(),
            "shell-tab-preview-sheet",
            "shell-tab-preview-sheets",
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Thumbnails

/// Tab thumbnails: a Part Studio's or an Assembly's parts drawn small (the insert dialog's
/// renderer), kept while the tab is unchanged. Only the tab whose card is up is drawn.
#[derive(Resource, Default)]
pub struct TabThumbs {
    map: HashMap<ElementId, (cadrs_core::Element, Option<Handle<Image>>)>,
    /// Bumped when a thumbnail is drawn.
    pub generation: u64,
    /// The tab whose thumbnail is wanted now.
    wanted: Option<ElementId>,
}

impl TabThumbs {
    fn get(&self, id: ElementId) -> Option<Handle<Image>> {
        self.map.get(&id).and_then(|(_, h)| h.clone())
    }
}

/// Studios bigger than this aren't rebuilt for their thumbnail: it is drawn from the parts on
/// screen once the studio has been open (P3E.3a), and shows the icon until then.
const THUMB_FEATURE_LIMIT: usize = 60;
const THUMB_SIZE: u32 = 160;

/// Draws the wanted tab's thumbnail, unless it is up to date.
fn fill_thumbs(world: &mut World) {
    let Some(id) = world.resource::<TabThumbs>().wanted else {
        return;
    };
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.doc.element(id) else {
        return;
    };
    let fresh = world
        .resource::<TabThumbs>()
        .map
        .get(&id)
        .is_some_and(|(e, _)| e == el);
    if fresh {
        return;
    }
    // P3E.3a (P3E.2 carried delta: a big studio's preview was its icon): a studio too big to
    // rebuild again for its thumbnail is drawn from the parts on screen, once they are built
    // from its current features.
    let big = matches!(el.kind, ElementKind::PartStudio { .. })
        && el.features().len() > THUMB_FEATURE_LIMIT;
    if big {
        let cache = world.resource::<crate::parts::PartCache>();
        let on_screen = cache.settled().is_some_and(|(sid, features, over)| {
            sid == el.id
                && features == el.features()
                && *over == crate::parts::PartOverride::default()
        });
        if !on_screen {
            return;
        }
    }
    let doc = doc.doc.clone();
    let Some(el) = doc.element(id).cloned() else {
        return;
    };
    let img = world.resource_scope(
        |world, mut parts: Mut<crate::assembly::AssemblyParts>| -> Option<image::RgbaImage> {
            match &el.kind {
                ElementKind::PartStudio { .. } if big => {
                    let cache = world.resource::<crate::parts::PartCache>();
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = cache
                        .parts
                        .iter()
                        .map(|p| {
                            (
                                &*p.solid,
                                cadrs_core::appearance::part_appearance(p, el.part_props()).rgb,
                            )
                        })
                        .collect();
                    (!list.is_empty())
                        .then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                ElementKind::PartStudio { .. } => {
                    let build = parts.build(&doc, el.id)?;
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = build
                        .parts
                        .iter()
                        .map(|p| {
                            (
                                &*p.solid,
                                cadrs_core::appearance::part_appearance(p, el.part_props()).rgb,
                            )
                        })
                        .collect();
                    (!list.is_empty())
                        .then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                ElementKind::Assembly => {
                    let asm = el.assembly_model()?;
                    let (ps, props) =
                        cadrs_core::assembly::instance_parts(&doc, asm, |e| parts.build(&doc, e));
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = ps
                        .iter()
                        .map(|p| {
                            (
                                &*p.solid,
                                cadrs_core::appearance::part_appearance(p, &props).rgb,
                            )
                        })
                        .collect();
                    (!list.is_empty())
                        .then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                _ => None,
            }
        },
    );
    let handle = img.map(|img| {
        let (w, h) = img.dimensions();
        world.resource_mut::<Assets<Image>>().add(Image::new(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            img.into_raw(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        ))
    });
    let mut th = world.resource_mut::<TabThumbs>();
    th.map.insert(id, (el, handle));
    th.generation += 1;
}

// ---------------------------------------------------------------------------------------------
// The card

const CARD_WIDTH: f32 = 270.0;
const THUMB_BOX: f32 = 72.0;

/// The card of the hovered tab, and the thumbnails' generation it was built with.
#[derive(Component)]
pub struct PreviewCard {
    tab: ElementId,
    generation: u64,
}

/// The hovered tab's card above it, at once (built again when its thumbnail comes). Not while
/// a menu is open or a tab is dragged.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show_preview(
    q_tabs: Query<(
        &TabButton,
        &Hovered,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
    )>,
    q_card: Query<(Entity, &PreviewCard)>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    drag: Res<crate::tab_folders::TabDrag>,
    mut thumbs: ResMut<TabThumbs>,
    doc: Option<Res<ActiveDocument>>,
    windows: Query<&Window>,
    theme: Res<Theme>,
    mut shown: Local<Option<ElementId>>,
    mut commands: Commands,
) {
    let busy = !q_menus.is_empty() || drag.moved;
    let over = q_tabs
        .iter()
        .find(|(_, h, ..)| h.get())
        .filter(|_| !busy)
        .map(|(b, _, n, at)| {
            let s = n.inverse_scale_factor();
            (
                b.0,
                Rect::from_center_size(at.translation * s, n.size() * s),
            )
        });
    if over.map(|(id, _)| id) != *shown {
        *shown = over.map(|(id, _)| id);
        for (e, _) in &q_card {
            commands.entity(e).try_despawn();
        }
    }
    let (Some((id, tab)), Some(doc)) = (over, doc) else {
        if thumbs.wanted.is_some() {
            thumbs.wanted = None;
        }
        return;
    };
    if thumbs.wanted != Some(id) {
        thumbs.wanted = Some(id);
    }
    if q_card
        .iter()
        .any(|(_, c)| c.tab == id && c.generation == thumbs.generation)
    {
        return;
    }
    for (e, _) in &q_card {
        commands.entity(e).try_despawn();
    }
    let Some(el) = doc.doc.element(id) else {
        return;
    };
    let Some(window) = windows.iter().next() else {
        return;
    };
    // Above the tab, lined up with its left edge and kept on the window.
    let left = tab.min.x.min(window.width() - CARD_WIDTH - 4.0).max(4.0);
    let bottom = window.height() - tab.min.y + 8.0;
    let t = &*theme;
    let thumb = thumbs.get(id);
    let (name, kind, detail, icon) = (
        el.name.clone(),
        tr(tab_type_key(el)),
        tab_detail(el),
        tab_icon(el),
    );
    commands
        .spawn((
            Name::new("tab-preview"),
            PreviewCard {
                tab: id,
                generation: thumbs.generation,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                bottom: Val::Px(bottom),
                width: Val::Px(CARD_WIDTH),
                padding: UiRect::all(Val::Px(8.0)),
                column_gap: Val::Px(12.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.22),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(10.0),
            ),
            GlobalZIndex(cadrs_ui::z::TOOLTIP),
            Pickable::IGNORE,
            DespawnOnExit(AppState::Document),
        ))
        .with_children(|c| {
            // The thumbnail, or the tab's icon until it is drawn (or where there is none).
            c.spawn((
                Node {
                    width: Val::Px(THUMB_BOX),
                    height: Val::Px(THUMB_BOX),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|b| match thumb {
                Some(h) => {
                    b.spawn((
                        ImageNode::new(h),
                        Node {
                            width: Val::Px(THUMB_BOX),
                            height: Val::Px(THUMB_BOX),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
                None => {
                    b.spawn((
                        cadrs_ui::icon(icon, 36.0, t.muted_foreground),
                        Pickable::IGNORE,
                    ));
                }
            });
            c.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    flex_shrink: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|col| {
                col.spawn((
                    Name::new("tab-preview-name"),
                    t.text(name, t.font_base, FontWeight::SEMIBOLD, t.foreground),
                    Pickable::IGNORE,
                ));
                col.spawn((
                    Text::new(kind),
                    TextFont {
                        style: FontStyle::Italic,
                        ..t.font(t.font_sm, FontWeight::NORMAL)
                    },
                    TextColor(t.muted_foreground),
                    Pickable::IGNORE,
                ));
                if let Some(d) = detail {
                    col.spawn((
                        t.text(d, t.font_xs, FontWeight::NORMAL, t.muted_foreground),
                        Node {
                            margin: UiRect::top(Val::Px(4.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
            });
        });
}
