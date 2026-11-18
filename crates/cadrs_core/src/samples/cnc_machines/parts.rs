//! The Part Studio "Machine parts": the parts of all three machines, each modelled where it
//! stands with every axis at home, so the assemblies' instances sit at the identity. The origin
//! is the centre of the table's top, Z up, the column behind it (+Y); mm.
//!
//! | Part | Shape | Extent |
//! |---|---|---|
//! | Base | box | x ±250, y −310…510, z −250…−110 |
//! | Column | box, on the back of the base | x ±150, y 310…510, z −110…850 |
//! | Saddle | box | x ±200, y ±150, z −110…−50 |
//! | Table | box with three T-slots along X in its top (12 wide at the top, 20 below, 10 deep, at y −90, 0, 90) | x ±300, y ±150, z −50…0 |
//! | Head (3- and 4-axis) | box, from the column's face out over the table | x ±110, y −110…310, z 500…800 |
//! | Spindle | Ø90 with a Ø60 nose under it, its axis at (0, 0) | z 420…500 and z 400…420 |
//! | Motor | Ø120 on top of the head | z 800…950 |
//! | Rotary body (4-axis) | box on the table's left | x −290…−140, y ±80, z 0…170 |
//! | Chuck (4-axis) | Ø125 with three jaws on its face, its axis along X at z 100 | x −140…−95, the jaws to x −70 |
//! | Magazine bracket | box out of the column's right side | x 150…400, y 320…400, z 560…620 |
//! | Carousel | Ø340 disc about (360, 360) with eight Ø50 pockets on a Ø260 circle, a Ø12 mark by the first pocket, and a Ø60 hub up to the bracket | z 500…530, the hub to z 560 |
//! | Rotary table base (5-axis) | Ø260 on the table | z 0…80 |
//! | Platter (5-axis) | Ø220 with four radial slots 12 wide and 10 deep and a Ø10 mark in its top | z 80…110 |
//! | Z slide (5-axis) | box on the column's face | x ±110, y 170…310, z 500…800 |
//! | Swivel head (5-axis) | box with a Ø180 pivot boss on its back, about Y at z 600 | x ±90, y −110…150 (the boss to 170), z 500…800 |
//!
//! The column stands 310 behind the table's centre (not 260): the table and the saddle are
//! 150 deep either side and travel 150 in Y, and must stay clear of it.

use crate::appearance::Appearance;
use crate::command::CommandError;
use crate::commands::{RenamePart, SetPartAppearance};
use crate::document::BooleanOp;
use crate::ids::{ElementId, FeatureId, PartId};
use crate::studio::Studio;

use super::{On, circle, polar, radial_rect, rect, solid, v};

const fn id(n: u128) -> FeatureId {
    super::feature(1, n)
}

pub const BASE: PartId = PartId::new(id(0x12), 0);
pub const COLUMN: PartId = PartId::new(id(0x22), 0);
pub const SADDLE: PartId = PartId::new(id(0x32), 0);
pub const TABLE: PartId = PartId::new(id(0x42), 0);
pub const HEAD: PartId = PartId::new(id(0x52), 0);
pub const SPINDLE: PartId = PartId::new(id(0x62), 0);
pub const MOTOR: PartId = PartId::new(id(0x72), 0);
pub const ROTARY_BODY: PartId = PartId::new(id(0x82), 0);
pub const CHUCK: PartId = PartId::new(id(0x92), 0);
pub const MAGAZINE_BRACKET: PartId = PartId::new(id(0xa2), 0);
pub const CAROUSEL: PartId = PartId::new(id(0xb2), 0);
pub const ROTARY_TABLE_BASE: PartId = PartId::new(id(0xc2), 0);
pub const PLATTER: PartId = PartId::new(id(0xd2), 0);
pub const Z_SLIDE: PartId = PartId::new(id(0xe2), 0);
pub const SWIVEL_HEAD: PartId = PartId::new(id(0xf2), 0);

/// The parts with their names, in the parts list's order.
pub const PARTS: [(PartId, &str); 15] = [
    (BASE, "Base"),
    (COLUMN, "Column"),
    (SADDLE, "Saddle"),
    (TABLE, "Table"),
    (HEAD, "Head"),
    (SPINDLE, "Spindle"),
    (MOTOR, "Motor"),
    (ROTARY_BODY, "Rotary body"),
    (CHUCK, "Chuck"),
    (MAGAZINE_BRACKET, "Magazine bracket"),
    (CAROUSEL, "Carousel"),
    (ROTARY_TABLE_BASE, "Rotary table base"),
    (PLATTER, "Platter"),
    (Z_SLIDE, "Z slide"),
    (SWIVEL_HEAD, "Swivel head"),
];

