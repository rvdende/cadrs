def cylinder_with_mushroom_fin_array_and_thickened_ribs(
    inner_radius: float = 5.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    fin_count: int = 8,
    fin_thickness: float = 0.5,
    fin_extra_thickness: float = 0.2,
    stem_length: float = 3.0,
    cap_thickness: float = 0.5,
    cap_diameter: float = 3.0,
    fin_height: float = None,
    rib_thickness: float = 0.3,
    rib_height: float = None,
    fillet_radius: float = 0.1
) -> cq.Workplane:
    import math
    """
    Cylinder with thickened conformal mushroom fins over an upper band,
    plus vertical ribs connecting each fin base just below the rim.
    """
    # Parameter validation
    if inner_radius < 0 or wall_thickness <= 0 or height <= 0:
        raise ValueError("inner_radius>=0, wall_thickness>0, height>0 required")
    fin_count = int(fin_count)
    if fin_count < 0:
        raise ValueError("fin_count must be a non-negative integer")
    if any(x < 0 for x in [fin_thickness, fin_extra_thickness, stem_length, cap_thickness, cap_diameter]):
        raise ValueError("fin_thickness, fin_extra_thickness, stem_length, cap_thickness, cap_diameter must be >= 0")
    if fin_height is None:
        fin_height = height / 2.0
    if fin_height <= 0 or fin_height > height:
        raise ValueError("fin_height must be > 0 and <= height")
    if rib_thickness < 0:
        raise ValueError("rib_thickness must be >= 0")
    if rib_height is None:
        rib_height = fin_height
    if rib_height < 0 or rib_height > fin_height:
        raise ValueError("rib_height must be >= 0 and <= fin_height")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # Compute radii
    outer_radius = inner_radius + wall_thickness

    # 1) Base cylinder and bore
    result = cq.Workplane("XY").circle(outer_radius).extrude(height)
    if inner_radius > 0:
        result = result.faces(">Z").workplane().circle(inner_radius).cutThruAll()

    # 2) Prepare fin dimensions
    eff_fin_thk = fin_thickness + 2.0 * fin_extra_thickness
    eff_cap_radius = cap_diameter / 2.0 + fin_extra_thickness

    # 3) Add each fin over upper fin_height band
    for i in range(fin_count):
        angle = 360.0 * i / fin_count
        fin = (
            cq.Workplane("YZ")
              .rect(eff_fin_thk, fin_height)
              .extrude(stem_length, combine=False)
        )
        if cap_thickness > 0 and cap_diameter > 0:
            cap = fin.faces(">X").workplane().circle(eff_cap_radius).extrude(cap_thickness, combine=False)
            fin = fin.union(cap)
        # position fin in the upper band just below the top face
        fin = fin.translate((outer_radius, 0, height - fin_height / 2.0))
        fin = fin.rotate((0, 0, 0), (0, 0, 1), angle)
        result = result.union(fin, clean=True)

    # 4) Add vertical ribs between adjacent fin bases near the top
    if fin_count > 1 and rib_thickness > 0 and rib_height > 0:
        rib_wp = cq.Workplane("XY").workplane(offset=height - rib_height)
        for i in range(fin_count):
            a1 = 2.0 * math.pi * i / fin_count
            a2 = 2.0 * math.pi * ((i + 1) % fin_count) / fin_count
            p1 = (outer_radius * math.cos(a1), outer_radius * math.sin(a1))
            p2 = (outer_radius * math.cos(a2), outer_radius * math.sin(a2))
            dx, dy = p2[0] - p1[0], p2[1] - p1[1]
            L = math.hypot(dx, dy)
            if L <= 1e-12:
                continue
            ux, uy = dx / L, dy / L
            px, py = -uy, ux
            w = rib_thickness / 2.0
            verts = [
                (p1[0] + px * w, p1[1] + py * w),
                (p2[0] + px * w, p2[1] + py * w),
                (p2[0] - px * w, p2[1] - py * w),
                (p1[0] - px * w, p1[1] - py * w),
            ]
            rib = rib_wp.polyline(verts).close().extrude(rib_height, combine=False)
            if fillet_radius > 0:
                rib = rib.edges().fillet(fillet_radius)
            result = result.union(rib, clean=True)

    return result
