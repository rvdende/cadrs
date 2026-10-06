//! The library browser (KiCad's "Choose Symbol" / "Choose Footprint"): a filter over every
//! enabled library's names, keywords and descriptions; the libraries as a tree (collapsed until
//! opened, opened where the filter matches); previews of the chosen symbol and its default
//! footprint, or of the chosen footprint.
//!
//! What a choice does depends on why the browser was opened ([`Purpose`]): place the symbol on
//! the schematic, or copy the symbol or footprint into the component being edited (its default
//! footprint along with a symbol, when the component has none yet).
//!
//! Names: `eda-chooser` (the dialog), `eda-chooser-filter` (`-field`), `eda-chooser-list`,
//! `eda-chooser-lib-<library>` (a library row), `eda-chooser-<library>-<item>` (an item row),
//! `eda-chooser-preview` (symbol or footprint), `eda-chooser-preview-footprint`,
//! `eda-chooser-description`, `eda-chooser-ok`, `eda-chooser-cancel`. Enter in the filter takes
//! the chosen item, else the first match.

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui_widgets::{Activate, observe};
use cadrs_eda::library::LibraryTable;
use cadrs_eda::render::{self, DrawList};
use cadrs_ui::prelude::*;
use cadrs_ui::{Dialog, TextSubmit};

use super::ui;
use crate::AppState;

/// What the browser lists.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Symbols; power ports only when `power`.
    Symbols { power: bool },
    /// Footprints, narrowed by these footprint filters when there are any.
    Footprints { globs: Vec<String> },
}

/// What choosing does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    /// The schematic's Add symbol / Add power: the symbol follows the pointer to be placed.
    Place,
    /// The component editor: the symbol becomes the component's (with its default footprint).
    ComponentSymbol,
    /// The component editor: the footprint becomes the component's.
    ComponentFootprint,
}

#[derive(Component)]
pub struct Chooser;

#[derive(Component)]
struct ChooserList;

/// A row: a library (toggles) or an item (chooses).
#[derive(Component, Clone)]
enum Row {
    Library(String),
    Item(String),
}

#[derive(Resource)]
pub struct Browser {
    pub kind: Kind,
    pub purpose: Purpose,
    /// The filter the list was last built for (`None`: rebuild).
    shown: Option<String>,
    pub chosen: Option<String>,
    /// Libraries opened by hand (an empty filter shows only these open).
    open: HashSet<String>,
    /// The chosen item the previews were last drawn for.
    previewed: Option<String>,
}

/// Rows at most (a broad filter over every library would otherwise build thousands).
const MAX_ROWS: usize = 400;
/// Preview sizes (px).
const PREVIEW: (u32, u32) = (340, 200);

pub fn register(app: &mut App) {
    app.add_systems(Update, (refresh, preview).chain().run_if(in_state(AppState::Document))).add_observer(on_row).add_observer(on_double).add_observer(on_submit);
}

pub fn open(w: &mut World, kind: Kind, purpose: Purpose) {
    close(w);
    w.insert_resource(Browser { kind: kind.clone(), purpose, shown: None, chosen: None, open: HashSet::new(), previewed: None });
    let theme = w.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let title = match &kind {
        Kind::Symbols { power: true } => "Choose a power symbol",
        Kind::Symbols { power: false } => "Choose a symbol",
        Kind::Footprints { .. } => "Choose a footprint",
    };
    let footprints = matches!(kind, Kind::Footprints { .. });
    w.spawn((
        Dialog::new("eda-chooser")
            .title(title)
            .width(820.0)
            .body(move |b| {
                b.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(12.0), ..default() }).with_children(|row| {
                    row.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(430.0), ..default() }).with_children(|left| {
                        left.spawn(TextInput::new("eda-chooser-filter").placeholder("Filter").width(Val::Percent(100.0)).height(28.0).build(&tb));
                        left.spawn((
                            Name::new("eda-chooser-list"),
                            ChooserList,
                            Node { flex_direction: FlexDirection::Column, height: Val::Px(420.0), overflow: Overflow::scroll_y(), margin: UiRect::top(Val::Px(6.0)), ..default() },
                        ));
                    });
                    row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), width: Val::Px(PREVIEW.0 as f32), ..default() }).with_children(|right| {
                        let frame = |n: &'static str| (Name::new(n), ImageNode::default(), Node { width: Val::Px(PREVIEW.0 as f32), height: Val::Px(PREVIEW.1 as f32), border: UiRect::all(Val::Px(1.0)), ..default() }, BorderColor::all(tb.border));
                        right.spawn(frame("eda-chooser-preview"));
                        if !footprints {
                            right.spawn(frame("eda-chooser-preview-footprint"));
                        }
                        right.spawn((Name::new("eda-chooser-description"), tb.text("", tb.font_sm, FontWeight::NORMAL, tb.muted_foreground), Node { max_width: Val::Px(PREVIEW.0 as f32), ..default() }));
                    });
                });
            })
            .footer(move |f| {
                f.spawn((
                    cadrs_ui::Button::new("eda-chooser-ok").label("OK").primary().build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept);
                    }),
                ));
                f.spawn((
                    cadrs_ui::Button::new("eda-chooser-cancel").label("Cancel").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close);
                    }),
                ));
            })
            .build(&theme),
        Chooser,
        DespawnOnExit(AppState::Document),
    ));
}

