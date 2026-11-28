def sphere_with_parametric_bumps_and_circular_spikes(
    sphere_radius: float = 10.0,
    bump_count: int = 50,
    min_bump_radius: float = 1.0,
    max_bump_radius: float = 2.0,
    embed_ratio: float = 0.5,
    min_bump_distance: float = None,
    spike_ring_count: int = 36,
    spike_length: float = 1.0,
    spike_base_radius: float = 0.5,
    spike_tip_radius: float = 0.1,
    seed: int = None
) -> "cq.Workplane":
    """
    Abstract: Sphere patterned with variable bumps and circular spikes.
    Detailed: Places bumps on the sphere surface using Poisson-disc sampling to ensure
    a minimum distance between bump centers, then adds a ring of conical spikes
    around the equator.
    Parameters:
        sphere_radius       -- Radius of the base sphere (> 0).
        bump_count          -- Number of bumps to place (integer >= 0).
        min_bump_radius     -- Minimum radius of each bump (> 0).
        max_bump_radius     -- Maximum radius of each bump (>= min_bump_radius).
        embed_ratio         -- Fraction of bump radius embedded into the sphere (0 <= embed_ratio <= 1).
        min_bump_distance   -- Minimum chord distance between bump centers (>= 0). If None defaults to 2*max_bump_radius.
        spike_ring_count    -- Number of spikes around the equator (integer >= 1).
        spike_length        -- Length of each spike (>= 0).
        spike_base_radius   -- Base radius of each conical spike (>= 0).
        spike_tip_radius    -- Tip radius of each conical spike (>= 0).
        seed                -- Random seed for reproducibility (int or None).
    Returns:
        cq.Workplane: The sphere with bumps and spikes.
    """
    import cadquery as cq, math, random

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be > 0")
    if not isinstance(bump_count, int) or bump_count < 0:
        raise ValueError("bump_count must be an integer >= 0")
    if min_bump_radius <= 0:
        raise ValueError("min_bump_radius must be > 0")
    if max_bump_radius < min_bump_radius:
        raise ValueError("max_bump_radius must be >= min_bump_radius")
    if embed_ratio < 0 or embed_ratio > 1:
        raise ValueError("embed_ratio must be between 0 and 1")
    if min_bump_distance is None:
        min_bump_distance = 2.0 * max_bump_radius
    if min_bump_distance < 0:
        raise ValueError("min_bump_distance must be >= 0")
    if not isinstance(spike_ring_count, int) or spike_ring_count < 1:
        raise ValueError("spike_ring_count must be an integer >= 1")
    if spike_length < 0:
        raise ValueError("spike_length must be >= 0")
    if spike_base_radius < 0:
        raise ValueError("spike_base_radius must be >= 0")
    if spike_tip_radius < 0:
        raise ValueError("spike_tip_radius must be >= 0")
    if seed is not None and not isinstance(seed, int):
        raise ValueError("seed must be an int or None")

    # Initialize random seed
    if seed is not None:
        random.seed(seed)

    # 1) Base sphere
    result = cq.Workplane("XY").sphere(sphere_radius)

    # 2) Poisson-disc sampling for bump centers on sphere surface
    spots = []  # list of (x,y,z,nx,ny,nz)
    attempts = 0
    max_attempts = bump_count * 20 if bump_count > 0 else 0
    while len(spots) < bump_count and attempts < max_attempts:
        attempts += 1
        # pick random point on unit sphere
        u = random.random()
        v = random.random()
        cos_t = 2.0 * u - 1.0
        sin_t = math.sqrt(max(0.0, 1.0 - cos_t * cos_t))
        phi = 2.0 * math.pi * v
        nx = sin_t * math.cos(phi)
        ny = sin_t * math.sin(phi)
        nz = cos_t
        # surface position
        x = sphere_radius * nx
        y = sphere_radius * ny
        z = sphere_radius * nz
        # enforce minimum distance
        ok = True
        for x2, y2, z2, _, _, _ in spots:
            dx = x - x2
            dy = y - y2
            dz = z - z2
            if dx*dx + dy*dy + dz*dz < min_bump_distance*min_bump_distance:
                ok = False
                break
        if ok:
            spots.append((x, y, z, nx, ny, nz))

    # 3) Add spherical-cap bumps
    for x, y, z, nx, ny, nz in spots:
        # choose bump size
        r = random.uniform(min_bump_radius, max_bump_radius)
        # position plane so that bump is embedded by embed_ratio
        center_offset = sphere_radius - embed_ratio * r
        ox = center_offset * nx
        oy = center_offset * ny
        oz = center_offset * nz
        plane = cq.Plane(origin=(ox, oy, oz), normal=(nx, ny, nz))
        # full sphere bump
        full = cq.Workplane(plane).sphere(r)
        # cutter block to remove half
        cutter = cq.Workplane(plane).rect(2*r, 2*r).extrude(-2*r)
        hemi = full.cut(cutter)
        result = result.union(hemi)

    # 4) Add a ring of conical spikes around the equator
    if spike_length > 0 and spike_base_radius > 0:
        overlap = 1e-6
        ext = spike_length + overlap
        # initial spike at +X equator
        plane0 = cq.Plane(origin=(sphere_radius, 0.0, 0.0), normal=(1.0, 0.0, 0.0))
        spike = (
            cq.Workplane(plane0)
              .circle(spike_base_radius)
              .workplane(offset=ext)
              .circle(spike_tip_radius)
              .loft(combine=True)
        ).translate((-overlap, 0.0, 0.0))
        # replicate around Z axis
        for k in range(spike_ring_count):
            angle = 360.0 * k / spike_ring_count
            spike_rot = spike.rotate((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), angle)
            result = result.union(spike_rot)

    return result
