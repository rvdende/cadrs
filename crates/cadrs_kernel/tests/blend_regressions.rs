//! Blends from models that failed in the app (their input bodies dumped with `CADRS_DUMP_DIR`).

use cadrs_kernel::native::*;
use cadrs_kernel::*;

/// A 2 mm chamfer along a round cut's rim, which runs over the top face, down two 5 mm fillets
/// and the side face: the contacts cross from the fillets onto the flat faces, and the sections
/// on either side of each crossing must agree (the boolean left an open shell).
#[test]
fn chamfer_over_fillets() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/chamfer_over_fillets.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges = [14, 16, 22, 25, 26].map(EdgeId);
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    // About the chain's length (176 mm) times the section (2 mm² where the faces are square,
    // 1.5 down the side where the pocket meets it at 49°), less where it bends. The local
    // chamfer builds it (the piece a hair long by edge 26's roll at its end goes, its sections
    // the neighbour's): its rails through the contacts of the ball d·tan(θ/2), as everywhere.
    // The blend tool measures d round the pocket's face instead: 323.15, the two differing by
    // a sliver along the side's two vertical rims.
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((316.0..316.7).contains(&removed), "{removed}");
    // The chamfer ends flush with the round cut's floor (z = 15.5) instead of digging into it:
    // the floor only gains the chamfer's two end sections (under 2 mm² each).
    let floor = |id: BodyId| -> f64 {
        k.faces(id)
            .unwrap()
            .iter()
            .filter(|f| f.kind == SurfaceKind::Plane && (f.center.z - 15.5).abs() < 1e-6)
            .map(|f| f.area)
            .sum()
    };
    let gained = floor(out) - floor(b);
    assert!(
        gained > 0.0 && gained < 4.0,
        "floor area {} -> {}",
        floor(b),
        floor(out)
    );
}

/// The same model's top arc alone (Tangent propagation off): the chamfer stops square at the
/// arc's ends, where the rim runs on smoothly over the fillets, instead of running on past them.
#[test]
fn chamfer_stops_where_the_chain_runs_on() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/chamfer_over_fillets.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let r = k
        .chamfer_with(
            b,
            &[EdgeId(22)],
            &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    // The arc (103.5 mm) times the section (2 mm², about 3% more for the section's offset from
    // the round cut's axis).
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((207.0..216.0).contains(&removed), "{removed}");
}

/// A 2 mm chamfer round a block's top: an outline with a notch (two inside corners) and a
/// rectangular hole (four more). Each pair of chamfers meets on the plane bisecting its corner:
/// a mitre at the convex corners, a valley at the inside ones. Exact volume: the section
/// (2 mm²) along the outline offset d/3 into the material (2d/3 shorter at a convex corner,
/// longer at an inside one); and no faces but the 14 the block had and the 12 chamfers.
#[test]
fn chamfer_inside_corners() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/chamfer_inside_corners.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges = [2, 6, 9, 12, 15, 18, 21, 23, 26, 30, 33, 35].map(EdgeId);
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((removed - 855.168388).abs() < 1e-4, "{removed}");
    assert_eq!(k.faces(out).unwrap().len(), 14 + 12);
}

/// The same block chamfered on its notch wall all round (its top and bottom edges cut, the
/// vertical inside corner filled), a top edge and two hole edges. Where the fill meets a cut,
/// the cut's plane runs on over the fill's end; a chamfer ending against an unchamfered wall
/// ends flush, its end part of that wall. No faces but the block's 14 and the 7 chamfers.
#[test]
fn chamfer_mixed_corners() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/chamfer_mixed_corners.body")).unwrap());
    let edges = [26, 35, 8, 10, 11, 12, 9].map(EdgeId);
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    assert_eq!(k.faces(out).unwrap().len(), 14 + 7);
    for (one, faces) in [
        (&[11][..], 15),
        (&[12, 11][..], 16),
        (&[10, 11, 12][..], 17),
        (&[26][..], 15),
    ] {
        let ids: Vec<EdgeId> = one.iter().map(|&i| EdgeId(i)).collect();
        let r = k
            .chamfer_with(b, &ids, &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)))
            .unwrap();
        assert_eq!(k.faces(r.bodies[0]).unwrap().len(), faces, "{one:?}");
    }
}

/// A wheel's five spoke windows chamfered 2 mm on both sides (a user's document): each side
/// face is split in two on its plane by a circle the windows' chamfers cross, and the edge list
/// holds an edge between two tangent cylinders. The tangent edge is passed over (as OCCT does),
/// the rails run on across the split onto the plane's other face, and where a window's outer
/// arc lies wholly past the split its strip starts on that face (the crossings of the split
/// circle are found however long it is).
#[test]
fn wheel_window_chamfer() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/wheel_window_chamfer.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges = [
        1, 2, 28, 29, 30, 31, 32, 33, 67, 68, 69, 70, 71, 72, 101, 102, 34, 35, 36, 37, 38, 39, 73,
        74, 75, 76, 77, 78, 107, 108, 10, 11, 12, 13, 14, 15, 49, 50, 51, 52, 53, 54, 84, 85, 16,
        17, 18, 19, 20, 21, 55, 56, 57, 58, 59, 60, 92, 93, 22, 23, 24, 25, 26, 27, 61, 62, 63, 64,
        65, 66, 98, 99,
    ]
    .map(EdgeId);
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(2.0)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    // Ten windows of about 71 mm round, a 2 mm² section.
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((1350.0..1500.0).contains(&removed), "{removed}");
}

/// A pipe snap's edges chamfered 0.5 mm (a user's document), one of them where a cross hole
/// meets the bore: the faces' angle varies along it, so its chamfer is a law of radii (the
/// boolean tool's result wasn't valid).
#[test]
fn pipe_snap_chamfer() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/pipe_snap_chamfer.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges = [
        4, 45, 46, 26, 2, 7, 10, 13, 16, 19, 32, 35, 38, 41, 43, 22, 25, 29, 0, 5, 8, 11, 14, 17,
        30, 33, 36, 39, 42, 20, 23, 27, 24, 28, 31, 15, 18, 21,
    ]
    .map(EdgeId);
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(0.5)),
        )
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    // OCCT takes 62.45 mm³ off.
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((60.0..65.0).contains(&removed), "{removed}");
}

/// The rims of a cross hole through a D-shaped block filleted 5 mm (a user's document): the
/// hole's axis lies where the block's half cylinder meets its flat sides, so the ball rolls
/// from the cylinder onto the planes. A crossing on the cylinder's far side (its surface goes
/// on round past the face) isn't a roll, nor one whose spine parameter isn't a number (the
/// ball rolled back and forth for ever).
#[test]
fn cross_hole_fillet() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/cross_hole_fillet.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let spec = FilletSpec {
        allow_overflow: true,
        propagate: true,
        ..FilletSpec::radius(5.0)
    };
    let r = k
        .fillet_with(b, &[11, 12, 2, 6].map(EdgeId), &spec)
        .unwrap();
    let out = r.bodies[0];
    assert!(k.get(out).unwrap().check().is_empty());
    // OCCT takes 1986 mm³ off.
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    assert!((1880.0..2040.0).contains(&removed), "{removed}");
}

/// The same document's Chamfer 1 round the opening (its rim edges, by tangent propagation),
/// on the body Fillet 2 left, chamfered 0.5 to 2 by the local chamfer alone: valid, and each
/// taking off about w²/2 a mm of rim (the narrowest's times (w/w₀)², a little more where corners
/// grow faster). `want` pins what it takes off now (not an independent answer: a change that
/// moves it should explain why); w²/2 of rim at 0.5 puts the rim at 312 mm at fillet 1 and 293
/// at fillet 4, two loops round the opening less its rounds' corners.
fn shell_opening_chamfer(data: &[u8], edges: &[u64], want: [f64; 4]) {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(data).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges: Vec<EdgeId> = edges.iter().map(|&e| EdgeId(e)).collect();
    for (w, want) in [0.5, 1.0, 1.5, 2.0].into_iter().zip(want) {
        let out = blend::chamfer_local(&mut k, b, &edges, w)
            .unwrap_or_else(|e| panic!("w {w}: {e}"))
            .bodies[0];
        let issues = k.get(out).unwrap().check();
        assert!(issues.is_empty(), "w {w}: {issues:?}");
        let removed = v0 - k.mass_properties(out).unwrap().volume;
        assert!(
            (removed - want).abs() < 1e-6 * want,
            "w {w}: takes off {removed}, want {want}"
        );
    }
    // The ratios to the narrowest, against (w/w₀)².
    for (i, w) in [1.0, 1.5, 2.0].into_iter().enumerate() {
        let ratio = want[i + 1] / (want[0] * (w / 0.5f64).powi(2));
        assert!((0.99..1.05).contains(&ratio), "w {w}: {ratio}");
    }
}

/// At fillet 1 mm the chamfer runs round and down rounds as narrow as itself or narrower: at 1
/// the corners' cones have their apexes on the top (a rail of no length); past it they fan (the
/// neighbours' rails meeting in a point short of the corner, a cone from there to its other
/// rail); rolls onto rounds and back the strips trace.
#[test]
fn shell_opening_chamfer_fillet1() {
    shell_opening_chamfer(
        include_bytes!("data/docs/shell_opening_fillet1.body"),
        &[
            0, 10, 18, 19, 34, 47, 48, 55, 64, 68, 75, 129, 131, 133, 135, 137, 2, 8, 16, 21, 49,
            60, 70, 71, 78, 100, 111, 128, 130, 132, 134, 136,
        ],
        [
            39.00744516247505,
            156.56053346663975,
            356.892814267776,
            648.179111849131,
        ],
    );
}

/// At fillet 4 mm the chain ends where the rim runs on, past a 7° kink, into an edge between
/// the same faces: square there, its cut crossing the round the strip rolled onto near its end;
/// at 2 a roll by a corner of four tangent faces is traced onto the face it lands on.
#[test]
fn shell_opening_chamfer_fillet4() {
    shell_opening_chamfer(
        include_bytes!("data/docs/shell_opening_fillet4.body"),
        &[
            0, 10, 18, 19, 34, 47, 48, 55, 64, 68, 75, 128, 131, 135, 137, 139, 2, 8, 16, 21, 49,
            60, 70, 71, 99, 110, 127, 129, 134, 136, 138,
        ],
        [
            36.63395889811363,
            147.10515071999544,
            334.9092740553824,
            607.2381007844815,
        ],
    );
}

/// Tangent propagation over the same opening (rounded all over: most of its edges are smooth,
/// their faces tangent): a chain from a sharp edge runs on along sharp edges only. A smooth
/// edge has nothing to blend; a chamfer's chain ran on from the opening's rim along every
/// smooth edge of the rounded body.
#[test]
fn tangent_chains_skip_smooth_edges() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/shell_opening_rounds.body")).unwrap());
    let edges = k.edges(b).unwrap();
    let smooth: Vec<EdgeId> = edges.iter().filter(|e| e.smooth).map(|e| e.id).collect();
    assert!(smooth.len() > 50, "{} smooth edges", smooth.len());
    for e in edges.iter().filter(|e| !e.smooth) {
        let chain = k.tangent_chain(b, e.id).unwrap();
        assert!(
            chain.iter().all(|x| !smooth.contains(x)),
            "{:?}'s chain {chain:?} runs onto a smooth edge",
            e.id
        );
    }
    // One rim's chain: it and its tangent neighbour.
    assert_eq!(
        k.tangent_chain(b, EdgeId(17)).unwrap(),
        [EdgeId(17), EdgeId(18)]
    );
}

