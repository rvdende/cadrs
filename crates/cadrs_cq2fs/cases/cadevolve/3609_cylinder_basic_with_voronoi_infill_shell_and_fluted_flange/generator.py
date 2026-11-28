def cylinder_basic_with_voronoi_infill_shell_and_fluted_flange(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    cell_count: int = 50,
    strut_radius: float = 0.2,
    flange_width: float = 5.0,
    flange_thickness: float = 3.0,
    flute_count: int = 8,
    flute_depth: float = 5.0
) -> "cq.Workplane":
    """
    Cylinder with a Voronoi‐perforated thin shell and a fluted mounting flange.
    Wraps the mid‐section in a Voronoi strut infill, then adds a radial‐fluted flange at the base.

    Parameters:
        outer_radius    – outer radius of the cylinder body (> 0)
        wall_thickness  – wall thickness of the shell (> 0, < outer_radius)
        height          – height of the cylindrical shell (> 0)
        cell_count      – number of Voronoi seed points (>= 1)
        strut_radius    – half‐width of each Voronoi beam (> 0)
        flange_width    – radial width of the flange (>= 0)
        flange_thickness– axial thickness of the flange (> 0)
        flute_count     – number of radial flutes on the flange (>= 0)
        flute_depth     – radial extension of each flute beyond the flange outer radius (>= 0)

    Returns:
        A CadQuery Workplane object representing the combined part.
    """
    import cadquery as cq
    import math, random
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
    if flange_width < 0:
        raise ValueError("flange_width must be >= 0")
    if flange_thickness <= 0:
        raise ValueError("flange_thickness must be positive")
    if flute_count < 0:
        raise ValueError("flute_count must be >= 0")
    if flute_depth < 0:
        raise ValueError("flute_depth must be >= 0")

    inner_radius = outer_radius - wall_thickness
    flange_outer = outer_radius + flange_width

    # 1) Build the annular flange at Z = 0 → flange_thickness
    flange = (
        cq.Workplane("XY")
          .circle(flange_outer)
          .circle(outer_radius)
          .extrude(flange_thickness)
    )
    model = flange

    # 2) Build the thin‐walled cylinder shell above the flange
    shell = (
        cq.Workplane("XY")
          .workplane(offset=flange_thickness)
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")
          .shell(-wall_thickness)
    )
    model = model.union(shell, clean=True)

    # 3) Add Voronoi infill inside the shell
    if height > 2.0 * wall_thickness:
        seeds = []
        for _ in range(cell_count):
            r = inner_radius * math.sqrt(random.random())
            theta = 2.0 * math.pi * random.random()
            seeds.append((r * math.cos(theta), r * math.sin(theta)))
        pts = np.array(seeds)
        vor = Voronoi(pts)

        def inside(pt):
            return pt[0]**2 + pt[1]**2 <= inner_radius**2 + 1e-8

        rod_height = height - 2.0 * wall_thickness
        lattice = None
        for v1, v2 in vor.ridge_vertices:
            if v1 < 0 or v2 < 0:
                continue
            p1 = vor.vertices[v1]
            p2 = vor.vertices[v2]
            if not (inside(p1) and inside(p2)):
                continue
            dx, dy = p2[0] - p1[0], p2[1] - p1[1]
            L = math.hypot(dx, dy)
            if L < 1e-6:
                continue
            ang = math.atan2(dy, dx)
            cos_a, sin_a = math.cos(ang), math.sin(ang)
            mx, my = 0.5 * (p1[0] + p2[0]), 0.5 * (p1[1] + p2[1])
            plane = cq.Plane(
                origin=(mx, my, flange_thickness + wall_thickness),
                normal=(0, 0, 1),
                xDir=(cos_a, sin_a, 0),
            )
            beam = (
                cq.Workplane(plane)
                  .rect(L, 2.0 * strut_radius)
                  .extrude(rod_height)
            )
            lattice = beam if lattice is None else lattice.union(beam)
        if lattice is not None:
            model = model.union(lattice, clean=True)

    # 4) Add radial flutes on the flange ring
    if flute_count > 0 and flute_depth > 0:
        step = 360.0 / flute_count
        mid_r = flange_outer + flute_depth / 2.0
        radial_w = flute_depth
        span = step * 0.5
        chord = 2.0 * mid_r * math.sin(math.radians(span / 2.0))
        flutes = None
        for i in range(flute_count):
            ang = i * step
            rib = (
                cq.Workplane("XY")
                  .transformed(rotate=(0, 0, ang))
                  .center(mid_r, 0)
                  .rect(radial_w, chord)
                  .extrude(flange_thickness)
            )
            flutes = rib if flutes is None else flutes.union(rib)
        model = model.union(flutes, clean=True)

    return model
