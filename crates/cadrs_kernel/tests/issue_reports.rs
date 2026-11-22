//! Bugs reported on GitHub, rebuilt from their descriptions, each with the cases round it: the
//! report's own shape is one point of a family (a count, a size, a way of building the same
//! body), and the family is what is tested. Expected values are closed forms, or identities
//! between results that must agree (a body shells alike however it was built; a cut by one
//! joined tool is the cut by its pieces).
//!
//! - #300: a part pattern's pins that overlap one another, cut from a ring one by one and
//!   joined.
//! - #297: a shell of a body whose end is a full round.
//! - #299: a fillet beside a chamfer, smaller than it, its size and larger.
//!
//! A test that fails today is `#[ignore]`d with the gap it waits on, as the springrep suites'.

use std::f64::consts::PI;

use cadrs_kernel::native::kernel::NativeKernel;
use cadrs_kernel::native::topo::CheckLevel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

/// The outline `curves` on `plane`, extruded by `h`.
fn prism(k: &mut NativeKernel, plane: Plane, curves: Vec<Curve2>, h: f64) -> BodyId {
    let region = Region {
        outer: Loop { curves },
        holes: vec![],
        source: None,
    };
    k.extrude(&Profile::new(plane, vec![region]), Extent::Blind(h))
        .unwrap()
        .bodies[0]
}

fn line(a: Point2<f64>, b: Point2<f64>) -> Curve2 {
    Curve2::Line { a, b, source: None }
}

/// The polygon through `pts` on Top, extruded by `h`.
fn polygon(k: &mut NativeKernel, pts: &[(f64, f64)], h: f64) -> BodyId {
    let c: Vec<Point2<f64>> = pts.iter().map(|&(x, y)| Point2::new(x, y)).collect();
    let curves = (0..c.len())
        .map(|i| line(c[i], c[(i + 1) % c.len()]))
        .collect();
    prism(k, Plane::top(), curves, h)
}

/// The circle of radius `r` about `c` on `plane`, extruded by `h`.
fn rod(k: &mut NativeKernel, plane: Plane, c: (f64, f64), r: f64, h: f64) -> BodyId {
    let circle = Curve2::Circle {
        center: Point2::new(c.0, c.1),
        radius: r,
        source: None,
    };
    prism(k, plane, vec![circle], h)
}

/// The flat face of `b` whose outward normal is `n`.
fn face(k: &NativeKernel, b: BodyId, n: Vector3<f64>) -> FaceId {
    k.faces(b)
        .unwrap()
        .into_iter()
        .find(|f| f.plane.is_some_and(|pl| pl.normal.dot(&n) > 1.0 - 1e-9))
        .expect("a face that way")
        .id
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn area(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().surface_area
}

/// `b` passes the full check (loops, orientation, faces crossing each other).
#[track_caller]
fn sound(k: &NativeKernel, b: BodyId, what: &str) {
    let found = k.get(b).unwrap().check_with(CheckLevel::Full);
    assert!(found.is_empty(), "{what}: {found:?}");
}

#[track_caller]
fn close(got: f64, want: f64, tol: f64, what: &str) {
    assert!(
        (got - want).abs() <= tol,
        "{what}: got {got}, expected {want} ± {tol}"
    );
}

/// Simpson's rule over [a, b] in `n` (even) steps.
fn integral(f: impl Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    let inner: f64 = (1..n)
        .map(|i| f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 })
        .sum();
    (f(a) + inner + f(b)) * h / 3.0
}

// ---------------------------------------------------------------------------------------------
// #300: a circular part pattern's pins (the seed and its copies), Remove, from a ring.

/// The ring's radii and height, and the height of the pins' axes.
const RING: (f64, f64, f64) = (40.0, 30.0, 20.0);
const PIN_Z: f64 = 10.0;
/// The pins reach this far from the ring's axis (past its outside).
const PIN_END: f64 = 50.0;

