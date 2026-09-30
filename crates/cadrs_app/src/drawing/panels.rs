//! The drawing's side panels and edge buttons, laid over the sheet area:
//!
//! - **Sheets flyout** (D2.7, X4; `lesson-drawing-properties-sheets-flyout.png`,
//!   `ex3-step2.png`): the icon on the left edge or Ctrl+S. "Sheets(n)" on a tinted header with
//!   an Insert sheet button, then one row per sheet with its referenced part or assembly under
//!   it, the sheet's views under that and each view's projected views under it (P3C.2).
//!   Double-clicking a sheet makes it active; its context menu has Properties…, Rename… and
//!   Delete. Clicking a view selects it (on its sheet).
//! - **Drawing properties** (D2.5, X5): the wrench on the right edge. Icon tabs for Units and
//!   precision, Dimensions, Annotations, Views, Construction geometry, Formats and Tables, each
//!   listing its settings under group headers; "Update properties from a template…" and "Lock
//!   drawing properties" at the bottom. Every change is an undoable drawing edit.
//! - The active sheet's name at the top left of the sheet area (`lesson-drawing-interface.png`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::commands::EditDrawing;
use cadrs_drawing::style::{FIELDS, FieldValue, StyleSection};
use cadrs_drawing::{Drawing, DrawingOp, SheetId};
use cadrs_ui::prelude::*;
use cadrs_ui::Button;
use cadrs_ui::{
    CheckboxChange, DoubleClickable, InlineEditOptions, Select, SelectChange, begin_inline_edit,
};

use super::{DrawingUi, active_drawing};
use crate::viewport::{ActiveKind, ViewportArea};
use crate::{ActiveDocument, AppState};

/// Width of the Sheets flyout (logical px).
pub const SHEETS_WIDTH: f32 = 220.0;
/// Width of the Drawing properties panel.
pub const PROPS_WIDTH: f32 = 262.0;

pub struct DrawingPanelsPlugin;

impl Plugin for DrawingPanelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelsSnapshot>()
            .add_systems(
                Update,
                (ensure_overlay, sync_panels)
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut s: ResMut<PanelsSnapshot>| {
                *s = PanelsSnapshot::default();
            })
            .add_observer(on_sheet_double_click)
            .add_observer(on_sheet_context_menu)
            .add_observer(on_sheet_menu_action)
            .add_observer(on_sheet_rename_commit)
            .add_observer(on_style_select)
            .add_observer(on_style_checkbox);
    }
}

/// The overlay over the sheet area holding the drawing's panels.
#[derive(Component)]
struct DrawingOverlay;

#[derive(Component)]
struct SheetsPanel;

#[derive(Component)]
struct PropsPanel;

#[derive(Component)]
struct SheetsToggle;

#[derive(Component)]
struct PropsToggle;

#[derive(Component)]
struct SheetChip;

/// A sheet's row in the flyout.
#[derive(Component, Clone, Copy)]
pub struct SheetRow(pub SheetId);

/// The anchor of a sheet row's context menu.
#[derive(Component, Clone, Copy)]
struct SheetMenuFor(SheetId);

/// What the panels were last built from.
#[derive(Resource, Default, PartialEq)]
struct PanelsSnapshot {
    key: Option<PanelsKey>,
}

#[derive(Clone, PartialEq)]
struct PanelsKey {
    element: ElementId,
    drawing: Drawing,
    active: usize,
    /// P3G.3 (ER5.6): each sheet's references, one per source (the sheet's own first, then
    /// each other source its views show), with their name, icon and link.
    sheets_open: bool,
    props_open: bool,
    section: usize,
    selected: Vec<cadrs_drawing::ViewId>,
    /// P3G.2 (ER5.6): each sheet reference's link (its source, version, tooltip and icon state)
    /// when it references a version, and the version each linked view shows.
    groups: Vec<Vec<RefGroup>>,
    view_versions: Vec<(cadrs_drawing::ViewId, String)>,
}

/// A reference row of the Sheets pane (P3G.3): one source of a sheet's views.
#[derive(Clone, PartialEq)]
struct RefGroup {
    source: ElementId,
    name: String,
    part: bool,
    asm: bool,
    /// Its version, tooltip and icon state, for a reference to a version.
    link: Option<(String, String, crate::linked::LinkIcon)>,
}

