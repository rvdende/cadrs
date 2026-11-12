//! Rebuilding the Gear feature (`crate::gear`): its plan's sections swept (extruded, or swept
//! along the axis with a twist and a scale) and either joined into the gear, cut from its
//! blank, or cut from the part the picked face is on. A child of `rebuild::kernel_ops`.

use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{
    Axis, BodyKind, Curve2, Extent, Kernel, Loop, OpResult, PathCurve, Plane, Profile, Region,
    SweepControl, SweepLaw, SweepSpec,
};
use nalgebra::{Point2, Point3, Unit, Vector3};

use crate::document::FaceRef;
use crate::gear::{self, Blank, Frame, GearFeature, Region2, Seg, Sweep};

fn v(a: Vec3) -> Vector3<f64> {
    Vector3::new(a[0], a[1], a[2])
}

fn plane(f: &Frame) -> Plane {
    Plane {
        origin: Point3::from(f.origin),
        x_dir: Unit::new_normalize(v(f.x)),
        normal: Unit::new_normalize(v(f.normal)),
    }
}

/// A section's loop as kernel curves, each with a source of its own (`next` counts them over
/// the whole feature, so every face it makes has its own name).
fn kernel_loop(l: &[Seg], next: &mut u64) -> Loop {
    let p = |q: [f64; 2]| Point2::new(q[0], q[1]);
    let curves = l
        .iter()
        .map(|s| {
            *next += 1;
            let source = Some(*next);
            match s {
                Seg::Line { a, b } => Curve2::Line {
                    a: p(*a),
                    b: p(*b),
                    source,
                },
                Seg::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => Curve2::Arc {
                    center: p(*center),
                    radius: *radius,
                    start_angle: *start,
                    sweep: *sweep,
                    source,
                },
                Seg::Spline { knots, poles } => Curve2::Spline {
                    degree: 3,
                    knots: knots.clone(),
                    poles: poles.iter().map(|q| p(*q)).collect(),
                    weights: Vec::new(),
                    source,
                },
                Seg::Circle { center, radius } => Curve2::Circle {
                    center: p(*center),
                    radius: *radius,
                    source,
                },
            }
        })
        .collect();
    Loop { curves }
}

fn kernel_regions(regions: &[Region2], next: &mut u64) -> Vec<Region> {
    regions
        .iter()
        .map(|r| {
            *next += 1;
            let source = Some(*next);
            Region {
                outer: kernel_loop(&r.outer, next),
                holes: r.holes.iter().map(|h| kernel_loop(h, next)).collect(),
                source,
            }
        })
        .collect()
}

fn polygon(points: &[[f64; 2]]) -> Region2 {
    Region2 {
        outer: (0..points.len())
            .map(|i| Seg::Line {
                a: points[i],
                b: points[(i + 1) % points.len()],
            })
            .collect(),
        holes: Vec::new(),
    }
}

/// The part and the index of its solid's face a gear's face reference names (by name on any
/// part first, then where the face was).
fn gear_face<'a>(state: &'a State, f: &FaceRef) -> Option<(&'a PartState, usize)> {
    let candidates: Vec<&PartState> = state
        .part(f.part)
        .into_iter()
        .chain(state.parts.iter().filter(|p| p.part.id != f.part))
        .collect();
    for by_name in [true, false] {
        for part in &candidates {
            if part.body.is_none() {
                continue;
            }
            let Ok((i, how)) = part.part.solid.resolve_face(&f.face, None, Some(f.seed)) else {
                continue;
            };
            if by_name && how == cadrs_kernel::naming::Match::Geometric {
                continue;
            }
            return Some((part, i));
        }
    }
    None
}

/// A sweep of a gear's plan as the kernel takes it: its profile, its path and how the profile
/// keeps its orientation, and its law. Also whether it is a plain extrude, and how long.
fn sweep_parts(s: &Sweep, next: &mut u64) -> (Profile, SweepSpec, SweepLaw, Option<f64>) {
    let profile = Profile::new(plane(&s.frame), kernel_regions(&s.regions, next));
    let travel = v(s.travel);
    let length = travel.norm();
    let straight = travel.cross(&v(s.frame.normal)).norm() < 1e-12 * length.max(1.0);
    let law = SweepLaw {
        twist: s.twist,
        scale: s.scale,
    };
    let from = Point3::from(s.frame.origin);
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path: vec![PathCurve::Bezier3 {
            poles: vec![from, from + travel],
            weights: Vec::new(),
        }],
        // Leaning (a rack's helical teeth): the section stays as drawn.
        control: if straight {
            SweepControl::None
        } else {
            SweepControl::KeepOrientation
        },
        corners: Default::default(),
        faces: Vec::new(),
    };
    let extrude = (law.is_identity() && straight).then_some(length);
    (profile, spec, law, extrude)
}

