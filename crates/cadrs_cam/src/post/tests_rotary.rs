//! Tests of posting through the machine's axes: every program is read back and run through
//! the machine's own forward kinematics, and must put the tool where it was asked.

use super::reader::{ReadMove, read};
use super::rotary::program_zero;
use super::*;
use crate::geom::{Frame, add, scale, sub};
use crate::kinematics::samples;
use crate::kinematics::{Chain, Cl, angle_between, turned};
use crate::machine::catalog;
use crate::tool::ToolKind;
use crate::toolpath::FeedKind;

const X: P3 = [1.0, 0.0, 0.0];
const Z: P3 = [0.0, 0.0, 1.0];

/// A mill on `d` with these axes.
fn machine_of(chain: Chain, d: Dialect, tcp: bool) -> Machine {
    let mut m = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill)
        .unwrap();
    m.dialect = d;
    m.post = PostSettings::for_dialect(d, MachineKind::Mill);
    m.post.rotary.tcp = tcp;
    m.chain = Some(chain);
    m
}

fn tool() -> Tool {
    Tool::new(ToolKind::BallMill, 8.0)
}

/// An operation that feeds through `path` (cutter locations in the work's coordinates) at
/// `feed`, after a rapid to its first.
fn leaning_op(path: &[Cl], feed: f64) -> NcOperation {
    let mut steps = vec![
        Step::Axis(path[0].axis),
        Step::Rapid(path[0].tip),
        Step::Spindle {
            rpm: 8000.0,
            cw: true,
        },
    ];
    for cl in &path[1..] {
        steps.push(Step::Axis(cl.axis));
        steps.push(Step::Feed {
            to: cl.tip,
            feed,
            kind: FeedKind::Cut,
        });
    }
    NcOperation {
        name: "Lean".into(),
        tool: tool(),
        tool_number: 3,
        toolpath: Toolpath {
            steps,
            warnings: vec![],
        },
        coolant: Coolant::Off,
        datum: 1,
        stop_before: false,
        force_tool_change: false,
        tilt: None,
    }
}

fn program(ops: Vec<NcOperation>) -> NcProgram {
    NcProgram {
        name: "Part".into(),
        number: 1,
        offsets: Vec::new(),
        operations: ops,
    }
}

/// The axis values a block read back puts the machine at.
fn joints(chain: &Chain, work: &Frame, length: f64, m: &ReadMove) -> Vec<f64> {
    let (lin, rot) = rotary::words_of(chain).unwrap();
    let mut q = chain.rest();
    for (i, letter) in &rot {
        q[*i] = m.angle(*letter);
    }
    if m.tcp {
        // The words are the tip in the work's coordinates: the control finds the slides.
        chain.slide_to(work.to_world(m.xyz), length, &q).unwrap()
    } else {
        let zero = program_zero(chain, work, length).unwrap();
        for (k, i) in lin.iter().enumerate() {
            q[*i] = zero[*i] + m.xyz[k];
        }
        q
    }
}

/// Where the tool is (the work's coordinates) along the feed moves of a program read back:
/// eight places a block, the axes run as the control runs them (every axis straight; under
/// tool centre point control the tip straight and the rotary axes straight). The first is
/// where the feeds start.
fn replay(chain: &Chain, work: &Frame, length: f64, moves: &[ReadMove]) -> Vec<Cl> {
    let mut out = Vec::new();
    let mut prev: Option<&ReadMove> = None;
    for m in moves {
        if let Some(p) = prev
            && !m.rapid
        {
            let (a, b) = (
                joints(chain, work, length, p),
                joints(chain, work, length, m),
            );
            if out.is_empty() {
                out.push(chain.locate(&a, length).seen_from(work));
            }
            for k in 1..=8 {
                let f = k as f64 / 8.0;
                let mut q: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x + (y - x) * f).collect();
                if m.tcp {
                    let tip = add(p.xyz, scale(sub(m.xyz, p.xyz), f));
                    q = chain.slide_to(work.to_world(tip), length, &q).unwrap();
                }
                out.push(chain.locate(&q, length).seen_from(work));
            }
        }
        prev = Some(m);
    }
    out
}

