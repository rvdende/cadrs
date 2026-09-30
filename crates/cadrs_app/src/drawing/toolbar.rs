//! The drawing toolbar (D2.4, D5.1), left to right like Onshape's: Undo/Redo, Update, view
//! creation, dimensions, manufacturing annotations, note/callout/table/BOM, centerline /
//! centermark / virtual sharp, sketch line and spline, Insert DXF/DWG and image.
//!
//! Every group is present now; tools arrive with their milestones (P3C.2 views, P3C.3
//! dimensions and geometric annotations, P3C.4 notes and tables, P3C.5 BOM and callouts, P3C.6
//! Update, P3C.7 sheet sketch and import, P3C.8 the untaught annotations). Until then each is
//! drawn disabled and its tooltip says so. Update from this workspace works (P3C.6: gold while
//! the drawing is out of date, see [`super::update`]). Insert view, Projected view and Auxiliary view work
//! (P3C.2), and Dimension ▾ (the last dimension tool chosen; the ▾ lists them all), Hole
//! callout, Centerline ▾ (its modes), Centermark and Virtual sharp (P3C.3); the active one is
//! shown selected.

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, observe};
use cadrs_ui::prelude::*;
use cadrs_ui::{ToolButton, toolbar_separator};

use super::DrawingUi;
use super::annotations::{AnnTool, AnnotationUi, CenterlineMode};
use super::view_tools::ViewTool;
use cadrs_drawing::annotation::DimTool;

/// The tools that work now.
const ENABLED: [&str; 22] = [
    "drawing-update",
    "drawing-insert-view",
    "drawing-projected-view",
    "drawing-auxiliary-view",
    "drawing-section-view",
    "drawing-detail-view",
    "drawing-dimension",
    "drawing-hole-callout",
    "drawing-surface-finish",
    "drawing-weld",
    "drawing-gdt",
    "drawing-centerline",
    "drawing-centermark",
    "drawing-virtual-sharp",
    "drawing-note",
    "drawing-callout",
    "drawing-table",
    "drawing-bom",
    "drawing-line",
    "drawing-spline",
    "drawing-insert-dxf",
    "drawing-insert-image",
];

/// The toolbar buttons of the view tools, highlighted while their tool is active.
pub struct DrawingToolbarPlugin;

impl Plugin for DrawingToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ToolChoices>()
            .add_systems(Update, sync_tool_buttons.run_if(in_state(crate::AppState::Document)));
    }
}

/// The dimension tool and centerline mode the buttons start (the last chosen from their ▾).
#[derive(Resource, Debug, Clone, Copy)]
pub struct ToolChoices {
    pub dimension: DimTool,
    pub centerline: CenterlineMode,
    /// The view kind the Detail view button starts (the last chosen from its ▾, P3C.8).
    pub view_kind: &'static str,
    /// What the Geometric tolerance button starts: a frame or a datum (P3C.8).
    pub gdt: AnnTool,
}

impl Default for ToolChoices {
    fn default() -> Self {
        Self {
            dimension: DimTool::Smart,
            centerline: CenterlineMode::PointToPoint,
            view_kind: "drawing-view-detail",
            gdt: AnnTool::Gdt,
        }
    }
}

impl ToolChoices {
    pub fn remember(&mut self, t: AnnTool) {
        match t {
            AnnTool::Dimension(d) => self.dimension = d,
            AnnTool::Centerline(c) => self.centerline = c,
            AnnTool::Gdt | AnnTool::Datum => self.gdt = t,
            _ => {}
        }
    }
}

fn sync_tool_buttons(
    ui: Res<DrawingUi>,
    ann: Res<AnnotationUi>,
    q: Query<(Entity, &Name, Has<cadrs_ui::Selected>)>,
    mut commands: Commands,
) {
    for (e, name, selected) in &q {
        let on = match name.as_str() {
            "drawing-insert-view" => ui.tool == ViewTool::Insert,
            "drawing-projected-view" => matches!(ui.tool, ViewTool::Projected { .. }),
            "drawing-auxiliary-view" => matches!(ui.tool, ViewTool::Auxiliary { .. }),
            "drawing-section-view" => matches!(ui.tool, ViewTool::Section { .. }),
            "drawing-detail-view" => ui.tool.is_view_kind() && !matches!(ui.tool, ViewTool::Section { .. }),
            "drawing-dimension" => matches!(ann.tool, AnnTool::Dimension(_)),
            "drawing-hole-callout" => ann.tool == AnnTool::HoleCallout,
            "drawing-gdt" => matches!(ann.tool, AnnTool::Gdt | AnnTool::Datum),
            "drawing-surface-finish" => ann.tool == AnnTool::SurfaceFinish,
            "drawing-weld" => ann.tool == AnnTool::Weld,
            "drawing-centerline" => matches!(ann.tool, AnnTool::Centerline(_)),
            "drawing-centermark" => ann.tool == AnnTool::Centermark,
            "drawing-virtual-sharp" => ann.tool == AnnTool::VirtualSharp,
            "drawing-note" => ann.tool == AnnTool::Note,
            "drawing-table" => ann.tool == AnnTool::Table,
            "drawing-callout" => ann.tool == AnnTool::Callout,
            "drawing-bom" => ann.tool == AnnTool::PlaceBom,
            "drawing-line" => ann.tool == AnnTool::SheetLine,
            "drawing-spline" => ann.tool == AnnTool::SheetSpline,
            _ => continue,
        };
        if on && !selected {
            commands.entity(e).try_insert(cadrs_ui::Selected);
        } else if !on && selected {
            commands.entity(e).try_remove::<cadrs_ui::Selected>();
        }
    }
}

