# How to add an icon

Icons are not drawn by hand. Each one is a Rust function in the generator (`crates/cadrs_icons_gen`) that returns
its SVG elements; `cargo run -r -p cadrs_icons_gen` turns those into `icons/<name>.svg`, `icons/dark/<name>.svg`
and a constant in `src/generated.rs`. Adding an icon takes four steps:

1. Pick its kind and the module it belongs in.
2. Write its drawing function.
3. List it in that module's `icons()`.
4. Generate, review and commit.

## 1. Pick the kind and the module

The kind decides the root stroke width, which colours the icon may use and whether apps can tint it.

| Kind     | For                                              | Stroke | Colours                            |
|----------|--------------------------------------------------|--------|------------------------------------|
| `Solid`  | Isometric CAD geometry: features, mates, documents | 1.1  | `INK TOP SOFT MID SHADE ACCENT`    |
| `Sketch` | Flat 2D sketch tools                             | 1.5    | `INK TOP SOFT MID SHADE ACCENT`    |
| `Glyph`  | Constraint and viewport markers shown at 10-16 px | 2     | `INK ACCENT`                       |
| `Line`   | Interface icons (undo, folder, search...)        | 1.75   | `INK ACCENT`                       |

Each family is one module in `crates/cadrs_icons_gen/src/icons/`:

| Module        | Contents                                                          |
|---------------|-------------------------------------------------------------------|
| `part`        | Part Studio features, analysis tools, part and sketch objects     |
| `sheet_metal` | Sheet metal features and panels                                   |
| `assembly`    | Mates, relations, assembly tools and document types               |
| `sketch`      | Sketch tools                                                      |
| `glyphs`      | Constraint glyphs and viewport markers                            |
| `ui_a`        | Navigation, actions and status                                    |
| `ui_b`        | Documents, folders, panels, people and objects                    |
| `eda`         | Schematic and PCB layout tools                                    |
| `cam`         | Machining: machines, cutters, setups, toolpaths and their commands |
| `platform`    | Operating systems, devices and code hosting, for download buttons |

Read the comment at the top of the module before you start: it describes that family's house
style and shared shapes (for example, documents in `ui_b` are 14 x 18 with a folded corner, and
sketch tools mark the points the user clicks with accent dots of one size). The conventions that
apply to every icon are at the top of `crates/cadrs_icons_gen/src/common.rs`.

## 2. Write the drawing function

An icon is a function returning `Vec<String>`, one SVG element per string, drawn back to front on
a 24 x 24 grid. Build the list with the `els!` macro, which also flattens nested lists and skips
`None`.

The helpers in `crates/cadrs_icons_gen/src/common.rs` cover most needs:

- Elements: `poly`, `polyline`, `line`, `seg` (a line between two points), `path`, `circle`,
  `ellipse`, `ellipse_rot`, `rect`.
- Strokes: `dash(width)` and `dash_dot(width)` give dashed and centre-line patterns that look the
  same at any width (they set `stroke-width` too).
- Arrows: `arrow` (a straight arrow with a filled head) and `curved_arrow` (along a point list).
- Curves: `arc_points`, `sample_cubic`, `offset_polyline`.
- Isometric: `projector(origin)` gives a function mapping `(x, y, z)` to the screen; `iso_box`,
  `cylinder` and `iso_circle` draw shaded boxes, upright cylinders and horizontal circles.
- `clip(subject, convex)` clips a polygon to a convex one. Use it instead of `<clipPath>`.

Most element helpers take a `fill` and an `extra` argument. `fill` is a palette colour or `""` for
none; `extra` is raw attributes such as `r#"stroke="none""#` or `r#"stroke-width="1""#`, or `""`.
Coordinates are written with at most two decimals, so diffs stay small.

Modules also have their own small helpers (`acc` to turn an element accent, `plus`, `dot`,
`doc` and so on); look at the icons next to yours and reuse what they use.

### Example: a line icon

A bookmark with a plus, added to `ui_b`. The ribbon is the object, so it stays ink; the plus is
the modifier, so it carries the accent.

```rust
fn bookmark_add() -> Vec<String> {
    // A ribbon bookmark with a plus: the plus is the modifier, so it carries the accent.
    els![path("M5 3.5 H14 V20.5 L9.5 16.5 L5 20.5 Z", "", ""), acc(plus(18.0, 7.0))]
}
```

### Example: a solid icon

