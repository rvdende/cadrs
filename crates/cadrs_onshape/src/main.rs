//! `cadrs-onshape`: imports scraped Onshape documents into the cadrs document store.
//!
//! ```text
//! cadrs-onshape import [<name or id>…] [--dry-run] [--timeout <s>]
//!                                                   import documents (all by default), each
//!                                                   in a child process (`import-one <id>
//!                                                   [--skip <feature ids>]`)
//! cadrs-onshape check-queries                       decode every geometry query in the data
//!
//! --raw <dir>        the scraped data (default ~/work/cadrs_onshape/raw)
//! --data-dir <dir>   the cadrs document store (default: the app's, as `cadrs` finds it)
//! --user <id>        who owns the imported documents (default: $USER, as the app sees you)
//! --timeout <s>      how long one feature may take before it counts as hung (default 300)
//! ```
//!
//! Importing a document again replaces the earlier import (the cadrs ids derive from
//! Onshape's). The report goes to stdout and to `<raw>/../import-report.txt`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use cadrs_core::Store;
use cadrs_onshape::{query, raw};

fn main() -> ExitCode {
    // An import builds a Derived feature's source tab long before the feature: keep every
    // output the cache has room for, not only those of the last few rebuilds.
    cadrs_core::rebuild::keep_unused_for(u64::MAX);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut raw_dir = default_raw();
    let mut data_dir = std::env::var_os("CADRS_DATA_DIR").map(PathBuf::from).or_else(Store::default_root);
    let mut dry_run = false;
    let mut skip = Vec::new();
    let mut timeout = Duration::from_secs(300);
    // Imported documents belong to the local user, as the app identifies them.
    let mut user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "user".into());
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--raw" => raw_dir = it.next().map(PathBuf::from).unwrap_or(raw_dir),
            "--data-dir" => data_dir = it.next().map(PathBuf::from),
            "--dry-run" => dry_run = true,
            "--user" => user = it.next().unwrap_or(user),
            "--skip" => skip = it.next().map(|s| s.split(',').map(String::from).collect()).unwrap_or_default(),
            "--timeout" => timeout = it.next().and_then(|s| s.parse().ok()).map(Duration::from_secs).unwrap_or(timeout),
            _ => rest.push(a),
        }
    }
    match rest.first().map(String::as_str) {
        Some("check-queries") => check_queries(&raw_dir),
        Some(cmd @ ("import" | "import-one")) => {
            let Some(data_dir) = data_dir else {
                eprintln!("no document store: pass --data-dir");
                return ExitCode::from(2);
            };
            if cmd == "import" {
                import(&raw_dir, &data_dir, &rest[1..], dry_run, timeout, &user)
            } else {
                import_one(&raw_dir, &Store::new(data_dir), rest.get(1).map_or("", String::as_str), &skip, dry_run, &user)
            }
        }
        _ => {
            eprintln!("usage: cadrs-onshape (import [<name or id>…] [--dry-run] | check-queries) [--raw <dir>] [--data-dir <dir>]");
            ExitCode::from(2)
        }
    }
}

