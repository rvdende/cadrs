//! The **Cut list** panel (issue #266): every Frame feature's members in the Part Studio, alike
//! ones counted together ([`cadrs_core::frame::cut_list`]), docked on the viewport's right.
//!
//! - **The toggle**: the tab bar's *Cut list* button (`panel-cut-list`, icon `cut-list`), shown
//!   once the Part Studio has a frame member; it opens and closes the panel.
//! - **The table**, by section (`cut-list-profile-<j>`: its name, then its rows `cut-list-row-<i>`,
//!   from 0): Item, Qty, Length and the end cuts' angles from square ("45° / 45°", "Coped"), a
//!   bent member's length marked so. Clicking a row selects
//!   its parts (again: deselects them); a row whose parts are selected shows so.
//! - **Mass**: a row's members together, where their parts have materials (or masses of their
//!   own); a section's heading names its material when its rows share one.
//! - **Totals** (`cut-list-total-<i>`): the stock of each section, its members' lengths summed,
//!   and its mass; the whole frame's mass (`cut-list-total-mass`).
//! - **Copy** (`cut-list-copy`): the table tab-separated on the clipboard. **Export CSV**
//!   (`cut-list-export`): "<Part Studio> cut list.csv" in the export folder.
//!
//! The list follows the parts there are ([`cadrs_core::frame::live_members`]): a member a later
//! feature joined to another part or deleted isn't listed, and a Pattern's or Mirror's copy of
//! one is a member of its own.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::PartId;
use cadrs_core::frame::{CutListRow, EndCut, MemberInfo};
use cadrs_sketch::units::Quantity;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use crate::parts::PartCache;
use crate::side_panel::{SidePanel, side_panel_header, side_panel_node};
use crate::viewport::{ActiveKind, Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct CutListPlugin;

impl Plugin for CutListPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (strip_button, sync_panel)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_observer(on_button);
    }
}

#[derive(Component)]
struct CutListPanel;

/// What the panel shows, to rebuild it only when that changes.
#[derive(Component, PartialEq, Eq, Clone)]
struct PanelKey(String);

/// A row: its parts.
#[derive(Component, Clone)]
struct CutRow(Vec<PartId>);

/// The members as the parts are now (pattern and mirror copies counted).
pub fn members(cache: &PartCache) -> Vec<MemberInfo> {
    cadrs_core::frame::live_members(&cache.frames, &cache.parts)
}

/// A part's name as the Parts list shows it.
fn part_name(cache: &PartCache, id: PartId) -> String {
    cache
        .parts
        .iter()
        .find(|p| p.id == id)
        .map(|p| cadrs_core::parts::display_name(p, &cache.props).to_string())
        .unwrap_or_default()
}

