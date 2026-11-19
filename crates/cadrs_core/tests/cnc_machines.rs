//! The "CNC machines" sample (issue #278, `samples::cnc_machines`): its fixture file is what
//! the sample builds; the parts are where they were designed; each machine's assembly solves
//! as built and an axis moves only what it carries; the CAM Studios' toolpaths compute.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::mate::{Dof, MateId, MateType};
use cadrs_core::assembly::solver::{Drive, SolveOptions};
use cadrs_core::assembly::{self, InstanceId, Pose, positions, structure};
use cadrs_core::cam::{CamId, geometry, program};
use cadrs_core::document::ElementKind;
use cadrs_core::samples::cnc_machines::assemblies::{mill3, mill4, mill5};
use cadrs_core::samples::cnc_machines::cam::{cam3, cam4, cam5};
use cadrs_core::samples::cnc_machines::{self as cnc, parts, work};
use cadrs_core::{Document, ElementId, PartId, Solid};

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn build(doc: &Document, studio: ElementId) -> Arc<cadrs_core::rebuild::Build> {
    let b = cadrs_core::rebuild::build(doc.element(studio).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    b
}

fn solid(b: &cadrs_core::rebuild::Build, part: PartId) -> &Solid {
    &b.part(part)
        .unwrap_or_else(|| panic!("{part:?} missing"))
        .solid
}

#[test]
fn the_fixture_is_current() {
    // `fixtures/cnc_machines.cadrs` is the sample as `samples::cnc_machines::document()` builds
    // it (open it with `cadrs fixtures/cnc_machines.cadrs`, or the scenario command
    // `fixture cnc_machines`). Regenerate it with
    // `CADRS_WRITE_FIXTURES=1 cargo test -r -p cadrs_core --test cnc_machines the_fixture`.
    let doc = cnc::document().unwrap();
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cnc_machines.cadrs");
    let text =
        ron::ser::to_string_pretty(&cnc::file(doc.clone()), ron::ser::PrettyConfig::default())
            .unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(
        stored.document, doc,
        "fixtures/cnc_machines.cadrs is out of date"
    );
    // The tabs, in order.
    let tabs: Vec<(&str, &str)> = doc
        .elements
        .iter()
        .map(|e| {
            let kind = match &e.kind {
                ElementKind::CamStudio(_) => "CAM Studio",
                _ if e.assembly_model().is_some() => "Assembly",
                ElementKind::PartStudio { .. } => "Part Studio",
                _ => "other",
            };
            (e.name.as_str(), kind)
        })
        .collect();
    assert_eq!(
        tabs,
        [
            ("Machine parts", "Part Studio"),
            ("Workholding", "Part Studio"),
            ("Test piece", "Part Studio"),
            ("Worm", "Part Studio"),
            ("3-axis mill", "Assembly"),
            ("4-axis mill", "Assembly"),
            ("5-axis mill", "Assembly"),
            ("CAM 3-axis", "CAM Studio"),
            ("CAM 4-axis", "CAM Studio"),
            ("CAM 5-axis", "CAM Studio"),
        ]
    );
}

#[test]
fn the_parts_are_where_they_were_designed() {
    let doc = cnc::document().unwrap();
    let boxes = |studio: ElementId, want: &[(PartId, [f64; 3], [f64; 3])]| {
        let b = build(&doc, studio);
        assert_eq!(b.parts.len(), want.len());
        for (p, lo, hi) in want {
            let (a, z) = solid(&b, *p).bounds().unwrap();
            assert!(
                dist(a, *lo) < 0.05 && dist(z, *hi) < 0.05,
                "{p:?}: {a:?} {z:?}"
            );
        }
    };
    boxes(
        cnc::MACHINE_PARTS,
        &[
            (
                parts::BASE,
                [-250.0, -310.0, -250.0],
                [250.0, 510.0, -110.0],
            ),
            (
                parts::COLUMN,
                [-150.0, 310.0, -110.0],
                [150.0, 510.0, 850.0],
            ),
            (
                parts::SADDLE,
                [-200.0, -150.0, -110.0],
                [200.0, 150.0, -50.0],
            ),
            (parts::TABLE, [-300.0, -150.0, -50.0], [300.0, 150.0, 0.0]),
            (parts::HEAD, [-110.0, -110.0, 500.0], [110.0, 310.0, 800.0]),
            (parts::SPINDLE, [-45.0, -45.0, 400.0], [45.0, 45.0, 500.0]),
            (parts::MOTOR, [-60.0, -60.0, 800.0], [60.0, 60.0, 950.0]),
            (
                parts::ROTARY_BODY,
                [-290.0, -80.0, 0.0],
                [-140.0, 80.0, 170.0],
            ),
            (parts::CHUCK, [-140.0, -62.5, 37.5], [-70.0, 62.5, 162.5]),
            (
                parts::MAGAZINE_BRACKET,
                [150.0, 320.0, 560.0],
                [400.0, 400.0, 620.0],
            ),
            (
                parts::CAROUSEL,
                [190.0, 190.0, 500.0],
                [530.0, 530.0, 560.0],
            ),
            (
                parts::ROTARY_TABLE_BASE,
                [-130.0, -130.0, 0.0],
                [130.0, 130.0, 80.0],
            ),
            (
                parts::PLATTER,
                [-110.0, -110.0, 80.0],
                [110.0, 110.0, 110.0],
            ),
            (
                parts::Z_SLIDE,
                [-110.0, 170.0, 500.0],
                [110.0, 310.0, 800.0],
            ),
            (
                parts::SWIVEL_HEAD,
                [-90.0, -110.0, 500.0],
                [90.0, 170.0, 800.0],
            ),
        ],
    );
    boxes(
        cnc::WORKHOLDING,
        &[
            (work::VISE, [-10.0, -60.0, -45.0], [70.0, 60.0, 8.0]),
            (work::CHUCK, [-70.0, -62.5, -62.5], [0.0, 62.5, 62.5]),
            (
                work::FIXTURE_PLATE,
                [-25.0, -60.0, -60.0],
                [0.0, 60.0, 60.0],
            ),
        ],
    );
    boxes(
        cnc::TEST_PIECE,
        &[(work::PIECE, [-25.0, -25.0, -20.0], [150.0, 25.0, 20.0])],
    );
    let tip = work::WORM_TIP_RADIUS;
    boxes(
        cnc::WORM,
        &[(
            work::WORM_PART,
            [work::WORM_START - work::WORM_SPIGOT, -tip, -tip],
            [work::WORM_START + work::WORM_LENGTH, tip, tip],
        )],
    );
    // The parts carry their names.
    let named = |studio: ElementId, part: PartId| {
        let el = doc.element(studio).unwrap();
        let b = cadrs_core::rebuild::build(el.features());
        cadrs_core::parts::display_name(b.part(part).unwrap(), el.part_props()).to_string()
    };
    for (part, name) in parts::PARTS {
        assert_eq!(named(cnc::MACHINE_PARTS, part), name);
    }
    assert_eq!(named(cnc::TEST_PIECE, work::PIECE), "Test piece");
    assert_eq!(named(cnc::WORM, work::WORM_PART), "Worm");
    assert_eq!(named(cnc::WORKHOLDING, work::VISE), "Vise");
}

/// How far `p` is from the X axis.
fn off_x(p: [f64; 3]) -> f64 {
    p[1].hypot(p[2])
}

#[test]
fn the_test_piece_has_a_feature_for_each_machine() {
    use cadrs_kernel::SurfaceKind;
    let doc = cnc::document().unwrap();
    let b = build(&doc, cnc::TEST_PIECE);
    let s = solid(&b, work::PIECE);
    let along = |a: [f64; 3], b: [f64; 3]| (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs();
    // 3-axis: the pocket's floor and the two Ø6 holes along Z.
    assert!(s.faces.iter().any(|f| {
        f.plane.is_some() && f.center.is_some_and(|c| dist(c, work::POCKET_FLOOR) < 1e-3)
    }));
    for h in work::HOLES {
        assert!(
            s.faces.iter().any(|f| {
                f.kind == Some(SurfaceKind::Cylinder)
                    && f.radius.is_some_and(|r| (r - 3.0).abs() < 1e-6)
                    && f.axis.is_some_and(|(p, d)| {
                        along(d, [0.0, 0.0, 1.0]) > 1.0 - 1e-9
                            && (p[0] - h[0]).hypot(p[1] - h[1]) < 1e-6
                    })
            }),
            "the hole at {h:?}"
        );
    }
    // 4th axis: three flats 17 from the axis, 120° apart, one facing +Z; and the groove, 2.5
    // deep into the shaft between its ends.
    let flats: Vec<[f64; 3]> = s
        .faces
        .iter()
        .filter(|f| f.plane.is_some())
        .filter_map(|f| f.center)
        .filter(|c| (c[0] - work::TOP_FLAT[0]).abs() < 1e-3 && (off_x(*c) - 17.0).abs() < 1e-3)
        .collect();
    assert_eq!(flats.len(), 3, "{flats:?}");
    assert!(flats.iter().any(|c| dist(*c, work::TOP_FLAT) < 1e-3));
    let (x0, x1) = work::GROOVE_X;
    let deepest = s
        .positions
        .iter()
        .filter(|p| p[0] > x0 + 1.0 && p[0] < x1 - 1.0)
        .map(|p| off_x(*p))
        .fold(f64::INFINITY, f64::min);
    assert!(
        (deepest - (work::SHAFT_RADIUS - 2.5)).abs() < 0.02,
        "the groove's bottom is {deepest} from the axis"
    );
    // 5-axis: the dome, and a Ø4 hole along each of the four directions, 6 deep.
    assert!(s.faces.iter().any(|f| {
        f.kind == Some(SurfaceKind::Sphere)
            && f.radius
                .is_some_and(|r| (r - work::DOME_RADIUS).abs() < 1e-6)
    }));
    for axis in work::DOME_HOLE_AXES {
        assert!(
            s.faces.iter().any(|f| {
                f.kind == Some(SurfaceKind::Cylinder)
                    && f.radius.is_some_and(|r| (r - 2.0).abs() < 1e-6)
                    && f.axis.is_some_and(|(_, d)| along(d, axis) > 1.0 - 1e-9)
            }),
            "the dome hole along {axis:?}"
        );
        // Its floor: a flat face 6 under the dome's surface, on the hole's axis.
        let floor: [f64; 3] = [0, 1, 2].map(|k| work::DOME_CENTRE[k] + 14.0 * axis[k]);
        assert!(
            s.faces
                .iter()
                .any(|f| f.plane.is_some() && f.center.is_some_and(|c| dist(c, floor) < 1e-3)),
            "the floor of the dome hole along {axis:?}"
        );
    }
    // The worm's thread stands on its core: the root between the turns, the tips over them.
    let b = build(&doc, cnc::WORM);
    let s = solid(&b, work::WORM_PART);
    let (lo, hi) = s
        .positions
        .iter()
        .filter(|p| p[0] > work::WORM_START + 10.0 && p[0] < work::WORM_START + 40.0)
        .map(|p| off_x(*p))
        .fold((f64::INFINITY, 0.0_f64), |(lo, hi), r| {
            (lo.min(r), hi.max(r))
        });
    assert!((lo - work::WORM_ROOT_RADIUS).abs() < 0.02, "root {lo}");
    assert!((hi - work::WORM_TIP_RADIUS).abs() < 0.02, "tip {hi}");
}

/// The solver's model of the assembly `el` and its source solids.
fn flat(doc: &Document, el: ElementId) -> (assembly::Assembly, HashMap<InstanceId, Arc<Solid>>) {
    let asm = doc.element(el).unwrap().assembly_model().unwrap();
    (
        structure::solver_model(doc, asm),
        assembly::document_occurrence_solids(doc, el),
    )
}

/// The instances that move, and where to, when `mate`'s `dof` is driven to `value` from home
/// the way Apply limit position and Animate do it: the mate snapped, its first connector's
/// instance the mover. Solving with the drive alone must come to the same placements.
fn driven(
    doc: &Document,
    el: ElementId,
    mate: MateId,
    dof: Dof,
    value: f64,
) -> HashMap<InstanceId, Pose> {
    let (model, solids) = flat(doc, el);
    let drives = vec![Drive { mate, dof, value }];
    let first = model.mate(mate).unwrap().mate().unwrap().connectors[0].instance;
    let sol = assembly::solve(
        &model,
        &solids,
        &SolveOptions {
            movers: vec![first],
            snap: Some(mate),
            drives: drives.clone(),
            ..Default::default()
        },
    );
    assert!(sol.converged, "residual {:e}", sol.residual);
    let plain = assembly::solve(
        &model,
        &solids,
        &SolveOptions {
            drives,
            ..Default::default()
        },
    );
    assert!(plain.converged, "residual {:e}", plain.residual);
    // A point off every axis: it moves with any motion of its instance.
    let probe = [37.0, -21.0, 13.0];
    for (a, b) in sol.poses.iter().zip(&plain.poses) {
        assert!(dist(a.1.apply(probe), b.1.apply(probe)) < 1e-6, "{}", a.0);
    }
    sol.poses
        .into_iter()
        .filter(|(_, p)| dist(p.apply(probe), probe) > 1e-6)
        .collect()
}

fn same_set(moved: &HashMap<InstanceId, Pose>, want: &[InstanceId]) {
    let mut got: Vec<InstanceId> = moved.keys().copied().collect();
    let mut want = want.to_vec();
    got.sort_by_key(|i| i.0);
    want.sort_by_key(|i| i.0);
    assert_eq!(got, want);
}

#[test]
fn each_machine_solves_as_built() {
    let doc = cnc::document().unwrap();
    let deg = |a: f64| a.to_radians();
    // An axis: its mate's name, type and limits (mm, or radians).
    type Axis<'a> = (&'a str, MateType, Option<(f64, f64)>);
    let machines: [(ElementId, usize, &[Axis]); 3] = [
        (
            mill3::ASSEMBLY,
            7,
            &[
                ("Y", MateType::Slider, Some((-150.0, 150.0))),
                ("X", MateType::Slider, Some((-250.0, 250.0))),
                ("Z", MateType::Slider, Some((-350.0, 0.0))),
            ],
        ),
        (
            mill4::ASSEMBLY,
            11,
            &[
                ("Y", MateType::Slider, Some((-150.0, 150.0))),
                ("X", MateType::Slider, Some((-250.0, 250.0))),
                ("Z", MateType::Slider, Some((-350.0, 0.0))),
                ("A", MateType::Revolute, None),
                ("Magazine", MateType::Revolute, None),
            ],
        ),
        (
            mill5::ASSEMBLY,
            10,
            &[
                ("Y", MateType::Slider, Some((-150.0, 150.0))),
                ("X", MateType::Slider, Some((-250.0, 250.0))),
                ("C", MateType::Revolute, None),
                ("Z", MateType::Slider, Some((-300.0, 0.0))),
                ("B", MateType::Revolute, Some((deg(-100.0), deg(100.0)))),
            ],
        ),
    ];
    for (el, instances, axes) in machines {
        let asm = doc.element(el).unwrap().assembly_model().unwrap();
        assert_eq!(asm.instances.len(), instances);
        assert!(asm.instances.iter().all(|i| i.pose == Pose::IDENTITY));
        assert_eq!(asm.instances.iter().filter(|i| i.fixed).count(), 1);
        // The axes, in order, among the Fastened mates.
        let got: Vec<Axis> = asm
            .mates
            .iter()
            .filter_map(|f| Some((f, f.mate()?)))
            .filter(|(_, m)| m.mate_type != MateType::Fastened)
            .map(|(f, m)| (f.name.as_str(), m.mate_type, m.limit(m.mate_type.dof()[0])))
            .collect();
        assert_eq!(got, axes, "{}", doc.element(el).unwrap().name);
        // Each is as the sample's own table has it: the first connector on what the axis
        // carries, the second on what carries it, both one frame on the axis whose Z points
        // against the axis's direction.
        let table: &[cnc::assemblies::Axis] = match instances {
            7 => &mill3::AXES,
            11 => &mill4::AXES,
            _ => &mill5::AXES,
        };
        assert_eq!(table.len(), axes.len());
        for (a, want) in table.iter().zip(axes) {
            let f = asm.mate(a.mate).unwrap();
            assert_eq!(f.name, want.0);
            assert_eq!(a.name, want.0);
            assert_eq!(a.dof(), f.mate().unwrap().mate_type.dof()[0]);
            let [first, second] = f.mate().unwrap().connectors;
            assert_eq!((first.instance, second.instance), (a.child, a.parent));
            assert_eq!(first.frame, second.frame);
            assert!(dist(first.frame.origin, a.origin) < 1e-12);
            assert!(dist(first.frame.z, a.direction.map(|c| -c)) < 1e-12);
        }
        // Every other mate is Fastened, its first connector on the part that is held.
        let fastened = asm.mates.len() - axes.len();
        assert_eq!(fastened, instances - 1 - axes.len());
        // Solved, nothing moves.
        let (model, solids) = flat(&doc, el);
        let sol = assembly::solve(&model, &solids, &SolveOptions::default());
        assert!(sol.converged, "residual {:e}", sol.residual);
        assert_eq!(sol.changed(&model), vec![]);
        assert!(assembly::lost_mates(&model, &solids).is_empty());
    }
}

#[test]
fn an_axis_moves_only_what_it_carries() {
    let doc = cnc::document().unwrap();
    let moved_by = |p: &Pose, t: [f64; 3]| {
        dist(p.rotate([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]) < 1e-9
            && dist(p.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]) < 1e-9
            && dist(p.translation, t) < 1e-6
    };
    // X by +100: the table and what stands on it, 100 along X.
    let carried: [(ElementId, MateId, Vec<InstanceId>); 3] = [
        (mill3::ASSEMBLY, mill3::X, vec![mill3::TABLE]),
        (
            mill4::ASSEMBLY,
            mill4::X,
            vec![mill4::TABLE, mill4::ROTARY_BODY, mill4::CHUCK],
        ),
        (
            mill5::ASSEMBLY,
            mill5::X,
            vec![mill5::TABLE, mill5::ROTARY_TABLE_BASE, mill5::PLATTER],
        ),
    ];
    for (el, x, want) in carried {
        let moved = driven(&doc, el, x, Dof::Z, 100.0);
        same_set(&moved, &want);
        assert!(moved.values().all(|p| moved_by(p, [100.0, 0.0, 0.0])));
    }
    // Y by −50 takes the saddle and the table; Z by −120 the head, the spindle and the motor.
    let moved = driven(&doc, mill3::ASSEMBLY, mill3::Y, Dof::Z, -50.0);
    same_set(&moved, &[mill3::SADDLE, mill3::TABLE]);
    assert!(moved.values().all(|p| moved_by(p, [0.0, -50.0, 0.0])));
    let moved = driven(&doc, mill3::ASSEMBLY, mill3::Z, Dof::Z, -120.0);
    same_set(&moved, &[mill3::HEAD, mill3::SPINDLE, mill3::MOTOR]);
    assert!(moved.values().all(|p| moved_by(p, [0.0, 0.0, -120.0])));
    // A by 90°: only the chuck, about its axis (+X): what was above the axis goes to −Y.
    let quarter = std::f64::consts::FRAC_PI_2;
    let moved = driven(&doc, mill4::ASSEMBLY, mill4::A, Dof::Angle, quarter);
    same_set(&moved, &[mill4::CHUCK]);
    let axis = parts::CHUCK_AXIS;
    let p = moved[&mill4::CHUCK].apply([axis[0], axis[1], axis[2] + 50.0]);
    assert!(dist(p, [axis[0], axis[1] - 50.0, axis[2]]) < 1e-6, "{p:?}");
    // The magazine by 45°: only the carousel, about its own axis.
    let moved = driven(
        &doc,
        mill4::ASSEMBLY,
        mill4::MAGAZINE,
        Dof::Angle,
        quarter / 2.0,
    );
    same_set(&moved, &[mill4::CAROUSEL]);
    let c = parts::CAROUSEL_AXIS;
    assert!(dist(moved[&mill4::CAROUSEL].apply(c), c) < 1e-6);
    // C by 90°: only the platter, about Z: +X goes to +Y.
    let moved = driven(&doc, mill5::ASSEMBLY, mill5::C, Dof::Angle, quarter);
    same_set(&moved, &[mill5::PLATTER]);
    let p = moved[&mill5::PLATTER].apply([80.0, 0.0, parts::PLATTER_TOP]);
    assert!(dist(p, [0.0, 80.0, parts::PLATTER_TOP]) < 1e-6, "{p:?}");
    // B by 90°: the swivel head with the spindle and the motor, about the pivot (+Y): the
    // spindle nose, 200 under the pivot, swings out to −X at the pivot's height.
    let moved = driven(&doc, mill5::ASSEMBLY, mill5::B, Dof::Angle, quarter);
    same_set(&moved, &[mill5::SWIVEL_HEAD, mill5::SPINDLE, mill5::MOTOR]);
    let pivot = parts::HEAD_PIVOT;
    let nose = moved[&mill5::SPINDLE].apply([0.0, 0.0, parts::SPINDLE_NOSE]);
    assert!(
        dist(nose, [parts::SPINDLE_NOSE - pivot[2], 0.0, pivot[2]]) < 1e-6,
        "{nose:?}"
    );
    // The 5-axis machine's Z by −100: the slide and all it carries.
    let moved = driven(&doc, mill5::ASSEMBLY, mill5::Z, Dof::Z, -100.0);
    same_set(
        &moved,
        &[
            mill5::Z_SLIDE,
            mill5::SWIVEL_HEAD,
            mill5::SPINDLE,
            mill5::MOTOR,
        ],
    );
    assert!(moved.values().all(|p| moved_by(p, [0.0, 0.0, -100.0])));
}

#[test]
fn the_pockets_turn_the_magazine() {
    let doc = cnc::document().unwrap();
    let asm = doc
        .element(mill4::ASSEMBLY)
        .unwrap()
        .assembly_model()
        .unwrap();
    assert_eq!(asm.named_positions.len(), parts::POCKETS);
    let (model, solids) = flat(&doc, mill4::ASSEMBLY);
    let c = parts::CAROUSEL_AXIS;
    let r = parts::POCKET_CIRCLE / 2.0;
    // The first pocket's centre on the carousel at home.
    let first = [c[0] + r, c[1], c[2]];
    for k in 1..=parts::POCKETS {
        let p = asm.named_position(mill4::pocket(k as u128)).unwrap();
        assert_eq!(p.name, format!("Pocket {k}"));
        // Applied, only the carousel moves, to where the position has it: its first pocket
        // 45° on for each.
        let sol = positions::solve_to(&model, &solids, p);
        assert!(sol.converged, "Pocket {k}: residual {:e}", sol.residual);
        let a = (45.0 * (k as f64 - 1.0)).to_radians();
        let want = [c[0] + r * a.cos(), c[1] + r * a.sin(), c[2]];
        for (id, pose) in &sol.poses {
            let stored = p.pose_of(*id).unwrap();
            assert!(
                dist(pose.apply(first), stored.apply(first)) < 1e-6,
                "Pocket {k}: {id}"
            );
            if *id == mill4::CAROUSEL {
                assert!(dist(pose.apply(first), want) < 1e-6, "Pocket {k}");
            } else {
                assert_eq!(stored, Pose::IDENTITY);
            }
        }
    }
}

#[test]
fn the_machines_are_light() {
    let doc = cnc::document().unwrap();
    let b = build(&doc, cnc::MACHINE_PARTS);
    for el in [mill3::ASSEMBLY, mill4::ASSEMBLY, mill5::ASSEMBLY] {
        let asm = doc.element(el).unwrap().assembly_model().unwrap();
        let (view, _) = assembly::instance_parts(&doc, asm, |_| Some(b.clone()));
        assert_eq!(view.len(), asm.instances.len());
        let triangles: usize = view.iter().map(|p| p.solid.indices.len() / 3).sum();
        eprintln!("{}: {triangles} triangles", doc.element(el).unwrap().name);
        assert!(triangles < 20_000, "{triangles}");
    }
}

#[test]
fn the_cam_studios_jobs_run_on_their_assemblies_machines() {
    let doc = cnc::document().unwrap();
    // Each job: its name, its machine's model, the assembly it is taken from, its axes, how
    // much holds the work and how many operations it has.
    type Row = (String, String, ElementId, String, usize, usize);
    let jobs = |studio: ElementId| -> Vec<Row> {
        let cam = doc.element(studio).unwrap().cam().unwrap();
        cam.jobs
            .iter()
            .map(|j| {
                let m = j.machine.as_ref().unwrap();
                let axes: Vec<String> = m.kinematics().joints().map(|a| a.name.clone()).collect();
                (
                    j.name.clone(),
                    m.model.clone(),
                    j.machine_source.as_ref().unwrap().reference.element,
                    axes.join(" "),
                    j.workholdings.len(),
                    j.setups[0].operations.len(),
                )
            })
            .collect()
    };
    let job = |name: &str, machine: &str, from: ElementId, axes: &str, held: usize, ops: usize| {
        (
            name.to_string(),
            machine.to_string(),
            from,
            axes.to_string(),
            held,
            ops,
        )
    };
    assert_eq!(
        jobs(cam3::STUDIO),
        [job("Job 1", "3-axis mill", cnc::MILL_3, "Z Y X", 1, 4)]
    );
    assert_eq!(
        jobs(cam4::STUDIO),
        [
            job("Job 1", "4-axis mill", cnc::MILL_4, "Z Y X A", 1, 5),
            job("Worm", "4-axis mill", cnc::MILL_4, "Z Y X A", 0, 4)
        ]
    );
    assert_eq!(
        jobs(cam5::STUDIO),
        [job("Job 1", "5-axis mill", cnc::MILL_5, "Z B Y X C", 1, 8)]
    );
    // The 4-axis mill's magazine is an actuator with its eight pockets, and the tool change
    // uses it.
    let def = doc
        .element(cnc::MILL_4)
        .unwrap()
        .assembly_model()
        .unwrap()
        .machine
        .clone()
        .unwrap();
    assert_eq!(def.actuators.len(), 1);
    assert_eq!(def.actuators[0].positions.len(), 8);
    assert_eq!(def.tool_change.magazine, Some(cnc::machines::MAGAZINE));
    assert_eq!(def.tool_change.pockets.len(), 8);
    // The components follow their Part Studios' workspaces, and build.
    for (studio, n) in [(cam3::STUDIO, 2), (cam4::STUDIO, 3), (cam5::STUDIO, 2)] {
        let cam = doc.element(studio).unwrap().cam().unwrap();
        assert_eq!(cam.components.len(), n);
        assert!(
            cam.components
                .iter()
                .all(|c| c.reference_label().ends_with("(workspace)"))
        );
        let b = cadrs_core::rebuild::build(cam.part_features());
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert_eq!(b.parts.len(), n);
    }
    // The 5-axis setup looks along the piece's axis, and its work plane along a dome hole.
    let cam = doc.element(cam5::STUDIO).unwrap().cam().unwrap();
    assert_eq!(cam.setup(cam5::SETUP).unwrap().1.wcs.z, [1.0, 0.0, 0.0]);
    let (plane, wp) = cam.work_plane_for(cam5::DRILL).unwrap();
    assert_eq!(plane, Some(cam5::HOLE_PLANE));
    assert_eq!(wp.z, work::DOME_HOLE_AXES[cam5::HOLE]);
}

/// The toolpath of the operation `op` of the CAM Studio `studio`, its model from the kernel.
fn toolpath(doc: &Document, studio: ElementId, op: CamId) -> cadrs_cam::Toolpath {
    let cam = doc.element(studio).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let input = program::op_input(cam, op, &parts).unwrap();
    let model = |levels: Vec<f64>| {
        geometry::model(program::model_request(cam, &input, levels))
            .wait()
            .expect("the kernel")
            .unwrap()
    };
    let first = model(vec![]);
    let levels = program::section_levels(&input, &first);
    let model = if levels.is_empty() {
        first
    } else {
        model(levels)
    };
    let name = &cam.operation(op).unwrap().2.name;
    program::generate(&input, &model, None).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The box of a toolpath's cutting moves (in its work plane's coordinates).
fn cuts(tp: &cadrs_cam::Toolpath) -> cadrs_cam::Aabb {
    use cadrs_cam::toolpath::{FeedKind, Step};
    cadrs_cam::Aabb::of(tp.steps.iter().filter_map(|s| match s {
        Step::Feed {
            to,
            kind: FeedKind::Cut,
            ..
        } => Some(*to),
        _ => None,
    }))
}

/// The NC program of the setup `setup` of the CAM Studio `studio`, from its toolpaths `paths`.
/// The machine must make every move of it within its travel.
fn posted(
    doc: &Document,
    studio: ElementId,
    setup: CamId,
    paths: &HashMap<CamId, cadrs_cam::Toolpath>,
) -> String {
    let cam = doc.element(studio).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let (job, setup) = cam.setup(setup).unwrap();
    let run = program::setup_run(cam, job, setup, &parts, paths, None).unwrap();
    let stock = program::stock_in(&program::stock_shape(cam, job, &parts).unwrap(), &run.frame);
    let beyond = program::travel_warnings(job, setup, &run, &stock);
    assert!(beyond.is_empty(), "{beyond:?}");
    program::post_setup(setup, job, &run).unwrap()
}

/// The poses a toolpath's cutting moves end at: the tip and the tool's axis.
fn poses(tp: &cadrs_cam::Toolpath) -> Vec<([f64; 3], [f64; 3])> {
    use cadrs_cam::toolpath::{FeedKind, Step};
    let mut axis = [0.0, 0.0, 1.0];
    let mut out = Vec::new();
    for s in &tp.steps {
        match s {
            Step::Axis(a) => axis = *a,
            Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } => out.push((*to, axis)),
            _ => {}
        }
    }
    out
}

