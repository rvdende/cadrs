//! Files dropped on the window, one place for every kind (issue #9). Each file goes where its
//! kind and the open page say, the same as the menus would put it:
//!
//! | Dropped | Documents page | Part Studio | Sketch being edited | Other tabs |
//! |---|---|---|---|---|
//! | Native: cadrs | a new library copy | (note) | (note) | (note) |
//! | Drawing: SVG, EPS, PS, PDF, AI, DXF, DWG | (note) | a new sketch of it | its curves added | (note) |
//! | Picture: PNG, JPEG, GIF | (note) | an Image ([`crate::picture`]) | (note) | (note) |
//! | Model: STEP, IGES, BREP | a new document ([`crate::import_file`]) | an Import ([`crate::import_dialog`]) | (note) | new tabs ([`crate::import_file`]) |
//! | Model: STL | a new document | an Import | (note) | (note) |
//!
//! - **Drawings** ([`cadrs_core::vector_import`]) are centred where they are dropped: in a Part
//!   Studio, on the default plane facing the view most (as pictures are), as a new sketch
//!   ("Insert <file>"); while sketching, on the sketch's plane, added to the sketch (one step,
//!   kept with the sketch's others). Several dropped together lie side by side.
//! - Whatever can't go where it was dropped says so in a note (`drop-toast`).

use std::path::{Path, PathBuf};

use bevy::picking::pointer::{PointerId, PointerLocation};
use bevy::prelude::*;
use bevy::window::{FileDragAndDrop, PrimaryWindow};
use cadrs_core::FeatureId;
use cadrs_core::document::ElementKind;
use cadrs_core::import::ImportFormat;
use cadrs_core::picture::ImageFormat;
use cadrs_core::vector_import::{AddVectorSketch, InsertVector, VectorFormat, bounds, read_sketch};
use cadrs_sketch::Vec2 as SVec2;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::{Notification, Theme, show_notification};

use crate::import_file::ImportTarget;
use crate::sketch::{PartStudioMode, SketchSession};
use crate::{ActiveDocument, AppState};

/// The gap between drawings dropped together (mm).
const DROP_GAP: f64 = 10.0;

pub struct DropImportPlugin;

impl Plugin for DropImportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, route_drops);
    }
}

/// What a dropped file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    Native,
    Drawing,
    Picture,
    /// STEP, IGES or BREP.
    Model,
    Mesh,
}

impl DropKind {
    pub fn of_path(path: &Path) -> Option<Self> {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cadrs"))
        {
            return Some(Self::Native);
        }
        if VectorFormat::of_path(path).is_some() {
            return Some(Self::Drawing);
        }
        if ImageFormat::of_path(path).is_some() {
            return Some(Self::Picture);
        }
        match ImportFormat::of_path(path)? {
            ImportFormat::Stl => Some(Self::Mesh),
            f if f.kernel().is_some() => Some(Self::Model),
            _ => None,
        }
    }
}

/// Where the open page would put a drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Documents,
    PartStudio,
    Sketch,
    /// An Assembly, a drawing, a PCB Studio…
    OtherTab,
}

fn page(world: &World) -> Option<Page> {
    match world.get_resource::<State<AppState>>()?.get() {
        AppState::Document => {}
        _ => return Some(Page::Documents),
    }
    let studio = world
        .get_resource::<ActiveDocument>()?
        .active_element()
        .is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. }));
    let sketching = world
        .get_resource::<State<PartStudioMode>>()
        .is_some_and(|m| *m.get() == PartStudioMode::Sketching)
        && world.contains_resource::<SketchSession>();
    Some(match (studio, sketching) {
        (true, true) => Page::Sketch,
        (true, false) => Page::PartStudio,
        (false, _) => Page::OtherTab,
    })
}

