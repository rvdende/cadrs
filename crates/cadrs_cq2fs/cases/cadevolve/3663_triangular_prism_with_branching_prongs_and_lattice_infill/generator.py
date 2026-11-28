def triangular_prism_with_branching_prongs_and_lattice_infill(
    A: float = 10.0,
    H: float = 25.0,
    wall_thickness: float = 1.0,
    cell_size: float = 5.0,
    strut_radius: float = 0.2,
    prong_radius: float = 0.5,
    prong_length: float = None,
    levels: int = 1,
    scale: float = 0.6,
    angle: float = 45.0
) -> "cq.Workplane":
    """
    A triangular prism ornamented with outward branching prongs and infilled
    with a tetrahedral lattice. Builds an equilateral triangular prism of side A
    and height H, shells it by wall_thickness, fills the interior with a
    tetrahedral lattice of cylindrical struts (cell size cell_size,
    strut radius strut_radius), and adds fractal branching prongs from each side
    face (prong radius prong_radius, prong_length, levels, scale, angle).
    """
    import cadquery as cq
    import math

    # Validate parameters
    if A <= 0 or H <= 0:
        raise ValueError("A and H must be positive")
    if wall_thickness <= 0 or wall_thickness * 2 >= H:
        raise ValueError("wall_thickness must be positive and less than half of H")
    if cell_size <= 0:
        raise ValueError("cell_size must be positive")
    if strut_radius <= 0:
        raise ValueError("strut_radius must be positive")
    if prong_radius <= 0:
        raise ValueError("prong_radius must be positive")
    if levels < 1:
        raise ValueError("levels must be at least 1")
    if not (0 < scale < 1):
        raise ValueError("scale must be between 0 and 1")
    if not (0 < angle < 180):
        raise ValueError("angle must be between 0 and 180 degrees")

    # Default prong length
    if prong_length is None:
        prong_length = min(A, H) * 0.3
    if prong_length <= 0:
        raise ValueError("prong_length must be positive")

    # Base triangle coordinates in XY
    sqrt3 = math.sqrt(3.0)
    tri_h = A * sqrt3 / 2.0
    base_pts = [
        (-A / 2.0, -tri_h / 3.0),
        ( A / 2.0, -tri_h / 3.0),
        (  0.0,   2.0 * tri_h / 3.0),
    ]

    # Build and shell the triangular prism
    prism = cq.Workplane("XY").polyline(base_pts).close().extrude(H)
    shell = prism.faces(">Z or <Z").shell(-wall_thickness)

    # Prepare lattice parameters
    inner_A = A - 2.0 * wall_thickness * sqrt3
    if inner_A <= 0:
        raise ValueError("wall_thickness too large for given A")
    tri_hi = inner_A * sqrt3 / 2.0
    inner_pts = [
        (-inner_A / 2.0, -tri_hi / 3.0),
        ( inner_A / 2.0, -tri_hi / 3.0),
        (     0.0      ,  2.0 * tri_hi / 3.0),
    ]
    # Edge direction vectors for inner triangle
    edges = []
    for i in range(3):
        x0, y0 = inner_pts[i]
        x1, y1 = inner_pts[(i + 1) % 3]
        edges.append((x1 - x0, y1 - y0))

    # Number of cells
    cells_edge = max(int(inner_A / cell_size), 1)
    cells_z = max(int(H / cell_size), 1)
    dz = H / cells_z

    def get_pt(e: int, k: int, j: int):
        x0, y0 = inner_pts[e]
        ex, ey = edges[e]
        t = k / float(cells_edge)
        x = x0 + ex * t
        y = y0 + ey * t
        z = j * dz
        return (x, y, z)

    result = shell

    # Vertical struts
    for j in range(cells_z):
        for e in range(3):
            for k in range(cells_edge + 1):
                p1 = get_pt(e, k, j)
                p2 = get_pt(e, k, j + 1)
                dx, dy, dz_ = p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]
                L = math.sqrt(dx*dx + dy*dy + dz_*dz_)
                if L < 1e-6:
                    continue
                nx, ny, nz = dx / L, dy / L, dz_ / L
                plane = cq.Plane(origin=p1, normal=(nx, ny, nz))
                rod = cq.Workplane(plane).circle(strut_radius).extrude(L)
                result = result.union(rod)

    # Diagonal struts (tetrahedral)
    face_pairs = [(0, 1), (1, 2), (2, 0)]
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
                    nx, ny, nz = dx / L, dy / L, dz_ / L
                    plane = cq.Plane(origin=p1, normal=(nx, ny, nz))
                    rod = cq.Workplane(plane).circle(strut_radius).extrude(L)
                    result = result.union(rod)

    # Build outward branching prongs on each side face
    # Compute face-edge vectors and normals
    for i in range(3):
        x0, y0 = base_pts[i]
        x1, y1 = base_pts[(i + 1) % 3]
        # Midpoint of base edge
        mx, my = (x0 + x1) / 2.0, (y0 + y1) / 2.0
        # Face normal in XY (outward)
        ex, ey = x1 - x0, y1 - y0
        Ledge = math.hypot(ex, ey)
        nx_f, ny_f = ey / Ledge, -ex / Ledge
        # Root point at mid-height
        root = (mx, my, H / 2.0)
        # First trunk
        segs = []
        p_start = root
        dir0 = (nx_f, ny_f)
        p_end = (p_start[0] + dir0[0] * prong_length,
                 p_start[1] + dir0[1] * prong_length,
                 p_start[2])
        segs.append((p_start, p_end))
        prev = [(p_end, dir0)]
        # Branch levels
        for lvl in range(2, levels + 1):
            new_prev = []
            Lseg = prong_length * (scale ** (lvl - 1))
            for pt, d in prev:
                for sgn in (1, -1):
                    th = math.radians(angle * sgn)
                    dx, dy = d
                    rx = dx * math.cos(th) - dy * math.sin(th)
                    ry = dx * math.sin(th) + dy * math.cos(th)
                    mag = math.hypot(rx, ry)
                    if mag < 1e-6:
                        continue
                    nd = (rx / mag, ry / mag)
                    p2 = (pt[0] + nd[0] * Lseg,
                          pt[1] + nd[1] * Lseg,
                          pt[2])
                    segs.append((pt, p2))
                    new_prev.append((p2, nd))
            prev = new_prev

        # Extrude prong rods
        for p1, p2 in segs:
            dx, dy, dz_ = p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]
            L = math.sqrt(dx*dx + dy*dy + dz_*dz_)
            if L < 1e-6:
                continue
            nx, ny, nz = dx / L, dy / L, dz_ / L
            plane = cq.Plane(origin=p1, normal=(nx, ny, nz))
            rod = cq.Workplane(plane).circle(prong_radius).extrude(L)
            result = result.union(rod)

    return result
