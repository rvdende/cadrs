def spherical_shell_with_geodesic_rib_lattice(
    outer_radius: float = 40.0,
    shell_thickness: float = 3.0,
    subdivisions: int = 2,
    rib_width: float = 1.5,
    rib_height: float = 0.8
) -> "cq.Workplane":
    """
    A spherical shell perforated in a geodesic pattern with raised lattice ribs.
    Hollow sphere of diameter 2*outer_radius, wall thickness shell_thickness.
    Geodesic tessellation with `subdivisions` per icosahedron face.
    Ribs of width rib_width and height rib_height are extruded along every edge.
    Returns a single solid, watertight shell with exterior ribs and perforations.
    """
    import cadquery as cq
    import math

    # parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if shell_thickness <= 0 or shell_thickness >= outer_radius:
        raise ValueError("shell_thickness must be > 0 and < outer_radius")
    if not isinstance(subdivisions, int) or subdivisions < 1:
        raise ValueError("subdivisions must be integer >= 1")
    if rib_width <= 0:
        raise ValueError("rib_width must be positive")
    if rib_height <= 0:
        raise ValueError("rib_height must be positive")

    # 1) build hollow spherical shell
    outer = cq.Workplane("XY").sphere(outer_radius)
    inner_radius = outer_radius - shell_thickness
    inner = cq.Workplane("XY").sphere(inner_radius)
    shell = outer.cut(inner)

    # 2) create geodesic subdivisions of an icosahedron
    phi = (1.0 + math.sqrt(5.0)) / 2.0
    raw_verts = [
        (-1,  phi,  0), ( 1,  phi,  0), (-1, -phi,  0), ( 1, -phi,  0),
        ( 0, -1,  phi), ( 0,  1,  phi), ( 0, -1, -phi), ( 0,  1, -phi),
        ( phi,  0, -1), ( phi,  0,  1), (-phi,  0, -1), (-phi,  0,  1),
    ]
    faces = [
        (0,11,5),(0,5,1),(0,1,7),(0,7,10),(0,10,11),
        (1,5,9),(5,11,4),(11,10,2),(10,7,6),(7,1,8),
        (3,9,4),(3,4,2),(3,2,6),(3,6,8),(3,8,9),
        (4,9,5),(2,4,11),(6,2,10),(8,6,7),(9,8,1),
    ]
    # normalize raw vertices to unit sphere
    vertices = []
    for x0, y0, z0 in raw_verts:
        d = math.sqrt(x0*x0 + y0*y0 + z0*z0)
        vertices.append((x0/d, y0/d, z0/d))

    def sphere_node(i, j, v0, v1, v2):
        # barycentric interpolation then renormalize
        k = subdivisions - i - j
        x = v0[0]*k + v1[0]*i + v2[0]*j
        y = v0[1]*k + v1[1]*i + v2[1]*j
        z = v0[2]*k + v1[2]*i + v2[2]*j
        inv = 1.0 / math.sqrt(x*x + y*y + z*z)
        return (x*inv, y*inv, z*inv)

    # collect small triangles and unique edges on outer sphere
    triangles = []
    edges_set = set()
    for ia, ib, ic in faces:
        v0, v1, v2 = vertices[ia], vertices[ib], vertices[ic]
        for i in range(subdivisions):
            for j in range(subdivisions - i):
                p1 = sphere_node(i,   j,   v0, v1, v2)
                p2 = sphere_node(i+1, j,   v0, v1, v2)
                p3 = sphere_node(i,   j+1, v0, v1, v2)
                triangles.append((p1, p2, p3))
                for pa, pb in ((p1, p2), (p2, p3), (p3, p1)):
                    A = (round(pa[0]*outer_radius,6),
                         round(pa[1]*outer_radius,6),
                         round(pa[2]*outer_radius,6))
                    B = (round(pb[0]*outer_radius,6),
                         round(pb[1]*outer_radius,6),
                         round(pb[2]*outer_radius,6))
                    edges_set.add(tuple(sorted([A, B])))
                if j < subdivisions - i - 1:
                    p4 = sphere_node(i+1, j+1, v0, v1, v2)
                    triangles.append((p2, p4, p3))
                    for pa, pb in ((p2, p4), (p4, p3), (p3, p2)):
                        A = (round(pa[0]*outer_radius,6),
                             round(pa[1]*outer_radius,6),
                             round(pa[2]*outer_radius,6))
                        B = (round(pb[0]*outer_radius,6),
                             round(pb[1]*outer_radius,6),
                             round(pb[2]*outer_radius,6))
                        edges_set.add(tuple(sorted([A, B])))

    # 3) subtract triangular panels through shell thickness only
    eps = 1e-3
    cut_depth = shell_thickness/2.0 + eps
    for tri in triangles:
        A = (tri[0][0]*outer_radius, tri[0][1]*outer_radius, tri[0][2]*outer_radius)
        B = (tri[1][0]*outer_radius, tri[1][1]*outer_radius, tri[1][2]*outer_radius)
        C = (tri[2][0]*outer_radius, tri[2][1]*outer_radius, tri[2][2]*outer_radius)
        cx = (A[0] + B[0] + C[0]) / 3.0
        cy = (A[1] + B[1] + C[1]) / 3.0
        cz = (A[2] + B[2] + C[2]) / 3.0
        AB = (B[0]-A[0], B[1]-A[1], B[2]-A[2])
        AC = (C[0]-A[0], C[1]-A[1], C[2]-A[2])
        # normal
        nx = AB[1]*AC[2] - AB[2]*AC[1]
        ny = AB[2]*AC[0] - AB[0]*AC[2]
        nz = AB[0]*AC[1] - AB[1]*AC[0]
        nl = math.sqrt(nx*nx + ny*ny + nz*nz)
        if nl == 0:
            continue
        nx, ny, nz = nx/nl, ny/nl, nz/nl
        # local x-axis (edge direction projected)
        dx, dy, dz = AB
        proj = dx*nx + dy*ny + dz*nz
        tx = dx - proj*nx
        ty = dy - proj*ny
        tz = dz - proj*nz
        tl = math.sqrt(tx*tx + ty*ty + tz*tz)
        if tl == 0:
            continue
        tx, ty, tz = tx/tl, ty/tl, tz/tl
        # local y-axis
        yx = ny*tz - nz*ty
        yy = nz*tx - nx*tz
        yz = nx*ty - ny*tx
        # project triangle vertices into 2D
        pts2d = []
        for P in (A, B, C):
            vx, vy, vz = P[0]-cx, P[1]-cy, P[2]-cz
            px = vx*tx + vy*ty + vz*tz
            py = vx*yx + vy*yy + vz*yz
            pts2d.append((px, py))
        plane = cq.Plane(origin=(cx, cy, cz), normal=(nx, ny, nz), xDir=(tx, ty, tz))
        cutter = (
            cq.Workplane(plane)
              .polyline(pts2d)
              .close()
              .extrude(cut_depth, both=True)
        )
        shell = shell.cut(cutter)

    # 4) build and union ribs along every unique edge
    ribs = None
    for key in edges_set:
        A, B = key
        mx = 0.5*(A[0] + B[0]); my = 0.5*(A[1] + B[1]); mz = 0.5*(A[2] + B[2])
        nl = math.sqrt(mx*mx + my*my + mz*mz)
        if nl == 0:
            continue
        nx, ny, nz = mx/nl, my/nl, mz/nl
        dx, dy, dz = B[0]-A[0], B[1]-A[1], B[2]-A[2]
        tl = math.sqrt(dx*dx + dy*dy + dz*dz)
        if tl == 0:
            continue
        tx, ty, tz = dx/tl, dy/tl, dz/tl
        plane = cq.Plane(origin=(mx, my, mz), normal=(nx, ny, nz), xDir=(tx, ty, tz))
        rib = cq.Workplane(plane).rect(tl, rib_width).extrude(rib_height)
        ribs = rib if ribs is None else ribs.union(rib, clean=True)

    # 5) combine shell and ribs into single solid
    result = shell.union(ribs, clean=True)
    return result
