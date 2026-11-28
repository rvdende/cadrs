def sphere_basic_with_partial_spherical_mesh(
    sphere_radius: float = 10.0,
    cap_angle: float = 45.0,
    mesh_size: float = 1.0,
    mesh_depth: float = 0.5,
    fillet_radius: float = 0.1
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be > 0")
    if cap_angle <= 0 or cap_angle > 90:
        raise ValueError("cap_angle must be > 0 and ≤ 90 degrees")
    if mesh_size <= 0:
        raise ValueError("mesh_size must be > 0")
    if mesh_depth <= 0 or mesh_depth >= sphere_radius:
        raise ValueError("mesh_depth must be > 0 and < sphere_radius")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be ≥ 0")

    # Compute cap geometry
    cap_rad = math.radians(cap_angle)
    z_cap = sphere_radius * math.cos(cap_rad)
    r_circle = sphere_radius * math.sin(cap_rad)
    ext_depth = (sphere_radius - z_cap) + mesh_depth

    # Base sphere
    base_sphere = cq.Workplane("XY").sphere(sphere_radius)

    # Create all triangular cutters and union them into one cutter
    plane = cq.Plane(origin=(0, 0, sphere_radius), normal=(0, 0, 1))
    s = mesh_size
    h_tri = s * math.sqrt(3.0) / 2.0
    max_row = int(math.ceil(r_circle / h_tri))
    max_col = int(math.ceil(r_circle / s)) + 1
    cutters = []
    for ri in range(-max_row, max_row + 1):
        y_base = ri * h_tri
        x_offset = (ri % 2) * (s / 2.0)
        for ci in range(-max_col, max_col + 1):
            x_base = ci * s + x_offset
            for dy in (h_tri, -h_tri):
                v1 = (x_base, y_base)
                v2 = (x_base + s / 2.0, y_base + dy)
                v3 = (x_base - s / 2.0, y_base + dy)
                if (
                    v1[0] ** 2 + v1[1] ** 2 <= r_circle**2
                    and v2[0] ** 2 + v2[1] ** 2 <= r_circle**2
                    and v3[0] ** 2 + v3[1] ** 2 <= r_circle**2
                ):
                    cutter = (
                        cq.Workplane(plane)
                        .polyline([v1, v2, v3])
                        .close()
                        .extrude(-ext_depth)
                    )
                    cutters.append(cutter)

    if cutters:
        union_cutter = cutters[0]
        for c in cutters[1:]:
            union_cutter = union_cutter.union(c)
        result = base_sphere.cut(union_cutter)
    else:
        result = base_sphere

    # Fillet edges if requested
    if fillet_radius > 0:
        result = result.edges().fillet(fillet_radius)

    return result
