//! Standard content (P3B.5, `intro-to-assemblies.md` A19, A21.2, A21.14, X12): the bundled
//! library against the standards' tables, the generated parts against the library, auto-size,
//! placement with Fastened mates (single, batch, flipped, stacked) and bulk edit.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_core::assembly::standard::{
    self, EditStandardContent, HoleSite, Kind, LibUnit, PART, StandardPart, StandardSpec, Stacking, auto_size, library, plan_insert, same_configuration, site_of_edge, sites_of_face,
};
use cadrs_core::assembly::structure::{occurrences, solver_model};
use cadrs_core::assembly::solver::SolveOptions;
use cadrs_core::assembly::mate::MateType;
use cadrs_core::assembly::{self, InstanceId};
use cadrs_core::rebuild::Build;
use cadrs_core::samples::pneumatic_ex2 as ex2;
use cadrs_core::{Document, ElementId, History, Solid};

const IN: f64 = 25.4;

/// The standards' tables, transcribed independently of the data file: (component, size, F max,
/// G max, H min, H max), inch.
const ANSI_HEX: &[(&str, &str, f64, f64, f64, f64)] = &[
    // ASME B18.2.1-2012 Table 2, hex cap screws.
    ("Hex cap screw", "1/4", 0.438, 0.505, 0.150, 0.163),
    ("Hex cap screw", "5/16", 0.500, 0.577, 0.195, 0.211),
    ("Hex cap screw", "3/8", 0.562, 0.650, 0.226, 0.243),
    ("Hex cap screw", "7/16", 0.625, 0.722, 0.272, 0.291),
    ("Hex cap screw", "1/2", 0.750, 0.866, 0.302, 0.323),
    ("Hex cap screw", "5/8", 0.938, 1.083, 0.378, 0.403),
    ("Hex cap screw", "3/4", 1.125, 1.299, 0.455, 0.483),
    ("Hex cap screw", "1", 1.500, 1.732, 0.591, 0.627),
    // ASME B18.2.2 Table 4, hex nuts.
    ("Hex nut", "1/4", 0.438, 0.505, 0.212, 0.226),
    ("Hex nut", "5/16", 0.500, 0.577, 0.258, 0.273),
    ("Hex nut", "3/8", 0.563, 0.650, 0.320, 0.337),
    ("Hex nut", "7/16", 0.688, 0.794, 0.365, 0.385),
    ("Hex nut", "1/2", 0.750, 0.866, 0.427, 0.448),
    ("Hex nut", "5/8", 0.938, 1.083, 0.535, 0.559),
    ("Hex nut", "3/4", 1.125, 1.299, 0.617, 0.665),
    ("Hex nut", "1", 1.500, 1.732, 0.831, 0.887),
];

/// ASME B18.6.3-2013 Table 17, slotted pan head: (size, A max, H max), inch.
const PAN_HEAD: &[(&str, f64, f64)] = &[("#4", 0.219, 0.068), ("#6", 0.270, 0.082), ("#8", 0.322, 0.096), ("#10", 0.373, 0.110), ("1/4", 0.492, 0.144)];

/// ASME B18.22.1 Table 1A, Type A plain washers: (size, ID, OD, t), inch.
const ANSI_WASHER: &[(&str, f64, f64, f64)] = &[
    ("1/4 N", 0.281, 0.625, 0.065),
    ("1/4 W", 0.312, 0.734, 0.065),
    ("5/16 N", 0.344, 0.688, 0.065),
    ("5/16 W", 0.375, 0.875, 0.083),
    ("3/8 N", 0.406, 0.812, 0.065),
    ("3/8 W", 0.438, 1.000, 0.083),
    ("1/2 N", 0.531, 1.062, 0.095),
    ("1/2 W", 0.562, 1.375, 0.109),
    ("5/8 N", 0.656, 1.312, 0.095),
    ("3/4 N", 0.812, 1.469, 0.134),
];

