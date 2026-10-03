//! The applied features (P3.6, PS14–PS16): **Fillet**, **Chamfer**, **Shell** and **Hole**, like
//! Onshape's (`training/intro-to-part-studios/ex3-step3.png`, `ex3-step5.png`, `ex3-step7.png`,
//! `ex3-step8.png`, `ex3-step9.png`, `lesson-fillet-and-chamfer.png`).
//!
//! - The toolbar's Fillet, Chamfer, Shell and Hole buttons insert "Fillet N" (…) and open its
//!   dialog ([`crate::applied_dialog`]); edges, faces, parts or sketch points selected before
//!   go into its first field. Double-clicking the feature in the list edits it.
//! - One selection field at a time takes the viewport's picks: edges and faces ("Edge of
//!   Extrude 1", "Face of Extrude 1"; a face fillets all its edges), the faces to remove or the
//!   parts to hollow, the hole's sketch points ("Vertex of Sketch 4", or a whole sketch picked in
//!   the feature list) and its merge scope.
//! - The result shows while the dialog is open; the picked edges and faces are drawn in amber
//!   where they were before the feature (a filleted edge is gone from the result), with their
//!   tangent chains when tangent propagation is on.
//! - ✓ or Enter accepts (one undo step, "Insert Fillet 1"), ✕ or Esc removes a new feature or
//!   reverts an edit. Every change is a command.
//! - The fillet's radius (or width) has an arrow manipulator on the first edge: dragging it
//!   changes the value live (PS14.5).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::applied::{
    ChamferFeature, EdgeOrFace, FilletFeature, FilletType, HoleFeature, HolePoint, ShellFeature,
};
use cadrs_core::commands::{AddFeature, ReplaceFeature, SetFeature};
use cadrs_core::document::{EdgeRef, FaceRef};
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind, Part, PartId};

use crate::parts::{PartCache, PickFilter};
use crate::viewport::{Pick, PickRequest, Selection};
use crate::{ActiveDocument, AppState};

pub struct AppliedPlugin;

impl Plugin for AppliedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BeforeParts>()
            .init_resource::<RadiusArrow>()
            .init_resource::<crate::transform_ui::XyzArrows>()
            .add_systems(
                Update,
                (applied_picks, applied_keys, radius_arrow_pointer, crate::transform_ui::xyz_arrow_pointer, crate::applied_dialog::sync_applied_dialog, crate::draft_ui::sync_fillet_entries, crate::sheetmetal_ui::sync_numbers)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .before(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                (roll_back_for_overrides, roll_back_to_edited, crate::extrude_dialog::sync_final_buttons, update_before_parts, default_hole_scope, show_references, draw_references, draw_sketch_point_picks)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                PostUpdate,
                (place_radius_arrow, crate::transform_ui::place_xyz_arrows, crate::draft_ui::place_radius_labels)
                    .before(bevy::ui::UiSystems::Layout)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |world: &mut World| {
                if world.contains_resource::<AppliedSession>() {
                    finish(world);
                }
            })
            .add_plugins(crate::applied_dialog::AppliedDialogPlugin);
    }
}

/// Which applied feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliedKind {
    Fillet,
    Chamfer,
    Shell,
    Hole,
    /// P3.7: the Plane, Sweep, Loft and Split features (`crate::advanced`).
    Plane,
    Sweep,
    Loft,
    Split,
    /// P3.8: patterns, Mirror and the Mate connector (`crate::pattern`).
    Pattern(cadrs_core::pattern::PatternKind),
    Mirror,
    MateConnector,
    /// P3.10: the Draft feature (`crate::draft_ui`).
    Draft,
    /// The Transform feature (`crate::transform_ui`).
    Transform,
    /// The surfacing features (`crate::surfacing_ui`).
    Thicken,
    Helix,
    Fill,
    /// P3I.2: the Sheet metal model (`crate::sheetmetal_ui`).
    SheetMetal,
    /// P3I.9: Sheet metal loft, Form, Tag (`crate::sheetmetal_p3i9_ui`).
    Sm9(crate::sheetmetal_p3i9_ui::Sm9Kind),
    /// P3I.4: Flange, Hem, Make joint (`crate::sheetmetal_features_ui`).
    SmFeature(crate::sheetmetal_features_ui::SmTool),
    /// P3I.5: the sheet metal features after it (`crate::sheetmetal_tools_ui`).
    SheetMetalTool(crate::sheetmetal_tools_ui::SmTool),
    /// P3I.6: the flat pattern extrude (`crate::flat_ui`).
    FlatExtrude,
}

impl AppliedKind {
    pub fn of(kind: &FeatureKind) -> Option<Self> {
        match kind {
            FeatureKind::Fillet(_) => Some(Self::Fillet),
            FeatureKind::Chamfer(_) => Some(Self::Chamfer),
            FeatureKind::Shell(_) => Some(Self::Shell),
            FeatureKind::Hole(_) => Some(Self::Hole),
            FeatureKind::Plane(_) => Some(Self::Plane),
            FeatureKind::Sweep(_) => Some(Self::Sweep),
            FeatureKind::Loft(_) => Some(Self::Loft),
            FeatureKind::Split(_) => Some(Self::Split),
            FeatureKind::Pattern(x) => Some(Self::Pattern(x.kind)),
            FeatureKind::Mirror(_) => Some(Self::Mirror),
            FeatureKind::MateConnector(_) => Some(Self::MateConnector),
            FeatureKind::Draft(_) => Some(Self::Draft),
            FeatureKind::Transform(_) => Some(Self::Transform),
            FeatureKind::Thicken(_) => Some(Self::Thicken),
            FeatureKind::Helix(_) => Some(Self::Helix),
            FeatureKind::Fill(_) => Some(Self::Fill),
            FeatureKind::SheetMetalModel(_) => Some(Self::SheetMetal),
            k @ (FeatureKind::SheetMetalLoft(_) | FeatureKind::Form(_) | FeatureKind::TagForm(_)) => crate::sheetmetal_p3i9_ui::Sm9Kind::of(k).map(Self::Sm9),
            FeatureKind::SheetMetal(x) => Some(Self::SmFeature(crate::sheetmetal_features_ui::SmTool::of(x))),
            FeatureKind::SheetMetalTool(x) => Some(Self::SheetMetalTool(crate::sheetmetal_tools_ui::SmTool::of(x))),
            FeatureKind::FlatExtrude(_) => Some(Self::FlatExtrude),
            _ => None,
        }
    }
}

