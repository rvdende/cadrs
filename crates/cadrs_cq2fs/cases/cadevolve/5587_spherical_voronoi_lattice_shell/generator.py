def spherical_voronoi_lattice_shell(
    outer_radius: float = 10.0,
    shell_thickness: float = 1.0,
    cell_count: int = 50,
    strut_radius: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math, random
    import numpy as np
    from scipy.spatial import Voronoi

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if shell_thickness <= 0 or shell_thickness >= outer_radius:
        raise ValueError("shell_thickness must be > 0 and < outer_radius")
    if not isinstance(cell_count, int) or cell_count < 1:
        raise ValueError("cell_count must be an integer >= 1")
    if strut_radius <= 0:
        raise ValueError("strut_radius must be positive")

    # Build hollow sphere
    inner_radius = outer_radius - shell_thickness
    outer_sphere = cq.Workplane("XY").sphere(outer_radius)
    inner_sphere = cq.Workplane("XY").sphere(inner_radius)
    shell_full = outer_sphere.cut(inner_sphere)

    # Slice away bottom half (z < 0)
    cutter = (
        cq.Workplane("XY")
          .box(2 * outer_radius, 2 * outer_radius, outer_radius, centered=(True, True, False))
          .translate((0, 0, -outer_radius))
    )
    dome = shell_full.cut(cutter)

    # Generate weighted random seeds inside the hemisphere interior
    seeds = []
    attempts = 0
    max_attempts = cell_count * 20
    while len(seeds) < cell_count and attempts < max_attempts:
        z = random.uniform(0, inner_radius)
        # bias toward equator
        if random.random() > (1.0 - z / inner_radius):
            attempts += 1
            continue
        rmax = math.sqrt(max(inner_radius * inner_radius - z * z, 0.0))
        r = rmax * math.sqrt(random.random())
        theta = 2.0 * math.pi * random.random()
        x = r * math.cos(theta)
        y = r * math.sin(theta)
        seeds.append((x, y, z))
        attempts += 1

    if len(seeds) < 2:
        return dome

    pts = np.array(seeds)
    vor = Voronoi(pts)

    # Build cylindrical struts along each finite ridge between seed pairs
    lattice = None
    for (i1, i2), ridge_verts in zip(vor.ridge_points, vor.ridge_vertices):
        # skip infinite facets
        if any(v < 0 for v in ridge_verts):
            continue
        p1 = pts[i1]
        p2 = pts[i2]
        # only keep within hemisphere
        if p1[2] < 0 or p2[2] < 0:
            continue
        dx = p2[0] - p1[0]
        dy = p2[1] - p1[1]
        dz = p2[2] - p1[2]
        L = math.sqrt(dx * dx + dy * dy + dz * dz)
        if L < 1e-8:
            continue
        ux, uy, uz = dx / L, dy / L, dz / L
        mx = 0.5 * (p1[0] + p2[0])
        my = 0.5 * (p1[1] + p2[1])
        mz = 0.5 * (p1[2] + p2[2])
        plane = cq.Plane(origin=(mx, my, mz), normal=(ux, uy, uz))
        rod = cq.Workplane(plane).circle(strut_radius).extrude(L)
        lattice = rod if lattice is None else lattice.union(rod)

    # Subtract the lattice struts to carve the internal void network
    if lattice is not None:
        dome = dome.cut(lattice, clean=True)

    return dome
