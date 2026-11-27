//! Translates [SolidCraft](https://github.com/tangency-labs/solidcraft) `recipe.json` parts into
//! cadrs documents.
//!
//! SolidCraft is an open-source CAD whose oracle tests describe each reference part as a
//! self-describing, parametric list of operations (`recipe.json`) with the values it must
//! measure (`expected.json`). This crate replays such a recipe through cadrs' document commands
//! (the same ones the app and its MCP tools run) into a normal cadrs feature list, and compares
//! the rebuilt part with the expected values by SolidCraft's own oracle rules.
//!
//! - [`run_recipe_case`] is `check`'s engine: a case folder's `recipe.json` built and compared.
//! - [`import`] writes a translated recipe into a cadrs document store as an editable document.
//! - [`Recipe::from_recipe`] gives the translated [`cadrs_core::Document`] without a store.
//!
//! The translation is the one `crates/cadrs_core/tests/reference_parts.rs` validates against the
//! 90 SolidCraft reference cases copied into this repo (89 pass; one known failure is documented
//! in `README.md`), so the same implementation is exercised by two suites.
//!
//! Recipe fixtures are SolidCraft's, MIT-licensed (Copyright (c) 2026 SolidCraft contributors);
//! see `crates/cadrs_core/tests/reference_parts/solidcraft/` and its README.

mod geom;
pub mod import;
pub mod oracle;
mod recipe;

use serde_json::Value;

pub use geom::V3;
pub use oracle::{
    ABS_FRAC, Check, MESH_TOL, Measured, Outcome, Pick, REL_TOL, add_target_values,
    compare_features, measure, run_recipe_case,
};
pub use recipe::{Recipe, Stop};

/// The recipe feature operations the translation handles (`recipe::Recipe::feature`'s arms).
///
/// Some are only partially handled: `thread` only when it is cosmetic
/// (`"modeled": false`), and `construction_plane` only for `offset` and `at_angle` methods.
pub const SUPPORTED_OPS: &[&str] = &[
    "sketch",
    "extrude",
    "revolve",
    "construction_plane",
    "fillet",
    "chamfer",
    "shell",
    "combine",
    "rectangular_pattern",
    "circular_pattern",
    "thread",
    "boundary_surface",
    "thicken",
    "remove",
    "sm_flange",
    "sm_hem",
    "sm_unfold",
    "sm_refold",
    "sheet_metal_rule_edit",
    "path_pattern",
    "mirror",
    "loft",
    "sweep",
    "hole",
    "draft",
    "split_body",
];

/// The sketch entity types that translate (`recipe::Recipe::sketch`'s arms).
pub const SUPPORTED_SKETCH_ENTITIES: &[&str] = &[
    "line",
    "circle",
    "arc_three_point",
    "arc_center_start_sweep",
    "point",
    // Partial: only a one-entity sketch whose fit points lie on a helix, which becomes a Helix
    // feature (case 49-helical-sweep); any other spline is unsupported.
    "fitted_spline",
];

/// The operations a recipe uses, in order: its features' `op`s, plus each sketch's entity types
/// as `sketch.<type>` (so a sketch is not reported as covered by an entity it cannot draw).
pub fn recipe_ops(recipe: &Value) -> Vec<String> {
    let mut ops = Vec::new();
    for f in recipe["features"].as_array().into_iter().flatten() {
        let op = f["op"].as_str().unwrap_or("");
        ops.push(op.to_string());
        if op == "sketch" {
            for e in f["entities"].as_array().into_iter().flatten() {
                ops.push(format!("sketch.{}", e["type"].as_str().unwrap_or("")));
            }
        }
    }
    ops
}

/// Whether an operation [`recipe_ops`] reported is handled.
pub fn op_supported(op: &str) -> bool {
    match op.strip_prefix("sketch.") {
        Some(t) => SUPPORTED_SKETCH_ENTITIES.contains(&t),
        None => SUPPORTED_OPS.contains(&op),
    }
}
