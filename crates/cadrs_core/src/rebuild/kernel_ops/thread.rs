//! Rebuilding the Thread feature (`crate::thread`): the thread's groove swept along a helix
//! and subtracted from the part the picked edge is on. A child of `rebuild::kernel_ops`.

use super::applied::edge_ids;
use super::*;
use cadrs_kernel::{BodyKind, Curve2, Loop, PathCurve, Plane, Profile, Region};
use cadrs_kernel::{SweepControl, SweepSpec};
use nalgebra::Point2;

use crate::surfacing::HelixGeom;
use crate::thread::{ThreadEnd, ThreadFeature, groove};

impl Rebuilder {
    pub(in crate::rebuild) fn thread(
        &mut self,
        id: FeatureId,
        x: &ThreadFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let r = x.edge.as_ref().ok_or("Select an edge")?;
        let lost = || "The thread's edge no longer exists".to_string();
        let (part, _) = edge_ids(state, r).ok_or_else(lost)?;
        let solid = &part.part.solid;
        let i = solid
            .resolve_edge(&r.edge, |e| Some(e.distance(r.seed)))
            .map_err(|_| lost())?
            .0;
        let cyl = crate::thread::cylinder(solid, i)
            .ok_or("Select a circular edge at the end of a cylinder or a hole")?;
        let (d, p) = (
            x.major().ok_or("Unknown thread size")?,
            x.pitch_mm().ok_or("Unknown thread pitch")?,
        );
        let section = groove(d, p, cyl.radius, cyl.external)?;
        // A pitch outside the edge, to the end of its length (less the groove's half width,
        // so the full thread is as long as asked) or a pitch past the cylinder's far end.
        let run_in = p;
        let length = match x.end {
            ThreadEnd::Blind => x.length,
            ThreadEnd::Full => cyl.height + p,
        };
        let height = run_in + length - p / 2.0;
        if height <= run_in {
            return Err("The thread is shorter than its pitch".into());
        }
        let at = |t: f64| [0, 1, 2].map(|k| cyl.start[k] + cyl.dir[k] * t);
        let helix = HelixGeom {
            origin: at(-run_in),
            axis: cyl.dir,
            x: cyl.x,
            r0: d / 2.0,
            r1: d / 2.0,
            height,
            turns: height / p,
            start_angle: 0.0,
            clockwise: x.clockwise,
        };
        let path: Vec<PathCurve> = helix
            .beziers()
            .into_iter()
            .map(|(poles, weights)| PathCurve::Bezier3 {
                poles: poles.iter().map(|q| Point3::from(*q)).collect(),
                weights,
            })
            .collect();
        // The section in the plane through the axis and the helix's start: x outward, y along
        // the axis (normal × x).
        let v = |a: [f64; 3]| Vector3::new(a[0], a[1], a[2]);
        let x_dir = Unit::new_normalize(v(cyl.x));
        let axis = Unit::new_normalize(v(cyl.dir));
        let plane = Plane {
            origin: Point3::from(helix.origin),
            x_dir,
            normal: Unit::new_normalize(x_dir.cross(&axis)),
        };
        let pts: Vec<Point2<f64>> = section.iter().map(|c| Point2::new(c[0], c[1])).collect();
        let profile = Profile::new(
            plane,
            vec![Region {
                outer: Loop {
                    curves: (0..pts.len())
                        .map(|k| Curve2::Line {
                            a: pts[k],
                            b: pts[(k + 1) % pts.len()],
                            source: None,
                        })
                        .collect(),
                },
                holes: Vec::new(),
                source: None,
            }],
        );
        let op = id.0;
        let target = part.part.id;
        let tool = self
            .kernel
            .sweep_with(
                &profile,
                &SweepSpec {
                    body: BodyKind::Solid,
                    path,
                    control: SweepControl::None,
                    corners: Default::default(),
                    faces: Vec::new(),
                },
            )
            .and_then(|r| self.name_new(state, op, &r))
            .map_err(|e| format!("Thread failed: {e}"))?;
        let scope = [target];
        let merge = Merge {
            op: BooleanOp::Remove,
            merge_all: false,
            scope: &scope,
            scope_set: true,
            surface: false,
        };
        let geoms = state.geoms.clone();
        let mut o = self.combine(id, &merge, tool, state, geoms)?;
        if !x.matches(2.0 * cyl.radius, cyl.external) {
            o.warning = o
                .warning
                .or(Some("Exact match for thread size not found".into()));
        }
        Ok(o)
    }
}
