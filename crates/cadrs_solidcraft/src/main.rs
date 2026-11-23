//! `cadrs-solidcraft`: translates SolidCraft `recipe.json` parts into cadrs documents.
//!
//! ```text
//! cadrs-solidcraft import <recipe.json>… [--data-dir <dir>] [--user <name>]
//!     Translates each recipe and saves it to the cadrs document store (default: the app's),
//!     as a normal editable document.
//! cadrs-solidcraft check <recipe.json>… [--json]
//!     Builds each recipe through cadrs (no store) and compares it with the `expected.json`
//!     beside it by SolidCraft's oracle rules; a table, the operations used and their coverage,
//!     and a nonzero exit when a case does not pass.
//! ```
//!
//! See `README.md` for what translates and what does not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cadrs_core::Store;

use cadrs_solidcraft::import::import_file;
use cadrs_solidcraft::{
    Outcome, SUPPORTED_OPS, SUPPORTED_SKETCH_ENTITIES, op_supported, recipe_ops, run_recipe_case,
};

const USAGE: &str = "\
usage:
  cadrs-solidcraft import <recipe.json>… [--data-dir <dir>] [--user <name>]
  cadrs-solidcraft check <recipe.json>… [--json]

import  translate each recipe into a cadrs document, saved in the store
        (--data-dir, default: the app's; --user, default: $USER)
check   build each recipe and compare it with the expected.json beside it;
        --json prints the same as JSON instead of a table
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    match cmd.as_str() {
        "import" => import(&args[1..]),
        "check" => check(&args[1..]),
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command `{other}`\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn import(args: &[String]) -> ExitCode {
    let mut data_dir: Option<PathBuf> = None;
    let mut user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data-dir" => data_dir = it.next().map(PathBuf::from),
            "--user" => user = it.next().cloned().unwrap_or(user),
            other if other.starts_with('-') => {
                eprintln!("unknown option `{other}`\n{USAGE}");
                return ExitCode::from(2);
            }
            other => paths.push(PathBuf::from(other)),
        }
    }
    if paths.is_empty() {
        eprintln!("import: no recipe.json given\n{USAGE}");
        return ExitCode::from(2);
    }
    let Some(data_dir) = data_dir.or_else(Store::default_root) else {
        eprintln!("no document store: pass --data-dir");
        return ExitCode::from(2);
    };
    let store = Store::new(data_dir);
    let mut ok = true;
    for path in &paths {
        match import_file(path, &store, &user) {
            Ok(imported) => {
                println!(
                    "saved {} as `{}` ({} features, id {})",
                    path.display(),
                    imported.name,
                    imported.features,
                    imported.id
                );
                for note in &imported.notes {
                    println!("  note: {note}");
                }
            }
            Err(e) => {
                ok = false;
                eprintln!("{}: {e}", path.display());
            }
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn check(args: &[String]) -> ExitCode {
    let mut json = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            other if other.starts_with('-') => {
                eprintln!("unknown option `{other}`\n{USAGE}");
                return ExitCode::from(2);
            }
            other => paths.push(PathBuf::from(other)),
        }
    }
    if paths.is_empty() {
        eprintln!("check: no recipe.json given\n{USAGE}");
        return ExitCode::from(2);
    }
    let mut rows: Vec<Outcome> = Vec::new();
    // How many cases use each operation (`sketch.<entity>` for sketch entities).
    let mut ops: BTreeMap<String, usize> = BTreeMap::new();
    for arg in &paths {
        let path = if arg.is_dir() {
            arg.join("recipe.json")
        } else {
            arg.clone()
        };
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let case = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(recipe) = serde_json::from_str::<serde_json::Value>(&text)
        {
            for op in recipe_ops(&recipe) {
                *ops.entry(op).or_default() += 1;
            }
        }
        if !dir.join("expected.json").is_file() {
            eprintln!(
                "{}: no expected.json beside it (a check compares against one)",
                path.display()
            );
            rows.push(Outcome {
                case,
                source: String::new(),
                pass: Some(false),
                checks: Vec::new(),
                note: "no expected.json beside the recipe".into(),
                measured: String::new(),
            });
            continue;
        }
        rows.push(run_recipe_case(&dir));
    }
    let passed = rows.iter().filter(|o| o.pass == Some(true)).count();
    let unsupported = rows.iter().filter(|o| o.pass.is_none()).count();
    let failed = rows.len() - passed - unsupported;
    let unsupported_ops: Vec<&String> = ops.keys().filter(|o| !op_supported(o)).collect();
    let seen: Vec<String> = ops.keys().cloned().collect();
    let not_seen: Vec<String> = SUPPORTED_OPS
        .iter()
        .map(|o| o.to_string())
        .chain(
            SUPPORTED_SKETCH_ENTITIES
                .iter()
                .map(|t| format!("sketch.{t}")),
        )
        .filter(|o| !seen.contains(o))
        .collect();
    if json {
        let cases: Vec<serde_json::Value> = rows
            .iter()
            .map(|o| {
                serde_json::json!({
                    "case": o.case,
                    "source": o.source,
                    "result": match o.pass {
                        Some(true) => "pass",
                        Some(false) => "fail",
                        None => "unsupported",
                    },
                    "checks": o.checks.iter().map(|c| serde_json::json!({
                        "what": c.what,
                        "error": c.error,
                        "ok": c.ok,
                    })).collect::<Vec<_>>(),
                    "note": o.note,
                    "measured": o.measured,
                })
            })
            .collect();
        let out = serde_json::json!({
            "cases": cases,
            "summary": {
                "cases": rows.len(),
                "passed": passed,
                "failed": failed,
                "unsupported": unsupported,
            },
            "ops": ops,
            "unsupported_ops": unsupported_ops,
            "supported_ops_not_exercised": not_seen,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        table(&rows, passed, unsupported);
        println!(
            "\nops: {} operation kinds used, {} unsupported",
            ops.len(),
            unsupported_ops.len()
        );
        if !unsupported_ops.is_empty() {
            let list: Vec<String> = unsupported_ops
                .iter()
                .map(|o| format!("{o} ({} cases)", ops[*o]))
                .collect();
            println!("  unsupported: {}", list.join(", "));
        }
        if !not_seen.is_empty() {
            println!(
                "  {} supported kinds these recipes do not use (see coverage.md)",
                not_seen.len()
            );
        }
    }
    if failed == 0 && unsupported == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The case table: result, the error per compared value, and the note.
fn table(rows: &[Outcome], passed: usize, unsupported: usize) {
    println!(
        "{:<34} {:<10} {:>9} {:>9} {:>6}  {}",
        "case", "result", "volume", "area", "faces", "note"
    );
    for o in rows {
        let err = |what: &str| {
            o.checks
                .iter()
                .find(|c| c.what == what)
                .map_or_else(|| "-".to_string(), |c| format!("{:.1e}", c.error))
        };
        let faces = o
            .checks
            .iter()
            .find(|c| c.what == "faces")
            .map_or_else(|| "-".to_string(), |c| format!("{:.0}", c.error));
        println!(
            "{:<34} {:<10} {:>9} {:>9} {:>6}  {}",
            o.case,
            match o.pass {
                Some(true) => "pass",
                Some(false) => "FAIL",
                None => "unsupported",
            },
            err("volume"),
            err("area"),
            faces,
            o.note
        );
    }
    println!(
        "\n{passed} / {} cases pass, {} fail, {} unsupported",
        rows.len(),
        rows.len() - passed - unsupported,
        unsupported
    );
}
