//! Lists what a KiCad project holds, as cadrs reads it: `cargo run -r -p cadrs_kicad --example
//! inventory -- <project dir>`.

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("project dir"));
    let p = cadrs_kicad::read_project(&dir).unwrap();
    let d = &p.design;
    for (i, sh) in d.schematic.sheets.iter().enumerate() {
        println!("sheet {i} {} paper {:?}", sh.name, sh.paper.name);
        for s in &sh.symbols {
            println!("  sym {} {} {} at {:?} angle {} mirror {:?} fp {:?}", s.reference(), s.symbol, s.value(), s.placement.at, s.placement.angle, s.placement.mirror, s.field("Footprint").map(|f| f.value()));
        }
        for w in &sh.wires {
            println!("  wire {:?} -> {:?} color {:?} width {}", w.a, w.b, w.stroke.color, w.stroke.width);
        }
        for l in &sh.labels {
            println!("  label {:?} {} at {:?} angle {}", l.kind, l.text.text, l.text.at, l.text.angle);
        }
        println!("  junctions {} no-connects {} notes {} drawings {}", sh.junctions.len(), sh.no_connects.len(), sh.notes.len(), sh.drawings.len());
    }
    let b = &d.board;
    for f in &b.footprints {
        println!("fp {} {} at {:?} angle {} side {:?} models {:?}", f.reference(), f.footprint.id, f.placement.at, f.placement.angle, f.placement.side, f.footprint.models.iter().map(|m| (&m.source, m.offset, m.rotation)).collect::<Vec<_>>());
    }
    println!("tracks {} vias {} zones {} shapes {} texts {}", b.tracks.len(), b.vias.len(), b.zones.len(), b.shapes.len(), b.texts.len());
    for s in &b.shapes {
        println!("  shape {:?} {:?}", s.layer, s.shape.geom);
    }
    for t in &b.texts {
        println!("  text {:?} {:?} at {:?}", t.layer, t.text.text, t.text.at);
    }
    for z in &b.zones {
        println!("  zone {:?} net {:?} layers {:?} pts {}", z.name, z.net, z.layers, z.outline.len());
    }
}
