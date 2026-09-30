//! P3C.7 (D2.10, X13): exporting drawings to PDF, DXF, DWG and images, and importing DXF onto a
//! sheet, on the Ex1 drawing of the Universal Joint Flange stand-in
//! (`cadrs_core::samples::ujoint_drawing`) and the three-sheet Hand Brake drawing.
#![cfg(feature = "occt")]

use std::path::PathBuf;

use cadrs_core::History;
use cadrs_core::commands::EditDrawing;
use cadrs_core::document::{Document, Element, ElementKind};
use cadrs_core::drawing_export::pages;
use cadrs_core::samples::{hand_brake as hb, ujoint as uj, ujoint_drawing as ex1};
use cadrs_drawing::export::{ExportOptions, Format, Item, Page, Shape, block_from_dxf, write_files};
use cadrs_drawing::sheet_sketch::{ItemKind, SketchItem};
use cadrs_drawing::{Drawing, DrawingOp, SheetId};

fn out_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-exports").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The stand-in document with the finished Ex1 drawing.
fn ex1_doc() -> (Document, Drawing) {
    let doc = uj::document().unwrap();
    let d = ex1::drawing(&doc, doc.elements[0].id).expect("the Ex1 drawing builds");
    (doc, d)
}

fn ex1_page() -> (Document, Drawing, Page) {
    let (doc, d) = ex1_doc();
    let p = pages(&doc, ex1::DRAWING_NAME, &d, &[0], Some((2026, 9, 24)), &|_| None);
    assert_eq!(p.len(), 1);
    let page = p.into_iter().next().unwrap();
    (doc, d, page)
}

#[test]
fn ex1_pdf_has_real_text_and_true_page_size() {
    let (_, _, page) = ex1_page();
    // Every dimension and callout is on the page as text.
    let strings = page.strings();
    for s in ["Ø4.750", "3.282", "2.061", "6.000", "2.600", "2.500", "43.0°", "120.0°", "Ø1.750", "Ø1.250", "4x Ø.266 THRU", "8x Ø.266 THRU"] {
        assert!(strings.iter().any(|t| t.contains(s)), "{s} missing from {strings:?}");
    }
    let dir = out_dir("ex1-pdf");
    let files = write_files(&[page], &ExportOptions { format: Format::Pdf, color: true, dpi: 150.0, ..Default::default() }, &dir, ex1::DRAWING_NAME).unwrap();
    assert_eq!(files.len(), 1);
    let pdf = std::fs::read(&files[0]).unwrap();
    // Our own reading of the content streams: the text is real text, and the page is ANSI A
    // landscape (11 × 8.5 in = 792 × 612 pt).
    let (texts, sizes) = cadrs_drawing::pdf::read_back(&pdf);
    assert!(texts.iter().any(|t| t == "Ø4.750"), "{texts:?}");
    assert_eq!(sizes.len(), 1);
    assert!((sizes[0].0 - 792.0).abs() < 1e-3 && (sizes[0].1 - 612.0).abs() < 1e-3, "{sizes:?}");
    // pdftotext, where it is installed.
    let pdftotext = ["/usr/bin/pdftotext", "/usr/local/bin/pdftotext"].into_iter().find(|p| std::path::Path::new(p).exists());
    match pdftotext {
        Some(tool) => {
            let out = std::process::Command::new(tool).arg(&files[0]).arg("-").output().expect("pdftotext runs");
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains("Ø4.750"), "pdftotext found no Ø4.750 in:\n{text}");
            assert!(text.contains("120.0°") && text.contains("Universal Joint Flange (stand-in)"), "{text}");
            // The counterbore and depth symbols, drawn as strokes, extract as text (ActualText,
            // P3C.8).
            assert!(text.contains('⌴') && text.contains('↧'), "pdftotext found no ⌴ or ↧ in:\n{text}");
        }
        None => eprintln!("pdftotext is not installed: checked the content streams only"),
    }
    // A picture of the PDF for reviewers: target/test-exports/ex1-pdf/render.png (mutool).
    let mutool = ["/usr/bin/mutool", "/usr/local/bin/mutool"].into_iter().find(|p| std::path::Path::new(p).exists());
    match mutool {
        Some(tool) => {
            let png = dir.join("render.png");
            let ok = std::process::Command::new(tool).args(["draw", "-q", "-r", "110", "-o"]).arg(&png).arg(&files[0]).status().is_ok_and(|s| s.success());
            assert!(ok && png.is_file(), "mutool could not draw the PDF");
            let img = image::open(&png).unwrap();
            assert_eq!((img.width(), img.height()), (1210, 935), "11 × 8.5 in at 110 dpi");
        }
        None => eprintln!("mutool is not installed: no render.png"),
    }
}

