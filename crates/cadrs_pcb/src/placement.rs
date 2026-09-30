//! A component's placement on the board in 3D, and back.
//!
//! **Board frame**: IDF board coordinates in x and y (mm), the board's bottom face at z = 0 and
//! its top face at z = thickness.
//!
//! **Package frame**: the `.emp` outline in x and y, the body from z = 0 (the mounting face) up
//! to z = height.
//!
//! A TOP component is turned `rotation` degrees counter-clockwise about z and moved to
//! `(x, y, thickness + mount_offset)`. A BOTTOM component is first flipped about its own Y axis
//! (the IDF spec's flip: a half turn about y, which maps the outline by `diag(-1, 1)` and points
//! the body down), so it is `(x, y, -mount_offset) + RotY(180°) · Rz(rotation) · p`: in plan the
//! same map as [`cadrs_idf::Placement::place_loops`], with the body below the board. Both are
//! rigid motions (no mirror), so a component body is a moved copy of its package's body.

use cadrs_idf::{MountSide, Placement};
use cadrs_kernel::Motion;
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};

/// Where a component sits: the placement fields that geometry fixes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Degrees in [0, 360).
    pub rotation: f64,
    pub side: MountSide,
    pub mount_offset: f64,
}

impl Pose {
    pub fn of(p: &Placement) -> Pose {
        Pose { x: p.x, y: p.y, rotation: p.rotation, side: p.side, mount_offset: p.mount_offset }
    }

    /// Writes the pose into a placement (other fields kept).
    pub fn apply(&self, p: &mut Placement) {
        p.x = self.x;
        p.y = self.y;
        p.rotation = self.rotation;
        p.side = self.side;
        p.mount_offset = self.mount_offset;
    }
}

fn rz(deg: f64) -> Matrix3<f64> {
    let (s, c) = cadrs_idf::geom::sin_cos_deg(deg);
    Matrix3::new(c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0)
}

/// Half turn about y: (x, y, z) → (−x, y, −z).
fn flip() -> Matrix3<f64> {
    Matrix3::new(-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0)
}

/// The motion from the package frame to the board frame for a pose (see the module docs).
pub fn pose_motion(pose: &Pose, thickness: f64) -> Motion {
    match pose.side {
        MountSide::Top => Motion { linear: rz(pose.rotation), translation: Vector3::new(pose.x, pose.y, thickness + pose.mount_offset) },
        MountSide::Bottom => Motion { linear: flip() * rz(pose.rotation), translation: Vector3::new(pose.x, pose.y, -pose.mount_offset) },
    }
}

/// The motion from the package frame to the board frame for a placement (lengths in mm).
pub fn placement_motion(p: &Placement, thickness: f64) -> Motion {
    pose_motion(&Pose::of(p), thickness)
}

/// The motion from a custom part's frame to the board frame (P3H.4, X10): the library's
/// transform into the package frame, then the placement's.
pub fn custom_motion(p: &Placement, thickness: f64, t: &cadrs_core::pcb::PartTransform) -> Motion {
    t.motion().then(&placement_motion(p, thickness))
}

/// Degrees normalised to [0, 360), with values within 1e-9° of 360 read as 0.
pub fn normalize_deg(d: f64) -> f64 {
    let r = d.rem_euclid(360.0);
    if (360.0 - r).abs() < 1e-9 { 0.0 } else { r }
}

