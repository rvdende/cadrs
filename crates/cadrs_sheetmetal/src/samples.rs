//! Small sheet metal models for tests, previews and scenarios: the shapes the course starts
//! with (an L, a U-channel, an open box with ripped corners, a partial flange, a hem, a rolled
//! tube) plus two that must fail (overlapping walls, bends closing a loop).

use std::f64::consts::PI;

use crate::model::{BuildError, HemAlignment, Joint, JointId, JointKind, Model, P3, RipStyle, SharpBuilder, Surface, V3, Wall, WallId};
use crate::params::Params;
use crate::poly::{P2, Polygon, Seg2};

fn rect(w: f64, h: f64) -> Polygon {
    Polygon::rect(P2::new(0.0, 0.0), P2::new(w, h))
}

/// A 50 × 40 base in the XY plane (material above) with a 30 high flange on its x = 50 edge,
/// bent up (towards the material) or down.
pub fn l_bracket(p: Params, up: bool) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let dir = if up { V3::z() } else { -V3::z() };
    let f = b.wall(P3::new(50.0, 0.0, 0.0), dir, V3::y(), rect(30.0, 40.0));
    b.bend(a, f, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 40.0, 0.0)));
    b.build()
}

/// A 60 × 40 base with 25 high walls bent up on both long edges.
pub fn u_channel(p: Params) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(60.0, 40.0));
    let right = b.wall(P3::new(60.0, 0.0, 0.0), V3::z(), V3::y(), rect(25.0, 40.0));
    let left = b.wall(P3::new(0.0, 40.0, 0.0), V3::z(), -V3::y(), rect(25.0, 40.0));
    b.bend(base, right, (P3::new(60.0, 0.0, 0.0), P3::new(60.0, 40.0, 0.0)));
    b.bend(base, left, (P3::new(0.0, 0.0, 0.0), P3::new(0.0, 40.0, 0.0)));
    b.build()
}

/// An open box: a 100 × 60 base with 20 high walls bent up on all four sides, ripped at the
/// corners with `style`.
pub fn open_box(p: Params, style: RipStyle) -> Result<Model, BuildError> {
    let (x, y, h) = (100.0, 60.0, 20.0);
    let mut b = SharpBuilder::new(p);
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(x, y));
    let east = b.wall(P3::new(x, 0.0, 0.0), V3::z(), V3::y(), rect(h, y));
    let north = b.wall(P3::new(x, y, 0.0), V3::z(), -V3::x(), rect(h, x));
    let west = b.wall(P3::new(0.0, y, 0.0), V3::z(), -V3::y(), rect(h, y));
    let south = b.wall(P3::new(0.0, 0.0, 0.0), V3::z(), V3::x(), rect(h, x));
    b.bend(base, east, (P3::new(x, 0.0, 0.0), P3::new(x, y, 0.0)));
    b.bend(base, north, (P3::new(x, y, 0.0), P3::new(0.0, y, 0.0)));
    b.bend(base, west, (P3::new(0.0, y, 0.0), P3::new(0.0, 0.0, 0.0)));
    b.bend(base, south, (P3::new(0.0, 0.0, 0.0), P3::new(x, 0.0, 0.0)));
    b.rip(east, north, (P3::new(x, y, 0.0), P3::new(x, y, h)), style);
    b.rip(north, west, (P3::new(0.0, y, 0.0), P3::new(0.0, y, h)), style);
    b.rip(west, south, (P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, h)), style);
    b.rip(south, east, (P3::new(x, 0.0, 0.0), P3::new(x, 0.0, h)), style);
    b.build()
}

/// A 50 × 40 base with a 30 high flange covering only y 10..30 of its x = 50 edge: the base
/// carries on past both ends of the bend, so both ends get bend reliefs.
pub fn partial_flange(p: Params) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 40.0));
    let f = b.wall(P3::new(50.0, 10.0, 0.0), V3::z(), V3::y(), rect(30.0, 20.0));
    b.bend(a, f, (P3::new(50.0, 10.0, 0.0), P3::new(50.0, 30.0, 0.0)));
    b.build()
}

/// A 50 × 20 wall with a 10 long hem folded back 180° over its material side at its x = 50 edge
/// (the hem's bend starting at the edge: In place).
pub fn hem(p: Params) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let a = b.wall(P3::origin(), V3::x(), V3::y(), rect(50.0, 20.0));
    b.hem(a, (P3::new(50.0, 0.0, 0.0), P3::new(50.0, 20.0, 0.0)), 10.0, true, HemAlignment::InPlace);
    b.build()
}

