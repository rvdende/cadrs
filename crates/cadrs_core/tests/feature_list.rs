//! P3.9: feature-list management on the Gear Cover stand-in: the rollback bar and suppression
//! (what is rebuilt, through the command and undo layer), deleting a folder with its features,
//! dependencies, and regeneration times.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::commands::{AddSketch, DeleteFeature, DeleteFolder, SetRollback, SetSuppressed, UnpackFolder};
use cadrs_core::document::Document;
use cadrs_core::feature_list::{children, parents};
use cadrs_core::rebuild;
use cadrs_core::samples::gear_cover as gc;
use cadrs_core::{ElementId, FeatureId, History};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = gc::document().expect("the stand-in builds");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn run(&mut self, c: &dyn cadrs_core::Command) {
        self.h.execute(&mut self.d, c).expect("command applies");
    }

    fn element(&self) -> &cadrs_core::Element {
        self.d.element(self.el).unwrap()
    }

    /// The volume of what is built (the active features).
    fn volume(&self) -> f64 {
        let b = rebuild::build(&self.element().active_features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.iter().map(|p| p.mass.as_ref().unwrap().volume).sum()
    }

    fn names(&self) -> Vec<String> {
        self.element().features().iter().map(|f| f.name.clone()).collect()
    }
}

/// The rounded rectangle (40 high) without the slope or the bosses.
fn block_volume() -> f64 {
    (2.0 * gc::HALF_WIDTH * gc::LENGTH - (4.0 - PI) * gc::CORNER_R * gc::CORNER_R) * gc::HEIGHT
}

fn bosses_volume() -> f64 {
    2.0 * PI * gc::BOSS_R * gc::BOSS_R * gc::BOSS_H
}

#[test]
fn rollback_bar_leaves_out_the_features_below_it() {
    let mut s = Studio::new();
    let full = s.volume();
    assert_eq!(s.element().rollback_index(), 6);
    // The bar under Extrude 1: only the block.
    s.run(&SetRollback { element: s.el, index: Some(2) });
    assert_eq!(s.element().active_features().len(), 2);
    assert!(s.element().is_rolled_back(gc::EXTRUDE_2) && !s.element().is_rolled_back(gc::EXTRUDE_1));
    close(s.volume(), block_volume(), 1e-3);
    // A new feature goes in at the bar, which stays below it.
    let sketch = FeatureId::new();
    s.run(&AddSketch { element: s.el, feature: sketch, plane: Some(cadrs_sketch::PlaneRef::Front) });
    assert_eq!(s.names()[2], "Sketch 4");
    assert_eq!(s.element().rollback_index(), 3);
    assert_eq!(s.element().active_features().last().map(|f| f.id), Some(sketch));
    // Deleting a feature above the bar keeps the bar under the same features.
    s.run(&DeleteFeature { element: s.el, feature: sketch, label: "Delete Sketch 4".into() });
    assert_eq!(s.element().rollback_index(), 2);
    // Saved and loaded with the document.
    let text = ron::to_string(&s.d).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(back.element(s.el).unwrap().rollback_index(), 2);
    // Undo moves it back; the end is "no rollback".
    s.h.undo(&mut s.d);
    s.h.undo(&mut s.d);
    assert_eq!(s.element().rollback_index(), 2);
    s.h.undo(&mut s.d);
    assert_eq!(s.element().rollback_index(), 6);
    close(s.volume(), full, 1e-6);
    s.run(&SetRollback { element: s.el, index: Some(6) });
    assert_eq!(s.element().features().len(), 6);
    assert!(!s.element().is_rolled_back(gc::EXTRUDE_3));
    assert!(s.h.execute(&mut s.d, &SetRollback { element: s.el, index: Some(7) }).is_err());
}

#[test]
fn suppressed_features_are_not_built() {
    let mut s = Studio::new();
    let full = s.volume();
    s.run(&SetSuppressed { element: s.el, features: vec![gc::EXTRUDE_3], suppressed: true, label: "Suppress Extrude 3".into() });
    assert!(s.element().is_suppressed(gc::EXTRUDE_3));
    // Still in the list, not built: the bosses are gone.
    assert_eq!(s.element().features().len(), 6);
    assert_eq!(s.element().active_features().len(), 5);
    close(full - s.volume(), bosses_volume(), 1e-3);
    // Suppressing it again changes nothing; it is saved with the document.
    s.run(&SetSuppressed { element: s.el, features: vec![gc::EXTRUDE_3], suppressed: true, label: "Suppress".into() });
    assert_eq!(s.element().suppressed(), &[gc::EXTRUDE_3]);
    let back: Document = ron::from_str(&ron::to_string(&s.d).unwrap()).unwrap();
    assert!(back.element(s.el).unwrap().is_suppressed(gc::EXTRUDE_3));
    // Unsuppress; and undo of the suppression.
    s.run(&SetSuppressed { element: s.el, features: vec![gc::EXTRUDE_3], suppressed: false, label: "Unsuppress".into() });
    close(s.volume(), full, 1e-6);
    s.h.undo(&mut s.d);
    assert!(s.element().is_suppressed(gc::EXTRUDE_3));
    // A suppressed feature that is deleted is forgotten.
    s.run(&DeleteFeature { element: s.el, feature: gc::EXTRUDE_3, label: "Delete".into() });
    assert!(s.element().suppressed().is_empty());
}

