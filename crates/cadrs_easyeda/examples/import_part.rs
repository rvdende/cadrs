//! Downloads a part from the JLCPCB/LCSC catalogue into a library folder, as the library
//! browser's Add to library does: `cargo run -r -p cadrs_easyeda --example import_part --
//! C2764087 ~/.local/share/cadrs/libraries/LCSC`.

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(lcsc), Some(dir)) = (args.next(), args.next()) else {
        eprintln!("usage: import_part <LCSC number> <library folder>");
        std::process::exit(2);
    };
    match cadrs_easyeda::import(&lcsc, std::path::Path::new(&dir), &Default::default()) {
        Ok(r) => {
            println!("{} / {} ({:?})", r.symbol, r.footprint, r.model);
            for w in r.warnings {
                println!("  warning: {w}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
