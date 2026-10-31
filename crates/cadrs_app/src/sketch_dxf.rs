//! The sketch toolbar's **Insert DXF or DWG** (P3I.6; exercise E1;
//! `reference/onshape/sheetmetal/ex1-importing-dxf-bend/step-03.png`): the **Insert a DXF or
//! DWG file** panel, floating over the view beside the sketch dialog as Onshape's:
//!
//! - two tabs: **Current folder** (the DXF and DWG files of the import folder — cadrs keeps
//!   files in folders, not in document tabs — each with a dark thumbnail of its drawing, as
//!   Onshape's list shows them) and **Other folders** (the file picker);
//! - **Search DXF or DWG files** (filters the list as typed);
//! - the list and the picker also take vector drawings (SVG, EPS, PS, PDF, AI;
//!   [`cadrs_core::vector_import`]), which come in at their own size (Units is for DXF and DWG);
//! - **Units** (Meter … Yard; the document's length unit to start with) and **Use file origin
//!   position** (on; off centres the drawing on the sketch origin);
//! - **Import…** (the file picker).
//!
//! A click on a file inserts it into the sketch being edited (one undoable step, "Insert
//! DXF/DWG", [`cadrs_core::dxf_import`]); its lines, arcs, circles, polylines and splines share
//! their end points so they close regions. A DWG goes through the external converter.
//!
//! The import folder: the scenario command `sketch-dxf-dir <path>`, else `$CADRS_IMPORT_DIR`,
//! else the working folder. Names: `sketch-dxf-dialog`, `sketch-dxf-tabs`, `sketch-dxf-search`,
//! `sketch-dxf-file-<name>`, `sketch-dxf-units`, `sketch-dxf-origin`, `sketch-dxf-import`,
//! `sketch-dxf-toast`.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::dxf_import::{DxfUnits, InsertDxf, read_file, sketch_of};
use cadrs_core::vector_import::{InsertVector, VectorFormat};
use cadrs_drawing::sheet_sketch::Entity as DxfEntity;
use cadrs_ui::i18n::{Translated, tr, tr_with};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    Button, CheckboxState, FilePicked, FloatingPanel, FloatingPanelClose, Notification,
    ScriptCommand, Select, SelectState, TabStrip, TabStripSelect, TextInputField,
    show_notification,
};

use crate::sketch::SketchSession;
use crate::{ActiveDocument, AppState};

pub struct SketchDxfPlugin;

impl Plugin for SketchDxfPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SketchDxfDir>()
            .add_systems(
                Update,
                (
                    script_dir,
                    on_file_picked,
                    filter_rows,
                    close_without_sketch,
                )
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_tab)
            .add_observer(on_close);
    }
}

/// The folder the dialog lists (`sketch-dxf-dir`, or the last one a file was picked from).
#[derive(Resource, Debug, Default, Clone)]
pub struct SketchDxfDir(pub Option<PathBuf>);

#[derive(Component)]
struct SketchDxfDialog;

/// A file row and its lower-case name (for the search).
#[derive(Component)]
struct DxfRow(String);

const WIDTH: f32 = 290.0;

