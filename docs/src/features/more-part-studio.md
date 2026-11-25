# Surfaces, direct edits and more

This page covers the Part Studio features that don't fit under sketching, extruding or
filleting: surface tools, edits made straight to a part's faces, generated parts such as gears
and frames, and the features that group parts or bring them in from elsewhere. Each feature's
dialog has a **?** that opens its section here. As with every feature, ✓ or <kbd>Enter</kbd>
keeps your changes and ✕ or <kbd>Esc</kbd> discards them.

## Thicken

Thicken turns surfaces into solids by giving them a thickness. It's on the toolbar's **Thicken**
button, which also holds **Fill** and **Helix**.

- The tabs at the top choose what happens to the result: **New** makes new parts, **Add**,
  **Remove** and **Intersect** combine it with the parts it touches.
- **Faces and surfaces to thicken** takes faces of parts, whole surface parts, sketch regions,
  or a whole sketch (pick its row in the feature list).
- **Thickness 1** grows the solid along the surface's normal. The arrow beside it flips the
  direction. **Thickness 2** grows it the other way at the same time.
- **Mid plane** replaces the two with one **Thickness**, split evenly on both sides.
- **Keep tools** keeps the surfaces you thickened. Otherwise they're used up.
- With Add, Remove or Intersect, **Merge with all** combines the result with every part it
  touches. Turn it off to pick the parts in **Merge scope**.

## Helix

Helix makes a helical curve. It shows in the Part Studio and can be used as a sweep path or
a frame's path.

- The first list chooses how the helix is placed:
  - **Cylinder/Cone** follows a **Cylindrical or conical face**, taking its radius and height
    from the face. On a cone the radius changes along the way.
  - **Axis** runs around an axis: a sketch line, an edge, a cylindrical face, a mate connector or
    an origin axis. You give it a **Radius**.
  - **Circle** starts from a circle and takes the circle's radius.
- The arrow beside the selection starts the helix at the other end.
- **Input type** decides how long it is:
  - **Turns** asks for the number of **Revolutions** over the height.
  - **Pitch** asks for the **Pitch** (the distance between turns), and the number of turns
    follows from the height.
  - **Turns and pitch** asks for both, and the height follows from them.
- With Axis and Circle, **Height** sets the height, unless the input type is Turns and pitch.
- **Start angle** turns the helix's starting point around the axis.
- **Direction** is **Clockwise** (a right-handed helix) or **Counterclockwise**.

## Fill

Fill makes a surface that spans a closed boundary of edges and sketch curves, for example to
close a hole in a surface model.

- **New** makes a separate surface. **Add** joins the fill to the surfaces it meets, and makes a
  solid if together they enclose a volume.
- **Edges and curves** takes the boundary. Part edges and sketch curves can be mixed, but
  together they must form a closed loop.
- **Continuity** sets how the fill meets its neighbours. Only **Position (G0)** is available for
  now: the fill touches the edges. **Tangency (G1)** and **Curvature (G2)** are listed but can't
  be chosen yet.
- With Add, **Merge scope (surfaces it meets)** limits which surfaces the fill is joined to.
  Leave it empty to join every surface it touches.

## Move face

Move face moves faces of a part along their own normals, and the faces around them follow, so a
wall slides or a hole gets bigger or smaller. It edits the faces themselves, whatever feature
made them, so it also works on imported parts. Move face, **Delete face** and **Simplify** share
one toolbar button. Select faces before you click it and they fill the dialog.

- **Faces to move** takes the faces.
- **Distance** is how far they move outward. A negative distance, or the opposite-direction
  arrow, moves them inward. On a hole, moving its face outward from the material makes the hole
  smaller.

## Delete face

Delete face removes faces from a part and extends the faces around them to close the gap. Use it
to take away a fillet, a chamfer, a hole, a boss or a groove, including on imported parts.

- **Faces to delete** takes the faces. Select all the faces of the detail you want gone, such as
  every face of a pocket. There are no other options.

## Simplify

