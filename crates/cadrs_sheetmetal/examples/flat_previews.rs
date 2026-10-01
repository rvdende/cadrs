//! Renders the sample models' flat patterns to PNG (and SVG) for checking them by eye:
//! `cargo run -p cadrs_sheetmetal --example flat_previews [out_dir]` (default
//! `target/sheetmetal`).

use cadrs_sheetmetal::flat::flatten;
use cadrs_sheetmetal::model::{Model, RipStyle};
use cadrs_sheetmetal::params::{BendReliefKind, CornerReliefKind, Params};
use cadrs_sheetmetal::{samples, svg};

fn base() -> Params {
    Params {
        thickness: 2.0,
        bend_radius: 3.0,
        ..Default::default()
    }
}

fn with_corner(kind: CornerReliefKind) -> Params {
    let mut p = base();
    p.corner_relief.kind = kind;
    p.corner_relief.size = 12.0;
    p.corner_relief.scale = 1.5;
    p
}

fn with_bend_relief(kind: BendReliefKind, extend: bool) -> Params {
    let mut p = base();
    p.bend_relief.kind = kind;
    p.bend_relief.depth = 4.0;
    p.bend_relief.extend = extend;
    p
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "target/sheetmetal".into());
    std::fs::create_dir_all(&out).expect("output folder");
    let ok = |m: Result<Model, _>| m.expect("sample builds");
    let mut cases: Vec<(&str, Model)> = vec![
        ("01-l-bracket-up", ok(samples::l_bracket(base(), true))),
        ("02-l-bracket-down", ok(samples::l_bracket(base(), false))),
        ("03-u-channel", ok(samples::u_channel(base()))),
        ("04-hem", samples::hem(base())),
        ("05-tube", samples::tube(base())),
        ("06-wall-into-half-tube", samples::wall_into_half_tube(base())),
        ("07-box-butt-direction-1", ok(samples::open_box(base(), RipStyle::ButtDirection1))),
    ];
    for kind in CornerReliefKind::ALL {
        let name: &'static str = Box::leak(format!("10-box-corner-{}", kind.label().to_lowercase().replace(" - ", "-")).into_boxed_str());
        cases.push((name, ok(samples::open_box(with_corner(kind), RipStyle::EdgeJoint))));
    }
    for (kind, extend) in BendReliefKind::ALL.iter().map(|k| (*k, false)).chain([(BendReliefKind::RectangleScaled, true)]) {
        let label = kind.label().to_lowercase().replace(" - ", "-");
        let name: &'static str = Box::leak(format!("20-partial-flange-{label}{}", if extend { "-extended" } else { "" }).into_boxed_str());
        cases.push((name, ok(samples::partial_flange(with_bend_relief(kind, extend)))));
    }
    cases.push(("30-collision", samples::hook_collision(base())));
    cases.push(("31-bend-loop", ok(samples::bend_loop(base()))));

    let mut opt = resvg::usvg::Options::default();
    // The project's Inter (CLAUDE.md), else whatever sans-serif the system has.
    let fonts = opt.fontdb_mut();
    fonts.load_font_file(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/Inter-Regular.ttf")).ok();
    fonts.load_system_fonts();
    fonts.set_sans_serif_family("Inter");
    for (name, model) in cases {
        let flat = flatten(&model);
        let doc = svg::flat_svg(&flat, name, 640.0);
        std::fs::write(format!("{out}/{name}.svg"), &doc).expect("write svg");
        let tree = resvg::usvg::Tree::from_str(&doc, &opt).expect("valid svg");
        let size = tree.size().to_int_size();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).expect("pixmap");
        resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
        pixmap.save_png(format!("{out}/{name}.png")).expect("write png");
        println!("{name}: {} part(s), {}", flat.parts.len(), if flat.is_ok() { "ok".to_string() } else { flat.errors[0].message() });
    }
}
