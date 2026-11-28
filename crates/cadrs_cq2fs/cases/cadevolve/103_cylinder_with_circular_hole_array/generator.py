def cylinder_with_circular_hole_array(
    radius: float = 10.0,
    height: float = 20.0,
    hole_diameter: float = 2.0,
    circumferential_count: int = 12,
    axial_count: int = 5,
):
    import cadquery as cq
    import math

    # 1) Base cylinder
    result = cq.Workplane("XY").circle(radius).extrude(height)

    # 2) Compute axial offsets (absolute from base)
    z_positions = [
        (j + 1) / (axial_count + 1) * height
        for j in range(axial_count)
    ]

    # 3) For each angular step, define a drilling plane and pushPoints for each row
    for i in range(circumferential_count):
        theta = 2.0 * math.pi * i / circumferential_count
        x0 = radius * math.cos(theta)
        y0 = radius * math.sin(theta)
        # plane normal = radial direction, xDir = global Z to orient holes along cylinder axis
        plane = cq.Plane(
            origin=(x0, y0, 0),
            normal=(math.cos(theta), math.sin(theta), 0),
            xDir=(0, 0, 1),
        )
        # local X is global Z, so (z,0) gives correct axial placement
        pts = [(z, 0.0) for z in z_positions]
        cutter = (
            cq.Workplane(plane)
              .pushPoints(pts)
              .circle(hole_diameter / 2.0)
              .extrude(-2.0 * radius)
        )
        result = result.cut(cutter)

    return result