Simplify cleans up parts whose faces and edges are split more than they need to be, which is
common in imported parts. Neighbouring faces that lie on the same surface are merged into one,
and so are edges on the same curve. Nothing else about the shape changes.

- **Faces of the parts to simplify** takes one face of each part you want simplified. The whole
  part is simplified, not only the face you pick.

## Gear

Gear makes involute gears, either cut into a face of an existing part or as a new part.

- **Cylinder, cone or flat face (optional)**: picking a face cuts the teeth into it, and sets the
  type from the face: a cylinder makes a **Spur gear**, a hole a **Ring gear**, a cone a **Bevel
  gear** and a flat face a **Rack**. The number of teeth is chosen to bring the module nearest
  to the one set, and the field shows the module the gear ends up with.
- **Type** also offers **Hypoid pair**, **Worm drive** and **Gear pair**, which are always made
  as new parts (see below).
- **Teeth**, **Module** (pitch diameter over the number of teeth; only for a gear made whole, or a
  rack) and **Pressure angle**.
- **Tooth line**: **Straight**, **Helical** or **Herringbone** (not for bevel gears), with a
  **Helix angle** and a **Direction** (**Right-hand** or **Left-hand**). A helical bevel gear
  has a **Spiral**: **Face-milled (Gleason)**, cut **Cut as** the **Gear**, **Pinion (localised
  contact)** or **Pinion (conjugate)** of a pair, with a **Cutter radius (0: auto)**; or
  **Equiangular**.
- A gear made whole has a **Face width**, a **Bore diameter** (spur and bevel), a **Rim
  thickness** (ring), a **Pitch angle** (bevel), a **Length** (rack) and a **Body thickness**
  (bevel and rack). **X**, **Y**, **Z** and **Axis** place it. A bevel gear can be placed
  **Position at the apex** instead of by its back face, and turned with **Tilt**.
- **Rotation** turns the teeth about the axis. **Backlash** makes the teeth thinner so that two
  gears run freely. A rack on a face has **Teeth across the face**, to space the teeth along the
  face's short side.

### Hypoid pair, worm drive and gear pair

- **Hypoid pair** makes one member of a spiral bevel pair whose shafts may be offset. Pick its
  **Axis** and its **Mate's axis** to take the shaft angle, offset and hand from them, or type
  the **Shaft angle** and **Offset**. Set its **Teeth** and **Mate's teeth**, **Spiral angle**,
  and **Cut as** **Gear** or **Pinion**. **Pressure shift** and **End relief** fine-tune the
  teeth.
- **Worm drive** makes the **Worm** or the **Worm wheel** (**Cut as**), from the worm's
  **Starts** and the wheel's **Teeth**, the module and the **Diameter quotient** (the worm's
  pitch diameter over its module). **Wheel's axis** places the drive.
- **Gear pair** makes both gears on two shafts from a **Ratio**: pick the **Gear's shaft** and
  the **Pinion's shaft**. Depending on how the shafts lie it makes a hypoid pair, or a worm drive
  when they are square and far enough apart. The dialog shows which. **Auto teeth** chooses the
  tooth counts to suit the ratio. In the view, the label over the gear shows the ratio: scroll
  on it or use its buttons to step the ratio, and drag the knob to change the gear's size.
- On shafts, **Hold gear on its shaft** keeps the gear where it is along its shaft, at the
  **Mount** distance.
- **Fast preview (blank only)** shows a generated gear's blank straight away while you move its
  shafts. The teeth are cut once you turn it off.

## Frame

Frame builds structural members (tubes, bars, beams, T-slot extrusions) along sketch lines or
edges, joined at the corners.

- **Sketch curves or edges** is the path. Picking a sketch's row in the feature list takes all of
  its curves; helices work too.
- **Profile** is the section: **T-slot extrusion**, **Square tube**, **Rectangular tube**,
  **Round tube**, **Square bar**, **Round bar**, **Flat bar**, **Angle**, **Channel (UPN)**,
  **I-beam (IPE)** or **Custom (sketch)**, with a **Size**. Custom takes a **Profile sketch**,
  with the sketch's origin on the path.
