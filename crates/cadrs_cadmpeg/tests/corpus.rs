//! cadmpeg's own public test files (its `corpus/`, beside this checkout), turned into STEP and
//! read by cadrs's STEP import: the whole way an Import of a native file goes. Skipped when the
//! corpus isn't there (`CADMPEG_CORPUS` names another folder).

use cadrs_kernel::Kernel;

fn corpus() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("CADMPEG_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cadmpeg/corpus")
        });
    p.is_dir().then_some(p)
}

#[test]
fn native_files_import_as_solids() {
    let Some(root) = corpus() else {
        eprintln!("no cadmpeg corpus: skipped");
        return;
    };
    for name in [
        "sldprt/Pair1/plane_front_blind.SLDPRT",
        "f3d/region_A.f3d",
        "freecad_fcstd/fixtures/binary_exact_shape.FCStd",
        "1WireRGBWController.f3d",
    ] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let t = std::time::Instant::now();
        let c = cadrs_cadmpeg::to_step(name, &bytes).unwrap_or_else(|e| panic!("{e}"));
        let mut k = cadrs_kernel::native::NativeKernel::new();
        let bodies = k
            .import_step(&c.step)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        eprintln!(
            "{name}: {} ({} bytes of STEP), {} solid(s), {:.0} ms",
            c.format,
            c.step.len(),
            bodies.len(),
            t.elapsed().as_secs_f64() * 1000.0
        );
        assert!(!bodies.is_empty(), "{name}: no solids");
    }
}
