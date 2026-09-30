//! The section, detail, crop, broken-out and break view tools (P3C.8, D4.12, X14); the views
//! themselves are `cadrs_drawing::view_kinds`.
//!
//! - **Section view** (toolbar): click a point on a view, then the cutting line's end (snapped
//!   horizontal or vertical within 4°); the section follows the cursor on either side of the
//!   line, aligned with its parent, and a click places it.
//! - **Detail view** (the view group's ▾, or its button): click the circle's centre on a view,
//!   then a point on the circle; the detail (twice the parent's scale) follows the cursor and a
//!   click places it.
//! - **Crop view**: click two corners of a rectangle on a view. **Crop view (spline)** and
//!   **Broken-out section**: click the boundary's points on a view; Enter, a double click or a
//!   click on the first point closes it. A broken-out section then asks for its depth (mm from
//!   the model origin along the direction of sight; the default 0 cuts through the origin).
//! - **Break view**: click the two break lines on a view (vertical when the clicks are further
//!   apart across than up).
//!
//! Every view made or changed is one undoable drawing edit.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_drawing::view::rotate;
use cadrs_drawing::view_kinds::{Boundary, Break, BrokenOut, detail_view, double, next_letter, section_view};
use cadrs_drawing::{DrawingOp, View, ViewId};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, form_row};

use super::view_tools::{ViewTool, edit_drawing, insert_view_op, sheet_center};
use super::views::{ViewCache, view_at};
use super::{DrawingUi, active_drawing};
use crate::{ActiveDocument, AppState};

/// What a boundary tool makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryUse {
    CropSpline,
    BrokenOut,
}

/// The view kinds of the Detail view ▾ (name, label, icon).
pub const VIEW_KINDS: [(&str, &str, &str); 5] = [
    ("drawing-view-detail", "Detail view", "find"),
    ("drawing-view-crop", "Crop view", "corner-rectangle"),
    ("drawing-view-crop-spline", "Crop view (spline)", "spline"),
    ("drawing-view-broken-out", "Broken-out section", "trim"),
    ("drawing-view-break", "Break view", "split"),
];

/// Starts a view kind's tool by its menu name.
pub fn start(world: &mut World, name: &str) {
    {
        let mut ann = world.resource_mut::<super::annotations::AnnotationUi>();
        ann.tool = super::annotations::AnnTool::None;
        ann.reset_picks();
        ann.selected.clear();
    }
    let tool = match name {
        "drawing-section-view" => ViewTool::Section { parent: None, a: None, b: None },
        "drawing-view-detail" => ViewTool::Detail { parent: None, center: None, radius: None },
        "drawing-view-crop" => ViewTool::Crop { view: None, a: None },
        "drawing-view-crop-spline" => ViewTool::Boundary { view: None, use_: BoundaryUse::CropSpline },
        "drawing-view-broken-out" => ViewTool::Boundary { view: None, use_: BoundaryUse::BrokenOut },
        "drawing-view-break" => ViewTool::Break { view: None, a: None },
        _ => return,
    };
    let mut ui = world.resource_mut::<DrawingUi>();
    let same = std::mem::discriminant(&ui.tool) == std::mem::discriminant(&tool);
    ui.tool = if same { ViewTool::None } else { tool };
    ui.ghost = None;
    ui.selected.clear();
    ui.tool_points.clear();
    ui.tool_strokes.clear();
}

/// The view the tool works in and the pointer in its 2D frame.
fn local(d: &cadrs_drawing::Drawing, index: usize, id: ViewId, p: Vec2) -> Option<(View, [f64; 2])> {
    let v = d.sheets.get(index)?.view(id)?.clone();
    let q = v.from_sheet([p.x as f64, p.y as f64]);
    Some((v, q))
}

/// Snaps the second point of a line horizontal or vertical within 4°.
fn snap_hv(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let ang = dy.atan2(dx).to_degrees().rem_euclid(90.0);
    if !(4.0..=86.0).contains(&ang) {
        if dx.abs() >= dy.abs() { [b[0], a[1]] } else { [a[0], b[1]] }
    } else {
        b
    }
}

