//! Converting pixel positions to named targets (`CADRS_RECORD_TARGETS=1`): for every pixel
//! position a step uses (`MoveTo`, `ClickAt`, `at(..)`), writes the target that names the same
//! place to `targets.tsv` in the run's output directory (`x`, `y`, the target, why), in the
//! order the steps run; `tools/name_targets.py` rewrites the scenario with them.
//!
//! Over the 3D view a point becomes a sketch-plane point, `world(..)` (the Top plane when no
//! sketch is open), or `xyz(..)` where that plane is seen edge-on; over the flat view,
//! `flat(..)`; over a UI node, `ui(name)` when the node's center lands on it too, otherwise
//! `ui_at(name, fx, fy)` for the same pixel. Each is checked to resolve to the same whole pixel.
//! Steps with pixel positions wait for the view to settle first, as the targets they become do.

use std::io::Write;
use std::path::Path;

use bevy::prelude::*;
use bevy::ui::{CalculatedClip, UiGlobalTransform, UiStack};

use crate::runner::{FlatToScreen, SpaceToScreen, WorldToScreen};
use crate::scenario::Step;

pub fn enabled() -> bool {
    std::env::var_os("CADRS_RECORD_TARGETS").is_some_and(|v| v != "0")
}

fn write(out_dir: &Path, line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out_dir.join("targets.tsv"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// After the scenario failed: the steps it didn't reach, against the layout as it is now. A
/// point in the 3D view stays that pixel of the view's area (the camera will have moved by
/// then); elsewhere it is that pixel of the named node there.
pub fn record_unreached<'a>(
    world: &mut World,
    out_dir: &Path,
    steps: impl Iterator<Item = &'a Step>,
) {
    let area = first_named(world, "viewport-area").map(|e| rect(world, e));
    for step in steps {
        for (x, y) in step.pixel_positions() {
            let p = Vec2::new(x, y);
            let (target, why) = match area {
                Some((c, size)) if (p - c).abs().cmple(size / 2.0).all() => (
                    ui_at_exact(p, "viewport-area", c, size),
                    "unreached, 3D view".into(),
                ),
                _ => {
                    let (t, why) = name_point_ui(world, p, true);
                    (t, format!("unreached, {why}"))
                }
            };
            write(out_dir, &format!("{}\t{}\t{target}\t{why}\n", p.x, p.y));
        }
    }
}

fn ui_at_exact(p: Vec2, name: &str, center: Vec2, size: Vec2) -> String {
    let px = p.round();
    let f = (p - center) / size + 0.5;
    for d in 2..=4 {
        let (fx, fy) = (round_to(f.x, d), round_to(f.y, d));
        let back = (center + (Vec2::new(fx, fy) - 0.5) * size).round();
        if back == px || d == 4 {
            return format!("ui_at({name:?}, {}, {})", fmt(fx, d), fmt(fy, d));
        }
    }
    unreachable!()
}

pub fn record(world: &mut World, out_dir: &Path, step: &Step) {
    let mut inner = step;
    while let Step::Now(s) = inner {
        inner = s;
    }
    // Where in a node a drag, press or scroll starts and ends matters.
    let exact = matches!(
        inner,
        Step::Drag(..)
            | Step::RightDrag(..)
            | Step::MiddleDrag(..)
            | Step::Press(_)
            | Step::PressWith(..)
            | Step::Scroll(..)
    );
    for (x, y) in step.pixel_positions() {
        let p = Vec2::new(x, y);
        let (target, why) = name_point(world, p, exact);
        write(out_dir, &format!("{}\t{}\t{target}\t{why}\n", p.x, p.y));
    }
}

/// The topmost pickable UI node at `p` (logical px).
fn top_node(world: &mut World, p: Vec2) -> Option<Entity> {
    let stack: Vec<Entity> = world.resource::<UiStack>().iter().collect();
    for &e in stack.iter().rev() {
        let Ok(er) = world.get_entity(e) else {
            continue;
        };
        let (Some(node), Some(t), Some(vis)) = (
            er.get::<ComputedNode>(),
            er.get::<UiGlobalTransform>(),
            er.get::<InheritedVisibility>(),
        ) else {
            continue;
        };
        if !vis.get() || node.size().x <= 0.0 || node.size().y <= 0.0 {
            continue;
        }
        if er.get::<Pickable>().is_some_and(|pk| !pk.is_hoverable) {
            continue;
        }
        let k = node.inverse_scale_factor();
        let center = t.translation * k;
        let half = node.size() * k / 2.0;
        if (p - center).abs().cmpgt(half).any() {
            continue;
        }
        if let Some(clip) = er.get::<CalculatedClip>()
            && !clip.contains_point(p / k)
        {
            continue;
        }
        return Some(e);
    }
    None
}

/// The entity and its nearest named ancestor (or itself).
fn named_ancestor(world: &World, mut e: Entity) -> Option<(Entity, String)> {
    loop {
        if let Some(n) = world.get::<Name>(e)
            && !n.as_str().is_empty()
        {
            return Some((e, n.as_str().to_string()));
        }
        e = world.get::<ChildOf>(e)?.parent();
    }
}

/// The first visible node with this name, as `ui(..)` resolves it.
fn first_named(world: &mut World, name: &str) -> Option<Entity> {
    let mut q = world.query::<(
        Entity,
        &Name,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>();
    q.iter(world)
        .find(|(_, n, node, _, vis)| {
            n.as_str() == name && vis.get() && node.size().x > 0.0 && node.size().y > 0.0
        })
        .map(|(e, ..)| e)
}

fn rect(world: &World, e: Entity) -> (Vec2, Vec2) {
    let node = world.get::<ComputedNode>(e).unwrap();
    let t = world.get::<UiGlobalTransform>(e).unwrap();
    let k = node.inverse_scale_factor();
    (t.translation * k, node.size() * k)
}

fn fmt(v: f32, d: usize) -> String {
    let s = format!("{v:.d$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

fn round_to(v: f32, d: usize) -> f32 {
    fmt(v, d).parse().unwrap()
}

const VIEW_NAMES: &[&str] = &["viewport-area", "viewport-backdrop"];

/// A control small enough that any point of it does the same (a button, a list row); in a
/// larger node (a panel, a 3D view) the point is kept.
const SMALL: Vec2 = Vec2::new(400.0, 100.0);

/// The named target at `p`, and why it was chosen. `exact`: the same pixel, not the node's
/// center.
pub fn name_point(world: &mut World, p: Vec2, exact: bool) -> (String, String) {
    let px = p.round();
    let hit = top_node(world, p);
    let named = hit.and_then(|h| named_ancestor(world, h));
    let in_view = match &named {
        None => true,
        Some((_, n)) => VIEW_NAMES.contains(&n.as_str()),
    };
    if in_view && let Some(t) = view_target(world, px) {
        return t;
    }
    name_point_ui(world, p, exact)
}

/// The UI target at `p`: `ui(name)` or `ui_at(name, ..)`.
fn name_point_ui(world: &mut World, p: Vec2, exact: bool) -> (String, String) {
    let named = top_node(world, p).and_then(|h| named_ancestor(world, h));
    let Some((mut e, mut name)) = named else {
        return (format!("at({}, {})", p.x, p.y), "nothing here".into());
    };
    // A name `ui(..)` finds: walk up to one that is the first of its name.
    let mut leaf = true;
    loop {
        if first_named(world, &name) == Some(e) {
            break;
        }
        leaf = false;
        let Some(parent) = world.get::<ChildOf>(e).map(|c| c.parent()) else {
            return (format!("at({}, {})", p.x, p.y), "no unique name".into());
        };
        match named_ancestor(world, parent) {
            Some(a) => (e, name) = a,
            None => return (format!("at({}, {})", p.x, p.y), "no unique name".into()),
        }
    }
    let (center, size) = rect(world, e);
    // The center hits the same named node: `ui(name)`.
    if leaf && !exact && size.cmple(SMALL).all() {
        let c = top_node(world, center.round())
            .and_then(|h| named_ancestor(world, h))
            .map(|(ce, _)| ce);
        if c == Some(e) {
            return (format!("ui({name:?})"), "center".into());
        }
    }
    // Otherwise the same pixel inside it.
    let why = if exact {
        "exact"
    } else if !leaf {
        "in parent"
    } else if size.cmpgt(SMALL).any() {
        "large node"
    } else {
        "off center"
    };
    (ui_at_exact(p, &name, center, size), why.into())
}

fn view_target(world: &mut World, px: Vec2) -> Option<(String, String)> {
    if let Some(m) = world.get_resource::<FlatToScreen>().and_then(|m| m.0) {
        let det = m.x_axis.perp_dot(m.y_axis);
        if det.abs() > 1e-6 {
            let inv = |q: Vec2| {
                let d = q - m.origin;
                Vec2::new(d.perp_dot(m.y_axis) / det, m.x_axis.perp_dot(d) / det)
            };
            let w = inv(px);
            for d in 0..=4 {
                let r = Vec2::new(round_to(w.x, d), round_to(w.y, d));
                if m.apply(r).round() == px || d == 4 {
                    return Some((
                        format!("flat({}, {})", fmt(r.x, d), fmt(r.y, d)),
                        "flat view".into(),
                    ));
                }
            }
        }
    }
    if let Some(m) = world.get_resource::<WorldToScreen>().and_then(|m| m.0) {
        let det = m.x_axis.perp_dot(m.y_axis);
        // Not edge-on.
        if det.abs() > 0.05 * m.x_axis.length() * m.y_axis.length() {
            let d0 = px - m.origin;
            let w = Vec2::new(d0.perp_dot(m.y_axis) / det, m.x_axis.perp_dot(d0) / det);
            for d in 0..=4 {
                let r = Vec2::new(round_to(w.x, d), round_to(w.y, d));
                if m.apply(r).round() == px {
                    return Some((
                        format!("world({}, {})", fmt(r.x, d), fmt(r.y, d)),
                        "sketch plane".into(),
                    ));
                }
            }
        }
    }
    let m = world.get_resource::<SpaceToScreen>().and_then(|m| m.0)?;
    // The pair of axes the view faces most; the third coordinate 0.
    let axes = [m.x_axis, m.y_axis, m.z_axis];
    let (i, j) = [(0usize, 1usize), (0, 2), (1, 2)]
        .into_iter()
        .max_by(|a, b| {
            let da = axes[a.0].perp_dot(axes[a.1]).abs();
            let db = axes[b.0].perp_dot(axes[b.1]).abs();
            da.total_cmp(&db)
        })
        .unwrap();
    let (a, b) = (axes[i], axes[j]);
    let det = a.perp_dot(b);
    if det.abs() < 1e-9 {
        return None;
    }
    let d0 = px - m.origin;
    let (u, v) = (d0.perp_dot(b) / det, a.perp_dot(d0) / det);
    for d in 0..=4 {
        let mut c = [0.0f32; 3];
        c[i] = round_to(u, d);
        c[j] = round_to(v, d);
        let back = (m.origin + m.x_axis * c[0] + m.y_axis * c[1] + m.z_axis * c[2]).round();
        if back == px {
            return Some((
                format!("xyz({}, {}, {})", fmt(c[0], d), fmt(c[1], d), fmt(c[2], d)),
                "3D view".into(),
            ));
        }
    }
    None
}
