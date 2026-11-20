//! brep-conformance's `Kernel` over cadrs's: the `cadrs-adapter` binary serves it as the
//! protocol on stdin and stdout, and `examples/suite.rs` runs the suite's cases on it in process.

use std::cell::RefCell;
use std::f64::consts::TAU;

use brep_protocol::{Kernel, Refusal, model, model::Primitive, profile};
use cadrs_kernel::native::tol;
use cadrs_kernel::{
    Axis, BodyId, BodyKind, BoolOp, Bspline3, ChamferOpts, ChamferSpec, Curve2, DefaultKernel,
    DistanceLocation, DistanceTarget, DraftSpec, EdgeId, Extent, FaceId, FaceInput, FilletLaw,
    FilletProfile, FilletSize, FilletSpec, Kernel as _, KernelError, LoftEnd, LoftSection,
    LoftSpec, Loop, Motion, OffsetSpec, OpResult, PathCurve, Plane, PointClass, Profile,
    ProjVisibility, ProjectOptions, Region, RevolveSpec, ScrewSpec, ShellSpec, SlabSpec, SplitTool,
    SweepControl, SweepLaw, SweepSpec, Tessellation, ThickenSpec, ViewFrame,
};
use nalgebra::{Matrix3, Point2, Point3, Unit, Vector3};

/// The cadrs release and the commit of its repository the adapter builds (Cargo.toml's `rev`).
const VERSION: &str = "0.2.8+6cae155";

/// The contract's linear tolerance ε, mm: a point names the edge or face within `10·ε` of it.
const EPS: f64 = 1e-7;

/// A handle: one of the kernel's bodies, or the empty solid, which the kernel reports as an
/// empty result rather than a body.
#[derive(Clone, Copy, Debug)]
pub enum Solid {
    Empty,
    Body(BodyId),
}

/// The kernel's session: it owns the bodies, and its operations never change their inputs.
pub struct Cadrs;

thread_local! {
    /// Each thread's own session: the in-process runner gives every worker thread its cases and
    /// resets the thread's session after each, and `serve` runs on one thread.
    static SESSION: RefCell<DefaultKernel> = RefCell::new(DefaultKernel::new());
}

impl Cadrs {
    /// Runs `f` on this thread's session (the examples reach the bodies behind a handle).
    pub fn with<T>(&self, f: impl FnOnce(&mut DefaultKernel) -> T) -> T {
        // The runner silences panics while it runs; `CADRS_PANICS=1` prints each with its
        // backtrace, the hook set at the first call so that it replaces the runner's.
        static TRACE: std::sync::Once = std::sync::Once::new();
        TRACE.call_once(|| {
            if std::env::var_os("CADRS_PANICS").is_some() {
                std::panic::set_hook(Box::new(|info| {
                    let trace = std::backtrace::Backtrace::force_capture();
                    eprintln!("{info}\n{trace}");
                }));
            }
        });
        SESSION.with(|k| f(&mut k.borrow_mut()))
    }
}

/// The kernel's error as the protocol's refusal, its kind the kernel's own.
fn refusal(e: KernelError) -> Refusal {
    let reason = e.message();
    match e {
        KernelError::Unsupported(_) => Refusal::unsupported(reason),
        KernelError::InvalidProfile(_)
        | KernelError::InvalidParameter(_)
        | KernelError::InvalidInput(_) => Refusal::invalid(reason),
        _ => Refusal::failed(reason),
    }
}

/// An operation's result as a handle: the kernel's one body, or the empty solid where the kernel
/// says the result is empty.
fn solid(r: Result<OpResult, KernelError>) -> Result<Solid, Refusal> {
    match r {
        Err(KernelError::EmptyResult(_)) => Ok(Solid::Empty),
        Err(e) => Err(refusal(e)),
        Ok(r) => match r.bodies[..] {
            [] => Ok(Solid::Empty),
            [b] => Ok(Solid::Body(b)),
            ref many => Err(Refusal::failed(format!(
                "the kernel gave {} bodies for one solid",
                many.len()
            ))),
        },
    }
}

fn p3(a: model::Vec3) -> Point3<f64> {
    Point3::new(a[0], a[1], a[2])
}

fn v3(a: model::Vec3) -> Vector3<f64> {
    Vector3::new(a[0], a[1], a[2])
}

fn unit(a: model::Vec3) -> Result<Unit<Vector3<f64>>, Refusal> {
    Unit::try_new(v3(a), 0.0).ok_or_else(|| Refusal::invalid(format!("{a:?} has no direction")))
}

fn arr(p: Point3<f64>) -> model::Vec3 {
    [p.x, p.y, p.z]
}

fn pt2(a: [f64; 2]) -> Point2<f64> {
    Point2::new(a[0], a[1])
}

/// The sketch plane of the frame's xy, its x the frame's.
fn xy(f: &model::Frame) -> Result<Plane, Refusal> {
    Ok(Plane {
        origin: p3(f.origin),
        x_dir: unit(f.x)?,
        normal: unit(f.z)?,
    })
}

