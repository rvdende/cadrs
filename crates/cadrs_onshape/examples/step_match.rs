//! How closely an import's parts match Onshape's own STEP export of them, exactly: each part
//! is exported to STEP, both files are read back, and their volumes, areas, centroids and
//! face counts are compared, with the largest distance from one body's vertices, edge points
//! and face points to the other body's exact faces (both ways).
//!
//! `cargo run -r -p cadrs_onshape --example step_match -- <file.ron> <folder of "<base> - <part>.step"> [part name…]`

use cadrs_core::document::Feature;
use cadrs_core::export::StepRequest;
use cadrs_kernel::{BodyId, DefaultKernel, DistanceTarget, Kernel, Tessellation};
use nalgebra::Point3;

/// A body's mass properties summed over the bodies a file holds.
fn mass(k: &DefaultKernel, bodies: &[BodyId]) -> (f64, f64, Point3<f64>, usize) {
    let (mut v, mut a, mut c, mut faces) = (0.0, 0.0, nalgebra::Vector3::zeros(), 0);
    for b in bodies {
        if let Ok(m) = k.mass_properties(*b) {
            v += m.volume;
            a += m.surface_area;
            c += m.center_of_mass.coords * m.volume;
        }
        faces += k.face_count(*b).unwrap_or(0);
    }
    (v, a, Point3::from(c / v.max(1e-300)), faces)
}

/// How far apart two faces' surfaces are (mm; directions as the gap they open over 1 m): the
/// planes' normals and offsets, or the axes' directions and positions and the radii. `None`
/// for surfaces of different kinds, or ones with neither a plane nor an axis.
fn surface_gap(a: &cadrs_kernel::FaceInfo, b: &cadrs_kernel::FaceInfo) -> Option<f64> {
    if a.kind != b.kind {
        return None;
    }
    let lever = 1000.0;
    if let (Some(p), Some(q)) = (&a.plane, &b.plane) {
        let (n, m) = (p.normal.into_inner(), q.normal.into_inner());
        let turn = n.cross(&m).norm() * lever;
        return Some(turn.max((q.origin - p.origin).dot(&n).abs()));
    }
    let (p, q) = (a.axis.as_ref()?, b.axis.as_ref()?);
    let (d, e) = (p.dir.into_inner(), q.dir.into_inner());
    let turn = d.cross(&e).norm() * lever;
    let off = q.origin - p.origin;
    // A cylinder's or cone's axis is a line; a sphere's or torus's centre is a point.
    let apart = match a.kind {
        cadrs_kernel::SurfaceKind::Cylinder => (off - d * off.dot(&d)).norm(),
        _ => off.norm(),
    };
    let radius = match (a.radius, b.radius) {
        (Some(r), Some(s)) => (r - s).abs(),
        _ => 0.0,
    };
    let minor = match (a.fillet_radius, b.fillet_radius) {
        (Some(r), Some(s)) => (r - s).abs(),
        _ => 0.0,
    };
    Some(turn.max(apart).max(radius).max(minor))
}

/// Each of `ours`' faces against the face of `theirs` whose surface is nearest it: the
/// largest gap, and how many have no such face (a spline surface, or no face of that kind).
fn surfaces(k: &DefaultKernel, ours: &[BodyId], theirs: &[BodyId]) -> (f64, usize) {
    let faces = |bs: &[BodyId]| -> Vec<cadrs_kernel::FaceInfo> {
        bs.iter()
            .flat_map(|b| k.faces(*b).unwrap_or_default())
            .collect()
    };
    let (a, b) = (faces(ours), faces(theirs));
    let (mut worst, mut loose) = (0.0f64, 0);
    for f in &a {
        match b
            .iter()
            .filter_map(|g| surface_gap(f, g))
            .min_by(|x, y| x.total_cmp(y))
        {
            Some(g) => worst = worst.max(g),
            None => loose += 1,
        }
    }
    (worst, loose)
}

