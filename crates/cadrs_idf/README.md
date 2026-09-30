# cadrs_idf

IDF 2.0 / 3.0 (Intermediate Data Format) parser, writer and zip export for PCB Studio
(stage 3H, milestone P3H.1). This crate doesn't depend on Bevy.

- `parse_emn` / `parse_emp` read board (`.emn`) and library (`.emp`) files. They return the
  value plus warnings, or an `IdfError { line, message }`.
- `write_emn` / `write_emp` write either version. The writer writes `Board::for_version(v)`, so
  `parse(write(x, v)) == x.for_version(v)` holds with exact `f64` equality (tested with
  proptest).
- `write_zip(board, library, version)` is PCB Studio's "Export this board to IDF". It produces
  `<board>/<board>.emn` and `<board>/<board>.emp`. `read_zip` and `read_pair` read them back.
- `geom::Loop` holds the geometry helpers: segments with arc centres, `is_closed`, `area`, and
  an exact `bbox` that includes arc extremes. `Placement::place_loops` puts a package outline
  onto the board, in mm.

Sample boards live in `fixtures/idf/`. See the README there.

## Spec notes

These notes were checked against the public, non-proprietary specs: *Intermediate Data Format,
Mechanical Data Exchange Specification*, Version 2.0 Rev. 3 (1993-01-05) and Version 3.0 Rev. 1
(1996-10-31).

- **Common to both versions**:
  - Section keywords are case-insensitive (`.X` … `.END_X`).
  - There is one record per line, and fields are separated by blanks. Strings that contain
    blanks are quoted with `"`. A `#` in column 1 starts a comment line.
  - Loop records are `label x y angle`. An angle of 0 is a line. Any other value is an arc with
    that included angle, positive counter-clockwise. Label 0 is the outline (CCW), and labels
    1..n are cut-outs (CW).
  - Header record 2 is `BOARD_FILE|LIBRARY_FILE <ver> "<source>" <date> <n>`. Board files add
    a third record, `<name> <units>`.
  - The date format is `yyyy/mm/dd.hh:mm:ss`. The examples in both specs use
    `mm/dd/yy.hh:mm:ss`, so we store the date as text.
- **IDF 2.0 does have keep areas.** Its board file has these sections: HEADER, BOARD_OUTLINE,
  OTHER_OUTLINE, ROUTE_OUTLINE, PLACE_OUTLINE, ROUTE_KEEPOUT, VIA_KEEPOUT, PLACE_KEEPOUT,
  PLACE_REGION, DRILLED_HOLES and PLACEMENT. The course says "IDF 2.0 can't carry
  keep-in/keep-out areas" (PCB9.7), but that describes Onshape's exporter, not the format.
- **What 2.0 lacks compared with 3.0**:
  - owner fields (MCAD/ECAD/UNOWNED);
  - panel files and PANEL_OUTLINE;
  - NOTES;
  - 360° circles (2.0 uses two 180° arcs);
  - the OTHER_OUTLINE side;
  - the ROUTE_OUTLINE layer record (2.0 applies it to all layers);
  - the PLACE_OUTLINE side/height record (2.0 applies it to both sides);
  - the INNER layer;
  - the hole type and hole owner fields (a 2.0 hole is `dia x y PTH|NPTH assoc`);
  - the placement mount offset (a 2.0 placement is `x y rot side status`);
  - the MCAD and ECAD statuses (2.0 has PLACED, UNPLACED and FIXED, and a blank status means
    placed);
  - library PROP records.

  Also, a 2.0 PLACE_KEEPOUT has *max and min* heights, where 3.0 has a single height.
- **Units**:
  - 2.0 allows `MM`, `THOU` and `TNM` (10 nm).
  - 3.0 allows only `MM` and `THOU`.
  - Neither version has `INCH`.
  - Library units are set per package.
- **Placement**:
  - A component's origin goes to (x, y).
  - A BOTTOM component is flipped about its local Y axis.
  - The rotation is CCW in the component's own frame. For bottom parts that means clockwise as
    seen from the top (spec Figure 1).
- **Our choices**:
  - `write_emn(b, V2)` follows the 2.0 spec, so it keeps keepouts, written in 2.0 syntax.
  - `write_zip` / `export_board` implement PCB Studio's export. They always write our own source
    string (`"cadrs PCB Studio v0.1"`, never Onshape's). At IDF 2.0 they also drop every keep
    area: route/place outlines, route/via/place keepouts and place regions. This matches the
    course and the export dialog's "IDF 3.0 supports keep-outs".
  - Converting 2.0 to 3.0 maps FIXED to MCAD and TNM to MM, and makes the 2.0 defaults
    explicit.
  - Numbers are written in Rust's shortest round-trip form (`0`, `270`, `0.0620000000000028`,
    `-5.55e-15`).
