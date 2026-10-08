//! Writes the built-in libraries (`libraries/` at the repository root) from parametric
//! generators, drawn for cadrs (no other tool's library data), with KiCad's library, part and
//! footprint names so designs read the same. The app never runs this: it loads the files.
//! Rerun it after changing a generator (it overwrites the libraries it makes):
//! `cargo run -r -p cadrs_libgen -- libraries`.
//!
//! - Symbols ([`symbols`]): `Device` (passives, diodes, transistors, crystal, antenna, …),
//!   `Switch`, `Connector` (1×N and 2×N generic connectors, screw terminals, test point),
//!   `power` (supply and ground ports, PWR_FLAG), `Regulator_Linear`, `Amplifier_Operational`,
//!   `LED`. Pins on the 1.27 mm grid.
//! - Footprints ([`footprints`]): chips 0201–2512, SOD/SMA diodes, SOT-23/223, SOIC/TSSOP/MSOP,
//!   QFN, LQFP/TQFP, pin headers and sockets, JST PH/XH, crystals, radial electrolytics, axial
//!   and TO-92 parts, mounting holes, test points, a push button, WS2812B. Each has a
//!   generated 3D body ([`cadrs_eda::model3d`]).

use cadrs_eda::footprint::*;
use cadrs_eda::graphics::{Fill, Geom, HAlign, Shape, Stroke};
use cadrs_eda::layer::{Layer, LayerSet};
use cadrs_eda::library::{Library, Scope};
use cadrs_eda::symbol::*;
use cadrs_eda::units::{Pt, Size, mm};

fn p(x: f64, y: f64) -> Pt {
    Pt::mm(x, y)
}

fn line(pts: &[(f64, f64)], w: f64, fill: Fill) -> SymbolGraphic {
    let pts: Vec<Pt> = pts.iter().map(|&(x, y)| p(x, y)).collect();
    let closed = fill != Fill::None && pts.len() > 2;
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Polyline { pts, closed }, stroke: Stroke::width(mm(w)), fill }), unit: 0, style: 0 }
}

fn rect(a: (f64, f64), b: (f64, f64), w: f64, fill: Fill) -> SymbolGraphic {
    SymbolGraphic { item: SymbolItem::Shape(Shape { geom: Geom::Rect { a: p(a.0, a.1), b: p(b.0, b.1) }, stroke: Stroke::width(mm(w)), fill }), unit: 0, style: 0 }
}


/// A two-pin vertical part (pins 1 at the top, 2 at the bottom, 3.81 mm from the centre).
fn two_pin_vertical(name: &str, prefix: &str, description: &str, keywords: &str, filters: &[&str]) -> Symbol {
    let mut s = new_symbol(name, prefix, description);
    s.keywords = keywords.into();
    s.footprint_filters = filters.iter().map(|f| f.to_string()).collect();
    s.show_pin_names = false;
    s.show_pin_numbers = false;
    s.pins = vec![pin("1", "~", PinType::Passive, (0.0, 3.81), 270.0, 1.27), pin("2", "~", PinType::Passive, (0.0, -3.81), 90.0, 1.27)];
    s
}

fn fp_shape(geom: Geom, layer: Layer, width: f64) -> FpShape {
    FpShape { id: uuid::Uuid::new_v4(), shape: Shape { geom, stroke: Stroke::width(mm(width)), fill: Fill::None }, layer }
}

/// A model file reference with its generated body (shown when the file isn't there).
fn model(path: &str, body: cadrs_eda::model3d::Body) -> Model3d {
    Model3d { body: Some(body), ..Model3d::file(path) }
}

fn rect_geom(x0: f64, y0: f64, x1: f64, y1: f64) -> Geom {
    Geom::Rect { a: p(x0, y0), b: p(x1, y1) }
}

mod footprints;
mod symbols;

/// Every built-in library.
fn libraries() -> Vec<Library> {
    let mut out = vec![symbols::device(), symbols::power(), symbols::switch(), symbols::connector_lib(), symbols::regulators(), symbols::opamps(), symbols::leds()];
    out.extend(footprints::footprint_libraries());
    out
}

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: cadrs_libgen <libraries dir>");
        std::process::exit(2);
    };
    let dir = std::path::Path::new(&dir);
    for lib in libraries() {
        let path = dir.join(&lib.name);
        // Start clean so renamed or dropped parts don't linger.
        let _ = std::fs::remove_dir_all(&path);
        if let Err(e) = cadrs_eda::library::save_library(&lib, &path) {
            eprintln!("{}: {e}", lib.name);
            std::process::exit(1);
        }
        println!("{:<40} {:>4} symbols {:>4} footprints", lib.name, lib.symbols.len(), lib.footprints.len());
    }
}