fn folder(world: &World) -> PathBuf {
    world
        .resource::<SketchDxfDir>()
        .0
        .clone()
        .or_else(|| std::env::var_os("CADRS_IMPORT_DIR").map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn script_dir(mut msgs: MessageReader<ScriptCommand>, mut dir: ResMut<SketchDxfDir>) {
    for m in msgs.read() {
        if let Some(p) = m.0.strip_prefix("sketch-dxf-dir ") {
            dir.0 = Some(PathBuf::from(p.trim()));
        }
    }
}

/// The DXF, DWG and other vector files of a folder, by name.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = cadrs_fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| cadrs_fs::is_file(p) && VectorFormat::of_path(p).is_some())
        .collect();
    v.sort();
    v.truncate(60);
    v
}

/// A dark thumbnail of a DXF's drawing (light strokes on near black, as Onshape's list), `w` × `h`.
fn thumbnail(path: &Path, w: u32, h: u32) -> Image {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for c in px.chunks_mut(4) {
        c.copy_from_slice(&[0x10, 0x10, 0x10, 0xff]);
    }
    let format = VectorFormat::of_path(path);
    let drawing = if format == Some(VectorFormat::Dxf) {
        cadrs_fs::read_to_string(path)
            .ok()
            .and_then(|t| cadrs_drawing::dxf::read_dxf(&t).ok())
    } else {
        None
    };
    // An SVG drawn from its sketch geometry (EPS and PDF would need their converters run for
    // each thumbnail: they stay blank).
    let svg = if format == Some(VectorFormat::Svg) {
        cadrs_fs::read(path)
            .ok()
            .and_then(|b| cadrs_core::vector_import::sketch_of_svg(&b).ok())
    } else {
        None
    };
    let svg_lines = svg.map(|(s, _)| {
        s.curves
            .keys()
            .map(|id| {
                cadrs_sketch::hit::curve_polyline(&s, id)
                    .into_iter()
                    .map(|p| [p.x, p.y])
                    .collect()
            })
            .collect::<Vec<Vec<[f64; 2]>>>()
    });
    if drawing.is_some() || svg_lines.is_some() {
        let lines: Vec<Vec<[f64; 2]>> = drawing
            .map(|d| {
                d.entities
                    .iter()
                    .filter_map(|e| match e {
                        DxfEntity::Line { a, b } => Some(vec![*a, *b]),
                        DxfEntity::Arc {
                            center,
                            radius,
                            start,
                            end,
                        } => Some(cadrs_drawing::sheet_sketch::arc_polyline(
                            *center, *radius, *start, *end,
                        )),
                        DxfEntity::Circle { center, radius } => Some(
                            cadrs_drawing::sheet_sketch::arc_polyline(*center, *radius, 0.0, 360.0),
                        ),
                        DxfEntity::Polyline { points, closed, .. } => {
                            let mut p = points.clone();
                            if *closed && let Some(f) = points.first() {
                                p.push(*f);
                            }
                            Some(p)
                        }
                        e @ DxfEntity::Spline { .. } => Some(cadrs_drawing::dxf::spline_points(e)),
                        _ => None,
                    })
                    .collect()
            })
            .or(svg_lines)
            .unwrap_or_default();
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in lines.iter().flatten() {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        if lo[0] < hi[0] {
            let s =
                ((w - 6) as f64 / (hi[0] - lo[0])).min((h - 6) as f64 / (hi[1] - lo[1]).max(1e-9));
            let (ox, oy) = (
                (w as f64 - (hi[0] - lo[0]) * s) / 2.0,
                (h as f64 - (hi[1] - lo[1]) * s) / 2.0,
            );
            let to = |p: [f64; 2]| {
                (
                    ox + (p[0] - lo[0]) * s,
                    h as f64 - (oy + (p[1] - lo[1]) * s),
                )
            };
            let mut plot = |x: f64, y: f64| {
                let (x, y) = (x.round() as i64, y.round() as i64);
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    let i = ((y as u32 * w + x as u32) * 4) as usize;
                    px[i..i + 4].copy_from_slice(&[0xd8, 0xd8, 0xd8, 0xff]);
                }
            };
            for l in &lines {
                for seg in l.windows(2) {
                    let (a, b) = (to(seg[0]), to(seg[1]));
                    let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil() as usize).max(1);
                    for k in 0..=n {
                        let t = k as f64 / n as f64;
                        plot(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                    }
                }
            }
        }
    }
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Insert DXF or DWG (the sketch toolbar's button): the dialog.
pub fn open(world: &mut World) {
    if !world.contains_resource::<SketchSession>() {
        return;
    }
    let dir = folder(world);
    let list = files(&dir);
    let thumbs: Vec<(PathBuf, Handle<Image>)> = {
        let mut images = world.resource_mut::<Assets<Image>>();
        list.iter()
            .map(|p| (p.clone(), images.add(thumbnail(p, 64, 40))))
            .collect()
    };
    let inch = world
        .get_resource::<ActiveDocument>()
        .is_some_and(|d| d.doc.units.length == cadrs_sketch::units::LengthUnit::Inch);
    let unit = if inch {
        DxfUnits::Inch
    } else {
        DxfUnits::Millimeter
    };
    let shown_dir = cadrs_ui::file_picker::display_dir(&dir);
    // Open already: once.
    let mut q = world.query_filtered::<Entity, With<SketchDxfDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let mut q_area = world.query_filtered::<Entity, With<crate::viewport::ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else {
        return;
    };
    // Near the view's full height, the list taking what the rest leaves (`step-03.png`: Units
    // and Import… at the bottom).
    let view_h = world.resource::<crate::viewport::ViewportRect>().0.height();
    let list_h = (view_h - PANEL_AT.y - PANEL_BOTTOM - OTHER_ROWS).max(200.0);
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    let mut commands = world.commands();
    // A floating panel over the view, below the sketch dialog's right edge, as Onshape's
    // (`step-03.png`), not a modal: the view stays usable.
    let panel = commands
        .spawn((
            FloatingPanel::new("sketch-dxf-dialog", tr("sketch-dxf-title"))
                .width(WIDTH)
                .at(PANEL_AT.x, PANEL_AT.y)
                .body(move |b| {
                    let t = &tb;
                    b.spawn((
                        Name::new("sketch-dxf-tabs-row"),
                        Node {
                            width: Val::Percent(100.0),
                            ..default()
                        },
                    ))
                    .with_child(
                        TabStrip::new("sketch-dxf-tabs")
                            .tab(tr("sketch-dxf-current-folder"))
                            .tab(tr("sketch-dxf-other-folders"))
                            .selected(0)
                            .build(t),
                    );
                    b.spawn((Node {
                        flex_direction: FlexDirection::Column,
                        margin: UiRect::vertical(Val::Px(6.0)),
                        ..default()
                    },))
                        .with_children(|h| {
                            h.spawn((
                                Name::new("sketch-dxf-folder"),
                                t.text(shown_dir, t.font_base, FontWeight::BOLD, t.foreground),
                            ));
                        });
                    b.spawn(
                        TextInput::new("sketch-dxf-search")
                            .placeholder(tr("sketch-dxf-search"))
                            .width(Val::Percent(100.0))
                            .height(26.0)
                            .build(t),
                    );
                    b.spawn((
                        Name::new("sketch-dxf-list"),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(2.0),
                            margin: UiRect::vertical(Val::Px(6.0)),
                            height: Val::Px(list_h),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(|l| {
                        if thumbs.is_empty() {
                            l.spawn((
                                t.text(
                                    tr("sketch-dxf-no-files"),
                                    t.font_sm,
                                    FontWeight::NORMAL,
                                    t.muted_foreground,
                                ),
                                Translated("sketch-dxf-no-files"),
                            ));
                        }
                        for (path, img) in thumbs {
                            let file = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("file")
                                .to_string();
                            l.spawn((
                                DxfRow(file.to_lowercase()),
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(8.0),
                                    ..default()
                                },
                            ))
                            .with_children(|r| {
                                r.spawn((
                                    ImageNode::new(img),
                                    Node {
                                        width: Val::Px(64.0),
                                        height: Val::Px(40.0),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                ));
                                let p = path.clone();
                                r.spawn((
                                    Button::new(format!("sketch-dxf-file-{file}"))
                                        .label(file.clone())
                                        .ghost()
                                        .build(t),
                                    observe(move |_: On<Activate>, mut commands: Commands| {
                                        let p = p.clone();
                                        commands.queue(move |w: &mut World| insert(w, &p));
                                    }),
                                ));
                            });
                        }
                    });
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text(
                                tr("common-units"),
                                t.font_sm,
                                FontWeight::BOLD,
                                t.foreground,
                            ),
                            Translated("common-units"),
                        ));
                        let mut s = Select::new("sketch-dxf-units")
                            .bordered()
                            .width(Val::Px(200.0));
                        for u in DxfUnits::ALL {
                            s = s.option(u.label(), true);
                        }
                        r.spawn(
                            s.selected(DxfUnits::ALL.iter().position(|u| *u == unit).unwrap_or(2))
                                .build(t),
                        );
                    });
                    b.spawn(
                        Checkbox::new("sketch-dxf-origin")
                            .label(tr("sketch-dxf-use-origin"))
                            .checked(true)
                            .height(24.0)
                            .build(t),
                    );
                    b.spawn((
                        Button::new("sketch-dxf-import")
                            .label(tr("sketch-dxf-import"))
                            .icon("file-import")
                            .ghost()
                            .small()
                            .build(t),
                        observe(|_: On<Activate>, mut commands: Commands| {
                            commands.queue(pick_other)
                        }),
                    ));
                })
                .build(&theme),
            SketchDxfDialog,
            DespawnOnExit(AppState::Document),
        ))
        .id();
    commands.entity(area).add_child(panel);
    world.flush();
}

/// Where the panel's top-left corner goes in the view (px).
const PANEL_AT: Vec2 = Vec2::new(236.0, 86.0);

/// The room left under the panel, and the height of everything in it but the file list (px).
const PANEL_BOTTOM: f32 = 30.0;
const OTHER_ROWS: f32 = 250.0;

fn close_panels(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<SketchDxfDialog>>();
    let panels: Vec<Entity> = q.iter(world).collect();
    for e in panels {
        world.entity_mut(e).despawn();
    }
}

fn on_close(
    ev: On<FloatingPanelClose>,
    q: Query<(), With<SketchDxfDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.entity(ev.entity).try_despawn();
    }
}

