def dome_with_filleted_lattice_shell(
    R: float = 12.0,
    thickness: float = 1.0,
    cell_size: float = 5.0,
    base_thickness: float = None,
    rib_count: int = 8,
    rib_width: float = 0.5,
    rib_height: float = None,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if cell_size <= thickness:
        raise ValueError(f"cell_size must be > thickness; got cell_size={cell_size}, thickness={thickness}")
    if base_thickness is None:
        base_thickness = thickness
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be positive; got {base_thickness}")
    if not isinstance(rib_count, int) or rib_count < 0:
        raise ValueError(f"rib_count must be a non-negative integer; got {rib_count}")
    if rib_width <= 0:
        raise ValueError(f"rib_width must be positive; got {rib_width}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be non-negative; got {fillet_radius}")

    # 1) Build hollow hemispherical shell above the XY plane
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
    shell = outer.cut(inner)

    # 2) Drill a triangular lattice of circular holes on the curved surface
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
            x_off = cell_size / 2.0 if (i % 2) != 0 else 0.0
            for j in range(-n_cols, n_cols + 1):
                x = j * cell_size + x_off
                if x * x + y * y > R * R:
                    continue
                z = math.sqrt(R * R - x * x - y * y)
                nx, ny, nz = x / R, y / R, z / R
                cyl = (
                    cq.Workplane(cq.Plane(origin=(x, y, z), normal=(nx, ny, nz)))
                      .circle(hole_r)
                      .extrude(thickness * 2.0, both=True)
                )
                shell = shell.cut(cyl)

    # 3) Fillet the lattice shell edges
    if fillet_radius > 0:
        shell = shell.edges().fillet(fillet_radius)

    # 4) Build and perforate the flat mounting plate at z=0 inside the shell
    base = cq.Workplane("XY").circle(inner_R).extrude(base_thickness)
    ring1_count = 6
    ring2_count = 12
    r1 = inner_R * 0.4
    r2 = inner_R * 0.8
    hole_r1 = thickness * 0.5
    hole_r2 = thickness * 0.3
    pos1 = [
        (r1 * math.cos(math.radians(a)), r1 * math.sin(math.radians(a)))
        for a in [360.0 * k / ring1_count for k in range(ring1_count)]
    ]
    pos2 = [
        (r2 * math.cos(math.radians(a)), r2 * math.sin(math.radians(a)))
        for a in [360.0 * k / ring2_count for k in range(ring2_count)]
    ]
    wp = base.faces(">Z").workplane()
    if hole_r1 > 0:
        wp = wp.pushPoints(pos1).circle(hole_r1)
    if hole_r2 > 0:
        wp = wp.pushPoints(pos2).circle(hole_r2)
    base = wp.cutThruAll()

    # 5) Add radial ribs on the plate underside into the interior
    half_w = rib_width / 2.0
    if rib_height is None:
        rib_height = inner_R
    if rib_count > 0 and rib_height > 0:
        for k in range(rib_count):
            angle = 360.0 * k / rib_count
            ang = math.radians(angle)
            cos_a, sin_a = math.cos(ang), math.sin(ang)
            pts = [
                (0.0, -half_w),
                (inner_R, -half_w),
                (inner_R,  half_w),
                (0.0,   half_w)
            ]
            pts_rot = [
                (x * cos_a - y * sin_a, x * sin_a + y * cos_a)
                for x, y in pts
            ]
            rib = (
                base.faces("<Z")
                  .workplane(invert=True)
                  .polyline(pts_rot)
                  .close()
                  .extrude(rib_height)
            )
            base = base.union(rib)

    # 6) Combine shell and plate into one solid
    result = shell.union(base).combine()
    return result
