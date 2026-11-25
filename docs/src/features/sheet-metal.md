# Sheet metal

A sheet metal model is a part made from sheet of one **thickness**, folded at **bends** of a
given inner **bend radius**. cadrs keeps three views of it in step: the folded part in the
Part Studio, its **flat pattern** (the sheet laid out before folding), and a table of its bends
and other joints. How long a bend is when laid flat is set by the **bend calculation**: a
**K Factor** (where the neutral layer lies, as a fraction of the thickness from the inside of
the bend), a **Bend allowance** or a **Bend deduction**. All lengths are in mm.

Start with **Sheet metal model** on the toolbar. Its **▾** lists the other sheet metal tools,
which change a model that is already there; you can also find them all in the tool search.

Once a Part Studio has a sheet metal model, the tab bar shows the **Sheet metal table and flat
view** button. The panel it opens shows:

- **Sheet metal context:** which model the panel shows.
- **Bends:** each bend's name, radius, angle, bend direction (**Up** or **Down**) and its K
  Factor, bend allowance or bend deduction. Double-click a radius or that last value to change
  it for one bend. Right-click a row to move it up or down, or to convert a bend to a rip.
- **Other joints:** rips and tangent joints, with their type and style, which you can change
  there. A rip can be converted to a bend.
- **The flat view:** the flat pattern with its bend centre lines and joint labels. Clicking a
  row, a bend in the flat view or a joint in the model selects the same joint in all three.
  Right-click a flat pattern to start a sketch on it, copy it, make a drawing of it, or export
  it as DXF or DWG.

## Sheet metal model

Makes a new sheet metal model. Three tabs at the top choose how:

- **Convert:** turns existing parts into sheet metal. Pick **Parts and surfaces to convert**,
  and optionally **Faces to exclude** (they get no wall) and **Edges or cylinders to bend**. The
  order you pick the bend edges in matters: it decides which walls each bend joins. Set a
  **Clearance from input** (with **Include bends**), and tick **Keep input parts** to keep the
  originals.
- **Extrude:** pull sketch curves out into walls. Pick **Sketch curves to extrude**, and
  **Arcs to extrude as bends** for arcs that should be bends rather than rolled walls. The end
  type is **Blind**, **Up to next**, **Up to face**, **Up to part** or **Up to vertex**, with
  **Depth**, **Symmetric** and **Second end position**.
- **Thicken:** give faces or sketch regions a thickness. Pick **Faces or sketch regions to
  thicken** (with **Tangent propagation**), and optionally **Edges or cylinders to bend**,
  **Clearance from input** and **Include bends**.

If you start the tool with parts selected it opens on Convert, with sketch curves on Extrude,
and with faces or regions on Thicken. The **Selections** heading is red until something is
picked.

The other sections set the model's values, which the other sheet metal features use unless you
override them:

- **General:** **Thickness** (its arrow chooses which side the material goes; Convert puts it
  inside the part so the part keeps its outer size), **Bend radius**, and **Flip direction up**,
  which shows the other side up in the flat pattern, so Up and Down bends swap.
- **Material:** **Bend calculation** (**K Factor**, **Bend allowance** or **Bend deduction**)
  with its value, and the **Rolled K Factor** used for rolled (curved) walls. Both K Factors
  must be between 0 and 1.
