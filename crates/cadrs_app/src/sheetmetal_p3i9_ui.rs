//! P3I.9's dialogs (SM19.2, SM20) in the applied-feature dialogs, with their own roles,
//! observers and syncing so the shared dialog code only hands over to this module:
//!
//! - **Sheet metal loft** (`help/feature-tools/sheet-metal-loft-dialog-01.png`): **New | Add**
//!   tabs, *Merge scope* (Add), *Profile 1*, *Profile 2*, the **Connections** checkbox opening
//!   the *Match connections* box (each connection's two points, its **Rip** checkbox, Add
//!   connection), *Chordal tolerance*, then for New the **General**, **Material** and **Relief**
//!   sections of the Sheet metal model (General open, the others closed, as the help shows).
//!   In the view each connection is a magenta line with a round handle at each end; dragging a
//!   handle slides it along its profile (one "Drag connection" step).
//! - **Form** (`formed-03-02.png`, `forms-selectPS-dialog-01.png`): *Form Part Studio* (a field
//!   with the Part Studio icon: "Select Part Studio…", or the form's name) opening the **Select
//!   Part Studio** panel beside the dialog (Current document | Other documents | Libraries, a
//!   search field, Library / Type / Form, the form's variables, **Done**); *Location(s)* with the
//!   mate connector icon and the opposite direction arrow; *Target face(s)*.
//! - **Tag** (`form-06a.png`): the type (*Form*), *Part to add*, *Part to remove*, *Sketch for
//!   flat view*, *Form origin mate connector*.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::applied::EdgeOrFace;
use cadrs_core::document::{FaceRef, RegionRef, VertexRef};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
use cadrs_core::sheetmetal::{CurveRef, SheetMetalExprs, SheetMetalModelFeature, plain};
use cadrs_core::sheetmetal_form::{FormFeature, FormLocation, FormPick, FormSource, FormVariable, LIBRARY_NAME, LibraryForm, TagFormFeature, studio_variables, tag_of};
use cadrs_core::sheetmetal_loft::{LoftConnection, LoftItem, RegionRefKey, SheetMetalLoftFeature, SmLoftOp};
use cadrs_core::{Feature, FeatureId, FeatureKind, PartId};
use cadrs_sheetmetal::params::{BendCalc, BendReliefKind, CornerReliefKind};
use cadrs_sketch::units::Quantity;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, Collapsible, CollapsibleToggled, FloatingPanel, FloatingPanelClose, NumberField, NumberFieldCommit, NumberFieldState, OptionRow, Select, SelectChange, SelectState,
    SelectionList, SelectionListActivate, SelectionListRemove, SelectionListState, TabStrip, TabStripSelect,
};

use crate::ActiveDocument;
use crate::applied::{AppliedField, AppliedKind, AppliedSession};
use crate::applied_dialog::SmNum;
use crate::parts::{PartCache, PickFilter};
use crate::viewport::{Pick, ViewportArea};

// ---------------------------------------------------------------------------------------------
// Kinds, fields and roles

/// Which of P3I.9's features a session edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sm9Kind {
    Loft,
    Form,
    Tag,
}

/// The selection fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sm9Field {
    LoftProfile1,
    LoftProfile2,
    LoftScope,
    /// A connection's points (its index).
    LoftConnection(usize),
    FormLocations,
    FormTargets,
    TagAdd,
    TagRemove,
    TagSketch,
    TagOrigin,
}

/// The numbers: the model settings (as the Sheet metal model's), the chordal tolerance and a
/// form variable in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sm9Num {
    Model(SmNum),
    Chordal,
    Var(usize),
}

/// What a widget of these dialogs is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sm9Role {
    LoftTab,
    List(Sm9Field),
    Num(Sm9Num),
    ThicknessFlip,
    FormFlip,
    FormStudio,
    FormConnectorIcon,
    AddConnection,
    BendCalc,
    CornerType,
    BendReliefType,
    TagType,
    PickerTab,
    PickerType,
    PickerForm,
    PickerDocument,
    PickerDone,
}

impl Sm9Kind {
    pub fn of(kind: &FeatureKind) -> Option<Self> {
        match kind {
            FeatureKind::SheetMetalLoft(_) => Some(Sm9Kind::Loft),
            FeatureKind::Form(_) => Some(Sm9Kind::Form),
            FeatureKind::TagForm(_) => Some(Sm9Kind::Tag),
            _ => None,
        }
    }
}

const SECTIONS: [(&str, &str); 3] = [("sm9-general", "General"), ("sm9-material", "Material"), ("sm9-relief", "Relief")];

pub struct Sm9Plugin;

impl Plugin for Sm9Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FormPicker>()
            .init_resource::<ConnDrag>()
            .add_systems(
                Update,
                (sync_dialog, sync_picker, place_handles, draw_connections)
                    .chain()
                    .after(crate::applied_dialog::sync_applied_dialog)
                    .run_if(in_state(crate::AppState::Document)),
            )
            .add_observer(on_tab)
            .add_observer(on_select)
            .add_observer(on_checkbox)
            .add_observer(on_number)
            .add_observer(on_button)
            .add_observer(on_list_remove)
            .add_observer(on_list_activate)
            .add_observer(on_section)
            .add_observer(on_picker_close);
    }
}

// ---------------------------------------------------------------------------------------------
// The toolbar, Search tools and the feature list

/// Built sheet metal tools of the Sheet metal model's ▾ (P3I.9's).
pub fn built(tool: &str) -> bool {
    matches!(tool, "sheet-metal-form" | "sheet-metal-loft")
}

/// A sheet metal ▾ menu item: starts its feature (`true` if it was one of P3I.9's).
pub fn menu_action(world: &mut World, item: &str) -> bool {
    let kind = match item {
        "sheet-metal-menu-loft" | "sheet-metal-loft" => Sm9Kind::Loft,
        "sheet-metal-menu-form" | "sheet-metal-form" => Sm9Kind::Form,
        "tag-form" => Sm9Kind::Tag,
        _ => return false,
    };
    crate::applied::begin(world, AppliedKind::Sm9(kind));
    true
}