/// The accuracy a STEP file states for itself (its `UNCERTAINTY_MEASURE_WITH_UNIT`), in mm:
/// Onshape writes 1e-8 m for exact geometry and 2e-5 m for a body whose edges it had to fit
/// (the curves where curved faces meet).
fn stated_uncertainty(text: &str) -> Option<f64> {
    let at = text.find("UNCERTAINTY_MEASURE_WITH_UNIT")?;
    let rest = &text[at..];
    let from = rest.find("LENGTH_MEASURE(")? + "LENGTH_MEASURE(".len();
    let to = from + rest[from..].find(')')?;
    let value: f64 = rest[from..to].trim().replace("E", "e").parse().ok()?;
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let metres = !squeezed.contains("SI_UNIT(.MILLI.,.METRE.)");
    Some(if metres { value * 1000.0 } else { value })
}

/// Points of the bodies: their vertices, their edges' middles, and their mesh's points.
fn points(k: &DefaultKernel, bodies: &[BodyId]) -> Vec<Point3<f64>> {
    let mut out = Vec::new();
    for b in bodies {
        if let Ok(vs) = k.vertices(*b) {
            out.extend(vs.iter().map(|v| v.point));
        }
        if let Ok(es) = k.edges(*b) {
            out.extend(es.iter().map(|e| e.mid));
        }
        if let Ok(m) = k.tessellate(*b, Tessellation::default()) {
            let step = (m.positions.len() / 400).max(1);
            // (Only mesh points that are on the body: the mesher now and then puts one a
            // little outside the tip of a sliver of a face, which says nothing of the body.)
            out.extend(m.positions.iter().step_by(step).copied().filter(|p| {
                k.min_distance(&DistanceTarget::Point(*p), &DistanceTarget::Body(*b))
                    .is_ok_and(|r| r.distance <= 1e-7)
            }));
        }
    }
    out
}

/// The largest distance from `pts` to the bodies' faces, and where.
fn furthest(k: &DefaultKernel, pts: &[Point3<f64>], bodies: &[BodyId]) -> (f64, Point3<f64>) {
    let mut worst = (0.0, Point3::origin());
    for p in pts {
        let d = bodies
            .iter()
            .filter_map(|b| {
                k.min_distance(&DistanceTarget::Point(*p), &DistanceTarget::Body(*b))
                    .ok()
            })
            .map(|r| r.distance)
            .fold(f64::INFINITY, f64::min);
        if d.is_finite() && d > worst.0 {
            worst = (d, *p);
        }
    }
    worst
}

/// One of Onshape's parts: its name, bodies, volume and centroid, and whether one of ours
/// has been paired with it.
type Theirs = (String, Vec<BodyId>, f64, Point3<f64>, bool);

