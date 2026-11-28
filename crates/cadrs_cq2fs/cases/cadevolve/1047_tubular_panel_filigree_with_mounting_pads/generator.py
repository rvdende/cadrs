def tubular_panel_filigree_with_mounting_pads(
    radius: float = 10.0,
    height: float = 30.0,
    wall_thickness: float = 1.0,
    mesh_size: float = 5.0
) -> "cq.Workplane":
    """
    A tubular panel with filigree pattern and four mounting pads.
    Builds the thin‐walled cylinder with a triangular filigree pattern
    (same as tubular_panel_filigree), then adds four rectangular pads
    at 0/90/180/270° around the mid‐height, each 2 mm thick, square of
    side = mesh_size, and fillets all pad edges by 0.5 mm.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if radius <= 0 or height <= 0:
        raise ValueError("radius and height must be positive")
    if wall_thickness <= 0 or wall_thickness >= radius:
        raise ValueError("wall_thickness must be > 0 and < radius")
    if mesh_size <= 0:
        raise ValueError("mesh_size must be positive")

    # 1) Build thin‐walled cylinder
    panel = (
        cq.Workplane("XY")
          .circle(radius)
          .circle(radius - wall_thickness)
          .extrude(height)
    )

    # 2) Prepare pattern parameters
    mid_R = radius - wall_thickness / 2.0
    circumference = 2.0 * math.pi * mid_R
    s = mesh_size
    cell_h = s * math.sqrt(3.0) / 2.0
    n_cols = max(int(math.ceil(circumference / s)), 1)
    n_rows = max(int(math.ceil(height / cell_h)), 1)
    half_w = circumference / 2.0

    # 3) Generate triangular cutter prisms
    cutters = []
    for j in range(n_rows):
        v0 = j * cell_h
        for i in range(n_cols):
            u0 = -half_w + i * s
            if (i + j) % 2 == 0:
                u1, v1 = u0, v0
                u2, v2 = u0 + s, v0
                u3, v3 = u0 + 0.5 * s, v0 + cell_h
            else:
                u1, v1 = u0, v0 + cell_h
                u2, v2 = u0 + s, v0 + cell_h
                u3, v3 = u0 + 0.5 * s, v0

            # skip triangles outside development bounds
            if (
                u1 < -half_w or u1 > half_w or
                u2 < -half_w or u2 > half_w or
                u3 < -half_w or u3 > half_w or
                v1 < 0 or v1 > height or
                v2 < 0 or v2 > height or
                v3 < 0 or v3 > height
            ):
                continue

            # map triangle to cylinder
            uc = (u1 + u2 + u3) / 3.0
            vc = (v1 + v2 + v3) / 3.0
            phi = uc / mid_R
            nx, ny = math.cos(phi), math.sin(phi)
            tx, ty = -ny, nx
            cx, cy, cz = mid_R * nx, mid_R * ny, vc
            pts2d = [
                (u1 - uc, v1 - vc),
                (u2 - uc, v2 - vc),
                (u3 - uc, v3 - vc),
            ]
            plane = cq.Plane(origin=(cx, cy, cz), normal=(nx, ny, 0.0), xDir=(tx, ty, 0.0))
            cutter = (
                cq.Workplane(plane)
                  .polyline(pts2d)
                  .close()
                  .extrude(wall_thickness * 1.1, both=True)
            )
            cutters.append(cutter)

    # 4) Subtract all cutters
    result = panel
    for cutter in cutters:
        result = result.cut(cutter)

    # 5) Add four mounting pads
    pad_thickness = 2.0        # radial thickness
    pad_size = mesh_size       # square side
    pad_fillet = 0.5           # fillet radius on pad edges
    z0 = height / 2.0
    for angle in (0.0, 90.0, 180.0, 270.0):
        rad = math.radians(angle)
        nx, ny = math.cos(rad), math.sin(rad)
        tx, ty = -ny, nx
        cx, cy = radius * nx, radius * ny
        pad_plane = cq.Plane(origin=(cx, cy, z0), normal=(nx, ny, 0.0), xDir=(tx, ty, 0.0))
        pad = (
            cq.Workplane(pad_plane)
              .rect(pad_size, pad_size)
              .extrude(pad_thickness)
              .edges()        # all pad edges
              .fillet(pad_fillet)
        )
        result = result.union(pad)

    return result
