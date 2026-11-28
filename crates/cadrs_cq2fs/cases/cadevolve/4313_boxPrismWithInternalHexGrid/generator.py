def boxPrismWithInternalHexGrid(
    length: float = 100.0,
    width: float = 60.0,
    height: float = 40.0,
    wall_thickness: float = 3.0,
    cell_size: float = 8.0,
    strut_thickness: float = 1.0,
    fillet_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    for name, val in (
        ("length", length),
        ("width", width),
        ("height", height),
        ("wall_thickness", wall_thickness),
        ("cell_size", cell_size),
        ("strut_thickness", strut_thickness),
    ):
        if val <= 0:
            raise ValueError(f"{name} must be positive")
    if length <= 2 * wall_thickness or width <= 2 * wall_thickness or height <= 2 * wall_thickness:
        raise ValueError("Overall dimensions must exceed twice the wall_thickness")

    # Outer shell
    outer = cq.Workplane("XY").box(length, width, height)

    # Inner cavity
    inner_length = length - 2.0 * wall_thickness
    inner_width = width - 2.0 * wall_thickness
    inner_height = height - 2.0 * wall_thickness
    inner = cq.Workplane("XY").box(inner_length, inner_width, inner_height)
    shell = outer.cut(inner)

    # Honeycomb spacing
    sqrt3 = math.sqrt(3.0)
    h_spacing = cell_size
    v_spacing = sqrt3 * cell_size / 2.0
    half_x = inner_length / 2.0
    half_y = inner_width / 2.0
    tol = 1e-6

    # Generate node points
    max_j = int(math.floor(half_y / v_spacing))
    max_i = int(math.floor(half_x / h_spacing))
    nodes = []
    for j in range(-max_j, max_j + 1):
        y = j * v_spacing
        x_offset = (j % 2) * (h_spacing / 2.0)
        for i in range(-max_i, max_i + 1):
            x = i * h_spacing + x_offset
            if abs(x) <= half_x + tol and abs(y) <= half_y + tol:
                nodes.append((round(x, 6), round(y, 6)))
    node_set = set(nodes)

    # Beam parameters
    fudge = min(wall_thickness, strut_thickness) * 0.5
    z0 = -inner_height / 2.0 - fudge
    extrude_len = inner_height + 2.0 * fudge
    neighbor_offsets = [
        (h_spacing, 0.0),
        (h_spacing / 2.0, v_spacing),
        (-h_spacing / 2.0, v_spacing),
    ]

    # Build and fuse lattice
    result = shell
    for x, y in nodes:
        for dx, dy in neighbor_offsets:
            x2 = round(x + dx, 6)
            y2 = round(y + dy, 6)
            if (x2, y2) in node_set:
                mx = 0.5 * (x + x2)
                my = 0.5 * (y + y2)
                beam_len = math.hypot(dx, dy)
                angle = math.atan2(dy, dx)
                pl = cq.Plane(
                    origin=(mx, my, z0),
                    normal=(0, 0, 1),
                    xDir=(math.cos(angle), math.sin(angle), 0),
                )
                beam = cq.Workplane(pl).rect(beam_len, strut_thickness).extrude(extrude_len, combine=False)
                # Fillet edges parallel to Z
                if fillet_radius > 0:
                    fr = min(fillet_radius, strut_thickness * 0.5) - 1e-6
                    if fr > 0:
                        try:
                            beam = beam.edges("|Z").fillet(fr)
                        except Exception:
                            pass
                result = result.union(beam)
    return result