/// How far `at` is from the path of cutter locations: the tip's distance from the nearest
/// point of its polyline, and the angle to the axis there.
fn off_path(path: &[Cl], at: &Cl) -> (f64, f64) {
    let mut best = (f64::INFINITY, 0.0);
    for w in path.windows(2) {
        let d = sub(w[1].tip, w[0].tip);
        let l2 = crate::geom::dot(d, d);
        let t = if l2 > 0.0 {
            (crate::geom::dot(sub(at.tip, w[0].tip), d) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let on = w[0].toward(&w[1], t);
        let off = dist(on.tip, at.tip);
        if off < best.0 {
            best = (off, angle_between(on.axis, at.axis));
        }
    }
    best
}

/// A sweep across the work with the tool leaning further and further round: cutter locations
/// in the table mount's coordinates. With `about`, the lean is a turn about that line alone
/// (all a machine with one rotary axis can do).
fn sweep(about: Option<P3>, tilt: f64) -> Vec<Cl> {
    (0..=40)
        .map(|k| {
            let t = k as f64 / 40.0;
            let tip = [
                -30.0 + 60.0 * t,
                -20.0 + 40.0 * t,
                5.0 + 10.0 * (t * 6.0).sin(),
            ];
            let axis = match about {
                Some(u) => turned(Z, u, -40.0 + 150.0 * t),
                None => {
                    let (lean, lon) = ((5.0 + tilt * t).to_radians(), (200.0 * t).to_radians());
                    [lean.sin() * lon.cos(), lean.sin() * lon.sin(), lean.cos()]
                }
            };
            Cl::new(tip, axis)
        })
        .collect()
}

#[test]
fn every_machine_on_every_control_reads_back_onto_its_cutter_locations() {
    // The work sits off the table mount's centre and turned on it.
    let work = Frame::from_zx([15.0, -8.0, 40.0], Z, [1.0, 0.3, 0.0]);
    let length = tool().gauge_length();
    let mut posted = 0;
    for (name, chain) in samples::mills() {
        let one = chain.rotaries().len() == 1;
        let on_table: Vec<Cl> = sweep(
            one.then_some(X),
            if name.contains("nutating") {
                60.0
            } else {
                75.0
            },
        );
        let path: Vec<Cl> = on_table.iter().map(|c| c.seen_from(&work)).collect();
        for d in Dialect::ALL {
            for tcp in [false, true] {
                let m = machine_of(chain.clone(), d, tcp);
                if tcp
                    && m.post
                        .rotary
                        .tcp_for(d, 1, chain.rotaries().len())
                        .is_none()
                {
                    continue;
                }
                let feed = 600.0;
                let nc = post_on(&program(vec![leaning_op(&path, feed)]), &m, &m.post, &work)
                    .unwrap_or_else(|e| panic!("{name} on {d:?}: {e}"));
                let moves = read(&nc);
                assert!(
                    moves.iter().any(|m| m.tcp) == tcp,
                    "{name} {d:?} {tcp}: {nc}"
                );
                let run = replay(&chain, &work, length, &moves);
                assert!(run.len() > 40, "{name} {d:?}: {nc}");
                // The run starts on the path's first location and ends on its last.
                let (first, last) = (run.first().unwrap(), run.last().unwrap());
                assert!(
                    dist(first.tip, path[0].tip) < 2e-3,
                    "{name} {d:?} {tcp}: {first:?}\n{nc}"
                );
                assert!(
                    dist(last.tip, path[40].tip) < 2e-3,
                    "{name} {d:?} {tcp}: {last:?}"
                );
                // Everywhere between, the tool is on the path within the post's tolerances
                // (and the words' rounding).
                let tol = &m.post.rotary;
                for at in &run {
                    let (off, lean) = off_path(&path, at);
                    assert!(
                        off < tol.tolerance + 2e-3,
                        "{name} {d:?} tcp {tcp}: {off} mm off at {at:?}"
                    );
                    assert!(
                        lean < tol.axis_tolerance + 0.05,
                        "{name} {d:?} tcp {tcp}: {lean} deg"
                    );
                }
                // And every location of the path is passed.
                for cl in &path {
                    let near = run
                        .iter()
                        .map(|r| dist(r.tip, cl.tip))
                        .fold(f64::INFINITY, f64::min);
                    assert!(near < 1.2, "{name} {d:?}: {near}");
                }
                // Inverse time: the blocks' times add up to the path's length at its feed.
                if !tcp {
                    let feeds: Vec<&ReadMove> = moves.iter().filter(|m| !m.rapid).collect();
                    assert!(
                        feeds.iter().all(|m| m.inverse_time && m.feed.is_some()),
                        "{nc}"
                    );
                    let minutes: f64 = feeds.iter().map(|m| 1.0 / m.feed.unwrap()).sum();
                    let want: f64 = path
                        .windows(2)
                        .map(|w| dist(w[0].tip, w[1].tip))
                        .sum::<f64>()
                        / feed;
                    assert!(
                        (minutes / want - 1.0).abs() < 0.02,
                        "{name} {d:?}: {minutes} against {want}"
                    );
                    assert!(nc.contains("G93") && nc.contains("G94"));
                }
                posted += 1;
            }
        }
    }
    // Six machines on five controls, and the five with two rotary axes again on the three
    // controls with tool centre point control.
    assert_eq!(posted, 6 * 5 + 5 * 3);
}

/// The last feed move of a program read back and the move before it.
fn last_feed(moves: &[ReadMove]) -> (&ReadMove, &ReadMove) {
    let i = moves.iter().rposition(|m| !m.rapid).unwrap();
    (&moves[i - 1], &moves[i])
}

/// The lines of a program without their sequence numbers.
fn lines(nc: &str) -> Vec<String> {
    nc.lines()
        .map(|l| {
            l.split_once(' ')
                .filter(|(n, _)| n.starts_with('N'))
                .map_or(l, |(_, r)| r)
                .to_string()
        })
        .collect()
}

/// brep-bench's `cnc/five-axis-dome`: a ball mill held square to a dome of radius 40. On a
/// trunnion whose axes meet at the dome's centre a pass up a meridian is the trunnion turning
/// alone, and a pass round the pole the platter alone: each is one block of one word.
#[test]
fn a_dome_meridian_is_one_block_of_the_trunnion_and_a_pole_circle_one_of_the_platter() {
    let mut chain = samples::trunnion_ac();
    chain.table[2].origin = [0.0; 3];
    chain.table[3].origin = [0.0; 3];
    chain.table_mount = Frame::WORLD;
    let n = |lat: f64, lon: f64| {
        let (a, o) = (lat.to_radians(), lon.to_radians());
        [a.cos() * o.cos(), a.cos() * o.sin(), a.sin()]
    };
    let at = |lat: f64, lon: f64| Cl::new(scale(n(lat, lon), 40.0), n(lat, lon));
    let meridian: Vec<Cl> = (0..=72).map(|k| at(3.0 + k as f64, 30.0)).collect();
    let circle: Vec<Cl> = (0..=360).map(|k| at(80.0, k as f64)).collect();
    let m = machine_of(chain, Dialect::LinuxCnc, false);
    let nc = post(&program(vec![leaning_op(&meridian, 500.0)]), &m, &m.post).unwrap();
    let feeds: Vec<String> = lines(&nc)
        .into_iter()
        .filter(|l| l.starts_with("G1 "))
        .collect();
    assert_eq!(feeds.len(), 1, "{nc}");
    // 72° of the trunnion and nothing else, in the time the ball takes over the arc.
    let arc = 40.0 * 72f64.to_radians();
    let words: Vec<&str> = feeds[0].split(' ').collect();
    assert_eq!(words.len(), 3, "{nc}");
    assert!(
        words[1].starts_with('A') && words[2].starts_with('F'),
        "{nc}"
    );
    let moves = read(&nc);
    let (start, end) = last_feed(&moves);
    assert!(
        ((end.angle('A') - start.angle('A')).abs() - 72.0).abs() < 1e-3,
        "{nc}"
    );
    // (The chords of a degree each are a hair shorter than the arc.)
    assert!(
        (1.0 / end.feed.unwrap() / (arc / 500.0) - 1.0).abs() < 1e-3,
        "{nc}"
    );
    // Round the pole: one block, the platter a whole turn.
    let nc = post(&program(vec![leaning_op(&circle, 500.0)]), &m, &m.post).unwrap();
    let feeds: Vec<String> = lines(&nc)
        .into_iter()
        .filter(|l| l.starts_with("G1 "))
        .collect();
    assert_eq!(feeds.len(), 1, "{nc}");
    assert!(feeds[0].split(' ').nth(1).unwrap().starts_with('C'), "{nc}");
    let moves = read(&nc);
    let (start, end) = last_feed(&moves);
    assert!(
        ((end.angle('C') - start.angle('C')).abs() - 360.0).abs() < 1e-3,
        "{nc}"
    );
}

/// The worm of brep-bench's `swept/worm-wheel-hobbed-by-its-worm`: a single-start thread of
/// lead 2π at pitch radius 10. A cutter following it on a rotary axis is one block: X and A in
/// step, X by the lead for every turn.
#[test]
fn a_worm_thread_is_one_block_of_a_slide_and_a_turn_in_step() {
    let chain = samples::fourth_axis();
    let lead = std::f64::consts::TAU;
    let (radius, turns) = (10.0, 5.0);
    let path: Vec<Cl> = (0..=(turns * 360.0) as usize)
        .map(|k| {
            let axis = turned(Z, X, k as f64);
            Cl::new(
                add([lead * k as f64 / 360.0, 0.0, 0.0], scale(axis, radius)),
                axis,
            )
        })
        .collect();
    for d in Dialect::ALL {
        let m = machine_of(chain.clone(), d, false);
        let nc = post(&program(vec![leaning_op(&path, 300.0)]), &m, &m.post).unwrap();
        let feeds: Vec<String> = lines(&nc)
            .into_iter()
            .filter(|l| l.starts_with("G1 "))
            .collect();
        assert_eq!(feeds.len(), 1, "{d:?}: {nc}");
        let moves = read(&nc);
        let (start, end) = last_feed(&moves);
        let (dx, da) = (end.xyz[0] - start.xyz[0], end.angle('A') - start.angle('A'));
        assert!((da.abs() - turns * 360.0).abs() < 1e-3, "{d:?}: {nc}");
        assert!((dx - lead * da.abs() / 360.0).abs() < 1e-3, "{d:?}: {nc}");
        // Y and Z never move: neither is written in the feed block.
        assert!(
            !feeds[0].contains('Y') && !feeds[0].contains('Z'),
            "{d:?}: {nc}"
        );
        // The time is the helix's length at the feed.
        let helix = (turns * std::f64::consts::TAU) * radius.hypot(lead / std::f64::consts::TAU);
        assert!(
            (1.0 / end.feed.unwrap() / (helix / 300.0) - 1.0).abs() < 1e-3,
            "{d:?}: {nc}"
        );
    }
}

/// A square cut in a plane tilted onto the part's side.
fn tilted_op() -> (NcOperation, Frame) {
    let frame = Frame::from_zx([0.0, -20.0, 5.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]);
    let mut steps = vec![
        Step::Rapid([0.0, 0.0, 5.0]),
        Step::Spindle {
            rpm: 9000.0,
            cw: true,
        },
        Step::Feed {
            to: [0.0, 0.0, -1.0],
            feed: 200.0,
            kind: FeedKind::Plunge,
        },
    ];
    for p in [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]] {
        steps.push(Step::Feed {
            to: [p[0], p[1], -1.0],
            feed: 800.0,
            kind: FeedKind::Cut,
        });
    }
    steps.push(Step::Rapid([0.0, 0.0, 5.0]));
    let op = NcOperation {
        name: "Side".into(),
        tool: Tool::new(ToolKind::EndMill, 6.0),
        tool_number: 1,
        toolpath: Toolpath {
            steps,
            warnings: vec![],
        },
        coolant: Coolant::Off,
        datum: 1,
        stop_before: false,
        force_tool_change: false,
        tilt: Some(Tilt::of(&frame)),
    };
    (op, frame)
}

#[test]
fn a_tilted_plane_is_posted_as_rotary_positions_where_the_control_has_no_tilted_plane() {
    let five = catalog()
        .into_iter()
        .find(|m| m.indexing.is_some())
        .unwrap();
    let (op, frame) = tilted_op();
    let mut flat = op.clone();
    flat.tilt = None;
    flat.name = "Top".into();
    for d in [
        Dialect::LinuxCnc,
        Dialect::Grbl,
        Dialect::Mach3,
        Dialect::Haas,
    ] {
        let mut m = five.clone();
        m.dialect = d;
        m.post = PostSettings::for_dialect(d, MachineKind::Mill);
        m.post.rotary.tcp = false;
        let nc = post(&program(vec![op.clone(), flat.clone()]), &m, &m.post).unwrap();
        let text = lines(&nc);
        // The plane's Z is the work's −Y: the trunnion a quarter turn, and no G68.2.
        assert!(!nc.contains("G68.2"), "{nc}");
        let turn = text
            .iter()
            .position(|l| l.starts_with("G0 A"))
            .unwrap_or_else(|| panic!("{nc}"));
        assert!(
            text[turn].contains("A90") || text[turn].contains("A-90"),
            "{d:?}: {nc}"
        );
        // Up clear before the axes turn: a home retract, and nothing that moves Z after it.
        let up = text[..turn]
            .iter()
            .rposition(|l| l.contains("G53 G0 Z") || l.contains("G28 Z0"))
            .unwrap_or_else(|| panic!("{d:?}: {nc}"));
        assert!(
            text[up + 1..turn].iter().all(|l| !l.contains('Z')),
            "{d:?}: {nc}"
        );
        // The moves read back onto the square, in the setup's coordinates.
        let chain = m.kinematics();
        let length = op.tool.gauge_length();
        let moves = read(&nc);
        let side: Vec<ReadMove> = moves
            .iter()
            .take_while(|m| m.angle('A').abs() > 1.0)
            .skip_while(|m| m.xyz[2].is_nan())
            .cloned()
            .collect();
        let corner = |x: f64, y: f64| frame.to_world([x, y, -1.0]);
        for want in [
            corner(10.0, 0.0),
            corner(10.0, 10.0),
            corner(0.0, 10.0),
            corner(0.0, 0.0),
        ] {
            let hit = side.iter().any(|m| {
                let at = chain.locate(&joints(&chain, &Frame::WORLD, length, m), length);
                dist(at.tip, want) < 2e-3 && angle_between(at.axis, frame.z) < 1e-3
            });
            assert!(hit, "{d:?}: no block at {want:?}\n{nc}");
        }
        // Back to zero with the tool up before the flat operation, and at the end.
        let home = text
            .iter()
            .position(|l| l == "G0 A0 C0" || l == "G0 A0. C0.")
            .unwrap_or_else(|| panic!("{nc}"));
        assert!(home > turn, "{nc}");
        // (The flat operation's first move is the first block after it.)
        let top = text.iter().rposition(|l| l.contains("Top")).unwrap();
        assert!(home > top && text[home + 1].starts_with("G0 X0"), "{nc}");
        // Nothing turns while the side is cut: its feeds are plain ones.
        assert!(!nc.contains("G93"), "{d:?}: {nc}");
        assert!(
            text.iter().any(|l| l == "X10 F800" || l == "X10. F800"),
            "{d:?}: {nc}"
        );
    }
    // Fanuc keeps its own tilted working plane.
    let fanuc = post(&program(vec![op.clone()]), &five, &five.post).unwrap();
    assert!(
        fanuc.contains("G68.2") && fanuc.contains("G53.1"),
        "{fanuc}"
    );
    // Unless the machine is set to post positions.
    let mut by_positions = five.clone();
    by_positions.post.rotary.tilted_plane = TiltedPlane::Positions;
    let nc = post(&program(vec![op]), &by_positions, &by_positions.post).unwrap();
    assert!(
        !nc.contains("G68.2") && nc.contains("G43.4 H1") && nc.contains("G49"),
        "{nc}"
    );
}

#[test]
fn tool_centre_point_control_is_switched_as_the_control_does_it() {
    let path: Vec<Cl> = sweep(None, 40.0);
    let chain = samples::trunnion_ac();
    let has = |d: Dialect, on: &str, off: &str| {
        let m = machine_of(chain.clone(), d, true);
        let nc = post(&program(vec![leaning_op(&path, 500.0)]), &m, &m.post).unwrap();
        let text = lines(&nc);
        let (a, b) = (
            text.iter().position(|l| l == on),
            text.iter().position(|l| l == off),
        );
        assert!(a.is_some() && b > a, "{d:?}: {nc}");
        // The tip is fed at the path's own feed: no inverse time.
        assert!(!nc.contains("G93") && nc.contains("F500"), "{d:?}: {nc}");
        nc
    };
    has(Dialect::Fanuc, "G43.4 H3", "G49");
    has(Dialect::Haas, "G234 H3", "G49");
    let lcnc = has(Dialect::LinuxCnc, "M428", "M429");
    assert!(lcnc.contains("G43 H3"), "{lcnc}");
    // A control without it posts the slides, whatever the setting.
    let m = machine_of(chain.clone(), Dialect::Grbl, true);
    let nc = post(&program(vec![leaning_op(&path, 500.0)]), &m, &m.post).unwrap();
    assert!(nc.contains("G93") && !nc.contains("G43"), "{nc}");
    // The machine's own codes replace the family's.
    let mut m = machine_of(chain, Dialect::Mach3, true);
    m.post.rotary.tcp_on = "M200; G43 H{h}".into();
    m.post.rotary.tcp_off = "M201".into();
    let nc = post(&program(vec![leaning_op(&path, 500.0)]), &m, &m.post).unwrap();
    let text = lines(&nc);
    assert!(
        text.contains(&"M200".to_string()) && text.contains(&"G43 H3".to_string()),
        "{nc}"
    );
    assert!(text.contains(&"M201".to_string()), "{nc}");
}

#[test]
fn what_the_machine_cannot_do_is_said() {
    let path: Vec<Cl> = sweep(None, 40.0);
    let op = leaning_op(&path, 500.0);
    // Three axes cannot lean.
    let three = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill)
        .unwrap();
    assert!(matches!(
        post(&program(vec![op.clone()]), &three, &three.post),
        Err(PostError::CantTilt(_))
    ));
    // A trunnion that stops short of the lean.
    let mut chain = samples::trunnion_ac();
    (chain.table[2].min, chain.table[2].max) = (-20.0, 20.0);
    let m = machine_of(chain, Dialect::LinuxCnc, false);
    let err = post(&program(vec![op.clone()]), &m, &m.post).unwrap_err();
    assert!(
        matches!(&err, PostError::Travel(s) if s.contains("A would be at")),
        "{err:?}"
    );
    // An arm has no post.
    let m = machine_of(samples::arm(), Dialect::LinuxCnc, false);
    assert!(matches!(
        post(&program(vec![op.clone()]), &m, &m.post),
        Err(PostError::NoPost(_))
    ));
    // A laser turns nothing.
    let mut laser = catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Laser)
        .unwrap();
    laser.chain = Some(samples::trunnion_ac());
    let mut beam = op;
    beam.tool = Tool::new(ToolKind::Laser, 0.2);
    assert!(matches!(
        post(&program(vec![beam]), &laser, &laser.post),
        Err(PostError::CantTilt(_))
    ));
}

