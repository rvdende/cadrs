def shell_cone_buckle_lattice(
    base_radius: float = 10.0,
    top_radius: float = 0.0,
    height: float = 20.0,
    thickness: float = 1.5,
    lattice_size: float = 5.0,
    buckle: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if base_radius <= 0 or height <= 0:
        raise ValueError("base_radius and height must be positive")
    if not (0 <= top_radius <= base_radius):
        raise ValueError("top_radius must be >= 0 and <= base_radius")
    if thickness <= 0 or thickness >= base_radius:
        raise ValueError("thickness must be > 0 and < base_radius")
    if lattice_size <= 0:
        raise ValueError("lattice_size must be positive")
    if buckle < 0:
        raise ValueError("buckle must be non-negative")

    # Build hollow conical shell by revolving profiles
    outer_pts = [(0.0, 0.0), (base_radius, 0.0)]
    if top_radius > 0:
        outer_pts.append((top_radius, height))
    outer_pts.append((0.0, height))
    outer = (
        cq.Workplane("XZ")
        .polyline(outer_pts)
        .close()
        .revolve(360.0, (0, 0, 0), (0, 0, 1))
    )

    inner_base = base_radius - thickness
    inner_top = max(top_radius - thickness, 0.0)
    inner_pts = [(0.0, 0.0), (inner_base, 0.0)]
    if inner_top > 0:
        inner_pts.append((inner_top, height))
    inner_pts.append((0.0, height))
    inner = (
        cq.Workplane("XZ")
        .polyline(inner_pts)
        .close()
        .revolve(360.0, (0, 0, 0), (0, 0, 1))
    )

    shell = outer.cut(inner)

    # Prepare triangular lattice on the mid-surface
    slant = math.hypot(base_radius - top_radius, height)
    k = (top_radius - base_radius) / height
    row_spacing = lattice_size * math.sqrt(3.0) / 2.0
    n_rows = int(math.floor(slant / row_spacing))
    if n_rows < 1 or buckle == 0.0:
        return shell

    n_theta = max(3, int(math.floor(2.0 * math.pi * base_radius / lattice_size)))
    dtheta = 2.0 * math.pi / n_theta

    # Compute grid points on mid-surface
    P = []
    for j in range(n_rows + 1):
        vj = j * row_spacing
        zj = (vj / slant) * height
        rj = base_radius + (top_radius - base_radius) * (zj / height)
        offset = 0.5 if (j % 2) else 0.0
        row = []
        for i in range(n_theta):
            phi = (i + offset) * dtheta
            row.append((rj * math.cos(phi), rj * math.sin(phi), zj))
        P.append(row)

    # Build and collect extruded triangles
    norm_n = math.sqrt(1.0 + k * k)
    extrude_wps = []
    for j in range(n_rows):
        even = (j % 2) == 0
        for i in range(n_theta):
            i1 = (i + 1) % n_theta
            p0, p1 = P[j][i], P[j][i1]
            p2, p3 = P[j + 1][i], P[j + 1][i1]
            tris = [(p0, p2, p1), (p1, p2, p3)] if even else [(p0, p1, p3), (p0, p3, p2)]
            for tri in tris:
                cx = sum(p[0] for p in tri) / 3.0
                cy = sum(p[1] for p in tri) / 3.0
                cz = sum(p[2] for p in tri) / 3.0
                phi_c = math.atan2(cy, cx)
                nx = math.cos(phi_c) / norm_n
                ny = math.sin(phi_c) / norm_n
                nz = -k / norm_n
                tx = -math.sin(phi_c)
                ty = math.cos(phi_c)
                tz = 0.0
                # local y-axis by cross(normal, xDir)
                yx = ny * tz - nz * ty
                yy = nz * tx - nx * tz
                yz = nx * ty - ny * tx
                plane = cq.Plane(origin=(cx, cy, cz), xDir=(tx, ty, tz), normal=(nx, ny, nz))
                pts2d = []
                for (x, y, z) in tri:
                    vx, vy, vz = x - cx, y - cy, z - cz
                    u = vx * tx + vy * ty + vz * tz
                    v = vx * yx + vy * yy + vz * yz
                    pts2d.append((u, v))
                extrude_wps.append(
                    cq.Workplane(plane)
                      .polyline(pts2d)
                      .close()
                      .extrude(buckle + 1e-6, combine=False)
                )

    # Fuse all panels one by one to avoid passing multiple args to union
    result = shell
    for wp in extrude_wps:
        result = result.union(wp.val())
    return result
