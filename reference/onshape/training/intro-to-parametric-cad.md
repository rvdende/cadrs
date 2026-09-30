# Onshape "Introduction to Parametric Feature-Based CAD": requirements for cadrs

Source: Onshape Learning Center, CAD Basics → Introduction to Parametric Feature-Based CAD
(`learn.onshape.com/learn/course/introduction-to-parametric-feature-based-cad/...`), read on
2026-09-25. The requirements below are paraphrased, not the course text. This is a short,
conceptual course of six video lessons and a quiz. It has **no hands-on exercises**, no drawings
with dimensions and no keyboard shortcuts. Its one image, a first-angle projection diagram, is in
`intro-to-parametric-cad/` (git-ignored, local only).

Each requirement has an ID (`P<lesson>.<n>`) so milestones and judges can refer to it. Many
lessons describe what a parametric CAD system *is* rather than how to click through it, so most
IDs are product-level requirements that cadrs has to meet to match the model the course teaches.
Onshape's cloud and PDM claims (P1) are listed for completeness, but they are mostly out of scope
for a local desktop app.

## 1. Introduction

### P1 History of CAD (video, 3:59)
Covers drafting boards, 2D CAD from the 1960s, 3D parametric modeling in the 1990s, and Onshape's
2015 cloud-native approach. It contrasts this with file-based CAD's problems: version confusion,
no simultaneous access, and data loss from local storage.
- P1.1 Keep design data in a single store per document, not in loose per-part files. A document
  holds its Part Studios, assemblies and drawings together.
- P1.2 Save versions automatically, with a history you can go back to (Onshape: automatic
  version control, no manual check-in or check-out).
- P1.3 Metadata on parts and documents (name, part number, etc.) that you can search and reuse.
  This is the built-in PDM idea.
- P1.4 (Out of scope for now) Real-time multi-user editing of the same design without file locks,
  and browser access with no install.

### P2 2D vs 3D CAD (video, 1:36)
- P2.1 In 2D CAD every orthographic view is drawn by hand and the views aren't linked. cadrs is a
  **parametric 3D** modeler: the part is a 3D model, and the drawing views come from it.
- P2.2 Every sketch and feature stores its own **parameters**, such as the sketch's dimensions
  and constraints or an extrude's depth. You can edit them later, and the model regenerates to
  match.
- P2.3 Changing a parameter, or a relationship between parameters, can produce a big design
  change without remodeling (full rebuild from the feature list).
- P2.4 2D drawing views stay **linked to the 3D model**. They're generated from it and update on
  their own when the model changes.

### P3 Utilizing CAD data (video, 2:23)
Uses of 3D CAD data downstream. Each one implies an export or app surface:
- P3.1 **Manufacturing drawings** of parts and assemblies, made from the model (see P2.4).
- P3.2 **2D export for cutting machines** (laser, plasma, waterjet): **DXF** and **DWG**.
- P3.3 **3D export for 3D printing**: **STL** and **OBJ**.
- P3.4 Export in a form **CAM** software can use for milling and turning. The course doesn't name
  one, but in practice that means STEP or Parasolid.
- P3.5 **Simulation / FEA** on the model (stress, durability). This is an optional,
  later-phase capability.
- P3.6 **Rendering**: photorealistic images of the model for marketing. This is an optional,
  later-phase capability.
- P3.7 The model data is also reused in assembly instructions and maintenance manuals, for
  example through exploded views and images from assemblies.

## 2. Parametric feature-based CAD

### P4 Introduction to 3D CAD (video, 0:49)
The core modeling workflow:
- P4.1 Parts start as **2D sketch profiles**. **Features** such as **Extrude** and **Revolve**
  then use those sketches to add depth.
- P4.2 A part is a series of features that run **top to bottom** in the feature list. Later
  features depend on earlier ones, so the list is a hierarchy of dependencies.
- P4.3 Each feature defines one aspect of the design, such as the overall shape, or a hole's size
  and position.
- P4.4 Parts are placed into an **assembly**, where the motion between them is defined with mates.
- P4.5 Parts and assemblies go onto **2D drawings** to communicate with manufacturing.

