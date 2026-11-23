//! Ported from springrep's tests/step_assemblies.rs (MIT,
//! https://github.com/springcraft-ai/springrep): several parts in one STEP file, each written
//! once and placed by its instances, and what a read says of each body: its part's name, the
//! uses that place it, and its colour. springrep's body colour is cadrs's face colours (every
//! face the part's colour); its per-body STEP ids are cadrs's occurrences' part indices.

#![allow(dead_code)]

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_kernel::exchange::{ExchangeFormat, ExportInstance, ImportedModel};
use cadrs_kernel::native::{NativeKernel, tess};
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

type K = NativeKernel;
type P3 = Point3<f64>;
type V3 = Vector3<f64>;

fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3::new(x, y, z)
}

/// springrep's `Profile::rect(center, w, h)`: the `w` × `h` rectangle centred on (`cx`, `cy`).
fn rect(cx: f64, cy: f64, w: f64, h: f64) -> Loop {
    Loop::rectangle(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0)
}

/// springrep's `Profile::circle(center, diameter)`: it takes a diameter.
fn circle(cx: f64, cy: f64, diameter: f64) -> Loop {
    Loop::circle([cx, cy], diameter / 2.0)
}

/// `outer` less `holes` on the plane z = `z` (springrep's `Frame::new((0, 0, z), Z, X)`),
/// extruded `h` up z.
fn extrude(k: &mut K, z: f64, outer: Loop, holes: Vec<Loop>, h: f64) -> BodyId {
    let p = Profile::with_holes(Plane::xy(z), outer, holes);
    k.extrude(&p, Extent::Blind(h)).unwrap().bodies[0]
}

fn boolean(k: &mut K, op: BoolOp, a: BodyId, b: BodyId) -> BodyId {
    let r = k.boolean(op, a, &[b]).unwrap();
    assert_eq!(r.bodies.len(), 1, "{op:?} made {} bodies", r.bodies.len());
    r.bodies[0]
}

fn union(k: &mut K, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Union, a, b)
}

fn difference(k: &mut K, a: BodyId, b: BodyId) -> BodyId {
    boolean(k, BoolOp::Subtract, a, b)
}

/// springrep's `unify`: faces and edges on one surface or curve merged.
fn unify(k: &mut K, b: BodyId) -> BodyId {
    k.simplify(b).unwrap().bodies[0]
}

fn valid(k: &K, b: BodyId) {
    let issues = k.get(b).unwrap().check();
    assert!(issues.is_empty(), "check: {issues:?}");
}

