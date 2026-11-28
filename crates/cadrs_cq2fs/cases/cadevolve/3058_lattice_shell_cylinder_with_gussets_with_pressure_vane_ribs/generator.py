def lattice_shell_cylinder_with_gussets_with_pressure_vane_ribs(
    outer_radius=50.0,
    wall_thickness=5.0,
    height=100.0,
    cell_size=10.0,
    rib_thickness=1.0,
    vane_count=8,
    vane_thickness=2.0,
    vane_inner_radius=None,
    fillet_radius=0.0,
):
    import cadquery as cq
    import math

    # Validate parameters
    if outer_radius <= 0.0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0.0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0.0:
        raise ValueError("height must be positive")
    if cell_size <= 0.0:
        raise ValueError("cell_size must be positive")
    if rib_thickness <= 0.0 or rib_thickness >= cell_size:
        raise ValueError("rib_thickness must be > 0 and < cell_size")
    if fillet_radius < 0.0:
        raise ValueError("fillet_radius must be >= 0")
    if not isinstance(vane_count, int) or vane_count < 0:
        raise ValueError("vane_count must be an integer >= 0")

    inner_radius = outer_radius - wall_thickness
    if vane_thickness <= 0.0 or vane_thickness >= inner_radius:
        raise ValueError("vane_thickness must be > 0 and < inner open radius")
    if vane_inner_radius is None:
        vane_inner_radius = 0.0
    if vane_inner_radius < 0.0 or (vane_inner_radius + vane_thickness) > inner_radius:
        raise ValueError(
            "vane_inner_radius must be >= 0 and vane_inner_radius + vane_thickness <= inner open radius"
        )

    rod_height = height

    # 1) Build hollow cylinder shell (open ends)
    shell = (
        cq.Workplane("XY")
        .circle(outer_radius)
        .extrude(height)
        .faces("|Z")
        .shell(-wall_thickness)
    )

    # 2) Build grid lattice struts inside
    half_rt = rib_thickness / 2.0
    max_index = int(math.floor(inner_radius / cell_size))
    positions = [i * cell_size for i in range(-max_index, max_index + 1)]
    x_positions = [x for x in positions if abs(x) + half_rt <= inner_radius]
    y_positions = [y for y in positions if abs(y) + half_rt <= inner_radius]

    lattice = None
    for x in x_positions:
        span = max(inner_radius**2 - (abs(x) + half_rt)**2, 0.0)
        y_max = math.sqrt(span)
        rod = (
            cq.Workplane("XY")
            .rect(rib_thickness, 2.0 * y_max)
            .extrude(rod_height)
            .translate((x, 0.0, 0.0))
        )
        lattice = rod if lattice is None else lattice.union(rod, clean=True)
    for y in y_positions:
        span = max(inner_radius**2 - (abs(y) + half_rt)**2, 0.0)
        x_max = math.sqrt(span)
        rod = (
            cq.Workplane("XY")
            .rect(2.0 * x_max, rib_thickness)
            .extrude(rod_height)
            .translate((0.0, y, 0.0))
        )
        lattice = rod if lattice is None else lattice.union(rod, clean=True)

    # 3) Add radial gusset ribs at lattice junctions
    gussets = None
    for x in x_positions:
        for y in y_positions:
            if x * x + y * y > (inner_radius - half_rt) ** 2:
                continue
            r_node = math.hypot(x, y)
            if r_node < 1e-6:
                continue
            ux, uy = x / r_node, y / r_node
            px, py = -uy, ux
            p0a = (x + px * half_rt, y + py * half_rt)
            p0b = (x - px * half_rt, y - py * half_rt)
            gus_len = inner_radius - r_node
            p1a = (x + ux * gus_len + px * half_rt, y + uy * gus_len + py * half_rt)
            p1b = (x + ux * gus_len - px * half_rt, y + uy * gus_len - py * half_rt)
            gus = (
                cq.Workplane("XY")
                .polyline([p0a, p0b, p1b, p1a])
                .close()
                .extrude(rod_height)
            )
            gussets = gus if gussets is None else gussets.union(gus, clean=True)

    # 4) Combine shell, lattice and gussets
    result = shell
    if lattice is not None:
        result = result.union(lattice, clean=True)
    if gussets is not None:
        result = result.union(gussets, clean=True)

    # 5) Add internal pressure‐vane ribs across the voids
    if vane_count > 0:
        vanes = None
        half_vt = vane_thickness / 2.0
        R_ext = inner_radius - half_vt
        for i in range(vane_count):
            angle_deg = 360.0 * i / vane_count
            p1 = (vane_inner_radius, -half_vt)
            p2 = (R_ext, -half_vt)
            arc_mid = (R_ext + half_vt * 0.5, 0.0)
            p3 = (R_ext, half_vt)
            p4 = (vane_inner_radius, half_vt)
            vane2d = (
                cq.Workplane("XY")
                .moveTo(*p1)
                .lineTo(*p2)
                .threePointArc(arc_mid, p3)
                .lineTo(*p4)
                .close()
            )
            vane = vane2d.extrude(rod_height).rotate((0, 0, 0), (0, 0, 1), angle_deg)
            vanes = vane if vanes is None else vanes.union(vane, clean=True)
        result = result.union(vanes, clean=True)

    # 6) Fillet vertical edges
    if fillet_radius > 0.0:
        maxf = min(
            wall_thickness,
            rib_thickness,
            (vane_thickness if vane_count > 0 else float("inf")),
        ) / 2.0
        f = min(fillet_radius, maxf)
        if f > 1e-6:
            result = result.edges("|Z").fillet(f)

    return result