/// One of the suite's segments as the kernel's curve.
fn curve(s: &profile::Seg) -> Result<Curve2, Refusal> {
    Ok(match *s {
        profile::Seg::Line { a, b } => Curve2::Line {
            a: pt2(a),
            b: pt2(b),
            source: None,
        },
        profile::Seg::Arc {
            center,
            radius,
            sweep,
            ..
        } if sweep.abs() >= TAU => Curve2::Circle {
            center: pt2(center),
            radius: if sweep > 0.0 { radius } else { -radius },
            source: None,
        },
        profile::Seg::Arc {
            center,
            radius,
            start,
            sweep,
        } => Curve2::Arc {
            center: pt2(center),
            radius,
            start_angle: start,
            sweep,
            source: None,
        },
        // The same B-spline: the kernel's spline runs over the same knots' domain, a periodic one
        // closed by its repeated poles.
        profile::Seg::Spline(ref sp) => Curve2::Spline {
            degree: sp.degree,
            knots: sp.knots.clone(),
            poles: sp.ctrl.iter().map(|&c| pt2(c)).collect(),
            weights: Vec::new(),
            source: None,
        },
        // The conic is the rational quadratic Bézier on `a`, `apex`, `b` with weights 1, w, 1.
        profile::Seg::Conic { a, apex, b, w } => Curve2::Spline {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            poles: vec![pt2(a), pt2(apex), pt2(b)],
            weights: vec![1.0, w, 1.0],
            source: None,
        },
        profile::Seg::Gn { .. } => {
            return Err(Refusal::unsupported(
                "a blend-section segment: the kernel's sketches have no such curve",
            ));
        }
    })
}

/// The suite's profile on `plane` as the kernel's: one region, the first loop its outline and the
/// rest its holes.
fn sketch(plane: Plane, p: &profile::Profile) -> Result<Profile, Refusal> {
    let mut loops = p.loops.iter().map(|l| {
        Ok(Loop {
            curves: l.iter().map(curve).collect::<Result<_, Refusal>>()?,
        })
    });
    let outer = loops
        .next()
        .ok_or_else(|| Refusal::invalid("a profile with no loop"))??;
    let holes = loops.collect::<Result<_, Refusal>>()?;
    Ok(Profile::new(
        plane,
        vec![Region {
            outer,
            holes,
            source: None,
        }],
    ))
}

/// Where the point is on the body's boundary, by the kernel's distance query, within `10·ε`.
fn locate(k: &DefaultKernel, b: BodyId, p: model::Vec3) -> Result<DistanceLocation, Refusal> {
    let r = k
        .min_distance(&DistanceTarget::Point(p3(p)), &DistanceTarget::Body(b))
        .map_err(refusal)?;
    if r.distance > 10.0 * EPS {
        return Err(Refusal::invalid(format!(
            "{p:?} is {:e} from the solid's boundary",
            r.distance
        )));
    }
    Ok(r.b_at)
}

/// The edge each point lies on: the edges the kernel's distance query puts within `10·ε` of it.
/// A seam, where a closed face meets itself, is not an edge of the solid.
fn edges_at(k: &DefaultKernel, b: BodyId, points: &[model::Vec3]) -> Result<Vec<EdgeId>, Refusal> {
    let edges: Vec<EdgeId> = k
        .edges(b)
        .map_err(refusal)?
        .into_iter()
        .filter(|e| !(e.faces[0].is_some() && e.faces[0] == e.faces[1]))
        .map(|e| e.id)
        .collect();
    points
        .iter()
        .map(|&p| {
            let mut on = Vec::new();
            for &e in &edges {
                let r = k
                    .min_distance(&DistanceTarget::Point(p3(p)), &DistanceTarget::Edge(b, e))
                    .map_err(refusal)?;
                if r.distance <= 10.0 * EPS {
                    on.push(e);
                }
            }
            match on[..] {
                [e] => Ok(e),
                [] => Err(Refusal::invalid(format!("no edge passes through {p:?}"))),
                _ => Err(Refusal::invalid(format!(
                    "{p:?} is a vertex of {} edges, not a point of one",
                    on.len()
                ))),
            }
        })
        .collect()
}

/// The face each point lies on. A point on an edge or a vertex is on several faces, and the
/// kernel answers for one: the adapter does not choose.
fn faces_at(k: &DefaultKernel, b: BodyId, points: &[model::Vec3]) -> Result<Vec<FaceId>, Refusal> {
    points.iter().map(|&p| face_at(k, b, p)).collect()
}

fn face_at(k: &DefaultKernel, b: BodyId, p: model::Vec3) -> Result<FaceId, Refusal> {
    match locate(k, b, p)? {
        DistanceLocation::Face(f, ..) => Ok(f),
        _ => Err(Refusal::unsupported(format!(
            "{p:?} lies on an edge: the kernel names one face there"
        ))),
    }
}