fn volume(k: &K, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

fn area(k: &K, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().surface_area
}

/// The body's mesh (springrep's `MeshOptions { linear, angular }`) is closed.
fn watertight(k: &K, b: BodyId, linear: f64, angular: f64) -> bool {
    let q = Tessellation {
        deflection: linear,
        angle: angular,
    };
    tess::check_tri_mesh(&k.tessellate(b, q).unwrap()).is_watertight()
}

/// Each face with its outward normal at (the point of it nearest) its centre.
fn face_list(k: &K, b: BodyId) -> Vec<(FaceInfo, V3)> {
    k.faces(b)
        .unwrap()
        .into_iter()
        .map(|f| {
            let n = k.face_normals_at(b, f.id, &[f.center]).unwrap()[0];
            (f, n)
        })
        .collect()
}

/// The faces `pick` takes (given each face and its outward normal).
fn faces(k: &K, b: BodyId, pick: impl Fn(&FaceInfo, &V3) -> bool) -> Vec<FaceId> {
    face_list(k, b)
        .iter()
        .filter(|(f, n)| pick(f, n))
        .map(|(f, _)| f.id)
        .collect()
}

fn kinds(k: &K, b: BodyId, kind: SurfaceKind) -> usize {
    k.faces(b)
        .unwrap()
        .iter()
        .filter(|f| f.kind == kind)
        .count()
}

/// Planar faces on the plane `n`·x = `d`, facing either way.
fn plane(n: [f64; 3], d: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    let n = V3::from(n).normalize();
    move |f, m| {
        f.kind == SurfaceKind::Plane
            && m.cross(&n).norm() < 1e-9
            && (f.center.coords.dot(&n) - d).abs() < 1e-7
    }
}

/// Planar faces on the plane `n`·x = `d` whose outward normal is `n`.
fn facing(n: [f64; 3], d: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    let on = plane(n, d);
    let n = V3::from(n).normalize();
    move |f, m| on(f, m) && m.dot(&n) > 0.0
}

/// Cylindrical faces of radius `r` about an axis through (`x`, `y`) along z.
fn cylinder(x: f64, y: f64, r: f64) -> impl Fn(&FaceInfo, &V3) -> bool {
    move |f, _| {
        f.kind == SurfaceKind::Cylinder
            && f.radius.is_some_and(|q| (q - r).abs() < 1e-7)
            && f.axis.is_some_and(|a| {
                let d = a.dir.into_inner();
                let o = a.origin - p3(x, y, 0.0);
                d.cross(&V3::z()).norm() < 1e-9 && (o - d * o.dot(&d)).norm() < 1e-7
            })
    }
}

/// The edges between a face `a` takes and one `c` takes (springrep's pair of face names).
fn pair(
    k: &K,
    b: BodyId,
    a: impl Fn(&FaceInfo, &V3) -> bool,
    c: impl Fn(&FaceInfo, &V3) -> bool,
) -> Vec<EdgeId> {
    let (fa, fc) = (faces(k, b, a), faces(k, b, c));
    k.edges(b)
        .unwrap()
        .iter()
        .filter(|e| match e.faces {
            [Some(x), Some(y)] => {
                (fa.contains(&x) && fc.contains(&y)) || (fa.contains(&y) && fc.contains(&x))
            }
            _ => false,
        })
        .map(|e| e.id)
        .collect()
}

/// The edge nearest `p` (springrep's `measure::edge_at`).
fn edge_at(k: &K, b: BodyId, p: P3) -> EdgeId {
    k.edges(b)
        .unwrap()
        .iter()
        .map(|e| {
            let d = k
                .min_distance(&DistanceTarget::Point(p), &DistanceTarget::Edge(b, e.id))
                .unwrap()
                .distance;
            (d, e.id)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
        .1
}

fn block(k: &mut K) -> BodyId {
    k.primitive(&Primitive::block(Point3::origin(), p3(10.0, 20.0, 30.0)))
        .unwrap()
        .bodies[0]
}

fn post(k: &mut K) -> BodyId {
    k.primitive(&Primitive::cylinder(
        Point3::origin(),
        Vector3::z(),
        5.0,
        10.0,
    ))
    .unwrap()
    .bodies[0]
}

/// springrep's RED, [0.8, 0.1, 0.1], as cadrs's 8-bit face colour.
const RED: [u8; 4] = [204, 26, 26, 255];

/// A block, and a post used twice: once stood 40 mm up, clear of the block, once laid along −y
/// at x = 50.
fn written(k: &mut K) -> Vec<u8> {
    let (block, post) = (block(k), post(k));
    let n = k.face_count(block).unwrap();
    k.set_face_colors(block, vec![Some(RED); n]);
    let parts = [(block, "block".to_string()), (post, "post".to_string())];
    let instances = [
        ExportInstance {
            part: 0,
            placement: Motion::identity(),
            name: "base".into(),
        },
        ExportInstance {
            part: 1,
            placement: Motion::translation(Vector3::new(0.0, 0.0, 40.0)),
            name: "post-a".into(),
        },
        ExportInstance {
            part: 1,
            placement: Motion::rotation(&Axis::x(), FRAC_PI_2)
                .then(&Motion::translation(Vector3::new(50.0, 0.0, 0.0))),
            name: "post-b".into(),
        },
    ];
    k.export_model(ExchangeFormat::Step, "stand", &parts, &instances)
        .unwrap()
}

fn read(k: &mut K, bytes: &[u8]) -> ImportedModel {
    k.import_model(ExchangeFormat::Step, bytes).unwrap()
}

#[test]
fn an_assembly_reads_back_with_each_instance_where_it_was_placed() {
    let mut k = K::new();
    let text = written(&mut k);
    // The post's geometry is in the file once, however often it is used.
    let s = String::from_utf8_lossy(&text);
    assert_eq!(s.matches("MANIFOLD_SOLID_BREP").count(), 2);
    let m = read(&mut k, &text);
    for p in &m.parts {
        valid(&k, p.body);
    }
    // 10 · 20 · 30 and twice π · 5² · 10.
    let want = 6000.0 + 2.0 * PI * 250.0;
    let got: f64 = m
        .occurrences
        .iter()
        .map(|o| volume(&k, m.parts[o.part].body))
        .sum();
    assert!((got - want).abs() < 1e-6 * want, "{got}");
    let named: Vec<(&str, &str)> = m
        .occurrences
        .iter()
        .map(|o| (m.parts[o.part].name.as_str(), o.name.as_str()))
        .collect();
    assert_eq!(
        named,
        [("block", "base"), ("post", "post-a"), ("post", "post-b")]
    );
    // One part, used twice.
    assert_eq!(m.parts.len(), 2);
    assert_eq!(m.occurrences[1].part, m.occurrences[2].part);
    let boxed = |k: &mut K, i: usize| {
        let o = &m.occurrences[i];
        let placed = k
            .transform_motion(m.parts[o.part].body, &o.placement)
            .unwrap();
        k.bounding_box(placed.bodies[0]).unwrap()
    };
    let near = |a: P3, e: [f64; 3]| (a - P3::from(e)).norm() < 1e-9;
    let b1 = boxed(&mut k, 1);
    assert!(
        near(b1.min, [-5.0, -5.0, 40.0]) && near(b1.max, [5.0, 5.0, 50.0]),
        "{b1:?}"
    );
    // A quarter turn about x takes z to −y: the post lies from y = 0 to −10.
    let b2 = boxed(&mut k, 2);
    assert!(
        near(b2.min, [45.0, -10.0, -5.0]) && near(b2.max, [55.0, 0.0, 5.0]),
        "{b2:?}"
    );
    // springrep: nothing of the file rejected (cadrs's import has no such list).
}

#[test]
fn a_parts_colour_is_its_bodies_colour_wherever_they_are_placed() {
    let mut k = K::new();
    let text = written(&mut k);
    let m = read(&mut k, &text);
    let colours: Vec<Vec<Option<[u8; 4]>>> =
        m.parts.iter().map(|p| k.face_colors(p.body)).collect();
    // The block's faces all red, the post's none.
    assert!(
        !colours[0].is_empty() && colours[0].iter().all(|c| *c == Some(RED)),
        "{colours:?}"
    );
    assert!(colours[1].iter().all(Option::is_none), "{colours:?}");
    // No occurrence carries a colour of its own.
    assert!(m.occurrences.iter().all(|o| o.color.is_none()));
}

#[test]
fn a_face_styled_apart_from_its_body_has_its_own_colour() {
    // As another system writes one face green: a style over the body's, naming the face.
    let mut k = K::new();
    let text = String::from_utf8(written(&mut k)).unwrap();
    let face: String = text
        .lines()
        .find(|l| l.contains("=ADVANCED_FACE("))
        .and_then(|l| l.split('=').next())
        .unwrap()
        .trim()
        .to_string();
    let extra = format!(
        "#900001=DRAUGHTING_PRE_DEFINED_COLOUR('green');\n\
         #900002=FILL_AREA_STYLE_COLOUR('',#900001);\n\
         #900003=FILL_AREA_STYLE('',(#900002));\n\
         #900004=SURFACE_STYLE_FILL_AREA(#900003);\n\
         #900005=SURFACE_SIDE_STYLE('',(#900004));\n\
         #900006=SURFACE_STYLE_USAGE(.BOTH.,#900005);\n\
         #900007=PRESENTATION_STYLE_ASSIGNMENT((#900006));\n\
         #900008=STYLED_ITEM('',(#900007),{face});\n"
    );
    let at = text.rfind("ENDSEC;").unwrap();
    let text = format!("{}{}{}", &text[..at], extra, &text[at..]);
    let m = read(&mut k, text.as_bytes());
    let green = [0, 255, 0, 255];
    let colours: Vec<Vec<Option<[u8; 4]>>> =
        m.parts.iter().map(|p| k.face_colors(p.body)).collect();
    let greens: usize = colours
        .iter()
        .map(|c| c.iter().filter(|&&c| c == Some(green)).count())
        .sum();
    assert_eq!(greens, 1, "{colours:?}");
    // The first face written is one of the block's, and the block's others stay red.
    assert!(colours[0].contains(&Some(green)), "{colours:?}");
    assert!(
        colours[0]
            .iter()
            .all(|&c| c == Some(green) || c == Some(RED)),
        "{colours:?}"
    );
}

#[test]
fn a_file_of_one_part_names_its_body_and_gives_it_no_use() {
    let mut k = K::new();
    let b = block(&mut k);
    let text = k
        .export_model(ExchangeFormat::Step, "lone", &[(b, "lone".into())], &[])
        .unwrap();
    let m = read(&mut k, &text);
    assert_eq!(m.parts.len(), 1);
    assert_eq!(m.parts[0].name, "lone");
    // No use of its own: the one occurrence a file without assemblies has, where the part is,
    // uncoloured (cadrs names that occurrence after its part; springrep gives the body no
    // instance name at all, which has no counterpart here).
    assert_eq!(m.occurrences.len(), 1);
    let o = &m.occurrences[0];
    assert!(!m.is_assembly(), "{o:?}");
    assert!(o.color.is_none());
    assert!(k.face_colors(m.parts[0].body).iter().all(Option::is_none));
}

#[test]
fn an_instance_of_a_part_the_file_lacks_is_invalid() {
    let mut k = K::new();
    let b = block(&mut k);
    let stray = [ExportInstance {
        part: 1,
        placement: Motion::identity(),
        name: "stray".into(),
    }];
    let r = k.export_model(ExchangeFormat::Step, "a", &[(b, "block".into())], &stray);
    assert!(r.is_err(), "an instance of part 1 of one written");
}
