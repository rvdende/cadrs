def truncated_dome_with_embossed_geo_lattice(
    radius: float = 50.0,
    cut_depth: float = 15.0,
    cell_size: float = 5.0,
    emboss_height: float = 1.0,
    fillet_radius: float = 2.0
) -> "cq.Workplane":
    """
    A hemisphere of given radius, truncated by a flat cap cut_depth below the apex,
    with a low‐relief triangular lattice embossed on its curved surface.

    Parameters:
        radius (float): radius of the original hemisphere (> 0).
        cut_depth (float): distance from the hemisphere apex down to the cut plane (0 < cut_depth < radius).
        cell_size (float): nominal edge length of the triangular tessellation (> 0).
        emboss_height (float): how far each lattice vertex is embossed outward (> 0).
        fillet_radius (float): radius of fillet at the base rim (>= 0).

    Returns:
        cq.Workplane: a single solid body, watertight, with hemisphere, flat cap, filleted rim,
                       and small spherical bumps at tessellation vertices.
    """
    import cadquery as cq
    import math

    # parameter validation
    if radius <= 0:
        raise ValueError(f"radius must be positive; got radius={radius}")
    if cut_depth <= 0 or cut_depth >= radius:
        raise ValueError(
            f"cut_depth must be > 0 and < radius; got cut_depth={cut_depth}, radius={radius}"
        )
    if cell_size <= 0:
        raise ValueError(f"cell_size must be positive; got cell_size={cell_size}")
    if emboss_height <= 0:
        raise ValueError(f"emboss_height must be positive; got emboss_height={emboss_height}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >= 0; got fillet_radius={fillet_radius}")

    # compute height of the cut plane (measured from XY-plane)
    flat_height = radius - cut_depth

    # 1) build the solid hemisphere above z=0
    dome = (
        cq.Workplane("XY")
          .sphere(radius)
          .cut(
              cq.Workplane("XY")
                .rect(2 * radius, 2 * radius)
                .extrude(-radius)
          )
    )

    # 2) truncate apex by cutting away above flat_height
    if flat_height < radius:
        cutter = (
            cq.Workplane("XY")
              .workplane(offset=flat_height)
              .rect(2 * radius, 2 * radius)
              .extrude(radius)
        )
        dome = dome.cut(cutter)

    # 3) fillet the circular base rim
    if fillet_radius > 0:
        dome = dome.faces("<Z").edges().fillet(fillet_radius)

    # 4) compute a triangular grid in the XY-plane, project onto sphere, emboss
    sqrt3 = math.sqrt(3.0)
    dy = cell_size * sqrt3 / 2.0
    n_rows = int(math.ceil(radius / dy))
    n_cols = int(math.ceil(2.0 * radius / cell_size))

    for i in range(-n_rows, n_rows + 1):
        y = i * dy
        if abs(y) > radius:
            continue
        x_off = (cell_size / 2.0) if (i % 2) else 0.0
        for j in range(-n_cols, n_cols + 1):
            x = j * cell_size + x_off
            if x * x + y * y > radius * radius:
                continue
            z = math.sqrt(radius * radius - x * x - y * y)
            if z > flat_height or z <= 0:
                continue
            dome = dome.union(
                cq.Workplane("XY")
                  .sphere(emboss_height)
                  .translate((x, y, z))
            )

    return dome
