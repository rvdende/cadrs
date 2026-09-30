//! IDF → B-rep (X5 forward): the board, its keep areas and its components as kernel bodies, in
//! the board frame of [`crate::placement`] (board coordinates in x, y; the board from z = 0 to
//! its thickness), in mm.
//!
//! - **Board**: the first BOARD_OUTLINE loop extruded by the thickness, less the later loops
//!   (cut-outs) and the DRILLED_HOLES (circles of their diameter). Named `Board [<name>]`
//!   (PCB7.2).
//! - **Keep areas** ([`crate::KeepArea`]): each loop set extruded by its height away from the
//!   board, from the top face up for TOP and from the bottom face down for BOTTOM (BOTH: one
//!   body each side); a [`MARKER`] thick sheet when there is no height. Other outlines are
//!   extruded by their thickness on their side.
//! - **Components**: the `.emp` outline extruded by the package height, then moved by
//!   [`crate::placement::placement_motion`]. Each package's body is made once and every
//!   instance is a moved copy. A package the library lacks gives a [`PLACEHOLDER`] box and a
//!   warning.

use std::collections::HashMap;

use cadrs_idf::{MountSide, Segment, Side};
use cadrs_kernel::{BodyId, Curve2, Extent, Kernel, KernelError, Loop, Motion, Plane, Profile, Region};
use nalgebra::{Point2, Point3};

use crate::board::{ItemId, KeepArea, KeepKind, PcbBoard};
use crate::colors::{BodyClass, Rgba, component_kind};
use crate::placement::placement_motion;

/// Thickness of a keep area with no height (mm).
pub const MARKER: f64 = 0.1;
/// Size of the box shown for a component whose package is missing (mm: x, y, height).
pub const PLACEHOLDER: [f64; 3] = [2.0, 2.0, 1.0];

/// A converted IDF loop as exact kernel curves (lines, arcs from the included angle, circles).
/// Zero-length segments are dropped.
pub fn idf_loop_to_kernel(l: &cadrs_idf::Loop) -> Loop {
    let p = |q: [f64; 2]| Point2::new(q[0], q[1]);
    let curves = l
        .segments()
        .filter_map(|s| match s {
            Segment::Line { start, end } => ((start[0] - end[0]).hypot(start[1] - end[1]) > 1e-12)
                .then(|| Curve2::Line { a: p(start), b: p(end), source: None }),
            Segment::Arc { start, center, radius, sweep, .. } => (radius > 1e-12).then(|| Curve2::Arc {
                center: p(center),
                radius,
                start_angle: (start[1] - center[1]).atan2(start[0] - center[0]),
                sweep: sweep.to_radians(),
                source: None,
            }),
            Segment::Circle { center, radius } => (radius > 1e-12).then(|| Curve2::Circle { center: p(center), radius, source: None }),
        })
        .collect();
    Loop { curves }
}

/// A body to make from a planar profile in the board frame: `loops` (the first the outer
/// boundary, the rest holes in it) on the plane z = `z0`, extruded by `depth` along +z (down
/// when negative).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyPlan {
    pub name: String,
    pub class: BodyClass,
    pub item: Option<ItemId>,
    pub loops: Vec<cadrs_idf::Loop>,
    pub z0: f64,
    pub depth: f64,
}

fn circle_loop(label: u32, x: f64, y: f64, r: f64) -> cadrs_idf::Loop {
    cadrs_idf::Loop::circle(label, x, y, r)
}

/// The board's plan: outline, cut-outs and drilled holes.
pub fn board_plan(pcb: &PcbBoard) -> Option<BodyPlan> {
    let o = pcb.board.outline.as_ref()?;
    let mut loops = o.loops.clone();
    if loops.is_empty() {
        return None;
    }
    for h in &pcb.board.holes {
        loops.push(circle_loop(1, h.x, h.y, h.dia / 2.0));
    }
    Some(BodyPlan {
        name: format!("Board [{}]", pcb.name()),
        class: BodyClass::Board,
        item: None,
        loops,
        z0: 0.0,
        depth: pcb.thickness(),
    })
}

/// The plans of one keep area (two for a BOTH area). `n` is its number among the areas of its
/// kind (1-based), for the name.
pub fn keep_plans(k: &KeepArea, n: usize, thickness: f64) -> Vec<BodyPlan> {
    let h = k.height.unwrap_or(MARKER);
    let class = if k.kind.is_keepout() {
        BodyClass::KeepOut
    } else if k.kind.is_keepin() {
        BodyClass::KeepIn
    } else {
        BodyClass::Other
    };
    let base = match k.kind {
        KeepKind::PlaceRegion if !k.label.is_empty() => format!("Keep-in {}", k.label),
        KeepKind::OtherOutline if !k.label.is_empty() => format!("Other outline {}", k.label),
        _ => format!("{} {n}", k.kind.label()),
    };
    let sides: &[(Side, &str)] = match k.side {
        Side::Top => &[(Side::Top, "")],
        Side::Bottom => &[(Side::Bottom, "")],
        Side::Both => &[(Side::Top, " (top)"), (Side::Bottom, " (bottom)")],
    };
    sides
        .iter()
        .map(|(side, suffix)| BodyPlan {
            name: format!("{base}{suffix}"),
            class,
            item: Some(k.id),
            loops: k.loops.clone(),
            z0: if *side == Side::Top { thickness } else { 0.0 },
            depth: if *side == Side::Top { h } else { -h },
        })
        .collect()
}

