def domeWithFloatingHexPanels(
    R: float = 20.0,
    thickness: float = 2.0,
    hex_cell_diameter: float = 4.0,
    hex_spacing: float = 1.0,
    support_diameter: float = 0.5,
    support_height: float = 1.0,
    panel_thickness: float = 1.0,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got R={R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be >0 and <R; got thickness={thickness}")
    if hex_cell_diameter <= 0:
        raise ValueError(f"hex_cell_diameter must be >0; got {hex_cell_diameter}")
    if hex_spacing < 0:
        raise ValueError(f"hex_spacing must be >=0; got {hex_spacing}")
    if support_diameter <= 0:
        raise ValueError(f"support_diameter must be >0; got {support_diameter}")
    if support_height <= 0:
        raise ValueError(f"support_height must be >0; got {support_height}")
    if panel_thickness <= 0:
        raise ValueError(f"panel_thickness must be >0; got {panel_thickness}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >=0; got {fillet_radius}")

    # Build hollow hemispherical shell (z ≥ 0)
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * R, 2 * R)
                .extrude(-R)
          )
    )
    inner_R = R - thickness
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * inner_R, 2 * inner_R)
                .extrude(-inner_R)
          )
    )
    dome = outer.cut(inner)

    # Compute hex‐grid centers over dome footprint
    sqrt3 = math.sqrt(3.0)
    hex_R = hex_cell_diameter / 2.0
    dx = hex_cell_diameter + hex_spacing
    dy = sqrt3 * hex_R + hex_spacing
    n_rows = int(math.ceil(R / dy))
    n_cols = int(math.ceil(2.0 * R / dx))

    hex_centers = []
    for i in range(-n_rows, n_rows + 1):
        y = i * dy
        if abs(y) > R:
            continue
        x_off = dx / 2.0 if (i % 2) else 0.0
        for j in range(-n_cols, n_cols + 1):
            x = j * dx + x_off
            if x * x + y * y > R * R:
                continue
            # skip the apex
            if abs(x) < 1e-6 and abs(y) < 1e-6:
                continue
            z = math.sqrt(R * R - x * x - y * y)
            hex_centers.append((x, y, z))

    # Circumscribed diameter for flat‐to‐flat = hex_cell_diameter
    circ_diam = hex_cell_diameter / math.cos(math.pi / 6)

    result = dome
    for x, y, z in hex_centers:
        # surface normal
        nx, ny, nz = x / R, y / R, z / R
        plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz))

        # support post
        support = (
            cq.Workplane(plane)
              .circle(support_diameter / 2.0)
              .extrude(support_height, combine=True, both=True)
        )
        if fillet_radius > 0:
            support = support.edges().fillet(fillet_radius)

        # floating hex panel
        panel = (
            cq.Workplane(plane)
              .workplane(offset=support_height)
              .polygon(6, circ_diam)
              .extrude(panel_thickness)
        )

        result = result.union(support).union(panel)

    return result
