//! Scratch: every case whose id starts with the argument, its act run in order in one process
//! (as one adapter serves them), each outcome printed.
use brep_conformance::generate;
use brep_conformance::model::Shape;
use brep_protocol::Kernel;
use cadrs_conformance::Cadrs;

fn main() {
    let prefix = std::env::args().nth(1).expect("an id prefix");
    let k = Cadrs;
    for c in generate::cases(1)
        .into_iter()
        .filter(|c| c.id.starts_with(&prefix))
    {
        let Some(f) = c.feature.as_ref() else {
            continue;
        };
        let Shape::Prim(p) = &c.a else { continue };
        let a = k.make(p).expect("a");
        let r = match f.act {
            generate::Act::FilletSetbackG2 => {
                k.fillet_setback_g2(&a, &f.points, f.size, f.radii[0])
            }
            generate::Act::FilletSetback => k.fillet_setback(&a, &f.points, f.size, f.radii[0]),
            _ => continue,
        };
        println!(
            "{}: {}",
            c.id,
            match &r {
                Ok(_) => "ok".to_string(),
                Err(e) => format!("{e:?}"),
            }
        );
    }
}
