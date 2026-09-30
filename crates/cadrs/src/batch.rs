//! Rendering many scenarios at once (the parallel mode of the scenario harness).
//!
//! `cadrs --headless --jobs N --scenarios a,b,c` or `cadrs --headless --jobs N --all` runs each
//! scenario in its own child process of this binary (`--headless --scenario <name>`), at most
//! `N` at a time, and prints one line per scenario and a summary. Every child is exactly the run
//! `cadrs --headless --scenario <name>` makes on its own (a fresh app, its own output and data
//! directories under `target/scenarios/<name>/`), so the PNGs are the same as rendering the
//! scenarios one by one; only the wall time changes. `--jobs` defaults to 4.
//!
//! Exit code: 0 when every scenario finished, 1 otherwise.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

/// What a batch run renders and how many at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub scenarios: Vec<String>,
    pub jobs: usize,
    /// Arguments passed on to every child (`--window-size`, `--no-cursor`).
    pub pass: Vec<String>,
}

/// Every `scenarios/*.ron`, by name, sorted.
pub fn all_scenarios() -> Vec<String> {
    let dir = cadrs_harness::scenario::scenarios_dir();
    let mut v: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "ron"))
                .filter_map(|p| Some(p.file_stem()?.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The batch the arguments ask for, if they use `--jobs`, `--scenarios` or `--all`; `Err` for
/// a bad value. Other arguments are left to [`cadrs_harness::HarnessOptions`].
pub fn parse(args: &[String]) -> Option<Result<Batch, String>> {
    if !args.iter().any(|a| matches!(a.as_str(), "--jobs" | "--scenarios" | "--all")) {
        return None;
    }
    Some((|| {
        let mut b = Batch { scenarios: Vec::new(), jobs: 4, pass: Vec::new() };
        let mut all = false;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--headless" => {}
                "--jobs" => {
                    let v = it.next().ok_or("--jobs needs a number")?;
                    b.jobs = v.parse::<usize>().map_err(|_| format!("bad --jobs {v:?}"))?.max(1);
                }
                "--scenarios" => {
                    let v = it.next().ok_or("--scenarios needs a comma-separated list")?;
                    b.scenarios.extend(v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from));
                }
                "--scenario" => b.scenarios.push(it.next().ok_or("--scenario needs a name")?.clone()),
                "--all" => all = true,
                "--window-size" => {
                    b.pass.push(a.clone());
                    b.pass.push(it.next().ok_or("--window-size needs WxH")?.clone());
                }
                "--no-cursor" => b.pass.push(a.clone()),
                other => return Err(format!("{other:?} can't be used with --jobs/--scenarios/--all")),
            }
        }
        if all {
            b.scenarios = all_scenarios();
        }
        if b.scenarios.is_empty() {
            return Err("no scenarios to render".into());
        }
        Ok(b)
    })())
}

/// Renders the batch; returns the process exit code.
pub fn run(b: &Batch) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cannot find this program: {e}");
            return 2;
        }
    };
    let start = Instant::now();
    let queue = Mutex::new(b.scenarios.iter().cloned().collect::<std::collections::VecDeque<_>>());
    let failed: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    let jobs = b.jobs.min(b.scenarios.len());
    eprintln!("rendering {} scenario(s), {jobs} at a time", b.scenarios.len());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    let Some(name) = queue.lock().unwrap().pop_front() else { break };
                    let t = Instant::now();
                    let out = Command::new(&exe)
                        .args(["--headless", "--scenario", &name])
                        .args(&b.pass)
                        .env("RUST_LOG", std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into()))
                        .stdout(Stdio::null())
                        .stderr(Stdio::piped())
                        .output();
                    let secs = t.elapsed().as_secs_f64();
                    match out {
                        Ok(o) if o.status.success() => eprintln!("  ok   {name} ({secs:.1} s)"),
                        Ok(o) => {
                            eprintln!("  FAIL {name} ({secs:.1} s, {})", o.status);
                            failed.lock().unwrap().push((name, String::from_utf8_lossy(&o.stderr).into_owned()));
                        }
                        Err(e) => {
                            eprintln!("  FAIL {name}: {e}");
                            failed.lock().unwrap().push((name, e.to_string()));
                        }
                    }
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    for (name, err) in &failed {
        let tail: Vec<&str> = err.lines().rev().take(15).collect();
        eprintln!("--- {name} ---");
        for l in tail.iter().rev() {
            eprintln!("{l}");
        }
    }
    eprintln!(
        "{} of {} scenario(s) finished in {:.1} s ({} failed); PNGs in {}",
        b.scenarios.len() - failed.len(),
        b.scenarios.len(),
        start.elapsed().as_secs_f64(),
        failed.len(),
        PathBuf::from("target/scenarios").display()
    );
    if failed.is_empty() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_batches_and_leaves_single_runs_alone() {
        assert!(parse(&args("--headless --scenario smoke")).is_none());
        let b = parse(&args("--headless --jobs 3 --scenarios a,b --scenario c --no-cursor")).unwrap().unwrap();
        assert_eq!(b.scenarios, ["a", "b", "c"]);
        assert_eq!(b.jobs, 3);
        assert_eq!(b.pass, ["--no-cursor"]);
        assert!(parse(&args("--jobs x --all")).unwrap().is_err());
        assert!(parse(&args("--jobs 2 --out d --scenario a")).unwrap().is_err());
        let all = parse(&args("--all")).unwrap().unwrap();
        assert!(all.scenarios.iter().any(|s| s == "smoke"));
        assert_eq!(all.jobs, 4);
    }
}
