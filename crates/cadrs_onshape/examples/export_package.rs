//! Writes a stored document as a portable `.cadrs` package.
//!
//! `cargo run -r -p cadrs_onshape --example export_package -- <store dir> <document name> <out.cadrs>`

use cadrs_core::Store;

fn main() {
    let mut args = std::env::args().skip(1);
    let store = Store::new(args.next().expect("a document store"));
    let name = args.next().expect("a document name");
    let out = std::path::PathBuf::from(args.next().expect("a file to write"));
    let (lib, _) = store.list();
    let entry = lib
        .entries
        .iter()
        .find(|e| e.name == name)
        .expect("a document of that name");
    let file = store.load(entry.id).expect("a readable document");
    cadrs_core::native::export(&out, &file.document, &file.meta, &store).expect("the package");
    println!("{}", out.display());
}
