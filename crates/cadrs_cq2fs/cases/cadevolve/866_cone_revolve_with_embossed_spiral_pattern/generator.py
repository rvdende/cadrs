def cone_revolve_with_embossed_spiral_pattern(
    base_radius: float = 10.0,
    top_radius: float = 2.0,
    height: float = 20.0,
    rope_radius: float = 0.5,
    turns: float = 3.0,
    segments_per_turn: int = 36
) -> "cq.Workplane":
    """
    Cone adorned with an embossed spiral rope motif along its slope.
    Project a helical path on the conical surface, approximate it
    with small wedge segments, and union them to create a raised rope.

    Parameters:
        base_radius       -- radius of the cone at its base (>0)
        top_radius        -- radius of the cone at its top (>=0 and <= base_radius)
        height            -- axial height of the cone (>0)
        rope_radius       -- radial thickness of the rope motif (>0)
        turns             -- number of full spiral revolutions around the cone (>0)
        segments_per_turn -- how many wedge segments per revolution (>=3)

    Returns:
        A cadquery.Workplane object of the cone with the embossed spiral.
    """
    import cadquery as cq
    import math

    # input validation
    if base_radius <= 0 or height <= 0:
        raise ValueError("base_radius and height must be positive")
    if top_radius < 0 or top_radius > base_radius:
        raise ValueError("top_radius must be >=0 and <= base_radius")
    if rope_radius <= 0:
        raise ValueError("rope_radius must be positive")
    if turns <= 0:
        return cq.Workplane("XY")  # just an empty object
    if segments_per_turn < 3:
        segments_per_turn = 3

    # Build the base cone by revolving an XZ‐profile
    cone = (
        cq.Workplane("XZ")
          .moveTo(0.0, 0.0)
          .lineTo(base_radius, 0.0)
          .lineTo(top_radius, height)
          .close()
          .revolve(360.0)
    )

    result = cone

    # Total number of wedge segments
    total_segments = max(1, int(math.ceil(segments_per_turn * turns)))

    # Slope of radius per unit Z
    slope = (top_radius - base_radius) / height

    # Build each small wedge and fuse
    for i in range(total_segments):
        # parametric fraction along the helix [0,1]
        t0 = i / total_segments
        t1 = (i + 1) / total_segments

        # angles in degrees
        phi0 = 360.0 * turns * t0
        phi1 = 360.0 * turns * t1
        dphi = phi1 - phi0

        # corresponding Z and radius at start/end
        z0 = height * t0
        z1 = height * t1
        r0 = base_radius + slope * z0
        r1 = base_radius + slope * z1

        # Define the 2D wedge profile in the XZ‐plane
        # p1: on cone surface at start, p2: outer rope surface at start
        # p3: outer rope surface at end,   p4: on cone surface at end
        p1 = (r0, z0)
        p2 = (r0 + rope_radius, z0)
        p3 = (r1 + rope_radius, z1)
        p4 = (r1, z1)

        # Build the wedge by polyline and revolve around local Y axis
        wedge = (
            cq.Workplane("XZ")
              .polyline([p1, p2, p3, p4])
              .close()
              .revolve(dphi, axisStart=(0, 0, 0), axisEnd=(0, 1, 0))
              # rotate into position around the global Z axis
              .rotate((0, 0, 0), (0, 0, 1), phi0)
        )
        result = result.union(wedge, clean=True)

    return result