#[test]
fn a_pdf_has_one_page_per_sheet_at_its_size() {
    let doc = hb::document().expect("the hand brake builds");
    let (el, d) = doc
        .elements
        .iter()
        .find_map(|e| match &e.kind {
            ElementKind::Drawing(d) => Some((e, d)),
            _ => None,
        })
        .expect("a drawing");
    let all: Vec<usize> = (0..d.sheets.len()).collect();
    let ps = pages(&doc, &el.name, d, &all, None, &|_| None);
    assert_eq!(ps.len(), 3);
    let pdf = cadrs_drawing::pdf::write_pdf(&ps, &Default::default());
    let (_, sizes) = cadrs_drawing::pdf::read_back(&pdf);
    assert_eq!(sizes.len(), 3, "one page per sheet");
    for (s, sheet) in sizes.iter().zip(&d.sheets) {
        let (w, h) = sheet.size_mm();
        let k = 72.0 / 25.4;
        assert!((s.0 - w * k).abs() < 1e-3 && (s.1 - h * k).abs() < 1e-3, "{s:?} for {w} × {h} mm");
    }
    // Black and white too.
    let bw = cadrs_drawing::pdf::write_pdf(&ps, &cadrs_drawing::pdf::PdfOptions { color: false });
    assert!(bw.starts_with(b"%PDF-1.7"));
}

/// The strokes of a page by kind, sorted, for comparing.
type Strokes = (Vec<[f64; 4]>, Vec<[f64; 5]>, Vec<[f64; 3]>);

fn strokes(page: &Page) -> Strokes {
    let (mut l, mut a, mut c) = (Vec::new(), Vec::new(), Vec::new());
    for it in &page.items {
        match it {
            Item::Stroke(Shape::Line { a: p, b: q }, _) => l.push([p[0], p[1], q[0], q[1]]),
            Item::Stroke(Shape::Arc { center, radius, start, end }, _) => a.push([center[0], center[1], *radius, *start, *end]),
            Item::Stroke(Shape::Circle { center, radius }, _) => c.push([center[0], center[1], *radius]),
            _ => {}
        }
    }
    let key = |v: &[f64]| v.iter().map(|x| format!("{:.4}", x)).collect::<Vec<_>>().join(",");
    l.sort_by_key(|v| key(v));
    a.sort_by_key(|v| key(v));
    c.sort_by_key(|v| key(v));
    (l, a, c)
}

fn same<const N: usize>(what: &str, a: &[[f64; N]], b: &[[f64; N]]) {
    assert_eq!(a.len(), b.len(), "{what} count");
    for (x, y) in a.iter().zip(b) {
        for k in 0..N {
            assert!((x[k] - y[k]).abs() < 1e-6, "{what}: {x:?} vs {y:?}");
        }
    }
}