/// The rims of an outward shell's opening filleted (a user's document: a 30 mm cube with every
/// edge rounded 5, shelled 6.5 outward with its back, two top rounds and their corner taken
/// away), from 0.5 to 4.5 mm. Five of the rounds run into the inside corners of the opening and
/// end at a pole of a corner sphere, where the faces past their rails (the top, the sphere)
/// only touch: each end is capped on the surface of the round the shell took away there, the
/// local fillet covering them all (the blend tool's flat ends stood out of the outer wall; a
/// cut traced on one of those faces reached the other rail's exit only roughly). From 3.54 the
/// two either side of the pie-shaped face weld across it.
#[test]
fn shell_opening_rounds() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/shell_opening_rounds.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let mut last = 0.0;
    for r in [0.5, 1.0, 1.5, 2.0, 2.8, 3.25, 4.0, 4.5] {
        let laws: Vec<FilletLaw> = [3, 1, 10, 16, 19, 49, 43]
            .map(|e| FilletLaw {
                edge: EdgeId(e),
                radii: vec![(0.0, r), (1.0, r)],
            })
            .to_vec();
        let out = blend::fillet_laws_local(&mut k, b, &laws)
            .unwrap_or_else(|e| panic!("r {r}: {e}"))
            .bodies[0];
        assert!(k.get(out).unwrap().check().is_empty(), "r {r}");
        // Five inside rounds add (1 − π/4)·r² a mm each, two outside ones take as much off:
        // (1 − π/4)·r²·L, L the net length, 19.5 mm or so (a little more the larger, by the
        // ends); and more the larger.
        let added = k.mass_properties(out).unwrap().volume - v0;
        assert!(added > last, "r {r}: adds {added}, no more than {last}");
        let l = added / ((1.0 - std::f64::consts::FRAC_PI_4) * r * r);
        assert!(
            (19.45..19.75).contains(&l),
            "r {r}: adds {added}, a net length {l}"
        );
        last = added;
        if r == 3.25 {
            assert!((44.3..44.5).contains(&added), "{added}");
        }
    }
}