/// The feature list's icon.
pub fn row_icon(kind: &FeatureKind) -> Option<&'static str> {
    Some(match kind {
        FeatureKind::SheetMetalLoft(_) => "sheet-metal-loft",
        FeatureKind::Form(_) => "sheet-metal-form",
        FeatureKind::TagForm(_) => "tag",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------
// New features and picks

fn units(world: &World) -> cadrs_sketch::units::Units {
    world.get_resource::<crate::WorkspaceUnits>().map(|u| u.0).unwrap_or_default()
}

/// A new feature (from the selection where it fits).
pub fn initial(world: &World, kind: Sm9Kind, picked: &[Pick]) -> Option<(&'static str, FeatureKind, AppliedField)> {
    let u = units(world);
    match kind {
        Sm9Kind::Loft => {
            let mut x = SheetMetalLoftFeature::default();
            let p = x.params;
            x.exprs.thickness = u.with_unit(p.thickness, Quantity::Length);
            x.exprs.bend_radius = u.with_unit(p.bend_radius, Quantity::Length);
            x.exprs.minimal_gap = u.with_unit(p.minimal_gap, Quantity::Length);
            x.exprs.bend_allowance = u.with_unit(p.bend_allowance, Quantity::Length);
            x.exprs.bend_deduction = u.with_unit(p.bend_deduction, Quantity::Length);
            x.exprs.corner_relief_size = u.with_unit(p.corner_relief.size, Quantity::Length);
            x.chordal_tolerance_expr = u.with_unit(x.chordal_tolerance, Quantity::Length);
            let cache = world.resource::<PartCache>();
            for pk in picked {
                if let Some(it) = loft_item(cache, *pk) {
                    x.profile1.push(it);
                }
            }
            let field = if x.profile1.is_empty() { Sm9Field::LoftProfile1 } else { Sm9Field::LoftProfile2 };
            Some(("Sheet metal loft", FeatureKind::SheetMetalLoft(x), AppliedField::Sm9(field)))
        }
        Sm9Kind::Form => Some(("Form", FeatureKind::Form(FormFeature::default()), AppliedField::Sm9(Sm9Field::FormLocations))),
        Sm9Kind::Tag => Some(("Tag", FeatureKind::TagForm(TagFormFeature::default()), AppliedField::Sm9(Sm9Field::TagAdd))),
    }
}

/// The field an edit starts in.
pub fn first_field(kind: &FeatureKind) -> AppliedField {
    AppliedField::Sm9(match kind {
        FeatureKind::Form(_) => Sm9Field::FormLocations,
        FeatureKind::TagForm(_) => Sm9Field::TagAdd,
        _ => Sm9Field::LoftProfile1,
    })
}

fn region_key(cache: &PartCache, sketch: FeatureId, index: u32) -> Option<RegionRefKey> {
    let r = cache.sketch_regions(sketch)?.regions.get(index as usize)?.clone();
    Some(RegionRefKey::of(&RegionRef::new(sketch, &r)))
}

fn loft_item(cache: &PartCache, p: Pick) -> Option<LoftItem> {
    Some(match p {
        Pick::Region(s, i) => LoftItem::Region(region_key(cache, s, i)?),
        Pick::SketchCurve(sketch, curve) => LoftItem::Curve(CurveRef { sketch, curve }),
        Pick::SketchPoint(sketch, point) => LoftItem::SketchPoint { sketch, point },
        Pick::Vertex(part, vertex) => {
            let point = cache.part(part)?.solid.vertex(&vertex)?.point;
            LoftItem::Vertex(VertexRef { part, vertex, point })
        }
        Pick::Face(..) | Pick::Edge(..) => match crate::applied::entity_of(cache, p)? {
            EdgeOrFace::Face(f) => LoftItem::Face(f),
            EdgeOrFace::Edge(e) => LoftItem::Edge(e),
        },
        _ => return None,
    })
}

fn same_item(a: &LoftItem, b: &LoftItem) -> bool {
    match (a, b) {
        (LoftItem::Region(x), LoftItem::Region(y)) => x.sketch == y.sketch && (x.seed.x - y.seed.x).abs() < 1e-9 && (x.seed.y - y.seed.y).abs() < 1e-9,
        (LoftItem::Face(x), LoftItem::Face(y)) => x.face == y.face,
        (LoftItem::Edge(x), LoftItem::Edge(y)) => x.edge == y.edge,
        (LoftItem::Vertex(x), LoftItem::Vertex(y)) => x.vertex == y.vertex,
        (a, b) => a == b,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    match list.iter().position(|y| *y == x) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(x),
    }
}

fn features_of(world: &World) -> Vec<Feature> {
    world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.features().to_vec()).unwrap_or_default()
}

fn is_sketch(features: &[Feature], f: FeatureId) -> bool {
    features.iter().any(|x| x.id == f && x.sketch().is_some())
}

/// A pick into a field. Returns false if it doesn't fit.
pub fn pick(world: &mut World, kind: &mut FeatureKind, field: Sm9Field, pick: Pick) -> bool {
    let features = features_of(world);
    let cache = world.resource::<PartCache>();
    let mut next: Option<Sm9Field> = None;
    match (kind, field) {
        (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftProfile1 | Sm9Field::LoftProfile2) => {
            let Some(it) = loft_item(cache, pick) else { return false };
            let list = if field == Sm9Field::LoftProfile1 { &mut x.profile1 } else { &mut x.profile2 };
            match list.iter().position(|y| same_item(y, &it)) {
                Some(i) => {
                    list.remove(i);
                }
                None => {
                    // A region, face or point is a whole profile: on to the next field.
                    let whole = matches!(it, LoftItem::Region(_) | LoftItem::Face(_) | LoftItem::SketchPoint { .. } | LoftItem::Vertex(_));
                    if whole {
                        list.clear();
                        if field == Sm9Field::LoftProfile1 {
                            next = Some(Sm9Field::LoftProfile2);
                        }
                    }
                    list.push(it);
                }
            }
        }
        (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftScope) => match pick.part() {
            Some(p) => toggle(&mut x.merge_scope, p),
            None => return false,
        },
        (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftConnection(i)) => {
            // A vertex or edge of a profile: the connection's end on the nearer profile.
            let point = match pick {
                Pick::Vertex(part, v) => cache.part(part).and_then(|p| p.solid.vertex(&v)).map(|v| v.point),
                Pick::Edge(part, e) => cache.part(part).and_then(|p| p.solid.edge(&e)).map(|e| e.midpoint()),
                Pick::SketchPoint(s, p) => features.iter().find(|f| f.id == s).and_then(|f| f.sketch()).and_then(|sk| Some(sk.plane?.frame().to_world(sk.geometry.points.get(p)?.pos))),
                Pick::SketchCurve(s, c) => features
                    .iter()
                    .find(|f| f.id == s)
                    .and_then(|f| f.sketch())
                    .and_then(|sk| Some(sk.plane?.frame().to_world(curve_mid(&sk.geometry, c)?))),
                _ => None,
            };
            let Some(point) = point else { return false };
            let Some(g) = world.resource::<PartCache>().arrows.get(&world.resource::<AppliedSession>().feature).and_then(|a| Guides::decode(a)) else { return false };
            let Some(c) = x.connections.get_mut(i) else { return false };
            let (t1, d1) = g.nearest_t(&g.p1, g.closed.0, Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32));
            let (t2, d2) = g.nearest_t(&g.p2, g.closed.1, Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32));
            if d1 <= d2 {
                c.t1 = t1;
            } else {
                c.t2 = t2;
            }
        }
        (FeatureKind::Form(x), Sm9Field::FormLocations) => {
            let loc = match pick {
                Pick::SketchPoint(sketch, point) => FormLocation::Connector(ConnectorRef::Implicit(ConnectorOrigin::SketchPoint { sketch, point })),
                Pick::Vertex(part, vertex) => {
                    let Some(point) = cache.part(part).and_then(|p| p.solid.vertex(&vertex)).map(|v| v.point) else { return false };
                    FormLocation::Connector(ConnectorRef::Implicit(ConnectorOrigin::Vertex(VertexRef { part, vertex, point })))
                }
                Pick::Feature(f) if crate::pattern::is_connector(&features, f) => FormLocation::Connector(ConnectorRef::Feature(f)),
                Pick::Feature(f) if is_sketch(&features, f) => FormLocation::SketchPoints(f),
                _ => return false,
            };
            toggle(&mut x.locations, loc);
        }
        (FeatureKind::Form(x), Sm9Field::FormTargets) => {
            let Some(EdgeOrFace::Face(f)) = crate::applied::entity_of(cache, pick) else { return false };
            match x.targets.iter().position(|g| g.face == f.face) {
                Some(i) => {
                    x.targets.remove(i);
                }
                None => x.targets.push(f),
            }
        }
        (FeatureKind::TagForm(x), Sm9Field::TagAdd | Sm9Field::TagRemove) => {
            let Some(p) = pick.part() else { return false };
            let list = if field == Sm9Field::TagAdd { &mut x.add } else { &mut x.remove };
            toggle(list, p);
            next = Some(if field == Sm9Field::TagAdd { Sm9Field::TagRemove } else { Sm9Field::TagSketch });
        }
        (FeatureKind::TagForm(x), Sm9Field::TagSketch) => {
            let s = match pick {
                Pick::Feature(f) if is_sketch(&features, f) => f,
                Pick::SketchCurve(s, _) | Pick::SketchPoint(s, _) => s,
                _ => return false,
            };
            x.sketch = if x.sketch == Some(s) { None } else { Some(s) };
            next = Some(Sm9Field::TagOrigin);
        }
        (FeatureKind::TagForm(x), Sm9Field::TagOrigin) => {
            let c = match pick {
                Pick::Feature(f) if crate::pattern::is_connector(&features, f) => ConnectorRef::Feature(f),
                Pick::Origin => ConnectorRef::Implicit(ConnectorOrigin::Origin),
                _ => return false,
            };
            x.origin = if x.origin == Some(c) { None } else { Some(c) };
        }
        _ => return false,
    }
    if let (Some(f), Some(mut s)) = (next, world.get_resource_mut::<AppliedSession>()) {
        s.field = AppliedField::Sm9(f);
    }
    true
}

