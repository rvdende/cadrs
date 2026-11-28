def triangular_prism_with_circular_hole_array_with_grip_textures(
    side_length: float = 10.0,
    height: float = 25.0,
    hole_radius: float = 1.0,
    nx: int = 2,
    ny: int = 1,
    rib_thickness: float = 0.5,
    spacing_u: float = None,
    spacing_v: float = None,
    fillet_radius: float = 0.0,
    bump_radius: float = 0.5,
    bump_count_u: int = None,
    bump_count_v: int = None
):
    import cadquery as cq, math

    # Validate parameters
    if side_length <= 0 or height <= 0:
        raise ValueError("side_length and height must be positive")
    if hole_radius <= 0:
        raise ValueError("hole_radius must be positive")
    if nx < 1 or ny < 1:
        raise ValueError("nx and ny must be at least 1")
    if rib_thickness < 0 or fillet_radius < 0:
        raise ValueError("rib_thickness and fillet_radius must be non-negative")
    if bump_radius <= 0:
        raise ValueError("bump_radius must be positive")

    # Triangle geometry
    h_tri = side_length * math.sqrt(3.0) / 2.0
    cx, cy = side_length / 2.0, h_tri / 3.0
    outer_pts = [
        (-cx,       -cy),
        ( side_length - cx, -cy),
        ( side_length/2.0 - cx, h_tri - cy),
    ]

    # Make the base prism
    prism = cq.Workplane("XY").polyline(outer_pts).close().extrude(height)

    # Compute bump grid if needed
    if bump_count_u is None:
        bump_count_u = max(1, int(math.floor(side_length / (4.0 * bump_radius))))
    if bump_count_v is None:
        bump_count_v = max(1, int(math.floor(height / (4.0 * bump_radius))))
    if bump_count_u < 1 or bump_count_v < 1:
        raise ValueError("bump_count_u and bump_count_v must be at least 1")

    # Bump spacing and margins
    if bump_count_u > 1:
        spacing_b_u = (side_length - 2.0 * bump_radius) / float(bump_count_u - 1)
        margin_b_u = bump_radius
    else:
        spacing_b_u, margin_b_u = 0.0, side_length / 2.0
    if bump_count_v > 1:
        spacing_b_v = (height - 2.0 * bump_radius) / float(bump_count_v - 1)
        margin_b_v = bump_radius
    else:
        spacing_b_v, margin_b_v = 0.0, height / 2.0

    # Union hemispherical bumps (full spheres) onto each face
    for i in range(3):
        x0, y0 = outer_pts[i]
        x1, y1 = outer_pts[(i + 1) % 3]
        dx, dy = x1 - x0, y1 - y0
        L = math.hypot(dx, dy)
        ux, uy = dx / L, dy / L
        nx_f, ny_f = uy, -ux
        plane_bump = cq.Plane(origin=(x0, y0, 0.0), normal=(nx_f, ny_f, 0.0), xDir=(ux, uy, 0.0))
        for iv in range(bump_count_v):
            z_b = margin_b_v + iv * spacing_b_v
            for iu in range(bump_count_u):
                u_b = margin_b_u + iu * spacing_b_u
                if u_b < margin_b_u - 1e-6 or u_b > L - margin_b_u + 1e-6:
                    continue
                bump = cq.Workplane(plane_bump).pushPoints([(u_b, z_b)]).sphere(bump_radius)
                prism = prism.union(bump)

    # Prepare hole‐cutting parameters
    min_margin = hole_radius + rib_thickness
    # u‐spacing
    if spacing_u is None:
        if nx > 1:
            spacing_h_u = (side_length - 2 * min_margin) / float(nx - 1)
            margin_h_u = min_margin
        else:
            spacing_h_u, margin_h_u = 0.0, side_length / 2.0
    else:
        if spacing_u <= 0:
            raise ValueError("spacing_u must be positive")
        spacing_h_u = spacing_u
        margin_h_u = (
            (side_length - spacing_h_u * (nx - 1)) / 2.0 if nx > 1 else side_length / 2.0
        )
    if margin_h_u < min_margin - 1e-6:
        raise ValueError("spacing_u and rib_thickness leave too little edge margin")
    # v‐spacing
    if spacing_v is None:
        if ny > 1:
            spacing_h_v = (height - 2 * min_margin) / float(ny - 1)
            margin_h_v = min_margin
        else:
            spacing_h_v, margin_h_v = 0.0, height / 2.0
    else:
        if spacing_v <= 0:
            raise ValueError("spacing_v must be positive")
        spacing_h_v = spacing_v
        margin_h_v = (
            (height - spacing_h_v * (ny - 1)) / 2.0 if ny > 1 else height / 2.0
        )
    if margin_h_v < min_margin - 1e-6:
        raise ValueError("spacing_v and rib_thickness leave too little top/bottom margin")

    extrude_len = h_tri * 1.1
    # Cut holes
    for i in range(3):
        x0, y0 = outer_pts[i]
        x1, y1 = outer_pts[(i + 1) % 3]
        dx, dy = x1 - x0, y1 - y0
        L = math.hypot(dx, dy)
        ux, uy = dx / L, dy / L
        nx_f, ny_f = uy, -ux
        plane_hole = cq.Plane(origin=(x0, y0, 0.0), normal=(nx_f, ny_f, 0.0), xDir=(ux, uy, 0.0))
        pts = []
        for row in range(ny):
            z0 = margin_h_v + row * spacing_h_v
            stagger = 0.5 * spacing_h_u if (row % 2 == 1) else 0.0
            for col in range(nx):
                u0 = margin_h_u + col * spacing_h_u + stagger
                if u0 < margin_h_u - 1e-6 or u0 > L - margin_h_u + 1e-6:
                    continue
                pts.append((u0, z0))
        for u0, z0 in pts:
            cutter = (
                cq.Workplane(plane_hole)
                  .pushPoints([(u0, z0)])
                  .circle(hole_radius)
                  .extrude(-extrude_len)
            )
            prism = prism.cut(cutter)

    # Fillet hole edges
    if fillet_radius > 1e-8:
        prism = prism.edges("%Circle").fillet(fillet_radius)

    return prism
