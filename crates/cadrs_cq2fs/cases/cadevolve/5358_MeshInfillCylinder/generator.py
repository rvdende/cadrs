def MeshInfillCylinder(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    mesh_size: float = 5.0,
    mesh_depth: float = None,
    chamfer_size: float = 0.3
) -> cq.Workplane:
    """
    Thin‐walled cylinder with internal triangular mesh infill.
    Mesh beams are extruded inward from the inner shell surface,
    and vertical beam edges on each beam are chamfered for a smooth transition.

    Parameters:
        outer_radius  -- outer radius of cylinder (>0)
        wall_thickness-- shell thickness (0 < wall_thickness < outer_radius)
        height        -- cylinder height (>0)
        mesh_size     -- side length of each triangular cell (>0)
        mesh_depth    -- depth to extrude each beam inward (<= inner_radius).
                         Defaults to inner_radius.
        chamfer_size  -- size of chamfer to apply to vertical edges on beams (>=0)
    Returns:
        A CadQuery Workplane representing the final solid.
    """
    import math

    # Parameter validation
    if outer_radius <= 0 or height <= 0:
        raise ValueError("outer_radius and height must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if mesh_size <= 0:
        raise ValueError("mesh_size must be positive")
    inner_radius = outer_radius - wall_thickness
    if mesh_depth is None:
        mesh_depth = inner_radius
    if mesh_depth <= 0 or mesh_depth > inner_radius:
        raise ValueError("mesh_depth must be > 0 and <= inner_radius")
    if chamfer_size < 0:
        raise ValueError("chamfer_size must be non‐negative")

    # Build open‐ended shell
    outer = cq.Workplane("XY").circle(outer_radius).extrude(height)
    eps = 1e-6
    inner = (
        cq.Workplane("XY")
          .circle(inner_radius)
          .extrude(height + 2 * eps)
          .translate((0, 0, -eps))
    )
    result = outer.cut(inner)

    # Mesh grid parameters on the unwrapped inner surface
    total_arc = 2.0 * math.pi * inner_radius
    half_w = total_arc / 2.0
    cell_h = mesh_size * math.sqrt(3.0) / 2.0
    n_cols = max(int(math.floor(total_arc / mesh_size)), 1)
    n_rows = max(int(math.ceil(height / cell_h)), 1)

    # Generate triangular beams
    for j in range(n_rows):
        v0 = j * cell_h
        for i in range(n_cols):
            u0 = -half_w + i * mesh_size
            # choose triangle orientation
            if (i + j) % 2 == 0:
                pts = [
                    (u0, v0),
                    (u0 + mesh_size, v0),
                    (u0 + mesh_size / 2.0, v0 + cell_h),
                ]
            else:
                pts = [
                    (u0, v0 + cell_h),
                    (u0 + mesh_size, v0 + cell_h),
                    (u0 + mesh_size / 2.0, v0),
                ]
            # skip triangles outside bounds
            if any(u < -half_w or u > half_w or v < 0 or v > height for u, v in pts):
                continue
            # centroid in unwrapped coords
            uc = sum(u for u, _ in pts) / 3.0
            vc = sum(v for _, v in pts) / 3.0
            # map centroid back onto 3D inner cylindrical surface
            phi = math.pi / 2.0 - uc / inner_radius
            nx = math.cos(phi)
            ny = math.sin(phi)
            # define a workplane tangent to the inner surface, pointing inward
            plane = cq.Plane(
                origin=(inner_radius * nx, inner_radius * ny, vc),
                normal=(-nx, -ny, 0.0),
                xDir=(-ny, nx, 0.0),
            )
            # local triangle coords centered at centroid
            pts2d = [(u - uc, v - vc) for u, v in pts]
            # extrude inward to form mesh beam
            beam = cq.Workplane(plane).polyline(pts2d).close().extrude(mesh_depth)
            # chamfer each beam's vertical edges if any
            if chamfer_size > 0:
                try:
                    beam = beam.edges("|Z").chamfer(chamfer_size)
                except ValueError:
                    pass
            # weld the beam into the shell
            result = result.union(beam, clean=True)

    return result
