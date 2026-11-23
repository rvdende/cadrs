//! Scratch: a dumped body's first vertices, to the last bit.
fn main() {
    let p = std::env::args().nth(1).unwrap();
    let b = cadrs_kernel::native::bodyio::read(&std::fs::read(p).unwrap()).unwrap();
    for v in b.verts.iter().take(6) {
        println!("{:?} tol {:e}", v.p, v.tol);
    }
}
