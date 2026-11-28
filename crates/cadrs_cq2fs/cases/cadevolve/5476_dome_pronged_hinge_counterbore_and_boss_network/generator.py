def dome_pronged_hinge_counterbore_and_boss_network(
    radius: float = 40.0,
    shell_thickness: float = 2.0,
    hinge_offset: float = 5.0,
    prong_width: float = 6.0,
    prong_depth: float = 3.0,
    prong_height: float = None,
    hole_diameter: float = 3.0,
    prong_count: int = 4,
    cbore_count: int = 8,
    cbore_diameter: float = 5.0,
    cbore_depth: float = 1.0,
    cell_size: float = 10.0,
    boss_height: float = 2.0,
    fillet_radius: float = 0.4
) -> "cq.Workplane":
    """
    Dome with pronged hinge features, counterbores, and an internal boss network.
    Four hinge prongs include holes; multiple peripheral counterbores;
    interior bosses connect the prongs via webs.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if radius <= 0:
        raise ValueError(f"radius must be > 0; got {radius}")
    if shell_thickness <= 0 or shell_thickness >= radius:
        raise ValueError(f"shell_thickness must be > 0 and < radius; got {shell_thickness}")
    inner_R = radius - shell_thickness
    if hinge_offset < 0 or hinge_offset >= radius:
        raise ValueError(f"hinge_offset must be >= 0 and < radius; got {hinge_offset}")
    if prong_width <= 0:
        raise ValueError(f"prong_width must be > 0; got {prong_width}")
    if prong_depth < 0 or prong_depth > inner_R:
        raise ValueError(f"prong_depth must be ≥ 0 and ≤ inner_R ({inner_R}); got {prong_depth}")
    if prong_height is None:
        prong_height = prong_depth
    if prong_height < 0 or prong_height > (radius - hinge_offset):
        raise ValueError(f"prong_height must be ≥ 0 and ≤ {radius - hinge_offset}; got {prong_height}")
    if hole_diameter <= 0:
        raise ValueError(f"hole_diameter must be > 0; got {hole_diameter}")
    if not isinstance(prong_count, int) or prong_count < 1:
        raise ValueError(f"prong_count must be int ≥ 1; got {prong_count}")
    if not isinstance(cbore_count, int) or cbore_count < 1:
        raise ValueError(f"cbore_count must be int ≥ 1; got {cbore_count}")
    if cbore_diameter <= 0 or cbore_diameter > 2 * radius:
        raise ValueError(f"cbore_diameter must be > 0 and ≤ {2*radius}; got {cbore_diameter}")
    if cbore_depth < 0 or cbore_depth > shell_thickness:
        raise ValueError(f"cbore_depth must be ≥ 0 and ≤ shell_thickness; got {cbore_depth}")
    if cell_size <= shell_thickness:
        raise ValueError(f"cell_size must be > shell_thickness; got {cell_size}")
    if boss_height <= 0:
        raise ValueError(f"boss_height must be > 0; got {boss_height}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be ≥ 0; got {fillet_radius}")

    # compute hinge plane radii
    z0 = hinge_offset
    r0o = math.sqrt(max(0.0, radius**2 - z0**2))
    r0i = max(0.0, r0o - shell_thickness)

    # 1) Hollow hemisphere shell, trimmed at hinge_offset
    outer = (
        cq.Workplane("XY")
          .sphere(radius)
          .cut(
              cq.Workplane("XY")
                .rect(2*radius, 2*radius)
                .extrude(-radius)
          )
    )
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .rect(2*inner_R, 2*inner_R)
                .extrude(-inner_R)
          )
    )
    dome = outer.cut(inner)
    # make flat rim at hinge plane
    if z0 > 0:
        dome = dome.cut(
            cq.Workplane("XY")
              .workplane(offset=z0)
              .rect(2*radius, 2*radius)
              .extrude(-radius)
        )

    # 2) Add prongs and holes
    for i in range(prong_count):
        ang = 360.0 * i / prong_count
        # prong plate
        prong = (
            cq.Workplane("XZ")
              .moveTo(r0o, z0)
              .lineTo(r0o - prong_depth, z0)
              .lineTo(r0o - prong_depth, z0 + prong_height)
              .lineTo(r0o, z0 + prong_height)
              .close()
              .extrude(prong_width/2.0, both=True)
              .rotate((0, 0, 0), (0, 0, 1), ang)
        )
        dome = dome.union(prong)
        # hinge pin hole through prong (axis normal to prong plate)
        phi = math.radians(ang)
        r_h = r0o - prong_depth/2.0
        z_h = z0 + prong_height/2.0
        hole_cutter = (
            cq.Workplane("XZ")
              .moveTo(r_h, z_h)
              .circle(hole_diameter/2.0)
              .extrude(prong_width*2, both=True)
              .rotate((0, 0, 0), (0, 0, 1), ang)
        )
        dome = dome.cut(hole_cutter)

    # 3) Peripheral counterbores on rim
    for j in range(cbore_count):
        ang = 360.0 * j / cbore_count
        phi = math.radians(ang)
        x_cb = r0o * math.cos(phi)
        y_cb = r0o * math.sin(phi)
        cb = (
            cq.Workplane("XY")
              .workplane(offset=z0)
              .center(x_cb, y_cb)
              .circle(cbore_diameter/2.0)
              .extrude(cbore_depth)
        )
        dome = dome.cut(cb)

    # 4) Interior boss network (triangular lattice of bosses)
    boss_dia = cell_size - shell_thickness
    if boss_dia > 0:
        boss_r = boss_dia / 2.0
        sqrt3 = math.sqrt(3.0)
        dy = cell_size * sqrt3 / 2.0
        n_rows = int(math.ceil(inner_R / dy))
        n_cols = int(math.ceil(2 * inner_R / cell_size))
        for ii in range(-n_rows, n_rows + 1):
            y = ii * dy
            if abs(y) > inner_R:
                continue
            x_off = (cell_size/2.0) if (ii % 2) else 0.0
            for jj in range(-n_cols, n_cols + 1):
                x = jj * cell_size + x_off
                if x*x + y*y > inner_R*inner_R:
                    continue
                z = math.sqrt(max(inner_R*inner_R - x*x - y*y, 0.0))
                # skip below hinge plane
                if z <= z0 + 1e-6:
                    continue
                # normal at inner surface
                nx, ny, nz = x/inner_R, y/inner_R, z/inner_R
                boss_plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz))
                boss = (
                    cq.Workplane(boss_plane)
                      .circle(boss_r)
                      .extrude(-boss_height)
                )
                if fillet_radius > 0:
                    try:
                        boss = boss.edges().fillet(fillet_radius)
                    except Exception:
                        pass
                dome = dome.union(boss)

    # 5) Optional fillet on all edges
    if fillet_radius > 0:
        try:
            dome = dome.edges().fillet(fillet_radius)
        except Exception:
            pass

    return dome.combine()