/// A section's name in the current language ("Square tube 40x40x3"); a custom section's or a
/// plate's as the cut list has it.
fn profile_text(members: &[MemberInfo], profile: &str) -> String {
    use cadrs_core::frame::{PlateKind, ProfileFamily};
    let n = |v: f64| {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let Some(m) = members.iter().find(|m| m.profile == profile) else {
        return profile.to_string();
    };
    // A plate's: its frame's section's.
    let section = |m: &MemberInfo| {
        let s = members
            .iter()
            .find(|o| o.feature == m.feature && !o.plate)
            .unwrap_or(m);
        profile_text(std::slice::from_ref(s), &s.profile)
    };
    match (m.plate_kind, m.family) {
        (Some(PlateKind::Cap { thickness }), _) => tr_with(
            "cut-list-cap",
            &[("thickness", &n(thickness)), ("profile", &section(m))],
        ),
        (Some(PlateKind::Gusset { size, thickness }), _) => tr_with(
            "cut-list-gusset",
            &[("size", &n(size)), ("thickness", &n(thickness))],
        ),
        (None, Some(ProfileFamily::Custom)) => {
            tr_with("model-frame-custom-named", &[("sketch", &m.size)])
        }
        (None, Some(f)) => format!("{} {}", crate::frame_ui::family_text(f), m.size),
        (None, None) => profile.to_string(),
    }
}

/// A mass in kg: "1.25 kg".
fn mass_text(kg: f64) -> String {
    let s = format!("{kg:.2}");
    format!("{} kg", s.trim_end_matches('0').trim_end_matches('.'))
}

/// An end cut as the table shows it.
fn end_text(e: EndCut) -> String {
    match e {
        EndCut::Coped => tr("cut-list-coped"),
        EndCut::Trimmed => tr("cut-list-trimmed"),
        e => e.text(),
    }
}

/// A row's end cuts: "45° / 45°".
fn ends_text(r: &CutListRow) -> String {
    format!("{} / {}", end_text(r.ends[0]), end_text(r.ends[1]))
}

/// A length in the workspace's units, to the 0.1 mm the cut list counts members alike by.
fn length_text(units: &cadrs_sketch::units::Units, mm: f64) -> String {
    format!(
        "{} {}",
        units.value((mm * 10.0).round() / 10.0, Quantity::Length),
        units.length.symbol()
    )
}

/// The tab bar's button shows in a Part Studio with frame members; the panel closes when there
/// are none.
fn strip_button(
    kind: Res<ActiveKind>,
    cache: Res<PartCache>,
    mut open: ResMut<SidePanel>,
    mut q: Query<(&Name, &mut Node)>,
    added: Query<(), Added<Name>>,
) {
    // Only when what it shows may have changed, or the tab bar was made again.
    if !cache.is_changed() && !kind.is_changed() && added.is_empty() {
        return;
    }
    // (A look, no copies: a part, or a copy of one, of a member.)
    let has = *kind == ActiveKind::PartStudio
        && cache.frames.iter().any(|m| {
            cache
                .parts
                .iter()
                .any(|p| p.id == m.part || p.source == Some(m.part))
        });
    for (n, mut node) in &mut q {
        if n.as_str() == "panel-cut-list" {
            let want = if has { Display::Flex } else { Display::None };
            if node.display != want {
                node.display = want;
            }
        }
    }
    if !has && *open == SidePanel::CutList && !cache.rebuilding {
        *open = SidePanel::None;
    }
}

/// Rebuilds the panel when it opens and when what it shows changes.
fn sync_panel(
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    cache: Res<PartCache>,
    selection: Res<Selection>,
    q_panel: Query<Entity, With<CutListPanel>>,
    mut commands: Commands,
) {
    let want = *open == SidePanel::CutList && *kind == ActiveKind::PartStudio;
    if !want {
        for e in &q_panel {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if cache.is_changed() || open.is_changed() || selection.is_changed() || q_panel.is_empty() {
        commands.queue(rebuild);
    }
}

fn rebuild(world: &mut World) {
    let cache = world.resource::<PartCache>();
    let members = members(cache);
    let rows = cadrs_core::frame::cut_list(&members);
    let totals = cadrs_core::frame::totals(&rows);
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let selected: Vec<PartId> = world
        .resource::<Selection>()
        .0
        .iter()
        .filter_map(|p| match p {
            Pick::Part(id) => Some(*id),
            _ => None,
        })
        .collect();
    let materials: Vec<cadrs_core::frame::RowMaterial> = rows
        .iter()
        .map(|r| cadrs_core::frame::row_material(r, &members, &cache.parts, &cache.props))
        .collect();
    let names: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.parts.iter().map(|p| part_name(cache, *p)).collect())
        .collect();
    let key = PanelKey(format!(
        "{rows:?}{names:?}{materials:?}{selected:?}{}{}",
        units.length.symbol(),
        units.decimals
    ));
    let mut q = world.query_filtered::<(Entity, &PanelKey), With<CutListPanel>>();
    let panels: Vec<(Entity, PanelKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    if panels.iter().any(|(_, k)| *k == key) {
        return;
    }
    for (e, _) in panels {
        world.entity_mut(e).despawn();
    }
    let t = world.resource::<Theme>().clone();
    let panel = world
        .spawn((
            Name::new("cut-list-panel"),
            CutListPanel,
            key,
            DespawnOnExit(AppState::Document),
            side_panel_node(&t),
        ))
        .id();
    world.entity_mut(panel).entry::<Node>().and_modify(|mut n| {
        n.width = Val::Px(340.0);
        n.flex_direction = FlexDirection::Column;
    });
    let cell = |w: f32, grow: bool| Node {
        width: Val::Px(w),
        flex_grow: if grow { 1.0 } else { 0.0 },
        flex_shrink: if grow { 1.0 } else { 0.0 },
        overflow: Overflow::clip(),
        ..default()
    };
    const COLS: [(f32, bool, &str); 5] = [
        (24.0, false, "cut-list-item"),
        (30.0, false, "cut-list-qty"),
        (0.0, true, "model-length"),
        (92.0, false, "cut-list-angles"),
        (62.0, false, "cut-list-mass"),
    ];
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        side_panel_header(p, &t, &tr("cut-list-title"), "cut-list-panel-close");
        p.spawn(Node {
            padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)),
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn(
                cadrs_ui::Button::new("cut-list-copy")
                    .label(tr("cut-list-copy"))
                    .icon("copy")
                    .small()
                    .build(&t),
            );
            r.spawn(
                cadrs_ui::Button::new("cut-list-export")
                    .label(tr("cut-list-export"))
                    .icon("file-export")
                    .small()
                    .build(&t),
            );
        });
        if rows.is_empty() {
            p.spawn((
                Name::new("cut-list-empty"),
                t.text(
                    tr("cut-list-empty"),
                    11.5,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Node {
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
            return;
        }
        p.spawn((
            Name::new("cut-list-header"),
            Node {
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                border: UiRect::vertical(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.panel_border),
        ))
        .with_children(|h| {
            for (w, grow, key) in COLS {
                h.spawn((
                    Name::new(format!("cut-list-header-{key}")),
                    t.text(tr(key), 11.0, FontWeight::MEDIUM, t.muted_foreground),
                    cell(w, grow),
                ));
            }
        });
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|list| {
            // Grouped by section: its name, then its rows.
            let mut last: Option<&str> = None;
            let mut group = 0;
            for (i, r) in rows.iter().enumerate() {
                if last != Some(r.profile.as_str()) {
                    last = Some(r.profile.as_str());
                    // The section, and its material if its rows all have one.
                    let material = rows
                        .iter()
                        .zip(&materials)
                        .filter(|(q, _)| q.profile == r.profile)
                        .map(|(_, m)| {
                            m.material.as_ref().map(|name| {
                                if m.nominal {
                                    tr_with("cut-list-nominal", &[("material", name)])
                                } else {
                                    name.clone()
                                }
                            })
                        })
                        .reduce(|a, b| if a == b { a } else { None })
                        .flatten();
                    let heading = match material {
                        Some(m) => format!("{} · {m}", profile_text(&members, &r.profile)),
                        None => profile_text(&members, &r.profile),
                    };
                    list.spawn((
                        Name::new(format!("cut-list-profile-{group}")),
                        t.text(heading, 11.5, FontWeight::BOLD, t.foreground),
                        Node {
                            margin: UiRect::new(
                                Val::Px(10.0),
                                Val::Px(10.0),
                                Val::Px(6.0),
                                Val::Px(2.0),
                            ),
                            ..default()
                        },
                    ));
                    group += 1;
                }
                let on = !r.parts.is_empty() && r.parts.iter().all(|p| selected.contains(p));
                let length = length_text(&units, r.length);
                let length = if r.plate {
                    "–".to_string()
                } else if r.curved {
                    tr_with("cut-list-bent", &[("length", &length)])
                } else {
                    length
                };
                let texts = [
                    r.item.to_string(),
                    r.quantity().to_string(),
                    length,
                    if r.plate { String::new() } else { ends_text(r) },
                    match materials[i].mass {
                        // (≈: the family's usual material, not the parts' own.)
                        Some(kg) if materials[i].nominal => format!("≈{}", mass_text(kg)),
                        Some(kg) => mass_text(kg),
                        None => "–".into(),
                    },
                ];
                let mut row = list.spawn((
                    Name::new(format!("cut-list-row-{i}")),
                    CutRow(r.parts.clone()),
                    bevy::ui_widgets::Button,
                    Node {
                        min_height: Val::Px(22.0),
                        padding: UiRect::horizontal(Val::Px(10.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        ..default()
                    },
                    Tooltip::new(names[i].join(", ")),
                ));
                if on {
                    row.insert(BackgroundColor(t.primary.with_alpha(0.18)));
                }
                row.with_children(|c| {
                    const CELLS: [&str; 5] = ["item", "qty", "length", "angles", "mass"];
                    for (((w, grow, _), text), cell_name) in COLS.iter().zip(texts).zip(CELLS) {
                        c.spawn((
                            Name::new(format!("cut-list-row-{i}-{cell_name}")),
                            t.text(text, 11.5, FontWeight::NORMAL, t.foreground),
                            cell(*w, *grow),
                            Pickable::IGNORE,
                        ));
                    }
                });
            }
        });
        p.spawn((
            Name::new("cut-list-totals"),
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::Px(6.0), Val::Px(8.0)),
                row_gap: Val::Px(2.0),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(t.panel_border),
        ))
        .with_children(|s| {
            s.spawn(t.text(
                tr("cut-list-totals"),
                11.0,
                FontWeight::MEDIUM,
                t.muted_foreground,
            ));
            for (i, (profile, length)) in totals.iter().enumerate() {
                let of = || {
                    rows.iter()
                        .zip(&materials)
                        .filter(|(r, _)| r.profile == *profile)
                };
                let count: usize = of().map(|(r, _)| r.quantity()).sum();
                let mass: Option<f64> = of().map(|(_, m)| m.mass).sum();
                let mut text = tr_with(
                    "cut-list-total",
                    &[
                        ("profile", &profile_text(&members, profile)),
                        ("length", &length_text(&units, *length)),
                        ("count", &count.to_string()),
                    ],
                );
                if let Some(kg) = mass {
                    text = format!("{text}, {}", mass_text(kg));
                }
                s.spawn((
                    Name::new(format!("cut-list-total-{i}")),
                    t.text(text, 11.5, FontWeight::NORMAL, t.foreground),
                ));
            }
            // Everything the frames weigh (members and plates), where every part has a material.
            let all: Option<f64> = materials.iter().map(|m| m.mass).sum();
            if let Some(kg) = all {
                s.spawn((
                    Name::new("cut-list-total-mass"),
                    t.text(
                        tr_with("cut-list-total-mass", &[("mass", &mass_text(kg))]),
                        11.5,
                        FontWeight::MEDIUM,
                        t.foreground,
                    ),
                ));
            }
        });
    });
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else {
        return;
    };
    let at = world
        .get::<Children>(parent)
        .and_then(|c| c.iter().position(|e| e == area))
        .map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
}

fn on_button(a: On<Activate>, q: Query<(&Name, Option<&CutRow>)>, mut commands: Commands) {
    let Ok((n, row)) = q.get(a.entity) else {
        return;
    };
    if let Some(CutRow(parts)) = row {
        let parts = parts.clone();
        commands.queue(move |world: &mut World| select_row(world, &parts));
        return;
    }
    match n.as_str() {
        "cut-list-copy" => commands.queue(copy_table),
        "cut-list-export" => commands.queue(export_csv),
        _ => {}
    }
}

/// A row's parts become the selection (or leave it, if they are all of it already).
fn select_row(world: &mut World, parts: &[PartId]) {
    let mut sel = world.resource_mut::<Selection>();
    let all = parts.iter().all(|p| sel.0.contains(&Pick::Part(*p)));
    if all {
        sel.0
            .retain(|p| !matches!(p, Pick::Part(id) if parts.contains(id)));
    } else {
        sel.0.clear();
        sel.0.extend(parts.iter().map(|p| Pick::Part(*p)));
    }
}

/// The table as text: comma-separated (`csv`) or tab-separated, and its row count.
fn table_text(world: &World, csv: bool) -> (String, usize) {
    let cache = world.resource::<PartCache>();
    let rows = cadrs_core::frame::cut_list(&members(cache));
    let n = rows.len();
    let members = members(cache);
    let material =
        |r: &CutListRow| cadrs_core::frame::row_material(r, &members, &cache.parts, &cache.props);
    let text = cadrs_core::frame::cut_list_csv(&rows, |p| part_name(cache, p), material);
    if csv {
        (text, n)
    } else {
        // Tab-separated for pasting into a spreadsheet: the same table, unquoted.
        let mut out = String::new();
        for r in std::iter::once(None).chain(rows.iter().map(Some)) {
            let cells: Vec<String> = match r {
                None => [
                    "Item",
                    "Qty",
                    "Profile",
                    "Length (mm)",
                    "Angle 1",
                    "Angle 2",
                    "Bent",
                    "Material",
                    "Mass (kg)",
                    "Parts",
                ]
                .map(String::from)
                .to_vec(),
                Some(r) => vec![
                    r.item.to_string(),
                    r.quantity().to_string(),
                    r.profile.clone(),
                    cadrs_core::frame::length_text(r.length),
                    r.ends[0].text(),
                    r.ends[1].text(),
                    if r.curved { "yes" } else { "no" }.into(),
                    material(r).material.unwrap_or_default(),
                    material(r)
                        .mass
                        .map(|m| format!("{m:.3}"))
                        .unwrap_or_default(),
                    r.parts
                        .iter()
                        .map(|p| part_name(cache, *p))
                        .collect::<Vec<_>>()
                        .join("; "),
                ],
            };
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        (out, n)
    }
}

fn toast(world: &mut World, n: cadrs_ui::toast::Notification) {
    let theme = world.resource::<Theme>().clone();
    let mut c = world.commands();
    cadrs_ui::toast::show_notification(
        &mut c,
        &theme,
        n.seconds(6.0).max_width(360.0).name("cut-list-toast"),
    );
}

fn copy_table(world: &mut World) {
    let (tsv, n) = table_text(world, false);
    if let Some(mut clip) = world.get_resource_mut::<bevy::clipboard::Clipboard>()
        && let Err(e) = clip.set_text(tsv)
    {
        warn!("clipboard: {e:?}");
    }
    toast(
        world,
        cadrs_ui::toast::Notification::info(tr_with(
            "cut-list-copied",
            &[("count", &n.to_string())],
        )),
    );
}

/// "<Part Studio> cut list.csv" in the export folder (a number added if it is taken).
fn export_csv(world: &mut World) {
    let (csv, _) = table_text(world, true);
    let name = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().map(|e| e.name.clone()))
        .unwrap_or_else(|| "Part Studio".into());
    let dir = crate::export_dir(world);
    let result = (|| -> Result<std::path::PathBuf, String> {
        let dir = dir.ok_or_else(|| tr("asm-bom-no-export-folder"))?;
        cadrs_fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let stem = cadrs_core::export::sanitize(&format!("{name} cut list"));
        let mut path = dir.join(format!("{stem}.csv"));
        let mut n = 2;
        while cadrs_fs::exists(&path) {
            path = dir.join(format!("{stem} ({n}).csv"));
            n += 1;
        }
        cadrs_fs::write(&path, csv).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    })();
    let note = match result {
        Ok(p) => {
            info!("exported {}", p.display());
            let file = p
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            cadrs_ui::toast::Notification::info(tr_with("cut-list-exported", &[("file", &file)]))
        }
        Err(e) => cadrs_ui::toast::Notification::warning(tr_with(
            "cut-list-export-failed",
            &[("error", &e)],
        )),
    };
    toast(world, note);
}
