//! P3F.6 (`intro-to-parametric-cad.md` P3.6): Render Studio renders the Control Arm.
//!
//! - A Render Studio tab of the Part Studio, made by `AddElement`, renders a 1920 × 1080 PNG:
//!   the file has that size, and the image isn't blank (its luminance variance is well above a
//!   flat image's 0).
//! - Rendering twice with the same seed gives identical bytes; another seed doesn't.
//! - Settings change through `SetRenderStudio` and undo.
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::command::History;
use cadrs_core::commands::{AddElement, NewElementKind};
use cadrs_core::document::{Feature, FeatureKind, SketchFeature};
use cadrs_core::render::{self, RenderBackground, RenderEnvironment, RenderView, SetRenderStudio};
use cadrs_core::{Document, Element, ElementId, ElementKind, FeatureId, PartProps, samples};
use cadrs_sketch::{PlaneRef, Sketch, Vec2};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn sketch(g: Sketch) -> Feature {
    Feature { id: FeatureId::new(), name: "Sketch 1".into(), kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Top), disable_imprinting: false, geometry: g }), suppress_by: None }
}

fn extrude(s: &Feature, seeds: &[Vec2], depth: f64) -> Feature {
    let g = &s.sketch().unwrap().geometry;
    Feature { id: FeatureId::new(), name: "Extrude".into(), kind: FeatureKind::Extrude(samples::extrude_of(samples::region_refs(s.id, g, seeds), depth)), suppress_by: None }
}

/// The Control Arm (PS6; both extrudes New, two parts), the first part in Aluminum - 6061 and
/// the second in ABS with a blue appearance; and a Render Studio of it.
fn control_arm_document() -> (Document, ElementId, ElementId) {
    let s = sketch(samples::control_arm_sketch());
    let e1 = extrude(&s, &[v(0.0, 26.0), v(60.0, 0.0), v(107.5 + 14.0, 0.0)], 40.0);
    let e2 = extrude(&s, &[v(-60.0, 0.0), v(-107.5 - 14.0, 0.0)], 25.0);
    let features = vec![s, e1, e2];
    let build = cadrs_core::rebuild::build(&features);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 2);
    let mut studio = Element::part_studio("Part Studio 1");
    let mut props = Vec::new();
    let mut p = PartProps::new(build.parts[0].id);
    p.material = cadrs_core::material::library("Aluminum - 6061");
    props.push(p);
    let mut p = PartProps::new(build.parts[1].id);
    p.material = cadrs_core::material::library("ABS");
    p.appearance = Some(cadrs_core::Appearance::rgb(41, 128, 185));
    props.push(p);
    if let ElementKind::PartStudio { features: f, parts, .. } = &mut studio.kind {
        *f = features;
        *parts = props;
    }
    let source = studio.id;
    let mut doc = Document::new("Control Arm");
    doc.elements = vec![studio];
    let mut history = History::default();
    let id = ElementId::new();
    history.execute(&mut doc, &AddElement { id, kind: NewElementKind::RenderStudio(Some(source)), name: None, after: Some(source) }).unwrap();
    assert_eq!(doc.element(id).unwrap().name, "Render Studio 1");
    (doc, source, id)
}

fn build_of(doc: &Document) -> impl FnMut(ElementId) -> Option<Arc<cadrs_core::rebuild::Build>> + '_ {
    |e| Some(cadrs_core::rebuild::build(doc.element(e)?.features()))
}

#[test]
fn control_arm_renders_a_full_hd_png_that_is_not_blank() {
    let (doc, _, id) = control_arm_document();
    let mut r = render::studio(&doc, id).unwrap().clone();
    assert_eq!((r.width, r.height), (1920, 1080));
    // Few samples: the test is about the file, not the noise.
    r.samples = 8;
    let img = render::render(&doc, &r, build_of(&doc), 4).unwrap();
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("control-arm-render.png");
    img.save(&path).unwrap();
    let back = image::open(&path).unwrap().to_rgba8();
    assert_eq!(back.dimensions(), (1920, 1080));
    let (mean, variance) = cadrs_render::luminance_stats(&back);
    assert!(variance > 0.005, "variance {variance}");
    assert!(mean > 0.3 && mean < 0.95, "mean {mean}");
    // The model is in the middle of the frame, the backdrop at the corners.
    let centre = back.get_pixel(960, 540);
    let corner = back.get_pixel(4, 4);
    assert_ne!(centre, corner);
}

#[test]
fn a_fixed_seed_renders_the_same_image() {
    let (doc, _, id) = control_arm_document();
    let mut r = render::studio(&doc, id).unwrap().clone();
    (r.width, r.height, r.samples, r.seed) = (320, 180, 6, 7);
    let a = render::render(&doc, &r, build_of(&doc), 1).unwrap();
    let b = render::render(&doc, &r, build_of(&doc), 4).unwrap();
    assert_eq!(a.as_raw(), b.as_raw());
    r.seed = 8;
    let c = render::render(&doc, &r, build_of(&doc), 4).unwrap();
    assert_ne!(a.as_raw(), c.as_raw());
}

#[test]
fn settings_change_through_commands_and_undo() {
    let (mut doc, source, id) = control_arm_document();
    let mut history = History::default();
    let mut s = render::studio(&doc, id).unwrap().clone();
    s.environment = RenderEnvironment::Outdoor;
    s.background = RenderBackground::Transparent;
    s.view = RenderView::Isometric;
    history.execute(&mut doc, &SetRenderStudio { element: id, studio: s.clone(), label: "Environment Outdoor".into() }).unwrap();
    assert_eq!(render::studio(&doc, id).unwrap().environment, RenderEnvironment::Outdoor);
    history.undo(&mut doc).expect("undo");
    assert_eq!(render::studio(&doc, id).unwrap().environment, RenderEnvironment::Studio);
    // A render can't reference a drawing or itself, nor be 0 pixels wide.
    let mut bad = s.clone();
    bad.source = Some(id);
    assert!(history.execute(&mut doc, &SetRenderStudio { element: id, studio: bad, label: "x".into() }).is_err());
    let mut bad = s;
    bad.source = Some(source);
    bad.width = 0;
    assert!(history.execute(&mut doc, &SetRenderStudio { element: id, studio: bad, label: "x".into() }).is_err());
    // The tab round-trips through the document file.
    let text = ron::to_string(&doc).unwrap();
    let back: Document = ron::from_str(&text).unwrap();
    assert_eq!(render::studio(&back, id), render::studio(&doc, id));
}

#[test]
fn transparent_background_and_orthographic_views_render() {
    let (doc, _, id) = control_arm_document();
    let mut r = render::studio(&doc, id).unwrap().clone();
    (r.width, r.height, r.samples) = (320, 180, 4);
    r.background = RenderBackground::Transparent;
    r.perspective = false;
    r.view = RenderView::Top;
    let img = render::render(&doc, &r, build_of(&doc), 4).unwrap();
    // Seen from above every corner is ground: clear but for the faintest shadow.
    assert!(img.get_pixel(0, 0)[3] < 40);
    // The arm is opaque (the centre is the hub's bore, where the ground shows through).
    assert!(img.pixels().filter(|p| p[3] == 255).count() > 320 * 180 / 10);
}