#[test]
fn dxf_round_trips_onto_a_new_sheet() {
    let (_, mut d, page) = ex1_page();
    let (lines, arcs, circles) = strokes(&page);
    assert!(lines.len() > 100 && !arcs.is_empty() && circles.len() > 10, "{} {} {}", lines.len(), arcs.len(), circles.len());
    let dir = out_dir("ex1-dxf");
    let files = write_files(std::slice::from_ref(&page), &ExportOptions { format: Format::Dxf, color: true, dpi: 150.0, ..Default::default() }, &dir, "ex1").unwrap();
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("AC1027") && text.contains("HIDDEN") && text.contains("PHANTOM"));
    let read = cadrs_drawing::dxf::read_dxf(&text).unwrap();
    assert_eq!(cadrs_drawing::dxf::counts(&read), (lines.len(), arcs.len(), circles.len()));
    assert!(read.skipped.is_empty(), "{:?}", read.skipped);
    // Insert DXF on a new sheet, without border or title block, with the block's origin where
    // the file's extent starts: the sheet draws exactly the exported strokes.
    let (lo, _) = cadrs_drawing::sheet_sketch::entities_bounds(&read.entities).unwrap();
    let block = block_from_dxf("ex1.dxf", &read, lo).unwrap();
    let sheet = SheetId::new();
    d.apply(&DrawingOp::InsertSheet { id: sheet, after: None }).unwrap();
    let mut props = d.sheet(sheet).unwrap().props();
    props.border = false;
    props.zones = false;
    props.title_block = false;
    d.apply(&DrawingOp::SetSheetProps { id: sheet, props }).unwrap();
    d.apply(&DrawingOp::AddSketchItems { sheet, items: vec![SketchItem::new(ItemKind::Block(block))] }).unwrap();
    let doc = uj::document().unwrap();
    // Sheet selection: only the asked sheet becomes a page.
    let picked = pages(&doc, "x", &d, &[1], None, &|_| None);
    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].name, d.sheets[1].name);
    let back = picked.into_iter().next().unwrap();
    let (l2, a2, c2) = strokes(&back);
    same("lines", &lines, &l2);
    same("arcs", &arcs, &a2);
    same("circles", &circles, &c2);
    // Texts come back too.
    assert!(back.strings().contains(&"Ø4.750"));
}

#[test]
fn dwg_export_uses_a_converter_or_is_skipped() {
    let Some(conv) = cadrs_drawing::dwg::find_converter().filter(|c| c.can_write()) else {
        eprintln!("skipped: no DWG converter on PATH ({})", cadrs_drawing::dwg::INSTALL_HINT);
        return;
    };
    let (_, _, page) = ex1_page();
    let dir = out_dir("ex1-dwg");
    let files = write_files(&[page], &ExportOptions { format: Format::Dwg, color: true, dpi: 150.0, ..Default::default() }, &dir, "ex1").expect("DWG export");
    let bytes = std::fs::read(&files[0]).unwrap();
    assert!(bytes.starts_with(b"AC10"), "{} wrote no DWG", conv.name());
    if conv.can_read() {
        let block = cadrs_drawing::export::block_from_file(&files[0], [0.0, 0.0]).expect("the DWG reads back");
        assert!(!block.entities.is_empty());
    }
}

/// X13: DWT, a template in DWG format through the same converter (skipped like DWG without
/// one).
#[test]
fn dwt_export_uses_a_converter_or_is_skipped() {
    let Some(conv) = cadrs_drawing::dwg::find_converter().filter(|c| c.can_write()) else {
        eprintln!("skipped: no DWG converter on PATH ({})", cadrs_drawing::dwg::INSTALL_HINT);
        return;
    };
    let (_, _, page) = ex1_page();
    let dir = out_dir("ex1-dwt");
    let files = write_files(&[page], &ExportOptions { format: Format::Dwt, ..Default::default() }, &dir, "ex1").expect("DWT export");
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("dwt"));
    let bytes = std::fs::read(&files[0]).unwrap();
    assert!(bytes.starts_with(b"AC10"), "{} wrote no DWG-format template", conv.name());
}

#[test]
fn dwt_is_a_converter_format() {
    assert!(Format::Dwt.needs_converter() && Format::Dwg.needs_converter() && !Format::Dxf.needs_converter());
    assert_eq!(Format::Dwt.extension(), "dwt");
}

#[test]
fn raster_export_writes_png_and_jpeg() {
    let (_, _, page) = ex1_page();
    let dir = out_dir("ex1-raster");
    for f in [Format::Png, Format::Jpeg] {
        let files = write_files(std::slice::from_ref(&page), &ExportOptions { format: f, color: true, dpi: 100.0, ..Default::default() }, &dir, "ex1").unwrap();
        let img = image::open(&files[0]).unwrap();
        // 11 × 8.5 in at 100 dpi.
        assert_eq!((img.width(), img.height()), (1100, 850));
    }
}

