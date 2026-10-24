//! Tests of the post processors.

use super::*;
use crate::machine::catalog;
use crate::tool::ToolKind;
use crate::toolpath::FeedKind;

fn machine(kind: MachineKind, d: Dialect) -> Machine {
    catalog()
        .into_iter()
        .find(|m| m.kind == kind && m.dialect == d)
        .unwrap()
}

fn square_op(tool: Tool, beam: bool) -> NcOperation {
    let mut steps = vec![Step::Rapid([0.0, 0.0, 5.0])];
    if beam {
        steps.push(Step::BeamOn { power: 0.5 });
    } else {
        steps.push(Step::Spindle {
            rpm: 10_000.0,
            cw: true,
        });
        steps.push(Step::Feed {
            to: [0.0, 0.0, -1.0],
            feed: 200.0,
            kind: FeedKind::Plunge,
        });
    }
    for p in [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]] {
        steps.push(Step::Feed {
            to: [p[0], p[1], if beam { 0.0 } else { -1.0 }],
            feed: 800.0,
            kind: FeedKind::Cut,
        });
    }
    if beam {
        steps.push(Step::BeamOff);
    }
    steps.push(Step::Rapid([0.0, 0.0, 5.0]));
    NcOperation {
        name: "Profile 1".into(),
        tool,
        tool_number: 1,
        toolpath: Toolpath {
            steps,
            warnings: vec![],
        },
        coolant: Coolant::Flood,
        datum: 1,
        stop_before: false,
        force_tool_change: false,
        tilt: None,
    }
}

