//! The suite's cases run on cadrs in process: quick reruns of a few tests while fixing them,
//! with the kernel's panics and their backtraces (`RUST_BACKTRACE=1`) printed where they happen.
//!
//! ```text
//! cargo run -r --example suite -- [--family A,B] [--id PART,PART] [--threads N] [--seed N]
//!                                 [--out results.json]
//! ```
//!
//! `--family` takes test paths, prefixes of them (`fillets/`) or family names, as the suite's own
//! command does; `--id` keeps the cases whose id contains one of the parts. It prints the suite's
//! summary and each case not passing with its first failed check.

use brep_conformance::{generate, report, run};
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let list = |flag: &str| value(flag).map(|v| v.split(',').map(String::from).collect::<Vec<_>>());
    let (families, ids) = (list("--family"), list("--id"));
    let seed = value("--seed").map_or(1, |s| s.parse().expect("--seed takes a number"));
    let threads = value("--threads").map_or_else(
        || std::thread::available_parallelism().map_or(4, |n| n.get()),
        |s| s.parse().expect("--threads takes a number"),
    );
    let cases: Vec<_> = generate::cases(seed)
        .into_iter()
        .filter(|c| {
            families.as_ref().is_none_or(|o| {
                o.iter()
                    .any(|f| f == c.family || c.test == f || c.test.starts_with(f.as_str()))
            })
        })
        .filter(|c| {
            ids.as_ref()
                .is_none_or(|o| o.iter().any(|i| c.id.contains(i.as_str())))
        })
        .collect();
    eprintln!("{} cases on {threads} threads", cases.len());
    let exports = run::Exports {
        glb: None,
        step: None,
    };
    let results = run::run(&cadrs_conformance::Cadrs, &cases, threads, None, &exports);
    if let Some(out) = value("--out") {
        std::fs::write(&out, serde_json::to_string(&results).unwrap()).expect("writing results");
    }
    let summary = report::summary(&results);
    // The per-test table, without the timing section.
    print!(
        "{}",
        summary.split("\ntime in units").next().unwrap_or(&summary)
    );
    for c in results["cases"].as_array().into_iter().flatten() {
        if c["outcome"] == "pass" {
            continue;
        }
        println!(
            "{} [{}] {}",
            c["id"].as_str().unwrap_or("?"),
            c["outcome"].as_str().unwrap_or("?"),
            first_failure(c)
        );
    }
}

fn first_failure(c: &Value) -> String {
    let failed = c["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|ch| ch["ok"] == false);
    let crashed = c["ops"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|o| o["outcome"] != "ok" && o["error"].is_string());
    let text = match (failed, crashed) {
        (_, Some(o)) => format!(
            "{}: {}",
            o["op"].as_str().unwrap_or("?"),
            o["error"].as_str().unwrap_or("")
        ),
        (Some(ch), None) => format!(
            "{}: {}",
            ch["clause"].as_str().unwrap_or("?"),
            ch["detail"].as_str().unwrap_or("")
        ),
        (None, None) => String::new(),
    };
    text.chars().take(240).collect()
}
