def triangular_prism_with_lattice_infill(
    A: float = 10.0,
    H: float = 25.0,
    wall_thickness: float = 1.0,
    cell_size: float = 5.0,
    strut_radius: float = 0.2,
    fillet_radius: float = None
) -> "cq.Workplane":
    import cadquery as cq, math
    # Parameter validation
    if A <= 0 or H <= 0:
        raise ValueError("A and H must be positive")
    if wall_thickness <= 0:
        raise ValueError("wall_thickness must be positive")
    if cell_size <= 0:
        raise ValueError("cell_size must be positive")
    if strut_radius <= 0:
        raise ValueError("strut_radius must be positive")
    if fillet_radius is None:
        fillet_radius = strut_radius
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non‐negative")

    # Build and shell outer prism
    sqrt3 = math.sqrt(3.0)
    tri_h = A * sqrt3 / 2.0
    pts2d = [
        (-A / 2.0, -tri_h / 3.0),
        ( A / 2.0, -tri_h / 3.0),
        (    0.0,  2.0 * tri_h / 3.0),
    ]
    prism = cq.Workplane("XY").polyline(pts2d).close().extrude(H)
    shell = prism.faces(">Z or <Z").shell(-wall_thickness)

    # Compute inner triangle for lattice
    inner_A = A - 2.0 * wall_thickness * sqrt3
    if inner_A <= 0:
        raise ValueError("wall_thickness too large for given A")
    tri_hi = inner_A * sqrt3 / 2.0
    inner_pts = [
        (-inner_A / 2.0, -tri_hi / 3.0),
        ( inner_A / 2.0, -tri_hi / 3.0),
        (     0.0,       2.0 * tri_hi / 3.0),
    ]
    # Edge vectors
    edges = []
    for i in range(3):
        x0, y0 = inner_pts[i]
        x1, y1 = inner_pts[(i + 1) % 3]
        edges.append((x1 - x0, y1 - y0))

    # Lattice parameters
    core_h = H - 2.0 * wall_thickness
    cells_edge = max(int(inner_A / cell_size), 1)
    cells_z = max(int(core_h / cell_size), 1)
    dz = core_h / float(cells_z)

    # point on edge e at (k,j)
    def get_pt(e, k, j):
        x0, y0 = inner_pts[e]
        ex, ey = edges[e]
        t = k / float(cells_edge)
        return (x0 + ex * t, y0 + ey * t, wall_thickness + j * dz)

    result = shell

    # vertical struts
    for j in range(cells_z):
        for e in range(3):
            for k in range(cells_edge + 1):
                p1 = get_pt(e, k, j)
                p2 = get_pt(e, k, j + 1)
                dx, dy, dz_ = p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]
                L = math.sqrt(dx*dx + dy*dy + dz_*dz_)
                if L < 1e-6:
                    continue
                nx, ny, nz = dx/L, dy/L, dz_/L
                rod = cq.Workplane(cq.Plane(origin=p1, normal=(nx, ny, nz))).circle(strut_radius).extrude(L)
                result = result.union(rod)

    # diagonal struts
    face_pairs = [(0,1), (1,2), (2,0)]
    for j in range(cells_z):
        for e1, e2 in face_pairs:
            for k in range(cells_edge + 1):
                for ea, eb in ((e1, e2), (e2, e1)):
                    p1 = get_pt(ea, k, j)
                    p2 = get_pt(eb, k, j + 1)
                    dx, dy, dz_ = p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]
                    L = math.sqrt(dx*dx + dy*dy + dz_*dz_)
                    if L < 1e-6:
                        continue
                    nx, ny, nz = dx/L, dy/L, dz_/L
                    rod = cq.Workplane(cq.Plane(origin=p1, normal=(nx, ny, nz))).circle(strut_radius).extrude(L)
                    result = result.union(rod)

    # fillet lattice nodes by adding small spheres
    node_positions = set(
        tuple(round(v, 6) for v in get_pt(e, k, j))
        for e in range(3)
        for j in range(cells_z + 1)
        for k in range(cells_edge + 1)
    )
    for x, y, z in node_positions:
        sph = cq.Workplane(cq.Plane(origin=(x, y, z), normal=(0, 0, 1))).sphere(fillet_radius * 2)
        result = result.union(sph)

    return result