#[test]
fn fanuc_program_shape() {
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    let p = NcProgram {
        name: "Bracket".into(),
        number: 1001,
        offsets: Vec::new(),
        operations: vec![square_op(Tool::new(ToolKind::EndMill, 6.0), false)],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    assert!(nc.starts_with("%\nO1001 (BRACKET)"));
    assert!(nc.contains("T1 M6"));
    // The tool's length on with the first move, after the work offset.
    let g54 = nc.find("G54").unwrap();
    let g43 = nc.find("G0 G43 H1 Z5.").expect(&nc);
    assert!(g54 < g43, "{nc}");
    assert!(nc.contains("S10000 M3"));
    assert!(nc.contains("M8"));
    assert!(nc.contains("G54"));
    assert!(nc.contains("X10"));
    assert!(nc.trim_end().ends_with("M30\n%") || nc.trim_end().ends_with('%'));
    assert!(nc.contains("N10 "));
}

#[test]
fn grbl_laser_uses_dynamic_power() {
    let m = machine(MachineKind::Laser, Dialect::Grbl);
    let p = NcProgram {
        name: "Sign".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![square_op(Tool::new(ToolKind::Laser, 0.1), true)],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    assert!(nc.contains("M4 S500"), "{nc}");
    assert!(!nc.contains("M6"));
    assert!(nc.trim_end().ends_with("M2"));
    assert!(!nc.contains('%'));
}

#[test]
fn plasma_linuxcnc_torch_codes() {
    let m = machine(MachineKind::Plasma, Dialect::LinuxCnc);
    let p = NcProgram {
        name: "Bracket".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![square_op(Tool::new(ToolKind::Plasma, 1.5), true)],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    assert!(nc.contains("M3 $0 S1"));
    assert!(nc.contains("M5 $0"));
}

#[test]
fn wrong_tool_is_refused() {
    let m = machine(MachineKind::Laser, Dialect::Grbl);
    let p = NcProgram {
        name: "x".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![square_op(Tool::new(ToolKind::EndMill, 6.0), false)],
    };
    assert!(matches!(
        post(&p, &m, &m.post),
        Err(PostError::WrongTool(_))
    ));
}

#[test]
fn inch_output_scales() {
    let m = machine(MachineKind::Mill, Dialect::LinuxCnc);
    let mut s = m.post.clone();
    s.output_unit = Unit::Inch;
    let p = NcProgram {
        name: "x".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![square_op(Tool::new(ToolKind::EndMill, 6.0), false)],
    };
    let nc = post(&p, &m, &s).unwrap();
    assert!(nc.contains("G20"));
    // 10 mm = 0.3937 in.
    assert!(nc.contains("X0.3937"), "{nc}");
}

#[test]
fn canned_cycles_and_expansion() {
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    let cycle = DrillCycle {
        at: [10.0, 20.0],
        r_plane: 2.0,
        bottom: -12.0,
        retract: 10.0,
        feed: 120.0,
        kind: CycleKind::Peck,
        peck: 3.0,
        dwell: 0.0,
        pitch: 0.0,
    };
    let op = NcOperation {
        name: "Drill".into(),
        tool: Tool::new(ToolKind::Drill, 5.0),
        tool_number: 3,
        toolpath: Toolpath {
            steps: vec![
                Step::Spindle {
                    rpm: 3000.0,
                    cw: true,
                },
                Step::Rapid([10.0, 20.0, 10.0]),
                Step::Drill(cycle),
            ],
            warnings: vec![],
        },
        coolant: Coolant::Off,
        datum: 1,
        stop_before: false,
        force_tool_change: false,
        tilt: None,
    };
    let p = NcProgram {
        name: "Holes".into(),
        number: 2,
        offsets: Vec::new(),
        operations: vec![op],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    assert!(nc.contains("G98 G83 X10. Y20. Z-12. R2. Q3. F120"), "{nc}");
    assert_points(&nc);
    let mut s = m.post.clone();
    s.canned_peck = false;
    let nc = post(&p, &m, &s).unwrap();
    assert!(!nc.contains("G83"));
    assert!(nc.contains("Z-12."));
    assert_points(&nc);
    // Without decimal points (LinuxCNC-style), integers stay bare.
    s.decimal_point = false;
    let nc = post(&p, &m, &s).unwrap();
    assert!(nc.contains("Z-12"), "{nc}");
    assert!(!nc.contains("Z-12."), "{nc}");
}

/// Every dimension word (X Y Z I J K R Q) in `nc` has a decimal point.
fn assert_points(nc: &str) {
    for line in nc.lines() {
        if line.starts_with('(') || line.starts_with('%') {
            continue;
        }
        for word in line.split_whitespace() {
            let mut c = word.chars();
            let a = c.next().unwrap();
            let rest: String = c.collect();
            if "XYZIJKRQ".contains(a) && rest.parse::<f64>().is_ok() {
                assert!(rest.contains('.'), "{word} in {line}");
            }
        }
    }
}

#[test]
fn drill_dwells_suit_the_dialect() {
    let cycle = DrillCycle {
        at: [10.0, 20.0],
        r_plane: 2.0,
        bottom: -5.0,
        retract: 10.0,
        feed: 120.0,
        kind: CycleKind::Drill,
        peck: 0.0,
        dwell: 0.5,
        pitch: 0.0,
    };
    for (d, want) in [
        (Dialect::Fanuc, " P500"),
        (Dialect::Haas, " P500"),
        (Dialect::LinuxCnc, " P0.5"),
        (Dialect::Mach3, " P0.5"),
    ] {
        let mut m = machine(MachineKind::Mill, Dialect::Fanuc);
        m.dialect = d;
        let s = PostSettings::for_dialect(d, MachineKind::Mill);
        let op = NcOperation {
            name: "Spot".into(),
            tool: Tool::new(ToolKind::Drill, 5.0),
            tool_number: 1,
            toolpath: Toolpath {
                steps: vec![Step::Rapid([10.0, 20.0, 10.0]), Step::Drill(cycle)],
                warnings: vec![],
            },
            coolant: Coolant::Off,
            datum: 1,
            stop_before: false,
            force_tool_change: false,
            tilt: None,
        };
        let p = NcProgram {
            name: "Holes".into(),
            number: 1,
            offsets: Vec::new(),
            operations: vec![op],
        };
        let nc = post(&p, &m, &s).unwrap();
        let line = nc.lines().find(|l| l.contains("G82")).expect(&nc);
        assert!(line.contains(want), "{d:?}: {line}");
    }
}

fn tap_program(d: Dialect) -> String {
    tap_program_fed(d, 625.0)
}

/// A tap at 500 rpm, 1.25 pitch, its cycle written with `feed`.
fn tap_program_fed(d: Dialect, feed: f64) -> String {
    let cycle = DrillCycle {
        at: [10.0, 20.0],
        r_plane: 2.0,
        bottom: -12.0,
        retract: 10.0,
        feed,
        kind: CycleKind::Tap,
        peck: 0.0,
        dwell: 0.0,
        pitch: 1.25,
    };
    let mut m = machine(MachineKind::Mill, Dialect::Fanuc);
    m.dialect = d;
    let s = PostSettings::for_dialect(d, MachineKind::Mill);
    let op = NcOperation {
        name: "Tap".into(),
        tool: Tool::new(ToolKind::Tap, 8.0),
        tool_number: 4,
        toolpath: Toolpath {
            steps: vec![
                Step::Spindle {
                    rpm: 500.0,
                    cw: true,
                },
                Step::Rapid([10.0, 20.0, 10.0]),
                Step::Drill(cycle),
            ],
            warnings: vec![],
        },
        coolant: Coolant::Off,
        datum: 1,
        stop_before: true,
        force_tool_change: false,
        tilt: None,
    };
    post(
        &NcProgram {
            name: "Taps".into(),
            number: 1,
            offsets: Vec::new(),
            operations: vec![op],
        },
        &m,
        &s,
    )
    .unwrap()
}

/// Without a tapping cycle (GRBL has none) the tap backs out with the spindle reversed.
#[test]
fn expanded_taps_reverse_the_spindle() {
    let nc = tap_program(Dialect::Grbl);
    let lines: Vec<&str> = nc.lines().collect();
    let bottom = lines.iter().position(|l| l.contains("Z-12")).expect(&nc);
    let rev = lines.iter().position(|l| *l == "S500 M4").expect(&nc);
    let out = lines.iter().rposition(|l| l.contains("Z2")).expect(&nc);
    assert!(bottom < rev && rev < out, "{nc}");
    // Turning forward again after.
    assert!(lines[out..].contains(&"S500 M3"), "{nc}");
}

#[test]
fn tapping_suits_the_dialect() {
    let lines = |nc: &str| -> Vec<String> {
        nc.lines()
            .map(|l| {
                l.split_once(' ')
                    .filter(|(n, _)| n.starts_with('N'))
                    .map_or(l, |(_, r)| r)
                    .to_string()
            })
            .collect()
    };
    let fanuc = lines(&tap_program(Dialect::Fanuc));
    let m29 = fanuc.iter().position(|l| l == "M29 S500").expect("M29");
    assert!(fanuc[m29 + 1].starts_with("G98 G84 "), "{fanuc:?}");
    let lcnc = tap_program(Dialect::LinuxCnc);
    assert!(lcnc.contains("G33.1 Z-12 K1.25"), "{lcnc}");
    assert!(!lcnc.contains("G84"), "{lcnc}");
    // M0 is kept even with a tool change, before it.
    let m0 = fanuc.iter().position(|l| l == "M0").expect("M0");
    let t4 = fanuc.iter().position(|l| l == "T4 M6").unwrap();
    assert!(m0 < t4);
}

#[test]
fn dwells_and_comments_suit_the_dialect() {
    assert_eq!(Dialect::Fanuc.dwell(1.0), "G4 X1.");
    assert_eq!(Dialect::Haas.dwell(0.5), "G4 P0.5");
    assert_eq!(Dialect::LinuxCnc.dwell(0.5), "G4 P0.5");
    assert_eq!(Dialect::Grbl.dwell(2.0), "G4 P2");
    assert_eq!(
        Dialect::Fanuc.comment("D6 end mill \u{2014} Ø6 (x2) 45°"),
        "(D6 end mill - D6 x2 45d)"
    );
    assert!(Dialect::Fanuc.comment("caf\u{e9}").is_ascii());
}

#[test]
fn arcs_are_fitted() {
    let mut steps = vec![Step::Rapid([10.0, 0.0, -1.0])];
    for i in 1..=16 {
        let a = std::f64::consts::PI * i as f64 / 16.0;
        steps.push(Step::Feed {
            to: [10.0 * a.cos(), 10.0 * a.sin(), -1.0],
            feed: 500.0,
            kind: FeedKind::Cut,
        });
    }
    let out = fit_arcs(&steps, 0.05);
    assert_eq!(out.len(), 2, "{out:?}");
    match &out[1] {
        Step::Arc { center, cw, .. } => {
            assert!(center[0].abs() < 1e-6 && center[1].abs() < 1e-6);
            assert!(!cw);
        }
        s => panic!("{s:?}"),
    }
}

#[test]
fn datum_codes() {
    assert_eq!(datum_code(1), "G54");
    assert_eq!(datum_code(6), "G59");
    assert_eq!(datum_code(7), "G54.1 P1");
    assert_eq!(Dialect::Haas.datum(8).as_deref(), Some("G154 P2"));
    assert_eq!(Dialect::LinuxCnc.datum(9).as_deref(), Some("G59.3"));
    assert_eq!(Dialect::LinuxCnc.datum(10), None);
    assert_eq!(Dialect::Mach3.datum(7).as_deref(), Some("G59 P7"));
    assert_eq!(Dialect::Grbl.datum(7), None);
    assert_eq!(Dialect::Grbl.datum(3).as_deref(), Some("G56"));
    assert_eq!(
        file_name("My part (v2)", Dialect::LinuxCnc),
        "My_part__v2.ngc"
    );
}

#[test]
fn qtplasmac_leaves_heights_to_the_material() {
    let m = machine(MachineKind::Plasma, Dialect::LinuxCnc);
    let mut tool = Tool::new(ToolKind::Plasma, 1.5);
    tool.beam.material = 3;
    tool.beam.thc = false;
    tool.beam.pierce_delay = 0.5;
    let mut op = square_op(tool, true);
    // A pierce at its height with its dwell, as the beam strategies make it.
    op.toolpath.steps.insert(1, Step::Rapid([0.0, 0.0, 3.8]));
    op.toolpath.steps.insert(3, Step::Dwell(0.5));
    let p = NcProgram {
        name: "Bracket".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![op],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    let body: Vec<&str> = nc.lines().filter(|l| !l.starts_with('(')).collect();
    assert!(body.iter().all(|l| !l.contains('Z')), "{nc}");
    assert!(body.iter().all(|l| !l.starts_with("G4")), "{nc}");
    let at = |code: &str| {
        body.iter()
            .position(|l| *l == code)
            .unwrap_or_else(|| panic!("{code} in {nc}"))
    };
    assert!(at("M190 P3") < at("M66 P3 L3 Q1"));
    assert!(at("M66 P3 L3 Q1") < at("M3 $0 S1"));
    assert!(at("M62 P2") < at("M3 $0 S1"));
    assert!(at("M63 P2") > at("M3 $0 S1"));
    assert!(
        nc.contains("(Torch: pierce 0.5 s at 3.8 mm, cut at 1.5 mm)"),
        "{nc}"
    );
    // QtPlasmaC's preamble; the torch off once for each time it's on.
    assert!(
        body.contains(&"G21 G40 G49 G64 P0.1 G80 G90 G92.1 G94 G97"),
        "{nc}"
    );
    assert_eq!(
        body.iter().filter(|l| **l == "M5 $0").count(),
        body.iter().filter(|l| **l == "M3 $0 S1").count(),
        "{nc}"
    );
}

/// Where a program takes the tool: every G0/G1/G2/G3 move read back (absolute XYZ, I/J from
/// the start), as points along it every so often. Panics on an arc whose ends aren't the
/// same distance from its centre.
fn read_back(nc: &str) -> Vec<P3> {
    let mut pos: [Option<f64>; 3] = [None; 3];
    let mut mode = 0u8;
    let mut out = Vec::new();
    for line in nc.lines() {
        let line = line.split('(').next().unwrap().trim();
        let words: Vec<&str> = line
            .split_whitespace()
            .filter(|w| !w.starts_with('N'))
            .collect();
        if words.is_empty() || words[0] == "%" {
            continue;
        }
        // Machine-coordinate and home moves leave the work coordinates.
        if words.iter().any(|w| matches!(*w, "G28" | "G53" | "G91")) {
            pos[2] = None;
            continue;
        }
        let mut to = pos;
        let (mut i, mut j) = (None, None);
        let mut moved = false;
        for w in &words {
            let (a, v) = w.split_at(1);
            match a {
                "G" => match v {
                    "0" | "00" => mode = 0,
                    "1" | "01" => mode = 1,
                    "2" | "02" => mode = 2,
                    "3" | "03" => mode = 3,
                    _ => {}
                },
                "X" | "Y" | "Z" => {
                    let k = "XYZ".find(a).unwrap();
                    to[k] = Some(v.parse().unwrap_or_else(|_| panic!("{w} in {line}")));
                    moved = true;
                }
                "I" => i = Some(v.parse::<f64>().unwrap()),
                "J" => j = Some(v.parse::<f64>().unwrap()),
                _ => {}
            }
        }
        if !moved || to.iter().any(|c| c.is_none()) {
            pos = to;
            continue;
        }
        let b = [to[0].unwrap(), to[1].unwrap(), to[2].unwrap()];
        if let (true, [Some(x), Some(y), Some(z)]) = (mode >= 2, pos) {
            let c = [x + i.unwrap(), y + j.unwrap()];
            let (r0, r1) = (
                crate::geom::dist2([x, y], c),
                crate::geom::dist2([b[0], b[1]], c),
            );
            assert!(
                (r0 - r1).abs() < 2e-3,
                "an arc's ends {r0} and {r1} from its centre: {line}"
            );
            let (a0, mut a1) = ((y - c[1]).atan2(x - c[0]), (b[1] - c[1]).atan2(b[0] - c[0]));
            let cw = mode == 2;
            if cw && a1 >= a0 - 1e-12 {
                a1 -= std::f64::consts::TAU;
            } else if !cw && a1 <= a0 + 1e-12 {
                a1 += std::f64::consts::TAU;
            }
            for k in 0..=16 {
                let f = k as f64 / 16.0;
                let a = a0 + (a1 - a0) * f;
                out.push([c[0] + r0 * a.cos(), c[1] + r0 * a.sin(), z + (b[2] - z) * f]);
            }
        } else {
            out.push(b);
        }
        pos = to;
    }
    out
}

fn dist_to_path(q: P3, segs: &[crate::toolpath::Segment]) -> f64 {
    segs.iter()
        .map(|s| {
            let d = crate::geom::sub(s.to, s.from);
            let l2 = crate::geom::dot(d, d);
            let t = if l2 < 1e-18 {
                0.0
            } else {
                (crate::geom::dot(crate::geom::sub(q, s.from), d) / l2).clamp(0.0, 1.0)
            };
            dist(q, crate::geom::lerp(s.from, s.to, t))
        })
        .fold(f64::INFINITY, f64::min)
}

/// Real toolpaths (a multi-pass profile with arc leads and a zigzag rough) posted for every
/// mill dialect and read back: every point the program moves through lies on the toolpath,
/// and every arc is a true arc.
#[test]
fn programs_read_back_onto_their_toolpaths() {
    use crate::strategy::testing::{pocket_block, stock_for};
    use crate::strategy::{CuttingMethod, Input, Params, Pattern, generate};
    let model = pocket_block();
    let stock = stock_for(&model, 5.0, 2.0);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let mut ops = Vec::new();
    for pattern in [Pattern::Profile2D, Pattern::Rough2D] {
        let mut p = Params::new(pattern, &tool, MachineKind::Mill);
        p.step_down = 5.0;
        p.passes = 2;
        p.cutting_method = CuttingMethod::Zigzag;
        let input = Input {
            model: &model,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        ops.push(NcOperation {
            name: format!("{pattern:?}"),
            tool: tool.clone(),
            tool_number: 1,
            toolpath: tp,
            coolant: Coolant::Off,
            datum: 1,
            stop_before: false,
            force_tool_change: false,
            tilt: None,
        });
    }
    for d in Dialect::ALL {
        let mut m = machine(MachineKind::Mill, Dialect::Fanuc);
        m.dialect = d;
        let s = PostSettings::for_dialect(d, MachineKind::Mill);
        let p = NcProgram {
            name: "Block".into(),
            number: 1,
            offsets: Vec::new(),
            operations: ops.clone(),
        };
        let nc = post(&p, &m, &s).unwrap();
        let segs: Vec<_> = ops
            .iter()
            .flat_map(|o| o.toolpath.motion([0.0, 0.0, 100.0], 5000.0))
            .collect();
        let pts = read_back(&nc);
        assert!(pts.len() > 100, "{d:?}");
        for q in pts {
            let e = dist_to_path(q, &segs);
            assert!(e < 0.01, "{d:?}: {q:?} is {e} mm off the toolpath");
        }
    }
}

#[test]
fn tilt_angles_rebuild_the_frame() {
    use crate::Frame;
    let rz = |a: f64| {
        let (s, c) = a.to_radians().sin_cos();
        [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
    };
    let rx = |a: f64| {
        let (s, c) = a.to_radians().sin_cos();
        [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
    };
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        m
    };
    for f in [
        Frame::from_zx([1.0, 2.0, 3.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        Frame::from_zx([0.0; 3], [1.0, 1.0, 1.0], [0.0, 0.0, 1.0]),
        Frame::from_zx([0.0; 3], [0.3, -0.2, -0.9], [1.0, 0.5, 0.0]),
        Frame::from_zx([0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ] {
        let t = Tilt::of(&f);
        let [a, b, c] = t.euler;
        let r = mul(mul(rz(a), rx(b)), rz(c));
        for (j, axis) in [f.x, f.y, f.z].iter().enumerate() {
            for i in 0..3 {
                assert!((r[i][j] - axis[i]).abs() < 1e-9, "{f:?} → {:?}", t.euler);
            }
        }
    }
    // Tipped 90° about X: the front of the part up to the spindle.
    let f = Frame::from_zx([0.0; 3], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let e = Tilt::of(&f).euler;
    assert!(
        e[0].abs() < 1e-9 && (e[1] - 90.0).abs() < 1e-9 && e[2].abs() < 1e-9,
        "{e:?}"
    );
}

#[test]
fn tilted_planes_post_as_g68_2() {
    let five = catalog()
        .into_iter()
        .find(|m| m.indexing.is_some() && m.dialect == Dialect::Fanuc)
        .unwrap();
    let frame = crate::Frame::from_zx([0.0, -20.0, 5.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let mut tilted = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    tilted.tilt = Some(Tilt::of(&frame));
    tilted.name = "Side".into();
    let flat = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    let p = NcProgram {
        name: "Block".into(),
        number: 7,
        offsets: Vec::new(),
        operations: vec![flat.clone(), tilted.clone(), flat.clone()],
    };
    let nc = post(&p, &five, &five.post).unwrap();
    let lines: Vec<&str> = nc
        .lines()
        .map(|l| {
            l.split_once(' ')
                .filter(|(n, _)| n.starts_with('N'))
                .map_or(l, |(_, r)| r)
        })
        .collect();
    let at = |code: &str| {
        lines
            .iter()
            .position(|l| l.starts_with(code))
            .unwrap_or_else(|| panic!("{code} in {nc}"))
    };
    assert!(lines.contains(&"G68.2 X0. Y-20. Z5. I0. J90. K0."), "{nc}");
    let g68 = at("G68.2");
    assert_eq!(lines[g68 + 1], "G53.1");
    // The tool's length again after the plane.
    assert_eq!(lines[g68 + 2], "G0 G43 H1 Z5.");
    // Up clear before tilting and before cancelling; the rotaries home after G69.
    assert!(lines[g68 - 2].starts_with("G91 G28 Z0."), "{nc}");
    let g69 = at("G69");
    assert!(
        g69 > g68 && lines[g69 - 2].starts_with("G91 G28 Z0."),
        "{nc}"
    );
    assert_eq!(lines[g69 + 1], "G0 A0. C0.", "{nc}");
    // The tilted operation's moves come between them, in its own coordinates.
    assert!(lines[g68..g69].iter().any(|l| l.contains("X10.")));
    assert_eq!(lines.iter().filter(|l| l.starts_with("G68.2")).count(), 1);
    // Two tilted operations with different tools: the plane is cancelled for the tool
    // change and set again after it.
    let mut t2 = tilted.clone();
    t2.tool_number = 2;
    let p2 = NcProgram {
        name: "Block".into(),
        number: 7,
        offsets: Vec::new(),
        operations: vec![tilted.clone(), t2],
    };
    let nc2 = post(&p2, &five, &five.post).unwrap();
    let l2: Vec<&str> = nc2
        .lines()
        .map(|l| {
            l.split_once(' ')
                .filter(|(n, _)| n.starts_with('N'))
                .map_or(l, |(_, r)| r)
        })
        .collect();
    let t2_at = l2.iter().position(|l| *l == "T2 M6").unwrap();
    let g69_at = l2.iter().position(|l| *l == "G69").unwrap();
    assert!(g69_at < t2_at, "{nc2}");
    assert_eq!(
        l2.iter().filter(|l| l.starts_with("G68.2")).count(),
        2,
        "{nc2}"
    );
    // Haas-style controls aren't given G68.2.
    let mut haas = five.clone();
    haas.dialect = Dialect::Haas;
    assert!(matches!(
        post(&p, &haas, &haas.post),
        Err(PostError::CantTilt(_))
    ));
    // Not on a 3-axis machine, nor on LinuxCNC.
    let three = machine(MachineKind::Mill, Dialect::Fanuc);
    assert!(matches!(
        post(&p, &three, &three.post),
        Err(PostError::CantTilt(_))
    ));
    let mut lcnc = five.clone();
    lcnc.dialect = Dialect::LinuxCnc;
    assert!(matches!(
        post(&p, &lcnc, &lcnc.post),
        Err(PostError::CantTilt(_))
    ));
}

#[test]
fn grbl_goes_up_before_it_goes_across() {
    let mut m = machine(MachineKind::Mill, Dialect::Fanuc);
    m.dialect = Dialect::Grbl;
    // (Ticked, it's still a manual change: GRBL has no M6.)
    m.tool_changer = true;
    let s = PostSettings::for_dialect(Dialect::Grbl, MachineKind::Mill);
    let mut op = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    op.toolpath.steps[0] = Step::Rapid([30.0, 40.0, 5.0]);
    let p = NcProgram {
        name: "Plate".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![op],
    };
    let nc = post(&p, &m, &s).unwrap();
    // After the M0 tool change, Z alone first: never a diagonal from the touch-off point.
    let stop = nc.find("M0").expect(&nc);
    let after = &nc[stop..];
    let first = after
        .lines()
        .find(|l| l.contains("G0") || l.contains('X'))
        .expect(&nc);
    assert!(first.contains("Z5") && !first.contains('X'), "{nc}");
    assert!(!nc.contains("G43") && !nc.contains("M6"), "{nc}");
}

#[test]
fn sequence_numbers_stay_five_digits() {
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    let mut op = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    let last = op.toolpath.steps.pop().unwrap();
    for i in 0..25_000 {
        let x = (i % 2) as f64 * 10.0;
        op.toolpath.steps.push(Step::Feed {
            to: [x, 0.0, -1.0],
            feed: 800.0,
            kind: FeedKind::Cut,
        });
    }
    op.toolpath.steps.push(last);
    let p = NcProgram {
        name: "Long".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![op],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    for l in nc.lines() {
        if let Some(n) = l.strip_prefix('N') {
            let digits = n.split(' ').next().unwrap();
            assert!(digits.len() <= 5, "{l}");
        }
    }
    assert!(nc.contains("M30"));
}

/// A profile with wear compensation: G41 (climb, the tool left of the path) on the straight run
/// into each arc lead-in, never on an arc; G40 after the lead-out; the cuts themselves where
/// they are without it. GRBL, without compensation, refuses it.
#[test]
fn wear_compensation_starts_on_a_straight_move() {
    use crate::geom::Aabb;
    use crate::model::{Model, box_triangles};
    use crate::strategy::{Compensation, Input, Params, Pattern, Stock, generate};
    let m = Model::from_triangles(
        "block",
        box_triangles(&Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0])),
    );
    // (A plate cut to size: the leads start off it.)
    let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0]));
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let toolpath = |c: Compensation| {
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.step_down = 5.0;
        p.compensation = c;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        generate(&input).unwrap()
    };
    let plain = toolpath(Compensation::Computer);
    let worn = toolpath(Compensation::Wear);
    let cuts = |tp: &Toolpath| -> Vec<P3> {
        tp.steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect()
    };
    // The same cuts (the last depth closing where its start, near the tool, put it).
    let (a, b) = (cuts(&plain), cuts(&worn));
    assert_eq!(a.len(), b.len());
    assert_eq!(a[..a.len() - 1], b[..b.len() - 1]);
    let mut op = square_op(tool.clone(), false);
    op.toolpath = worn;
    let program = NcProgram {
        name: "Plate".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![op],
    };
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    let nc = post(&program, &m, &m.post).unwrap();
    let lines: Vec<&str> = nc.lines().collect();
    let on: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("G41 D1"))
        .collect();
    // Two depths, each compensated once.
    assert_eq!(on.len(), 2, "{nc}");
    for &i in &on {
        assert!(lines[i].contains("G1") && !lines[i].contains("G2") && !lines[i].contains("G3"));
        // The arc lead-in next, then (after the contour and lead-out) G40.
        assert!(
            lines[i + 1].contains("G2") || lines[i + 1].contains("G3"),
            "{nc}"
        );
        let off = (i..lines.len())
            .find(|&k| lines[k].contains("G40"))
            .expect(&nc);
        assert!(on.iter().all(|&j| j <= i || j > off), "{nc}");
        // Cancelled on a straight move across, off the lead-out arc.
        assert!(lines[off].contains("G1 G40 "), "{nc}");
        assert!(
            lines[off - 1].contains("G2") || lines[off - 1].contains("G3"),
            "{nc}"
        );
    }
    assert!(!nc.contains("G42"), "{nc}");
    // The operator told what D1 holds, once, before the first G41.
    let note = nc.find("D1 holds the wear only").expect(&nc);
    assert!(note < nc.find("G41").unwrap());
    assert_eq!(nc.matches("holds the wear only").count(), 1);
    let mut grbl = machine(MachineKind::Mill, Dialect::Fanuc);
    grbl.dialect = Dialect::Grbl;
    let s = PostSettings::for_dialect(Dialect::Grbl, MachineKind::Mill);
    assert!(matches!(
        post(&program, &grbl, &s),
        Err(PostError::NoCompensation(_))
    ));
}

/// A frame with a window, profiled with wear compensation at two depths: a contour is
/// compensated at every depth or at none (never one depth and not the next). It starts on a
/// lead-in where there's room, else on the ramp in (the window, and the first depth in oversize
/// stock); a plunge in leaves no straight start, so its contours go uncompensated, and the
/// toolpath says so.
#[test]
fn wear_compensation_is_all_or_nothing_per_contour() {
    use crate::geom::Aabb;
    use crate::model::{Model, box_triangles};
    use crate::strategy::{Compensation, Entry, Input, Params, Pattern, Stock, generate};
    let mut tris = Vec::new();
    for (lo, hi) in [
        ([0.0, 0.0], [15.0, 40.0]),
        ([45.0, 0.0], [60.0, 40.0]),
        ([15.0, 0.0], [45.0, 10.0]),
        ([15.0, 30.0], [45.0, 40.0]),
    ] {
        tris.extend(box_triangles(&Aabb::new(
            [lo[0], lo[1], 0.0],
            [hi[0], hi[1], 10.0],
        )));
    }
    let m = Model::from_triangles("frame", tris);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let post_with = |stock: Stock, entry: Entry| {
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.step_down = 5.0;
        p.compensation = Compensation::Wear;
        p.links.entry = entry;
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        let tp = generate(&input).unwrap();
        let mut op = square_op(tool.clone(), false);
        op.toolpath = tp.clone();
        let program = NcProgram {
            name: "Frame".into(),
            number: 1,
            offsets: Vec::new(),
            operations: vec![op],
        };
        let mc = machine(MachineKind::Mill, Dialect::Fanuc);
        (tp, post(&program, &mc, &mc.post).unwrap())
    };
    let missed = |tp: &Toolpath| tp.warnings.iter().any(|w| w.contains("cut without"));
    let to_size = Stock::block(Aabb::new([0.0; 3], [60.0, 40.0, 10.0]));
    let oversize = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [65.0, 45.0, 10.0]));
    for stock in [to_size.clone(), oversize] {
        // Both contours at both depths, each started on a straight move.
        let (tp, nc) = post_with(stock, Entry::Ramp);
        assert_eq!(nc.matches("G41").count(), 4, "{nc}");
        assert!(!missed(&tp), "{:?}", tp.warnings);
        for l in nc.lines().filter(|l| l.contains("G41")) {
            assert!(
                l.contains("G1 ") && !l.contains("G2") && !l.contains("G3"),
                "{l}"
            );
        }
        assert_eq!(
            nc.matches("G41").count(),
            nc.matches("G40").count() - 1,
            "{nc}"
        );
        // Cancelled on a straight move across, never a Z-only one (the window has no room to
        // run out: back along the cut).
        for l in nc.lines().skip(10).filter(|l| l.contains("G40")) {
            assert!(
                l.contains("G1") && (l.contains('X') || l.contains('Y')),
                "{l}"
            );
        }
    }
    // Plunged in, the window has no straight start at any depth: never compensated.
    let (tp, nc) = post_with(to_size, Entry::Plunge);
    assert_eq!(nc.matches("G41").count(), 2, "{nc}");
    assert!(
        tp.warnings
            .iter()
            .any(|w| w.contains("1 contour is cut without")),
        "{:?}",
        tp.warnings
    );
}

/// Onto another work offset with the same tool: up to the first height on its own before
/// moving across (the offset's fixture may stand taller).
#[test]
fn a_new_work_offset_goes_up_before_across() {
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    let first = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    let mut second = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
    second.datum = 2;
    second.toolpath.steps[0] = Step::Rapid([30.0, 40.0, 25.0]);
    let p = NcProgram {
        name: "Two vises".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![first, second],
    };
    let nc = post(&p, &m, &m.post).unwrap();
    let after = &nc[nc.find("G55").expect(&nc)..];
    let first_move = after
        .lines()
        .skip(1)
        .find(|l| l.contains('X') || l.contains('Y') || l.contains('Z'))
        .expect(&nc);
    assert!(
        first_move.contains("Z25") && !first_move.contains('X') && !first_move.contains('Y'),
        "{nc}"
    );
}

/// A tap's feed is always its pitch a turn of the speed the program set, even if the cycle was
/// worked out for another speed (the machine's spindle limit lowered since).
#[test]
fn a_tap_feeds_its_pitch_at_the_speed_set() {
    let nc = tap_program_fed(Dialect::Fanuc, 900.0);
    let line = nc.lines().find(|l| l.contains("G84")).expect(&nc);
    assert!(line.ends_with("F625"), "{line}");
}

/// A stop before the second operation (same tool): the spindle and coolant go off before the
/// M0, and both come back on (M3, M8) before it cuts again.
#[test]
fn a_stop_turns_the_spindle_and_coolant_off_and_back_on() {
    for d in [Dialect::Fanuc, Dialect::Haas, Dialect::LinuxCnc] {
        let mut m = machine(MachineKind::Mill, Dialect::Fanuc);
        m.dialect = d;
        let s = PostSettings::for_dialect(d, MachineKind::Mill);
        let first = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
        let mut second = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
        second.stop_before = true;
        let p = NcProgram {
            name: "Stop".into(),
            number: 1,
            offsets: Vec::new(),
            operations: vec![first, second],
        };
        let nc = post(&p, &m, &s).unwrap();
        let lines: Vec<&str> = nc.lines().collect();
        let stop = lines
            .iter()
            .position(|l| l.split_whitespace().any(|w| w == "M0" || w == "M00"))
            .expect(&nc);
        let before = &lines[..stop];
        let m5 = before.iter().rposition(|l| l.contains("M5")).expect(&nc);
        let m9 = before.iter().rposition(|l| l.contains("M9")).expect(&nc);
        let cut_before = before.iter().rposition(|l| l.contains("G1")).unwrap();
        assert!(m5 > cut_before && m9 > cut_before, "{d:?}:\n{nc}");
        let after = &lines[stop..];
        let m3 = after.iter().position(|l| l.contains("M3")).expect(&nc);
        let m8 = after.iter().position(|l| l.contains("M8")).expect(&nc);
        let cut_after = after.iter().position(|l| l.contains("G1")).unwrap();
        assert!(m3 < cut_after && m8 < cut_after, "{d:?}:\n{nc}");
    }
}

/// From one operation to the next with the same tool and offset: never a diagonal rapid. Up
/// to the higher height first, across, then down.
#[test]
fn between_operations_up_across_down() {
    let m = machine(MachineKind::Mill, Dialect::Fanuc);
    for (z, want) in [(25.0, ["Z25.", "X30. Y40."]), (2.0, ["X30. Y40.", "Z2."])] {
        let first = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
        let mut second = square_op(Tool::new(ToolKind::EndMill, 6.0), false);
        second.name = "Second".into();
        second.toolpath.steps[0] = Step::Rapid([30.0, 40.0, z]);
        let p = NcProgram {
            name: "Two".into(),
            number: 1,
            offsets: Vec::new(),
            operations: vec![first, second],
        };
        let nc = post(&p, &m, &m.post).unwrap();
        let after = &nc[nc.find("(Second)").expect(&nc)..];
        let moves: Vec<&str> = after
            .lines()
            .filter(|l| l.contains('X') || l.contains('Y') || l.contains('Z'))
            .take(2)
            .collect();
        for (l, w) in moves.iter().zip(want) {
            let words: Vec<&str> = l
                .split_whitespace()
                .filter(|t| !t.starts_with('N'))
                .collect();
            let axes: Vec<&str> = words
                .iter()
                .copied()
                .filter(|t| t.starts_with(['X', 'Y', 'Z']))
                .collect();
            assert_eq!(axes.join(" "), w, "{nc}");
        }
    }
}
