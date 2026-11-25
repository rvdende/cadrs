# Part Studio features

A Part Studio is built from the features in its **feature list**, which run from top to bottom:
each one works on the parts the features above it made. Choosing a feature in the toolbar opens
its dialog, and the dialog stays open while you choose what the feature uses and how.

Most of a dialog is **selection fields** and settings. The highlighted field takes what you click
in the view or in the feature list; click another field to fill that one instead. Number fields
take a value with its unit, an expression such as `20 + 5`, or a variable. A value without a
unit is in the document's units (mm by default).

The foot of each dialog has:

- a **slider**: drag it left to see the Part Studio before this feature, right to see it after.
- **Final**, which shows the final result, with the features after this one rebuilt too (useful
  when you edit a feature high up the list).
- **?**, which opens this page.

Click the green **✓** (or press <kbd>Enter</kbd>) to accept the feature, or the **×** (or
<kbd>Esc</kbd>) to cancel. While something is missing or the feature fails, its name in the
dialog's title is dark red, ✓ is disabled, and the dialog says what is wrong.

### New, Add, Remove and Intersect

Features that make solids (Extrude, Revolve, Sweep, Loft, and part patterns and mirrors) have a
row of tabs for what to do with the new body:

- **New** makes new parts.
- **Add** joins the body to the parts it touches.
- **Remove** cuts the body from the parts it overlaps.
- **Intersect** keeps only where the body and the parts overlap.

With Add, Remove and Intersect, **Merge with all** makes the feature act on every part. Turn it
off to choose the parts yourself in **Merge scope**.

### Solid, Surface and Thin

Extrude, Revolve, Sweep and Loft also have tabs for the kind of body: **Solid**; **Surface**, a
sheet with no volume (it takes sketch curves rather than regions, and has no New/Add/Remove
tabs); or **Thin**, a wall around the profile. A thin body has **Thickness 1**, with
**Flip wall** to put the wall on the other side, and **Thickness 2** for the other side. **Mid
plane** centres the wall on the profile instead.

## Extrude

Extrude sweeps sketch regions or flat faces straight out of their plane.

- **Faces and sketch regions to extrude** (for a surface, **Sketch curves to extrude**): pick
  regions in the view, a whole sketch in the feature list, or a flat face of a part.
- The end condition: **Blind** (a **Depth**), **Up to next** (the next part face in the way),
  **Up to face**, **Up to part**, **Up to vertex**, or **Through all** (through every part). The
  arrow beside it flips the direction. An "Up to" end has an **Offset distance** option.
- **Direction** extrudes along a picked edge, line or normal instead of the sketch plane's normal.
- **Starting offset** starts the extrude away from the sketch plane.
- **Symmetric** (Blind and Through all only) extrudes both ways; the depth is the total.
- **Draft** (solids only) tapers the side faces by an angle.
- **Second end position** extrudes the other way too, with an end condition of its own.

## Revolve

Revolve turns sketch regions or flat faces about an axis.

- **Faces and sketch regions to revolve** (for a surface, **Sketch curves to revolve**).
- **Revolve axis**: a sketch line, a straight edge or an axis. The mate connector button beside
  it takes a mate connector's Z axis instead.
- The type: **Full** (360°), **Blind** (a **Revolve angle**, with an arrow to flip it),
  **Symmetric** (the angle split evenly both ways), or **Up to next**, **Up to face**, **Up to
  part** or **Up to vertex**, which have an **Offset angle** option.
- **Second end position** (one-sided types only) revolves the other way too.

## Sweep

Sweep moves a profile along a path.

- **Faces and sketch regions to sweep** (for a surface, **Edges and sketch curves to sweep**).
- **Sweep path**: connected edges or sketch curves, a whole sketch, or a curve such as a helix.
- How the profile follows the path:
  - **None**: the profile keeps its angle to the path.
  - **Keep profile orientation**: the profile keeps its orientation in space.
  - **Lock profile direction**: the profile's plane keeps containing a picked **Direction**.

## Loft

Loft makes a body that passes through a series of profiles.

- **Profiles**: sketch regions, whole sketches or flat faces, in the order the loft goes through
  them. Drag them to reorder. The first or last profile can be a point (a sketch point or a
  vertex), to close the loft to a tip. For a surface, a profile can be a chain of curves.
