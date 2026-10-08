//! Converts saved EasyEDA part data (the component API's JSON) and reports what came across:
//! `cargo run -r -p cadrs_easyeda --example convert_json -- <file.json>…`.

fn main() {
    for path in std::env::args().skip(1) {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
        let result = if v["result"].is_object() { &v["result"] } else { &v };
        match cadrs_easyeda::convert(result, "LCSC", &Default::default()) {
            Ok(c) => {
                let f = &c.footprint;
                println!("{path}: {} ({} pins, {} graphics) / {} ({} pads, {} shapes, {:?})", c.symbol.id, c.symbol.pins.len(), c.symbol.graphics.len(), f.id, f.pads.len(), f.shapes.len(), f.attrs.mount);
                for w in &c.warnings {
                    println!("  warning: {w}");
                }
                for e in cadrs_eda::lib_edit::check_footprint(f) {
                    println!("  check: {e}");
                }
                if let Some(m) = &c.model {
                    println!("  model {} at {:?} z {} rot {:?}", m.name, m.center, m.z, m.rotation);
                }
            }
            Err(e) => println!("{path}: {e}"),
        }
    }
}
