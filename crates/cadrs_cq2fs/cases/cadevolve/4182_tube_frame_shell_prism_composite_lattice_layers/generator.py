def tube_frame_shell_prism_composite_lattice_layers(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 30.0,
    cell_size: float = 5.0,
    rib_thickness: float = 1.0,
    layer_angle: float = 30.0,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    if cell_size <= 0:
        raise ValueError("cell_size must be positive")
    if rib_thickness <= 0 or rib_thickness > cell_size:
        raise ValueError("rib_thickness must be > 0 and <= cell_size")
    if layer_angle < 0 or layer_angle >= 180:
        raise ValueError("layer_angle must be in [0, 180)")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # 1) Build the open‐ended tube shell
    frame = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces(">Z or <Z")
          .shell(-wall_thickness)
    )

    # 2) Prepare hole‐pattern generator at mid‐wall radius
    mid_r = outer_radius - wall_thickness / 2.0

    def make_holes(angle_deg):
        holes = None
        circ = 2.0 * math.pi * mid_r
        h_pitch = cell_size
        v_pitch = cell_size
        cols = int(math.ceil(circ / h_pitch))
        rows = int(math.floor(height / v_pitch))
        half_cell = cell_size / 2.0
        offset = (angle_deg / 360.0) * circ
        for row in range(rows + 1):
            z = row * v_pitch
            if z < half_cell or z > height - half_cell:
                continue
            for col in range(cols + 1):
                u = col * h_pitch + offset
                if u < 0 or u >= circ:
                    continue
                theta = (u % circ) / mid_r
                x = mid_r * math.cos(theta)
                y = mid_r * math.sin(theta)
                plane = cq.Plane(
                    origin=cq.Vector(x, y, z),
                    xDir=cq.Vector(-math.sin(theta), math.cos(theta), 0),
                    normal=cq.Vector(-math.cos(theta), -math.sin(theta), 0)
                )
                hole = (
                    cq.Workplane(plane)
                      .rect(cell_size - rib_thickness, cell_size - rib_thickness)
                      .extrude(wall_thickness + 0.01)
                )
                holes = hole if holes is None else holes.union(hole)
        return holes

    # 3) Generate two rotated hole‐sets and cut from the shell
    holes1 = make_holes(0)
    holes2 = make_holes(layer_angle)
    holes = None
    if holes1 is not None and holes2 is not None:
        holes = holes1.union(holes2, clean=True)
    else:
        holes = holes1 or holes2

    if holes is not None:
        frame = frame.cut(holes, clean=True)

    # 4) Fillet all vertical edges
    if fillet_radius > 0:
        try:
            frame = frame.edges("|Z").fillet(fillet_radius)
        except Exception:
            pass

    return frame