/// The panel goes with the sketch.
fn close_without_sketch(
    session: Option<Res<SketchSession>>,
    q: Query<Entity, With<SketchDxfDialog>>,
    mut commands: Commands,
) {
    if session.is_none() {
        for e in &q {
            commands.entity(e).try_despawn();
        }
    }
}

/// Other folders / Import…: the file picker.
fn pick_other(world: &mut World) {
    let dir = folder(world);
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::open_file_picker(
        &mut commands,
        &theme,
        "sketch-dxf-picker",
        &tr("sketch-dxf-picker-title"),
        "sketch-insert-dxf",
        dir,
        &VectorFormat::EXTENSIONS,
    );
    world.flush();
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if ev.index == 1
        && q.get(ev.entity)
            .is_ok_and(|n| n.as_str() == "sketch-dxf-tabs")
    {
        commands.queue(pick_other);
    }
}

fn on_file_picked(mut msgs: MessageReader<FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != "sketch-insert-dxf" {
            continue;
        }
        let path = m.path.clone();
        commands.queue(move |w: &mut World| {
            w.resource_mut::<SketchDxfDir>().0 = path.parent().map(Path::to_path_buf);
            insert(w, &path);
        });
    }
}

/// The search filters the rows.
fn filter_rows(
    q_search: Query<(&Name, &EditableText), With<TextInputField>>,
    mut q: Query<(&DxfRow, &mut Node)>,
) {
    let Some((_, t)) = q_search
        .iter()
        .find(|(n, _)| n.as_str() == "sketch-dxf-search-field")
    else {
        return;
    };
    let want = t.value().to_string().to_lowercase();
    for (row, mut node) in &mut q {
        let d = if want.is_empty() || row.0.contains(want.trim()) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != d {
            node.display = d;
        }
    }
}

