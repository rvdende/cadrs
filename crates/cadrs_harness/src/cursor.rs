//! Software cursor sprites for screenshots.
//!
//! Screenshots never contain the OS cursor, so judges could not see which cursor the app shows.
//! With the scenario option `cursor: true` (the default) the harness paints a sprite of the
//! current [`CursorKind`] at the synthetic pointer into every screenshot. The sprites are drawn
//! after the classic desktop cursors: white arrow and hand with a black outline; black caret,
//! crosshair and move arrows with a white halo so they read on any background.

use image::RgbImage;
use cadrs_ui::CursorKind;

/// A sprite: rows of `B` (black), `W` (white) and ` ` (transparent), and its hotspot.
struct Sprite {
    rows: &'static [&'static str],
    hot: (i32, i32),
    /// Add a 1 px white halo around the black pixels.
    halo: bool,
}

const ARROW: Sprite = Sprite {
    rows: &[
        "B           ",
        "BB          ",
        "BWB         ",
        "BWWB        ",
        "BWWWB       ",
        "BWWWWB      ",
        "BWWWWWB     ",
        "BWWWWWWB    ",
        "BWWWWWWWB   ",
        "BWWWWWWWWB  ",
        "BWWWWWWWWWB ",
        "BWWWWWWBBBBB",
        "BWWWBWWB    ",
        "BWWB BWWB   ",
        "BWB  BWWB   ",
        "BB    BWWB  ",
        "B     BWWB  ",
        "       BB   ",
    ],
    hot: (0, 0),
    halo: false,
};

const HAND: Sprite = Sprite {
    rows: &[
        "     BB          ",
        "    BWWB         ",
        "    BWWB         ",
        "    BWWB         ",
        "    BWWB         ",
        "    BWWBBB       ",
        "    BWWBWWBBB    ",
        "    BWWBWWBWWBB  ",
        "    BWWBWWBWWBWB ",
        " BB BWWWWWWWWBWWB",
        "BWWBBWWWWWWWWWWWB",
        "BWWWBWWWWWWWWWWWB",
        " BWWBWWWWWWWWWWWB",
        "  BWBWWWWWWWWWWWB",
        "  BWWWWWWWWWWWWWB",
        "   BWWWWWWWWWWWB ",
        "   BWWWWWWWWWWWB ",
        "    BWWWWWWWWWB  ",
        "    BWWWWWWWWWB  ",
        "    BBBBBBBBBBB  ",
    ],
    hot: (5, 0),
    halo: false,
};

const GRABBING: Sprite = Sprite {
    rows: &[
        "    BB BB BB     ",
        "   BWWBWWBWWBB   ",
        "   BWWWWWWWWBWB  ",
        " BBBWWWWWWWWWWB  ",
        "BWWBWWWWWWWWWWB  ",
        "BWWWWWWWWWWWWWB  ",
        " BWWWWWWWWWWWWB  ",
        "  BWWWWWWWWWWB   ",
        "   BWWWWWWWWWB   ",
        "    BWWWWWWWB    ",
        "    BWWWWWWWB    ",
        "    BBBBBBBBB    ",
    ],
    hot: (8, 6),
    halo: false,
};

const TEXT: Sprite = Sprite {
    rows: &[
        "BBB BBB", "   B   ", "   B   ", "   B   ", "   B   ", "   B   ", "   B   ", "   B   ",
        "   B   ", "   B   ", "   B   ", "   B   ", "   B   ", "   B   ", "   B   ", "BBB BBB",
    ],
    hot: (3, 8),
    halo: true,
};

const CROSSHAIR: Sprite = Sprite {
    rows: &[
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "                     ",
        "                     ",
        "BBBBBBBB     BBBBBBBB",
        "                     ",
        "                     ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
        "          B          ",
    ],
    hot: (10, 10),
    halo: true,
};

const MOVE: Sprite = Sprite {
    rows: &[
        "         B         ",
        "        BBB        ",
        "       BBBBB       ",
        "      BBBBBBB      ",
        "         B         ",
        "         B         ",
        "   B     B     B   ",
        "  BB     B     BB  ",
        " BBB     B     BBB ",
        "BBBBBBBBBBBBBBBBBBB",
        " BBB     B     BBB ",
        "  BB     B     BB  ",
        "   B     B     B   ",
        "         B         ",
        "         B         ",
        "      BBBBBBB      ",
        "       BBBBB       ",
        "        BBB        ",
        "         B         ",
    ],
    hot: (9, 9),
    halo: true,
};