/// Where the column's face is (y): the head and the Z slide run on it.
pub const COLUMN_FACE: f64 = 310.0;
/// The spindle nose's end at home (z), on the axis x 0, y 0.
pub const SPINDLE_NOSE: f64 = 400.0;
/// The 4th axis: a point on the chuck's axis (along X) at its back face.
pub const CHUCK_AXIS: [f64; 3] = [-140.0, 0.0, 100.0];
/// The face of the 4th axis's jaws (x): a chucked part's shoulder sits here.
pub const CHUCK_JAW_FACE: f64 = -70.0;
/// The carousel's axis (along Z) at its underside.
pub const CAROUSEL_AXIS: [f64; 3] = [360.0, 360.0, 500.0];
/// The carousel's pockets and the circle their centres are on.
pub const POCKETS: usize = 8;
pub const POCKET_CIRCLE: f64 = 260.0;
/// The platter's top (z), where work sits on the 5-axis machine.
pub const PLATTER_TOP: f64 = 110.0;
/// The swivel head's pivot: about Y, through this point on the Z slide's face.
pub const HEAD_PIVOT: [f64; 3] = [0.0, 170.0, 600.0];

/// A three-jaw chuck with its axis along X through (`y`, `z`): a Ø125 body 45 long whose face
/// is at x = `face`, and three jaws 25 long on it, 18 wide, from 15 to 55 off the axis (so
/// they grip Ø30). Its sketches and extrudes are `ids` (body, then jaws).
pub(super) fn chuck(
    s: &mut dyn Studio,
    el: ElementId,
    ids: [(FeatureId, FeatureId); 2],
    face: f64,
    (y, z): (f64, f64),
) -> Result<(), CommandError> {
    let part = PartId::new(ids[0].1, 0);
    solid(
        s,
        el,
        ids[0],
        On::Right,
        vec![circle(y, z, 125.0)],
        &[v(y, z)],
        (face - 45.0, face),
        BooleanOp::New,
        &[],
    )?;
    let angles = [90.0, 210.0, 330.0];
    solid(
        s,
        el,
        ids[1],
        On::Right,
        angles
            .iter()
            .map(|a| radial_rect((y, z), *a, 15.0, 55.0, 18.0))
            .collect(),
        &angles.map(|a| polar((y, z), a, 35.0)),
        (face, face + 25.0),
        BooleanOp::Add,
        &[part],
    )
}

/// The table's section on the Right plane (y, z): 300 × 50 with three T-slots in its top.
fn table_section() -> Vec<(f64, f64)> {
    let mut p = vec![(-150.0, -50.0), (150.0, -50.0), (150.0, 0.0)];
    for c in [90.0, 0.0, -90.0] {
        p.extend([
            (c + 6.0, 0.0),
            (c + 6.0, -4.0),
            (c + 10.0, -4.0),
            (c + 10.0, -10.0),
            (c - 10.0, -10.0),
            (c - 10.0, -4.0),
            (c - 6.0, -4.0),
            (c - 6.0, 0.0),
        ]);
    }
    p.push((-150.0, 0.0));
    p
}

