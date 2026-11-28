def dome_shell_with_integrated_branching_boss_mesh_and_snaps(
    R: float = 20.0,
    thickness: float = 1.0,
    cell_size: float = 5.0,
    boss_height: float = 2.0,
    base_thickness: float = None,
    snap_count: int = 6,
    snap_depth: float = 1.0,
    snap_width: float = 2.0,
    snap_height: float = 1.0,
    snap_z_offset: float = None,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    """
    Build a hemispherical shell of outer radius R and wall thickness `thickness`,
    trimmed at `base_thickness` (flat ring at the bottom).  Inside the dome is a
    triangular lattice of small bosses (branching mesh) of diameter (cell_size - thickness)
    and height `boss_height`, anchored to the inner surface.  Around the rim are
    radial snap hooks projecting inward to secure a base plate.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}")
    inner_R = R - thickness
    if cell_size <= thickness:
        raise ValueError(f"cell_size must be > thickness; got cell_size={cell_size}, thickness={thickness}")
    if boss_height <= 0:
        raise ValueError(f"boss_height must be > 0; got {boss_height}")
    if base_thickness is None:
        base_thickness = thickness
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be > 0; got {base_thickness}")
    if snap_count < 0:
        raise ValueError(f"snap_count must be >= 0; got {snap_count}")
    if snap_count and (snap_depth <= 0 or snap_width <= 0 or snap_height <= 0):
        raise ValueError("If snap_count>0, snap_depth, snap_width, snap_height must be > 0")
    if snap_z_offset is None:
        snap_z_offset = base_thickness - snap_height
    if snap_z_offset < 0 or snap_z_offset + snap_height > base_thickness:
        raise ValueError(
            f"snap_z_offset must satisfy 0 <= snap_z_offset <= base_thickness - snap_height; got {snap_z_offset}"
        )

    # 1) Build hollow hemisphere shell, trimmed by a flat ring of thickness base_thickness
    outer_cut_depth = R + base_thickness
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * R, 2 * R)
                .extrude(-outer_cut_depth)
          )
    )
    inner_cut_depth = inner_R + base_thickness
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * inner_R, 2 * inner_R)
                .extrude(-inner_cut_depth)
          )
    )
    shell = outer.cut(inner)

    # 2) Add flat base ring
    outer_cyl = cq.Workplane("XY").circle(R).extrude(base_thickness)
    inner_cyl = cq.Workplane("XY").circle(inner_R).extrude(base_thickness)
    base_ring = outer_cyl.cut(inner_cyl)
    part = shell.union(base_ring)

    # 3) Interior branching mesh: triangular lattice of bosses
    boss_dia = cell_size - thickness
    boss_r = boss_dia / 2.0
    sqrt3 = math.sqrt(3.0)
    dy = cell_size * sqrt3 / 2.0
    n_rows = int(math.ceil(inner_R / dy))
    n_cols = int(math.ceil(2 * inner_R / cell_size))
    if boss_dia > 0:
        for i in range(-n_rows, n_rows + 1):
            y = i * dy
            if abs(y) > inner_R:
                continue
            x_off = cell_size / 2.0 if (i % 2) else 0.0
            for j in range(-n_cols, n_cols + 1):
                x = j * cell_size + x_off
                if x * x + y * y > inner_R * inner_R:
                    continue
                z = math.sqrt(inner_R * inner_R - x * x - y * y)
                if z <= base_thickness + 1e-6:
                    continue
                nx, ny, nz = x / inner_R, y / inner_R, z / inner_R
                plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz))
                boss = cq.Workplane(plane).circle(boss_r).extrude(-boss_height)
                part = part.union(boss)

    # 4) Snap-fit hooks around the rim of the base ring (pointing inward)
    for k in range(snap_count):
        angle = 360.0 * k / snap_count
        hook_wp = (
            cq.Workplane("XY")
              .transformed(
                  rotate=(0, 0, angle + 180.0),
                  offset=(inner_R, 0, snap_z_offset)
              )
        )
        hook = hook_wp.box(snap_depth, snap_width, snap_height, centered=(False, True, False))
        if fillet_radius > 0:
            hook = hook.edges().fillet(fillet_radius)
        part = part.union(hook)

    # 5) Return the combined solid
    return part.combine()
