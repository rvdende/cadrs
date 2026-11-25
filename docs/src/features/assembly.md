# Assemblies

An assembly places **instances** of parts, from this document's Part Studios or from other
documents, and holds them together with **mates**. An instance stays live: change the part in its
Part Studio and the assembly shows the change at once.

**Insert** (<kbd>I</kbd>) opens **Insert parts and assemblies**. Its tabs are **Current
document** (with **Part Studios**, **Assemblies** and **Boards** below), **Other documents** (see
[Using one document in another](../concepts/linking.md)) and **Standard content** (screws, nuts
and other standard parts). Click a part, or a Part Studio's row for all its parts, then click in
the view to drop an instance there; each click drops another. **Inserted:** counts what the
dialog added, and **Undo to remove instances** takes them out again one by one. ✕ removes all of
them.

To move an instance, click one of its faces, edges or corners to show the **triad** there, then
drag an arrow to slide it, a ring to turn it, or the triad's centre to move the triad itself
(right-click it for **Move to origin**). You can also drag an instance directly: it moves as far
as its mates and limits allow, and the instances mated to it follow.

Right-click an instance and choose **Fix** to hold it in place; **Unfix** frees it. A new
assembly has nothing fixed, so fix one instance (or mate it to the assembly's origin) to give the
rest something to hang on.

Mates aren't solved continuously while their dialog is open: picking, flipping and changing an
offset place the moving instance for that mate only. **Solve** in the mate dialog solves every
mate, and ✓ solves everything and adds the mate as one undo step.

Mates, groups and relations are listed under **Mate Features** in the panel on the left.
Double-click one to edit it, or right-click it for **Edit…**, **Reset** (back to where the mate
was made), **Apply limit position**, **Suppress**, **Animate…** and more. The Instances list
shows how many degrees of freedom each instance has left.

## Mate

A mate joins two instances and sets how they may still move relative to each other. The toolbar
has a button for each type; <kbd>M</kbd> starts a **Fastened** mate. Once the dialog is open,
you can switch type from its dropdown without picking again.

| Type | Allows |
| --- | --- |
| **Fastened** | No motion. |
| **Revolute** | Turning about Z. |
| **Slider** | Sliding along Z. |
| **Cylindrical** | Sliding along and turning about Z. |
| **Pin slot** | Turning about Z and sliding along X. The first connector is the slot's, with X along the slot. |
| **Planar** | Sliding along X and Y and turning about Z. |
| **Ball** | Turning about X, Y and Z. |
| **Parallel** | Sliding along X, Y and Z and turning about Z: the Z axes stay parallel. |
| **Tangent** | Two faces, edges or corners stay touching. |
| **Width** | Tabs stay centred between two faces. |

X, Y and Z are the axes of the mate connectors: the mate puts the first connector onto the
second.

The dialog's choices:

- **Mate connectors:** pick two. Hovering a face, edge or corner of an instance shows its
  connector points as dots, with the nearest one drawn as a small triad; a click takes it. Hold
  <kbd>Shift</kbd> to stay on the hovered face or edge, so you can reach a point in a tight spot.
  Explicit [mate connectors](#mate-connector) can be picked too. After the second pick, the
  first instance that is free to move jumps into place.
- **Entities** (Tangent only, instead of connectors): two faces, straight edges or corners.
  **Tangent propagation** (on by default) carries the contact on to faces tangent to the picked
  one.
- **Tab mate connectors** and **Width mate connectors** (Width only): one or two connectors on
  the tab, and exactly two on the faces it sits between. Click a field to make it the one picks
  go to. An instance can't be in both.
- **Offset** (every type but Tangent and Width): **X**, **Y** and **Z** distances, and a
  **Rotation angle** about the axis chosen in **Rotate about X**, **Rotate about Y** or **Rotate
  about Z**.
- **Limits** (Revolute, Slider, Cylindrical, Pin slot and Planar): a minimum and maximum for each
  of the type's motions. Dragging and animation stop there.
- **Simulation connection:** bonds the two parts in a simulation.

Values take units and expressions (`0.5 in`, `90 deg`, `#gap`); without a unit, lengths are in
mm and angles in degrees.

Along the bottom: **Flip primary axis** turns the mate over (for Tangent, it puts the entities on
each other's other side), **Reorient secondary axis** turns it a quarter turn about Z, **Animate
mate DOF** plays the mate's first motion once, and **Solve** solves all the assembly's mates.

## Mate connector

A mate connector is a coordinate system on an instance (an origin and X, Y, Z axes) that mates
can use. Parts carry the mate connectors made in their Part Studio, and you can make more in the
assembly with **Mate connector** (<kbd>Ctrl</kbd>+<kbd>M</kbd>). <kbd>K</kbd> shows or hides
them all.

The dialog's choices:

- The origin type: **On entity** puts the connector at the point of the face, edge or corner you
  pick; **Between entities** puts it midway between that point and a second pick, the **Between
  entity** (on the same instance).
- **Origin entity:** the point the connector sits on.
- **Realign:** sets the **Primary axis** (Z) and **Secondary axis** (X) from picked edges'
  directions or faces' normals.
- **Move:** moves the connector by **X translation**, **Y translation** and **Z translation**,
  and turns it by **Rotation** about its Z.
- **Owner entity:** the instance the connector belongs to and moves with.
- **Flip primary axis** and **Reorient secondary axis (90°)** at the bottom.

A mate's own connectors can be adjusted the same way: expand the mate in Mate Features,
right-click a connector and choose **Edit**. The mate's instance follows the change.

## Relation

A relation ties the motion of one mate to another's. Choose it from the toolbar, or from the
**Relations** button's menu.

| Type | Mates | Field |
| --- | --- | --- |
| **Gear** | Two turning mates (Revolute, Cylindrical or Pin slot), the driver first. | **Ratio** as two numbers: the first mate turns by the first while the second turns by the second, the opposite way, as meshing gears do. |
| **Rack and pinion** | The pinion's turning mate (Revolute, Cylindrical or Pin slot), then the rack's sliding mate (Slider, Cylindrical or Pin slot). | **Distance per revolution:** how far the rack moves for one turn of the pinion. |
| **Screw** | One Cylindrical mate: its turn drives its slide. | **Pitch:** the travel per turn. |
| **Linear** | Two sliding mates (Slider, Cylindrical or Pin slot). | **Ratio:** how far the second slides for each mm the first does. |

Fill the **Mates** field by clicking mates in the Mate Features list or their glyphs in the
view. A mate of the wrong type is refused with a message saying why. **Reverse direction** turns
the relation the other way (for Screw, a left-hand thread).

Once added, the related mates move together when you drag, animate, **Reset** or **Apply limit
position**.

## Group

A group holds instances together as one rigid body, keeping their positions relative to each
other, without mating each pair.

The dialog has a single **Instances** field: click instances in the view or in the Instances
list. Instances already selected when you open it are included. ✓ adds the group to Mate
Features.

A group isn't fixed in space: it still needs **Fix** or a mate, like a single instance.

## Animate

Animate plays a mate's motion. Right-click a mate that can move in Mate Features and choose
**Animate…**. (The ▶ button in the mate dialog plays the mate's first motion once, through its
range and back.)

The dialog's choices:

- **Mate:** the mate being animated.
- Which motion to drive, when the mate has more than one: **X translation**, **Y translation**,
  **Z translation** or **Z rotation**.
- **Start** and **End:** the range to play. They begin at the mate's limits when it has them,
  otherwise 0 to 25.4 mm, or 0 to 360°.
- **Steps:** how many steps from Start to End (25 to begin with). The animation takes one step
  per frame, so fewer steps play faster.
- The playback type: **Single** plays once, **Reciprocate** goes back and forth until stopped,
  **Loop** plays Start to End over and over.
- **Reverse direction:** plays from End to Start.
- **Current value:** where the mate is now, with the direction it's moving in.
- **Play** and **Stop**.

Each step moves the mate and solves the whole assembly, so mated instances and relations follow.
Animating changes nothing in the document: closing the dialog puts the assembly back as it was.