/// The references of `sheet` of drawing `id`, one per source (see [`RefGroup`]).
fn ref_groups(doc: &crate::ActiveDocument, status: &crate::linked::LinkStatus, id: ElementId, sheet: &cadrs_drawing::Sheet) -> Vec<RefGroup> {
    let mut refs: Vec<cadrs_drawing::ObjectRef> = Vec::new();
    for r in sheet.reference.iter().chain(sheet.views.iter().map(|v| &v.reference)) {
        if refs.iter().all(|x| x.element != r.element) {
            refs.push(*r);
        }
    }
    refs.into_iter()
        .filter_map(|r| {
            let e = ElementId(r.element);
            let link = doc.doc.linked_element(e).and_then(|l| {
                let site = cadrs_core::link_update::RefSite::Drawing { element: id, source: e };
                let u = cadrs_core::link_update::use_at(&doc.doc, site)?;
                let (tip, icon) = crate::linked::link_badge(&doc.doc, status, &u.reference, e, site);
                Some((l.version_name.clone(), tip, icon))
            });
            // Another document's copy names its document ("Block · Block source").
            let name = super::reference_props(&doc.doc, Some(r)).name?;
            let name = match doc.doc.linked_element(e) {
                Some(l) if l.source.document_or(doc.doc.id) != doc.doc.id => format!("{name} · {}", l.document_name),
                _ => name,
            };
            Some(RefGroup {
                source: e,
                name,
                part: r.part.is_some(),
                asm: cadrs_core::drawing_assembly::is_assembly(&doc.doc, e),
                link,
            })
        })
        .collect()
}

fn ensure_overlay(
    q_overlay: Query<(), With<DrawingOverlay>>,
    q_area: Query<Entity, With<ViewportArea>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if !q_overlay.is_empty() {
        return;
    }
    let Ok(area) = q_area.single() else {
        return;
    };
    let t = theme.clone();
    commands.entity(area).with_children(|vp| {
        vp.spawn((
            Name::new("drawing-overlay"),
            DrawingOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            ZIndex(2),
        ))
        .with_children(|o| {
            o.spawn((
                Name::new("drawing-sheet-chip"),
                SheetChip,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(6.0),
                    top: Val::Px(6.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(0xcf, 0xcf, 0xcf)),
                Pickable::IGNORE,
                children![(
                    t.text("", t.font_sm, FontWeight::MEDIUM, t.foreground),
                    Pickable::IGNORE,
                )],
            ));
            o.spawn((
                Name::new("drawing-sheets-panel"),
                SheetsPanel,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(SHEETS_WIDTH),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::right(Val::Px(1.0)),
                    display: Display::None,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(t.background),
                BorderColor::all(t.panel_border),
            ));
            o.spawn((
                Name::new("drawing-properties-panel"),
                PropsPanel,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    width: Val::Px(PROPS_WIDTH),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::left(Val::Px(1.0)),
                    display: Display::None,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(t.background),
                BorderColor::all(t.panel_border),
            ));
            o.spawn((
                ToolButton::new("drawing-sheets-toggle", "list")
                    .icon_size(18.0)
                    .tooltip("Sheets (Ctrl+S)")
                    .build(&t),
                SheetsToggle,
                observe(|_: On<Activate>, mut ui: ResMut<DrawingUi>| {
                    ui.sheets_open = !ui.sheets_open;
                }),
            ))
            .insert((edge_button(&t, true), edge_visuals(&t)));
            o.spawn((
                ToolButton::new("drawing-properties-toggle", "tool")
                    .icon_size(18.0)
                    .tooltip("Drawing properties")
                    .build(&t),
                PropsToggle,
                observe(|_: On<Activate>, mut ui: ResMut<DrawingUi>| {
                    ui.props_open = !ui.props_open;
                }),
            ))
            .insert((edge_button(&t, false), edge_visuals(&t)));
        });
    });
}

/// A white tab with a grey border, like the feature panel's toggle.
fn edge_visuals(t: &Theme) -> cadrs_ui::Visuals {
    let mut v = cadrs_ui::button::visuals_for(t, cadrs_ui::ButtonVariant::Ghost);
    v.background = cadrs_ui::StateColors::new(t.background, t.ghost_hover, t.ghost_active, t.background)
        .with_selected(t.ghost_hover);
    v.border = cadrs_ui::StateColors::all(t.panel_border);
    v
}

