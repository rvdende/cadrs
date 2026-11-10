//! Determinism: the same operations give bit-identical results when run again, and on 1 or 4
//! rayon threads. Every family of `Kernel` operation runs: extrude and revolve, booleans of each
//! kind, fillets, chamfers, shell, offsets (rounded and sharp), draft, sweep, loft, sew and heal,
//! and the STEP import of the results. Compared: each body as the body format writes it
//! (`write_body`, exact doubles), its tessellation and mass properties (by their `Debug` text,
//! which spells every double so it reads back to the same bits), the operation's history, the
//! STEP file of all of them (less the time stamp in its header) and the bodies that file reads
//! back as.

use std::f64::consts::PI;

use cadrs_kernel::native::kernel::{NativeKernel, subset};
use cadrs_kernel::native::topo::CheckLevel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    let c = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
    Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: c[i],
                b: c[(i + 1) % 4],
                source: None,
            })
            .collect(),
    }
}

fn circle(x: f64, y: f64, r: f64) -> Loop {
    Loop {
        curves: vec![Curve2::Circle {
            center: p(x, y),
            radius: r,
            source: None,
        }],
    }
}

fn solid(k: &mut NativeKernel, plane: Plane, outer: Loop, extent: Extent) -> BodyId {
    let r = Region {
        outer,
        holes: vec![],
        source: None,
    };
    k.extrude(&Profile::new(plane, vec![r]), extent)
        .unwrap()
        .bodies[0]
}

/// The XZ plane (normal −Y).
fn front() -> Plane {
    Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    }
}

