//! The STEP and IGES corpus: every `.stp`/`.step` and `.igs`/`.iges` file of OCCT's public
//! dataset (`step/` and `iges/` of opencascade-dataset-7.8.0, at `$CADRS_OCCT_DATA`, else
//! `../occt-dataset/opencascade-dataset-7.8.0` beside the checkout) read by the native readers
//! and checked as `native::exchange`'s `read_a_corpus` does: valid topology (edges off their
//! faces within the file's own tolerance aside), a positive volume per solid equal to its
//! tessellation's within 1 % and unchanged by a move, and no face dropped.
//!
//! Two-sided, like the generated suites' known failures: a file not in [`KNOWN_BAD`] must read
//! and check clean (a regression otherwise), and a file in it must still fail (stale otherwise).
//! The README's count of files read is this test's. It needs the dataset (the gate and the nightly
//! CI job have it; CI's per-push subset doesn't run this binary).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cadrs_kernel::native::exchange::split_into_solids;
use cadrs_kernel::native::math::Affine;
use cadrs_kernel::native::{iges, mass, step, tess};

/// The files that don't read clean today, by name, with what they hit (the first problem the
/// check reports). Many are OCCT's own bug-report files, broken on purpose.
const KNOWN_BAD: &[(&str, &str)] = &[
    (
        "iges/BUC50025.igs",
        "read: the file holds no B-rep solids, shells or trimmed surfaces",
    ),
    (
        "iges/Disc.igs",
        "read: not an IGES file (no directory or parameter section)",
    ),
    (
        "iges/OCC131-2.igs",
        "1 check errors (face 0 loop 0: p-curve gap after coedge 0: (0.000e0, -9.766e-2))",
    ),
    (
        "iges/OCC17026-1.igs",
        "read: the file holds no B-rep solids, shells or trimmed surfaces",
    ),
    (
        "iges/OCC17026-2.igs",
        "read: the file holds no B-rep solids, shells or trimmed surfaces",
    ),
    (
        "iges/OCC26138_Torus.igs",
        "4 check errors (face 0 loop 0 coedge 0 (edge 0): p-curve off the edge by 7.405e-2)",
    ),
    (
        "iges/PRO19800.igs",
        "read: the file holds no B-rep we can read (face 0: loops wrap around the surface and no seam could be added)",
    ),
    (
        "iges/UKI60998.igs",
        "read: the file holds no B-rep we can read (IGES face 9: directory entry 0 is missing)",
    ),
    (
        "iges/bad_header.igs",
        "4 check errors (face 0 loop 0 coedge 0 (edge 0): p-curve off the edge by 8.298e0)",
    ),
    (
        "iges/bug22869_Spring.igs",
        "12 check errors (face 0 loop 0: p-curve gap after coedge 0: (0.000e0, -7.930e-5))",
    ),
    (
        "iges/bug23171_tube_surf.igs",
        "read: the file holds no B-rep we can read (IGES face 3: unsupported IGES surface type 140)",
    ),
    (
        "iges/bug23591_VZ-387.igs",
        "read: the file holds no B-rep we can read (IGES face 1: unsupported IGES surface type 143)",
    ),
    (
        "iges/bug25837_S.igs",
        "read: the file holds no B-rep solids, shells or trimmed surfaces",
    ),
    (
        "iges/bug33327.igs",
        "8 check errors (edge 0 used (1, 0) times in a closed shell); volume 0.000000 vs mesh 9666666.666667; 4 chec…",
    ),
    (
        "iges/bug8363_f45.igs",
        "6 check errors (face 0 loop 0: coedge 0 doesn't meet the next)",
    ),
    (
        "step/bm2_ec_exhaust-A.stp",
        "4 check errors (face 33 loop 0 coedge 3 (edge 70): p-curve off the edge by 3.513e0)",
    ),
    (
        "step/bm2_pe_t4-B.stp",
        "1 check errors (edge 1 is used by 3 faces (a sheet must be manifold))",
    ),
    (
        "step/bug24595_281390.stp",
        "8 check errors (face 2 loop 0 coedge 0 (edge 6): p-curve off the edge by 3.159e0); volume 26227.500595 vs m…",
    ),
    (
        "step/bug24983_111FACE.stp",
        "9 check errors (edge 1 used (0, 1) times in a closed shell); volume 20.251896 vs mesh 25.521671",
    ),
    (
        "step/bug25013_25013face.stp",
        "7 check errors (face 0 loop 0: p-curve gap after coedge 3: (2.133e0, -1.167e-1)); volume 0.051232 vs mesh -…",
    ),
    (
        "step/bug25013_25013faces.stp",
        "17 check errors (edge 0 used (0, 1) times in a closed shell); volume 32.359538 vs mesh -1068.287699",
    ),
    (
        "step/bug25415_part_of_hdrv8w64p254_4x2_1016x502h838p.stp",
        "50 check errors (face 0 loop 0: p-curve gap after coedge 0: (3.063e-1, -1.531e-1)); volume 3.115555 vs mesh…",
    ),
    ("step/bug27333_simple.stp", "read: the file holds no shapes"),
    (
        "step/bug27453_165-174_piattello.stp",
        "3 check errors (face 28 loop 0 coedge 0 (edge 332): p-curve off the edge by 1.006e0)",
    ),
    (
        "step/bug28414_bug_toroidal_surface.stp",
        "2 check errors (edge 0 used (1, 0) times in a closed shell); volume 166.399606 vs mesh -4633.052811",
    ),
    (
        "step/bug28454_directives.stp",
        "read: STEP syntax error at line 5170: expected ',' or ')'",
    ),
    (
        "step/bug29478.stp",
        "2 check errors (face 0 loop 0 coedge 1 (edge 1): p-curve off the edge by 5.141e-3)",
    ),
    (
        "step/bug30790.step",
        "4 check errors (edge 0 used (1, 0) times in a closed shell); 3936 faces dropped (face 2: a loop doesn't close)",
    ),
    (
        "step/bug31292.stp",
        "8 check errors (edge 0 used (0, 1) times in a closed shell); volume 403.650459 vs mesh 442.967014",
    ),
    (
        "step/bug32239.STEP",
        "6 check errors (face 510 loop 0 coedge 0 (edge 7): p-curve off the edge by 6.858e-1); 1 check errors (face …",
    ),
    (
        "step/bug32314_corrupted.stp",
        "read: STEP syntax error at line 12: expected '('",
    ),
    (
        "step/ec_exhaust-A.stp",
        "volume 1536723.001863 vs mesh 1063280.267061",
    ),
    (
        "step/trj3_s1-al-214.stp",
        "24 check errors (face 12 loop 0: coedge 0 doesn't meet the next); 3 faces dropped (face 0: a loop doesn't c…",
    ),
    (
        "step/trj3_s1-ct-214.stp",
        "1 check errors (face 9 loop 0 coedge 3 (edge 7): p-curve off the edge by 2.822e-3); 3 faces dropped (face 15: loops wrap around the surface and no seam could be added)",
    ),
    (
        "step/trj3_s1-md-214.stp",
        "5 check errors (face 0 loop 0: p-curve gap after coedge 1: (2.115e-4, 1.500e0)); 1 faces dropped (face 0: l…",
    ),
    (
        "step/trj4_k1_geo-md-203.stp",
        "7 check errors (face 20 loop 0 coedge 0 (edge 64): p-curve off the edge by 1.502e-3)",
    ),
    (
        "step/trj4_k1_geo-pe-203.stp",
        "2 check errors (edge 14 used (0, 2) times in a closed shell); volume 6124.483818 vs mesh 2918.608507",
    ),
    (
        "step/trj4_k1_geo-pe-214.stp",
        "2 check errors (edge 14 used (0, 2) times in a closed shell); volume 6124.483818 vs mesh 2918.608507",
    ),
    (
        "step/trj4_k1_geo-tu-214.stp",
        "4 faces dropped (face 0: a loop doesn't close)",
    ),
    (
        "step/trj4_k1_top-md-203.stp",
        "1 faces dropped (face 17: loops wrap around the surface and no seam could be added)",
    ),
    (
        "step/trj4_k1_top-tu-214.stp",
        "2 faces dropped (face 0: a loop doesn't close)",
    ),
    ("step/trj4_xr1-db-214.stp", "read: the file holds no shapes"),
    ("step/trj4_xr1-ec-214.stp", "read: the file holds no shapes"),
    ("step/trj4_xr1-id-214.stp", "read: the file holds no shapes"),
    ("step/trj4_xr1-pe-214.stp", "read: the file holds no shapes"),
    ("step/trj4_xr1-tc-214.stp", "read: the file holds no shapes"),
    ("step/trj4_xr1-tu-214.stp", "read: the file holds no shapes"),
    (
        "step/trj5_k1-al-214.stp",
        "10 check errors (face 36 loop 0 coedge 5 (edge 122): p-curve off the edge by 7.620e-1)",
    ),
    (
        "step/trj5_k1-md-203.stp",
        "1 faces dropped (face 17: loops wrap around the surface and no seam could be added)",
    ),
    (
        "step/trj5_k1-sh-203.stp",
        "1 check errors (edge 23 used (0, 1) times in a closed shell); 1 faces dropped (face 43: a loop doesn't close)",
    ),
    (
        "step/trj5_s1-in-214.stp",
        "volume 1827.979158 vs mesh -3251.840326; volume 5456.844752 vs mesh 10540.511488; 7 check errors (face 0 lo…",
    ),
    (
        "step/trj6_k1-al-214.stp",
        "2 check errors (face 1 loop 0 coedge 13 (edge 33): p-curve off the edge by 1.016e0)",
    ),
    (
        "step/trj6_k1-md-214.stp",
        "1 faces dropped (face 17: loops wrap around the surface and no seam could be added)",
    ),
    (
        "step/trj7_k1-al-214.stp",
        "3 check errors (face 4 loop 0: p-curve gap after coedge 5: (-1.007e-1, 5.599e-11))",
    ),
    ("step/xr1-db-214_0.stp", "read: the file holds no shapes"),
    ("step/xr1-db-214_1.stp", "read: the file holds no shapes"),
    ("step/xr1-db-214_2.stp", "read: the file holds no shapes"),
    ("step/xr1-db-214_6.stp", "read: the file holds no shapes"),
];

