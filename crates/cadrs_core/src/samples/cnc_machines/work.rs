//! What is machined and what holds it: the Part Studios "Test piece", "Worm" and
//! "Workholding" (mm).
//!
//! The **Test piece** is one part with its axis along X and features for each machine:
//!
//! | Feature | Shape | For |
//! |---|---|---|
//! | Block | 60 × 50 × 30 at x 0…60, y ±25, z ±15 | all |
//! | Shaft | Ø40 at x 60…130 | all |
//! | Chucking spigot | Ø30 at x −25…0 | the chuck and the fixture plate |
//! | Dome | R20 about (130, 0, 0), to x 150 | 5-axis |
//! | Pocket | 30 × 20, 8 deep in the block's top (x 15…45, y ±10, floor at z 7) | 3-axis |
//! | Holes | two Ø6 through the block along Z, at (7.5, 10) and (52.5, −10) | 3-axis |
//! | Flats | three, 120° apart round the shaft at x 70…85, 17 from the axis (one faces +Z) | 4th axis |
//! | Groove | a Ø5 round groove on a helix round the shaft: pitch 20, 1.5 turns, x 95…125, 2.5 deep | 4th axis |
//! | Dome holes | four Ø4 holes 6 deep, square to the dome 45° up from its rim, towards ±Y and ±Z | 5-axis |
//!
//! The **Worm** has the sizes of the worm of [`crate::samples::gears::worm_drive`] (single
//! start, module 2, diameter quotient 10, 50 long) and lies on the X axis from x 200 to 250,
//! with a Ø20 chucking spigot at x 125…200: beyond the Test piece's dome, as the two are
//! components of one CAM Studio, which shows them where they are modelled. It is made from
//! plain features, the way it would be milled, as the Gear feature's worm builds without its
//! thread for now ("Only the core is shown"): a Ø24 blank, and the space between the thread's
//! turns (straight sides 20° from square, an Archimedean worm's axial section: 3.142 wide on
//! the Ø20 pitch cylinder, down to Ø15.2) cut out along a helix of 6.283 lead that runs out of
//! both ends.
//!
//! The **Workholding** parts are drawn round the Test piece where it lies, as CAM components
//! are not placed (so in their own Part Studio they lie in each other):
//!
//! | Part | Shape |
//! |---|---|
//! | Vise | a base (x −10…70, y ±60, z −45…−25) open under the block (a window x 0…60, y ±19), two jaws on the block's y faces up to z 8, and two ledges the block's edges sit on; clear of the shaft. Seen from above nothing of it is under the pocket or the holes: toolpaths keep off the workholding's whole footprint |
//! | Chuck | the 4th axis's chuck ([`super::parts`]) with its jaws on the spigot: x −70…0 |
//! | Fixture plate | a Ø120 ring 25 thick on the spigot (x −25…0), its faces square to X |

use cadrs_sketch::SketchOp;

use crate::advanced::{PathRef, SweepFeature};
use crate::appearance::Appearance;
use crate::command::CommandError;
use crate::commands::{AddFeature, RenamePart, SetPartAppearance};
use crate::document::{AxisRef, BooleanOp, FeatureKind};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::studio::Studio;
use crate::surfacing::{HelixFeature, HelixPath, HelixType};

use super::{On, circle, mm, polar, radial_rect, rect, sketch, solid, turned, v};

const fn hold(n: u128) -> FeatureId {
    super::feature(2, n)
}

const fn piece(n: u128) -> FeatureId {
    super::feature(3, n)
}

const fn worm_id(n: u128) -> FeatureId {
    super::feature(4, n)
}

/// The Test piece (the Part Studio [`super::TEST_PIECE`]'s only part).
pub const PIECE: PartId = PartId::new(piece(0x12), 0);
/// The Worm (the Part Studio [`super::WORM`]'s only part).
pub const WORM_PART: PartId = PartId::new(worm_id(0x12), 0);
/// The Workholding parts.
pub const VISE: PartId = PartId::new(hold(0x12), 0);
pub const CHUCK: PartId = PartId::new(hold(0x22), 0);
pub const FIXTURE_PLATE: PartId = PartId::new(hold(0x32), 0);

