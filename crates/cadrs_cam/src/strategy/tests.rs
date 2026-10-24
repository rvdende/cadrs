//! Tests of the strategies' shared settings: patterns, levels and repeats.

use super::*;
use crate::geom::P3;
use crate::machine::MachineKind;
use crate::toolpath::FeedKind;

#[test]
fn patterns_by_type_and_tool() {
    let mill = Pattern::available(Type::Edges, ToolKind::EndMill, MachineKind::Mill);
    assert!(mill.contains(&Pattern::Profile2D));
    assert!(!mill.contains(&Pattern::Cut));
    let laser = Pattern::available(Type::Edges, ToolKind::Laser, MachineKind::Laser);
    assert_eq!(laser, vec![Pattern::Cut, Pattern::Etch]);
    assert!(
        Pattern::available(Type::Holes, ToolKind::Drill, MachineKind::Mill)
            .contains(&Pattern::Drill)
    );
    assert!(Pattern::available(Type::Edges, ToolKind::EndMill, MachineKind::Laser).is_empty());
    // Every pattern has a tool and machine it runs with.
    for p in Pattern::ALL {
        assert!(
            ToolKind::ALL
                .iter()
                .any(|t| MachineKind::ALL.iter().any(|m| p.allowed(*t, *m))),
            "{p:?}"
        );
    }
}

#[test]
fn levels_end_at_bottom_with_floors() {
    let m = testing::pocket_block();
    let stock = testing::stock_for(&m, 5.0, 1.0);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let mut p = Params::new(Pattern::Rough2D, &tool, MachineKind::Mill);
    p.step_down = 5.0;
    p.floor_stock = 0.0;
    let input = Input {
        model: &m,
        stock: &stock,
        tool: &tool,
        machine: MachineKind::Mill,
        params: &p,
        rest: None,
        fixtures: &[],
    };
    let l = input.levels(21.0, 0.0, true);
    assert_eq!(*l.last().unwrap(), 0.0);
    assert!(l.iter().any(|z| (z - 12.0).abs() < 1e-9), "{l:?}");
    assert!(l.windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn repeats_place_copies() {
    let tp = Toolpath {
        steps: vec![
            Step::Spindle {
                rpm: 1000.0,
                cw: true,
            },
            Step::Rapid([10.0, 0.0, 5.0]),
            Step::Arc {
                to: [10.0, 0.0, -1.0],
                center: [5.0, 0.0],
                cw: false,
                feed: 100.0,
                kind: crate::toolpath::FeedKind::Cut,
            },
        ],
        warnings: vec![],
    };
    let lin = Repeat::Linear {
        count: 3,
        step: [20.0, 0.0],
    }
    .apply(tp.clone());
    assert_eq!(lin.steps.len(), 1 + 2 * 3, "the spindle once");
    assert!(matches!(lin.steps[5], Step::Rapid(p) if (p[0] - 50.0).abs() < 1e-9));
    let circ = Repeat::Circular {
        count: 4,
        center: [0.0, 0.0],
        angle: 90.0,
    }
    .apply(tp);
    // The second copy turned 90°: (10, 0) to (0, 10), its arc's centre with it.
    assert!(
        matches!(circ.steps[3], Step::Rapid(p) if p[0].abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9)
    );
    assert!(
        matches!(circ.steps[4], Step::Arc { center, .. } if center[0].abs() < 1e-9 && (center[1] - 5.0).abs() < 1e-9)
    );
}

/// A profile along two picked bottom edges of a block (an L, open): the tool runs a radius off
/// them, outside the block, on whichever side the material isn't, and cuts nothing of it.
/// Inside with nothing picked means the part's through holes; a block has none.
#[test]
fn a_profile_on_open_edges_keeps_off_the_part() {
    use crate::model::{Edge, Model, box_triangles};
    use crate::sim::{SimOp, simulate_mill_on};
    let mut m = Model::from_triangles(
        "block",
        box_triangles(&Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 12.0])),
    );
    for (name, a, b) in [
        ("e1", [0.0, 0.0, 0.0], [60.0, 0.0, 0.0]),
        ("e2", [60.0, 0.0, 0.0], [60.0, 40.0, 0.0]),
    ] {
        m.edges.push(Edge {
            name: name.into(),
            body: 0,
            points: vec![a, b],
            circle: None,
        });
    }
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [65.0, 45.0, 12.0]));
    for (side, picked) in [
        (Side::Auto, true),
        (Side::Outside, true),
        (Side::Auto, false),
    ] {
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.side = side;
        // (Both orders of the picks: the side comes from the material, not the order.)
        if picked {
            p.drive.edges = vec!["e2".into(), "e1".into()];
        }
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
        let cuts: Vec<P3> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect();
        assert!(!cuts.is_empty());
        if picked {
            // Along the bottom edge a radius below it, along the right one a radius right.
            assert!(cuts.iter().any(|q| (q[1] + 3.0).abs() < 0.05), "{side:?}");
            assert!(cuts.iter().any(|q| (q[0] - 63.0).abs() < 0.05), "{side:?}");
        }
        let segs = tp.motion([0.0, 0.0, 60.0], 5_000.0);
        let r = simulate_mill_on(
            stock.height_map(0.25),
            &m,
            &[],
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
        );
        assert_eq!(r.gouge_cells, 0, "{side:?} {picked}: {}", r.gouge_depth);
    }
    let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
    p.side = Side::Inside;
    let input = Input {
        model: &m,
        stock: &stock,
        tool: &tool,
        machine: MachineKind::Mill,
        params: &p,
        rest: None,
        fixtures: &[],
    };
    let e = generate(&input).unwrap_err();
    assert!(e.contains("no through holes"), "{e}");
    // Inside on the picked edges is the part's side: cut, but said.
    let mut q = p.clone();
    q.drive.edges = vec!["e1".into(), "e2".into()];
    let input = Input {
        params: &q,
        ..input
    };
    let tp = generate(&input).unwrap();
    assert!(
        tp.warnings.iter().any(|w| w.contains("cuts into the part")),
        "{:?}",
        tp.warnings
    );
}

