//! Renders every icon into one contact sheet, light variants on white above dark variants on a
//! dark band: `cargo run --example preview [out.png] [scale]`.

use resvg::{tiny_skia, usvg};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "target/preview.png".into());
    let scale: f32 = args
        .next()
        .map_or(4.0, |s| s.parse().expect("scale must be a number"));

    let cell = 24.0 * scale + 16.0;
    let cols = 6u32;
    let rows = (cadrs_icons::ALL.len() as u32).div_ceil(cols);
    let band = rows as f32 * cell;
    let mut sheet = tiny_skia::Pixmap::new(cols * cell as u32, 2 * band as u32).unwrap();
    sheet.fill(tiny_skia::Color::WHITE);
    let dark_band = tiny_skia::Rect::from_xywh(0.0, band, cols as f32 * cell, band).unwrap();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(0x18, 0x18, 0x1b, 0xff);
    sheet.fill_rect(dark_band, &paint, tiny_skia::Transform::identity(), None);

    for (dark, top) in [(false, 0.0), (true, band)] {
        for (i, icon) in cadrs_icons::ALL.iter().enumerate() {
            let tree = usvg::Tree::from_str(icon.themed(dark), &usvg::Options::default())
                .unwrap_or_else(|e| panic!("{}: {e}", icon.name));
            let (col, row) = (i as u32 % cols, i as u32 / cols);
            let transform = tiny_skia::Transform::from_scale(scale, scale)
                .post_translate(col as f32 * cell + 8.0, top + row as f32 * cell + 8.0);
            resvg::render(&tree, transform, &mut sheet.as_mut());
        }
    }
    sheet.save_png(&out).unwrap();
    println!("wrote {out}");
}
