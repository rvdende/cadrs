def spur_gear_with_diagonal_cutouts_with_gusseted_web(
    n_teeth: int = 16,
    module: float = 2.0,
    thickness: float = 6.0,
    slot_width: float = 0.5,
    slot_depth: float = None,
    relief_angle: float = 30.0,
    fillet_radius: float = None,
    web_thickness: float = None,
    gusset_length: float = None,
    gusset_width: float = None,
    gusset_fillet: float = 0.2
):
    import cadquery as cq
    import math

    # Basic gear parameters
    pitch_dia = module * n_teeth
    r_pitch = pitch_dia / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_outer = r_pitch + addendum
    r_root = r_pitch - dedendum

    # Slot defaults and fillet clamp
    if slot_depth is None:
        slot_depth = dedendum * 0.5
    if fillet_radius is None:
        fillet_radius = slot_width * 0.5
    max_rad = min(slot_width, slot_depth) * 0.45
    rad = min(fillet_radius, max_rad) if fillet_radius and fillet_radius > 0 else 0.0

    # Web/gusset defaults
    if web_thickness is None:
        web_thickness = slot_width
    web_thickness = max(0.0, min(web_thickness, slot_width))
    if gusset_length is None:
        gusset_length = slot_depth * 0.2
    gusset_length = max(0.0, min(gusset_length, slot_depth / 2.0))
    if gusset_width is None:
        gusset_width = slot_width * 1.5

    # Blank gear body
    gear = cq.Workplane("XY").circle(r_root).extrude(thickness)

    # Add trapezoidal teeth
    half_ang = math.pi / (2.0 * n_teeth)
    for i in range(n_teeth):
        theta = 2.0 * math.pi * i / n_teeth
        a1 = theta - half_ang
        a2 = theta + half_ang
        pts = [
            (r_root * math.cos(a1), r_root * math.sin(a1)),
            (r_outer * math.cos(a1), r_outer * math.sin(a1)),
            (r_outer * math.cos(a2), r_outer * math.sin(a2)),
            (r_root * math.cos(a2), r_root * math.sin(a2)),
        ]
        tooth = cq.Workplane("XY").polyline(pts).close().extrude(thickness)
        gear = gear.union(tooth)

    # Subtract diagonal relief slots
    for i in range(n_teeth):
        total_ang = 360.0 * (i + 0.5) / n_teeth + relief_angle
        ang_rad = math.radians(total_ang)
        cx = (r_root - slot_depth / 2.0) * math.cos(ang_rad)
        cy = (r_root - slot_depth / 2.0) * math.sin(ang_rad)
        cutter = (
            cq.Workplane("XY")
              .transformed(offset=(cx, cy, 0), rotate=(0, 0, total_ang))
              .box(slot_depth, slot_width, thickness, centered=(True, True, False))
        )
        if rad > 1e-6:
            try:
                cutter = cutter.edges("|Z").fillet(rad)
            except Exception:
                pass
        gear = gear.cut(cutter)

    # Add gusseted web across each slot
    web_depth = slot_depth - 2.0 * gusset_length
    r_web_center = r_root - slot_depth / 2.0
    if web_thickness > 1e-6 and web_depth > 1e-6:
        for i in range(n_teeth):
            total_ang = 360.0 * (i + 0.5) / n_teeth + relief_angle
            ang_rad = math.radians(total_ang)
            cxw = r_web_center * math.cos(ang_rad)
            cyw = r_web_center * math.sin(ang_rad)
            # Rectangular web
            web = (
                cq.Workplane("XY")
                  .transformed(offset=(cxw, cyw, 0), rotate=(0, 0, total_ang))
                  .box(web_depth, web_thickness, thickness, centered=(True, True, False))
            )
            gear = gear.union(web)
            # Triangular gussets
            if gusset_length > 1e-6 and gusset_width > 1e-6:
                half_gw = gusset_width / 2.0
                pts_outer = [
                    (web_depth / 2.0, -half_gw),
                    (web_depth / 2.0,  half_gw),
                    (web_depth / 2.0 + gusset_length, 0.0),
                ]
                outer = (
                    cq.Workplane("XY")
                      .transformed(offset=(cxw, cyw, 0), rotate=(0, 0, total_ang))
                      .polyline(pts_outer)
                      .close()
                      .extrude(thickness)
                )
                pts_inner = [
                    (-web_depth / 2.0, -half_gw),
                    (-web_depth / 2.0,  half_gw),
                    (-web_depth / 2.0 - gusset_length, 0.0),
                ]
                inner = (
                    cq.Workplane("XY")
                      .transformed(offset=(cxw, cyw, 0), rotate=(0, 0, total_ang))
                      .polyline(pts_inner)
                      .close()
                      .extrude(thickness)
                )
                if gusset_fillet and gusset_fillet > 1e-6:
                    try:
                        outer = outer.edges("|Z").fillet(gusset_fillet)
                    except Exception:
                        pass
                    try:
                        inner = inner.edges("|Z").fillet(gusset_fillet)
                    except Exception:
                        pass
                gear = gear.union(outer).union(inner)

    return gear