/// The selection field that takes the viewport's picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliedField {
    /// Entities to fillet or chamfer.
    Entities,
    /// A chamfer's direction overrides.
    Overrides,
    /// The shell's faces to remove (or parts to hollow).
    Faces,
    /// The hole's sketch points.
    Points,
    /// The hole's merge scope.
    MergeScope,
    /// A full round's first side face, center face and second side face (PS14.1).
    Side1,
    Center,
    Side2,
    /// P3.7: a Plane's entities.
    PlaneEntities,
    /// A Sweep's faces and sketch regions, its path, and the direction Lock profile direction
    /// keeps.
    Profile,
    Path,
    LockDirection,
    /// A Loft's profiles.
    Profiles,
    /// P3.11: a Loft's Normal direction / Tangent direction conditions' directions.
    LoftStartDirection,
    LoftEndDirection,
    /// A Split's parts and what splits them.
    SplitParts,
    SplitTool,
    /// P3.8: what a pattern or mirror copies (parts, features or faces); a linear pattern's
    /// directions; a circular pattern's axis; a curve pattern's path; the instances skipped; a
    /// mirror's plane; a mate connector's origin; a hole's mate connectors.
    PatternEntities,
    PatternDirection,
    PatternDirection2,
    PatternAxis,
    PatternPath,
    SkipList,
    MirrorPlane,
    ConnectorOrigin,
    /// P3.11: a Mate connector's Alignment and Owner part.
    ConnectorAlignment,
    ConnectorOwner,
    HoleConnectors,
    /// P3B.7 (A22.4–A22.6): a connector's Between entity, its Realign primary and secondary
    /// axes (its Owner entity is `ConnectorOwner` above).
    ConnectorBetween,
    ConnectorPrimary,
    ConnectorSecondary,
    /// P3.10: the Draft's neutral plane and faces to draft; the hole's start plane and its Up
    /// to entity; a variable fillet's vertices and points on edges; the Boolean's faces to
    /// offset.
    DraftNeutral,
    DraftFaces,
    HoleStartPlane,
    HoleUpTo,
    FilletVertices,
    FilletEdgePoints,
    /// The Transform's parts, its line, direction, mate connectors (from and to), axis and the
    /// point it scales about.
    TransformParts,
    TransformLine,
    TransformDirection,
    TransformFrom,
    TransformTo,
    TransformAxis,
    TransformScalePoint,
    /// The surfacing features: a Thicken's faces and surfaces, a Helix's face, axis or circle,
    /// a Fill's boundary.
    ThickenEntities,
    HelixEntity,
    FillEdges,
    /// P3I.2: the Sheet metal model's fields: Convert's parts, faces to exclude and edges or
    /// cylinders to bend (Thicken's too); Extrude's curves, arcs to extrude as bends and the Up
    /// to entities of its two ends; Thicken's faces and regions.
    SmParts,
    SmExclude,
    SmBends,
    SmCurves,
    SmArcs,
    SmFaces,
    SmUpTo,
    SmSecondUpTo,
    /// P3I.9's fields (`crate::sheetmetal_p3i9_ui`).
    Sm9(crate::sheetmetal_p3i9_ui::Sm9Field),
    /// P3I.4: the Flange, Hem and Make joint fields.
    Smf(crate::sheetmetal_features_ui::SmfField),
    /// P3I.5: a field of the sheet metal features after the model (`crate::sheetmetal_tools_ui`
    /// numbers them).
    SmTool(u8),
    /// P3I.6: the flat pattern extrude's regions.
    FlatRegions,
}

/// The applied feature whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct AppliedSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub kind: AppliedKind,
    pub is_new: bool,
    pub mark: usize,
    pub before: Option<Feature>,
    pub field: AppliedField,
    /// The dialog's Final button: show the features after this one too (PS21.11).
    pub show_final: bool,
    /// P3.10 (P3.8 judge): a new hole's Merge scope is still to be filled with the part its
    /// first point drills.
    pub scope_auto: bool,
    /// P3I.2: which of the Sheet metal model dialog's sections are open (Selections, General,
    /// Material, Relief).
    pub sections: [bool; 4],
}

impl AppliedSession {
    /// What a click can pick for the active field.
    pub fn pick_filter(&self) -> PickFilter {
        let none = PickFilter {
            origin: false,
            planes: [false; 3],
            faces: false,
            planar_only: false,
            edges: false,
            regions: false,
            sketch_curves: false,
            sketch_points: false,
            plane_features: false,
            connectors: false,
            // A pattern's Skip dots are clicked in any field (PS22.5).
            dots: matches!(self.kind, AppliedKind::Pattern(_)).then_some(self.feature),
            // The feature's own faces (a fillet's round) can't be picked for it.
            skip_op: Some(self.feature.0),
        };
        let planes = [true; 3];
        match self.field {
            AppliedField::Entities => PickFilter { faces: true, edges: true, ..none },
            AppliedField::Overrides => PickFilter { edges: true, ..none },
            AppliedField::Faces | AppliedField::MergeScope => PickFilter { faces: true, edges: true, ..none },
            AppliedField::Points => PickFilter { sketch_points: true, ..none },
            AppliedField::Side1 | AppliedField::Center | AppliedField::Side2 => PickFilter { faces: true, ..none },
            AppliedField::PlaneEntities => PickFilter {
                origin: true,
                planes,
                plane_features: true,
                faces: true,
                edges: true,
                sketch_curves: true,
                sketch_points: true,
                // The plane can't be built on itself.
                skip_op: None,
                ..none
            },
            AppliedField::Profile => PickFilter { faces: true, planar_only: true, regions: true, ..none },
            AppliedField::Path => PickFilter { edges: true, sketch_curves: true, ..none },
            AppliedField::LockDirection => PickFilter { planes, plane_features: true, faces: true, planar_only: true, edges: true, ..none },
            AppliedField::LoftStartDirection | AppliedField::LoftEndDirection => {
                PickFilter { planes, plane_features: true, faces: true, planar_only: true, edges: true, sketch_curves: true, connectors: true, ..none }
            }
            // P3.10 (PS20.1): non-planar faces are profiles too.
            AppliedField::Profiles => PickFilter { faces: true, regions: true, sketch_points: true, edges: true, ..none },
            AppliedField::SplitParts => PickFilter { faces: true, edges: true, ..none },
            AppliedField::SplitTool => PickFilter { planes, plane_features: true, faces: true, ..none },
            // P3.8. Seeds: parts (any face or edge), features (the faces they made) or faces;
            // the faces and edges a pattern made itself are fine to pick for its seeds too.
            AppliedField::PatternEntities => PickFilter { faces: true, edges: true, skip_op: None, ..none },
            AppliedField::PatternDirection | AppliedField::PatternDirection2 => PickFilter {
                planes,
                plane_features: true,
                faces: true,
                planar_only: true,
                edges: true,
                sketch_curves: true,
                connectors: true,
                ..none
            },
            AppliedField::PatternAxis => PickFilter { origin: true, faces: true, edges: true, sketch_curves: true, connectors: true, ..none },
            AppliedField::PatternPath => PickFilter { edges: true, sketch_curves: true, ..none },
            AppliedField::SkipList => none,
            AppliedField::MirrorPlane => PickFilter { planes, plane_features: true, faces: true, planar_only: true, connectors: true, ..none },
            AppliedField::ConnectorOrigin | AppliedField::ConnectorBetween => PickFilter {
                origin: true,
                faces: true,
                edges: true,
                sketch_points: true,
                sketch_curves: true,
                skip_op: None,
                ..none
            },
            AppliedField::ConnectorPrimary | AppliedField::ConnectorSecondary => {
                PickFilter { faces: true, edges: true, sketch_curves: true, skip_op: None, ..none }
            }
            AppliedField::ConnectorOwner => PickFilter { faces: true, edges: true, skip_op: None, ..none },
            AppliedField::HoleConnectors => PickFilter { origin: true, faces: true, edges: true, connectors: true, ..none },
            AppliedField::ConnectorAlignment => {
                PickFilter { planes, plane_features: true, faces: true, planar_only: true, edges: true, sketch_curves: true, connectors: true, ..none }
            }
            AppliedField::DraftNeutral | AppliedField::HoleStartPlane => {
                PickFilter { planes, plane_features: true, faces: true, planar_only: true, connectors: true, ..none }
            }
            AppliedField::HoleUpTo => PickFilter { planes, plane_features: true, faces: true, connectors: true, ..none },
            AppliedField::DraftFaces => PickFilter { faces: true, ..none },
            AppliedField::FilletVertices => PickFilter { edges: true, ..none },
            AppliedField::FilletEdgePoints => PickFilter { edges: true, ..none },
            // The parts' faces and edges pick the parts (the Transform keeps their names, so its
            // own op never shows on them).
            AppliedField::TransformParts => PickFilter { faces: true, edges: true, skip_op: None, ..none },
            AppliedField::TransformLine => PickFilter { edges: true, sketch_curves: true, ..none },
            AppliedField::TransformDirection => PickFilter {
                planes,
                plane_features: true,
                faces: true,
                planar_only: true,
                edges: true,
                sketch_curves: true,
                connectors: true,
                ..none
            },
            AppliedField::TransformFrom | AppliedField::TransformTo | AppliedField::TransformScalePoint => PickFilter {
                origin: true,
                faces: true,
                edges: true,
                sketch_points: true,
                sketch_curves: true,
                connectors: true,
                ..none
            },
            AppliedField::TransformAxis => PickFilter { origin: true, faces: true, edges: true, sketch_curves: true, connectors: true, ..none },
            AppliedField::ThickenEntities => PickFilter { faces: true, regions: true, ..none },
            AppliedField::HelixEntity => PickFilter { faces: true, edges: true, sketch_curves: true, connectors: true, ..none },
            AppliedField::FillEdges => PickFilter { edges: true, sketch_curves: true, ..none },
            AppliedField::Sm9(f) => crate::sheetmetal_p3i9_ui::pick_filter(f, none),
            AppliedField::FlatRegions => PickFilter { regions: true, ..none },
            f @ AppliedField::SmTool(_) => crate::sheetmetal_tools_ui::pick_filter(f, none),
            f => crate::sheetmetal_ui::pick_filter(f, cadrs_core::document::EndType::Blind, none)
                .or_else(|| crate::sheetmetal_features_ui::pick_filter(f, none))
                .unwrap_or(none),
        }
    }
}