/// The middle of the pocket's floor.
pub const POCKET_FLOOR: [f64; 3] = [30.0, 0.0, 7.0];
/// The two Ø6 holes' axes (x, y); they run along Z.
pub const HOLES: [[f64; 2]; 2] = [[7.5, 10.0], [52.5, -10.0]];
/// The middle of the flat that faces +Z.
pub const TOP_FLAT: [f64; 3] = [77.5, 0.0, 17.0];

/// The flat `angle` degrees round the shaft from the one that faces +Z (0, 120 or 240,
/// counter-clockwise seen from +X): its middle and its outward normal.
pub fn flat_at(angle: f64) -> ([f64; 3], [f64; 3]) {
    let (s, c) = (90.0 + angle).to_radians().sin_cos();
    ([TOP_FLAT[0], TOP_FLAT[2] * c, TOP_FLAT[2] * s], [0.0, c, s])
}

/// The middle of the Vise's underside: what stands on a machine's table.
pub const VISE_FOOT: [f64; 3] = [30.0, 0.0, -45.0];
/// The groove's helix: where it starts and ends along X, its pitch, and the shaft's radius it
/// winds on.
pub const GROOVE_X: (f64, f64) = (95.0, 125.0);
pub const GROOVE_PITCH: f64 = 20.0;
pub const SHAFT_RADIUS: f64 = 20.0;
/// The dome's centre and radius.
pub const DOME_CENTRE: [f64; 3] = [130.0, 0.0, 0.0];
pub const DOME_RADIUS: f64 = 20.0;
/// The dome holes: their diameter and depth, and the directions they are drilled along,
/// outward (each 45° from +X).
pub const DOME_HOLE: (f64, f64) = (4.0, 6.0);
pub const DOME_HOLE_AXES: [[f64; 3]; 4] = {
    let h = std::f64::consts::FRAC_1_SQRT_2;
    [[h, 0.0, h], [h, 0.0, -h], [h, h, 0.0], [h, -h, 0.0]]
};

/// Where the dome hole along `axis` breaks the dome's surface.
pub fn dome_hole_mouth(axis: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| DOME_CENTRE[k] + DOME_RADIUS * axis[k])
}