/// What one run produced, case by case.
#[derive(Debug, PartialEq)]
struct Run {
    /// (case, body bytes, mesh, mass properties, history)
    cases: Vec<(&'static str, Vec<u8>, String, String, String)>,
    step: Vec<u8>,
    /// The bodies the STEP file reads back as (body bytes).
    imported: Vec<Vec<u8>>,
}

/// The sample of operations, one or more of each family: booleans with analytic and numeric
/// intersections, fillets, chamfers, offsets, a shell, a draft, a revolve, a sweep, a loft, a
/// sewing and a heal, each result checked, tessellated and measured, all of them written to one
/// STEP file, and that file read back.
fn run() -> Run {
    let mut k = NativeKernel::new();
    let bx = solid(
        &mut k,
        Plane::top(),
        rect(-5.0, -4.0, 5.0, 4.0),
        Extent::Blind(6.0),
    );
    let hole = solid(
        &mut k,
        Plane::top(),
        circle(1.0, 0.5, 2.0),
        Extent::Symmetric(20.0),
    );
    let cross = solid(
        &mut k,
        front(),
        circle(0.0, 3.0, 1.5),
        Extent::Symmetric(30.0),
    );
    let upright = solid(
        &mut k,
        Plane::top(),
        circle(0.0, 0.0, 2.5),
        Extent::Blind(12.0),
    );
    let mut cases: Vec<(&'static str, OpResult)> = Vec::new();
    let cut = k.boolean(BoolOp::Subtract, bx, &[hole]).unwrap();
    let cut_id = cut.bodies[0];
    cases.push(("box minus cylinder", cut));
    // Two cylinders crossing square: a numeric (non-planar) intersection curve.
    cases.push((
        "crossed cylinders",
        k.boolean(BoolOp::Union, upright, &[cross]).unwrap(),
    ));
    cases.push((
        "box and cylinder in common",
        k.boolean(BoolOp::Intersect, bx, &[upright]).unwrap(),
    ));
    let vertical: Vec<EdgeId> = k
        .edges(bx)
        .unwrap()
        .iter()
        .filter(|e| (e.start.z - e.end.z).abs() > 1.0)
        .map(|e| e.id)
        .collect();
    cases.push((
        "box's vertical edges filleted",
        k.fillet(bx, &vertical, 1.0).unwrap(),
    ));
    let rim: Vec<EdgeId> = k
        .edges(cut_id)
        .unwrap()
        .iter()
        .filter(|e| e.curve == CurveKind::Circle)
        .map(|e| e.id)
        .collect();
    cases.push(("hole's rims filleted", k.fillet(cut_id, &rim, 0.5).unwrap()));
    cases.push((
        "rounded offset",
        k.offset(
            cut_id,
            &OffsetSpec {
                distance: 0.75,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap(),
    ));
    let top = k
        .faces(bx)
        .unwrap()
        .into_iter()
        .max_by(|a, b| a.center.z.total_cmp(&b.center.z))
        .unwrap()
        .id;
    cases.push(("shelled box", k.shell(bx, &[top], 0.8).unwrap()));
    cases.push((
        "sharp offset",
        k.offset(
            bx,
            &OffsetSpec {
                distance: 0.5,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap(),
    ));
    cases.push((
        "box's vertical edges chamfered",
        k.chamfer(bx, &vertical, ChamferSpec::EqualDistance(0.8))
            .unwrap(),
    ));
    let sides: Vec<FaceId> = k
        .faces(bx)
        .unwrap()
        .into_iter()
        .filter(|f| f.plane.is_some_and(|pl| pl.normal.z.abs() < 1e-9))
        .map(|f| f.id)
        .collect();
    let draft = DraftSpec {
        faces: sides,
        angle: 3f64.to_radians(),
        pull: Vector3::z_axis(),
        neutral: Point3::origin(),
        neutral_normal: None,
        tangent_propagation: true,
    };
    cases.push(("box's sides drafted", k.draft(bx, &draft).unwrap()));
    let ring = Profile::new(
        front(),
        vec![Region {
            outer: rect(2.0, 0.0, 6.0, 5.0),
            holes: vec![],
            source: None,
        }],
    );
    cases.push((
        "revolved ring",
        k.revolve(
            &ring,
            Axis {
                origin: Point3::origin(),
                dir: Vector3::z_axis(),
            },
            3.0,
        )
        .unwrap(),
    ));
    // A circle along a straight, a bend and a straight.
    let top_plane = Plane::top();
    let path = vec![
        PathCurve::Sketch {
            plane: top_plane,
            curve: Curve2::Line {
                a: p(0.0, 0.0),
                b: p(20.0, 0.0),
                source: None,
            },
        },
        PathCurve::Sketch {
            plane: top_plane,
            curve: Curve2::Arc {
                center: p(20.0, 10.0),
                radius: 10.0,
                start_angle: -PI / 2.0,
                sweep: PI / 2.0,
                source: None,
            },
        },
        PathCurve::Sketch {
            plane: top_plane,
            curve: Curve2::Line {
                a: p(30.0, 10.0),
                b: p(30.0, 25.0),
                source: None,
            },
        },
    ];
    let across = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let disc = Profile::new(
        across,
        vec![Region {
            outer: circle(0.0, 0.0, 1.0),
            holes: vec![],
            source: None,
        }],
    );
    let sweep = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        corners: Default::default(),
        faces: vec![],
    };
    cases.push((
        "circle swept round a bend",
        k.sweep_with(&disc, &sweep).unwrap(),
    ));
    let at = |z: f64, outer: Loop| {
        Profile::new(
            Plane {
                origin: Point3::new(0.0, 0.0, z),
                ..Plane::top()
            },
            vec![Region {
                outer,
                holes: vec![],
                source: None,
            }],
        )
    };
    let sections = vec![
        LoftSection::Profile(at(0.0, rect(-3.0, -3.0, 3.0, 3.0))),
        LoftSection::Profile(at(8.0, circle(0.0, 0.0, 2.0))),
    ];
    cases.push((
        "square lofted to a circle",
        k.loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections,
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 9,
        })
        .unwrap(),
    ));
    // The box's faces as six sheets, sewn back into a solid.
    let whole = (**k.get(bx).unwrap()).clone();
    let sheets: Vec<BodyId> = (0..whole.faces.len())
        .map(|f| k.insert(subset(&whole, &[f], false)))
        .collect();
    cases.push(("box's faces sewn", k.sew_solid(&sheets, 1e-6).unwrap()));
    let rounded = cases
        .iter()
        .find(|c| c.0 == "box's vertical edges filleted")
        .map(|c| c.1.bodies[0])
        .unwrap();
    let heal = [
        HealOp::Fix {
            precision: 1e-7,
            max_tolerance: 1e-3,
        },
        HealOp::SameParameter { tolerance: 1e-6 },
        HealOp::UpdateTolerance,
    ];
    cases.push((
        "filleted box healed",
        Kernel::heal(&mut k, rounded, &heal).unwrap(),
    ));
    let mut out = Vec::new();
    let mut parts = Vec::new();
    for (name, r) in cases {
        let id = r.bodies[0];
        let issues = k.get(id).unwrap().check_with(CheckLevel::Full);
        assert!(issues.is_empty(), "{name}: {issues:?}");
        let mesh = k.tessellate(id, Tessellation::default()).unwrap();
        let mass = k.mass_properties(id).unwrap();
        out.push((
            name,
            k.write_body(id).unwrap(),
            format!("{mesh:?}"),
            format!("{mass:?}"),
            format!("{:?}", r.history),
        ));
        parts.push((id, name.to_string()));
    }
    let step = k
        .export_model(exchange::ExchangeFormat::Step, "determinism", &parts, &[])
        .unwrap();
    let read = k
        .import_model(exchange::ExchangeFormat::Step, &step)
        .unwrap();
    assert_eq!(read.parts.len(), parts.len());
    let imported = read
        .parts
        .iter()
        .map(|p| k.write_body(p.body).unwrap())
        .collect();
    Run {
        cases: out,
        step: without_timestamp(step),
        imported,
    }
}

/// The STEP text with its header's time stamp (the one line that should differ) blanked.
fn without_timestamp(step: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8(step).unwrap();
    text.lines()
        .map(|l| {
            if l.starts_with("FILE_NAME(") {
                "FILE_NAME(time stamp left out)"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

/// Says which output of which case differs (the whole texts are too long to print).
#[track_caller]
fn same(a: &Run, b: &Run, what: &str) {
    for (x, y) in a.cases.iter().zip(&b.cases) {
        assert!(x.1 == y.1, "{what}: {}: the body differs", x.0);
        assert!(x.2 == y.2, "{what}: {}: the mesh differs", x.0);
        assert!(
            x.3 == y.3,
            "{what}: {}: the mass properties differ: {} vs {}",
            x.0,
            x.3,
            y.3
        );
        assert!(x.4 == y.4, "{what}: {}: the history differs", x.0);
    }
    assert_eq!(a.cases.len(), b.cases.len());
    for (i, (x, y)) in a.imported.iter().zip(&b.imported).enumerate() {
        assert!(
            x == y,
            "{what}: the STEP file's part {i} reads back differently"
        );
    }
    assert_eq!(a.imported.len(), b.imported.len());
    if a.step != b.step {
        let line = a
            .step
            .split(|&c| c == b'\n')
            .zip(b.step.split(|&c| c == b'\n'))
            .position(|(x, y)| x != y);
        panic!("{what}: the STEP files differ (first at line {line:?})");
    }
}

fn on_threads(n: usize) -> Run {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .unwrap()
        .install(run)
}

#[test]
fn repeat_runs_are_identical() {
    let first = on_threads(4);
    let second = on_threads(4);
    same(&first, &second, "a second run");
}

#[test]
fn thread_count_does_not_change_results() {
    let one = on_threads(1);
    let four = on_threads(4);
    same(&one, &four, "1 vs 4 threads");
}

/// A STEP file of B-spline faces (OCCT's dataset, when it is there: `$CADRS_OCCT_DATA`, else
/// beside the checkout) reads to the same bodies on 1 and 4 threads: its faces' p-curves are
/// fitted in parallel.
#[test]
fn step_import_is_the_same_on_any_thread_count() {
    let root = std::env::var("CADRS_OCCT_DATA").unwrap_or_else(|_| {
        format!(
            "{}/../../../occt-dataset/opencascade-dataset-7.8.0",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let Ok(bytes) = std::fs::read(format!("{root}/step/bug24595_281390.stp")) else {
        return;
    };
    let read = |n: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
            .install(|| {
                let mut k = NativeKernel::new();
                let ids = k.import_step(&bytes).unwrap();
                ids.iter()
                    .map(|&b| k.write_body(b).unwrap())
                    .collect::<Vec<_>>()
            })
    };
    assert!(read(1) == read(4), "the bodies differ");
}
