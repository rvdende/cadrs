def hollow_cyl_shell_with_variable_spoke_web(
    inner_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    start_spoke_count: int = 8,
    end_spoke_count: int = 16,
    spoke_width: float = 1.0,
    segments: int = 10
) -> cq.Workplane:
    # parameter validation
    if inner_radius <= 0 or wall_thickness <= 0 or height <= 0:
        raise ValueError("inner_radius, wall_thickness, and height must be positive")
    if wall_thickness >= inner_radius:
        raise ValueError("wall_thickness must be less than inner_radius")
    if start_spoke_count < 1 or end_spoke_count < 1:
        raise ValueError("start_spoke_count and end_spoke_count must be at least 1")
    if spoke_width <= 0:
        raise ValueError("spoke_width must be positive")
    if segments < 1:
        raise ValueError("segments must be at least 1")

    outer_radius = inner_radius + wall_thickness
    radial_length = wall_thickness
    mid_radius = inner_radius + radial_length / 2.0

    # build hollow cylinder shell
    shell = cq.Workplane("XY").circle(outer_radius).circle(inner_radius).extrude(height)

    # build all cutter solids
    dz = height / segments
    cutters: list[cq.Workplane] = []
    for i in range(segments):
        t = 0.0 if segments == 1 else float(i) / (segments - 1)
        count_i = int(round(start_spoke_count + (end_spoke_count - start_spoke_count) * t))
        count_i = max(count_i, 1)
        z0 = dz * i
        # one rectangular prism for this segment
        base_rect = cq.Workplane("XY").rect(radial_length, spoke_width).extrude(dz)
        for j in range(count_i):
            angle = 360.0 * j / count_i
            cutter = (
                base_rect
                  .translate((mid_radius, 0, z0))
                  .rotate((0, 0, 0), (0, 0, 1), angle)
            )
            cutters.append(cutter)

    # union all cutters and cut in one operation
    if cutters:
        cutter_union = cutters[0]
        for c in cutters[1:]:
            cutter_union = cutter_union.union(c)
        result = shell.cut(cutter_union)
    else:
        result = shell

    return result