/// ISO 4762 (dk max, k max, s), ISO 4032 and ISO 4035 (s max, m max), ISO 7089 (d1, d2, h), mm.
const ISO_4762: &[(&str, f64, f64, f64)] = &[
    ("M3", 5.5, 3.0, 2.5),
    ("M4", 7.0, 4.0, 3.0),
    ("M5", 8.5, 5.0, 4.0),
    ("M6", 10.0, 6.0, 5.0),
    ("M8", 13.0, 8.0, 6.0),
    ("M10", 16.0, 10.0, 8.0),
    ("M12", 18.0, 12.0, 10.0),
    ("M16", 24.0, 16.0, 14.0),
];
const ISO_4032: &[(&str, f64, f64)] = &[("M3", 5.5, 2.4), ("M4", 7.0, 3.2), ("M5", 8.0, 4.7), ("M6", 10.0, 5.2), ("M8", 13.0, 6.8), ("M10", 16.0, 8.4), ("M12", 18.0, 10.8), ("M16", 24.0, 14.8)];
const ISO_4035: &[(&str, f64, f64)] = &[("M3", 5.5, 1.8), ("M4", 7.0, 2.2), ("M5", 8.0, 2.7), ("M6", 10.0, 3.2), ("M8", 13.0, 4.0), ("M10", 16.0, 5.0), ("M12", 18.0, 6.0), ("M16", 24.0, 8.0)];
const ISO_7089: &[(&str, f64, f64, f64)] = &[
    ("3", 3.2, 7.0, 0.5),
    ("4", 4.3, 9.0, 0.8),
    ("5", 5.3, 10.0, 1.0),
    ("6", 6.4, 12.0, 1.6),
    ("8", 8.4, 16.0, 1.6),
    ("10", 10.5, 20.0, 2.0),
    ("12", 13.0, 24.0, 2.5),
    ("16", 17.0, 30.0, 3.0),
];

fn near(what: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (±{tol})");
}

/// The nominal part of a size ("1/4-28" → "1/4", "#10-24" → "#10", "M6" → "M6").
fn nominal(size: &str) -> &str {
    match size.split_once('-') {
        Some((n, _)) => n,
        None => size,
    }
}

/// Every row of the data file cites a standard table and matches it.
#[test]
fn every_bundled_size_matches_its_standard() {
    let mut rows = 0;
    for st in &library().standards {
        for cat in &st.categories {
            for class in &cat.classes {
                for comp in &class.components {
                    for s in &comp.sizes {
                        rows += 1;
                        let what = format!("{} {} {}", st.name, comp.name, s.name);
                        assert!(s.source.contains(&comp.standard), "{what}: its source doesn't cite {}", comp.standard);
                        match (st.unit, comp.kind) {
                            (LibUnit::Inch, Kind::HexCapScrew | Kind::HexNut) => {
                                let &(_, _, f, g, hmin, hmax) = ANSI_HEX.iter().find(|r| r.0 == comp.name && r.1 == nominal(&s.name)).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} F"), s.f, f, 0.0006);
                                near(&format!("{what} G"), s.g, g, 0.0006);
                                assert!(s.h >= hmin && s.h <= hmax, "{what}: H {} not in {hmin}..{hmax}", s.h);
                            }
                            (LibUnit::Inch, Kind::PanHeadScrew) => {
                                let &(_, a, h) = PAN_HEAD.iter().find(|r| r.0 == nominal(&s.name)).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} A"), s.head.unwrap(), a, 1e-9);
                                near(&format!("{what} H"), s.h, h, 1e-9);
                            }
                            (LibUnit::Inch, Kind::PlainWasher) => {
                                let &(_, id, od, t) = ANSI_WASHER.iter().find(|r| r.0 == s.name).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} ID"), s.inner.unwrap(), id, 1e-9);
                                near(&format!("{what} OD"), s.head.unwrap(), od, 1e-9);
                                near(&format!("{what} t"), s.h, t, 1e-9);
                            }
                            (LibUnit::Millimeter, Kind::SocketHeadCapScrew) => {
                                let &(_, dk, k, sk) = ISO_4762.iter().find(|r| r.0 == s.name).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} dk"), s.head.unwrap(), dk, 1e-9);
                                near(&format!("{what} k"), s.h, k, 1e-9);
                                near(&format!("{what} s"), s.socket.unwrap(), sk, 1e-9);
                            }
                            (LibUnit::Millimeter, Kind::HexNut) => {
                                let table = if comp.standard == "ISO 4035" { ISO_4035 } else { ISO_4032 };
                                let &(_, sf, m) = table.iter().find(|r| r.0 == s.name).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} s"), s.f, sf, 1e-9);
                                near(&format!("{what} m"), s.h, m, 1e-9);
                                // e min is below the exact hexagon's corners.
                                assert!(s.g < s.f * 2.0 / 3f64.sqrt(), "{what}: e");
                            }
                            (LibUnit::Millimeter, Kind::PlainWasher) => {
                                let &(_, d1, d2, h) = ISO_7089.iter().find(|r| r.0 == s.name).unwrap_or_else(|| panic!("{what}: not in the table"));
                                near(&format!("{what} d1"), s.inner.unwrap(), d1, 1e-9);
                                near(&format!("{what} d2"), s.head.unwrap(), d2, 1e-9);
                                near(&format!("{what} h"), s.h, h, 1e-9);
                            }
                            other => panic!("{what}: no table for {other:?}"),
                        }
                    }
                }
            }
        }
    }
    assert!(rows >= 70, "{rows} rows");
}