/// Where a toolpath drills (in its work plane's coordinates).
fn drilled(tp: &cadrs_cam::Toolpath) -> Vec<[f64; 2]> {
    tp.steps
        .iter()
        .filter_map(|s| match s {
            cadrs_cam::Step::Drill(c) => Some(c.at),
            _ => None,
        })
        .collect()
}

/// The origin of the Test piece's setups that look down Z: the top centre of its stock (the
/// piece is 175 × 50 × 40 from (−25, −25, −20); its stock 2 round it and 1 over it).
const STOCK_TOP: [f64; 3] = [62.5, 0.0, 21.0];

#[test]
fn the_3_axis_toolpaths_compute() {
    let doc = cnc::document().unwrap();
    // The rough clears the stock over the pocket (a tool radius round it) and the pocket down
    // to its floor, less the 0.3 it leaves.
    let rough = toolpath(&doc, cam3::STUDIO, cam3::ROUGH);
    assert!(rough.warnings.is_empty(), "{:?}", rough.warnings);
    let b = cuts(&rough);
    let floor = work::POCKET_FLOOR;
    let (x0, x1) = (
        floor[0] - 18.0 - STOCK_TOP[0],
        floor[0] + 18.0 - STOCK_TOP[0],
    );
    assert!(b.min[0] > x0 - 1e-3 && b.max[0] < x1 + 1e-3, "{b:?}");
    assert!(b.min[1] > -13.0 - 1e-3 && b.max[1] < 13.0 + 1e-3, "{b:?}");
    assert!(
        (b.min[2] - (floor[2] + 0.3 - STOCK_TOP[2])).abs() < 1e-3,
        "{b:?}"
    );
    // Both holes are drilled, through the block (its underside is 36 under the stock's top).
    let drill = toolpath(&doc, cam3::STUDIO, cam3::DRILL);
    assert!(drill.warnings.is_empty(), "{:?}", drill.warnings);
    let mut at = drilled(&drill);
    at.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(at.len(), 2);
    for (got, want) in at.iter().zip(work::HOLES) {
        assert!(
            (got[0] - (want[0] - STOCK_TOP[0])).hypot(got[1] - want[1]) < 1e-6,
            "{got:?}"
        );
    }
    for s in &drill.steps {
        if let cadrs_cam::Step::Drill(c) = s {
            assert!(c.bottom < -15.0 - STOCK_TOP[2], "{c:?}");
        }
    }
    // The setup posts: both tools, the drilling as a cycle.
    let paths = HashMap::from([(cam3::ROUGH, rough), (cam3::DRILL, drill)]);
    let nc = posted(&doc, cam3::STUDIO, cam3::SETUP, &paths);
    assert!(nc.contains("O1001"), "{nc}");
    assert!(nc.contains("T1 M6") && nc.contains("T2 M6"), "{nc}");
    assert!(nc.contains("G81") || nc.contains("G83"), "{nc}");
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_4_axis_toolpaths_compute() {
    let doc = cnc::document().unwrap();
    // The flat that faces +Z is faced at its height, between its ends.
    let face = toolpath(&doc, cam4::STUDIO, cam4::FACE);
    assert!(face.warnings.is_empty(), "{:?}", face.warnings);
    let b = cuts(&face);
    let flat = work::TOP_FLAT;
    assert!((b.min[2] - (flat[2] - STOCK_TOP[2])).abs() < 1e-3, "{b:?}");
    assert!(
        b.min[0] > flat[0] - 7.5 - STOCK_TOP[0] && b.max[0] < flat[0] + 7.5 - STOCK_TOP[0],
        "{b:?}"
    );
    // The flat 120° round is faced in its own plane, the tool along its normal.
    let round = toolpath(&doc, cam4::STUDIO, cam4::FACE_ROUND);
    assert!(!round.is_empty());
    // Round the shaft the ball's tip keeps to the shaft (20 from the axis), dipping into the
    // groove, the tool pointing at the axis all the way.
    let shaft = toolpath(&doc, cam4::STUDIO, cam4::SHAFT);
    assert!(shaft.is_multi_axis());
    // (The setup's origin is the stock's top centre: the axis is 21 under it.)
    let from_axis = |p: [f64; 3]| p[1].hypot(p[2] + STOCK_TOP[2]);
    let ps = poses(&shaft);
    assert!(ps.len() > 500, "{}", ps.len());
    for (tip, axis) in &ps {
        let r = from_axis(*tip);
        assert!(
            r < work::SHAFT_RADIUS + 0.05 && r > work::SHAFT_RADIUS - 3.0,
            "{tip:?}: {r}"
        );
        assert!(axis[0].abs() < 1e-9);
    }
    assert!(
        ps.iter()
            .any(|(tip, _)| from_axis(*tip) < work::SHAFT_RADIUS - 0.15)
    );
    // The setup posts for the 4-axis mill: the chuck indexed a third of a turn for the second
    // flat, then turning on and on under the ball, with inverse-time feeds.
    let (face_tp, round_tp, shaft_tp) = (face.clone(), round.clone(), shaft.clone());
    let paths = HashMap::from([
        (cam4::FACE, face),
        (cam4::FACE_ROUND, round),
        (cam4::SHAFT, shaft),
    ]);
    let nc = posted(&doc, cam4::STUDIO, cam4::SETUP, &paths);
    assert!(nc.contains("O1001") || nc.contains("T1 M6"), "{nc}");
    let moves = cadrs_cam::post::reader::read(&nc);
    assert!(
        moves
            .iter()
            .any(|m| m.rapid && (m.angle('A').abs() - 120.0).abs() < 1e-6),
        "{nc}"
    );
    let turned = moves.iter().map(|m| m.angle('A').abs()).fold(0.0, f64::max);
    assert!(turned > 360.0 * 10.0 && nc.contains("G93"), "{turned}");
    // The worm: roughed in levels, then finished along its thread at its own lead.
    let rough = toolpath(&doc, cam4::STUDIO, cam4::ROUGH);
    assert!(rough.is_multi_axis() && !rough.is_empty());
    let thread = toolpath(&doc, cam4::STUDIO, cam4::THREAD);
    let ps = poses(&thread);
    assert!(ps.len() > 1000, "{}", ps.len());
    // Along each pass X goes on by the lead for every turn: over two neighbouring places the
    // step along the axis is the lead times the turn.
    let angle = |a: [f64; 3]| a[2].atan2(a[1]).to_degrees();
    let mut in_step = 0;
    for w in ps.windows(2) {
        let mut turn = angle(w[1].1) - angle(w[0].1);
        if turn.abs() > 180.0 {
            turn -= 360.0 * turn.signum();
        }
        if turn.abs() > 1e-6 && turn.abs() < 10.0 {
            let lead = (w[1].0[0] - w[0].0[0]) / turn.abs() * 360.0;
            assert!((lead.abs() - work::WORM_LEAD).abs() < 1e-4, "{lead}");
            in_step += 1;
        }
    }
    assert!(in_step > 1000);
    // It reaches the thread's root and leaves its tips.
    let deepest = ps
        .iter()
        .map(|(t, _)| from_axis_of_worm(*t))
        .fold(f64::INFINITY, f64::min);
    assert!(deepest < work::WORM_ROOT_RADIUS + 0.3, "{deepest}");
    // Verify on the solid stock: the bar is cut down to the worm, the thread's spaces opened,
    // without a cut or a rapid going into the worm.
    {
        let paths = HashMap::from([(cam4::ROUGH, rough.clone()), (cam4::THREAD, thread.clone())]);
        let cam = doc.element(cam4::STUDIO).unwrap().cam().unwrap();
        let parts = cadrs_core::rebuild::build(cam.part_features())
            .parts
            .clone();
        let (job, setup) = cam.setup(cam4::WORM_SETUP).unwrap();
        let run = program::setup_run(cam, job, setup, &parts, &paths, None).unwrap();
        assert!(program::run_needs_solid(&run));
        let r = program::simulate_solid(cam, job, setup, &run, &parts, &paths, None, 0.3).unwrap();
        assert!(r.removed > 2000.0, "{}", r.removed);
        assert!(
            r.gouge_depth < 0.3,
            "gouged {} at {:?}",
            r.gouge_depth,
            r.gouges.first()
        );
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        // Simulate's stock as the run plays, cut on from where it was to each place in turn
        // (part way along moves too): less and less of the bar, down to what the whole run
        // leaves.
        let start = program::solid_start(cam, job, setup, &run, &parts, &paths, None, 0.3).unwrap();
        let moves: usize = start.ops.iter().map(|(_, m)| m.len()).sum();
        let mut live = cadrs_core::cam::cadrs_cam::sim::LiveStock::new(start.stock, start.ops);
        let mut seen = Vec::new();
        for i in 1..40 {
            live.cut_to(i * moves / 40, 0.5);
            seen.push(live.stock.volume());
        }
        live.cut_to(moves - 1, 1.0);
        assert!(seen.windows(2).all(|w| w[1] <= w[0] + 1e-6), "{seen:?}");
        assert!(seen[0] - seen[seen.len() - 1] > 0.5 * r.removed, "{seen:?}");
        assert!(
            (live.stock.volume() - r.stock.volume()).abs() < 1e-3 * r.removed,
            "{} {}",
            live.stock.volume(),
            r.stock.volume()
        );
    }
    let (rough_tp, thread_tp) = (rough.clone(), thread.clone());
    let nc = posted(
        &doc,
        cam4::STUDIO,
        cam4::WORM_SETUP,
        &HashMap::from([(cam4::ROUGH, rough), (cam4::THREAD, thread)]),
    );
    assert!(nc.contains("O1002") || nc.contains("T2 M6"), "{nc}");
    assert!(nc.contains("G93") && nc.contains(" A"), "{nc}");
    // Every control posts both setups. A machine with one rotary axis has no tool centre
    // point control and no tilted working plane: plain X Y Z A.
    let piece = HashMap::from([
        (cam4::FACE, face_tp.clone()),
        (cam4::FACE_ROUND, round_tp.clone()),
        (cam4::SHAFT, shaft_tp.clone()),
    ]);
    let worm = HashMap::from([
        (cam4::ROUGH, rough_tp.clone()),
        (cam4::THREAD, thread_tp.clone()),
    ]);
    for (setup, paths) in [(cam4::SETUP, &piece), (cam4::WORM_SETUP, &worm)] {
        for (d, nc) in on_every_control(&doc, cam4::STUDIO, setup, paths) {
            runnable(d, &nc);
            for five_axis in ["G43.4", "G234", "M428", "G68.2"] {
                assert!(!nc.contains(five_axis), "{d:?} wrote {five_axis}");
            }
            // The program never touches the work offsets to count the axis down.
            for offsets in ["G92", "G10 L20", "G10 L2 "] {
                assert!(!nc.contains(offsets), "{d:?} writes {offsets}");
            }
            // On a control that rolls the axis over (all but GRBL) the angles stay within a
            // turn; GRBL's are written as turned.
            let most = largest_word(&nc, 'A');
            let rolls = d != cadrs_cam::Dialect::Grbl;
            assert!(!rolls || most < 360.0, "{d:?}: A{most}");
        }
    }
}

/// The setup's program on each of the five controls, tool centre point control as each
/// control has it by default: what the job's machine would post if it were on that control.
fn on_every_control(
    doc: &Document,
    studio: ElementId,
    setup: CamId,
    paths: &HashMap<CamId, cadrs_cam::Toolpath>,
) -> Vec<(cadrs_cam::Dialect, String)> {
    let cam = doc.element(studio).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let (job, setup) = cam.setup(setup).unwrap();
    let run = program::setup_run(cam, job, setup, &parts, paths, None).unwrap();
    cadrs_cam::Dialect::ALL
        .into_iter()
        .map(|d| {
            let mut j = job.clone();
            let m = j.machine.as_mut().unwrap();
            m.dialect = d;
            m.post = cadrs_cam::PostSettings::for_dialect(d, cadrs_cam::MachineKind::Mill);
            let nc = program::post_setup(setup, &j, &run).unwrap_or_else(|e| panic!("{d:?}: {e}"));
            (d, nc)
        })
        .collect()
}

/// What every program a machinist would run is held to: no hair's-breadth block at the
/// largest inverse-time feed, and the tool's length switched on with a move down in Z
/// (`G0 G43 H1 Z…`), never by itself where the control would move to apply it.
fn runnable(d: cadrs_cam::Dialect, nc: &str) {
    assert!(!nc.contains("F99999"), "{d:?}: a block at F99999");
    for l in nc.lines().filter(|l| l.contains("G43 H")) {
        assert!(l.contains('Z'), "{d:?}: {l}");
    }
}

/// The largest rotary word a program writes for `letter`.
fn largest_word(nc: &str, letter: char) -> f64 {
    nc.lines()
        .filter(|l| !l.trim_start().starts_with('('))
        .flat_map(|l| l.split_whitespace())
        .filter_map(|w| w.strip_prefix(letter)?.parse::<f64>().ok())
        .fold(0.0, |m: f64, v| m.max(v.abs()))
}

/// A point's distance from the worm's axis, in the Worm setup's coordinates (its origin the
/// top centre of the bar: 1 over the thread's tips).
fn from_axis_of_worm(p: [f64; 3]) -> f64 {
    p[1].hypot(p[2] + work::WORM_TIP_RADIUS + 1.0)
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_5_axis_toolpaths_compute() {
    let doc = cnc::document().unwrap();
    // The ball mill goes over the dome with its axis square to it, up to 60° over.
    let parallel = toolpath(&doc, cam5::STUDIO, cam5::PARALLEL);
    assert!(parallel.is_multi_axis());
    let ps = poses(&parallel);
    assert!(ps.len() > 200, "{}", ps.len());
    let lean = |a: [f64; 3]| a[2].clamp(-1.0, 1.0).acos().to_degrees();
    assert!(ps.iter().all(|(_, a)| lean(*a) <= 60.0 + 1e-6));
    assert!(ps.iter().any(|(_, a)| lean(*a) > 45.0));
    // The four holes in the dome, each down its own axis (45° over, a quarter turn apart).
    let all = toolpath(&doc, cam5::STUDIO, cam5::DRILL_ALL);
    let mut axes: Vec<[f64; 3]> = Vec::new();
    let mut axis = [0.0, 0.0, 1.0];
    for s in &all.steps {
        match s {
            cadrs_cam::Step::Axis(a) => axis = *a,
            cadrs_cam::Step::Feed {
                kind: cadrs_cam::toolpath::FeedKind::Plunge,
                ..
            } if !axes
                .iter()
                .any(|b| (0..3).all(|k| (axis[k] - b[k]).abs() < 1e-6)) =>
            {
                axes.push(axis)
            }
            _ => {}
        }
    }
    assert_eq!(axes.len(), 4, "{axes:?}");
    assert!(axes.iter().all(|a| (lean(*a) - 45.0).abs() < 1e-6));
    // In the tilted plane the one hole along it is drilled, at the plane's origin, 6 deep
    // and no deeper than its drill's point.
    let drill = toolpath(&doc, cam5::STUDIO, cam5::DRILL);
    assert!(drill.warnings.is_empty(), "{:?}", drill.warnings);
    let cycles: Vec<_> = drill
        .steps
        .iter()
        .filter_map(|s| match s {
            cadrs_cam::Step::Drill(c) => Some(*c),
            _ => None,
        })
        .collect();
    assert_eq!(cycles.len(), 1);
    assert!(cycles[0].at[0].hypot(cycles[0].at[1]) < 1e-6, "{cycles:?}");
    let depth = work::DOME_HOLE.1;
    assert!(
        cycles[0].bottom < -depth + 0.5 && cycles[0].bottom > -depth - 2.0,
        "{cycles:?}"
    );
    // The setup posts for the 5-axis mill: the dome and the holes under tool centre point
    // control with the head and the table turning, the last hole in a tilted working plane.
    let rough = toolpath(&doc, cam5::STUDIO, cam5::ROUGH);
    assert!(!rough.is_empty() && !rough.is_multi_axis());
    let paths = HashMap::from([
        (cam5::ROUGH, rough),
        (cam5::PARALLEL, parallel),
        (cam5::DRILL_ALL, all),
        (cam5::DRILL, drill),
    ]);
    // Verify on the solid stock (the tool leans: a height map can't hold it): the dome is
    // cut to its shape without the tool, its holder or a rapid going into the part or the
    // fixture plate.
    {
        let cam = doc.element(cam5::STUDIO).unwrap().cam().unwrap();
        let parts = cadrs_core::rebuild::build(cam.part_features())
            .parts
            .clone();
        let (job, setup) = cam.setup(cam5::SETUP).unwrap();
        let run = program::setup_run(cam, job, setup, &parts, &paths, None).unwrap();
        assert!(program::run_needs_solid(&run));
        let r = program::simulate_solid(cam, job, setup, &run, &parts, &paths, None, 0.3).unwrap();
        assert!(r.removed > 1000.0, "{}", r.removed);
        assert!(
            r.gouge_depth < 0.3,
            "gouged {} at {:?}",
            r.gouge_depth,
            r.gouges.first()
        );
        assert!(r.rapid_hits.is_empty(), "{:?}", r.rapid_hits);
        assert!(r.holder_hits.is_empty(), "{:?}", r.holder_hits);
        assert!(r.fixture_hits.is_empty(), "{:?}", r.fixture_hits);
        let mesh = r.stock.mesh(2);
        assert!(mesh.indices.len() > 3000);
    }
    // On every control, with tool centre point control or without, the finish runs on
    // without lifting off for a rotary axis to come round: the platter turns where the tool
    // stands upright (at the dome's top, across a hole's mouth).
    for (d, nc) in on_every_control(&doc, cam5::STUDIO, cam5::SETUP, &paths) {
        assert!(!nc.contains("Rewind"), "{d:?} rewinds");
        runnable(d, &nc);
        // Read back as each control reads it (LinuxCNC's wrapped C by the sign of its words,
        // the others the shortest way), no cut turns the platter more than a few degrees.
        let moves = cadrs_cam::post::reader::read(&nc);
        for w in moves.windows(2) {
            let turn = w[1].angle('C') - w[0].angle('C');
            // (A rapid may turn further, the tool clear, but never half a turn, where which
            // way it goes would be the control's choice.)
            let most = if w[1].rapid { 179.0 } else { 45.0 };
            assert!(
                turn.abs() < most,
                "{d:?}: C turns {turn} at line {}",
                w[1].line
            );
        }
        assert!(!nc.contains("G92") && !nc.contains("G10 L20"), "{d:?}");
    }
    let nc = posted(&doc, cam5::STUDIO, cam5::SETUP, &paths);
    assert!(nc.contains("O1001"), "{nc}");
    assert!(nc.contains("G43.4") && nc.contains("G49"), "{nc}");
    let moves = cadrs_cam::post::reader::read(&nc);
    assert!(moves.iter().any(|m| m.angle('B').abs() > 40.0), "{nc}");
    assert!(moves.iter().any(|m| m.angle('C').abs() > 80.0), "{nc}");
    assert_eq!(nc.matches("G68.2").count(), 1, "{nc}");
}

/// A job's machine is worked out again when the machine's parts change, not only its mates:
/// the key it is kept by takes in the Part Studio its instances come from.
#[test]
fn the_machine_key_follows_the_machines_parts() {
    use cadrs_core::cam::machine_key;
    use cadrs_core::document::ElementKind;
    let doc = cnc::document().unwrap();
    let before = machine_key(&doc, cnc::MILL_5);
    // The same document, a feature of "Machine parts" renamed.
    let mut changed = doc.clone();
    let el = changed.element_mut(cnc::MACHINE_PARTS).unwrap();
    let ElementKind::PartStudio { features, .. } = &mut el.kind else {
        panic!("Machine parts is a Part Studio");
    };
    features[0].name.push_str(" (moved)");
    assert_ne!(machine_key(&changed, cnc::MILL_5), before);
    // Another studio's change doesn't.
    let mut other = doc.clone();
    let el = other.element_mut(cnc::WORM).unwrap();
    let ElementKind::PartStudio { features, .. } = &mut el.kind else {
        panic!("Worm is a Part Studio");
    };
    features[0].name.push_str(" (moved)");
    assert_eq!(machine_key(&other, cnc::MILL_5), before);
}

/// The worm's run cut exactly as Simulate plays it (`cam::exact`): the bar a solid of the
/// kernel's, each move the screw the machine makes, runs of moves on one screw cut as one (a
/// whole pass of 27 turns one screw about the machine's A axis). The first pass, played a
/// tenth at a time: every cut exact, the stock less and less.
#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn the_worm_is_cut_exactly_as_it_plays() {
    use std::time::Instant;
    let doc = cnc::document().unwrap();
    let rough = toolpath(&doc, cam4::STUDIO, cam4::ROUGH);
    let thread = toolpath(&doc, cam4::STUDIO, cam4::THREAD);
    let paths = HashMap::from([(cam4::ROUGH, rough), (cam4::THREAD, thread)]);
    let cam = doc.element(cam4::STUDIO).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let (job, setup) = cam.setup(cam4::WORM_SETUP).unwrap();
    let run = program::setup_run(cam, job, setup, &parts, &paths, None).unwrap();
    let machine = job.machine.as_ref().unwrap();
    let shape = program::stock_shape(cam, job, &parts).unwrap();
    let stock = program::stock_in(&shape, &run.frame);
    let above = [0.0, 0.0, stock.bounds.max[2] + 30.0];
    let motions = program::run_motions(setup, job, &run, above);
    let track = program::axis_track(machine, &run, &motions);
    let moves: usize = motions.iter().map(|m| m.len()).sum();
    let at = Instant::now();
    let mut exact =
        cadrs_core::cam::exact::ExactStock::new(machine, &run, &shape, &motions, &track.rows)
            .unwrap();
    let full = exact.volume();
    {
        let sizes = exact.cut_sizes();
        let mut hist = std::collections::BTreeMap::new();
        for n in &sizes {
            *hist.entry(n.next_power_of_two()).or_insert(0usize) += 1;
        }
        println!("cut sizes (moves, up to a power of two: count): {hist:?}");
    }
    println!(
        "worm: {moves} moves in {} cuts, set up in {:.2} s, stock {full:.1} mm³",
        exact.cut_count(),
        at.elapsed().as_secs_f64()
    );
    // The first pass (the bar opened along the thread at the first level, 27 turns), cut
    // as it plays: a tenth at a time, each a cut of the one screw as far as the tool has gone,
    // then whole.
    let first_pass = std::env::var("WORM_UPTO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2172)
        .min(moves - 1);
    let mut seen = Vec::new();
    for i in 1..=10 {
        let t = Instant::now();
        exact.cut_to(first_pass * i / 10, 0.5);
        println!(
            "  to move {}: cut {:.3} s, {:.1} mm³",
            first_pass * i / 10,
            t.elapsed().as_secs_f64(),
            exact.volume()
        );
        seen.push(exact.volume());
    }
    let (mesh, _) = exact.mesh(0.01).unwrap();
    println!("{} triangles", mesh.indices.len());
    assert!(exact.failed.is_empty(), "{:?}", exact.failed);
    assert!(full - seen[seen.len() - 1] > 400.0, "{seen:?}");
    assert!(seen.windows(2).all(|w| w[1] <= w[0] + 1e-6), "{seen:?}");
}

/// Scratch: a setup's whole run cut exactly as Simulate would (`CAM=3|4|5`), reporting time and
/// failures.
#[test]
#[ignore = "diagnostic"]
fn tmp_exact_run() {
    use std::time::Instant;
    let which = std::env::var("CAM").unwrap_or("3".into());
    let (studio, setup_id, ops): (ElementId, CamId, Vec<CamId>) = match which.as_str() {
        "3" => (cam3::STUDIO, cam3::SETUP, cam3::TOOLPATHS.to_vec()),
        "4" => (cam4::STUDIO, cam4::SETUP, cam4::TOOLPATHS[..3].to_vec()),
        _ => (cam5::STUDIO, cam5::SETUP, cam5::TOOLPATHS.to_vec()),
    };
    let doc = cnc::document().unwrap();
    let paths: HashMap<CamId, cadrs_cam::Toolpath> = ops
        .iter()
        .map(|&op| (op, toolpath(&doc, studio, op)))
        .collect();
    let cam = doc.element(studio).unwrap().cam().unwrap();
    let parts = cadrs_core::rebuild::build(cam.part_features())
        .parts
        .clone();
    let (job, setup) = cam.setup(setup_id).unwrap();
    let run = program::setup_run(cam, job, setup, &parts, &paths, None).unwrap();
    let machine = job.machine.as_ref().unwrap();
    let shape = program::stock_shape(cam, job, &parts).unwrap();
    let stock = program::stock_in(&shape, &run.frame);
    let above = [0.0, 0.0, stock.bounds.max[2] + 30.0];
    let motions = program::run_motions(setup, job, &run, above);
    let track = program::axis_track(machine, &run, &motions);
    let moves: usize = motions.iter().map(|m| m.len()).sum();
    if let Ok(n) = std::env::var("PRINT_MOVES") {
        let n: usize = n.parse().unwrap_or(60);
        println!("stock {:?}", stock.bounds);
        for m in motions.iter().flatten().take(n) {
            println!("  {m:?}");
        }
        return;
    }
    let mut exact =
        cadrs_core::cam::exact::ExactStock::new(machine, &run, &shape, &motions, &track.rows)
            .unwrap();
    let full = exact.volume();
    println!(
        "cam {which}: {moves} moves in {} cuts, stock {full:.1}",
        exact.cut_count()
    );
    let steps: usize = std::env::var("STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let at = Instant::now();
    for i in 1..=steps {
        let n = (i * moves / steps).min(moves - 1);
        let t = Instant::now();
        exact.cut_to(n, if i == steps { 1.0 } else { 0.5 });
        println!(
            "  to move {n}: {:.2} s, {:.1} mm³, {} failed so far",
            t.elapsed().as_secs_f64(),
            exact.volume(),
            exact.failed.len()
        );
        if at.elapsed().as_secs_f64()
            > std::env::var("LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600.0)
        {
            println!("  (stopped: ten minutes)");
            break;
        }
    }
    let r = program::simulate_solid(cam, job, setup, &run, &parts, &paths, None, 0.3).unwrap();
    println!(
        "removed exactly {:.1}, on the lattice {:.1}; failures {:?}",
        full - exact.volume(),
        r.removed,
        exact
            .failed
            .iter()
            .map(|(c, e)| format!("{c}: {}", e.chars().take(100).collect::<String>()))
            .take(5)
            .collect::<Vec<_>>()
    );
}