/// A ring about Z, and the pins of a circular pattern of `count`: a rod of radius `a` along +X
/// from x = `start` out through the ring's wall (the seed, first) and its copies turned about
/// Z by 360°/`count` each. (A part pattern's Remove cuts with the seed and the copies.)
fn ring_and_pins(count: usize, start: f64, a: f64) -> (NativeKernel, BodyId, Vec<BodyId>) {
    let (ro, ri, h) = RING;
    let mut k = NativeKernel::new();
    let outer = rod(&mut k, Plane::top(), (0.0, 0.0), ro, h);
    let bore = rod(&mut k, Plane::top(), (0.0, 0.0), ri, h);
    let ring = k.boolean(BoolOp::Subtract, outer, &[bore]).unwrap().bodies[0];
    // The Right plane moved out along X: sketch x is model Y, sketch y is model Z.
    let right = Plane {
        origin: Point3::new(start, 0.0, 0.0),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let seed = rod(&mut k, right, (0.0, PIN_Z), a, PIN_END - start);
    let z = Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    };
    let pins = std::iter::once(seed)
        .chain((1..count).map(|i| {
            let turn = Motion::rotation(&z, 2.0 * PI * i as f64 / count as f64);
            k.transform_motion(seed, &turn).unwrap().bodies[0]
        }))
        .collect();
    (k, ring, pins)
}

/// What a pin of radius `a` takes out of the ring's wall: over the pin's section (y = a sin φ
/// across, a cos φ up and down), the wall's thickness along the pin there,
/// √(Ro² − y²) − √(Ri² − y²).
fn pin_hole(a: f64) -> f64 {
    let (ro, ri, _) = RING;
    integral(
        |phi| {
            let y = a * phi.sin();
            let wall = (ro * ro - y * y).sqrt() - (ri * ri - y * y).sqrt();
            2.0 * a * a * phi.cos().powi(2) * wall
        },
        -PI / 2.0,
        PI / 2.0,
        2000,
    )
}

/// The pins as tools one by one (as a part pattern's Remove has them), and joined into one body
/// first (as its Add and Intersect join them): the same cut, each hole the closed form; and the
/// union and intersection with the joined body account for every bit of volume
/// (|A ∪ B| + |A ∩ B| = |A| + |B|). `skip` leaves one pin out (a skipped instance).
fn pins_cut_the_ring(count: usize, start: f64, a: f64, skip: Option<usize>) {
    let (mut k, ring, mut pins) = ring_and_pins(count, start, a);
    if let Some(i) = skip {
        pins.remove(i);
    }
    let what = format!("{count} round less {skip:?}, from x = {start}, radius {a}");
    let holes = pins.len() as f64 * pin_hole(a);
    let v_ring = volume(&k, ring);
    let tol = 1e-6 * v_ring;

    let apart = k.boolean(BoolOp::Subtract, ring, &pins).unwrap().bodies[0];
    sound(&k, apart, &what);
    close(volume(&k, apart), v_ring - holes, tol, &what);

    let joined = match pins[..] {
        [one] => one,
        _ => {
            let u = k.boolean(BoolOp::Union, pins[0], &pins[1..]).unwrap();
            assert_eq!(u.bodies.len(), 1, "{what}: the pins as one body");
            u.bodies[0]
        }
    };
    sound(&k, joined, &what);
    let v_joined = volume(&k, joined);

    let cut = k.boolean(BoolOp::Subtract, ring, &[joined]);
    let cut = cut
        .unwrap_or_else(|e| panic!("{what}: ring − pins: {e}"))
        .bodies[0];
    sound(&k, cut, &what);
    close(volume(&k, cut), v_ring - holes, tol, &what);
    close(
        area(&k, cut),
        area(&k, apart),
        1e-6 * area(&k, apart),
        &what,
    );
    assert_eq!(
        k.faces(cut).unwrap().len(),
        k.faces(apart).unwrap().len(),
        "{what}: the same faces either way"
    );

    let both = k.boolean(BoolOp::Union, ring, &[joined]);
    let both = both
        .unwrap_or_else(|e| panic!("{what}: ring ∪ pins: {e}"))
        .bodies[0];
    sound(&k, both, &what);
    close(volume(&k, both), v_ring + v_joined - holes, tol, &what);

    let common = k.boolean(BoolOp::Intersect, ring, &[joined]);
    let common = common.unwrap_or_else(|e| panic!("{what}: ring ∩ pins: {e}"));
    let v_common: f64 = common.bodies.iter().map(|b| volume(&k, *b)).sum();
    close(v_common, holes, tol, &what);
}

/// Issue #300's family: the pins meet at the ring's axis (the pin sketched on a plane through
/// it and extruded outward), where each overlaps its neighbours. All of them, and all but the
/// seed.
#[test]
fn pins_patterned_from_the_axis_cut_a_ring() {
    for count in [2, 3, 5, 6, 8, 12] {
        pins_cut_the_ring(count, 0.0, 4.0, None);
        pins_cut_the_ring(count, 0.0, 4.0, Some(0));
    }
}