#[test]
fn deleting_a_folder_deletes_its_features() {
    let mut s = Studio::new();
    s.run(&DeleteFolder { element: s.el, folder: gc::FOLDER });
    assert!(s.element().features().is_empty());
    assert!(s.element().folders().is_empty());
    assert_eq!(s.h.undo(&mut s.d).as_deref(), Some("Delete folder"));
    assert_eq!(s.element().features().len(), 6);
    assert_eq!(s.element().folders().len(), 1);
    // Unpacking keeps them.
    s.run(&UnpackFolder { element: s.el, folder: gc::FOLDER });
    assert_eq!(s.element().features().len(), 6);
    assert!(s.element().folders().is_empty());
}

#[test]
fn dependencies_of_the_gear_cover() {
    let s = Studio::new();
    let f = s.element().features();
    assert_eq!(parents(f, gc::EXTRUDE_1), vec![gc::SKETCH_1]);
    // Extrude 2 cuts the cover (its merge scope is found by itself): its sketch only.
    assert_eq!(parents(f, gc::EXTRUDE_2), vec![gc::SKETCH_2]);
    // P3.11: with the build, the cover it cuts (Extrude 1's part) is its parent too.
    let b = rebuild::build(f);
    assert_eq!(cadrs_core::feature_list::parents_with(f, &b.parts, &b.uses, gc::EXTRUDE_2), vec![gc::EXTRUDE_1, gc::SKETCH_2]);
    assert_eq!(children(f, gc::SKETCH_3), vec![gc::EXTRUDE_3]);
    assert_eq!(children(f, gc::SKETCH_1), vec![gc::EXTRUDE_1]);
}

#[test]
fn regeneration_times_are_measured_per_feature() {
    let s = Studio::new();
    // A fresh rebuilder: every feature is computed.
    let mut r = rebuild::Rebuilder::new();
    let b = r.rebuild(s.element().features());
    let ids: Vec<FeatureId> = b.times.iter().map(|(f, _)| *f).collect();
    assert_eq!(ids, vec![gc::EXTRUDE_1, gc::EXTRUDE_2, gc::EXTRUDE_3]);
    assert!(b.times.iter().all(|(_, t)| !t.is_zero()), "{:?}", b.times);
    // A second rebuild takes them from the cache and keeps their times.
    let again = r.rebuild(s.element().features());
    assert_eq!(again.computed, 0);
    assert_eq!(again.times, b.times);
    // P3.11: the sketches have times too (finding their regions), kept while they don't change.
    let sketches: Vec<FeatureId> = b.sketch_times.iter().map(|(f, _)| *f).collect();
    assert_eq!(sketches, vec![gc::SKETCH_1, gc::SKETCH_2, gc::SKETCH_3]);
    assert!(b.sketch_times.iter().all(|(_, t)| !t.is_zero()), "{:?}", b.sketch_times);
    assert_eq!(again.sketch_times, b.sketch_times);
}

/// The bracket (P3.9's filter and folder stand-in): a plate, ten bosses, two fillets.
fn bracket() -> (Document, ElementId) {
    let mut d = Document::empty("Bracket");
    let el = cadrs_core::Element::part_studio("Bracket");
    let id = el.id;
    d.elements.push(el);
    let mut h = History::default();
    cadrs_core::samples::bracket::build_in(&mut cadrs_core::samples::gear_cover::DocHistory(&mut d, &mut h), id).expect("the bracket builds");
    (d, id)
}

