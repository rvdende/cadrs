//! cq2fs: CADEvolve (CadQuery) generators -> FeatureScript -> Parasolid (through Onshape) -> compared
//! with cadrs. See this crate's README.md for the whole loop.
//!
//! ```text
//! cq2fs scan [--out target/cq2fs/tapes.jsonl]   record every generator (for choosing cases)
//! cq2fs translate <tapes.jsonl> [--ok-out f]    which tapes translate, and why the others do not
//! cq2fs selectors < strings                     compile selector strings (tests/selector_grammar_check.py)
//! cq2fs add <set.json> <i> [<i> ...]            add generators to a case set (generator.py, tape.json)
//! cq2fs record <set.json>                       re-record every case's tape.json
//! cq2fs emit <set.json> <i>                     one case's FeatureScript, as a lambda running it
//! cq2fs serve <set.json> [--batch 4] [--rerun] [--only i,j] [--host test_shell] [--port 8766]
//!                                               job server for the Onshape tab: writes parasolid.json
//! cq2fs cadrs <set.json> [--out <results.jsonl>] run the set on cadrs's CadQuery (crates/cadrs_py)
//! cq2fs compare <set.json> [--cadrs <results.jsonl>] [--out report.json]
//! ```
//!
//! A case set is `{"generators": path, "dir": dir, "cases": [{"i", "name"}]}`, paths relative to the
//! set file; each case's files are in `<dir>/<i>_<name>/`: generator.py (the CADEvolve source),
//! tape.json (what the recorder saw), case.fs (what Parasolid ran), parasolid.json (its answer).

mod compare;
mod emit;
mod selector;
mod serve;
mod sigs;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CRATE: &str = env!("CARGO_MANIFEST_DIR");

pub fn expand(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(s),
    }
}

/// The Python that runs generators: the recorder needs numpy and scipy for some of them.
pub fn python() -> PathBuf {
    std::env::var("CQ2FS_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| expand("~/Work/cadevolve/.venv-cadrs/bin/python"))
}

pub struct CaseSet {
    pub path: PathBuf,
    pub generators: PathBuf,
    pub dir: PathBuf,
    pub cases: Vec<(i64, String)>,
    pub raw: Value,
}

impl CaseSet {
    pub fn load(path: &Path) -> Result<CaseSet> {
        let raw: Value = serde_json::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        )?;
        let base = path.parent().unwrap_or(Path::new("."));
        let rel = |s: &str| -> PathBuf {
            let p = expand(s);
            if p.is_absolute() { p } else { base.join(p) }
        };
        Ok(CaseSet {
            path: path.to_path_buf(),
            generators: rel(raw["generators"].as_str().context("set.generators")?),
            dir: rel(raw["dir"].as_str().context("set.dir")?),
            cases: raw["cases"]
                .as_array()
                .context("set.cases")?
                .iter()
                .map(|c| {
                    (
                        c["i"].as_i64().unwrap(),
                        c["name"].as_str().unwrap().to_string(),
                    )
                })
                .collect(),
            raw,
        })
    }
    pub fn case_dir(&self, i: i64, name: &str) -> PathBuf {
        self.dir.join(format!("{i}_{name}"))
    }
    pub fn tape(&self, i: i64, name: &str) -> Result<Value> {
        let p = self.case_dir(i, name).join("tape.json");
        Ok(serde_json::from_str(
            &std::fs::read_to_string(&p)
                .with_context(|| format!("{} (run `cq2fs record`)", p.display()))?,
        )?)
    }
    pub fn save(&self) -> Result<()> {
        let mut raw = self.raw.clone();
        raw["cases"] = json!(
            self.cases
                .iter()
                .map(|(i, n)| json!({"i": i, "name": n}))
                .collect::<Vec<_>>()
        );
        std::fs::write(&self.path, serde_json::to_string_pretty(&raw)? + "\n")?;
        Ok(())
    }
}

pub fn load_tapes(path: &Path) -> Result<BTreeMap<i64, Value>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(line)?;
        out.insert(v["i"].as_i64().unwrap_or(-1), v);
    }
    Ok(out)
}

/// Run the recorder (recorder/trace.py) on generators `idx` (all when empty) into `out`.
/// `fails`: generators to replay with steps raising (each case's injected failures, its
/// tape.json's `fail_at`).
fn trace(
    generators: &Path,
    out: &Path,
    idx: &[i64],
    fails: &BTreeMap<i64, Vec<i64>>,
) -> Result<()> {
    let mut cmd = std::process::Command::new(python());
    cmd.env("CQ2FS_BIN", std::env::current_exe()?);
    cmd.arg("-I")
        .arg(Path::new(CRATE).join("recorder/trace.py"))
        .arg(generators)
        .arg(out);
    for i in idx {
        cmd.arg(i.to_string());
    }
    for (i, steps) in fails {
        if !steps.is_empty() {
            let s: Vec<String> = steps.iter().map(|x| x.to_string()).collect();
            cmd.arg("--fail").arg(format!("{i}={}", s.join(",")));
        }
    }
    let st = cmd.status().context("running the recorder")?;
    if !st.success() {
        bail!("recorder failed: {st}");
    }
    Ok(())
}