impl Rebuilder {
    /// One sweep of a gear's plan.
    fn gear_sweep(&mut self, s: &Sweep, next: &mut u64) -> cadrs_kernel::Result<OpResult> {
        let (profile, spec, law, extrude) = sweep_parts(s, next);
        match extrude {
            Some(length) => self.kernel.extrude(&profile, Extent::Blind(length)),
            None => self.kernel.sweep_with_law(&profile, &spec, &law),
        }
    }

    /// The solid a gear's teeth are cut from.
    fn gear_blank(&mut self, b: &Blank, next: &mut u64) -> cadrs_kernel::Result<OpResult> {
        match b {
            Blank::Revolve { frame, polygon: p } => {
                let profile = Profile::new(plane(frame), kernel_regions(&[polygon(p)], next));
                let axis = Axis {
                    origin: Point3::from(frame.origin),
                    dir: Unit::new_normalize(v(frame.y())),
                };
                self.kernel.revolve(&profile, axis, std::f64::consts::TAU)
            }
            Blank::Extrude {
                frame,
                polygon: p,
                depth,
            } => {
                let profile = Profile::new(plane(frame), kernel_regions(&[polygon(p)], next));
                self.kernel.extrude(&profile, Extent::Blind(*depth))
            }
        }
    }

    pub(in crate::rebuild) fn gear(
        &mut self,
        id: FeatureId,
        x: &GearFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let op = id.0;
        let target = match &x.face {
            Some(r) => {
                let (part, i) = gear_face(state, r).ok_or("The gear's face no longer exists")?;
                let t = gear::target(&part.part.solid, i)
                    .ok_or("Select a cylinder, a hole, a cone or a flat face")?;
                Some((part.part.id, t))
            }
            None => None,
        };
        let plan = gear::plan(x, target.as_ref().map(|t| &t.1))?;
        let mut next = 0u64;
        let mut made: Vec<(BodyId, BodyNames)> = Vec::new();
        // A gear's halves that twist (herringbone teeth): each starts on the section the one
        // before ends on, so the kernel joins them there as it sweeps them, with no boolean.
        let chained =
            plan.sweeps.len() > 1 && plan.sweeps.iter().all(|s| s.twist != 0.0 && s.scale == 1.0);
        if chained {
            let pieces: Vec<(Profile, SweepSpec, SweepLaw)> = plan
                .sweeps
                .iter()
                .map(|s| {
                    let (profile, spec, law, _) = sweep_parts(s, &mut next);
                    (profile, spec, law)
                })
                .collect();
            let joined = self
                .kernel
                .sweep_chain(&pieces)
                .and_then(|r| self.name_new(state, op, &r))
                .map_err(|err| format!("Gear failed: {err}"))?;
            made.push(joined);
        }
        for s in plan.sweeps.iter().filter(|_| !chained) {
            match self
                .gear_sweep(s, &mut next)
                .and_then(|r| self.name_new(state, op, &r))
            {
                Ok(b) => made.push(b),
                Err(err) => {
                    for (b, _) in made {
                        self.kernel.release(b);
                    }
                    return Err(format!("Gear failed: {err}"));
                }
            }
        }
        let teeth = self.fuse_made(op, "Gear", made)?;
        let geoms = state.geoms.clone();
        // The gear knows the part it cuts, and a gear made whole is a part of its own: neither
        // asks what the new body touches, nor previews what it adds or removes (each a boolean
        // with every part near it, several times the gear's own).
        let none = Contacts::default();
        // Cut into the picked face's part.
        if let Some((part, _)) = target {
            let scope = [part];
            let merge = Merge {
                op: BooleanOp::Remove,
                merge_all: false,
                scope: &scope,
                scope_set: true,
                surface: false,
            };
            return self.apply_extrude_op(id, &merge, teeth, state, &none, geoms);
        }
        let body = match &plan.blank {
            None => teeth,
            Some(blank) => {
                let cut = self
                    .gear_blank(blank, &mut next)
                    .and_then(|r| self.name_new(state, op, &r))
                    .and_then(|blank| {
                        let res = self.kernel.boolean(BoolOp::Subtract, blank.0, &[teeth.0]);
                        let named = res.and_then(|res| {
                            let body = res.bodies[0];
                            let inputs = [(blank.0, &blank.1), (teeth.0, &teeth.1)];
                            Ok((
                                body,
                                naming::name_body(&self.kernel, body, op, &res.history, &inputs)?,
                            ))
                        });
                        self.kernel.release(blank.0);
                        named
                    });
                self.kernel.release(teeth.0);
                cut.map_err(|err| format!("Gear failed: {err}"))?
            }
        };
        let merge = Merge {
            op: BooleanOp::New,
            merge_all: false,
            scope: &[],
            scope_set: false,
            surface: false,
        };
        self.apply_extrude_op(id, &merge, body, state, &none, geoms)
    }
}