/// Adds the Test piece to the Part Studio `el`.
pub fn test_piece(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    let me = [PIECE];
    let ids = |n: u128| (piece(n), piece(n + 1));
    // The block, the shaft and the spigot.
    solid(
        s,
        el,
        ids(0x11),
        On::Top,
        vec![rect(0.0, -25.0, 60.0, 25.0)],
        &[v(30.0, 0.0)],
        (-15.0, 15.0),
        New,
        &[],
    )?;
    for (n, d, x) in [
        (0x13, 2.0 * SHAFT_RADIUS, (60.0, DOME_CENTRE[0])),
        (0x15, 30.0, (-25.0, 0.0)),
    ] {
        solid(
            s,
            el,
            ids(n),
            On::Right,
            vec![circle(0.0, 0.0, d)],
            &[v(0.0, 0.0)],
            x,
            Add,
            &me,
        )?;
    }
    // The dome: a quarter disc on Front (sketch x = model X, y = model Z) turned about X.
    let (cx, r) = (DOME_CENTRE[0], DOME_RADIUS);
    turned(
        s,
        el,
        ids(0x17),
        On::Front,
        (v(cx - 10.0, 0.0), v(cx + r + 10.0, 0.0)),
        vec![
            SketchOp::AddArc {
                center: v(cx, 0.0),
                start: v(cx + r, 0.0),
                end: v(cx, r),
                construction: false,
            },
            SketchOp::AddPolyline {
                points: vec![v(cx, r), v(cx, 0.0), v(cx + r, 0.0)],
                closed: false,
                construction: false,
                label: "Add line",
            },
        ],
        v(cx + r / 3.0, r / 3.0),
        Add,
        &me,
    )?;
    // 3-axis: the pocket in the block's top and the two holes through it.
    let floor = POCKET_FLOOR;
    solid(
        s,
        el,
        ids(0x21),
        On::Top,
        vec![rect(floor[0] - 15.0, -10.0, floor[0] + 15.0, 10.0)],
        &[v(floor[0], floor[1])],
        (floor[2], 15.0),
        Remove,
        &me,
    )?;
    solid(
        s,
        el,
        ids(0x23),
        On::Top,
        HOLES.iter().map(|h| circle(h[0], h[1], 6.0)).collect(),
        &HOLES.map(|h| v(h[0], h[1])),
        (-15.0, 15.0),
        Remove,
        &me,
    )?;
    // 4th axis: the three flats, each cut by a bar across the shaft from 17 off its axis out.
    let angles = [90.0, 210.0, 330.0];
    let flat = TOP_FLAT;
    solid(
        s,
        el,
        ids(0x31),
        On::Right,
        angles
            .iter()
            .map(|a| radial_rect((0.0, 0.0), *a, flat[2], 25.0, 30.0))
            .collect(),
        &angles.map(|a| polar((0.0, 0.0), a, 21.0)),
        (flat[0] - 7.5, flat[0] + 7.5),
        Remove,
        &me,
    )?;
    // The groove: a helix about X (its axis a line on Top) that starts on top of the shaft,
    // and a Ø5 circle there on Front, swept along it and cut out.
    let (x0, x1) = GROOVE_X;
    let g = sketch(
        s,
        el,
        piece(0x33),
        On::Top,
        vec![SketchOp::AddPolyline {
            points: vec![v(x0, 0.0), v(x1, 0.0)],
            closed: false,
            construction: true,
            label: "Add line",
        }],
    )?;
    let line = g
        .curves
        .keys()
        .next()
        .ok_or_else(|| CommandError::Invalid("the groove's axis is missing".into()))?;
    let turns = (x1 - x0) / GROOVE_PITCH;
    s.run(&AddFeature {
        element: el,
        feature: piece(0x34),
        base_name: "Helix".into(),
        kind: FeatureKind::Helix(HelixFeature {
            helix_type: HelixType::Axis,
            axis: Some(AxisRef::SketchCurve {
                sketch: piece(0x33),
                curve: line,
            }),
            path: HelixPath::TurnsAndPitch,
            revolutions: turns,
            revolutions_expr: turns.to_string(),
            pitch: GROOVE_PITCH,
            pitch_expr: mm(GROOVE_PITCH),
            height: x1 - x0,
            height_expr: mm(x1 - x0),
            radius: SHAFT_RADIUS,
            radius_expr: mm(SHAFT_RADIUS),
            // From the helix's reference direction (+Y for an axis along X) round to +Z.
            start_angle: 90.0,
            start_angle_expr: "90 deg".into(),
            ..HelixFeature::default()
        }),
    })?;
    let g = sketch(
        s,
        el,
        piece(0x35),
        On::Front,
        vec![circle(x0, SHAFT_RADIUS, 5.0)],
    )?;
    let regions = crate::samples::region_refs(piece(0x35), &g, &[v(x0, SHAFT_RADIUS)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(
            "the groove's profile is missing".into(),
        ));
    }
    s.run(&AddFeature {
        element: el,
        feature: piece(0x36),
        base_name: "Sweep".into(),
        kind: FeatureKind::Sweep(SweepFeature {
            regions,
            path: vec![PathRef::Curve(piece(0x34))],
            op: Remove,
            merge_scope: me.to_vec(),
            ..SweepFeature::default()
        }),
    })?;
    // 5-axis: the dome holes. Each is half its section (a 2 × 7 strip from 6 under the dome's
    // surface to 1 over it, in the plane through X and its axis) turned about its axis: the
    // ones towards ±Z drawn on Front, the ones towards ±Y on Top.
    let (d, depth) = DOME_HOLE;
    for (k, axis) in DOME_HOLE_AXES.iter().enumerate() {
        let (on, up) = if axis[2] != 0.0 {
            (On::Front, axis[2])
        } else {
            (On::Top, axis[1])
        };
        // In the sketch: along the axis, and across it.
        let (a, n) = (v(axis[0], up), v(-up, axis[0]));
        let at = |r: f64, w: f64| v(cx + r * a.x + w * n.x, r * a.y + w * n.y);
        let (r0, r1) = (r - depth, r + 1.0);
        turned(
            s,
            el,
            ids(0x41 + 2 * k as u128),
            on,
            (at(r0 - 4.0, 0.0), at(r1 + 4.0, 0.0)),
            vec![SketchOp::AddPolyline {
                points: vec![at(r0, 0.0), at(r1, 0.0), at(r1, d / 2.0), at(r0, d / 2.0)],
                closed: true,
                construction: false,
                label: "Add polygon",
            }],
            at((r0 + r1) / 2.0, d / 4.0),
            Remove,
            &me,
        )?;
    }
    s.run(&RenamePart {
        element: el,
        part: PIECE,
        name: "Test piece".into(),
    })?;
    s.run(&SetPartAppearance {
        element: el,
        parts: vec![PIECE],
        appearance: Some(Appearance::rgb(204, 208, 214)),
    })
}