### P5 Design intent (video, 1:56)
Design intent means planning, organizing and anticipating changes, and building that into the
model. It decides which features you choose, how they relate as parents and children, and which
parameters drive them. The worked example is a hydraulic cylinder around a piston. The
capabilities it relies on:
- P5.1 A **master sketch** that drives a Revolve feature. It encodes the bore as the piston
  diameter plus a clearance, using sketch constraints and dimensions.
- P5.2 **Variables**: named values that can be used in dimensions and feature parameters, for
  example the piston diameter and the clearance. Changing a variable updates everything that uses
  it.
- P5.3 Dimensions can take **expressions** (such as variable + clearance), not just plain
  numbers. This follows from P5.1–P5.2.
- P5.4 The **Use** (project) sketch tool links a diameter to another part's geometry, so the
  press-fit clamp bore follows the piston when the piston changes. See also sketching course
  S20.
- P5.5 A **Linear pattern** feature repeats a feature, here the O-ring groove. The copies update
  when the original feature changes.
- P5.6 Parent/child relationships are kept as the model rebuilds. Changing the one input (the
  piston diameter) has to update the whole model without breaking downstream features.

### P6 View projections (video, 2:45; image `intro-to-parametric-cad/view-projections-first-angle.png`)
- P6.1 **Orthographic** projection shows a 3D object by projecting it onto perpendicular planes.
  There are six standard views: **Front, Top, Right, Left, Back, Bottom** (like the faces of a
  die). They're used on manufacturing drawings.
- P6.2 Drawings support both view layout standards:
  - **First-angle**: each view goes on the opposite side from where it's seen. For example, the
    left view is placed to the **right** of the front view, and the top view **below** it.
  - **Third-angle**: the views unfold naturally. The right view goes to the right of the front,
    and the top view above it.
  The image shows a first-angle sheet: a 4×4 L-shaped block with a 2×2 notch, the front view top
  left, the left view to its right, the top view below the front, and an isometric view bottom
  right.
- P6.3 **Axonometric** (parallel) projections turn the object so several sides show at once.
  There are three standard ones: **Isometric**, **Dimetric** and **Trimetric**.
  - Isometric: equal 120° angles between the projected axes, with equal scale. It's the most
    common on drawings.
  - Dimetric: two axes share a scale and the third differs.
  - Trimetric: all three axes are scaled differently.
- P6.4 In a Part Studio or Assembly, clicking a **corner of the view cube** turns the camera to a
  **trimetric** view from that corner. The six named orthographic views are also available on
  the view cube.

## Knowledge check
The course ends with a quiz on "Onshape's innovations and view projections" (P1, P6). It wasn't
started, because that would record quiz attempts on the user's account, so its questions aren't
captured. Expect it to test first-angle vs third-angle placement, the six orthographic views, the
isometric/dimetric/trimetric definitions, and the cloud/PDM benefits.

## Exercises
None. This course is lecture-only.

## Cross-cutting requirements found in the course
- X1 **Parametric regeneration**: every feature keeps editable parameters, and editing one
  rebuilds the ordered feature list (P2.2, P2.3, P4.2, P5.6).
- X2 **Variables and expressions** in dimensions and feature fields (P5.2, P5.3).
- X3 **Features beyond Extrude**: Revolve (P4.1, P5.1) and Linear pattern (P5.5).
- X4 **Assemblies with mates / motion** (P4.4).
- X5 **Drawings linked to the model**, with orthographic views, first-angle and third-angle
  standards, and an isometric view (P2.4, P3.1, P6.1–P6.3).
- X6 **View cube**: faces give the six orthographic views, and corners give trimetric views
  (P6.4). Named standard views include Isometric, Dimetric and Trimetric.
- X7 **Exports**: DXF/DWG (2D), STL/OBJ (3D print), plus a CAM-grade B-rep format (P3.2–P3.4).
- X8 **Automatic version history** for documents (P1.2).
- X9 Cross-part references through **Use** (P5.4, S20).
