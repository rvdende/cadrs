def cone_revolve_with_voronoi_mesh(
    base_radius: float = 12.0,
    top_radius: float = 6.0,
    height: float = 25.0,
    shell_thickness: float = 1.0,
    cell_count: int = 50,
    rib_thickness: float = 2.0
) -> "cq.Workplane":
    """
    Create a revolved conical frustum with an internal Voronoi-pattern rib mesh.
    
    Parameters:
        base_radius     – radius of the cone at z=0
        top_radius      – radius of the cone at z=height
        height          – height of the cone along +Z
        shell_thickness – wall thickness of the shell
        cell_count      – number of random seed points for the Voronoi diagram
        rib_thickness   – thickness of each Voronoi rib
    """
    import cadquery as cq
    import math
    import random
    from scipy.spatial import Voronoi

    # Validate inputs
    if base_radius <= 0 or top_radius <= 0 or height <= 0:
        raise ValueError("base_radius, top_radius, and height must be positive")
    if shell_thickness <= 0 or shell_thickness >= min(base_radius, top_radius):
        raise ValueError("shell_thickness must be >0 and < min(base_radius, top_radius)")
    if cell_count < 1:
        raise ValueError("cell_count must be at least 1")
    if rib_thickness <= 0:
        raise ValueError("rib_thickness must be positive")

    # Compute inner radii
    inner_r1 = base_radius - shell_thickness
    inner_r2 = top_radius - shell_thickness

    # Build outer frustum by revolving a profile in the XZ plane
    outer = (
        cq.Workplane("XZ")
          .moveTo(0.0, 0.0)
          .lineTo(base_radius, 0.0)
          .lineTo(top_radius, height)
          .close()
          .revolve(360.0)
    )
    # Build inner frustum for hollow shell
    inner = (
        cq.Workplane("XZ")
          .moveTo(0.0, 0.0)
          .lineTo(inner_r1, 0.0)
          .lineTo(inner_r2, height)
          .close()
          .revolve(360.0)
    )
    # Hollow shell
    shell = outer.cut(inner)

    # If too short for ribs, return shell only
    if height <= 2.0 * shell_thickness:
        return shell

    rod_height = height - 2.0 * shell_thickness

    # Generate random seed points in bottom inner circle
    seeds = []
    for _ in range(cell_count):
        r = inner_r1 * math.sqrt(random.random())
        theta = 2.0 * math.pi * random.random()
        seeds.append((r * math.cos(theta), r * math.sin(theta)))

    vor = Voronoi(seeds)

    # Helper to test if a point lies inside the bottom inner circle
    def inside(pt):
        return pt[0] * pt[0] + pt[1] * pt[1] <= inner_r1 * inner_r1 + 1e-8

    # Build ribs along Voronoi edges
    ribs = []
    for v1, v2 in vor.ridge_vertices:
        if v1 < 0 or v2 < 0:
            continue
        p1 = vor.vertices[v1]
        p2 = vor.vertices[v2]
        if not (inside(p1) and inside(p2)):
            continue
        dx = p2[0] - p1[0]
        dy = p2[1] - p1[1]
        length = math.hypot(dx, dy)
        if length < 1e-6:
            continue
        angle = math.atan2(dy, dx)
        mx = 0.5 * (p1[0] + p2[0])
        my = 0.5 * (p1[1] + p2[1])
        plane = cq.Plane(
            origin=(mx, my, shell_thickness),
            normal=(0, 0, 1),
            xDir=(math.cos(angle), math.sin(angle), 0)
        )
        beam = cq.Workplane(plane).rect(length, rib_thickness).extrude(rod_height)
        ribs.append(beam)

    # If no ribs, return shell
    if not ribs:
        return shell

    # Fuse all ribs
    ribs_union = ribs[0]
    for rib in ribs[1:]:
        ribs_union = ribs_union.union(rib)

    # Clip ribs to the inner frustum and fuse into shell
    ribs_clipped = ribs_union.intersect(inner)
    result = shell.union(ribs_clipped)

    return result