/// The generated part's extents (mm): x, y and z ranges of its mesh.
fn extents(s: &Solid) -> [(f64, f64); 3] {
    let mut e = [(f64::MAX, f64::MIN); 3];
    for p in &s.positions {
        for k in 0..3 {
            e[k].0 = e[k].0.min(p[k]);
            e[k].1 = e[k].1.max(p[k]);
        }
    }
    e
}

fn build_of(spec: &StandardSpec) -> Arc<Build> {
    let el = standard::generate(spec).unwrap_or_else(|e| panic!("{}: {e}", spec.part_name()));
    let b = cadrs_core::rebuild::build(el.features());
    assert!(b.errors.is_empty(), "{}: {:?}", spec.part_name(), b.errors);
    b
}

/// Every bundled size generates one solid whose head or nut dimensions are the table's: the
/// hexagon's width across flats F (its Y extent) and corners F·2/√3 (its X extent, within the
/// table's G), the head height or thickness H, the head or washer diameters, the length.
#[test]
fn every_bundled_size_generates_its_dimensions() {
    for st in &library().standards {
        let u = st.unit.mm();
        for cat in &st.categories {
            for class in &cat.classes {
                for comp in &class.components {
                    for s in &comp.sizes {
                        let mut spec = StandardSpec::new(&st.name, &cat.name, &class.name, &comp.name).unwrap();
                        spec.size = s.name.clone();
                        spec.normalize();
                        let what = spec.part_name();
                        let b = build_of(&spec);
                        assert_eq!(b.parts.len(), 1, "{what}: {} parts", b.parts.len());
                        let p = b.part(PART).unwrap_or_else(|| panic!("{what}: no part {PART:?}"));
                        let [x, y, z] = extents(&p.solid);
                        let h = s.h * u;
                        near(&format!("{what} top"), z.1, h, 1e-3);
                        let bottom = match spec.length {
                            Some(l) => -l * u,
                            None => 0.0,
                        };
                        near(&format!("{what} bottom"), z.0, bottom, 1e-3);
                        match comp.kind {
                            Kind::HexCapScrew | Kind::HexNut => {
                                near(&format!("{what} F"), y.1 - y.0, s.f * u, 1e-3);
                                let corners = s.f * u * 2.0 / 3f64.sqrt();
                                near(&format!("{what} corners"), x.1 - x.0, corners, 1e-3);
                                if st.unit == LibUnit::Inch {
                                    // Within G max (and the table's tolerance below it).
                                    assert!(corners <= s.g * u + 0.02 && corners >= s.g * u * 0.97, "{what}: corners {corners} vs G {}", s.g * u);
                                }
                            }
                            Kind::PanHeadScrew | Kind::SocketHeadCapScrew | Kind::PlainWasher => {
                                let dia = s.head.unwrap() * u;
                                // The mesh's vertices lie on the circle: its extent is the diameter
                                // up to the tessellation's chord.
                                assert!((x.1 - x.0) <= dia + 1e-3 && (x.1 - x.0) >= dia * 0.99, "{what}: diameter {} vs {dia}", x.1 - x.0);
                            }
                        }
                        let m = p.mass.as_ref().unwrap_or_else(|| panic!("{what}: no mass"));
                        assert!(m.volume > 0.0, "{what}: volume");
                    }
                }
            }
        }
    }
}

