import cadquery as cq
import math

def triangular_prism_hollow(A=10.0, H=25.0, wall_thickness=1.0):
    """
    Abstract: Hollow triangular prism by offsetting its walls inward to create a thin shell.
    Detailed: Build an equilateral triangular prism of base side A and height H.
    Subtract a smaller concentric triangular prism inset by wall_thickness from all sides,
    producing a uniform shell thickness of wall_thickness.
    
    Parameters:
    A              -- side length of the equilateral base triangle
    H              -- extrusion height of the prism
    wall_thickness -- thickness of the prism walls
    """
    # Outer triangle height and centroid
    h_outer = A * math.sqrt(3) / 2.0
    cx_outer = A / 2.0
    cy_outer = h_outer / 3.0
    outer_pts = [
        (-cx_outer, -cy_outer),
        (A - cx_outer, -cy_outer),
        (A/2.0 - cx_outer, h_outer - cy_outer)
    ]
    # Compute inner triangle side length by offsetting edges inward by wall_thickness
    inner_A = A - 2.0 * wall_thickness * math.sqrt(3)
    if inner_A <= 0:
        raise ValueError("wall_thickness too large for given A")
    # Inner triangle height and centroid
    h_inner = inner_A * math.sqrt(3) / 2.0
    cx_inner = inner_A / 2.0
    cy_inner = h_inner / 3.0
    inner_pts = [
        (-cx_inner, -cy_inner),
        (inner_A - cx_inner, -cy_inner),
        (inner_A/2.0 - cx_inner, h_inner - cy_inner)
    ]
    # Build outer and inner prisms and subtract
    outer = cq.Workplane("XY").polyline(outer_pts).close().extrude(H)
    inner = cq.Workplane("XY").polyline(inner_pts).close().extrude(H)
    return outer.cut(inner)
