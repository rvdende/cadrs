def dome_lattice_shell_with_ribbed_lugs(
    R: float = 12.0,
    thickness: float = 1.0,
    cell_size: float = 5.0,
    base_thickness: float = None,
    lug_count: int = 6,
    lug_depth: float = 1.0,
    lug_width: float = 2.0,
    lug_height: float = 1.0,
    lug_z_offset: float = None,
    fillet_radius: float = 0.2,
    rib_thickness: float = 0.5,
    rib_fillet_radius: float = 0.1
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if cell_size <= 0:
        raise ValueError(f"cell_size must be > 0; got {cell_size}")
    if base_thickness is None:
        base_thickness = thickness
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be > 0; got {base_thickness}")
    if lug_count < 0:
        raise ValueError(f"lug_count must be >= 0; got {lug_count}")
    if lug_count > 0 and (lug_depth <= 0 or lug_width <= 0 or lug_height <= 0):
        raise ValueError("If lug_count>0, lug_depth, lug_width, lug_height must all be > 0")
    if lug_z_offset is None:
        lug_z_offset = base_thickness - lug_height
    if lug_z_offset < 0 or lug_z_offset + lug_height > base_thickness:
        raise ValueError(
            f"lug_z_offset must satisfy 0 <= lug_z_offset <= base_thickness - lug_height; got {lug_z_offset}"
        )
    if rib_thickness <= 0:
        raise ValueError(f"rib_thickness must be > 0; got {rib_thickness}")
    if rib_fillet_radius < 0:
        raise ValueError(f"rib_fillet_radius must be >= 0; got {rib_fillet_radius}")

    inner_R = R - thickness

    # 1) Build hollow hemisphere shell, trimmed at base_thickness
    outer = (
        cq.Workplane("XY")
        .sphere(R)
        .cut(
            cq.Workplane("XY")
            .workplane(offset=base_thickness)
            .rect(2 * R, 2 * R)
            .extrude(-(R + base_thickness))
        )
    )
    inner = (
        cq.Workplane("XY")
        .sphere(inner_R)
        .cut(
            cq.Workplane("XY")
            .workplane(offset=base_thickness)
            .rect(2 * inner_R, 2 * inner_R)
            .extrude(-(inner_R + base_thickness))
        )
    )
    shell = outer.cut(inner)

    # 2) Drill triangular lattice of holes on curved portion (z > base_thickness)
    sqrt3 = math.sqrt(3.0)
    dy = cell_size * sqrt3 / 2.0
    n_rows = int(math.ceil(R / dy))
    n_cols = int(math.ceil(2 * R / cell_size))
    hole_r = (cell_size - thickness) / 2.0
    if hole_r > 0:
        for i in range(-n_rows, n_rows + 1):
            y = i * dy
            if abs(y) > R:
                continue
            x_off = cell_size / 2.0 if (i % 2) else 0.0
            for j in range(-n_cols, n_cols + 1):
                x = j * cell_size + x_off
                if x * x + y * y > R * R:
                    continue
                z = math.sqrt(R * R - x * x - y * y)
                if z <= base_thickness:
                    continue
                nx, ny, nz = x / R, y / R, z / R
                cutter = (
                    cq.Workplane(cq.Plane(origin=(x, y, z), normal=(nx, ny, nz)))
                      .circle(hole_r)
                      .extrude(thickness * 2.0, both=True)
                )
                shell = shell.cut(cutter)

    # 3) Flat base ring
    base_ring = (
        cq.Workplane("XY").circle(R).extrude(base_thickness)
        .cut(cq.Workplane("XY").circle(inner_R).extrude(base_thickness))
    )
    part = shell.union(base_ring)

    # 4) Add radial snap lugs and reinforcing ribs
    if lug_count > 0:
        # snap lugs
        for k in range(lug_count):
            angle = 360.0 * k / lug_count
            z0 = lug_z_offset
            lug_wp = cq.Workplane("XY").transformed(rotate=(0, 0, angle), offset=(R, 0, z0))
            lug = lug_wp.box(lug_depth, lug_width, lug_height, centered=(False, True, False))
            if fillet_radius > 0:
                lug = lug.edges().fillet(fillet_radius)
            part = part.union(lug)
        # reinforcing triangular ribs on lug edges
        for k in range(lug_count):
            angle = 360.0 * k / lug_count
            ang = math.radians(angle)
            ux, uy = math.cos(ang), math.sin(ang)
            radial = (ux, uy, 0.0)
            tangent = (-uy, ux, 0.0)
            z0 = lug_z_offset
            for side in (1.0, -1.0):
                normal = (tangent[0] * side, tangent[1] * side, 0.0)
                origin = (
                    R * ux + normal[0] * (lug_width / 2.0),
                    R * uy + normal[1] * (lug_width / 2.0),
                    z0
                )
                rib_plane = cq.Plane(origin=origin, normal=normal, xDir=radial)
                rib = (
                    cq.Workplane(rib_plane)
                      .polyline([(0.0, 0.0), (-lug_depth, 0.0), (-lug_depth, lug_height)])
                      .close()
                      .extrude(rib_thickness)
                )
                if rib_fillet_radius > 0:
                    rib = rib.edges().fillet(rib_fillet_radius)
                part = part.union(rib)

    return part.combine()
