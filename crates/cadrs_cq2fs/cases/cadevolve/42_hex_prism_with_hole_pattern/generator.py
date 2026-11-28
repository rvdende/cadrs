import cadquery as cq
import math

def hex_prism_with_hole_pattern(
    A=10.0,
    H=20.0,
    hole_diameter=2.0,
    nx=5,
    ny=3,
    spacing_x=None,
    spacing_y=None
):
    """
    Abstract: Hexagonal prism with patterned holes.
    Detailed: Start with a regular hexagonal prism of circumradius A and height H.
    On its top face, drill a rectangular grid of through-holes of diameter hole_diameter,
    with nx holes along X and ny holes along Y, spaced by spacing_x and spacing_y.
    If spacing_x or spacing_y is None, holes are evenly distributed across the prism's
    bounding box ([-A..A] in X and Y).
    """
    # Create the base hexagonal prism (circumradius = A)
    prism = cq.Workplane("XY").polygon(6, 2.0 * A).extrude(H)

    # Default spacings if not provided: divide bounding box into (n+1) equal gaps
    if spacing_x is None:
        spacing_x = 2.0 * A / (nx + 1)
    if spacing_y is None:
        spacing_y = 2.0 * A / (ny + 1)

    # Compute margins to center the grid in [-A..A] range
    if nx > 1:
        margin_x = (2.0 * A - spacing_x * (nx - 1)) / 2.0
    else:
        margin_x = A
    if ny > 1:
        margin_y = (2.0 * A - spacing_y * (ny - 1)) / 2.0
    else:
        margin_y = A

    xs = [ -A + margin_x + i * spacing_x for i in range(nx) ]
    ys = [ -A + margin_y + j * spacing_y for j in range(ny) ]
    points = [(x, y) for x in xs for y in ys]

    # Drill the through-holes on the top face
    prism = prism.faces(">Z").workplane().pushPoints(points).hole(hole_diameter)

    return prism

# Example usage:
# result = hex_prism_with_hole_pattern(A=15, H=10, hole_diameter=3, nx=4, ny=4)
# show_object(result)  # in a CadQuery viewer environment
