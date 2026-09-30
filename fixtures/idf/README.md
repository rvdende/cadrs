# IDF sample fixtures

We wrote these sample IDF boards ourselves for PCB Studio (stage 3H, course item X4). They are
not copies of the course's files, which aren't available. Each folder holds a `.emn`/`.emp`
pair named after the board, in the same shape as a PCB Studio export. All of them are IDF 3.0
except `v2 sample/`. The tests are in `crates/cadrs_idf/tests/fixtures.rs`.

## (a) `cell phone/` — Cell phone (PCB6)

- Units are MM, and the board is `MCAD`-owned.
- **Thickness is 0.062 mm.** This reproduces a course quirk: the slide says 0.062 mm, where a
  real board would be 0.062 in. Onshape's own export shows `0.0620000000000028`.
- Outline: the 81 × 146 mm rectangle centred on the origin, with R8 90° corner arcs, exactly as
  in the PCB6 screenshot: (32.5,−73,0) (40.5,−65,90) (40.5,65,0) (32.5,73,90) (−32.5,73,0)
  (−40.5,65,90) (−40.5,−65,0) (−32.5,−73,90) (32.5,−73,0).
- Hand-derived outline values:
  - The bbox is 81 × 146.
  - The first arc runs from (32.5,−73) to (40.5,−65) with a 90° angle. Its centre is
    (32.5,−65) and its radius is 8.
  - Area = 81·146 − (4−π)·8² = 11570 + 64π ≈ 11771.0619 mm², running CCW.
- Two `PLACE_KEEPOUT MCAD`, both `BOTTOM` with height 1. The height is the 1 mm downward
  extrude from PCB6 step 6.
  - Battery: the rectangle (−30,−55)–(30,35), 60 × 90 = 5400 mm².
  - Antenna: 5 edges, (−30,50) (30,50) (30,62) (0,68) (−30,62). Area = 60·12 + 60·6/2 =
    900 mm².
- `DRILLED_HOLES` and `PLACEMENT` are empty, and the library is empty (header only).

## (b) `secondary board/` — secondary board (PCB10)

- Units are MM. Thickness is 0.84.
- Outline from the course: (−5.20972, −22.74338) to (45.59028, 15.35662). That is
  50.8 × 38.1 mm (2 × 1.5 in). There are no holes.