/// A laser cut along a picked open edge: the beam runs half its kerf off it, outside the part.
#[test]
fn a_cut_on_an_open_edge_keeps_its_kerf_off_the_part() {
    use crate::model::{Edge, Model, box_triangles};
    let mut m = Model::from_triangles(
        "plate",
        box_triangles(&Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 3.0])),
    );
    m.edges.push(Edge {
        name: "e1".into(),
        body: 0,
        points: vec![[60.0, 0.0, 3.0], [0.0, 0.0, 3.0]],
        circle: None,
    });
    let laser = Tool::new(ToolKind::Laser, 0.2);
    let stock = Stock::block(Aabb::new([-5.0, -5.0, 0.0], [65.0, 45.0, 3.0]));
    let mut p = Params::new(Pattern::Cut, &laser, MachineKind::Laser);
    p.drive.edges = vec!["e1".into()];
    let input = Input {
        model: &m,
        stock: &stock,
        tool: &laser,
        machine: MachineKind::Laser,
        params: &p,
        rest: None,
        fixtures: &[],
    };
    let tp = generate(&input).unwrap();
    let ys: Vec<f64> = tp
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } => Some(to[1]),
            _ => None,
        })
        .collect();
    assert!(!ys.is_empty());
    for y in ys {
        assert!((y + 0.1).abs() < 1e-6, "{y}");
    }
}

/// A floor edge at a wall's foot (a 5 mm base, a wall standing on its back half to 12): picked
/// either way round, the tool runs a radius off it on the floor's side, down to the edge (the
/// selected contour), and cuts neither the wall nor the floor.
#[test]
fn a_profile_on_a_floor_edge_keeps_off_the_wall() {
    use crate::model::{Edge, Model, box_triangles};
    use crate::sim::{SimOp, simulate_mill_on};
    let mut tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 5.0]));
    tris.extend(box_triangles(&Aabb::new(
        [0.0, 20.0, 5.0],
        [60.0, 40.0, 12.0],
    )));
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 12.0]));
    for points in [
        vec![[0.0, 20.0, 5.0], [60.0, 20.0, 5.0]],
        vec![[60.0, 20.0, 5.0], [0.0, 20.0, 5.0]],
    ] {
        let mut m = Model::from_triangles("step", tris.clone());
        m.edges.push(Edge {
            name: "foot".into(),
            body: 0,
            points: points.clone(),
            circle: None,
        });
        let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
        p.drive.edges = vec!["foot".into()];
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
        let cuts: Vec<P3> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Feed {
                    to,
                    kind: FeedKind::Cut,
                    ..
                } => Some(*to),
                _ => None,
            })
            .collect();
        assert!(!cuts.is_empty());
        for q in &cuts {
            assert!((q[1] - 17.0).abs() < 0.05, "{points:?}: {q:?}");
            assert!(q[2] > 5.0 - 1e-6, "{points:?}: {q:?}");
        }
        let segs = tp.motion([0.0, 0.0, 60.0], 5_000.0);
        let r = simulate_mill_on(
            stock.height_map(0.25),
            &m,
            &[],
            &[SimOp {
                tool: &tool,
                segments: &segs,
            }],
        );
        assert_eq!(r.gouge_cells, 0, "{points:?}: {}", r.gouge_depth);
    }
}