fn route_drops(mut drops: MessageReader<FileDragAndDrop>, mut commands: Commands) {
    let paths: Vec<PathBuf> = drops
        .read()
        .filter_map(|d| match d {
            FileDragAndDrop::DroppedFile { path_buf, .. } => Some(path_buf.clone()),
            _ => None,
        })
        .collect();
    if !paths.is_empty() {
        commands.queue(move |world: &mut World| dropped(world, paths));
    }
}

/// Puts each dropped file where it goes (see the module docs).
pub fn dropped(world: &mut World, paths: Vec<PathBuf>) {
    let Some(page) = page(world) else { return };
    // Where the drag last was (Wayland says; the window's cursor doesn't follow a drag there),
    // else the cursor, else the pointer that moved last (a scenario's).
    let at = world
        .resource_mut::<crate::file_drop::DropPosition>()
        .0
        .take()
        .or_else(|| {
            let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
            q.iter(world).next().and_then(|w| w.cursor_position())
        });
    let at = at.or_else(|| {
        let id = world.resource::<crate::tool_input::ActivePointer>().0?;
        let mut q = world.query::<(&PointerId, &PointerLocation)>();
        q.iter(world)
            .find(|(p, _)| **p == id)
            .and_then(|(_, l)| l.location.as_ref().map(|l| l.position))
    });
    let mut drawings = Vec::new();
    let mut pictures = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("the file")
            .to_string();
        let Some(kind) = DropKind::of_path(&path) else {
            notes.push(Notification::warning(tr_with(
                "io-drop-unknown-kind",
                &[("name", &name)],
            )));
            continue;
        };
        match (kind, page) {
            (DropKind::Native, Page::Documents) => crate::native_document::import(world, &path),
            (DropKind::Native, _) => {
                notes.push(Notification::warning(tr("io-drop-native-on-documents")))
            }
            (DropKind::Drawing, Page::PartStudio | Page::Sketch) => drawings.push(path),
            (DropKind::Picture, Page::PartStudio) => pictures.push(path),
            (DropKind::Model | DropKind::Mesh, Page::PartStudio) => {
                crate::import_dialog::import_dropped(world, &path)
            }
            (DropKind::Model, Page::Documents) => {
                let theme = world.resource::<Theme>().clone();
                let mut commands = world.commands();
                crate::import_file::read(&mut commands, &theme, ImportTarget::NewDocument, &path);
                world.flush();
            }
            (DropKind::Mesh, Page::Documents) => crate::landing::import_mesh_document(world, &path),
            (DropKind::Model, Page::OtherTab) => {
                let theme = world.resource::<Theme>().clone();
                let mut commands = world.commands();
                crate::import_file::read(
                    &mut commands,
                    &theme,
                    ImportTarget::ActiveDocument,
                    &path,
                );
                world.flush();
            }
            (_, Page::Sketch) => notes.push(Notification::warning(tr_with(
                "io-drop-close-sketch",
                &[("name", &name)],
            ))),
            (DropKind::Drawing | DropKind::Picture, Page::Documents) => notes.push(
                Notification::warning(tr_with("io-drop-open-part-studio", &[("name", &name)])),
            ),
            (_, Page::OtherTab) => notes.push(Notification::warning(tr_with(
                "io-drop-into-part-studio",
                &[("name", &name)],
            ))),
        }
    }
    if !pictures.is_empty() {
        crate::picture::insert_pictures(world, pictures, at);
    }
    if !drawings.is_empty() {
        notes.extend(insert_drawings(world, page, drawings, at));
    }
    toast(world, notes);
}