#[test]
fn sheet_lines_and_splines_undo_and_redo() {
    let mut doc = Document::new("Doc");
    let t = cadrs_drawing::template::builtin("ANSI_A_INCH.dwt").unwrap();
    let el = Element::drawing("Drawing 1", Drawing::from_template(&t, None));
    let id = el.id;
    doc.elements.push(el);
    let mut h = History::default();
    let sheet = match &doc.element(id).unwrap().kind {
        ElementKind::Drawing(d) => d.sheets[0].id,
        _ => unreachable!(),
    };
    let sketch = |doc: &Document| match &doc.element(id).unwrap().kind {
        ElementKind::Drawing(d) => d.sheets[0].sketch.clone(),
        _ => unreachable!(),
    };
    let line = SketchItem::new(ItemKind::Line { a: [10.0, 10.0], b: [60.0, 30.0] });
    let spline = SketchItem::new(ItemKind::Spline { points: vec![[20.0, 50.0], [40.0, 70.0], [60.0, 45.0], [80.0, 60.0]] });
    h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::AddSketchItems { sheet, items: vec![line.clone()] } }).unwrap();
    h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::AddSketchItems { sheet, items: vec![spline.clone()] } }).unwrap();
    assert_eq!(sketch(&doc), vec![line.clone(), spline.clone()]);
    // A grip drag and a move, each one step.
    let dragged = cadrs_drawing::sheet_sketch::drag_grip(&spline, cadrs_drawing::sheet_sketch::ItemGrip::Point(1), [40.0, 90.0]);
    h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::SetSketchItems { sheet, items: vec![dragged.clone()], label: "Drag spline point".into() } }).unwrap();
    let moved = cadrs_drawing::sheet_sketch::moved(&line, [5.0, -5.0]);
    h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::SetSketchItems { sheet, items: vec![moved.clone()], label: "Move line".into() } }).unwrap();
    assert_eq!(sketch(&doc), vec![moved.clone(), dragged.clone()]);
    h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::DeleteSketchItems { sheet, ids: vec![moved.id] } }).unwrap();
    assert_eq!(sketch(&doc), vec![dragged.clone()]);
    assert!(h.undo(&mut doc).is_some());
    assert_eq!(sketch(&doc), vec![moved.clone(), dragged.clone()]);
    assert!(h.undo(&mut doc).is_some());
    assert!(h.undo(&mut doc).is_some());
    assert_eq!(sketch(&doc), vec![line.clone(), spline.clone()]);
    assert!(h.undo(&mut doc).is_some());
    assert_eq!(sketch(&doc), vec![line.clone()]);
    assert!(h.undo(&mut doc).is_some());
    assert!(sketch(&doc).is_empty());
    assert!(h.redo(&mut doc).is_some());
    assert!(h.redo(&mut doc).is_some());
    assert_eq!(sketch(&doc), vec![line, spline]);
    // A one-point spline is refused.
    let bad = SketchItem::new(ItemKind::Spline { points: vec![[0.0, 0.0]] });
    assert!(h.execute(&mut doc, &EditDrawing { element: id, op: DrawingOp::AddSketchItems { sheet, items: vec![bad] } }).is_err());
}

// ---------------------------------------------------------------------------------------------
// The insert fixtures: an original logo drawn here (not copied from anywhere).

