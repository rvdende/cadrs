def wedge_lattice_infill_with_gradient_thickness(
    L: float = 30.0,
    W: float = 20.0,
    H: float = 15.0,
    shell_thickness: float = 2.0,
    branch_thickness_min: float = 0.5,
    branch_thickness_max: float = 2.5,
    num_seed_points: int = 30,
    neighbor_count: int = 3,
    seed: int = 0
) -> "cq.Workplane":
    import cadquery as cq, math, random

    # Validate inputs
    if L <= 0 or W <= 0 or H <= 0:
        raise ValueError("L, W, and H must be positive")
    if shell_thickness <= 0 or shell_thickness >= min(L, H):
        raise ValueError("shell_thickness must be >0 and less than min(L, H)")
    if branch_thickness_min <= 0 or branch_thickness_max < branch_thickness_min:
        raise ValueError("branch_thickness_min must be >0 and <= branch_thickness_max")
    num_seed_points = int(num_seed_points)
    if num_seed_points < 1:
        raise ValueError("num_seed_points must be >= 1")
    neighbor_count = int(neighbor_count)
    if neighbor_count < 1:
        raise ValueError("neighbor_count must be >= 1")

    # 1) Build the base wedge
    wedge_base = (
        cq.Workplane("XZ")
          .polyline([(0.0, 0.0), (L, 0.0), (0.0, H)])
          .close()
          .extrude(W)
    )

    # 2) Shell the wedge (hollow out the two Y-faces)
    shell = wedge_base.faces(">Y or <Y").shell(-shell_thickness)

    # 3) Generate random seed points inside the triangular profile
    random.seed(seed)
    pts = []
    for _ in range(num_seed_points):
        u = random.random()
        v = random.random()
        if u + v > 1.0:
            u, v = 1.0 - u, 1.0 - v
        pts.append((u * L, v * H))
    pts.extend([(0.0, 0.0), (L, 0.0), (0.0, H)])

    # 4) Compute nearest‐neighbor segments
    segments = []
    for i, (x1, z1) in enumerate(pts):
        dlist = []
        for j in range(i + 1, len(pts)):
            x2, z2 = pts[j]
            d = (x2 - x1) ** 2 + (z2 - z1) ** 2
            dlist.append((d, j))
        dlist.sort(key=lambda t: t[0])
        for k in range(min(neighbor_count, len(dlist))):
            _, j = dlist[k]
            segments.append(((x1, z1), pts[j]))

    # 5) Prepare segments data and find max radial distance
    seg_data = []
    for (x1, z1), (x2, z2) in segments:
        dx, dz = x2 - x1, z2 - z1
        length = math.hypot(dx, dz)
        if length < 1e-6:
            continue
        mid_x = 0.5 * (x1 + x2)
        mid_z = 0.5 * (z1 + z2)
        dist_mid = math.hypot(mid_x, mid_z)
        seg_data.append((x1, z1, x2, z2, length, mid_x, mid_z, dist_mid))
    if not seg_data:
        return shell
    max_radial = max(d[-1] for d in seg_data)

    # 6) Build gradient‐thickness rods and collect those inside the wedge
    rods = None
    for x1, z1, x2, z2, length, mid_x, mid_z, dist_mid in seg_data:
        t_val = branch_thickness_min + (branch_thickness_max - branch_thickness_min) * (dist_mid / max_radial)
        half_t = 0.5 * t_val
        angle = math.degrees(math.atan2(z2 - z1, x2 - x1))
        rod = (
            cq.Workplane("XZ")
              .polyline([
                  (-length / 2.0,  half_t),
                  ( length / 2.0,  half_t),
                  ( length / 2.0, -half_t),
                  (-length / 2.0, -half_t),
              ])
              .close()
              .extrude(W)
              .rotate((0, 0, 0), (0, 1, 0), angle)
              .translate((mid_x, 0.0, mid_z))
        )
        inside = rod.intersect(wedge_base)
        rods = inside if rods is None else rods.union(inside)

    # 7) Union shell and infill
    result = shell.union(rods, clean=True)
    return result