/// A19.2: bolts round down, nuts round up.
#[test]
fn auto_size_rounds_bolts_down_and_nuts_up() {
    let ansi = library().standard("ANSI inch").unwrap();
    let hcs = ansi.category("Bolts & screws").unwrap().class("Hex bolts").unwrap().component("Hex cap screw").unwrap();
    let nut = ansi.category("Nuts").unwrap().class("Hex nuts").unwrap().component("Hex nut").unwrap();
    assert_eq!(auto_size(LibUnit::Inch, hcs, 0.266 * IN, "1/4-20").as_deref(), Some("1/4-20"));
    // The thread series is kept.
    assert_eq!(auto_size(LibUnit::Inch, hcs, 0.266 * IN, "1/4-28").as_deref(), Some("1/4-28"));
    assert_eq!(auto_size(LibUnit::Inch, hcs, 0.375 * IN, "1/4-28").as_deref(), Some("3/8-16"));
    assert_eq!(auto_size(LibUnit::Inch, nut, 0.375 * IN, "1/4-20").as_deref(), Some("3/8-16"));
    assert_eq!(auto_size(LibUnit::Inch, nut, 0.36 * IN, "1/4-20").as_deref(), Some("3/8-16"));
    assert_eq!(auto_size(LibUnit::Inch, nut, 0.266 * IN, "1/4-20").as_deref(), Some("5/16-18"));
    assert_eq!(auto_size(LibUnit::Inch, hcs, 0.2 * IN, "1/4-20"), None);
    let iso = library().standard("ISO").unwrap();
    let shcs = iso.category("Bolts & screws").unwrap().class("Socket head screws").unwrap().component("Socket head cap screw").unwrap();
    assert_eq!(auto_size(LibUnit::Millimeter, shcs, 6.6, "M3").as_deref(), Some("M6"));
}

// ---------------------------------------------------------------------------------------------
// Placement on the Ex2 end state

struct Asm {
    doc: Document,
    h: History,
    builds: HashMap<ElementId, Arc<Build>>,
}

impl Asm {
    fn new() -> Self {
        Self { doc: ex2::document().unwrap(), h: History::default(), builds: HashMap::new() }
    }

    fn build(&mut self, el: ElementId) -> Option<Arc<Build>> {
        if !self.builds.contains_key(&el) {
            let b = cadrs_core::rebuild::build(self.doc.element(el)?.features());
            self.builds.insert(el, b);
        }
        self.builds.get(&el).cloned()
    }

    fn model(&self) -> &cadrs_core::assembly::Assembly {
        self.doc.element(ex2::ASSEMBLY).unwrap().assembly_model().unwrap()
    }

    fn solids(&mut self) -> HashMap<InstanceId, Arc<Solid>> {
        let els: Vec<ElementId> = occurrences(&self.doc, self.model()).iter().map(|o| o.element).collect();
        for e in els {
            self.build(e);
        }
        let builds = self.builds.clone();
        assembly::occurrence_solids(&self.doc, self.model(), |e| builds.get(&e).cloned())
    }

    /// The circular edges of an instance's part with this diameter (in) whose centre is at z (in).
    fn sites(&mut self, instance: InstanceId, dia: f64, z: f64) -> Vec<HoleSite> {
        let solid = self.solids()[&instance].clone();
        let mut out: Vec<HoleSite> = solid
            .edges
            .iter()
            .filter(|e| e.circle.is_some_and(|c| (2.0 * c.radius - dia * IN).abs() < 1e-3 && (c.center[2] - z * IN).abs() < 1e-3))
            .filter_map(|e| site_of_edge(&solid, instance, &e.name))
            .collect();
        out.sort_by_key(|a| a.edge.index);
        out
    }

    fn insert(&mut self, spec: &StandardSpec, sites: &[HoleSite], flip: bool, stacking: Stacking) -> Vec<InstanceId> {
        let solids = self.solids();
        let part = StandardPart::new(spec).unwrap();
        let cmd = plan_insert(&self.doc, ex2::ASSEMBLY, part, sites, flip, stacking, &solids).unwrap();
        let ids = cmd.inserts.iter().map(|i| i.instance).collect();
        self.h.execute(&mut self.doc, &cmd).unwrap();
        ids
    }

