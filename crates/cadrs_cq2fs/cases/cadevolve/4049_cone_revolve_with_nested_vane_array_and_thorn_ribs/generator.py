def cone_revolve_with_nested_vane_array_and_thorn_ribs(
    base_radius: float = 10.0,
    top_radius: float = 1e-6,
    height: float = 20.0,
    wall_thickness: float = 1.0,
    vane_count: int = 8,
    vane_thickness: float = 1.0,
    vane_depth: float = None,
    vane_fillet: float = 0.0,
    rib_height: float = 0.5,
    rib_axial_width: float = 1.0,
    rib_tangential_thickness: float = 0.2,
    rib_fillet: float = 0.0,
    angle_offset: float = 0.0
) -> "cq.Workplane":
    import cadquery as cq
    import math
    from cadquery import Vector

    # --- Validate inputs ---
    if base_radius <= 0 or height <= 0:
        raise ValueError("base_radius and height must be > 0")
    if top_radius < 0 or top_radius > base_radius:
        raise ValueError("top_radius must be >= 0 and <= base_radius")
    if wall_thickness <= 0 or wall_thickness >= base_radius:
        raise ValueError("wall_thickness must be > 0 and < base_radius")
    if vane_count < 1:
        raise ValueError("vane_count must be >= 1")
    if vane_thickness <= 0:
        raise ValueError("vane_thickness must be > 0")
    if vane_fillet < 0:
        raise ValueError("vane_fillet must be >= 0")
    if rib_height <= 0 or rib_axial_width <= 0 or rib_tangential_thickness <= 0:
        raise ValueError("rib dimensions must be > 0")
    if rib_fillet < 0:
        raise ValueError("rib_fillet must be >= 0")

    # --- Compute inner radii ---
    inner_radius = base_radius - wall_thickness
    if inner_radius <= 0:
        raise ValueError("Resulting inner radius must be > 0")
    inner_top_radius = max(top_radius - wall_thickness, 0.0)

    # --- 1) Build outer frustum ---
    if top_radius > 0.0:
        outer_pts = [(0, 0), (base_radius, 0), (top_radius, height), (0, height)]
    else:
        outer_pts = [(0, 0), (base_radius, 0), (0, height)]
    outer = cq.Workplane("XZ").polyline(outer_pts).close().revolve(360.0)

    # --- 2) Build inner frustum and shell ---
    if inner_top_radius > 0.0:
        inner_pts = [(0, 0), (inner_radius, 0), (inner_top_radius, height), (0, height)]
    else:
        inner_pts = [(0, 0), (inner_radius, 0), (0, height)]
    inner_solid = cq.Workplane("XZ").polyline(inner_pts).close().revolve(360.0)
    shell = outer.cut(inner_solid)

    # --- 3) Nested internal vane array (two‐deep) ---
    angle_step = 360.0 / vane_count
    # determine depths
    if vane_depth is None or vane_depth <= 0.0:
        full_depth = inner_radius
    else:
        full_depth = min(inner_radius, vane_depth)
    half_depth = full_depth * 0.5

    fins = None
    # both vanes start at the shell interior
    for depth in (full_depth, half_depth):
        for i in range(vane_count):
            ang = angle_offset + angle_step * i
            phi = math.radians(ang)
            x0 = inner_radius * math.cos(phi)
            y0 = inner_radius * math.sin(phi)
            plane = (
                cq.Workplane("XY")
                  .transformed(
                      offset=Vector(x0, y0, 0.0),
                      rotate=Vector(0.0, 0.0, ang + 180.0)
                  )
            )
            vane = plane.box(depth, vane_thickness, height, centered=(False, True, False))
            if vane_fillet > 0.0:
                vane = vane.edges("|Z").fillet(vane_fillet)
            trimmed = vane.intersect(inner_solid)
            fins = trimmed if fins is None else fins.union(trimmed)

    result = shell.union(fins) if fins is not None else shell

    # --- 4) External thorn‐shaped ribs (alternating) ---
    mid_z = height * 0.5
    r_mid = base_radius + (top_radius - base_radius) * (mid_z / height)
    half_ax = rib_axial_width * 0.5
    rib_pts = [
        (r_mid,               mid_z - half_ax),
        (r_mid + rib_height,  mid_z),
        (r_mid,               mid_z + half_ax),
    ]
    rib_template = cq.Workplane("XZ").polyline(rib_pts).close().extrude(rib_tangential_thickness)
    if rib_fillet > 0.0:
        rib_template = rib_template.edges().fillet(rib_fillet)

    for i in range(vane_count):
        angle_rib = angle_offset + angle_step * (i + 0.5)
        rib = rib_template.rotate((0, 0, 0), (0, 0, 1), angle_rib)
        result = result.union(rib)

    return result