/// The pose a board-frame motion puts a component at: the exact inverse of [`pose_motion`].
/// The package's z axis pointing up means TOP, down BOTTOM. Tilted components (neither, within
/// 1e-6) are an error: IDF has no tilt.
pub fn pose_from_motion(m: &Motion, thickness: f64) -> Result<Pose, String> {
    if m.is_reflection() {
        return Err("a mirrored component can't be placed".into());
    }
    let z = m.linear.column(2);
    let l = &m.linear;
    let t = &m.translation;
    if (z.z - 1.0).abs() < 1e-6 {
        let rot = l[(1, 0)].atan2(l[(0, 0)]).to_degrees();
        Ok(Pose { x: t.x, y: t.y, rotation: normalize_deg(rot), side: MountSide::Top, mount_offset: t.z - thickness })
    } else if (z.z + 1.0).abs() < 1e-6 {
        // linear = flip · Rz: first column (−c, s, 0).
        let rot = l[(1, 0)].atan2(-l[(0, 0)]).to_degrees();
        Ok(Pose { x: t.x, y: t.y, rotation: normalize_deg(rot), side: MountSide::Bottom, mount_offset: -t.z })
    } else {
        Err("the component is tilted: its top isn't parallel to the board".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Point3;

    #[test]
    fn placement_inverse() {
        let t = 1.6;
        for side in [MountSide::Top, MountSide::Bottom] {
            for rot in [0.0, 12.5, 90.0, 180.0, 270.0, 359.25] {
                let pose = Pose { x: 12.345, y: -6.789, rotation: rot, side, mount_offset: 0.25 };
                let back = pose_from_motion(&pose_motion(&pose, t), t).unwrap();
                assert_eq!(back.side, side);
                for (a, b) in [(back.x, pose.x), (back.y, pose.y), (back.rotation, pose.rotation), (back.mount_offset, pose.mount_offset)] {
                    assert!((a - b).abs() < 1e-9, "{pose:?} → {back:?}");
                }
            }
        }
        // Rotations outside [0, 360) come back normalised.
        let pose = Pose { x: 0.0, y: 0.0, rotation: -90.0, side: MountSide::Top, mount_offset: 0.0 };
        assert!((pose_from_motion(&pose_motion(&pose, t), t).unwrap().rotation - 270.0).abs() < 1e-9);
    }

    #[test]
    fn custom_part_transform_composes_with_the_placement() {
        // The library maps the part by rotate 90° about Z, then translate (1, 2, 0); the
        // component sits at (10, 20), rotation 90, TOP, on a 1.6 mm board.
        // Part point (1, 0, 0): rotate → (0, 1, 0); translate → (1, 3, 0) in the package frame;
        // the placement turns it 90° → (−3, 1, 0) and moves it to (10, 20, 1.6) → (7, 21, 1.6).
        // Part point (0, 0, 1): → (0, 0, 1) → (1, 2, 1) → (−2, 1, 1) → (8, 21, 2.6).
        let p = Placement {
            package: "QFP100_600MIL".into(),
            part_number: "VPU-7100".into(),
            refdes: "U1".into(),
            x: 10.0,
            y: 20.0,
            mount_offset: 0.0,
            rotation: 90.0,
            side: MountSide::Top,
            status: cadrs_idf::Status::Placed,
        };
        let t = cadrs_core::pcb::PartTransform { translate: [1.0, 2.0, 0.0], rotate: [0.0, 0.0, 90.0] };
        let m = custom_motion(&p, 1.6, &t);
        for (q, want) in [(Point3::new(1.0, 0.0, 0.0), Point3::new(7.0, 21.0, 1.6)), (Point3::new(0.0, 0.0, 1.0), Point3::new(8.0, 21.0, 2.6))] {
            let got = m.point(&q);
            assert!((got - want).norm() < 1e-12, "{q} → {got}, want {want}");
        }
    }

    #[test]
    fn bottom_matches_idf_plan_map() {
        // A package point (1, 0) at rotation 90 on the bottom: IDF gives (x, y) + M·R·p =
        // (x, y) + diag(−1, 1)·(0, 1) = (x, y + 1).
        let pose = Pose { x: 5.0, y: 5.0, rotation: 90.0, side: MountSide::Bottom, mount_offset: 0.0 };
        let q = pose_motion(&pose, 1.0).point(&Point3::new(1.0, 0.0, 2.0));
        assert!((q - Point3::new(5.0, 6.0, -2.0)).norm() < 1e-12, "{q}");
    }
}