/// The worm: where it starts along X, its length, its module, the radii of its pitch cylinder,
/// of its thread's tips and of its root, and its lead (π × the module for one start).
pub const WORM_START: f64 = 200.0;
/// The worm's chucking spigot's length: long enough to hold it well out from the chuck, so
/// the spindle clears the jaws at the thread's start.
pub const WORM_SPIGOT: f64 = 75.0;
/// How far back from the thread the chuck's jaws grip the spigot.
pub const WORM_GRIP: f64 = 50.0;
pub const WORM_LENGTH: f64 = 50.0;
pub const WORM_MODULE: f64 = 2.0;
pub const WORM_PITCH_RADIUS: f64 = 10.0;
pub const WORM_TIP_RADIUS: f64 = WORM_PITCH_RADIUS + WORM_MODULE;
pub const WORM_ROOT_RADIUS: f64 = WORM_PITCH_RADIUS - 1.2 * WORM_MODULE;
pub const WORM_LEAD: f64 = std::f64::consts::PI * WORM_MODULE;
/// The helix the thread's space is cut along: how far before the blank's end its middle starts
/// (so the cut runs out of both ends) and how many turns it makes.
const WORM_CUT_LEAD_IN: f64 = 5.0;
const WORM_CUT_TURNS: f64 = 9.5;

/// Adds the Worm to the Part Studio `el`: a blank of the thread's tip diameter, the space
/// between the thread's turns cut out of it along a helix about X, then the spigot.
pub fn worm(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    let me = [WORM_PART];
    solid(
        s,
        el,
        (worm_id(0x11), worm_id(0x12)),
        On::Right,
        vec![circle(0.0, 0.0, 2.0 * WORM_TIP_RADIUS)],
        &[v(0.0, 0.0)],
        (WORM_START, WORM_START + WORM_LENGTH),
        New,
        &[],
    )?;
    // The helix: about X (its axis a line on Top), on the pitch cylinder, starting on top.
    let x0 = WORM_START - WORM_CUT_LEAD_IN;
    let height = WORM_CUT_TURNS * WORM_LEAD;
    let g = sketch(
        s,
        el,
        worm_id(0x13),
        On::Top,
        vec![SketchOp::AddPolyline {
            points: vec![v(x0, 0.0), v(x0 + height, 0.0)],
            closed: false,
            construction: true,
            label: "Add line",
        }],
    )?;
    let line = g
        .curves
        .keys()
        .next()
        .ok_or_else(|| CommandError::Invalid("the worm's axis is missing".into()))?;
    s.run(&AddFeature {
        element: el,
        feature: worm_id(0x14),
        base_name: "Helix".into(),
        kind: FeatureKind::Helix(HelixFeature {
            helix_type: HelixType::Axis,
            axis: Some(AxisRef::SketchCurve {
                sketch: worm_id(0x13),
                curve: line,
            }),
            path: HelixPath::TurnsAndPitch,
            revolutions: WORM_CUT_TURNS,
            revolutions_expr: WORM_CUT_TURNS.to_string(),
            // (In full: rounded to the micron they would no longer be the values.)
            pitch: WORM_LEAD,
            pitch_expr: format!("{WORM_LEAD} mm"),
            height,
            height_expr: format!("{height} mm"),
            radius: WORM_PITCH_RADIUS,
            radius_expr: mm(WORM_PITCH_RADIUS),
            start_angle: 90.0,
            start_angle_expr: "90 deg".into(),
            ..HelixFeature::default()
        }),
    })?;
    // The space's axial section on Front (sketch x = model X, y = model Z), over the helix's
    // start: half the lead wide on the pitch cylinder, its sides 20° from square, from the
    // root out to 1 past the tips.
    let slope = 20f64.to_radians().tan();
    let half = |r: f64| WORM_LEAD / 4.0 + (r - WORM_PITCH_RADIUS) * slope;
    let (root, out) = (WORM_ROOT_RADIUS, WORM_TIP_RADIUS + 1.0);
    let g = sketch(
        s,
        el,
        worm_id(0x15),
        On::Front,
        vec![super::poly(&[
            (x0 - half(root), root),
            (x0 + half(root), root),
            (x0 + half(out), out),
            (x0 - half(out), out),
        ])],
    )?;
    let regions = crate::samples::region_refs(worm_id(0x15), &g, &[v(x0, WORM_PITCH_RADIUS)]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid(
            "the worm's thread section is missing".into(),
        ));
    }
    s.run(&AddFeature {
        element: el,
        feature: worm_id(0x16),
        base_name: "Sweep".into(),
        kind: FeatureKind::Sweep(SweepFeature {
            regions,
            path: vec![PathRef::Curve(worm_id(0x14))],
            op: Remove,
            merge_scope: me.to_vec(),
            ..SweepFeature::default()
        }),
    })?;
    solid(
        s,
        el,
        (worm_id(0x17), worm_id(0x18)),
        On::Right,
        vec![circle(0.0, 0.0, 20.0)],
        &[v(0.0, 0.0)],
        (WORM_START - WORM_SPIGOT, WORM_START),
        Add,
        &me,
    )?;
    s.run(&RenamePart {
        element: el,
        part: WORM_PART,
        name: "Worm".into(),
    })?;
    s.run(&SetPartAppearance {
        element: el,
        parts: vec![WORM_PART],
        appearance: Some(Appearance::rgb(190, 150, 90)),
    })
}

