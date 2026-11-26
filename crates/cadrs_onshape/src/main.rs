//! `cadrs-onshape`: imports scraped Onshape documents into the cadrs document store.
//!
//! ```text
//! cadrs-onshape import [<name or id>…] [--dry-run] [--timeout <s>]
//!                                                   import documents (all by default), each
//!                                                   in a child process (`import-one <id>
//!                                                   [--skip <feature ids>]`)
//! cadrs-onshape check-queries                       decode every geometry query in the data
//!
//! cadrs-onshape export <document name or id>… [--out <file.cadrs or dir>] [--all]
//!                                                   write stored documents as portable `.cadrs`
//!                                                   packages (for attaching to an issue, sharing,
//!                                                   or opening in another cadrs)
//! cadrs-onshape step <document name or id>… [--out <file.step or dir>] [--all] [--individual]
//!                                                   rebuild stored documents and export their
//!                                                   Part Studios as STEP (to compare with Onshape's
//!                                                   own STEP export)
//! cadrs-onshape list [--json]                       the documents in the store
//!
//! --raw <dir>        the scraped data (default ~/work/cadrs_onshape/raw)
//! --data-dir <dir>   the cadrs document store (default: the app's, as `cadrs` finds it)
//! --user <id>        who owns the imported documents (default: $USER, as the app sees you)
//! --timeout <s>      how long one feature may take before it counts as hung (default 300)
//! --json <file>      also write the run's report as JSON: each document's features and
//!                    parts, every defect, and the defects grouped into tickets
//!                    (`report::aggregate`). The desktop app's ticket list reads it.
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
    let mut data_dir = std::env::var_os("CADRS_DATA_DIR")
        .map(PathBuf::from)
        .or_else(Store::default_root);
    let mut dry_run = false;
    let mut skip = Vec::new();
    let mut timeout = Duration::from_secs(300);
    let mut json_out: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut all = false;
    let mut individual = false;
    // Imported documents belong to the local user, as the app identifies them.
    let mut user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--raw" => raw_dir = it.next().map(PathBuf::from).unwrap_or(raw_dir),
            "--data-dir" => data_dir = it.next().map(PathBuf::from),
            "--dry-run" => dry_run = true,
            "--user" => user = it.next().unwrap_or(user),
            "--skip" => {
                skip = it
                    .next()
                    .map(|s| s.split(',').map(String::from).collect())
                    .unwrap_or_default()
            }
            "--timeout" => {
                timeout = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(timeout)
            }
            "--json" => json_out = it.next().map(PathBuf::from),
            "--out" => out = it.next().map(PathBuf::from),
            "--all" => all = true,
            "--individual" => individual = true,
            _ => rest.push(a),
        }
    }
    match rest.first().map(String::as_str) {
        Some("check-queries") => check_queries(&raw_dir),
        Some("list") => list_documents(&data_dir, json_out.is_some()),
        Some("export") => export(&data_dir, &rest[1..], out.as_deref(), all),
        Some("step") => step(&data_dir, &rest[1..], out.as_deref(), all, individual),
        Some("compare") => match (rest.get(1), rest.get(2)) {
            (Some(c), Some(s)) => match cadrs_onshape::compare::compare(c.as_ref(), s.as_ref()) {
                Ok(j) => {
                    println!("{j}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("compare: {e}");
                    ExitCode::FAILURE
                }
            },
            _ => {
                eprintln!("usage: cadrs-onshape compare <file.cadrs> <file.step>");
                ExitCode::from(2)
            }
        },
        Some(cmd @ ("import" | "import-one")) => {
            let Some(data_dir) = data_dir else {
                eprintln!("no document store: pass --data-dir");
                return ExitCode::from(2);
            };
            if cmd == "import" {
                import(
                    &raw_dir,
                    &data_dir,
                    &rest[1..],
                    dry_run,
                    timeout,
                    &user,
                    json_out.as_deref(),
                )
            } else {
                import_one(
                    &raw_dir,
                    &Store::new(data_dir),
                    rest.get(1).map_or("", String::as_str),
                    &skip,
                    dry_run,
                    &user,
                    json_out.as_deref(),
                )
            }
        }
        _ => {
            eprintln!(
                "usage: cadrs-onshape (import [<name or id>…] [--dry-run] [--json <file>] | list [--json] | export <name or id>… [--out <file.cadrs or dir>] [--all] | step <name or id>… [--out <file.step or dir>] [--all] [--individual] | compare <file.cadrs> <file.step> | check-queries) [--raw <dir>] [--data-dir <dir>]"
            );
            ExitCode::from(2)
        }
    }
}