fn curve_mid(g: &cadrs_sketch::Sketch, c: cadrs_sketch::CurveId) -> Option<cadrs_sketch::Vec2> {
    match g.curves.get(c)?.kind {
        cadrs_sketch::CurveKind::Line { a, b } => {
            let (a, b) = (g.pos(a), g.pos(b));
            Some(cadrs_sketch::Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
        }
        cadrs_sketch::CurveKind::Arc { .. } => {
            let a = g.arc_geom(c)?;
            Some(a.point_at(a.start_angle + a.sweep / 2.0))
        }
        cadrs_sketch::CurveKind::Circle { center, .. } => Some(g.pos(center)),
        _ => None,
    }
}

/// What a pick of `field` may be.
pub fn pick_filter(field: Sm9Field, none: PickFilter) -> PickFilter {
    match field {
        Sm9Field::LoftProfile1 | Sm9Field::LoftProfile2 => PickFilter { faces: true, edges: true, regions: true, sketch_curves: true, sketch_points: true, ..none },
        Sm9Field::LoftScope => PickFilter { faces: true, edges: true, ..none },
        Sm9Field::LoftConnection(_) => PickFilter { edges: true, sketch_curves: true, sketch_points: true, ..none },
        Sm9Field::FormLocations => PickFilter { edges: true, sketch_points: true, connectors: true, ..none },
        Sm9Field::FormTargets => PickFilter { faces: true, planar_only: true, ..none },
        Sm9Field::TagAdd | Sm9Field::TagRemove => PickFilter { faces: true, edges: true, ..none },
        Sm9Field::TagSketch => PickFilter { sketch_curves: true, sketch_points: true, ..none },
        Sm9Field::TagOrigin => PickFilter { origin: true, connectors: true, ..none },
    }
}

/// What the fields refer to, shown selected in the view.
pub fn references(kind: &FeatureKind, cache: &PartCache) -> Vec<Pick> {
    let face = |f: &FaceRef| cache.parts.iter().find(|p| p.solid.face(&f.face).is_some()).map(|p| Pick::Face(p.id, f.face));
    let mut v = Vec::new();
    match kind {
        FeatureKind::SheetMetalLoft(x) => {
            for it in x.profile1.iter().chain(&x.profile2) {
                match it {
                    LoftItem::Face(f) => v.extend(face(f)),
                    LoftItem::Curve(c) => v.push(Pick::SketchCurve(c.sketch, c.curve)),
                    LoftItem::SketchPoint { sketch, point } => v.push(Pick::SketchPoint(*sketch, *point)),
                    LoftItem::Edge(e) => {
                        if let Some(p) = cache.parts.iter().find(|p| p.solid.edge(&e.edge).is_some()) {
                            v.push(Pick::Edge(p.id, e.edge));
                        }
                    }
                    _ => {}
                }
            }
        }
        FeatureKind::Form(x) => {
            v.extend(x.targets.iter().filter_map(face));
            for l in &x.locations {
                if let FormLocation::Connector(ConnectorRef::Implicit(ConnectorOrigin::SketchPoint { sketch, point })) = l {
                    v.push(Pick::SketchPoint(*sketch, *point));
                }
            }
        }
        _ => {}
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Labels

fn op_name(features: &[Feature], op: cadrs_sketch::OpId) -> String {
    features.iter().find(|f| f.id.0 == op).map_or("part".into(), |f| f.name.clone())
}

fn name_of(features: &[Feature], id: FeatureId) -> String {
    features.iter().find(|f| f.id == id).map_or("sketch".into(), |f| f.name.clone())
}

fn item_label(features: &[Feature], it: &LoftItem) -> String {
    match it {
        LoftItem::Region(r) => format!("Face of {}", name_of(features, r.sketch)),
        LoftItem::Face(f) => format!("Face of {}", op_name(features, f.face.op)),
        LoftItem::Curve(c) => format!("Edge of {}", name_of(features, c.sketch)),
        LoftItem::Edge(e) => format!("Edge of {}", op_name(features, crate::parts::edge_maker(features, &e.edge))),
        LoftItem::SketchPoint { sketch, .. } => format!("Point of {}", name_of(features, *sketch)),
        LoftItem::Vertex(v) => format!("Vertex of {}", name_of(features, v.part.feature)),
    }
}

fn location_label(features: &[Feature], l: &FormLocation) -> String {
    match l {
        FormLocation::SketchPoints(s) => format!("Vertices of {}", name_of(features, *s)),
        FormLocation::Connector(ConnectorRef::Implicit(ConnectorOrigin::SketchPoint { sketch, .. })) => format!("Point of {}", name_of(features, *sketch)),
        FormLocation::Connector(ConnectorRef::Implicit(ConnectorOrigin::Vertex(v))) => format!("Vertex of {}", name_of(features, v.part.feature)),
        FormLocation::Connector(c) => c.label(features),
    }
}

/// Every list's items.
pub fn lists(features: &[Feature], cache: &PartCache, kind: &FeatureKind) -> Vec<(Sm9Field, Vec<String>)> {
    match kind {
        FeatureKind::SheetMetalLoft(x) => {
            let mut v = vec![
                (Sm9Field::LoftProfile1, x.profile1.iter().map(|i| item_label(features, i)).collect()),
                (Sm9Field::LoftProfile2, x.profile2.iter().map(|i| item_label(features, i)).collect()),
                (Sm9Field::LoftScope, crate::applied::part_names(cache, &x.merge_scope)),
            ];
            for (i, c) in x.connections.iter().enumerate() {
                v.push((Sm9Field::LoftConnection(i), vec![format!("Point of Profile 1 ({:.0}%)", c.t1 * 100.0), format!("Point of Profile 2 ({:.0}%)", c.t2 * 100.0)]));
            }
            v
        }
        FeatureKind::Form(x) => vec![
            (Sm9Field::FormLocations, x.locations.iter().map(|l| location_label(features, l)).collect()),
            (Sm9Field::FormTargets, x.targets.iter().map(|f| format!("Face of {}", op_name(features, f.face.op))).collect()),
        ],
        FeatureKind::TagForm(x) => vec![
            (Sm9Field::TagAdd, crate::applied::part_names(cache, &x.add)),
            (Sm9Field::TagRemove, crate::applied::part_names(cache, &x.remove)),
            (Sm9Field::TagSketch, x.sketch.iter().map(|s| name_of(features, *s)).collect()),
            (Sm9Field::TagOrigin, x.origin.iter().map(|c| c.label(features)).collect()),
        ],
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// The dialogs

/// The dialog's name (`<name>-dialog`) and width.
pub fn name(kind: &FeatureKind) -> Option<(&'static str, f32)> {
    Some(match kind {
        FeatureKind::SheetMetalLoft(_) => ("sheet-metal-loft", 230.0),
        FeatureKind::Form(_) => ("sheet-metal-form", 216.0),
        FeatureKind::TagForm(_) => ("tag", 230.0),
        _ => return None,
    })
}

/// What decides the dialog's rows.
pub fn layout(kind: &FeatureKind) -> Option<String> {
    Some(match kind {
        FeatureKind::SheetMetalLoft(x) => {
            let p = &x.params;
            format!(
                "sm9-loft {:?} {} {:?} {:?} {:?} {:?} {}",
                x.op,
                x.connections_on,
                x.connections.iter().map(|c| c.rip).collect::<Vec<_>>(),
                p.bend_calc,
                p.corner_relief.kind,
                p.bend_relief.kind,
                x.flip_thickness
            )
        }
        FeatureKind::Form(x) => format!("sm9-form {:?} {}", x.form.as_ref().map(|f| f.name.clone()), x.flip),
        FeatureKind::TagForm(_) => "sm9-tag".into(),
        _ => return None,
    })
}

fn list(b: &mut ChildSpawner, t: &Theme, name: &str, placeholder: &str, field: Sm9Field, items: &[(Sm9Field, Vec<String>)], active: AppliedField) {
    let it = items.iter().find(|(f, _)| *f == field).map(|(_, v)| v.clone()).unwrap_or_default();
    b.spawn((Sm9Role::List(field), SelectionList::new(name.to_string()).placeholder(placeholder).items(it).active(active == AppliedField::Sm9(field)).build(t)))
        .entry::<Node>()
        .and_modify(|mut n| {
            n.flex_grow = 0.0;
            n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(3.0), Val::Px(3.0));
        });
}

fn number(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, num: Sm9Num, text: &str, flip: Option<(Sm9Role, bool)>) {
    b.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), margin: UiRect::new(Val::Px(-16.0), Val::ZERO, Val::Px(1.0), Val::Px(1.0)), ..default() }).with_children(|r| {
        r.spawn((Sm9Role::Num(num), NumberField::new(name.to_string(), label.to_string()).text(text.to_string()).label_width(112.0).build(t)))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        match flip {
            Some((role, on)) => crate::extrude_dialog::flip_button_any(r, t, &format!("{name}-flip"), role, on, "Opposite direction"),
            None => {
                r.spawn(Node { width: Val::Px(22.0), flex_shrink: 0.0, ..default() });
            }
        }
    });
}

fn select(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, role: Sm9Role, options: &[String], selected: usize) {
    let mut s = Select::new(name.to_string());
    for o in options {
        s = s.option(o.clone(), true);
    }
    b.spawn(Node { height: Val::Px(28.0), margin: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(1.0), Val::Px(1.0)), align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() })
        .with_children(|r| {
            if !label.is_empty() {
                r.spawn((t.text(label, 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(90.0), flex_shrink: 0.0, ..default() }));
            }
            r.spawn((role, s.selected(selected).build(t))).entry::<Node>().and_modify(|mut n| n.flex_grow = 1.0);
        });
}

fn check(b: &mut ChildSpawner, t: &Theme, name: &str, label: &str, on: bool) {
    b.spawn(OptionRow::new(name.to_string(), label.to_string()).checked(on).build(t));
}

fn labels<T: Copy>(all: &[T], label: impl Fn(T) -> &'static str) -> Vec<String> {
    all.iter().map(|x| label(*x).to_string()).collect()
}

fn index_of<T: PartialEq>(all: &[T], x: &T) -> usize {
    all.iter().position(|y| y == x).unwrap_or(0)
}

/// The loft's settings as a Sheet metal model's (to share its numbers and ranges).
fn as_model(x: &SheetMetalLoftFeature) -> SheetMetalModelFeature {
    SheetMetalModelFeature { params: x.params, exprs: x.exprs.clone(), flip_thickness: x.flip_thickness, ..Default::default() }
}

fn model_text(x: &SheetMetalLoftFeature, n: SmNum) -> String {
    let e = &x.exprs;
    match n {
        SmNum::Thickness => e.thickness.clone(),
        SmNum::BendRadius => e.bend_radius.clone(),
        SmNum::KFactor => e.k_factor.clone(),
        SmNum::RolledK => e.rolled_k_factor.clone(),
        SmNum::Allowance => e.bend_allowance.clone(),
        SmNum::Deduction => e.bend_deduction.clone(),
        SmNum::MinimalGap => e.minimal_gap.clone(),
        SmNum::CornerScale => e.corner_relief_scale.clone(),
        SmNum::CornerSize => e.corner_relief_size.clone(),
        SmNum::BendDepthScale => e.bend_relief_depth_scale.clone(),
        SmNum::BendWidthScale => e.bend_relief_width_scale.clone(),
        _ => String::new(),
    }
}