fn main() {
    let mut args = std::env::args().skip(1);
    let dump = args.next().expect("a dumped feature list");
    // `--read <file.step>`: what the file reads as (its bodies' solids, volumes and faces).
    if dump == "--read" {
        let mut k = DefaultKernel::new();
        let path = args.next().expect("a STEP file");
        match k.import_step(&std::fs::read(&path).expect("a readable STEP file")) {
            Ok(bodies) => {
                for b in bodies {
                    println!(
                        "body: {:?} solids, volume {:?}",
                        k.solid_count(b),
                        k.mass_properties(b).map(|m| m.volume)
                    );
                    for f in k.faces(b).unwrap_or_default() {
                        println!("   {:?} area {:.6} r {:?}", f.kind, f.area, f.radius);
                    }
                    // Its mesh against its box: a face meshed outside its bounds shows here.
                    if let (Ok(m), Ok(bb)) =
                        (k.tessellate(b, Tessellation::default()), k.bounding_box(b))
                    {
                        let mut per_face: std::collections::BTreeMap<u32, f64> = Default::default();
                        for (t, f) in m.indices.iter().zip(&m.triangle_faces) {
                            for i in t {
                                let p = m.positions[*i as usize];
                                let out = (0..3)
                                    .map(|a| (bb.min[a] - p[a]).max(p[a] - bb.max[a]).max(0.0))
                                    .fold(0.0, f64::max);
                                let e = per_face.entry(f.0 as u32).or_default();
                                *e = e.max(out);
                            }
                        }
                        println!("   mesh points outside the box, by face: {per_face:?}");
                        // `STEP_NEAR=x,y,z`: how near each face's mesh comes to that point,
                        // and how far the kernel says the body is from it.
                        if let Some(at) = std::env::var("STEP_NEAR").ok().and_then(|v| {
                            let c: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                            (c.len() == 3).then(|| Point3::new(c[0], c[1], c[2]))
                        }) {
                            let mut near: std::collections::BTreeMap<u32, f64> = Default::default();
                            for (t, f) in m.indices.iter().zip(&m.triangle_faces) {
                                for i in t {
                                    let d = (m.positions[*i as usize] - at).norm();
                                    let e = near.entry(f.0 as u32).or_insert(f64::MAX);
                                    *e = e.min(d);
                                }
                            }
                            println!(
                                "   nearest mesh point by face: {near:.4?}; the body is {:?} away",
                                k.min_distance(
                                    &DistanceTarget::Point(at),
                                    &DistanceTarget::Body(b)
                                )
                                .map(|r| r.distance)
                            );
                        }
                        // Each face's mesh area against its exact area: a face meshed over
                        // the wrong part of its surface shows here.
                        let mut area: std::collections::BTreeMap<u32, f64> = Default::default();
                        for (t, f) in m.indices.iter().zip(&m.triangle_faces) {
                            let [a, b, c] = t.map(|i| m.positions[i as usize]);
                            *area.entry(f.0 as u32).or_default() +=
                                (b - a).cross(&(c - a)).norm() / 2.0;
                        }
                        for f in k.faces(b).unwrap_or_default() {
                            let meshed = area.get(&(f.id.0 as u32)).copied().unwrap_or(0.0);
                            println!(
                                "   face {}: exact {:.4}, meshed {:.4} ({:+.2}%)",
                                f.id.0,
                                f.area,
                                meshed,
                                100.0 * (meshed - f.area) / f.area.max(1e-12)
                            );
                        }
                    }
                }
            }
            Err(e) => println!("doesn't read: {e}"),
        }
        return;
    }
    let folder = std::path::PathBuf::from(args.next().expect("a folder of STEP files"));
    let only: Vec<String> = args.collect();
    let features: Vec<Feature> =
        ron::from_str(&std::fs::read_to_string(&dump).expect("readable")).expect("a feature list");
    let b = cadrs_core::rebuild::build(&features);
    let parts: Vec<(cadrs_core::ids::PartId, String)> = b
        .parts
        .iter()
        .filter(|p| only.is_empty() || only.contains(&p.name))
        // (A composite part is its members, each compared as itself.)
        .filter(|p| !b.composites.iter().any(|c| c.part == p.id))
        .map(|p| (p.id, p.name.clone()))
        .collect();
    // `STEP_MATCH_MESH=1`: each face's shown mesh against its exact area, as the app draws it.
    if std::env::var_os("STEP_MATCH_MESH").is_some() {
        for p in b.parts.iter().filter(|p| only.contains(&p.name)) {
            let s = &p.solid;
            for (i, f) in s.faces.iter().enumerate() {
                let mut meshed = 0.0;
                for t in f.first_triangle..f.first_triangle + f.triangle_count {
                    let v = |k: usize| {
                        let q = s.positions[s.indices[3 * t + k] as usize];
                        nalgebra::Vector3::new(q[0] as f64, q[1] as f64, q[2] as f64)
                    };
                    meshed += (v(1) - v(0)).cross(&(v(2) - v(0))).norm() / 2.0;
                }
                let exact = f.area.unwrap_or(0.0);
                println!(
                    "{} face {i}: exact {exact:.4}, meshed {meshed:.4} ({:+.2}%)",
                    p.name,
                    100.0 * (meshed - exact) / exact.max(1e-12)
                );
            }
        }
        return;
    }
    let files = cadrs_core::rebuild::export_step(
        features,
        StepRequest {
            parts: parts.clone(),
            y_up: false,
            individual: true,
        },
    )
    .wait()
    .expect("the parts export");
    // Onshape's files by part name ("<base> - <part>.step").
    let theirs: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&folder)
        .expect("a readable folder")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "step"))
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_string();
            let name = stem.rsplit(" - ").next()?.to_string();
            Some((name, p))
        })
        .collect();
    let (mut worst_all, mut compared, mut bad) = (0.0f64, 0, Vec::new());
    let (mut worst_surface, mut off_surface) = (0.0f64, Vec::<String>::new());
    let mut within_stated = 0usize;
    println!(
        "{:<10} {:<10} {:>14} {:>10} {:>10} {:>10} {:>7} {:>10} {:>11} {:>11} {:>8}",
        "part",
        "Onshape's",
        "volume mm³",
        "Δvolume",
        "Δarea",
        "Δcentroid",
        "faces",
        "surfaces",
        "ours→theirs",
        "theirs→ours",
        "file ±"
    );
    let mut k = DefaultKernel::new();
    // Onshape's bodies, and which of our parts each is: the one whose centroid is nearest
    // (the parts are numbered differently).
    let mut their_bodies: Vec<Theirs> = Vec::new();
    let mut uncertainty: Vec<(String, f64)> = Vec::new();
    for (name, path) in &theirs {
        let bytes = std::fs::read(path).expect("a readable STEP file");
        let stated = stated_uncertainty(&String::from_utf8_lossy(&bytes)).unwrap_or(0.0);
        uncertainty.push((name.clone(), stated));
        match k.import_step(&bytes) {
            Ok(b) => {
                let (v, _, c, _) = mass(&k, &b);
                if v <= 0.0 {
                    println!("{name:<20} Onshape's STEP reads as no solid");
                    continue;
                }
                their_bodies.push((name.clone(), b, v, c, false));
            }
            Err(e) => println!("{name:<20} Onshape's STEP doesn't read: {e}"),
        }
    }
    for (i, (_, name)) in parts.iter().enumerate() {
        let Some(file) = files.get(i) else { continue };
        // `STEP_MATCH_KEEP=<dir>`: our STEP files, kept to look at.
        if let Some(dir) = std::env::var_os("STEP_MATCH_KEEP") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join(format!("{name}.step")),
                &file.bytes,
            );
        }
        let ours = match k.import_step(&file.bytes) {
            Ok(b) => b,
            Err(e) => {
                println!("{name:<20} our STEP doesn't read back: {e}");
                bad.push(name.clone());
                continue;
            }
        };
        let (v1, _, c1, _) = mass(&k, &ours);
        let Some(at) = their_bodies
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.4)
            .map(|(j, t)| (j, (t.3 - c1).norm() + (t.2 - v1).abs().cbrt()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, cost)| *cost < 25.0)
            .map(|(j, _)| j)
        else {
            println!("{name:<20} has no part of Onshape's left to compare with");
            bad.push(name.clone());
            continue;
        };
        their_bodies[at].4 = true;
        let (their_name, them) = (their_bodies[at].0.clone(), their_bodies[at].1.clone());
        // `STEP_MATCH_WALL=<thickness>`: how far each body's wall is from that thickness, along
        // the normals of its spline faces (a tube swept along a curve no kernel sweeps exactly:
        // which of the two is nearer the true tube).
        if let Some(want) = std::env::var("STEP_MATCH_WALL")
            .ok()
            .and_then(|w| w.parse::<f64>().ok())
        {
            for (who, bodies) in [("ours", &ours), ("theirs", &them)] {
                for b in bodies.iter() {
                    let spline: Vec<cadrs_kernel::FaceId> = k
                        .faces(*b)
                        .unwrap_or_default()
                        .iter()
                        .filter(|f| f.kind == cadrs_kernel::SurfaceKind::Other)
                        .map(|f| f.id)
                        .collect();
                    let Ok(m) = k.tessellate(*b, Tessellation::default()) else {
                        continue;
                    };
                    // Each mesh point's faces.
                    let mut on: Vec<Option<cadrs_kernel::FaceId>> = vec![None; m.positions.len()];
                    for (t, f) in m.indices.iter().zip(&m.triangle_faces) {
                        for i in t {
                            on[*i as usize] = Some(*f);
                        }
                    }
                    let (mut worst, mut sum, mut n) = (0.0f64, 0.0, 0usize);
                    for (i, p) in m.positions.iter().enumerate() {
                        if !on[i].is_some_and(|f| spline.contains(&f)) {
                            continue;
                        }
                        let dir = -m.normals[i];
                        let Ok(hits) = k.ray_hits(*b, *p, dir) else {
                            continue;
                        };
                        let Some(t) = hits.iter().map(|h| h.t).find(|t| *t > 0.5 * want) else {
                            continue;
                        };
                        if (t - want).abs() < 0.1 * want {
                            worst = worst.max((t - want).abs());
                            sum += (t - want).abs();
                            n += 1;
                        }
                    }
                    println!(
                        "   {who:<6} wall: {n} points, off {want} by at most {worst:.3e} mm, {:.3e} on average",
                        sum / n.max(1) as f64
                    );
                }
            }
        }
        if std::env::var_os("STEP_MATCH_FACES").is_some() {
            for (who, bodies) in [("ours", &ours), ("theirs", &them)] {
                for b in bodies.iter() {
                    for f in k.faces(*b).unwrap_or_default() {
                        println!(
                            "   {who:<6} {:?} area {:.6} r {:?} axis {:?} plane {:?}",
                            f.kind,
                            f.area,
                            f.radius,
                            f.axis.map(|a| (
                                a.origin.coords.as_slice().to_vec(),
                                a.dir.as_slice().to_vec()
                            )),
                            f.plane.map(|p| (
                                p.origin.coords.as_slice().to_vec(),
                                p.normal.as_slice().to_vec()
                            )),
                        );
                    }
                    for e in k.edges(*b).unwrap_or_default() {
                        println!(
                            "   {who:<6} edge {:?} length {:.6} mid {:?}",
                            e.curve,
                            e.length,
                            e.mid.coords.as_slice()
                        );
                    }
                }
            }
        }
        let (v1, a1, c1, f1) = mass(&k, &ours);
        let (v2, a2, c2, f2) = mass(&k, &them);
        let to_theirs = furthest(&k, &points(&k, &ours), &them);
        let to_ours = furthest(&k, &points(&k, &them), &ours);
        let worst = to_theirs.0.max(to_ours.0);
        worst_all = worst_all.max(worst);
        compared += 1;
        let (gap, loose) = surfaces(&k, &ours, &them);
        let (gap_back, loose_back) = surfaces(&k, &them, &ours);
        let (gap, loose) = (gap.max(gap_back), loose.max(loose_back));
        worst_surface = worst_surface.max(gap);
        let surf = if loose == 0 {
            format!("{gap:.1e}")
        } else {
            format!("{gap:.1e}+{loose}")
        };
        if gap > 1e-6 || loose > 0 {
            off_surface.push(name.clone());
        }
        let faces = if f1 == f2 {
            format!("{f1}")
        } else {
            format!("{f1}/{f2}")
        };
        let stated = uncertainty
            .iter()
            .find(|(n, _)| *n == their_name)
            .map_or(0.0, |u| u.1);
        // (Beside the file's own accuracy, what the comparison's own arithmetic can tell.)
        let within = worst <= stated.max(1e-6) && f1 == f2;
        let flag = if within {
            ""
        } else {
            "  <-- beyond the file's accuracy"
        };
        within_stated += usize::from(within);
        println!(
            "{name:<10} {their_name:<10} {v2:>14.4} {:>10.2e} {:>10.2e} {:>10.2e} {faces:>7} {surf:>10} {:>11.2e} {:>11.2e} {stated:>8.0e}{flag}",
            v1 - v2,
            a1 - a2,
            (c1 - c2).norm(),
            to_theirs.0,
            to_ours.0
        );
        if !within {
            let at = if to_theirs.0 >= to_ours.0 {
                to_theirs.1
            } else {
                to_ours.1
            };
            println!("     worst at ({:.4}, {:.4}, {:.4})", at.x, at.y, at.z);
            bad.push(name.clone());
        }
    }
    for t in their_bodies.iter().filter(|t| !t.4) {
        println!("Onshape's {} has no part of ours", t.0);
    }
    println!(
        "surfaces: largest gap {worst_surface:.3e} mm; {} parts with a surface over 1e-6 mm off or unmatched: {}",
        off_surface.len(),
        off_surface.join(", ")
    );
    println!(
        "{compared} parts compared, {within_stated} within the accuracy Onshape's file states (and 1e-6 mm); largest distance {worst_all:.3e} mm; {} beyond it: {}",
        bad.len(),
        bad.join(", ")
    );
}
