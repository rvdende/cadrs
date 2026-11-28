def sphere_with_gaussian_bumps_and_mount_bosses(
    sphere_radius: float = 10.0,
    num_bumps: int = 50,
    bump_radius_mean: float = 1.0,
    bump_radius_std: float = 0.2,
    bump_height_mean: float = 0.5,
    bump_height_std: float = 0.1,
    seed: int = None,
    boss_count: int = 6,
    boss_radius: float = 1.0,
    boss_height: float = 2.0,
    boss_fillet: float = 0.2,
    boss_latitude: float = 90.0,
    fin_count: int = 4,
    fin_length: float = 1.5,
    fin_width: float = 0.3
) -> "cq.Workplane":
    import cadquery as cq, math, random

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be > 0")
    if not isinstance(num_bumps, int) or num_bumps < 0:
        raise ValueError("num_bumps must be an integer >= 0")
    if bump_radius_mean <= 0:
        raise ValueError("bump_radius_mean must be > 0")
    if bump_radius_std < 0:
        raise ValueError("bump_radius_std must be >= 0")
    if bump_height_mean <= 0:
        raise ValueError("bump_height_mean must be > 0")
    if bump_height_std < 0:
        raise ValueError("bump_height_std must be >= 0")
    if seed is not None and not isinstance(seed, int):
        raise ValueError("seed must be an int or None")
    if not isinstance(boss_count, int) or boss_count < 0:
        raise ValueError("boss_count must be an integer >= 0")
    if boss_radius <= 0:
        raise ValueError("boss_radius must be > 0")
    if boss_height <= 0:
        raise ValueError("boss_height must be > 0")
    if boss_fillet < 0:
        raise ValueError("boss_fillet must be >= 0")
    if boss_latitude < 0 or boss_latitude > 180:
        raise ValueError("boss_latitude must be between 0 and 180")
    if not isinstance(fin_count, int) or fin_count < 0:
        raise ValueError("fin_count must be an integer >= 0")
    if fin_length <= 0:
        raise ValueError("fin_length must be > 0")
    if fin_width <= 0:
        raise ValueError("fin_width must be > 0")

    # helper for vector ops
    def v_cross(a, b):
        return (a[1]*b[2] - a[2]*b[1],
                a[2]*b[0] - a[0]*b[2],
                a[0]*b[1] - a[1]*b[0])
    def v_norm(a):
        l = math.sqrt(a[0]*a[0] + a[1]*a[1] + a[2]*a[2])
        return (a[0]/l, a[1]/l, a[2]/l) if l else (0.0, 0.0, 0.0)

    # Base sphere
    result = cq.Workplane("XY").sphere(sphere_radius)

    # Random seed
    if seed is not None:
        random.seed(seed)

    # Gaussian bumps
    for _ in range(num_bumps):
        r = random.gauss(bump_radius_mean, bump_radius_std)
        while r <= 0:
            r = random.gauss(bump_radius_mean, bump_radius_std)
        h = random.gauss(bump_height_mean, bump_height_std)
        while h <= 0:
            h = random.gauss(bump_height_mean, bump_height_std)

        cos_theta = random.uniform(-1.0, 1.0)
        sin_theta = math.sqrt(max(0.0, 1.0 - cos_theta*cos_theta))
        phi = random.uniform(0.0, 2.0*math.pi)
        nx = sin_theta*math.cos(phi)
        ny = sin_theta*math.sin(phi)
        nz = cos_theta

        sx, sy, sz = (nx*sphere_radius, ny*sphere_radius, nz*sphere_radius)
        r_small = (r*r + h*h) / (2.0*h)
        cx = sx - (r_small - h)*nx
        cy = sy - (r_small - h)*ny
        cz = sz - (r_small - h)*nz

        plane = cq.Plane(origin=(cx, cy, cz), normal=(nx, ny, nz))
        bump_full = cq.Workplane(plane).sphere(r_small)
        result = result.cut(bump_full).union(bump_full)

    # Mounting bosses with radial fins
    if boss_count > 0:
        eps = 1e-6
        theta_rad = math.radians(boss_latitude)
        sin_t = math.sin(theta_rad)
        cos_t = math.cos(theta_rad)
        for j in range(boss_count):
            phi_j = 2.0*math.pi*j/boss_count
            nx = sin_t*math.cos(phi_j)
            ny = sin_t*math.sin(phi_j)
            nz = cos_t
            n = (nx, ny, nz)
            ref = (0, 0, 1) if abs(nz) < 0.9 else (0, 1, 0)
            t1 = v_norm(v_cross(n, ref))
            origin = (nx*(sphere_radius - eps),
                      ny*(sphere_radius - eps),
                      nz*(sphere_radius - eps))
            plane = cq.Plane(origin=origin, xDir=t1, normal=n)

            boss = cq.Workplane(plane).circle(boss_radius).extrude(boss_height + eps)
            if boss_fillet > 0:
                boss = boss.edges().fillet(boss_fillet)

            for i in range(fin_count):
                ang = 360.0 * i / fin_count
                fin = (
                    cq.Workplane(plane)
                      .transformed(rotate=(0, 0, ang))
                      .center(boss_radius, 0)
                      .rect(fin_length, fin_width)
                      .extrude(boss_height + eps)
                )
                boss = boss.union(fin)

            result = result.union(boss)

    return result
