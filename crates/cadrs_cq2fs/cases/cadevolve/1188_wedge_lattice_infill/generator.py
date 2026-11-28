def wedge_lattice_infill(L: float = 30.0,
                         W: float = 20.0,
                         H: float = 15.0,
                         branch_thickness: float = 2.0,
                         num_seed_points: int = 30,
                         neighbor_count: int = 3,
                         seed: int = 0) -> "cq.Workplane":
    import cadquery as cq, math, random

    # Parameter validation
    if L <= 0 or W <= 0 or H <= 0:
        raise ValueError("L, W, and H must be positive")
    if branch_thickness <= 0:
        raise ValueError("branch_thickness must be positive")
    num_seed_points = int(num_seed_points)
    if num_seed_points < 1:
        raise ValueError("num_seed_points must be an integer >= 1")
    neighbor_count = int(neighbor_count)
    if neighbor_count < 1:
        raise ValueError("neighbor_count must be an integer >= 1")

    # 1) Build the basic wedge
    wedge_base = (
        cq.Workplane("XZ")
          .polyline([(0.0, 0.0), (L, 0.0), (0.0, H)])
          .close()
          .extrude(W)
    )

    # 2) Hollow the wedge to create an outer shell
    shell = wedge_base.faces(">Y or <Y").shell(-branch_thickness)

    # 3) Generate random seed points inside the triangular profile
    random.seed(seed)
    pts = []
    for _ in range(num_seed_points):
        u = random.random()
        v = random.random()
        if u + v > 1.0:
            u, v = 1.0 - u, 1.0 - v
        pts.append((u * L, v * H))
    # anchor seeds at the triangle vertices
    pts.extend([(0.0, 0.0), (L, 0.0), (0.0, H)])

    # 4) Compute nearest-neighbor segments
    segments = []
    npts = len(pts)
    for i, (x1, z1) in enumerate(pts):
        dlist = []
        for j in range(i + 1, npts):
            x2, z2 = pts[j]
            d = (x2 - x1)**2 + (z2 - z1)**2
            dlist.append((d, j))
        dlist.sort(key=lambda t: t[0])
        for k in range(min(neighbor_count, len(dlist))):
            j = dlist[k][1]
            segments.append(((x1, z1), pts[j]))

    # 5) Build the branch rods and collect those inside the wedge volume
    half_t = branch_thickness * 0.5
    rods = None
    for (x1, z1), (x2, z2) in segments:
        dx, dz = x2 - x1, z2 - z1
        length = math.hypot(dx, dz)
        if length < 1e-6:
            continue
        mid_x = 0.5 * (x1 + x2)
        mid_z = 0.5 * (z1 + z2)
        angle = math.degrees(math.atan2(dz, dx))
        rod = (
            cq.Workplane("XZ")
              .polyline([
                  (-length/2.0,  half_t),
                  ( length/2.0,  half_t),
                  ( length/2.0, -half_t),
                  (-length/2.0, -half_t),
              ])
              .close()
              .extrude(W)
              .rotate((0, 0, 0), (0, 1, 0), angle)
              .translate((mid_x, 0.0, mid_z))
        )
        inside = rod.intersect(wedge_base)
        rods = inside if rods is None else rods.union(inside)

    # 6) Combine shell and infill rods into one solid body
    result = shell.union(rods, clean=True) if rods is not None else shell
    return result
