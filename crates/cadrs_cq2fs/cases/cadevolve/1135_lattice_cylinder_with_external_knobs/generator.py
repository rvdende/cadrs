def lattice_cylinder_with_external_knobs(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 30.0,
    vertical_count: int = 12,
    ring_count: int = 6,
    rib_diameter: float = 1.0,
    knob_diameter: float = 2.0,
    knob_height: float = 1.0
) -> cq.Workplane:
    import math
    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    if vertical_count < 1:
        raise ValueError("vertical_count must be at least 1")
    if ring_count < 1:
        raise ValueError("ring_count must be at least 1")
    if rib_diameter <= 0:
        raise ValueError("rib_diameter must be positive")
    if knob_diameter <= 0:
        raise ValueError("knob_diameter must be positive")
    if knob_height <= 0:
        raise ValueError("knob_height must be positive")

    # 1) Base hollow cylinder (open ends)
    shell = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")
          .shell(-wall_thickness)
    )

    inner_radius = outer_radius - wall_thickness
    rib_radius = rib_diameter / 2.0
    r_lattice = inner_radius + rib_radius

    # positions for rings and ribs
    z_list = [(i + 1) * height / (ring_count + 1) for i in range(ring_count)]
    angle_list = [2 * math.pi * i / vertical_count for i in range(vertical_count)]

    result = shell

    # 2) Horizontal ring ribs
    for z in z_list:
        ring = (
            cq.Workplane("XZ")
              .moveTo(r_lattice, 0)
              .circle(rib_radius)
              .revolve(360.0, (0, 0, 0), (0, 1, 0))
              .translate((0, 0, z))
        )
        result = result.union(ring)

    # 3) Vertical ribs
    for theta in angle_list:
        x = r_lattice * math.cos(theta)
        y = r_lattice * math.sin(theta)
        rod = (
            cq.Workplane("XY")
              .pushPoints([(x, y)])
              .circle(rib_radius)
              .extrude(height)
        )
        result = result.union(rod)

    # 4) Knobs at each lattice node on outer shell
    knob_radius = knob_diameter / 2.0
    for z in z_list:
        for theta in angle_list:
            x = outer_radius * math.cos(theta)
            y = outer_radius * math.sin(theta)
            nx = math.cos(theta)
            ny = math.sin(theta)
            tx, ty = -ny, nx
            plane = cq.Plane(origin=(x, y, z), xDir=(tx, ty, 0), normal=(nx, ny, 0))
            knob = (
                cq.Workplane(plane)
                  .circle(knob_radius)
                  .extrude(knob_height)
            )
            result = result.union(knob)

    return result