/// The box of a small tab on the sheet area's edge (left or right), halfway down.
fn edge_button(t: &Theme, left: bool) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: if left { Val::Px(0.0) } else { Val::Auto },
            right: if left { Val::Auto } else { Val::Px(0.0) },
            top: Val::Percent(50.0),
            margin: UiRect::top(Val::Px(-14.0)),
            width: Val::Px(26.0),
            height: Val::Px(28.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: if left {
                UiRect::new(Val::ZERO, Val::Px(1.0), Val::Px(1.0), Val::Px(1.0))
            } else {
                UiRect::new(Val::Px(1.0), Val::ZERO, Val::Px(1.0), Val::Px(1.0))
            },
            border_radius: if left {
                BorderRadius::right(Val::Px(t.radius))
            } else {
                BorderRadius::left(Val::Px(t.radius))
            },
            ..default()
        },
        BackgroundColor(t.background),
        BorderColor::all(t.panel_border),
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_panels(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    ui: Res<DrawingUi>,
    theme: Res<Theme>,
    mut snapshot: ResMut<PanelsSnapshot>,
    mut q_overlay: Query<&mut Visibility, With<DrawingOverlay>>,
    mut q_sheets: Query<(Entity, &mut Node), (With<SheetsPanel>, Without<PropsPanel>)>,
    mut q_props: Query<(Entity, &mut Node), (With<PropsPanel>, Without<SheetsPanel>)>,
    mut q_toggles: Query<
        (&mut Node, Has<SheetsToggle>),
        (Or<(With<SheetsToggle>, With<PropsToggle>)>, Without<SheetsPanel>, Without<PropsPanel>),
    >,
    mut q_chip: Query<
        (&mut Node, &Children),
        (With<SheetChip>, Without<SheetsPanel>, Without<PropsPanel>, Without<SheetsToggle>, Without<PropsToggle>),
    >,
    mut q_text: Query<&mut Text>,
    status: Res<crate::linked::LinkStatus>,
    mut commands: Commands,
) {
    let drawing = *kind == ActiveKind::Drawing;
    for mut v in &mut q_overlay {
        v.set_if_neq(if drawing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let key = doc.as_deref().filter(|_| drawing).and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        Some(PanelsKey {
            element: id,
            drawing: d.clone(),
            active: ui.sheet_index(id, d),
            groups: d.sheets.iter().map(|s| ref_groups(doc, &status, id, s)).collect(),
            view_versions: d
                .sheets
                .iter()
                .flat_map(|s| s.views.iter())
                .filter_map(|v| doc.doc.linked_element(ElementId(v.reference.element)).map(|l| (v.id, l.version_name.clone())))
                .collect(),
            sheets_open: ui.sheets_open,
            props_open: ui.props_open,
            section: ui.props_section,
            selected: ui.selected.clone(),
        })
    });
    if key == snapshot.key {
        return;
    }
    snapshot.key = key.clone();
    let Some(key) = key else {
        return;
    };
    let t = theme.clone();
    // Panels and the edge buttons beside them.
    if let Ok((e, mut n)) = q_sheets.single_mut() {
        n.display = if key.sheets_open { Display::Flex } else { Display::None };
        commands.entity(e).despawn_children();
        if key.sheets_open {
            let k = key.clone();
            commands.entity(e).with_children(|p| sheets_flyout(p, &t, &k));
        }
    }
    if let Ok((e, mut n)) = q_props.single_mut() {
        n.display = if key.props_open { Display::Flex } else { Display::None };
        commands.entity(e).despawn_children();
        if key.props_open {
            let k = key.clone();
            commands.entity(e).with_children(|p| properties_panel(p, &t, &k));
        }
    }
    for (mut n, is_sheets) in &mut q_toggles {
        if is_sheets {
            n.left = Val::Px(if key.sheets_open { SHEETS_WIDTH } else { 0.0 });
        } else {
            n.right = Val::Px(if key.props_open { PROPS_WIDTH } else { 0.0 });
        }
    }
    if let Ok((mut n, children)) = q_chip.single_mut() {
        n.left = Val::Px(6.0 + if key.sheets_open { SHEETS_WIDTH } else { 0.0 });
        if let Some(&c) = children.first()
            && let Ok(mut text) = q_text.get_mut(c)
        {
            let name = key
                .drawing
                .sheets
                .get(key.active)
                .map(|s| s.name.clone())
                .unwrap_or_default();
            if text.0 != name {
                text.0 = name;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sheets flyout

fn sheets_flyout(p: &mut ChildSpawnerCommands, t: &Theme, key: &PanelsKey) {
    p.spawn((
        Name::new("sheets-header"),
        Node {
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            padding: UiRect::new(Val::Px(10.0), Val::Px(6.0), Val::ZERO, Val::ZERO),
            align_items: AlignItems::Center,
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(header_tint()),
        BorderColor::all(Color::srgb_u8(0xd4, 0xd8, 0xde)),
    ))
    .with_children(|h| {
        h.spawn((
            t.text(
                format!("Sheets({})", key.drawing.sheets.len()),
                t.font_md,
                FontWeight::SEMIBOLD,
                t.primary,
            ),
            Node {
                flex_grow: 1.0,
                ..default()
            },
            Pickable::IGNORE,
        ));
        h.spawn((
            ToolButton::new("drawing-insert-sheet", "file-new")
                .icon_size(18.0)
                .tooltip("Insert sheet")
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(insert_sheet);
            }),
        ));
    });
    for (i, sheet) in key.drawing.sheets.iter().enumerate() {
        let active = i == key.active;
        p.spawn((
            TreeItem::new(format!("sheet-row-{}", i + 1), sheet.name.clone())
                .disclosure((!key.groups[i].is_empty()).then_some(true))
                .icon("details", 16.0)
                .icon_color(t.tool_foreground)
                .weight(if active { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .selected(active)
                .editable()
                .left(4.0)
                .height(26.0)
                .build(t),
            SheetRow(sheet.id),
            DoubleClickable,
            ContextMenuTarget,
        ));
        // P3G.3 (ER5.6): a reference row per source, each with its own version, icon and menu
        // (Pin / Update), and under it the views of that source.
        let mut k = 0;
        for (g_index, g) in key.groups[i].iter().enumerate() {
            let views: Vec<&cadrs_drawing::View> = sheet.views.iter().filter(|v| v.reference.element == g.source.0).collect();
            let row_name = if g_index == 0 { format!("sheet-ref-{}", i + 1) } else { format!("sheet-ref-{}-{}", i + 1, g_index + 1) };
            let mut row = p.spawn(
                TreeItem::new(row_name.clone(), g.name.clone())
                    .disclosure((!views.is_empty()).then_some(true))
                    .icon(if g.part { "part" } else if g.asm { "assembly" } else { "part-studio" }, 14.0)
                    .icon_color(t.tool_foreground)
                    .trailing(g.link.as_ref().map(|l| l.0.clone()), t.muted_foreground)
                    .left(22.0)
                    .height(24.0)
                    .build(t),
            );
            // A reference to a version shows its icon (a click, or right-click → Pin reference /
            // Update linked document…, opens the Reference manager).
            row.insert((crate::reference_manager::SheetRefRow { drawing: key.element, source: g.source }, ContextMenuTarget));
            // P3G.4 (P3G.3 carried): the label is cut in the narrow pane; its tooltip names the
            // source document and version in full.
            let full = match &g.link {
                Some((_, tip, _)) => tip.lines().next().unwrap_or_default().trim_start_matches("Linked document: ").to_string(),
                None => format!("{} (this document)", g.name),
            };
            row.insert(cadrs_ui::Tooltip::new(full));
            if let Some((_, tip, icon)) = g.link.clone() {
                let (drawing, source) = (key.element, g.source);
                row.with_children(|r| {
                    crate::linked::spawn_link_icon(r, format!("{row_name}-linked"), icon, &tip, crate::linked::LinkTarget::Drawing(drawing, source));
                    // Inset from the pane's edge, so the icon isn't clipped.
                    r.spawn((Node { width: Val::Px(8.0), flex_shrink: 0.0, ..default() }, Pickable::IGNORE));
                });
            }
            // Views: those without a parent on this sheet first, each with its projected views.
            let roots: Vec<&cadrs_drawing::View> = views.iter().copied().filter(|v| v.parent.is_none_or(|p| sheet.view(p).is_none())).collect();
            for root in roots {
                view_rows(p, t, key, sheet, root, 0, i, &mut k);
            }
        }
    }
}

/// The tint of the flyout's header.
fn header_tint() -> Color {
    Color::srgb_u8(0xee, 0xf1, 0xf5)
}

/// A view's row in the flyout, then its children's (depth-first).
#[allow(clippy::too_many_arguments)]
fn view_rows(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    key: &PanelsKey,
    sheet: &cadrs_drawing::Sheet,
    v: &cadrs_drawing::View,
    depth: usize,
    sheet_index: usize,
    k: &mut usize,
) {
    if depth > 16 {
        return;
    }
    *k += 1;
    let children: Vec<&cadrs_drawing::View> = sheet.views.iter().filter(|c| c.parent == Some(v.id)).collect();
    let (id, sheet_id) = (v.id, sheet.id);
    // P3G.2: a view of a version names it ("V1").
    let version = key.view_versions.iter().find(|(i, _)| *i == v.id).map(|(_, n)| n.clone());
    p.spawn((
        TreeItem::new(format!("sheet-view-{}-{}", sheet_index + 1, *k), v.name.clone())
            .trailing(version, t.muted_foreground)
            .disclosure((!children.is_empty()).then_some(true))
            .icon(view_icon(v), 14.0)
            .icon_color(t.tool_foreground)
            .selected(key.selected.contains(&v.id))
            .left(40.0 + 16.0 * depth as f32 + if children.is_empty() { 20.0 } else { 0.0 })
            .height(24.0)
            .build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |w: &mut World| {
                activate_sheet(w, sheet_id);
                w.resource_mut::<DrawingUi>().selected = vec![id];
            });
        }),
    ));
    for c in children {
        view_rows(p, t, key, sheet, c, depth + 1, sheet_index, k);
    }
}

fn view_icon(v: &cadrs_drawing::View) -> &'static str {
    match v.kind {
        cadrs_drawing::ViewKind::Base => "part",
        cadrs_drawing::ViewKind::Projected if v.fold.is_none() => "part",
        cadrs_drawing::ViewKind::Projected => "replicate",
        cadrs_drawing::ViewKind::Auxiliary => "arrow-up-right",
        cadrs_drawing::ViewKind::Section => "section-view",
        cadrs_drawing::ViewKind::Detail => "find",
    }
}

fn edit(world: &mut World, op: DrawingOp) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    let Some((element, _)) = active_drawing(&doc) else {
        return false;
    };
    match doc.execute(&EditDrawing { element, op }) {
        Ok(()) => true,
        Err(e) => {
            warn!("drawing edit refused: {e}");
            false
        }
    }
}

