//! Sample machines of each kind, for tests here and in the crates above (their own tests
//! check a post or a simulation on every arrangement of axes).

use super::{Chain, Joint};
use crate::geom::{Frame, P3, normalize};

const X: P3 = [1.0, 0.0, 0.0];
const Y: P3 = [0.0, 1.0, 0.0];
const Z: P3 = [0.0, 0.0, 1.0];

/// A slide named `name` along `dir`, 500 mm either way.
pub fn lin(name: &str, dir: P3) -> Joint {
    Joint::linear(name, dir, -500.0, 500.0)
}

/// A vertical mill whose table moves in X and Y under a head that moves in Z, with a trunnion
/// on the table: A about a line along X below the platter, C about the platter's own axis,
/// which misses A's line by 10 mm (the two don't meet).
pub fn trunnion_ac() -> Chain {
    Chain {
        tool: vec![lin("Z", Z)],
        table: vec![
            lin("X", [-1.0, 0.0, 0.0]),
            lin("Y", [0.0, -1.0, 0.0]),
            Joint::rotary("A", [0.0, 0.0, -60.0], X, -120.0, 120.0),
            Joint::rotary("C", [0.0, 10.0, 0.0], Z, 0.0, 0.0).endless(),
        ],
        tool_mount: Frame::from_zx([0.0, 0.0, 300.0], Z, X),
        table_mount: Frame::from_zx([0.0, 10.0, 0.0], Z, X),
    }
}

/// The same with a B trunnion (about Y).
pub fn trunnion_bc() -> Chain {
    let mut c = trunnion_ac();
    c.table[2] = Joint::rotary("B", [0.0, 0.0, -60.0], Y, -120.0, 120.0);
    c
}

/// A swivel head (B about Y, 150 mm above the gauge point) over a rotary table (C).
pub fn head_table() -> Chain {
    Chain {
        tool: vec![
            lin("X", X),
            lin("Z", Z),
            Joint::rotary("B", [0.0, 0.0, 450.0], Y, -110.0, 110.0),
        ],
        table: vec![
            lin("Y", [0.0, -1.0, 0.0]),
            Joint::rotary("C", [0.0, 0.0, 0.0], Z, 0.0, 0.0).endless(),
        ],
        tool_mount: Frame::from_zx([0.0, 0.0, 300.0], Z, X),
        table_mount: Frame::WORLD,
    }
}

/// A gantry with a fork head: C about the vertical, then B about a line square to it.
pub fn head_head() -> Chain {
    Chain {
        tool: vec![
            lin("X", X),
            lin("Y", Y),
            lin("Z", Z),
            Joint::rotary("C", [0.0, 0.0, 0.0], Z, -360.0, 360.0),
            Joint::rotary("B", [0.0, 0.0, 420.0], Y, -100.0, 100.0),
        ],
        table: vec![],
        tool_mount: Frame::from_zx([0.0, 0.0, 300.0], Z, X),
        table_mount: Frame::WORLD,
    }
}

/// A table whose first rotary leans at 45° (a nutating table): it reaches a tilt of 90°.
pub fn nutating() -> Chain {
    let lean = normalize([0.0, 1.0, 1.0]);
    Chain {
        tool: vec![lin("Z", Z)],
        table: vec![
            lin("X", [-1.0, 0.0, 0.0]),
            lin("Y", [0.0, -1.0, 0.0]),
            Joint::rotary("B", [0.0, 0.0, 0.0], lean, -180.0, 180.0),
            Joint::rotary("C", [0.0, 0.0, 0.0], Z, 0.0, 0.0).endless(),
        ],
        tool_mount: Frame::from_zx([0.0, 0.0, 300.0], Z, X),
        table_mount: Frame::WORLD,
    }
}

/// A router with a rotary axis along X on its bed.
pub fn fourth_axis() -> Chain {
    Chain {
        tool: vec![lin("X", X), lin("Y", Y), lin("Z", Z)],
        table: vec![Joint::rotary("A", [0.0, 0.0, 50.0], X, 0.0, 0.0).endless()],
        tool_mount: Frame::from_zx([0.0, 0.0, 200.0], Z, X),
        table_mount: Frame::from_zx([0.0, 0.0, 50.0], Z, X),
    }
}

/// An arm of six turns, laid out like the usual six-axis robot (shoulder, elbow, a wrist of
/// three axes meeting at a point), its flange looking along +X at home.
pub fn arm() -> Chain {
    Chain {
        tool: vec![
            Joint::rotary("J1", [0.0, 0.0, 0.0], Z, -170.0, 170.0),
            Joint::rotary("J2", [60.0, 0.0, 170.0], Y, -90.0, 90.0),
            Joint::rotary("J3", [60.0, 0.0, 475.0], Y, -140.0, 60.0),
            Joint::rotary("J4", [60.0, 0.0, 475.0], X, -165.0, 165.0),
            Joint::rotary("J5", [280.0, 0.0, 475.0], Y, -105.0, 105.0),
            Joint::rotary("J6", [280.0, 0.0, 475.0], X, -155.0, 155.0),
        ],
        table: vec![],
        // The spindle on the flange cuts with its tool pointing down at home.
        tool_mount: Frame::from_zx([340.0, 0.0, 475.0], Z, X),
        table_mount: Frame::from_zx([350.0, 0.0, 250.0], Z, X),
    }
}

/// Every sample mill with its name.
pub fn mills() -> Vec<(&'static str, Chain)> {
    vec![
        ("trunnion A/C", trunnion_ac()),
        ("trunnion B/C", trunnion_bc()),
        ("head B, table C", head_table()),
        ("fork head C/B", head_head()),
        ("nutating table", nutating()),
        ("rotary A", fourth_axis()),
    ]
}
