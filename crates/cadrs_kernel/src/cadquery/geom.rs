//! CadQuery's geometry types: vectors (nalgebra's [`V3`]), [`Plane`], [`Location`] and
//! [`BoundBox`] (`cadquery/occ_impl/geom.py`).

use super::{CqResult, value_err};
use crate::native::math::{Affine, P3, V3};
use nalgebra::{Matrix3, Rotation3, Unit};

/// A point as CadQuery's `Vector` holds it.
pub fn p(v: V3) -> P3 {
    P3::from(v)
}

/// OCCT's x direction for a frame of normal `n` alone (`gp_Ax2(P, N)`): perpendicular to `n`
/// with the coordinate of `n`'s smallest component zero.
pub fn default_x(n: &V3) -> V3 {
    let (a, b, c) = (n.x, n.y, n.z);
    let (aa, ba, ca) = (a.abs(), b.abs(), c.abs());
    let x = if ba <= aa && ba <= ca {
        if aa > ca {
            V3::new(-c, 0.0, a)
        } else {
            V3::new(c, 0.0, -a)
        }
    } else if aa <= ba && aa <= ca {
        if ba > ca {
            V3::new(0.0, -c, b)
        } else {
            V3::new(0.0, c, -b)
        }
    } else if aa > ba {
        V3::new(-b, a, 0.0)
    } else {
        V3::new(b, -a, 0.0)
    };
    x.normalize()
}

/// The x axis of OCCT's `gp_Ax3(origin, z, x)`: `x` made perpendicular to `z` (or OCCT's
/// default where it is parallel).
fn ax3_x(z: &V3, x: &V3) -> V3 {
    let px = x - z * x.dot(z);
    if px.norm() < 1e-12 {
        default_x(z)
    } else {
        px.normalize()
    }
}

/// A work plane: an origin and orthonormal axes (`cadquery.Plane`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// The origin.
    pub origin: V3,
    /// The x direction, as given (unit; CadQuery doesn't make it perpendicular to `z_dir`).
    pub x_dir: V3,
    /// `z_dir × x_dir`, unit.
    pub y_dir: V3,
    /// The normal, unit.
    pub z_dir: V3,
}

impl Plane {
    /// `Plane(origin, xDir, normal)`; OCCT's default x direction when `x_dir` is None.
    pub fn new(origin: V3, x_dir: Option<V3>, normal: V3) -> CqResult<Self> {
        if normal.norm() == 0.0 {
            return value_err!("normal should be non null");
        }
        let z = normal.normalize();
        let x = match x_dir {
            None => default_x(&z),
            Some(x) if x.norm() == 0.0 => return value_err!("xDir should be non null"),
            Some(x) => x,
        };
        Ok(Self::of_axes(origin, x, z))
    }

    /// The plane of unit normal `z` with x direction `x` (normalized here).
    pub fn of_axes(origin: V3, x: V3, z: V3) -> Self {
        let x = x.normalize();
        let y = z.cross(&x).normalize();
        Self {
            origin,
            x_dir: x,
            y_dir: y,
            z_dir: z,
        }
    }

    /// The named planes (`Plane.named`).
    pub fn named(name: &str, origin: V3) -> CqResult<Self> {
        let (x, z) = match name {
            "XY" | "front" => ((1.0, 0.0, 0.0), (0.0, 0.0, 1.0)),
            "YZ" => ((0.0, 1.0, 0.0), (1.0, 0.0, 0.0)),
            "ZX" => ((0.0, 0.0, 1.0), (0.0, 1.0, 0.0)),
            "XZ" => ((1.0, 0.0, 0.0), (0.0, -1.0, 0.0)),
            "YX" => ((0.0, 1.0, 0.0), (0.0, 0.0, -1.0)),
            "ZY" => ((0.0, 0.0, 1.0), (-1.0, 0.0, 0.0)),
            "back" => ((-1.0, 0.0, 0.0), (0.0, 0.0, -1.0)),
            "left" => ((0.0, 0.0, 1.0), (-1.0, 0.0, 0.0)),
            "right" => ((0.0, 0.0, -1.0), (1.0, 0.0, 0.0)),
            "top" => ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
            "bottom" => ((1.0, 0.0, 0.0), (0.0, -1.0, 0.0)),
            _ => {
                return value_err!(
                    "Supported names are ['XY', 'YZ', 'ZX', 'XZ', 'YX', 'ZY', 'front', 'back', 'left', 'right', 'top', 'bottom']"
                );
            }
        };
        Ok(Self::of_axes(
            origin,
            V3::new(x.0, x.1, x.2),
            V3::new(z.0, z.1, z.2),
        ))
    }