/// Record cases' tapes, each replaying the failures `fails` gives it (or, when it gives none,
/// those its tape.json already replays).
pub fn record_cases(set: &CaseSet, idx: &[i64], fails: &BTreeMap<i64, Vec<i64>>) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!(
        "cq2fs-record-{}-{}.jsonl",
        std::process::id(),
        idx.first().unwrap_or(&0)
    ));
    let mut all = BTreeMap::new();
    for (i, name) in &set.cases {
        if !idx.contains(i) {
            continue;
        }
        let f = match fails.get(i) {
            Some(f) => f.clone(),
            None => set
                .tape(*i, name)
                .ok()
                .and_then(|t| {
                    t["fail_at"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
                })
                .unwrap_or_default(),
        };
        all.insert(*i, f);
    }
    trace(&set.generators, &tmp, idx, &all)?;
    let tapes = load_tapes(&tmp)?;
    let gens: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&set.generators)?)?;
    for i in idx {
        let g = &gens[*i as usize];
        let name = g["name"].as_str().unwrap();
        let dir = set.case_dir(*i, name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("generator.py"),
            format!("{}\n", g["code"].as_str().unwrap_or("")),
        )?;
        std::fs::write(
            dir.join("tape.json"),
            serde_json::to_string_pretty(&tapes[i])? + "\n",
        )?;
    }
    std::fs::remove_file(&tmp).ok();
    Ok(())
}

pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest = &args[args.len().min(1)..];
    match cmd {
        "scan" => {
            let gens = expand(&flag(rest, "--generators").unwrap_or_else(|| {
                "~/Work/cadevolve/CADEvolve-G/parametric_generators.json".into()
            }));
            let out = PathBuf::from(
                flag(rest, "--out")
                    .unwrap_or_else(|| format!("{CRATE}/../../target/cq2fs/tapes.jsonl")),
            );
            std::fs::create_dir_all(out.parent().unwrap())?;
            trace(&gens, &out, &[], &BTreeMap::new())?;
            println!("{}", out.display());
        }
        "translate" => {
            let tapes = load_tapes(Path::new(rest.first().context("tapes path")?))?;
            let mut ok = 0;
            let mut ok_ids = vec![];
            let mut why: BTreeMap<String, Vec<i64>> = BTreeMap::new();
            for (i, rec) in &tapes {
                if rec["status"] != "ok" {
                    continue;
                }
                match emit::translate(rec) {
                    Ok(_) => {
                        ok += 1;
                        ok_ids.push(*i);
                    }
                    Err(e) => {
                        let key: String = e
                            .chars()
                            .map(|c| if c.is_ascii_digit() { 'N' } else { c })
                            .take(90)
                            .collect();
                        why.entry(key).or_default().push(*i);
                    }
                }
            }
            if let Some(p) = flag(rest, "--ok-out") {
                std::fs::write(p, serde_json::to_string(&ok_ids)?)?;
            }
            println!(
                "{ok} translatable of {} recorded ({} generators)",
                tapes.values().filter(|r| r["status"] == "ok").count(),
                tapes.len()
            );
            let mut v: Vec<_> = why.into_iter().collect();
            v.sort_by_key(|(_, l)| std::cmp::Reverse(l.len()));
            for (k, l) in v.iter().take(40) {
                println!("{:6}  {}   e.g. {:?}", l.len(), k, &l[..l.len().min(4)]);
            }
        }
        "selectors" => {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines() {
                let s: String = serde_json::from_str(&line?)?;
                match selector::compile_str(&s) {
                    Ok(v) => println!("{v}"),
                    Err(e) => println!("{}", json!({"error": e})),
                }
            }
        }
        "add" => {
            let mut set = CaseSet::load(Path::new(rest.first().context("set.json")?))?;
            let gens: Vec<Value> =
                serde_json::from_str(&std::fs::read_to_string(&set.generators)?)?;
            let idx: Vec<i64> = rest[1..]
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;
            for i in &idx {
                if !set.cases.iter().any(|(j, _)| j == i) {
                    set.cases
                        .push((*i, gens[*i as usize]["name"].as_str().unwrap().to_string()));
                }
            }
            record_cases(&set, &idx, &BTreeMap::new())?;
            set.save()?;
        }
        "record" => {
            let set = CaseSet::load(Path::new(rest.first().context("set.json")?))?;
            let idx: Vec<i64> = set.cases.iter().map(|(i, _)| *i).collect();
            record_cases(&set, &idx, &BTreeMap::new())?;
        }
        "emit" => {
            let set = CaseSet::load(Path::new(rest.first().context("set.json")?))?;
            let i: i64 = rest.get(1).context("case index")?.parse()?;
            let (_, name) = set
                .cases
                .iter()
                .find(|(j, _)| *j == i)
                .context("no such case in the set")?;
            let t = emit::translate(&set.tape(i, name)?).map_err(anyhow::Error::msg)?;
            print!("{}", emit::batch(&[&t]));
        }
        "serve" => serve::run(rest)?,
        "cadrs" => compare::run_cadrs(rest)?,
        "compare" => compare::run(rest)?,
        _ => bail!(
            "usage: cq2fs scan|translate|selectors|add|record|emit|serve|cadrs|compare (see src/main.rs)"
        ),
    }
    Ok(())
}