pub fn close(w: &mut World) {
    let mut q = w.query_filtered::<Entity, With<Chooser>>();
    let es: Vec<Entity> = q.iter(w).collect();
    for e in es {
        w.entity_mut(e).despawn();
    }
}

/// The items matching the filter, as (library, id, description).
fn hits(lib: &LibraryTable, kind: &Kind, filter: &str) -> Vec<(String, String, String)> {
    let lib_of = |id: &str| id.split_once(':').map_or(String::new(), |(l, _)| l.to_string());
    match kind {
        Kind::Symbols { power } => lib
            .search_symbols(filter, *power)
            .into_iter()
            .map(|s| (lib_of(&s.id), s.id.clone(), s.field(cadrs_eda::symbol::fields::DESCRIPTION).map_or(String::new(), |f| f.value().to_string())))
            .collect(),
        Kind::Footprints { globs } => lib.search_footprints(filter, globs).into_iter().map(|f| (lib_of(&f.id), f.id.clone(), f.description.clone())).collect(),
    }
}

fn accept(w: &mut World) {
    let Some(b) = w.get_resource::<Browser>() else { return };
    let (kind, purpose, chosen) = (b.kind.clone(), b.purpose, b.chosen.clone());
    let lib = ui::libraries(w);
    let filter = ui::text_value(w, "eda-chooser-filter");
    let id = chosen.or_else(|| hits(&lib, &kind, &filter).first().map(|h| h.1.clone()));
    close(w);
    let Some(id) = id else { return };
    match purpose {
        Purpose::Place => {
            if let Some(sym) = lib.symbol(&id).cloned() {
                super::schematic_tools::set_tool(w, super::schematic_tools::Tool::Place(Box::new(sym)));
            }
        }
        Purpose::ComponentSymbol => {
            let Some(sym) = lib.symbol(&id).cloned() else { return };
            let fp = sym.field(cadrs_eda::symbol::fields::FOOTPRINT).and_then(|f| lib.footprint(f.value())).cloned();
            super::part_tools::commit(w, "Symbol from library", |c| {
                if c.footprint.as_ref().is_none_or(|f| f.pads.is_empty())
                    && let Some(fp) = fp
                {
                    c.footprint = Some(fp);
                }
                c.symbol = Some(sym);
                Ok(())
            });
        }
        Purpose::ComponentFootprint => {
            if let Some(fp) = lib.footprint(&id).cloned() {
                super::part_tools::commit(w, "Footprint from library", |c| {
                    c.footprint = Some(fp);
                    Ok(())
                });
            }
        }
    }
}

/// Rebuilds the list when the filter or the open libraries change.
fn refresh(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ChooserList>>();
    let Some(list) = q.iter(world).next() else { return };
    let filter = ui::text_value(world, "eda-chooser-filter");
    let Some(b) = world.get_resource::<Browser>() else { return };
    if b.shown.as_deref() == Some(filter.as_str()) {
        return;
    }
    let (kind, chosen, open) = (b.kind.clone(), b.chosen.clone(), b.open.clone());
    let lib = ui::libraries(world);
    let found = hits(&lib, &kind, &filter);
    // Libraries in table order with their hits; a filter opens every library it matches in.
    let mut groups: Vec<(String, Vec<(String, String)>)> = vec![];
    for l in lib.enabled() {
        if groups.iter().any(|g| g.0 == l.name) {
            continue;
        }
        let items: Vec<(String, String)> = found.iter().filter(|h| h.0 == l.name).map(|h| (h.1.clone(), h.2.clone())).collect();
        if !items.is_empty() {
            groups.push((l.name.clone(), items));
        }
    }
    let filtering = !filter.trim().is_empty();
    world.resource_mut::<Browser>().shown = Some(filter);
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|l| {
        let mut rows = 0;
        for (name, items) in groups {
            let is_open = filtering || open.contains(&name);
            l.spawn((
                ListItem::new(format!("eda-chooser-lib-{}", crate::pcb::slug(&name))).label(format!("{name} ({})", items.len())).disclosure(Some(is_open)).weight(FontWeight::MEDIUM).height(24.0).build(&theme),
                Row::Library(name.clone()),
            ));
            rows += 1;
            if !is_open {
                continue;
            }
            for (id, desc) in items {
                if rows >= MAX_ROWS {
                    break;
                }
                let short = id.split_once(':').map_or(id.as_str(), |(_, n)| n).to_string();
                let row = format!("eda-chooser-{}", crate::pcb::slug(&id));
                // Long descriptions are cut so they stay clear of the name.
                let desc = if desc.chars().count() > 44 { format!("{}…", desc.chars().take(43).collect::<String>()) } else { desc };
                l.spawn((ListItem::new(row).label(short).detail(desc).padding_left(22.0).height(24.0).selected(chosen.as_deref() == Some(id.as_str())).build(&theme), Row::Item(id), cadrs_ui::DoubleClickable));
                rows += 1;
            }
        }
    });
    world.flush();
}