/// Adds the Vise, the Chuck and the Fixture plate to the Part Studio `el`.
pub fn workholding(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New};
    // The vise: its base (open under the block), then the jaws either side of the block and
    // the ledges under its edges.
    solid(
        s,
        el,
        (hold(0x11), hold(0x12)),
        On::Top,
        vec![rect(-10.0, -60.0, 70.0, 60.0), rect(0.0, -19.0, 60.0, 19.0)],
        &[v(-5.0, 0.0)],
        (-45.0, -25.0),
        New,
        &[],
    )?;
    solid(
        s,
        el,
        (hold(0x13), hold(0x14)),
        On::Top,
        vec![rect(0.0, 25.0, 60.0, 45.0), rect(0.0, -45.0, 60.0, -25.0)],
        &[v(30.0, 35.0), v(30.0, -35.0)],
        (-25.0, 8.0),
        Add,
        &[VISE],
    )?;
    solid(
        s,
        el,
        (hold(0x15), hold(0x16)),
        On::Top,
        vec![rect(0.0, 19.0, 60.0, 25.0), rect(0.0, -25.0, 60.0, -19.0)],
        &[v(30.0, 22.0), v(30.0, -22.0)],
        (-25.0, -15.0),
        Add,
        &[VISE],
    )?;
    // The chuck, its jaws round the spigot and against the block's end.
    super::parts::chuck(
        s,
        el,
        [(hold(0x21), hold(0x22)), (hold(0x23), hold(0x24))],
        -25.0,
        (0.0, 0.0),
    )?;
    // The fixture plate: a ring on the spigot.
    solid(
        s,
        el,
        (hold(0x31), hold(0x32)),
        On::Right,
        vec![circle(0.0, 0.0, 120.0), circle(0.0, 0.0, 30.0)],
        &[v(40.0, 0.0)],
        (-25.0, 0.0),
        New,
        &[],
    )?;
    for (part, name, a) in [
        (VISE, "Vise", Appearance::rgb(58, 110, 165)),
        (CHUCK, "Chuck", Appearance::rgb(168, 172, 178)),
        (
            FIXTURE_PLATE,
            "Fixture plate",
            Appearance::rgb(112, 120, 130),
        ),
    ] {
        s.run(&RenamePart {
            element: el,
            part,
            name: name.into(),
        })?;
        s.run(&SetPartAppearance {
            element: el,
            parts: vec![part],
            appearance: Some(a),
        })?;
    }
    Ok(())
}