- **Relief:** **Minimal gap** (the smallest gap at a rip), the **Corner relief type** where two
  bends meet, and the **Bend relief type** where a bend ends beside material that carries on.
  See [Corner](#corner) and [Bend relief](#bend-relief) for the types.

A new model starts at 1 mm thick with a 1 mm bend radius, a K Factor of 0.45, a Rolled K Factor
of 0.5 and a 0.2 mm minimal gap. A value outside its range turns its field red, with the range
as its tooltip, and the feature can't be accepted until you fix it.

## Flange

Adds a wall, bent from a free edge of a sheet metal part. Pick the **Edges or side faces to
flange**; several edges make several flanges, mitred where they meet.

- **Flange alignment:** where the new wall sits against the edge: **Inner**, **Outer** or
  **Middle** puts that face of the flange on it, **Hold line** starts the bend at it.
- **End type:** **Blind** with a **Distance** (measured from the outer virtual sharp to the
  flange's tip), **Up to entity**, or **Up to entity with offset** with an **Offset value**.
- The angle: **Bend angle** with its value, **Align to geometry** with **Parallel to**, or
  **Angle from direction** with a **Direction** and **Angle**. The arrow beside it flips the
  flange to the other side.
- **Automatic miter** mitres the corners between flanges; turn it off to type a **Miter angle**.
- **Use model bend radius**; turn it off to give this flange its own **Bend radius**.
- **Partial flange** (once an edge is picked) runs the flange along part of the edge.
  **Overall parameters** chooses **Per edge** or **Per chain** (the arrow flips sides) and
  **Hold adjacent edges**; **End conditions** sets how far in each end starts, with an optional
  **Second bound**.

Arrows in the view drag the distance and the bounds of a partial flange.

## Hem

Folds the edge of a wall back on itself. Pick the **Edges or side faces to hem**, then the hem
type. The arrow beside the type folds it to the other side.

- **Straight:** folded flat. **Flattened** lays it flat against the wall, the model's minimal
  gap away; turn it off to set an **Inner radius**. **Total length** is measured from the hem's
  outermost point to its end.
- **Rolled:** a curl with an **Inner radius** and an **Angle** between 180 and 360 degrees.
- **Tear drop:** an **Inner radius**, then a gap to the wall: **Minimal gap** uses the model's,
  or turn it off to type a **Gap**. Also a **Total length**.

**Hem alignment** is **Outer** (the hem's outside lies on the original edge, so the wall gets
shorter) or **In place** (the hem's bend starts at the edge). **Corner type** is **Simple** or
**Closed**. A new hem starts with the values of the last hem you made.

## Make joint

Joins two walls of a sheet metal model where their edges meet. Pick exactly two **Edges or side
faces to join**, then the type:

- **Rip:** the walls stay separate with a gap. Its style is **Edge joint** (both walls stop
  where their inner faces meet, the minimal gap apart), or **Butt joint - Direction 1** or
  **Direction 2** (one wall runs on to cover the other's end; only for 90° joints).
- **Bend:** the walls are joined by a bend, with **Use model bend radius** or a **Bend radius**
  of its own.

## Modify joint

Changes one joint of a sheet metal model. It isn't on the toolbar: you make one by editing a row
of the sheet metal table (a radius, a value, a type or style, or **Convert … to rip** / **Convert
… to bend**), and edit it afterwards from the feature list like any feature.

- **Joint:** the joint it changes. With the field active, click another bend or rip of the
  model to pick it instead. The joint shows selected in the model while the dialog is open.
- The type: **Bend**, **Rip** or **Tangent**. A rip has its style (**Edge joint**, **Butt joint -
  Direction 1** or **Direction 2**; the butt joints only for 90° joints).
- A bend: **Use model bend radius**, or its own **Bend radius**; **Use model K Factor**, or its
  own **Bend calculation** (**K Factor**, **Bend allowance** or **Bend deduction**) and value.
  A K Factor here may be from -1.5 to 1, and has a slider too.

A bend made by a Bend or Jog feature is changed in that feature instead, and can't be converted
to a rip. A hem's bend can't be a rip either.

## Finish

**Finish sheet metal model** ends a sheet metal model. Pick its **Sheet metal parts**. Features
after this one treat the parts as ordinary solids: what they do doesn't show in the flat pattern,
as the dialog warns. The flat pattern and the table stay as they were at the Finish.

## Tab

Adds material to a wall from a sketch profile. Pick the **Tab profile**: closed sketch regions,
or whole sketches. **Flange to merge** is the walls the tab joins; picking a profile fills it in
with the walls under it, and left empty it is every wall the profile is parallel to and touches.
A tab bridging two walls in one plane merges them.

**Subtraction scope** is the parts to cut a clearance pocket from around the tab, and
**Subtraction offset** is how far the pocket is grown all round the tab.

## Bend

Folds a flat wall along a line. The bend is cut out of the wall rather than added to it, so the
flat pattern keeps its size.

- **Bend line:** a sketch line or a part edge. The arrow beside it (**Hold opposite side**)
  chooses which side of the line moves.
- **Sheet metal face to bend:** filled in from the line when there is one face under it.
- **Bend alignment:** where the bend sits on the line: **Bend line** (the bend's middle in the
  flat), **Hold line** (the bend starts at the line), **Hold other line** (it ends there), or
  **Inner**, **Outer**, **Middle** (that face of the bent wall on the line).
- The angle: **Bend angle**, **Align to geometry** (with **Parallel to**) or **Angle from
  direction** (with **Direction** and **Angle**). Its arrow (**Opposite angle**) bends the
  other way; so does clicking the arrow on the bend line in the view.
- **Use model bend radius** and **Use model K Factor**; turn either off to give this bend its
  own **Bend radius** or **K Factor**.

## Jog

Puts a step in a flat wall: two opposite bends with a short wall between them, so the far part
of the wall is offset from where it was. The dialog starts with the same rows as
[Bend](#bend), then:

- **Bounding type:** **Blind** with a **Jog offset** (also dragged with the arrow in the view),
  **Up to entity** with an optional **Offset distance**, or **Thickness** with a **Thickness
  factor** (the offset is that many thicknesses).
- **Jog offset anchor:** how the offset is measured: **Inside** (to the far wall's inner face),
  **Outside** (to its outer face) or **Nominal** (the overall height, from the other face).
- **Preserve material:** without it, the sheet is first stretched so the far end stays where it
  was; with it, no material is added.

If the offset is too small for the two bends, the jog fails.

## Corner

Sets the relief of one corner, where two bends meet, overriding the model's **Corner relief
type**. Pick the **Corner** (a face, edge or vertex of it), then the **Corner relief type**:

- **Simple:** the corner is left as the bends leave it.
- **Closed:** nothing is removed; the bends are mitred so the corner closes as far as it can.
- **Square - Sized** with a **Corner relief width**, or **Round - Sized** with a **Corner relief
  diameter**.
- **Rectangle - Scaled** or **Round - Scaled** with a **Corner relief scale** (1 to 2), sized
  from the bends.

## Bend relief

Sets the relief at one end of a bend, where the bend ends but a wall carries on along the bend
line, overriding the model's **Bend relief type**. Pick the **Bend relief** (a face, edge or
vertex at the bend's end), then the **Bend relief type**:

- **Rectangle - Scaled** or **Obround - Scaled**, with a **Bend relief depth scale** (1 to 5)
  and **Bend relief width scale** (0.0625 to 2). The width is that many thicknesses.
- **Square - Sized** or **Obround - Sized** (only here, not in the model), with a **Bend relief
  depth**. These are one thickness wide.
- **Tear:** no material removed; the sheet is only slit at the bend's end.

**Extend bend relief** runs the cut the other way, to the end of the sheet.

## Corner break

Rounds or chamfers the corners of sheet metal walls. Two tabs, **Fillet**
and **Chamfer**; in both, pick the **Entities to fillet or chamfer** (corner edges or vertices).

- **Fillet:** **Measurement** is **Radius** or **Width**, then its value. **Control** is
  **Distance**. **Asymmetric** adds a **Second radius** (or **Second width**) for the other side
  of the corner, with an arrow (**Flip asymmetric**) to swap them. **Allow edge overflow** lets
  a round longer than the corner's edges run on past them instead of failing.
- **Chamfer:** **Measurement** is **Offset** or **Tangent**, and **Chamfer type** is **Equal
  distance** (**Distance**), **Two distances** (**Distance 1**, **Distance 2**) or **Distance
  and angle** (**Distance**, **Angle**). The arrow beside the distance swaps the sides.

Make corner breaks once the joints are final: after one, the table no longer lets you change a
joint's type or style, or convert it.

## Sheet metal loft

Makes sheet metal walls between two profiles. On the toolbar's **▾** it is called **Loft**.

- **New** makes a new sheet metal model; **Add** adds walls to an existing one, chosen in
  **Merge scope**.
- **Profile 1** and **Profile 2:** a sketch region, a face, edges (sketch curves or part edges,
  chained) or a point.
- **Connections:** tick it to open **Match connections**, where each connection joins a point
  of profile 1 to a point of profile 2. Pick a vertex or edge for each end (**Vertices or
  edges**), and tick **Rip** to cut the sheet there. **Add connection** adds another. In the
  view, each connection is a line with a handle at each end; drag a handle to slide it along
  its profile.
- **Chordal tolerance:** how far the flat walls may stray from the true loft surface (1 mm to
  start with).

For **New**, the **General**, **Material** and **Relief** sections are the same as in [Sheet
metal model](#sheet-metal-model). A closed loft is ripped at its first connection.

## Form

Presses a form (a louver, dimple, emboss and so on) into the faces of a sheet metal model. A
form is a Part Studio with a [Tag](#tag) feature.

- **Form Part Studio:** click **Select Part Studio…** to open the **Select Part Studio** panel.
  Its tabs, **Current document**, **Other documents** and **Libraries**, list the forms; the
  search field filters them by name, type or document. The panel shows a preview of the chosen
  form and its variables, which you can set for this feature. Click **Done**.
- **Location(s):** where to place copies: sketch points, a whole sketch's points, vertices or
  mate connectors (the mate connector button picks one in the view). The arrow beside it
  presses the form the other way.
- **Target face(s):** the faces of the sheet metal model the form goes into.

The built-in library has a **Louver**, **Bridge lance**, **Dimple**, **Emboss** and **Extruded
hole**. A form can't touch side walls, rolled walls, rips, joints or corners. Each form shows in
the flat pattern as its outline and a centre mark.

## Tag

Makes a Part Studio a form that the [Form](#form) feature can place. Find it in the tool search
as **Tag (Form)**; the feature is called **Tag**. Its type is **Form**, and you pick:

- **Part to add:** the material the form adds to the sheet.
- **Part to remove:** the material it cuts away.
- **Sketch for flat view** (optional): a sketch of construction curves, the outline the flat
  pattern shows for the form.
- **Form origin mate connector:** the point placed at each location. It lies on the sheet's
  face with its Z axis pointing out of the sheet; the sheet is below it, from −thickness to 0.

The Part Studio also needs a Length variable named `thickness`: when the form is placed, the
Form feature sets it to the sheet metal model's thickness. A Part Studio without one can't be
picked as a form.

## Flat extrude

Adds material to, or cuts it from, the sheet using a sketch drawn on the flat pattern. First
right-click a flat pattern in the flat view and choose **New sketch on**; that sketch is drawn
in the flat view, and the flat's bend centre lines are in it as construction lines to dimension
from.

With regions of that sketch selected, **Extrude** (on the toolbar, or
<kbd>Shift</kbd>+<kbd>E</kbd>) opens a short dialog, also called **Extrude**:

- **Add** or **Remove:** it starts on **Remove** when every picked region lies on material, and
  on **Add** otherwise.
- **Faces and sketch regions to extrude:** the regions, picked in the flat view. They show
  orange there and, folded, on the 3D part.

The flat pattern changes and the part is folded again from it. An ordinary 3D extrude of a flat
pattern sketch doesn't work.
