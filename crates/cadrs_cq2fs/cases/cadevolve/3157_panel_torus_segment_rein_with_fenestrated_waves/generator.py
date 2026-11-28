def panel_torus_segment_rein_with_fenestrated_waves(
    major_radius=50.0,
    minor_radius=10.0,
    thickness=2.0,
    segment_angle=180.0,
    rib_count_radial=8,
    rib_height_radial=2.0,
    rib_width_angle_radial=5.0,
    rib_count_circum=12,
    rib_height_circum=2.0,
    rib_width_angle_circum=5.0,
    fillet_radius=1.0,
    hole_count=48,
    amplitude=4.0,
    wavelength=180.0,
    min_hole_dia=0.5,
    max_hole_dia=2.0,
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # validate thickness
    if thickness <= 0 or thickness >= minor_radius:
        raise ValueError("thickness must be > 0 and < minor_radius")
    inner_minor = minor_radius - thickness

    # 1) hollow torus segment
    outer = (
        cq.Workplane("XZ")
          .moveTo(major_radius, 0)
          .circle(minor_radius)
          .revolve(segment_angle, (0, 0, 0), (0, 1, 0))
    )
    inner = (
        cq.Workplane("XZ")
          .moveTo(major_radius, 0)
          .circle(inner_minor)
          .revolve(segment_angle, (0, 0, 0), (0, 1, 0))
    )
    result = outer.cut(inner)

    # 2) radial ribs
    if rib_count_radial > 0 and rib_height_radial > 0 and rib_width_angle_radial > 0:
        h_r = min(rib_height_radial, inner_minor)
        half_ang = math.radians(rib_width_angle_radial / 2)
        r0 = inner_minor
        r1 = inner_minor - h_r
        p0 = [(major_radius + r0 * math.cos(a), r0 * math.sin(a)) for a in (-half_ang, half_ang)]
        p1 = [(major_radius + r1 * math.cos(a), r1 * math.sin(a)) for a in ( half_ang, -half_ang)]
        step = 360.0 / rib_count_radial
        for i in range(rib_count_radial):
            phi = math.radians(i * step)
            c, s = math.cos(phi), math.sin(phi)
            pts = []
            for x, z in p0 + p1:
                dx, dz = x - major_radius, z
                pts.append((dx * c - dz * s + major_radius, dx * s + dz * c))
            rib = (
                cq.Workplane("XZ")
                  .polyline(pts)
                  .close()
                  .revolve(segment_angle, (0, 0, 0), (0, 1, 0))
            )
            result = result.union(rib)

    # 3) circumferential ribs
    if rib_count_circum > 0 and rib_height_circum > 0 and rib_width_angle_circum > 0:
        half_ang = math.radians(rib_width_angle_circum / 2)
        r0 = minor_radius
        r2 = minor_radius + rib_height_circum
        q0 = [(major_radius + r0 * math.cos(a), r0 * math.sin(a)) for a in (-half_ang, half_ang)]
        q1 = [(major_radius + r2 * math.cos(a), r2 * math.sin(a)) for a in ( half_ang, -half_ang)]
        step = 360.0 / rib_count_circum
        for i in range(rib_count_circum):
            phi = math.radians(i * step)
            c, s = math.cos(phi), math.sin(phi)
            pts = []
            for x, z in q0 + q1:
                dx, dz = x - major_radius, z
                pts.append((dx * c - dz * s + major_radius, dx * s + dz * c))
            rib = (
                cq.Workplane("XZ")
                  .polyline(pts)
                  .close()
                  .revolve(segment_angle, (0, 0, 0), (0, 1, 0))
            )
            result = result.union(rib)

    # 4) wave-pattern fenestrated holes on outer surface
    if hole_count > 1 and amplitude >= 0 and max_hole_dia > 0:
        eps = 1e-6
        hole_depth = thickness + eps
        for i in range(hole_count):
            ang = segment_angle * (i / float(hole_count - 1))
            phi = math.radians(ang)
            theta = ang * (2.0 * math.pi / wavelength)
            y = amplitude * math.sin(theta)
            if abs(y) >= minor_radius:
                continue
            sinl = y / minor_radius
            cosl = math.sqrt(max(0.0, 1.0 - sinl * sinl))
            rloc = major_radius + minor_radius * cosl
            x0 = rloc * math.cos(phi)
            z0 = rloc * math.sin(phi)
            nx = math.cos(phi) * cosl
            ny = sinl
            nz = math.sin(phi) * cosl
            dia = min_hole_dia + (max_hole_dia - min_hole_dia) * (1 - abs(y) / minor_radius)
            hole_r = dia * 0.5
            cutter = (
                cq.Workplane(cq.Plane(origin=(x0, y, z0), normal=(nx, ny, nz)))
                  .circle(hole_r)
                  .extrude(-hole_depth)
            )
            result = result.cut(cutter)

    # 5) fillet
    if fillet_radius > 0:
        try:
            result = result.edges().fillet(fillet_radius)
        except Exception:
            pass

    return result
