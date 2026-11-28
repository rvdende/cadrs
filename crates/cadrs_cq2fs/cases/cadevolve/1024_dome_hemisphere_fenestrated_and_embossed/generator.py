def dome_hemisphere_fenestrated_and_embossed(
    R: float = 12.0,
    thickness: float = 1.0,
    window_cols: int = 12,
    window_rows: int = 3,
    window_width: float = 1.0,
    window_height: float = 1.5,
    ring_width: float = 0.2,
    ring_height: float = 0.1
) -> "cq.Workplane":
    """
    Dome with arranged fenestrations and embossed accent rings.

    Creates a hemispherical dome of radius R and wall thickness 'thickness',
    cuts an array of arched windows into its surface, then embosses a ring
    around the perimeter of each fenestration.

    Parameters:
        R (float): Radius of the dome (> 0).
        thickness (float): Wall thickness (0 < thickness < R).
        window_cols (int): Number of windows around the circumference (>= 1).
        window_rows (int): Number of window rows from base to apex (>= 1).
        window_width (float): Width of each window (0 < window_width <= 2*R).
        window_height (float): Height of the rectangular portion of each window (0 < window_height <= 2*R).
        ring_width (float): Radial width of embossed ring around each window (> 0).
        ring_height (float): Height of embossed ring above dome surface (> 0).
    Returns:
        cq.Workplane: The dome with fenestrations and embossed rings.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got {thickness}")
    if not isinstance(window_cols, int) or window_cols < 1:
        raise ValueError(f"window_cols must be a positive integer; got {window_cols}")
    if not isinstance(window_rows, int) or window_rows < 1:
        raise ValueError(f"window_rows must be a positive integer; got {window_rows}")
    if window_width <= 0 or window_width > 2 * R:
        raise ValueError(f"window_width must be > 0 and <= {2*R}; got {window_width}")
    if window_height <= 0 or window_height > 2 * R:
        raise ValueError(f"window_height must be > 0 and <= {2*R}; got {window_height}")
    if ring_width <= 0:
        raise ValueError(f"ring_width must be > 0; got {ring_width}")
    if ring_height <= 0:
        raise ValueError(f"ring_height must be > 0; got {ring_height}")

    # 1) Build outer hemisphere
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2*R, 2*R)
                .extrude(-R)
          )
    )
    # 2) Hollow it to create shell
    inner_R = R - thickness
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

    # Precompute some values
    arch_r = window_width / 2.0
    cut_depth = R + thickness
    vert_spacing = R / (window_rows + 1)

    # 3) Cut the arched windows
    for i in range(window_rows):
        z_mid = (i+1) * vert_spacing
        # skip windows that would poke above the dome
        if z_mid + arch_r >= R:
            continue
        r_mid = math.sqrt(R*R - z_mid*z_mid)
        if r_mid < arch_r:
            continue
        for j in range(window_cols):
            phi = 2*math.pi * j / window_cols
            x = r_mid * math.cos(phi)
            y = r_mid * math.sin(phi)
            # inward-pointing normal
            nx, ny, nz = -x/R, -y/R, -z_mid/R
            # local X direction (tangent to latitude)
            tx, ty, tz = -y/r_mid, x/r_mid, 0.0
            plane = cq.Plane(origin=(x, y, z_mid),
                             normal=(nx, ny, nz),
                             xDir=(tx, ty, tz))
            wp = cq.Workplane(plane)
            wp = (wp.moveTo(-arch_r, -window_height/2.0)
                     .lineTo(-arch_r,  window_height/2.0)
                     .threePointArc((0.0, window_height/2.0 + arch_r),
                                    ( arch_r, window_height/2.0))
                     .lineTo( arch_r, -window_height/2.0)
                     .close())
            cutter = wp.extrude(cut_depth)
            dome = dome.cut(cutter)

    # 4) Emboss accent rings around each window
    for i in range(window_rows):
        z_mid = (i+1) * vert_spacing
        if z_mid + arch_r >= R:
            continue
        r_mid = math.sqrt(R*R - z_mid*z_mid)
        if r_mid < arch_r:
            continue
        for j in range(window_cols):
            phi = 2*math.pi * j / window_cols
            x = r_mid * math.cos(phi)
            y = r_mid * math.sin(phi)
            # same inward-pointing normal
            nx, ny, nz = -x/R, -y/R, -z_mid/R
            tx, ty, tz = -y/r_mid, x/r_mid, 0.0
            plane2 = cq.Plane(origin=(x, y, z_mid),
                              normal=(nx, ny, nz),
                              xDir=(tx, ty, tz))
            # interior window profile
            wp_int = cq.Workplane(plane2)
            wp_int = (wp_int.moveTo(-arch_r, -window_height/2.0)
                         .lineTo(-arch_r,  window_height/2.0)
                         .threePointArc((0.0, window_height/2.0 + arch_r),
                                        ( arch_r, window_height/2.0))
                         .lineTo( arch_r, -window_height/2.0)
                         .close())
            # exterior (offset) profile
            half_oe = arch_r + ring_width
            wp_ext = cq.Workplane(plane2)
            wp_ext = (wp_ext.moveTo(-half_oe, -window_height/2.0 - ring_width)
                         .lineTo(-half_oe,  window_height/2.0)
                         .threePointArc((0.0, window_height/2.0 + half_oe),
                                        ( half_oe, window_height/2.0))
                         .lineTo( half_oe, -window_height/2.0 - ring_width)
                         .close())
            # extrude inward into the shell
            solid_ext = wp_ext.extrude(-ring_height)
            solid_int = wp_int.extrude(-ring_height)
            ring = solid_ext.cut(solid_int)
            dome = dome.union(ring)

    return dome