/// Imports the scraped documents whose name or id contains one of `filters` (all if none),
/// each in a child process (`import-one`) with a time limit: a rebuild that hangs in the
/// kernel can't be interrupted, so the child is killed and the document imported again
/// without the feature it was on.
fn import(raw_dir: &Path, data_dir: &Path, filters: &[String], dry_run: bool, timeout: Duration, user: &str) -> ExitCode {
    let docs: Vec<_> = raw::documents(raw_dir)
        .into_iter()
        .filter(|d| filters.is_empty() || filters.iter().any(|f| d.id == *f || d.name.to_lowercase().contains(&f.to_lowercase())))
        .collect();
    if docs.is_empty() {
        eprintln!("no scraped documents match under {}", raw_dir.display());
        return ExitCode::FAILURE;
    }
    let docs = raw::derive_order(docs);
    let exe = std::env::current_exe().expect("own path");
    let progress = std::env::temp_dir().join(format!("cadrs-onshape-progress-{}", std::process::id()));
    let mut text = String::new();
    let mut failed = 0;
    for d in &docs {
        let mut skip: Vec<String> = Vec::new();
        let out = loop {
            std::fs::remove_file(&progress).ok();
            let mut cmd = Command::new(&exe);
            cmd.arg("import-one").arg(&d.id).arg("--raw").arg(raw_dir).arg("--data-dir").arg(data_dir).arg("--user").arg(user);
            if dry_run {
                cmd.arg("--dry-run");
            }
            if !skip.is_empty() {
                cmd.arg("--skip").arg(skip.join(","));
            }
            cmd.env("CADRS_ONSHAPE_PROGRESS", &progress).stdout(Stdio::piped()).stderr(Stdio::inherit());
            let Ok(mut child) = cmd.spawn() else { break None };
            let mut stdout = child.stdout.take().expect("piped");
            let reader = std::thread::spawn(move || {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut stdout, &mut s).ok();
                s
            });
            // The time limit is per feature: the child writes the feature it is on to
            // `progress`, and a large document may take much longer than any one feature.
            let mut started = Instant::now();
            let mut on = String::new();
            // A child that crashed (the kernel can segfault) is handled like one that hung: the
            // feature it was on is left out next time.
            let finished = loop {
                if let Ok(Some(status)) = child.try_wait() {
                    break status.code().is_some();
                }
                let now_on = std::fs::read_to_string(&progress).unwrap_or_default();
                if now_on != on {
                    on = now_on;
                    started = Instant::now();
                }
                if started.elapsed() > timeout {
                    child.kill().ok();
                    child.wait().ok();
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            let s = reader.join().unwrap_or_default();
            if finished {
                break Some(s);
            }
            match std::fs::read_to_string(&progress) {
                Ok(f) if !skip.contains(&f) && skip.len() < 8 => {
                    eprintln!("  {}: feature {f} hung or crashed; importing again without it", d.name);
                    skip.push(f);
                }
                _ => break (!s.is_empty()).then_some(s),
            }
        };
        let line = out.unwrap_or_else(|| {
            failed += 1;
            format!("# {} ({})\n  ! import failed or kept hanging\n", d.name, d.id)
        });
        print!("{line}");
        text += &line;
    }
    std::fs::remove_file(&progress).ok();
    // A dry run over some documents doesn't replace the report of the last full import.
    if let Some(parent) = raw_dir.parent().filter(|_| filters.is_empty() || !dry_run) {
        std::fs::write(parent.join("import-report.txt"), &text).ok();
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Imports one document (by Onshape id) and prints its report.
fn import_one(raw_dir: &Path, store: &Store, id: &str, skip: &[String], dry_run: bool, user: &str) -> ExitCode {
    let Some(d) = raw::documents(raw_dir).into_iter().find(|d| d.id == id) else {
        eprintln!("no scraped document {id}");
        return ExitCode::FAILURE;
    };
    let options = cadrs_onshape::Options {
        skip: skip.iter().cloned().collect(),
        progress: std::env::var_os("CADRS_ONSHAPE_PROGRESS").map(PathBuf::from),
        only: None,
    };
    // Derived features read other documents from the store.
    cadrs_core::derived::set_document_loader(Some(cadrs_core::derived::store_loader(store.clone())));
    // Rebuild snapshots next to the documents, where the app keeps them: a Derived feature
    // restores its source document's Part Studio from one instead of rebuilding it.
    if let Some(dir) = store.root().parent() {
        let disk = cadrs_core::blob_store::DiskStore::new(dir.join("cache").join("session"));
        cadrs_core::rebuild::session::set_store(Some(std::sync::Arc::new(disk)));
    }
    let started = Instant::now();
    let imported = cadrs_onshape::import_document(&d, user, &options);
    // This document's Part Studios as built, for later documents that derive from them (and
    // the app opening it).
    for el in &imported.doc.elements {
        let features = el.features().to_vec();
        if el.assembly_model().is_none() && features.iter().any(cadrs_core::Feature::is_part_feature) {
            cadrs_core::rebuild::save_snapshot_now(features);
        }
    }
    let mut line = format!("{}", imported.report);
    let mut ok = true;
    if !dry_run {
        match store.save(&imported.doc, &imported.meta) {
            Ok(()) => {
                let thumb = d.dir.join("thumbnail.png");
                if thumb.exists() {
                    std::fs::copy(&thumb, store.thumbnail_path(imported.doc.id)).ok();
                }
                // It must load back as the app will load it.
                if let Err(e) = store.load(imported.doc.id) {
                    ok = false;
                    line += &format!("  ! saved but does not load: {e}\n");
                }
            }
            Err(e) => {
                ok = false;
                line += &format!("  ! not saved: {e}\n");
            }
        }
    }
    line += &format!("  ({:.1} s)\n", started.elapsed().as_secs_f64());
    print!("{line}");
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn default_raw() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join("work/cadrs_onshape/raw"))
        .unwrap_or_else(|| PathBuf::from("raw"))
}

/// Decodes every `qCompressed` query in every scraped feature list and reports failures.
fn check_queries(raw_dir: &std::path::Path) -> ExitCode {
    let (mut ok, mut other, mut failed) = (0, 0, 0);
    for doc in raw::documents(raw_dir) {
        for el in doc.elements() {
            let Some(f) = el.features() else { continue };
            for feat in f["features"].as_array().into_iter().flatten() {
                for p in feat["parameters"].as_array().into_iter().flatten() {
                    for q in p["queries"].as_array().into_iter().flatten() {
                        let Some(s) = q["queryString"].as_str() else { continue };
                        match query::decode(s) {
                            Ok(Some(_)) => ok += 1,
                            Ok(None) => other += 1,
                            Err(e) => {
                                failed += 1;
                                eprintln!("{} / {} / {}: {e}", doc.name, feat["name"], p["parameterId"]);
                            }
                        }
                    }
                }
            }
        }
    }
    println!("decoded {ok}, other forms {other}, failed {failed}");
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
