//! **Drawing sections in the Part Studio**: a section view of a drawing is a cut of the Part
//! Studio it draws, so the studio shows it too.
//!
//! - The feature list has a **Sections** group under Default geometry, one row per section view
//!   of the document's drawings that draw this studio ("Section A-A"). Hovering a row draws the
//!   cutting plane in the view; clicking it cuts the 3D view by that plane ([`crate::section_view`],
//!   the same cutaway as **Section view…**) seen as the drawing sees it; clicking it again ends
//!   the section.
//! - The **Named views** panel lists the same sections after the saved views
//!   ([`crate::view_options`]).
//! - From the drawing: a double click on a section view or on its cutting line, or the view
//!   menu's **Show section in …**, switches to the Part Studio with that section on
//!   ([`jump`]). While the pointer is over one, the hint at the bottom of the sheet says so.
//!
//! Nothing is stored: the sections are read from the drawings each frame they are needed, so a
//! section moved, renamed or deleted in the drawing is moved, renamed or gone here.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::document::{Document, ElementKind};
use cadrs_drawing::{ViewId, ViewKind};
use cadrs_ui::TreeItem;
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;

use crate::parts::PartCache;
use crate::section_view::{self, SectionPlaneGizmos, SectionRef, SectionViews};
use crate::viewport::{ActiveKind, ViewportView};
use crate::{ActiveDocument, AppState};

pub struct DrawingSectionsPlugin;

impl Plugin for DrawingSectionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingSection>().add_systems(
            Update,
            (apply_pending, sync_rows, draw_hovered_plane)
                .chain()
                .run_if(in_state(AppState::Document)),
        );
    }
}

/// A drawing's section view as its Part Studio sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingSection {
    pub drawing: ElementId,
    pub drawing_name: String,
    pub view: ViewId,
    /// "Section A-A".
    pub name: String,
    pub studio: ElementId,
    /// A point of the cutting plane (model mm).
    pub origin: Vec3,
    /// The unit normal towards the material the section removes (towards the eye).
    pub removed: Vec3,
}

/// The section views of the document's drawings that cut `studio`, in tab then sheet order.
pub fn sections_of(doc: &Document, studio: ElementId) -> Vec<DrawingSection> {
    let mut out = Vec::new();
    for el in &doc.elements {
        let ElementKind::Drawing(d) = &el.kind else {
            continue;
        };
        for v in d.sheets.iter().flat_map(|s| s.views.iter()) {
            if let Some(s) = section_of(el.id, &el.name, v)
                && s.studio == studio
            {
                out.push(s);
            }
        }
    }
    out
}

/// The section a drawing view stands for, if it is a section view.
pub fn section_of(
    drawing: ElementId,
    drawing_name: &str,
    v: &cadrs_drawing::View,
) -> Option<DrawingSection> {
    if v.kind != ViewKind::Section {
        return None;
    }
    let cut = v.cut.as_ref()?;
    // The view removes what is nearer the eye than `depth` along its direction of sight.
    let dir = Vec3::new(
        v.frame.dir[0] as f32,
        v.frame.dir[1] as f32,
        v.frame.dir[2] as f32,
    );
    Some(DrawingSection {
        drawing,
        drawing_name: drawing_name.to_string(),
        view: v.id,
        name: v.name.clone(),
        studio: ElementId(v.reference.element),
        origin: dir * cut.depth as f32,
        removed: -dir,
    })
}

/// The section view `view` of the open document's drawings.
pub fn find(doc: &Document, view: ViewId) -> Option<DrawingSection> {
    doc.elements.iter().find_map(|el| {
        let ElementKind::Drawing(d) = &el.kind else {
            return None;
        };
        d.sheets
            .iter()
            .flat_map(|s| s.views.iter())
            .find(|v| v.id == view)
            .and_then(|v| section_of(el.id, &el.name, v))
    })
}

