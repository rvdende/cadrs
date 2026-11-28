def cylinder_with_wave_ripple_surface_and_spoke_web_and_countersunk_mounts(
    resolution=360,
    radius=10.0,
    amplitude=0.5,
    wave_count=6,
    height=5.0,
    spoke_thickness=0.5,
    web_z_thickness=3.0,
    spoke_count=6,
    hole_count=6,
    pattern_radius=None,
    hole_diameter=2.5,
    countersink_diameter=5.0,
    countersink_angle=82.0,
    row_count=2,
    fillet_radius=0.2,
):
    import cadquery as cq, math

    # Parameter validation
    if resolution < 3:
        raise ValueError("resolution must be at least 3")
    if radius <= 0 or height <= 0:
        raise ValueError("radius and height must be positive")
    if amplitude < 0:
        raise ValueError("amplitude must be non-negative")
    if wave_count < 1:
        raise ValueError("wave_count must be at least 1")
    if spoke_thickness <= 0 or web_z_thickness <= 0:
        raise ValueError("spoke_thickness and web_z_thickness must be positive")
    if spoke_count < 1:
        raise ValueError("spoke_count must be at least 1")
    if amplitude + spoke_thickness / 2.0 >= radius:
        raise ValueError("amplitude + half spoke_thickness must be less than radius")
    if hole_count < 0 or hole_diameter <= 0:
        raise ValueError("hole_count must be >= 0 and hole_diameter positive")
    if countersink_diameter <= 0 or not (0 < countersink_angle < 180):
        raise ValueError("invalid countersink parameters")
    if row_count < 1 or row_count > 2:
        raise ValueError("row_count must be 1 or 2")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non-negative")

    # 1) Build wavy outer cylinder
    pts = []
    for i in range(resolution):
        theta = 2.0 * math.pi * i / resolution
        r_mod = radius + amplitude * math.sin(wave_count * theta)
        pts.append((r_mod * math.cos(theta), r_mod * math.sin(theta)))
    wavy_cyl = cq.Workplane("XY").polyline(pts).close().extrude(height)

    # 2) Hollow core just enough to preserve troughs
    half_spoke = spoke_thickness / 2.0
    inner_rad = max(radius - amplitude - half_spoke, 0.0)
    inner_cyl = cq.Workplane("XY").circle(inner_rad).extrude(height)
    shell = wavy_cyl.cut(inner_cyl)

    # 3) Create radial internal webs
    z_start = (height - web_z_thickness) / 2.0
    webs = None
    for i in range(spoke_count):
        ang = 360.0 * i / float(spoke_count)
        web = (
            cq.Workplane("XY")
              .workplane(offset=z_start)
              .polyline([
                  (0.0,               -half_spoke),
                  (0.0,                half_spoke),
                  (radius - half_spoke,  half_spoke),
                  (radius - half_spoke, -half_spoke),
              ])
              .close()
              .extrude(web_z_thickness)
              .rotate((0, 0, 0), (0, 0, 1), ang)
        )
        webs = web if webs is None else webs.union(web)
    result = shell.union(webs) if webs is not None else shell

    # 4) Add countersunk mount holes
    if hole_count > 0:
        pr = radius * 0.75 if pattern_radius is None else pattern_radius
        pr = min(pr, radius - countersink_diameter / 2.0)
        angle_step = 2.0 * math.pi / hole_count
        angle_offset = angle_step / 2.0
        hole_positions = [
            (pr * math.cos(j * angle_step + angle_offset),
             pr * math.sin(j * angle_step + angle_offset))
            for j in range(hole_count)
        ]
        # Top face
        result = (
            cq.Workplane("XY")
              .add(result.val())
              .faces(">Z")
              .workplane()
              .pushPoints(hole_positions)
              .cskHole(hole_diameter, countersink_diameter, countersink_angle, height)
        )
        # Bottom face for second row
        if row_count == 2:
            result = (
                cq.Workplane("XY")
                  .add(result.val())
                  .faces("<Z")
                  .workplane(invert=True)
                  .pushPoints(hole_positions)
                  .cskHole(hole_diameter, countersink_diameter, countersink_angle, height)
            )

    # 5) Fillet vertical edges
    if fillet_radius > 0:
        result = result.edges("|Z").fillet(fillet_radius)

    return result
