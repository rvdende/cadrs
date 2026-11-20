//! A chain or part case's steps run on cadrs one by one, each step's body checked:
//! `cargo run -r --example chain -- <case id>` (CADRS_BOOL_DEBUG=1 for the boolean's story).
use brep_conformance::generate::{self, Step};
use brep_conformance::model::Shape;
use brep_protocol::Kernel;
use cadrs_conformance::{Cadrs, Solid};

fn build(k: &Cadrs, s: &Shape) -> Solid {
    match s {
        Shape::Prim(p) => k.make(p).expect("making a primitive"),
        Shape::Bool(op, a, b) => k
            .boolean(*op, &build(k, a), &build(k, b))
            .expect("a boolean of the shape"),
    }
}

fn main() {
    let id = std::env::args().nth(1).expect("a case id");
    let c = generate::cases(1)
        .into_iter()
        .find(|c| c.id == id)
        .expect("no case of that id");
    let steps = c.exact.chain.clone().expect("a chain case");
    let k = Cadrs;
    let mut s = build(&k, &c.a);
    for (i, st) in steps.iter().enumerate() {
        let r = match st {
            Step::Cut(t) => k.difference(&s, &build(&k, t)),
            Step::Join(t) => k.union(&s, &build(&k, t)),
            Step::Move(m) => k.transform(&s, *m),
            Step::MirrorJoin(m) => k.transform(&s, *m).and_then(|t| k.union(&s, &t)),
            Step::StepRoundTrip => k.write_step(&s).and_then(|t| k.read_step(&t)),
            Step::Act(f) => match f.act {
                generate::Act::Fillet => {
                    let t = std::time::Instant::now();
                    // (CHAIN_TIMES=n: the step n times over, for a profiler.)
                    let n: usize = std::env::var("CHAIN_TIMES")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1);
                    for _ in 1..n {
                        let _ = k.fillet(&s, &f.points, f.size);
                    }
                    let r = k.fillet(&s, &f.points, f.size);
                    println!(
                        "  fillet of {} edges by {}: {:?}",
                        f.points.len(),
                        f.size,
                        t.elapsed()
                    );
                    r
                }
                generate::Act::Chamfer => {
                    let t = std::time::Instant::now();
                    let r = k.chamfer(&s, &f.points, f.size);
                    println!("  chamfer by {}: {:?}", f.size, t.elapsed());
                    r
                }
                generate::Act::Offset => {
                    let t = std::time::Instant::now();
                    let r = k.offset(&s, f.size);
                    println!("  offset by {}: {:?}", f.size, t.elapsed());
                    r
                }
                _ => {
                    println!("step {}: act {:?} (not run here)", i + 1, f.act);
                    return;
                }
            },
        };
        match r {
            Ok(n) => {
                println!("step {}: {} ok, volume {:?}", i + 1, kind(st), k.volume(&n));
                s = n;
            }
            Err(e) => {
                println!("step {}: {} refused: {e}", i + 1, kind(st));
                if let Step::Cut(t) | Step::Join(t) = st {
                    println!("  tool: {t:?}");
                }
                return;
            }
        }
    }
}

fn kind(s: &Step) -> &'static str {
    match s {
        Step::Cut(_) => "cut",
        Step::Join(_) => "join",
        Step::Move(_) => "move",
        Step::MirrorJoin(_) => "mirror join",
        Step::StepRoundTrip => "STEP round trip",
        Step::Act(_) => "act",
    }
}
