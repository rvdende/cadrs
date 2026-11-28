def cone_revolve_with_hole_spoke_pattern_and_reinforcement_with_through_lattice_infill(
    bottom_outer_radius: float = 20.0,
    top_outer_radius: float = 5.0,
    height: float = 40.0,
    wall_thickness: float = 2.0,
    hole_diameter: float = 8.0,
    spoke_count: int = 8,
    spoke_thickness: float = 1.0,
    reinforcement_depth: float = 10.0,
    cell_size: float = 5.0,
    strut_diameter: float = 1.0,
    fillet_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if bottom_outer_radius <= 0 or top_outer_radius < 0:
        raise ValueError("bottom_outer_radius must be >0 and top_outer_radius >=0")
    if height <= 0:
        raise ValueError("height must be >0")
    if wall_thickness <= 0 or wall_thickness >= bottom_outer_radius:
        raise ValueError("wall_thickness must be >0 and < bottom_outer_radius")
    if top_outer_radius > 0 and wall_thickness >= top_outer_radius:
        raise ValueError("wall_thickness must be < top_outer_radius")
    if hole_diameter < 0:
        raise ValueError("hole_diameter must be >=0")
    hole_radius = hole_diameter * 0.5
    if hole_diameter > 0 and hole_radius > top_outer_radius:
        raise ValueError("hole_diameter too large for top face")
    spoke_count = int(spoke_count)
    if spoke_count < 0:
        raise ValueError("spoke_count must be >=0")
    if spoke_thickness <= 0:
        raise ValueError("spoke_thickness must be >0")
    if reinforcement_depth < 0 or reinforcement_depth > height:
        raise ValueError("reinforcement_depth must be in [0, height]")
    if cell_size <= 0:
        raise ValueError("cell_size must be >0")
    if strut_diameter <= 0 or strut_diameter >= cell_size:
        raise ValueError("strut_diameter must be >0 and < cell_size")
    if fillet_radius < 0 or fillet_radius > (strut_diameter / 2.0):
        raise ValueError("fillet_radius must be >=0 and <= strut_diameter/2")

    # Compute inner radii
    inner_bottom_radius = bottom_outer_radius - wall_thickness
    inner_top_radius = max(0.0, top_outer_radius - wall_thickness)

    # Build outer frustum
    taper_outer = math.degrees(
        math.atan2(bottom_outer_radius - top_outer_radius, height)
    )
    outer = (
        cq.Workplane("XY")
          .circle(bottom_outer_radius)
          .extrude(height, taper=taper_outer)
    )

    # Build inner frustum
    taper_inner = math.degrees(
        math.atan2(inner_bottom_radius - inner_top_radius, height)
    )
    inner = (
        cq.Workplane("XY")
          .circle(inner_bottom_radius)
          .extrude(height, taper=taper_inner)
    )

    # Make the thin shell
    shell = outer.cut(inner)

    # Build lattice rods
    max_inner_radius = max(inner_bottom_radius, inner_top_radius)
    n = int(math.ceil(max_inner_radius / cell_size))
    coords = [i * cell_size + cell_size / 2.0 for i in range(-n, n)]
    half_strut = strut_diameter / 2.0
    lattice = None

    # Bars along X
    for y in coords:
        if abs(y) + half_strut <= max_inner_radius:
            x_span = math.sqrt(max_inner_radius**2 - (abs(y) + half_strut)**2)
            bar = (
                cq.Workplane("XY")
                  .center(0, y)
                  .rect(2.0 * x_span, strut_diameter)
                  .extrude(height)
            )
            lattice = bar if lattice is None else lattice.union(bar)

    # Bars along Y
    for x in coords:
        if abs(x) + half_strut <= max_inner_radius:
            y_span = math.sqrt(max_inner_radius**2 - (abs(x) + half_strut)**2)
            bar = (
                cq.Workplane("XY")
                  .center(x, 0)
                  .rect(strut_diameter, 2.0 * y_span)
                  .extrude(height)
            )
            lattice = bar if lattice is None else lattice.union(bar)

    # Trim and fillet lattice to fit the cavity
    if lattice is not None:
        lattice = lattice.intersect(inner)
        if fillet_radius > 0.0:
            try:
                lattice = lattice.edges().fillet(fillet_radius)
            except Exception:
                pass

    # Combine shell and lattice
    result = shell
    if lattice is not None:
        result = result.union(lattice, clean=True)

    # Drill the central hole (after lattice) so it cuts both shell and rods
    if hole_diameter > 0:
        result = result.faces(">Z").workplane().hole(hole_diameter)

    # Add radial reinforcement spokes on the shell rim
    if spoke_count > 0 and hole_radius < top_outer_radius:
        length_spoke = top_outer_radius - hole_radius
        center_offset = 0.5 * (hole_radius + top_outer_radius)
        for i in range(spoke_count):
            angle = 360.0 * i / float(spoke_count)
            spoke = (
                result.faces(">Z").workplane()
                  .transformed(offset=(center_offset, 0, 0), rotate=(0, 0, angle))
                  .rect(length_spoke, spoke_thickness)
                  .extrude(-reinforcement_depth)
            )
            result = result.union(spoke)

    return result