/// The same opening's two inside rounds either side of the pie-shaped face where the shell
/// stopped taking a round away (a quarter annulus, 5 to 11.5 from its centre, its walls square
/// to each other and to it): past about 3.54 their rails would cross on it, so they weld,
/// meeting along its middle where the ball touches all three faces (its radius the distance
/// from the pie's centre: a cone), each round its full radius again past where they part.
/// Past 11.5 the radius is nowhere: too large, as a fillet past the full round is.
#[test]
fn shell_opening_rounds_weld_across_the_pie() {
    let mut k = NativeKernel::new();
    let b = k.insert(bodyio::read(include_bytes!("data/docs/shell_opening_rounds.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let fillet = |k: &mut NativeKernel, r: f64| {
        let laws: Vec<FilletLaw> = [10, 16]
            .map(|e| FilletLaw {
                edge: EdgeId(e),
                radii: vec![(0.0, r), (1.0, r)],
            })
            .to_vec();
        blend::fillet_laws_local(k, b, &laws).map(|r| r.bodies[0])
    };
    let added = |k: &mut NativeKernel, r: f64| {
        let out = fillet(k, r).unwrap();
        assert!(k.get(out).unwrap().check().is_empty(), "r {r}");
        k.mass_properties(out).unwrap().volume - v0
    };
    // Apart: two exact rounds.
    let apart = added(&mut k, 3.25);
    assert!((29.6..29.8).contains(&apart), "{apart}");
    // Welded near the pie's inner arc, parting on it; nearly all along.
    let some = added(&mut k, 6.0);
    assert!((97.5..97.8).contains(&some), "{some}");
    let most = added(&mut k, 11.0);
    assert!((179.7..180.0).contains(&most), "{most}");
    assert!(fillet(&mut k, 12.0).is_err());
}

/// A degenerate edge among the picked ones (OCCT's `explode` lists a cone's apex; blend
/// encoderegularity_a5 picks two pole edges) is passed over, as OCCT's blend does.
#[test]
fn degenerate_picked_edge_is_passed_over() {
    let mut k = NativeKernel::new();
    let s = k.pcone_a(None, 50.0, 0.0, 120.0, 90.0).unwrap();
    let es = k.explode_edges(&s).unwrap();
    let (draw::Shape::Body(b), draw::Shape::BodyEdge(_, apex), draw::Shape::BodyEdge(_, arc)) =
        (&s, &es[0], &es[2])
    else {
        panic!("not a body's edges")
    };
    assert!(k.get(*b).unwrap().edges()[apex.0 as usize].degenerate);
    let area = |k: &mut NativeKernel, edges: &[EdgeId]| {
        let r = k.fillet(*b, edges, 10.0).unwrap();
        k.mass_properties(r.bodies[0]).unwrap().surface_area
    };
    let both = area(&mut k, &[*apex, *arc]);
    let one = area(&mut k, &[*arc]);
    assert!((both - one).abs() < 1e-9 * one, "{both} {one}");
    assert!(k.fillet(*b, &[*apex], 10.0).is_err());
}

/// Dr Browns cap, Fillet 1: R4 rounds along a window cut into a dome (a spindle torus's band)
/// across the dome's seam, whose input came with the window's loop crossing the seam while the
/// seam edge, not split there, ran on through the window. The local fillet couldn't place its
/// rails in that face (and the blend tool's boolean left an open shell); the hole is opened
/// into the outer loop first. Built here: the dome less a bar across its seam, the window's
/// seam pieces then joined back into one edge running through it, as the document had them.
#[test]
fn fillet_round_a_window_across_a_seam() {
    use nalgebra::{Point2, Point3, Vector3};
    use std::f64::consts::TAU;
    let mut k = NativeKernel::new();
    let (r, cx, cz) = (80.0_f64, -56.0_f64, 1.0_f64);
    let top = (-cx / r).acos();
    let (x1, z1) = (cx + r * top.cos(), cz + r * top.sin());
    let curves = vec![
        Curve2::Line {
            a: Point2::new(0.0, cz),
            b: Point2::new(cx + r, cz),
            source: None,
        },
        Curve2::Arc {
            center: Point2::new(cx, cz),
            radius: r,
            start_angle: 0.0,
            sweep: top,
            source: None,
        },
        Curve2::Line {
            a: Point2::new(x1, z1),
            b: Point2::new(0.0, cz),
            source: None,
        },
    ];
    let xz = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let profile = Profile::new(
        xz,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    let dome = k
        .revolve(
            &profile,
            Axis {
                origin: Point3::origin(),
                dir: Vector3::z_axis(),
            },
            TAU,
        )
        .unwrap()
        .bodies[0];
    let side = Plane {
        origin: Point3::new(0.0, 40.0, 0.0),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let c = Curve2::Circle {
        center: Point2::new(24.68, 31.92),
        radius: 11.25,
        source: None,
    };
    let bar = k
        .extrude(
            &Profile::new(
                side,
                vec![Region {
                    outer: Loop { curves: vec![c] },
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(80.0),
        )
        .unwrap()
        .bodies[0];
    let cut = k.boolean(BoolOp::Subtract, dome, &[bar]).unwrap().bodies[0];
    // The torus face: its loop runs up the seam, round one half of the window, up the seam
    // again, … and back down. Join the seam's pieces and give the window a loop of its own.
    let mut b = (**k.get(cut).unwrap()).clone();
    let fi = b
        .faces
        .iter()
        .position(|f| matches!(f.surf, surface::Surface::Torus { .. }))
        .unwrap();
    let l = b.faces[fi].loops[0].clone();
    let seam = |e: usize| l.iter().filter(|c| c.edge == e).count() == 2;
    let n = l.len();
    // The two window halves: the coedges between two different seam pieces.
    let halves: Vec<usize> = (0..n)
        .filter(|&i| {
            let (p, q) = (l[(i + n - 1) % n].edge, l[(i + 1) % n].edge);
            !seam(l[i].edge) && seam(p) && seam(q) && p != q
        })
        .collect();
    let halves: [usize; 2] = halves
        .clone()
        .try_into()
        .unwrap_or_else(|_| panic!("{halves:?}"));
    assert_eq!(
        halves.len(),
        2,
        "{:?}",
        l.iter()
            .map(|c| (c.edge, c.fwd, seam(c.edge)))
            .collect::<Vec<_>>()
    );
    let (below, above) = (l[(halves[0] + n - 1) % n].edge, l[(halves[0] + 1) % n].edge);
    let (lo, hi) = if b.edges[below].t[0] < b.edges[above].t[0] {
        (below, above)
    } else {
        (above, below)
    };
    let joined = topo::Edge {
        t: [b.edges[lo].t[0], b.edges[hi].t[1]],
        v: [b.edges[lo].v[0], b.edges[hi].v[1]],
        ..b.edges[lo].clone()
    };
    b.edges[lo] = joined;
    let mut outer = Vec::new();
    let mut hole = Vec::new();
    for (i, c) in l.iter().enumerate() {
        if halves.contains(&i) {
            hole.push(c.clone());
        } else if c.edge == hi {
            continue;
        } else {
            outer.push(c.clone());
        }
    }
    // The hole's second half in the first's period (its p-curves running on through the seam).
    let du = hole[0].pc.point(b.edges[hole[0].edge].t[0]).x
        - hole[1].pc.point(b.edges[hole[1].edge].t[1]).x;
    let du = (du / TAU).round() * TAU;
    hole[1].pc = hole[1]
        .pc
        .transformed(&math::Affine::translation(math::V3::new(du, 0.0, 0.0)));
    b.faces[fi].loops = vec![outer, hole];
    // The seam's upper piece, unused now, as a point at the apex (so edge indices stay).
    b.edges[hi].degenerate = true;
    let id = k.insert(b);
    let window = halves.map(|i| EdgeId(l[i].edge as u64));
    let v0 = k.mass_properties(id).unwrap().volume;
    let spec = FilletSpec {
        size: FilletSize::Radius(4.0),
        allow_overflow: true,
        profile: FilletProfile::Circular,
        propagate: true,
    };
    let out = k.fillet_with(id, &window, &spec).unwrap().bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    // A convex round removes a little material all round the window.
    let v = k.mass_properties(out).unwrap().volume;
    assert!(v < v0 && v > v0 - 200.0, "{v0} -> {v}");
}

/// A cabinet side (garbage_cabinet, Chamfer 1): an L-shaped plate 20 deep whose front 5 mm
/// is cut on a diagonal, chamfered 5 mm along both inside edges of the L's step (fills) and
/// four outside edges at the diagonal's ends (cuts). Where the fills' tools run on past their
/// corners and the cuts nearly meet, the tools left islands of 0.02 to 0.04 mm³ as separate
/// solids (seven parts where there are three); a blend never splits a solid, so the result is
/// one solid, as the input was.
#[test]
fn chamfer_leaves_no_islands() {
    let mut k = NativeKernel::new();
    let p = |x: f64, z: f64| nalgebra::Point2::new(x, z);
    let polygon = |pts: &[nalgebra::Point2<f64>]| {
        let n = pts.len();
        Loop {
            curves: (0..n)
                .map(|i| Curve2::Line {
                    a: pts[i],
                    b: pts[(i + 1) % n],
                    source: None,
                })
                .collect(),
        }
    };
    // Sketched on the XZ plane (in-plane y along +Z), extruded along −Y.
    let at = |y: f64| Plane {
        origin: nalgebra::Point3::new(0.0, y, 0.0),
        x_dir: nalgebra::Vector3::x_axis(),
        normal: -nalgebra::Vector3::y_axis(),
    };
    let prism = |k: &mut NativeKernel, y: f64, depth: f64, pts: &[nalgebra::Point2<f64>]| {
        let region = Region {
            outer: polygon(pts),
            holes: vec![],
            source: None,
        };
        k.extrude(&Profile::new(at(y), vec![region]), Extent::Blind(depth))
            .unwrap()
            .bodies[0]
    };
    let front = prism(
        &mut k,
        0.0,
        5.0,
        &[
            p(0.0, 0.0),
            p(5.0, 0.0),
            p(55.0, 50.0),
            p(55.0, 55.0),
            p(0.0, 55.0),
        ],
    );
    let back = prism(
        &mut k,
        -5.0,
        15.0,
        &[
            p(0.0, 0.0),
            p(5.0, 0.0),
            p(5.0, 50.0),
            p(55.0, 50.0),
            p(55.0, 55.0),
            p(0.0, 55.0),
        ],
    );
    let b = k.boolean(BoolOp::Union, front, &[back]).unwrap().bodies[0];
    let v0 = k.mass_properties(b).unwrap().volume;
    // The edges by their midpoints: the step's two inside edges, the outside edges at x = 5,
    // z = 0 and x = 55, z = 50 either side of the step.
    let mids = [
        (5.0, -5.0, 25.0),
        (30.0, -5.0, 50.0),
        (5.0, -12.5, 0.0),
        (5.0, -2.5, 0.0),
        (5.0, -12.5, 50.0),
        (55.0, -12.5, 50.0),
        (55.0, -2.5, 50.0),
    ];
    let infos = k.edges(b).unwrap();
    let edges: Vec<EdgeId> = mids
        .iter()
        .map(|&(x, y, z)| {
            let m = nalgebra::Point3::new(x, y, z);
            infos
                .iter()
                .find(|e| (nalgebra::center(&e.start, &e.end) - m).norm() < 1e-9)
                .expect("the edge")
                .id
        })
        .collect();
    let r = k
        .chamfer_with(
            b,
            &edges,
            &ChamferOpts::new(ChamferSpec::EqualDistance(5.0)),
        )
        .unwrap();
    let out = k.get(r.bodies[0]).unwrap();
    assert!(out.check().is_empty(), "{:?}", out.check());
    assert_eq!(out.solids.len(), 1);
    // The fills (two inside edges, 50 mm each, 12.5 mm² of section) outweigh the cuts.
    let v = k.mass_properties(r.bodies[0]).unwrap().volume;
    assert!(v > v0, "{v0} -> {v}");
}

/// Edges of body `b` picked by `pick` filleted 5 mm by local trimming alone (no tool and
/// boolean): the result checked, and its volume and area against OCCT's.
fn local_fillet_matches(
    k: &mut NativeKernel,
    b: BodyId,
    pick: &dyn Fn(&[EdgeInfo]) -> Vec<EdgeId>,
    volume: f64,
    area: f64,
) {
    let edges = pick(&k.edges(b).unwrap());
    let laws: Vec<FilletLaw> = edges
        .iter()
        .map(|&edge| FilletLaw {
            edge,
            radii: vec![(0.0, 5.0), (1.0, 5.0)],
        })
        .collect();
    let r = blend::fillet_laws_local(k, b, &laws).unwrap();
    let out = r.bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    let m = k.mass_properties(out).unwrap();
    assert!(
        (m.volume - volume).abs() < 1e-4 * volume,
        "volume {} (OCCT {volume})",
        m.volume
    );
    assert!(
        (m.surface_area - area).abs() < 1e-4 * area,
        "area {} (OCCT {area})",
        m.surface_area
    );
}

/// A convex fillet round one of two fused cylinders' top meeting the concave one up the valley
/// between them, the top's edge round the other cylinder left sharp (OCCT blend simple P2:
/// cylinders of radius 50 and height 150, their axes 70 apart): the strips stop where their
/// rails cross on the cylinder they share; the convex one at its section there, the concave
/// one running on to the top's edge round the other cylinder; and a three-sided face fills the
/// corner, its side on the top running out over the valley the concave fillet fills (the top
/// grows there).
#[test]
fn fillet_convex_meets_concave() {
    let mut k = NativeKernel::new();
    let b = two_cylinders(&mut k);
    let pick = |es: &[EdgeInfo]| -> Vec<EdgeId> {
        // The valley's edge at y < 50, and the first cylinder's top arc.
        let valley = es
            .iter()
            .find(|e| e.curve == CurveKind::Line && e.mid.y < 50.0)
            .expect("the valley");
        let top = es
            .iter()
            .find(|e| e.mid.z > 149.0 && e.circle.is_some_and(|c| (c.center.x - 50.0).abs() < 1e-6))
            .expect("the top arc");
        vec![valley.id, top.id]
    };
    local_fillet_matches(&mut k, b, &pick, 2133929.39, 98102.78);
}

/// The reverse: a short cylinder standing out of a tall one, filleted round its top (convex)
/// and where its top meets the tall cylinder (concave), and up one valley between them (OCCT
/// blend simple P7: radius 50, heights 200 and 120, the short one's base at 30 and the axes 50
/// apart). At the valley's top all three meet (a patched corner); at the other end of the
/// arcs the valley is left sharp: the concave fillet stops at its section through the rails'
/// crossing on the short cylinder's top, the convex one runs on down to the valley.
#[test]
fn fillet_concave_meets_convex() {
    let mut k = NativeKernel::new();
    let c1 = k.pcylinder(50.0, 200.0).unwrap();
    let c1 = k.ttranslate(&c1, 50.0, 50.0, 0.0).unwrap();
    let c2 = k.pcylinder(50.0, 120.0).unwrap();
    let c2 = k.ttranslate(&c2, 100.0, 50.0, 30.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&c1, &c2).unwrap() else {
        panic!("not a body")
    };
    let pick = |es: &[EdgeInfo]| -> Vec<EdgeId> {
        let mut out: Vec<EdgeId> = es
            .iter()
            .filter(|e| (e.mid.z - 150.0).abs() < 1e-6 && e.circle.is_some())
            .map(|e| e.id)
            .collect();
        out.push(
            es.iter()
                .find(|e| e.curve == CurveKind::Line && e.mid.y < 50.0 && e.mid.z > 30.0)
                .expect("the valley")
                .id,
        );
        out
    };
    local_fillet_matches(&mut k, b, &pick, 2144319.08, 99976.90);
}

/// Edges of body `b` picked by `pick` filleted along the radius law `radii` by local trimming
/// alone: the result checked; its volume and area.
fn local_fillet_law(
    k: &mut NativeKernel,
    b: BodyId,
    pick: &dyn Fn(&[EdgeInfo]) -> Vec<EdgeId>,
    radii: &[(f64, f64)],
) -> (f64, f64) {
    let edges = pick(&k.edges(b).unwrap());
    let laws: Vec<FilletLaw> = edges
        .iter()
        .map(|&edge| FilletLaw {
            edge,
            radii: radii.to_vec(),
        })
        .collect();
    let r = blend::fillet_laws_local(k, b, &laws).unwrap();
    let out = r.bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    let m = k.mass_properties(out).unwrap();
    (m.volume, m.surface_area)
}

/// The circle at the foot of a cylinder standing on a plate, picked by its radius and height.
fn foot_circle(es: &[EdgeInfo], r: f64, z: f64) -> Vec<EdgeId> {
    vec![
        es.iter()
            .find(|e| {
                e.circle.is_some_and(|c| (c.radius - r).abs() < 1e-6) && (e.mid.z - z).abs() < 1e-6
            })
            .expect("the foot circle")
            .id,
    ]
}

/// A 20 mm fillet round a boss (radius 15) near a plate's corner: its rail on the plate runs
/// off over one side, round the corner and back over the other side, and is off the plate where
/// the boss's seam is, so the band's seam is elsewhere and its rail on the boss crosses the
/// boss's seam on the way round (OCCT blend complex D6's configuration). The plate's sides
/// grow to the fillet's cut by them. Against the same solid built by OCCT's booleans (OCCT's
/// own fillet of it leaves the plate's top untrimmed): the plate and boss fused with the ring
/// the fillet fills (a cylinder of radius 35 less the torus the ball sweeps and the boss) cut
/// to the plate's outline.
#[test]
fn closed_fillet_hangs_round_a_plate_corner() {
    let mut k = NativeKernel::new();
    let p = k.box_(100.0, 100.0, 20.0).unwrap();
    let c = k.pcylinder(15.0, 60.0).unwrap();
    let c = k.ttranslate(&c, 80.0, 80.0, 0.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&p, &c).unwrap() else {
        panic!("not a body")
    };
    let (v, a) = local_fillet_law(
        &mut k,
        b,
        &|es: &[EdgeInfo]| foot_circle(es, 15.0, 20.0),
        &[(0.0, 20.0), (1.0, 20.0)],
    );
    let (volume, area) = (237236.757534805, 31401.465564791);
    assert!(
        (v - volume).abs() < 1e-5 * volume,
        "volume {v} (OCCT {volume})"
    );
    assert!((a - area).abs() < 1e-5 * area, "area {a} (OCCT {area})");
}

/// A fillet whose radius varies round a boss (radius 8) on a 20 mm cube, from 1 mm at the
/// boss's seam to 4 mm opposite (OCCT blend buildevol H9's model and law, the law closed
/// periodically as OCCT closes it): its rail on the cube's top runs off over each of the four
/// sides and back, and the search along the sides seeded with one radius misses some of those
/// crossings. OCCT's area (its law interpolates the radii by a periodic cubic, not linearly).
#[test]
fn variable_closed_fillet_runs_off_a_plate() {
    let mut k = NativeKernel::new();
    let p = k.box_at(-10.0, -10.0, -10.0, 20.0, 20.0, 20.0).unwrap();
    let c = k.pcylinder(8.0, 25.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&p, &c).unwrap() else {
        panic!("not a body")
    };
    let (_, a) = local_fillet_law(
        &mut k,
        b,
        &|es: &[EdgeInfo]| foot_circle(es, 8.0, 10.0),
        &[(0.0, 1.0), (0.5, 4.0), (1.0, 3.0)],
    );
    let area = 3105.92;
    assert!((a - area).abs() < 1e-3 * area, "area {a} (OCCT {area})");
}

/// A square pyramid (base 250 × 250 at y = 0, apex at (125, 175, 125)) with one side split in
/// two along a line from the apex to the middle of its base edge, the halves on one plane (OCCT
/// blend tolblend_simple F6, CFI_13_c1ffk): its faces sewn from planar polygons.
fn split_pyramid(k: &mut NativeKernel) -> BodyId {
    let (a, b0, b1, b2, b3, m) = (
        [125.0, 175.0, 125.0],
        [0.0, 0.0, 0.0],
        [250.0, 0.0, 0.0],
        [250.0, 0.0, 250.0],
        [0.0, 0.0, 250.0],
        [0.0, 0.0, 125.0],
    );
    let polys: [&[[f64; 3]]; 6] = [
        &[b0, b1, b2, b3],
        &[b0, a, b1],
        &[b1, a, b2],
        &[b2, a, b3],
        &[b3, a, m],
        &[m, a, b0],
    ];
    let faces: Vec<draw::Shape> = polys
        .iter()
        .map(|p| {
            let mut closed = p.to_vec();
            closed.push(p[0]);
            let w = k.polyline(&closed).unwrap();
            k.mkplane(&w).unwrap()
        })
        .collect();
    let refs: Vec<&draw::Shape> = faces.iter().collect();
    let draw::Shape::Body(b) = k.sewing(1e-6, &refs).unwrap() else {
        panic!("not a body")
    };
    b
}

/// Three of the split pyramid's apex edges rounded 25 by local trimming (OCCT blend
/// tolblend_simple F6), the split side's edge among them: the corner is set back, and that
/// edge's ball rolls from one half of the split side onto the other short of the setback. The
/// strip between the vertex and the roll lies in the corner's hole (left out); the one past the
/// roll ends at the setback. The area within 1e-4 of OCCT's, the volume within 0.5 % (the
/// corner patch differs from OCCT's filling).
#[test]
fn fillet_set_back_corner_past_a_roll() {
    let mut k = NativeKernel::new();
    let b = split_pyramid(&mut k);
    let es = k.edges(b).unwrap();
    let at = |m: [f64; 3]| {
        es.iter()
            .find(|e| (e.mid - nalgebra::Point3::from(m)).norm() < 1e-6)
            .expect("an apex edge")
            .id
    };
    let edges = [
        at([62.5, 87.5, 187.5]),
        at([187.5, 87.5, 187.5]),
        at([187.5, 87.5, 62.5]),
    ];
    let laws: Vec<FilletLaw> = edges
        .iter()
        .map(|&edge| FilletLaw {
            edge,
            radii: vec![(0.0, 25.0), (1.0, 25.0)],
        })
        .collect();
    let out = blend::fillet_laws_local(&mut k, b, &laws).unwrap().bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    let m = k.mass_properties(out).unwrap();
    let (volume, area) = (3603190.53, 165927.97);
    assert!(
        (m.volume - volume).abs() < 5e-3 * volume,
        "volume {} (OCCT {volume})",
        m.volume
    );
    assert!(
        (m.surface_area - area).abs() < 1e-4 * area,
        "area {} (OCCT {area})",
        m.surface_area
    );
}

/// All four of the split pyramid's apex edges rounded by local trimming, with radii 20, 25, 30
/// and 20 (OCCT blend tolblend_simple F5): both edges beside the split side roll over its
/// split short of the setback. Area within 0.2 % of OCCT's (the corner patch differs from
/// OCCT's filling).
#[test]
fn fillet_set_back_corner_past_two_rolls() {
    let mut k = NativeKernel::new();
    let b = split_pyramid(&mut k);
    let es = k.edges(b).unwrap();
    let at = |m: [f64; 3]| {
        es.iter()
            .find(|e| (e.mid - nalgebra::Point3::from(m)).norm() < 1e-6)
            .expect("an apex edge")
            .id
    };
    let picked = [
        ([62.5, 87.5, 187.5], 20.0),
        ([187.5, 87.5, 187.5], 25.0),
        ([187.5, 87.5, 62.5], 30.0),
        ([62.5, 87.5, 62.5], 20.0),
    ];
    let laws: Vec<FilletLaw> = picked
        .iter()
        .map(|&(m, r)| FilletLaw {
            edge: at(m),
            radii: vec![(0.0, r), (1.0, r)],
        })
        .collect();
    let out = blend::fillet_laws_local(&mut k, b, &laws).unwrap().bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    let area = 165055.0;
    let a = k.mass_properties(out).unwrap().surface_area;
    assert!((a - area).abs() < 2e-3 * area, "area {a} (OCCT {area})");
}

/// Edge `e` of body `b` filleted with radius `r` by local trimming alone (no tool and boolean),
/// the result checked: its body.
fn local_fillet(k: &mut NativeKernel, b: BodyId, e: EdgeId, r: f64) -> BodyId {
    let laws = vec![FilletLaw {
        edge: e,
        radii: vec![(0.0, r), (1.0, r)],
    }];
    let out = blend::fillet_laws_local(k, b, &laws).unwrap().bodies[0];
    assert!(
        k.get(out).unwrap().check().is_empty(),
        "{:?}",
        k.get(out).unwrap().check()
    );
    out
}

/// The edge of body `b` whose middle is at `p`.
fn edge_at(k: &NativeKernel, b: BodyId, p: [f64; 3]) -> EdgeId {
    let p = nalgebra::Point3::new(p[0], p[1], p[2]);
    k.edges(b)
        .unwrap()
        .iter()
        .find(|e| (e.mid - p).norm() < 1e-6)
        .expect("no edge there")
        .id
}

/// The midpoint rule's integral of `f` over [a, b] in n steps.
fn integral(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize) -> f64 {
    let h = (b - a) / n as f64;
    (0..n).map(|i| f(a + (i as f64 + 0.5) * h)).sum::<f64>() * h
}

/// A Ø92 hole in a 100 mm square block's top rounded R30 (SMLib's cliff_4): the rail on the
/// top (radius 76) lies outside the top face all round, so the top goes whole and the round is
/// cut by the four sides, its cut on each running from one vertical edge to the next (SMLib's
/// cliff roll-over). OCCT fails; checked against the round's exact volume and area (a torus
/// quarter swept round the hole, clipped by the square).
#[test]
fn fillet_rim_off_its_face_all_round() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut k = NativeKernel::new();
    let bx = k.box_(100.0, 100.0, 60.0).unwrap();
    let c = k.pcylinder(46.0, 80.0).unwrap();
    let c = k.ttranslate(&c, 50.0, 50.0, -10.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&bx, &c).unwrap() else {
        panic!("not a body")
    };
    let m0 = k.mass_properties(b).unwrap();
    let e = edge_at(&k, b, [4.0, 50.0, 60.0]);
    let out = local_fillet(&mut k, b, e, 30.0);
    let m = k.mass_properties(out).unwrap();
    assert_eq!(k.faces(out).unwrap().len(), k.faces(b).unwrap().len());
    // The circle of radius ρ about the hole's axis inside the square, the round's depth below
    // the top at ρ (its section a quarter circle about ρ = 76, 30 down), and ρ along that
    // quarter (φ from the top).
    let len = |rho: f64| {
        if rho <= 50.0 {
            2.0 * PI * rho
        } else {
            (2.0 * PI - 8.0 * (50.0 / rho).acos()).max(0.0) * rho
        }
    };
    let depth = |rho: f64| {
        if rho >= 76.0 {
            0.0
        } else {
            30.0 - (900.0 - (76.0 - rho).powi(2)).sqrt()
        }
    };
    let rho = |phi: f64| 76.0 - 30.0 * phi.sin();
    let removed = integral(
        &|phi| depth(rho(phi)) * len(rho(phi)) * 30.0 * phi.cos(),
        0.0,
        FRAC_PI_2,
        200_000,
    );
    let round = integral(&|phi| len(rho(phi)) * 30.0, 0.0, FRAC_PI_2, 200_000);
    let sides = 4.0 * integral(&|y| depth((2500.0 + y * y).sqrt()), -50.0, 50.0, 200_000);
    let area =
        m0.surface_area - (10000.0 - PI * 46.0 * 46.0) - 2.0 * PI * 46.0 * 30.0 - sides + round;
    assert!(
        (m0.volume - m.volume - removed).abs() < 1e-5 * m0.volume,
        "removed {} (exact {removed})",
        m0.volume - m.volume
    );
    assert!(
        (m.surface_area - area).abs() < 1e-5 * area,
        "area {} (exact {area})",
        m.surface_area
    );
}

/// A cylinder's end rounded with its own radius (OCCT blend simple Q4, our edge order): every
/// ball is the same one, the round a half sphere and the end face gone whole. OCCT's volume and
/// area.
#[test]
fn fillet_round_a_whole_end() {
    let mut k = NativeKernel::new();
    let draw::Shape::Body(b) = k.pcylinder(1.0, 10.0).unwrap() else {
        panic!("not a body")
    };
    let e = edge_at(&k, b, [-1.0, 0.0, 10.0]);
    let out = local_fillet(&mut k, b, e, 1.0);
    let m = k.mass_properties(out).unwrap();
    assert!(
        (m.volume - 30.368728985).abs() < 1e-6 * m.volume,
        "volume {}",
        m.volume
    );
    assert!(
        (m.surface_area - 65.973445725).abs() < 1e-6 * m.surface_area,
        "area {}",
        m.surface_area
    );
}

/// A block's top front edge filleted R30 where it ends at a half-round notch of radius 30
/// (OCCT blend simple X4): the strip's end on the notch's wall leaves both rails' exits tangent
/// to the wall's edges, so the cut on the wall turns back along them; the end curve marched
/// densely where it runs along a rail. OCCT's own result takes off twice the material; checked
/// against the exact volume (the round's section, clipped by the notch, along the edge).
#[test]
fn fillet_end_tangent_to_a_notchs_edges() {
    let mut k = NativeKernel::new();
    let bx = k.box_(120.0, 100.0, 50.0).unwrap();
    let bx = k.ttranslate(&bx, -60.0, -50.0, 0.0).unwrap();
    let c = k.pcylinder(30.0, 60.0).unwrap();
    let c = k.ttranslate(&c, 0.0, -50.0, -5.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&bx, &c).unwrap() else {
        panic!("not a body")
    };
    let v0 = k.mass_properties(b).unwrap().volume;
    let e = edge_at(&k, b, [45.0, -50.0, 50.0]);
    let out = local_fillet(&mut k, b, e, 30.0);
    let removed = v0 - k.mass_properties(out).unwrap().volume;
    // At x the solid starts at y0 (the notch) and the round takes 30 − √(900 − (y + 20)²) off
    // the height up to y = −20.
    let section = |x: f64| {
        let y0 = (-50.0 + (900.0 - x * x).max(0.0).sqrt()).max(-50.0);
        if y0 >= -20.0 {
            0.0
        } else {
            integral(
                &|y| 30.0 - (900.0 - (y + 20.0).powi(2)).max(0.0).sqrt(),
                y0,
                -20.0,
                2000,
            )
        }
    };
    let exact = integral(&section, 0.0, 60.0, 3000);
    assert!(
        (removed - exact).abs() < 1e-4 * exact,
        "removed {removed} (exact {exact})"
    );
}

/// The opening oracle (research/PAPER_SCAN.md item 21): every edge of a convex solid filleted
/// with radius r is the solid shrunk by r with its edges kept sharp, then grown by r with them
/// rounded (rolling a ball inside it). A skew pentagonal prism, its vertices meeting three
/// edges at unequal angles: the local fillet's strips and sphere corners against two offsets.
#[test]
fn fillet_all_convex_edges_is_an_opening() {
    use nalgebra::Point2;
    let mut k = NativeKernel::new();
    let pts = [
        (0.0, 0.0),
        (40.0, -4.0),
        (52.0, 22.0),
        (27.0, 41.0),
        (-6.0, 26.0),
    ]
    .map(|(x, y)| Point2::new(x, y));
    let outer = Loop {
        curves: (0..5)
            .map(|i| Curve2::Line {
                a: pts[i],
                b: pts[(i + 1) % 5],
                source: None,
            })
            .collect(),
    };
    let prism = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer,
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(17.0),
        )
        .unwrap()
        .bodies[0];
    let r = 3.0;
    let edges: Vec<EdgeId> = k.edges(prism).unwrap().iter().map(|e| e.id).collect();
    let spec = FilletSpec {
        size: FilletSize::Radius(r),
        allow_overflow: true,
        profile: FilletProfile::Circular,
        propagate: false,
    };
    let filleted = k.fillet_with(prism, &edges, &spec).unwrap().bodies[0];
    assert!(k.get(filleted).unwrap().check().is_empty());
    let shrunk = k
        .offset(
            prism,
            &OffsetSpec {
                distance: -r,
                faces: vec![],
                sharp: true,
            },
        )
        .unwrap()
        .bodies[0];
    let opened = k
        .offset(
            shrunk,
            &OffsetSpec {
                distance: r,
                faces: vec![],
                sharp: false,
            },
        )
        .unwrap()
        .bodies[0];
    let (a, b) = (
        k.mass_properties(filleted).unwrap(),
        k.mass_properties(opened).unwrap(),
    );
    assert!(
        (a.volume - b.volume).abs() < 1e-6 * b.volume,
        "fillet volume {} vs opening {}",
        a.volume,
        b.volume
    );
    assert!(
        (a.surface_area - b.surface_area).abs() < 1e-6 * b.surface_area,
        "fillet area {} vs opening {}",
        a.surface_area,
        b.surface_area
    );
}

/// The edges of body `b` nearest the points `at` (their middles).
fn edges_near(k: &NativeKernel, b: BodyId, at: &[[f64; 3]]) -> Vec<EdgeId> {
    let es = k.edges(b).unwrap();
    at.iter()
        .map(|p| {
            let p = nalgebra::Point3::new(p[0], p[1], p[2]);
            es.iter()
                .min_by(|x, y| (x.mid - p).norm().total_cmp(&(y.mid - p).norm()))
                .expect("no edges")
                .id
        })
        .collect()
}

/// `got` (volume, area) within `rel` of OCCT's.
fn near_occt(got: (f64, f64), volume: f64, area: f64, rel: f64) {
    let (v, a) = got;
    assert!(
        (v - volume).abs() < rel * volume,
        "volume {v} (OCCT {volume})"
    );
    assert!((a - area).abs() < rel * area, "area {a} (OCCT {area})");
}

/// The outline of two circles of radius 50 whose centres are 70 apart, extruded 150 (two fused
/// cylinders, OCCT blend simple P2 and P3's model).
fn two_cylinders(k: &mut NativeKernel) -> BodyId {
    use nalgebra::{Point2, Point3, Vector3};
    use std::f64::consts::PI;
    let a = (35.0_f64 / 50.0).acos();
    let curves = vec![
        Curve2::Arc {
            center: Point2::new(50.0, 50.0),
            radius: 50.0,
            start_angle: a,
            sweep: 2.0 * (PI - a),
            source: None,
        },
        Curve2::Arc {
            center: Point2::new(120.0, 50.0),
            radius: 50.0,
            start_angle: PI + a,
            sweep: 2.0 * (PI - a),
            source: None,
        },
    ];
    let xy = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let profile = Profile::new(
        xy,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    k.extrude(&profile, Extent::Blind(150.0)).unwrap().bodies[0]
}

/// Two fused cylinders rounded 5 mm along both valleys between them and round both tops (OCCT
/// blend simple P3): at each valley's top the two convex fillets meet the concave one in a
/// patched corner. The two tops' fillets cross there at an angle, tangent to the top they
/// share: each one's curve across its strip must leave the rails' crossing on its own side of
/// the mitre between them, or the strips cross.
#[test]
fn fillet_two_tops_meeting_a_valley() {
    let mut k = NativeKernel::new();
    let b = two_cylinders(&mut k);
    let pick = |es: &[EdgeInfo]| -> Vec<EdgeId> {
        es.iter()
            .filter(|e| {
                (e.curve == CurveKind::Line && (e.mid.x - 85.0).abs() < 1e-6)
                    || ((e.mid.z - 150.0).abs() < 1e-6 && e.circle.is_some())
            })
            .map(|e| e.id)
            .collect()
    };
    let got = local_fillet_law(&mut k, b, &pick, &[(0.0, 5.0), (1.0, 5.0)]);
    near_occt(got, 2133270.476235686, 97351.925992673, 1e-4);
}

/// A keyhole boss (circles of radius 22.5 and 10, their centres 25 apart, 32 high) on a disc,
/// its small circle's wall split in two faces at the circle's quadrant point just past one
/// valley, filleted 10 mm along both valleys (OCCT blend complex E8). The ball touches the
/// small circle's wall past the split: the narrow face beside the valley goes whole under the
/// strip, which runs on the face beyond.
#[test]
fn fillet_valley_past_a_split_wall() {
    use nalgebra::{Point2, Point3, Vector3};
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    // The circles cross at (±x, y); round the big one the long way from (−x, y) to (x, y), then
    // round the small one back, split at its quadrant point (10, 25).
    let (x, y) = (8.99218410621135_f64, 20.625_f64);
    let (big, small) = (y.atan2(-x), (y - 25.0).atan2(x));
    let curves = vec![
        Curve2::Arc {
            center: Point2::new(0.0, 0.0),
            radius: 22.5,
            start_angle: big,
            sweep: 2.0 * PI - (2.0 * big - PI),
            source: None,
        },
        Curve2::Arc {
            center: Point2::new(0.0, 25.0),
            radius: 10.0,
            start_angle: small,
            sweep: -small,
            source: None,
        },
        Curve2::Arc {
            center: Point2::new(0.0, 25.0),
            radius: 10.0,
            start_angle: 0.0,
            sweep: PI - small,
            source: None,
        },
    ];
    let top = Plane {
        origin: Point3::new(0.0, 0.0, 8.0),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let profile = Profile::new(
        top,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    let boss = k.extrude(&profile, Extent::Blind(32.0)).unwrap().bodies[0];
    let disc = k.pcylinder(55.0, 8.0).unwrap();
    let boss = draw::Shape::Body(boss);
    let draw::Shape::Body(b) = k.bfuse(&disc, &boss).unwrap() else {
        panic!("not a body")
    };
    let edges = edges_near(&k, b, &[[x, y, 24.0], [-x, y, 24.0]]);
    let pick = |_: &[EdgeInfo]| edges.clone();
    let got = local_fillet_law(&mut k, b, &pick, &[(0.0, 10.0), (1.0, 10.0)]);
    near_occt(got, 134300.125321031, 26887.758736863, 1e-4);
}

/// A block with a wall along one side and a rib 5 high across its top from the wall (OCCT
/// blend complex F3: the rib's cross-section at the wall is lower than the fillet), the
/// valley between the wall and the top filleted 10 mm up to the rib. The rail on the wall
/// runs on above the rib: it stops where the rib's side's plane meets it, past the top of the
/// rib's edge up the wall; the rib's side is cut by the end curve up to the rib's top, and a
/// face in its plane closes the strip's end above the rib.
#[test]
fn fillet_end_against_a_low_rib() {
    use nalgebra::{Point2, Point3, Vector3};
    let mut k = NativeKernel::new();
    let (x0, h, y1, y2) = (
        39.570556640625,
        40.644775390625,
        70.0884127089926,
        133.783823524844,
    );
    // The block and rib's section across y (u) and z (v), extruded along x from the wall.
    let pts = [
        (0.0, 0.0),
        (200.0, 0.0),
        (200.0, h),
        (y2, h),
        (y2, h + 5.0),
        (y1, h + 5.0),
        (y1, h),
        (0.0, h),
    ];
    let curves = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            Curve2::Line {
                a: Point2::new(a.0, a.1),
                b: Point2::new(b.0, b.1),
                source: None,
            }
        })
        .collect();
    let side = Plane {
        origin: Point3::new(x0, 0.0, 0.0),
        x_dir: Vector3::y_axis(),
        normal: Vector3::x_axis(),
    };
    let profile = Profile::new(
        side,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    );
    let block = k
        .extrude(&profile, Extent::Blind(200.0 - x0))
        .unwrap()
        .bodies[0];
    let wall = k.box_at(0.0, 0.0, 0.0, x0, 200.0, 200.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&draw::Shape::Body(block), &wall).unwrap() else {
        panic!("not a body")
    };
    let edges = edges_near(&k, b, &[[x0, 0.5 * y1, h]]);
    let pick = |_: &[EdgeInfo]| edges.clone();
    let got = local_fillet_law(&mut k, b, &pick, &[(0.0, 10.0), (1.0, 10.0)]);
    // As the tool-and-boolean blend builds it; OCCT's differs by 0.05% in volume and 0.2% in
    // area (its test allows 1%).
    near_occt(got, 2941102.404416552, 189822.764688955, 2e-3);
    let (v, a) = got;
    assert!((v - 2939543.2106198).abs() < 1e-6 * v, "volume {v}");
    assert!((a - 190177.27128303).abs() < 1e-6 * a, "area {a}");
}

/// A box's top edge rounded 10 mm and a boss of radius 30 standing on its top, the boss's foot
/// filleted 15 mm (OCCT blend simple W9): the fillet's rail on the top runs over the round's
/// tangent edge, and its ball rolls onto the round and back. The closed edge opens at its
/// vertex, its strips rolling as an open edge's do and meeting there in a section.
#[test]
fn fillet_round_a_boss_rolling_over_a_round() {
    let mut k = NativeKernel::new();
    let bx = k.box_at(-50.0, -50.0, 0.0, 100.0, 100.0, 100.0).unwrap();
    let draw::Shape::Body(bx) = bx else {
        panic!("not a body")
    };
    let edge = edge_at(&k, bx, [50.0, 0.0, 100.0]);
    let rounded = local_fillet(&mut k, bx, edge, 10.0);
    let boss = k.pcylinder(30.0, 30.0).unwrap();
    let boss = k.ttranslate(&boss, 0.0, 0.0, 100.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&draw::Shape::Body(rounded), &boss).unwrap() else {
        panic!("not a body")
    };
    let got = local_fillet_law(
        &mut k,
        b,
        &|es: &[EdgeInfo]| foot_circle(es, 30.0, 100.0),
        &[(0.0, 15.0), (1.0, 15.0)],
    );
    near_occt(got, 1092675.152814246, 64076.597602439, 1e-4);
}

/// A quarter ellipse (150 by 100) over a cone's profile revolved 270° (OCCT blend simple H4),
/// the revolution's end edge along the cone filleted 10 mm. At its top end the rail on the end
/// plane meets the ellipse's edge only 110 mm on, but meets the ellipsoid's surface just below
/// the vertex, on the ellipse run on past it: it stops there, and the strip's end curve on the
/// ellipsoid runs between the two rails' ends there.
#[test]
fn fillet_end_on_a_curved_face_past_its_edge() {
    let mut k = NativeKernel::new();
    let w1 = k.ellipse([0.0, 0.0, 0.0], None, None, 15.0, 10.0);
    let w1 = k.mkedge(&w1, 0.0, std::f64::consts::FRAC_PI_2);
    let w1 = k.trotate(&w1, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 90.0).unwrap();
    let w2 = k
        .polyline(&[
            [0.0, 0.0, 10.0],
            [0.0, 0.0, -10.0],
            [25.0, 0.0, -10.0],
            [15.0, 0.0, 0.0],
        ])
        .unwrap();
    let w = k.wire(&[&w1, &w2]).unwrap();
    let w = k.mkplane(&w).unwrap();
    let s = k.revol(&w, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 270.0).unwrap();
    let draw::Shape::Body(b) = k.tscale(&s, 0.0, 0.0, 0.0, 10.0).unwrap() else {
        panic!("not a body")
    };
    let edges = edges_near(&k, b, &[[0.0, -200.0, -50.0]]);
    let pick = |_: &[EdgeInfo]| edges.clone();
    let (_, a) = local_fillet_law(&mut k, b, &pick, &[(0.0, 10.0), (1.0, 10.0)]);
    // (OCCT's area to its test's six figures.)
    let area = 426548.0;
    assert!((a - area).abs() < 1e-3 * area, "area {a} (OCCT {area})");
}

/// A half cylinder (radius 50, 20 high) under a ruled flare up to a half ellipse (50 by 60,
/// 20 higher): the concave edge between them, the half circle, has its faces turn tangent at
/// its ends (both are the plane x = ±50 there; OCCT blend tolblend_simple B9's
/// configuration). Filleted R7.5, the strip narrows to nothing and ends at each vertex (a
/// pinch) instead of losing its ball where the sections grow ill-conditioned. OCCT fails at
/// this radius (and adds ten times the material at R2); checked against the material the
/// fillet adds, integrated over the sections in the planes through the axis (12.1534 mm³; the
/// ball's contacts on the flare lie a little off those planes).
#[test]
fn fillet_pinches_to_its_vertices() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut k = NativeKernel::new();
    let (r, h) = (50.0, 20.0);
    let c = k.circle(0.0, 0.0, 0.0, r);
    let arc = k.mkedge(&c, 0.0, PI);
    let d0 = k.polyline(&[[-r, 0.0, 0.0], [r, 0.0, 0.0]]).unwrap();
    let w0 = k.wire(&[&arc, &d0]).unwrap();
    let e = k.ellipse(
        [0.0, 0.0, h],
        Some([0.0, 0.0, 1.0]),
        Some([0.0, 1.0, 0.0]),
        r + 10.0,
        r,
    );
    let top = k.mkedge(&e, -FRAC_PI_2, FRAC_PI_2);
    let d1 = k.polyline(&[[-r, 0.0, h], [r, 0.0, h]]).unwrap();
    let w1 = k.wire(&[&top, &d1]).unwrap();
    let flare = k.thrusections(true, true, &[&w0, &w1]).unwrap();
    let wall = k.pcylinder_a(None, r, h, 180.0).unwrap();
    let wall = k.ttranslate(&wall, 0.0, 0.0, -h).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&flare, &wall).unwrap() else {
        panic!("not a body")
    };
    let v0 = k.mass_properties(b).unwrap().volume;
    let e = edge_at(&k, b, [0.0, r, 0.0]);
    let out = local_fillet(&mut k, b, e, 7.5);
    let added = k.mass_properties(out).unwrap().volume - v0;
    assert!((added - 12.1534).abs() < 1e-3 * 12.1534, "added {added}");
}

/// A block's top edge (x = 0, z = 15) filleted R7 over the foot of a boss on its side: a
/// ruled B-spline surface from a circle of radius 10 round the x axis at x = 0 to one at
/// x = 20 shifted 10 in y (OCCT blend buildevol K1's configuration). The fillet's rail on the
/// side runs over the boss's foot, so the boss grows back to the fillet along the
/// continuation of its surface past its range. Exact volume (the fillet's section along the
/// edge, less the part of it inside the boss's continuation); OCCT's area (its volume is
/// 10 mm³ more).
#[test]
fn fillet_over_a_bosss_foot_grows_the_boss() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let bx = k.box_at(-40.0, -20.0, -20.0, 40.0, 40.0, 35.0).unwrap();
    let c0 = k.circle_n(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 10.0);
    let c1 = k.circle_n(20.0, 10.0, 0.0, 1.0, 0.0, 0.0, 10.0);
    let (e0, e1) = (k.mkedge_full(&c0), k.mkedge_full(&c1));
    let (w0, w1) = (k.wire(&[&e0]).unwrap(), k.wire(&[&e1]).unwrap());
    let boss = k.thrusections(true, true, &[&w0, &w1]).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&bx, &boss).unwrap() else {
        panic!("not a body")
    };
    let v0 = k.mass_properties(b).unwrap().volume;
    let e = edge_at(&k, b, [0.0, 0.0, 15.0]);
    let out = local_fillet(&mut k, b, e, 7.0);
    let m = k.mass_properties(out).unwrap();
    // The fillet's section: the corner x ∈ [−7, 0], z ∈ [8, 15] outside the circle of radius
    // 7 about (−7, 8); the boss's continuation keeps its chord 2√(100 − z²) of it below z = 10.
    let kept = integral(
        &|x| {
            let z0 = 8.0 + (49.0 - (x + 7.0).powi(2)).max(0.0).sqrt();
            if z0 >= 10.0 {
                0.0
            } else {
                integral(&|z| 2.0 * (100.0 - z * z).sqrt(), z0, 10.0, 400)
            }
        },
        -7.0,
        0.0,
        4000,
    );
    let volume = v0 - 40.0 * 49.0 * (1.0 - PI / 4.0) + kept;
    assert!(
        (m.volume - volume).abs() < 1e-8 * volume,
        "volume {} (exact {volume})",
        m.volume
    );
    let area = 9992.454253841;
    assert!(
        (m.surface_area - area).abs() < 1e-6 * area,
        "area {} (OCCT {area})",
        m.surface_area
    );
}

/// A 5 mm cube's top edge at x = 0 rounded R1 and a boss (radius 1.118) standing on its top in
/// the middle, the boss's foot filleted R1 (SMLib's cyl_on_box_w_fillet, test 407): the rail
/// on the top reaches x = 0.382, over the round's tangent edge (x = 1), so the closed edge
/// opens at its vertex and the ball rolls onto the round and back. The edge's middle, where the
/// reference ball starts, is on the round's side: the reference moves to a station on the top
/// first (from there the ball rolls off and back on). Against OCCT (SMLib: 135.461936,
/// 162.494585).
#[test]
fn fillet_round_a_boss_whose_middle_is_on_a_round() {
    let mut k = NativeKernel::new();
    let bx = k.box_(5.0, 5.0, 5.0).unwrap();
    let draw::Shape::Body(bx) = bx else {
        panic!("not a body")
    };
    let edge = edge_at(&k, bx, [0.0, 2.5, 5.0]);
    let rounded = local_fillet(&mut k, bx, edge, 1.0);
    let r = 1.118033988749895;
    let boss = k.pcylinder(r, 2.5).unwrap();
    let boss = k.ttranslate(&boss, 2.5, 2.5, 5.0).unwrap();
    let draw::Shape::Body(b) = k.bfuse(&draw::Shape::Body(rounded), &boss).unwrap() else {
        panic!("not a body")
    };
    let got = local_fillet_law(
        &mut k,
        b,
        &|es: &[EdgeInfo]| foot_circle(es, r, 5.0),
        &[(0.0, 1.0), (1.0, 1.0)],
    );
    near_occt(got, 135.461681312, 162.501577499, 1e-5);
}

/// A block (223 by 129) with a blind hole (radius 23.9) whose rim is rounded R15, and a boss
/// (radius 18.9) on its top near a corner and the hole, its foot filleted R10 (SMLib's
/// cyl_on_box_w_tan_hole, test 417): the rail on the top rolls onto the hole's round and back
/// (the closed edge opens at its vertex) and runs off over the block's two sides by the
/// corner. Each strip's cliffs are looked for between its own ends: the strip from the second
/// roll on to the vertex ends a turn on from where the edge starts. OCCT fails; SMLib's result
/// isn't a solid. Checked against the tool-and-boolean blend (0.01% in volume, 0.3% in area:
/// its sections over the round are coarser) and pinned.
#[test]
fn fillet_round_a_boss_rolling_over_a_hole_round_and_off_a_corner() {
    let mut k = NativeKernel::new();
    let bx = k
        .box_at(
            -111.4043197631836,
            -64.5198745727539,
            -58.498023986816406,
            222.8086395263672,
            129.0397491455078,
            58.498023986816406,
        )
        .unwrap();
    let (rh, dh) = (23.902366638183587, 43.87351608276367);
    let hole = k.pcylinder(rh, dh).unwrap();
    let hole = k
        .ttranslate(&hole, -41.72284698486328, -4.301324844360352, -dh)
        .unwrap();
    let draw::Shape::Body(b) = k.bcut(&bx, &hole).unwrap() else {
        panic!("not a body")
    };
    let rim = foot_circle(&k.edges(b).unwrap(), rh, 0.0)[0];
    let rounded = local_fillet(&mut k, b, rim, 15.0);
    let rb = 18.94536972045897;
    let boss = k.pcylinder(rb, 80.0).unwrap();
    let boss = k
        .ttranslate(&boss, -86.45662689208984, 40.43245315551758, 0.0)
        .unwrap();
    let draw::Shape::Body(b) = k.bfuse(&draw::Shape::Body(rounded), &boss).unwrap() else {
        panic!("not a body")
    };
    let got = local_fillet_law(
        &mut k,
        b,
        &|es: &[EdgeInfo]| foot_circle(es, rb, 0.0),
        &[(0.0, 10.0), (1.0, 10.0)],
    );
    near_occt(got, 1688028.079073282, 113814.188352180, 3e-3);
    let (v, a) = got;
    assert!((v - 1687855.613231097).abs() < 1e-6 * v, "volume {v}");
    assert!((a - 113478.325943454).abs() < 1e-6 * a, "area {a}");
}

/// Dr Browns cap, Fillet 1, its input as the boolean before it now makes it: the dome's seam
/// moved round clear of the window (to u = π/4), the window's loop still drawn a period away
/// (valid: the check takes a loop a whole period off), and the R4 rails round the window
/// crossing the seam and back (the window lies 0.04 rad from it). The window's loop is moved
/// into the outer loop's chart, each rail split where it crosses the seam (a vertex on the
/// seam, at the rail's foot), and the rails' p-curves closed onto each other where the two
/// strips meet. By local trimming alone, valid. The input is a user document's, so it lives
/// outside the repo: `$CADRS_DOC_FIXTURES/drbrowns_fillet1_window.body` (skipped without it).
#[test]
fn fillet_round_a_window_by_a_moved_seam() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let bytes = std::fs::read(format!("{dir}/drbrowns_fillet1_window.body")).unwrap();
    let b = bodyio::read(&bytes).unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let laws: Vec<FilletLaw> = [64u64, 65, 17, 28]
        .iter()
        .map(|&e| FilletLaw {
            edge: EdgeId(e),
            radii: vec![(0.0, 4.0), (1.0, 4.0)],
        })
        .collect();
    let out = blend::fillet_laws_local(&mut k, id, &laws).unwrap().bodies[0];
    let o = k.get(out).unwrap();
    assert_eq!(o.check_with(topo::CheckLevel::Full), Vec::<String>::new());
    assert_eq!(o.solids.len(), 1);
}

/// Both ends of a hole through a plate as thick as twice the chamfer (3030 2020 connect,
/// Chamfer 1): the two closed strips' rails meet round the hole's middle and take its wall
/// whole, one rail the edge of both strips (its way round settled by the loop that closes).
/// By local trimming alone; a plate 4 thick less a hole of radius 3, both rims chamfered 2.
#[test]
fn chamfers_from_both_ends_take_a_hole_whole() {
    let mut k = NativeKernel::new();
    let p = k.box_(20.0, 20.0, 4.0).unwrap();
    let c = k.pcylinder(3.0, 10.0).unwrap();
    let c = k.ttranslate(&c, 10.0, 10.0, -3.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&p, &c).unwrap() else {
        panic!("not a body")
    };
    let es = k.edges(b).unwrap();
    let rims: Vec<EdgeId> = [0.0, 4.0]
        .iter()
        .flat_map(|&z| foot_circle(&es, 3.0, z))
        .collect();
    let out = blend::chamfer_local(&mut k, b, &rims, 2.0).unwrap().bodies[0];
    let o = k.get(out).unwrap();
    assert_eq!(o.check_with(topo::CheckLevel::Full), Vec::<String>::new());
    assert!(
        !o.faces
            .iter()
            .any(|f| matches!(f.surf, surface::Surface::Cylinder { .. })),
        "the hole's wall stays"
    );
    // The plate less two cones meeting at the middle: 20·20·4 − 2 · π/3 · 2 · (3² + 3·5 + 5²).
    let v = mass::body_mass(o).volume;
    let want = 1600.0 - 2.0 * std::f64::consts::PI / 3.0 * 2.0 * (9.0 + 15.0 + 25.0);
    assert!((v - want).abs() < 1e-6 * want, "volume {v}, want {want}");
}

/// The same from the document (3030 2020 connect, Chamfer 1), its input outside the repo.
#[test]
fn doc_3030_2020_connect_chamfer1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b =
        bodyio::read(&std::fs::read(format!("{dir}/c3030_2020_connect_chamfer1.body")).unwrap())
            .unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let edges: Vec<EdgeId> = [21u64, 19, 18, 16, 0, 5, 14, 10, 1, 6, 4, 11]
        .iter()
        .map(|&e| EdgeId(e))
        .collect();
    let out = blend::chamfer_local(&mut k, id, &edges, 2.0)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// pibox, speaker, Chamfer 1 (its input outside the repo): 7 mm chamfers along a plate whose
/// face has two slits in it (an edge out and a coincident one back), the trimming's split losing
/// the piece they are in; the chamfers' ends cross onto round faces, their cuts there the
/// chamfer's plane meeting the face in closed form (a fitted cut swung off the plane).
#[test]
fn doc_pibox_speaker_chamfer1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b = bodyio::read(&std::fs::read(format!("{dir}/pibox_speaker_chamfer1.body")).unwrap())
        .unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let out = blend::chamfer_local(&mut k, id, &[EdgeId(55), EdgeId(9)], 7.0)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// racingrigrumble, Chamfer 1 (its input outside the repo): a chamfer along tangent edges
/// whose radii from the faces' angles at each come out 2·10⁻⁶ apart, so their end balls at the
/// shared vertex miss each other by as much: one ball for both.
#[test]
fn doc_racingrigrumble_chamfer1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b = bodyio::read(&std::fs::read(format!("{dir}/racingrigrumble_chamfer1.body")).unwrap())
        .unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let out = blend::chamfer_local(&mut k, id, &[EdgeId(3), EdgeId(6), EdgeId(7)], 1.0)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// A chamfer at the foot of a bore's wall wider than the step it stands on (Breville espresso
/// parts, Chamfer 1): the chamfer adds material all round, its rail on the step past the step's
/// inner (convex) rim into the narrower bore below. The step's plane grows inward to the rail
/// (a lip over the bore), the step itself under the chamfer. A tube (radius 80, 15 high) bored
/// 71.5 above z = 3.1 and 68.5 through, the 71.5 rim chamfered 5: the triangle (5 by 5, its
/// centroid at radius 69.8333) turned round the axis added.
#[test]
fn chamfer_at_a_bore_foot_grows_the_step_over_the_bore() {
    let mut k = NativeKernel::new();
    let tube = k.pcylinder(80.0, 15.0).unwrap();
    let wide = k.pcylinder(71.5, 20.0).unwrap();
    let wide = k.ttranslate(&wide, 0.0, 0.0, 3.1).unwrap();
    let narrow = k.pcylinder(68.5, 30.0).unwrap();
    let narrow = k.ttranslate(&narrow, 0.0, 0.0, -5.0).unwrap();
    let a = k.bcut(&tube, &wide).unwrap();
    let draw::Shape::Body(b) = k.bcut(&a, &narrow).unwrap() else {
        panic!("not a body")
    };
    let before = mass::body_mass(k.get(b).unwrap()).volume;
    let es = k.edges(b).unwrap();
    let rim = foot_circle(&es, 71.5, 3.1);
    let out = blend::chamfer_local(&mut k, b, &rim, 5.0).unwrap().bodies[0];
    let o = k.get(out).unwrap();
    assert_eq!(o.check_with(topo::CheckLevel::Full), Vec::<String>::new());
    let added = 2.0 * std::f64::consts::PI * (71.5 * 2.0 + 66.5) / 3.0 * 12.5;
    let v = mass::body_mass(o).volume;
    assert!(
        (v - before - added).abs() < 1e-6 * v,
        "added {}, want {added}",
        v - before
    );
}

/// Breville espresso parts, Chamfer 1 (its input outside the repo).
#[test]
fn doc_breville_chamfer1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b = bodyio::read(&std::fs::read(format!("{dir}/breville_chamfer1.body")).unwrap()).unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let out = blend::chamfer_local(&mut k, id, &[EdgeId(5)], 5.0)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// Breville espresso parts, Chamfer 2 (its input outside the repo): the same growth outward,
/// the rail the grown face's outer loop.
#[test]
fn doc_breville_chamfer2() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b = bodyio::read(&std::fs::read(format!("{dir}/breville_chamfer2.body")).unwrap()).unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let out = blend::chamfer_local(&mut k, id, &[EdgeId(7), EdgeId(8)], 5.0)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// A wall standing on a floor through which a slot runs to the floor's far side, its inner end
/// a half circle tangent to its sides (the cable organizer's comb, below): the inside edge
/// rounded R10, its rail on the floor running over the slot. Over the slot the fillet is cut by
/// the slot's sides and round end run on up (SMLib's cliff), the cut going from flat side to
/// round end where the faces are tangent (no sign change to stop at), on the way it came. Its
/// exact volume: the round's section along the wall, less the part over the slot.
#[test]
fn fillet_over_a_slot_with_a_round_end() {
    let mut k = NativeKernel::new();
    let floor = k.box_at(0.0, 0.0, 0.0, 40.0, 30.0, 8.0).unwrap();
    let wall = k.box_at(0.0, 0.0, 0.0, 2.0, 30.0, 20.0).unwrap();
    let block = k.bfuse(&floor, &wall).unwrap();
    let slot = k.box_at(6.5, 10.0, -1.0, 40.0, 7.0, 10.0).unwrap();
    let end = k.pcylinder(3.5, 10.0).unwrap();
    let end = k.ttranslate(&end, 6.5, 13.5, -1.0).unwrap();
    let block = k.bcut(&block, &slot).unwrap();
    let draw::Shape::Body(b) = k.bcut(&block, &end).unwrap() else {
        panic!("not a body")
    };
    let v0 = k.mass_properties(b).unwrap().volume;
    let e = edge_at(&k, b, [2.0, 15.0, 8.0]);
    let out = local_fillet(&mut k, b, e, 10.0);
    let v = k.mass_properties(out).unwrap().volume;
    // The round's height over the floor at x (its centre at x = 12, z = 18), and where the
    // slot starts at y.
    let h = |x: f64| 10.0 - (100.0 - (12.0 - x).powi(2)).sqrt();
    let start = |y: f64| 6.5 - (3.5f64.powi(2) - (y - 13.5).powi(2)).max(0.0).sqrt();
    let section = 100.0 * (1.0 - std::f64::consts::FRAC_PI_4);
    let over = integral(&|y| integral(&h, start(y), 12.0, 2000), 10.0, 17.0, 2000);
    let added = 30.0 * section - over;
    assert!(
        (v - v0 - added).abs() < 1e-5 * v0,
        "added {} (exact {added})",
        v - v0
    );
}

/// A wall 3 thick, its end's four edges rounded R1.5 (half its thickness; the cable
/// organizer's teeth, below): the rounds of the two side edges have their rails on one line
/// down the end face, which goes, and the top and bottom rounds' rails on it have no length,
/// their mitres' vertices at one point. Exact volume: the end a half cylinder with its top
/// and bottom rounded by quarter cylinders, mitred.
#[test]
fn fillet_a_walls_end_from_both_sides() {
    use std::f64::consts::PI;
    let mut k = NativeKernel::new();
    let draw::Shape::Body(b) = k.box_at(0.0, 0.0, 0.0, 10.0, 3.0, 11.0).unwrap() else {
        panic!("not a body")
    };
    let edges = edges_near(
        &k,
        b,
        &[
            [10.0, 0.0, 5.5],
            [10.0, 3.0, 5.5],
            [10.0, 1.5, 0.0],
            [10.0, 1.5, 11.0],
        ],
    );
    let laws: Vec<FilletLaw> = edges
        .iter()
        .map(|&edge| FilletLaw {
            edge,
            radii: vec![(0.0, 1.5), (1.0, 1.5)],
        })
        .collect();
    let out = blend::fillet_laws_local(&mut k, b, &laws).unwrap().bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
    // Past x = 8.5 the section at u = x - 8.5 is 2s wide and 8 + 2s high, s = √(r² - u²).
    let r: f64 = 1.5;
    let volume = 8.5 * 3.0 * 11.0 + 4.0 * PI * r * r + 8.0 / 3.0 * r.powi(3);
    let v = k.mass_properties(out).unwrap().volume;
    assert!(
        (v - volume).abs() < 1e-6 * volume,
        "volume {v} (exact {volume})"
    );
}

/// cable organizer (copy 2), Fillet 2 (its input outside the repo): the teeth's ends rounded
/// R1.5 all round, each tooth 3 thick (see `fillet_a_walls_end_from_both_sides`).
#[test]
fn doc_cable_organizer_copy2_fillet2() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b =
        bodyio::read(&std::fs::read(format!("{dir}/cable_organizer_copy2_fillet2.body")).unwrap())
            .unwrap();
    let edges: Vec<EdgeId> = [
        149, 157, 270, 313, 249, 250, 260, 259, 269, 268, 267, 266, 265, 264, 262, 280, 279, 278,
        277, 276, 275, 274, 273, 272, 271, 215, 205, 199, 191, 185, 177, 171, 163, 143, 135, 129,
        121, 115, 107, 101, 93, 87, 79, 73, 65, 59, 51, 45, 37, 31, 21, 337, 339, 341, 343, 221,
        345, 231, 237, 311, 247, 335, 5, 333, 15, 331, 329, 327, 325, 323, 321, 319, 317, 315, 150,
        156, 214, 206, 198, 192, 184, 178, 170, 164, 142, 136, 128, 122, 114, 108, 100, 94, 86, 80,
        72, 66, 58, 52, 44, 38, 30, 22, 222, 230, 238, 246, 6, 14,
    ]
    .map(EdgeId)
    .to_vec();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let laws: Vec<FilletLaw> = edges
        .iter()
        .map(|&edge| FilletLaw {
            edge,
            radii: vec![(0.0, 1.5), (1.0, 1.5)],
        })
        .collect();
    let out = blend::fillet_laws_local(&mut k, id, &laws).unwrap().bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// cable organizer, Fillet 1, in three part studios (their inputs outside the repo): a wall's
/// foot rounded R10 along a floor that is a comb, its rail running over the slots between the
/// teeth (see `fillet_over_a_slot_with_a_round_end`).
#[test]
fn doc_cable_organizer_fillet1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    for (name, e) in [
        ("cable_organizer_fillet1", 210),
        ("cable_organizer_copy2_fillet1", 290),
        ("cable_organizer_copy1_fillet1", 148),
    ] {
        let b = bodyio::read(&std::fs::read(format!("{dir}/{name}.body")).unwrap()).unwrap();
        let mut k = NativeKernel::new();
        let id = k.insert(b);
        let out = local_fillet(&mut k, id, EdgeId(e), 10.0);
        assert_eq!(
            k.get(out).unwrap().check_with(topo::CheckLevel::Full),
            Vec::<String>::new(),
            "{name}"
        );
    }
}

