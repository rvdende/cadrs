def cylinder_basic_with_voronoi_mesh(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    cell_count: int = 50,
    strut_radius: float = 0.2
) -> "cq.Workplane":
    """
    Cylinder basic with voronoi mesh.
    Builds an open-ended cylindrical shell of given outer_radius, wall_thickness, and height,
    populates the inner circular cross-section with cell_count random seeds,
    computes the 2D Voronoi diagram, and for each finite ridge fully inside
    the inner radius creates a rectangular beam (width 2*strut_radius, length = ridge length)
    extruded between z=wall_thickness and z=height-wall_thickness. All beams
    are unioned with the shell.
    """
    import cadquery as cq
    import math
    import random
    import numpy as np
    from scipy.spatial import Voronoi

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    if cell_count < 1:
        raise ValueError("cell_count must be at least 1")
    if strut_radius <= 0:
        raise ValueError("strut_radius must be positive")

    inner_radius = outer_radius - wall_thickness

    # Build and shell the cylinder (open ends)
    shell = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")
          .shell(-wall_thickness)
    )

    # No room for lattice if too short
    if height <= 2.0 * wall_thickness:
        return shell

    # Generate random seed points uniformly in the inner circle
    seeds = []
    for _ in range(cell_count):
        r = inner_radius * math.sqrt(random.random())
        theta = random.random() * 2.0 * math.pi
        seeds.append([r * math.cos(theta), r * math.sin(theta)])
    pts_arr = np.array(seeds)

    # Compute 2D Voronoi diagram
    vor = Voronoi(pts_arr)

    # Helper to test if a point is inside the inner circle
    def _inside(pt):
        return pt[0]*pt[0] + pt[1]*pt[1] <= inner_radius*inner_radius + 1e-8

    rod_height = height - 2.0 * wall_thickness
    result = shell

    # Build and union beams for each finite ridge inside the circle
    for vpair in vor.ridge_vertices:
        v1, v2 = vpair
        if v1 < 0 or v2 < 0:
            continue
        p1 = vor.vertices[v1]
        p2 = vor.vertices[v2]
        if not (_inside(p1) and _inside(p2)):
            continue
        dx = p2[0] - p1[0]
        dy = p2[1] - p1[1]
        L = math.hypot(dx, dy)
        if L < 1e-6:
            continue
        ang = math.atan2(dy, dx)
        mx = 0.5*(p1[0] + p2[0])
        my = 0.5*(p1[1] + p2[1])
        plane = cq.Plane(
            origin=(mx, my, wall_thickness),
            normal=(0, 0, 1),
            xDir=(math.cos(ang), math.sin(ang), 0)
        )
        beam = (
            cq.Workplane(plane)
              .rect(L, 2.0*strut_radius)
              .extrude(rod_height)
        )
        result = result.union(beam)

    return result
