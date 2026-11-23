# cadrs_solidcraft

A translator from [SolidCraft](https://github.com/tangency-labs/solidcraft) `recipe.json` parts
into cadrs documents, plus a CLI (`cadrs-solidcraft`) and tests that drive SolidCraft's reference
cases through cadrs.

SolidCraft is an open-source CAD whose oracle tests describe each of its reference parts as a
self-describing, parametric list of operations — a `recipe.json` — together with the values the
part must measure (`expected.json`). The 90 fixtures in
`crates/cadrs_core/tests/reference_parts/solidcraft/` are those cases, copied unchanged (MIT;
see [License](#license)).

## What it does

- **Library**: [`Recipe::from_recipe`] turns a parsed recipe into a normal cadrs
  [`Document`](cadrs_core::Document) by replaying each operation through the same document
  commands the app and its MCP tools run (`AddSketch`, `AddExtrude`, `AddFeature`, …). The
  result is an editable feature list, not a mesh: cadrs rebuilds it through its own kernel.
- **Oracle**: [`run_recipe_case`] rebuilds the translated part and compares it with the case's
  `expected.json` by SolidCraft's own oracle rules — body count, volume and area within a
  relative 1e-3 unless the case says otherwise, centre of mass and bounding box within 1e-3 of
  the bounding box's diagonal, face count where the geometry fixes it, and a STEP export read
  back with the same volume and area. cadrs additionally checks the mesh it draws (each face's
  triangles cover its exact area within 1e-2).
- **Store import**: [`import::import_text`] saves a translated recipe into a cadrs document
  store (`document.ron` per document, as the app and the Onshape importer write), owned by the
  given user, and reads it back to prove it loads.

`Recipe` is the translation already exercised by
`crates/cadrs_core/tests/reference_parts.rs` since issue #101 — this crate is that
implementation extracted, so the same code is validated by two suites: that file's per-case
tests and this crate's fixture tests.

## Status

**90 / 90 fixtures translate; 89 / 90 pass the oracle.** `75-sm-bracket-holes` is a known
geometry failure, not a translation gap: SolidCraft mitres two flanges' corner, cadrs makes no
corner between flanges of two features. It is listed in the `cadrs_core` reference-parts README.
`coverage.md` has the operation-by-operation table (which operations are supported, partial or
unsupported, and how many cases use and pass each) and the supported sketch entities,
constraints and dimensions.

Every operation used by the 90 fixtures is translated, so no fixture is reported as
unsupported; the coverage floor this crate's test asserts is 89 passing cases. The task
description for this work mentioned 84 fixtures — the repository actually contains 90
(`find … -name recipe.json | wc -l` → 90, each with an `expected.json`).

## CLI

```sh
# Translate and save recipes into a cadrs document store, as normal editable documents.
# --data-dir defaults to the app's store; --user defaults to $USER.
cargo run -r -p cadrs_solidcraft --offline -- import path/to/recipe.json… [--data-dir <dir>] [--user <name>]

# Build each recipe through cadrs (no store) and compare it with the expected.json beside it.
# Prints a table, the operations used and their coverage; exits nonzero on failures.
cargo run -r -p cadrs_solidcraft --offline -- check path/to/recipe.json… [--json]
```

`check --json` prints the same results as JSON (`cases`, `summary`, `ops`, `unsupported_ops`,
`supported_ops_not_exercised`). Both commands take several recipes at once; `import` reports
per-file errors and exits nonzero if any failed.

## Tests

```sh
cargo test -r -p cadrs_solidcraft --offline              # this crate: fixtures + CLI smoke tests
cargo test -r -p cadrs_core --test reference_parts --offline   # the same cases, one test each
```

- `tests/reference_cases.rs` walks `crates/cadrs_core/tests/reference_parts/solidcraft/` (via
  `CARGO_MANIFEST_DIR`), runs all 90 cases on the available threads, asserts every case is
  translated, that only the documented known failure fails, and that at least 89 pass.
- `tests/cli.rs` runs the real binary: `check` on the box case must be green and print the
  passing result as JSON, and `import` into a temporary data dir must write a store document
  that loads, is named after the recipe, and rebuilds to the box's 24000 mm³.

## The workflow difference (fidelity limits)

The user's question was whether SolidCraft's operations can be imported "given the different
workflow". They can, because both systems are parametric at heart; what differs is what the
operations *are*, and that is where fidelity is lost:

- **SolidCraft recipes are an ordered list of operations selected against transient geometry.**
  Each operation names what it applies to *at that moment* (a sketch's entity ids, a loop of
  curve ids, a face/edge by a point on it, a body by the name the recipe gave it). The recipe is
  the whole truth: replaying it needs no history beyond the ids it defines itself.
- **cadrs documents are feature lists replayed through a history.** The translator replays the
  recipe's operations as cadrs features in order, resolving the recipe's references to cadrs
  feature/entity ids as it goes (a recipe face is picked by the nearest built face to its
  point; the translator runs a rebuild after each operation that needs geometry, as SolidCraft
  does with its own kernel). The resulting document then lives its own life: later edits are
  cadrs edits, and a rebuild after an edit can fail where the original recipe would not, because
  cadrs' kernel and SolidCraft's may resolve the same selection differently.
- **The fixtures contain operations SolidCraft's format can express but cadrs has no equivalent
  for.** The translator reports those as unsupported rather than guessing (see `coverage.md`);
  an unsupported case is not silently skipped, and `check` exits nonzero for it.
- **Even for supported operations, exact selection can differ.** SolidCraft's reference parts
  include conventions cadrs does not have (the mitred sheet-metal corner of
  `75-sm-bracket-holes`) and values that depend on the kernel's curve representation (the
  `17-loft` circle/spline pairing). The oracle rules (relative 1e-3) tolerate representation
  differences, not different topology.
- **Recipes with parameters** are evaluated with the recipe's `parameters` map (the format's
  expression syntax is a subset the translator evaluates itself, not cadrs' variable system), so
  the imported document has baked values, not a cadrs variable table.

So: import is faithful for the operations covered, and reviewable — the imported document is a
normal cadrs document with a feature per operation, so a user can see and edit what was built.
It is not a promise that every recipe that SolidCraft can run will import, and a document
imported from a recipe is not the same object as SolidCraft's recipe: it is a cadrs feature
list that reproduces the part.

## License

The crate is cadrs' (MIT OR Apache-2.0, see the repository's license). The recipe fixtures are
SolidCraft's, used under the MIT License: Copyright (c) 2026 SolidCraft contributors; SolidCraft
is also available under the Apache License 2.0. The fixture files are not modified. The
translation follows SolidCraft's reader (`apps/solidcraft-cli/src/recipe.rs`) and comparison
rules (`apps/solidcraft-cli/src/oracle.rs`) for what the format means.
