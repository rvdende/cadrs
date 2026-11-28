def dome_hemisphere_fenestrated(
    R: float = 12.0,
    thickness: float = 1.5,
    hole_radius: float = 0.5,
    ring_count: int = 4,
    holes_per_ring: int = 24,
    polar_start: float = 40.0,
    polar_end: float = 80.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # parameter validation
    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be >0 and <R; got {thickness}")
    if hole_radius <= 0:
        raise ValueError(f"hole_radius must be positive; got {hole_radius}")
    if not isinstance(ring_count, int) or ring_count < 1:
        raise ValueError(f"ring_count must be a positive integer; got {ring_count}")
    if not isinstance(holes_per_ring, int) or holes_per_ring < 1:
        raise ValueError(f"holes_per_ring must be a positive integer; got {holes_per_ring}")
    if polar_start < 0 or polar_start >= polar_end or polar_end > 90:
        raise ValueError(
            f"polar_start/polar_end must satisfy 0 ≤ start < end ≤ 90; got {polar_start}, {polar_end}"
        )

    # build hollow hemisphere
    outer = (
        cq.Workplane("XZ")
          .moveTo(-R, 0)
          .threePointArc((0, R), (R, 0))
          .close()
          .revolve(180, (0, 0, 0), (1, 0, 0))
    )
    inner = (
        cq.Workplane("XZ")
          .moveTo(-(R - thickness), 0)
          .threePointArc((0, R - thickness), (R - thickness, 0))
          .close()
          .revolve(180, (0, 0, 0), (1, 0, 0))
    )
    dome = outer.cut(inner)

    # cut each hole on the curved surface
    for i in range(ring_count):
        if ring_count > 1:
            theta = math.radians(polar_start + (polar_end - polar_start) * i / (ring_count - 1))
        else:
            theta = math.radians(polar_start)
        for j in range(holes_per_ring):
            phi = 2 * math.pi * j / holes_per_ring
            x = R * math.sin(theta) * math.cos(phi)
            y = R * math.sin(theta) * math.sin(phi)
            z = R * math.cos(theta)
            nx, ny, nz = x / R, y / R, z / R
            plane = cq.Plane(origin=(x, y, z), normal=(-nx, -ny, -nz))
            cyl = cq.Workplane(plane).circle(hole_radius).extrude(thickness * 2)
            dome = dome.cut(cyl)

    return dome
