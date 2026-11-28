def plate_with_offset_hole_matrix_and_prismatic_handles(
    length: float = 100.0,
    width: float = 60.0,
    thickness: float = 5.0,
    nx: int = 6,
    ny: int = 4,
    spacing_x: float = 15.0,
    spacing_y: float = 15.0,
    hole_diameter: float = 5.0,
    lattice_width: float = 2.0,
    handle_length: float = 20.0,
    handle_width: float = 5.0,
    handle_height: float = 5.0
) -> "cq.Workplane":
    """
    Plate with offset (staggered) hole matrix, lattice cutouts, and prismatic handles on both Y-sides.

    Parameters:
        length         – overall X dimension of the plate (>0)
        width          – overall Y dimension of the plate (>0)
        thickness      – plate thickness in Z (>0)
        nx             – number of holes per even row (>=1)
        ny             – number of rows of holes (>=1)
        spacing_x      – center‐to‐center spacing of holes in X (>0)
        spacing_y      – center‐to‐center spacing of holes in Y (>0)
        hole_diameter  – diameter of each circular hole (>0)
        lattice_width  – width of the lattice slot cutouts (>0)
        handle_length  – length of the prismatic handle along X (>0)
        handle_width   – cross‐section of handle in Z (>0)
        handle_height  – extrusion depth of handle along Y (extends out from plate) (>0)
    """
    import cadquery as cq
    import math

    # Parameter validation and conversion
    length = float(length)
    width = float(width)
    thickness = float(thickness)
    spacing_x = float(spacing_x)
    spacing_y = float(spacing_y)
    hole_diameter = float(hole_diameter)
    lattice_width = float(lattice_width)
    handle_length = float(handle_length)
    handle_width = float(handle_width)
    handle_height = float(handle_height)
    nx = int(nx)
    ny = int(ny)

    if length <= 0 or width <= 0 or thickness <= 0:
        raise ValueError("length, width, and thickness must be positive")
    if nx < 1 or ny < 1:
        raise ValueError("nx and ny must be at least 1")
    if spacing_x <= 0 or spacing_y <= 0:
        raise ValueError("spacing_x and spacing_y must be positive")
    if hole_diameter <= 0 or lattice_width <= 0:
        raise ValueError("hole_diameter and lattice_width must be positive")
    if handle_length <= 0 or handle_width <= 0 or handle_height <= 0:
        raise ValueError("handle dimensions must be positive")

    # Half‐extents
    half_x = length / 2.0
    half_y = width / 2.0
    radius = hole_diameter / 2.0

    # Compute margins so holes are centered
    margin_x = (length - spacing_x * (nx - 1)) / 2.0 if nx > 1 else half_x
    margin_y = (width  - spacing_y * (ny - 1)) / 2.0 if ny > 1 else half_y

    # 1) Build the base plate
    plate = cq.Workplane("XY").box(length, width, thickness)

    # 2) Compute hole positions in a staggered grid
    hole_map = {}
    hole_points = []
    for r in range(ny):
        y = -half_y + margin_y + r * spacing_y
        offset = (spacing_x / 2.0) if (r % 2 == 1) else 0.0
        for c in range(nx):
            x = -half_x + margin_x + c * spacing_x + offset
            if (-half_x + radius) <= x <= (half_x - radius) and (-half_y + radius) <= y <= (half_y - radius):
                hole_map[(r, c)] = (x, y)
                hole_points.append((x, y))

    # 3) Drill circular holes through the full thickness
    if hole_points:
        plate = (
            plate
            .faces(">Z")
            .workplane()
            .pushPoints(hole_points)
            .hole(hole_diameter)
        )

    # 4) Build lattice slot cutters (horizontal and diagonal)
    cutters = []
    z_mid = thickness / 2.0
    for (r, c), (x0, y0) in hole_map.items():
        # horizontal neighbor
        hkey = (r, c + 1)
        if hkey in hole_map:
            x1, y1 = hole_map[hkey]
            dx, dy = x1 - x0, y1 - y0
            length_slot = math.hypot(dx, dy) - hole_diameter
            if length_slot > 1e-6:
                mx, my = (x0 + x1) / 2.0, (y0 + y1) / 2.0
                angle = math.degrees(math.atan2(dy, dx))
                cutter = (
                    cq.Workplane("XY")
                      .transformed(offset=(mx, my, z_mid), rotate=(0, 0, angle))
                      .rect(length_slot, lattice_width)
                      .extrude(-thickness)
                )
                cutters.append(cutter)
        # diagonal neighbors in next row
        if r < ny - 1:
            if r % 2 == 0:
                neighbors = [(r + 1, c), (r + 1, c - 1)]
            else:
                neighbors = [(r + 1, c), (r + 1, c + 1)]
            for dkey in neighbors:
                if dkey in hole_map:
                    x1, y1 = hole_map[dkey]
                    dx, dy = x1 - x0, y1 - y0
                    length_slot = math.hypot(dx, dy) - hole_diameter
                    if length_slot > 1e-6:
                        mx, my = (x0 + x1) / 2.0, (y0 + y1) / 2.0
                        angle = math.degrees(math.atan2(dy, dx))
                        cutter = (
                            cq.Workplane("XY")
                              .transformed(offset=(mx, my, z_mid), rotate=(0, 0, angle))
                              .rect(length_slot, lattice_width)
                              .extrude(-thickness)
                        )
                        cutters.append(cutter)

    # 5) Subtract all lattice cutters
    if cutters:
        slot_cutter = cutters[0]
        for c in cutters[1:]:
            slot_cutter = slot_cutter.union(c)
        plate = plate.cut(slot_cutter)

    # 6) Add prismatic handles on the two Y-sides
    #    Handles extend by handle_height in Y, width in Z = handle_width, span along X = handle_length
    handle_plus = (
        plate.faces(">Y")
             .workplane()
             .rect(handle_length, handle_width)
             .extrude(handle_height)
    )
    handle_minus = (
        plate.faces("<Y")
             .workplane()
             .rect(handle_length, handle_width)
             .extrude(handle_height)
    )
    plate = plate.union(handle_plus).union(handle_minus)

    return plate
