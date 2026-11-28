def dome_with_prismatic_fins_and_flanged_lip(
    radius: float = 12.0,
    thickness: float = 2.0,
    strut_count: int = 16,
    ring_height: float = None,
    web_thickness: float = 2.0,
    vent_count: int = 16,
    vent_width: float = 2.0,
    vent_depth: float = 1.0,
    vent_height: float = 1.5,
    flange_width: float = 5.0,
    flange_length: float = 5.0,
    flange_thickness: float = 1.0,
    flange_fillet: float = 0.3,
    edge_fillet: float = 0.2,
    fin_count: int = 8,
    fin_width: float = 2.0,
    fin_depth: float = 1.0,
    fin_height: float = 2.0,
    fin_fillet: float = 0.1,
    lip_thickness: float = 1.0,
    lip_height: float = 1.0,
    lip_fillet: float = 0.7,
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # --- parameter validation ---
    if radius <= 0:
        raise ValueError(f"radius must be > 0; got {radius}")
    if thickness <= 0 or thickness >= radius:
        raise ValueError(f"thickness must be > 0 and < radius; got {thickness}")
    inner_R = radius - thickness
    if ring_height is None:
        ring_height = inner_R * 0.5
    if ring_height <= 0 or ring_height >= inner_R:
        raise ValueError(f"ring_height must be > 0 and < inner_R; got {ring_height}")
    if strut_count < 0:
        raise ValueError(f"strut_count must be >= 0; got {strut_count}")
    if web_thickness <= 0:
        raise ValueError(f"web_thickness must be > 0; got {web_thickness}")
    if vent_count < 0:
        raise ValueError(f"vent_count must be >= 0; got {vent_count}")
    if vent_width < 0 or vent_width > 2 * radius:
        raise ValueError(f"vent_width must be >= 0 and <= 2*radius; got {vent_width}")
    if vent_depth < 0 or vent_depth > radius:
        raise ValueError(f"vent_depth must be >= 0 and <= radius; got {vent_depth}")
    if vent_height < 0 or vent_height > radius:
        raise ValueError(f"vent_height must be >= 0 and <= radius; got {vent_height}")
    if flange_width < 0 or flange_length < 0 or flange_thickness < 0:
        raise ValueError(
            f"flange dimensions must be >= 0; got width={flange_width}, length={flange_length}, thickness={flange_thickness}"
        )
    if flange_fillet < 0:
        raise ValueError(f"flange_fillet must be >= 0; got {flange_fillet}")
    if edge_fillet < 0:
        raise ValueError(f"edge_fillet must be >= 0; got {edge_fillet}")
    if fin_count < 0:
        raise ValueError(f"fin_count must be >= 0; got {fin_count}")
    if fin_width < 0 or fin_depth < 0 or fin_height < 0:
        raise ValueError(
            f"fin dimensions must be >= 0; got width={fin_width}, depth={fin_depth}, height={fin_height}"
        )
    if fin_fillet < 0:
        raise ValueError(f"fin_fillet must be >= 0; got {fin_fillet}")
    if lip_thickness < 0 or lip_height < 0:
        raise ValueError(
            f"lip dimensions must be >= 0; got thickness={lip_thickness}, height={lip_height}"
        )
    if lip_fillet < 0:
        raise ValueError(f"lip_fillet must be >= 0; got {lip_fillet}")

    # --- 1) create open hemisphere ---
    outer = (
        cq.Workplane("XY")
          .sphere(radius)
          .cut(
              cq.Workplane("XY")
                .box(2 * radius, 2 * radius, radius, centered=(True, True, False))
                .translate((0, 0, -radius))
          )
    )

    # --- 2) shell inward to get thickness ---
    dome = outer.faces("<Z").shell(-thickness)

    # --- 3) internal vented web (ring + struts) ---
    r_ring = math.sqrt(max(0.0, inner_R**2 - ring_height**2))
    radial_span = inner_R - r_ring
    mid_radial = r_ring + radial_span / 2.0

    ring = (
        cq.Workplane("XY")
          .workplane(offset=ring_height)
          .circle(inner_R)
          .extrude(web_thickness / 2.0, both=True)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=ring_height)
                .circle(r_ring)
                .extrude(web_thickness / 2.0, both=True)
          )
    )

    struts = None
    for i in range(strut_count):
        phi = 360.0 * i / strut_count
        s = (
            cq.Workplane("XY")
              .transformed(rotate=(0.0, 0.0, phi))
              .center(mid_radial, -ring_height / 2.0)
              .rect(radial_span, ring_height)
              .extrude(web_thickness / 2.0, both=True)
        )
        struts = s if struts is None else struts.union(s)

    webs = ring if struts is None else ring.union(struts)
    dome = dome.union(webs)

    # --- 4) cut triangular vents ---
    if vent_count and vent_width and vent_depth and vent_height:
        half_ang = math.asin(min(1.0, vent_width / (2.0 * radius)))
        inner_vr = radius - vent_depth
        outer_vr = radius + vent_depth
        p1 = (outer_vr * math.cos(half_ang),  outer_vr * math.sin(half_ang))
        p2 = (outer_vr * math.cos(half_ang), -outer_vr * math.sin(half_ang))
        p3 = (inner_vr, 0.0)
        vents = None
        for j in range(vent_count):
            v = (
                cq.Workplane("XY")
                  .moveTo(*p1)
                  .lineTo(*p2)
                  .lineTo(*p3)
                  .close()
                  .extrude(vent_height)
                  .rotate((0, 0, 0), (0, 0, 1), 360.0 * j / vent_count)
            )
            vents = v if vents is None else vents.union(v)
        dome = dome.cut(vents)

    # --- 5) union snap-flanges ---
    flange_group = None
    for i in range(vent_count):
        if i % 2 != 0:
            continue
        ang = 360.0 * i / vent_count
        cx = radius * math.cos(math.radians(ang))
        cy = radius * math.sin(math.radians(ang))
        fl = (
            cq.Workplane("XY")
              .transformed(offset=(cx, cy, 0.0), rotate=(0.0, 0.0, ang))
              .box(flange_length, flange_width, flange_thickness, centered=(False, True, False))
        )
        if flange_fillet:
            fl = fl.edges("|Z").fillet(flange_fillet)
        flange_group = fl if flange_group is None else flange_group.union(fl)
    if flange_group:
        dome = dome.union(flange_group)

    # --- 6) union prismatic fins at web ring ---
    fin_group = None
    if fin_count:
        for i in range(fin_count):
            ang = 360.0 * i / fin_count
            f = (
                cq.Workplane("XY")
                  .transformed(rotate=(0, 0, ang))
                  .workplane(offset=ring_height + web_thickness / 2.0)
                  .center(mid_radial, 0.0)
                  .rect(fin_depth, fin_width)
                  .extrude(fin_height)
            )
            if fin_fillet:
                f = f.edges("|Z").fillet(fin_fillet)
            fin_group = f if fin_group is None else fin_group.union(f)
        dome = dome.union(fin_group)

    # --- 7) extruded lip around base ---
    if lip_thickness and lip_height:
        lip = (
            cq.Workplane("XZ")
              .moveTo(radius, 0.0)
              .hLine(lip_thickness)
              .vLine(lip_height)
              .hLine(-lip_thickness)
              .close()
              .revolve(360)
        )
        if lip_fillet:
            rf = min(lip_fillet, lip_thickness, lip_height)
            if rf:
                lip = lip.edges().fillet(rf)
        dome = dome.union(lip)

    # --- 8) fillet bottom rim ---
    if edge_fillet:
        dome = dome.faces("<Z").edges().fillet(edge_fillet)

    # --- 9) combine into one solid ---
    try:
        dome = dome.combine()
    except Exception:
        pass

    return dome