fn on_row(a: On<Activate>, q: Query<&Row>, mut commands: Commands) {
    let Ok(r) = q.get(a.entity) else { return };
    let r = r.clone();
    commands.queue(move |w: &mut World| {
        let Some(mut b) = w.get_resource_mut::<Browser>() else { return };
        match r {
            Row::Library(name) => {
                if !b.open.remove(&name) {
                    b.open.insert(name);
                }
            }
            Row::Item(id) => b.chosen = Some(id),
        }
        // Redraw the list (the selection or the open libraries changed).
        b.shown = None;
    });
}

/// Enter in the filter takes the chosen item, else the first match.
fn on_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "eda-chooser-filter-field") {
        commands.queue(accept);
    }
}

/// A draw list as an image `size` px, fitted with a margin.
pub fn draw_list_image(d: &DrawList, size: (u32, u32)) -> Option<Image> {
    use resvg::{tiny_skia, usvg};
    let svg = render::to_svg(d);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let s = tree.size();
    let k = ((size.0 as f32 * 0.9) / s.width()).min((size.1 as f32 * 0.9) / s.height());
    let (dx, dy) = ((size.0 as f32 - s.width() * k) / 2.0, (size.1 as f32 - s.height() * k) / 2.0);
    let mut pix = tiny_skia::Pixmap::new(size.0, size.1)?;
    let bg = d.background;
    pix.fill(tiny_skia::Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k).post_translate(dx, dy), &mut pix.as_mut());
    Some(Image::new(Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }, TextureDimension::D2, pix.take(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD))
}

fn set_image(w: &mut World, name: &str, list: Option<DrawList>) {
    let image = list.and_then(|d| draw_list_image(&d, PREVIEW)).map(|i| w.resource_mut::<Assets<Image>>().add(i));
    let mut q = w.query::<(&Name, &mut ImageNode)>();
    if let Some((_, mut node)) = q.iter_mut(w).find(|(n, _)| n.as_str() == name) {
        node.image = image.unwrap_or_default();
    }
}

/// Draws the previews when the chosen item changes.
fn preview(world: &mut World) {
    let Some(b) = world.get_resource::<Browser>() else { return };
    if b.previewed == b.chosen {
        return;
    }
    let (kind, chosen) = (b.kind.clone(), b.chosen.clone());
    world.resource_mut::<Browser>().previewed = chosen.clone();
    let lib = ui::libraries(world);
    let sch = render::SchematicTheme::default();
    let brd = render::BoardTheme::default();
    let (mut main, mut second, mut text) = (None, None, String::new());
    if let Some(id) = &chosen {
        match kind {
            Kind::Symbols { .. } => {
                if let Some(s) = lib.symbol(id) {
                    main = Some(render::tight(render::symbol_view(Some(s), &sch, &[]), cadrs_eda::units::mm(1.5)));
                    let fp_id = s.field(cadrs_eda::symbol::fields::FOOTPRINT).map(|f| f.value().to_string()).unwrap_or_default();
                    second = lib.footprint(&fp_id).map(|f| render::tight(render::footprint_view(Some(f), &brd, &[]), cadrs_eda::units::mm(1.0)));
                    let desc = s.field(cadrs_eda::symbol::fields::DESCRIPTION).map_or("", |f| f.value());
                    text = format!("{id}\n{desc}");
                    if !s.keywords.is_empty() {
                        text += &format!("\nKeywords: {}", s.keywords);
                    }
                    if !fp_id.is_empty() {
                        text += &format!("\nFootprint: {fp_id}");
                    }
                }
            }
            Kind::Footprints { .. } => {
                if let Some(f) = lib.footprint(id) {
                    main = Some(render::tight(render::footprint_view(Some(f), &brd, &[]), cadrs_eda::units::mm(1.0)));
                    text = format!("{id}\n{}\n{} pads", f.description, f.pads.len());
                }
            }
        }
    }
    set_image(world, "eda-chooser-preview", main);
    set_image(world, "eda-chooser-preview-footprint", second);
    ui::set_label(world, "eda-chooser-description", &text);
}

/// Double-clicking an item takes it.
fn on_double(ev: On<cadrs_ui::DoubleClick>, q: Query<&Row>, mut commands: Commands) {
    if let Ok(Row::Item(id)) = q.get(ev.entity) {
        let id = id.clone();
        commands.queue(move |w: &mut World| {
            if let Some(mut b) = w.get_resource_mut::<Browser>() {
                b.chosen = Some(id);
            }
            accept(w);
        });
    }
}
