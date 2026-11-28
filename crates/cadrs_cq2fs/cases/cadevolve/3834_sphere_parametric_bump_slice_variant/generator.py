def sphere_parametric_bump_slice_variant(
    sphere_radius: float = 10.0,
    latitudes: int = 4,
    longitudes: int = 8,
    min_bump_radius: float = 1.0,
    max_bump_radius: float = 2.0,
    embed_ratio: float = 0.5,
    slice_height: float = 5.0,
    slice_angle: float = 15.0,
    density_spread: float = 1.0,
    fillet_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq, math

    # Validate parameters
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be positive")
    if not isinstance(latitudes, int) or latitudes < 1:
        raise ValueError("latitudes must be an integer >= 1")
    if not isinstance(longitudes, int) or longitudes < 1:
        raise ValueError("longitudes must be an integer >= 1")
    if min_bump_radius <= 0:
        raise ValueError("min_bump_radius must be positive")
    if max_bump_radius < min_bump_radius:
        raise ValueError("max_bump_radius must be >= min_bump_radius")
    if embed_ratio < 0 or embed_ratio > 1:
        raise ValueError("embed_ratio must be between 0 and 1")
    if slice_height < 0 or slice_height > 2 * sphere_radius:
        raise ValueError("slice_height must be between 0 and 2*sphere_radius")
    if density_spread < 0:
        raise ValueError("density_spread must be >= 0")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")
    if not isinstance(slice_angle, (int, float)):
        raise ValueError("slice_angle must be a number")

    # compute cut‐plane parameters
    z0 = -sphere_radius + slice_height
    alpha = math.radians(slice_angle)
    # rotated plane normal components
    n_py = -math.sin(alpha)
    n_pz = math.cos(alpha)
    cutter_size = 2 * (sphere_radius + max_bump_radius + 1.0)

    # 1) Build base sphere
    result = cq.Workplane("XY").sphere(sphere_radius)

    # 2) Add bumps with density weighted by distance to the rotated cut‐plane
    for i in range(1, latitudes + 1):
        theta = math.pi * i / (latitudes + 1)
        sin_t = math.sin(theta)
        cos_t = math.cos(theta)
        frac = (i - 1) / (latitudes - 1) if latitudes > 1 else 0.5
        bump_r = min_bump_radius + (max_bump_radius - min_bump_radius) * frac
        center_off = sphere_radius - embed_ratio * bump_r
        ring_z = center_off * cos_t

        # distance from ring center to rotated plane
        dist = n_pz * (ring_z - z0)
        weight = max(0.0, 1.0 - abs(dist) / sphere_radius)
        count = max(1, int(round(longitudes * (1 + density_spread * weight))))

        for j in range(count):
            phi = 2.0 * math.pi * j / count
            nx = sin_t * math.cos(phi)
            ny = sin_t * math.sin(phi)
            nz = cos_t
            ox = center_off * nx
            oy = center_off * ny
            oz = center_off * nz

            # create hemisphere bump
            bump_plane = cq.Plane(origin=(ox, oy, oz), normal=(nx, ny, nz))
            hemi = (
                cq.Workplane(bump_plane)
                  .sphere(bump_r)
                  .cut(
                      cq.Workplane(bump_plane)
                        .rect(2 * bump_r, 2 * bump_r)
                        .extrude(-2 * bump_r)
                  )
            )
            result = result.union(hemi)

    # 3) Slice with rotated plane
    slicer = (
        cq.Workplane("XY")
          .workplane(offset=z0)
          .transformed(rotate=(slice_angle, 0, 0))
          .rect(cutter_size, cutter_size)
          .extrude(cutter_size)
    )
    result = result.cut(slicer)

    # 4) Optional fillet on all edges
    if fillet_radius > 0:
        try:
            result = result.edges().fillet(fillet_radius)
        except Exception:
            pass

    return result