/// The dialog's body.
pub fn body(b: &mut ChildSpawner, t: &Theme, kind: &FeatureKind, field: AppliedField, items: &[(Sm9Field, Vec<String>)], open: [bool; 3]) {
    match kind {
        FeatureKind::SheetMetalLoft(x) => loft_body(b, t, x, field, items, open),
        FeatureKind::Form(x) => crate::applied_dialog::body_column(b, |c| {
            c.spawn((t.text("Form Part Studio", 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::new(Val::Px(4.0), Val::ZERO, Val::Px(4.0), Val::Px(2.0)), ..default() }));
            let label = x.form.as_ref().map_or("Select Part Studio...".to_string(), |f| f.name.clone());
            let picked = x.form.is_some();
            c.spawn((Sm9Role::FormStudio, Button::new("form-studio-field").label(label).icon("part-studio").outline().width(Val::Percent(100.0)).tooltip("Select Part Studio").build(t)))
                .entry::<Node>()
                .and_modify(move |mut n| {
                    n.justify_content = JustifyContent::FlexStart;
                    n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(1.0), Val::Px(4.0));
                    if !picked {
                        n.height = Val::Px(30.0);
                    }
                });
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                r.spawn(Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, ..default() }).with_children(|l| list(l, t, "form-locations-field", "Location(s)", Sm9Field::FormLocations, items, field));
                r.spawn((Sm9Role::FormConnectorIcon, IconButton::new("form-connector", "mate-connector").icon_size(18.0).tooltip("Mate connector: pick an implicit one in the view").build(t)))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(22.0);
                        n.height = Val::Px(22.0);
                        n.flex_shrink = 0.0;
                    });
                crate::extrude_dialog::flip_button_any(r, t, "form-flip", Sm9Role::FormFlip, x.flip, "Opposite direction");
            });
            list(c, t, "form-targets-field", "Target face(s)", Sm9Field::FormTargets, items, field);
        }),
        FeatureKind::TagForm(_) => crate::applied_dialog::body_column(b, |c| {
            select(c, t, "tag-type", "", Sm9Role::TagType, &["Form".to_string()], 0);
            list(c, t, "tag-add-field", "Part to add", Sm9Field::TagAdd, items, field);
            list(c, t, "tag-remove-field", "Part to remove", Sm9Field::TagRemove, items, field);
            list(c, t, "tag-sketch-field", "Sketch for flat view", Sm9Field::TagSketch, items, field);
            list(c, t, "tag-origin-field", "Form origin mate connector", Sm9Field::TagOrigin, items, field);
        }),
        _ => {}
    }
}