A pocket cut into a block, added to `part`. The block is context, in neutral greys; the pocket's
floor is what the tool makes, so it is the accent. The inner walls and floor are clipped to the
opening so they never draw over the block's top face.

```rust
fn pocket() -> Vec<String> {
    // Context: the block. Output: the recess cut into its top face, its floor in accent.
    let o = (12.0, 12.0);
    let p = projector(o);
    let (s, h, d) = (8.0, 6.0, 2.0); // block size and height, pocket depth
    let (a, b) = (1.5, 6.5); // the pocket's extent in x and y
    let z = h - d;
    let rim = [p(a, a, h), p(b, a, h), p(b, b, h), p(a, b, h)];
    els![
        iso_box(o, 0.0, s, 0.0, s, 0.0, h),
        // The two inner walls that face the camera, and the floor, cut to the opening.
        poly(&clip(&[p(a, a, h), p(b, a, h), p(b, a, z), p(a, a, z)], &rim), SHADE, ""),
        poly(&clip(&[p(a, a, h), p(a, b, h), p(a, b, z), p(a, a, z)], &rim), MID, ""),
        poly(&clip(&[p(a, a, z), p(b, a, z), p(b, b, z), p(a, b, z)], &rim), ACCENT, ""),
    ]
}
```

In isometric icons, faces are lit from above right: `TOP` on faces pointing up, `MID` on faces
pointing right (+x), `SHADE` on faces pointing left (+y), `SOFT` on bevels and secondary faces.

### Rules the drawing must follow

- Keep the drawing inside 1.5 to 22.5 so strokes are not clipped. Solid and sketch icons are then
  zoomed to fit their box automatically; glyph and line icons keep the standard padding.
- Use only the palette constants (`INK`, `TOP`, `SOFT`, `MID`, `SHADE`, `ACCENT`), never other
  colours. The dark variant and app recolouring work by swapping exactly these.
- Use the accent for one idea: what the tool acts on or creates, or the modifier in a UI icon
  (the plus, the arrow, the magnifier). Glyph and line icons are tinted by apps, so the accent must
  never carry the meaning on its own. Single marks (chevrons, close, check, plain arrows) stay ink
  only.
- No `id` attributes, so no `<clipPath>`, `<defs>` or gradients: ids clash when icons are inlined
  into one page.
- No detail smaller than 2 units in glyph icons; they are shown at 10-16 px.

## 3. List it in `icons()`

Add a line to the module's `icons()` at the top of the file:

```rust
def("bookmark-add", Line, "Add bookmark", bookmark_add),
```

- The name is the file stem, in kebab-case, unique across the whole set. It becomes the constant
  `BOOKMARK_ADD` and the lookup key for `cadrs_icons::get("bookmark-add")`.
- The title is the human-readable name shown in tooltips and the docs.
- The position in the list is the icon's place in the toolbar order, in `cadrs_icons::ALL`, so put it next to related icons.

## 4. Generate, review and commit

Render just your module into a scratch directory, and look at it at the sizes apps use:

```sh
cargo run -r -p cadrs_icons_gen -- --only ui_b --out /tmp/try
cargo run -r -p cadrs_icons --example sheet -- /tmp/try.png /tmp/try bookmark-add clock
```

The sheet shows each icon at 96, 24 and 16 px, light above dark. Put an existing icon from the same
family next to yours to check weight and size match. Iterate until it reads well at 16 px.

Then regenerate the whole set and run the checks:

```sh
cargo run -r -p cadrs_icons_gen               # rewrites icons/, icons/dark/ and src/generated.rs
cargo test -r -p cadrs_icons
cargo run -r -p cadrs_icons_gen -- --check    # what CI runs: fails if the committed files are stale
```

The generator stops without writing anything if an icon breaks a rule, naming the module and icon:

```text
part.pocket: colour #ff0000 not allowed for kind Solid
```

It also rejects names that are not kebab-case, duplicate names, malformed XML and `id`
attributes.

Commit the drawing function together with the generated files: the new `icons/<name>.svg`, the
new `icons/dark/<name>.svg` and `src/generated.rs`. Never edit those generated files by hand.

## Renaming or removing an icon

Change or delete its line in `icons()` (and its function) and run the generator. A full run
deletes the SVG files of icons that no longer exist. Renaming or removing an icon removes a
public constant; fix its uses in cadrs in the same change.
