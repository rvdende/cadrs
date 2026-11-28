def cone_revolve_with_gradated_fenestrations(
    base_radius: float = 20.0,
    top_radius: float = 0.0,
    height: float = 40.0,
    ring_count: int = 5,
    fenestration_count: int = 8,
    window_max_diameter: float = 5.0,
    window_min_diameter: float = 1.0,
    cut_depth: float = 1.5,
    chamfer_size: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math
    from cadquery import Plane

    # Parameter validation
    if base_radius <= 0:
        raise ValueError(f"base_radius must be > 0; got {base_radius}")
    if top_radius < 0 or top_radius > base_radius:
        raise ValueError(f"top_radius must be >= 0 and <= base_radius; got {top_radius}")
    if height <= 0:
        raise ValueError(f"height must be > 0; got {height}")
    ring_count = int(ring_count)
    if ring_count < 1:
        raise ValueError(f"ring_count must be >= 1; got {ring_count}")
    fenestration_count = int(fenestration_count)
    if fenestration_count < 1:
        raise ValueError(f"fenestration_count must be >= 1; got {fenestration_count}")
    if window_max_diameter <= 0:
        raise ValueError(f"window_max_diameter must be > 0; got {window_max_diameter}")
    if window_min_diameter < 0 or window_min_diameter > window_max_diameter:
        raise ValueError(
            f"window_min_diameter must be >= 0 and <= window_max_diameter; got {window_min_diameter}"
        )
    if cut_depth <= 0:
        raise ValueError(f"cut_depth must be > 0; got {cut_depth}")
    if chamfer_size < 0:
        raise ValueError(f"chamfer_size must be >= 0; got {chamfer_size}")

    # 1) Build the cone by revolving an XZ profile
    profile = (
        cq.Workplane("XZ")
          .moveTo(0.0, 0.0)
          .lineTo(base_radius, 0.0)
          .lineTo(top_radius, height)
          .close()
    )
    cone = profile.revolve(360.0)

    # 2) Cut rings of circular windows shrinking towards apex
    dr_dz = (top_radius - base_radius) / height
    for ring in range(ring_count):
        z = height * (ring + 1) / (ring_count + 1)
        r_local = base_radius + dr_dz * z
        if ring_count > 1:
            diameter = (
                window_max_diameter
                - ring * (window_max_diameter - window_min_diameter) / (ring_count - 1)
            )
        else:
            diameter = window_max_diameter

        for i in range(fenestration_count):
            angle = 2 * math.pi * i / fenestration_count
            cos_a = math.cos(angle)
            sin_a = math.sin(angle)
            x = r_local * cos_a
            y = r_local * sin_a

            # define a workplane tangent to the cone side
            normal = (cos_a, sin_a, -dr_dz)
            x_dir = (-sin_a, cos_a, 0.0)
            plane = Plane(origin=(x, y, z), normal=normal, xDir=x_dir)

            # cut circular hole normal to surface
            cutter = (
                cq.Workplane(plane)
                  .circle(diameter / 2.0)
                  .extrude(-cut_depth, combine=False)
            )
            # chamfer around each opening
            if chamfer_size > 0:
                cutter = cutter.edges().chamfer(chamfer_size)
            cone = cone.cut(cutter)

    return cone