fn loft_body(b: &mut ChildSpawner, t: &Theme, x: &SheetMetalLoftFeature, field: AppliedField, items: &[(Sm9Field, Vec<String>)], open: [bool; 3]) {
    let mut strip = TabStrip::new("sheet-metal-loft-op").compact();
    for op in SmLoftOp::ALL {
        strip = strip.tab(op.label());
    }
    b.spawn((Sm9Role::LoftTab, strip.selected(index_of(&SmLoftOp::ALL, &x.op)).build(t)));
    let items: Vec<(Sm9Field, Vec<String>)> = items.to_vec();
    let x = x.clone();
    let tt = t.clone();
    b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(6.0), Val::Px(4.0), Val::Px(4.0), Val::ZERO), ..default() }).with_children(move |c| {
        let t = &tt;
        if x.op == SmLoftOp::Add {
            list(c, t, "sm-loft-scope-field", "Merge scope", Sm9Field::LoftScope, &items, field);
        }
        list(c, t, "sm-loft-profile1-field", "Profile 1", Sm9Field::LoftProfile1, &items, field);
        list(c, t, "sm-loft-profile2-field", "Profile 2", Sm9Field::LoftProfile2, &items, field);
        c.spawn(OptionRow::new("sm9-connections", "Connections").chevron().checked(x.connections_on).build(t));
        if x.connections_on {
            c.spawn(Node {
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(4.0)),
                margin: UiRect::new(Val::Px(16.0), Val::Px(2.0), Val::Px(2.0), Val::Px(4.0)),
                ..default()
            })
            .insert(BorderColor::all(t.border))
            .with_children(|m| {
                m.spawn(t.text("Match connections", 10.0, bevy::text::FontWeight::NORMAL, t.muted_foreground));
                for (i, c) in x.connections.iter().enumerate() {
                    list(m, t, &format!("sm-loft-connection-{i}-field"), "Vertices or edges", Sm9Field::LoftConnection(i), &items, field);
                    check(m, t, &format!("sm9-rip-{i}"), "Rip", c.rip);
                }
                m.spawn((Sm9Role::AddConnection, Button::new("sm-loft-add-connection").label("Add connection").ghost().small().build(t)));
            });
        }
        number(c, t, "sm-loft-chordal-tolerance", "Chordal tolerance", Sm9Num::Chordal, &x.chordal_tolerance_expr, None);
        if x.op == SmLoftOp::New {
            let x1 = x.clone();
            let t1 = t.clone();
            c.spawn(
                Collapsible::new(SECTIONS[0].0, SECTIONS[0].1)
                    .section()
                    .open(open[0])
                    .content(move |c| {
                        let (x, t) = (&x1, &t1);
                        number(c, t, "sm-loft-thickness", "Thickness", Sm9Num::Model(SmNum::Thickness), &x.exprs.thickness, Some((Sm9Role::ThicknessFlip, x.flip_thickness)));
                        number(c, t, "sm-loft-bend-radius", "Bend radius", Sm9Num::Model(SmNum::BendRadius), &x.exprs.bend_radius, None);
                        check(c, t, "sm9-flip-direction-up", "Flip direction up", x.params.flip_direction_up);
                    })
                    .build(t),
            );
            let x2 = x.clone();
            let t2 = t.clone();
            c.spawn(
                Collapsible::new(SECTIONS[1].0, SECTIONS[1].1)
                    .section()
                    .open(open[1])
                    .content(move |c| {
                        let (x, t) = (&x2, &t2);
                        let p = &x.params;
                        select(c, t, "sm-loft-bend-calculation", "Bend calculation", Sm9Role::BendCalc, &labels(&BendCalc::ALL, BendCalc::label), index_of(&BendCalc::ALL, &p.bend_calc));
                        match p.bend_calc {
                            BendCalc::KFactor => number(c, t, "sm-loft-k-factor", "Default bend K Factor", Sm9Num::Model(SmNum::KFactor), &x.exprs.k_factor, None),
                            BendCalc::BendAllowance => number(c, t, "sm-loft-bend-allowance", "Default bend allowance", Sm9Num::Model(SmNum::Allowance), &x.exprs.bend_allowance, None),
                            BendCalc::BendDeduction => number(c, t, "sm-loft-bend-deduction", "Default bend deduction", Sm9Num::Model(SmNum::Deduction), &x.exprs.bend_deduction, None),
                        }
                        number(c, t, "sm-loft-rolled-k-factor", "Rolled K Factor", Sm9Num::Model(SmNum::RolledK), &x.exprs.rolled_k_factor, None);
                    })
                    .build(t),
            );
            let x3 = x.clone();
            let t3 = t.clone();
            c.spawn(
                Collapsible::new(SECTIONS[2].0, SECTIONS[2].1)
                    .section()
                    .open(open[2])
                    .content(move |c| {
                        let (x, t) = (&x3, &t3);
                        let p = &x.params;
                        number(c, t, "sm-loft-minimal-gap", "Minimal gap", Sm9Num::Model(SmNum::MinimalGap), &x.exprs.minimal_gap, None);
                        select(c, t, "sm-loft-corner-relief-type", "Corner relief type", Sm9Role::CornerType, &labels(&CornerReliefKind::ALL, CornerReliefKind::label), index_of(&CornerReliefKind::ALL, &p.corner_relief.kind));
                        match p.corner_relief.kind {
                            k if k.is_scaled() => number(c, t, "sm-loft-corner-relief-scale", "Corner relief scale", Sm9Num::Model(SmNum::CornerScale), &x.exprs.corner_relief_scale, None),
                            CornerReliefKind::SquareSized => number(c, t, "sm-loft-corner-relief-size", "Corner relief width", Sm9Num::Model(SmNum::CornerSize), &x.exprs.corner_relief_size, None),
                            CornerReliefKind::RoundSized => number(c, t, "sm-loft-corner-relief-size", "Corner relief diameter", Sm9Num::Model(SmNum::CornerSize), &x.exprs.corner_relief_size, None),
                            _ => {}
                        }
                        select(c, t, "sm-loft-bend-relief-type", "Bend relief type", Sm9Role::BendReliefType, &labels(&BendReliefKind::MODEL, BendReliefKind::label), index_of(&BendReliefKind::MODEL, &p.bend_relief.kind));
                        if p.bend_relief.kind.is_scaled() {
                            number(c, t, "sm-loft-bend-relief-depth-scale", "Bend relief depth scale", Sm9Num::Model(SmNum::BendDepthScale), &x.exprs.bend_relief_depth_scale, None);
                            number(c, t, "sm-loft-bend-relief-width-scale", "Bend relief width scale", Sm9Num::Model(SmNum::BendWidthScale), &x.exprs.bend_relief_width_scale, None);
                        }
                    })
                    .build(t),
            );
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Keeping the dialog in step

fn session_kind(session: &Option<Res<AppliedSession>>, doc: &Option<Res<ActiveDocument>>) -> Option<(FeatureKind, Vec<Feature>)> {
    let s = session.as_ref()?;
    if !matches!(s.kind, AppliedKind::Sm9(_)) {
        return None;
    }
    let el = doc.as_ref()?.doc.element(s.element)?;
    Some((el.feature(s.feature)?.kind.clone(), el.features().to_vec()))
}

fn number_text(kind: &FeatureKind, n: Sm9Num) -> Option<String> {
    match (kind, n) {
        (FeatureKind::SheetMetalLoft(x), Sm9Num::Chordal) => Some(x.chordal_tolerance_expr.clone()),
        (FeatureKind::SheetMetalLoft(x), Sm9Num::Model(m)) => Some(model_text(x, m)),
        _ => None,
    }
}

fn range_error(kind: &FeatureKind, n: Sm9Num) -> Option<String> {
    match (kind, n) {
        (FeatureKind::SheetMetalLoft(x), Sm9Num::Chordal) => (x.chordal_tolerance.is_nan() || x.chordal_tolerance <= 0.0).then(|| "Chordal tolerance must be greater than 0".into()),
        (FeatureKind::SheetMetalLoft(x), Sm9Num::Model(m)) => crate::sheetmetal_ui::range_error(&as_model(x), m),
        _ => None,
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_dialog(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    focus: Res<InputFocus>,
    q_edit: Query<&cadrs_ui::NumberFieldEdit>,
    mut q_lists: Query<(&Sm9Role, &mut SelectionListState)>,
    mut q_numbers: Query<(Entity, &Sm9Role, &mut NumberFieldState, Option<&Tooltip>), Without<SelectionListState>>,
    mut commands: Commands,
) {
    let Some((kind, features)) = session_kind(&session, &doc) else { return };
    let field = session.as_ref().map(|s| s.field);
    let items = lists(&features, &cache, &kind);
    for (role, mut l) in &mut q_lists {
        let Sm9Role::List(f) = *role else { continue };
        let it = items.iter().find(|(g, _)| *g == f).map(|(_, v)| v.clone()).unwrap_or_default();
        let want = SelectionListState { items: it, active: field == Some(AppliedField::Sm9(f)), error: false, red_items: false, red: Vec::new() };
        if *l != want {
            *l = want;
        }
    }
    let editing = focus.get().and_then(|f| q_edit.get(f).ok()).map(|e| e.0);
    for (entity, role, mut st, tip) in &mut q_numbers {
        let Sm9Role::Num(n) = *role else { continue };
        let err = range_error(&kind, n);
        if editing != Some(entity)
            && let Some(text) = number_text(&kind, n)
        {
            let unparsed = st.error && err.is_none() && st.text != text;
            if !unparsed {
                let want = NumberFieldState { text, error: err.is_some() };
                if *st != want {
                    *st = want;
                }
            }
        }
        let want_tip = err.or_else(|| st.error.then(|| "Not a valid value".to_string()));
        match (want_tip, tip) {
            (Some(m), Some(t)) if t.text == m => {}
            (Some(m), _) => {
                commands.entity(entity).insert(Tooltip::error(m));
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<Tooltip>();
            }
            (None, None) => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn change(commands: &mut Commands, label: &'static str, f: impl FnOnce(&mut FeatureKind) + Send + 'static) {
    commands.queue(move |world: &mut World| crate::applied::change_kind(world, label, f));
}

fn set_field(commands: &mut Commands, f: Sm9Field) {
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<AppliedSession>() {
            s.field = AppliedField::Sm9(f);
        }
    });
}

fn on_tab(ev: On<TabStripSelect>, q: Query<&Sm9Role>, mut commands: Commands) {
    let i = ev.index;
    match q.get(ev.entity) {
        Ok(Sm9Role::LoftTab) => {
            let op = SmLoftOp::ALL[i.min(1)];
            change(&mut commands, "New / Add", move |k| {
                if let FeatureKind::SheetMetalLoft(x) = k {
                    x.op = op;
                }
            });
            set_field(&mut commands, if op == SmLoftOp::Add { Sm9Field::LoftScope } else { Sm9Field::LoftProfile1 });
        }
        Ok(Sm9Role::PickerTab) => {
            commands.queue(move |world: &mut World| {
                let mut p = world.resource_mut::<FormPicker>();
                p.tab = i.min(2);
                p.form = 0;
                p.kind = 0;
                p.document = 0;
                p.values.clear();
                refresh_picker_values(world);
            });
        }
        _ => {}
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Sm9Role>, mut commands: Commands) {
    let i = ev.index;
    let Ok(role) = q.get(ev.entity).copied() else { return };
    match role {
        Sm9Role::BendCalc | Sm9Role::CornerType | Sm9Role::BendReliefType => {
            let label = match role {
                Sm9Role::BendCalc => "Bend calculation",
                Sm9Role::CornerType => "Corner relief type",
                _ => "Bend relief type",
            };
            change(&mut commands, label, move |k| {
                if let FeatureKind::SheetMetalLoft(x) = k {
                    let p = &mut x.params;
                    match role {
                        Sm9Role::BendCalc => p.bend_calc = BendCalc::ALL[i.min(2)],
                        Sm9Role::CornerType => p.corner_relief.kind = CornerReliefKind::ALL[i.min(5)],
                        _ => p.bend_relief.kind = BendReliefKind::MODEL[i.min(2)],
                    }
                }
            });
        }
        Sm9Role::PickerType | Sm9Role::PickerForm | Sm9Role::PickerDocument => {
            commands.queue(move |world: &mut World| {
                let mut p = world.resource_mut::<FormPicker>();
                match role {
                    Sm9Role::PickerType => {
                        p.kind = i;
                        p.form = 0;
                    }
                    Sm9Role::PickerDocument => {
                        p.document = i;
                        p.form = 0;
                    }
                    _ => p.form = i,
                }
                p.values.clear();
                refresh_picker_values(world);
            });
        }
        _ => {}
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let on = ev.checked;
    let n = name.as_str().to_string();
    if !n.starts_with("sm9-") {
        return;
    }
    commands.queue(move |world: &mut World| {
        let guides = world.get_resource::<AppliedSession>().and_then(|s| world.resource::<PartCache>().arrows.get(&s.feature).and_then(|a| Guides::decode(a)));
        let mut field = None;
        crate::applied::change_kind(world, "Connections", |k| {
            let FeatureKind::SheetMetalLoft(x) = k else { return };
            if n == "sm9-connections-checkbox" {
                x.connections_on = on;
                if on && x.connections.is_empty() {
                    // Start from the matched start: where the loft connects them now.
                    let c = guides.as_ref().and_then(|g| g.start_connection()).unwrap_or(LoftConnection { t1: 0.0, t2: 0.0, rip: false });
                    x.connections.push(c);
                }
                if on {
                    field = Some(Sm9Field::LoftConnection(0));
                }
            } else if n == "sm9-flip-direction-up-checkbox" {
                x.params.flip_direction_up = on;
            } else if let Some(i) = n.strip_prefix("sm9-rip-").and_then(|r| r.strip_suffix("-checkbox")).and_then(|r| r.parse::<usize>().ok())
                && let Some(c) = x.connections.get_mut(i)
            {
                c.rip = on;
            }
        });
        if let (Some(f), Some(mut s)) = (field, world.get_resource_mut::<AppliedSession>()) {
            s.field = AppliedField::Sm9(f);
        }
    });
}

fn on_number(ev: On<NumberFieldCommit>, q: Query<&Sm9Role>, mut commands: Commands) {
    let Ok(Sm9Role::Num(n)) = q.get(ev.entity).copied() else { return };
    let (entity, text, enter) = (ev.entity, ev.text.trim().to_string(), ev.enter);
    commands.queue(move |world: &mut World| {
        let units = world.resource::<crate::WorkspaceUnits>().0;
        let q = match n {
            Sm9Num::Model(m) => crate::sheetmetal_ui::quantity(m),
            Sm9Num::Chordal => Quantity::Length,
            Sm9Num::Var(i) => {
                if world.resource::<FormPicker>().values.get(i).is_some_and(|v| v.angle) {
                    Quantity::Angle
                } else {
                    Quantity::Length
                }
            }
        };
        let parsed = crate::sheetmetal_ui::parse(&text, q, &units, world.resource::<crate::variables_ui::ActiveVariables>());
        if let Some(mut st) = world.get_mut::<NumberFieldState>(entity) {
            st.error = parsed.is_none();
            if parsed.is_none() {
                st.text = text.clone();
            }
        }
        let Some((v, expr)) = parsed else { return };
        match n {
            Sm9Num::Var(i) => {
                let mut p = world.resource_mut::<FormPicker>();
                if let Some(x) = p.values.get_mut(i) {
                    x.value = v;
                    x.expr = expr.clone();
                }
                if let Some(mut st) = world.get_mut::<NumberFieldState>(entity) {
                    st.text = expr;
                }
                return;
            }
            Sm9Num::Chordal => crate::applied::change_kind(world, "Chordal tolerance", |k| {
                if let FeatureKind::SheetMetalLoft(x) = k {
                    x.chordal_tolerance = v;
                    x.chordal_tolerance_expr = expr;
                }
            }),
            Sm9Num::Model(m) => crate::applied::change_kind(world, crate::sheetmetal_ui::number_label(m), |k| {
                if let FeatureKind::SheetMetalLoft(x) = k {
                    let mut tmp = as_model(x);
                    crate::sheetmetal_ui::set_number(&mut tmp, m, v, expr);
                    x.params = tmp.params;
                    x.exprs = tmp.exprs;
                }
            }),
        }
        if enter {
            world.resource_mut::<InputFocus>().clear();
            crate::applied::accept(world);
        }
    });
}

fn on_button(a: On<Activate>, q: Query<&Sm9Role>, mut commands: Commands) {
    let Ok(role) = q.get(a.entity).copied() else { return };
    match role {
        Sm9Role::ThicknessFlip => change(&mut commands, "Flip thickness direction", |k| {
            if let FeatureKind::SheetMetalLoft(x) = k {
                x.flip_thickness = !x.flip_thickness;
            }
        }),
        Sm9Role::FormFlip => change(&mut commands, "Opposite direction", |k| {
            if let FeatureKind::Form(x) = k {
                x.flip = !x.flip;
            }
        }),
        Sm9Role::FormConnectorIcon => set_field(&mut commands, Sm9Field::FormLocations),
        Sm9Role::AddConnection => change(&mut commands, "Add connection", |k| {
            if let FeatureKind::SheetMetalLoft(x) = k {
                let last = x.connections.last().copied().unwrap_or(LoftConnection { t1: 0.0, t2: 0.0, rip: false });
                x.connections.push(LoftConnection { t1: (last.t1 + 0.25).fract(), t2: (last.t2 + 0.25).fract(), rip: false });
            }
        }),
        Sm9Role::FormStudio => commands.queue(open_picker),
        Sm9Role::PickerDone => commands.queue(picker_done),
        _ => {}
    }
}

fn on_list_remove(ev: On<SelectionListRemove>, q: Query<&Sm9Role>, mut commands: Commands) {
    let i = ev.index;
    let Ok(Sm9Role::List(f)) = q.get(ev.entity).copied() else { return };
    change(&mut commands, "Remove selection", move |k| {
        fn at<T>(v: &mut Vec<T>, i: usize) {
            if i < v.len() {
                v.remove(i);
            }
        }
        match (k, f) {
            (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftProfile1) => at(&mut x.profile1, i),
            (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftProfile2) => at(&mut x.profile2, i),
            (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftScope) => at(&mut x.merge_scope, i),
            (FeatureKind::SheetMetalLoft(x), Sm9Field::LoftConnection(c)) => {
                at(&mut x.connections, c);
                if x.connections.is_empty() {
                    x.connections_on = false;
                }
            }
            (FeatureKind::Form(x), Sm9Field::FormLocations) => at(&mut x.locations, i),
            (FeatureKind::Form(x), Sm9Field::FormTargets) => at(&mut x.targets, i),
            (FeatureKind::TagForm(x), Sm9Field::TagAdd) => at(&mut x.add, i),
            (FeatureKind::TagForm(x), Sm9Field::TagRemove) => at(&mut x.remove, i),
            (FeatureKind::TagForm(x), Sm9Field::TagSketch) => x.sketch = None,
            (FeatureKind::TagForm(x), Sm9Field::TagOrigin) => x.origin = None,
            _ => {}
        }
    });
}

fn on_list_activate(ev: On<SelectionListActivate>, q: Query<&Sm9Role>, mut commands: Commands) {
    if let Ok(Sm9Role::List(f)) = q.get(ev.entity).copied() {
        set_field(&mut commands, f);
    }
}

/// A loft section opened or closed: kept in the session (its slots 1–3) while the dialog is open.
fn on_section(ev: On<CollapsibleToggled>, q: Query<&Name>, session: Option<ResMut<AppliedSession>>) {
    let Ok(name) = q.get(ev.entity) else { return };
    if let (Some(i), Some(mut s)) = (SECTIONS.iter().position(|(n, _)| *n == name.as_str()), session)
        && s.sections[i + 1] != ev.open
    {
        s.sections[i + 1] = ev.open;
    }
}

// ---------------------------------------------------------------------------------------------
// The Select Part Studio panel

/// A form Part Studio of a document.
#[derive(Debug, Clone)]
struct DocForm {
    document: Option<(cadrs_core::DocumentId, String)>,
    element: cadrs_core::ElementId,
    name: String,
    studio: Vec<Feature>,
}

/// The Select Part Studio panel's state.
#[derive(Resource, Debug, Default)]
pub struct FormPicker {
    pub open: bool,
    /// Current document, Other documents, Libraries.
    pub tab: usize,
    pub kind: usize,
    pub form: usize,
    pub document: usize,
    pub values: Vec<FormVariable>,
    current: Vec<DocForm>,
    others: Vec<DocForm>,
    spawned: Option<(Entity, String)>,
}

/// The forms of a document's Part Studios (those with a Tag (Form)).
fn doc_forms(doc: &cadrs_core::Document, other: bool) -> Vec<DocForm> {
    doc.elements
        .iter()
        .filter(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))
        .filter(|e| tag_of(e.features()).is_some())
        .map(|e| DocForm { document: other.then(|| (doc.id, doc.name.clone())), element: e.id, name: e.name.clone(), studio: e.features().to_vec() })
        .collect()
}

fn library_forms(kind: usize) -> Vec<LibraryForm> {
    let kinds = library_kinds();
    let k = kinds.get(kind).copied().unwrap_or("Cut forms");
    LibraryForm::ALL.into_iter().filter(|f| f.kind() == k).collect()
}

fn library_kinds() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for f in LibraryForm::ALL {
        if !v.contains(&f.kind()) {
            v.push(f.kind());
        }
    }
    v
}

fn open_picker(world: &mut World) {
    let doc = world.get_resource::<ActiveDocument>().map(|d| d.doc.clone());
    let current = doc.as_ref().map(|d| doc_forms(d, false)).unwrap_or_default();
    let mut others = Vec::new();
    if let Some(store) = world.get_resource::<crate::DocumentStore>() {
        let (lib, _) = store.0.list();
        for entry in lib.entries.iter().filter(|e| e.meta.trashed.is_none()) {
            if doc.as_ref().is_some_and(|d| d.id == entry.id) {
                continue;
            }
            if let Ok(f) = store.0.load(entry.id) {
                others.extend(doc_forms(&f.document, true));
            }
        }
    }
    // The feature's pick, if any, is where the panel opens.
    let pick = crate::applied::current(world).and_then(|f| match f.kind {
        FeatureKind::Form(x) => x.form.map(|p| (p, x.variables)),
        _ => None,
    });
    let mut p = world.resource_mut::<FormPicker>();
    p.open = true;
    p.current = current;
    p.others = others;
    p.tab = 2;
    p.kind = 0;
    p.form = 0;
    p.document = 0;
    p.values.clear();
    if let Some((pick, vars)) = pick {
        match pick.source {
            FormSource::Library(f) => {
                p.tab = 2;
                p.kind = library_kinds().iter().position(|k| *k == f.kind()).unwrap_or(0);
                p.form = library_forms(p.kind).iter().position(|g| *g == f).unwrap_or(0);
            }
            FormSource::Current { element } => {
                p.tab = 0;
                p.form = p.current.iter().position(|d| d.element == element).unwrap_or(0);
            }
            FormSource::Other { element, .. } => {
                p.tab = 1;
                p.form = p.others.iter().position(|d| d.element == element).unwrap_or(0);
            }
        }
        p.values = vars;
    }
    if p.values.is_empty() {
        refresh_picker_values(world);
    }
}

/// The variables of the form the panel shows, at their defaults.
fn refresh_picker_values(world: &mut World) {
    let mut p = world.resource_mut::<FormPicker>();
    p.values = match p.tab {
        2 => library_forms(p.kind).get(p.form).map(|f| f.variables()).unwrap_or_default(),
        0 => p.current.get(p.form).map(|d| studio_variables(&d.studio)).unwrap_or_default(),
        _ => p.others.get(p.form).map(|d| studio_variables(&d.studio)).unwrap_or_default(),
    };
}

fn picker_done(world: &mut World) {
    let p = world.resource::<FormPicker>();
    let (pick, values) = match p.tab {
        2 => {
            let Some(f) = library_forms(p.kind).get(p.form).copied() else { return };
            (FormPick { source: FormSource::Library(f), name: f.label().into(), document_name: LIBRARY_NAME.into(), studio: Vec::new() }, p.values.clone())
        }
        0 => {
            let Some(d) = p.current.get(p.form) else { return };
            let doc_name = world.get_resource::<ActiveDocument>().map(|x| x.doc.name.clone()).unwrap_or_default();
            (FormPick { source: FormSource::Current { element: d.element }, name: d.name.clone(), document_name: doc_name, studio: d.studio.clone() }, p.values.clone())
        }
        _ => {
            let Some(d) = p.others.get(p.form) else { return };
            let Some((doc, dn)) = d.document.clone() else { return };
            (FormPick { source: FormSource::Other { document: doc, element: d.element }, name: d.name.clone(), document_name: dn, studio: d.studio.clone() }, p.values.clone())
        }
    };
    world.resource_mut::<FormPicker>().open = false;
    crate::applied::change_kind(world, "Select Part Studio", |k| {
        if let FeatureKind::Form(x) = k {
            x.form = Some(pick);
            x.variables = values;
        }
    });
    if let Some(mut s) = world.get_resource_mut::<AppliedSession>() {
        s.field = AppliedField::Sm9(Sm9Field::FormLocations);
    }
}

fn on_picker_close(ev: On<FloatingPanelClose>, q: Query<&Name>, mut p: ResMut<FormPicker>) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "form-picker") {
        p.open = false;
    }
}

/// Spawns, rebuilds and removes the panel.
fn sync_picker(world: &mut World) {
    let in_form = world.get_resource::<AppliedSession>().is_some_and(|s| s.kind == AppliedKind::Sm9(Sm9Kind::Form));
    if !in_form && world.resource::<FormPicker>().open {
        world.resource_mut::<FormPicker>().open = false;
    }
    let p = world.resource::<FormPicker>();
    let key = format!("{} {} {} {} {} {:?}", p.open, p.tab, p.kind, p.form, p.document, p.values.iter().map(|v| v.expr.clone()).collect::<Vec<_>>());
    let spawned = p.spawned.clone();
    if !p.open {
        if let Some((e, _)) = spawned {
            if let Ok(em) = world.get_entity_mut(e) {
                em.despawn();
            }
            world.resource_mut::<FormPicker>().spawned = None;
        }
        return;
    }
    if spawned.as_ref().is_some_and(|(e, k)| *k == key && world.get_entity(*e).is_ok()) {
        return;
    }
    if let Some((e, _)) = spawned
        && let Ok(em) = world.get_entity_mut(e)
    {
        em.despawn();
    }
    let mut q = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q.iter(world).next() else { return };
    let theme = world.resource::<Theme>().clone();
    let p = world.resource::<FormPicker>();
    let tab = p.tab;
    let values = p.values.clone();
    let (rows, empty): (Vec<(&str, Sm9Role, Vec<String>, usize)>, Option<&str>) = match tab {
        2 => (
            vec![
                ("Library", Sm9Role::PickerDocument, vec![LIBRARY_NAME.to_string()], 0),
                ("Type", Sm9Role::PickerType, library_kinds().iter().map(|k| k.to_string()).collect(), p.kind),
                ("Form", Sm9Role::PickerForm, library_forms(p.kind).iter().map(|f| f.label().to_string()).collect(), p.form),
            ],
            None,
        ),
        0 if p.current.is_empty() => (Vec::new(), Some("No Part Studio of this document has a Tag (Form)")),
        0 => (vec![("Form", Sm9Role::PickerForm, p.current.iter().map(|d| d.name.clone()).collect(), p.form)], None),
        _ if p.others.is_empty() => (Vec::new(), Some("No other document has a form Part Studio")),
        _ => (
            vec![(
                "Form",
                Sm9Role::PickerForm,
                p.others.iter().map(|d| format!("{} — {}", d.document.as_ref().map_or("", |x| x.1.as_str()), d.name)).collect(),
                p.form,
            )],
            None,
        ),
    };
    let rows: Vec<(String, Sm9Role, Vec<String>, usize)> = rows.into_iter().map(|(a, b, c, d)| (a.to_string(), b, c, d)).collect();
    let empty = empty.map(str::to_string);
    let t = theme.clone();
    let tf = theme.clone();
    let left = world.query_filtered::<&ComputedNode, With<cadrs_ui::FeatureDialogState>>().iter(world).next().map_or(230.0, |n| n.size().x * n.inverse_scale_factor() + 6.0);
    let panel = FloatingPanel::new("form-picker", "Select Part Studio")
        .width(300.0)
        .at(left, 2.0)
        .body(move |b| {
            let t = &t;
            b.spawn((Sm9Role::PickerTab, TabStrip::new("form-picker-source").tab("Current document").tab("Other documents").tab("Libraries").selected(tab).build(t)));
            b.spawn(Node { padding: UiRect::all(Val::Px(6.0)), ..default() }).with_children(|r| {
                r.spawn(cadrs_ui::TextInput::new("form-picker-search").placeholder("Search by name, folder, or configuration").build(t));
            });
            if let Some(e) = &empty {
                b.spawn((t.text(e.clone(), 11.0, bevy::text::FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::all(Val::Px(8.0)), ..default() }));
            }
            for (label, role, opts, sel) in &rows {
                let name = format!("form-picker-{}", label.to_lowercase());
                select(b, t, &name, label, *role, opts, *sel);
            }
            if !values.is_empty() {
                b.spawn((Node { height: Val::Px(3.0), margin: UiRect::vertical(Val::Px(4.0)), ..default() }, BackgroundColor(t.border)));
                b.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(20.0), Val::Px(6.0), Val::ZERO, Val::Px(6.0)), ..default() }).with_children(|c| {
                    for (i, v) in values.iter().enumerate() {
                        number(c, t, &format!("form-picker-var-{}", v.name.to_lowercase()), &v.name, Sm9Num::Var(i), &v.expr, None);
                    }
                });
            }
        })
        .footer(move |f| {
            f.spawn((Sm9Role::PickerDone, Button::new("form-picker-done").label("Done").outline().build(&tf)));
        })
        .build(&theme);
    let e = world.spawn(panel).id();
    world.entity_mut(area).add_child(e);
    world.resource_mut::<FormPicker>().spawned = Some((e, key));
}

