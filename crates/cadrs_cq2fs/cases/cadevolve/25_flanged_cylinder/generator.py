import cadquery as cq
import math
import cadquery as cq

def flanged_cylinder(
    radius: float = 5.0,
    height: float = 20.0,
    flange_radius: float = 8.0,
    flange_thickness: float = 3.0,
) -> cq.Workplane:
    """
    Abstract: Cylinder with an external flange.
    Detailed: Begin with a basic solid cylinder. On one end face, add an annular
    ring of larger outer diameter and specified thickness to form a flange,
    using a boolean union of the cylinder and an annular ring.

    Parameters:
    radius          -- radius of the main cylinder
    height          -- height of the main cylinder
    flange_radius   -- outer radius of the flange (must be >= radius)
    flange_thickness-- thickness of the flange
    """
    # Base cylinder
    result = cq.Workplane("XY").circle(radius).extrude(height)
    # Add full-disk flange on top
    result = result.faces(">Z").workplane().circle(flange_radius).extrude(flange_thickness)
    # Cut out the inner cylinder from the flange only, leaving an annular ring
    result = result.faces(">Z").workplane().circle(radius).cutBlind(-flange_thickness)
    return result

# Example usage:
# model = flanged_cylinder(radius=5, height=20, flange_radius=10, flange_thickness=4)
# show_object(model)  # in a CadQuery viewer environment
result = flanged_cylinder()