- There are 20 placements, `X0`…`X19`, all `PLACED`. The last two (X18 SOT23, X19 1206C) are on
  the `BOTTOM`.
  - X0 and X1 match the course's visible records:
    - X0: `BUTTON_EVQPUA02 5209001 X0` / `24.47 -9.48 0 270 TOP PLACED`.
    - X1: `CRYSTAL_CX_4V 4510219 X1` / `32.37 3.1 -5.55e-15 270 TOP PLACED`. The mount offset
      is the near-zero noise the course shows as "~0".
  - **X2 `uBGA48_7.4X7.1` (7401048)** is at X = 4.064182376174947 (the course's value), with
    **Y = −16.5**, rotation 90, TOP.
    - Rotated 90°, its 7.4 × 7.1 outline covers [0.514, 7.614] × [−20.2, −12.8].
    - The PCB10 keep-out is a 0.5 × 0.375 in (12.7 × 9.525 mm) corner profile at the
      bottom-left corner, with an R0.25 in (6.35 mm) fillet on its inner corner. It covers
      [−5.20972, 7.49028] × [−22.74338, −13.21838]. The fillet centre is (1.14028, −19.56838).
    - The two overlap. For example, the point (1.0, −19.0) lies in both, and X2 is the only
      top-side part that touches the keep-out.
    - Keep-out area = 12.7·9.525 − (1−π/4)·6.35² ≈ 112.314 mm².
    - **After moving +25.4 mm in Y**, X2 is at Y = 8.9 and covers [5.2, 12.6] in Y. That stays
      on the board (top edge 15.35662), and it clears the keep-out and every other part.
  - No two top-side footprints overlap, and every footprint stays on the board.
- Library: every package has a rectangular outline centred on its origin.

  | package | part | W × H mm | height | PROP |
  |---|---|---|---|---|
  | BUTTON_EVQPUA02 | 5209001 | 4.7 × 3.5 | 1.65 | |
  | CRYSTAL_CX_4V | 4510219 | 2.5 × 2.0 | 0.65 | FREQUENCY 32768 |
  | uBGA48_7.4X7.1 | 7401048 | 7.4 × 7.1 | 1.2 | THETA_JC 18 |
  | 1210_SR73K2E | 3302210 | 3.2 × 2.5 | 0.6 | RESISTANCE 10000, TOLERANCE 1, POWER_MAX 500 |
  | TSSOP_20 | 2140020 | 6.5 × 6.4 | 1.2 | |
  | 1206C | 1106001 | 3.2 × 1.6 | 1.1 | CAPACITANCE 0.1, TOLERANCE 10 |
  | SOT23 | 2302300 | 2.9 × 2.4 | 1.1 | POWER_MAX 350 |

## (c) `vision controller/` and `vision controller thou/` — Vision PCB (PCB8)

These are one board designed on a 1 thou grid. The first copy is written in MM and the second
in THOU (1 thou = 0.0254 mm). After converting units, the two files give the same geometry to
within 1e-9 mm.

- The board is 4 × 3 in: (0,0)–(101.6, 76.2) mm, or 4000 × 3000 thou. It is `ECAD`-owned, with
  thickness 62 thou (1.5748 mm).
- There are 4 corner mounting holes: `NPTH BOARD MTG MCAD`, Ø 125 thou (3.175 mm), at
  (150,150), (3850,150), (3850,2850) and (150,2850) thou.
  - Each hole has a round `ROUTE_KEEPOUT ALL` (a 360° circle with r = 3.175 mm), so its area
    is π·3.175².
- One NOTES record: "Vision controller rev A".
- There are 29 placements:
  - **J1 and J2 `HDR_1X20`** are the two long header strips. Each is 2000 × 100 thou
    (50.8 × 2.54 mm), 340 thou tall, and `MCAD`-fixed. They sit along the top and bottom edges.
  - **U1 `QFP100_600MIL` (VPU-7100) is component "A"**, the large central square IC at the
    board centre (2000, 1500) thou.
    - Its footprint is 600 × 600 thou = **15.24 × 15.24 mm**, so its **top-face area is
      232.2576 mm²** (0.36 in²).
    - It is 63 thou (1.6002 mm) tall.
  - **U2 `QFN64_400MIL`** is the second large square IC, 10.16 mm square (103.2256 mm²).
  - Passives: R1–R10 (0603R), C1–C10 (0603C, with C10 on the bottom), D1–D3 (SOD123F, with D3 on
    the bottom), Y1 (CRYSTAL_HC49) and Q1 (SOT23).

## (d) `v2 sample/` — IDF 2.0

This sample is a 2000 × 1500 thou board in 2.0 syntax. It exercises:

- lowercase keywords (`.header`, `board_file`, `thou`, `norefdes`);
- comments and blank lines;
- sections with no owner fields;
- a circular cut-out written as two −180° arcs (r = 100 thou, clockwise, label 1);
- OTHER_OUTLINE with no side;
- ROUTE_OUTLINE and PLACE_OUTLINE with no extra record;
- ROUTE_KEEPOUT ALL;
- VIA_KEEPOUT;
- PLACE_KEEPOUT `BOTTOM 250 0` (max/min heights);
- PLACE_REGION `TOP memory`;
- 5-field drilled holes;
- 5-field placements, including `FIXED`, `UNPLACED` and one with a blank status (which means
  placed).

The library mixes THOU, MM and 2.0's TNM units. For example, `lcc20` is 457200 TNM tall, which
is 4.572 mm. It also has a MECHANICAL `extractor` with arcs, taken from the spec's example.
