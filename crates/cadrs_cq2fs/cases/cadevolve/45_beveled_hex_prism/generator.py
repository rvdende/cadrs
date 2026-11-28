import cadquery as cq
import math

def beveled_hex_prism(side=10.0, height=20.0, bevel=1.0):
    """
    Hexagonal prism with beveled ends.
    A hexagonal prism whose top and bottom hexagonal faces have equal chamfer cuts
    applied around their perimeters.

    Parameters:
        side   -- circumscribed radius of the hexagon cross‐section
        height -- prism height along Z
        bevel  -- chamfer distance applied to each end edge
    """
    # Create the base hexagonal prism
    prism = cq.Workplane("XY").polygon(6, side).extrude(height)
    # Chamfer the perimeter edges on the top face
    prism = prism.faces(">Z").edges().chamfer(bevel)
    # Chamfer the perimeter edges on the bottom face
    prism = prism.faces("<Z").edges().chamfer(bevel)
    return prism