#[test]
fn a_rotary_axis_at_its_stop_comes_round_with_the_tool_lifted() {
    // Three turns round a bar on a rotary axis that stops at ±200°.
    let mut chain = samples::fourth_axis();
    chain.table[0].continuous = false;
    (chain.table[0].min, chain.table[0].max) = (-200.0, 200.0);
    let path: Vec<Cl> = (0..=1080)
        .step_by(5)
        .map(|k| {
            let axis = turned(Z, X, k as f64);
            Cl::new(add([k as f64 / 36.0, 0.0, 0.0], scale(axis, 15.0)), axis)
        })
        .collect();
    let m = machine_of(chain.clone(), Dialect::LinuxCnc, false);
    let nc = post(&program(vec![leaning_op(&path, 400.0)]), &m, &m.post).unwrap();
    let moves = read(&nc);
    assert!(
        moves.iter().all(|m| m.angle('A').abs() <= 200.0 + 1e-6),
        "{nc}"
    );
    assert!(nc.matches("Rewind").count() >= 2, "{nc}");
    // Each rewind lifts the tool by the post's retract before the axis comes round: the
    // rapid that turns it is made 25 mm off the bar.
    let length = tool().gauge_length();
    let turns: Vec<&ReadMove> = moves
        .windows(2)
        .filter(|w| w[1].rapid && (w[1].angle('A') - w[0].angle('A')).abs() > 180.0)
        .map(|w| &w[1])
        .collect();
    assert!(turns.len() >= 2, "{nc}");
    for t in turns {
        let at = chain.locate(&joints(&chain, &Frame::WORLD, length, t), length);
        let from_axis = (at.tip[1].powi(2) + at.tip[2].powi(2)).sqrt();
        assert!((from_axis - 40.0).abs() < 1e-2, "{from_axis}");
    }
    // And the cut still ends where the path does.
    let (_, last) = last_feed(&moves);
    let end = chain.locate(&joints(&chain, &Frame::WORLD, length, last), length);
    assert!(dist(end.tip, path.last().unwrap().tip) < 2e-3);
}
