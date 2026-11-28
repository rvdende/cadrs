def cone_revolve_with_perforated_lattice_shell(
    outer_radius: float = 10.0,
    height: float = 20.0,
    thickness: float = 1.0,
    n_cols: int = 12,
    n_rows: int = 10,
    side_length: float = 1.0
) -> 'cq.Workplane':
    """
    Hollow conical shell perforated with an equilateral triangular lattice pattern.

    Parameters:
        outer_radius (float): Base radius of the cone (> 0).
        height (float): Height of the cone (> 0).
        thickness (float): Radial wall thickness (0 < thickness < outer_radius).
        n_cols (int): Number of triangles around the circumference (>= 1).
        n_rows (int): Number of triangle rows along the height (>= 1).
        side_length (float): Side length of each equilateral triangular opening (> 0).

    Returns:
        cq.Workplane: The perforated hollow conical shell.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if height <= 0:
        raise ValueError("height must be positive")
    if thickness <= 0 or thickness >= outer_radius:
        raise ValueError("thickness must be > 0 and < outer_radius")
    if n_cols < 1 or n_rows < 1:
        raise ValueError("n_cols and n_rows must be at least 1")
    if side_length <= 0:
        raise ValueError("side_length must be positive")

    # Build hollow conical shell
    outer = (
        cq.Workplane("XZ")
          .moveTo(0, 0)
          .lineTo(0, height)
          .lineTo(outer_radius, 0)
          .close()
          .revolve(360)
    )
    inner_radius = outer_radius - thickness
    inner = (
        cq.Workplane("XZ")
          .moveTo(0, 0)
          .lineTo(0, height)
          .lineTo(inner_radius, 0)
          .close()
          .revolve(360)
    )
    shell = outer.cut(inner)

    # Compute Z positions for triangular rows using exact triangular spacing
    if n_rows == 1:
        z_positions = [height / 2.0]
    else:
        # vertical spacing from equilateral triangles
        h_tri = math.sqrt(3.0) / 2.0 * side_length
        total_span = h_tri * float(n_rows - 1)
        if total_span > height:
            raise ValueError("n_rows and side_length too large for given height")
        margin = (height - total_span) / 2.0
        z_positions = [margin + i * h_tri for i in range(n_rows)]

    # Precompute cut parameters
    slope = outer_radius / height
    inv_norm = math.sqrt(1.0 + slope * slope)
    eps = 1e-3
    cut_length = thickness * inv_norm / 2.0 + eps

    # Build and union all triangular cutters
    union_cutter = None
    angle_step = 2.0 * math.pi / n_cols
    # loop over rows
    for i, zc in enumerate(z_positions):
        # scale radius linearly by height
        scale = 1.0 - zc / height
        mid_rad = (outer_radius - thickness / 2.0) * scale
        # offset alternate rows by half a cell for triangular layout
        angle_offset = (i % 2) * (angle_step / 2.0)
        for j in range(n_cols):
            angle = angle_offset + j * angle_step
            ca = math.cos(angle)
            sa = math.sin(angle)
            x = mid_rad * ca
            y = mid_rad * sa
            # surface normal on cone
            nx = ca / inv_norm
            ny = sa / inv_norm
            nz = slope / inv_norm
            # local tangent for triangle orientation
            tx, ty, tz = -sa, ca, 0.0
            # define plane at triangle center
            plane = cq.Plane(origin=(x, y, zc), normal=(nx, ny, nz), xDir=(tx, ty, tz))
            # equilateral triangle centered at origin
            h_tri = math.sqrt(3.0) / 2.0 * side_length
            p1 = (-0.5 * side_length, -h_tri / 3.0)
            p2 = ( 0.5 * side_length, -h_tri / 3.0)
            p3 = ( 0.0,              2.0 * h_tri / 3.0)
            # create cutter
            cutter = (
                cq.Workplane(plane)
                  .polyline([p1, p2, p3])
                  .close()
                  .extrude(cut_length, both=True)
            )
            # accumulate union
            if union_cutter is None:
                union_cutter = cutter
            else:
                union_cutter = union_cutter.union(cutter)

    # subtract pattern from shell
    result = shell.cut(union_cutter) if union_cutter is not None else shell
    return result