/// Insert sheet: a new sheet after the active one, which becomes active.
pub fn insert_sheet(world: &mut World) {
    let Some((element, after)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        let ui = world.resource::<DrawingUi>();
        Some((id, d.sheets.get(ui.sheet_index(id, d)).map(|s| s.id)))
    }) else {
        return;
    };
    let id = SheetId::new();
    if edit(world, DrawingOp::InsertSheet { id, after }) {
        world.resource_mut::<DrawingUi>().active_sheet.insert(element, id);
    }
}

/// Makes `sheet` the active sheet of the active drawing.
pub fn activate_sheet(world: &mut World, sheet: SheetId) {
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| active_drawing(d).map(|(id, _)| id))
    else {
        return;
    };
    world.resource_mut::<DrawingUi>().active_sheet.insert(element, sheet);
}

fn on_sheet_double_click(ev: On<DoubleClick>, q: Query<&SheetRow>, mut commands: Commands) {
    if let Ok(row) = q.get(ev.entity) {
        let id = row.0;
        commands.queue(move |world: &mut World| activate_sheet(world, id));
    }
}

fn on_sheet_context_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&SheetRow>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let (Ok(row), Some(doc)) = (q.get(ev.entity), doc) else {
        return;
    };
    let count = active_drawing(&doc).map(|(_, d)| d.sheets.len()).unwrap_or(0);
    let menu = Menu::new("sheet-context-menu")
        .min_width(160.0)
        .item(MenuItem::new("sheet-menu-activate", "Activate").icon("details"))
        .separator()
        .item(MenuItem::new("sheet-menu-properties", "Properties…").icon("info"))
        .item(MenuItem::new("sheet-menu-rename", "Rename…").icon("edit"))
        .item(
            MenuItem::new("sheet-menu-delete", "Delete")
                .icon("delete")
                .disabled(count <= 1),
        );
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((SheetMenuFor(row.0), DespawnOnExit(AppState::Document)));
}

