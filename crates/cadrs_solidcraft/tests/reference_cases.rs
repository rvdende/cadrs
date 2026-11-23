//! The SolidCraft reference cases, through the translator crate.
//!
//! Each fixture folder (`crates/cadrs_core/tests/reference_parts/solidcraft/<case>/`, copied
//! unchanged from SolidCraft's `tests/oracle/`, MIT) holds a `recipe.json` and the values the
//! part must measure (`expected.json`). [`run_recipe_case`] translates the recipe into cadrs
//! features, rebuilds it and compares it by SolidCraft's oracle rules — the same implementation
//! `crates/cadrs_core/tests/reference_parts.rs` validates case by case.
//!
//! Status today: 90 fixture folders, 89 pass, 1 known failure (`75-sm-bracket-holes`: its two
//! flanges meet in a mitred corner SolidCraft makes and cadrs does not). Every operation the
//! fixtures use is translated — the cases that are not translated are none, so the coverage
//! floor below is about *passing*, not about ops.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cadrs_solidcraft::{Outcome, run_recipe_case};

/// The fixtures: SolidCraft's cases copied into `cadrs_core`'s tests folder.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cadrs_core/tests/reference_parts/solidcraft")
}

/// The fixture folders, sorted, each with a `recipe.json` and an `expected.json`.
fn case_dirs() -> Vec<PathBuf> {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(root())
        .expect("the SolidCraft fixtures are in the repo")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("recipe.json").is_file() && p.join("expected.json").is_file())
        .collect();
    cases.sort();
    cases
}

/// Cases that are not translated yet: the operations they use. None today; a case here must be
/// reported as unsupported (not failing) for the test to pass.
const KNOWN_UNSUPPORTED: &[&str] = &[];
/// Cases that are translated and build, but whose geometry does not (yet) match the expected
/// values. Each is documented in `coverage.md`.
const KNOWN_FAILURES: &[&str] = &["75-sm-bracket-holes"];
/// The number of fixture folders expected (a new fixture must update this and be looked at).
const CASES: usize = 90;
/// The coverage floor: at least this many cases must pass. That is 89/90 today (the one known
/// failure above); a regression below it fails this test.
const PASS_FLOOR: usize = 89;

/// Runs every case, on the available threads: each case is a kernel rebuild of its own, as the
/// per-case tests of `reference_parts` run in parallel.
fn run_all(cases: &[PathBuf]) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Outcome)>> = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(8)
        .min(cases.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(dir) = cases.get(i) else { break };
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_recipe_case(dir)
                    }));
                    let outcome = outcome.unwrap_or_else(|_| Outcome {
                        case: dir.file_name().unwrap().to_string_lossy().to_string(),
                        source: String::new(),
                        pass: Some(false),
                        checks: Vec::new(),
                        note: "failed: panicked".into(),
                        measured: String::new(),
                    });
                    done.lock().unwrap().push((i, outcome));
                }
            });
        }
    });
    let mut outcomes = done.into_inner().unwrap();
    outcomes.sort_by_key(|(i, _)| *i);
    outcomes.into_iter().map(|(_, o)| o).collect()
}

#[test]
fn fixtures_are_whole() {
    assert_eq!(
        case_dirs().len(),
        CASES,
        "the fixture folders changed: update CASES and look at the new case"
    );
}

#[test]
fn reference_cases_pass() {
    let cases = case_dirs();
    let outcomes = run_all(&cases);
    assert_eq!(outcomes.len(), cases.len(), "every case ran");
    let mut failures: Vec<String> = Vec::new();
    let mut unsupported: Vec<String> = Vec::new();
    let mut passed = 0;
    for o in &outcomes {
        match o.pass {
            Some(true) => passed += 1,
            Some(false) if KNOWN_FAILURES.contains(&o.case.as_str()) => {
                println!("KNOWN FAILURE {}: {}\n  {}", o.case, o.note, o.measured);
            }
            Some(false) => failures.push(format!("{}: {}\n  {}", o.case, o.note, o.measured)),
            None if KNOWN_UNSUPPORTED.contains(&o.case.as_str()) => {
                println!("KNOWN UNSUPPORTED {}: {}", o.case, o.note);
            }
            None => unsupported.push(format!("{}: {}", o.case, o.note)),
        }
    }
    println!(
        "{passed} / {} SolidCraft cases pass ({} known failures, {} unsupported)",
        outcomes.len(),
        KNOWN_FAILURES.len(),
        unsupported.len()
    );
    assert!(
        unsupported.is_empty(),
        "cases used an operation that is not translated, but is not in KNOWN_UNSUPPORTED:\n{}",
        unsupported.join("\n")
    );
    assert!(
        failures.is_empty(),
        "cases that should pass do not (and are not in KNOWN_FAILURES):\n{}",
        failures.join("\n")
    );
    assert!(
        passed >= PASS_FLOOR,
        "coverage floor: {passed} / {} pass, the floor is {PASS_FLOOR}",
        outcomes.len()
    );
}
