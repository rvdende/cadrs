def cone_revolve_with_topography_cutouts_and_ribs(
    base_radius: float = 10.0,
    top_radius: float = 1e-6,
    height: float = 20.0,
    ring_count: int = 3,
    ring_height: float = None,
    radial_vane_count: int = 16,
    vane_width: float = 0.5,
    cut_depth: float = 1.0,
    rib_thickness: float = 0.2,
    fillet_radius: float = 0.1
) -> "cq.Workplane":
    """
    Create a conical frustum with terrain‐style ring cutouts, radial vane slots,
    and reinforcing radial ribs at each ring‐vane intersection.
    Parameters:
        base_radius       – radius at z=0 (>0)
        top_radius        – radius at z=height (>=0 and <= base_radius)
        height            – cone height (>0)
        ring_count        – number of horizontal ring cutouts (>=0)
        ring_height       – axial height of each ring; if None, uses cut_depth
        radial_vane_count – number of radial slots (>=0)
        vane_width        – width of each radial slot (>0)
        cut_depth         – radial depth of ring cutouts (>0 and <= base_radius)
        rib_thickness     – radial protrusion of reinforcing ribs (>0)
        fillet_radius     – fillet radius on rib edges (>=0)
    Returns:
        A cq.Workplane of the finished solid.
    """
    import cadquery as cq
    import math
    from cadquery import Vector

    # --- Validate inputs ---
    if base_radius <= 0 or height <= 0:
        raise ValueError("base_radius and height must be > 0")
    if top_radius < 0 or top_radius > base_radius:
        raise ValueError("top_radius must be >= 0 and <= base_radius")
    ring_count = int(ring_count)
    if ring_count < 0:
        raise ValueError("ring_count must be >= 0")
    radial_vane_count = int(radial_vane_count)
    if radial_vane_count < 0:
        raise ValueError("radial_vane_count must be >= 0")
    if vane_width <= 0:
        raise ValueError("vane_width must be > 0")
    if cut_depth <= 0 or cut_depth > base_radius:
        raise ValueError("cut_depth must be > 0 and <= base_radius")
    if ring_height is None:
        ring_height = cut_depth
    if ring_height <= 0 or ring_height > height:
        raise ValueError("ring_height must be > 0 and <= height")
    if rib_thickness <= 0:
        raise ValueError("rib_thickness must be > 0")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # --- 1) Basic conical frustum ---
    cone = (
        cq.Workplane("XZ")
          .moveTo(0, 0)
          .lineTo(base_radius, 0)
          .lineTo(top_radius, height)
          .lineTo(0, height)
          .close()
          .revolve(360.0)
    )
    result = cone

    # --- 2) Ring (annular) cutouts ---
    for i in range(ring_count):
        z = height * (i + 1) / (ring_count + 1)
        rz = base_radius + (top_radius - base_radius) * (z / height)
        r_outer = rz
        r_inner = max(rz - cut_depth, 0.0)
        half_h = ring_height / 2.0
        pts = [
            (r_outer, z + half_h),
            (r_inner, z + half_h),
            (r_inner, z - half_h),
            (r_outer, z - half_h),
        ]
        cutter = cq.Workplane("XZ").polyline(pts).close().revolve(360.0)
        result = result.cut(cutter)

    # --- 3) Radial vane slots ---
    radial_length = base_radius - top_radius
    if radial_vane_count > 0 and radial_length > 0:
        ext_len = radial_length + cut_depth
        half_len = ext_len / 2.0
        cutter_height = height * 1.1
        for j in range(radial_vane_count):
            angle = 360.0 * j / radial_vane_count
            phi = math.radians(angle)
            x0 = half_len * math.cos(phi)
            y0 = half_len * math.sin(phi)
            cutter = (
                cq.Workplane("XY")
                  .transformed(
                      offset=Vector(x0, y0, height/2.0),
                      rotate=Vector(0, 0, angle)
                  )
                  .box(ext_len, vane_width, cutter_height)
            )
            result = result.cut(cutter)

    # --- 4) Reinforcing radial ribs at each ring-vane intersection ---
    if radial_vane_count > 0:
        for i in range(ring_count):
            z = height * (i + 1) / (ring_count + 1)
            rz = base_radius + (top_radius - base_radius) * (z / height)
            r_inner = max(rz - cut_depth, 0.0)
            r_outer = rz
            half_h = ring_height / 2.0
            for j in range(radial_vane_count):
                angle = 360.0 * j / radial_vane_count
                # build rib cross-section in XZ and extrude in Y
                pts = [
                    (r_inner, z - half_h),
                    (r_outer + rib_thickness, z - half_h),
                    (r_outer + rib_thickness, z + half_h),
                    (r_inner, z + half_h),
                ]
                rib = (
                    cq.Workplane("XZ")
                      .polyline(pts)
                      .close()
                      .extrude(rib_thickness)
                      .rotate((0, 0, 0), (0, 0, 1), angle)
                )
                if fillet_radius > 0:
                    try:
                        rib = rib.edges().fillet(fillet_radius)
                    except Exception:
                        pass
                result = result.union(rib)

    return result