fn body(s: &Solid) -> Result<BodyId, Refusal> {
    match *s {
        Solid::Body(b) => Ok(b),
        Solid::Empty => Err(Refusal::invalid("the solid is empty")),
    }
}

/// The matrix and translation of the rows `[M | t]`.
fn rows(m: [[f64; 4]; 3]) -> (Matrix3<f64>, Vector3<f64>) {
    (
        Matrix3::from_fn(|i, j| m[i][j]),
        Vector3::new(m[0][3], m[1][3], m[2][3]),
    )
}

const UNSUPPORTED: [(&str, &str); 0] = [];

impl Kernel for Cadrs {
    type Solid = Solid;

    fn name(&self) -> String {
        "cadrs".into()
    }

    fn version(&self) -> String {
        VERSION.into()
    }

    fn tolerance(&self) -> f64 {
        tol::POINT
    }

    fn configuration(&self) -> String {
        "chamfers do not run on past their faces (ChamferOpts::overflow false); meshes bound the sag alone (Tessellation::angle π)".into()
    }

    fn reset(&self) {
        SESSION.with(|k| *k.borrow_mut() = DefaultKernel::new());
    }

    fn unsupported(&self) -> Vec<String> {
        UNSUPPORTED.iter().map(|(n, _)| n.to_string()).collect()
    }

    fn make(&self, p: &Primitive) -> Result<Solid, Refusal> {
        use cadrs_kernel::Primitive as K;
        let f = p.frame();
        let (o, z) = (p3(f.origin), unit(f.z)?);
        self.with(|k| match *p {
            Primitive::Box { size, .. } => {
                let world = f.x == [1.0, 0.0, 0.0] && f.y == [0.0, 1.0, 0.0];
                let lo = if world { o } else { Point3::origin() };
                let made = solid(k.primitive(&K::Box {
                    min: lo,
                    max: lo + v3(size),
                }))?;
                if world {
                    return Ok(made);
                }
                // The kernel's box is square to the axes: built at the origin, then placed.
                let b = body(&made)?;
                let r = Matrix3::from_columns(&[v3(f.x), v3(f.y), v3(f.z)]);
                let placed = solid(k.transform_affine(b, r, o.coords));
                k.release(b);
                placed
            }
            Primitive::Cylinder { radius, height, .. } => solid(k.primitive(&K::Cylinder {
                base: o,
                axis: z,
                radius,
                height,
            })),
            Primitive::Sphere { radius, .. } => solid(k.primitive(&K::Sphere {
                center: o,
                radius,
                axis: z,
                seam: unit(f.x)?,
            })),
            Primitive::Torus { major, minor, .. } => solid(k.primitive(&K::Torus {
                center: o,
                axis: z,
                major,
                minor,
            })),
            Primitive::Cone {
                bottom,
                top,
                height,
                ..
            } => solid(k.primitive(&K::Cone {
                base: o,
                axis: z,
                base_radius: bottom,
                top_radius: top,
                height,
            })),
            Primitive::Extrude {
                ref profile,
                height,
                ..
            } => solid(k.extrude(&sketch(xy(&f)?, profile)?, Extent::Blind(height))),
            Primitive::Revolve {
                ref profile, angle, ..
            } => {
                // Drawn with x out from the axis and y along it: the plane of the frame's x and z.
                let plane = Plane {
                    origin: o,
                    x_dir: unit(f.x)?,
                    normal: unit([-f.y[0], -f.y[1], -f.y[2]])?,
                };
                let axis = Axis { origin: o, dir: z };
                let whole = angle.abs() >= TAU;
                let spec = RevolveSpec::solid(axis, (!whole).then_some(angle));
                solid(k.revolve_with(&sketch(plane, profile)?, &spec))
            }
            Primitive::Sweep {
                ref profile,
                ref path,
                ..
            } => {
                // The path in the plane of the frame's z (its start, 2D x) and x (where its bends
                // turn, 2D y).
                let plane = Plane {
                    origin: o,
                    x_dir: z,
                    normal: unit(f.y)?,
                };
                let (mut at, mut heading) = (Point2::origin(), 0.0_f64);
                let mut curves = Vec::new();
                for seg in path {
                    let dir = nalgebra::Vector2::new(heading.cos(), heading.sin());
                    let curve = match *seg {
                        model::PathSeg::Line { length } => {
                            let end = at + dir * length;
                            let c = Curve2::Line {
                                a: at,
                                b: end,
                                source: None,
                            };
                            at = end;
                            c
                        }
                        model::PathSeg::Arc { radius, angle } => {
                            let left = nalgebra::Vector2::new(-dir.y, dir.x);
                            let center = at + left * radius * angle.signum();
                            let start = (at - center).y.atan2((at - center).x);
                            let end = start + angle;
                            let c = Curve2::Arc {
                                center,
                                radius,
                                start_angle: start,
                                sweep: angle,
                                source: None,
                            };
                            at = center + nalgebra::Vector2::new(end.cos(), end.sin()) * radius;
                            heading += angle;
                            c
                        }
                    };
                    curves.push(PathCurve::Sketch { plane, curve });
                }
                let spec = SweepSpec {
                    body: BodyKind::Solid,
                    path: curves,
                    control: SweepControl::None,
                    faces: Vec::new(),
                    corners: Default::default(),
                };
                solid(k.sweep_with(&sketch(xy(&f)?, profile)?, &spec))
            }
            Primitive::SweepPath {
                ref profile,
                ref path,
                twist,
                ..
            } => {
                let world = |c: [f64; 3]| o + v3(f.x) * c[0] + v3(f.y) * c[1] + v3(f.z) * c[2];
                let spec = SweepSpec {
                    body: BodyKind::Solid,
                    path: vec![PathCurve::Bspline(Bspline3 {
                        degree: path.degree,
                        knots: path.knots.clone(),
                        poles: path.ctrl.iter().map(|&c| world(c)).collect(),
                        weights: Vec::new(),
                    })],
                    control: SweepControl::None,
                    faces: Vec::new(),
                    corners: Default::default(),
                };
                let law = SweepLaw {
                    twist,
                    scale: 1.0,
                    spiral: false,
                };
                solid(k.sweep_with_law(&sketch(xy(&f)?, profile)?, &spec, &law))
            }
            Primitive::Loft {
                ref profile,
                scale,
                height,
                ..
            } => {
                // The top section is the protocol crate's scaled copy, the same for every adapter.
                let mut top = xy(&f)?;
                top.origin += z.into_inner() * height;
                let spec = LoftSpec {
                    body: BodyKind::Solid,
                    sections: vec![
                        LoftSection::Profile(sketch(xy(&f)?, profile)?),
                        LoftSection::Profile(sketch(top, &profile.scaled(scale))?),
                    ],
                    start: LoftEnd::default(),
                    end: LoftEnd::default(),
                    source: 0,
                };
                solid(k.loft_with(&spec))
            }
            Primitive::Screw {
                ref profile,
                rise,
                angle,
                ..
            } => {
                // Drawn as for revolve: x out from the axis and y along it.
                let plane = Plane {
                    origin: o,
                    x_dir: unit(f.x)?,
                    normal: unit([-f.y[0], -f.y[1], -f.y[2]])?,
                };
                let spec = ScrewSpec {
                    axis: Axis { origin: o, dir: z },
                    rise,
                    angle,
                };
                solid(k.screw(&sketch(plane, profile)?, &spec))
            }
            Primitive::Slab {
                size,
                ref knots_u,
                ref knots_v,
                ref heights,
                ..
            } => solid(k.slab(&SlabSpec {
                origin: o,
                x: unit(f.x)?,
                y: unit(f.y)?,
                size,
                knots_u: knots_u.clone(),
                knots_v: knots_v.clone(),
                heights: heights.clone(),
            })),
        })
    }