/// The parts before the feature being edited (its references are drawn on them).
#[derive(Resource, Debug, Default)]
pub struct BeforeParts {
    key: Option<(FeatureId, u64)>,
    pub parts: Vec<Part>,
}

/// Starts a new applied feature in the active Part Studio (the toolbar's buttons).
pub fn begin(world: &mut World, kind: AppliedKind) {
    if world.contains_resource::<AppliedSession>() {
        finish(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
    let picked: Vec<Pick> = world.resource::<Selection>().0.clone();
    let cache = world.resource::<PartCache>();
    let entities: Vec<EdgeOrFace> = picked.iter().filter_map(|p| entity_of(cache, *p)).collect();
    let faces: Vec<FaceRef> = entities
        .iter()
        .filter_map(|e| match e {
            EdgeOrFace::Face(f) => Some(*f),
            _ => None,
        })
        .collect();
    let points: Vec<HolePoint> = picked
        .iter()
        .filter_map(|p| match p {
            Pick::SketchPoint(sketch, point) => Some(HolePoint { sketch: *sketch, point: *point }),
            _ => None,
        })
        .collect();
    let sketches: Vec<FeatureId> = picked
        .iter()
        .filter_map(|p| match p {
            Pick::Feature(f) => Some(*f),
            _ => None,
        })
        .collect();
    // P3.7: the new features' first parameters from the selection (P3.8: patterns, mirror,
    // mate connector).
    let advanced = crate::advanced::initial(world, kind, &picked)
        .or_else(|| crate::pattern::initial(world, kind, &picked))
        .or_else(|| crate::surfacing_ui::initial(world, kind, &picked))
        .or_else(|| (kind == AppliedKind::SheetMetal).then(|| crate::sheetmetal_ui::initial(world, &picked)).flatten())
        .or_else(|| match kind {
            AppliedKind::Sm9(k) => crate::sheetmetal_p3i9_ui::initial(world, k, &picked),
            AppliedKind::SmFeature(t) => crate::sheetmetal_features_ui::initial(world, t, &picked),
            _ => None,
        })
        .or_else(|| crate::sheetmetal_tools_ui::initial(world, kind, &picked))
        .or_else(|| (kind == AppliedKind::FlatExtrude).then(|| crate::flat_ui::initial(world, &picked)).flatten());
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc
        .active_element()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
    else {
        return;
    };
    let sketches: Vec<FeatureId> = sketches
        .into_iter()
        .filter(|s| doc.active_element().and_then(|e| e.feature(*s)).is_some_and(|f| f.sketch().is_some()))
        .collect();
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    let (add, field) = match kind {
        AppliedKind::Fillet => (
            AddFeature::fillet(element, feature, FilletFeature { entities, ..FilletFeature::default() }),
            AppliedField::Entities,
        ),
        AppliedKind::Chamfer => (
            AddFeature::chamfer(element, feature, ChamferFeature { entities, ..ChamferFeature::default() }),
            AppliedField::Entities,
        ),
        AppliedKind::Shell => (
            AddFeature::shell(element, feature, ShellFeature { faces, ..ShellFeature::default() }),
            AppliedField::Faces,
        ),
        AppliedKind::Hole => {
            let mut h = HoleFeature { points, sketches, ..HoleFeature::default() };
            // A new hole in a document in inches starts from the inch table.
            if doc.doc.units.length == cadrs_sketch::units::LengthUnit::Inch {
                h.spec.standard = cadrs_core::hole::HoleStandard::Ansi;
                h.spec.apply_table();
            }
            (AddFeature::hole(element, feature, h), AppliedField::Points)
        }
        AppliedKind::Draft => (
            AddFeature::draft(element, feature, crate::draft_ui::initial(&picked, &faces)),
            if faces.is_empty() { AppliedField::DraftNeutral } else { AppliedField::DraftFaces },
        ),
        AppliedKind::Transform => {
            let (x, field) = crate::transform_ui::initial(&doc.doc, element, &picked);
            (AddFeature::transform(element, feature, x), field)
        }
        AppliedKind::Plane
        | AppliedKind::Sweep
        | AppliedKind::Loft
        | AppliedKind::Split
        | AppliedKind::Pattern(_)
        | AppliedKind::Mirror
        | AppliedKind::MateConnector
        | AppliedKind::Thicken
        | AppliedKind::Helix
        | AppliedKind::Fill
        | AppliedKind::SheetMetal
        | AppliedKind::Sm9(_)
        | AppliedKind::SmFeature(_)
        | AppliedKind::SheetMetalTool(_)
        | AppliedKind::FlatExtrude => {
            let Some((base, kind, field)) = advanced else { return };
            (AddFeature { element, feature, base_name: base.into(), kind }, field)
        }
    };
    if let Err(e) = doc.execute(&add) {
        warn!("cannot insert the feature: {e}");
        return;
    }
    start(world, element, feature, kind, true, mark, None, field);
}

/// Opens an existing applied feature for editing.
pub fn edit(world: &mut World, feature: FeatureId) {
    if world.get_resource::<AppliedSession>().is_some_and(|s| s.feature == feature) {
        return;
    }
    if world.contains_resource::<AppliedSession>() {
        finish(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el.feature(feature).cloned() else {
        return;
    };
    let Some(kind) = AppliedKind::of(&before.kind) else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    let field = match kind {
        AppliedKind::Fillet if before.fillet().is_some_and(|x| x.kind == FilletType::FullRound) => AppliedField::Side1,
        AppliedKind::Fillet | AppliedKind::Chamfer => AppliedField::Entities,
        AppliedKind::Shell => AppliedField::Faces,
        AppliedKind::Hole => AppliedField::Points,
        AppliedKind::Plane => AppliedField::PlaneEntities,
        AppliedKind::Sweep => AppliedField::Profile,
        AppliedKind::Loft => AppliedField::Profiles,
        AppliedKind::Split => AppliedField::SplitParts,
        AppliedKind::Pattern(_) | AppliedKind::Mirror => AppliedField::PatternEntities,
        AppliedKind::MateConnector => AppliedField::ConnectorOrigin,
        AppliedKind::Draft => AppliedField::DraftFaces,
        AppliedKind::Transform => AppliedField::TransformParts,
        AppliedKind::Thicken => AppliedField::ThickenEntities,
        AppliedKind::Helix => AppliedField::HelixEntity,
        AppliedKind::Fill => AppliedField::FillEdges,
        AppliedKind::SheetMetal => match &before.kind {
            FeatureKind::SheetMetalModel(x) => crate::sheetmetal_ui::first_field(x.operation),
            _ => AppliedField::SmParts,
        },
        AppliedKind::Sm9(_) => crate::sheetmetal_p3i9_ui::first_field(&before.kind),
        AppliedKind::SmFeature(_) => AppliedField::Smf(crate::sheetmetal_features_ui::SmfField::Edges),
        AppliedKind::SheetMetalTool(t) => crate::sheetmetal_tools_ui::first_field(t),
        AppliedKind::FlatExtrude => AppliedField::FlatRegions,
    };
    start(world, element, feature, kind, false, mark, Some(before), field);
}

#[allow(clippy::too_many_arguments)]
fn start(
    world: &mut World,
    element: ElementId,
    feature: FeatureId,
    kind: AppliedKind,
    is_new: bool,
    mark: usize,
    before: Option<Feature>,
    field: AppliedField,
) {
    world.resource_mut::<Selection>().0.clear();
    world.insert_resource(AppliedSession {
        element,
        feature,
        kind,
        is_new,
        mark,
        before,
        field,
        show_final: false,
        scope_auto: is_new && kind == AppliedKind::Hole,
        // P3I.9: the loft's General open, Material and Relief closed (its help dialog).
        sections: if matches!(kind, AppliedKind::Sm9(_)) { [true, true, false, false] } else { [true; 4] },
    });
    // The hole's sketch stays shown while its points are picked (it isn't consumed yet).
    world.resource_mut::<crate::parts::PartOverride>().editing = Some(feature);
}

fn end(world: &mut World) {
    world.remove_resource::<AppliedSession>();
    *world.resource_mut::<RadiusArrow>() = RadiusArrow::default();
    *world.resource_mut::<crate::transform_ui::XyzArrows>() = Default::default();
    *world.resource_mut::<crate::parts::PartOverride>() = crate::parts::PartOverride::default();
    world.resource_mut::<Selection>().0.clear();
}

/// The feature being edited.
pub fn current(world: &World) -> Option<Feature> {
    let s = world.get_resource::<AppliedSession>()?;
    world.get_resource::<ActiveDocument>()?.doc.element(s.element)?.feature(s.feature).cloned()
}

/// Replaces the feature's parameters (a command).
pub fn set(world: &mut World, kind: FeatureKind, label: &str) {
    let Some(s) = world.get_resource::<AppliedSession>().cloned() else {
        return;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    if let Err(e) = doc.execute(&SetFeature { element: s.element, feature: s.feature, kind, label: label.into() }) {
        warn!("cannot change the feature: {e}");
    }
}

/// Changes the feature's parameters with `f` (one command, if anything changed).
pub fn change_kind(world: &mut World, label: &str, f: impl FnOnce(&mut FeatureKind)) {
    let Some(feature) = current(world) else { return };
    let mut k = feature.kind.clone();
    f(&mut k);
    if k != feature.kind {
        set(world, k, label);
    }
}

/// ✓ / Enter: keeps the feature if it rebuilds.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<AppliedSession>().cloned() else {
        return;
    };
    let Some(f) = current(world).filter(|f| f.is_valid()) else {
        return;
    };
    // A feature that doesn't rebuild is kept (red in the list with its reason, as Onshape keeps
    // the funnel's failing Shell, `ex4-step10.png`); an edit before it can fix it (PS21.11).
    let label = if s.is_new { format!("Insert {}", f.name) } else { format!("Edit {}", f.name) };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    end(world);
}

/// ✕ / Esc: removes a new feature or reverts an edit.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<AppliedSession>().cloned() else {
        return;
    };
    let cur = current(world);
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let name = cur.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        if s.is_new {
            doc.discard_element_since(s.mark, s.element);
        } else {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
            if let (Some(before), Some(f)) = (s.before.clone(), cur)
                && f != before
            {
                let _ = doc.execute(&ReplaceFeature { element: s.element, feature: before, label: format!("Cancel {name}") });
            }
        }
    }
    end(world);
}