/// Width (px) of the ▾ part at the right of a dropdown tool button.
const CARET_WIDTH: f32 = 16.0;

/// Whether the click on `button` was on its ▾.
fn on_caret(world: &mut World, button: Entity) -> bool {
    // The mouse pointer's own location (the viewport's copy isn't kept on Drawing tabs).
    use bevy::picking::pointer::{PointerId, PointerLocation};
    let mut q = world.query::<(&PointerId, &PointerLocation)>();
    let x = q
        .iter(world)
        .find(|(id, _)| **id == PointerId::Mouse)
        .and_then(|(_, l)| l.location.as_ref().map(|l| l.position.x))
        .unwrap_or_else(|| world.resource::<crate::viewport::ViewportDrag>().pointer().x);
    world
        .get::<ComputedNode>(button)
        .zip(world.get::<bevy::ui::UiGlobalTransform>(button))
        .is_some_and(|(n, t)| {
            let s = n.inverse_scale_factor();
            x >= (t.translation.x + n.size().x / 2.0) * s - CARET_WIDTH
        })
}

/// What a toolbar click does.
fn start_tool(world: &mut World, name: &str, button: Entity) {
    let current = world.resource::<DrawingUi>().tool;
    let choices = *world.resource::<ToolChoices>();
    super::notes::commit_edit(world);
    match name {
        "drawing-update" => {
            super::update::start_update(world);
            return;
        }
        "drawing-note" => {
            super::notes::start_note_tool(world, AnnTool::Note);
            return;
        }
        "drawing-line" => {
            super::sheet_items::start_tool(world, AnnTool::SheetLine);
            return;
        }
        "drawing-spline" => {
            super::sheet_items::start_tool(world, AnnTool::SheetSpline);
            return;
        }
        "drawing-insert-dxf" => {
            super::sheet_items::open_dxf_picker(world);
            return;
        }
        "drawing-insert-image" => {
            super::sheet_items::open_image_picker(world);
            return;
        }
        "drawing-bom" => {
            if world.resource::<AnnotationUi>().tool == AnnTool::PlaceBom {
                world.resource_mut::<AnnotationUi>().toggle(AnnTool::None);
            } else {
                super::bom_tools::open_bom_tool(world);
            }
            return;
        }
        "drawing-callout" => {
            if world.resource::<AnnotationUi>().tool == AnnTool::Callout {
                world.resource_mut::<AnnotationUi>().toggle(AnnTool::None);
            } else {
                super::bom_tools::start_callout_tool(world);
            }
            return;
        }
        "drawing-table" => {
            if world.resource::<AnnotationUi>().tool == AnnTool::Table {
                world.resource_mut::<AnnotationUi>().toggle(AnnTool::None);
            } else {
                super::note_bar::open_table_dialog(world);
            }
            return;
        }
        _ => {}
    }
    match name {
        "drawing-detail-view" if on_caret(world, button) => {
            super::annotations::open_tool_menu(world, button, name);
            return;
        }
        "drawing-detail-view" => {
            super::view_kind_tools::start(world, choices.view_kind);
            return;
        }
        "drawing-section-view" => {
            super::view_kind_tools::start(world, name);
            return;
        }
        _ => {}
    }
    let annotation = match name {
        "drawing-dimension" | "drawing-centerline" | "drawing-gdt" if on_caret(world, button) => {
            super::annotations::open_tool_menu(world, button, name);
            return;
        }
        "drawing-dimension" => Some(AnnTool::Dimension(choices.dimension)),
        "drawing-centerline" => Some(AnnTool::Centerline(choices.centerline)),
        "drawing-hole-callout" => Some(AnnTool::HoleCallout),
        "drawing-gdt" => Some(choices.gdt),
        "drawing-surface-finish" => Some(AnnTool::SurfaceFinish),
        "drawing-weld" => Some(AnnTool::Weld),
        "drawing-centermark" => Some(AnnTool::Centermark),
        "drawing-virtual-sharp" => Some(AnnTool::VirtualSharp),
        _ => None,
    };
    if let Some(t) = annotation {
        super::annotations::start_tool(world, t);
        return;
    }
    // A view tool ends the annotation tool.
    world.resource_mut::<AnnotationUi>().toggle(AnnTool::None);
    match name {
        "drawing-insert-view" => {
            if current == ViewTool::Insert {
                super::view_tools::end_tool(world);
            } else {
                super::view_tools::open_insert_view(world);
            }
        }
        "drawing-projected-view" => {
            let mut ui = world.resource_mut::<DrawingUi>();
            if matches!(current, ViewTool::Projected { .. }) {
                ui.tool = ViewTool::None;
                ui.ghost = None;
            } else {
                // From the selected view, if one is selected.
                let parent = ui.selected.first().copied();
                ui.tool = ViewTool::Projected { parent };
                ui.ghost = None;
            }
        }
        "drawing-auxiliary-view" => {
            let mut ui = world.resource_mut::<DrawingUi>();
            ui.tool = if matches!(current, ViewTool::Auxiliary { .. }) {
                ViewTool::None
            } else {
                ViewTool::Auxiliary { from: None }
            };
            ui.ghost = None;
            // The pick is an edge: no view stays selected (orange) while the tool runs.
            ui.selected.clear();
        }
        _ => {}
    }
}