    fn loft_through(
        &self,
        frame: &model::Frame,
        profiles: &[profile::Profile],
        heights: &[f64],
    ) -> Result<Solid, Refusal> {
        if profiles.len() != heights.len() {
            return Err(Refusal::invalid(format!(
                "{} profiles at {} heights",
                profiles.len(),
                heights.len()
            )));
        }
        let sections = profiles
            .iter()
            .zip(heights)
            .map(|(p, &h)| {
                let mut plane = xy(frame)?;
                plane.origin += v3(frame.z) * h;
                Ok(LoftSection::Profile(sketch(plane, p)?))
            })
            .collect::<Result<_, Refusal>>()?;
        let spec = LoftSpec {
            body: BodyKind::Solid,
            sections,
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 0,
        };
        self.with(|k| solid(k.loft_with(&spec)))
    }

    fn union(&self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        match (*a, *b) {
            (Solid::Empty, other) | (other, Solid::Empty) => Ok(other),
            (Solid::Body(a), Solid::Body(b)) => {
                self.with(|k| solid(k.boolean(BoolOp::Union, a, &[b])))
            }
        }
    }

    fn intersection(&self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        match (*a, *b) {
            (Solid::Body(a), Solid::Body(b)) => {
                self.with(|k| solid(k.boolean(BoolOp::Intersect, a, &[b])))
            }
            _ => Ok(Solid::Empty),
        }
    }

    fn difference(&self, a: &Solid, b: &Solid) -> Result<Solid, Refusal> {
        match (*a, *b) {
            (Solid::Empty, _) => Ok(Solid::Empty),
            (a, Solid::Empty) => Ok(a),
            (Solid::Body(a), Solid::Body(b)) => {
                self.with(|k| solid(k.boolean(BoolOp::Subtract, a, &[b])))
            }
        }
    }

    fn union_all(&self, solids: &[&Solid]) -> Result<Solid, Refusal> {
        let bodies: Vec<BodyId> = solids.iter().filter_map(|s| body(s).ok()).collect();
        match bodies[..] {
            [] => Ok(Solid::Empty),
            [one] => Ok(Solid::Body(one)),
            [first, ref rest @ ..] => self.with(|k| solid(k.boolean(BoolOp::Union, first, rest))),
        }
    }