    /// Solves: nothing may move (the fasteners were placed where their mates hold them).
    fn assert_solved(&mut self) {
        let flat = solver_model(&self.doc, self.model());
        let solids = self.solids();
        let s = assembly::solve(&flat, &solids, &SolveOptions::default());
        assert!(s.converged, "residual {}", s.residual);
        let moved = s.changed(&flat);
        assert!(moved.is_empty(), "{} instances moved", moved.len());
    }
}

fn screw() -> StandardSpec {
    let mut s = StandardSpec::new("ANSI inch", "Bolts & screws", "Hex bolts", "Hex cap screw").unwrap();
    s.size = "1/4-28".into();
    s.material = "Stainless Steel".into();
    s.normalize();
    s
}

fn nut() -> StandardSpec {
    let mut s = StandardSpec::new("ANSI inch", "Nuts", "Hex nuts", "Hex nut").unwrap();
    s.size = "3/8-16".into();
    s.material = "Stainless Steel".into();
    s.normalize();
    s
}

/// A21.2 / A19.6: six hex cap screws on the Retaining Plate's six hole edges: 6 instances, 6
/// Fastened mates, heads on the plate's top face, shanks down its holes.
#[test]
fn six_screws_on_the_retaining_plate() {
    let mut a = Asm::new();
    let sites = a.sites(ex2::RETAINING_PLATE, 0.266, 6.625);
    assert_eq!(sites.len(), 6);
    assert!(sites.iter().all(|s| !s.shaft && (s.diameter - 0.266 * IN).abs() < 1e-6));
    let (mates0, n0) = (a.model().mates.len(), a.model().instances.len());
    let ids = a.insert(&screw(), &sites, false, Stacking::Plain);
    assert_eq!(a.model().instances.len(), n0 + 6);
    let fastened: Vec<_> = a.model().mates[mates0..].iter().filter(|m| m.mate().is_some_and(|m| m.mate_type == MateType::Fastened)).collect();
    assert_eq!(fastened.len(), 6);
    assert_eq!(fastened[0].name, "Fastened 12");
    assert_eq!(a.doc.standard_content.len(), 1);
    let p = &a.doc.standard_content[0];
    assert_eq!(p.description, "Hex cap screw 1/4-28 x 0.75 Stainless Steel");
    assert_eq!(p.part_number, "HCS-1/4-28-0.75-SS");
    for (k, id) in ids.iter().enumerate() {
        let inst = a.model().instance(*id).unwrap();
        assert_eq!(inst.index, k as u32 + 1);
        // The bearing face on the plate's top, the axis vertical, on the hole circle r 0.55.
        let t = inst.pose.translation;
        near("screw z", t[2], 6.625 * IN, 1e-6);
        near("screw r", (t[0] * t[0] + t[1] * t[1]).sqrt(), 0.55 * IN, 1e-6);
        near("screw axis", inst.pose.rotate([0.0, 0.0, 1.0])[2], 1.0, 1e-9);
    }
    a.assert_solved();
    // The instance's name, from the generated part.
    let b = a.build(a.doc.standard_content[0].element.id).unwrap();
    let name = assembly::source_part_name(&a.doc, &a.model().instance(ids[0]).unwrap().source, Some(&b));
    assert_eq!(name, "Hex cap screw 1/4-28 x 0.75");
    // Select same configuration: the six.
    assert_eq!(same_configuration(a.model(), ids[2], true), ids);
    // One undo step removes all six and the configuration.
    a.h.undo(&mut a.doc);
    assert_eq!(a.model().instances.len(), n0);
    assert!(a.doc.standard_content.is_empty());
}