// ---------------------------------------------------------------------------------------------
// The connections in the view

/// The loft's profiles and connections as its rebuild sends them (the feature's `arrows`, see
/// `cadrs_core`'s `encode_guides`).
#[derive(Debug, Clone, Default)]
pub struct Guides {
    pub matched: Vec<(Vec3, Vec3)>,
    pub p1: Vec<Vec3>,
    pub p2: Vec<Vec3>,
    pub closed: (bool, bool),
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32)
}

impl Guides {
    pub fn decode(a: &[([f64; 3], [f64; 3])]) -> Option<Guides> {
        let (head, flags) = *a.first()?;
        let (nc, n1, n2) = (head[0] as usize, head[1] as usize, head[2] as usize);
        if a.len() != 1 + nc + n1 + n2 {
            return None;
        }
        let matched = a[1..1 + nc].iter().map(|(p, q)| (v3(*p), v3(*q))).collect();
        let p1 = a[1 + nc..1 + nc + n1].iter().map(|(p, _)| v3(*p)).collect();
        let p2 = a[1 + nc + n1..].iter().map(|(p, _)| v3(*p)).collect();
        Some(Guides { matched, p1, p2, closed: (flags[0] > 0.5, flags[1] > 0.5) })
    }

    fn segs(pts: &[Vec3], closed: bool) -> Vec<(Vec3, Vec3)> {
        let n = pts.len();
        if n < 2 {
            return Vec::new();
        }
        let m = if closed { n } else { n - 1 };
        (0..m).map(|i| (pts[i], pts[(i + 1) % n])).collect()
    }