#[test]
fn filter_on_the_bracket() {
    use cadrs_core::feature_list::{FeatureFacts, Filter, type_label};
    use cadrs_core::samples::bracket as br;
    let (mut d, el_id) = bracket();
    let el = d.element(el_id).unwrap();
    let b = rebuild::build(el.features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    // The plate with its bosses, less the corner fillets.
    let v = b.parts[0].mass.as_ref().unwrap().volume;
    let bosses: f64 = br::bosses().iter().map(|(_, h)| PI * 36.0 * h).sum();
    assert!(v < 120.0 * 80.0 * 10.0 + bosses && v > 120.0 * 80.0 * 10.0 + bosses - 4.0 * 25.0 * 10.0, "{v}");
    // P3.11 (P3.9 judge): the facts the list filters on come from the build, as in the app: a
    // feature's parts are the parts it made or changed, by their shown names (the plate is
    // renamed "Bracket"), and its error is the rebuild's.
    let names = |el: &cadrs_core::Element, b: &rebuild::Build, q: &str| -> Vec<String> {
        let f = Filter::parse(q).unwrap();
        el.features()
            .iter()
            .filter(|x| {
                let parts: Vec<&str> = b
                    .parts
                    .iter()
                    .filter(|p| p.feature == x.id || p.features.contains(&x.id))
                    .map(|p| cadrs_core::parts::display_name(p, el.part_props()))
                    .collect();
                f.matches(&FeatureFacts {
                    name: &x.name,
                    type_label: type_label(&x.kind),
                    folder: el.folder_of(x.id).map(|g| g.name.as_str()),
                    error: b.error(x.id).is_some() || !x.is_valid(),
                    parts,
                    variables: cadrs_core::feature_list::variable_facts(x),
                })
            })
            .map(|x| x.name.clone())
            .collect()
    };
    assert_eq!(names(el, &b, "Extrude").len(), 11);
    assert_eq!(names(el, &b, "Extrude 1"), ["Extrude 1", "Extrude 10", "Extrude 11"]);
    assert_eq!(names(el, &b, "\"Extrude 1\""), ["Extrude 1"]);
    assert_eq!(names(el, &b, ":type Fillet"), ["Fillet 1", "Fillet 2"]);
    assert_eq!(names(el, &b, ":folder bosses").len(), 11);
    // Extrude 1 to 11 and the two fillets made or changed the Bracket; sketches make no part.
    assert_eq!(names(el, &b, ":part bracket").len(), 13);
    assert!(names(el, &b, ":part part 1").is_empty());
    assert!(names(el, &b, ":errors").is_empty());
    // Dependencies: the fillets are Extrude 1's children (their edges are its), Sketch 2's
    // children the ten bosses.
    let f = el.features();
    let kids = children(f, br::EXTRUDE_1);
    assert!(kids.contains(&br::FILLET_1) && kids.contains(&br::FILLET_2), "{kids:?}");
    assert_eq!(children(f, br::SKETCH_2), br::BOSSES.to_vec());

    // A fillet too big for the plate fails: `:errors` finds it, and its part is still named.
    let mut fillet = el.feature(br::FILLET_1).unwrap().kind.clone();
    if let cadrs_core::FeatureKind::Fillet(x) = &mut fillet {
        x.size = 60.0;
        x.size_expr = "60 mm".into();
    }
    let mut h = History::default();
    h.execute(&mut d, &cadrs_core::commands::SetFeature { element: el_id, feature: br::FILLET_1, kind: fillet, label: "Fillet".into() })
        .unwrap();
    let el = d.element(el_id).unwrap();
    let b = rebuild::build(el.features());
    assert!(b.error(br::FILLET_1).is_some(), "{:?}", b.errors);
    assert_eq!(names(el, &b, ":errors"), ["Fillet 1"]);
    assert_eq!(names(el, &b, ":errors fillet"), ["Fillet 1"]);
}

/// P3.11 (PS11.2, P3.9 judge): what the rebuild knows adds parents the references don't name.
/// The bosses are Adds whose merge scope they found by themselves, so the plate's feature,
/// Extrude 1, is their parent; Fillet 2 is picked on the plate's rim, and its tangent chain runs
/// round Fillet 1's corner faces, so Fillet 1 is its parent too.
#[test]
fn implicit_parents_of_the_bracket() {
    use cadrs_core::feature_list::{children_with, parents_with};
    use cadrs_core::samples::bracket as br;
    let (d, el) = bracket();
    let el = d.element(el).unwrap();
    let f = el.features();
    let b = rebuild::build(f);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    // The references alone: a boss names only its sketch, Fillet 2 only the plate's faces.
    assert_eq!(parents(f, br::BOSSES[0]), vec![br::SKETCH_2]);
    assert_eq!(parents(f, br::FILLET_2), vec![br::EXTRUDE_1]);
    // With the build.
    assert_eq!(parents_with(f, &b.parts, &b.uses, br::BOSSES[0]), vec![br::EXTRUDE_1, br::SKETCH_2]);
    assert_eq!(parents_with(f, &b.parts, &b.uses, br::FILLET_2), vec![br::EXTRUDE_1, br::FILLET_1]);
    assert_eq!(parents_with(f, &b.parts, &b.uses, br::FILLET_1), vec![br::EXTRUDE_1]);
    let mut kids = vec![];
    kids.extend(br::BOSSES);
    kids.extend([br::FILLET_1, br::FILLET_2]);
    assert_eq!(children_with(f, &b.parts, &b.uses, br::EXTRUDE_1), kids);
    assert_eq!(children_with(f, &b.parts, &b.uses, br::FILLET_1), vec![br::FILLET_2]);
    // The plate's own sketch is still only Extrude 1's parent.
    assert_eq!(children_with(f, &b.parts, &b.uses, br::SKETCH_1), vec![br::EXTRUDE_1]);
}