/// Four round, but the pins start short of the axis: their ends just clear of each other (5),
/// touching along a line (4), and overlapping by a sliver (3).
#[test]
fn four_pins_that_stop_short_of_the_axis_cut_a_ring() {
    for start in [5.0, 4.0, 3.0] {
        pins_cut_the_ring(4, start, 4.0, None);
        pins_cut_the_ring(4, start, 4.0, Some(0));
    }
}

/// Four round from the axis: joined, a cross of two rods of one radius.
#[test]
fn four_pins_from_the_axis_cut_a_ring() {
    pins_cut_the_ring(4, 0.0, 4.0, None);
    pins_cut_the_ring(4, 0.0, 3.0, None);
}

/// Three of the four: they join into a T (the two on one line into one rod, the third meeting
/// it square, all of one radius, so the T's two rods cross in two half ellipses that touch),
/// and the T as a tool leaves the boolean an open shell, for Remove, Add and Intersect alike.
/// (The same pins as three tools cut it, which is how a part pattern's Remove has them.)
#[test]
#[ignore = "a T of three equal rods meeting at the axis, as one tool on a ring, leaves an open shell"]
fn three_of_four_pins_from_the_axis_cut_a_ring() {
    pins_cut_the_ring(4, 0.0, 4.0, Some(0));
    pins_cut_the_ring(4, 0.0, 3.0, Some(0));
}

// ---------------------------------------------------------------------------------------------
// #297: a shell of a body whose end is a full round.

/// A bar along X, `l` long, `w` wide at x = 0 and `h` high, its sides leaning in by `d` each
/// over its length.
const BAR: (f64, f64, f64) = (30.0, 10.0, 8.0);

/// The radius of the round touching the bar's end (x = l) and both sides, and the unit normal
/// of the side at low y: ρ(len − d) = l(w/2 − d), from the distance of the round's centre
/// (l − ρ, w/2) to that side.
fn end_round(d: f64) -> (f64, Vector3<f64>) {
    let (l, w, _) = BAR;
    let len = l.hypot(d);
    (
        l * (w / 2.0 - d) / (len - d),
        Vector3::new(d, -l, 0.0) / len,
    )
}

/// The bar with a square end, to be rounded.
fn square_bar(k: &mut NativeKernel, d: f64) -> BodyId {
    let (l, w, h) = BAR;
    polygon(k, &[(0.0, 0.0), (l, d), (l, w - d), (0.0, w)], h)
}

/// The bar's end rounded by a full round between its sides.
fn bar_by_full_round(k: &mut NativeKernel, d: f64) -> BodyId {
    let b = square_bar(k, d);
    let (_, n) = end_round(d);
    let (low, high) = (face(k, b, n), face(k, b, Vector3::new(n.x, -n.y, 0.0)));
    let end = face(k, b, Vector3::x());
    k.full_round(b, low, end, high).unwrap().bodies[0]
}

/// The same bar from a sketch of its outline: three lines and the tangent arc.
fn bar_by_arc(k: &mut NativeKernel, d: f64) -> BodyId {
    let (l, w, h) = BAR;
    let (rho, n) = end_round(d);
    let c = Point2::new(l - rho, w / 2.0);
    let low = Point2::new(c.x + rho * n.x, c.y + rho * n.y);
    let high = Point2::new(low.x, w - low.y);
    let arc = Curve2::Arc {
        center: c,
        radius: rho,
        start_angle: n.y.atan2(n.x),
        sweep: 2.0 * (-n.y).atan2(n.x),
        source: None,
    };
    let (o, top) = (Point2::new(0.0, 0.0), Point2::new(0.0, w));
    let curves = vec![line(o, low), arc, line(high, top), line(top, o)];
    prism(k, Plane::top(), curves, h)
}

/// The straight bar's end rounded by filleting its two upright edges there by half its width.
fn bar_by_fillets(k: &mut NativeKernel) -> BodyId {
    let (l, w, _) = BAR;
    let b = square_bar(k, 0.0);
    let upright: Vec<EdgeId> = k
        .edges(b)
        .unwrap()
        .iter()
        .filter(|e| (e.start.x - l).abs() < 1e-9 && (e.end.x - l).abs() < 1e-9)
        .filter(|e| (e.start.z - e.end.z).abs() > 1e-9)
        .map(|e| e.id)
        .collect();
    assert_eq!(upright.len(), 2);
    k.fillet_with(b, &upright, &FilletSpec::radius(w / 2.0))
        .unwrap()
        .bodies[0]
}