/// Drawings dropped on a Part Studio (each a new sketch) or into the sketch being edited, the
/// first centred at `at`, the next ones to its right. Returns the notes to show.
fn insert_drawings(
    world: &mut World,
    page: Page,
    paths: Vec<PathBuf>,
    at: Option<Vec2>,
) -> Vec<Notification> {
    let Some(element) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element())
        .map(|e| e.id)
    else {
        return vec![];
    };
    // The sketch's plane, or the plane facing the view; the point under the pointer on it.
    let (plane, mut at) = match page {
        Page::Sketch => {
            let map = world.resource::<crate::sketch_tools::SketchScreen>().active;
            (
                None,
                at.and_then(|c| map?.to_sketch(c)).unwrap_or(SVec2::ZERO),
            )
        }
        _ => {
            let (plane, [x, y]) = crate::picture::drop_spot(world, at);
            (Some(plane), SVec2::new(x, y))
        }
    };
    let mut notes = Vec::new();
    let mut first = true;
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("the drawing")
            .to_string();
        let (geometry, report) = match read_sketch(&path, None, false) {
            Ok(g) => g,
            Err(e) => {
                notes.push(Notification::warning(tr_with(
                    "io-cannot-insert",
                    &[("name", &name), ("error", &e.to_string())],
                )));
                continue;
            }
        };
        let Some((lo, hi)) = bounds(&geometry).filter(|_| report.curves() > 0) else {
            notes.push(Notification::warning(tr_with(
                "io-drop-no-curves",
                &[("name", &name)],
            )));
            continue;
        };
        // Side by side: this one's left edge a gap after the last one's right edge.
        if !first {
            at.x += (hi.x - lo.x) / 2.0;
        }
        let result = match (page, plane) {
            (Page::Sketch, _) => {
                let Some(feature) = world.get_resource::<SketchSession>().map(|s| s.feature) else {
                    break;
                };
                world
                    .resource_mut::<ActiveDocument>()
                    .execute(&InsertVector {
                        element,
                        feature,
                        file_name: name.clone(),
                        geometry,
                        offset: at,
                    })
            }
            (_, Some(plane)) => {
                let cmd = AddVectorSketch {
                    element,
                    feature: FeatureId::new(),
                    plane,
                    file_name: name.clone(),
                    geometry,
                    offset: at,
                };
                world.resource_mut::<ActiveDocument>().execute(&cmd)
            }
            _ => break,
        };
        match result {
            Ok(()) => {
                info!("inserted {name}: {report:?}");
                notes.push(
                    Notification::info(tr_with(
                        "io-drop-inserted-curves",
                        &[("name", &name), ("count", &report.curves().to_string())],
                    ))
                    .seconds(5.0),
                );
                at.x += (hi.x - lo.x) / 2.0 + DROP_GAP;
                first = false;
            }
            Err(e) => notes.push(Notification::warning(tr_with(
                "io-cannot-insert",
                &[("name", &name), ("error", &e.to_string())],
            ))),
        }
    }
    if page == Page::Sketch && !first {
        // As the Insert panel: constraint glyphs only on hover and selection (a drawing's every
        // shared corner had one).
        world
            .resource_mut::<crate::sketch::SketchViewSettings>()
            .show_constraints = false;
    }
    notes
}

fn toast(world: &mut World, notes: Vec<Notification>) {
    if notes.is_empty() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    for n in notes {
        show_notification(&mut commands, &theme, n.name("drop-toast"));
    }
    world.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        let k = |p: &str| DropKind::of_path(Path::new(p));
        assert_eq!(k("logo.SVG"), Some(DropKind::Drawing));
        assert_eq!(k("a.eps"), Some(DropKind::Drawing));
        assert_eq!(k("a.ai"), Some(DropKind::Drawing));
        assert_eq!(k("plate.dxf"), Some(DropKind::Drawing));
        assert_eq!(k("photo.jpeg"), Some(DropKind::Picture));
        assert_eq!(k("bracket.step"), Some(DropKind::Model));
        assert_eq!(k("bracket.igs"), Some(DropKind::Model));
        assert_eq!(k("bracket.stl"), Some(DropKind::Mesh));
        assert_eq!(k("project.CADRS"), Some(DropKind::Native));
        assert_eq!(k("notes.txt"), None);
    }
}