/// The ghost view and the preview strokes (sheet mm) of the active view-kind tool.
pub fn preview(world: &mut World) {
    let tool = world.resource::<DrawingUi>().tool;
    if !matches!(
        tool,
        ViewTool::Section { .. } | ViewTool::Detail { .. } | ViewTool::Crop { .. } | ViewTool::Break { .. } | ViewTool::Boundary { .. }
    ) {
        return;
    }
    let Some((d, index)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        Some((d.clone(), world.resource::<DrawingUi>().sheet_index(id, d)))
    }) else {
        return;
    };
    let Some(p) = world.resource::<DrawingUi>().pointer else {
        return;
    };
    let cursor = [p.x as f64, p.y as f64];
    let points = world.resource::<DrawingUi>().tool_points.clone();
    let ghost_id = world.resource::<DrawingUi>().ghost_id;
    let mut strokes: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut ghost: Option<View> = None;
    match tool {
        ViewTool::Section { parent: Some(pid), a: Some(a), b } => {
            if let Some((v, q)) = local(&d, index, pid, p) {
                match b {
                    None => {
                        let b = snap_hv(a, q);
                        strokes.push(vec![v.to_sheet(a), v.to_sheet(b)]);
                    }
                    Some(b) => {
                        let letter = next_letter(d.sheets.iter().flat_map(|s| s.views.iter()));
                        if let Some(mut s) = section_view(&v, a, b, cursor, d.projection, &letter) {
                            s.id = ghost_id;
                            s.scale = d.effective_scale(pid).unwrap_or(s.scale);
                            ghost = Some(s);
                        }
                    }
                }
            }
        }
        ViewTool::Detail { parent: Some(pid), center: Some(c), radius } => {
            if let Some((v, q)) = local(&d, index, pid, p) {
                match radius {
                    None => {
                        let r = (q[0] - c[0]).hypot(q[1] - c[1]);
                        let pts = cadrs_drawing::view_kinds::circle_polygon(c, r.max(1e-6), 96);
                        let mut s: Vec<[f64; 2]> = pts.iter().map(|x| v.to_sheet(*x)).collect();
                        s.push(s[0]);
                        strokes.push(s);
                    }
                    Some(r) => {
                        let letter = next_letter(d.sheets.iter().flat_map(|s| s.views.iter()));
                        let scale = double(d.effective_scale(pid).unwrap_or(v.scale));
                        let mut dv = detail_view(&v, c, r, scale, cursor, &letter);
                        dv.id = ghost_id;
                        ghost = Some(dv);
                    }
                }
            }
        }
        ViewTool::Crop { view: Some(vid), a: Some(a) } | ViewTool::Break { view: Some(vid), a: Some(a) } => {
            if let Some((v, q)) = local(&d, index, vid, p) {
                if matches!(tool, ViewTool::Crop { .. }) {
                    let r = Boundary::rectangle(a, q).polygon();
                    let mut s: Vec<[f64; 2]> = r.iter().map(|x| v.to_sheet(*x)).collect();
                    s.push(s[0]);
                    strokes.push(s);
                } else {
                    // The two break lines across the view.
                    let vertical = (q[0] - a[0]).abs() >= (q[1] - a[1]).abs();
                    let g = world.resource::<ViewCache>().geometry(&v);
                    let (lo, hi) = super::views::sheet_bounds(&v, g.as_deref());
                    for c in [a, q] {
                        let s = v.to_sheet(c);
                        strokes.push(if vertical {
                            vec![[s[0], lo[1] - 3.0], [s[0], hi[1] + 3.0]]
                        } else {
                            vec![[lo[0] - 3.0, s[1]], [hi[0] + 3.0, s[1]]]
                        });
                    }
                }
            }
        }
        ViewTool::Boundary { view: Some(vid), use_ } => {
            if let Some((v, q)) = local(&d, index, vid, p) {
                let mut pts = points.clone();
                pts.push(q);
                let shown: Vec<[f64; 2]> = if pts.len() >= 3 {
                    let mut c = cadrs_drawing::view_kinds::closed_spline(&pts, 12);
                    c.push(c[0]);
                    c
                } else {
                    pts.clone()
                };
                strokes.push(shown.iter().map(|x| v.to_sheet(*x)).collect());
                let _ = use_;
            }
        }
        _ => {}
    }
    // Keep the ghost centred on its fold line (sections) once its geometry is known.
    if let Some(g) = &mut ghost {
        world.resource_scope(|w, mut cache: Mut<ViewCache>| {
            if let Some(doc) = w.get_resource::<ActiveDocument>() {
                cache.ensure(&doc.doc, active_drawing(doc).map(|(_, d)| d), g);
            }
        });
        if let (Some(n), Some(parent)) = (g.fold, g.parent.and_then(|pid| d.sheets.get(index).and_then(|s| s.view(pid)))) {
            let cache = world.resource::<ViewCache>();
            if let Some(geo) = cache.geometry(g)
                && let Some((lo, hi)) = geo.projection.bounds().map(|(a, b)| ([a.x, a.y], [b.x, b.y]))
            {
                let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                let n = rotate(n, parent.rotation);
                let off = rotate([c[0] * g.scale.factor(), c[1] * g.scale.factor()], g.rotation);
                let t = off[0] * n[0] + off[1] * n[1];
                g.anchor = [g.anchor[0] - n[0] * t, g.anchor[1] - n[1] * t];
            }
        }
    }
    let mut ui = world.resource_mut::<DrawingUi>();
    if ui.tool_strokes != strokes {
        ui.tool_strokes = strokes;
    }
    if ui.ghost != ghost {
        ui.ghost = ghost;
    }
}