- **End conditions** shape the loft at its first and last profile (**Start profile condition**,
  **End profile condition**): **None**, **Normal to profile**, **Tangent to profile**, **Match
  tangent** and **Match curvature** (these two continue the faces next to a face profile), or
  **Normal direction** and **Tangent direction**, which take a picked direction. Each condition
  but None has a **Start magnitude** or **End magnitude**: how strongly the loft keeps that
  direction.

## Fillet

Fillet rounds edges. It has two tabs.

**Edge** rounds the **Entities to fillet**: edges, or faces (all of a face's edges).
**Tangent propagation** adds the edges that continue them smoothly.

- **Measurement**: **Radius**, or **Width** (the distance across the fillet between the lines
  where it meets the faces).
- **Control**, the cross section: **Distance** (circular), **Conic** (with a **Rho** between 0
  and 1), or **Curvature** (curvature continuous, with a **Magnitude**). Conic and Curvature are
  for straight edges between flat faces.
- **Asymmetric**: a **Second radius** on the other face.
- **Partial fillet**: rounds only part of an edge, between a **First bound** and a **Second
  bound**, given by **Boundary type** as a **Parameter** (0 to 1 along the edge) or a
  **Length**.
- **Variable fillet**: a radius for each of the edges' **Vertices**, and **Points on edges**
  (**Add point on edge**, each with its **Location** from 0 to 1 and its radius). **Smooth
  transition** blends the radius smoothly between them.
- **Allow edge overflow** (on by default): turn it off to refuse a fillet that runs onto other
  faces.
- **Smooth fillet corners**: where three fillets meet, sets the corner back and blends it with
  one smooth patch.

**Full round** replaces a face with a round that meets both its neighbours: pick **Side face 1**,
the **Center face** to replace, and **Side face 2**.

## Chamfer

Chamfer bevels edges.

- **Entities to chamfer**: the edges, with **Tangent propagation** as for a fillet.
- **Measurement**: **Offset** (measured across the faces) or **Tangent** (along them).
- **Chamfer type**: **Equal distance** (one **Distance**), **Two distances** (**Distance 1**
  and **Distance 2**), or **Distance and angle**.
- With two distances or an angle, the arrow beside the first value swaps which face takes it,
  for every edge. **Direction overrides** lists edges to swap on their own.

## Shell

Shell hollows a part out, leaving walls of a given thickness.

- **Faces to remove**: the faces that become openings.
- **Hollow**: instead of removing faces, pick **Parts to hollow**; each becomes a closed hollow
  part.
- **Shell thickness**. The arrow beside it grows the walls outward instead of inward.
- **Rounded**: rounds the walls' corners (as a ball rolled along them would). Off, the walls'
  faces are extended until they meet, with sharp corners.

## Draft

Draft tilts faces by an angle, for parts that are moulded or cast.

- **Neutral plane**: a default plane, a Plane feature, a flat face or a mate connector. The
  faces turn about where they meet it, and its normal is the pull direction.
- **Faces to draft**.
- **Draft angle**, between 0 and 90°. The arrow beside it reverses the pull direction.
- **Tangent propagation** (on by default) drafts the faces tangent to the picked ones too.

The **Parting line** tab and **Reference entity propagation** are shown but not available yet:
use **Neutral plane**.

## Hole

Hole drills standard holes at sketch points. The feature's name follows the hole's callout.

### Where the holes go

- **Sketch points to place holes**: sketch points, or a whole sketch (a hole at each of its
  points). The mate connector button beside it takes mate connectors instead; a hole is drilled
  along each one's −Z axis.
- **Merge scope**: the parts to cut. Left empty, a hole cuts every part it reaches.

### Its size

- The **Inch** and **Metric** tabs (also the **Standard** list, **ANSI** or **ISO**) choose the
  size tables. Inch sizes are in inches.
- The style tabs: **Simple**, **Counterbore** or **Countersink**.
- **Hole type**:
  - **Drilled**: a plain hole of a drill **Size**.
  - **Clearance**: a hole a fastener of **Size** passes through, with a **Fastener fit** of
    **Close**, **Normal** or **Loose**.
  - **Tapped**: a threaded hole of **Size** and **Pitch** (**Tap type** is **Straight tap**).
    **Thread class** adds the thread's **Class** to the callout.
  - **PEM®** (Metric only): the mounting hole of a self-clinching **Fastener**, **Self-clinching
    nut (CLS)** or **Self-clinching stud (FH)**.
- **Hole Ø** (for a tapped hole, **Tap drill Ø**) follows the size, and can be changed.

### Where it starts and ends

- **Start plane**: **Start from part** (where the part begins), **Start from sketch plane**, or
  **Start from selected plane**, which takes a **Hole start plane** (a plane, a flat face or a
  mate connector).
- **Termination**: **Through** (every part in the merge scope), **Blind** (a **Hole depth** of
  the full diameter; the drill point goes past it), **Up to next** (until the hole leaves the
  part it entered), or **Up to entity** (until the full diameter reaches a picked face or
  plane). The arrow beside it drills the other way. Up to next and Up to entity have an
  **Offset**.
- **Tip angle** (not for Through): the drill point's angle, 118° by default.
- Counterbores have a **Counterbore Ø** and **Counterbore depth**; countersinks a **Countersink
  Ø** and **Countersink angle**.
- A tapped hole that isn't Through has a **Tapped depth**; a blind one also shows the **Tap
  clearance**, the plain hole below the thread in threads. Changing it changes the tapped depth.

### Tolerances

**Diameter tolerance**, **Depth tolerance** and the counterbore's and countersink's tolerances go
into the callout. Each is **None**, **Symmetrical** (a **Tolerance**), **Deviation** (**Upper**
and **Lower**), **Limits**, **Min**, **Max** or **Basic**, with a **Precision**.

## Thread

Thread cuts a modelled screw thread into a cylinder (an external thread on a shaft) or into a
hole's wall (an internal thread).

- **Cylinder or hole edge**: the circular edge at the end the thread starts from. The size is
  fitted to the cylinder you pick.
- **Standard** (**ISO** or **ANSI**), **Size** and **Pitch**. **Custom**, at the end of the
  sizes, takes a **Major diameter** and a **Pitch** you type.
- **Blind** (a **Length** from the edge) or **Up to end** (the whole cylinder, running out past
  its far end).
- **Direction**: **Right-hand** or **Left-hand**.

## Boolean

Boolean combines parts that already exist. Choose the operation with the tabs:

- **Union** joins the **Tools** into one part.
- **Subtract** cuts the **Tools** from the **Targets**. **Offset** first grows the tools by a
  **Distance** (the arrow shrinks them instead): **Offset all** their faces, or only the **Faces
  to offset**.
- **Intersect** keeps where the **Tools** overlap. With **Targets**, it keeps where each target
  overlaps the tools, and **Keep remainder parts** keeps the rest of each target too.

The tools are used up unless **Keep tools** is on.

## Split

Split cuts parts or faces with a plane, a face or a sketch. Choose with the tabs:

- **Part**: **Parts or surfaces to split** are cut into pieces, each a part of its own.
- **Face**: **Faces to split** are cut where the tool crosses them; the part stays one part.

**Entity to split with**: a plane, a part face (a flat face splits along its whole plane), or a
sketch (its curves extruded both ways through the parts). The Face type can also split along a
curve lying on the faces.

For parts: **Keep tools** keeps a surface used as the tool; **Trim to face boundaries** (with a
face as the tool) splits only within the face's edges; turn off **Keep both sides** to keep only
the pieces in front of the tool, or with the arrow, behind it. For faces, **Keep tool surfaces
and curves** keeps the tool.

## Transform

Transform moves, turns, copies or scales **Parts to transform**. Its type:

- **Translate by line**: along a **Line or axis** by its length (the arrow reverses it).
- **Translate by distance**: along a **Direction** by a **Distance**.
- **Translate by XYZ**: by **X**, **Y** and **Z** distances.
- **Transform by mate connectors**: so that the **From mate connector** lands on the **To mate
  connector**, with **Flip primary axis** and **Reorient secondary axis** to turn it.
- **Rotate**: about an **Axis of rotation** by an **Angle**.
- **Copy in place**: a copy, where the parts are.
- **Scale uniformly**: by a **Scale** factor, about a **Point to scale about** (the origin if
  none).

**Copy part** moves a copy and leaves the original where it was.

## Linear pattern

Linear pattern copies along a direction.

- The type: **Part pattern**, **Feature pattern** or **Face pattern**, with **Parts to
  pattern**, **Features to pattern** or **Faces to pattern**. A part pattern has the New, Add,
  Remove and Intersect tabs. The **+** beside the faces opens **Create selection**.
- **Direction**: an edge, a sketch line, a face's or plane's normal, or a mate connector's Z axis.
- **Distance** between instances, and **Instance count**, which counts the original. The arrow
  reverses the direction.
- **Centered** spreads the instances about the original.
- **Second direction** adds another direction, with its own distance, count and **Centered**,
  for a grid.
- **Reapply features** (feature pattern): each copy is rebuilt where it lands instead of copying
  the material, so it fits the part there.
- **Skip instances**: click instances in the view to leave them out.

## Circular pattern

Circular pattern copies around an axis. It has the same types, Reapply features and Skip
instances as the [linear pattern](#linear-pattern).

- **Axis of pattern**: a circular edge, a cylindrical face, a sketch circle or line, or (with the
  button beside it) a mate connector's Z axis.
- **Angle** and **Instance count**; the arrow turns the other way.
- **Equal spacing** (on by default) spreads the instances evenly over the angle: a full turn by
  the count, otherwise with the first and last at the ends. Off, the angle is the step between
  instances.
- **Centered** spreads them about the original.

## Curve pattern

Curve pattern copies along a path. It has the same types, Reapply features and Skip instances as
the [linear pattern](#linear-pattern).

- **Path to pattern along**: edges or sketch curves.
- **Instance count**; the arrow runs the other way.
- **Equal spacing** (on by default) spreads the instances evenly along the path. Off, it takes a
  **Distance** between them.
- **Tangent to curve** (on by default) turns each copy to follow the path.

## Mirror

Mirror copies parts, features or faces to the other side of a plane.

- The type: **Part mirror**, **Feature mirror** or **Face mirror**, with **Parts to mirror**,
  **Features to mirror** or **Faces to mirror**. A part mirror has the New, Add, Remove and
  Intersect tabs: Add joins each part and its image into one symmetric part.
- **Mirror plane**: a default plane, a Plane feature or a flat face, or with the button beside
  it, a mate connector's XY plane.
- **Reapply features** (feature mirror): the features are rebuilt on the other side instead of
  their material copied.

## Plane

Plane makes a reference plane, to sketch on or to use in other features. Pick its **Entities**,
then the type:

- **Offset**: a plane or flat face, moved by an **Offset distance**.
- **Plane point**: parallel to a plane, through a point.
- **Line angle**: through a line, turned by an **Angle** about it.
- **Point normal**: through a point, square to a line.
- **Three point**: through three points.
- **Mid plane**: halfway between two planes or two points.
- **Curve point**: square to a curve, at a point on it.
- **Fit**: the best fit through three or more points.
- **Tangent**: tangent to a cylindrical or conical face, through a point or at an **Angle**
  round its axis.

**Flip alignment** (Line angle, Tangent and Mid plane) chooses the other of two possible
planes. **Flip normal** turns the plane over.

## Mate connector

A mate connector is a point with axes, which assemblies use to place parts, and which features
such as Revolve, Hole and Transform can use as a point, an axis or a plane.

- **Origin type**: **On entity** puts it on the **Origin entity**, at the point you click (such
  as a face's centre or a corner). **Between entities** puts it midway between the origin entity
  and the **Between entity**.
- **Alignment**: an entity whose direction its primary (Z) axis takes.
- **Realign** sets its **Primary axis** (Z) and **Secondary axis** (X) from picked entities.
- **Move** offsets it by an **X translation**, **Y translation** and **Z translation** along its
  own axes, and a **Rotation** about Z.
- **Owner entity**: the part it belongs to and goes with into assemblies. Otherwise the origin
  entity's part owns it.
- **Flip primary axis** reverses Z; **Reorient secondary axis** turns X by 90° about Z.