    /// The plane of a location (`Plane(loc)`): its rotation's x and z axes at its translation.
    pub fn of_location(loc: &Location) -> Self {
        let (t, r) = loc.to_tuple();
        let m = euler_xyz(r.x.to_radians(), r.y.to_radians(), r.z.to_radians());
        Self::of_axes(t, m * V3::x(), m * V3::z())
    }

    /// The frame's axes as OCCT builds it (`gp_Ax3(origin, z, x)`).
    pub fn frame(&self) -> (V3, V3, V3) {
        let x = ax3_x(&self.z_dir, &self.x_dir);
        (x, self.z_dir.cross(&x), self.z_dir)
    }

    /// The map from plane coordinates to model ones (`rG`).
    pub fn to_world_map(&self) -> Affine {
        let (x, y, z) = self.frame();
        Affine {
            m: Matrix3::from_columns(&[x, y, z]),
            t: self.origin,
        }
    }

    /// The map from model coordinates to plane ones (`fG`).
    pub fn to_local_map(&self) -> Affine {
        let (x, y, z) = self.frame();
        let m = Matrix3::from_columns(&[x, y, z]).transpose();
        Affine {
            m,
            t: -(m * self.origin),
        }
    }

    /// `toWorldCoords`.
    pub fn to_world(&self, v: V3) -> V3 {
        let (x, y, z) = self.frame();
        self.origin + x * v.x + y * v.y + z * v.z
    }

    /// `toLocalCoords` of a point.
    pub fn to_local(&self, v: V3) -> V3 {
        let (x, y, z) = self.frame();
        let d = v - self.origin;
        V3::new(d.dot(&x), d.dot(&y), d.dot(&z))
    }

    /// `setOrigin2d`.
    pub fn set_origin_2d(&mut self, x: f64, y: f64) {
        self.origin = self.to_world(V3::new(x, y, 0.0));
    }

    /// `rotated((rx, ry, rz))`: turned about its own x, then y, then z axis (degrees), applied
    /// as one rotation `Rx·Ry·Rz` of the axes about the model origin.
    pub fn rotated(&self, deg: V3) -> CqResult<Self> {
        let r = deg * (std::f64::consts::PI / 180.0);
        let rot = |axis: &V3, a: f64| Rotation3::from_axis_angle(&Unit::new_normalize(*axis), a);
        let t = rot(&self.x_dir, r.x) * rot(&self.y_dir, r.y) * rot(&self.z_dir, r.z);
        Plane::new(self.origin, Some(t * self.x_dir), t * self.z_dir)
    }

    /// The location of the plane's frame (`plane.location`).
    pub fn location(&self) -> Location {
        Location(self.to_world_map())
    }

    /// CadQuery's plane equality: origins within 1e-6, axes within 1e-6 of parallel.
    pub fn same(&self, o: &Plane) -> bool {
        (self.origin - o.origin).norm() < 1e-6
            && (self.z_dir.dot(&o.z_dir) - 1.0).abs() < 1e-6
            && (self.x_dir.dot(&o.x_dir) - 1.0).abs() < 1e-6
    }
}

/// OCCT's extrinsic XYZ Euler rotation: about the model x, then y, then z axes.
pub fn euler_xyz(rx: f64, ry: f64, rz: f64) -> Matrix3<f64> {
    let r = |axis: V3, a: f64| *Rotation3::from_axis_angle(&Unit::new_normalize(axis), a).matrix();
    r(V3::z(), rz) * r(V3::y(), ry) * r(V3::x(), rx)
}

/// The extrinsic XYZ Euler angles (radians) of a rotation: `m = Rz·Ry·Rx`.
pub fn euler_of(m: &Matrix3<f64>) -> V3 {
    // m[2][0] = -sin(ry).
    let sy = (-m[(2, 0)]).clamp(-1.0, 1.0);
    let ry = sy.asin();
    if sy.abs() < 1.0 - 1e-12 {
        let rx = m[(2, 1)].atan2(m[(2, 2)]);
        let rz = m[(1, 0)].atan2(m[(0, 0)]);
        V3::new(rx, ry, rz)
    } else {
        // Gimbal lock: put the turn in rx.
        let rx = (-m[(1, 2)]).atan2(m[(1, 1)]);
        V3::new(rx, ry, 0.0)
    }
}