    fn difference_all(&self, target: &Solid, tools: &[&Solid]) -> Result<Solid, Refusal> {
        let Solid::Body(target) = *target else {
            return Ok(Solid::Empty);
        };
        let tools: Vec<BodyId> = tools.iter().filter_map(|s| body(s).ok()).collect();
        if tools.is_empty() {
            return Ok(Solid::Body(target));
        }
        self.with(|k| solid(k.boolean(BoolOp::Subtract, target, &tools)))
    }

    fn volume(&self, s: &Solid) -> Result<f64, Refusal> {
        match *s {
            Solid::Empty => Ok(0.0),
            Solid::Body(b) => {
                self.with(|k| k.mass_properties(b).map(|m| m.volume).map_err(refusal))
            }
        }
    }

    fn area(&self, s: &Solid) -> Result<f64, Refusal> {
        match *s {
            Solid::Empty => Ok(0.0),
            Solid::Body(b) => self.with(|k| {
                k.mass_properties(b)
                    .map(|m| m.surface_area)
                    .map_err(refusal)
            }),
        }
    }

    fn check(&self, s: &Solid) -> Result<Vec<String>, Refusal> {
        match *s {
            Solid::Empty => Ok(Vec::new()),
            Solid::Body(b) => self.with(|k| Ok(k.get(b).map_err(refusal)?.check())),
        }
    }

