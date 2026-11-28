def dome_fenestrated_louvered(
    outer_radius: float = 10.0,
    thickness: float = 1.0,
    latitudes: int = 3,
    longitudes: int = 12,
    slot_width: float = None,
    slot_height: float = None,
    slot_depth: float = None,
    slot_angle_deg: float = 30.0,
    fillet_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math
    from cadquery import Plane

    # Validate inputs
    if outer_radius <= 0:
        raise ValueError("outer_radius must be > 0")
    if thickness <= 0 or thickness >= outer_radius:
        raise ValueError("thickness must be > 0 and < outer_radius")
    if latitudes < 0 or longitudes < 0:
        raise ValueError("latitudes and longitudes must be >= 0")

    # Build full spherical shell
    shell = (
        cq.Workplane("XZ")
          .moveTo(0.0, outer_radius)
          .threePointArc((outer_radius, 0.0), (0.0, -outer_radius))
          .lineTo(0.0, -outer_radius + thickness)
          .threePointArc((outer_radius - thickness, 0.0), (0.0, outer_radius - thickness))
          .close()
          .revolve(360.0)
    )

    # Cut bottom half to make a dome
    cutter = (
        cq.Workplane("XY")
          .box(2 * outer_radius + 1e-3, 2 * outer_radius + 1e-3, outer_radius + 1e-3,
               centered=(True, True, False))
          .translate((0, 0, -outer_radius - 1e-3))
    )
    dome = shell.cut(cutter)

    # Cut fenestration slots
    mid_r = outer_radius - thickness / 2.0
    if latitudes > 0 and longitudes > 0:
        delta_theta = (math.pi / 2.0) / (latitudes + 1)
        default_h = mid_r * delta_theta
        cos_a = math.cos(math.radians(slot_angle_deg))
        sin_a = math.sin(math.radians(slot_angle_deg))
        depth = slot_depth if (slot_depth and slot_depth > 0) else thickness * 1.2

        for i in range(latitudes):
            theta = (i + 1) * delta_theta
            sin_t = math.sin(theta)
            cos_t = math.cos(theta)
            z0 = mid_r * cos_t
            ring_r = mid_r * sin_t

            h = slot_height if (slot_height and slot_height > 0) else default_h
            w = (
                slot_width
                if (slot_width and slot_width > 0)
                else (2 * math.pi * ring_r / longitudes) * 0.8
            )

            for j in range(longitudes):
                phi = 2.0 * math.pi * j / longitudes
                nx = math.cos(phi)
                ny = math.sin(phi)
                x0 = ring_r * nx
                y0 = ring_r * ny

                # Inward normal at mid-surface
                rx = sin_t * nx
                ry = sin_t * ny
                rz = cos_t
                normal = (-rx, -ry, -rz)

                # Circumferential tangent for xDir
                circ = (-ny, nx, 0.0)
                xdir = (circ[0] * cos_a, circ[1] * cos_a, sin_a)

                plane = Plane(origin=(x0, y0, z0), normal=normal, xDir=xdir)
                slot_cut = cq.Workplane(plane).rect(w, h).extrude(depth)
                dome = dome.cut(slot_cut)

    # Fillet all sharp edges if requested
    if fillet_radius and fillet_radius > 0:
        try:
            dome = dome.edges().fillet(fillet_radius)
        except Exception:
            pass

    return dome