/// A21.14: nuts on the rod holes, on top of the Top Cap (up) and under the Rear Cap (down).
#[test]
fn nuts_go_out_of_the_part_on_either_side() {
    let mut a = Asm::new();
    let top = a.sites(ex2::TOP_CAP, 0.375, 6.5);
    let bottom = a.sites(ex2::REAR_CAP, 0.375, 0.0);
    assert_eq!((top.len(), bottom.len()), (4, 4));
    let mut all = top.clone();
    all.extend(bottom.iter().copied());
    let ids = a.insert(&nut(), &all, false, Stacking::Plain);
    assert_eq!(ids.len(), 8);
    for (k, id) in ids.iter().enumerate() {
        let p = a.model().instance(*id).unwrap().pose;
        let up = p.rotate([0.0, 0.0, 1.0])[2];
        if k < 4 {
            near("top nut z", p.translation[2], 6.5 * IN, 1e-6);
            near("top nut axis", up, 1.0, 1e-9);
        } else {
            near("bottom nut z", p.translation[2], 0.0, 1e-6);
            near("bottom nut axis", up, -1.0, 1e-9);
        }
    }
    a.assert_solved();
    // A flips: the nut goes into the part instead.
    let flipped = a.insert(&nut(), &top[..1], true, Stacking::Plain);
    near("flipped", a.model().instance(flipped[0]).unwrap().pose.rotate([0.0, 0.0, 1.0])[2], -1.0, 1e-9);
}

/// A19.7: a washer inserted furthest from the selection goes on top of the nut; one inserted
/// closest goes under it, and the nut moves out by the washer's thickness.
#[test]
fn stacking_closest_and_furthest() {
    let mut a = Asm::new();
    let site = a.sites(ex2::TOP_CAP, 0.375, 6.5)[..1].to_vec();
    let n = a.insert(&nut(), &site, false, Stacking::Plain)[0];
    let mut washer = StandardSpec::new("ANSI inch", "Washers", "Plain washers", "Plain washer").unwrap();
    washer.size = "3/8 N".into();
    washer.normalize();
    let w1 = a.insert(&washer, &site, false, Stacking::Furthest)[0];
    near("furthest", a.model().instance(w1).unwrap().pose.translation[2], (6.5 + 0.328) * IN, 1e-6);
    let w2 = a.insert(&washer, &site, false, Stacking::Closest)[0];
    near("closest", a.model().instance(w2).unwrap().pose.translation[2], 6.5 * IN, 1e-6);
    near("nut moved out", a.model().instance(n).unwrap().pose.translation[2], (6.5 + 0.065) * IN, 1e-6);
    near("washer moved out", a.model().instance(w1).unwrap().pose.translation[2], (6.5 + 0.065 + 0.328) * IN, 1e-6);
    a.assert_solved();
}

/// A19.6: a face with holes gives one site per hole.
#[test]
fn a_face_gives_its_holes() {
    let mut a = Asm::new();
    let solid = a.solids()[&ex2::RETAINING_PLATE].clone();
    let top = solid.faces.iter().find(|f| f.plane.is_some_and(|p| (p.origin[2] - 6.625 * IN).abs() < 1e-6 && p.u[0] * p.v[1] - p.u[1] * p.v[0] > 0.0)).expect("the plate's top face");
    let sites = sites_of_face(&solid, ex2::RETAINING_PLATE, &top.name);
    // Six screw holes and the Ø0.5 centre hole.
    assert_eq!(sites.len(), 7);
}

/// A19.9: bulk edit of three screws' size and length; their mates stay and still hold.
#[test]
fn bulk_edit_three_sizes() {
    let mut a = Asm::new();
    let sites = a.sites(ex2::RETAINING_PLATE, 0.266, 6.625);
    let ids = a.insert(&screw(), &sites, false, Stacking::Plain);
    let mates = a.model().mates.len();
    let three = &ids[..3];
    let cmd = EditStandardContent::new(&a.doc, ex2::ASSEMBLY, three, Some("5/16-18"), Some(1.0)).unwrap();
    a.h.execute(&mut a.doc, &cmd).unwrap();
    assert_eq!(a.model().mates.len(), mates);
    assert_eq!(a.doc.standard_content.len(), 2);
    let edited = &a.doc.standard_content[1];
    assert_eq!(edited.spec.part_name(), "Hex cap screw 5/16-18 x 1");
    for (k, id) in three.iter().enumerate() {
        let i = a.model().instance(*id).unwrap();
        assert_eq!(i.source, edited.source());
        assert_eq!(i.index, k as u32 + 1);
    }
    assert_eq!(same_configuration(a.model(), ids[4], true), ids[3..].to_vec());
    a.assert_solved();
    a.h.undo(&mut a.doc);
    assert_eq!(a.doc.standard_content.len(), 1);
}

