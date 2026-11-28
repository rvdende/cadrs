def fractal_lattice_snap_dome(
    R: float = 12.0,
    thickness: float = 1.0,
    subdivisions: int = 1,
    beam_radius: float = 0.2,
    fractal_levels: int = 2,
    fractal_scale: float = 0.5,
    base_thickness: float = None,
    lug_count: int = 6,
    lug_depth: float = 1.0,
    lug_width: float = 2.0,
    lug_height: float = 1.0,
    lug_z_offset: float = None,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    """
    Variation of lattice_shell_dome_with_snaps with fractal lattice pattern.
    Extends lattice_shell_dome_with_snaps by replacing uniform lattice with a fractal Voronoi pattern for visual complexity.

    Parameters:
    - R: Outer radius of hemisphere (>0)
    - thickness: shell thickness (0 < thickness < R)
    - subdivisions: geodesic subdivisions (int >=1)
    - beam_radius: radius of lattice rods (>0)
    - fractal_levels: number of recursive branching levels (int >=1)
    - fractal_scale: branch size scale (0 < scale <1)
    - base_thickness: thickness of flat base (default = thickness)
    - lug_count: number of snap lugs (>=0)
    - lug_depth, lug_width, lug_height: dimensions of snap lugs (>0 if lug_count>0)
    - lug_z_offset: vertical offset of lugs from base (default = base_thickness - lug_height)
    - fillet_radius: fillet on lugs and vertical edges (>=0)
    """
    import cadquery as cq
    import math

    # parameter validation
    R = float(R)
    thickness = float(thickness)
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got {thickness}")
    if not isinstance(subdivisions, int) or subdivisions < 1:
        raise ValueError(f"subdivisions must be integer >= 1; got {subdivisions}")
    if beam_radius <= 0:
        raise ValueError(f"beam_radius must be > 0; got {beam_radius}")
    fractal_levels = int(fractal_levels)
    if fractal_levels < 1:
        raise ValueError(f"fractal_levels must be integer >= 1; got {fractal_levels}")
    fractal_scale = float(fractal_scale)
    if fractal_scale <= 0 or fractal_scale >= 1.0:
        raise ValueError(f"fractal_scale must be > 0 and < 1; got {fractal_scale}")
    if base_thickness is None:
        base_thickness = thickness
    base_thickness = float(base_thickness)
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be > 0; got {base_thickness}")
    lug_count = int(lug_count)
    if lug_count < 0:
        raise ValueError(f"lug_count must be >= 0; got {lug_count}")
    lug_depth = float(lug_depth)
    lug_width = float(lug_width)
    lug_height = float(lug_height)
    if lug_count > 0 and (lug_depth <= 0 or lug_width <= 0 or lug_height <= 0):
        raise ValueError("If lug_count>0, lug_depth, lug_width, lug_height must be > 0")
    if lug_z_offset is None:
        lug_z_offset = base_thickness - lug_height
    lug_z_offset = float(lug_z_offset)
    if lug_z_offset < 0 or lug_z_offset + lug_height > base_thickness:
        raise ValueError(f"lug_z_offset must satisfy 0 <= lug_z_offset <= base_thickness - lug_height; got {lug_z_offset}")
    fillet_radius = float(fillet_radius)
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >= 0; got {fillet_radius}")

    inner_R = R - thickness

    # 1) Build hollow trimmed hemisphere shell
    outer_cut_depth = R + base_thickness
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * R, 2 * R)
                .extrude(-outer_cut_depth)
          )
    )
    inner_cut_depth = inner_R + base_thickness
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * inner_R, 2 * inner_R)
                .extrude(-inner_cut_depth)
          )
    )
    shell = outer.cut(inner)

    # 2) Build base geodesic edges on inner sphere
    phi = (1.0 + math.sqrt(5.0)) / 2.0
    raw_verts = [
        (-1,  phi,  0), ( 1,  phi,  0), (-1, -phi,  0), ( 1, -phi,  0),
        ( 0, -1,  phi), ( 0,  1,  phi), ( 0, -1, -phi), ( 0,  1, -phi),
        ( phi,  0, -1), ( phi,  0,  1), (-phi,  0, -1), (-phi,  0,  1),
    ]
    faces = [
        (0,11, 5), (0, 5, 1), (0, 1, 7), (0, 7,10), (0,10,11),
        (1, 5, 9), (5,11, 4), (11,10,2), (10,7, 6), (7, 1, 8),
        (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9),
        (4, 9, 5), (2, 4,11), (6, 2,10), (8, 6, 7), (9, 8, 1),
    ]
    vertices = []
    for x0, y0, z0 in raw_verts:
        d = math.sqrt(x0*x0 + y0*y0 + z0*z0)
        vertices.append((x0/d * inner_R, y0/d * inner_R, z0/d * inner_R))

    def node_pt(i, j, v0, v1, v2):
        k = subdivisions - i - j
        x = v0[0]*k + v1[0]*i + v2[0]*j
        y = v0[1]*k + v1[1]*i + v2[1]*j
        z = v0[2]*k + v1[2]*i + v2[2]*j
        norm = math.sqrt(x*x + y*y + z*z)
        return (x/norm * inner_R, y/norm * inner_R, z/norm * inner_R)

    edges_seen = set()
    base_edges = []
    geodesic_rods = None
    for ia, ib, ic in faces:
        v0, v1, v2 = vertices[ia], vertices[ib], vertices[ic]
        for i in range(subdivisions):
            for j in range(subdivisions - i):
                p1 = node_pt(i,   j,   v0, v1, v2)
                p2 = node_pt(i+1, j,   v0, v1, v2)
                p3 = node_pt(i,   j+1, v0, v1, v2)
                segs = [(p1, p2), (p2, p3), (p3, p1)]
                if j < (subdivisions - i - 1):
                    p4 = node_pt(i+1, j+1, v0, v1, v2)
                    segs += [(p2, p4), (p4, p3)]
                for a, b in segs:
                    if a[2] < 0 and b[2] < 0:
                        continue
                    r1 = (round(a[0],6), round(a[1],6), round(a[2],6))
                    r2 = (round(b[0],6), round(b[1],6), round(b[2],6))
                    key = tuple(sorted((r1, r2)))
                    if key in edges_seen:
                        continue
                    edges_seen.add(key)
                    base_edges.append((a, b))
                    dx, dy, dz = b[0]-a[0], b[1]-a[1], b[2]-a[2]
                    length = math.sqrt(dx*dx + dy*dy + dz*dz)
                    mid = ((a[0]+b[0])/2, (a[1]+b[1])/2, (a[2]+b[2])/2)
                    xdir = (1,0,0) if abs(dz) > 0.99 else (0,0,1)
                    plane2 = cq.Plane(origin=mid, xDir=xdir, normal=(dx, dy, dz))
                    cyl = (
                        cq.Workplane(plane2)
                          .circle(beam_radius)
                          .extrude(length/2.0, both=True)
                    )
                    geodesic_rods = cyl if geodesic_rods is None else geodesic_rods.union(cyl, clean=True)
    geodesic_lattice = geodesic_rods if geodesic_rods is not None else cq.Workplane("XY")

    # 3) Fractal branching
    fractal_segments = []
    def branch(edges, level):
        for a, b in edges:
            ax, ay, az = a
            bx, by, bz = b
            mx, my, mz = (ax+bx)/2.0, (ay+by)/2.0, (az+bz)/2.0
            dx_d, dy_d, dz_d = bx-ax, by-ay, bz-az
            L = math.sqrt(dx_d*dx_d + dy_d*dy_d + dz_d*dz_d)
            if L == 0:
                continue
            nl = math.sqrt(mx*mx + my*my + mz*mz)
            if nl == 0:
                continue
            nx, ny, nz = mx/nl, my/nl, mz/nl
            vx, vy, vz = -nx, -ny, -nz
            branch_len = L * fractal_scale
            ex, ey, ez = mx + vx * branch_len, my + vy * branch_len, mz + vz * branch_len
            fractal_segments.append(((mx, my, mz), (ex, ey, ez), level))
            if level < fractal_levels:
                branch([((mx, my, mz), (ex, ey, ez))], level + 1)
    branch(base_edges, 1)

    fractal_rods = None
    for a, b, level in fractal_segments:
        ax, ay, az = a
        bx, by, bz = b
        dx_d, dy_d, dz_d = bx-ax, by-ay, bz-az
        length = math.sqrt(dx_d*dx_d + dy_d*dy_d + dz_d*dz_d)
        mid = ((ax+bx)/2.0, (ay+by)/2.0, (az+bz)/2.0)
        xdir = (1,0,0) if abs(dz_d) > 0.99 else (0,0,1)
        plane2 = cq.Plane(origin=mid, xDir=xdir, normal=(dx_d, dy_d, dz_d))
        radius = beam_radius * (fractal_scale ** level)
        cyl = (
            cq.Workplane(plane2)
              .circle(radius)
              .extrude(length/2.0, both=True)
        )
        fractal_rods = cyl if fractal_rods is None else fractal_rods.union(cyl, clean=True)
    fractal_lattice = fractal_rods if fractal_rods is not None else cq.Workplane("XY")

    lattice = geodesic_lattice.union(fractal_lattice, clean=True)

    # 4) Combine shell and lattice, add base ring
    part = shell.union(lattice, clean=True)
    outer_cyl = cq.Workplane("XY").circle(R).extrude(base_thickness)
    inner_cyl = cq.Workplane("XY").circle(inner_R).extrude(base_thickness)
    base_ring = outer_cyl.cut(inner_cyl)
    part = part.union(base_ring, clean=True)

    # 5) Add radial snap lugs
    for k in range(lug_count):
        angle = 360.0 * k / lug_count
        z0 = lug_z_offset
        lug = (
            cq.Workplane("XY")
              .transformed(rotate=(0,0,angle), offset=(R,0,z0))
              .box(lug_depth, lug_width, lug_height, centered=(False, True, False))
        )
        if fillet_radius > 0:
            try:
                lug = lug.edges().fillet(fillet_radius)
            except Exception:
                pass
        part = part.union(lug, clean=True)

    # 6) Optional fillet on vertical edges
    if fillet_radius > 0:
        try:
            part = part.edges("|Z").fillet(fillet_radius)
        except Exception:
            pass

    return part.combine()