fn on_sheet_menu_action(
    ev: On<MenuAction>,
    q_anchor: Query<&SheetMenuFor>,
    mut commands: Commands,
) {
    let Ok(target) = q_anchor.get(ev.entity) else {
        return;
    };
    let id = target.0;
    match ev.item.as_str() {
        "sheet-menu-activate" => commands.queue(move |w: &mut World| activate_sheet(w, id)),
        "sheet-menu-properties" => {
            commands.queue(move |w: &mut World| super::sheet_dialog::open_sheet_properties(w, id))
        }
        "sheet-menu-rename" => commands.queue(move |w: &mut World| rename_sheet(w, id)),
        "sheet-menu-delete" => commands.queue(move |w: &mut World| {
            edit(w, DrawingOp::DeleteSheet { id });
        }),
        _ => {}
    }
}

/// Rename…: edits the sheet's name in place in the flyout.
fn rename_sheet(world: &mut World, id: SheetId) {
    let name = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| active_drawing(d)?.1.sheet(id).map(|s| s.name.clone()));
    let Some(name) = name else {
        return;
    };
    let mut q = world.query::<(Entity, &SheetRow)>();
    let Some(row) = q.iter(world).find(|(_, r)| r.0 == id).map(|(e, _)| e) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut opts = InlineEditOptions::new("sheet-rename");
    opts.width = Val::Px(150.0);
    opts.height = 22.0;
    opts.font_size = Some(theme.font_sm);
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    begin_inline_edit(&mut commands, &theme, row, name, opts);
    world.flush();
}

