def sphere_with_cylindrical_hole(radius: float = 10.0, hole_radius: float = 2.0) -> cq.Workplane:
    """
    Abstract: Sphere with central bore.
    Detailed: Create a sphere of given radius, and subtract a cylindrical hole of given radius
    running through its center along the Z-axis.

    Parameters:
        radius      -- radius of the sphere
        hole_radius -- radius of the cylindrical hole
    """
    if hole_radius <= 0:
        raise ValueError("hole_radius must be positive")
    if hole_radius >= radius:
        raise ValueError("hole_radius must be smaller than radius")
    sphere = cq.Workplane("XY").sphere(radius)
    cutter = cq.Workplane("XY").circle(hole_radius).extrude(radius, both=True)
    return sphere.cut(cutter)