    /// The point `t` along a profile.
    pub fn at(pts: &[Vec3], closed: bool, t: f64) -> Vec3 {
        let segs = Self::segs(pts, closed);
        if segs.is_empty() {
            return pts.first().copied().unwrap_or(Vec3::ZERO);
        }
        let total: f32 = segs.iter().map(|(a, b)| a.distance(*b)).sum();
        let mut want = (t.clamp(0.0, 1.0) as f32) * total;
        for (i, (a, b)) in segs.iter().enumerate() {
            let l = a.distance(*b);
            if want <= l || i + 1 == segs.len() {
                return a.lerp(*b, (want / l.max(1e-12)).min(1.0));
            }
            want -= l;
        }
        segs[0].0
    }

    /// The parameter of the profile point nearest `p`, and how far it is.
    pub fn nearest_t(&self, pts: &[Vec3], closed: bool, p: Vec3) -> (f64, f32) {
        self.nearest_by(pts, closed, |q| q.distance(p))
    }

    fn nearest_by(&self, pts: &[Vec3], closed: bool, dist: impl Fn(Vec3) -> f32) -> (f64, f32) {
        let segs = Self::segs(pts, closed);
        let total: f32 = segs.iter().map(|(a, b)| a.distance(*b)).sum::<f32>().max(1e-12);
        let (mut best, mut best_t, mut run) = (f32::MAX, 0.0f32, 0.0f32);
        for (a, b) in &segs {
            let l = a.distance(*b);
            for k in 0..=16 {
                let s = k as f32 / 16.0;
                let d = dist(a.lerp(*b, s));
                if d < best {
                    best = d;
                    best_t = (run + l * s) / total;
                }
            }
            run += l;
        }
        (f64::from(best_t), best)
    }

    /// The loft's matched start as a connection.
    pub fn start_connection(&self) -> Option<LoftConnection> {
        let (a, b) = *self.matched.first()?;
        Some(LoftConnection { t1: self.nearest_t(&self.p1, self.closed.0, a).0, t2: self.nearest_t(&self.p2, self.closed.1, b).0, rip: false })
    }
}

/// A connection end being dragged: (connection, profile 1 or 2, its parameter).
#[derive(Resource, Debug, Default)]
pub struct ConnDrag(Option<(usize, u8, f64)>);

/// A connection's handle in the view.
#[derive(Component, Debug, Clone, Copy)]
struct ConnHandle {
    conn: usize,
    side: u8,
}

fn loft_state(world: &World) -> Option<(SheetMetalLoftFeature, Guides)> {
    let s = world.get_resource::<AppliedSession>()?;
    let f = crate::applied::current(world)?;
    let FeatureKind::SheetMetalLoft(x) = f.kind else { return None };
    let g = Guides::decode(world.resource::<PartCache>().arrows.get(&s.feature)?)?;
    Some((x, g))
}

/// Where each handle goes (screen px), with the drag's parameter while one is dragged.
fn handle_points(x: &SheetMetalLoftFeature, g: &Guides, drag: &ConnDrag) -> Vec<(usize, u8, Vec3)> {
    let mut out = Vec::new();
    if !x.connections_on {
        return out;
    }
    for (i, c) in x.connections.iter().enumerate() {
        for side in [1u8, 2] {
            let mut t = if side == 1 { c.t1 } else { c.t2 };
            if let Some((di, ds, dt)) = drag.0
                && di == i
                && ds == side
            {
                t = dt;
            }
            let (pts, closed) = if side == 1 { (&g.p1, g.closed.0) } else { (&g.p2, g.closed.1) };
            out.push((i, side, Guides::at(pts, closed, t)));
        }
    }
    out
}