/// `fixtures/logo.dxf`: a small original mark, 16 × 5 mm to sit in the title block's Company
/// cell (a ring with an arc and a chevron inside, the word "cadrs" and a spline swoosh under
/// it), as an R2013 DXF in mm.
fn logo_dxf() -> String {
    use cadrs_drawing::export::{Layer, Pen, Text};
    let mut page = Page { name: "logo".into(), width: 17.0, height: 6.0, items: Vec::new() };
    let pen = || Pen::new(0.25, Layer::Import);
    page.items.push(Item::Stroke(Shape::Circle { center: [3.0, 3.0], radius: 2.5 }, pen()));
    page.items.push(Item::Stroke(Shape::Arc { center: [3.0, 3.0], radius: 1.6, start: 30.0, end: 330.0 }, pen()));
    page.items.push(Item::Stroke(Shape::Line { a: [3.0, 3.0], b: [4.6, 3.9] }, pen()));
    page.items.push(Item::Stroke(Shape::Line { a: [3.0, 3.0], b: [4.6, 2.1] }, pen()));
    let pts = [[7.0, 0.6], [10.0, 1.4], [13.0, 0.7], [16.5, 1.3]];
    let (knots, control) = cadrs_drawing::sheet_sketch::spline_nurbs(&pts);
    page.items.push(Item::Stroke(Shape::Spline { knots, control, fit: pts.to_vec(), points: Vec::new() }, pen()));
    page.items.push(Item::Fill { points: vec![[1.6, 2.5], [2.4, 3.0], [1.6, 3.5]], color: [0, 0, 0], layer: Layer::Import });
    page.items.push(Item::Text(Text {
        pos: [7.0, 2.3],
        height: 2.4,
        text: "cadrs".into(),
        bold: false,
        italic: false,
        rotation: 0.0,
        color: [0, 0, 0],
        layer: Layer::Import,
    }));
    cadrs_drawing::dxf::write_dxf(&page)
}

/// `fixtures/logo.png`: a 240 × 120 original badge drawn with tiny-skia.
fn logo_png() -> Vec<u8> {
    use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
    let mut pm = Pixmap::new(240, 120).unwrap();
    pm.fill(Color::from_rgba8(0, 0, 0, 0));
    let mut paint = Paint { anti_alias: true, ..Paint::default() };
    // A rounded blue badge.
    let mut pb = PathBuilder::new();
    let (x0, y0, x1, y1, r) = (4.0f32, 4.0f32, 236.0f32, 116.0f32, 18.0f32);
    pb.move_to(x0 + r, y0);
    pb.line_to(x1 - r, y0);
    pb.quad_to(x1, y0, x1, y0 + r);
    pb.line_to(x1, y1 - r);
    pb.quad_to(x1, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1);
    pb.quad_to(x0, y1, x0, y1 - r);
    pb.line_to(x0, y0 + r);
    pb.quad_to(x0, y0, x0 + r, y0);
    pb.close();
    let badge = pb.finish().unwrap();
    paint.set_color_rgba8(0x1f, 0x5f, 0xb4, 255);
    pm.fill_path(&badge, &paint, FillRule::Winding, Transform::identity(), None);
    // A white gear-ish ring and three bars.
    paint.set_color_rgba8(255, 255, 255, 255);
    let ring = PathBuilder::from_circle(60.0, 60.0, 34.0).unwrap();
    pm.stroke_path(&ring, &paint, &Stroke { width: 10.0, ..Stroke::default() }, Transform::identity(), None);
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        let tooth = PathBuilder::from_circle(60.0 + 42.0 * c, 60.0 + 42.0 * s, 7.0).unwrap();
        pm.fill_path(&tooth, &paint, FillRule::Winding, Transform::identity(), None);
    }
    paint.set_color_rgba8(0xf2, 0xb5, 0x1d, 255);
    for (i, w) in [90.0f32, 70.0, 100.0].iter().enumerate() {
        let rect = Rect::from_xywh(116.0, 30.0 + 24.0 * i as f32, *w, 14.0).unwrap();
        pm.fill_rect(rect, &paint, Transform::identity(), None);
    }
    pm.encode_png().unwrap()
}

#[test]
fn the_insert_fixtures_are_current() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let (dxf, png) = (logo_dxf(), logo_png());
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(dir.join("logo.dxf"), &dxf).unwrap();
        std::fs::write(dir.join("logo.png"), &png).unwrap();
    }
    assert_eq!(std::fs::read_to_string(dir.join("logo.dxf")).unwrap(), dxf, "fixtures/logo.dxf is out of date (CADRS_WRITE_FIXTURES=1)");
    assert_eq!(std::fs::read(dir.join("logo.png")).unwrap(), png, "fixtures/logo.png is out of date (CADRS_WRITE_FIXTURES=1)");
    let d = cadrs_drawing::dxf::read_dxf(&dxf).unwrap();
    assert_eq!(cadrs_drawing::dxf::counts(&d), (2, 1, 1));
    let b = block_from_dxf("logo.dxf", &d, [0.0, 0.0]).unwrap();
    assert_eq!(b.entities.len(), 7);
}
