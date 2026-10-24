//! Writes the "Getting Started" course's schematic and board as SVG and PNG (a quick look
//! without the app): `cargo run -r -p cadrs_eda --example course_svg -- <out dir>`.

use cadrs_eda::render::*;
use resvg::{tiny_skia, usvg};

fn save(out: &std::path::Path, name: &str, d: &DrawList, px_per_mm: f32) {
    let svg = to_svg(d);
    std::fs::write(out.join(format!("{name}.svg")), &svg).unwrap();
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
    let size = tree.size();
    // The SVG's size is in mm (96 dpi in usvg's units: 3.78 px per mm).
    let k = px_per_mm / 3.7795;
    let mut pix =
        tiny_skia::Pixmap::new((size.width() * k) as u32, (size.height() * k) as u32).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(k, k),
        &mut pix.as_mut(),
    );
    pix.save_png(out.join(format!("{name}.png"))).unwrap();
}

fn main() {
    let out = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/course_svg".into()),
    );
    std::fs::create_dir_all(&out).unwrap();
    let (d, _, _) = cadrs_eda::getting_started::gs25();
    save(
        &out,
        "schematic",
        &schematic(
            &d.schematic,
            0,
            &SchematicTheme::default(),
            &Highlight::default(),
        ),
        4.0,
    );
    // The dark scheme, its hairlines 1.5 px wide as the app draws them.
    let px_per_mm = 8.0;
    let mut dark = schematic(
        &d.schematic,
        0,
        &SchematicTheme::dark(),
        &Highlight::default(),
    );
    for l in dark.lines.iter_mut().filter(|l| l.width == 0) {
        l.width = (1.5e6 / px_per_mm) as i64;
    }
    save(&out, "schematic_dark", &dark, px_per_mm);
    // A bigger one: the power monitor (an IC, horizontal labels).
    let (pm, _) = cadrs_eda::power_monitor::design();
    // As drawn, and with the MOSI net under the pointer (lit).
    let mosi = pm.schematic.sheets[0]
        .labels
        .iter()
        .find(|l| l.text.text == "MOSI")
        .and_then(|l| cadrs_eda::sch_edit::net_items_at(&pm.schematic, 0, l.text.at, 100_000))
        .map(|(_, items)| items)
        .unwrap_or_default();
    for (name, hover) in [
        ("power_monitor_dark", vec![]),
        ("power_monitor_hover", mosi),
    ] {
        let mut d = schematic(
            &pm.schematic,
            0,
            &SchematicTheme::dark(),
            &Highlight {
                hover,
                ..Default::default()
            },
        );
        for l in d.lines.iter_mut().filter(|l| l.width == 0) {
            l.width = (1.5e6 / px_per_mm) as i64;
        }
        save(&out, name, &d, px_per_mm);
    }
    save(
        &out,
        "board",
        &board(&d.board, &BoardTheme::default(), &BoardView::all(&d.board)),
        16.0,
    );
    println!("{}", out.display());
}