/// Accepts if it can, otherwise cancels.
pub fn finish(world: &mut World) {
    accept(world);
    if world.contains_resource::<AppliedSession>() {
        cancel(world);
    }
}

/// A picked edge or face as a feature's reference, with a point on it.
pub fn entity_of(cache: &PartCache, p: Pick) -> Option<EdgeOrFace> {
    match p {
        Pick::Edge(part, edge) => {
            let solid = &cache.part(part)?.solid;
            let seed = solid.edge(&edge)?.midpoint();
            Some(EdgeOrFace::Edge(EdgeRef { part, edge, seed }))
        }
        Pick::Face(part, face) => {
            let solid = &cache.part(part)?.solid;
            let i = solid.faces.iter().position(|f| f.name == face)?;
            let seed = solid.face_point(i)?;
            Some(EdgeOrFace::Face(FaceRef { part, face, seed }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    if let Some(i) = list.iter().position(|y| *y == x) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

/// Same entity (by name) whatever the stored point.
fn toggle_entity(list: &mut Vec<EdgeOrFace>, x: EdgeOrFace) {
    let same = |a: &EdgeOrFace| match (a, &x) {
        (EdgeOrFace::Edge(a), EdgeOrFace::Edge(b)) => a.edge == b.edge,
        (EdgeOrFace::Face(a), EdgeOrFace::Face(b)) => a.face == b.face,
        _ => false,
    };
    if let Some(i) = list.iter().position(same) {
        list.remove(i);
    } else {
        list.push(x);
    }
}

fn applied_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<AppliedSession>>,
    create: Option<Res<crate::create_selection::CreateSelection>>,
    replace: Option<Res<crate::replace_reference::ReplaceSession>>,
    mut commands: Commands,
) {
    // Create selection (X12) takes the picks while its panel is open, and so does the
    // Replace reference dialog (P3D.4).
    let (Some(s), None, None) = (session, create, replace) else {
        picks.clear();
        return;
    };
    for p in picks.read() {
        let Some(pick) = p.0 else { continue };
        let field = s.field;
        commands.queue(move |world: &mut World| {
            let Some(f) = current(world) else { return };
            let entity = entity_of(world.resource::<PartCache>(), pick);
            let mut kind = f.kind.clone();
            match (&mut kind, field) {
                (FeatureKind::Fillet(x), AppliedField::Entities) => match entity {
                    Some(e) => toggle_entity(&mut x.entities, e),
                    None => return,
                },
                (FeatureKind::Chamfer(x), AppliedField::Entities) => match entity {
                    Some(e) => toggle_entity(&mut x.entities, e),
                    None => return,
                },
                (FeatureKind::Chamfer(x), AppliedField::Overrides) => match entity {
                    Some(EdgeOrFace::Edge(e)) => {
                        if let Some(i) = x.overrides.iter().position(|o| o.edge == e.edge) {
                            x.overrides.remove(i);
                        } else {
                            x.overrides.push(e);
                        }
                    }
                    _ => return,
                },
                (FeatureKind::Shell(x), AppliedField::Faces) => {
                    if x.hollow {
                        match pick.part() {
                            Some(part) => toggle(&mut x.parts, part),
                            None => return,
                        }
                    } else {
                        match entity {
                            Some(EdgeOrFace::Face(face)) => {
                                if let Some(i) = x.faces.iter().position(|o| o.face == face.face) {
                                    x.faces.remove(i);
                                } else {
                                    x.faces.push(face);
                                }
                            }
                            _ => return,
                        }
                    }
                }
                (FeatureKind::Hole(x), AppliedField::Points) => match pick {
                    Pick::SketchPoint(sketch, point) => toggle(&mut x.points, HolePoint { sketch, point }),
                    // PS15.2: a Mate connector picked in the list or the view is a place too.
                    Pick::Feature(f) => {
                        let features = world
                            .get_resource::<ActiveDocument>()
                            .and_then(|d| d.active_element())
                            .map(|e| e.features().to_vec())
                            .unwrap_or_default();
                        if crate::pattern::is_connector(&features, f) {
                            toggle(&mut x.connectors, cadrs_core::mate::ConnectorRef::Feature(f));
                        } else {
                            toggle(&mut x.sketches, f);
                        }
                    }
                    _ => return,
                },
                (FeatureKind::Hole(x), AppliedField::MergeScope) => match pick.part() {
                    Some(part) => toggle(&mut x.merge_scope, part),
                    None => return,
                },
                (FeatureKind::Fillet(x), AppliedField::Side1 | AppliedField::Center | AppliedField::Side2) => {
                    let Some(EdgeOrFace::Face(face)) = entity else { return };
                    let (list, next) = match field {
                        AppliedField::Side1 => (&mut x.side1, AppliedField::Center),
                        AppliedField::Center => (&mut x.center, AppliedField::Side2),
                        _ => (&mut x.side2, AppliedField::Side2),
                    };
                    // One face each; picking fills the field and moves on to the next.
                    if list.first().is_some_and(|f| f.face == face.face) {
                        list.clear();
                    } else {
                        *list = vec![face];
                        if let Some(mut s) = world.get_resource_mut::<AppliedSession>() {
                            s.field = next;
                        }
                    }
                }
                (k @ (FeatureKind::Plane(_) | FeatureKind::Sweep(_) | FeatureKind::Loft(_) | FeatureKind::Split(_)), _) => {
                    if !crate::advanced::pick(world, k, field, pick) {
                        return;
                    }
                }
                // P3.10: the Draft, the hole's start plane and Up to entity, a variable fillet's
                // vertices and points on edge.
                (k @ FeatureKind::Draft(_), _)
                | (k @ FeatureKind::Hole(_), AppliedField::HoleStartPlane | AppliedField::HoleUpTo)
                | (k @ FeatureKind::Fillet(_), AppliedField::FilletVertices | AppliedField::FilletEdgePoints) => {
                    if !crate::draft_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ FeatureKind::Transform(_), _) => {
                    if !crate::transform_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ (FeatureKind::Thicken(_) | FeatureKind::Helix(_) | FeatureKind::Fill(_)), _) => {
                    if !crate::surfacing_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ FeatureKind::SheetMetalModel(_), _) => {
                    if !crate::sheetmetal_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ (FeatureKind::SheetMetalLoft(_) | FeatureKind::Form(_) | FeatureKind::TagForm(_)), AppliedField::Sm9(f)) => {
                    if !crate::sheetmetal_p3i9_ui::pick(world, k, f, pick) {
                        return;
                    }
                }
                (k @ FeatureKind::SheetMetal(_), _) => {
                    if !crate::sheetmetal_features_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ FeatureKind::SheetMetalTool(_), _) => {
                    if !crate::sheetmetal_tools_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                (k @ FeatureKind::FlatExtrude(_), _) => {
                    if !crate::flat_ui::pick(world, k, field, pick) {
                        return;
                    }
                }
                // P3.8 (and a hole's mate connectors, PS15.2).
                (k @ (FeatureKind::Pattern(_) | FeatureKind::Mirror(_) | FeatureKind::MateConnector(_)), _)
                | (k @ FeatureKind::Hole(_), AppliedField::HoleConnectors) => {
                    if !crate::pattern::pick(world, k, field, pick) {
                        return;
                    }
                }
                _ => return,
            }
            set(world, kind, "Select");
        });
    }
}

fn applied_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<AppliedSession>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    focus: Res<bevy::input_focus::InputFocus>,
    menus: Query<(), With<cadrs_ui::menu::MenuPopup>>,
    mut menu_open: Local<bool>,
    mut commands: Commands,
) {
    // Esc closes an open menu (a select's list) first, not the dialog: the menu may already be
    // gone this frame, so last frame's state counts too.
    let was_open = std::mem::replace(&mut *menu_open, !menus.is_empty());
    if session.is_none() || was_open || *menu_open {
        keys.clear();
        return;
    }
    for k in keys.read() {
        if k.state != ButtonState::Pressed || !q_dialogs.is_empty() || focus.get().is_some() {
            continue;
        }
        match k.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// References in the view

/// While the chamfer's Direction overrides (or a variable fillet's vertices and points, P3.10)
/// take picks, the view shows the parts before it, so the edges it rounds can be picked again.
fn roll_back_for_overrides(session: Option<Res<AppliedSession>>, composite: Option<Res<crate::composite_ui::CompositeSession>>, mut over: ResMut<crate::parts::PartOverride>) {
    // P3H.6: the Composite part dialog sets the view itself.
    if composite.is_some() {
        return;
    }
    // P3.10: a variable fillet's vertices and points are picked on the edges it rounds too.
    // P3I.2: a Convert's faces to exclude and edges to bend are picked on the part it consumes.
    let want = session
        .filter(|s| {
            matches!(s.field, AppliedField::Overrides | AppliedField::FilletVertices | AppliedField::FilletEdgePoints | AppliedField::SmExclude | AppliedField::SmBends)
        })
        .map(|s| s.feature);
    if over.rolled_back != want {
        over.rolled_back = want;
    }
}

/// While a feature before the end is edited, the view shows the Part Studio up to it, unless
/// the dialog's Final is on (PS21.11).
fn roll_back_to_edited(
    session: Option<Res<AppliedSession>>,
    extrude: Option<Res<crate::extrude::ExtrudeSession>>,
    boolean: Option<Res<crate::boolean::BooleanSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut over: ResMut<crate::parts::PartOverride>,
    (sketch, composite): (Option<Res<crate::sketch::SketchSession>>, Option<Res<crate::composite_ui::CompositeSession>>),
) {
    // P3H.6: the Composite part dialog sets the view itself.
    if composite.is_some() {
        return;
    }
    // P3D.1 (IR5.4): a sketch edited before the end rolls the Part Studio back to it (the
    // features after it greyed, italic and not built, `ex1-step3.png`) unless its Final is on.
    if session.is_none()
        && extrude.is_none()
        && boolean.is_none()
        && let Some(x) = sketch.as_ref()
    {
        let want = doc.as_ref().and_then(|d| {
            let el = d.doc.element(x.element)?;
            (!x.show_final && crate::feature_list::last_built(el) != Some(x.feature)).then_some(x.feature)
        });
        if over.rollback_to != want {
            over.rollback_to = want;
        }
        return;
    }
    // The Boolean dialog (P3.9): rolled back the same way.
    if session.is_none()
        && extrude.is_none()
        && let Some(x) = boolean.as_ref()
    {
        let want = doc.as_ref().and_then(|d| {
            let el = d.doc.element(x.element)?;
            (!x.show_final && crate::feature_list::last_built(el) != Some(x.feature)).then_some(x.feature)
        });
        if over.rollback_to != want {
            over.rollback_to = want;
        }
        return;
    }
    // The Extrude and Revolve dialogs (their shared session): rolled back the same way.
    if session.is_none()
        && let Some(x) = extrude.as_ref()
    {
        let want = doc.as_ref().and_then(|d| {
            let el = d.doc.element(x.element)?;
            let last = crate::feature_list::last_built(el);
            (!x.show_final && last != Some(x.feature)).then_some(x.feature)
        });
        if over.rollback_to != want {
            over.rollback_to = want;
        }
        return;
    }
    // With Final on, the edited feature isn't shown as a preview either: the final model as
    // it will be (`ex4-step11.png`).
    if let Some(s) = session.as_ref() {
        let editing = (!s.show_final).then_some(s.feature);
        if over.editing != editing {
            over.editing = editing;
        }
    }
    let want = session.zip(doc).and_then(|(s, d)| {
        let el = d.doc.element(s.element)?;
        let last = crate::feature_list::last_built(el);
        (!s.show_final && last != Some(s.feature)).then_some(s.feature)
    });
    if over.rollback_to != want {
        over.rollback_to = want;
    }
}

/// The parts before the feature being edited (cached on the rebuild thread).
fn update_before_parts(session: Option<Res<AppliedSession>>, doc: Option<Res<ActiveDocument>>, mut before: ResMut<BeforeParts>) {
    let (Some(s), Some(doc)) = (session, doc) else {
        if before.key.is_some() {
            *before = BeforeParts::default();
        }
        return;
    };
    let Some(el) = doc.doc.element(s.element) else { return };
    let features = el.active_features();
    let Some(i) = features.iter().position(|f| f.id == s.feature) else { return };
    let upstream = &features[..i];
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{upstream:?}").hash(&mut h);
        h.finish()
    };
    if before.key == Some((s.feature, key)) {
        return;
    }
    before.parts = cadrs_core::rebuild::build(upstream).parts.clone();
    before.key = Some((s.feature, key));
}

/// A new hole's Merge scope defaults to the part its points drill (P3.8 judge; Onshape fills
/// the field with the drilled part), once, as soon as the rebuild knows it.
fn default_hole_scope(session: Option<Res<AppliedSession>>, cache: Res<PartCache>, mut commands: Commands) {
    let Some(s) = session.filter(|s| s.scope_auto) else { return };
    if cache.rebuilding {
        return;
    }
    let Some(drilled) = cache.contacts.get(&s.feature).map(|c| c.overlaps.clone()).filter(|d| !d.is_empty()) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<AppliedSession>() {
            s.scope_auto = false;
        }
        let Some(f) = current(world) else { return };
        let FeatureKind::Hole(mut h) = f.kind else { return };
        if h.merge_scope.is_empty() {
            h.merge_scope = drilled;
            set(world, FeatureKind::Hole(h), "Merge scope");
        }
    });
}

/// The picks of the dialog show as selected where they exist in the result too.
fn show_references(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<PartCache>,
    create: Option<Res<crate::create_selection::CreateSelection>>,
    mut selection: ResMut<Selection>,
    mut failed: ResMut<crate::parts::FailedReferences>,
) {
    // Red only when the rebuild fails (walls that cross), not while parameters are missing.
    let fails = session.as_ref().zip(doc.as_ref()).is_some_and(|(s, d)| {
        cache.errors.contains_key(&s.feature)
            && d.doc.element(s.element).and_then(|e| e.feature(s.feature)).is_some_and(|f| f.is_valid())
    });
    let (Some(s), Some(doc)) = (session, doc) else {
        if *failed != crate::parts::FailedReferences::default() {
            *failed = crate::parts::FailedReferences::default();
        }
        return;
    };
    let Some(f) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)) else {
        return;
    };
    let mut want: Vec<Pick> = Vec::new();
    match &f.kind {
        FeatureKind::Shell(x) if x.hollow => want.extend(x.parts.iter().map(|p| Pick::Part(*p))),
        // The points (the merge scope's parts stay as they are, `ex3-step5.png`).
        FeatureKind::Hole(x) => {
            want.extend(x.points.iter().map(|p| Pick::SketchPoint(p.sketch, p.point)));
            // P3.11 (P3.6 judge): a whole sketch's points are marked as picked points are.
            let features = doc.doc.element(s.element).map(|e| e.features()).unwrap_or(&[]);
            for sk in &x.sketches {
                if let Some(g) = features.iter().find(|f| f.id == *sk).and_then(|f| f.sketch()) {
                    want.extend(cadrs_core::hole::hole_vertices(&g.geometry).into_iter().map(|p| Pick::SketchPoint(*sk, p)));
                }
            }
            want.extend(x.connectors.iter().filter_map(crate::pattern::connector_pick));
        }
        k @ (FeatureKind::Plane(_) | FeatureKind::Sweep(_) | FeatureKind::Loft(_) | FeatureKind::Split(_)) => {
            let features = doc.doc.element(s.element).map(|e| e.features()).unwrap_or(&[]);
            want.extend(crate::advanced::references(k, &cache, features));
        }
        k @ (FeatureKind::Pattern(_) | FeatureKind::Mirror(_) | FeatureKind::MateConnector(_)) => {
            want.extend(crate::pattern::references(k, &cache));
        }
        k @ FeatureKind::Draft(_) => want.extend(crate::draft_ui::references(k, &cache)),
        k @ FeatureKind::Transform(_) => want.extend(crate::transform_ui::references(k, &cache)),
        k @ (FeatureKind::Thicken(_) | FeatureKind::Helix(_) | FeatureKind::Fill(_)) => want.extend(crate::surfacing_ui::references(k, &cache)),
        k @ FeatureKind::SheetMetalModel(_) => want.extend(crate::sheetmetal_ui::references(k, &cache)),
        k @ (FeatureKind::SheetMetalLoft(_) | FeatureKind::Form(_)) => want.extend(crate::sheetmetal_p3i9_ui::references(k, &cache)),
        k @ FeatureKind::SheetMetal(_) => want.extend(crate::sheetmetal_features_ui::references(k, &cache)),
        k @ FeatureKind::SheetMetalTool(_) => want.extend(crate::sheetmetal_tools_ui::references(k, &cache)),
        _ => {}
    }
    // P3.10: a hole's start plane and Up to entity, a variable fillet's vertices.
    if matches!(f.kind, FeatureKind::Hole(_) | FeatureKind::Fillet(_)) {
        want.extend(crate::draft_ui::references(&f.kind, &cache));
    }
    // A failing feature: the body its references are on is tinted red (`ex4-step10.png`: only the
    // shelled loft, not the handle joined to it: the faces the same features made as the faces
    // referenced), the references themselves stay in the selection colour.
    let mut parts: Vec<PartId> = Vec::new();
    let mut ops: Vec<cadrs_sketch::OpId> = Vec::new();
    if fails {
        let (entities, faces): (Vec<PartId>, Vec<cadrs_sketch::OpId>) = match &f.kind {
            FeatureKind::Fillet(x) => (x.entities.iter().map(|e| e.part()).collect(), x.entities.iter().map(|e| e.op()).collect()),
            FeatureKind::Chamfer(x) => (x.entities.iter().map(|e| e.part()).collect(), x.entities.iter().map(|e| e.op()).collect()),
            FeatureKind::Shell(x) if x.hollow => (x.parts.clone(), Vec::new()),
            FeatureKind::Shell(x) => (x.faces.iter().map(|f| f.part).collect(), x.faces.iter().map(|f| f.face.op).collect()),
            FeatureKind::Pattern(cadrs_core::pattern::PatternFeature { faces, .. })
            | FeatureKind::Mirror(cadrs_core::pattern::MirrorFeature { faces, .. })
                if !faces.is_empty() =>
            {
                (faces.iter().map(|f| f.part).collect(), faces.iter().map(|f| f.face.op).collect())
            }
            _ => (want.iter().filter_map(|p| p.part()).collect(), Vec::new()),
        };
        for p in entities {
            if !parts.contains(&p) {
                parts.push(p);
            }
        }
        for o in faces {
            if !ops.contains(&o) {
                ops.push(o);
            }
        }
    }
    // The faces it refers to keep the selection tint over the red (the shell's faces to remove).
    let refs: Vec<Pick> = match &f.kind {
        FeatureKind::Shell(x) if fails && !x.hollow => x.faces.iter().map(|g| Pick::Face(g.part, g.face)).collect(),
        _ => Vec::new(),
    };
    let want_failed = crate::parts::FailedReferences(fails, parts, ops, refs);
    if *failed != want_failed {
        *failed = want_failed;
    }
    // Create selection's chain (X12) or pocket (P3.8), until Add selection puts it in the field.
    if let Some(c) = create {
        want.extend(c.edges.iter().map(|(p, e)| Pick::Edge(*p, *e)));
        want.extend(c.faces.iter().map(|(p, f)| Pick::Face(*p, *f)));
    }
    if selection.0 != want {
        selection.0 = want;
    }
}

/// The edges (with their tangent chains) and faces a fillet, chamfer or shell refers to, drawn
/// in amber on the parts as they were before it.
fn draw_references(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    before: Res<BeforeParts>,
    failed: Res<crate::parts::FailedReferences>,
    mut g: Gizmos<crate::parts::PickedEdgeGizmos>,
) {
    // The references stay in the selection colour while the feature fails; the body is red
    // (`ex4-step10.png`).
    let _ = &failed;
    let color = crate::parts::SELECTED;
    let (Some(s), Some(doc)) = (session, doc) else {
        return;
    };
    let Some(f) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)) else {
        return;
    };
    let v3 = |p: &[f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    // The Transform: its parts' outlines where they were before it.
    if let FeatureKind::Transform(x) = &f.kind {
        for part in before.parts.iter().filter(|p| x.parts.contains(&p.id)) {
            for e in &part.solid.edges {
                g.linestrip(e.points.iter().map(v3), color);
            }
        }
        return;
    }
    let (entities, propagate): (Vec<EdgeOrFace>, bool) = match &f.kind {
        FeatureKind::Fillet(x) if x.kind == FilletType::FullRound => (
            x.side1.iter().chain(&x.center).chain(&x.side2).map(|f| EdgeOrFace::Face(*f)).collect(),
            false,
        ),
        FeatureKind::Fillet(x) => (x.entities.clone(), x.tangent_propagation),
        FeatureKind::Chamfer(x) => (
            x.entities.iter().copied().chain(x.overrides.iter().map(|e| EdgeOrFace::Edge(*e))).collect(),
            x.tangent_propagation,
        ),
        FeatureKind::Shell(x) if !x.hollow => (x.faces.iter().map(|f| EdgeOrFace::Face(*f)).collect(), false),
        FeatureKind::Draft(x) => (x.faces.iter().map(|f| EdgeOrFace::Face(*f)).collect(), false),
        k @ FeatureKind::SheetMetalModel(_) => (crate::sheetmetal_ui::drawn_references(k), false),
        k @ FeatureKind::SheetMetal(_) => (crate::sheetmetal_features_ui::drawn_references(k), false),
        _ => return,
    };
    for e in &entities {
        let Some(part) = before.parts.iter().find(|p| p.id == e.part()).or_else(|| before.parts.first()) else {
            continue;
        };
        let solid = &part.solid;
        match e {
            EdgeOrFace::Edge(r) => {
                let names = if propagate { tangent_chain(solid, &r.edge) } else { vec![r.edge] };
                for n in names {
                    if let Some(edge) = solid.edge(&n) {
                        g.linestrip(edge.points.iter().map(v3), color);
                    }
                }
            }
            EdgeOrFace::Face(r) => {
                if let Some(face) = solid.face(&r.face) {
                    for l in &face.loops {
                        let mut pts: Vec<Vec3> = l.iter().map(v3).collect();
                        if let Some(first) = pts.first().copied() {
                            pts.push(first);
                        }
                        g.linestrip(pts, color);
                    }
                }
            }
        }
    }
}