fn toast(world: &mut World, note: Notification) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    show_notification(&mut commands, &theme, note.name("sketch-dxf-toast"));
    world.flush();
}

/// Inserts a file into the sketch being edited with the dialog's units and origin, and closes
/// the dialog.
fn insert(world: &mut World, path: &Path) {
    let unit = {
        let mut q = world.query::<(&Name, &SelectState)>();
        q.iter(world)
            .find(|(n, _)| n.as_str() == "sketch-dxf-units")
            .map(|(_, s)| s.selected)
            .unwrap_or(2)
    };
    let origin = {
        let mut q = world.query::<(&Name, &CheckboxState)>();
        q.iter(world)
            .find(|(n, _)| n.as_str() == "sketch-dxf-origin")
            .is_none_or(|(_, s)| s.checked)
    };
    close_panels(world);
    let Some((element, feature)) = world
        .get_resource::<SketchSession>()
        .map(|s| (s.element, s.feature))
    else {
        return;
    };
    if !matches!(
        VectorFormat::of_path(path),
        Some(VectorFormat::Dxf | VectorFormat::Dwg)
    ) {
        return insert_vector(world, path, element, feature, origin);
    }
    let d = match read_file(path) {
        Ok(d) => d,
        Err(e) => {
            return toast(
                world,
                Notification::warning(tr_with(
                    "sketch-dxf-cannot-insert-named",
                    &[
                        ("file", &path.display().to_string()),
                        ("error", &e.to_string()),
                    ],
                )),
            );
        }
    };
    let (geometry, report) =
        sketch_of(&d, DxfUnits::ALL[unit.min(DxfUnits::ALL.len() - 1)], origin);
    if report.curves() == 0 {
        return toast(
            world,
            Notification::warning(tr_with(
                "sketch-dxf-no-curves",
                &[("file", &path.display().to_string())],
            )),
        );
    }
    let r = world.resource_mut::<ActiveDocument>().execute(&InsertDxf {
        element,
        feature,
        geometry,
    });
    match r {
        Ok(()) => {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .map_or_else(|| tr("sketch-dxf-the-file"), str::to_string);
            info!("inserted {name}: {report:?}");
            // Constraint glyphs only on hover and selection, as Onshape's sketch has them (Show
            // constraints off, `step-03.png`): an imported drawing's every shared corner had a
            // glyph.
            world
                .resource_mut::<crate::sketch::SketchViewSettings>()
                .show_constraints = false;
            toast(
                world,
                Notification::info(tr_with(
                    "sketch-dxf-inserted",
                    &[("file", &name), ("count", &report.curves().to_string())],
                ))
                .seconds(5.0),
            );
        }
        Err(e) => toast(
            world,
            Notification::warning(tr_with(
                "sketch-dxf-cannot-insert",
                &[("error", &e.to_string())],
            )),
        ),
    }
}

