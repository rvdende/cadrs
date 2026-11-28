def facade_panel_with_nested_rhombus_and_lattice(
    length: float = 200.0,
    width: float = 100.0,
    thickness: float = 5.0,
    lattice_nx: int = 4,
    lattice_ny: int = 3,
    lattice_wall_thickness: float = 3.0,
    rhombus_nx: int = 5,
    rhombus_ny: int = 4,
    hole_width: float = 20.0,
    hole_height: float = 10.0,
    inner_scale: float = 0.5,
    pocket_depth: float = 2.0,
    fillet_radius: float = 1.0,
    mount_offset: float = 10.0,
    tab_size: float = 10.0,
    mount_hole_dia: float = 4.0,
    countersink_dia: float = 8.0,
    countersink_angle: float = 90.0
) -> "cq.Workplane":
    """
    Façade panel with a lattice‐style infill and a nested rhombus pattern on the front.
    Small square mounting tabs with countersunk holes are added on the back corners.
    """
    import cadquery as cq

    # Parameter validation
    if length <= 0 or width <= 0 or thickness <= 0:
        raise ValueError("length, width, and thickness must be positive")
    if lattice_nx < 1 or lattice_ny < 1:
        raise ValueError("lattice_nx and lattice_ny must be >= 1")
    if lattice_wall_thickness <= 0:
        raise ValueError("lattice_wall_thickness must be positive")
    if rhombus_nx < 1 or rhombus_ny < 1:
        raise ValueError("rhombus_nx and rhombus_ny must be >= 1")
    if hole_width <= 0 or hole_height <= 0:
        raise ValueError("hole_width and hole_height must be positive")
    if not (0 < inner_scale < 1):
        raise ValueError("inner_scale must be between 0 and 1")
    if pocket_depth <= 0 or pocket_depth > thickness:
        raise ValueError("pocket_depth must be >0 and <= thickness")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non‐negative")
    if mount_offset < 0:
        raise ValueError("mount_offset must be non‐negative")
    if tab_size <= 0:
        raise ValueError("tab_size must be positive")
    if mount_hole_dia <= 0:
        raise ValueError("mount_hole_dia must be positive")
    if countersink_dia < mount_hole_dia:
        raise ValueError("countersink_dia must be >= mount_hole_dia")
    if not (0 < countersink_angle < 180):
        raise ValueError("countersink_angle must be between 0 and 180°")

    half_x = length / 2.0
    half_y = width / 2.0
    half_thick = thickness / 2.0

    # Lattice spacing and hole dimensions
    lat_sp_x = length / (lattice_nx + 1)
    lat_sp_y = width  / (lattice_ny + 1)
    hole_lat_x = lat_sp_x - lattice_wall_thickness
    hole_lat_y = lat_sp_y - lattice_wall_thickness
    if hole_lat_x <= 0 or hole_lat_y <= 0:
        raise ValueError("lattice_wall_thickness too large for given lattice counts")

    # Build base panel
    panel = cq.Workplane("XY").box(length, width, thickness)

    # Cut lattice holes
    lat_centers = []
    for i in range(lattice_ny):
        y = -half_y + (i + 1) * lat_sp_y
        for j in range(lattice_nx):
            x = -half_x + (j + 1) * lat_sp_x
            # ensure hole fully inside
            if (-half_x + hole_lat_x / 2.0 <= x <= half_x - hole_lat_x / 2.0 and
                -half_y + hole_lat_y / 2.0 <= y <= half_y - hole_lat_y / 2.0):
                lat_centers.append((x, y))
    for x, y in lat_centers:
        cutter = (
            cq.Workplane("XY")
              .transformed(offset=(x, y, half_thick))
              .rect(hole_lat_x, hole_lat_y)
              .extrude(-thickness)
        )
        panel = panel.cut(cutter)

    # Fillet only the external vertical edges of the panel
    if fillet_radius > 0:
        panel = panel.edges("(>X or <X or >Y or <Y) and |Z").fillet(fillet_radius)

    # Nested rhombus pattern (shallow pocket + inner through‐cut)
    outer_hw2 = hole_width / 2.0
    outer_hh2 = hole_height / 2.0
    inner_hw2 = outer_hw2 * inner_scale
    inner_hh2 = outer_hh2 * inner_scale

    r_sp_x = length / (rhombus_nx + 1)
    r_sp_y = width  / (rhombus_ny + 1)
    rhom_centers = []
    for i in range(rhombus_ny):
        y = -half_y + (i + 1) * r_sp_y
        shift = (r_sp_x / 2.0) if (i % 2 == 1) else 0.0
        for j in range(rhombus_nx):
            x = -half_x + shift + (j + 1) * r_sp_x
            if abs(x) <= half_x - outer_hw2 and abs(y) <= half_y - outer_hh2:
                rhom_centers.append((x, y))

    # Outer shallow pocket
    for x, y in rhom_centers:
        pocket = (
            cq.Workplane("XY")
              .transformed(offset=(x, y, half_thick))
              .moveTo(0.0, outer_hh2)
              .lineTo(outer_hw2, 0.0)
              .lineTo(0.0, -outer_hh2)
              .lineTo(-outer_hw2, 0.0)
              .close()
              .extrude(-pocket_depth)
        )
        panel = panel.cut(pocket)
    # Inner through‐cut
    for x, y in rhom_centers:
        hole = (
            cq.Workplane("XY")
              .transformed(offset=(x, y, half_thick))
              .moveTo(0.0, inner_hh2)
              .lineTo(inner_hw2, 0.0)
              .lineTo(0.0, -inner_hh2)
              .lineTo(-inner_hw2, 0.0)
              .close()
              .extrude(-thickness)
        )
        if fillet_radius > 0:
            hole = hole.edges("|Z").fillet(fillet_radius)
        panel = panel.cut(hole)

    # Mounting tabs with countersunk holes on the back face
    # Tab centers inset from each corner
    if not (tab_size / 2.0 <= mount_offset <= min(half_x, half_y) - tab_size / 2.0):
        raise ValueError("mount_offset and tab_size combination places tabs out of bounds")
    tab_centers = [
        (-half_x + mount_offset, -half_y + mount_offset),
        (-half_x + mount_offset,  half_y - mount_offset),
        ( half_x - mount_offset, -half_y + mount_offset),
        ( half_x - mount_offset,  half_y - mount_offset),
    ]
    # Extrude square tabs downward
    panel = (
        panel.faces("<Z")
             .workplane()
             .pushPoints(tab_centers)
             .rect(tab_size, tab_size)
             .extrude(thickness)
    )
    # Add countersunk mounting holes through each tab
    panel = (
        panel.faces("<Z")
             .workplane()
             .pushPoints(tab_centers)
             .cskHole(mount_hole_dia, countersink_dia, countersink_angle)
    )

    return panel
