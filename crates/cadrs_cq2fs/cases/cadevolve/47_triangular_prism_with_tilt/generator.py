import cadquery as cq
import math

def triangular_prism_with_tilt(A=10.0, H=25.0, tilt_angle=30.0, cut_offset=0.0):
    """
    Tilted-end triangular prism.

    A triangular prism of base edge length A and height H, with its top face
    cut by a plane tilted by tilt_angle degrees about the X-axis. The cutting
    plane is offset into the prism by cut_offset (positive values move the
    plane downwards from the top face).

    Parameters:
        A (float): side length of the equilateral triangle base.
        H (float): extrusion height of the prism.
        tilt_angle (float): rotation angle in degrees of the cutting plane about the X-axis.
        cut_offset (float): distance to move the cutting plane into the prism from the top face.

    Returns:
        cadquery.Workplane: the resulting solid with a tilted end.
    """
    # Compute the height of the equilateral triangle
    tri_h = A * math.sqrt(3) / 2.0

    # Build the triangular prism
    prism = (
        cq.Workplane("XY")
        .polyline([(0.0, 0.0), (A, 0.0), (A/2.0, tri_h)])
        .close()
        .extrude(H)
    )

    # Create a workplane on the top face, offset into the prism, then rotate it
    cutting_plane = (
        prism.faces(">Z")
        .workplane(offset=-cut_offset)
        .transformed(rotate=cq.Vector(tilt_angle, 0.0, 0.0))
    )

    # Split the solid with that plane, keeping the bottom portion (the main prism)
    result = cutting_plane.split(keepBottom=True)
    return result
