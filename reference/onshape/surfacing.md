# Splines, Thicken, Helix, Fill (and Sweep/Loft notes)

Notes for the Onshape features the importer met in the scraped documents
(`~/work/cadrs_onshape/raw`) and cadrs lacked. Sources: Onshape Help (read 2026-09-29), the
scraped `features.json` / `sketches.json`, and the importer's report. Summarised in our own
words; short labels are Onshape's.

- Spline: https://cad.onshape.com/help/Content/sketch-tools-spline.htm
- Thicken: https://cad.onshape.com/help/Content/thicken.htm
- Helix: https://cad.onshape.com/help/Content/helix.htm
- Fill: https://cad.onshape.com/help/Content/fill.htm
- Sweep: https://cad.onshape.com/help/Content/sweep.htm
- Loft: https://cad.onshape.com/help/Content/loft.htm

## Sketch spline (SP)

- SP1 Tool: click to start, click each point, **double-click** to end (or the context menu's
  "Confirm spline"). A click back on the first point **closes** the spline (periodic).
- SP2 Points can be dragged afterwards; the spline follows. (A separate Spline point tool
  adds points; not in cadrs.)
- SP3 **Handles**: tangent handles at the ends (white points), dragged along or away from the
  spline to set the end tangent; they cannot be deleted. Handles and spline points take
  dimensions and constraints (cadrs: the points yes, the handles by dragging only).
- SP4 Curvature comb from the context menu (not in cadrs).
- SP5 Data (`BTCurveGeometryInterpolatedSpline`, `skInterpolatedSpline`,
  `skInterpolatedSplineSegment`): `interpolationPoints` (m), `isPeriodic`,
  `startDerivative`/`endDerivative` (m per unit of the spline's parameter; zero = free end),
  `startHandle`/`endHandle` positions. Checked on innerdoor_honda92: the parameter is
  **centripetal** (span lengths ∝ √chord, normalised to 1) and the handle is the Bézier point
  next to the end: `start + derivative · h₀ / 3` (matches to 1e-9;
  `cadrs_sketch::spline::tests::onshape_handles_match`). A periodic spline lists each point
  once. Handle lines appear in `sketches.json` as construction `skLineSegment`s named
  `<id>.startHandle` / `<id>.endHandle`.
- SP6 Control-point splines (`skSpline`, `BTCurveGeometrySpline`: `degree`, `controlPoints`,
  `knots`, `isPeriodic`) appear as construction (projected) curves; cadrs imports them as an
  interpolated spline through 24 points on them.

## Thicken (TK)

- TK1 Result: New / Add / Remove / Intersect.
- TK2 Selections: a sketch, part faces or surfaces.
- TK3 Thickness: Mid plane (one symmetric thickness) or Thickness 1 (along the normal) and
  Thickness 2 (the other side), with the opposite direction arrow. The three values are kept
  separately when switching.
- TK4 Keep tools (else the thickened surfaces are consumed); Merge scope / Merge with all.
- TK5 Add/Remove/Intersect with an empty merge scope is an error in Onshape (cadrs falls
  back to the parts the new body touches, as its Extrude does).
- TK6 Data (`featureType: thicken`): `operationType`, `entities` (faces), `midplane`,
  `thickness1`, `thickness2`, `thickness`, `oppositeDirection`, `keepTools`,
  `booleanScope`. Seen in centurion_remote (7 faces, 1.4 mm) and pibox/piboxv7 (0.8 mm,
  opposite direction).

## Helix (HX)

- HX1 Types: Cylinder/Cone (a face), Axis, Circle.
- HX2 Input type: Turns, Pitch, or Turns and pitch.
- HX3 Start: a start angle (from a reference) or a start point; end: the face's height or an
  end point.
- HX4 Direction: Clockwise / Counterclockwise; opposite direction arrow flips the start end.
- HX5 On a face the helix starts "at the start of the revolve or the x axis of an extruded
  circle".
- HX6 It makes a **curve** (listed under Curves); the face's part is not consumed. Show
  start and end profiles is a display option.
- HX7 Data (`featureType: helix`): `axisType` (SURFACE, …), `entities`, `pathType`
  (TURNS, PITCH, TURNS_PITCH), `revolutions`, `helicalPitch`, `height`, `startAngle`,
  `startRadius`, `endRadius`, `handedness` (CW/CCW), `oppositeDirection`. Seen in
  squish_bottle_adapter (20 turns × 2 mm pitch; 2 turns × 4.125 mm), each the path of a Sweep.

## Fill (FL)

- FL1 Boundary: edges, surfaces' edges, sketch or 3D curves forming a closed chain (red dots
  mark open ends).