/// A vector drawing other than DXF or DWG into the sketch being edited, at its own size.
fn insert_vector(
    world: &mut World,
    path: &Path,
    element: cadrs_core::ElementId,
    feature: cadrs_core::FeatureId,
    origin: bool,
) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        // (Saved with the sketch as the file's name: not translated.)
        .unwrap_or("the file")
        .to_string();
    let (geometry, report) = match cadrs_core::vector_import::read_sketch(path, None, origin) {
        Ok(g) => g,
        Err(e) => {
            return toast(
                world,
                Notification::warning(tr_with(
                    "sketch-dxf-cannot-insert-named",
                    &[("file", &name), ("error", &e.to_string())],
                )),
            );
        }
    };
    if report.curves() == 0 {
        return toast(
            world,
            Notification::warning(tr_with("sketch-dxf-nothing-to-insert", &[("file", &name)])),
        );
    }
    let cmd = InsertVector {
        element,
        feature,
        file_name: name.clone(),
        geometry,
        offset: cadrs_sketch::Vec2::ZERO,
    };
    match world.resource_mut::<ActiveDocument>().execute(&cmd) {
        Ok(()) => {
            world
                .resource_mut::<crate::sketch::SketchViewSettings>()
                .show_constraints = false;
            toast(
                world,
                Notification::info(tr_with(
                    "sketch-dxf-inserted",
                    &[("file", &name), ("count", &report.curves().to_string())],
                ))
                .seconds(5.0),
            );
        }
        Err(e) => toast(
            world,
            Notification::warning(tr_with(
                "sketch-dxf-cannot-insert",
                &[("error", &e.to_string())],
            )),
        ),
    }
}
