def sphere_with_gaussian_bumps_and_wave_ribs(
    sphere_radius: float = 10.0,
    num_bumps: int = 50,
    bump_radius_mean: float = 1.0,
    bump_radius_std: float = 0.2,
    bump_height_mean: float = 0.5,
    bump_height_std: float = 0.1,
    seed: int = None,
    rib_width_angle: float = 20.0,
    rib_height: float = 1.0,
    wave_count: int = 8,
    samples: int = 180,
    rib_latitudes: list = None,
    fillet_radius: float = 0.1
) -> "cq.Workplane":
    import cadquery as cq, random, math

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be > 0")
    if not isinstance(num_bumps, int) or num_bumps < 0:
        raise ValueError("num_bumps must be integer >= 0")
    if bump_radius_mean <= 0:
        raise ValueError("bump_radius_mean must be > 0")
    if bump_radius_std < 0:
        raise ValueError("bump_radius_std must be >= 0")
    if bump_height_mean <= 0:
        raise ValueError("bump_height_mean must be > 0")
    if bump_height_std < 0:
        raise ValueError("bump_height_std must be >= 0")
    if seed is not None and not isinstance(seed, int):
        raise ValueError("seed must be int or None")
    if rib_width_angle <= 0 or rib_width_angle >= 180:
        raise ValueError("rib_width_angle must be >0 and <180")
    if rib_height < 0:
        raise ValueError("rib_height must be >= 0")
    if not isinstance(wave_count, int) or wave_count < 1:
        raise ValueError("wave_count must be integer >= 1")
    if not isinstance(samples, int) or samples < 3:
        raise ValueError("samples must be integer >= 3")
    if rib_latitudes is not None:
        if not isinstance(rib_latitudes, (list, tuple)) or len(rib_latitudes) < 1:
            raise ValueError("rib_latitudes must be a list of one or more floats")
        for lat in rib_latitudes:
            if not isinstance(lat, (int, float)):
                raise ValueError("each rib_latitude must be a number")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # Seed randomness
    if seed is not None:
        random.seed(seed)

    # 1) Build base sphere and apply Gaussian bumps
    result = cq.Workplane("XY").sphere(sphere_radius)
    for _ in range(num_bumps):
        # sample bump base radius and height
        r = random.gauss(bump_radius_mean, bump_radius_std)
        while r <= 0:
            r = random.gauss(bump_radius_mean, bump_radius_std)
        h0 = random.gauss(bump_height_mean, bump_height_std)
        while h0 <= 0:
            h0 = random.gauss(bump_height_mean, bump_height_std)

        # random direction on sphere
        cos_t = random.uniform(-1.0, 1.0)
        sin_t = math.sqrt(max(0.0, 1.0 - cos_t * cos_t))
        phi = random.uniform(0.0, 2.0 * math.pi)
        nx = sin_t * math.cos(phi)
        ny = sin_t * math.sin(phi)
        nz = cos_t

        # compute small-sphere radius and center
        r_small = (r * r + h0 * h0) / (2.0 * h0)
        sx, sy, sz = (nx * sphere_radius, ny * sphere_radius, nz * sphere_radius)
        cx = sx - (r_small - h0) * nx
        cy = sy - (r_small - h0) * ny
        cz = sz - (r_small - h0) * nz

        # place bump by cut+union
        plane = cq.Plane(origin=(cx, cy, cz), normal=(nx, ny, nz))
        bump_full = cq.Workplane(plane).sphere(r_small)
        result = result.cut(bump_full).union(bump_full)

    # 2) Determine latitudes for ribs
    if rib_latitudes is None:
        rib_latitudes = [0.0]

    # 3) Build a single ring-shaped wave rib profile in XZ-plane
    segs = samples
    half_wrad = math.radians(rib_width_angle) / 2.0
    angle_span = 2.0 * half_wrad
    embed = 1e-3  # small inset for inner profile

    outer_pts = []
    for j in range(segs + 1):
        t = j / segs
        phi_local = -half_wrad + t * angle_span
        # smooth window weight
        w = 0.5 * (math.cos(math.pi * phi_local / half_wrad) + 1.0) if half_wrad > 0 else 1.0
        wave_val = math.sin(2.0 * math.pi * wave_count * t)
        # offset from base sphere
        offset = rib_height * w * 0.5 * (1.0 + wave_val)
        r_out = sphere_radius + offset
        outer_pts.append((r_out * math.cos(phi_local), r_out * math.sin(phi_local)))

    inner_pts = []
    for j in range(segs, -1, -1):
        t = j / segs
        phi_local = -half_wrad + t * angle_span
        r_in = sphere_radius - embed
        inner_pts.append((r_in * math.cos(phi_local), r_in * math.sin(phi_local)))

    profile = outer_pts + inner_pts

    # create the ring by revolving the 2D profile around Z
    wave_shell = cq.Workplane("XZ").polyline(profile).close() \
                    .revolve(360.0, axisStart=(0, 0, 0), axisEnd=(0, 0, 1))

    # 4) Place and fuse ribs at each latitude
    for lat in rib_latitudes:
        rib = wave_shell.rotate((0, 0, 0), (1, 0, 0), lat)
        result = result.union(rib, clean=True)

    # 5) Optional fillet on all edges to smooth transitions
    if fillet_radius > 0:
        try:
            result = result.edges().fillet(fillet_radius)
        except Exception:
            pass

    return result