/// Imports the scraped documents whose name or id contains one of `filters` (all if none),
/// each in a child process (`import-one`) with a time limit: a rebuild that hangs in the
/// kernel can't be interrupted, so the child is killed and the document imported again
/// without the feature it was on.
fn import(
    raw_dir: &Path,
    data_dir: &Path,
    filters: &[String],
    dry_run: bool,
    timeout: Duration,
    user: &str,
    json_out: Option<&Path>,
) -> ExitCode {
    let docs: Vec<_> = raw::documents(raw_dir)
        .into_iter()
        .filter(|d| {
            filters.is_empty()
                || filters
                    .iter()
                    .any(|f| d.id == *f || d.name.to_lowercase().contains(&f.to_lowercase()))
        })
        .collect();
    if docs.is_empty() {
        eprintln!("no scraped documents match under {}", raw_dir.display());
        return ExitCode::FAILURE;
    }
    let docs = raw::derive_order(docs);
    let exe = std::env::current_exe().expect("own path");
    let progress =
        std::env::temp_dir().join(format!("cadrs-onshape-progress-{}", std::process::id()));
    // Each child writes its document's report here (as JSON), for this run's aggregate.
    let child_json =
        std::env::temp_dir().join(format!("cadrs-onshape-report-{}.json", std::process::id()));
    let mut text = String::new();
    let mut documents: Vec<serde_json::Value> = Vec::new();
    let mut failed = 0;
    for d in &docs {
        let mut skip: Vec<String> = Vec::new();
        let out = loop {
            std::fs::remove_file(&progress).ok();
            std::fs::remove_file(&child_json).ok();
            let mut cmd = Command::new(&exe);
            cmd.arg("import-one")
                .arg(&d.id)
                .arg("--raw")
                .arg(raw_dir)
                .arg("--data-dir")
                .arg(data_dir)
                .arg("--user")
                .arg(user);
            if json_out.is_some() {
                cmd.arg("--json").arg(&child_json);
            }
            if dry_run {
                cmd.arg("--dry-run");
            }
            if !skip.is_empty() {
                cmd.arg("--skip").arg(skip.join(","));
            }
            cmd.env("CADRS_ONSHAPE_PROGRESS", &progress)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            let Ok(mut child) = cmd.spawn() else {
                break None;
            };
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
                    eprintln!(
                        "  {}: feature {f} hung or crashed; importing again without it",
                        d.name
                    );
                    skip.push(f);
                }
                _ => break (!s.is_empty()).then_some(s),
            }
        };
        if json_out.is_some() {
            match std::fs::read(&child_json)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            {
                Some(doc) => documents.push(doc),
                None => {
                    failed += 1;
                    documents.push(serde_json::json!({
                        "name": d.name,
                        "onshape_id": d.id,
                        "failed": true,
                    }));
                }
            }
        }
        let line = out.unwrap_or_else(|| {
            if json_out.is_none() {
                failed += 1;
            }
            format!(
                "# {} ({})\n  ! import failed or kept hanging\n",
                d.name, d.id
            )
        });
        print!("{line}");
        text += &line;
    }
    std::fs::remove_file(&progress).ok();
    std::fs::remove_file(&child_json).ok();
    // A dry run over some documents doesn't replace the report of the last full import.
    if let Some(parent) = raw_dir.parent().filter(|_| filters.is_empty() || !dry_run) {
        std::fs::write(parent.join("import-report.txt"), &text).ok();
    }
    if let Some(path) = json_out {
        let tool = format!("cadrs-onshape {}", env!("CARGO_PKG_VERSION"));
        let json = cadrs_onshape::report::aggregate(documents, &tool, user, dry_run);
        match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(path, bytes) {
                    eprintln!("cannot write JSON report {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("cannot serialize JSON report: {e}"),
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Imports one document (by Onshape id) and prints its report.
fn import_one(
    raw_dir: &Path,
    store: &Store,
    id: &str,
    skip: &[String],
    dry_run: bool,
    user: &str,
    json_out: Option<&Path>,
) -> ExitCode {
    let Some(d) = raw::documents(raw_dir).into_iter().find(|d| d.id == id) else {
        eprintln!("no scraped document {id}");
        return ExitCode::FAILURE;
    };
    let options = cadrs_onshape::Options {
        skip: skip.iter().cloned().collect(),
        progress: std::env::var_os("CADRS_ONSHAPE_PROGRESS").map(PathBuf::from),
        only: None,
        store: Some(store.clone()),
    };
    // Derived features read other documents from the store.
    cadrs_core::derived::set_document_loader(Some(cadrs_core::derived::store_loader(
        store.clone(),
    )));
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
        if el.assembly_model().is_none()
            && features.iter().any(cadrs_core::Feature::is_part_feature)
        {
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
                // Onshape's versions other documents may pin (those scraped), as its history.
                if let Some(log) =
                    cadrs_onshape::history_with_versions(&d, &imported.doc, &imported.meta, user)
                    && let Err(e) = log.save(store)
                {
                    line += &format!("  ! versions not saved: {e}\n");
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
    if let Some(path) = json_out {
        let json = imported.report.to_json(started.elapsed().as_secs_f64());
        match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(path, bytes) {
                    line += &format!("  ! JSON report not written: {e}\n");
                }
            }
            Err(e) => line += &format!("  ! JSON report not serialized: {e}\n"),
        }
    }
    print!("{line}");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The documents in the store, as JSON (`--json`) or a table: the app's preflight checks that an
/// import report's documents really exist before it offers to export them.
fn list_documents(data_dir: &Option<PathBuf>, json: bool) -> ExitCode {
    let Some(dir) = data_dir else {
        eprintln!("no document store: pass --data-dir");
        return ExitCode::from(2);
    };
    let store = Store::new(dir.clone());
    let (library, _) = store.list();
    if json {
        let docs: Vec<serde_json::Value> = library
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id.0.to_string(),
                    "name": e.name,
                    "created": e.meta.created,
                    "modified": e.meta.modified,
                })
            })
            .collect();
        match serde_json::to_string_pretty(&serde_json::json!({
            "store": dir.display().to_string(),
            "documents": docs,
        })) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("cannot serialize the document list: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        for e in &library.entries {
            println!("{}  {}", e.id.0, e.name);
        }
    }
    ExitCode::SUCCESS
}