/// The section of the active drawing a sheet point (mm) is on: the section view under it, or
/// the cutting line (within `tol` sheet mm) drawn on its parent.
pub fn section_at(
    d: &cadrs_drawing::Drawing,
    sheet: usize,
    hovered: Option<ViewId>,
    p: Vec2,
    tol: f64,
) -> Option<ViewId> {
    let sheet = d.sheets.get(sheet)?;
    let on_line = sheet.views.iter().find(|v| {
        let (Some(line), Some(parent)) = (&v.section, v.parent.and_then(|id| sheet.view(id)))
        else {
            return false;
        };
        let (a, b) = (parent.to_sheet(line.a), parent.to_sheet(line.b));
        let (a, b) = (
            Vec2::new(a[0] as f32, a[1] as f32),
            Vec2::new(b[0] as f32, b[1] as f32),
        );
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 {
            ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        ((a + ab * t) - p).length() as f64 <= tol
    });
    on_line.map(|v| v.id).or_else(|| {
        hovered
            .and_then(|id| sheet.view(id))
            .filter(|v| v.kind == ViewKind::Section && v.cut.is_some())
            .map(|v| v.id)
    })
}

/// A section waiting for its Part Studio's tab to be the one shown.
#[derive(Resource, Debug, Default)]
pub struct PendingSection(pub Option<DrawingSection>);

/// Switches to the section's Part Studio and shows the section there.
pub fn jump(world: &mut World, s: DrawingSection) {
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.set_active(s.studio);
    }
    world.resource_mut::<PendingSection>().0 = Some(s);
}

/// Shows the section view `view` of the open document in its Part Studio.
pub fn jump_to_view(world: &mut World, view: ViewId) -> bool {
    let Some(s) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| find(&d.doc, view))
    else {
        return false;
    };
    jump(world, s);
    true
}

/// A click on a section's row (the feature list, the Named views panel): shows it, or ends it
/// when it is the one shown.
pub fn toggle(world: &mut World, view: ViewId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let Some(s) = find(&doc.doc, view) else {
        return;
    };
    let shown = section_view::drawing_section_shown(world.resource::<SectionViews>(), s.studio);
    if shown == Some(view) && doc.active == Some(s.studio) {
        section_view::exit(world);
    } else {
        jump(world, s);
    }
}

/// The drawing section the active tab is cut by.
pub fn shown(world: &World) -> Option<ViewId> {
    let el = world.get_resource::<ActiveDocument>()?.active?;
    section_view::drawing_section_shown(world.resource::<SectionViews>(), el)
}

/// Once the section's Part Studio is the tab on screen (its camera is the viewport's): cuts it
/// and turns the view to look at the cut as the drawing does.
fn apply_pending(world: &mut World) {
    let Some(s) = world.resource::<PendingSection>().0.clone() else {
        return;
    };
    let active = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active);
    if active != Some(s.studio) {
        world.resource_mut::<PendingSection>().0 = None;
        return;
    }
    if world.resource::<ViewportView>().element != Some(s.studio)
        || *world.resource::<ActiveKind>() != ActiveKind::PartStudio
    {
        return;
    }
    world.resource_mut::<PendingSection>().0 = None;
    section_view::show_plane(
        world,
        s.studio,
        SectionRef::Drawing(s.view),
        s.name.clone(),
        s.origin,
        s.removed,
    );
    let mut view = world.resource_mut::<ViewportView>();
    let to = view.target().seen_from(s.removed);
    view.animate_to(to);
}

// ---------------------------------------------------------------------------------------------
// The feature list's Sections group

/// The container of the Sections group in the feature list (empty with no sections).
#[derive(Component, Default)]
pub struct SectionRows {
    built: Option<Vec<(ViewId, String, bool)>>,
}

/// A section's row.
#[derive(Component, Debug, Clone, Copy)]
pub struct SectionRow(pub ViewId);

fn sync_rows(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    views: Res<SectionViews>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &mut SectionRows)>,
    mut commands: Commands,
) {
    let Some(doc) = doc else { return };
    let Some(el) = doc.active.filter(|_| *kind == ActiveKind::PartStudio) else {
        return;
    };
    let sections = sections_of(&doc.doc, el);
    let shown = section_view::drawing_section_shown(&views, el);
    let several = sections.iter().any(|s| s.drawing != sections[0].drawing);
    let key: Vec<(ViewId, String, bool)> = sections
        .iter()
        .map(|s| {
            let label = if several {
                format!("{} ({})", s.name, s.drawing_name)
            } else {
                s.name.clone()
            };
            (s.view, label, shown == Some(s.view))
        })
        .collect();
    for (root, mut rows) in &mut q {
        if rows.built.as_ref() == Some(&key) {
            continue;
        }
        rows.built = Some(key.clone());
        commands.entity(root).despawn_related::<Children>();
        if key.is_empty() {
            continue;
        }
        let t = theme.clone();
        let key = key.clone();
        commands.entity(root).with_children(move |c| {
            c.spawn(
                TreeItem::new("feature-sections", tr("shell-doc-sections"))
                    .left(2.0)
                    .weight(FontWeight::NORMAL)
                    .build(&t),
            );
            for (i, (view, label, on)) in key.into_iter().enumerate() {
                c.spawn((
                    TreeItem::new(format!("feature-section-{i}"), label)
                        .icon("section-view", 16.0)
                        .left(22.0)
                        .selected(on)
                        .build(&t),
                    SectionRow(view),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |world: &mut World| toggle(world, view));
                    }),
                ));
            }
        });
    }
}

