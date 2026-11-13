//! Renders SVG files from disk into a labelled review sheet, so icons can be checked without
//! rebuilding the crate:
//!
//!     cargo run --release --example sheet -- OUT.png DIR [NAME...]
//!
//! DIR holds `<name>.svg` and `dark/<name>.svg`. Without names, every SVG in DIR is included.
//! Each cell shows the icon large, then at 24 px and 16 px, with its name; the light variants sit
//! on white above the dark variants on a dark band.

use resvg::{tiny_skia, usvg};
use std::path::Path;

const BIG: f32 = 96.0;
const COLS: usize = 6;
const CELL_W: f32 = BIG + 24.0 + 16.0 + 40.0;
const CELL_H: f32 = BIG + 34.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: sheet OUT.png DIR [NAME...]");
    let dir = args.next().expect("usage: sheet OUT.png DIR [NAME...]");
    let mut names: Vec<String> = args.collect();
    if names.is_empty() {
        names = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "svg"))
            .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
    }

    let rows = names.len().div_ceil(COLS);
    let band = rows as f32 * CELL_H + 12.0;
    let (w, h) = ((COLS as f32 * CELL_W + 12.0) as u32, (2.0 * band) as u32);
    let mut sheet = tiny_skia::Pixmap::new(w, h).unwrap();
    sheet.fill(tiny_skia::Color::WHITE);
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0x18, 0x18, 0x1b, 0xff);
    let dark_rect = tiny_skia::Rect::from_xywh(0.0, band, w as f32, band).unwrap();
    sheet.fill_rect(dark_rect, &paint, tiny_skia::Transform::identity(), None);

    let mut opts = usvg::Options::default();
    opts.fontdb_mut().load_system_fonts();
    let mut labels = String::new();

    for (dark, top) in [(false, 0.0), (true, band)] {
        for (i, name) in names.iter().enumerate() {
            let file = if dark {
                Path::new(&dir).join("dark").join(format!("{name}.svg"))
            } else {
                Path::new(&dir).join(format!("{name}.svg"))
            };
            let x = 12.0 + (i % COLS) as f32 * CELL_W;
            let y = top + 12.0 + (i / COLS) as f32 * CELL_H;
            let color = if dark { "#e4e4e7" } else { "#262626" };
            match std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|s| usvg::Tree::from_str(&s, &opts).map_err(|e| e.to_string()))
            {
                Ok(tree) => {
                    for (size, dx, dy) in [
                        (BIG, 0.0, 0.0),
                        (24.0, BIG + 12.0, BIG - 40.0),
                        (16.0, BIG + 48.0, BIG - 32.0),
                    ] {
                        let s = size / 24.0;
                        let t = tiny_skia::Transform::from_scale(s, s)
                            .post_translate((x + dx).round(), (y + dy).round());
                        resvg::render(&tree, t, &mut sheet.as_mut());
                    }
                }
                Err(e) => eprintln!("{name}: {e}"),
            }
            labels.push_str(&format!(
                r#"<text x="{}" y="{}" font-family="DejaVu Sans, Lato, sans-serif" font-size="12" fill="{color}">{name}</text>"#,
                x,
                y + BIG + 18.0
            ));
        }
    }

    let label_svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}">{labels}</svg>"#
    );
    let tree = usvg::Tree::from_str(&label_svg, &opts).unwrap();
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut sheet.as_mut());
    sheet.save_png(&out).unwrap();
    println!("wrote {out} ({} icons)", names.len());
}