/// A rolled wall: a piece of a cylinder of definition radius `radius` (the inner face) about
/// the z axis, `arc` long on that face and `height` tall.
pub fn rolled_wall(id: u32, radius: f64, arc: f64, height: f64) -> Wall {
    Wall {
        id: WallId(id),
        surface: Surface::Rolled {
            axis_origin: P3::origin(),
            axis: V3::z(),
            start: V3::x(),
            radius,
            material_outside: true,
        },
        outline: rect(arc, height),
    }
}

/// A full tube of inner radius 10, 30 tall, split along its seam.
pub fn tube(p: Params) -> Model {
    Model {
        params: p,
        walls: vec![rolled_wall(0, 10.0, 2.0 * PI * 10.0, 30.0)],
        ..Default::default()
    }
}

/// A 20 × 30 flat wall (XY plane, material above) running smoothly into a half cylinder of inner
/// radius 10 that curls down from its x = 20 edge.
pub fn wall_into_half_tube(p: Params) -> Model {
    Model {
        params: p,
        walls: vec![
            Wall {
                id: WallId(0),
                surface: Surface::Planar {
                    origin: P3::origin(),
                    u: V3::x(),
                    v: V3::y(),
                },
                outline: rect(20.0, 30.0),
            },
            Wall {
                id: WallId(1),
                surface: Surface::Rolled {
                    axis_origin: P3::new(20.0, 0.0, -10.0),
                    axis: V3::y(),
                    start: V3::z(),
                    radius: 10.0,
                    material_outside: true,
                },
                outline: rect(PI * 10.0, 30.0),
            },
        ],
        joints: vec![Joint {
            id: JointId(0),
            name: "Tangent 1".into(),
            a: WallId(0),
            b: WallId(1),
            kind: JointKind::Tangent {
                on_a: Seg2::new(P2::new(20.0, 0.0), P2::new(20.0, 30.0)),
                on_b: Seg2::new(P2::new(0.0, 0.0), P2::new(0.0, 30.0)),
            },
        }],
        ..Default::default()
    }
}

/// Must fail: a 10 × 10 base with 30 high walls bent up on its x = 10 and y = 10 edges; the
/// second wall has a hook that reaches round below the base, so flat it lands on the first wall.
pub fn hook_collision(p: Params) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(10.0, 10.0));
    let east = b.wall(P3::new(10.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 10.0));
    // Plane y = 10, local (height, 10 − x): material towards −y.
    let hook = Polygon::new(vec![
        P2::new(0.0, 0.0),
        P2::new(0.0, 10.0),
        P2::new(20.0, 10.0),
        P2::new(20.0, -25.0),
        P2::new(-8.0, -25.0),
        P2::new(-8.0, -15.0),
        P2::new(2.0, -15.0),
        P2::new(2.0, 0.0),
    ]);
    let north = b.wall(P3::new(10.0, 10.0, 0.0), V3::z(), -V3::x(), hook);
    b.bend(base, east, (P3::new(10.0, 0.0, 0.0), P3::new(10.0, 10.0, 0.0)));
    b.bend(base, north, (P3::new(10.0, 10.0, 0.0), P3::new(0.0, 10.0, 0.0)));
    b.build()
}

/// Must fail: two walls bent up from a corner of a base and bent to each other as well.
pub fn bend_loop(p: Params) -> Result<Model, BuildError> {
    let mut b = SharpBuilder::new(p);
    let base = b.wall(P3::origin(), V3::x(), V3::y(), rect(40.0, 40.0));
    let east = b.wall(P3::new(40.0, 0.0, 0.0), V3::z(), V3::y(), rect(30.0, 40.0));
    let north = b.wall(P3::new(40.0, 40.0, 0.0), V3::z(), -V3::x(), rect(30.0, 40.0));
    b.bend(base, east, (P3::new(40.0, 0.0, 0.0), P3::new(40.0, 40.0, 0.0)));
    b.bend(base, north, (P3::new(40.0, 40.0, 0.0), P3::new(0.0, 40.0, 0.0)));
    b.bend(east, north, (P3::new(40.0, 40.0, 0.0), P3::new(40.0, 40.0, 30.0)));
    b.build()
}