- FL2 Continuity per boundary: Position (G0), Tangency (G1), Curvature (G2), with the
  adjacent faces for G1/G2.
- FL3 Guides (vertices, points, curves) with Sampled or Precise constraint mode and a sample
  size; Show isocurves with a count.
- FL4 Surface option: New, or Add (merged with the surfaces in the merge scope). A fill that
  closes a set of surfaces makes a **solid** (unless New).
- FL5 Data (`featureType: fill`): `edges` (an array of items: `continuity`,
  `adjacentFaces`, `entities`), `surfaceOperationType`, `addGuides`, `guideEntities`,
  `constraintMode`, `sampleSize`. Seen in centurion_remote (capping a swept surface) and
  pibox/piboxv7 (extrude cap edges): flat boundaries.

## Sweep and Loft notes (SW, LO)

- SW1 Fields: creation type (Solid / Surface / Thin), result, profiles (faces, sketch
  regions, edges), path, profile control (None, Keep profile orientation, Lock profile faces,
  Lock profile direction), twist (turns / angle / pitch), scale, trim ends (thin), merge scope.
- SW2 Surface sweeps take open or closed sketch curves (edges) as profiles.
- LO1 Profiles: sketch regions or curves, faces, surfaces, points; at least two, in order;
  guides, a path (with a section count), end conditions, match connections. Surface lofts
  take curve (wire) profiles.

## What cadrs does (2026-09-29)

| ID | Status |
|---|---|
| SP1–SP3, SP5 | Spline tool (points, close on the first point, double-click / Enter / Esc to end), end handles on selected (or tangent-set) open splines, dragging one sets the tangent (one undo step); splines in regions, extrude, revolve, thin walls, sweep paths; importer maps SP5. |
| SP4 | Not done (no curvature comb, no Spline point tool; dimensions and constraints to the spline curve itself are not offered, only to its points). |
| SP6 | Approximated: an interpolated spline through 24 points on the B-spline (they are construction curves in the data). |
| TK1–TK4, TK6 | Thicken feature, dialog, importer. Each face is thickened on its own and the slabs fused (faces meeting at a convex corner leave a notch the size of the thickness on the outside). "Along the normal" is OCCT's face orientation, which for surfaces can differ from Onshape's (pibox: the direction differs on the fill). |
| HX1–HX4, HX6, HX7 | Helix feature (Cylinder/Cone, Axis, Circle; Turns, Pitch, Turns and pitch; start angle; clockwise/counterclockwise; opposite direction), shown as a curve in the view (not in the Parts list), a sweep path; importer. On a face it starts at the end on the sketch plane of the feature that made the face, angle 0 along that sketch's x axis. Start/end points and the end radius are not imported. |
| FL1, FL4, FL5 | Fill of a flat boundary (a planar face) and of a non-flat boundary of three or four curves (a Coons patch as a B-spline surface), New / Add (sewn with the surfaces it meets into a solid when they close, else a surface of its own); importer. |
| FL2, FL3 | Continuity stored, built as Position (G0); guides not done. |
| SW1, SW2 | Sweep importer (solid, surface and thin; regions, faces, sketches; sketch curve, edge and helix paths; profile control; twist and scale noted as not imported). A sweep along a helix with a boolean grows its profile by 1e-4 (a thread touching its cylinder, which OCCT would not join). Paths with corners (non-tangent joints) sweep wrongly in cadrs (centurion_remote). |
| LO1 | Wire (curve) profiles still not imported (a surface loft between two edges in 9c8239a6…). |
