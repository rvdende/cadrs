//! Draws the built-in libraries as two contact sheets (a quick look without the app):
//! `cargo run -r -p cadrs_eda --example library_gallery -- <out dir>` writes `symbols.png` (every
//! symbol but the long connector series) and `footprints.png` (one of each footprint family).

use cadrs_eda::board::{Board, PlacedFootprint};
use cadrs_eda::footprint::FootprintPlacement;
use cadrs_eda::library::LibraryTable;
use cadrs_eda::render::*;
use cadrs_eda::schematic::Paper;
use cadrs_eda::units::{Pt, Size};
use resvg::{tiny_skia, usvg};

fn save(out: &std::path::Path, name: &str, d: &DrawList, px_per_mm: f32) {
    let svg = to_svg(d);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
    let size = tree.size();
    let k = px_per_mm / 3.7795;
    let mut pix = tiny_skia::Pixmap::new((size.width() * k) as u32, (size.height() * k) as u32).unwrap();
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    pix.save_png(out.join(format!("{name}.png"))).unwrap();
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "target/library_gallery".into()));
    std::fs::create_dir_all(&out).unwrap();
    let t = LibraryTable::builtin();

    // Symbols on a grid, 25.4 mm apart.
    let mut d = cadrs_eda::Design::new();
    d.schematic.sheets[0].paper = Paper { name: "User".into(), size: Size::mm(330.0, 230.0) };
    let shown = |id: &str| !id.starts_with("Connector:Conn_") || id.ends_with("01x04") || id.ends_with("02x05_Odd_Even");
    let ids: Vec<_> = t.libraries.iter().flat_map(|l| l.symbols.iter()).filter(|s| shown(&s.id) && !s.id.starts_with("Connector:Screw_Terminal_01x0") || s.id.ends_with("Terminal_01x03")).cloned().collect();
    for (i, s) in ids.iter().enumerate() {
        let (c, r) = ((i % 12) as f64, (i / 12) as f64);
        cadrs_eda::sch_edit::place_symbol(&mut d.schematic, 0, s, Pt::mm(20.32 + c * 25.4, 210.82 - r * 27.94), uuid::Uuid::new_v4());
    }
    save(&out, "symbols", &schematic(&d.schematic, 0, &SchematicTheme::default(), &Highlight::default()), 5.0);

    // One footprint of each family on a board, 14 mm apart (wider ones get more room).
    let picks = [
        "Resistor_SMD:R_0402_1005Metric",
        "Capacitor_SMD:C_0805_2012Metric",
        "Inductor_SMD:L_1210_3225Metric",
        "LED_SMD:LED_0603_1608Metric",
        "Diode_SMD:D_SOD-123",
        "Diode_SMD:D_SMA",
        "Package_TO_SOT_SMD:SOT-23",
        "Package_TO_SOT_SMD:SOT-23-6",
        "Package_TO_SOT_SMD:SOT-223-3_TabPin2",
        "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm",
        "Package_SO:TSSOP-20_4.4x6.5mm_P0.65mm",
        "Package_DFN_QFN:QFN-32-1EP_5x5mm_P0.5mm_EP3.45x3.45mm",
        "Package_QFP:LQFP-48_7x7mm_P0.5mm",
        "Crystal:Crystal_SMD_3225-4Pin_3.2x2.5mm",
        "LED_SMD:LED_WS2812B_PLCC4_5.0x5.0mm_P3.2mm",
        "Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical",
        "Connector_PinHeader_2.54mm:PinHeader_2x05_P2.54mm_Vertical",
        "Connector_JST:JST_PH_B4B-PH-K_1x04_P2.00mm_Vertical",
        "Connector_JST:JST_XH_B3B-XH-A_1x03_P2.50mm_Vertical",
        "Capacitor_THT:CP_Radial_D6.3mm_P2.50mm",
        "Package_TO_SOT_THT:TO-92_Inline",
        "Button_Switch_THT:SW_PUSH_6mm",
        "MountingHole:MountingHole_3.2mm_M3_Pad",
        "TestPoint:TestPoint_Pad_D1.5mm",
    ];
    let mut b = Board::default();
    for (i, id) in picks.iter().enumerate() {
        let f = t.footprint(id).unwrap_or_else(|| panic!("{id}"));
        let (c, r) = ((i % 6) as f64, (i / 6) as f64);
        b.footprints.push(PlacedFootprint {
            id: uuid::Uuid::new_v4(),
            footprint: f.clone(),
            placement: FootprintPlacement { at: Pt::mm(8.0 + c * 16.0, -8.0 - r * 18.0), ..Default::default() },
            locked: false,
            symbol: None,
        });
    }
    save(&out, "footprints", &board(&b, &BoardTheme::default(), &BoardView::all(&b)), 12.0);
    println!("{}", out.display());
}