/// A click with a view-kind tool; `true` when the tool took it.
pub fn click(world: &mut World, p: Vec2, double: bool) -> bool {
    let tool = world.resource::<DrawingUi>().tool;
    let Some((d, index, sheet_id, pad)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        let ui = world.resource::<DrawingUi>();
        let index = ui.sheet_index(id, d);
        let pad = super::current_view(doc, ui).map(|(_, v)| 6.0 / v.ppm as f64).unwrap_or(1.0);
        Some((d.clone(), index, d.sheets[index].id, pad))
    }) else {
        return false;
    };
    // The first click may land a little outside the view (a crop or boundary around it).
    let pick_pad = if tool.picks_view() { pad.max(6.0) } else { pad };
    let hit = view_at(&d, index, world.resource::<ViewCache>(), p, pick_pad);
    let ghost = world.resource::<DrawingUi>().ghost.clone();
    let set_tool = |w: &mut World, t: ViewTool| {
        let mut ui = w.resource_mut::<DrawingUi>();
        ui.tool = t;
        ui.tool_strokes.clear();
    };
    match tool {
        ViewTool::Section { parent: None, .. } => {
            if let Some(h) = hit
                && let Some((_, q)) = local(&d, index, h, p)
            {
                set_tool(world, ViewTool::Section { parent: Some(h), a: Some(q), b: None });
            }
        }
        ViewTool::Section { parent: Some(pid), a: Some(a), b: None } => {
            if let Some((_, q)) = local(&d, index, pid, p) {
                let b = snap_hv(a, q);
                if (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-3 {
                    set_tool(world, ViewTool::Section { parent: Some(pid), a: Some(a), b: Some(b) });
                }
            }
        }
        ViewTool::Section { parent: Some(_), b: Some(_), .. } | ViewTool::Detail { radius: Some(_), .. } => {
            if let Some(mut v) = ghost {
                v.id = ViewId::new();
                let op = insert_view_op(world, sheet_id, v);
                if edit_drawing(world, op) {
                    let mut ui = world.resource_mut::<DrawingUi>();
                    ui.tool = ViewTool::None;
                    ui.ghost = None;
                    ui.ghost_id = ViewId::new();
                    ui.tool_strokes.clear();
                    ui.selected.clear();
                }
            }
        }
        ViewTool::Detail { parent: None, .. } => {
            if let Some(h) = hit
                && let Some((_, q)) = local(&d, index, h, p)
            {
                set_tool(world, ViewTool::Detail { parent: Some(h), center: Some(q), radius: None });
            }
        }
        ViewTool::Detail { parent: Some(pid), center: Some(c), radius: None } => {
            if let Some((_, q)) = local(&d, index, pid, p) {
                let r = (q[0] - c[0]).hypot(q[1] - c[1]);
                if r > 1e-3 {
                    set_tool(world, ViewTool::Detail { parent: Some(pid), center: Some(c), radius: Some(r) });
                }
            }
        }
        ViewTool::Crop { view: None, .. } | ViewTool::Break { view: None, .. } => {
            if let Some(h) = hit
                && let Some((_, q)) = local(&d, index, h, p)
            {
                let t = if matches!(tool, ViewTool::Crop { .. }) {
                    ViewTool::Crop { view: Some(h), a: Some(q) }
                } else {
                    ViewTool::Break { view: Some(h), a: Some(q) }
                };
                set_tool(world, t);
            }
        }
        ViewTool::Crop { view: Some(vid), a: Some(a) } => {
            if let Some((mut v, q)) = local(&d, index, vid, p) {
                if (q[0] - a[0]).abs() < 1e-3 || (q[1] - a[1]).abs() < 1e-3 {
                    return true;
                }
                v.crop = Some(Boundary::rectangle(a, q));
                edit_drawing(world, DrawingOp::SetView { view: v, label: "Crop view".into() });
                super::view_tools::end_tool(world);
            }
        }
        ViewTool::Break { view: Some(vid), a: Some(a) } => {
            if let Some((mut v, q)) = local(&d, index, vid, p) {
                let vertical = (q[0] - a[0]).abs() >= (q[1] - a[1]).abs();
                let i = if vertical { 0 } else { 1 };
                let (lo, hi) = (a[i].min(q[i]), a[i].max(q[i]));
                if hi - lo <= cadrs_drawing::view_kinds::BREAK_GAP / v.scale.factor() {
                    return true;
                }
                v.breaks.push(Break { vertical, lo, hi });
                edit_drawing(world, DrawingOp::SetView { view: v, label: "Break view".into() });
                super::view_tools::end_tool(world);
            }
        }
        ViewTool::Boundary { view, use_ } => {
            let vid = match view {
                Some(v) => v,
                None => match hit {
                    Some(h) => h,
                    None => return true,
                },
            };
            let Some((v, q)) = local(&d, index, vid, p) else { return true };
            let pts = world.resource::<DrawingUi>().tool_points.clone();
            // A click on the first point (or a double click) closes the boundary.
            let near_first = pts.first().is_some_and(|f| {
                let (a, b) = (v.to_sheet(*f), v.to_sheet(q));
                (a[0] - b[0]).hypot(a[1] - b[1]) < 2.0 * pad
            });
            if (near_first || double) && pts.len() >= 3 {
                finish_boundary(world, vid, use_);
                return true;
            }
            let mut ui = world.resource_mut::<DrawingUi>();
            ui.tool = ViewTool::Boundary { view: Some(vid), use_ };
            ui.tool_points.push(q);
        }
        _ => return false,
    }
    true
}