/// Hovered and selected sketch points (a hole's places): a dot about 9 px across, facing the
/// viewer, in the hover or selection orange.
fn draw_sketch_point_picks(
    cache: Res<PartCache>,
    highlight: Res<crate::viewport::PlaneHighlight>,
    selection: Res<Selection>,
    view: Res<crate::viewport::ViewportView>,
    mut g: Gizmos<crate::parts::VertexGizmos>,
) {
    let v = view.view;
    let rot = Quat::from_rotation_arc(Vec3::Z, v.back());
    for sc in &cache.sketch_curves {
        for (id, p) in &sc.points {
            let pick = Pick::SketchPoint(sc.sketch, *id);
            let color = if highlight.is_hovered(pick) {
                crate::parts::HOVER
            } else if selection.contains(pick) {
                crate::parts::SELECTED
            } else {
                continue;
            };
            let at = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
            for r in [0.8, 1.6, 2.4, 3.2, 4.2] {
                g.circle(Isometry3d::new(at, rot), r * v.scale, color).resolution(20);
            }
        }
    }
}

/// An edge and the edges tangent-connected to it on the display mesh (their polylines meet end
/// to end, running on within 1°): what tangent propagation adds.
pub fn tangent_chain(solid: &cadrs_core::Solid, edge: &cadrs_sketch::EdgeName) -> Vec<cadrs_sketch::EdgeName> {
    let Some(start) = solid.edge(edge) else {
        return vec![*edge];
    };
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let unit = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let dist = |a: [f64; 3], b: [f64; 3]| {
        let d = sub(a, b);
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    // The ends of a polyline with the direction it leaves each end in.
    let ends = |pts: &[[f64; 3]]| -> Vec<([f64; 3], [f64; 3])> {
        let n = pts.len();
        if n < 2 || dist(pts[0], pts[n - 1]) < 1e-6 {
            return Vec::new();
        }
        vec![(pts[0], unit(sub(pts[1], pts[0]))), (pts[n - 1], unit(sub(pts[n - 2], pts[n - 1])))]
    };
    let cos = (1.0f64).to_radians().cos();
    let mut chain = vec![start.name];
    let mut todo = vec![start.name];
    while let Some(cur) = todo.pop() {
        let Some(e) = solid.edge(&cur) else { continue };
        for (p, d) in ends(&e.points) {
            for o in &solid.edges {
                if chain.contains(&o.name) {
                    continue;
                }
                for (q, dq) in ends(&o.points) {
                    // Continuing: the other edge leaves the joint the opposite way.
                    let dot = d[0] * dq[0] + d[1] * dq[1] + d[2] * dq[2];
                    if dist(p, q) < 1e-4 && dot <= -cos {
                        chain.push(o.name);
                        todo.push(o.name);
                        break;
                    }
                }
            }
        }
    }
    chain
}

// ---------------------------------------------------------------------------------------------
// The fillet's radius arrow (PS14.5) and the Offset plane's distance arrow (P3.7, PS12.2)

/// The fillet dialog's arrow manipulator (and the Plane dialog's, for an Offset plane): where it
/// is on screen and a drag in progress.
#[derive(Resource, Debug, Default)]
pub struct RadiusArrow {
    /// Its base and tip on screen.
    pub base_tip: Option<(Vec2, Vec2)>,
    /// The edge's point it stands on and its direction (world), and mm per screen px along it.
    axis: Option<(Vec3, Vec3)>,
    pub hovered: bool,
    pub drag: Option<RadiusDrag>,
}

#[derive(Debug, Clone)]
pub struct RadiusDrag {
    start: Vec2,
    start_size: f64,
    dir_px: Vec2,
    /// The feature's parameters as dragged.
    pub kind: FeatureKind,
}

/// The value an arrow drags: a fillet's size, an Offset plane's distance.
fn arrow_size(k: &FeatureKind) -> Option<f64> {
    match k {
        FeatureKind::Fillet(f) => Some(f.size),
        FeatureKind::Plane(p) if p.kind == cadrs_core::plane::PlaneType::Offset => Some(p.offset),
        // P3I.2: a sheet metal Extrude's depth.
        FeatureKind::SheetMetalModel(x) if x.operation == cadrs_core::sheetmetal::SheetMetalOp::Extrude => Some(x.depth),
        _ => None,
    }
}

fn set_arrow_size(k: &mut FeatureKind, v: f64, expr: String) {
    match k {
        FeatureKind::Fillet(f) => {
            f.size = v;
            f.size_expr = expr;
        }
        FeatureKind::Plane(p) => {
            p.offset = v;
            p.offset_expr = expr;
        }
        FeatureKind::SheetMetalModel(x) => {
            x.depth = v;
            x.depth_expr = expr;
        }
        _ => {}
    }
}

const ARROW_LEN: f32 = 44.0;

/// Grabs and drags the arrow: the size follows the pointer along the arrow, snapped like the
/// extrude's depth, and one undo step is recorded on release.
#[allow(clippy::too_many_arguments)]
fn radius_arrow_pointer(
    mut inputs: MessageReader<bevy::picking::pointer::PointerInput>,
    session: Option<Res<AppliedSession>>,
    mut arrow: ResMut<RadiusArrow>,
    doc: Option<Res<ActiveDocument>>,
    view: Res<crate::viewport::ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    units: Res<crate::WorkspaceUnits>,
    mut over: ResMut<crate::parts::PartOverride>,
    mut commands: Commands,
) {
    use bevy::picking::pointer::{PointerAction, PointerButton, PointerId};
    let Some(s) = session.filter(|s| matches!(s.kind, AppliedKind::Fillet | AppliedKind::Plane | AppliedKind::SheetMetal)) else {
        inputs.clear();
        return;
    };
    let near = |p: Vec2, (a, b): (Vec2, Vec2)| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        p.distance(a + ab * t) <= 8.0
    };
    arrow.hovered = arrow.base_tip.is_some_and(|bt| near(drag.pointer(), bt));
    let kind = doc.as_ref().and_then(|d| Some(d.doc.element(s.element)?.feature(s.feature)?.kind.clone()));
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        match input.action {
            PointerAction::Press(PointerButton::Primary) => {
                if let (Some(bt), Some(k), Some((_, dir))) = (arrow.base_tip, kind.clone(), arrow.axis)
                    && near(pos, bt)
                    && let Some(size) = arrow_size(&k)
                {
                    let dir_px = view.view.project_vector(dir);
                    if dir_px.length() > 0.05 {
                        arrow.drag = Some(RadiusDrag { start: pos, start_size: size, dir_px, kind: k });
                    }
                }
            }
            PointerAction::Move { .. } => {
                if let Some(d) = arrow.drag.as_mut() {
                    let along = (pos - d.start).dot(d.dir_px.normalize()) / d.dir_px.length();
                    let step = crate::extrude::snap_step(d.dir_px.length());
                    let size = ((d.start_size + along as f64) / step).round() * step;
                    let size = size.max(step);
                    if arrow_size(&d.kind).is_some_and(|v| (size - v).abs() > 1e-9) {
                        set_arrow_size(&mut d.kind, size, units.0.with_unit(size, cadrs_sketch::units::Quantity::Length));
                    }
                    let want = Some((s.feature, d.kind.clone()));
                    if over.applied != want {
                        over.applied = want;
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) if arrow.drag.is_some() => {
                commands.queue(|world: &mut World| {
                    world.resource_mut::<crate::parts::PartOverride>().applied = None;
                    let Some(d) = world.resource_mut::<RadiusArrow>().drag.take() else { return };
                    if current(world).is_some_and(|f| f.kind != d.kind) {
                        let label = match &d.kind {
                            FeatureKind::Fillet(f) if f.measurement == cadrs_core::applied::FilletMeasurement::Width => "Drag width",
                            FeatureKind::Fillet(_) => "Drag radius",
                            FeatureKind::SheetMetalModel(_) => "Drag depth",
                            _ => "Drag offset",
                        };
                        set(world, d.kind, label);
                    }
                });
            }
            PointerAction::Cancel => {
                arrow.drag = None;
                over.applied = None;
            }
            _ => {}
        }
    }
}

#[derive(Component)]
struct RadiusArrowNode;

#[derive(Component)]
struct RadiusArrowLine;

/// Places the arrow at the middle of the first picked edge (on the parts before the fillet),
/// pointing out of the corner between its faces (the fillet's radius grows that way).
#[allow(clippy::too_many_arguments)]
fn place_radius_arrow(
    session: Option<Res<AppliedSession>>,
    doc: Option<Res<ActiveDocument>>,
    before: Res<BeforeParts>,
    view: Res<crate::viewport::ViewportView>,
    rect: Res<crate::viewport::ViewportRect>,
    mut arrow: ResMut<RadiusArrow>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut q: Query<(Entity, &mut Node, &mut bevy::ui::UiTransform, &mut Visibility), With<RadiusArrowNode>>,
    mut q_line: Query<&mut ImageNode, With<RadiusArrowLine>>,
    mut commands: Commands,
) {
    let dragged = arrow.drag.as_ref().and_then(|d| arrow_size(&d.kind));
    // An Offset plane: at the new plane's origin, along the offset (its base plane's normal,
    // reversed by the flip).
    let plane_target = session.as_ref().filter(|s| s.kind == AppliedKind::Plane).and_then(|s| {
        let x = match &doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.kind {
            FeatureKind::Plane(x) if x.kind == cadrs_core::plane::PlaneType::Offset => x.clone(),
            _ => return None,
        };
        let base = match x.entities.first()? {
            cadrs_core::plane::PlaneEntity::Plane(p) => p.frame(),
            cadrs_core::plane::PlaneEntity::Face(f) => before.parts.iter().find(|p| p.id == f.part)?.solid.face(&f.face)?.plane?,
            _ => return None,
        };
        let n = base.normal();
        let dir = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32).normalize_or_zero() * if x.flip { -1.0 } else { 1.0 };
        let o = Vec3::new(base.origin[0] as f32, base.origin[1] as f32, base.origin[2] as f32);
        Some((o + dir * dragged.unwrap_or(x.offset) as f32, dir))
    });
    let target = plane_target.or_else(|| session.as_ref().filter(|s| s.kind == AppliedKind::Fillet).and_then(|s| {
        let f = doc.as_ref()?.doc.element(s.element)?.feature(s.feature)?.fillet()?.clone();
        if f.kind != FilletType::Edge {
            return None;
        }
        let first = f.entities.iter().find_map(|e| match e {
            EdgeOrFace::Edge(r) => Some(*r),
            _ => None,
        })?;
        let part = before.parts.iter().find(|p| p.id == first.part)?;
        let edge = part.solid.edge(&first.edge)?;
        let mid = edge.midpoint();
        // Out of the corner: the sum of the two faces' outward normals (planar faces), else
        // straight up.
        let n = first
            .edge
            .faces
            .iter()
            .filter_map(|x| part.solid.face(x)?.plane)
            .map(|pl| {
                let v = pl.normal();
                Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32).normalize_or_zero()
            })
            .fold(Vec3::ZERO, |a, b| a + b);
        let dir = if n.length() > 1e-3 { n.normalize() } else { Vec3::Z };
        // It stands off the edge by the size, so it moves with the value (and with the pointer
        // while dragged: the drag maps pointer travel along it to mm 1:1).
        let size = dragged.unwrap_or(f.size) as f32;
        Some((Vec3::new(mid[0] as f32, mid[1] as f32, mid[2] as f32) + dir * size, dir))
    }));
    // P3I.2: a sheet metal Extrude's depth, at the end of its first curve's sweep.
    let sm_target = session.as_ref().filter(|s| s.kind == AppliedKind::SheetMetal).and_then(|s| {
        let el = doc.as_ref()?.doc.element(s.element)?;
        let FeatureKind::SheetMetalModel(x) = &el.feature(s.feature)?.kind else { return None };
        crate::sheetmetal_ui::depth_arrow(el.features(), x, dragged.unwrap_or(x.depth))
    });
    let target = target.or(sm_target);
    let arrow_name = if plane_target.is_some() {
        "plane-offset-arrow"
    } else if sm_target.is_some() {
        "sm-depth-arrow"
    } else {
        "fillet-arrow"
    };
    arrow.axis = target;
    let placed = target.and_then(|(p, dir)| {
        let d = view.view.project_vector(dir);
        (d.length() >= 0.05).then(|| {
            let b = rect.to_screen(view.view.project(p));
            (b, b + d.normalize() * ARROW_LEN)
        })
    });
    arrow.base_tip = placed;
    let Some((base, tip)) = placed else {
        for (e, ..) in &q {
            commands.entity(e).try_despawn();
        }
        return;
    };
    if q.is_empty() {
        let Some(area) = q_area.iter().next() else { return };
        let e = commands
            .spawn((
                Name::new(arrow_name),
                RadiusArrowNode,
                Node { position_type: PositionType::Absolute, width: Val::Px(ARROW_LEN), height: Val::Px(ARROW_LEN), ..default() },
                bevy::ui::UiTransform::default(),
                Visibility::Hidden,
                Pickable::IGNORE,
                ZIndex(-3),
                DespawnOnExit(AppState::Document),
                children![
                    (
                        cadrs_ui::icon::icon_in(
                            "manipulator-arrow-halo",
                            ARROW_LEN,
                            Color::srgba_u8(0x3c, 0x46, 0x4e, 0xb0),
                            Node { position_type: PositionType::Absolute, ..default() },
                        ),
                        Pickable::IGNORE,
                    ),
                    (
                        RadiusArrowLine,
                        cadrs_ui::icon::icon_in(
                            "manipulator-arrow-line",
                            ARROW_LEN,
                            Color::WHITE,
                            Node { position_type: PositionType::Absolute, ..default() },
                        ),
                        Pickable::IGNORE,
                    ),
                ],
            ))
            .id();
        commands.entity(area).add_child(e);
        return;
    }
    let u = (tip - base).normalize_or_zero();
    let center = (base + tip) / 2.0 - rect.0.min;
    for (_, mut node, mut transform, mut vis) in &mut q {
        let (l, t) = (Val::Px(center.x - ARROW_LEN / 2.0), Val::Px(center.y - ARROW_LEN / 2.0));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
        let want = bevy::ui::UiTransform { rotation: Rot2::radians(u.x.atan2(-u.y)), ..default() };
        if *transform != want {
            *transform = want;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
    let c = if arrow.hovered || arrow.drag.is_some() { Color::srgb_u8(0xff, 0xb4, 0x5a) } else { Color::WHITE };
    for mut img in &mut q_line {
        if img.color != c {
            img.color = c;
        }
    }
}

/// The part names a Parts field shows.
pub fn part_names(cache: &PartCache, ids: &[PartId]) -> Vec<String> {
    ids.iter().map(|p| cache.part_name(*p).unwrap_or("Part").to_string()).collect()
}
