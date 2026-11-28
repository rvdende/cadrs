def dome_hemisphere_with_elliptical_fenestration_and_embossed_trace(
    R: float = 12.0,
    thickness: float = 1.0,
    window_cols: int = 12,
    window_rows: int = 3,
    window_major: float = 1.0,
    window_minor: float = 0.5,
    trace_width: float = 0.05,
    trace_height: float = 0.1
):
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got {thickness}")
    if not isinstance(window_cols, int) or window_cols < 1:
        raise ValueError(f"window_cols must be integer>=1; got {window_cols}")
    if not isinstance(window_rows, int) or window_rows < 1:
        raise ValueError(f"window_rows must be integer>=1; got {window_rows}")
    if window_major <= 0 or window_major > 2 * R:
        raise ValueError(f"window_major must be > 0 and <= {2*R}; got {window_major}")
    if window_minor <= 0 or window_minor > 2 * R:
        raise ValueError(f"window_minor must be > 0 and <= {2*R}; got {window_minor}")
    if trace_width <= 0 or trace_width > 2 * R:
        raise ValueError(f"trace_width must be > 0 and <= {2*R}; got {trace_width}")
    if trace_height <= 0 or trace_height > R:
        raise ValueError(f"trace_height must be > 0 and <= {R}; got {trace_height}")

    # 1) Build hollow hemisphere shell
    half_cut_outer = cq.Workplane("XY").workplane(offset=-R).box(
        2 * R, 2 * R, R, centered=(True, True, False)
    )
    outer = cq.Workplane("XY").sphere(R).cut(half_cut_outer)

    inner_R = R - thickness
    half_cut_inner = cq.Workplane("XY").workplane(offset=-inner_R).box(
        2 * inner_R, 2 * inner_R, inner_R, centered=(True, True, False)
    )
    inner = cq.Workplane("XY").sphere(inner_R).cut(half_cut_inner)

    dome = outer.cut(inner)

    # 2) Cut all elliptical fenestrations
    cut_depth = thickness + 1e-3
    fenestration_cutters = None
    for i in range(window_rows):
        z_mid = (i + 1) * R / (window_rows + 1)
        if z_mid >= R:
            continue
        r_sq = R * R - z_mid * z_mid
        if r_sq <= 0:
            continue
        r_mid = math.sqrt(r_sq)
        if r_mid < window_major / 2.0:
            continue
        for j in range(window_cols):
            phi = 2 * math.pi * j / window_cols
            x0 = r_mid * math.cos(phi)
            y0 = r_mid * math.sin(phi)
            # plane at window center
            nx, ny, nz = -x0 / R, -y0 / R, -z_mid / R
            if r_mid > 1e-6:
                tx, ty, tz = -y0 / r_mid, x0 / r_mid, 0.0
            else:
                tx, ty, tz = 1.0, 0.0, 0.0
            plane = cq.Plane(origin=(x0, y0, z_mid), normal=(nx, ny, nz), xDir=(tx, ty, tz))
            cutter = cq.Workplane(plane).ellipse(window_major / 2.0, window_minor / 2.0).extrude(cut_depth)
            fenestration_cutters = cutter if fenestration_cutters is None else fenestration_cutters.union(cutter)
    if fenestration_cutters is not None:
        dome = dome.cut(fenestration_cutters)

    # 3) Prepare a thin interior shell for embossing traces
    shell_outer = cq.Workplane("XY").sphere(inner_R)
    shell_inner = cq.Workplane("XY").sphere(max(inner_R - trace_height, 0.0))
    shell_int = shell_outer.cut(shell_inner)
    half_cut_shell = cq.Workplane("XY").workplane(offset=-inner_R).box(
        2 * inner_R, 2 * inner_R, inner_R, centered=(True, True, False)
    )
    shell_int = shell_int.cut(half_cut_shell)

    # 4) Emboss radial trace lines on the interior surface
    slab_height = inner_R + trace_height
    slab_template = cq.Workplane("XY").box(
        2 * slab_height, trace_width, slab_height, centered=(True, True, False)
    )
    radial_slabs = None
    for j in range(window_cols):
        phi_deg = 360.0 * j / window_cols
        slab = slab_template.rotate((0, 0, 0), (0, 0, 1), phi_deg)
        radial_slabs = slab if radial_slabs is None else radial_slabs.union(slab)
    if radial_slabs is not None:
        ridges = shell_int.intersect(radial_slabs)
        dome = dome.union(ridges)

    return dome
