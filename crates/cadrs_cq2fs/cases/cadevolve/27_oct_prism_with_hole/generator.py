import cadquery as cq
import math

def oct_prism_with_hole(A=8.0, H=20.0, hole_diameter=4.0):
    """
    Abstract: Octagonal prism with end hole
    Detailed: Creates a regular octagonal prism of side length A and height H,
              then drills a centered circular hole of diameter hole_diameter
              through the entire thickness.
    Parameters:
        A              -- side length of the octagon
        H              -- height of the prism along Z
        hole_diameter  -- diameter of the through hole
    """
    # compute circumscribed circle diameter for an octagon of side length A
    circ_d = A / math.sin(math.pi / 8)
    # build the octagonal prism
    prism = cq.Workplane("XY").polygon(8, circ_d).extrude(H)
    # drill a centered through hole from top face
    result = prism.faces(">Z").workplane().hole(hole_diameter)
    return result