/// Closes the boundary being drawn: a spline crop at once, a broken-out section after its depth.
pub fn finish_boundary(world: &mut World, vid: ViewId, use_: BoundaryUse) {
    let pts = std::mem::take(&mut world.resource_mut::<DrawingUi>().tool_points);
    if pts.len() < 3 {
        return;
    }
    let Some((_, mut v)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| d.view(vid).map(|(i, v)| (i, v.clone())))
    else {
        return;
    };
    super::view_tools::end_tool(world);
    world.resource_mut::<DrawingUi>().tool_strokes.clear();
    match use_ {
        BoundaryUse::CropSpline => {
            v.crop = Some(Boundary::spline(pts));
            edit_drawing(world, DrawingOp::SetView { view: v, label: "Crop view".into() });
        }
        BoundaryUse::BrokenOut => open_depth_dialog(world, vid, pts),
    }
}

/// Enter closes a boundary being drawn.
pub fn enter(world: &mut World) -> bool {
    if let ViewTool::Boundary { view: Some(v), use_ } = world.resource::<DrawingUi>().tool {
        finish_boundary(world, v, use_);
        return true;
    }
    false
}

#[derive(Component, Clone)]
struct DepthDialog {
    view: ViewId,
    points: Vec<[f64; 2]>,
}

