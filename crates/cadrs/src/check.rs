//! `cadrs --document <name or id> --check [--headless] [--data-dir <dir>] [--part-studio <name>]
//! [--sketch <name>] [--feature <name>] [--steps] [--dof]`: rebuilds a stored document (or one Part Studio of it) without opening a
//! window and reports every feature its feature lists would show in
//! error or warning, and why ([`cadrs_core::check`]). Every feature is computed again (no
//! saved rebuild is restored). Exits 1 when something is in error, 2 when the document can't be
//! found or read.

use std::path::PathBuf;

use cadrs_core::Store;
use cadrs_core::check::Severity;

pub struct Check {
    /// A document's name (any case) or id (or the start of one).
    document: String,
    data_dir: Option<PathBuf>,
    /// Only this Part Studio (`--part-studio <name>`).
    studio: Option<String>,
    /// A sketch to write out too (`--sketch <name>`).
    sketch: Option<String>,
    /// The parts after each part feature (`--steps`).
    steps: bool,
    /// Each sketch's degrees of freedom left (`--dof`).
    dof: bool,
    /// A feature to write out too (`--feature <name>`).
    feature: Option<String>,
}

/// `Some` when the arguments ask for a check.
pub fn parse(args: &[String]) -> Option<Result<Check, String>> {
    if !args.iter().any(|a| a == "--check") {
        return None;
    }
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(document) = value("--document") else {
        return Some(Err("--check needs --document <name or id>".into()));
    };
    // Where the app looks: --data-dir, else CADRS_DATA_DIR, else the platform data dir.
    let data_dir = value("--data-dir").map(PathBuf::from).or_else(|| std::env::var_os("CADRS_DATA_DIR").map(PathBuf::from)).or_else(Store::default_root);
    Some(Ok(Check { document, data_dir, studio: value("--part-studio"), sketch: value("--sketch"), steps: args.iter().any(|a| a == "--steps"), dof: args.iter().any(|a| a == "--dof"), feature: value("--feature") }))
}

pub fn run(c: &Check) -> u8 {
    let Some(root) = &c.data_dir else {
        eprintln!("no document store: pass --data-dir");
        return 2;
    };
    let store = Store::new(root);
    let (lib, _) = store.list();
    let want = c.document.to_lowercase();
    let mut found: Vec<_> = lib.entries.iter().filter(|e| e.id.to_string() == want || e.name.to_lowercase() == want).collect();
    if found.is_empty() {
        found = lib.entries.iter().filter(|e| e.id.to_string().starts_with(&want)).collect();
    }
    // Not one in the trash, when there is one that isn't.
    if found.len() > 1 && found.iter().any(|e| e.meta.trashed.is_none()) {
        found.retain(|e| e.meta.trashed.is_none());
    }
    let entry = match found.as_slice() {
        [e] => *e,
        [] => {
            eprintln!("no document \"{}\" in {}", c.document, root.display());
            return 2;
        }
        many => {
            eprintln!("\"{}\" names {} documents; pass the id:", c.document, many.len());
            for e in many {
                eprintln!("  {}  {}", e.id, e.name);
            }
            return 2;
        }
    };
    let file = match store.load(entry.id) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot read {} ({}): {e}", entry.name, entry.id);
            return 2;
        }
    };
    println!("{} ({}) in {}", entry.name, entry.id, root.display());
    let (mut errors, mut warnings) = (0, 0);
    for s in cadrs_core::check::check(&file.document, c.studio.as_deref()) {
        println!("\n# {}: {} features, {} parts, rebuilt in {:.1} s", s.name, s.features, s.parts, s.elapsed.as_secs_f64());
        if s.issues.is_empty() {
            println!("  no issues");
        }
        for i in &s.issues {
            let tag = match i.severity {
                Severity::Error => {
                    errors += 1;
                    "ERROR  "
                }
                Severity::Warning => {
                    warnings += 1;
                    "WARNING"
                }
            };
            println!("  {tag}  {}: {}", i.feature, i.detail);
        }
        if c.dof {
            println!("  Sketches (degrees of freedom left; 0 is fully defined):");
            for (name, dof) in &s.sketch_dof {
                println!("    {name}: {dof}");
            }
        }
    }
    println!("\n{errors} error(s), {warnings} warning(s)");
    if c.steps
        && let Some((name, features)) = cadrs_core::check::studio_features(&file.document, c.studio.as_deref())
    {
        println!("\nSteps of {name} (parts by volume, mm³, @ centre of mass, mm):");
        for (feature, parts) in cadrs_core::check::steps(&features) {
            let v: Vec<String> = parts.iter().map(|(_, v, c)| match c { Some(c) => format!("{v:.1}@({:.2} {:.2} {:.2})", c[0], c[1], c[2]), None => format!("{v:.1}") }).collect();
            println!("  {feature}: {}", v.join(", "));
        }
    }
    if let Some(name) = &c.feature {
        match cadrs_core::check::describe_feature(&file.document, c.studio.as_deref(), name) {
            Ok(text) => println!("\n{text}"),
            Err(e) => eprintln!("{e}"),
        }
    }
    if let Some(name) = &c.sketch {
        match cadrs_core::check::describe_sketch(&file.document, c.studio.as_deref(), name) {
            Ok(text) => println!("\n{text}"),
            Err(e) => eprintln!("{e}"),
        }
    }
    u8::from(errors > 0)
}