- **Alignment** sets which point of the section sits on the path, and **Angle** turns it about
  the path. **Mirror profile** flips it left for right.
- **Merge tangent segments** makes curves that meet smoothly into one member.
- **End offset** lengthens the members' free ends (negative: shortens them).
- **Turned members (curves)** gives chosen members an angle of their own.
- **Corners**: **Miter**, **Butt**, **Coped butt** (the second member cut to the shape of the
  first) or **None**. The button beside it swaps which member runs on.
- **Corner overrides (points)**: pick sketch points or part vertices at corners that should be
  joined differently. Each gets its own row with its corner type, its flip, a **Gusset** box and,
  where three or more members meet, a button that picks which pair is joined.
- **Trim to parts** and **Trim to planes** cut the members back at other parts or at planes and
  flat faces. **Keep the other side** keeps the other pieces instead of the largest.
- **End caps** close the free ends of hollow members, with a **Thickness**. **Set into the end**
  puts the cap inside the member.
- **Gussets** adds triangular plates in the corners, with a **Gusset size**, **Thickness**,
  **Chamfer** and **Position** (**Middle**, **Top** or **Bottom** of the section).

### Frame trim and Cut list

**Frame trim** cuts members to one another where they meet. Its first list, **Members that run
through**, stays whole. Each list after it, **Members cut to those above**, is cut to the lists
above it.

The **Cut list** feature lists frame members as pieces to cut: **All frame members**, or the
parts you pick in **Members and parts to list**. **Add an override** sets a **Column** to a
**Value** for some parts (**All parts**, **Same parts as previous**, or the ones you pick). The
**Cut list** button in the tab bar opens the cut list panel, with lengths, angles, stock totals
and mass, which you can copy or export as CSV.

## Composite part

Composite part groups parts under one name without changing their shape. Composite parts
appear under **Composite parts** in the Parts list.

- **Parts and composite parts** takes the members. While the dialog is open the view shows the
  Part Studio as it was before the feature, so you can pick the parts. Drag a box over empty
  space to add every part inside it.
- **Closed** makes the composite part behave as a single part: it is listed, picked and
  inserted into assemblies as one, and its members can't be picked on their own.

## Derived

Derived brings parts, sketches, planes and mate connectors of another Part Studio into this one,
or a PCB Studio board with its components. A tab of this document follows its edits as you make
them. A version, of this document or another, stays as it was until you update it (see [Using
one document in another](../concepts/linking.md)).

- **Source**: **Current document** lists this document's other Part Studios and boards, and its
  version graph picks a version instead. **Other documents** browses your other documents. Once
  chosen, **Change** picks another.
- **Derive**: **Part Studio (every part, sketch and plane)**, or tick the items you want one by
  one.
- **Locations**: mate connectors of this Part Studio, or its origin. Each location gets a copy.
  Left empty, one copy goes at the origin.
- **Placement**: **Base origin** puts the source's origin on each location, and **Base mate
  connector** uses one of the source's mate connectors instead.
- **Include mate connectors** brings the source's mate connectors along. **Include properties**
  brings every property. Without it, only the name, material and appearance come over.

A Part Studio can't derive itself, the same Part Studio twice, or a chain that leads back to
itself.

### Boards

A derived board has other options. **Show** is the **3D model** or the **Layout**. **Components**
is **All**, **Top**, **Bottom** or **None**, or pick them one by one, and **Include parts placed
off the board** adds the parts outside the outline. Without a location the board sits on the
plane of the sketch it was made from. **Board point on the location** (**Fixed**, **Origin**,
**Centre** or **Footprint**) and **Board face on the location** (**Bottom face** or **Top face**)
set how it sits on a mate connector. **Show the mask, copper and silkscreen** draws the board's
layers.

In the feature list a Derived row opens to show what it brought in. For a source at a version,
its menu adds **Open linked document**, **Update linked document…** and pinning or unpinning the
reference.
