//! Scratch: faces of a dumped body, their surfaces in full.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(&a[1]).unwrap()).unwrap();
    for f in a[2..].iter().map(|x| x.parse::<usize>().unwrap()) {
        let face = &b.faces[f];
        println!(
            "face {f}: {:?} sense {} loops {:?}",
            face.surf,
            face.sense,
            face.loops.iter().map(|l| l.len()).collect::<Vec<_>>()
        );
    }
}