/// (name, icon, dropdown, tooltip).
type Tool = (&'static str, &'static str, bool, &'static str);

/// The toolbar's groups.
pub const GROUPS: [&[Tool]; 7] = [
    &[("drawing-update", "history", false, "Update from this workspace (Ctrl+Q)")],
    &[
        ("drawing-insert-view", "part", false, "Insert view"),
        ("drawing-projected-view", "replicate", false, "Projected view"),
        ("drawing-auxiliary-view", "arrow-up-right", false, "Auxiliary view"),
        ("drawing-section-view", "section-view", false, "Section view"),
        ("drawing-detail-view", "find", true, "Detail view"),
    ],
    &[
        ("drawing-dimension", "dimension", true, "Dimension (D)"),
        ("drawing-hole-callout", "hole", false, "Hole callout"),
    ],
    &[
        ("drawing-surface-finish", "check", false, "Surface finish symbol"),
        ("drawing-weld", "fillet", false, "Weld symbol"),
        ("drawing-gdt", "constraint-parallel", true, "Geometric tolerance"),
    ],
    &[
        ("drawing-note", "note", false, "Note (N)"),
        ("drawing-callout", "comments", false, "Callout"),
        ("drawing-table", "custom-table", false, "Table"),
        ("drawing-bom", "bill-of-materials", false, "Insert BOM"),
    ],
    &[
        ("drawing-centerline", "construction", true, "Centerline"),
        ("drawing-centermark", "origin", false, "Centermark"),
        ("drawing-virtual-sharp", "extend", false, "Virtual sharp"),
    ],
    &[
        ("drawing-line", "line", false, "Line"),
        ("drawing-spline", "spline", false, "Spline"),
        ("drawing-insert-dxf", "file-import", false, "Insert DXF or DWG"),
        ("drawing-insert-image", "image", false, "Insert image"),
    ],
];

/// The milestone a disabled tool waits for, for its tooltip.
fn not_yet(name: &str) -> &'static str {
    match name {
        "drawing-update" => "updating views from the workspace arrives in a later version",
        "drawing-line" | "drawing-spline" | "drawing-insert-dxf" | "drawing-insert-image" => {
            "sheet sketching and import arrive in a later version"
        }
        _ => "not available yet",
    }
}

pub fn drawing_toolbar(tb: &mut ChildSpawnerCommands, t: &Theme) {
    crate::document::undo_redo(tb, t);
    // P3G.2 (ER4.3): shown when the document references versions.
    crate::reference_manager::update_all_button(tb, t);
    for group in GROUPS.iter() {
        tb.spawn(toolbar_separator(t));
        for (name, icon, dropdown, tip) in group.iter() {
            if ENABLED.contains(name) {
                let n: &'static str = name;
                tb.spawn((
                    ToolButton::new(*name, *icon).dropdown(*dropdown).tooltip(*tip).build(t),
                    observe(move |a: On<Activate>, mut commands: Commands| {
                        let button = a.entity;
                        commands.queue(move |w: &mut World| start_tool(w, n, button));
                    }),
                ));
                continue;
            }
            tb.spawn(
                ToolButton::new(*name, *icon)
                    .dropdown(*dropdown)
                    .disabled(true)
                    .tooltip(format!("{tip}: {}", not_yet(name)))
                    .build(t),
            );
        }
    }
}
