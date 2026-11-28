def dome_with_crown_gear_teeth_and_prong_spike_array(
    radius: float = 50.0,
    n_teeth: int = 24,
    module: float = 2.0,
    tooth_height: float = None,
    tooth_depth: float = None,
    prong_count: int = 12,
    prong_thickness: float = 1.0,
    prong_height: float = 5.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if radius <= 0:
        raise ValueError(f"radius must be > 0; got {radius}")
    if not isinstance(n_teeth, int) or n_teeth < 1:
        raise ValueError(f"n_teeth must be a positive integer; got {n_teeth}")
    if module <= 0:
        raise ValueError(f"module must be > 0; got {module}")

    if tooth_height is None:
        tooth_height = module
    if tooth_depth is None:
        tooth_depth = module
    if tooth_height < 0 or tooth_depth < 0:
        raise ValueError(f"tooth_height and tooth_depth must be >= 0; got {tooth_height}, {tooth_depth}")

    if not isinstance(prong_count, int) or prong_count < 0:
        raise ValueError(f"prong_count must be a non-negative integer; got {prong_count}")
    if prong_thickness < 0:
        raise ValueError(f"prong_thickness must be >= 0; got {prong_thickness}")
    if prong_height < 0:
        raise ValueError(f"prong_height must be >= 0; got {prong_height}")

    # 1) Build solid hemisphere (flat at z=0) by revolving arc
    dome = (
        cq.Workplane("XZ")
          .moveTo(-radius, 0.0)
          .threePointArc((0.0, radius), (radius, 0.0))
          .close()
          .revolve(180.0, (0, 0, 0), (1, 0, 0))
    )
    result = dome

    # 2) Add crown-gear teeth around equator
    chord = 2.0 * radius * math.sin(math.pi / n_teeth)
    half_chord = chord / 2.0
    for i in range(n_teeth):
        angle = 360.0 * i / n_teeth
        tooth = (
            cq.Workplane("XZ")
              .polyline([
                  (radius, 0.0),
                  (radius, tooth_height),
                  (radius + tooth_depth, tooth_height),
                  (radius + tooth_depth, 0.0),
              ])
              .close()
              .extrude(half_chord, both=True)
              .rotate((0, 0, 0), (0, 0, 1), angle)
        )
        result = result.union(tooth)

    # 3) Add radial prong spikes from apex down by prong_height
    if prong_count > 0 and prong_thickness > 0 and prong_height > 0:
        half_t = prong_thickness / 2.0
        for j in range(prong_count):
            angle = 360.0 * j / prong_count
            prong = (
                cq.Workplane("XZ")
                  .polyline([
                      (0.0, radius),
                      (0.0, radius - prong_height),
                      (radius, 0.0),
                  ])
                  .close()
                  .extrude(half_t, both=True)
                  .rotate((0, 0, 0), (0, 0, 1), angle)
            )
            result = result.union(prong)

    return result.combine()