/// A placement: a rigid motion (`cadquery.Location`, OCCT's `TopLoc_Location`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location(pub Affine);

impl Default for Location {
    fn default() -> Self {
        Self(Affine::identity())
    }
}

impl Location {
    /// A translation.
    pub fn translation(t: V3) -> Self {
        Self(Affine::translation(t))
    }

    /// `Location(x, y, z, rx, ry, rz)`: extrinsic XYZ angles in degrees, then the translation.
    pub fn from_xyz_angles(t: V3, deg: V3) -> Self {
        Self(Affine {
            m: euler_xyz(deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians()),
            t,
        })
    }

    /// `Location(t, axis, angle)`: a turn of `deg` about `axis` through the origin, then `t`.
    pub fn from_axis_angle(t: V3, axis: V3, deg: f64) -> Self {
        let r = Rotation3::from_axis_angle(&Unit::new_normalize(axis), deg.to_radians());
        Self(Affine { m: *r.matrix(), t })
    }

    /// `Location(plane)`: from the plane's coordinates to the model's.
    pub fn of_plane(p: &Plane) -> Self {
        p.location()
    }

    /// `Location(plane, origin)`.
    pub fn of_plane_at(p: &Plane, origin: V3) -> Self {
        let mut q = *p;
        q.origin = origin;
        q.location()
    }

    /// `loc.inverse`.
    pub fn inverse(&self) -> Self {
        let mi = self.0.m.try_inverse().unwrap_or_else(Matrix3::identity);
        Self(Affine {
            m: mi,
            t: -(mi * self.0.t),
        })
    }

    /// `self * other`: `other` first, then `self`.
    pub fn then_after(&self, other: &Location) -> Self {
        Self(Affine {
            m: self.0.m * other.0.m,
            t: self.0.m * other.0.t + self.0.t,
        })
    }

    /// `loc ** n`.
    pub fn pow(&self, n: i32) -> Self {
        let base = if n < 0 { self.inverse() } else { *self };
        let mut r = Location::default();
        for _ in 0..n.unsigned_abs() {
            r = r.then_after(&base);
        }
        r
    }

    /// `toTuple()`: the translation, and the extrinsic XYZ angles in degrees.
    pub fn to_tuple(&self) -> (V3, V3) {
        let e = euler_of(&self.0.m);
        (
            self.0.t,
            V3::new(e.x.to_degrees(), e.y.to_degrees(), e.z.to_degrees()),
        )
    }

    /// The image of a point.
    pub fn apply(&self, v: V3) -> V3 {
        self.0.m * v + self.0.t
    }
}

/// An axis-aligned box (`cadquery.BoundBox`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundBox {
    /// The least corner.
    pub min: V3,
    /// The greatest corner.
    pub max: V3,
}

impl BoundBox {
    /// The box of points.
    pub fn of_points<'a>(pts: impl IntoIterator<Item = &'a V3>) -> Option<Self> {
        let mut it = pts.into_iter();
        let first = *it.next()?;
        let mut b = Self {
            min: first,
            max: first,
        };
        for q in it {
            b.min = b.min.inf(q);
            b.max = b.max.sup(q);
        }
        Some(b)
    }

    /// The box of both.
    pub fn union(&self, o: &BoundBox) -> BoundBox {
        BoundBox {
            min: self.min.inf(&o.min),
            max: self.max.sup(&o.max),
        }
    }

    /// `center`.
    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }

    /// `DiagonalLength`.
    pub fn diagonal(&self) -> f64 {
        (self.max - self.min).norm()
    }

    /// `enlarge(tol)`.
    pub fn enlarged(&self, tol: f64) -> BoundBox {
        let d = V3::repeat(tol);
        BoundBox {
            min: self.min - d,
            max: self.max + d,
        }
    }

    /// `isInside(b2)`: `b2` strictly inside this one.
    pub fn is_inside(&self, b2: &BoundBox) -> bool {
        b2.min.x > self.min.x
            && b2.min.y > self.min.y
            && b2.min.z > self.min.z
            && b2.max.x < self.max.x
            && b2.max.y < self.max.y
            && b2.max.z < self.max.z
    }
}