fn on_sheet_rename_commit(ev: On<InlineEditCommit>, q: Query<&SheetRow>, mut commands: Commands) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    let (id, name) = (row.0, ev.value.clone());
    commands.queue(move |w: &mut World| {
        edit(w, DrawingOp::RenameSheet { id, name });
    });
}

// ---------------------------------------------------------------------------------------------
// Drawing properties

fn section_icon(s: StyleSection) -> &'static str {
    match s {
        StyleSection::UnitsPrecision => "ruler",
        StyleSection::Dimensions => "dimension",
        StyleSection::Annotations => "text",
        StyleSection::Views => "visible",
        StyleSection::Construction => "construction",
        StyleSection::Formats => "settings",
        StyleSection::Tables => "custom-table",
    }
}

fn properties_panel(p: &mut ChildSpawnerCommands, t: &Theme, key: &PanelsKey) {
    let d = &key.drawing;
    let section = StyleSection::ALL[key.section.min(StyleSection::ALL.len() - 1)];
    p.spawn((
        Name::new("drawing-properties-header"),
        Node {
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            t.text("Drawing properties", t.font_md, FontWeight::SEMIBOLD, t.primary),
            Pickable::IGNORE,
        )],
    ));
    p.spawn((
        Name::new("drawing-properties-tabs"),
        Node {
            flex_shrink: 0.0,
            padding: UiRect::horizontal(Val::Px(6.0)),
            column_gap: Val::Px(2.0),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|tabs| {
        for (i, s) in StyleSection::ALL.iter().enumerate() {
            tabs.spawn((
                ToolButton::new(format!("drawing-props-tab-{}", s.key()), section_icon(*s))
                    .icon_size(18.0)
                    .selected(i == key.section)
                    .tooltip(s.label())
                    .build(t),
                observe(move |_: On<Activate>, mut ui: ResMut<DrawingUi>| {
                    ui.props_section = i;
                }),
            ));
        }
    });
    p.spawn((
        Name::new("drawing-properties-body"),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ))
    .with_children(|b| {
        b.spawn((
            Name::new("drawing-properties-section"),
            t.text(section.label(), t.font_base, FontWeight::SEMIBOLD, t.foreground),
            Node {
                margin: UiRect::new(Val::Px(10.0), Val::ZERO, Val::Px(8.0), Val::Px(4.0)),
                ..default()
            },
            Pickable::IGNORE,
        ));
        let mut group = "";
        for f in FIELDS.iter().filter(|f| f.section == section) {
            if f.group != group {
                group = f.group;
                b.spawn((
                    Node {
                        height: Val::Px(26.0),
                        flex_shrink: 0.0,
                        padding: UiRect::left(Val::Px(10.0)),
                        align_items: AlignItems::Center,
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xf2, 0xf2, 0xf2)),
                    children![(
                        t.text(group, t.font_sm, FontWeight::SEMIBOLD, t.foreground),
                        Pickable::IGNORE,
                    )],
                ));
            }
            let name = format!("drawing-prop-{}", f.key);
            match d.style.get(f.key) {
                Some(FieldValue::Bool(on)) => {
                    b.spawn(
                        Checkbox::new(name)
                            .label(f.label)
                            .checked(on)
                            .disabled(d.locked)
                            .build(t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| n.margin = UiRect::left(Val::Px(12.0)));
                }
                Some(FieldValue::Choice(i)) => {
                    let options = d.style.options(f.key);
                    b.spawn(Node {
                        height: Val::Px(30.0),
                        flex_shrink: 0.0,
                        padding: UiRect::new(Val::Px(12.0), Val::Px(8.0), Val::ZERO, Val::ZERO),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text(
                                f.label,
                                t.font_sm,
                                FontWeight::NORMAL,
                                if d.locked { t.disabled_foreground } else { t.foreground },
                            ),
                            Node {
                                width: Val::Px(118.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                        if d.locked {
                            r.spawn((
                                Name::new(name),
                                t.text(
                                    options.get(i).cloned().unwrap_or_default(),
                                    t.font_sm,
                                    FontWeight::NORMAL,
                                    t.disabled_foreground,
                                ),
                                Pickable::IGNORE,
                            ));
                        } else {
                            let mut s = Select::new(name).width(Val::Px(112.0));
                            for o in options {
                                s = s.option(o, true);
                            }
                            r.spawn(s.selected(i).build(t));
                        }
                    });
                }
                None => {}
            }
        }
    });
    // Footer.
    p.spawn((
        Name::new("drawing-properties-footer"),
        Node {
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)),
            border: UiRect::top(Val::Px(1.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|f| {
        f.spawn((
            Button::new("drawing-props-update-template")
                .label("Update properties from a template…")
                .icon("file-import")
                .icon_size(14.0)
                .link()
                .disabled(d.locked)
                .tooltip("Replace the drawing properties with a template's")
                .build(t),
            observe(|_: On<Activate>, mut commands: Commands| {
                commands.queue(super::create_dialog::open_update_from_template);
            }),
        ));
        f.spawn(
            Checkbox::new("drawing-props-lock")
                .label("Lock drawing properties")
                .checked(d.locked)
                .build(t),
        );
    });
}

fn on_style_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let Some(key) = name.as_str().strip_prefix("drawing-prop-") else {
        return;
    };
    let key = key.to_string();
    let index = ev.index;
    commands.queue(move |w: &mut World| set_style(w, &key, FieldValue::Choice(index)));
}

fn on_style_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let checked = ev.checked;
    if name.as_str() == "drawing-props-lock" {
        commands.queue(move |w: &mut World| {
            edit(w, DrawingOp::SetLocked(checked));
        });
        return;
    }
    let Some(key) = name.as_str().strip_prefix("drawing-prop-") else {
        return;
    };
    let key = key.to_string();
    commands.queue(move |w: &mut World| set_style(w, &key, FieldValue::Bool(checked)));
}

fn set_style(world: &mut World, key: &str, value: FieldValue) {
    let Some(mut style) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| active_drawing(d).map(|(_, dr)| dr.style.clone()))
    else {
        return;
    };
    if style.set(key, value).is_ok() {
        edit(world, DrawingOp::SetStyle(style));
    }
}
