//! Prints the faces of a Part Studio's parts as an import built them, one line each (surface
//! kind, plane normal and offset, area mm², bounding box mm), to compare with Onshape's
//! `bodydetails.json` and find where an imported part differs.
//!
//! `cargo run -r -p cadrs_onshape --example faces_dump -- <store dir> <Part Studio name>`

use cadrs_core::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let store = Store::new(args.next().expect("a document store"));
    let name = args.next().expect("a Part Studio name");
    let (lib, _) = store.list();
    for entry in &lib.entries {
        let Ok(file) = store.load(entry.id) else { continue };
        for el in file.document.elements.iter().filter(|e| e.name == name && e.features_mut_free()) {
            let b = cadrs_core::rebuild::build(&el.active_features());
            for p in &b.parts {
                let s = &p.solid;
                println!("part {} vol {:.1}", p.name, p.mass.as_ref().map_or(0.0, |m| m.volume));
                for (i, f) in s.faces.iter().enumerate() {
                    let tris = &s.indices[f.first_triangle * 3..(f.first_triangle + f.triangle_count) * 3];
                    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
                    let mut area = 0.0;
                    for t in tris.chunks_exact(3) {
                        let [a, b, c] = [s.positions[t[0] as usize], s.positions[t[1] as usize], s.positions[t[2] as usize]];
                        for q in [a, b, c] {
                            for k in 0..3 {
                                lo[k] = lo[k].min(q[k]);
                                hi[k] = hi[k].max(q[k]);
                            }
                        }
                        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                        area += 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    }
                    let plane = f.plane.map(|pl| {
                        let n = [pl.u[1] * pl.v[2] - pl.u[2] * pl.v[1], pl.u[2] * pl.v[0] - pl.u[0] * pl.v[2], pl.u[0] * pl.v[1] - pl.u[1] * pl.v[0]];
                        let d = n[0] * pl.origin[0] + n[1] * pl.origin[1] + n[2] * pl.origin[2];
                        format!("PLANE n ({:.3} {:.3} {:.3}) d {:.3}", n[0], n[1], n[2], d)
                    });
                    println!(
                        "  {i:3} {:<40} area {:9.2}  box [{:.2} {:.2} {:.2}] .. [{:.2} {:.2} {:.2}]",
                        plane.unwrap_or_else(|| format!("{:?}", f.kind)),
                        area,
                        lo[0],
                        lo[1],
                        lo[2],
                        hi[0],
                        hi[1],
                        hi[2]
                    );
                }
            }
        }
    }
}

trait PartStudio {
    fn features_mut_free(&self) -> bool;
}

impl PartStudio for cadrs_core::Element {
    /// A Part Studio (it has a feature list).
    fn features_mut_free(&self) -> bool {
        matches!(self.kind, cadrs_core::ElementKind::PartStudio { .. })
    }
}