/// The cutting plane of the section whose row the pointer is on, drawn in the view.
fn draw_hovered_plane(
    q: Query<(&SectionRow, &Hovered)>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    mut gizmos: Gizmos<SectionPlaneGizmos>,
) {
    let Some(view) = q.iter().find(|(_, h)| h.get()).map(|(r, _)| r.0) else {
        return;
    };
    let Some(s) = doc.and_then(|d| find(&d.doc, view)) else {
        return;
    };
    let Some((lo, hi)) = section_view::parts_bounds(&cache) else {
        return;
    };
    let n = s.removed.normalize_or_zero();
    let middle = (lo + hi) / 2.0;
    let c = middle - n * (middle - s.origin).dot(n);
    let u = if n.cross(Vec3::Z).length() > 1e-3 {
        Vec3::Z.cross(n).normalize()
    } else {
        Vec3::X
    };
    let v = n.cross(u);
    let h = (hi - lo).length() * 0.6;
    let corners = [
        c - u * h - v * h,
        c + u * h - v * h,
        c + u * h + v * h,
        c - u * h + v * h,
    ];
    let color = Color::srgb_u8(0xF2, 0x8C, 0x1E);
    for k in 0..4 {
        gizmos.line(corners[k], corners[(k + 1) % 4], color);
    }
    // The side that goes: a short arrow from the middle.
    gizmos.arrow(c, c + n * h * 0.35, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_drawing::{NamedView, ObjectRef, Scale, View};

    #[test]
    fn a_section_view_is_its_studios_cut_towards_the_eye() {
        let studio = ElementId::new();
        let parent = View::base(
            ObjectRef {
                element: studio.0,
                part: None,
            },
            NamedView::Front,
            Scale::new(1, 1),
            [100.0, 100.0],
        );
        // A vertical cutting line at x = 3 of the front view, seen from the right.
        let v = cadrs_drawing::view_kinds::section_view(
            &parent,
            [3.0, -10.0],
            [3.0, 10.0],
            [200.0, 100.0],
            cadrs_drawing::Projection::Third,
            "A",
        )
        .unwrap();
        let drawing = ElementId::new();
        let s = section_of(drawing, "Drawing 1", &v).unwrap();
        assert_eq!(s.studio, studio);
        assert_eq!(s.name, "Section A-A");
        // The plane is x = 3 and the material on the eye's side (+X, third angle) goes.
        assert!((s.origin.dot(s.removed) - 3.0).abs() < 1e-6, "{s:?}");
        assert!((s.removed - Vec3::X).length() < 1e-6, "{s:?}");
        // The parent is no section.
        assert!(section_of(drawing, "Drawing 1", &parent).is_none());
    }

    #[test]
    fn a_point_on_the_cutting_line_or_the_section_view_finds_the_section() {
        let studio = ElementId::new();
        let parent = View::base(
            ObjectRef {
                element: studio.0,
                part: None,
            },
            NamedView::Front,
            Scale::new(2, 1),
            [100.0, 100.0],
        );
        let section = cadrs_drawing::view_kinds::section_view(
            &parent,
            [3.0, -10.0],
            [3.0, 10.0],
            [200.0, 100.0],
            cadrs_drawing::Projection::Third,
            "A",
        )
        .unwrap();
        let (pid, sid) = (parent.id, section.id);
        let mut d =
            cadrs_drawing::Drawing::from_template(&cadrs_drawing::builtin_templates()[0], None);
        d.sheets[0].views = vec![parent, section];
        // The line is at x = 3 of the parent, at 2:1: sheet x = 106, y from 80 to 120.
        assert_eq!(
            section_at(&d, 0, Some(pid), Vec2::new(106.4, 95.0), 1.0),
            Some(sid)
        );
        // Off the line, over the parent: nothing.
        assert_eq!(
            section_at(&d, 0, Some(pid), Vec2::new(90.0, 95.0), 1.0),
            None
        );
        // Past the line's end.
        assert_eq!(section_at(&d, 0, None, Vec2::new(106.0, 130.0), 1.0), None);
        // Over the section view itself.
        assert_eq!(
            section_at(&d, 0, Some(sid), Vec2::new(200.0, 100.0), 1.0),
            Some(sid)
        );
    }
}