/// Every keep area's plans, in [`PcbBoard::keep_areas`] order, numbered per kind.
pub fn all_keep_plans(pcb: &PcbBoard, filter: impl Fn(&KeepArea) -> bool) -> Vec<BodyPlan> {
    let mut counts: HashMap<KeepKind, usize> = HashMap::new();
    let t = pcb.thickness();
    let mut out = Vec::new();
    for k in pcb.keep_areas() {
        let n = counts.entry(k.kind).or_default();
        *n += 1;
        if filter(&k) {
            out.extend(keep_plans(&k, *n, t));
        }
    }
    out
}

/// The name of a component's body: "U1 QFP100_600MIL".
pub fn component_name(p: &cadrs_idf::Placement) -> String {
    format!("{} {}", p.refdes, p.package)
}

/// A component's plan in board coordinates (the placed outline, extruded from the mounting
/// face away from the board); a [`PLACEHOLDER`] box if its package is missing.
pub fn component_plan(pcb: &PcbBoard, id: ItemId, p: &cadrs_idf::Placement) -> BodyPlan {
    let t = pcb.thickness();
    let (pkg, class) = match find_package(pcb, p) {
        Some(k) => (k.clone(), BodyClass::Component(component_kind(&p.package))),
        None => (placeholder_package(p), BodyClass::Placeholder),
    };
    let h = pkg.height.max(MARKER);
    let (z0, depth) = match p.side {
        MountSide::Top => (t + p.mount_offset, h),
        MountSide::Bottom => (-p.mount_offset, -h),
    };
    BodyPlan { name: component_name(p), class, item: Some(id), loops: p.place_loops(&pkg, cadrs_idf::Units::Mm), z0, depth }
}

/// The [`PLACEHOLDER`] box as a package, centred on its origin.
pub fn placeholder_package(p: &cadrs_idf::Placement) -> cadrs_idf::Package {
    let [w, d, h] = PLACEHOLDER;
    cadrs_idf::Package {
        kind: cadrs_idf::PackageKind::Electrical,
        name: p.package.clone(),
        part_number: p.part_number.clone(),
        units: cadrs_idf::Units::Mm,
        height: h,
        loops: vec![cadrs_idf::Loop::rect(0, -w / 2.0, -d / 2.0, w / 2.0, d / 2.0)],
        props: vec![],
    }
}

/// The package of a placement: by name and part number, else by name alone.
pub fn find_package<'a>(pcb: &'a PcbBoard, p: &cadrs_idf::Placement) -> Option<&'a cadrs_idf::Package> {
    pcb.library.package(&p.package, &p.part_number).or_else(|| pcb.library.packages.iter().find(|k| k.name == p.package))
}

/// One body of a [`BoardGeometry`].
#[derive(Clone, Debug, PartialEq)]
pub struct PcbBody {
    pub name: String,
    pub class: BodyClass,
    pub color: Rgba,
    pub body: BodyId,
    /// The component or keep area it shows (`None` for the board).
    pub item: Option<ItemId>,
    /// Components: the motion from the package frame to the board frame (its instance
    /// transform).
    pub motion: Option<Motion>,
}

/// The bodies of a board, in the board frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardGeometry {
    pub bodies: Vec<PcbBody>,
    /// Missing packages and the like; the geometry is still made.
    pub warnings: Vec<String>,
}

impl BoardGeometry {
    pub fn board(&self) -> Option<&PcbBody> {
        self.bodies.iter().find(|b| b.class == BodyClass::Board)
    }

    pub fn components(&self) -> impl Iterator<Item = &PcbBody> {
        self.bodies.iter().filter(|b| b.class.is_component())
    }

    pub fn keeps(&self) -> impl Iterator<Item = &PcbBody> {
        self.bodies.iter().filter(|b| b.class.is_keep())
    }

    pub fn named(&self, name: &str) -> Option<&PcbBody> {
        self.bodies.iter().find(|b| b.name == name)
    }

    /// Releases every body.
    pub fn release(self, kernel: &mut dyn Kernel) {
        for b in self.bodies {
            kernel.release(b.body);
        }
    }
}

