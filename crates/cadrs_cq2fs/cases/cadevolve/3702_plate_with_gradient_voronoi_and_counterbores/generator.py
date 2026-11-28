def plate_with_gradient_voronoi_and_counterbores(
    length=100.0,
    width=60.0,
    thickness=5.0,
    npoints=50,
    gradient_exp=2.0,
    hole_dia=5.0,
    cbore_dia=10.0,
    cbore_depth=1.5,
    seed=0
):
    import cadquery as cq
    import random
    import math

    # Parameter normalization and validation
    length = float(length); width = float(width); thickness = float(thickness)
    npoints = int(npoints); gradient_exp = float(gradient_exp)
    hole_dia = float(hole_dia); cbore_dia = float(cbore_dia)
    cbore_depth = float(cbore_depth); seed = int(seed)

    if length <= 0 or width <= 0 or thickness <= 0:
        raise ValueError("length, width, and thickness must be positive")
    if npoints < 1:
        raise ValueError("npoints must be at least 1")
    if gradient_exp <= 0:
        raise ValueError("gradient_exp must be > 0")
    if hole_dia <= 0:
        raise ValueError("hole_dia must be positive")
    if cbore_dia < hole_dia:
        raise ValueError("cbore_dia must be >= hole_dia")
    if cbore_depth < 0:
        raise ValueError("cbore_depth must be non-negative")

    half_x = length / 2.0
    half_y = width / 2.0

    # Generate seed points with a lateral gradient distribution
    random.seed(seed)
    seeds = []
    for _ in range(npoints):
        u = random.random()
        x = -half_x + length * (u ** gradient_exp)
        y = random.uniform(-half_y, half_y)
        seeds.append((x, y))

    # Helper: clip a convex polygon by a half‐plane n·(x,y) <= c
    def clip_poly(poly, n, c):
        out = []
        if not poly:
            return out
        for i in range(len(poly)):
            x1, y1 = poly[i]
            x2, y2 = poly[(i + 1) % len(poly)]
            v1 = n[0] * x1 + n[1] * y1
            v2 = n[0] * x2 + n[1] * y2
            in1 = v1 <= c
            in2 = v2 <= c
            if in1:
                out.append((x1, y1))
            if in1 ^ in2:
                d = v2 - v1
                if abs(d) > 1e-9:
                    t = (c - v1) / d
                    xi = x1 + (x2 - x1) * t
                    yi = y1 + (y2 - y1) * t
                    out.append((xi, yi))
        return out

    # Build voronoi cells by clipping the bounding rectangle
    bbox = [(-half_x, -half_y), (half_x, -half_y),
            (half_x, half_y),    (-half_x, half_y)]
    cells = []
    for px, py in seeds:
        region = bbox[:]
        pnorm2 = px * px + py * py
        for qx, qy in seeds:
            if qx == px and qy == py:
                continue
            nvec = (qx - px, qy - py)
            cval = (qx * qx + qy * qy - pnorm2) / 2.0
            region = clip_poly(region, nvec, cval)
            if len(region) <= 2:
                break
        if len(region) > 2:
            cells.append(region)

    # Count edges and keep only interior edges (shared by two cells)
    def quantize(pt, prec=6):
        return (round(pt[0], prec), round(pt[1], prec))
    edge_counts = {}
    for poly in cells:
        L = len(poly)
        for i in range(L):
            p1 = poly[i]
            p2 = poly[(i + 1) % L]
            q1 = quantize(p1)
            q2 = quantize(p2)
            if q1 == q2:
                continue
            key = (q1, q2) if q1 < q2 else (q2, q1)
            edge_counts[key] = edge_counts.get(key, 0) + 1
    interior_edges = [edge for edge, cnt in edge_counts.items() if cnt == 2]

    # Create the base plate
    plate = cq.Workplane("XY").box(length, width, thickness)

    # Cut the voronoi edges as narrow rectangular slots
    kerf = thickness * 0.2
    halfk = kerf / 2.0
    for p1, p2 in interior_edges:
        x1, y1 = p1; x2, y2 = p2
        dx, dy = x2 - x1, y2 - y1
        seg_len = math.hypot(dx, dy)
        if seg_len < 1e-6:
            continue
        nx, ny = -dy / seg_len, dx / seg_len
        pa = (x1 + nx * halfk, y1 + ny * halfk)
        pb = (x2 + nx * halfk, y2 + ny * halfk)
        pc = (x2 - nx * halfk, y2 - ny * halfk)
        pd = (x1 - nx * halfk, y1 - ny * halfk)
        cutter = (
            cq.Workplane("XY")
              .workplane(offset=-thickness/2.0)
              .polyline([pa, pb, pc, pd])
              .close()
              .extrude(thickness * 2.0)
              .val()
        )
        plate = plate.cut(cutter)

    # Add four counterbored holes in the corners
    off = cbore_dia / 2.0
    hole_positions = [
        (-half_x + off, -half_y + off),
        (-half_x + off,  half_y - off),
        ( half_x - off, -half_y + off),
        ( half_x - off,  half_y - off),
    ]
    for hx, hy in hole_positions:
        # counterbore pocket
        cbore_cutter = (
            cq.Workplane("XY")
              .workplane(offset=thickness/2.0)
              .center(hx, hy)
              .circle(cbore_dia / 2.0)
              .extrude(-cbore_depth)
              .val()
        )
        plate = plate.cut(cbore_cutter)
        # through hole
        hole_cutter = (
            cq.Workplane("XY")
              .workplane(offset=thickness/2.0)
              .center(hx, hy)
              .circle(hole_dia / 2.0)
              .extrude(-thickness)
              .val()
        )
        plate = plate.cut(hole_cutter)

    return plate
