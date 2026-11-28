def dome_hemisphere_with_mushroom_connector_web_and_framed_cutouts(
    R: float = 12.0,
    thickness: float = 1.0,
    connector_count: int = 8,
    connector_stem_radius: float = 0.5,
    connector_stem_length: float = 2.0,
    connector_head_radius: float = 1.0,
    connector_head_height: float = 0.3,
    polar_angle: float = 45.0,
    window_cols: int = 8,
    window_rows: int = 3,
    window_width: float = 1.0,
    window_height: float = 1.5,
    frame_thickness: float = 0.2,
    emboss_height: float = 0.1,
    groove_radius: float = 0.05
) -> "cq.Workplane":
    """
    Hemisphere dome with mushroom-head webs and framed cutout panels.
    Integrates mushroom-style rib supports on the interior and cuts rectangular
    framed windows externally, in one hemispherical body.

    Parameters:
        R (float): Outer radius of the hemisphere (> 0).
        thickness (float): Wall thickness of the hemisphere (0 < thickness < R).
        connector_count (int): Number of interior mushroom connectors (>= 0).
        connector_stem_radius (float): Radius of each connector stem (> 0).
        connector_stem_length (float): Length of each connector stem (> 0).
        connector_head_radius (float): Radius of each mushroom head (>= stem radius).
        connector_head_height (float): Height of each mushroom head (>= 0).
        polar_angle (float): Polar angle (deg) from +Z for connector ring (0–90).
        window_cols (int): Number of window columns (>=1).
        window_rows (int): Number of window rows (>=1).
        window_width (float): Width of each window (>0, <=2R).
        window_height (float): Height of each window (>0, <=2R).
        frame_thickness (float): Thickness of window frame (>0).
        emboss_height (float): Frame emboss height (>=0).
        groove_radius (float): Fillet radius for frame edges (>=0).
    """
    import cadquery as cq
    import math

    # parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got {thickness}")
    if not isinstance(connector_count, int) or connector_count < 0:
        raise ValueError(f"connector_count must be a non-negative integer; got {connector_count}")
    if connector_stem_radius <= 0:
        raise ValueError(f"connector_stem_radius must be > 0; got {connector_stem_radius}")
    if connector_head_radius < connector_stem_radius:
        raise ValueError(
            f"connector_head_radius must be >= connector_stem_radius; "
            f"got head_radius={connector_head_radius}, stem_radius={connector_stem_radius}"
        )
    if connector_stem_length <= 0:
        raise ValueError(f"connector_stem_length must be > 0; got {connector_stem_length}")
    if connector_head_height < 0:
        raise ValueError(f"connector_head_height must be >= 0; got {connector_head_height}")
    if polar_angle < 0 or polar_angle > 90:
        raise ValueError(f"polar_angle must be between 0 and 90; got {polar_angle}")
    if not isinstance(window_cols, int) or window_cols < 1:
        raise ValueError(f"window_cols must be >= 1; got {window_cols}")
    if not isinstance(window_rows, int) or window_rows < 1:
        raise ValueError(f"window_rows must be >= 1; got {window_rows}")
    if window_width <= 0 or window_width > 2 * R:
        raise ValueError(f"window_width must be > 0 and <= {2*R}; got {window_width}")
    if window_height <= 0 or window_height > 2 * R:
        raise ValueError(f"window_height must be > 0 and <= {2*R}; got {window_height}")
    if frame_thickness <= 0:
        raise ValueError(f"frame_thickness must be > 0; got {frame_thickness}")
    if emboss_height < 0:
        raise ValueError(f"emboss_height must be >= 0; got {emboss_height}")
    if groove_radius < 0:
        raise ValueError(f"groove_radius must be >= 0; got {groove_radius}")

    # Build outer hemisphere
    outer = (
        cq.Workplane("XY")
        .sphere(R)
        .cut(
            cq.Workplane("XY")
            .rect(2 * R, 2 * R)
            .extrude(-R)
        )
    )
    # Build inner hemisphere (shell)
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

    # Add interior mushroom connectors
    if connector_count > 0:
        theta = math.radians(polar_angle)
        r0 = inner_R
        for i in range(connector_count):
            phi = 2 * math.pi * i / connector_count
            dx = math.sin(theta) * math.cos(phi)
            dy = math.sin(theta) * math.sin(phi)
            dz = math.cos(theta)
            ox, oy, oz = dx * r0, dy * r0, dz * r0
            # normal into interior
            nx, ny, nz = -dx, -dy, -dz
            stem = (
                cq.Workplane(cq.Plane(origin=(ox, oy, oz), normal=(nx, ny, nz)))
                .circle(connector_stem_radius)
                .extrude(connector_stem_length)
            )
            hx = ox + nx * connector_stem_length
            hy = oy + ny * connector_stem_length
            hz = oz + nz * connector_stem_length
            head = (
                cq.Workplane(cq.Plane(origin=(hx, hy, hz), normal=(nx, ny, nz)))
                .circle(connector_head_radius)
                .extrude(connector_head_height)
            )
            dome = dome.union(stem).union(head)

    # Cut framed rectangular windows on exterior
    half_w = window_width / 2.0
    half_h = window_height / 2.0
    cut_depth = 2 * R
    # vertical spacing of rows
    for i in range(window_rows):
        z_mid = (i + 1) * R / (window_rows + 1)
        if z_mid + half_h >= R:
            continue
        r_mid = math.sqrt(max(R * R - z_mid * z_mid, 0.0))
        if r_mid < half_w:
            continue
        for j in range(window_cols):
            phi = 2 * math.pi * j / window_cols
            x = r_mid * math.cos(phi)
            y = r_mid * math.sin(phi)
            nx, ny, nz = x / R, y / R, z_mid / R
            tx, ty = -y / r_mid, x / r_mid
            plane = cq.Plane(origin=(x, y, z_mid), normal=(nx, ny, nz), xDir=(tx, ty, 0.0))
            # cut window
            cutter = (
                cq.Workplane(plane)
                .rect(window_width, window_height)
                .extrude(-cut_depth)
            )
            dome = dome.cut(cutter)
            # build frame
            half_ow = half_w + frame_thickness
            half_oh = half_h + frame_thickness
            outer_f = (
                cq.Workplane(plane)
                .rect(2 * half_ow, 2 * half_oh)
                .extrude(emboss_height)
            )
            inner_f = (
                cq.Workplane(plane)
                .rect(window_width, window_height)
                .extrude(emboss_height + 1e-6)
            )
            frame = outer_f.cut(inner_f)
            if groove_radius > 1e-6:
                fgroove = min(groove_radius, emboss_height * 0.5, frame_thickness * 0.5)
                try:
                    frame = frame.edges().fillet(fgroove)
                except Exception:
                    pass
            dome = dome.union(frame)

    return dome.combine()
