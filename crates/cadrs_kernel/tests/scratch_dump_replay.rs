//! (Scratch, not for committing: replays a dumped blend from CADRS_REPLAY=<dump stem>.)
use cadrs_kernel::native::*;
use cadrs_kernel::*;

#[test]
#[ignore]
fn replay_dump() {
    let stem = std::env::var("CADRS_REPLAY").unwrap();
    let body = bodyio::read(&std::fs::read(format!("{stem}.body")).unwrap()).unwrap();
    let txt = std::fs::read_to_string(format!("{stem}.txt")).unwrap();
    let ids: Vec<EdgeId> = txt
        .split("EdgeId(")
        .skip(1)
        .map(|x| EdgeId(x.split(')').next().unwrap().parse().unwrap()))
        .collect();
    let r: f64 = txt
        .split("radius ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut k = NativeKernel::new();
    let b = k.insert(body);
    let res = k.fillet(b, &ids, r);
    println!(
        "{:?}",
        res.as_ref()
            .map(|r| r.bodies.len())
            .map_err(|e| e.to_string())
    );
}

#[test]
#[ignore]
fn show_dump_entities() {
    let stem = std::env::var("CADRS_REPLAY").unwrap();
    let b = bodyio::read(&std::fs::read(format!("{stem}.body")).unwrap()).unwrap();
    for e in std::env::var("CADRS_EDGES")
        .unwrap()
        .split(',')
        .map(|x| x.parse::<usize>().unwrap())
    {
        let ed = &b.edges[e];
        let fs: Vec<(usize, bool)> = b
            .faces
            .iter()
            .enumerate()
            .flat_map(|(fi, f)| {
                f.loops
                    .iter()
                    .flatten()
                    .filter(move |c| c.edge == e)
                    .map(move |c| (fi, c.fwd))
            })
            .collect();
        println!(
            "edge {e}: {:?} .. {:?}  faces {:?}",
            ed.point(ed.t[0]),
            ed.point(ed.t[1]),
            fs
        );
        for (fi, _) in &fs {
            let f = &b.faces[*fi];
            let surf = format!("{:?}", f.surf);
            println!(
                "  face {fi}: {} loops {:?}",
                &surf[..surf.len().min(160)],
                f.loops
                    .iter()
                    .map(|l| l.iter().map(|c| c.edge).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            );
        }
    }
}