/// A21.2–A21.4, A21.16: with the six screws on the Retaining Plate, Move to new subassembly
/// (Retaining Plate, Top Cap, O-Ring 0.125 <3>, <4>) leaves the screws (and their mates, now
/// across a level) at the top; the Top Cap subassembly's contents, and so its mass properties,
/// are the course's: 0.48362 lb, 5.05843 in³, CoM (0, 0, 6.03294) in.
#[test]
fn screws_stay_at_the_top_when_the_plate_moves_into_a_subassembly() {
    use cadrs_core::assembly::structure::MoveToNewSubassembly;
    let mut a = Asm::new();
    let sites = a.sites(ex2::RETAINING_PLATE, 0.266, 6.625);
    let screws = a.insert(&screw(), &sites, false, Stacking::Plain);
    let [_, _, r3, r4] = ex2::ORINGS_125;
    let sub_el = ElementId::from_u128(0x5c0e_0000_0000_0000_0000_0000_0000_0301);
    a.h.execute(
        &mut a.doc,
        &MoveToNewSubassembly {
            element: ex2::ASSEMBLY,
            instances: vec![ex2::RETAINING_PLATE, ex2::TOP_CAP, r3, r4],
            new_element: sub_el,
            instance: InstanceId::from_u128(0x5c0e_0000_0000_0000_0000_0000_0000_3001),
            name: None,
            after: None,
        },
    )
    .unwrap();
    assert!(screws.iter().all(|s| a.model().instance(*s).is_some()));
    let fastened_to_plate = a.model().mates.iter().filter(|m| m.mate().is_some_and(|m| screws.contains(&m.connectors[0].instance))).count();
    assert_eq!(fastened_to_plate, 6);
    a.assert_solved();
    let sub = a.doc.element(sub_el).unwrap().assembly_model().unwrap().clone();
    assert_eq!(sub.instances.len(), 4);
    let builds = a.builds.clone();
    let (parts, props) = assembly::instance_parts(&a.doc, &sub, |e| builds.get(&e).cloned());
    let ids: Vec<InstanceId> = sub.instances.iter().map(|i| i.id).collect();
    let m = assembly::mass_report(&parts, &props, &ids).unwrap();
    let mass = m.mass.unwrap();
    let rel = |what: &str, got: f64, want: f64| assert!((got - want).abs() <= 1e-4 * want.abs().max(1.0), "{what}: {got} vs {want}");
    rel("mass lb", mass.mass / 0.453_592_37, 0.48362);
    rel("volume in³", m.volume / IN.powi(3), 5.05843);
    rel("CoM z in", mass.center_of_mass.z / IN, 6.03294);
    assert!(mass.center_of_mass.x.abs() < 1e-6 && mass.center_of_mass.y.abs() < 1e-6);
}

/// P3B.5 judge: an ISO 4762 socket head cap screw has its hex socket: the head's volume is the
/// cylinder's less the recess (a hexagon of `s` across flats, down to half the head height).
#[test]
fn iso_4762_heads_have_the_hex_socket() {
    let mut spec = StandardSpec::new("ISO", "Bolts & screws", "Socket head screws", "Socket head cap screw").unwrap();
    spec.size = "M6".into();
    spec.length = Some(16.0);
    spec.normalize();
    let b = build_of(&spec);
    let p = b.part(PART).unwrap();
    let v = p.mass.unwrap().volume;
    let (dk, k, s, d, l) = (10.0f64, 6.0f64, 5.0f64, 6.0f64, 16.0f64);
    let solid = std::f64::consts::PI * (dk * dk / 4.0 * k + d * d / 4.0 * l);
    let hex = 3f64.sqrt() / 2.0 * s * s;
    let recess = hex * (k - 0.5 * k);
    assert!((v - (solid - recess)).abs() < 0.02 * solid, "V {v} vs {} (no recess {solid})", solid - recess);
    // The recess's floor is a face at half the head height.
    let floor = p.solid.faces.iter().filter(|f| f.plane.is_some_and(|pl| (pl.origin[2] - 0.5 * k).abs() < 1e-6)).count();
    assert!(floor >= 1, "no socket floor");
}