/// The vertices of body `b` (by id) nearest the points `at`, and how far each is from it.
fn vertex_gaps(k: &NativeKernel, b: BodyId, at: &[[f64; 3]]) -> Vec<f64> {
    let o = k.get(b).unwrap();
    at.iter()
        .map(|p| {
            let p = nalgebra::Point3::new(p[0], p[1], p[2]);
            o.verts
                .iter()
                .map(|v| (v.p - p).norm())
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

/// Edges of body `b` chamfered `d` by local trimming alone: the result fully checked; its
/// volume and area.
fn local_chamfer(k: &mut NativeKernel, b: BodyId, edges: &[EdgeId], d: f64) -> (BodyId, f64, f64) {
    let out = blend::chamfer_local(k, b, edges, d).unwrap().bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
    let m = k.mass_properties(out).unwrap();
    (out, m.volume, m.surface_area)
}

/// A plate whose outline turns 135° inwards at the origin (a wall along +y meeting one along
/// (−1, −1)), chamfered 1 up that concave corner and along the slanted wall's foot (remote
/// sentry, Chamfer 1): the cutting chamfer's plane runs on over the filling one's end and ends
/// flush with the other wall, which grows down over the cut past the concave edge. The volume
/// is the plate and the fill's prism less what lies under the cutting plane short of the wall
/// (integrated over z by clipping the sections; the tool-and-boolean blend's too).
#[test]
fn chamfer_mixed_corner_at_135_degrees() {
    let mut k = NativeKernel::new();
    let outline = [
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 0.0],
        [-10.0, -10.0, 0.0],
        [-5.0, -15.0, 0.0],
        [15.0, -15.0, 0.0],
        [15.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    let w = k.polyline(&outline).unwrap();
    let f = k.mkplane(&w).unwrap();
    let draw::Shape::Body(b) = k.prism(&f, 0.0, 0.0, 5.0).unwrap() else {
        panic!("not a body")
    };
    let edges = edges_near(&k, b, &[[0.0, 0.0, 2.5], [-5.0, -5.0, 0.0]]);
    let (out, v, _) = local_chamfer(&mut k, b, &edges, 1.0);
    // The rails' crossing, where the cutting plane crosses the fill's rail on the wall, and
    // where the cutting chamfer's floor rail meets the wall's plane.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let corner = [[-h, -h, 1.0], [0.0, 1.0, 1.0 + h], [0.0, -2.0 * h, 0.0]];
    for g in vertex_gaps(&k, out, &corner) {
        assert!(g < 1e-7, "a corner point off by {g}");
    }
    assert_eq!(
        k.faces(out).unwrap().len(),
        8 + 2,
        "the plate's faces and the chamfers'"
    );
    let want = 2306.9264790838406;
    assert!((v - want).abs() < 1e-8 * want, "volume {v}, want {want}");
}

/// A box with a cylinder standing half out of one side, chamfered 1 along the box's top edge
/// there, the cylinder's top rim beside it and the concave edge where the cylinder leaves the
/// box: a mixed chamfer corner closed by a triangle, its side across the rim's chamfer (a cone)
/// the curve the triangle's plane cuts that in (remote sentry, Chamfer 1). The volume is this
/// construction's (no reference chamfers this corner); the concave edge's contacts lie 1 along
/// the box's side and 1 round the cylinder, its fill under the top chamfer about 0.97 more than
/// the ball rule's (0.480 against 0.372 a section).
#[test]
fn chamfer_mixed_corner_on_a_cone() {
    use nalgebra::Point2;
    let mut k = NativeKernel::new();
    let line = |a: (f64, f64), b: (f64, f64)| Curve2::Line {
        a: Point2::new(a.0, a.1),
        b: Point2::new(b.0, b.1),
        source: None,
    };
    let half = std::f64::consts::FRAC_PI_2;
    let outer = Loop {
        curves: vec![
            line((0.0, 0.0), (20.0, 0.0)),
            line((20.0, 0.0), (20.0, 5.0)),
            Curve2::Arc {
                center: Point2::new(20.0, 10.0),
                radius: 5.0,
                start_angle: -half,
                sweep: 2.0 * half,
                source: None,
            },
            line((20.0, 15.0), (20.0, 20.0)),
            line((20.0, 20.0), (0.0, 20.0)),
            line((0.0, 20.0), (0.0, 0.0)),
        ],
    };
    let region = Region {
        outer,
        holes: vec![],
        source: None,
    };
    let b = k
        .extrude(
            &Profile::new(Plane::top(), vec![region]),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    let edges = edges_near(
        &k,
        b,
        &[[20.0, 2.5, 10.0], [25.0, 10.0, 10.0], [20.0, 5.0, 5.0]],
    );
    let (out, v, a) = local_chamfer(&mut k, b, &edges, 1.0);
    // The corner's triangle: a plane bounded by two lines and the curve on the cone.
    let o = k.get(out).unwrap();
    let cap = o.faces.iter().any(|f| {
        matches!(f.surf, surface::Surface::Plane { .. })
            && f.loops.len() == 1
            && f.loops[0].len() == 3
            && f.loops[0]
                .iter()
                .any(|c| !matches!(o.edges[c.edge].curve, curve::Curve::Line { .. }))
    });
    assert!(cap, "no corner triangle with a curved side");
    let (want_v, want_a) = (4386.985846155351, 1719.332028409182);
    assert!(
        (v - want_v).abs() < 1e-6 * want_v,
        "volume {v}, want {want_v}"
    );
    assert!(
        (a - want_a).abs() < 1e-6 * want_a,
        "area {a}, want {want_a}"
    );
}

/// A cylinder with a slice off its side (a D-shaped prism), chamfered 1 round its top and up the
/// two edges where the cut meets the cylinder: at each top corner two flat chamfers and the
/// rim's cone meet in a point, where the line the flat ones meet in crosses the cone; the
/// cone's ends there the conics the flat ones cut it in (gridfinity, Chamfer 2). The volume is
/// that of the prism cut by every chamfer's plane and the rim's cone (integrated over z by
/// clipping the sections; the chamfers up the sides through their contacts 1 along the flat
/// face and 1 round the cylinder, as `chamfer_plane_to_cylinder_measures_round_the_cylinder`).
#[test]
fn chamfer_corner_of_two_planes_and_a_cone() {
    let mut k = NativeKernel::new();
    let c = k.pcylinder(10.0, 10.0).unwrap();
    let bx = k.box_at(-20.0, -20.0, -1.0, 14.0, 40.0, 12.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&c, &bx).unwrap() else {
        panic!("not a body")
    };
    // The top's edges (the rim in pieces at the cylinder's seam) and the cut's two sides.
    let edges: Vec<EdgeId> = k
        .edges(b)
        .unwrap()
        .iter()
        .filter(|e| e.mid.z > 10.0 - 1e-9 || (e.mid.x + 6.0).abs() < 1e-9 && e.mid.y.abs() > 7.0)
        .map(|e| e.id)
        .collect();
    assert_eq!(edges.len(), 5, "{edges:?}");
    let (_, v, _) = local_chamfer(&mut k, b, &edges, 1.0);
    let want = 2657.5007292150626;
    assert!((v - want).abs() < 1e-8 * want, "volume {v}, want {want}");
}

/// The D-shaped prism's chamfer section (a cylinder of radius `r` cut at x = −6, the edge at
/// (−6, 8)), its contacts `along` down the flat face and `alpha` (radians) round the cylinder:
/// the triangle between the edge and the contacts and the circular segment over its curved side.
fn d_prism_section(r: f64, along: f64, alpha: f64) -> f64 {
    let (ex, ey) = (-6.0_f64, 8.0_f64);
    let (px, py) = (ex, ey - along);
    let q = ey.atan2(ex) - alpha;
    let (qx, qy) = (r * q.cos(), r * q.sin());
    let triangle = ((px - ex) * (qy - ey) - (qx - ex) * (py - ey)).abs() / 2.0;
    triangle + r * r / 2.0 * (alpha - alpha.sin())
}

/// A cylinder of radius 10 cut at x = −6 (a D-shaped prism) and its two edges where the cut
/// meets the cylinder.
fn d_prism(k: &mut NativeKernel) -> (BodyId, Vec<EdgeId>) {
    let c = k.pcylinder(10.0, 10.0).unwrap();
    let bx = k.box_at(-20.0, -20.0, -1.0, 14.0, 40.0, 12.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&c, &bx).unwrap() else {
        panic!("not a body")
    };
    let edges: Vec<EdgeId> = k
        .edges(b)
        .unwrap()
        .iter()
        .filter(|e| (e.mid.x + 6.0).abs() < 1e-9 && e.mid.y.abs() > 7.0)
        .map(|e| e.id)
        .collect();
    assert_eq!(edges.len(), 2, "{edges:?}");
    (b, edges)
}

/// The D-shaped prism ([`d_prism`]) chamfered 1 up its two edges: the contacts lie 1 from each
/// edge along the flat face and 1 round the cylinder (Offset measures along the faces; the ball
/// whose contacts on the faces' tangent planes are 1 away, of radius tan(θ/2) = 2 here, would
/// touch the flat face 1.07 from the edge and the cylinder 1.20 round it). Each section takes
/// off [`d_prism_section`] with α = 1/R, over the height 10.
#[test]
fn chamfer_plane_to_cylinder_measures_round_the_cylinder() {
    let mut k = NativeKernel::new();
    let (b, edges) = d_prism(&mut k);
    let before = k.mass_properties(b).unwrap().volume;
    let (_, v, _) = local_chamfer(&mut k, b, &edges, 1.0);
    let want = before - 2.0 * 10.0 * d_prism_section(10.0, 1.0, 0.1);
    assert!((v - want).abs() < 1e-9 * want, "volume {v}, want {want}");
}

/// Equal-distance chamfers `d` measured along the faces' tangent planes (ChamferMeasure::Tangent)
/// on `edges` by local trimming alone (no fallback to the blend tool): the result checked, its
/// volume.
fn local_tangent_chamfer(
    k: &mut NativeKernel,
    b: BodyId,
    edges: &[EdgeId],
    d: f64,
) -> (BodyId, f64) {
    let body = (**k.get(b).unwrap()).clone();
    let dists: Vec<(usize, [f64; 2])> = edges.iter().map(|e| (e.0 as usize, [d, d])).collect();
    let (out, _) = fillet::chamfer_edges_measured(&body, b, &dists, true).unwrap();
    assert_eq!(out.check_with(topo::CheckLevel::Full), Vec::<String>::new());
    let out = k.insert(out);
    (out, k.mass_properties(out).unwrap().volume)
}

/// The D-shaped prism ([`d_prism`]) chamfered 2 up one edge measured along the faces' tangent
/// planes by the blend tool alone: the same tool as the chamfer of 2 along the flat face and the
/// arc R·atan(2/R) round the cylinder (to where the tangent distance's chord ends), as the local
/// chamfer's ([`chamfer_plane_to_cylinder_tangent_measure`]); not the chord taken as an arc.
#[test]
fn tool_chamfer_plane_to_cylinder_tangent_measure() {
    let mut k = NativeKernel::new();
    let (b, edges) = d_prism(&mut k);
    let edge = &edges[..1];
    let mut volume = |spec: ChamferSpec, measurement: ChamferMeasure| {
        let opts = ChamferOpts {
            measurement,
            ..ChamferOpts::new(spec)
        };
        let out = blend::chamfer_tool(&mut k, b, edge, &opts).unwrap().bodies[0];
        assert_eq!(
            k.get(out).unwrap().check_with(topo::CheckLevel::Full),
            Vec::<String>::new()
        );
        k.mass_properties(out).unwrap().volume
    };
    let tangent = volume(ChamferSpec::EqualDistance(2.0), ChamferMeasure::Tangent);
    let arc = 10.0 * (0.2_f64).atan();
    let chord = 20.0 * ((0.2_f64).atan() / 2.0).sin();
    // (Which side the cylinder is on in the edge's order: one of the two.)
    let near = |x: f64| (x - tangent).abs() < 1e-9 * tangent;
    let mut two = |a: f64, c: f64| volume(ChamferSpec::TwoDistances(a, c), ChamferMeasure::Offset);
    let by_arc = [two(2.0, arc), two(arc, 2.0)];
    let by_chord = [two(2.0, chord), two(chord, 2.0)];
    assert!(
        by_arc.iter().any(|&v| near(v)) && !by_chord.iter().any(|&v| near(v)),
        "tangent {tangent}, by arc {by_arc:?}, by chord {by_chord:?}"
    );
}

/// A cylinder of radius 10 under a hemisphere of the same radius, cut at x = −6, chamfered 1 up
/// each side and over the top (Tangent measure): each straight plane-to-cylinder edge runs on
/// smoothly into the plane-to-sphere arc, which isn't measured round its sphere, so the whole
/// chain takes the ball rule with the tangent distance itself, the ball whose offsets put its
/// contacts on the faces' tangent planes 1 from the edge: of radius tan(θ/2) = 2 (θ ≈ 126.9°),
/// centre 2 from the cut and 8 from the axis, so the cut's contact lies 8 − 4√3 from each
/// straight edge (measured round the cylinder, 1; with the chord of 1 taken as a tangent
/// distance, nearer).
#[test]
fn chamfer_tangent_chain_round_a_cylinder_onto_a_sphere() {
    use nalgebra::{Point3, Vector3};
    let mut k = NativeKernel::new();
    let ax = draw::Ax3 {
        origin: Point3::new(0.0, 0.0, -10.0),
        z: Vector3::z(),
        x: Vector3::x(),
    };
    let c = k.pcylinder_on(&ax, 10.0, 10.0).unwrap();
    let s = k.psphere(10.0).unwrap();
    let cap = k.bfuse(&c, &s).unwrap();
    let bx = k.box_at(-20.0, -20.0, -21.0, 14.0, 40.0, 42.0).unwrap();
    let draw::Shape::Body(b) = k.bcut(&cap, &bx).unwrap() else {
        panic!("not a body")
    };
    let edges: Vec<EdgeId> = k
        .edges(b)
        .unwrap()
        .iter()
        .filter(|e| (e.mid.x + 6.0).abs() < 1e-9 && e.mid.z > -10.0 + 1e-9)
        .map(|e| e.id)
        .collect();
    assert!(edges.len() >= 3, "{edges:?}");
    let (out, _) = local_tangent_chamfer(&mut k, b, &edges, 1.0);
    // The cut's straight boundaries up the sides, halfway up.
    let ys: Vec<f64> = k
        .edges(out)
        .unwrap()
        .iter()
        .filter(|e| (e.mid.x + 6.0).abs() < 1e-9 && (e.mid.z + 5.0).abs() < 1e-6)
        .map(|e| e.mid.y.abs())
        .collect();
    let want = 4.0 * 3.0_f64.sqrt();
    assert!(
        ys.len() == 2 && ys.iter().all(|y| (y - want).abs() < 1e-9),
        "{ys:?}, want ±{want}"
    );
}

/// The D-shaped prism ([`d_prism`]) chamfered 2 up its two edges measured along the faces'
/// tangent planes: 2 down the flat face, and round the cylinder to the chord's end of the
/// tangent distance, the angle atan(2/R).
#[test]
fn chamfer_plane_to_cylinder_tangent_measure() {
    let mut k = NativeKernel::new();
    let (b, edges) = d_prism(&mut k);
    let before = k.mass_properties(b).unwrap().volume;
    let v = local_tangent_chamfer(&mut k, b, &edges, 2.0).1;
    let want = before - 2.0 * 10.0 * d_prism_section(10.0, 2.0, (0.2_f64).atan());
    assert!((v - want).abs() < 1e-9 * want, "volume {v}, want {want}");
}

/// remote sentry, Chamfer 1 (its input outside the repo): 0.5 mm chamfers round a plate's
/// outline and pockets, with mixed corners at 135° and corner triangles across cones.
#[test]
fn doc_remote_sentry_chamfer1() {
    let Ok(dir) = std::env::var("CADRS_DOC_FIXTURES") else {
        return;
    };
    let b = bodyio::read(&std::fs::read(format!("{dir}/remote_sentry_chamfer1.body")).unwrap())
        .unwrap();
    let mut k = NativeKernel::new();
    let id = k.insert(b);
    let edges: Vec<EdgeId> = [
        30u64, 66, 67, 68, 33, 37, 38, 39, 141, 140, 5, 6, 13, 69, 70, 71, 9, 17, 26, 20, 23, 34,
        35, 36, 19, 14, 142, 8, 10, 143, 27, 44, 45, 73, 72, 22, 24, 2, 3, 1, 16, 12,
    ]
    .iter()
    .map(|&e| EdgeId(e))
    .collect();
    let out = blend::chamfer_local(&mut k, id, &edges, 0.5)
        .unwrap()
        .bodies[0];
    assert_eq!(
        k.get(out).unwrap().check_with(topo::CheckLevel::Full),
        Vec::<String>::new()
    );
}

/// A user's document (ekgpulse, the bracket's Chamfer 1): 5 mm equal chamfers on 23 inside
/// corners of a 3D-printed bracket, among them two meeting under its lid round an outside corner
/// of the walls below (mitred: the corner filled up to the lower of their faces). Onshape's own
/// evaluation of the document (its volume before and after the chamfer, 63 700.87 and
/// 65 172.88 mm³, each to about ±1.6) has the chamfer add 1 472.02 mm³, the reference here. (The
/// input is the import's, 91.8 mm³ short of Onshape's before the chamfer; and where a hole in
/// the lid runs over one chamfer Onshape carries it on through the fill, which this kernel
/// doesn't yet: 1.4 mm³ more.)
#[test]
fn ekgpulse_bracket_chamfer() {
    let mut k = NativeKernel::new();
    let b =
        k.insert(bodyio::read(include_bytes!("data/docs/ekgpulse_bracket_chamfer.body")).unwrap());
    let v0 = k.mass_properties(b).unwrap().volume;
    let edges = [
        244, 246, 247, 253, 251, 99, 88, 93, 95, 292, 296, 305, 299, 359, 348, 360, 349, 50, 52,
        48, 54, 352, 353,
    ]
    .map(EdgeId);
    let out = k
        .chamfer(b, &edges, ChamferSpec::EqualDistance(5.0))
        .unwrap()
        .bodies[0];
    let issues = k.get(out).unwrap().check();
    assert!(issues.is_empty(), "{issues:?}");
    let added = k.mass_properties(out).unwrap().volume - v0;
    assert!((1_470.4..1_473.7).contains(&added), "added {added}");
}
