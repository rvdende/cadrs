import cadquery as cq
import math
import cadquery as cq

def perforated_plate(length=100.0, width=50.0, thickness=5.0,
                     hole_diameter=5.0, nx=5, ny=3,
                     spacing_x=20.0, spacing_y=15.0):
    """
    Abstract: Flat rectangular prism with a regular grid of through-holes.
    Detailed: Begin with a box_prism. On the top face sketch a rectangular
    array of points. At each grid point extrude-cut a cylinder of
    hole_diameter through the full thickness, producing a uniformly
    perforated plate.

    Parameters:
    length       -- plate X dimension
    width        -- plate Y dimension
    thickness    -- plate Z dimension
    hole_diameter-- diameter of each through-hole
    nx           -- number of holes along X
    ny           -- number of holes along Y
    spacing_x    -- center-to-center spacing between holes along X
    spacing_y    -- center-to-center spacing between holes along Y
    """
    # create the base plate
    plate = cq.Workplane("XY").box(length, width, thickness)

    # compute margins so the grid is centered
    if nx > 1:
        margin_x = (length - spacing_x * (nx - 1)) / 2.0
    else:
        margin_x = length / 2.0
    if ny > 1:
        margin_y = (width - spacing_y * (ny - 1)) / 2.0
    else:
        margin_y = width / 2.0

    # generate grid point coordinates relative to center
    xs = [ -length/2 + margin_x + i * spacing_x for i in range(nx) ]
    ys = [ -width/2  + margin_y + j * spacing_y for j in range(ny) ]
    points = [(x, y) for x in xs for y in ys]

    # cut through-holes at each grid point
    result = plate.faces(">Z") \
                  .workplane() \
                  .pushPoints(points) \
                  .hole(hole_diameter)

    return result
result = perforated_plate()