fn shell_spec(open: FaceId, t: f64, sharp: bool, outward: bool) -> ShellSpec {
    ShellSpec {
        remove: vec![open],
        thickness: t,
        outward,
        hollow: false,
        sharp,
        nothing_hollowed: NothingHollowed::default(),
    }
}

/// The bar's flat faces by name: the two the round's axis pierces, the one opposite the round
/// and the two the round runs into.
fn bar_openings(d: f64) -> [(&'static str, Vector3<f64>); 5] {
    let (_, n) = end_round(d);
    [
        ("top", Vector3::z()),
        ("bottom", -Vector3::z()),
        ("back", -Vector3::x()),
        ("low side", n),
        ("high side", Vector3::new(n.x, -n.y, 0.0)),
    ]
}

/// The rounded outward shell of a leaning bar opened at its back fails today however the bar
/// was made ([`a_rounded_outward_shell_of_a_leaning_round_bar_opens_at_its_back`]).
fn waits(d: f64, opening: &str, sharp: bool, outward: bool) -> bool {
    d > 0.0 && opening == "back" && !sharp && outward
}

/// Issue #297: two parts of the same outside shape, one extruded with its round end sketched,
/// one rounded by a full round; the first shelled and the second did not. Whichever way the
/// bar's end was made (full round, sketched arc, two fillets of half its width), with straight
/// or leaning sides, it is one shape: every shell of it builds, sound, to the same volume and
/// area, whichever face is opened, inward or outward, sharp or rounded.
#[test]
fn a_round_ended_bar_shells_alike_however_its_end_was_made() {
    let (l, w, h) = BAR;
    let t = 0.8;
    for lean in [0.0, 0.1, 0.25] {
        let d = lean * w / 2.0;
        let mut k = NativeKernel::new();
        let mut bars = vec![
            ("full round", bar_by_full_round(&mut k, d)),
            ("arc", bar_by_arc(&mut k, d)),
        ];
        if d == 0.0 {
            bars.push(("fillets", bar_by_fillets(&mut k)));
        }
        let (v0, a0) = (volume(&k, bars[0].1), area(&k, bars[0].1));
        for &(made, b) in &bars {
            let what = format!("lean {lean}, by {made}");
            sound(&k, b, &what);
            assert_eq!(
                k.faces(b).unwrap().len(),
                6,
                "{what}: one face for the round"
            );
            close(volume(&k, b), v0, 1e-9 * v0, &what);
            close(area(&k, b), a0, 1e-9 * a0, &what);
        }
        for (opening, n) in bar_openings(d) {
            for (sharp, outward) in [(true, false), (false, false), (true, true), (false, true)] {
                if waits(d, opening, sharp, outward) {
                    continue;
                }
                let mut first: Option<(f64, f64, usize)> = None;
                for &(made, b) in &bars {
                    let what = format!(
                        "lean {lean}, by {made}, open {opening}, sharp {sharp}, outward {outward}"
                    );
                    let open = face(&k, b, n);
                    let r = k.shell_with(b, &shell_spec(open, t, sharp, outward));
                    let s = r.unwrap_or_else(|e| panic!("{what}: {e}")).bodies[0];
                    sound(&k, s, &what);
                    let got = (volume(&k, s), area(&k, s), k.faces(s).unwrap().len());
                    let want = *first.get_or_insert(got);
                    close(got.0, want.0, 1e-7 * want.0, &what);
                    close(got.1, want.1, 1e-7 * want.1, &what);
                    assert_eq!(got.2, want.2, "{what}: faces");
                    if !outward {
                        assert!(got.0 < v0, "{what}: nothing was hollowed");
                    }
                }
                // The straight bar's inward shells by hand: the bar less its cavity, the
                // outline inset by t (its round's radius r − t) under the walls that stay.
                if d == 0.0 && !outward {
                    let r = w / 2.0;
                    let round = PI * (r - t) * (r - t) / 2.0;
                    let cavity = match opening {
                        "top" | "bottom" => ((l - r - t) * (w - 2.0 * t) + round) * (h - t),
                        "back" => ((l - r) * (w - 2.0 * t) + round) * (h - 2.0 * t),
                        _ => continue,
                    };
                    let got = first.expect("shelled").0;
                    close(got, v0 - cavity, 1e-9 * v0, &format!("open {opening}"));
                }
            }
        }
    }
}

/// Found beside #297: the walls of a leaning round-ended bar grown outward with rounded
/// corners, the face opposite the round removed. The sharp shell of it builds; the rounded one
/// leaves its boolean an open shell, whether the round was a full round or a sketched arc.
#[test]
#[ignore = "a rounded outward shell of a tapered bar with a round end, opened opposite the round, leaves an open shell"]
fn a_rounded_outward_shell_of_a_leaning_round_bar_opens_at_its_back() {
    let (_, w, _) = BAR;
    for lean in [0.1, 0.25] {
        let d = lean * w / 2.0;
        let mut k = NativeKernel::new();
        let b = bar_by_arc(&mut k, d);
        let back = face(&k, b, -Vector3::x());
        let sharp = k.shell_with(b, &shell_spec(back, 0.8, true, true)).unwrap();
        let rounded = k
            .shell_with(b, &shell_spec(back, 0.8, false, true))
            .unwrap();
        sound(&k, rounded.bodies[0], "rounded");
        // Rounding the outside takes a little off the sharp corners, no more.
        let (vs, vr) = (volume(&k, sharp.bodies[0]), volume(&k, rounded.bodies[0]));
        assert!(vr < vs && vr > 0.95 * vs, "lean {lean}: {vr} of {vs}");
    }
}

// ---------------------------------------------------------------------------------------------
// #299: a fillet beside a chamfer.

/// A block from the origin, and the chamfer `c` of its top edge at x = l (along Y).
const BLOCK: (f64, f64, f64) = (40.0, 30.0, 20.0);

fn chamfered_block(k: &mut NativeKernel, spec: ChamferSpec) -> BodyId {
    let (l, w, h) = BLOCK;
    let b = polygon(k, &[(0.0, 0.0), (l, 0.0), (l, w), (0.0, w)], h);
    let edge = k
        .edges(b)
        .unwrap()
        .into_iter()
        .find(|e| (e.mid - Point3::new(l, w / 2.0, h)).norm() < 1e-9)
        .expect("the top edge at x = l")
        .id;
    k.chamfer(b, &[edge], spec).unwrap().bodies[0]
}

/// How far a fillet of radius `r` between two square faces stands off one of them at `u` from
/// the other, written over the angle φ round its arc: u = r(1 − cos φ), the gap r(1 − sin φ).
/// `along(u)` is the body's length along the edge there; the fillet removes ∫ gap · along du.
fn square_fillet(r: f64, along: impl Fn(f64) -> f64, from: f64, to: f64) -> f64 {
    integral(
        |phi| {
            let u = r * (1.0 - phi.cos());
            r * (1.0 - phi.sin()) * along(u) * r * phi.sin()
        },
        from,
        to,
        2000,
    )
}

/// What a fillet of radius `r` removes along an edge of the block chamfered by `c`, by where
/// the edge's middle is. Every edge is between two flat faces, so the fillet is a cylinder cut
/// off by the faces at its ends, and what it removes is its section (the corner less the
/// quarter disc) times the body's length along the edge at each point of the section.
fn fillet_removes(mid: Point3<f64>, c: f64, r: f64) -> f64 {
    let (l, w, h) = BLOCK;
    let at = |v: f64, x: f64| (v - x).abs() < 1e-9;
    let plain = |len: f64| (1.0 - PI / 4.0) * r * r * len;
    // An edge that runs `len` to the face the chamfer cut, and there meets the chamfer: at `u`
    // from that face the body goes on `u` further under the chamfer, until the chamfer's top
    // edge (`u` = c), past which it has its full length (a fillet larger than the chamfer runs
    // on over the face beyond it). The integral is split at that corner.
    let up_to_the_chamfer = |len: f64| {
        let along = |u: f64| len - c + u.min(c);
        let corner = if r > c {
            (1.0 - c / r).acos()
        } else {
            PI / 2.0
        };
        square_fillet(r, along, 0.0, corner) + square_fillet(r, along, corner, PI / 2.0)
    };
    let (on_chamfer_x, on_chamfer_z) = (at(mid.x, l - c / 2.0), at(mid.z, h - c / 2.0));
    if at(mid.x, l - c) && at(mid.z, h) || at(mid.x, l) && at(mid.z, h - c) {
        // The chamfer's two long edges: faces at 135°, so the contacts are r·tan(π/8) from the
        // edge and the arc turns π/4.
        r * r * ((PI / 8.0).tan() - PI / 8.0) * w
    } else if on_chamfer_x && on_chamfer_z {
        // The chamfer's two short edges, on the side faces: the side face widens by the depth
        // into it at both ends (the top and the face x = l, each at 45° to the edge).
        square_fillet(r, |u| c * 2f64.sqrt() + 2.0 * u, 0.0, PI / 2.0)
    } else if at(mid.x, l) && !at(mid.z, 0.0) {
        up_to_the_chamfer(h)
    } else if at(mid.z, h) && !at(mid.x, 0.0) {
        up_to_the_chamfer(l)
    } else if at(mid.x, 0.0) && (at(mid.y, 0.0) || at(mid.y, w)) {
        plain(h)
    } else if at(mid.y, w / 2.0) {
        plain(w)
    } else {
        plain(l)
    }
}

/// Issue #299: "a simple fillet fails when added to an edge that has a chamfer larger than the
/// fillet" (a 5 mm chamfer: R6 built, R4 did not). Every edge of the chamfered block, one at a
/// time, with fillets smaller than the chamfer, of its size and larger, the chamfer given as a
/// distance and as a distance and 45°: each builds, sound, and removes its closed form. (The
/// fillets of the edges that end at the chamfer end on it when they are smaller, and run on
/// over the face past it when they are larger.)
#[test]
fn a_fillet_beside_a_chamfer_of_any_size() {
    for c in [5.0, 7.0] {
        let specs = [
            ChamferSpec::EqualDistance(c),
            ChamferSpec::DistanceAngle {
                distance: c,
                angle: PI / 4.0,
            },
        ];
        for spec in specs {
            let mut k = NativeKernel::new();
            let b = chamfered_block(&mut k, spec);
            let (l, w, h) = BLOCK;
            let v0 = volume(&k, b);
            close(v0, l * w * h - c * c / 2.0 * w, 1e-6, "the chamfer");
            let edges = k.edges(b).unwrap();
            assert_eq!(edges.len(), 15);
            for r in [1.0, 4.0, 5.0, 6.0, 7.0, 8.0] {
                for e in &edges {
                    let what = format!("{spec:?}, R{r} on the edge at {:.1?}", e.mid.coords);
                    let f = k.fillet_with(b, &[e.id], &FilletSpec::radius(r));
                    let f = f.unwrap_or_else(|err| panic!("{what}: {err}")).bodies[0];
                    sound(&k, f, &what);
                    close(v0 - volume(&k, f), fillet_removes(e.mid, c, r), 1e-6, &what);
                }
            }
        }
    }
}

/// Beside #299: the two edges of a side face that end at the chamfer, either side of its short
/// edge there (the upright one at x = l and the top one), filleted together. Their fillets'
/// contacts on the side face cross when 2r > c: the two then share what both would remove,
/// the corner where at `u` from one face and `t` from the other (u + t > c, under the chamfer)
/// the smaller gap is taken once. Apart (2r ≤ c) they remove what each removes alone.
#[test]
fn fillets_either_side_of_a_chamfers_end_share_their_overlap() {
    let (l, _, h) = BLOCK;
    let c = 5.0;
    let mut k = NativeKernel::new();
    let b = chamfered_block(&mut k, ChamferSpec::EqualDistance(c));
    let v0 = volume(&k, b);
    let edges = k.edges(b).unwrap();
    let at = |p: Point3<f64>| {
        let e = edges.iter().find(|e| (e.mid - p).norm() < 1e-9);
        e.expect("an edge there").clone()
    };
    let upright = at(Point3::new(l, 0.0, (h - c) / 2.0));
    let top = at(Point3::new((l - c) / 2.0, 0.0, h));
    for r in [1.0, 2.5, 4.0] {
        let what = format!("R{r}");
        let f = k.fillet_with(b, &[upright.id, top.id], &FilletSpec::radius(r));
        let f = f.unwrap_or_else(|err| panic!("{what}: {err}")).bodies[0];
        sound(&k, f, &what);
        let gap = |t: f64| r - (r * r - (r - t) * (r - t)).sqrt();
        // Over t from c/2 to r, the gap at t (the smaller of the two for u < t) times the
        // width of u there, t − (c − t); twice, for u > t.
        let shared = if 2.0 * r > c {
            2.0 * integral(|t| gap(t) * (2.0 * t - c), c / 2.0, r, 2000)
        } else {
            0.0
        };
        let alone = fillet_removes(upright.mid, c, r) + fillet_removes(top.mid, c, r);
        close(v0 - volume(&k, f), alone - shared, 1e-6, &what);
    }
}