/// A file-name-safe version of a document name for its exported package.
fn package_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches(['-', '.']).to_string();
    if out.is_empty() { "document".into() } else { out }
}

/// Writes stored documents as portable `.cadrs` packages: one file for one document, a folder when
/// exporting several or `--all`. Packages keep the document's history, blobs and thumbnail, so they
/// can be attached to an issue or opened on another machine.
fn export(data_dir: &Option<PathBuf>, names: &[String], out: Option<&Path>, all: bool) -> ExitCode {
    let Some(dir) = data_dir else {
        eprintln!("no document store: pass --data-dir");
        return ExitCode::from(2);
    };
    let store = Store::new(dir.clone());
    let (library, _) = store.list();
    let entries: Vec<_> = if all {
        library.entries.iter().collect()
    } else {
        names
            .iter()
            .filter_map(|want| {
                let found = library.entries.iter().find(|e| {
                    e.name == *want
                        || e.name.to_lowercase().contains(&want.to_lowercase())
                        || e.id.0.to_string() == *want
                });
                if found.is_none() {
                    eprintln!("no stored document matching {want}");
                }
                found
            })
            .collect()
    };
    if entries.is_empty() {
        eprintln!("nothing to export under {}", dir.display());
        return ExitCode::FAILURE;
    }
    let single_file = entries.len() == 1
        && out.is_some_and(|p| p.extension().is_some_and(|e| e == "cadrs"));
    let out_dir = match out {
        Some(p) if !single_file => p.to_path_buf(),
        _ if !single_file => PathBuf::from("cadrs-exports"),
        _ => PathBuf::new(),
    };
    if !single_file && let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for entry in entries {
        let file = match store.load(entry.id) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{}: cannot load: {e}", entry.name);
                failed += 1;
                continue;
            }
        };
        let target = if single_file {
            out.expect("single file has an --out").to_path_buf()
        } else {
            out_dir.join(format!("{}.cadrs", package_name(&entry.name)))
        };
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty())
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("{}: cannot create {}: {e}", entry.name, parent.display());
            failed += 1;
            continue;
        }
        match cadrs_core::native::export(&target, &file.document, &file.meta, &store) {
            Ok(()) => println!("{}", target.display()),
            Err(e) => {
                eprintln!("{}: cannot export: {e}", entry.name);
                failed += 1;
            }
        }
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Rebuilds stored documents with the cadrs kernel and writes their Part Studios as STEP: one file
/// per Part Studio (`<document> - <part studio>.step`, or just `<document>.step` when it has one),
/// every part of it in the file unless `--individual`. This is the STEP to compare with Onshape's
/// own export of the same Part Studio: `compare` does the same comparison directly from the
/// stored document, so this is for tools that want a file.
fn step(
    data_dir: &Option<PathBuf>,
    names: &[String],
    out: Option<&Path>,
    all: bool,
    individual: bool,
) -> ExitCode {
    let Some(dir) = data_dir else {
        eprintln!("no document store: pass --data-dir");
        return ExitCode::from(2);
    };
    let store = Store::new(dir.clone());
    let (library, _) = store.list();
    let entries: Vec<_> = if all {
        library.entries.iter().collect()
    } else {
        names
            .iter()
            .filter_map(|want| {
                let found = library.entries.iter().find(|e| {
                    e.name == *want
                        || e.name.to_lowercase().contains(&want.to_lowercase())
                        || e.id.0.to_string() == *want
                });
                if found.is_none() {
                    eprintln!("no stored document matching {want}");
                }
                found
            })
            .collect()
    };
    if entries.is_empty() {
        eprintln!("nothing to export under {}", dir.display());
        return ExitCode::FAILURE;
    }
    let out_dir = match out {
        Some(p) if !p.extension().is_some_and(|e| e == "step") => p.to_path_buf(),
        _ => PathBuf::from("step-exports"),
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create {}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let mut failed = 0;
    for entry in entries {
        let file = match store.load(entry.id) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{}: cannot load: {e}", entry.name);
                failed += 1;
                continue;
            }
        };
        let studios: Vec<_> = file
            .document
            .elements
            .iter()
            .filter(|el| matches!(el.kind, cadrs_core::document::ElementKind::PartStudio { .. }))
            .collect();
        if studios.is_empty() {
            eprintln!("{}: no Part Studio", entry.name);
            failed += 1;
            continue;
        }
        for studio in studios {
            let features = studio.features_to_build();
            let built = cadrs_core::rebuild::build(&features);
            let parts: Vec<(cadrs_core::ids::PartId, String)> = built
                .parts
                .iter()
                .filter(|p| !built.composites.iter().any(|c| c.part == p.id))
                .map(|p| (p.id, p.name.clone()))
                .collect();
            if parts.is_empty() {
                eprintln!("{} / {}: no parts to export", entry.name, studio.name);
                failed += 1;
                continue;
            }
            let request = cadrs_core::export::StepRequest {
                parts,
                y_up: false,
                individual,
            };
            let files = match cadrs_core::rebuild::export_step(features, request).wait() {
                Ok(files) => files,
                Err(e) => {
                    eprintln!("{} / {}: cannot export STEP: {e}", entry.name, studio.name);
                    failed += 1;
                    continue;
                }
            };
            let base = if studios_len(&file.document, &entry.name) > 1 {
                format!("{} - {}", entry.name, studio.name)
            } else {
                entry.name.clone()
            };
            for step_file in files {
                // One file with every part keeps the Part Studio's name; --individual adds the
                // part's name (and its own file).
                let stem = if individual {
                    cadrs_core::export::file_name(&base, step_file.part.as_deref())
                } else {
                    cadrs_core::export::sanitize(&base)
                };
                let target = cadrs_core::export::unique_path(&out_dir, &stem);
                match std::fs::write(&target, &step_file.bytes) {
                    Ok(()) => println!("{}", target.display()),
                    Err(e) => {
                        eprintln!("{}: cannot write {}: {e}", entry.name, target.display());
                        failed += 1;
                    }
                }
            }
        }
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// How many Part Studios a document has (one keeps the file named just after the document).
fn studios_len(document: &cadrs_core::document::Document, _name: &str) -> usize {
    document
        .elements
        .iter()
        .filter(|el| matches!(el.kind, cadrs_core::document::ElementKind::PartStudio { .. }))
        .count()
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
                        let Some(s) = q["queryString"].as_str() else {
                            continue;
                        };
                        match query::decode(s) {
                            Ok(Some(_)) => ok += 1,
                            Ok(None) => other += 1,
                            Err(e) => {
                                failed += 1;
                                eprintln!(
                                    "{} / {} / {}: {e}",
                                    doc.name, feat["name"], p["parameterId"]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    println!("decoded {ok}, other forms {other}, failed {failed}");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
