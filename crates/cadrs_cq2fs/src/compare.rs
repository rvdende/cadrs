//! Compare cadrs's measures of each case with Parasolid's (parasolid.json in each case directory).
//!
//!   cq2fs compare <set.json> --cadrs <cadrs results.jsonl> [--out report.json]
//!
//! cadrs results are tools/cadevolve/worker.py lines: {"i", "status", "vol", "area", "bbox",
//! "solids", "faces", "valid"}. Verdicts:
//!   agree        volume and area within REL, bounding box within ABS, same number of solids
//!   differs      both produced a solid and they disagree
//!   cadrs-fails  Parasolid built it, cadrs did not
//!   both-fail / parasolid-fails / untranslated / pending

use crate::{CRATE, CaseSet, flag, python};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const REL: f64 = 1e-6;
pub const ABS: f64 = 1e-5;

pub fn verdict(p: &Value, c: Option<&Value>) -> (String, Value) {
    let pst = p["status"].as_str().unwrap_or("pending");
    let cst = c.and_then(|c| c["status"].as_str()).unwrap_or("missing");
    if pst == "untranslated" || pst == "pending" {
        return (pst.to_string(), json!({}));
    }
    let pok = pst == "ok";
    let cok = cst == "ok";
    match (pok, cok) {
        (false, false) => (
            "both-fail".into(),
            json!({"parasolid": p["err"], "cadrs": c.map(|c| c["err"].clone())}),
        ),
        (false, true) => ("parasolid-fails".into(), json!({"parasolid": p["err"]})),
        (true, false) => (
            "cadrs-fails".into(),
            json!({"cadrs": c.map(|c| c["err"].clone()), "cadrs_status": cst}),
        ),
        (true, true) => {
            let c = c.unwrap();
            let rel = |a: f64, b: f64| (a - b).abs() / a.abs().max(b.abs()).max(1e-12);
            let pv = p["vol"].as_f64().unwrap_or(f64::NAN);
            let cv = c["vol"].as_f64().unwrap_or(f64::NAN);
            let pa = p["area"].as_f64().unwrap_or(f64::NAN);
            let ca = c["area"].as_f64().unwrap_or(f64::NAN);
            let dv = rel(pv, cv);
            let da = rel(pa, ca);
            let mut db = 0.0f64;
            if let (Some(pb), Some(cb)) = (p["bbox"].as_array(), c["bbox"].as_array()) {
                for (x, y) in pb.iter().zip(cb) {
                    db = db.max((x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(0.0)).abs());
                }
            }
            let ps = p["solids"].as_f64().map(|x| x as i64).unwrap_or(-1);
            let cs = c["solids"].as_f64().map(|x| x as i64).unwrap_or(-1);
            let details = json!({"dvol": dv, "darea": da, "dbbox": db, "vol": [pv, cv], "area": [pa, ca],
                                 "solids": [ps, cs], "faces": [p["faces"], c["faces"]], "cadrs_valid": c["valid"]});
            let ok = dv <= REL && da <= REL && db <= ABS && ps == cs;
            ((if ok { "agree" } else { "differs" }).into(), details)
        }
    }
}

pub fn run(args: &[String]) -> Result<()> {
    let set = CaseSet::load(&PathBuf::from(args.first().context("set.json")?))?;
    let flag = |name: &str| flag(args, name);
    let mut cadrs: BTreeMap<i64, Value> = BTreeMap::new();
    if let Some(p) = flag("--cadrs") {
        for line in std::fs::read_to_string(&p)?.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                cadrs.insert(v["i"].as_i64().unwrap_or(-1), v);
            }
        }
    }
    let mut rows = vec![];
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (i, name) in &set.cases {
        let dir = set.case_dir(*i, name);
        let p: Value = std::fs::read_to_string(dir.join("parasolid.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({"status": "pending"}));
        let (v, d) = verdict(&p, cadrs.get(i));
        *counts.entry(v.clone()).or_default() += 1;
        println!(
            "{:5} {:44} {:16} {}",
            i,
            name.chars().take(44).collect::<String>(),
            v,
            d
        );
        rows.push(json!({"i": i, "name": name, "verdict": v, "details": d}));
    }
    println!("{counts:?}");
    if let Some(out) = flag("--out") {
        std::fs::write(
            out,
            serde_json::to_string_pretty(&json!({"counts": counts, "rows": rows}))?,
        )?;
    }
    Ok(())
}

/// Run the set's generators on cadrs's CadQuery (crates/cadrs_py/python, its native module a
/// link to target/release/lib_cadrs.so: `cargo build -r -p cadrs_py` first) with the repo's own
/// CADEvolve runner, tools/cadevolve/run.py.
pub fn run_cadrs(args: &[String]) -> Result<()> {
    let set = CaseSet::load(&PathBuf::from(args.first().context("set.json")?))?;
    let repo = Path::new(CRATE).join("../..");
    let out = flag(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| set.dir.join("cadrs.jsonl"));
    let names: Vec<&str> = set.cases.iter().map(|(_, n)| n.as_str()).collect();
    let mut cmd = std::process::Command::new("python3");
    // --tolerance parasolid|default|<mm>: the kernel's session tolerances (CADRS_TOLERANCE)
    if let Some(t) = flag(args, "--tolerance") {
        cmd.env("CADRS_TOLERANCE", t);
    }
    let st = cmd
        .arg("-I")
        .arg(repo.join("tools/cadevolve/run.py"))
        .arg("--python")
        .arg(python())
        .arg("--path")
        .arg(repo.join("crates/cadrs_py/python"))
        .arg("--out")
        .arg(&out)
        .arg("--jobs")
        .arg(flag(args, "--jobs").unwrap_or_else(|| "6".into()))
        .arg("--timeout")
        .arg(flag(args, "--timeout").unwrap_or_else(|| "120".into()))
        .arg("--only")
        .arg(names.join(","))
        .status()?;
    if !st.success() {
        bail!("tools/cadevolve/run.py failed: {st}");
    }
    // run.py selects by name: keep only this set's indices (names can repeat across generators)
    let keep: std::collections::BTreeSet<i64> = set.cases.iter().map(|(i, _)| *i).collect();
    let text = std::fs::read_to_string(&out)?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| {
            serde_json::from_str::<Value>(l)
                .ok()
                .and_then(|v| v["i"].as_i64())
                .is_some_and(|i| keep.contains(&i))
        })
        .collect();
    std::fs::write(&out, lines.join("\n") + "\n")?;
    println!("{} results in {}", lines.len(), out.display());
    Ok(())
}
