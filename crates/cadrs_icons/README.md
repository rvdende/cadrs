# cadrs_icons

cadrs's SVG icons, embedded as Rust constants. Vendored from
[icon-rs](https://github.com/rvdende/icon-rs) (0.5.1) so new icons land with the cadrs change that
needs them; icon-rs itself is no longer a dependency and is not kept in sync. Four kinds, drawn
procedurally so the set stays consistent:

- **Solid**: shaded isometric features, mates, relations and document types.
- **Sketch**: flat sketch tools, with the points the user picks in the accent colour.
- **Glyph**: bold sketch constraints and viewport markers, legible at 10-16 px.
- **Line**: interface icons.

Glyph and line icons are drawn in ink with a small accent detail, so apps can tint them.

```rust
use cadrs_icons::{Palette, Rgb, EXTRUDE, UNDO};

let light: &str = EXTRUDE.svg;       // for light backgrounds
let dark: &str = EXTRUDE.svg_dark;   // for dark backgrounds
let svg = cadrs_icons::get("loft").unwrap().themed(is_dark);

// Any other theme: a line colour and an accent. Faces are neutral tints of the line.
let palette = Palette::new(Rgb(0x0c, 0x4a, 0x6e), Rgb(0xf5, 0x9e, 0x0b));
let custom: String = EXTRUDE.recolor(&palette);

// Glyph and line icons are ink plus a little accent: paint the ink any colour.
assert!(UNDO.kind.is_tintable());
let white: String = UNDO.tinted(Rgb::WHITE);

// In HTML, let CSS colour the ink.
let html: String = UNDO.current_color();
```

## Development

The icons are drawn in Rust by the generator in `crates/cadrs_icons_gen` (not published).

- `icons/*.svg`, `icons/dark/*.svg`, `src/generated.rs`: generated; don't edit by hand.
- `cadrs_icons_gen/src/common.rs`: the palette, the drawing conventions for each kind, and the SVG
  and geometry helpers. `src/palette.rs` holds the crate's `Rgb` and `Palette`; the generator
  includes it too, and a test keeps the two palettes in step.
- `cadrs_icons_gen/src/icons/`: one module per family (`part`, `sheet_metal`, `assembly`, `sketch`,
  `glyphs`, `ui_a`, `ui_b`, `eda` for schematic and PCB layout, `cam` for machining, `platform` for
  operating systems and code hosting). Each icon is a function returning its SVG elements, listed
  in the module's `icons()` in toolbar order.

```sh
cargo run -r -p cadrs_icons_gen                             # validate, fit and rewrite icons/ and src/generated.rs
cargo run -r -p cadrs_icons_gen -- --check                  # fail if those are out of date (CI runs this)
cargo run -r -p cadrs_icons_gen -- --only sketch --out DIR  # render one module into DIR for review
```

The generator checks every icon (kebab-case name, colours allowed for its kind, well-formed XML,
no ids), zooms solid and sketch icons to their ink, and writes each light SVG with its dark variant.

To add an icon, write its function in the right module, add it to that module's `icons()`, and
run the generator. [HOWTO.md](HOWTO.md) walks through it with examples.

Review images:

- `cargo run -r -p cadrs_icons --example sheet -- OUT.png crates/cadrs_icons/icons [NAME...]`: a
  labelled review sheet at 96, 24 and 16 px in both themes. Pass the `--out` directory of a
  `--only` run in place of the icons folder to review a module before regenerating.
- `cargo run -r -p cadrs_icons --example preview [out.png] [scale]`: a contact sheet of every icon
  (default `target/preview.png`).