/// Adds the parts, their names and looks to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    use BooleanOp::{Add, New, Remove};
    // A box: its corners in x and y, and its range in z.
    let block = |s: &mut dyn Studio, n: u128, x: (f64, f64), y: (f64, f64), z: (f64, f64)| {
        solid(
            s,
            el,
            (id(n), id(n + 1)),
            On::Top,
            vec![rect(x.0, y.0, x.1, y.1)],
            &[v((x.0 + x.1) / 2.0, (y.0 + y.1) / 2.0)],
            z,
            New,
            &[],
        )
    };
    let face = COLUMN_FACE;
    block(
        s,
        0x11,
        (-250.0, 250.0),
        (-310.0, face + 200.0),
        (-250.0, -110.0),
    )?;
    block(
        s,
        0x21,
        (-150.0, 150.0),
        (face, face + 200.0),
        (-110.0, 850.0),
    )?;
    block(s, 0x31, (-200.0, 200.0), (-150.0, 150.0), (-110.0, -50.0))?;
    solid(
        s,
        el,
        (id(0x41), id(0x42)),
        On::Right,
        vec![super::poly(&table_section())],
        &[v(0.0, -30.0)],
        (-300.0, 300.0),
        New,
        &[],
    )?;
    block(s, 0x51, (-110.0, 110.0), (-110.0, face), (500.0, 800.0))?;
    // The spindle and its nose, the motor above them.
    let round = |s: &mut dyn Studio,
                 n: u128,
                 (x, y): (f64, f64),
                 d: f64,
                 z: (f64, f64),
                 op: BooleanOp,
                 scope: &[PartId]| {
        solid(
            s,
            el,
            (id(n), id(n + 1)),
            On::Top,
            vec![circle(x, y, d)],
            &[v(x, y)],
            z,
            op,
            scope,
        )
    };
    round(s, 0x61, (0.0, 0.0), 90.0, (420.0, 500.0), New, &[])?;
    round(
        s,
        0x63,
        (0.0, 0.0),
        60.0,
        (SPINDLE_NOSE, 420.0),
        Add,
        &[SPINDLE],
    )?;
    round(s, 0x71, (0.0, 0.0), 120.0, (800.0, 950.0), New, &[])?;
    // The 4th axis: its body on the table's left and the chuck on the body's face.
    block(s, 0x81, (-290.0, -140.0), (-80.0, 80.0), (0.0, 170.0))?;
    chuck(
        s,
        el,
        [(id(0x91), id(0x92)), (id(0x93), id(0x94))],
        CHUCK_JAW_FACE - 25.0,
        (CHUCK_AXIS[1], CHUCK_AXIS[2]),
    )?;
    // The tool magazine: a bracket out of the column's side, the carousel hung under it.
    let c = (CAROUSEL_AXIS[0], CAROUSEL_AXIS[1]);
    block(
        s,
        0xa1,
        (150.0, c.0 + 40.0),
        (c.1 - 40.0, c.1 + 40.0),
        (560.0, 620.0),
    )?;
    let mut disc = vec![circle(c.0, c.1, 340.0)];
    for k in 0..POCKETS {
        let p = polar(c, 360.0 / POCKETS as f64 * k as f64, POCKET_CIRCLE / 2.0);
        disc.push(circle(p.x, p.y, 50.0));
    }
    // A mark inward of the first pocket, so a turn of one pocket shows.
    let mark = polar(c, 0.0, 80.0);
    disc.push(circle(mark.x, mark.y, 12.0));
    solid(
        s,
        el,
        (id(0xb1), id(0xb2)),
        On::Top,
        disc,
        &[v(c.0, c.1)],
        (500.0, 530.0),
        New,
        &[],
    )?;
    round(s, 0xb3, c, 60.0, (530.0, 560.0), Add, &[CAROUSEL])?;
    // The 5-axis machine's rotary table, and its platter with slots and a mark in its top.
    round(s, 0xc1, (0.0, 0.0), 260.0, (0.0, 80.0), New, &[])?;
    round(s, 0xd1, (0.0, 0.0), 220.0, (80.0, PLATTER_TOP), New, &[])?;
    let quarters = [0.0, 90.0, 180.0, 270.0];
    let mut cuts: Vec<_> = quarters
        .iter()
        .map(|a| radial_rect((0.0, 0.0), *a, 30.0, 95.0, 12.0))
        .collect();
    let mark = polar((0.0, 0.0), 45.0, 80.0);
    cuts.push(circle(mark.x, mark.y, 10.0));
    let mut seeds: Vec<_> = quarters
        .iter()
        .map(|a| polar((0.0, 0.0), *a, 60.0))
        .collect();
    seeds.push(mark);
    solid(
        s,
        el,
        (id(0xd3), id(0xd4)),
        On::Top,
        cuts,
        &seeds,
        (PLATTER_TOP - 10.0, PLATTER_TOP),
        Remove,
        &[PLATTER],
    )?;
    // The 5-axis machine's head: a slide on the column and the head that tilts on it.
    let pivot = HEAD_PIVOT;
    block(s, 0xe1, (-110.0, 110.0), (pivot[1], face), (500.0, 800.0))?;
    block(
        s,
        0xf1,
        (-90.0, 90.0),
        (-110.0, pivot[1] - 20.0),
        (500.0, 800.0),
    )?;
    solid(
        s,
        el,
        (id(0xf3), id(0xf4)),
        On::Front,
        vec![circle(pivot[0], pivot[2], 180.0)],
        &[v(pivot[0], pivot[2])],
        (pivot[1] - 20.0, pivot[1]),
        Add,
        &[SWIVEL_HEAD],
    )?;
    for (part, name) in PARTS {
        s.run(&RenamePart {
            element: el,
            part,
            name: name.into(),
        })?;
    }
    for (parts, a) in [
        // The castings.
        (vec![BASE, COLUMN], Appearance::rgb(196, 200, 204)),
        (
            vec![SADDLE, ROTARY_BODY, ROTARY_TABLE_BASE, MAGAZINE_BRACKET],
            Appearance::rgb(112, 120, 130),
        ),
        // The ground steel the work sits on or is held in.
        (vec![TABLE, PLATTER, CHUCK], Appearance::rgb(168, 172, 178)),
        (
            vec![HEAD, Z_SLIDE, SWIVEL_HEAD],
            Appearance::rgb(58, 110, 165),
        ),
        (vec![SPINDLE], Appearance::rgb(70, 74, 80)),
        (vec![MOTOR], Appearance::rgb(226, 112, 28)),
        (vec![CAROUSEL], Appearance::rgb(236, 178, 46)),
    ] {
        s.run(&SetPartAppearance {
            element: el,
            parts,
            appearance: Some(a),
        })?;
    }
    Ok(())
}