fn extrude_loops(kernel: &mut dyn Kernel, loops: &[cadrs_idf::Loop], z0: f64, depth: f64) -> Result<BodyId, KernelError> {
    let plane = Plane { origin: Point3::new(0.0, 0.0, z0), ..Plane::top() };
    let mut kl = loops.iter().map(idf_loop_to_kernel).filter(|l| !l.curves.is_empty());
    let outer = kl.next().ok_or_else(|| KernelError::InvalidProfile("the outline is empty".into()))?;
    let holes: Vec<Loop> = kl.collect();
    let region = Region { outer: outer.clone(), holes: holes.clone(), source: None };
    match kernel.extrude(&Profile::new(plane, vec![region]), Extent::Blind(depth)) {
        Ok(r) => first_body(r.bodies),
        // Cut-outs that touch the outline or each other can't be holes of one face: cut them
        // one by one.
        Err(_) if !holes.is_empty() => {
            let one = |k: &mut dyn Kernel, l: Loop, d: f64, z: f64| -> Result<BodyId, KernelError> {
                let plane = Plane { origin: Point3::new(0.0, 0.0, z), ..Plane::top() };
                first_body(k.extrude(&Profile::new(plane, vec![Region { outer: l, holes: vec![], source: None }]), Extent::Blind(d))?.bodies)
            };
            let mut body = one(kernel, outer, depth, z0)?;
            let s = depth.signum();
            for h in holes {
                let tool = one(kernel, h, depth + 2.0 * s, z0 - s)?;
                let r = kernel.boolean(cadrs_kernel::BoolOp::Subtract, body, &[tool])?;
                kernel.release(tool);
                kernel.release(body);
                body = first_body(r.bodies)?;
            }
            Ok(body)
        }
        Err(e) => Err(e),
    }
}

fn first_body(bodies: Vec<BodyId>) -> Result<BodyId, KernelError> {
    bodies.into_iter().next().ok_or_else(|| KernelError::OperationFailed("no body was made".into()))
}

/// A package's body in the package frame: its outline extruded from z = 0 up by its height
/// (at least [`MARKER`]).
pub fn package_body(kernel: &mut dyn Kernel, pkg: &cadrs_idf::Package) -> Result<BodyId, KernelError> {
    extrude_loops(kernel, &pkg.loops, 0.0, pkg.height.max(MARKER))
}

/// Makes a plan's body.
pub fn plan_body(kernel: &mut dyn Kernel, plan: &BodyPlan) -> Result<PcbBody, KernelError> {
    let body = extrude_loops(kernel, &plan.loops, plan.z0, plan.depth)?;
    Ok(PcbBody { name: plan.name.clone(), class: plan.class, color: plan.class.color(), body, item: plan.item, motion: None })
}

/// The whole board as kernel bodies: the board, every keep area, every component (see the
/// module docs). Kernel failures on a keep area or component are warnings; a board that can't
/// be made is an error.
pub fn board_geometry(kernel: &mut dyn Kernel, pcb: &PcbBoard) -> Result<BoardGeometry, KernelError> {
    let mut g = BoardGeometry::default();
    let t = pcb.thickness();
    if let Some(plan) = board_plan(pcb) {
        g.bodies.push(plan_body(kernel, &plan)?);
    } else {
        g.warnings.push("The board has no outline".into());
    }
    for plan in all_keep_plans(pcb, |_| true) {
        match plan_body(kernel, &plan) {
            Ok(b) => g.bodies.push(b),
            Err(e) => g.warnings.push(format!("{}: {e}", plan.name)),
        }
    }
    // One body per package, in the package frame; instances are moved copies.
    let mut protos: HashMap<Option<(String, String)>, BodyId> = HashMap::new();
    for (id, p) in pcb.components() {
        let pkg = find_package(pcb, p);
        let key = pkg.map(|k| (k.name.clone(), k.part_number.clone()));
        let class = if pkg.is_some() { BodyClass::Component(component_kind(&p.package)) } else { BodyClass::Placeholder };
        if pkg.is_none() {
            g.warnings.push(format!("{}: package {} ({}) isn't in the library; shown as a placeholder box", p.refdes, p.package, p.part_number));
        }
        let proto = match protos.get(&key) {
            Some(b) => *b,
            None => {
                let k = pkg.cloned().unwrap_or_else(|| placeholder_package(p));
                let made = extrude_loops(kernel, &k.loops, 0.0, k.height.max(MARKER));
                match made {
                    Ok(b) => {
                        protos.insert(key.clone(), b);
                        b
                    }
                    Err(e) => {
                        g.warnings.push(format!("{}: {e}", p.refdes));
                        continue;
                    }
                }
            }
        };
        let motion = placement_motion(p, t);
        match kernel.transform_motion(proto, &motion).and_then(|r| first_body(r.bodies)) {
            Ok(body) => g.bodies.push(PcbBody {
                name: component_name(p),
                class,
                color: class.color(),
                body,
                item: Some(id),
                motion: Some(motion),
            }),
            Err(e) => g.warnings.push(format!("{}: {e}", p.refdes)),
        }
    }
    for (_, b) in protos {
        kernel.release(b);
    }
    Ok(g)
}