    fn classify(&self, s: &Solid, points: &[model::Vec3]) -> Result<Vec<model::Side>, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(vec![model::Side::Out; points.len()]);
        };
        let pts: Vec<Point3<f64>> = points.iter().map(|&p| p3(p)).collect();
        self.with(|k| {
            let sides = k.classify_many(b, &pts, EPS).map_err(refusal)?;
            Ok(sides
                .into_iter()
                .map(|c| match c {
                    PointClass::Inside => model::Side::In,
                    PointClass::OnBoundary => model::Side::On,
                    PointClass::Outside => model::Side::Out,
                })
                .collect())
        })
    }

    fn closest(&self, s: &Solid, points: &[model::Vec3]) -> Result<Vec<model::Vec3>, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            points
                .iter()
                .map(|&p| {
                    k.min_distance(&DistanceTarget::Point(p3(p)), &DistanceTarget::Solid(b))
                        .map(|r| arr(r.b))
                        .map_err(refusal)
                })
                .collect()
        })
    }

    fn normal(
        &self,
        s: &Solid,
        points: &[model::Vec3],
    ) -> Result<Vec<Option<model::Vec3>>, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(vec![None; points.len()]);
        };
        let pts: Vec<Point3<f64>> = points.iter().map(|&p| p3(p)).collect();
        self.with(|k| {
            let n = k.boundary_normals(b, &pts, 10.0 * EPS).map_err(refusal)?;
            Ok(n.into_iter().map(|n| n.map(|n| [n.x, n.y, n.z])).collect())
        })
    }

    fn fillet(&self, s: &Solid, edges: &[model::Vec3], radius: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.fillet(b, &ids, radius))
        })
    }

    fn fillet_conic(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        setback: f64,
        rho: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let spec = FilletSpec {
            size: FilletSize::Setback(setback),
            allow_overflow: true,
            profile: FilletProfile::Conic { rho },
            propagate: true,
        };
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.fillet_with(b, &ids, &spec))
        })
    }

    fn fillet_g2(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        offset: f64,
        center: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let spec = FilletSpec {
            size: FilletSize::Radius(offset),
            allow_overflow: true,
            profile: FilletProfile::Thumbweight {
                center: Some(center),
            },
            propagate: true,
        };
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.fillet_with(b, &ids, &spec))
        })
    }

    fn fillet_setback(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        radius: f64,
        setback: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.fillet_setback(b, &ids, &FilletSpec::radius(radius), setback))
        })
    }

    fn fillet_setback_g2(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        radius: f64,
        setback: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let spec = FilletSpec {
            profile: FilletProfile::Thumbweight { center: None },
            ..FilletSpec::radius(radius)
        };
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.fillet_setback(b, &ids, &spec, setback))
        })
    }

    fn fillet_faces(
        &self,
        s: &Solid,
        face_a: model::Vec3,
        face_b: model::Vec3,
        radius: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let (fa, fb) = (face_at(k, b, face_a)?, face_at(k, b, face_b)?);
            solid(k.fillet_faces(b, fa, fb, radius))
        })
    }

    fn fillet_radii(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        radii: &[f64],
    ) -> Result<Solid, Refusal> {
        if edges.len() != radii.len() {
            return Err(Refusal::invalid(format!(
                "{} edges but {} radii",
                edges.len(),
                radii.len()
            )));
        }
        let b = body(s)?;
        self.with(|k| {
            let laws: Vec<FilletLaw> = edges_at(k, b, edges)?
                .into_iter()
                .zip(radii)
                .map(|(edge, &r)| FilletLaw {
                    edge,
                    radii: vec![(0.0, r), (1.0, r)],
                })
                .collect();
            solid(k.fillet_variable(b, &laws, true))
        })
    }

    fn fillet_variable(
        &self,
        s: &Solid,
        edge: model::Vec3,
        from: model::Vec3,
        r_from: f64,
        r_to: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let e = edges_at(k, b, &[edge])?[0];
            let info = k
                .edges(b)
                .map_err(refusal)?
                .into_iter()
                .find(|i| i.id == e)
                .ok_or_else(|| Refusal::failed("the kernel lost the edge it named"))?;
            // The law runs from the edge's start: `from` names one of its ends.
            let at_start = (info.start - p3(from)).norm() <= (info.end - p3(from)).norm();
            let radii = if at_start {
                vec![(0.0, r_from), (1.0, r_to)]
            } else {
                vec![(0.0, r_to), (1.0, r_from)]
            };
            solid(k.fillet_variable(b, &[FilletLaw { edge: e, radii }], true))
        })
    }

    fn full_round(
        &self,
        s: &Solid,
        side_a: model::Vec3,
        middle: model::Vec3,
        side_b: model::Vec3,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let (a, m, c) = (
                face_at(k, b, side_a)?,
                face_at(k, b, middle)?,
                face_at(k, b, side_b)?,
            );
            solid(k.full_round(b, a, m, c))
        })
    }

    fn chamfer(&self, s: &Solid, edges: &[model::Vec3], distance: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            let opts = ChamferOpts {
                overflow: false,
                ..ChamferOpts::new(ChamferSpec::EqualDistance(distance))
            };
            solid(k.chamfer_with(b, &ids, &opts))
        })
    }

    fn chamfer_distances(
        &self,
        s: &Solid,
        edges: &[model::Vec3],
        distances: &[f64],
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let ids = edges_at(k, b, edges)?;
            solid(k.chamfer_distances(b, &ids, distances, false))
        })
    }

    fn chamfer_two(
        &self,
        s: &Solid,
        edge: model::Vec3,
        face: model::Vec3,
        d1: f64,
        d2: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let e = edges_at(k, b, &[edge])?;
            let f = face_at(k, b, face)?;
            let opts = ChamferOpts {
                overflow: false,
                ..ChamferOpts::new(ChamferSpec::TwoDistances(d1, d2)).first_on(f)
            };
            solid(k.chamfer_with(b, &e, &opts))
        })
    }

    fn offset(&self, s: &Solid, distance: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let spec = OffsetSpec {
            distance,
            faces: Vec::new(),
            sharp: false,
        };
        self.with(|k| solid(k.offset(b, &spec)))
    }

    fn offset_faces(
        &self,
        s: &Solid,
        faces: &[model::Vec3],
        distance: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let spec = OffsetSpec {
                distance: 0.0,
                faces: faces_at(k, b, faces)?
                    .into_iter()
                    .map(|f| (f, distance))
                    .collect(),
                sharp: true,
            };
            solid(k.offset(b, &spec))
        })
    }

    fn delete_faces(&self, s: &Solid, faces: &[model::Vec3]) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let ids = faces_at(k, b, faces)?;
            solid(k.delete_faces(b, &ids))
        })
    }

    fn move_faces(
        &self,
        s: &Solid,
        faces: &[model::Vec3],
        matrix: [[f64; 4]; 3],
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let (linear, translation) = rows(matrix);
        self.with(|k| {
            let ids = faces_at(k, b, faces)?;
            solid(k.move_faces(
                b,
                &ids,
                &Motion {
                    linear,
                    translation,
                },
            ))
        })
    }

    fn thicken(&self, s: &Solid, faces: &[model::Vec3], thickness: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let faces = faces_at(k, b, faces)?
                .into_iter()
                .map(|face| FaceInput {
                    body: b,
                    face,
                    source: 0,
                })
                .collect();
            let spec = ThickenSpec {
                bodies: Vec::new(),
                faces,
                profiles: Vec::new(),
                along: thickness.max(0.0),
                against: (-thickness).max(0.0),
                sharp: false,
                source: 0,
            };
            solid(k.thicken_surfaces(&spec))
        })
    }

    fn shell(&self, s: &Solid, open: &[model::Vec3], thickness: f64) -> Result<Solid, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let spec = ShellSpec {
                remove: faces_at(k, b, open)?,
                thickness,
                outward: false,
                hollow: open.is_empty(),
                sharp: false,
                nothing_hollowed: Default::default(),
            };
            solid(k.shell_with(b, &spec))
        })
    }

    fn draft(
        &self,
        s: &Solid,
        faces: &[model::Vec3],
        origin: model::Vec3,
        pull: model::Vec3,
        angle: f64,
    ) -> Result<Solid, Refusal> {
        let b = body(s)?;
        let pull = unit(pull)?;
        self.with(|k| {
            let spec = DraftSpec {
                faces: faces_at(k, b, faces)?,
                angle,
                pull,
                neutral: p3(origin),
                neutral_normal: None,
                tangent_propagation: false,
            };
            solid(k.draft(b, &spec))
        })
    }

    fn transform(&self, s: &Solid, matrix: [[f64; 4]; 3]) -> Result<Solid, Refusal> {
        self.affine(s, matrix)
    }

    fn affine(&self, s: &Solid, matrix: [[f64; 4]; 3]) -> Result<Solid, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(Solid::Empty);
        };
        let (m, t) = rows(matrix);
        self.with(|k| solid(k.transform_affine(b, m, t)))
    }

    fn raycast(&self, s: &Solid, o: model::Vec3, d: model::Vec3) -> Result<Vec<f64>, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(Vec::new());
        };
        self.with(|k| {
            let hits = k.ray_hits(b, p3(o), v3(d)).map_err(refusal)?;
            // The kernel casts a whole line: the ray is its meetings ahead of the origin.
            Ok(hits.iter().map(|h| h.t).filter(|&t| t >= 0.0).collect())
        })
    }

    fn write_step(&self, s: &Solid) -> Result<String, Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let bytes = k.export_step(&[b]).map_err(refusal)?;
            String::from_utf8(bytes).map_err(|e| Refusal::failed(e.to_string()))
        })
    }

    fn read_step(&self, text: &str) -> Result<Solid, Refusal> {
        self.with(|k| {
            let bodies = k.import_step(text.as_bytes()).map_err(refusal)?;
            match bodies[..] {
                [] => Ok(Solid::Empty),
                [b] => Ok(Solid::Body(b)),
                // The file's solids, gathered as one set.
                ref many => solid(k.compound(many)),
            }
        })
    }

    fn sew(&self, text: &str, tolerance: f64) -> Result<Solid, Refusal> {
        self.with(|k| solid(k.sew_step(text.as_bytes(), tolerance)))
    }

    fn centroid(&self, s: &Solid) -> Result<Option<model::Vec3>, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(None);
        };
        self.with(|k| {
            let m = k.mass_properties(b).map_err(refusal)?;
            Ok(Some(arr(m.center_of_mass)))
        })
    }

    fn inertia(&self, s: &Solid) -> Result<[[f64; 3]; 3], Refusal> {
        let Solid::Body(b) = *s else {
            return Ok([[0.0; 3]; 3]);
        };
        self.with(|k| {
            let i = k.mass_properties(b).map_err(refusal)?.inertia;
            Ok([0, 1, 2].map(|r| [0, 1, 2].map(|c| i[(r, c)])))
        })
    }

    fn distance(&self, a: &Solid, b: &Solid) -> Result<f64, Refusal> {
        let (a, b) = (body(a)?, body(b)?);
        self.with(|k| {
            k.min_distance(&DistanceTarget::Solid(a), &DistanceTarget::Solid(b))
                .map(|r| r.distance)
                .map_err(refusal)
        })
    }

    fn curvature(
        &self,
        s: &Solid,
        points: &[model::Vec3],
    ) -> Result<Vec<Option<[f64; 2]>>, Refusal> {
        let b = body(s)?;
        let pts: Vec<Point3<f64>> = points.iter().map(|&p| p3(p)).collect();
        self.with(|k| k.curvatures_at(b, &pts).map_err(refusal))
    }

    fn section(
        &self,
        s: &Solid,
        origin: model::Vec3,
        normal: model::Vec3,
        tolerance: f64,
    ) -> Result<Vec<Vec<model::Vec3>>, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(Vec::new());
        };
        self.with(|k| {
            let pieces = k
                .plane_section(b, p3(origin), v3(normal), tolerance)
                .map_err(refusal)?;
            Ok(pieces
                .into_iter()
                .map(|p| p.into_iter().map(arr).collect())
                .collect())
        })
    }

    fn section_area(
        &self,
        s: &Solid,
        origin: model::Vec3,
        normal: model::Vec3,
    ) -> Result<f64, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(0.0);
        };
        self.with(|k| k.section_area(b, p3(origin), v3(normal)).map_err(refusal))
    }

    fn betti(&self, s: &Solid) -> Result<[u64; 3], Refusal> {
        let Solid::Body(b) = *s else {
            return Ok([0, 0, 0]);
        };
        self.with(|k| k.betti_numbers(b).map_err(refusal))
    }

    fn interference(&self, a: &Solid, b: &Solid) -> Result<brep_protocol::Meeting, Refusal> {
        let (a, b) = (body(a)?, body(b)?);
        self.with(|k| {
            let m = k.interference(a, b).map_err(refusal)?;
            Ok(brep_protocol::Meeting {
                volume: m.volume,
                gap: m.gap,
            })
        })
    }

    fn bounding_box(&self, s: &Solid) -> Result<[model::Vec3; 2], Refusal> {
        let b = body(s)?;
        self.with(|k| {
            let bb = k.bounding_box(b).map_err(refusal)?;
            Ok([arr(bb.min), arr(bb.max)])
        })
    }

    fn split(
        &self,
        s: &Solid,
        origin: model::Vec3,
        normal: model::Vec3,
    ) -> Result<(Solid, Solid), Refusal> {
        let Solid::Body(b) = *s else {
            return Ok((Solid::Empty, Solid::Empty));
        };
        let n = unit(normal)?;
        let o = p3(origin);
        let hint = if n.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let plane = Plane {
            origin: o,
            x_dir: Unit::new_normalize(hint - n.into_inner() * hint.dot(&n)),
            normal: n,
        };
        self.with(|k| {
            let r = k.split(b, &SplitTool::Plane(plane), 0).map_err(refusal)?;
            // Sorted by side: the kernel's result's solids, each by where its centroid lies.
            let (mut ahead, mut behind) = (Vec::new(), Vec::new());
            let mut pieces = Vec::new();
            for body in r.bodies {
                for s in k.split_solids(body).map_err(refusal)? {
                    pieces.extend(s.bodies);
                }
            }
            for piece in pieces {
                let c = k.mass_properties(piece).map_err(refusal)?.center_of_mass;
                if (c - o).dot(&n) > 0.0 {
                    ahead.push(piece);
                } else {
                    behind.push(piece);
                }
            }
            let mut side = |pieces: Vec<BodyId>| match pieces[..] {
                [] => Ok(Solid::Empty),
                [one] => Ok(Solid::Body(one)),
                ref many => solid(k.compound(many)),
            };
            Ok((side(ahead)?, side(behind)?))
        })
    }

    fn mesh(&self, s: &Solid, tolerance: f64) -> Result<brep_protocol::Mesh, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(brep_protocol::Mesh::default());
        };
        self.with(|k| {
            let m = k
                .tessellate(
                    b,
                    Tessellation {
                        deflection: tolerance,
                        // The protocol bounds the sag alone: no angle limit of the kernel's own.
                        angle: std::f64::consts::PI,
                    },
                )
                .map_err(refusal)?;
            Ok(brep_protocol::Mesh {
                positions: m.positions.iter().map(|&p| arr(p)).collect(),
                triangles: m.indices,
            })
        })
    }

    fn display(&self, s: &Solid, tolerance: f64) -> Result<brep_protocol::Display, Refusal> {
        let Solid::Body(b) = *s else {
            return Ok(brep_protocol::Display::default());
        };
        self.with(|k| {
            let m = k
                .tessellate(
                    b,
                    Tessellation {
                        deflection: tolerance,
                        // The protocol bounds the sag alone: no angle limit of the kernel's own.
                        angle: std::f64::consts::PI,
                    },
                )
                .map_err(refusal)?;
            let smooth: std::collections::HashSet<EdgeId> = k
                .edges(b)
                .map_err(refusal)?
                .into_iter()
                .filter(|e| e.smooth || (e.faces[0].is_some() && e.faces[0] == e.faces[1]))
                .map(|e| e.id)
                .collect();
            // Each face its own vertices, in the order its triangles first use them.
            let mut faces: Vec<(FaceId, brep_protocol::DisplayFace, Vec<Option<u32>>)> = Vec::new();
            for (t, &f) in m.indices.iter().zip(&m.triangle_faces) {
                let i = match faces.iter().position(|(id, ..)| *id == f) {
                    Some(i) => i,
                    None => {
                        faces.push((f, Default::default(), vec![None; m.positions.len()]));
                        faces.len() - 1
                    }
                };
                let (_, face, map) = &mut faces[i];
                let tri = t.map(|v| {
                    *map[v as usize].get_or_insert_with(|| {
                        face.positions.push(arr(m.positions[v as usize]));
                        let n = m.normals[v as usize];
                        face.normals.push([n.x, n.y, n.z]);
                        face.positions.len() as u32 - 1
                    })
                });
                face.triangles.push(tri);
            }
            Ok(brep_protocol::Display {
                faces: faces.into_iter().map(|(_, f, _)| f).collect(),
                edges: m
                    .edges
                    .iter()
                    .filter(|(id, _)| !smooth.contains(id))
                    .map(|(_, line)| line.iter().map(|&p| arr(p)).collect())
                    .collect(),
            })
        })
    }

    fn project(
        &self,
        s: &Solid,
        direction: model::Vec3,
        tolerance: f64,
    ) -> Result<brep_protocol::Projection, Refusal> {
        let b = body(s)?;
        let (u, _) = brep_protocol::view_axes(direction);
        let frame = ViewFrame::new(v3(direction), v3(u));
        let opts = ProjectOptions {
            tolerance,
            hidden: true,
        };
        self.with(|k| {
            let drawn = k.project(&[b], &frame, &opts).map_err(refusal)?;
            let mut out = brep_protocol::Projection::default();
            for e in drawn.edges {
                let line = e.points.iter().map(|p| [p.x, p.y]).collect();
                match e.visibility {
                    ProjVisibility::Visible => out.visible.push(line),
                    ProjVisibility::Hidden => out.hidden.push(line),
                }
            }
            Ok(out)
        })
    }
}