fn place_handles(world: &mut World) {
    let state = loft_state(world);
    let want: Vec<(usize, u8, Vec3)> = state.as_ref().map(|(x, g)| handle_points(x, g, world.resource::<ConnDrag>())).unwrap_or_default();
    let mut q = world.query::<(Entity, &ConnHandle)>();
    let have: Vec<(Entity, ConnHandle)> = q.iter(world).map(|(e, h)| (e, *h)).collect();
    if have.len() != want.len() {
        for (e, _) in &have {
            if let Ok(em) = world.get_entity_mut(*e) {
                em.despawn();
            }
        }
        let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
        let Some(area) = qa.iter(world).next() else { return };
        let theme = world.resource::<Theme>().clone();
        for (i, side, _) in &want {
            let e = world
                .spawn((
                    Name::new(format!("sm-loft-connection-{i}-handle-{side}")),
                    ConnHandle { conn: *i, side: *side },
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(14.0),
                        height: Val::Px(14.0),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(7.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0x55, 0x55, 0x55)),
                    BackgroundColor(Color::WHITE.with_alpha(0.85)),
                    Tooltip::new("Drag along the profile"),
                    GlobalZIndex(5),
                ))
                .observe(on_handle_drag)
                .observe(on_handle_drag_end)
                .id();
            world.entity_mut(area).add_child(e);
            let _ = &theme;
        }
        return;
    }
    let view = world.resource::<crate::viewport::ViewportView>().view;
    let rect = *world.resource::<crate::viewport::ViewportRect>();
    let area_min = rect.0.min;
    for (e, h) in have {
        let Some((_, _, p)) = want.iter().find(|(i, s, _)| *i == h.conn && *s == h.side) else { continue };
        let at = rect.to_screen(view.project(*p)) - area_min;
        if let Some(mut n) = world.get_mut::<Node>(e) {
            let (l, t) = (Val::Px(at.x - 7.0), Val::Px(at.y - 7.0));
            if n.left != l || n.top != t {
                n.left = l;
                n.top = t;
            }
        }
    }
}

fn on_handle_drag(ev: On<Pointer<Drag>>, q: Query<&ConnHandle>, mut commands: Commands) {
    let Ok(h) = q.get(ev.entity).copied() else { return };
    let pos = ev.pointer_location.position;
    commands.queue(move |world: &mut World| {
        let Some((_, g)) = loft_state(world) else { return };
        let view = world.resource::<crate::viewport::ViewportView>().view;
        let rect = *world.resource::<crate::viewport::ViewportRect>();
        let (pts, closed) = if h.side == 1 { (&g.p1, g.closed.0) } else { (&g.p2, g.closed.1) };
        let (t, _) = g.nearest_by(pts, closed, |q| rect.to_screen(view.project(q)).distance(pos));
        world.resource_mut::<ConnDrag>().0 = Some((h.conn, h.side, t));
    });
}

fn on_handle_drag_end(_: On<Pointer<DragEnd>>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        let Some((i, side, t)) = world.resource_mut::<ConnDrag>().0.take() else { return };
        crate::applied::change_kind(world, "Drag connection", |k| {
            if let FeatureKind::SheetMetalLoft(x) = k
                && let Some(c) = x.connections.get_mut(i)
            {
                if side == 1 {
                    c.t1 = t;
                } else {
                    c.t2 = t;
                }
            }
        });
    });
}

fn draw_connections(world: &mut World) {
    let Some((x, g)) = loft_state(world) else { return };
    let pts = handle_points(&x, &g, world.resource::<ConnDrag>());
    let mut sys = bevy::ecs::system::SystemState::<Gizmos<crate::parts::PickedEdgeGizmos>>::new(world);
    let mut gz = sys.get_mut(world);
    let magenta = Color::srgb_u8(0xc8, 0x3c, 0xd8);
    for i in 0..x.connections.len() {
        let a = pts.iter().find(|(c, s, _)| *c == i && *s == 1).map(|p| p.2);
        let b = pts.iter().find(|(c, s, _)| *c == i && *s == 2).map(|p| p.2);
        if let (Some(a), Some(b)) = (a, b) {
            gz.line(a, b, magenta);
        }
    }
    sys.apply(world);
}

// ---------------------------------------------------------------------------------------------
// Scenario set-ups

/// `Custom("sm9 …")` set-ups (P3I.9 scenarios):
/// - `sm9 loft-profiles`: a 100 × 80 rectangle on Top (Sketch 1) and a Ø60 circle on "Plane 1",
///   60 above Top (Sketch 2);
/// - `sm9 louver-box`: a 120 × 80 × 40 block converted to sheet metal (2 mm, bends along the
///   2 mm thick, every edge a rip), "Plane 1" on its outer top face and "Sketch 2" there with
///   ten points in two columns, for louvers on its top wall;
/// - `sm9 form-library`: stores cadrs's forms library document (so Other documents lists it).
pub fn script(world: &mut World, arg: &str) {
    use cadrs_core::commands::{AddFeature, AddSketch, EditSketch};
    use cadrs_sketch::{FeaturePlane, PlaneFrame, PlaneRef, SketchOp, Vec2};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element().map(|e| e.id) else { return };
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    };
    let mut run = |doc: &mut ActiveDocument, c: &dyn cadrs_core::Command| {
        if let Err(e) = doc.execute(c) {
            warn!("sm9 set-up: {e}");
        }
    };
    match arg.trim() {
        "loft-profiles" => {
            let a = FeatureId::new();
            run(&mut doc, &AddSketch { element: el, feature: a, plane: Some(PlaneRef::Top) });
            run(&mut doc, &EditSketch { element: el, feature: a, op: rect(-50.0, -40.0, 50.0, 40.0) });
            let pf = FeatureId::new();
            let plane = cadrs_core::plane::PlaneFeature {
                entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Top)],
                offset: 60.0,
                offset_expr: "60 mm".into(),
                ..Default::default()
            };
            run(&mut doc, &AddFeature { element: el, feature: pf, base_name: "Plane".into(), kind: FeatureKind::Plane(plane) });
            let b = FeatureId::new();
            let frame = PlaneFrame { origin: [0.0, 0.0, 60.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
            run(&mut doc, &AddSketch { element: el, feature: b, plane: Some(PlaneRef::Feature(FeaturePlane::new(pf.0, frame))) });
            run(&mut doc, &EditSketch { element: el, feature: b, op: SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 30.0, construction: false } });
        }
        "louver-box" => {
            let s = FeatureId::new();
            run(&mut doc, &AddSketch { element: el, feature: s, plane: Some(PlaneRef::Top) });
            run(&mut doc, &EditSketch { element: el, feature: s, op: rect(0.0, 0.0, 120.0, 80.0) });
            let g = doc.active_element().and_then(|e| e.feature(s)).and_then(|f| f.sketch()).map(|k| k.geometry.clone());
            let Some(g) = g else { return };
            let regions = cadrs_core::samples::region_refs(s, &g, &[Vec2::new(60.0, 40.0)]);
            let e = FeatureId::new();
            run(&mut doc, &cadrs_core::commands::AddExtrude { element: el, feature: e, extrude: Default::default() });
            run(&mut doc, &cadrs_core::commands::SetExtrude { element: el, feature: e, extrude: cadrs_core::samples::extrude_of(regions, 40.0), label: "Extrude".into() });
            let mut sm = SheetMetalModelFeature { parts: vec![PartId::new(e, 0)], ..Default::default() };
            sm.params.thickness = 2.0;
            sm.params.bend_radius = 2.0;
            sm.exprs = SheetMetalExprs::of(&sm.params);
            run(&mut doc, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(sm) });
            // The louvers' points on "Plane 1", the box's outer top face (42 up).
            let pf = FeatureId::new();
            let plane = cadrs_core::plane::PlaneFeature {
                entities: vec![cadrs_core::plane::PlaneEntity::Plane(PlaneRef::Top)],
                offset: 42.0,
                offset_expr: "42 mm".into(),
                ..Default::default()
            };
            run(&mut doc, &AddFeature { element: el, feature: pf, base_name: "Plane".into(), kind: FeatureKind::Plane(plane) });
            let p = FeatureId::new();
            let frame = PlaneFrame { origin: [0.0, 0.0, 42.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] };
            run(&mut doc, &AddSketch { element: el, feature: p, plane: Some(PlaneRef::Feature(FeaturePlane::new(pf.0, frame))) });
            for x in [35.0, 85.0] {
                for y in [16.0, 28.0, 40.0, 52.0, 64.0] {
                    run(&mut doc, &EditSketch { element: el, feature: p, op: SketchOp::AddPoint { pos: Vec2::new(x, y) } });
                }
            }
        }
        "form-library" => {
            drop(doc);
            let Ok(lib) = cadrs_core::samples::sheetmetal_forms::document() else { return };
            if let Some(store) = world.get_resource::<crate::DocumentStore>() {
                let meta = cadrs_core::library::DocumentMeta::new("cadrs", 1_790_553_600);
                if let Err(e) = store.0.save(&lib, &meta) {
                    warn!("sm9 form-library: {e}");
                }
            }
        }
        other => warn!("sm9: unknown set-up {other:?}"),
    }
}

/// The model settings' expressions for display (keeps `plain` in use for scalar fields).
pub fn scalar(v: f64) -> String {
    plain(v)
}
