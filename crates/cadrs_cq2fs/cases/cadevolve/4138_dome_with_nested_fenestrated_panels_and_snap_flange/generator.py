def dome_with_nested_fenestrated_panels_and_snap_flange(
    R: float = 20.0,
    thickness: float = 1.0,
    split_angle_deg: float = 45.0,
    hex_cell_size: float = 5.0,
    flange_thickness: float = 2.0,
    flange_width: float = 4.0,
    snap_teeth: int = 12,
    snap_depth: float = 1.0,
    snap_height: float = 1.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if split_angle_deg <= 0 or split_angle_deg >= 90:
        raise ValueError(f"split_angle_deg must be in (0,90); got {split_angle_deg}")
    if hex_cell_size <= 0:
        raise ValueError(f"hex_cell_size must be > 0; got {hex_cell_size}")
    if flange_thickness <= 0:
        raise ValueError(f"flange_thickness must be > 0; got {flange_thickness}")
    if flange_width <= 0:
        raise ValueError(f"flange_width must be > 0; got {flange_width}")
    if snap_teeth < 0:
        raise ValueError(f"snap_teeth must be >= 0; got {snap_teeth}")
    if snap_depth <= 0:
        raise ValueError(f"snap_depth must be > 0; got {snap_depth}")
    if snap_height <= 0:
        raise ValueError(f"snap_height must be > 0; got {snap_height}")

    # 1) Build hollow hemisphere shell
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

    # 2) Split shell into two nested panels at split_angle_deg
    split_angle = math.radians(split_angle_deg)
    z_split = R * math.cos(split_angle)
    big = 4 * R
    below_box = (
        cq.Workplane("XY")
          .workplane(offset=z_split)
          .rect(big, big)
          .extrude(-3 * R)
    )
    above_box = (
        cq.Workplane("XY")
          .workplane(offset=z_split)
          .rect(big, big)
          .extrude(3 * R)
    )
    panel1 = shell.cut(below_box)  # top cap
    panel2 = shell.cut(above_box)  # lower shell

    # 3) Fenestrate each panel with circular holes in a hex grid
    dy = hex_cell_size * math.sqrt(3.0) / 2.0
    n_rows = int(math.ceil(R / dy))
    n_cols = int(math.ceil(2.0 * R / hex_cell_size))
    holes1 = []
    holes2 = []
    for i in range(-n_rows, n_rows + 1):
        y = i * dy
        x_off = (hex_cell_size / 2.0) if (i % 2) else 0.0
        for j in range(-n_cols, n_cols + 1):
            x = j * hex_cell_size + x_off
            if x * x + y * y > R * R:
                continue
            if abs(x) < 1e-6 and abs(y) < 1e-6:
                continue
            z = math.sqrt(max(R * R - x * x - y * y, 0.0))
            if z >= z_split:
                holes1.append((x, y, z))
            else:
                holes2.append((x, y, z))
    hole_diam = hex_cell_size * 0.8
    cut_depth = thickness * 2.0 + 0.1

    for x, y, z in holes1:
        normal = (x / R, y / R, z / R)
        cx, cy = -normal[1], normal[0]
        cl = math.hypot(cx, cy)
        xdir = (cx / cl, cy / cl, 0.0) if cl > 1e-6 else (1.0, 0.0, 0.0)
        plane = cq.Plane(origin=(x, y, z), normal=normal, xDir=xdir)
        cutter = (
            cq.Workplane(plane)
              .circle(hole_diam / 2.0)
              .extrude(-cut_depth)
        )
        panel1 = panel1.cut(cutter)

    for x, y, z in holes2:
        normal = (x / R, y / R, z / R)
        cx, cy = -normal[1], normal[0]
        cl = math.hypot(cx, cy)
        xdir = (cx / cl, cy / cl, 0.0) if cl > 1e-6 else (1.0, 0.0, 0.0)
        plane = cq.Plane(origin=(x, y, z), normal=normal, xDir=xdir)
        cutter = (
            cq.Workplane(plane)
              .circle(hole_diam / 2.0)
              .extrude(-cut_depth)
        )
        panel2 = panel2.cut(cutter)

    # 4) Add a snap-fit flange on the interior rim
    flange = (
        cq.Workplane("XY")
          .circle(inner_R)
          .circle(inner_R - flange_width)
          .extrude(-flange_thickness)
    )

    # 5) Add radially-oriented snap teeth on the underside of the interior flange
    if snap_teeth > 0:
        mean_r = inner_R - flange_width / 2.0
        arc_len = 2.0 * math.pi * mean_r / snap_teeth
        tooth_len = arc_len * 0.6
        flange_with_teeth = flange
        for k in range(snap_teeth):
            ang_deg = 360.0 * k / snap_teeth
            tooth = (
                cq.Workplane("XY")
                  .center(mean_r, 0.0)
                  .rect(snap_depth, tooth_len)
                  .extrude(-snap_height)
                  .rotate((0, 0, 0), (0, 0, 1), ang_deg)
            )
            flange_with_teeth = flange_with_teeth.union(tooth)
        flange = flange_with_teeth

    # 6) Combine panels and interior flange into one solid
    return panel1.union(panel2).union(flange).combine()
