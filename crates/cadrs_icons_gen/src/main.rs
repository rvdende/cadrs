//! Generates cadrs_icons' icon set (see `crates/cadrs_icons/HOWTO.md`):
//!
//!     cargo run -r -p cadrs_icons_gen -- [--only MODULE] [--out DIR] [--check]

#[macro_use]
mod common;
mod generate;
mod icons;
#[allow(dead_code)]
#[path = "../../cadrs_icons/src/palette.rs"]
mod palette;

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("cadrs_icons");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = generate::main(&root, &args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
