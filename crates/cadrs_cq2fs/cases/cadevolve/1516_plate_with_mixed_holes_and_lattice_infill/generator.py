def plate_with_mixed_holes_and_lattice_infill(
    length=100.0,
    width=60.0,
    thickness=5.0,
    nx=5,
    ny=4,
    spacing_x=20.0,
    spacing_y=15.0,
    hole_diameter=5.0,
    square_size=8.0,
    cell_size=10.0,
    strut_radius=0.5
):
    import cadquery as cq
    import math

    # Parameter conversion and validation
    length = float(length)
    width = float(width)
    thickness = float(thickness)
    nx = int(nx)
    ny = int(ny)
    spacing_x = float(spacing_x)
    spacing_y = float(spacing_y)
    hole_diameter = float(hole_diameter)
    square_size = float(square_size)
    cell_size = float(cell_size)
    strut_radius = float(strut_radius)

    if length <= 0 or width <= 0 or thickness <= 0:
        raise ValueError("length, width, and thickness must be positive")
    if thickness >= min(length, width):
        raise ValueError("thickness must be less than plate dimensions")
    if nx < 1 or ny < 1:
        raise ValueError("nx and ny must be >= 1")
    if spacing_x <= 0 or spacing_y <= 0:
        raise ValueError("spacing_x and spacing_y must be positive")
    if hole_diameter <= 0 or square_size <= 0:
        raise ValueError("hole_diameter and square_size must be positive")
    if cell_size <= 0 or strut_radius <= 0:
        raise ValueError("cell_size and strut_radius must be positive")

    half_x = length / 2.0
    half_y = width / 2.0
    r = hole_diameter / 2.0
    hs = square_size / 2.0

    # Compute margins to center the hole grid
    if nx > 1:
        margin_x = (length - spacing_x * (nx - 1)) / 2.0
    else:
        margin_x = half_x
    if ny > 1:
        margin_y = (width - spacing_y * (ny - 1)) / 2.0
    else:
        margin_y = half_y

    # Classify hole centers into circles or squares (checkerboard)
    circle_pts = []
    square_pts = []
    for row in range(ny):
        y = -half_y + margin_y + row * spacing_y
        for col in range(nx):
            x = -half_x + margin_x + col * spacing_x
            if (row + col) % 2 == 0:
                if abs(x) <= half_x - hs and abs(y) <= half_y - hs:
                    square_pts.append((x, y))
            else:
                if abs(x) <= half_x - r and abs(y) <= half_y - r:
                    circle_pts.append((x, y))

    # Build the base plate
    plate = cq.Workplane("XY").box(length, width, thickness)

    # Build triangular lattice nodes
    h = cell_size * math.sqrt(3.0) / 2.0
    max_i = int(math.floor(half_y / h))
    nodes = {}
    for i in range(-max_i, max_i + 1):
        y = i * h
        if abs(y) > half_y + 1e-6:
            continue
        offset_i = (i % 2) * (cell_size / 2.0)
        j_min = int(math.ceil((-half_x - offset_i) / cell_size))
        j_max = int(math.floor((half_x - offset_i) / cell_size))
        for j in range(j_min, j_max + 1):
            x = j * cell_size + offset_i
            if abs(x) <= half_x + 1e-6:
                nodes[(i, j)] = (x, y)

    # Build lattice rods as a list of Workplanes
    rods = []
    for (i, j), (x, y) in nodes.items():
        # horizontal neighbor
        for di, dj in [(0, 1), (1, 1 if (i % 2) == 1 else 0), (1, 0 if (i % 2) == 1 else -1)]:
            nb = (i + di, j + dj)
            if nb in nodes:
                x2, y2 = nodes[nb]
                dx, dy = x2 - x, y2 - y
                L = math.hypot(dx, dy)
                if L > 1e-6:
                    mx, my = (x + x2) / 2.0, (y + y2) / 2.0
                    angle = math.degrees(math.atan2(dy, dx))
                    rod = (
                        cq.Workplane("XY")
                          .transformed(offset=(mx, my, -thickness / 2.0), rotate=(0, 0, angle))
                          .rect(L, 2.0 * strut_radius)
                          .extrude(thickness)
                    )
                    rods.append(rod)

    # Combine plate and lattice by unioning each rod
    result = plate
    for rod in rods:
        result = result.union(rod)

    # Subtract circular holes
    if circle_pts:
        result = (
            result.faces(">Z")
                  .workplane()
                  .pushPoints(circle_pts)
                  .circle(r)
                  .cutThruAll()
        )

    # Subtract square holes
    if square_pts:
        result = (
            result.faces(">Z")
                  .workplane()
                  .pushPoints(square_pts)
                  .rect(square_size, square_size)
                  .cutThruAll()
        )

    return result
