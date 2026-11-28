import cadquery as cq
import math
import cadquery as cq

def box_with_cross_hole(length=100.0, width=50.0, height=30.0, hole_diameter=20.0):
    """
    Abstract: Box prism with two orthogonal through-holes.
    Detailed: Start from a simple box prism. Create two cylindrical cut-through holes
    of equal diameter: one drilled along the X-axis and one along the Y-axis,
    both centered at the box mid-plane, forming a cross-shaped tunnel.
    
    Parameters:
    length        -- size of the box along the X-axis
    width         -- size of the box along the Y-axis
    height        -- size of the box along the Z-axis
    hole_diameter -- diameter of both orthogonal through-holes
    """
    box = cq.Workplane("XY").box(length, width, height)
    cyl_x = (
        cq.Workplane("YZ")
        .circle(hole_diameter/2.0)
        .extrude(length/2.0, both=True)
    )
    cyl_y = (
        cq.Workplane("XZ")
        .circle(hole_diameter/2.0)
        .extrude(width/2.0, both=True)
    )
    return box.cut(cyl_x).cut(cyl_y)
result = box_with_cross_hole()
