def cylinder_shell_with_diagonal_wave_louvres(
    radius: float = 20.0,
    thickness: float = 2.0,
    height: float = 40.0,
    slot_count: int = 12,
    slot_width: float = 2.0,
    slot_angle_deg: float = 30.0,
    wave_count: int = 3,
    wave_amplitude: float = 3.0,
    chamfer: float = 0.5,
    wave_points: int = 50
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Validate parameters
    if radius <= 0 or thickness <= 0 or thickness >= radius or height <= 0:
        raise ValueError("Require radius > thickness > 0 and height > 0")
    if slot_count < 1:
        raise ValueError("slot_count must be >= 1")
    if slot_width <= 0:
        raise ValueError("slot_width must be > 0")
    if slot_angle_deg <= 0 or slot_angle_deg >= 90:
        raise ValueError("slot_angle_deg must be between 0 and 90")
    if wave_count < 1:
        raise ValueError("wave_count must be >= 1")
    if wave_amplitude < 0:
        raise ValueError("wave_amplitude must be >= 0")
    if wave_points < 2:
        raise ValueError("wave_points must be >= 2")

    # 1) Build the thin‐walled cylinder shell
    outer = cq.Workplane("XY").circle(radius).extrude(height)
    inner_r = radius - thickness
    if inner_r > 0:
        inner = cq.Workplane("XY").circle(inner_r).extrude(height)
        shell = outer.cut(inner)
    else:
        shell = outer
    result = shell

    # Mid‐surface radius for positioning cutters
    mid_r = radius - thickness / 2.0

    # Depth to cut (a bit more than thickness to ensure through‐cut)
    cut_depth = thickness + 1e-3

    # Convert slot angle to radians
    slot_angle = math.radians(slot_angle_deg)

    # Compute half‐length of slot in the cutter plane so that its projection
    # along Z covers the full cylinder height: u_extents·sin(angle) = height
    half_length = height / (2.0 * math.sin(slot_angle))

    # Precompute 2D sine‐wave sample coordinates in the slot plane
    pts_u = [
        -half_length + (2.0 * half_length) * i / (wave_points - 1)
        for i in range(wave_points)
    ]
    pts_v = [
        wave_amplitude * math.sin(2.0 * math.pi * wave_count * i / (wave_points - 1))
        for i in range(wave_points)
    ]
    half_w = slot_width / 2.0

    # Angular step between slots
    step_ang = 2.0 * math.pi / slot_count

    # 2) Cut each diagonal, wave‐shaped slot
    for i in range(slot_count):
        phi0 = step_ang * i
        nx = math.cos(phi0)
        ny = math.sin(phi0)
        # Build a workplane whose normal is the radial direction (nx,ny,0)
        # and whose X‐axis is tilted by slot_angle from the cylinder axis
        cos_a = math.cos(slot_angle)
        sin_a = math.sin(slot_angle)
        xdir = (-ny * cos_a, nx * cos_a, sin_a)
        plane = cq.Plane(
            origin=(mid_r * nx, mid_r * ny, height / 2.0),
            normal=(nx, ny, 0.0),
            xDir=xdir
        )
        # Build a closed 2D profile in that plane: a sine wave ± half slot_width
        outer2d = [(u, v + half_w) for u, v in zip(pts_u, pts_v)]
        inner2d = [(u, v - half_w) for u, v in zip(pts_u, pts_v)]
        pts2d = outer2d + inner2d[::-1]
        cutter = cq.Workplane(plane).polyline(pts2d).close().extrude(-cut_depth)
        # Try to chamfer cutter edges so the subtraction leaves chamfered slot edges
        if chamfer > 0:
            try:
                cutter = cutter.edges().chamfer(chamfer)
            except Exception:
                # If chamfer fails, proceed without chamfer on cutter
                pass
        result = result.cut(cutter)

    return result
