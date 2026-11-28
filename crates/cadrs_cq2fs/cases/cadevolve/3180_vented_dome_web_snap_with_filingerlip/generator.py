def vented_dome_web_snap_with_filingerlip(
    R: float = 12.0,
    thickness: float = 1.0,
    cell_size: float = 5.0,
    hole_radius: float = 1.0,
    hinge_offset: float = 1.0,
    hinge_height: float = 2.0,
    hinge_thickness: float = 0.5,
    relief_radius: float = 0.3,
    snap_hook_width: float = 2.0,
    snap_hook_height: float = 1.0,
    snap_hook_depth: float = 0.5,
    hook_count: int = 12,
    vent_snap_wall: float = 0.2,
    vent_snap_height: float = 0.5,
    vent_snap_fillet: float = 0.1,
    lip_thickness: float = 1.0,
    lip_height: float = 1.0,
    lip_fillet: float = 0.7
) -> "cq.Workplane":
    """
    A vented dome web with snap features and an integrated finger lip.
    Based on dome_vented_web_snap, extrude a lip around snap perimeter for grip,
    fillet the lip by lip_fillet, and unify the body.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got {thickness}")
    if cell_size <= 0:
        raise ValueError(f"cell_size must be > 0; got {cell_size}")
    if hole_radius <= 0:
        raise ValueError(f"hole_radius must be > 0; got {hole_radius}")
    if hinge_offset < 0 or hinge_height <= 0 or hinge_offset + hinge_height > R:
        raise ValueError(
            f"hinge_offset and hinge_height must define a band within [0, R]; "
            f"got offset={hinge_offset}, height={hinge_height}"
        )
    if hinge_thickness <= 0 or hinge_thickness >= thickness:
        raise ValueError(f"hinge_thickness must be > 0 and < thickness; got {hinge_thickness}")
    if relief_radius < 0:
        raise ValueError(f"relief_radius must be >= 0; got {relief_radius}")
    if snap_hook_width <= 0 or snap_hook_height <= 0:
        raise ValueError(f"hook dimensions must be > 0; got width={snap_hook_width}, height={snap_hook_height}")
    if snap_hook_depth < 0 or snap_hook_depth > hinge_thickness:
        raise ValueError(
            f"snap_hook_depth must be >= 0 and <= hinge_thickness; got {snap_hook_depth}"
        )
    if hook_count < 1:
        raise ValueError(f"hook_count must be >= 1; got {hook_count}")
    if vent_snap_wall < 0:
        raise ValueError(f"vent_snap_wall must be >= 0; got {vent_snap_wall}")
    if vent_snap_height < 0 or vent_snap_height > thickness:
        raise ValueError(f"vent_snap_height must be >= 0 and <= thickness; got {vent_snap_height}")
    if vent_snap_fillet < 0:
        raise ValueError(f"vent_snap_fillet must be >= 0; got {vent_snap_fillet}")
    if lip_thickness < 0:
        raise ValueError(f"lip_thickness must be >= 0; got {lip_thickness}")
    if lip_height < 0:
        raise ValueError(f"lip_height must be >= 0; got {lip_height}")
    if lip_fillet < 0:
        raise ValueError(f"lip_fillet must be >= 0; got {lip_fillet}")

    # 1) Outer hemisphere
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * R, 2 * R)
                .extrude(-R)
          )
    )

    # 2) Hollow inside
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

    # 3) Drill a triangular lattice of circular vents
    sqrt3 = math.sqrt(3.0)
    dy = cell_size * sqrt3 / 2.0
    n_rows = int(math.ceil(inner_R / dy))
    n_cols = int(math.ceil(2.0 * inner_R / cell_size))
    vent_planes = []
    for i in range(-n_rows, n_rows + 1):
        y = i * dy
        if abs(y) > inner_R:
            continue
        x_off = (cell_size / 2.0) if (i % 2) else 0.0
        for j in range(-n_cols, n_cols + 1):
            x = j * cell_size + x_off
            if x * x + y * y > inner_R * inner_R:
                continue
            z = math.sqrt(inner_R * inner_R - x * x - y * y)
            nx, ny, nz = x / inner_R, y / inner_R, z / inner_R
            plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz))
            cutter = cq.Workplane(plane).circle(hole_radius).extrude(thickness * 2.0)
            dome = dome.cut(cutter)
            vent_planes.append((x, y, z, nx, ny, nz))

    # 4) Thin living-hinge band
    z0 = hinge_offset
    z1 = hinge_offset + hinge_height
    r0_outer = math.sqrt(max(0.0, R * R - z0 * z0))
    r1_outer = math.sqrt(max(0.0, R * R - z1 * z1))
    r0_inner = r0_outer - thickness
    r1_inner = r1_outer - thickness
    reduction = thickness - hinge_thickness
    r0_cut = r0_inner + reduction
    r1_cut = r1_inner + reduction
    hinge_cut = (
        cq.Workplane("XZ")
          .moveTo(r0_inner, z0)
          .lineTo(r1_inner, z1)
          .lineTo(r1_cut, z1)
          .lineTo(r0_cut, z0)
          .close()
          .revolve(360)
    )
    dome = dome.cut(hinge_cut)

    # 5) Relief grooves at hinge edges
    for z_rel in (z0, z1):
        r_rel = math.sqrt(max(0.0, R * R - z_rel * z_rel))
        groove = (
            cq.Workplane("XZ")
              .pushPoints([(r_rel, z_rel)])
              .circle(relief_radius)
              .revolve(360)
        )
        dome = dome.cut(groove)

    # 6) Snap‐fit hooks inside hinge band
    for k in range(hook_count):
        angle = 360.0 * k / hook_count
        hook = (
            cq.Workplane("XY")
              .transformed(offset=(0, 0, z1), rotate=(0, 0, angle))
              .center(r1_inner - snap_hook_width / 2.0, 0)
              .rect(snap_hook_width, snap_hook_height)
              .extrude(-snap_hook_depth)
        )
        dome = dome.union(hook)

    # 7) Vent snap‐rings
    for x, y, z, nx, ny, nz in vent_planes:
        plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz))
        outer_r = hole_radius + vent_snap_wall
        ring_outer = cq.Workplane(plane).circle(outer_r).extrude(-vent_snap_height)
        cutter = cq.Workplane(plane).circle(hole_radius).extrude(-vent_snap_height)
        ring = ring_outer.cut(cutter)
        if vent_snap_fillet > 0:
            r_f = min(vent_snap_fillet, vent_snap_height, vent_snap_wall)
            if r_f > 0:
                try:
                    ring = ring.edges().fillet(r_f)
                except Exception:
                    pass
        dome = dome.union(ring)

    # 8) Finger lip around snap perimeter for grip
    if lip_thickness > 0 and lip_height > 0:
        lip_profile = (
            cq.Workplane("XZ")
              .moveTo(r1_outer, z1)
              .lineTo(r1_outer + lip_thickness, z1)
              .lineTo(r1_outer + lip_thickness, z1 + lip_height)
              .lineTo(r1_outer, z1 + lip_height)
              .close()
              .revolve(360)
        )
        if lip_fillet > 0:
            rf = min(lip_fillet, lip_thickness, lip_height)
            if rf > 0:
                try:
                    lip_profile = lip_profile.edges().fillet(rf)
                except Exception:
                    pass
        dome = dome.union(lip_profile)

    # 9) Combine all into one solid
    try:
        dome = dome.combine()
    except Exception:
        pass

    return dome
