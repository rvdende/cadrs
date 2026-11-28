def dome_hemisphere_with_lattice_shell_with_counterbored_hole_ring(
    R: float = 20.0,
    thickness: float = 2.0,
    cell_size: float = 5.0,
    ring_hole_count: int = 8,
    ring_z: float = None,
    hole_diameter: float = 3.0,
    bore_diameter: float = None,
    bore_depth: float = None,
    ring_angle_offset: float = 0.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got R={R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if cell_size <= 0:
        raise ValueError(f"cell_size must be > 0; got cell_size={cell_size}")
    ring_hole_count = int(ring_hole_count)
    if ring_hole_count < 0:
        raise ValueError(f"ring_hole_count must be >= 0; got {ring_hole_count}")
    inner_R = R - thickness
    if ring_z is None:
        ring_z = R * 0.5
    if ring_z < 0 or ring_z > inner_R:
        raise ValueError(
            f"ring_z must be >= 0 and <= R-thickness; got ring_z={ring_z}, R={R}, thickness={thickness}"
        )
    if hole_diameter <= 0 or hole_diameter >= 2 * inner_R:
        raise ValueError(
            f"hole_diameter must be > 0 and < {2*inner_R}; got hole_diameter={hole_diameter}"
        )
    if bore_diameter is None:
        bore_diameter = hole_diameter * 2.0
    if bore_diameter <= hole_diameter or bore_diameter >= 2 * inner_R:
        raise ValueError(
            f"bore_diameter must be > hole_diameter and < {2*inner_R}; "
            f"got bore_diameter={bore_diameter}, hole_diameter={hole_diameter}"
        )
    if bore_depth is None:
        bore_depth = thickness
    if bore_depth <= 0 or bore_depth > thickness:
        raise ValueError(f"bore_depth must be > 0 and <= thickness; got bore_depth={bore_depth}")

    # build outer hemisphere
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * R, 2 * R)
                .extrude(-R)
          )
    )

    # build inner hemisphere for shell
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * inner_R, 2 * inner_R)
                .extrude(-inner_R)
          )
    )
    shell = outer.cut(inner)

    cutter_union = None

    # lattice of through‐holes
    sqrt3 = math.sqrt(3.0)
    dy = cell_size * sqrt3 / 2.0
    n_rows = int(math.ceil(R / dy))
    n_cols = int(math.ceil(2.0 * R / cell_size))
    lattice_hole_r = max(0.0, (cell_size - thickness) / 2.0)
    if lattice_hole_r > 1e-6:
        for i in range(-n_rows, n_rows + 1):
            y = i * dy
            if abs(y) > R:
                continue
            x_off = (cell_size / 2.0) if (i % 2) else 0.0
            for j in range(-n_cols, n_cols + 1):
                x = j * cell_size + x_off
                if x * x + y * y > R * R:
                    continue
                if abs(x) < 1e-6 and abs(y) < 1e-6:
                    continue
                z = math.sqrt(R * R - x * x - y * y)
                nx, ny, nz = x / R, y / R, z / R
                plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz), xDir=(-ny, nx, 0.0))
                hole = (
                    cq.Workplane(plane)
                      .circle(lattice_hole_r)
                      .extrude(thickness * 2.0, both=True)
                )
                cutter_union = hole if cutter_union is None else cutter_union.union(hole)

    # ring of counterbored holes on inner surface
    if ring_hole_count > 0:
        r_xy = math.sqrt(max(inner_R * inner_R - ring_z * ring_z, 0.0))
        for k in range(ring_hole_count):
            theta = 2.0 * math.pi * k / ring_hole_count + math.radians(ring_angle_offset)
            x0 = r_xy * math.cos(theta)
            y0 = r_xy * math.sin(theta)
            z0 = ring_z
            nx, ny, nz = x0 / inner_R, y0 / inner_R, z0 / inner_R
            plane = cq.Plane(origin=(x0, y0, z0), normal=(nx, ny, nz), xDir=(-ny, nx, 0.0))
            through = (
                cq.Workplane(plane)
                  .circle(hole_diameter / 2.0)
                  .extrude(thickness * 2.0, both=True)
            )
            # bore extruded into the shell material (positive direction)
            bore = (
                cq.Workplane(plane)
                  .circle(bore_diameter / 2.0)
                  .extrude(bore_depth)
            )
            comb = through.union(bore)
            cutter_union = comb if cutter_union is None else cutter_union.union(comb)

    if cutter_union is not None:
        shell = shell.cut(cutter_union)

    # cut four rectangular slots into the rim
    pocket_length = cell_size
    pocket_width = thickness
    for k in range(4):
        angle = 360.0 * k / 4.0
        cutp = (
            cq.Workplane("XY")
              .transformed(rotate=(0, 0, angle))
              .center(R - pocket_length / 2.0, 0)
              .rect(pocket_length, pocket_width)
              .extrude(R * 2.0, both=True)
        )
        shell = shell.cut(cutp)

    return shell
