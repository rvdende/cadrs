def dome_hemisphere_with_embossed_flow_contours(
    R: float = 12.0,
    flow_count: int = 8,
    flow_thickness: float = 0.5,
    flow_height: float = 0.2,
    chamfer_size: float = 0.05
) -> "cq.Workplane":
    """
    Dome embossed with meridian-style flow lines.
    Projects meridional slabs onto the hemispherical dome and embosses raised lines
    following flow patterns. Chamfers all edges of each band for a smooth relief.

    Parameters:
        R (float): Radius of the hemisphere (> 0).
        flow_count (int): Number of flow lines (>= 1).
        flow_thickness (float): Tangential width of each line (> 0).
        flow_height (float): Radial emboss height (> 0).
        chamfer_size (float): Size of the chamfer on band edges (>= 0).
    Returns:
        cq.Workplane: The hemisphere dome with embossed flow contours.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be positive; got {R}")
    if not isinstance(flow_count, int) or flow_count < 1:
        raise ValueError(f"flow_count must be a positive integer; got {flow_count}")
    if flow_thickness <= 0:
        raise ValueError(f"flow_thickness must be positive; got {flow_thickness}")
    if flow_height <= 0:
        raise ValueError(f"flow_height must be positive; got {flow_height}")
    if chamfer_size < 0:
        raise ValueError(f"chamfer_size must be non-negative; got {chamfer_size}")

    # 1) Base hemisphere dome (upper half of sphere)
    dome = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .rect(2 * R, 2 * R)
                .extrude(-R)
          )
    )

    # 2) Thin spherical shell for embossing
    shell = (
        cq.Workplane("XY")
          .sphere(R + flow_height)
          .cut(cq.Workplane("XY").sphere(R))
          .cut(
              cq.Workplane("XY")
                .transformed(offset=(0, 0, -(R + flow_height)))
                .box(
                    2 * (R + flow_height),
                    2 * (R + flow_height),
                    (R + flow_height),
                    centered=(True, True, False)
                )
          )
    )

    # Accumulate result starting with the solid dome
    result = dome

    # 3) Build a rectangular slab that will carve out one meridian band
    #    Length in X covers radial direction, width in Y is the tangential thickness,
    #    height in Z covers the full dome height.
    slab = cq.Workplane("XY").box(
        2 * (R + flow_height),
        flow_thickness,
        2 * (R + flow_height)
    )

    # 4) For each angular position create a band by intersecting slab with shell
    for i in range(flow_count):
        phi_deg = 360.0 * i / flow_count
        # rotate the slab about the Z-axis to place it on the meridian at angle phi
        slab_i = slab.rotate((0, 0, 0), (0, 0, 1), phi_deg)
        # carve the band out of the thin shell
        band = shell.intersect(slab_i)
        # chamfer all edges of the band for smooth blending
        if chamfer_size > 0:
            try:
                band = band.edges().chamfer(chamfer_size)
            except Exception:
                # if chamfer fails for any reason, skip it
                pass
        # fuse the embossed band onto the base dome
        result = result.union(band)

    return result
