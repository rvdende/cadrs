def gear_segment_with_geodesic_reinforced_web(
    n_teeth: int = 16,
    module: float = 2.0,
    thickness: float = 6.0,
    segment_angle: float = 90.0,
    hole_radius: float = None,
    web_inner_radius: float = None,
    web_radial_count: int = 4,
    web_angular_count: int = 8,
    beam_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    from cadquery import Vector
    import math

    # 1) basic gear radii
    pitch_dia = module * n_teeth
    r_pitch = pitch_dia / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_root = r_pitch - dedendum
    r_outer = r_pitch + addendum
    circular_pitch = math.pi * module
    half_tooth = circular_pitch / 4.0

    # 2) root‐circle wedge
    theta = math.radians(segment_angle)
    segment = (
        cq.Workplane("XY")
          .moveTo(0, 0)
          .lineTo(r_root, 0)
          .threePointArc(
              (r_root * math.cos(theta/2), r_root * math.sin(theta/2)),
              (r_root * math.cos(theta),   r_root * math.sin(theta))
          )
          .close()
          .extrude(thickness)
    )

    # 3) tooth profile
    tooth_profile = [
        (r_root, -half_tooth),
        (r_outer, -half_tooth),
        (r_outer,  half_tooth),
        (r_root,  half_tooth),
    ]

    # 4) place teeth
    teeth_in_seg = int(n_teeth * segment_angle / 360.0 + 1e-6)
    for i in range(teeth_in_seg):
        angle = 360.0 * i / n_teeth
        tooth = (
            cq.Workplane("XY")
              .polyline(tooth_profile)
              .close()
              .extrude(thickness)
              .rotate((0, 0, 0), (0, 0, 1), angle)
        )
        segment = segment.union(tooth)

    # 5) central hole
    if hole_radius is None:
        hole_radius = module
    segment = segment.cut(
        cq.Workplane("XY").circle(hole_radius).extrude(thickness)
    )

    # 6) web radii
    if web_inner_radius is None:
        web_inner_radius = hole_radius + module
    web_outer_radius = r_root

    # 7) geodesic web
    if web_radial_count > 0 and web_angular_count > 0 and beam_radius > 0:
        # build nodes
        nodes = {}
        for i in range(web_radial_count + 1):
            ri = web_inner_radius + (web_outer_radius - web_inner_radius) * i / web_radial_count
            for j in range(web_angular_count + 1):
                ang = j * theta / web_angular_count
                nodes[(i, j)] = (ri * math.cos(ang), ri * math.sin(ang))
        # collect unique edges
        edges = set()
        for i in range(web_radial_count + 1):
            for j in range(web_angular_count + 1):
                if j < web_angular_count:
                    edges.add(((i, j), (i, j+1)))
                if i < web_radial_count:
                    edges.add(((i, j), (i+1, j)))
                if i < web_radial_count and j < web_angular_count:
                    edges.add(((i, j), (i+1, j+1)))
                if i < web_radial_count and j > 0:
                    edges.add(((i+1, j), (i, j-1)))
        # build rods
        z0 = thickness / 2.0
        for (i1, j1), (i2, j2) in edges:
            a = nodes[(i1, j1)]
            b = nodes[(i2, j2)]
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy)
            az = math.degrees(math.atan2(dy, dx))
            # plane at start, rotated to align normal with edge direction
            wp = (
                cq.Workplane("XY")
                  .transformed(offset=Vector(a[0], a[1], z0), rotate=Vector(0, 0, az))
                  .transformed(rotate=Vector(0, 90, 0))
            )
            rod = wp.circle(beam_radius).extrude(length)
            segment = segment.union(rod)

    return segment