fn open_depth_dialog(world: &mut World, view: ViewId, points: Vec<[f64; 2]>) {
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("broken-out-dialog")
            .title("Broken-out section")
            .width(340.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(form_row(t, "broken-out-depth-row", "Depth", 90.0)).with_child(
                    TextInput::new("broken-out-depth").value("0 mm").width(Val::Px(140.0)).select_all_on_focus().build(t),
                );
                b.spawn((
                    t.text(
                        "From the model origin along the direction of sight: the material in front of this depth inside the boundary is removed.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node { max_width: Val::Px(300.0), ..default() },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("broken-out-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_depth_dialog);
                    }),
                ));
                f.spawn((
                    Button::new("broken-out-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<DepthDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        DepthDialog { view, points },
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// A length typed in a field: mm, or with a unit (mm, cm, m, in).
pub fn parse_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    let (num, k) = if let Some(n) = s.strip_suffix("mm") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix("cm") {
        (n, 10.0)
    } else if let Some(n) = s.strip_suffix("in") {
        (n, 25.4)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1000.0)
    } else {
        (s, 1.0)
    };
    num.trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| v * k)
}

fn apply_depth_dialog(world: &mut World) {
    let mut q = world.query::<(Entity, &DepthDialog)>();
    let Some((entity, dialog)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    let mut qf = world.query::<(&Name, &bevy::text::EditableText)>();
    let depth = qf
        .iter(world)
        .find(|(n, _)| n.as_str() == "broken-out-depth-field")
        .and_then(|(_, t)| parse_mm(&t.value().to_string()));
    let Some(depth) = depth else {
        return;
    };
    let view = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| d.view(dialog.view).map(|(_, v)| v.clone()));
    if let Some(mut v) = view {
        v.broken_out = Some(BrokenOut { boundary: Boundary::spline(dialog.points.clone()), depth });
        edit_drawing(world, DrawingOp::SetView { view: v, label: "Broken-out section".into() });
    }
    world.trigger(DialogClose { entity });
}

/// The tool's hint.
pub fn hint(tool: ViewTool) -> Option<&'static str> {
    Some(match tool {
        ViewTool::Section { parent: None, .. } => "Section view: click the cutting line's start on a view",
        ViewTool::Section { b: None, .. } => "Section view: click the cutting line's end",
        ViewTool::Section { .. } => "Section view: move to either side and click to place · Esc to cancel",
        ViewTool::Detail { parent: None, .. } => "Detail view: click the centre of the detail circle on a view",
        ViewTool::Detail { radius: None, .. } => "Detail view: click a point on the circle",
        ViewTool::Detail { .. } => "Detail view: click to place the detail · Esc to cancel",
        ViewTool::Crop { view: None, .. } => "Crop view: click a corner of the crop rectangle on a view",
        ViewTool::Crop { .. } => "Crop view: click the opposite corner",
        ViewTool::Break { view: None, .. } => "Break view: click the first break line on a view",
        ViewTool::Break { .. } => "Break view: click the second break line",
        ViewTool::Boundary { view: None, .. } => "Click the boundary's points on a view",
        ViewTool::Boundary { use_: BoundaryUse::CropSpline, .. } => {
            "Crop view: click the boundary's points · Enter or the first point closes it"
        }
        ViewTool::Boundary { .. } => "Broken-out section: click the boundary's points · Enter or the first point closes it",
        _ => return None,
    })
}

/// The centre of a view on the sheet (for scenarios' checks).
pub fn center_of(world: &World, v: &View) -> [f64; 2] {
    sheet_center(world.resource::<ViewCache>(), v)
}
