def dome_with_embossed_contours_and_cutouts_and_snap_features(
    R: float = 12.0,
    contour_count: int = 3,
    contour_spacing: float = None,
    contour_width: float = 0.5,
    contour_height: float = 0.2,
    cut_cols: int = 12,
    cut_rows: int = 3,
    cut_width: float = 1.0,
    cut_height: float = 1.5,
    cut_depth: float = 0.2,
    frame_thickness: float = 0.1,
    emboss_height: float = 0.05,
    snap_count: int = 6,
    snap_width: float = 0.5,
    snap_height: float = 0.5,
    snap_depth: float = 0.3
) -> "cq.Workplane":
    import math
    import cadquery as cq

    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if not isinstance(contour_count, int) or contour_count < 1:
        raise ValueError(f"contour_count must be integer>=1; got {contour_count}")
    if contour_spacing is None:
        contour_spacing = R / (contour_count + 1)
    elif contour_spacing <= 0:
        raise ValueError(f"contour_spacing must be positive; got {contour_spacing}")
    if contour_width <= 0 or contour_width > 2 * R:
        raise ValueError(f"contour_width must be >0 and <= {2*R}; got {contour_width}")
    if contour_height <= 0:
        raise ValueError(f"contour_height must be positive; got {contour_height}")
    if not isinstance(cut_cols, int) or cut_cols < 1:
        raise ValueError(f"cut_cols must be integer>=1; got {cut_cols}")
    if not isinstance(cut_rows, int) or cut_rows < 1:
        raise ValueError(f"cut_rows must be integer>=1; got {cut_rows}")
    if cut_width <= 0 or cut_width > 2 * R:
        raise ValueError(f"cut_width must be >0 and <= {2*R}; got {cut_width}")
    if cut_height <= 0 or cut_height > 2 * R:
        raise ValueError(f"cut_height must be >0 and <= {2*R}; got {cut_height}")
    if cut_depth <= 0:
        raise ValueError(f"cut_depth must be positive; got {cut_depth}")
    if frame_thickness <= 0:
        raise ValueError(f"frame_thickness must be positive; got {frame_thickness}")
    if emboss_height <= 0:
        raise ValueError(f"emboss_height must be positive; got {emboss_height}")
    if not isinstance(snap_count, int) or snap_count < 0:
        raise ValueError(f"snap_count must be integer>=0; got {snap_count}")
    if snap_width <= 0 or snap_width > 2 * R:
        raise ValueError(f"snap_width must be >0 and <= {2*R}; got {snap_width}")
    if snap_height <= 0 or snap_height > 2 * R:
        raise ValueError(f"snap_height must be >0 and <= {2*R}; got {snap_height}")
    if snap_depth <= 0:
        raise ValueError(f"snap_depth must be positive; got {snap_depth}")

    # 1) Build base hemisphere (upper half)
    base = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * R, 2 * R)
                .extrude(-2 * R)
          )
    )
    result = base

    # 2) Collect window cutters and frames
    half_w = cut_width / 2.0
    half_h = cut_height / 2.0
    cut_spacing_val = R / (cut_rows + 1)
    cutters = []
    frames = []
    for i in range(cut_rows):
        z_cut = (i + 1) * cut_spacing_val
        if z_cut <= 0 or z_cut >= R:
            continue
        r_mid = math.sqrt(max(0.0, R * R - z_cut * z_cut))
        if r_mid < half_w:
            continue
        for j in range(cut_cols):
            phi = 2 * math.pi * j / cut_cols
            x = r_mid * math.cos(phi)
            y = r_mid * math.sin(phi)
            nx, ny, nz = x / R, y / R, z_cut / R
            if r_mid > 1e-6:
                tx, ty = -y / r_mid, x / r_mid
            else:
                tx, ty = 1.0, 0.0
            plane = cq.Plane(origin=(x, y, z_cut),
                             normal=(nx, ny, nz),
                             xDir=(tx, ty, 0.0))
            cutters.append(
                cq.Workplane(plane)
                  .rect(2 * half_w, 2 * half_h)
                  .extrude(-(R + cut_depth + 1e-3))
            )
            half_ow = half_w + frame_thickness
            half_oh = half_h + frame_thickness
            outer = (
                cq.Workplane(plane)
                  .rect(2 * half_ow, 2 * half_oh)
                  .extrude(emboss_height)
            )
            inner = (
                cq.Workplane(plane)
                  .rect(2 * half_w, 2 * half_h)
                  .extrude(emboss_height + 1e-3)
            )
            frames.append(outer.cut(inner))

    for cutter in cutters:
        result = result.cut(cutter)
    for frame in frames:
        result = result.union(frame)

    # 3) Emboss concentric contour bands
    shell = (
        cq.Workplane("XY")
          .sphere(R + contour_height)
          .cut(cq.Workplane("XY").sphere(R))
          .cut(
              cq.Workplane("XY")
                .transformed(offset=(0, 0, -(R + contour_height)))
                .box(
                    2 * (R + contour_height),
                    2 * (R + contour_height),
                    R + contour_height,
                    centered=(True, True, False)
                )
          )
    )
    half_cw = contour_width / 2.0
    cyl_h = R + contour_height
    rings = []
    for i in range(contour_count):
        z_mid = (i + 1) * contour_spacing
        if z_mid <= 0 or z_mid >= R:
            continue
        r_mid = math.sqrt(max(0.0, R * R - z_mid * z_mid))
        r_in = max(0.0, r_mid - half_cw)
        r_out = r_mid + half_cw
        annulus = (
            cq.Workplane("XY")
              .circle(r_out)
              .extrude(cyl_h)
              .cut(cq.Workplane("XY").circle(r_in).extrude(cyl_h))
        )
        rings.append(annulus.intersect(shell))

    for ring in rings:
        result = result.union(ring)

    # 4) Add snap feature protrusions around the rim
    snaps = []
    if snap_count > 0:
        for k in range(snap_count):
            phi = 2 * math.pi * k / snap_count
            xs = R * math.cos(phi)
            ys = R * math.sin(phi)
            nx, ny = xs / R, ys / R
            if R > 1e-6:
                tx, ty = -ys / R, xs / R
            else:
                tx, ty = 1.0, 0.0
            plane = cq.Plane(origin=(xs, ys, 0.0),
                             normal=(nx, ny, 0.0),
                             xDir=(tx, ty, 0.0))
            snaps.append(
                cq.Workplane(plane)
                  .rect(snap_width, snap_height)
                  .extrude(snap_depth)
            )

    for snap in snaps:
        result = result.union(snap)

    return result
