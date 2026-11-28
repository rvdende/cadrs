def torus_revolve_with_profiled_fenestration(
    major_radius: float = 50.0,
    minor_radius: float = 10.0,
    thickness: float = 2.0,
    segment_angle: float = 180.0,
    segment_count: int = 8,
    channel_width_angle: float = 10.0,
    channel_depth: float = None,
    fen_cols: int = 6,
    fen_rows: int = 3,
    fen_width: float = 1.0,
    fen_height: float = 1.0,
    fen_margin_radial: float = None,
    fen_depth: float = 2.0,
    fillet_radius: float = 0.4
) -> "cq.Workplane":
    import cadquery as cq, math

    # build hollow torus segment
    inner_minor = minor_radius - thickness
    outer = (
        cq.Workplane("XZ")
          .moveTo(major_radius, 0.0)
          .circle(minor_radius)
          .revolve(segment_angle, (0.0, 0.0, 0.0), (0.0, 1.0, 0.0))
    )
    inner = (
        cq.Workplane("XZ")
          .moveTo(major_radius, 0.0)
          .circle(inner_minor)
          .revolve(segment_angle, (0.0, 0.0, 0.0), (0.0, 1.0, 0.0))
    )
    result = outer.cut(inner)

    # channel depth limited to shell
    if channel_depth is None:
        channel_depth = thickness
    channel_depth = min(channel_depth, thickness - 1e-6)
    r_cs = minor_radius - channel_depth

    # longitudinal channels
    overlap = 1e-6
    r_outer = major_radius + minor_radius + overlap
    r_inner = r_outer - channel_depth - overlap
    half_ang = 0.5 * math.radians(channel_width_angle)
    step = math.radians(segment_angle) / float(segment_count)
    height = 2.0 * (minor_radius + overlap)
    for i in range(segment_count):
        phi = step * (i + 0.5)
        a1 = phi - half_ang
        a2 = phi + half_ang
        p1 = (r_inner * math.cos(a1), r_inner * math.sin(a1))
        p2 = (r_outer * math.cos(a1), r_outer * math.sin(a1))
        p3 = (r_outer * math.cos(a2), r_outer * math.sin(a2))
        p4 = (r_inner * math.cos(a2), r_inner * math.sin(a2))
        mid_o = (r_outer * math.cos(phi), r_outer * math.sin(phi))
        mid_i = (r_inner * math.cos(phi), r_inner * math.sin(phi))
        cutter = (
            cq.Workplane("XZ")
              .polyline([p1, p2])
              .threePointArc(mid_o, p3)
              .lineTo(*p4)
              .threePointArc(mid_i, p1)
              .close()
              .extrude(height)
              .translate((0.0, -height / 2.0, 0.0))
        )
        result = result.cut(cutter)

    # fenestrate channel bottoms
    if fen_cols >= 1 and fen_rows >= 1:
        if fen_margin_radial is None:
            fen_margin_radial = fen_height * 0.5
        angle_step = segment_angle / float(fen_cols + 1)
        if fen_rows == 1:
            y_positions = [0.0]
        else:
            span = 2.0 * r_cs - 2.0 * fen_margin_radial
            y_step = span / float(fen_rows - 1)
            y_positions = [
                -r_cs + fen_margin_radial + j * y_step
                for j in range(fen_rows)
            ]
        for y_loc in y_positions:
            sin_l = y_loc / r_cs
            cos_l = math.sqrt(max(0.0, 1.0 - sin_l * sin_l))
            r_base = major_radius + r_cs * cos_l
            for col in range(fen_cols):
                phi_deg = angle_step * (col + 1)
                phi = math.radians(phi_deg)
                cosp = math.cos(phi)
                sinp = math.sin(phi)
                origin = (r_base * cosp, y_loc, r_base * sinp)
                xDir = (-sinp, 0.0, cosp)
                normal = (cosp * cos_l, sin_l, sinp * cos_l)
                cutter = (
                    cq.Workplane(cq.Plane(origin=origin, xDir=xDir, normal=normal))
                      .rect(fen_width, fen_height)
                      .extrude(-fen_depth)
                )
                result = result.cut(cutter)

    # fillet channel edges
    if fillet_radius > 0.0:
        try:
            result = result.edges("|Y").fillet(fillet_radius)
        except ValueError:
            pass

    return result