const ROW_RESIZE: Sprite = Sprite {
    rows: &[
        "    B    ",
        "   BBB   ",
        "  BBBBB  ",
        " BBBBBBB ",
        "    B    ",
        "BBBBBBBBB",
        "         ",
        "BBBBBBBBB",
        "    B    ",
        " BBBBBBB ",
        "  BBBBB  ",
        "   BBB   ",
        "    B    ",
    ],
    hot: (4, 6),
    halo: true,
};

const COL_RESIZE: Sprite = Sprite {
    rows: &[
        "     B B     ",
        "   B B B B   ",
        "  BB B B BB  ",
        " BBB B B BBB ",
        "BBBBBB BBBBBB",
        " BBB B B BBB ",
        "  BB B B BB  ",
        "   B B B B   ",
        "     B B     ",
    ],
    hot: (6, 4),
    halo: true,
};

const NOT_ALLOWED: Sprite = Sprite {
    rows: &[
        "     BBBBBB     ",
        "   BBBBBBBBBB   ",
        "  BBB      BBB  ",
        " BBBB       BBB ",
        " BBBBB       BB ",
        "BBB BBB      BBB",
        "BB   BBB      BB",
        "BB    BBB     BB",
        "BB     BBB    BB",
        "BB      BBB   BB",
        "BBB      BBB BBB",
        " BB       BBBBB ",
        " BBB       BBBB ",
        "  BBB      BBB  ",
        "   BBBBBBBBBB   ",
        "     BBBBBB     ",
    ],
    hot: (8, 8),
    halo: true,
};

fn sprite(kind: CursorKind) -> &'static Sprite {
    match kind {
        CursorKind::Default => &ARROW,
        CursorKind::Pointer => &HAND,
        CursorKind::Text => &TEXT,
        CursorKind::Crosshair => &CROSSHAIR,
        CursorKind::Move => &MOVE,
        CursorKind::Grabbing => &GRABBING,
        CursorKind::NotAllowed => &NOT_ALLOWED,
        CursorKind::RowResize => &ROW_RESIZE,
        CursorKind::ColResize => &COL_RESIZE,
    }
}

/// Paints the cursor `kind` with its hotspot at `pos` (image pixels).
pub fn draw_cursor(img: &mut RgbImage, kind: CursorKind, pos: (f32, f32)) {
    let s = sprite(kind);
    let ox = pos.0.round() as i32 - s.hot.0;
    let oy = pos.1.round() as i32 - s.hot.1;
    let cell = |x: i32, y: i32| -> u8 {
        if x < 0 || y < 0 {
            return b' ';
        }
        s.rows
            .get(y as usize)
            .and_then(|r| r.as_bytes().get(x as usize))
            .copied()
            .unwrap_or(b' ')
    };
    let (w, h) = (img.width() as i32, img.height() as i32);
    let mut put = |x: i32, y: i32, v: u8| {
        let (px, py) = (ox + x, oy + y);
        if px >= 0 && py >= 0 && px < w && py < h {
            img.put_pixel(px as u32, py as u32, image::Rgb([v, v, v]));
        }
    };
    let height = s.rows.len() as i32;
    let width = s.rows.iter().map(|r| r.len()).max().unwrap_or(0) as i32;
    if s.halo {
        for y in -1..=height {
            for x in -1..=width {
                if cell(x, y) != b' ' {
                    continue;
                }
                let near = (-1..=1).any(|dy| (-1..=1).any(|dx| cell(x + dx, y + dy) == b'B'));
                if near {
                    put(x, y, 255);
                }
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            match cell(x, y) {
                b'B' => put(x, y, 0),
                b'W' => put(x, y, 255),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites_are_drawn_at_their_hotspot() {
        for kind in [
            CursorKind::Default,
            CursorKind::Pointer,
            CursorKind::Text,
            CursorKind::Crosshair,
            CursorKind::Move,
            CursorKind::Grabbing,
            CursorKind::NotAllowed,
        ] {
            let mut img = RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 128]));
            draw_cursor(&mut img, kind, (30.0, 30.0));
            let changed = img.pixels().filter(|p| p.0 != [128, 128, 128]).count();
            assert!(changed > 20, "{kind:?} drew {changed} pixels");
            // Rows of a sprite have equal widths (a typo would shift the art).
            let s = sprite(kind);
            let w = s.rows[0].len();
            assert!(s.rows.iter().all(|r| r.len() == w), "{kind:?} rows differ in width");
        }
        // Drawing off the image edge is fine.
        let mut img = RgbImage::new(8, 8);
        draw_cursor(&mut img, CursorKind::Pointer, (-3.0, 6.0));
    }
}