fn dataset() -> PathBuf {
    let root = std::env::var("CADRS_OCCT_DATA").unwrap_or_else(|_| {
        format!(
            "{}/../../../occt-dataset/opencascade-dataset-7.8.0",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let p = PathBuf::from(root);
    assert!(
        p.join("step").is_dir(),
        "OCCT's dataset isn't at {}: set CADRS_OCCT_DATA to the dataset's folder",
        p.display()
    );
    p
}

/// The corpus: the dataset's STEP and IGES files, sorted.
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in ["step", "iges"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_lowercase();
            if matches!(ext.as_str(), "stp" | "step" | "igs" | "iges") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// What a file read to: `Ok(solids)` when it reads and checks clean, else what is wrong.
fn check_file(path: &Path) -> Result<usize, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    let m = if ext.starts_with("ig") {
        iges::read(&bytes)
    } else {
        step::read(&bytes)
    }
    .map_err(|e| format!("read: {e}"))?;
    let mut problems = Vec::new();
    let mut solids = 0;
    for p in &m.parts {
        solids += p.body.solids.len();
        // Edges off their faces within the file's own tolerance are not our doing.
        let size = p.body.bbox().diagonal();
        let errs: Vec<String> = p
            .body
            .check()
            .into_iter()
            .filter(|e| {
                !e.split("off the edge by ")
                    .nth(1)
                    .and_then(|d| d.parse::<f64>().ok())
                    .is_some_and(|d| d < 1e-3f64.max(1e-6 * size))
            })
            .collect();
        if let Some(e) = errs.first() {
            problems.push(format!("{} check errors ({e})", errs.len()));
        }
        for piece in split_into_solids(&p.body) {
            if piece.solids.is_empty() {
                continue;
            }
            let v = mass::body_mass(&piece).volume;
            let defl = (piece.bbox().diagonal() * 2e-4).max(1e-4);
            let (fm, _) = tess::mesh_body(&piece, defl, 0.1);
            let mv: f64 = fm
                .iter()
                .flat_map(|f| {
                    f.indices
                        .iter()
                        .map(|t| t.map(|i| f.positions[i as usize].coords))
                })
                .map(|[a, b, c]| a.dot(&b.cross(&c)) / 6.0)
                .sum();
            if v <= 0.0 || (mv - v).abs() > 1e-2 * v.abs().max(1e-9) {
                problems.push(format!("volume {v:.6} vs mesh {mv:.6}"));
            }
            // A closed, consistently oriented shell's volume doesn't move with it.
            let moved = piece.transformed(&Affine::translation(nalgebra::Vector3::new(
                1000.0, -700.0, 300.0,
            )));
            let vm = mass::body_mass(&moved).volume;
            if (vm - v).abs() > 1e-6 * v.abs().max(1.0) {
                problems.push(format!("volume {v:.6}, moved {vm:.6}"));
            }
        }
    }
    let dropped: Vec<&String> = m
        .warnings
        .iter()
        .filter(|w| {
            w.contains("no seam")
                || w.contains("no bounds")
                || w.contains("doesn't close")
                || w.contains("unsupported")
        })
        .collect();
    if let Some(w) = dropped.first() {
        problems.push(format!("{} faces dropped ({w})", dropped.len()));
    }
    if problems.is_empty() {
        Ok(solids)
    } else {
        Err(problems.join("; "))
    }
}

#[test]
fn occt_dataset_step_and_iges() {
    let root = dataset();
    let all = files(&root);
    assert!(
        !all.is_empty(),
        "no STEP or IGES files under {}",
        root.display()
    );
    // The files on a few threads (the test's own: the gate runs its binaries' tests in parallel).
    let next = AtomicUsize::new(0);
    let got: Mutex<Vec<(usize, Result<usize, String>)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(p) = all.get(i) else { break };
                    let r = std::panic::catch_unwind(|| check_file(p))
                        .unwrap_or_else(|_| Err("panicked".into()));
                    got.lock().unwrap_or_else(|e| e.into_inner()).push((i, r));
                }
            });
        }
    });
    let mut got = got.into_inner().unwrap_or_else(|e| e.into_inner());
    got.sort_by_key(|x| x.0);
    let (mut ok, mut with_solids, mut wrong) = (0, 0, Vec::new());
    for (i, r) in &got {
        let name = all[*i]
            .strip_prefix(&root)
            .unwrap_or(&all[*i])
            .display()
            .to_string();
        let known = KNOWN_BAD.iter().find(|k| k.0 == name);
        match (r, known) {
            (Ok(solids), None) => {
                ok += 1;
                with_solids += usize::from(*solids > 0);
                println!("ok    {name}: {solids} solids");
            }
            (Ok(_), Some(_)) => wrong.push(format!(
                "{name}: a known bad file now reads clean: take it off KNOWN_BAD"
            )),
            (Err(e), Some(_)) => println!("known {name}: {e}"),
            (Err(e), None) => wrong.push(format!("{name}: {e}")),
        }
    }
    println!(
        "{ok} of {} files read and check clean ({with_solids} with solids); {} known bad",
        all.len(),
        KNOWN_BAD.len()
    );
    assert!(
        wrong.is_empty(),
        "{} files changed:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}