/// Inside a through hole, nothing picked, climb: round it counter-clockwise (the material
/// round the hole on the right), leads in the hole, wear compensation on the left (G41), as
/// when the hole's edges are picked.
#[test]
fn inside_a_hole_climbs_with_the_material_on_the_right() {
    use crate::model::{Model, box_triangles};
    let mut tris = Vec::new();
    for (lo, hi) in [
        ([0.0, 0.0], [20.0, 40.0]),
        ([40.0, 0.0], [60.0, 40.0]),
        ([20.0, 0.0], [40.0, 10.0]),
        ([20.0, 30.0], [40.0, 40.0]),
    ] {
        tris.extend(box_triangles(&Aabb::new(
            [lo[0], lo[1], 0.0],
            [hi[0], hi[1], 12.0],
        )));
    }
    let m = Model::from_triangles("frame", tris);
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 12.0]));
    let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
    p.side = Side::Inside;
    p.step_down = 12.0;
    p.compensation = Compensation::Wear;
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
    let ring: Vec<P2> = tp
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } => Some([to[0], to[1]]),
            _ => None,
        })
        .collect();
    assert!(
        crate::geom::ring_area2(&ring) > 0.0,
        "clockwise: conventional"
    );
    assert!(
        tp.steps
            .iter()
            .any(|s| matches!(s, Step::Compensation(Some(true))))
    );
    assert!(
        !tp.steps
            .iter()
            .any(|s| matches!(s, Step::Compensation(Some(false))))
    );
}

/// A through hole's top edges picked (the ones in view), the bottom left at the selection: the
/// profile cuts down the hole's wall to the model's bottom, and says so.
#[test]
fn a_contour_picked_by_its_top_edges_is_cut_down_its_wall() {
    use crate::model::{Edge, Model, box_triangles};
    let mut tris = Vec::new();
    for (lo, hi) in [
        ([0.0, 0.0], [20.0, 40.0]),
        ([40.0, 0.0], [60.0, 40.0]),
        ([20.0, 0.0], [40.0, 10.0]),
        ([20.0, 30.0], [40.0, 40.0]),
    ] {
        tris.extend(box_triangles(&Aabb::new(
            [lo[0], lo[1], 0.0],
            [hi[0], hi[1], 12.0],
        )));
    }
    let mut m = Model::from_triangles("frame", tris);
    m.edges.push(Edge {
        name: "hole-top".into(),
        body: 0,
        points: vec![
            [20.0, 10.0, 12.0],
            [40.0, 10.0, 12.0],
            [40.0, 30.0, 12.0],
            [20.0, 30.0, 12.0],
            [20.0, 10.0, 12.0],
        ],
        circle: None,
    });
    let tool = Tool::new(ToolKind::EndMill, 6.0);
    let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 12.0]));
    let mut p = Params::new(Pattern::Profile2D, &tool, MachineKind::Mill);
    p.drive.edges = vec!["hole-top".into()];
    assert_eq!(p.heights.bottom.from, HeightRef::Selection);
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
    let lowest = tp
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } => Some(to[2]),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    assert!(lowest < 1e-6, "{lowest}");
    assert!(
        tp.warnings
            .iter()
            .any(|w| w.contains("picked edges are at the top")),
        "{:?}",
        tp.warnings
    );
}
