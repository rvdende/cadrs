//! The Image feature: a picture inserted into a Part Studio is a flat surface part named after
//! its file, carrying the picture where it lies; Transform moves the picture with the part and
//! copies it onto copies.
#![cfg(feature = "occt")]

use std::sync::Arc;

use cadrs_core::commands::AddFeature;
use cadrs_core::document::{Document, FeatureKind};
use cadrs_core::parts::PartKind;
use cadrs_core::picture::AddImage;
use cadrs_core::rebuild;
use cadrs_core::transform::{TransformFeature, TransformType};
use cadrs_core::{FeatureId, History};
use cadrs_sketch::PlaneRef;

#[track_caller]
fn close3(a: [f64; 3], b: [f64; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= 1e-6, "got {a:?}, expected {b:?}");
    }
}

/// A `w` × `h` PNG.
fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([20, 120, 220, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn an_image_is_a_surface_part_whose_picture_moves_and_copies_with_it() {
    let mut d = Document::new("Image");
    let el = d.elements[0].id;
    let mut h = History::default();
    // 200 × 100 px on Top, centred at (10, 20): 100 × 50 mm.
    let image = FeatureId::new();
    let cmd = AddImage { element: el, feature: image, file_name: "Logo.PNG".into(), bytes: Arc::new(png(200, 100)), plane: PlaneRef::Top, center: [10.0, 20.0] };
    h.execute(&mut d, &cmd).unwrap();
    let features = d.element(el).unwrap().features().to_vec();
    assert_eq!(features[0].name, "Image 1");
    let b = rebuild::build(&features);
    assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
    assert_eq!(b.parts.len(), 1);
    let p = &b.parts[0];
    assert_eq!((p.name.as_str(), p.kind), ("Logo", PartKind::Surface));
    let area: f64 = p.solid.faces.iter().filter_map(|f| f.area).sum();
    assert!((area - 5000.0).abs() < 1e-3, "area {area}");
    let [pic] = &p.solid.images[..] else { panic!("one picture: {:?}", p.solid.images) };
    close3(pic.corner, [-40.0, -5.0, 0.0]);
    close3(pic.u, [100.0, 0.0, 0.0]);
    close3(pic.v, [0.0, 50.0, 0.0]);
    let part = p.id;

    // Moved 5 along Z: the picture follows.
    let mv = TransformFeature { parts: vec![part], dz: 5.0, dz_expr: "5 mm".into(), ..TransformFeature::new(TransformType::TranslateXyz) };
    h.execute(&mut d, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Transform".into(), kind: FeatureKind::Transform(mv) }).unwrap();
    // Copied 30 along X: the copy shows it too, the original keeps its own.
    let cp = TransformFeature { parts: vec![part], copy: true, dx: 30.0, dx_expr: "30 mm".into(), ..TransformFeature::new(TransformType::TranslateXyz) };
    h.execute(&mut d, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Transform".into(), kind: FeatureKind::Transform(cp) }).unwrap();
    let b = rebuild::build(d.element(el).unwrap().features());
    assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
    assert_eq!(b.parts.len(), 2);
    let moved = b.parts.iter().find(|q| q.id == part).unwrap();
    close3(moved.solid.images[0].corner, [-40.0, -5.0, 5.0]);
    close3(moved.solid.images[0].u, [100.0, 0.0, 0.0]);
    let copy = b.parts.iter().find(|q| q.id != part).unwrap();
    let [pic] = &copy.solid.images[..] else { panic!("the copy shows the picture") };
    close3(pic.corner, [-10.0, -5.0, 5.0]);
    close3(pic.v, [0.0, 50.0, 0.0]);
    assert_eq!(pic.blob, moved.solid.images[0].blob);
}

#[test]
fn an_image_blob_is_saved_with_its_document() {
    let mut d = Document::new("Image");
    let el = d.elements[0].id;
    let cmd = AddImage { element: el, feature: FeatureId::new(), file_name: "photo.jpeg".into(), bytes: Arc::new(png(3, 2)), plane: PlaneRef::Front, center: [0.0, 0.0] };
    History::default().execute(&mut d, &cmd).unwrap();
    let used = cadrs_core::blobs::used_by(&d);
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].1, "jpeg");
}
